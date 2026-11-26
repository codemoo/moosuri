//! Private runner for the pinned Google Cloud SDK inside the authentication
//! capsule. Every method performs exactly one fixed SDK command; callers
//! supply only validated typed values, never arguments, paths or shell text.
//! The caller owns durable intent and reconciliation between operations, and
//! nothing here retries project creation or deletes a Cloud resource.
//!
//! Dropping an `Sdk` only removes the private configuration directory. That
//! cannot prove remote token revocation: call `revoke_account` and then
//! `prove_signed_out` as explicit awaited steps. If `finish_login` fails it
//! kills the login child and makes one bounded best-effort
//! `cleanup_login_credentials`, independent of the caller's cancellation token;
//! if a larger operation future is dropped instead, the controller must await
//! `cleanup_login_credentials` itself (async Drop cannot). Successful login keeps
//! its credentials until the project/API work completes. Output is never traced or
//! placed into errors; errors are the coarse nonsecret `Error` enum.
use super::Error;
use crate::browser::private_proxy;
use serde_json::Value;
use std::{
    collections::BTreeMap,
    net::SocketAddr,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::{Child, ChildStderr, Command},
};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

const GCLOUD: &str = "/opt/google-cloud-sdk/bin/gcloud";
const VERSION_FILE: &str = "/opt/google-cloud-sdk/VERSION";
const VERSION: &str = "594.0.1";
const PROFILES: &str = "/profiles";
const MAX_OUTPUT: usize = 512 * 1024;
const MAX_LOGIN_STDERR: usize = 16 * 1024;
const URL_WAIT: Duration = Duration::from_secs(30);
const LOGIN_WAIT: Duration = Duration::from_secs(10 * 60);
const QUICK: Duration = Duration::from_secs(60);
const CREATE_WAIT: Duration = Duration::from_secs(180);
const ENABLE_WAIT: Duration = Duration::from_secs(300);
const CLEANUP_STEP: Duration = Duration::from_secs(20);
const REAP_WAIT: Duration = Duration::from_secs(5);

const AUTH_PREFIX: &str = "https://accounts.google.com/o/oauth2/auth?";
const SDK_CLIENT_ID: &str = "32555940559.apps.googleusercontent.com";
const OPENED_MARKER: &str = "Your browser has been opened to visit:";
const SCOPES: [&str; 7] = [
    "openid",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/appengine.admin",
    "https://www.googleapis.com/auth/sqlservice.login",
    "https://www.googleapis.com/auth/compute",
    "https://www.googleapis.com/auth/accounts.reauth",
];
/// Exactly the Workspace APIs the installation uses.
pub(super) const APIS: [&str; 5] = [
    "gmail.googleapis.com",
    "drive.googleapis.com",
    "docs.googleapis.com",
    "sheets.googleapis.com",
    "slides.googleapis.com",
];
const PURPOSE_LABEL: (&str, &str) = ("moosuri-purpose", "oauth");

fn unavailable<E>(_: E) -> Error {
    Error::Unavailable
}

