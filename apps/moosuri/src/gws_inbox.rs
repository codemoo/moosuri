//! Local, fixed Gmail metadata reader. The server receives neither OAuth credentials
//! nor general command authority; authentication remains with this device's gws.
use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use clap::Args;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::OpenOptions,
    io::Read,
    net::SocketAddr,
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{io::AsyncReadExt, sync::Semaphore};
use zeroize::Zeroizing;

const OUTPUT_LIMIT: usize = 131072;
#[derive(Args)]
pub struct Options {
    /// Only loopback listeners are accepted. Tunnel this endpoint separately if required.
    #[arg(long, default_value = "127.0.0.1:3221")]
    bind: SocketAddr,
    /// Absolute, owner-only regular file containing a random 64-character hex shared key.
    #[arg(long)]
    key_file: PathBuf,
    /// Exact Google mailbox identity; every provider request is pinned to it.
    #[arg(long)]
    mailbox: String,
    /// Explicit absolute gws executable; otherwise GWS_PATH, then PATH discovery.
    #[arg(long)]
    gws_path: Option<PathBuf>,
    /// Explicit local action: open official GWS read-only Gmail sign-in, then exit.
    #[arg(long)]
    login: bool,
}
struct Bridge {
    executable: PathBuf,
    mailbox: String,
    key_digest: [u8; 32],
    gate: Arc<Semaphore>,
    directory: tempfile::TempDir,
}
#[derive(Serialize)]
struct Inbox {
    mailbox: String,
    messages: Vec<Message>,
}
#[derive(Serialize)]
struct Message {
    id: String,
    from: String,
    subject: String,
    date: String,
    internal_date: i64,
}
#[derive(Deserialize)]
struct Empty {}

