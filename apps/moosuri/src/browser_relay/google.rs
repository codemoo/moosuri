//! Opt-in Google sign-in for exactly one enrolled member, through one fixed owner bridge socket.
//!
//! Member runtime -> relay (member browser socket): `{"v":1,"generation":g,"workspace_id":uuid,"op":"google_sign_in"}`.
//! Relay -> owner bridge: `{"v":1,"operation":"google_sign_in","request_id":<workspace uuid>}` (one JSON line).
//! Bridge -> relay: `{"v":1,"status":"ready|needs_human|blocked|unavailable","reason":<enum>}`.
//! Nothing else is ever sent: no email, URL, argument, secret or target comes from the browser or the member.
//! The bridge owns the fixed account and verifies the expected email at myaccount.google.com itself.
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, VecDeque},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    time::timeout,
};
use uuid::Uuid;

pub const OP: &str = "google_sign_in";
/// Whole attempt, including a slow human-free Google page flow. The member-side call waits slightly longer.
pub const DEADLINE: Duration = Duration::from_secs(255);
const REPLY_BYTES: u64 = 1024;
const REMEMBERED: usize = 64;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    Ready,
    NeedsHuman,
    Blocked,
    Unavailable,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Reason {
    Verified,
    SignInRequired,
    HumanVerification,
    AccountMismatch,
    BrowserRejected,
    NotConfigured,
    NotEnrolled,
    Busy,
    Timeout,
    BridgeError,
    AlreadyAttempted,
}
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Outcome {
    pub v: u32,
    pub status: Status,
    pub reason: Reason,
}
impl Outcome {
    pub fn unavailable(reason: Reason) -> Self {
        Self {
            v: 1,
            status: Status::Unavailable,
            reason,
        }
    }
    /// Only these status/reason pairs exist; "ready" can never carry any other reason.
    pub fn consistent(&self) -> bool {
        use Reason::*;
        self.v == 1
            && match self.status {
                Status::Ready => self.reason == Verified,
                Status::NeedsHuman => matches!(self.reason, SignInRequired | HumanVerification),
                Status::Blocked => matches!(self.reason, AccountMismatch | BrowserRejected),
                Status::Unavailable => !matches!(
                    self.reason,
                    Verified
                        | SignInRequired
                        | HumanVerification
                        | AccountMismatch
                        | BrowserRejected
                ),
            }
    }
}

/// The member-to-relay request. It is closed: exactly these keys, nothing else.
pub struct Request {
    pub workspace: Uuid,
    pub generation: i64,
}
impl Request {
    /// `None` when the line is not a Google sign-in operation at all (the normal tab protocol applies).
    pub fn parse(bytes: &[u8]) -> Option<Result<Self, &'static str>> {
        let value: Value = serde_json::from_slice(bytes).ok()?;
        let map = value.as_object()?;
        if map.get("op")? != OP {
            return None;
        }
        Some((|| {
            if map.len() != 4 || map.get("v") != Some(&json!(1)) {
                return Err("invalid");
            }
            let generation = map
                .get("generation")
                .and_then(Value::as_i64)
                .filter(|g| *g > 0)
                .ok_or("invalid")?;
            let workspace: Uuid =
                serde_json::from_value(map.get("workspace_id").cloned().ok_or("invalid")?)
                    .map_err(|_| "invalid")?;
            if workspace.is_nil() {
                return Err("invalid");
            }
            Ok(Self {
                workspace,
                generation,
            })
        })())
    }
}

#[derive(Clone)]
pub struct Config {
    pub account: Uuid,
    pub socket: PathBuf,
    pub peer_uid: u32,
}
/// All three flags or none. Absent flags disable the feature; a partial set is a configuration error.
pub fn config(
    account: Option<Uuid>,
    socket: Option<PathBuf>,
    peer_uid: Option<u32>,
) -> Result<Option<Config>, String> {
    let (account, socket, peer_uid) = match (account, socket, peer_uid) {
        (None, None, None) => return Ok(None),
        (Some(a), Some(s), Some(u)) => (a, s, u),
        _ => return Err("browser_relay_google_signin_incomplete".into()),
    };
    let parent = socket.parent().filter(|_| socket.file_name().is_some());
    let parent = parent.ok_or("browser_relay_google_signin_path_invalid")?;
    if account.is_nil() || !socket.is_absolute() || socket.as_os_str().len() > 100 {
        return Err("browser_relay_google_signin_invalid".into());
    }
    // The socket may appear later; its directory and every ancestor must already be root-trusted.
    super::trusted_ancestors(parent)?;
    let m = std::fs::symlink_metadata(parent).map_err(|_| "browser_relay_path_invalid")?;
    if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err("browser_relay_path_untrusted".into());
    }
    Ok(Some(Config {
        account,
        socket,
        peer_uid,
    }))
}