/// Lowercase account email; cannot start with `-`, so it is never an option.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct Email(String);
impl Email {
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let raw = raw.to_ascii_lowercase();
        let (local, domain) = raw.split_once('@').ok_or(Error::Invalid)?;
        let local_ok = !local.is_empty()
            && local.len() <= 64
            && local.starts_with(|c: char| c.is_ascii_alphanumeric())
            && local
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._+-".contains(&b));
        let domain_ok = domain.contains('.')
            && domain.split('.').all(|l| {
                !l.is_empty()
                    && l.len() <= 63
                    && !l.starts_with('-')
                    && !l.ends_with('-')
                    && l.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
        if raw.len() > 254 || !local_ok || !domain_ok {
            return Err(Error::Invalid);
        }
        Ok(Self(raw))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl std::fmt::Debug for Email {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Email(..)")
    }
}

/// Deterministic caller-supplied `moosuri-` plus 20 lowercase hex digits.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct ProjectId(String);
impl ProjectId {
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let hex = raw.strip_prefix("moosuri-").ok_or(Error::Invalid)?;
        if hex.len() == 20 && hex.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f')) {
            Ok(Self(raw.to_owned()))
        } else {
            Err(Error::Invalid)
        }
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Installation/setup ownership label value: 8..=63 of `[a-z0-9-]`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct SetupLabel(String);
impl SetupLabel {
    pub fn parse(raw: &str) -> Result<Self, Error> {
        let ok = (8..=63).contains(&raw.len())
            && raw.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
            && raw
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if ok {
            Ok(Self(raw.to_owned()))
        } else {
            Err(Error::Invalid)
        }
    }
}

/// Opaque login material. Not serializable; `Debug` is redacted. The URL is
/// exactly what the SDK emitted, to be opened only in the same existing Chrome.
pub(super) struct LoginRequest {
    url: Zeroizing<String>,
    callback: Zeroizing<String>,
    state: Zeroizing<String>,
    /// Domain of the expected account; the only value Google may echo as `hd`.
    domain: String,
}
impl LoginRequest {
    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn callback(&self) -> &str {
        &self.callback
    }
    pub fn state(&self) -> &str {
        &self.state
    }
    /// CDP `Fetch` URL pattern matching only this exact loopback origin. It is a
    /// prefilter: the full URL is still checked by `accept_callback`.
    pub fn fetch_pattern(&self) -> Result<String, Error> {
        let url = Url::parse(&self.callback).map_err(|_| Error::Invalid)?;
        Ok(format!(
            "http://{}:{}/*",
            url.host_str().ok_or(Error::Invalid)?,
            url.port().ok_or(Error::Invalid)?
        ))
    }
    /// Accepts a browser request URL only if it is exactly this login's callback
    /// carrying one safe authorization code and this login's unique state.
    pub fn accept_callback(&self, raw: &str) -> Result<Callback, Error> {
        validate_callback(raw, &self.callback, &self.state, &self.domain)?;
        Ok(Callback {
            url: Zeroizing::new(raw.to_owned()),
        })
    }
}

/// Opaque validated authorization callback (contains the one-time code). Not
/// cloneable or serializable; `Debug` is redacted; memory is zeroized on drop.
pub(super) struct Callback {
    url: Zeroizing<String>,
}
impl std::fmt::Debug for Callback {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Callback(..)")
    }
}
impl std::fmt::Debug for LoginRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LoginRequest(..)")
    }
}

/// Owns a child and its whole process group (the SDK entry is a shell wrapper
/// that execs Python). Dropping kills the group.
struct Group(Child);
impl Group {
    fn spawn(command: &mut Command) -> Result<Self, Error> {
        command.kill_on_drop(true).process_group(0);
        Ok(Self(command.spawn().map_err(unavailable)?))
    }
}
impl Group {
    fn kill(&mut self) {
        if let Some(pid) = self.0.id() {
            // SAFETY: plain signal to the group this struct created.
            unsafe {
                libc::kill(-(pid as i32), libc::SIGKILL);
            }
        }
        let _ = self.0.start_kill();
    }
    /// Kill the group and reap the leader (bounded) so nothing can still be storing
    /// tokens. `Ok` only when the leader's exit was actually observed.
    async fn kill_and_reap(&mut self) -> Result<(), Error> {
        self.kill();
        match tokio::time::timeout(REAP_WAIT, self.0.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => Err(Error::Unavailable),
        }
    }
}
impl Drop for Group {
    fn drop(&mut self) {
        self.kill();
    }
}

/// A running browser login. Dropping it kills the SDK login process.
pub(super) struct LoginHandle {
    group: Group,
    stderr: ChildStderr,
    request: LoginRequest,
    email: Email,
    owner: PathBuf,
}
impl LoginHandle {
    pub fn request(&self) -> &LoginRequest {
        &self.request
    }
    /// Kills the login process group and awaits reaping of the leader, so a
    /// caller can prove no login is running before `cleanup_login_credentials`
    /// (e.g. when Chrome authorization fails). Drop alone does not prove reaping.
    pub async fn stop(mut self) -> Result<(), Error> {
        self.group.kill_and_reap().await
    }
}

/// Result of an exact-id project probe (see `Sdk::probe_project`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ProjectPresence {
    Absent,
    Owned,
}
impl std::fmt::Debug for LoginHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("LoginHandle(..)")
    }
}

pub(super) struct Sdk {
    dir: tempfile::TempDir,
    gcloud: PathBuf,
    proxy: SocketAddr,
}
impl std::fmt::Debug for Sdk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Sdk(..)")
    }
}

