//! One bounded request per member socket. Never forward HTTP headers or return arbitrary broker fields.
use super::browser::{Failure, Service};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::os::unix::fs::{FileTypeExt, MetadataExt};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub(super) fn available(s: &Service) -> bool {
    s.socket
        .as_ref()
        .and_then(|p| std::fs::symlink_metadata(p).ok())
        .is_some_and(|m| {
            m.file_type().is_socket()
                && m.mode() & 0o077 == 0
                && m.uid() == unsafe { libc::geteuid() }
                && m.nlink() == 1
        })
}
#[derive(Deserialize)]
struct Envelope {
    ok: bool,
    code: Option<String>,
    result: Option<Value>,
}
pub(super) async fn call<T: DeserializeOwned>(
    s: &Service,
    workspace: &str,
    op: &str,
    args: Value,
) -> Result<T, Failure> {
    // The host has one command admission slot per member. Do not let overlapping frame polls
    // race a mutation; queue one bounded mutation and discard obsolete passive captures.
    let _slot = if matches!(op, "frame" | "tabs") {
        s.broker_slot.try_lock().map_err(|_| Failure::Busy)?
    } else {
        tokio::time::timeout(std::time::Duration::from_secs(10), s.broker_slot.lock())
            .await
            .map_err(|_| Failure::Busy)?
    };
    let retry = matches!(op, "tabs" | "frame");
    let mut answer = once(s, workspace, op, &args).await;
    if retry && matches!(answer, Err(Failure::Busy)) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        answer = once(s, workspace, op, &args).await;
    }
    answer
}
async fn once<T: DeserializeOwned>(
    s: &Service,
    workspace: &str,
    op: &str,
    args: &Value,
) -> Result<T, Failure> {
    let mut args = args.clone();
    if !available(s) {
        return Err(Failure::Unavailable);
    }
    args["v"] = 1.into();
    args["generation"] = s.owner.generation.into();
    args["workspace_id"] = workspace.into();
    args["op"] = op.into();
    let mut bytes = serde_json::to_vec(&args).map_err(|_| Failure::Invalid)?;
    if bytes.len() > 16 * 1024 {
        return Err(Failure::Invalid);
    }
    bytes.push(b'\n');
    let limit = if op == "frame" {
        3 * 1024 * 1024 + 8192
    } else {
        128 * 1024
    };
    let time = match op {
        "open" | "follow" => 80,
        "frame" | "tabs" => 8,
        _ => 45,
    };
    let future = async {
        let sock = s.socket.as_ref().ok_or(Failure::Unavailable)?;
        let mut stream = tokio::net::UnixStream::connect(sock)
            .await
            .map_err(|_| Failure::Unavailable)?;
        if stream.peer_cred().ok().map(|p| p.uid()) != Some(s.uid) {
            return Err(Failure::Unavailable);
        }
        stream
            .write_all(&bytes)
            .await
            .map_err(|_| Failure::Unknown)?;
        let mut body = Vec::new();
        stream
            .take(limit + 1)
            .read_to_end(&mut body)
            .await
            .map_err(|_| Failure::Unknown)?;
        if body.len() as u64 > limit || body.pop() != Some(b'\n') || body.contains(&b'\n') {
            return Err(Failure::Unknown);
        }
        let reply: Envelope = serde_json::from_slice(&body).map_err(|_| Failure::Unknown)?;
        if !reply.ok {
            return Err(match reply.code.as_deref() {
                Some("stale") => Failure::Stale,
                Some("invalid") => Failure::Invalid,
                Some("forbidden") => Failure::Forbidden,
                Some("missing") => Failure::Missing,
                Some("capacity") => Failure::Busy,
                _ => Failure::Unknown,
            });
        }
        serde_json::from_value(reply.result.ok_or(Failure::Unknown)?).map_err(|_| Failure::Unknown)
    };
    tokio::time::timeout(std::time::Duration::from_secs(time), future)
        .await
        .map_err(|_| Failure::Unknown)?
}