#[derive(Default)]
struct State {
    done: HashMap<Uuid, Option<Outcome>>,
    order: VecDeque<Uuid>,
    in_flight: bool,
}
/// Per-installation admission: one bridge attempt at a time, and every workspace id at most once.
pub struct Gate {
    config: Config,
    state: Mutex<State>,
}
struct Lease<'a>(&'a Gate);
impl Drop for Lease<'_> {
    fn drop(&mut self) {
        if let Ok(mut s) = self.0.state.lock() {
            s.in_flight = false;
        }
    }
}
impl Gate {
    pub fn new(config: Config) -> Self {
        Self {
            config,
            state: Mutex::default(),
        }
    }
    fn lease(&self, workspace: Uuid) -> Result<Lease<'_>, Outcome> {
        let mut s = self
            .state
            .lock()
            .map_err(|_| Outcome::unavailable(Reason::BridgeError))?;
        // A replay returns the recorded result; an attempt that never recorded one is uncertain and never re-run.
        if let Some(previous) = s.done.get(&workspace) {
            return Err(previous.unwrap_or(Outcome::unavailable(Reason::AlreadyAttempted)));
        }
        if s.in_flight {
            return Err(Outcome::unavailable(Reason::Busy));
        }
        s.in_flight = true;
        s.done.insert(workspace, None);
        s.order.push_back(workspace);
        while s.order.len() > REMEMBERED {
            if let Some(old) = s.order.pop_front() {
                s.done.remove(&old);
            }
        }
        Ok(Lease(self))
    }
    fn record(&self, workspace: Uuid, outcome: Outcome) {
        if let Ok(mut s) = self.state.lock()
            && let Some(slot) = s.done.get_mut(&workspace)
        {
            *slot = Some(outcome);
        }
    }
    /// One bridge attempt for `account`'s workspace. A different member is refused without touching the bridge.
    pub async fn run(&self, account: Uuid, workspace: Uuid) -> Outcome {
        if account != self.config.account {
            return Outcome::unavailable(Reason::NotEnrolled);
        }
        let _lease = match self.lease(workspace) {
            Ok(lease) => lease,
            Err(outcome) => return outcome,
        };
        let outcome = match timeout(DEADLINE, ask(&self.config, workspace)).await {
            Ok(Ok(outcome)) => outcome,
            Ok(Err(reason)) => Outcome::unavailable(reason),
            Err(_) => Outcome::unavailable(Reason::Timeout),
        };
        self.record(workspace, outcome);
        outcome
    }
}