impl Sdk {
    /// Fresh mode-0700 directory under `/profiles` (tmpfs); pinned SDK only.
    pub fn create(proxy: SocketAddr) -> Result<Self, Error> {
        let version = std::fs::read_to_string(VERSION_FILE).map_err(unavailable)?;
        if version.trim() != VERSION {
            return Err(Error::Unavailable);
        }
        Self::open(Path::new(PROFILES), PathBuf::from(GCLOUD), proxy)
    }

    fn open(root: &Path, gcloud: PathBuf, proxy: SocketAddr) -> Result<Self, Error> {
        if private_proxy(&proxy.ip().to_string()).ok() != Some(proxy) {
            return Err(Error::Unavailable);
        }
        let dir = tempfile::Builder::new()
            .prefix("moosuri-gcloud-")
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(root)
            .map_err(unavailable)?;
        for name in ["home", "config", "tmp"] {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(dir.path().join(name))
                .map_err(unavailable)?;
        }
        Ok(Self { dir, gcloud, proxy })
    }

    fn env(&self) -> Vec<(&'static str, String)> {
        let sub = |n: &str| self.dir.path().join(n).to_string_lossy().into_owned();
        let proxy = format!("http://{}", self.proxy);
        vec![
            ("PATH", "/usr/bin:/bin".into()),
            ("HOME", sub("home")),
            ("TMPDIR", sub("tmp")),
            ("CLOUDSDK_CONFIG", sub("config")),
            ("BROWSER", "/usr/bin/true".into()),
            // Pinned SDK requires a compositor variable even with BROWSER=true.
            ("DISPLAY", ":99".into()),
            ("PYTHONDONTWRITEBYTECODE", "1".into()),
            ("CLOUDSDK_CORE_DISABLE_USAGE_REPORTING", "true".into()),
            ("CLOUDSDK_CORE_DISABLE_FILE_LOGGING", "true".into()),
            ("CLOUDSDK_CORE_DISABLE_PROMPTS", "true".into()),
            ("CLOUDSDK_CORE_CHECK_GCE_METADATA", "false".into()),
            (
                "CLOUDSDK_COMPONENT_MANAGER_DISABLE_UPDATE_CHECK",
                "true".into(),
            ),
            ("CLOUDSDK_PROXY_TYPE", "http".into()),
            ("CLOUDSDK_PROXY_ADDRESS", self.proxy.ip().to_string()),
            ("CLOUDSDK_PROXY_PORT", self.proxy.port().to_string()),
            ("HTTPS_PROXY", proxy.clone()),
            ("HTTP_PROXY", proxy),
        ]
    }

    fn command(&self, args: &[&str], stdout: Stdio, stderr: Stdio) -> Command {
        let mut command = Command::new(&self.gcloud);
        command
            .env_clear()
            .envs(self.env())
            .args(args)
            .current_dir(self.dir.path().join("home"))
            .stdin(Stdio::null())
            .stdout(stdout)
            .stderr(stderr);
        command
    }

    async fn exec(
        &self,
        args: &[&str],
        limit: Duration,
        cancel: &CancellationToken,
    ) -> Result<Zeroizing<Vec<u8>>, Error> {
        run(
            self.command(args, Stdio::piped(), Stdio::null()),
            MAX_OUTPUT,
            limit,
            cancel,
        )
        .await
    }

