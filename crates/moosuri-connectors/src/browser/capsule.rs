//! Trusted fixed Docker launcher for one unauthenticated rendered-page read.
//! No command, mount, environment, action or tenant comes from model arguments.
use super::{BrowserError, net::public_url, private_proxy};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, Command},
    sync::Semaphore,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
const SECCOMP_DIGEST: &str = "18c6523f4de6aebb9d220d98fea8a03edb381d97237f954ed9d1f47d233f661c";
const OWNER: &str = "io.moosuri.browser.owner";
const READ: &str = "io.moosuri.browser.read";
const PROJECT: &str = "com.docker.compose.project";
const MAX_RESPONSE: usize = 64 * 1024;
const DOCKER_CONFIG: &str = "/run/moosuri-browser-empty-docker-config";
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CapsuleConfig {
    pub image_id: String,
    pub network_id: String,
    pub proxy_container_id: String,
    pub proxy_ip: String,
    pub owner: Uuid,
    pub project: String,
    pub seccomp_file: PathBuf,
    pub verified: bool,
    pub smoke_evidence_sha256: String,
    pub javascript_enabled: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedPage {
    pub url: String,
    pub title: String,
    pub text: String,
    pub links: Vec<RenderedLink>,
    pub truncated: bool,
    pub warning: String,
    pub retrieved_at_unix_ms: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RenderedLink {
    pub text: String,
    pub url: String,
}
#[derive(Clone)]
pub struct CapsuleReader {
    config: Arc<CapsuleConfig>,
    pin: String,
    capacity: Arc<Semaphore>,
    #[cfg(test)]
    fixture: Option<Arc<Fixture>>,
}
#[cfg(test)]
struct Fixture {
    script: String,
    cleaned: std::sync::atomic::AtomicUsize,
}
fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl CapsuleConfig {
    pub fn from_json(bytes: &str) -> Result<Self, BrowserError> {
        if bytes.len() > 16384 {
            return Err(BrowserError::Setup);
        }
        let config: Self = serde_json::from_str(bytes).map_err(|_| BrowserError::Setup)?;
        config.validate()?;
        Ok(config)
    }
    fn validate(&self) -> Result<(), BrowserError> {
        if !self.verified
            || !hex64(&self.smoke_evidence_sha256)
            || self.smoke_evidence_sha256.bytes().all(|b| b == b'0')
            || !self.image_id.strip_prefix("sha256:").is_some_and(hex64)
            || !hex64(&self.network_id)
            || !hex64(&self.proxy_container_id)
            || self.owner.is_nil()
            || self.project.is_empty()
            || self.project.len() > 80
            || !self
                .project
                .bytes()
                .next()
                .is_some_and(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            || !self
                .project
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_-".contains(&b))
            || !self.seccomp_file.is_absolute()
        {
            return Err(BrowserError::IsolationRequired);
        }
        private_proxy(&self.proxy_ip)?;
        validate_seccomp(&self.seccomp_file)
    }
}
fn validate_seccomp(path: &Path) -> Result<(), BrowserError> {
    use std::io::Read;
    let mut options = std::fs::OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    }
    let file = options
        .open(path)
        .map_err(|_| BrowserError::IsolationRequired)?;
    let meta = file
        .metadata()
        .map_err(|_| BrowserError::IsolationRequired)?;
    if !meta.is_file() || meta.len() > 65536 {
        return Err(BrowserError::IsolationRequired);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.nlink() != 1
            || meta.mode() & 0o022 != 0
            || !(meta.uid() == 0 || meta.uid() == unsafe { libc::geteuid() })
        {
            return Err(BrowserError::IsolationRequired);
        }
    }
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| BrowserError::IsolationRequired)?;
    if format!("{:x}", Sha256::digest(&bytes)) != SECCOMP_DIGEST {
        return Err(BrowserError::IsolationRequired);
    }
    Ok(())
}
impl CapsuleReader {
    pub fn new(config: CapsuleConfig) -> Result<Self, BrowserError> {
        config.validate()?;
        let pin = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&config).map_err(|_| BrowserError::Setup)?)
        );
        Ok(Self {
            config: Arc::new(config),
            pin,
            capacity: Arc::new(Semaphore::new(2)),
            #[cfg(test)]
            fixture: None,
        })
    }
    pub fn deployment_pin(&self) -> &str {
        &self.pin
    }
    /// The caller supplies an authenticated account UUID, never a model argument.
    /// Dropping this future cancels an independently owned cleanup task as well.
    pub async fn read(
        &self,
        account: Uuid,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<RenderedPage, BrowserError> {
        let url = public_url(url)?.to_string();
        if account.is_nil() {
            return Err(BrowserError::IsolationRequired);
        }
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| BrowserError::Capacity)?;
        let local = cancel.child_token();
        let _drop = CancelOnDrop(local.clone());
        let this = self.clone();
        let task = tokio::spawn(async move {
            let _capacity = capacity;
            this.owned_read(account, &url, &local).await
        });
        task.await.map_err(|_| BrowserError::Setup)?
    }
    async fn owned_read(
        &self,
        account: Uuid,
        url: &str,
        cancel: &CancellationToken,
    ) -> Result<RenderedPage, BrowserError> {
        if cancel.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        let read = Uuid::new_v4();
        let name = format!("moosuri-browser-read-{read}");
        #[cfg(test)]
        let fixture = self.fixture.is_some();
        #[cfg(not(test))]
        let fixture = false;
        if !fixture {
            if !cfg!(target_os = "linux") {
                return Err(BrowserError::IsolationRequired);
            }
            self.config.validate()?;
            self.validate_resources(cancel).await?;
            empty_docker_config(Path::new(DOCKER_CONFIG))?;
        }
        let mut command = self.worker_command(account, read, &name);
        command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| BrowserError::Setup)?;
        let mut creation_observed = false;
        let result = {
            let operation = read_worker(
                &mut child,
                account,
                url,
                self.config.javascript_enabled,
                &mut creation_observed,
            );
            tokio::select! {biased;_=cancel.cancelled()=>Err(BrowserError::Cancelled),result=tokio::time::timeout(Duration::from_secs(40),operation)=>result.unwrap_or(Err(BrowserError::Timeout))}
        };
        // Always close/kill/reap the CLI, even on malformed or stalled stdout. Docker
        // removal is by fresh name plus exact owner/read labels; it never removes siblings.
        let cleanup = self
            .cleanup(&mut child, &name, read, creation_observed)
            .await;
        #[cfg(test)]
        if let Some(f) = &self.fixture {
            f.cleaned.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        cleanup?;
        result
    }
    fn worker_command(&self, account: Uuid, read: Uuid, name: &str) -> Command {
        #[cfg(test)]
        if let Some(f) = &self.fixture {
            let mut c = Command::new("/bin/sh");
            c.arg("-c")
                .arg(&f.script)
                .env_clear()
                .env("PATH", "/usr/bin:/bin");
            return c;
        }
        let mut c = docker();
        c.args([
            "run",
            "--rm",
            "--interactive",
            "--pull=never",
            "--name",
            name,
            "--network",
            &self.config.network_id,
            "--read-only",
            "--cap-drop=ALL",
            "--cap-add=NET_ADMIN",
            "--cap-add=SETUID",
            "--cap-add=SETGID",
            "--cap-add=SETPCAP",
            "--security-opt=no-new-privileges:true",
        ])
        .arg(format!(
            "--security-opt=seccomp={}",
            self.config.seccomp_file.display()
        ))
        .args([
            "--tmpfs",
            "/profiles:rw,noexec,nosuid,size=128m,uid=10001,gid=10001,mode=0700",
            "--tmpfs",
            "/tmp:rw,noexec,nosuid,size=64m,mode=1777",
            "--tmpfs",
            "/run:rw,noexec,nosuid,size=1m,mode=0755",
            "--shm-size=256m",
            "--pids-limit=256",
            "--memory=768m",
            "--cpus=1",
        ])
        .arg(format!("--label={OWNER}={}", self.config.owner))
        .arg(format!("--label={READ}={read}"))
        .arg(format!("--label={PROJECT}={}", self.config.project))
        .arg(format!("--env=MOOSURI_BROWSER_TENANT={account}"))
        .arg(format!(
            "--env=MOOSURI_BROWSER_PROXY_IP={}",
            self.config.proxy_ip
        ))
        .arg(format!(
            "--env=MOOSURI_BROWSER_JAVASCRIPT={}",
            if self.config.javascript_enabled {
                "enabled"
            } else {
                "disabled"
            }
        ))
        .arg(&self.config.image_id);
        c
    }
    async fn validate_resources(&self, cancel: &CancellationToken) -> Result<(), BrowserError> {
        let net = docker_output(
            &["network", "inspect", &self.config.network_id],
            cancel,
            Duration::from_secs(3),
        )
        .await?;
        let rows: Vec<Value> =
            serde_json::from_slice(&net).map_err(|_| BrowserError::IsolationRequired)?;
        let network = rows
            .first()
            .filter(|_| rows.len() == 1)
            .ok_or(BrowserError::IsolationRequired)?;
        if network["Id"] != self.config.network_id
            || network["Internal"] != true
            || network["Labels"][OWNER] != self.config.owner.to_string()
            || network["Labels"][PROJECT] != self.config.project
        {
            return Err(BrowserError::IsolationRequired);
        }
        let proxy = docker_output(
            &[
                "container",
                "inspect",
                "--format",
                PROXY_FORMAT,
                &self.config.proxy_container_id,
            ],
            cancel,
            Duration::from_secs(3),
        )
        .await?;
        let proxy: Value =
            serde_json::from_slice(&proxy).map_err(|_| BrowserError::IsolationRequired)?;
        let attached = proxy["networks"].as_object().is_some_and(|nets| {
            nets.values().any(|n| {
                n["NetworkID"] == self.config.network_id && n["IPAddress"] == self.config.proxy_ip
            })
        });
        if proxy["id"] != self.config.proxy_container_id
            || proxy["image"] != self.config.image_id
            || proxy["running"] != true
            || proxy["labels"][OWNER] != self.config.owner.to_string()
            || proxy["labels"][PROJECT] != self.config.project
            || !attached
        {
            return Err(BrowserError::IsolationRequired);
        }
        Ok(())
    }
    async fn cleanup(
        &self,
        child: &mut Child,
        name: &str,
        read: Uuid,
        creation_observed: bool,
    ) -> Result<(), BrowserError> {
        if let Some(mut input) = child.stdin.take() {
            let _ = tokio::time::timeout(
                Duration::from_millis(100),
                input.write_all(b"{\"op\":\"shutdown\"}\n"),
            )
            .await;
            drop(input);
        }
        let reaped = match tokio::time::timeout(Duration::from_millis(250), child.wait()).await {
            Ok(Ok(_)) => true,
            _ => {
                let _ = child.start_kill();
                matches!(
                    tokio::time::timeout(Duration::from_millis(500), child.wait()).await,
                    Ok(Ok(_))
                )
            }
        };
        #[cfg(test)]
        if self.fixture.is_some() {
            return if reaped {
                Ok(())
            } else {
                Err(BrowserError::Setup)
            };
        }
        let cancel = CancellationToken::new();
        // A killed CLI does not establish that the daemon finished its create.
        // Reconcile boundedly and remove an immutable ID only after exact identity
        // checks. Before readiness, never-observed absence remains uncertain.
        for attempt in 0..3 {
            let listed = docker_output(
                &[
                    "container",
                    "ls",
                    "--all",
                    "--no-trunc",
                    "--format",
                    OWNED_LIST_FORMAT,
                    "--filter",
                    &format!("name=^/{name}$"),
                ],
                &cancel,
                Duration::from_millis(500),
            )
            .await?;
            match cleanup_decision(
                &listed,
                name,
                read,
                self.config.owner,
                creation_observed,
                attempt == 2,
            )? {
                CleanupDecision::Clean => {
                    return if reaped {
                        Ok(())
                    } else {
                        Err(BrowserError::Setup)
                    };
                }
                CleanupDecision::Remove(id) => {
                    let removed = docker_output(
                        &["container", "rm", "--force", &id],
                        &cancel,
                        Duration::from_secs(1),
                    )
                    .await;
                    // --rm can finish (or start removal) after the list above.
                    // A failed removal is not proof of either presence or absence.
                    // Reconcile only the already observed immutable ID; never
                    // remove a replacement with the same name.
                    confirm_removal(removed, || async {
                        let remaining = docker_output(
                            &[
                                "container",
                                "ls",
                                "--all",
                                "--no-trunc",
                                "--format",
                                OWNED_LIST_FORMAT,
                                "--filter",
                                &format!("id={id}"),
                            ],
                            &cancel,
                            Duration::from_millis(250),
                        )
                        .await?;
                        match cleanup_decision(
                            &remaining,
                            name,
                            read,
                            self.config.owner,
                            true,
                            true,
                        )? {
                            CleanupDecision::Clean => Ok(true),
                            CleanupDecision::Remove(found) if found == id => Ok(false),
                            _ => Err(BrowserError::IsolationRequired),
                        }
                    })
                    .await?;
                    return if reaped {
                        Ok(())
                    } else {
                        Err(BrowserError::Setup)
                    };
                }
                CleanupDecision::Retry => tokio::time::sleep(Duration::from_millis(100)).await,
                CleanupDecision::Unconfirmed => return Err(BrowserError::CleanupUnconfirmed),
            }
        }
        Err(BrowserError::CleanupUnconfirmed)
    }
}
async fn confirm_removal<F, Fut>(
    removed: Result<Vec<u8>, BrowserError>,
    mut absent: F,
) -> Result<(), BrowserError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<bool, BrowserError>>,
{
    if removed.is_ok() {
        return Ok(());
    }
    for attempt in 0..3 {
        if absent().await? {
            return Ok(());
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Err(BrowserError::CleanupUnconfirmed)
}
const PROXY_FORMAT: &str = r#"{"id":{{json .Id}},"image":{{json .Image}},"running":{{json .State.Running}},"labels":{{json .Config.Labels}},"networks":{{json .NetworkSettings.Networks}}}"#;
// Docker list prints only these public identity fields, never environment or
// host configuration. --no-trunc supplies the full immutable container ID.
const OWNED_LIST_FORMAT: &str = r#"{"id":{{json .ID}},"name":{{json .Names}},"owner":{{json (.Label "io.moosuri.browser.owner")}},"read":{{json (.Label "io.moosuri.browser.read")}}}"#;
#[derive(Debug, PartialEq, Eq)]
enum CleanupDecision {
    Clean,
    Remove(String),
    Retry,
    Unconfirmed,
}
fn cleanup_decision(
    listed: &[u8],
    name: &str,
    read: Uuid,
    owner: Uuid,
    creation_observed: bool,
    last: bool,
) -> Result<CleanupDecision, BrowserError> {
    if listed.iter().all(u8::is_ascii_whitespace) {
        return Ok(if creation_observed {
            CleanupDecision::Clean
        } else if last {
            CleanupDecision::Unconfirmed
        } else {
            CleanupDecision::Retry
        });
    }
    let found: Value =
        serde_json::from_slice(listed).map_err(|_| BrowserError::IsolationRequired)?;
    let id = found["id"]
        .as_str()
        .filter(|id| hex64(id))
        .ok_or(BrowserError::IsolationRequired)?;
    if found["name"] != name
        || found["owner"] != owner.to_string()
        || found["read"] != read.to_string()
    {
        return Err(BrowserError::IsolationRequired);
    }
    Ok(CleanupDecision::Remove(id.to_owned()))
}
// Docker's config can inject proxy environment variables even with env_clear.
// Reserve an absent or empty private directory; never load an operator's login config.
fn empty_docker_config(path: &Path) -> Result<(), BrowserError> {
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err(BrowserError::IsolationRequired),
    };
    if !meta.is_dir() {
        return Err(BrowserError::IsolationRequired);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if meta.mode() & 0o022 != 0
            || !(meta.uid() == 0 || meta.uid() == unsafe { libc::geteuid() })
        {
            return Err(BrowserError::IsolationRequired);
        }
    }
    if std::fs::read_dir(path)
        .map_err(|_| BrowserError::IsolationRequired)?
        .next()
        .is_some()
    {
        return Err(BrowserError::IsolationRequired);
    }
    Ok(())
}
fn docker() -> Command {
    let mut c = Command::new("/usr/bin/docker");
    c.env_clear()
        .env("PATH", "/usr/bin:/bin")
        .env("LANG", "C.UTF-8")
        .args([
            "--host=unix:///var/run/docker.sock",
            "--config=/run/moosuri-browser-empty-docker-config",
        ]);
    c
}
async fn docker_output(
    args: &[&str],
    cancel: &CancellationToken,
    deadline: Duration,
) -> Result<Vec<u8>, BrowserError> {
    empty_docker_config(Path::new(DOCKER_CONFIG))?;
    let mut c = docker();
    c.args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = c.spawn().map_err(|_| BrowserError::Setup)?;
    let operation = async {
        let mut bytes = Vec::new();
        child
            .stdout
            .take()
            .ok_or(BrowserError::Setup)?
            .take(MAX_RESPONSE as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| BrowserError::Setup)?;
        if bytes.len() > MAX_RESPONSE {
            return Err(BrowserError::OutputTooLarge);
        }
        let status = child.wait().await.map_err(|_| BrowserError::Setup)?;
        if !status.success() {
            return Err(BrowserError::Setup);
        }
        Ok(bytes)
    };
    let result = tokio::select! {biased;_=cancel.cancelled()=>Err(BrowserError::Cancelled),result=tokio::time::timeout(deadline,operation)=>result.unwrap_or(Err(BrowserError::Timeout))};
    if !matches!(child.try_wait(), Ok(Some(_))) {
        let _ = child.start_kill();
        if !matches!(
            tokio::time::timeout(Duration::from_millis(500), child.wait()).await,
            Ok(Ok(_))
        ) {
            return Err(BrowserError::Setup);
        }
    }
    result
}
struct CancelOnDrop(CancellationToken);
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.cancel();
    }
}
async fn read_worker(
    child: &mut Child,
    account: Uuid,
    url: &str,
    javascript: bool,
    creation_observed: &mut bool,
) -> Result<RenderedPage, BrowserError> {
    let stdout = child.stdout.take().ok_or(BrowserError::Setup)?;
    let mut reader = BufReader::new(stdout);
    let ready = read_line(&mut reader, 2048).await?;
    if ready["ready"] != true
        || ready["javascript_enabled"] != javascript
        || ready["tenant"] != account.to_string()
    {
        return Err(BrowserError::Setup);
    }
    *creation_observed = true;
    let mut request =
        serde_json::to_vec(&json!({"op":"open","url":url})).map_err(|_| BrowserError::Setup)?;
    request.push(b'\n');
    child
        .stdin
        .as_mut()
        .ok_or(BrowserError::Setup)?
        .write_all(&request)
        .await
        .map_err(|_| BrowserError::Setup)?;
    let response = read_line(&mut reader, MAX_RESPONSE).await?;
    if response["ok"] != true {
        return Err(BrowserError::Network);
    }
    let result = &response["result"];

    let url = result["url"].as_str().ok_or(BrowserError::Protocol)?;
    let url = public_url(url)?.to_string();
    let text = result["text"]
        .as_str()
        .filter(|s| s.len() <= 8192)
        .ok_or(BrowserError::OutputTooLarge)?
        .to_owned();
    let title = result["title"]
        .as_str()
        .filter(|s| s.len() <= 512)
        .ok_or(BrowserError::OutputTooLarge)?
        .to_owned();
    let links = result["links"]
        .as_array()
        .filter(|v| v.len() <= 4)
        .ok_or(BrowserError::Protocol)?
        .iter()
        .map(|v| {
            let text = v["text"]
                .as_str()
                .filter(|s| s.len() <= 512)
                .ok_or(BrowserError::OutputTooLarge)?
                .into();
            let url = public_url(v["url"].as_str().ok_or(BrowserError::Protocol)?)?.to_string();
            Ok(RenderedLink { text, url })
        })
        .collect::<Result<Vec<_>, BrowserError>>()?;
    let truncated = result["truncated"]
        .as_bool()
        .ok_or(BrowserError::Protocol)?;
    let retrieved_at_unix_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| BrowserError::Setup)?
        .as_millis()
        .try_into()
        .map_err(|_| BrowserError::Setup)?;
    Ok(RenderedPage {
        url,
        title,
        text,
        links,
        truncated,
        warning: if javascript {
            "isolated_browser; public_unauthenticated; javascript_enabled"
        } else {
            "isolated_browser; public_unauthenticated; javascript_disabled"
        }
        .into(),
        retrieved_at_unix_ms,
    })
}
async fn read_line<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Result<Value, BrowserError> {
    let mut bytes = Vec::new();
    (&mut *reader)
        .take(limit as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| BrowserError::Protocol)?;
    if bytes.len() > limit {
        return Err(BrowserError::OutputTooLarge);
    }
    if !bytes.ends_with(b"\n") {
        return Err(BrowserError::Protocol);
    }
    serde_json::from_slice(&bytes).map_err(|_| BrowserError::Protocol)
}
#[cfg(test)]
mod tests;