fn verified_socket(path: &Path, uid: u32) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|m| {
        m.file_type().is_socket() && m.uid() == uid && m.mode() & 0o022 == 0 && m.nlink() == 1
    })
}
async fn ask(config: &Config, workspace: Uuid) -> Result<Outcome, Reason> {
    let bridge = Reason::BridgeError;
    if !verified_socket(&config.socket, config.peer_uid) {
        return Err(bridge);
    }
    let mut stream = UnixStream::connect(&config.socket)
        .await
        .map_err(|_| bridge)?;
    if stream.peer_cred().ok().map(|c| c.uid()) != Some(config.peer_uid) {
        return Err(bridge);
    }
    let mut line = serde_json::to_vec(&json!({"v":1,"operation":OP,"request_id":workspace}))
        .map_err(|_| bridge)?;
    line.push(b'\n');
    stream.write_all(&line).await.map_err(|_| bridge)?;
    let mut body = Vec::new();
    BufReader::new(stream)
        .take(REPLY_BYTES + 1)
        .read_until(b'\n', &mut body)
        .await
        .map_err(|_| bridge)?;
    if body.len() as u64 > REPLY_BYTES || body.pop() != Some(b'\n') {
        return Err(bridge);
    }
    let outcome: Outcome = serde_json::from_slice(&body).map_err(|_| bridge)?;
    outcome.consistent().then_some(outcome).ok_or(bridge)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::PermissionsExt,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
    };
    use tokio::net::UnixListener;

    struct Bridge {
        _dir: tempfile::TempDir,
        socket: PathBuf,
        seen: Arc<Mutex<Vec<Vec<u8>>>>,
        connections: Arc<AtomicUsize>,
    }
    fn bridge(reply: &'static str) -> Bridge {
        let dir = tempfile::Builder::new()
            .prefix("gs-")
            .tempdir_in("/tmp")
            .unwrap();
        let socket = dir.path().canonicalize().unwrap().join("b.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let connections = Arc::new(AtomicUsize::new(0));
        let (s, c) = (seen.clone(), connections.clone());
        tokio::spawn(async move {
            while let Ok((stream, _)) = listener.accept().await {
                c.fetch_add(1, Ordering::SeqCst);
                let mut reader = BufReader::new(stream);
                let mut line = Vec::new();
                let _ = reader.read_until(b'\n', &mut line).await;
                s.lock().unwrap().push(line);
                let _ = reader.get_mut().write_all(reply.as_bytes()).await;
            }
        });
        Bridge {
            _dir: dir,
            socket,
            seen,
            connections,
        }
    }
    fn gate(b: &Bridge, account: Uuid, uid: u32) -> Gate {
        Gate::new(Config {
            account,
            socket: b.socket.clone(),
            peer_uid: uid,
        })
    }
    fn me() -> u32 {
        unsafe { libc::geteuid() }
    }

    #[test]
    fn member_request_is_closed_and_typed() {
        let w = Uuid::new_v4();
        let ok = json!({"v":1,"generation":3,"workspace_id":w,"op":"google_sign_in"});
        let parsed = Request::parse(&serde_json::to_vec(&ok).unwrap())
            .unwrap()
            .ok()
            .unwrap();
        assert_eq!((parsed.workspace, parsed.generation), (w, 3));
        for extra in ["email", "url", "args"] {
            let mut v = ok.clone();
            v[extra] = json!("x");
            assert!(
                Request::parse(&serde_json::to_vec(&v).unwrap())
                    .unwrap()
                    .is_err()
            );
        }
        let tab = json!({"v":1,"generation":3,"workspace_id":w,"op":"tabs"});
        assert!(Request::parse(&serde_json::to_vec(&tab).unwrap()).is_none());
        assert!(config(None, None, None).unwrap().is_none());
        assert!(config(Some(w), None, Some(7)).is_err());
    }

    #[test]
    fn only_consistent_status_reason_pairs_exist() {
        let o = |status, reason| {
            Outcome {
                v: 1,
                status,
                reason,
            }
            .consistent()
        };
        assert!(o(Status::Ready, Reason::Verified));
        assert!(!o(Status::Ready, Reason::SignInRequired));
        assert!(!o(Status::NeedsHuman, Reason::Verified));
        assert!(o(Status::Blocked, Reason::AccountMismatch));
        assert!(!o(Status::Unavailable, Reason::Verified));
        assert!(o(Status::Unavailable, Reason::Timeout));
        assert!(
            serde_json::from_str::<Outcome>(
                r#"{"v":1,"status":"ready","reason":"verified","email":"a@b"}"#
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn bridge_gets_only_the_fixed_request_and_replay_is_not_rerun() {
        let b = bridge("{\"v\":1,\"status\":\"ready\",\"reason\":\"verified\"}\n");
        let (account, workspace) = (Uuid::new_v4(), Uuid::new_v4());
        let g = gate(&b, account, me());
        let first = g.run(account, workspace).await;
        assert_eq!(
            (first.status, first.reason),
            (Status::Ready, Reason::Verified)
        );
        assert_eq!(g.run(account, workspace).await, first);
        assert_eq!(b.connections.load(Ordering::SeqCst), 1);
        let sent: Value = serde_json::from_slice(&b.seen.lock().unwrap()[0]).unwrap();
        assert_eq!(
            sent,
            json!({"v":1,"operation":"google_sign_in","request_id":workspace})
        );
    }

    #[tokio::test]
    async fn another_member_never_reaches_the_bridge() {
        let b = bridge("{\"v\":1,\"status\":\"ready\",\"reason\":\"verified\"}\n");
        let g = gate(&b, Uuid::new_v4(), me());
        let o = g.run(Uuid::new_v4(), Uuid::new_v4()).await;
        assert_eq!(
            (o.status, o.reason),
            (Status::Unavailable, Reason::NotEnrolled)
        );
        assert_eq!(b.connections.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn wrong_peer_or_forged_reply_is_unavailable_and_not_retried() {
        let account = Uuid::new_v4();
        let b = bridge("{\"v\":1,\"status\":\"ready\",\"reason\":\"verified\"}\n");
        let wrong = gate(&b, account, me() ^ 1);
        let o = wrong.run(account, Uuid::new_v4()).await;
        assert_eq!(
            (o.status, o.reason),
            (Status::Unavailable, Reason::BridgeError)
        );
        assert_eq!(b.connections.load(Ordering::SeqCst), 0);
        let forged = bridge("{\"v\":1,\"status\":\"ready\",\"reason\":\"sign_in_required\"}\n");
        let (g, w) = (gate(&forged, account, me()), Uuid::new_v4());
        assert_eq!(g.run(account, w).await.reason, Reason::BridgeError);
        assert_eq!(g.run(account, w).await.reason, Reason::BridgeError);
        assert_eq!(forged.connections.load(Ordering::SeqCst), 1);
    }
}