    /// Starts `gcloud auth login <email>` in default browser mode (localhost
    /// callback inside this capsule) and captures the emitted auth URL.
    pub async fn begin_login(
        &self,
        email: &Email,
        cancel: &CancellationToken,
    ) -> Result<LoginHandle, Error> {
        let mut command = self.command(
            &["auth", "login", &email.0, "--launch-browser", "--quiet"],
            Stdio::null(),
            Stdio::piped(),
        );
        let mut group = Group::spawn(&mut command)?;
        let mut stderr = group.0.stderr.take().ok_or(Error::Unavailable)?;
        let read = async {
            let mut seen = Zeroizing::new(Vec::with_capacity(MAX_LOGIN_STDERR));
            let mut chunk = Zeroizing::new([0u8; 2048]);
            loop {
                let n = stderr.read(&mut chunk[..]).await.map_err(unavailable)?;
                if n == 0 {
                    return Err(Error::Unavailable);
                }
                if seen.len() + n > MAX_LOGIN_STDERR {
                    return Err(Error::Blocked);
                }
                seen.extend_from_slice(&chunk[..n]);
                if let Some(url) = extract_url(&seen) {
                    return Ok(url);
                }
            }
        };
        let url = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Err(Error::Unavailable),
            r = tokio::time::timeout(URL_WAIT, read) => r.map_err(unavailable)??,
        };
        let request = validate_login_url(&url, email)?;
        Ok(LoginHandle {
            group,
            stderr,
            request,
            email: email.clone(),
            owner: self.dir.path().to_owned(),
        })
    }

    /// Hands the browser-captured callback to THIS login's own loopback listener.
    /// Validated again here against the handle's typed request; a callback for any
    /// other request/host/port/state never connects. A 2xx/3xx status only means the
    /// listener took it: success is still proven solely by `finish_login`.
    pub async fn deliver_callback(
        &self,
        handle: &LoginHandle,
        callback: Callback,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        if handle.owner != self.dir.path() {
            return Err(Error::Invalid);
        }
        let request = &handle.request;
        validate_callback(
            &callback.url,
            &request.callback,
            &request.state,
            &request.domain,
        )?;
        let url = Url::parse(&callback.url).map_err(|_| Error::Invalid)?;
        let (host, port) = (url.host_str().ok_or(Error::Invalid)?, url.port());
        let target = Zeroizing::new(format!(
            "{}?{}",
            url.path(),
            url.query().ok_or(Error::Invalid)?
        ));
        deliver(port.ok_or(Error::Invalid)?, host, &target, cancel).await
    }

    /// Waits for the SDK login child to succeed, then requires the credentialed
    /// account list to be exactly the expected email.
    pub async fn finish_login(
        &self,
        handle: LoginHandle,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let LoginHandle {
            mut group,
            stderr,
            email,
            owner,
            request: _,
        } = handle;
        if owner != self.dir.path() {
            return Err(Error::Invalid);
        }
        let result = self.await_login(&mut group, stderr, &email, cancel).await;
        if result.is_err() {
            // Child first, so it cannot store a token after cleanup; then clean this
            // private config. The original classification is preserved.
            let _ = group.kill_and_reap().await;
            let _ = self.cleanup_login_credentials().await;
        }
        result
    }

    /// Fixed `gcloud auth revoke --all --quiet` in THIS private config only, then
    /// Uses its own token so a cancelled operation cannot skip it. An initially
    /// empty private configuration needs no revoke (the SDK returns exit 1 for it).
    /// Otherwise revoke must succeed and a fresh list must prove no account remains;
    /// removing the directory is never treated as revocation. Callers must ensure no
    /// login child is still running.
    pub async fn cleanup_login_credentials(&self) -> Result<(), Error> {
        let token = CancellationToken::new();
        let before = self
            .exec(&["auth", "list", "--format=json"], CLEANUP_STEP, &token)
            .await
            .and_then(|out| parse_accounts(&out))?;
        if before.is_empty() {
            return Ok(());
        }
        let revoked = self
            .exec(
                &["auth", "revoke", "--all", "--quiet"],
                CLEANUP_STEP,
                &token,
            )
            .await;
        let listed = self
            .exec(&["auth", "list", "--format=json"], CLEANUP_STEP, &token)
            .await
            .and_then(|out| parse_accounts(&out));
        match (revoked, listed) {
            (Ok(_), Ok(accounts)) if accounts.is_empty() => Ok(()),
            (Ok(_), Ok(_)) => Err(Error::Blocked),
            _ => Err(Error::Unavailable),
        }
    }

    async fn await_login(
        &self,
        group: &mut Group,
        mut stderr: ChildStderr,
        email: &Email,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let status = {
            // Keep draining stderr so the child never blocks on a full pipe.
            let drain = async {
                let mut sink = Zeroizing::new([0u8; 2048]);
                while stderr.read(&mut sink[..]).await.is_ok_and(|n| n > 0) {}
                std::future::pending::<()>().await
            };
            tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(Error::Unavailable),
                _ = tokio::time::sleep(LOGIN_WAIT) => return Err(Error::NeedsHuman),
                _ = drain => unreachable!(),
                s = group.0.wait() => s.map_err(unavailable)?,
            }
        };
        if !status.success() {
            return Err(Error::NeedsHuman);
        }
        let accounts = self.accounts(cancel).await?;
        match accounts.as_slice() {
            [(account, status)] if *account == email.0 && status == "ACTIVE" => Ok(()),
            _ => Err(Error::Blocked),
        }
    }

    async fn accounts(&self, cancel: &CancellationToken) -> Result<Vec<(String, String)>, Error> {
        let out = self
            .exec(&["auth", "list", "--format=json"], QUICK, cancel)
            .await?;
        parse_accounts(&out)
    }

    /// `gcloud projects describe`: succeeds only for an ACTIVE project whose id,
    /// name and exact label set prove it is ours. Foreign projects are Blocked.
    pub async fn describe_project(
        &self,
        id: &ProjectId,
        setup: &SetupLabel,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let out = self
            .exec(
                &["projects", "describe", id.as_str(), "--format=json"],
                QUICK,
                cancel,
            )
            .await?;
        verify_project(&out, id, setup)
    }

    /// Fixed `gcloud projects list --filter=projectId=<id>`. `Absent` only for a
    /// successful command returning exactly `[]`; a command failure, malformed
    /// output, a foreign project or multiple entries are errors, never Absent.
    /// Does not check billing; the caller must separately `verify_unbilled`.
    pub async fn probe_project(
        &self,
        id: &ProjectId,
        setup: &SetupLabel,
        cancel: &CancellationToken,
    ) -> Result<ProjectPresence, Error> {
        let filter = format!("--filter=projectId={}", id.as_str());
        let out = self
            .exec(
                &["projects", "list", &filter, "--format=json"],
                QUICK,
                cancel,
            )
            .await?;
        parse_project_presence(&out, id, setup)
    }

    /// One `gcloud projects create` attempt. Never retried here; after any
    /// error (including cancellation) the caller must reconcile by describing.
    pub async fn create_project(
        &self,
        id: &ProjectId,
        setup: &SetupLabel,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let name = format!("--name={}", id.as_str());
        let labels = format!(
            "--labels={}={},moosuri-setup={}",
            PURPOSE_LABEL.0, PURPOSE_LABEL.1, setup.0
        );
        self.exec(
            &["projects", "create", id.as_str(), &name, &labels, "--quiet"],
            CREATE_WAIT,
            cancel,
        )
        .await
        .map(drop)
    }

    /// `gcloud billing projects describe`: billing disabled, no linked account.
    pub async fn verify_unbilled(
        &self,
        id: &ProjectId,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let out = self
            .exec(
                &[
                    "billing",
                    "projects",
                    "describe",
                    id.as_str(),
                    "--format=json",
                ],
                QUICK,
                cancel,
            )
            .await?;
        verify_unbilled(&out, id)
    }

    /// One `gcloud services enable` for exactly `APIS`.
    pub async fn enable_apis(
        &self,
        id: &ProjectId,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let project = format!("--project={}", id.as_str());
        let mut args = vec!["services", "enable"];
        args.extend(APIS);
        args.extend([project.as_str(), "--quiet"]);
        self.exec(&args, ENABLE_WAIT, cancel).await.map(drop)
    }

    /// `gcloud services list --enabled`: all of `APIS` must be enabled. New
    /// projects also carry Google default APIs, so extras are not rejected.
    pub async fn verify_apis(
        &self,
        id: &ProjectId,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        let project = format!("--project={}", id.as_str());
        let out = self
            .exec(
                &["services", "list", "--enabled", &project, "--format=json"],
                QUICK,
                cancel,
            )
            .await?;
        verify_apis(&out)
    }

    /// One `gcloud auth revoke <email>` (explicit step; remote revocation is
    /// only as good as the SDK command succeeding).
    pub async fn revoke_account(
        &self,
        email: &Email,
        cancel: &CancellationToken,
    ) -> Result<(), Error> {
        self.exec(&["auth", "revoke", &email.0, "--quiet"], QUICK, cancel)
            .await
            .map(drop)
    }

    /// `gcloud auth list` must show no credentialed account at all.
    pub async fn prove_signed_out(&self, cancel: &CancellationToken) -> Result<(), Error> {
        if self.accounts(cancel).await?.is_empty() {
            Ok(())
        } else {
            Err(Error::Blocked)
        }
    }
}

