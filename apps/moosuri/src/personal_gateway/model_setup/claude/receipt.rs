//! Non-secret operation receipt and the subscription config publication. The
//! receipt never holds an email, URL, code or session token; it binds the FULL
//! owner (account, tenant, volume, generation). The config is published only
//! through the same private-directory atomic pattern as the API connection and
//! is compare-and-set on the current connection revision.
use super::*;
use crate::engine_login::{Endpoint, Owner, model_catalog};

pub(super) static LOCK: Mutex<()> = Mutex::new(());
const FILE: &str = "claude-subscription-operation.json";
const PENDING: &str = "claude-subscription-operation.pending";
/// Legacy connect v2 and disconnect v3 remain readable. A v4 connect receipt
/// durably holds a browser-start intent until actual cleanup is acknowledged.
pub(super) const VERSION: u32 = 2;
const DISCONNECT_VERSION: u32 = 3;
const BROWSER_VERSION: u32 = 4;
fn is_false(value: &bool) -> bool {
    !*value
}

/// What the one operation receipt is for. A disconnect names the connection
/// revision it removes in `target_revision`; its own `request_id` is only the
/// operation id and is never a host request or a connection revision.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Mode {
    #[default]
    Connect,
    Disconnect,
}
impl Mode {
    pub(super) fn version(self) -> u32 {
        match self {
            Self::Connect => VERSION,
            Self::Disconnect => DISCONNECT_VERSION,
        }
    }
    fn is_connect(&self) -> bool {
        *self == Self::Connect
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub version: u32,
    pub request_id: Uuid,
    pub account_id: Uuid,
    pub tenant_id: Uuid,
    pub volume_id: Uuid,
    pub generation: i64,
    pub digest: String,
    #[serde(default)]
    pub config_sha256: Option<String>,
    pub replace_revision: Option<Uuid>,
    pub model: String,
    pub effort: Option<String>,
    pub state: Stage,
    pub updated: u64,
    #[serde(default, skip_serializing_if = "Mode::is_connect")]
    pub mode: Mode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_revision: Option<Uuid>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub browser_pending: bool,
}
/// How the currently published config relates to one receipt's request.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Relation {
    /// No config at all.
    Absent,
    /// A readable config that names another revision.
    Other,
    /// The config names this request and matches the receipt's owner and model.
    Exact,
    /// The config names this request but not exactly what the receipt says.
    Mismatch,
    /// The config could not be read or has no parseable revision.
    Unreadable,
}
impl Relation {
    /// Only a definite "this request is not published" may lead to a
    /// destructive host retirement.
    pub(super) fn may_retire(self) -> bool {
        matches!(self, Self::Absent | Self::Other)
    }
}
/// Exact parser of the published runtime-model config against one receipt.
pub(super) fn classify(config: Option<&[u8]>, r: &Receipt) -> Relation {
    let Some(bytes) = config else {
        return Relation::Absent;
    };
    let Ok(v) = serde_json::from_slice::<serde_json::Value>(bytes) else {
        return Relation::Unreadable;
    };
    let Some(revision) = v
        .get("connection_revision")
        .and_then(|x| x.as_str())
        .and_then(|s| s.parse::<Uuid>().ok())
    else {
        return Relation::Unreadable;
    };
    if revision != r.request_id {
        return Relation::Other;
    }
    if r.config_sha256.as_deref() != Some(sha(bytes).as_str()) {
        return Relation::Mismatch;
    }
    let binding = v["bindings"]
        .as_array()
        .filter(|b| b.len() == 1)
        .map(|b| &b[0]);
    let id = |b: &serde_json::Value, key: &str, want: Uuid| {
        b[key].as_str().and_then(|s| s.parse::<Uuid>().ok()) == Some(want)
    };
    let exact = v["subscription"] == "claude_code"
        && v["model"].as_str() == Some(r.model.as_str())
        && v["effort"].as_str() == r.effort.as_deref()
        && binding.is_some_and(|b| {
            id(b, "account_id", r.account_id)
                && id(b, "tenant_id", r.tenant_id)
                && id(b, "volume_id", r.volume_id)
                && b["generation"].as_i64() == Some(r.generation)
        });
    if exact {
        Relation::Exact
    } else {
        Relation::Mismatch
    }
}
pub(super) fn sha(bytes: &[u8]) -> String {
    use sha2::Digest;
    format!("{:x}", sha2::Sha256::digest(bytes))
}
impl Receipt {
    /// Full owner equality: no receipt is ever shown to, or recovered by, an
    /// administrator that differs in any of account, tenant, volume or generation.
    pub(super) fn owner_is(&self, a: &Allocation) -> bool {
        owner_of(a).is_some_and(|o| {
            o.account_id == self.account_id
                && o.tenant_id == self.tenant_id
                && o.volume_id == self.volume_id
                && o.generation == self.generation
        })
    }
    /// A stored non-terminal state has no live job here. If the config now
    /// names exactly this request it still needs the root mark; an unreadable or
    /// mismatching config keeps cleanup pending; otherwise it was interrupted.
    pub(super) fn effective(&self, rel: Relation) -> Stage {
        if self.state.terminal() {
            return self.state;
        }
        // A disconnect without a live job always needs its recovery to run:
        // the config relation says nothing about the root cleanup.
        if self.mode == Mode::Disconnect {
            return Stage::CleanupPending;
        }
        match rel {
            Relation::Exact => Stage::CleanupPending,
            Relation::Mismatch | Relation::Unreadable => Stage::CleanupPending,
            Relation::Absent | Relation::Other => {
                if self.state == Stage::CleanupPending {
                    Stage::CleanupPending
                } else {
                    Stage::Interrupted
                }
            }
        }
    }
    /// Needs explicit cleanup before another setup may start.
    pub(super) fn unresolved(&self, _rel: Relation) -> bool {
        !self.state.terminal()
    }
    pub(super) fn view(&self, rel: Relation) -> serde_json::Value {
        self.view_as(self.effective(rel))
    }
    pub(super) fn view_as(&self, state: Stage) -> serde_json::Value {
        let mut v = serde_json::json!({"request_id":self.request_id,"state":state,
            "model":self.model,"effort":self.effort});
        if state == Stage::Ready {
            v["connection_revision"] = serde_json::json!(self.request_id);
        }
        if self.mode == Mode::Disconnect {
            v["kind"] = serde_json::json!("disconnect");
        }
        v
    }
    /// Version, mode, target and state must agree: a v2 receipt is a connect
    /// receipt, a v3 one a disconnect receipt with its target.
    fn well_formed(&self) -> bool {
        let lifecycle = matches!(self.state, Stage::Disconnecting | Stage::Disconnected);
        match (self.version, self.mode) {
            (VERSION, Mode::Connect) => {
                self.target_revision.is_none() && !lifecycle && !self.browser_pending
            }
            (BROWSER_VERSION, Mode::Connect) => {
                self.target_revision.is_none()
                    && !lifecycle
                    && self.browser_pending
                    && !self.state.terminal()
            }
            (DISCONNECT_VERSION, Mode::Disconnect) => {
                self.target_revision
                    .is_some_and(|t| !t.is_nil() && t != self.request_id)
                    && self.replace_revision.is_none()
                    && !self.browser_pending
                    && (lifecycle
                        || self.state == Stage::CleanupPending
                        || (self.state == Stage::Failed && self.config_sha256.is_none()))
            }
            _ => false,
        }
    }
}

