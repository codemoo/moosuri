//! One request from the installation's private socket. The enrolled VM runs
//! kiri-chrome's official fresh-profile protocol. Only the outcome leaves it.
use clap::Args;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Write},
    os::{
        fd::AsFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    process::{Child, Command},
    time::timeout,
};
use uuid::Uuid;

#[derive(Args)]
pub struct Options {
    /// Private state directory for sign-in receipts. No browser material is stored.
    #[arg(long)]
    state_directory: PathBuf,
    /// Owner-only file containing the enrolled member's Google email address.
    #[arg(long)]
    email_file: PathBuf,
    /// Absolute, owner-controlled Kiri VM SSH launcher installed by kiri-chrome.
    #[arg(long)]
    vm_ssh: PathBuf,
    /// Unix UID of the installation browser relay.
    #[arg(long, default_value_t = 0)]
    relay_uid: u32,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u8,
    operation: String,
    request_id: Uuid,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    v: u8,
    status: String,
    reason: String,
}
impl Receipt {
    fn new(status: &str, reason: &str) -> Self {
        Self {
            v: 1,
            status: status.into(),
            reason: reason.into(),
        }
    }
    fn unknown() -> Self {
        Self::new("unavailable", "bridge_error")
    }
}
const ORIGIN: &str = "https://accounts.google.com";
const GUEST: &str =
    r#"exec /usr/bin/python3 -B "$HOME/.local/share/codex-kiri-chrome/secure_google.py""#;

async fn frame<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Option<Value> {
    let mut bytes = Vec::new();
    (&mut *reader)
        .take(4097)
        .read_until(b'\n', &mut bytes)
        .await
        .ok()?;
    if bytes.len() > 4096 || bytes.pop()? != b'\n' {
        return None;
    }
    serde_json::from_slice(&bytes).ok()
}

fn guest(options: &Options, mode: &str) -> Option<Child> {
    Command::new(&options.vm_ssh)
        .arg(format!("{GUEST} {mode}"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .ok()
}
async fn send(child: &mut Child, value: Value) -> Option<()> {
    let mut bytes = serde_json::to_vec(&value).ok()?;
    bytes.push(b'\n');
    child.stdin.as_mut()?.write_all(&bytes).await.ok()?;
    Some(())
}

fn provider_result(value: &Value) -> Option<Receipt> {
    let status = value["status"].as_str()?;
    let reason = value["reason"].as_str()?;
    let receipt = match (status, reason) {
        ("blocked", "browser_rejected") => Receipt::new("blocked", "browser_rejected"),
        ("needs_human", "account_not_verified") | ("blocked", "unexpected_origin") => {
            Receipt::new("unavailable", "bridge_error")
        }
        ("needs_human", "provider_sign_in_required") => {
            Receipt::new("needs_human", "sign_in_required")
        }
        (
            "needs_human",
            "verification"
            | "phone_approval_timeout"
            | "owner_approval_required"
            | "unsupported_sign_in_step"
            | "form_changed",
        ) => Receipt::new("needs_human", "human_verification"),
        ("error" | "cancelled", _) => Receipt::unknown(),
        _ => return None,
    };
    Some(receipt)
}
fn hex32(value: &Value) -> Option<&str> {
    value
        .as_str()
        .filter(|v| v.len() == 32 && v.bytes().all(|b| b.is_ascii_hexdigit()))
}
async fn signin(options: &Options, email: &str) -> Option<Receipt> {
    let mut child = guest(options, "signin")?;
    let mut reader = BufReader::new(child.stdout.take()?);
    let opening = frame(&mut reader).await?;
    if opening != json!({"prompt":"identifier","origin":ORIGIN}) {
        return provider_result(&opening["result"]);
    }
    send(&mut child, json!({"value":email})).await?;
    let mut response = frame(&mut reader).await?;
    if let Some(notice) = response["notice"].as_str() {
        if !matches!(notice, "phone_approval" | "verification_pending")
            || response != json!({"notice":notice,"origin":ORIGIN})
        {
            return None;
        }
        response = frame(&mut reader).await?;
    }
    drop(child.stdin.take());
    if !child.wait().await.ok()?.success() {
        return None;
    }
    let result = &response["result"];
    if let Some(status) = provider_result(result) {
        return Some(status);
    }
    if result["status"] != "ready"
        || result["profile"] != "fresh"
        || hex32(&result["tab"]).is_none()
    {
        return None;
    }
    let session = hex32(&result["session"])?;
    // This operation verifies sign-in. It has no downstream browser task that
    // needs to retain the authenticated profile.
    let mut finish = guest(options, "session")?;
    let mut reader = BufReader::new(finish.stdout.take()?);
    send(
        &mut finish,
        json!({"session":session,"request":{"op":"finish"}}),
    )
    .await?;
    drop(finish.stdin.take());
    if frame(&mut reader).await? != json!({"finished":true}) || !finish.wait().await.ok()?.success()
    {
        return None;
    }
    Some(Receipt::new("ready", "verified"))
}

fn owner_path(path: &Path, uid: u32, directory: bool) -> bool {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return false;
    };
    path.is_absolute()
        && fs::canonicalize(path).ok().as_deref() == Some(path)
        && meta.uid() == uid
        && meta.mode() & 0o077 == 0
        && if directory {
            meta.is_dir()
        } else {
            meta.is_file() && meta.nlink() == 1
        }
}

async fn attempt(
    options: &Options,
    email: &str,
    id: Uuid,
    socket: &mut UnixStream,
) -> io::Result<Receipt> {
    let directory = File::open(&options.state_directory)?;
    if directory.try_lock_exclusive().is_err() {
        return Ok(Receipt::new("unavailable", "busy"));
    }
    let path = options.state_directory.join(id.to_string());
    let pending = match OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
    {
        Ok(mut file) => {
            file.write_all(b"{\"v\":1,\"status\":\"unavailable\",\"reason\":\"outcome_unknown\"}")?;
            file.sync_all()?;
            directory.sync_all()?;
            file
        }
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
            let bytes = fs::read(&path)?;
            return Ok(serde_json::from_slice(&bytes).unwrap_or_else(|_| Receipt::unknown()));
        }
        Err(e) => return Err(e),
    };
    drop(pending);
    let mut extra = [0];
    let result = tokio::select! {
        biased;
        _ = socket.read(&mut extra) => Receipt::unknown(),
        result = timeout(Duration::from_secs(105), signin(options,email)) =>
            result.ok().flatten().unwrap_or_else(Receipt::unknown),
    };
    let mut file = tempfile::NamedTempFile::new_in(&options.state_directory)?;
    serde_json::to_writer(&mut file, &result)?;
    file.as_file().sync_all()?;
    file.persist(path)?;
    directory.sync_all()?;
    Ok(result)
}