/// Runs one child with bounded stdout and time; stderr/stdin are not used.
async fn run(
    mut command: Command,
    max: usize,
    limit: Duration,
    cancel: &CancellationToken,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    let mut group = Group::spawn(&mut command)?;
    let mut stdout = group.0.stdout.take().ok_or(Error::Unavailable)?;
    let work = async {
        let out = read_bounded(&mut stdout, max).await?;
        let status = group.0.wait().await.map_err(unavailable)?;
        if status.success() {
            Ok(out)
        } else {
            Err(Error::Unavailable)
        }
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Unavailable),
        r = tokio::time::timeout(limit, work) => r.unwrap_or(Err(Error::Unavailable)),
    }
}

async fn read_bounded<R: AsyncRead + Unpin>(
    reader: &mut R,
    max: usize,
) -> Result<Zeroizing<Vec<u8>>, Error> {
    // Full capacity up front so zeroizing storage is never reallocated.
    let mut out = Zeroizing::new(Vec::with_capacity(max));
    let mut chunk = Zeroizing::new([0u8; 4096]);
    loop {
        let n = reader.read(&mut chunk[..]).await.map_err(unavailable)?;
        if n == 0 {
            return Ok(out);
        }
        if out.len() + n > max {
            return Err(Error::Blocked);
        }
        out.extend_from_slice(&chunk[..n]);
    }
}