fn publish(d: &File, pending: &str, target: &str, bytes: &[u8]) -> io::Result<()> {
    match unlink(d, pending) {
        Ok(()) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(e) => return Err(e),
    }
    let mut file = open(d, pending, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    let from = CString::new(pending).map_err(|_| io::ErrorKind::InvalidInput)?;
    let to = CString::new(target).map_err(|_| io::ErrorKind::InvalidInput)?;
    if unsafe { libc::renameat(d.as_raw_fd(), from.as_ptr(), d.as_raw_fd(), to.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    d.sync_all()
}
pub(super) fn read_receipt(d: &File) -> io::Result<Option<Receipt>> {
    let Some(bytes) = read(d, FILE)? else {
        return Ok(None);
    };
    let r: Receipt =
        serde_json::from_slice(&bytes).map_err(|_| io::Error::from(io::ErrorKind::InvalidData))?;
    if !r.well_formed() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(Some(r))
}
pub(super) fn write_receipt(d: &File, r: &Receipt) -> io::Result<()> {
    let mut r = r.clone();
    r.version = if r.browser_pending {
        BROWSER_VERSION
    } else {
        r.mode.version()
    };
    if !r.well_formed() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let bytes = serde_json::to_vec(&r).map_err(|_| io::ErrorKind::InvalidData)?;
    if bytes.len() > MAX_FILE {
        return Err(io::ErrorKind::InvalidData.into());
    }
    publish(d, PENDING, FILE, &bytes)
}
/// First write of a new setup. An unresolved receipt of another request is
/// never overwritten (the in-memory check can be stale after a restart); it
/// needs explicit cleanup. `AlreadyExists` means exactly that.
pub(super) fn begin(d: &File, fresh: &Receipt) -> io::Result<()> {
    if let Some(old) = read_receipt(d)? {
        if old.request_id != fresh.request_id && old.unresolved(inspect(d, Some(&old)).relation) {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
    }
    write_receipt(d, fresh)
}

pub(super) struct Seen {
    /// The published revision; `Err` when a config exists but is unreadable.
    pub current: Result<Option<Uuid>, ()>,
    pub relation: Relation,
}
/// One read of the config: its revision and its relation to `receipt`.
pub(super) fn inspect(d: &File, receipt: Option<&Receipt>) -> Seen {
    match read(d, CONFIG_FILE) {
        Err(_) => Seen {
            current: Err(()),
            relation: Relation::Unreadable,
        },
        Ok(None) => Seen {
            current: Ok(None),
            relation: Relation::Absent,
        },
        Ok(Some(c)) => Seen {
            current: revision_of(&c).map(Some).ok_or(()),
            relation: receipt.map_or(Relation::Other, |r| classify(Some(c.as_slice()), r)),
        },
    }
}
/// The published connection revision. A config that exists but has no
/// parseable revision is never silently treated as absent.
pub(super) fn current_revision(d: &File) -> io::Result<Option<Uuid>> {
    match read(d, CONFIG_FILE)? {
        None => Ok(None),
        Some(c) => revision_of(&c)
            .map(Some)
            .ok_or_else(|| io::ErrorKind::InvalidData.into()),
    }
}

pub(super) struct Publication<'a> {
    pub request: Uuid,
    pub owner: &'a Owner,
    pub peer_uid: u32,
    pub endpoint: &'a Endpoint,
    pub model: &'a str,
    pub effort: Option<&'a str>,
}
/// Runtime-model subscription config for exactly one owner/peer/worker. Other
/// members get no worker: the dynamic broker reports them unavailable until
/// their own subscription worker exists. Mirrors the runtime-model checks
/// (the parser itself is private to that module).
pub(super) fn config_bytes(p: &Publication) -> Option<Vec<u8>> {
    let euid = unsafe { libc::geteuid() };
    let o = p.owner;
    if p.request.is_nil()
        || o.generation <= 0
        || [o.account_id, o.tenant_id, o.volume_id]
            .iter()
            .any(Uuid::is_nil)
        || p.peer_uid == 0
        || p.endpoint.validate().is_err()
        || p.endpoint.worker_uid == p.peer_uid
        || p.endpoint.worker_uid == euid
        || !valid_model(p.model)
        || !p.effort.is_none_or(valid_effort)
        || model_catalog(p.model, p.effort, &[]).is_none()
    {
        return None;
    }
    serde_json::to_vec(&serde_json::json!({
        "version":1,"connection_revision":p.request,"provider":provider(),
        "model":p.model,"effort":p.effort,"models":[],"key_file":null,
        "subscription":"claude_code",
        "bindings":[{"account_id":o.account_id,"tenant_id":o.tenant_id,"volume_id":o.volume_id,
            "generation":o.generation,"peer_uid":p.peer_uid,
            "budget":{"max_calls":100,"max_tokens":500_000,
                "reservation_tokens":moosuri_core::DEFAULT_MEMBER_MODEL_RESERVATION_TOKENS},
            "worker":p.endpoint}]
    }))
    .ok()
}
/// Compare-and-set publication, run under the registry lock and a fresh admin
/// pin. The active config is untouched unless `base` is still current. The
/// exact same bytes are an idempotent success.
pub(super) fn commit(d: &File, expected: &[u8], base: Option<Uuid>) -> io::Result<()> {
    match read(d, CONFIG_FILE)? {
        Some(current) if current.as_slice() == expected => return confirm_durable(d, expected),
        Some(current) if revision_of(&current) != base || base.is_none() => {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        None if base.is_some() => return Err(io::ErrorKind::AlreadyExists.into()),
        _ => (),
    }
    publish(d, PENDING_FILE, CONFIG_FILE, expected)?;
    // The replaced API key (if any) is unreferenced now. Best effort.
    prune_keys(d, "");
    d.sync_all()
}

/// Re-open the protected file, verify the same bytes, then fsync both file and
/// directory. Visibility after a failed rename/fsync is not durability.
pub(super) fn confirm_durable(d: &File, expected: &[u8]) -> io::Result<()> {
    let mut file = open(d, CONFIG_FILE, libc::O_RDONLY)?;
    let mut bytes = Zeroizing::new(Vec::new());
    (&mut file)
        .take((MAX_FILE + 1) as u64)
        .read_to_end(&mut bytes)?;
    if bytes.as_slice() != expected {
        return Err(io::ErrorKind::InvalidData.into());
    }
    // Rewriting avoids relying on a second fsync after an earlier writeback
    // error that may have cleared dirty pages. The caller holds the config lock.
    publish(d, PENDING_FILE, CONFIG_FILE, expected)
}

/// Outcome of one publication attempt, judged from the config actually on disk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Settled {
    /// The exact config is durable; the root record is not yet marked.
    Exact,
    /// The exact config is durable and the root record is marked published.
    Marked,
    /// The exact config is durable but the root mark failed (retryable).
    RootPending,
    /// The base revision is no longer current: nothing of ours was written.
    Conflict,
    /// An IO failure and the re-read config is definitely not ours.
    Absent,
    /// An IO failure and the config could not be re-read: nothing is known.
    Uncertain,
}
pub(super) fn settle(d: &File, expected: &[u8], base: Option<Uuid>) -> Settled {
    settle_after(d, expected, commit(d, expected, base))
}
/// A failed fsync may follow a durable rename: the actual config decides, and
/// an unreadable config is never reported as absent.
pub(super) fn settle_after(d: &File, expected: &[u8], committed: io::Result<()>) -> Settled {
    match committed {
        Ok(()) => Settled::Exact,
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Settled::Conflict,
        Err(_) => match read(d, CONFIG_FILE) {
            Ok(Some(c)) if c.as_slice() == expected => {
                if confirm_durable(d, expected).is_ok() {
                    Settled::Exact
                } else {
                    Settled::Uncertain
                }
            }
            Ok(_) => Settled::Absent,
            Err(_) => Settled::Uncertain,
        },
    }
}