pub async fn run(options: &Options) -> Result<(), String> {
    if !options.bind.ip().is_loopback() || !valid_mailbox(&options.mailbox) {
        return Err("gws_inbox_requires_loopback_and_exact_mailbox".into());
    }
    if options.login {
        let executable = resolve_executable(options.gws_path.as_deref())?;
        let status = tokio::process::Command::new(executable)
            .args(["auth", "login", "--readonly", "--services", "gmail"])
            .status()
            .await
            .map_err(|_| "gws_inbox_login_failed")?;
        return if status.success() {
            Ok(())
        } else {
            Err("gws_inbox_login_failed".into())
        };
    }
    let key = read_key(&options.key_file)?;
    let executable = resolve_executable(options.gws_path.as_deref())?;
    let bridge = Arc::new(Bridge {
        executable,
        mailbox: options.mailbox.clone(),
        key_digest: Sha256::digest(key.as_bytes()).into(),
        gate: Arc::new(Semaphore::new(1)),
        directory: tempfile::tempdir().map_err(|_| "gws_inbox_private_directory_failed")?,
    });
    let version = bridge.command(&["--version"], None).await?;
    if !std::str::from_utf8(&version)
        .ok()
        .and_then(|s| s.lines().next())
        .is_some_and(|line| line.starts_with("gws 0.22."))
    {
        return Err("gws_inbox_requires_gws_0_22".into());
    }
    let listener = tokio::net::TcpListener::bind(options.bind)
        .await
        .map_err(|_| "gws_inbox_bind_failed")?;
    // No mailbox, key, provider response or diagnostic is printed.
    println!("Local gws INBOX metadata bridge ready.");
    axum::serve(listener, router(bridge))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await
        .map_err(|_| "gws_inbox_server_failed".into())
}
fn router(bridge: Arc<Bridge>) -> Router {
    Router::new().fallback(endpoint).with_state(bridge)
}
async fn endpoint(State(bridge): State<Arc<Bridge>>, request: Request) -> Response {
    let mut response = endpoint_inner(bridge, request).await;
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    response
}
async fn endpoint_inner(bridge: Arc<Bridge>, request: Request) -> Response {
    // Never expose a browser-operable or arbitrary local HTTP proxy.
    if request.headers().contains_key(header::ORIGIN)
        || request.headers().contains_key(header::COOKIE)
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let Some(key) = request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.strip_prefix("Bearer "))
    else {
        return StatusCode::UNAUTHORIZED.into_response();
    };
    let digest: [u8; 32] = Sha256::digest(key.as_bytes()).into();
    if key.len() != 64
        || ring::hmac::verify(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"moosuri/gws-inbox-key/v1"),
            &digest,
            ring::hmac::sign(
                &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, b"moosuri/gws-inbox-key/v1"),
                &bridge.key_digest,
            )
            .as_ref(),
        )
        .is_err()
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    let path = request.uri().path().to_owned();
    if request.method() != Method::POST
        || !["/v1/inbox/latest", "/v1/profile/check", "/v1/auth/login"].contains(&path.as_str())
        || request.uri().query().is_some()
    {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Ok(_permit) = bridge.gate.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let bytes = match tokio::time::timeout(
        Duration::from_secs(3),
        to_bytes(request.into_body(), 128),
    )
    .await
    {
        Ok(Ok(bytes)) => bytes,
        _ => return StatusCode::BAD_REQUEST.into_response(),
    };
    if path == "/v1/profile/check" {
        let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        let Some(nonce) = value
            .get("nonce")
            .and_then(Value::as_str)
            .filter(|s| uuid::Uuid::parse_str(s).is_ok())
        else {
            return StatusCode::BAD_REQUEST.into_response();
        };
        if value.as_object().is_none_or(|o| o.len() != 1) {
            return StatusCode::BAD_REQUEST.into_response();
        }
        let state = match bridge.profile().await {
            Ok(()) => "ready",
            Err(e) if e == "gws_inbox_mailbox_mismatch" => "wrong_account",
            Err(e) if e == "gws_inbox_login_required" => "login_required",
            Err(_) => "offline",
        };
        let mailbox = if state == "ready" {
            bridge.mailbox.as_str()
        } else {
            ""
        };
        let signed = serde_json::to_vec(&("moosuri/gws-profile/v1", nonce, state, mailbox))
            .expect("fixed profile tuple");
        let tag = ring::hmac::sign(
            &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &bridge.key_digest),
            &signed,
        );
        let signature: String = tag.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        return axum::Json(json!({"state":state,"mailbox":mailbox,"signature":signature}))
            .into_response();
    }
    // serde's ordinary empty struct permits unknown fields; explicitly require {}.
    let Ok(value) = serde_json::from_slice::<Value>(&bytes) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    if !value.as_object().is_some_and(|o| o.is_empty())
        || serde_json::from_value::<Empty>(value).is_err()
    {
        return StatusCode::BAD_REQUEST.into_response();
    }
    if path == "/v1/auth/login" {
        // Only this explicit authenticated server action can launch local sign-in.
        // No provider URL, secret, browser content or token crosses the bridge.
        let bridge = bridge.clone();
        drop(_permit);
        let Ok(permit) = bridge.gate.clone().try_acquire_owned() else {
            return StatusCode::TOO_MANY_REQUESTS.into_response();
        };
        let mut command = bridge.base_command();
        // Allow the official sign-in process to open the signed-in desktop's browser.
        #[cfg(target_os = "linux")]
        for name in [
            "DISPLAY",
            "XAUTHORITY",
            "WAYLAND_DISPLAY",
            "XDG_RUNTIME_DIR",
            "DBUS_SESSION_BUS_ADDRESS",
        ] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        command
            .args(["auth", "login", "--readonly", "--services", "gmail"])
            .stdout(Stdio::null());
        let Ok(mut child) = command.spawn() else {
            return StatusCode::BAD_GATEWAY.into_response();
        };
        tokio::spawn(async move {
            let _bridge = bridge;
            let _permit = permit;
            let _group = ProcessGroup(child.id());
            let _ = tokio::time::timeout(Duration::from_secs(300), child.wait()).await;
        });
        return axum::Json(json!({"state":"login_started"})).into_response();
    }
    match tokio::time::timeout(Duration::from_secs(45), bridge.latest()).await {
        Ok(Ok(inbox)) => axum::Json(inbox).into_response(),
        Ok(Err(_)) => StatusCode::BAD_GATEWAY.into_response(),
        Err(_) => StatusCode::GATEWAY_TIMEOUT.into_response(),
    }
}
impl Bridge {
    fn base_command(&self) -> tokio::process::Command {
        let mut cmd = tokio::process::Command::new(&self.executable);
        cmd.env_clear();
        for name in [
            "HOME",
            "PATH",
            "XDG_CONFIG_HOME",
            "GOOGLE_WORKSPACE_CLI_CONFIG_DIR",
            "GOOGLE_WORKSPACE_CLI_CREDENTIALS_FILE",
            "GOOGLE_WORKSPACE_CLI_TOKEN",
            "GOOGLE_WORKSPACE_CLI_CLIENT_ID",
            "GOOGLE_WORKSPACE_CLI_CLIENT_SECRET",
            "GOOGLE_WORKSPACE_CLI_KEYRING_BACKEND",
        ] {
            if let Some(value) = std::env::var_os(name) {
                cmd.env(name, value);
            }
        }
        cmd.env("GOOGLE_WORKSPACE_CLI_LOG", "off")
            .current_dir(self.directory.path())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        #[cfg(unix)]
        cmd.process_group(0);
        cmd
    }
    async fn command(
        &self,
        args: &[&str],
        params: Option<Value>,
    ) -> Result<Zeroizing<Vec<u8>>, String> {
        let mut cmd = self.base_command();
        cmd.args(args).stderr(Stdio::piped());
        if let Some(params) = params {
            cmd.arg("--params")
                .arg(params.to_string())
                .args(["--format", "json"]);
        }
        #[cfg(unix)]
        cmd.process_group(0);
        let mut child = cmd.spawn().map_err(|_| "gws_inbox_command_failed")?;
        let _group = ProcessGroup(child.id());
        tokio::time::timeout(Duration::from_secs(10), async {
            let mut bytes = Zeroizing::new(Vec::new());
            let mut errors = Zeroizing::new(Vec::new());
            let stdout = child.stdout.take().ok_or("gws_inbox_command_failed")?;
            let stderr = child.stderr.take().ok_or("gws_inbox_command_failed")?;
            let mut stdout = stdout.take((OUTPUT_LIMIT + 1) as u64);
            let mut stderr = stderr.take((OUTPUT_LIMIT + 1) as u64);
            let (out, err) = tokio::join!(
                stdout.read_to_end(&mut bytes),
                stderr.read_to_end(&mut errors)
            );
            out.map_err(|_| "gws_inbox_command_failed")?;
            err.map_err(|_| "gws_inbox_command_failed")?;
            if bytes.len() > OUTPUT_LIMIT || errors.len() > OUTPUT_LIMIT {
                return Err("gws_inbox_command_failed".into());
            }
            if !child
                .wait()
                .await
                .map_err(|_| "gws_inbox_command_failed")?
                .success()
            {
                if auth_failure(&bytes) || auth_failure(&errors) {
                    return Err("gws_inbox_login_required".into());
                }
                return Err("gws_inbox_command_failed".into());
            }
            Ok(bytes)
        })
        .await
        .map_err(|_| "gws_inbox_command_timeout")?
    }
    async fn api(&self, args: &[&str], mut params: Value) -> Result<Value, String> {
        params["userId"] = Value::String(self.mailbox.clone());
        let bytes = self.command(args, Some(params)).await?;
        serde_json::from_slice(&bytes).map_err(|_| "gws_inbox_invalid_response".into())
    }
    async fn profile(&self) -> Result<(), String> {
        // `me` is used only for identity. Listing remains pinned to the configured mailbox.
        let bytes = self
            .command(
                &["gmail", "users", "getProfile"],
                Some(json!({"userId":"me","fields":"emailAddress"})),
            )
            .await?;
        let profile: Value =
            serde_json::from_slice(&bytes).map_err(|_| "gws_inbox_invalid_response")?;
        if profile.get("emailAddress").and_then(Value::as_str) != Some(self.mailbox.as_str()) {
            return Err("gws_inbox_mailbox_mismatch".into());
        }
        Ok(())
    }
    async fn latest(self: &Arc<Self>) -> Result<Inbox, String> {
        self.profile().await?;
        let list = self.api(&["gmail", "users", "messages", "list"], json!({"labelIds":["INBOX"], "maxResults":20,"includeSpamTrash":false,"fields":"messages(id)"})).await?;
        let items = match list.get("messages") {
            Some(Value::Array(items)) if items.len() <= 20 => items.as_slice(),
            None if list.is_object() => &[],
            _ => return Err("gws_inbox_invalid_response".into()),
        };
        let mut messages = Vec::new();
        let mut ids = std::collections::HashSet::new();
        let mut selected = Vec::new();
        for item in items {
            let id = item
                .get("id")
                .and_then(Value::as_str)
                .filter(|s| valid_id(s))
                .ok_or("gws_inbox_invalid_response")?;
            if !ids.insert(id) {
                return Err("gws_inbox_invalid_response".into());
            }
            selected.push(id.to_owned());
        }
        for batch in selected.chunks(4) {
            let mut jobs = tokio::task::JoinSet::new();
            for id in batch {
                let bridge = self.clone();
                let id = id.clone();
                jobs.spawn(async move {
                    let message = bridge.api(&["gmail", "users", "messages", "get"], json!({"id":id,"format":"metadata","metadataHeaders":["From","Subject","Date"],"fields":"id,labelIds,internalDate,payload(headers)"})).await?;
                    parse_message(&message, &id)
                });
            }
            while let Some(result) = jobs.join_next().await {
                messages.push(result.map_err(|_| "gws_inbox_command_failed")??);
            }
        }
        messages.sort_by(|a, b| {
            b.internal_date
                .cmp(&a.internal_date)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(Inbox {
            mailbox: self.mailbox.clone(),
            messages,
        })
    }
}
fn auth_failure(bytes: &[u8]) -> bool {
    let value = serde_json::from_slice::<Value>(bytes).unwrap_or(Value::Null);
    if value
        .pointer("/error/code")
        .and_then(Value::as_i64)
        .is_some_and(|n| n == 401 || n == 403)
    {
        return true;
    }
    // Diagnostics are inspected privately and are never forwarded or logged.
    let text = Zeroizing::new(String::from_utf8_lossy(bytes).to_ascii_lowercase());
    [
        "no credentials found",
        "not authenticated",
        "invalid_grant",
        "authentication required",
        "login required",
        "autherror",
        "unauthenticated",
    ]
    .iter()
    .any(|s| text.contains(s))
}
struct ProcessGroup(Option<u32>);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0 {
            let _ = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
        }
    }
}
fn valid_mailbox(value: &str) -> bool {
    value.len() <= 254
        && value.split('@').count() == 2
        && value.split('@').all(|s| !s.is_empty())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~@".contains(&b))
}
fn valid_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 128 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn parse_message(value: &Value, expected_id: &str) -> Result<Message, String> {
    let invalid = || "gws_inbox_invalid_response".to_owned();
    if value.get("id").and_then(Value::as_str) != Some(expected_id)
        || !value
            .get("labelIds")
            .and_then(Value::as_array)
            .is_some_and(|v| v.iter().any(|x| x.as_str() == Some("INBOX")))
    {
        return Err(invalid());
    }
    let internal_date = value
        .get("internalDate")
        .and_then(Value::as_str)
        .and_then(|s| s.parse::<i64>().ok())
        .filter(|n| *n >= 0)
        .ok_or_else(invalid)?;
    let headers = value
        .pointer("/payload/headers")
        .and_then(Value::as_array)
        .filter(|a| a.len() <= 128)
        .ok_or_else(invalid)?;
    let get = |name: &str| -> Result<String, String> {
        let matching: Vec<_> = headers
            .iter()
            .filter(|h| {
                h.get("name")
                    .and_then(Value::as_str)
                    .is_some_and(|n| n.eq_ignore_ascii_case(name))
            })
            .collect();
        if matching.len() > 1 {
            return Err(invalid());
        }
        let Some(header) = matching.first() else {
            return Ok(String::new());
        };
        let text = header
            .get("value")
            .and_then(Value::as_str)
            .filter(|s| s.len() <= 16384)
            .ok_or_else(invalid)?;
        let mut clean = String::new();
        for c in text.chars().filter(|c| {
            !c.is_control() && !matches!(*c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        }) {
            if clean.len() + c.len_utf8() > if name == "Date" { 512 } else { 1024 } {
                break;
            }
            clean.push(c);
        }
        Ok(clean)
    };
    Ok(Message {
        id: expected_id.into(),
        from: get("From")?,
        subject: get("Subject")?,
        date: get("Date")?,
        internal_date,
    })
}
fn read_key(path: &Path) -> Result<Zeroizing<String>, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let invalid = || "gws_inbox_requires_private_key_file".to_owned();
    if !path.is_absolute() {
        return Err(invalid());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| invalid())?;
    let meta = file.metadata().map_err(|_| invalid())?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.nlink() != 1
        || meta.permissions().mode() & 0o077 != 0
        || meta.len() > 66
    {
        return Err(invalid());
    }
    let mut value = Zeroizing::new(String::new());
    file.take(67)
        .read_to_string(&mut value)
        .map_err(|_| invalid())?;
    let value = Zeroizing::new(value.trim_end_matches(['\r', '\n']).to_owned());
    if value.len() != 64 || !value.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    Ok(value)
}
fn resolve_executable(explicit: Option<&Path>) -> Result<PathBuf, String> {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let invalid = || "gws_inbox_executable_unavailable".to_owned();
    let selected = explicit
        .map(Path::to_path_buf)
        .or_else(|| std::env::var_os("GWS_PATH").map(PathBuf::from));
    let candidate = if let Some(selected) = selected {
        if !selected.is_absolute() {
            return Err(invalid());
        }
        selected
    } else {
        std::env::split_paths(&std::env::var_os("PATH").ok_or_else(invalid)?)
            .filter(|p| p.is_absolute())
            .map(|p| p.join("gws"))
            .find(|p| p.is_file())
            .ok_or_else(invalid)?
    };
    let mut canonical = candidate.canonicalize().map_err(|_| invalid())?;
    // npm's shim can auto-install binaries. Use its already installed native
    // sibling instead; starting this reader must never install software.
    if canonical.file_name().is_some_and(|name| name == "run.js") {
        canonical = canonical
            .parent()
            .ok_or_else(invalid)?
            .join("bin/gws")
            .canonicalize()
            .map_err(|_| invalid())?;
    }
    let meta = canonical.metadata().map_err(|_| invalid())?;
    if !meta.is_file()
        || ![0, unsafe { libc::geteuid() }].contains(&meta.uid())
        || meta.permissions().mode() & 0o022 != 0
        || meta.permissions().mode() & 0o111 == 0
    {
        return Err(invalid());
    }
    let mut magic = [0; 4];
    std::fs::File::open(&canonical)
        .and_then(|mut f| f.read_exact(&mut magic))
        .map_err(|_| invalid())?;
    if ![
        [0x7f, b'E', b'L', b'F'],
        [0xcf, 0xfa, 0xed, 0xfe],
        [0xfe, 0xed, 0xfa, 0xcf],
        [0xca, 0xfe, 0xba, 0xbe],
        [0xca, 0xfe, 0xba, 0xbf],
    ]
    .contains(&magic)
    {
        return Err("gws_inbox_requires_native_gws".into());
    }
    Ok(canonical)
}

#[cfg(test)]
#[path = "gws_inbox/tests.rs"]
mod tests;