/// First complete non-empty line after the SDK's "browser has been opened" text.
fn extract_url(buf: &[u8]) -> Option<Zeroizing<String>> {
    let text = Zeroizing::new(String::from_utf8_lossy(buf).into_owned());
    let rest = text.split_once(OPENED_MARKER)?.1;
    for line in rest.split_inclusive('\n') {
        if !line.ends_with('\n') {
            return None;
        }
        let line = line.trim();
        if !line.is_empty() {
            return Some(Zeroizing::new(line.to_owned()));
        }
    }
    None
}

fn token(value: &str, min: usize, max: usize, extra: &[u8]) -> bool {
    (min..=max).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || extra.contains(&b))
}

/// Strict allowlist of the SDK's standard authorization request. The URL is
/// retained verbatim; nothing is rewritten.
fn validate_login_url(raw: &str, email: &Email) -> Result<LoginRequest, Error> {
    if raw.len() > 4096
        || !raw.starts_with(AUTH_PREFIX)
        || !raw.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(Error::Invalid);
    }
    let url = Url::parse(raw).map_err(|_| Error::Invalid)?;
    if url.scheme() != "https"
        || url.host_str() != Some("accounts.google.com")
        || url.port().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/o/oauth2/auth"
        || url.fragment().is_some()
    {
        return Err(Error::Invalid);
    }
    let mut q: BTreeMap<String, Zeroizing<String>> = BTreeMap::new();
    for (k, v) in url.query_pairs() {
        if q.insert(k.into_owned(), Zeroizing::new(v.into_owned()))
            .is_some()
        {
            return Err(Error::Invalid); // duplicate field
        }
    }
    const REQUIRED: [&str; 7] = [
        "response_type",
        "client_id",
        "redirect_uri",
        "scope",
        "state",
        "code_challenge",
        "code_challenge_method",
    ];
    const OPTIONAL: [&str; 3] = ["access_type", "prompt", "login_hint"];
    if REQUIRED.iter().any(|k| !q.contains_key(*k))
        || q.keys()
            .any(|k| !REQUIRED.contains(&k.as_str()) && !OPTIONAL.contains(&k.as_str()))
    {
        return Err(Error::Invalid);
    }
    let get = |k: &str| q.get(k).map(|v| v.as_str());
    let mut scopes: Vec<&str> = get("scope").unwrap_or("").split(' ').collect();
    scopes.sort_unstable();
    let mut expected = SCOPES;
    expected.sort_unstable();
    let state = get("state").unwrap_or("");
    let challenge = get("code_challenge").unwrap_or("");
    let ok = get("response_type") == Some("code")
        && get("client_id") == Some(SDK_CLIENT_ID)
        && scopes == expected
        && get("code_challenge_method") == Some("S256")
        && token(challenge, 43, 43, b"-_")
        && token(state, 16, 128, b"-_.~")
        && get("access_type").is_none_or(|v| v == "offline")
        && get("prompt").is_none_or(|v| matches!(v, "select_account" | "consent"))
        && get("login_hint").is_none_or(|v| v == email.0);
    let callback = get("redirect_uri").unwrap_or("");
    if !ok || !callback_ok(callback) {
        return Err(Error::Invalid);
    }
    Ok(LoginRequest {
        url: Zeroizing::new(raw.to_owned()),
        callback: Zeroizing::new(callback.to_owned()),
        state: Zeroizing::new(state.to_owned()),
        domain: email
            .0
            .split_once('@')
            .map(|(_, d)| d.to_owned())
            .unwrap_or_default(),
    })
}