pub async fn serve(options: &Options) -> Result<(), String> {
    let uid = unsafe { libc::geteuid() };
    if uid == 0
        || !owner_path(&options.state_directory, uid, true)
        || !owner_path(&options.email_file, uid, false)
        || !owner_path(&options.vm_ssh, uid, false)
    {
        return Err("signin_owner_configuration_invalid".into());
    }
    let email = fs::read_to_string(&options.email_file)
        .map_err(|_| "signin_owner_configuration_invalid")?;
    let email = email.trim_end_matches(['\n', '\r']);
    if email.len() > 254
        || email.split('@').count() != 2
        || email.chars().any(char::is_whitespace)
        || !email.is_ascii()
    {
        return Err("signin_owner_configuration_invalid".into());
    }
    let fd = std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| "signin_socket_required")?;
    let stream = std::os::unix::net::UnixStream::from(fd);
    stream
        .set_nonblocking(true)
        .map_err(|_| "signin_socket_required")?;
    let mut stream = UnixStream::from_std(stream).map_err(|_| "signin_socket_required")?;
    if stream.peer_cred().ok().map(|p| p.uid()) != Some(options.relay_uid) {
        return Err("signin_peer_forbidden".into());
    }
    let mut reader = BufReader::new(&mut stream);
    let first = timeout(Duration::from_secs(5), frame(&mut reader))
        .await
        .ok()
        .flatten()
        .ok_or("signin_invalid_request")?;
    if !reader.buffer().is_empty() {
        return Err("signin_invalid_request".into());
    }
    let request: Request = serde_json::from_value(first).map_err(|_| "signin_invalid_request")?;
    if request.v != 1 || request.operation != "google_sign_in" || request.request_id.is_nil() {
        return Err("signin_invalid_request".into());
    }
    let result = attempt(options, email, request.request_id, &mut stream)
        .await
        .map_err(|_| "signin_receipt_unavailable")?;
    let mut bytes = serde_json::to_vec(&result).map_err(|_| "signin_receipt_unavailable")?;
    bytes.push(b'\n');
    stream
        .write_all(&bytes)
        .await
        .map_err(|_| "signin_client_closed")?;
    Ok(())
}
