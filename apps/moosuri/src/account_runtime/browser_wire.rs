//! One bounded request per member socket. Never forward HTTP headers or return arbitrary broker fields.
use super::browser::{Failure, Service};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use std::{
    collections::HashMap,
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, OnceLock, Weak},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    sync::Mutex as DispatchLock,
};

/// Everything one broker request needs from a member runtime: the fixed socket, its expected peer uid,
/// the grant generation and the member's single command-admission slot. Borrowed, so the human browser
/// service and the public-watch reader share exactly the same admission slot and the same checks.
pub(super) struct Endpoint<'a> {
    pub(super) socket: Option<&'a Path>,
    pub(super) uid: u32,
    pub(super) generation: u64,
    pub(super) slot: &'a DispatchLock<()>,
}
fn endpoint(s: &Service) -> Endpoint<'_> {
    Endpoint {
        socket: s.socket.as_deref(),
        uid: s.uid,
        generation: s.owner.generation,
        slot: &s.broker_slot,
    }
}

/// The member's single command-admission slot for one broker socket. Whoever talks to the same socket
/// (the human browser service, the public-watch reader) gets the same slot, so their commands queue
/// instead of colliding at the host. Entries disappear once every holder is dropped.
pub(super) fn slot_for(socket: Option<&Path>) -> Arc<DispatchLock<()>> {
    static SLOTS: OnceLock<Mutex<HashMap<PathBuf, Weak<DispatchLock<()>>>>> = OnceLock::new();
    let Some(socket) = socket else {
        return Arc::default();
    };
    let Ok(mut slots) = SLOTS.get_or_init(Default::default).lock() else {
        return Arc::default();
    };
    slots.retain(|_, slot| slot.strong_count() > 0);
    if let Some(slot) = slots.get(socket).and_then(Weak::upgrade) {
        return slot;
    }
    let slot: Arc<DispatchLock<()>> = Arc::default();
    slots.insert(socket.to_path_buf(), Arc::downgrade(&slot));
    slot
}

fn present(e: &Endpoint<'_>) -> bool {
    e.socket
        .and_then(|p| std::fs::symlink_metadata(p).ok())
        .is_some_and(|m| {
            m.file_type().is_socket()
                && m.mode() & 0o077 == 0
                && m.uid() == unsafe { libc::geteuid() }
                && m.nlink() == 1
        })
}
pub(super) fn available(s: &Service) -> bool {
    present(&endpoint(s))
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
    call_on(&endpoint(s), workspace, op, args).await
}

/// The only broker operations a disposable public read may use: open the fresh workspace on the pinned
/// page, and stop it. There is deliberately no way to name any other operation through this entry point
/// (no frame, input, follow, switch, tab or takeover), so a public read can never act on a page.
#[derive(Clone, Copy)]
pub(super) enum Disposable {
    Open,
    Stop,
}
pub(super) fn disposable_available(e: &Endpoint<'_>) -> bool {
    present(e)
}
pub(super) async fn disposable<T: DeserializeOwned>(
    e: &Endpoint<'_>,
    workspace: &str,
    op: Disposable,
    args: Value,
) -> Result<T, Failure> {
    let op = match op {
        Disposable::Open => "open",
        Disposable::Stop => "stop",
    };
    call_on(e, workspace, op, args).await
}

async fn call_on<T: DeserializeOwned>(
    e: &Endpoint<'_>,
    workspace: &str,
    op: &str,
    args: Value,
) -> Result<T, Failure> {
    // The host has one command admission slot per member. Do not let overlapping frame polls
    // race a mutation; queue one bounded mutation and discard obsolete passive captures.
    let _slot = if matches!(op, "frame" | "tabs") {
        e.slot.try_lock().map_err(|_| Failure::Busy)?
    } else {
        tokio::time::timeout(std::time::Duration::from_secs(10), e.slot.lock())
            .await
            .map_err(|_| Failure::Busy)?
    };
    let retry = matches!(op, "tabs" | "frame");
    let mut answer = once(e, workspace, op, &args).await;
    if retry && matches!(answer, Err(Failure::Busy)) {
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        answer = once(e, workspace, op, &args).await;
    }
    answer
}
async fn once<T: DeserializeOwned>(
    e: &Endpoint<'_>,
    workspace: &str,
    op: &str,
    args: &Value,
) -> Result<T, Failure> {
    let mut args = args.clone();
    if !present(e) {
        return Err(Failure::Unavailable);
    }
    args["v"] = 1.into();
    args["generation"] = e.generation.into();
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
        let sock = e.socket.ok_or(Failure::Unavailable)?;
        let mut stream = tokio::net::UnixStream::connect(sock)
            .await
            .map_err(|_| Failure::Unavailable)?;
        if stream.peer_cred().ok().map(|p| p.uid()) != Some(e.uid) {
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