/// Exact callback check. `expected` is the already-validated `redirect_uri`
/// (`http://localhost|127.0.0.1:<port>/`). Allowed query fields are `code` and
/// `state` (each exactly once) plus the Google-added `scope`, `authuser`,
/// `prompt`, `hd`, each validated; anything else (notably `error`) is rejected.
fn validate_callback(raw: &str, expected: &str, state: &str, domain: &str) -> Result<(), Error> {
    if raw.len() > 4096 || !raw.bytes().all(|b| b.is_ascii_graphic()) || !callback_ok(expected) {
        return Err(Error::Invalid);
    }
    let (url, base) = match (Url::parse(raw), Url::parse(expected)) {
        (Ok(url), Ok(base)) => (url, base),
        _ => return Err(Error::Invalid),
    };
    if url.scheme() != "http"
        || url.host_str() != base.host_str()
        || url.port() != base.port()
        || url.path() != base.path()
        || url.fragment().is_some()
        || !url.username().is_empty()
        || url.password().is_some()
        || state.is_empty()
    {
        return Err(Error::Invalid);
    }
    let (mut code, mut got_state) = (None, None);
    let mut seen: Vec<String> = Vec::new();
    for (k, v) in url.query_pairs() {
        let k = k.into_owned();
        if seen.contains(&k) || seen.len() >= 8 {
            return Err(Error::Invalid);
        }
        seen.push(k.clone());
        let v = Zeroizing::new(v.into_owned());
        let ok = match k.as_str() {
            "code" => {
                code = Some(());
                token(&v, 1, 512, b"-_./~")
            }
            "state" => {
                got_state = Some(v.as_str() == state);
                true
            }
            "scope" => {
                let mut extra = SCOPES.to_vec();
                extra.extend(["email", "profile"]);
                v.len() <= 1024 && !v.is_empty() && v.split(' ').all(|s| extra.contains(&s))
            }
            "authuser" => token(&v, 1, 2, b"") && v.bytes().all(|b| b.is_ascii_digit()),
            "prompt" => matches!(
                v.as_str(),
                "none"
                    | "consent"
                    | "select_account"
                    | "select_account consent"
                    | "consent select_account"
            ),
            "hd" => !domain.is_empty() && v.eq_ignore_ascii_case(domain),
            _ => false, // includes `error`, `error_description`, unknown fields
        };
        if !ok {
            return Err(Error::Invalid);
        }
    }
    if code.is_some() && got_state == Some(true) {
        Ok(())
    } else {
        Err(Error::Invalid)
    }
}

const DELIVER_WAIT: Duration = Duration::from_secs(10);
const MAX_DELIVER_RESPONSE: usize = 16 * 1024;

/// One fixed, direct `GET` to the SDK's own listener at 127.0.0.1:`port`. The
/// socket is raw TCP (no proxy environment can apply), the destination address is
/// fixed rather than resolved, no redirect is followed, the response is bounded and
/// only its status line is examined. `target` is the already-validated
/// path-and-query.
async fn deliver(
    port: u16,
    host: &str,
    target: &str,
    cancel: &CancellationToken,
) -> Result<(), Error> {
    use tokio::{io::AsyncWriteExt, net::TcpStream};
    let work = async {
        let addr = SocketAddr::from((std::net::Ipv4Addr::LOCALHOST, port));
        let mut stream = TcpStream::connect(addr).await.map_err(unavailable)?;
        let request = Zeroizing::new(format!(
            "GET {target} HTTP/1.1\r\nHost: {host}:{port}\r\nConnection: close\r\nAccept: */*\r\n\r\n"
        ));
        stream
            .write_all(request.as_bytes())
            .await
            .map_err(unavailable)?;
        let mut head = Zeroizing::new(Vec::with_capacity(MAX_DELIVER_RESPONSE));
        let mut chunk = Zeroizing::new([0u8; 1024]);
        loop {
            if head.windows(2).any(|w| w == b"\r\n") || head.len() >= MAX_DELIVER_RESPONSE {
                break;
            }
            let n = stream.read(&mut chunk[..]).await.map_err(unavailable)?;
            if n == 0 {
                break;
            }
            head.extend_from_slice(&chunk[..n]);
        }
        let line = head.split(|b| *b == b'\r').next().unwrap_or_default();
        let ok = (line.starts_with(b"HTTP/1.0 ") || line.starts_with(b"HTTP/1.1 "))
            && line.len() >= 12
            && matches!(line[9], b'2' | b'3')
            && line[10].is_ascii_digit()
            && line[11].is_ascii_digit();
        if ok { Ok(()) } else { Err(Error::Blocked) }
    };
    tokio::select! {
        biased;
        _ = cancel.cancelled() => Err(Error::Unavailable),
        r = tokio::time::timeout(DELIVER_WAIT, work) => r.unwrap_or(Err(Error::Unavailable)),
    }
}

/// The SDK's own loopback server: `http://localhost:<port>/` and nothing else.
fn callback_ok(raw: &str) -> bool {
    let Ok(url) = Url::parse(raw) else {
        return false;
    };
    url.as_str() == raw
        && url.scheme() == "http"
        && matches!(url.host_str(), Some("localhost" | "127.0.0.1"))
        && url.port().is_some_and(|p| p >= 1024)
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.username().is_empty()
        && url.password().is_none()
}

fn json(buf: &[u8]) -> Result<Value, Error> {
    serde_json::from_slice(buf).map_err(unavailable)
}

fn verify_project(buf: &[u8], id: &ProjectId, setup: &SetupLabel) -> Result<(), Error> {
    verify_project_value(&json(buf)?, id, setup)
}

/// Strict `projects list` result for one exact id: `[]` is Absent; exactly one
/// element proving our ownership is Owned. Anything else is never Absent.
fn parse_project_presence(
    buf: &[u8],
    id: &ProjectId,
    setup: &SetupLabel,
) -> Result<ProjectPresence, Error> {
    let v = json(buf)?;
    match v.as_array().ok_or(Error::Unavailable)?.as_slice() {
        [] => Ok(ProjectPresence::Absent),
        [one] if one.is_object() => {
            verify_project_value(one, id, setup).map(|()| ProjectPresence::Owned)
        }
        [_] => Err(Error::Unavailable),
        _ => Err(Error::Blocked),
    }
}

fn verify_project_value(v: &Value, id: &ProjectId, setup: &SetupLabel) -> Result<(), Error> {
    let state = v.get("lifecycleState").or_else(|| v.get("state"));
    let labels = v.get("labels").and_then(Value::as_object);
    let labels_ok = labels.is_some_and(|l| {
        l.len() == 2
            && l.get(PURPOSE_LABEL.0).and_then(Value::as_str) == Some(PURPOSE_LABEL.1)
            && l.get("moosuri-setup").and_then(Value::as_str) == Some(setup.0.as_str())
    });
    if v.get("projectId").and_then(Value::as_str) == Some(id.as_str())
        && v.get("name").and_then(Value::as_str) == Some(id.as_str())
        && state.and_then(Value::as_str) == Some("ACTIVE")
        && labels_ok
    {
        Ok(())
    } else {
        Err(Error::Blocked)
    }
}

fn verify_unbilled(buf: &[u8], id: &ProjectId) -> Result<(), Error> {
    let v = json(buf)?;
    let no_account = match v.get("billingAccountName") {
        None => true,
        Some(a) => a.as_str() == Some(""),
    };
    if v.get("projectId").and_then(Value::as_str) == Some(id.as_str())
        && v.get("billingEnabled") == Some(&Value::Bool(false))
        && no_account
    {
        Ok(())
    } else {
        Err(Error::Blocked)
    }
}

fn verify_apis(buf: &[u8]) -> Result<(), Error> {
    let v = json(buf)?;
    let list = v.as_array().ok_or(Error::Unavailable)?;
    let names: Vec<&str> = list
        .iter()
        .filter_map(|s| s.pointer("/config/name").and_then(Value::as_str))
        .collect();
    if APIS.iter().all(|api| names.contains(api)) {
        Ok(())
    } else {
        Err(Error::Blocked)
    }
}

fn parse_accounts(buf: &[u8]) -> Result<Vec<(String, String)>, Error> {
    let v = json(buf)?;
    v.as_array()
        .ok_or(Error::Unavailable)?
        .iter()
        .map(|a| {
            let field = |k: &str| {
                a.get(k)
                    .and_then(Value::as_str)
                    .map(str::to_ascii_lowercase)
            };
            Ok((
                field("account").ok_or(Error::Unavailable)?,
                a.get("status")
                    .and_then(Value::as_str)
                    .ok_or(Error::Unavailable)?
                    .to_owned(),
            ))
        })
        .collect()
}

#[cfg(test)]
mod tests;
