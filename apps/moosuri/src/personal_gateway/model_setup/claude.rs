//! The installation administrator's own Claude subscription as the managed
//! connection. One authenticated admin session drives a bounded in-memory job:
//! host preparation, the official login helper, activation, then a
//! compare-and-set publication of the runtime-model config and the root mark
//! (`host::published`) inside the same registry-locked commit. The session
//! token, authorization URL and code live only in memory and never in a
//! receipt or log.
//!
//! The sign-in happens inside this Kiri: the helper's authorization URL goes
//! only to a sealed browser owned by a supervisor task (`browser`), which
//! captures the official callback itself and hands the code once to the helper.
//! No URL or code is ever part of an HTTP request or response.
//!
//! Ownership semantics. The initiating session (same session token and login
//! epoch) alone sees the browser frame, may send it input or cancel a running
//! login. Any other session of the exact same owner (account, tenant, volume,
//! generation) may read the state and, only for an operation that needs cleanup
//! (live `cleanup_pending`, or a receipt left by a restart), take over the
//! cleanup. No other owner sees or touches the operation.
//!
//! Host calls (`prepare`, `activate`, `retire`, `retire_request`) run blocking
//! work that cannot be cancelled by dropping the future, so they are never
//! raced against a timeout: a deadline cancels their token and the call is
//! awaited to completion. The global setup slot (a non-terminal job) and the
//! owner lease are released only after the host reports the result.
use super::*;
use crate::{engine_login::Owner, installation::claude_runtime::connection as host};
use moosuri_connectors::browser::capsule::{WorkspaceFrame, WorkspaceInput};
use serde_json::{Value, json};
use std::process::Stdio;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    sync::mpsc,
    time::{MissedTickBehavior, interval, sleep, timeout},
};
pub(in crate::personal_gateway) mod browser;
mod lifecycle;
mod protocol;
mod receipt;
#[cfg(test)]
mod tests;
use protocol::{Outcome, Sequence, Step};
use receipt::{Mode, Receipt, Relation, Settled};

const UNAVAILABLE: StatusCode = StatusCode::SERVICE_UNAVAILABLE;
const MAX_JOBS: usize = 4;
const PREPARE_LIMIT: Duration = Duration::from_secs(20 * 60);
/// The login itself is bounded; a throttled or hidden page never cancels it.
const LOGIN_LIMIT: Duration = Duration::from_secs(6 * 60);
const ACTIVATE_LIMIT: Duration = Duration::from_secs(2 * 60);
const EXIT_GRACE: Duration = Duration::from_secs(10);
const AUTH_INTERVAL: Duration = Duration::from_secs(2);
const RETENTION: Duration = Duration::from_secs(600);

pub(in crate::personal_gateway) type Jobs = Arc<Mutex<Vec<Job>>>;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Stage {
    Preparing,
    AwaitingCode,
    Verifying,
    Activating,
    Ready,
    Cancelled,
    Failed,
    Interrupted,
    CleanupPending,
    /// Explicit administrator disconnect in progress (never cancellable).
    Disconnecting,
    /// Root cleanup acknowledged and this terminal receipt durable.
    Disconnected,
}
impl Stage {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Ready | Self::Cancelled | Self::Failed | Self::Interrupted | Self::Disconnected
        )
    }
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum Stop {
    Cancelled,
    Failed,
}
#[derive(Clone, Copy, PartialEq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Op {
    Start,
    Status,
    /// Latest sanitized browser frame (initiating session only).
    Frame,
    /// One human input to the browser frame (initiating session only).
    Input,
    Cancel,
    /// `request_id` is the fresh operation id; `replace_revision` the exact
    /// current connection revision to disconnect. Nothing else is accepted.
    Disconnect,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    op: Op,
    request_id: Uuid,
    #[serde(default)]
    email: Option<String>,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    hosting_terms_accepted: Option<bool>,
    #[serde(default)]
    replace_revision: Option<Uuid>,
    /// Token of the frame the input was made on (`op:input` only).
    #[serde(default)]
    frame_token: Option<String>,
    #[serde(default)]
    input: Option<WorkspaceInput>,
}
impl Input {
    fn well_formed(&self) -> bool {
        let bare = self.email.is_none()
            && self.model.is_none()
            && self.effort.is_none()
            && self.hosting_terms_accepted.is_none()
            && self.replace_revision.is_none();
        let no_browser = self.frame_token.is_none() && self.input.is_none();
        match self.op {
            Op::Start => {
                no_browser
                    && self.email.is_some()
                    && self.model.is_some()
                    && self.hosting_terms_accepted.is_some()
            }
            Op::Input => bare && self.frame_token.is_some() && self.input.is_some(),
            Op::Status | Op::Frame | Op::Cancel => bare && no_browser,
            Op::Disconnect => {
                self.email.is_none()
                    && self.model.is_none()
                    && self.effort.is_none()
                    && self.hosting_terms_accepted.is_none()
                    && no_browser
                    && self.replace_revision.is_some()
            }
        }
    }
}
fn valid_model(model: &str) -> bool {
    model.starts_with("claude-")
        && model.len() <= 200
        && model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
}
fn valid_effort(effort: &str) -> bool {
    matches!(effort, "low" | "medium" | "high")
}
fn valid_email(email: &str) -> bool {
    email.len() <= 254
        && email.bytes().all(|b| b.is_ascii_graphic())
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty()
                && !domain.contains('@')
                && domain.contains('.')
                && !domain.starts_with('.')
                && !domain.ends_with('.')
        })
}
/// Binds a request id to the full owner and inputs, so a reused id with
/// different input (or replacement target) is a conflict, never a replay.
fn digest_of(
    a: &Allocation,
    email: &str,
    model: &str,
    effort: Option<&str>,
    replace: Option<Uuid>,
) -> String {
    hash(&format!(
        "claude-subscription-v2\n{}\n{}\n{}\n{}\n{email}\n{model}\n{}\n{}",
        a.account_id,
        a.tenant_id,
        a.volume_id,
        a.generation,
        effort.unwrap_or("-"),
        replace.map(|r| r.to_string()).unwrap_or_default()
    ))
}
fn owner_of(a: &Allocation) -> Option<Owner> {
    Some(Owner {
        account_id: Uuid::parse_str(&a.account_id).ok()?,
        tenant_id: Uuid::parse_str(&a.tenant_id).ok()?,
        volume_id: Uuid::parse_str(&a.volume_id).ok()?,
        generation: a.generation,
    })
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

pub(in crate::personal_gateway) struct Job {
    id: Uuid,
    allocation: Allocation,
    epoch: i64,
    /// Current controlling session, memory only; cleared at the terminal state.
    session: Zeroizing<String>,
    session_hash: String,
    digest: String,
    config_sha256: Option<String>,
    replace: Option<Uuid>,
    /// `Disconnect`: `target` is the connection revision being removed.
    mode: Mode,
    target: Option<Uuid>,
    /// Connection revision observed at start; every checkpoint compares it.
    base: Option<Uuid>,
    model: String,
    effort: Option<String>,
    stage: Stage,
    after: Stage,
    /// Hand-over to the login helper of the one code the browser captured.
    code: Option<mpsc::Sender<Zeroizing<String>>>,
    code_taken: bool,
    /// Bounded command channel and latest sanitized frame of the live sign-in
    /// browser. The browser itself belongs to its supervisor task, never to
    /// this mutex; both are cleared before it shuts down.
    browser: Option<mpsc::Sender<browser::Command>>,
    frame: Option<WorkspaceFrame>,
    /// The browser failed on its own (not the owner's cancel).
    faulted: bool,
    /// The browser may still exist (shutdown unacknowledged, or a crash left
    /// it). Blocks every terminal state and the release of the setup slot.
    browser_unresolved: bool,
    cancel: CancellationToken,
    committing: bool,
    revoked: bool,
    retiring: bool,
    expires: Instant,
    /// Held only while teardown has not been acknowledged.
    prepared: Option<host::Prepared>,
}
enum CodeStep {
    Sent,
    Replay,
    NotReady,
    Unavailable,
}
impl Job {
    #[allow(clippy::too_many_arguments)]
    fn new(
        id: Uuid,
        a: &SessionAllocation,
        session: &str,
        digest: String,
        replace: Option<Uuid>,
        base: Option<Uuid>,
        model: String,
        effort: Option<String>,
        code: mpsc::Sender<Zeroizing<String>>,
        cancel: CancellationToken,
    ) -> Self {
        Self {
            id,
            allocation: a.allocation.clone(),
            epoch: a.epoch,
            session: Zeroizing::new(session.to_owned()),
            session_hash: hash(session),
            digest,
            config_sha256: None,
            replace,
            mode: Mode::Connect,
            target: None,
            base,
            model,
            effort,
            stage: Stage::Preparing,
            after: Stage::Failed,
            code: Some(code),
            code_taken: false,
            browser: None,
            frame: None,
            faulted: false,
            browser_unresolved: false,
            cancel,
            committing: false,
            revoked: false,
            retiring: false,
            expires: Instant::now() + RETENTION,
            prepared: None,
        }
    }
    /// A job that only cleans up an orphan receipt (no live login, no host
    /// handle): it occupies the single setup slot until the host answers.
    fn orphan(r: &Receipt, a: &SessionAllocation, session: &str) -> Self {
        let mut job = Self::new(
            r.request_id,
            a,
            session,
            r.digest.clone(),
            r.replace_revision,
            None,
            r.model.clone(),
            r.effort.clone(),
            mpsc::channel(1).0,
            CancellationToken::new(),
        );
        job.code = None;
        job.config_sha256 = r.config_sha256.clone();
        job.stage = Stage::CleanupPending;
        job.after = Stage::Cancelled;
        job.retiring = true;
        // A disconnect keeps its own mode and the OLD connection revision it
        // targets; the operation id is never reused as that target.
        job.mode = r.mode;
        job.target = r.target_revision;
        if r.mode == Mode::Disconnect {
            job.after = Stage::Disconnected;
        }
        job
    }
    fn same_admin(&self, a: &SessionAllocation) -> bool {
        self.allocation == a.allocation
    }
    /// Exact initiating session: same allocation (incl. generation), epoch and
    /// session token. Another admin, session or epoch has no control or URL.
    fn owned_by(&self, a: &SessionAllocation, session: &str) -> bool {
        self.same_admin(a) && self.epoch == a.epoch && self.session_hash == hash(session)
    }
    fn session(&self) -> SessionAllocation {
        SessionAllocation {
            allocation: self.allocation.clone(),
            epoch: self.epoch,
        }
    }
    fn view(&self, owner: bool) -> Value {
        let mut v = json!({"request_id":self.id,"state":self.stage,
            "model":self.model,"effort":self.effort});
        // Never a URL or code: only the exact initiating session learns that
        // a live browser frame exists.
        if owner
            && self.stage == Stage::AwaitingCode
            && self.browser.is_some()
            && !self.cancel.is_cancelled()
        {
            v["browser_available"] = json!(true);
        }
        if self.stage == Stage::Ready {
            v["connection_revision"] = json!(self.id);
        }
        if self.mode == Mode::Disconnect {
            v["kind"] = json!("disconnect");
        }
        v
    }
    /// The browser's captured code is handed to the login helper exactly once;
    /// any repeat only reads the current state. Internal only: no HTTP
    /// operation reaches it.
    fn accept_code(&mut self, code: Zeroizing<String>) -> CodeStep {
        match self.stage {
            Stage::Preparing => CodeStep::NotReady,
            Stage::AwaitingCode if !self.code_taken => {
                match self.code.as_ref().map(|tx| tx.try_send(code)) {
                    Some(Ok(())) => {
                        self.code_taken = true;
                        self.stage = Stage::Verifying;
                        CodeStep::Sent
                    }
                    _ => CodeStep::Unavailable,
                }
            }
            _ => CodeStep::Replay,
        }
    }
    fn snapshot(&self) -> Receipt {
        let (account_id, tenant_id, volume_id) = match owner_of(&self.allocation) {
            Some(o) => (o.account_id, o.tenant_id, o.volume_id),
            None => Default::default(),
        };
        Receipt {
            version: self.mode.version(),
            request_id: self.id,
            account_id,
            tenant_id,
            volume_id,
            generation: self.allocation.generation,
            digest: self.digest.clone(),
            config_sha256: self.config_sha256.clone(),
            replace_revision: self.replace,
            model: self.model.clone(),
            effort: self.effort.clone(),
            state: self.stage,
            updated: now(),
            mode: self.mode,
            target_revision: self.target,
            browser_pending: self.browser_unresolved,
        }
    }
}
fn prune(jobs: &mut Vec<Job>) {
    let now = Instant::now();
    jobs.retain(|j| !j.stage.terminal() || j.expires > now);
}
fn with_job<T>(g: &Gateway, id: Uuid, f: impl FnOnce(&mut Job) -> T) -> Option<T> {
    let mut jobs = g.claude_jobs.lock().ok()?;
    jobs.iter_mut().find(|j| j.id == id).map(f)
}
fn lock(g: &Gateway) -> Result<std::sync::MutexGuard<'_, Vec<Job>>, StatusCode> {
    g.claude_jobs.lock().map_err(|_| UNAVAILABLE)
}
/// A pending or unreadable receipt reserves the setup slot across every
/// connection mode. No API-key save may bypass unfinished subscription cleanup.
pub(super) fn ensure_settled(d: &File) -> io::Result<()> {
    if receipt::read_receipt(d)?.is_some_and(|r| !r.state.terminal()) {
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    Ok(())
}
pub(super) fn unfinished(d: &File) -> bool {
    ensure_settled(d).is_err()
}
fn wrap(operation: Value) -> Value {
    json!({"supported":true,"operation":operation})
}

/// Reauthenticate a session token against the private member service and the
/// installation registry: current route, role, epoch, generation and owner.
async fn admin_for(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
) -> Result<control::AdminSession, StatusCode> {
    recheck(g, a).await?;
    let (status, body, _) = timeout(
        Duration::from_secs(5),
        g.proxy(a, Method::GET, "/api/v1/auth/session", None, Some(session)),
    )
    .await
    .map_err(|_| UNAVAILABLE)??;
    if status != StatusCode::OK {
        return Err(status);
    }
    let view = member_session_view(&body, a)?;
    let root = g.registry.clone();
    let (allocation, epoch) = (a.allocation.clone(), a.epoch);
    tokio::task::spawn_blocking(move || {
        control::checked_session(&root, &allocation, &view)
            .and_then(|p| p.at_epoch(epoch))
            .map_err(admin::error)
    })
    .await
    .map_err(|_| UNAVAILABLE)?
}
/// Non-secret receipt IO. It needs no admin pin, so a revoked admin's job can
/// still record its terminal state.
async fn receipts_io<T: Send + 'static>(
    g: &Gateway,
    action: impl FnOnce(&File) -> io::Result<T> + Send + 'static,
) -> io::Result<T> {
    let dir = g
        .model_setup_dir
        .clone()
        .ok_or_else(|| io::Error::from(io::ErrorKind::NotFound))?;
    tokio::task::spawn_blocking(move || {
        let _guard = receipt::LOCK
            .lock()
            .map_err(|_| io::Error::from(io::ErrorKind::Other))?;
        model_dir(&dir).and_then(|d| action(&d))
    })
    .await
    .map_err(|_| io::Error::from(io::ErrorKind::Other))?
}
async fn receipts<T: Send + 'static>(
    g: &Gateway,
    action: impl FnOnce(&File) -> io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    receipts_io(g, action).await.map_err(|_| UNAVAILABLE)
}
struct Stored {
    current: Result<Option<Uuid>, ()>,
    receipt: Option<Receipt>,
    relation: Relation,
}
/// Read-only reconciliation. A published config alone cannot prove the root
/// worker mark completed; only the job's authenticated recovery may finish it.
async fn stored(g: &Gateway) -> Result<Stored, StatusCode> {
    receipts(g, move |d| {
        let receipt = receipt::read_receipt(d)?;
        let seen = receipt::inspect(d, receipt.as_ref());
        Ok(Stored {
            current: seen.current,
            receipt,
            relation: seen.relation,
        })
    })
    .await
}
async fn persist(g: &Gateway, id: Uuid) -> bool {
    let current = g.clone();
    receipts(g, move |d| {
        // Snapshot while holding the receipt lock. A terminal disk receipt may
        // precede finish's in-memory update; it must never regress in that gap.
        let Some(mut r) = with_job(&current, id, |j| j.snapshot()) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        if let Some(old) = receipt::read_receipt(d)? {
            if old.request_id != id {
                return Err(io::ErrorKind::AlreadyExists.into());
            }
            if old.state.terminal() {
                return Ok(());
            }
            // Registry-locked disconnect intent may reach disk before the
            // drive task mirrors it in memory. Preserve that exact digest.
            if old.mode == Mode::Disconnect {
                if r.mode != old.mode || r.target_revision != old.target_revision {
                    return Err(io::ErrorKind::InvalidData.into());
                }
                if let Some(sha) = old.config_sha256 {
                    if r.config_sha256.as_ref().is_some_and(|next| next != &sha) {
                        return Err(io::ErrorKind::InvalidData.into());
                    }
                    r.config_sha256 = Some(sha);
                }
            }
        }
        if r.state.terminal() {
            return Ok(());
        }
        receipt::write_receipt(d, &r)
    })
    .await
    .is_ok()
}
async fn advance(g: &Gateway, id: Uuid, stage: Stage) {
    with_job(g, id, |j| {
        if !j.stage.terminal() && j.stage != Stage::CleanupPending {
            j.stage = stage;
        }
    });
    persist(g, id).await;
}
async fn finish(g: &Gateway, id: Uuid, stage: Option<Stage>) {
    // No terminal state, and so no released slot, while a sign-in browser may
    // still exist. Callers hold first; this is the last line of defense.
    let unresolved = with_job(g, id, |j| {
        let unresolved = j.browser_unresolved;
        if unresolved {
            j.stage = Stage::CleanupPending;
            j.retiring = false;
        }
        unresolved
    });
    if unresolved != Some(false) {
        persist(g, id).await;
        return;
    }
    let Some(r) = with_job(g, id, |j| {
        let mut r = j.snapshot();
        r.state = stage.unwrap_or(j.after);
        r.updated = now();
        r
    }) else {
        return;
    };
    let mut durable = false;
    for attempt in 0..3 {
        let next = r.clone();
        if receipts_io(g, move |d| {
            // receipts_io already holds the receipt lock.
            if next.mode == Mode::Disconnect && next.state == Stage::Failed {
                let old = receipt::read_receipt(d)?.ok_or(io::ErrorKind::NotFound)?;
                if old.request_id != next.request_id || old.config_sha256.is_some() {
                    return Err(io::ErrorKind::InvalidData.into());
                }
            }
            receipt::write_receipt(d, &next)
        })
        .await
        .is_ok()
        {
            durable = true;
            break;
        }
        if attempt < 2 {
            sleep(Duration::from_millis(100)).await;
        }
    }
    with_job(g, id, |j| {
        j.after = r.state;
        j.stage = if durable {
            r.state
        } else {
            Stage::CleanupPending
        };
        j.browser = None;
        j.frame = None;
        j.code = None;
        j.prepared = None;
        j.retiring = false;
        if durable {
            j.session = Zeroizing::new(String::new());
        }
        j.expires = Instant::now() + RETENTION;
    });
    if !durable {
        // Merge the durable intent just like every other delayed writer; a
        // failed finalizer must not erase an acknowledged cleanup obligation.
        persist(g, id).await;
    }
}

/// Current admin (original session) and the connection revision seen at start.
async fn check(g: &Gateway, id: Uuid, cancel: &CancellationToken) -> Result<(), Stop> {
    if cancel.is_cancelled() {
        return Err(Stop::Cancelled);
    }
    let (a, session, base) = with_job(g, id, |j| (j.session(), j.session.clone(), j.base))
        .ok_or_else(|| failed("check_job_absent"))?;
    let admin = admin_for(g, &a, &session)
        .await
        .map_err(|_| failed("check_authority"))?;
    if local_with(g, admin, receipt::current_revision)
        .await
        .map_err(|_| failed("check_revision_read"))?
        != base
    {
        return Err(failed("check_revision_changed"));
    }
    if cancel.is_cancelled() {
        return Err(Stop::Cancelled);
    }
    Ok(())
}
struct Deadline(CancellationToken, tokio::task::JoinHandle<()>);
/// Cancels the child token after `limit`. The host call it guards is still
/// awaited to completion; the token only asks it to stop.
fn deadline(parent: &CancellationToken, limit: Duration) -> Deadline {
    let token = parent.child_token();
    let timer = token.clone();
    Deadline(
        token,
        tokio::spawn(async move {
            sleep(limit).await;
            timer.cancel();
        }),
    )
}
impl Drop for Deadline {
    fn drop(&mut self) {
        self.1.abort();
    }
}

enum Msg {
    Line(Zeroizing<Vec<u8>>),
    End,
    Bad,
}
async fn read_lines(out: ChildStdout, tx: mpsc::Sender<Msg>) {
    let mut reader = BufReader::new(out);
    for _ in 0..protocol::MAX_LINES {
        let mut line = Zeroizing::new(Vec::new());
        let read = (&mut reader)
            .take(protocol::MAX_LINE as u64 + 1)
            .read_until(b'\n', &mut line)
            .await;
        let msg = match read {
            Ok(0) => Msg::End,
            Ok(_) => {
                if line.pop() == Some(b'\n') && line.len() <= protocol::MAX_LINE {
                    Msg::Line(line)
                } else {
                    Msg::Bad
                }
            }
            Err(_) => Msg::Bad,
        };
        let last = !matches!(msg, Msg::Line(_));
        if tx.send(msg).await.is_err() || last {
            return;
        }
    }
    let _ = tx.send(Msg::Bad).await;
}

/// A terminal CLI-side failure. Only a static code is logged, never CLI output,
/// URLs, codes or identifiers; the control decision is the returned `Stop` alone.
fn diagnostic(code: &'static str) {
    use std::io::Write as _;
    let _ = writeln!(std::io::stderr().lock(), "moosuri: claude_login_{code}");
}
fn failed(code: &'static str) -> Stop {
    diagnostic(code);
    Stop::Failed
}

/// The helper's stdin stays open until its result and exit. Ready needs a
/// ready result, no trailing event, output EOF and OS exit status 0. The
/// authorization URL is handed only to the sealed browser; its supervisor
/// returns the captured code through the job's one-shot hand-over.
#[allow(clippy::too_many_arguments)]
async fn converse(
    g: &Gateway,
    id: Uuid,
    child: &mut Child,
    stdin: &mut ChildStdin,
    rx: &mut mpsc::Receiver<Msg>,
    code_rx: &mut mpsc::Receiver<Zeroizing<String>>,
    cancel: &CancellationToken,
    pump: &mut Option<browser::Pump>,
) -> Result<(), Stop> {
    let mut seq = Sequence::default();
    let (mut exit, mut end, mut published, mut sent) = (None, false, false, false);
    let limit = sleep(LOGIN_LIMIT);
    tokio::pin!(limit);
    loop {
        if let (Some(outcome), true, Some(status)) = (seq.finished(), end, exit) {
            let status: std::process::ExitStatus = status;
            return if outcome == Outcome::Ready && status.success() && published && sent {
                Ok(())
            } else if outcome != Outcome::Ready {
                Err(failed("result_not_ready"))
            } else if !status.success() {
                Err(failed("exit_status"))
            } else {
                Err(failed("incomplete"))
            };
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(Stop::Cancelled),
            _ = &mut limit => return Err(failed(if seq.finished().is_some() { "exit_grace" } else { "deadline" })),
            msg = rx.recv(), if !end => match msg {
                Some(Msg::Line(line)) => match seq.accept(&line) {
                    Ok(Step::Url(url)) => {
                        check(g, id, cancel).await?;
                        if !protocol::browser_ready(&url) {
                            return Err(failed("url_rejected"));
                        }
                        // Ready once the browser's first frame exists; the
                        // supervisor then sets `awaiting_code`.
                        *pump = Some(browser::launch(g, id, url, cancel).await?);
                        published = true;
                        persist(g, id).await;
                    }
                    Ok(Step::Result(Outcome::Ready)) => {
                        if !published || !sent { return Err(failed("result_early")); }
                        limit.as_mut().reset(tokio::time::Instant::now() + EXIT_GRACE);
                    }
                    Ok(Step::Result(Outcome::Cancelled)) => return Err(Stop::Cancelled),
                    Ok(Step::Result(_)) => return Err(failed("result_failed")),
                    Err(()) => return Err(failed("protocol_invalid")),
                },
                Some(Msg::End) => {
                    end = true;
                    if seq.finished().is_none() {
                        return Err(failed("stream_ended"));
                    }
                }
                Some(Msg::Bad) | None => return Err(failed("stream_failed")),
            },
            code = code_rx.recv(), if published && !sent && seq.finished().is_none() => {
                let Some(code) = code else { return Err(failed("handoff_closed")) };
                check(g, id, cancel).await?;
                let mut input = Zeroizing::new(Vec::with_capacity(code.len() + 1));
                input.extend_from_slice(code.as_bytes());
                input.push(b'\n');
                drop(code);
                timeout(Duration::from_secs(5), async {
                    stdin.write_all(&input).await?;
                    stdin.flush().await
                })
                .await
                .map_err(|_| failed("code_write_timeout"))?
                .map_err(|_| failed("code_write_failed"))?;
                sent = true;
            }
            status = child.wait(), if exit.is_none() => match status {
                Ok(status) => exit = Some(status),
                Err(_) => return Err(failed("wait_failed")),
            },
        }
    }
}
async fn login(
    g: &Gateway,
    id: Uuid,
    prepared: &host::Prepared,
    code_rx: &mut mpsc::Receiver<Zeroizing<String>>,
    cancel: &CancellationToken,
) -> Result<(), Stop> {
    let mut command = host::login_command(prepared).map_err(|_| failed("command_invalid"))?;
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| failed("spawn_failed"))?;
    let (Some(mut stdin), Some(stdout)) = (child.stdin.take(), child.stdout.take()) else {
        let _ = child.start_kill();
        let _ = timeout(EXIT_GRACE, child.wait()).await;
        return Err(failed("stdio_missing"));
    };
    let (tx, mut rx) = mpsc::channel(4);
    let reader = tokio::spawn(read_lines(stdout, tx));
    let mut pump = None;
    // The independent timer also bounds browser startup and the shared admission lock.
    let login_deadline = deadline(cancel, LOGIN_LIMIT);
    let mut result = converse(
        g,
        id,
        &mut child,
        &mut stdin,
        &mut rx,
        code_rx,
        &login_deadline.0,
        &mut pump,
    )
    .await;
    if login_deadline.0.is_cancelled() && !cancel.is_cancelled() {
        // The browser supervisor also cancels this child token on a fault.
        // It sets faulted before cancellation, so do not label that as a timer.
        if matches!(&result, Err(Stop::Cancelled)) && with_job(g, id, |j| j.faulted) == Some(false)
        {
            diagnostic("deadline");
        }
        result = Err(Stop::Failed);
    }
    if result.is_err() {
        // Stop frame access and dispatch before waiting for any child to be reaped.
        cancel.cancel();
        with_job(g, id, |j| {
            j.browser = None;
            j.frame = None;
        });
        let _ = child.start_kill();
    }
    drop(stdin);
    // A helper that does not leave on its own is killed and reaped, never
    // abandoned: the login process must not outlive this setup.
    if timeout(EXIT_GRACE, child.wait()).await.is_err() {
        let _ = child.start_kill();
        let _ = timeout(EXIT_GRACE, child.wait()).await;
    }
    reader.abort();
    // The sign-in browser is shut down with an acknowledgement before this
    // login can end; an unconfirmed shutdown is recorded on the job.
    if let Some(pump) = pump.take() {
        pump.finish(g, id).await;
    }
    result
}

/// What the host holds for a running job. `request` is set from the moment the
/// host may have recorded the request (so even a failed or cancelled prepare
/// is retired by request id); `prepared` while a handle is held. Both are
/// cleared only after the root mark succeeded.
#[derive(Default)]
struct Held {
    prepared: Option<host::Prepared>,
    request: bool,
}
type Slot = Arc<std::sync::Mutex<Option<host::Prepared>>>;
/// Everything up to and including publication and the root mark.
async fn steps(
    g: &Gateway,
    id: Uuid,
    request: host::Request,
    peer: u32,
    code_rx: &mut mpsc::Receiver<Zeroizing<String>>,
    cancel: &CancellationToken,
    held: &mut Held,
) -> Result<(), Stop> {
    let (owner, request_id) = (request.owner, request.request_id);
    let (model, effort) = (request.model.clone(), request.effort.clone());
    let base = with_job(g, id, |j| j.base).ok_or(Stop::Failed)?;
    check(g, id, cancel).await?;
    held.request = true;
    let made = {
        let limit = deadline(cancel, PREPARE_LIMIT);
        host::prepare(request, &limit.0).await
    };
    let prepared = match made {
        Ok(p) => &*held.prepared.insert(p),
        Err(_) if cancel.is_cancelled() => return Err(Stop::Cancelled),
        Err(reason) => {
            // Fixed diagnostic vocabulary only; no child output or credentials.
            let code = match reason.as_str() {
                "claude_runtime_useradd_failed" => "identity_create_failed",
                "claude_connection_identity_command_invalid" => "identity_command_invalid",
                "claude_runtime_identity_missing" => "identity_missing",
                "claude_runtime_foreign_account" => "identity_mismatch",
                "claude_runtime_identity_out_of_range" => "identity_range",
                "kiri_install_command_timeout" => "host_timeout",
                "kiri_install_command_cancelled" => "host_cancelled",
                "kiri_install_foreign_or_changed_file" => "host_file_conflict",
                _ => "host_prepare_failed",
            };
            {
                use std::io::Write as _;
                let _ = writeln!(
                    std::io::stderr().lock(),
                    "moosuri: claude_subscription_prepare_{code}"
                );
            }
            return Err(Stop::Failed);
        }
    };
    if prepared.request.owner != owner
        || prepared.request.request_id != request_id
        || prepared.endpoint.validate().is_err()
    {
        return Err(Stop::Failed);
    }
    check(g, id, cancel).await?;
    login(g, id, prepared, code_rx, cancel).await?;
    // The browser's acknowledged shutdown precedes activation.
    if with_job(g, id, |j| j.browser_unresolved) != Some(false) {
        return Err(Stop::Failed);
    }
    advance(g, id, Stage::Activating).await;
    check(g, id, cancel).await?;
    let activated = {
        let limit = deadline(cancel, ACTIVATE_LIMIT);
        host::activate(prepared, &limit.0).await
    };
    match activated {
        Ok(()) => (),
        Err(_) if cancel.is_cancelled() => return Err(Stop::Cancelled),
        Err(_) => return Err(Stop::Failed),
    }
    // Fresh authentication of the ORIGINAL session right before publication.
    let (a, session) = with_job(g, id, |j| (j.session(), j.session.clone())).ok_or(Stop::Failed)?;
    let admin = admin_for(g, &a, &session).await.map_err(|_| Stop::Failed)?;
    with_job(g, id, |j| {
        if cancel.is_cancelled() {
            Err(Stop::Cancelled)
        } else {
            j.committing = true;
            Ok(())
        }
    })
    .ok_or(Stop::Failed)??;
    let bytes = receipt::config_bytes(&receipt::Publication {
        request: request_id,
        owner: &owner,
        peer_uid: peer,
        endpoint: &prepared.endpoint,
        model: &model,
        effort: effort.as_deref(),
    })
    .ok_or(Stop::Failed)?;
    // The config commit and the root mark share one registry-locked section.
    // Persist exact publication intent first: owner/model equality alone does
    // not establish that the worker endpoint, provider and budget are ours.
    with_job(g, id, |j| j.config_sha256 = Some(receipt::sha(&bytes)));
    if !persist(g, id).await {
        return Err(Stop::Failed);
    }
    // The handle travels through a slot because the blocking closure must own
    // it; it comes back to `held` whatever the outcome.
    let slot: Slot = Arc::new(std::sync::Mutex::new(held.prepared.take()));
    let inner = slot.clone();
    let result = local_with(g, admin, move |d| {
        let settled = receipt::settle(d, &bytes, base);
        if settled != Settled::Exact {
            return Ok(settled);
        }
        let guard = inner.lock().map_err(|_| io::Error::other("held"))?;
        let Some(p) = guard.as_ref() else {
            return Ok(Settled::RootPending);
        };
        Ok(match host::published(p) {
            Ok(()) => Settled::Marked,
            Err(_) => Settled::RootPending,
        })
    })
    .await;
    held.prepared = slot.lock().ok().and_then(|mut s| s.take());
    match result {
        Ok(Settled::Marked) => {
            *held = Held::default();
            Ok(())
        }
        // Every other outcome keeps the host state held: cleanup re-reads the
        // actual config and either retries the root mark (exact config is
        // published, so nothing is retired) or retires (definitely not ours).
        // An unreadable config never counts as absent.
        _ => Err(Stop::Failed),
    }
}
async fn run(
    g: Gateway,
    id: Uuid,
    request: host::Request,
    peer: u32,
    mut code_rx: mpsc::Receiver<Zeroizing<String>>,
    cancel: CancellationToken,
) {
    let mut held = Held::default();
    let stage = match steps(&g, id, request, peer, &mut code_rx, &cancel, &mut held).await {
        Ok(()) => Stage::Ready,
        Err(Stop::Failed) => Stage::Failed,
        Err(Stop::Cancelled) => {
            // A browser fault cancels the login itself: that is a failure.
            if with_job(&g, id, |j| j.revoked || j.faulted).unwrap_or(true) {
                Stage::Failed
            } else {
                Stage::Cancelled
            }
        }
    };
    if stage == Stage::Failed {
        let cause = with_job(&g, id, |j| {
            if j.revoked {
                "terminal_revoked"
            } else if j.faulted {
                "terminal_browser_fault"
            } else {
                "terminal_failed"
            }
        })
        .unwrap_or("terminal_job_absent");
        diagnostic(cause);
    } else if stage == Stage::Cancelled {
        diagnostic("terminal_cancelled");
    }
    // The browser is gone (acknowledged) or the job holds. It never reaches
    // cancelled, failed-and-released or ready with an unconfirmed browser;
    // the host state stays held for the same retry (`cancel`) as any cleanup.
    if with_job(&g, id, |j| j.browser_unresolved) != Some(false) {
        let prepared = held.prepared;
        with_job(&g, id, move |j| {
            j.after = stage;
            j.stage = Stage::CleanupPending;
            j.retiring = false;
            j.prepared = prepared;
        });
        persist(&g, id).await;
        return;
    }
    if held.prepared.is_none() && !held.request {
        finish(&g, id, Some(stage)).await;
        return;
    }
    with_job(&g, id, |j| {
        j.after = stage;
        j.stage = Stage::CleanupPending;
        j.browser = None;
        j.frame = None;
        j.retiring = true;
    });
    persist(&g, id).await;
    cleanup(g, id, held.prepared).await;
}

enum Disposition {
    /// The exact config for this request is published: never retire.
    Published,
    /// The config definitely does not name this request.
    Retire,
    /// Unreadable or mismatching: no destructive step, keep pending.
    Hold,
}
async fn disposition(g: &Gateway, id: Uuid) -> Disposition {
    let Some(r) = with_job(g, id, |j| j.snapshot()) else {
        return Disposition::Hold;
    };
    match receipts(g, move |d| Ok(receipt::inspect(d, Some(&r)).relation)).await {
        Ok(Relation::Exact) => Disposition::Published,
        Ok(rel) if rel.may_retire() => Disposition::Retire,
        _ => Disposition::Hold,
    }
}
/// Retry publication under a fresh admin pin. Recheck the exact config and
/// make it durable before marking root state, including after gateway restart.
async fn mark(g: &Gateway, id: Uuid, held: &mut Option<host::Prepared>) -> bool {
    let Some((a, session, r)) = with_job(g, id, |j| (j.session(), j.session.clone(), j.snapshot()))
    else {
        return false;
    };
    let Some(owner) = owner_of(&a.allocation) else {
        return false;
    };
    let Ok(admin) = admin_for(g, &a, &session).await else {
        return false;
    };
    let slot: Slot = Arc::new(std::sync::Mutex::new(held.take()));
    let inner = slot.clone();
    let ok = local_with(g, admin, move |d| {
        let bytes = read(d, CONFIG_FILE)?.ok_or_else(|| io::Error::other("config"))?;
        if receipt::classify(Some(&bytes), &r) != Relation::Exact {
            return Err(io::Error::other("config"));
        }
        receipt::confirm_durable(d, &bytes)?;
        let guard = inner.lock().map_err(|_| io::Error::other("held"))?;
        match guard.as_ref() {
            Some(p) => host::published(p),
            None => host::published_request(owner, id),
        }
        .map_err(|_| io::Error::other("root"))
    })
    .await
    .is_ok();
    *held = slot.lock().ok().and_then(|mut s| s.take());
    ok
}
async fn retire(owner: Option<Owner>, id: Uuid, held: Option<&host::Prepared>) -> bool {
    match (held, owner) {
        (Some(p), _) => host::retire(p).await.is_ok(),
        (None, Some(owner)) => host::retire_request(owner, id).await.is_ok(),
        (None, None) => false,
    }
}
/// A successor is released only after the host acknowledges retirement or the
/// exact publication is confirmed. Host calls are awaited to completion. A
/// failed or unknown result stays `cleanup_pending`; an explicit cancel by an
/// authenticated current session of the exact owner retries it.
async fn cleanup(g: Gateway, id: Uuid, mut prepared: Option<host::Prepared>) {
    // Host state is touched only once no sign-in browser can still exist.
    if with_job(&g, id, |j| j.browser_unresolved) != Some(false) {
        if browser::inventory_confirmed(&g).await {
            with_job(&g, id, |j| j.browser_unresolved = false);
        } else {
            with_job(&g, id, |j| {
                j.prepared = prepared;
                j.retiring = false;
            });
            return;
        }
    }
    let owner = with_job(&g, id, |j| owner_of(&j.allocation)).flatten();
    for attempt in 1..=3u64 {
        let done = match disposition(&g, id).await {
            Disposition::Published => mark(&g, id, &mut prepared)
                .await
                .then_some(Some(Stage::Ready)),
            Disposition::Retire => retire(owner, id, prepared.as_ref()).await.then_some(None),
            Disposition::Hold => None,
        };
        if let Some(stage) = done {
            finish(&g, id, stage).await;
            return;
        }
        sleep(Duration::from_secs(2 * attempt)).await;
    }
    with_job(&g, id, |j| {
        j.prepared = prepared;
        j.retiring = false;
    });
}
/// Fail-closed supervision of the original session. A hidden or throttled page
/// never cancels the login: it is bounded by `LOGIN_LIMIT` and the host limits.
async fn watch(g: Gateway, id: Uuid, cancel: CancellationToken) {
    enum Watch {
        Done,
        Skip,
        Check(SessionAllocation, Zeroizing<String>),
    }
    let mut tick = interval(AUTH_INTERVAL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    tick.tick().await;
    let mut transient = 0u32;
    loop {
        tick.tick().await;
        let watch = with_job(&g, id, |j| {
            if j.stage.terminal() || j.stage == Stage::CleanupPending {
                Watch::Done
            } else if j.committing {
                Watch::Skip
            } else {
                Watch::Check(j.session(), j.session.clone())
            }
        })
        .unwrap_or(Watch::Done);
        let (a, session) = match watch {
            Watch::Done => return,
            Watch::Skip => continue,
            Watch::Check(a, session) => (a, session),
        };
        let revoke = match admin_for(&g, &a, &session).await {
            Ok(_) => {
                transient = 0;
                false
            }
            Err(UNAVAILABLE) => {
                transient += 1;
                transient >= 5
            }
            Err(_) => true,
        };
        if revoke {
            let first = with_job(&g, id, |j| {
                let first = !j.revoked;
                j.revoked = true;
                first
            }) == Some(true);
            if first {
                diagnostic(if transient >= 5 {
                    "watch_unavailable"
                } else {
                    "watch_revoked"
                });
            }
            cancel.cancel();
        }
    }
}

pub(super) async fn handle(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        g.headers(&headers, &method)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let (a, session) = authenticated_allocation(&g, &headers, true).await?;
        let _admin = admin_for(&g, &a, &session).await?;
        if !g.model_setup {
            return Err(StatusCode::NOT_FOUND);
        }
        let _slot = g.inflight.try_acquire().map_err(|_| UNAVAILABLE)?;
        if method == Method::GET && body.is_empty() {
            return list(&g, &a, &session).await;
        }
        if method != Method::POST
            || one(&headers, "content-type").and_then(|s| s.split(';').next())
                != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let input: Input = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        if input.request_id.is_nil() || !input.well_formed() {
            return Err(StatusCode::BAD_REQUEST);
        }
        match input.op {
            Op::Start => start(&g, &a, &session, input).await,
            Op::Status => status(&g, &a, &session, input.request_id).await,
            Op::Frame => browser::frame(&g, &a, &session, input.request_id),
            Op::Input => {
                let (Some(token), Some(typed)) = (input.frame_token, input.input) else {
                    return Err(StatusCode::BAD_REQUEST);
                };
                browser::input(&g, &a, &session, input.request_id, token, typed).await
            }
            Op::Cancel => cancel(&g, &a, &session, input.request_id).await,
            Op::Disconnect => lifecycle::disconnect(&g, &a, &session, input).await,
        }
    }
    .await;
    match result {
        Ok(v) => response(StatusCode::OK, v.to_string().into_bytes()),
        Err(status) => failure(status),
    }
}
/// Reload view: the live job (URL only to its exact session) or this owner's
/// own receipt, reconciled with the published config. Another owner's
/// operation is never visible.
async fn list(g: &Gateway, a: &SessionAllocation, session: &str) -> Result<Value, StatusCode> {
    let live = {
        let mut jobs = lock(g)?;
        prune(&mut jobs);
        jobs.iter_mut().rev().find(|j| j.same_admin(a)).map(|j| {
            let own = j.owned_by(a, session);
            j.view(own)
        })
    };
    let Stored {
        current,
        receipt,
        relation,
    } = stored(g).await?;
    let operation = live.or_else(|| {
        receipt
            .filter(|r| r.owner_is(&a.allocation))
            .map(|r| r.view(relation))
    });
    let recoverable = operation.as_ref().is_some_and(|v| {
        matches!(
            v["state"].as_str(),
            Some(
                "preparing"
                    | "awaiting_code"
                    | "verifying"
                    | "activating"
                    | "interrupted"
                    | "cleanup_pending"
                    | "disconnecting"
            )
        )
    });
    // A damaged config must not hide the owner's pending recovery operation.
    // This is a read-only view; every mutation still rechecks the exact config.
    let current = match current {
        Ok(current) => current,
        Err(_) if recoverable => None,
        Err(_) => return Err(UNAVAILABLE),
    };
    let replacement = if let (Some(owner), Some(target)) = (owner_of(&a.allocation), current) {
        receipts(g, move |d| {
            Ok(read(d, CONFIG_FILE)?
                .as_deref()
                .is_some_and(|bytes| {
                    lifecycle::management(bytes, &owner, target)
                        == Some(lifecycle::Management::External)
                })
                .then_some(target))
        })
        .await?
    } else {
        None
    };
    Ok(json!({"supported":true,"saved":current.is_some(),
        "connection_revision":current,"replacement_revision":replacement,"operation":operation}))
}
async fn start(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    input: Input,
) -> Result<Value, StatusCode> {
    let (id, replace) = (input.request_id, input.replace_revision);
    let (Some(email), Some(model)) = (input.email, input.model) else {
        return Err(StatusCode::BAD_REQUEST);
    };
    if input.hosting_terms_accepted != Some(true)
        || !valid_email(&email)
        || !valid_model(&model)
        || !input.effort.as_deref().is_none_or(valid_effort)
        || replace.is_some_and(|r| r.is_nil() || r == id)
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    let digest = digest_of(
        &a.allocation,
        &email,
        &model,
        input.effort.as_deref(),
        replace,
    );
    {
        let mut jobs = lock(g)?;
        prune(&mut jobs);
        if let Some(j) = jobs.iter().find(|j| j.id == id) {
            if !j.owned_by(a, session) || j.digest != digest {
                return Err(StatusCode::CONFLICT);
            }
            return Ok(wrap(j.view(true)));
        }
        // One setup at a time: it owns the single config, receipt and host slot.
        if jobs.iter().any(|j| !j.stage.terminal()) {
            return Err(StatusCode::CONFLICT);
        }
    }
    let seen = stored(g).await?;
    let current = seen.current.map_err(|_| UNAVAILABLE)?;
    if let Some(r) = seen.receipt.as_ref() {
        // A lost reply after a restart: the same exact request only reads its receipt.
        if r.request_id == id {
            if r.owner_is(&a.allocation) && r.digest == digest {
                return Ok(wrap(r.view(seen.relation)));
            }
            return Err(StatusCode::CONFLICT);
        }
        // An unresolved receipt (interrupted, cleanup pending) is never
        // overwritten: it must be cleaned up explicitly first.
        if r.unresolved(seen.relation) {
            return Err(StatusCode::CONFLICT);
        }
    }
    // An existing connection is replaced only by naming its current revision.
    match (current, replace) {
        (Some(c), Some(r)) if c == r => (),
        (None, None) => (),
        _ => return Err(StatusCode::CONFLICT),
    }
    // The sign-in happens only in the sealed in-app browser. An installation
    // without it has no login path and no external-link fallback.
    if g.claude_browser.is_none() {
        return Err(UNAVAILABLE);
    }
    let (_, peer) = g.socket(&a.allocation)?;
    let owner = owner_of(&a.allocation).ok_or(UNAVAILABLE)?;
    let request = host::Request {
        request_id: id,
        owner,
        email,
        model: model.clone(),
        effort: input.effort.clone(),
        hosting_terms_accepted: true,
    };
    let (tx, rx) = mpsc::channel(1);
    let cancel = CancellationToken::new();
    let job = Job::new(
        id,
        a,
        session,
        digest,
        replace,
        current,
        model,
        input.effort,
        tx,
        cancel.clone(),
    );
    let first = job.snapshot();
    let view = {
        let mut jobs = lock(g)?;
        prune(&mut jobs);
        if jobs.iter().any(|j| !j.stage.terminal()) {
            return Err(StatusCode::CONFLICT);
        }
        while jobs.len() >= MAX_JOBS {
            jobs.remove(0);
        }
        let view = job.view(true);
        jobs.push(job);
        view
    };
    if let Err(e) = receipts_io(g, move |d| receipt::begin(d, &first)).await {
        lock(g)?.retain(|j| j.id != id);
        return Err(if e.kind() == io::ErrorKind::AlreadyExists {
            StatusCode::CONFLICT
        } else {
            UNAVAILABLE
        });
    }
    tokio::spawn(run(g.clone(), id, request, peer, rx, cancel.clone()));
    tokio::spawn(watch(g.clone(), id, cancel));
    Ok(wrap(view))
}
async fn status(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
) -> Result<Value, StatusCode> {
    {
        let jobs = lock(g)?;
        if let Some(j) = jobs.iter().find(|j| j.id == id) {
            if !j.same_admin(a) {
                return Err(StatusCode::NOT_FOUND);
            }
            return Ok(wrap(j.view(j.owned_by(a, session))));
        }
    }
    let seen = stored(g).await?;
    seen.receipt
        .filter(|r| r.request_id == id && r.owner_is(&a.allocation))
        .map(|r| wrap(r.view(seen.relation)))
        .ok_or(StatusCode::NOT_FOUND)
}
/// Cancel a running login (exact initiating session only), retry the cleanup
/// of a live `cleanup_pending` job, or recover an orphan receipt left by a
/// restart. Cleanup takeover needs only an authenticated current session of the
/// exact owner (the caller was reauthenticated by `handle`).
async fn cancel(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
) -> Result<Value, StatusCode> {
    enum Next {
        Done(Value),
        Retry(Option<host::Prepared>, Value, Mode),
        Orphan,
    }
    let next = {
        let mut jobs = lock(g)?;
        match jobs.iter_mut().find(|j| j.id == id) {
            None => Next::Orphan,
            Some(j) if !j.same_admin(a) => return Err(StatusCode::NOT_FOUND),
            Some(j) if j.stage == Stage::CleanupPending => {
                j.session = Zeroizing::new(session.to_owned());
                j.session_hash = hash(session);
                j.epoch = a.epoch;
                if j.retiring {
                    Next::Done(wrap(j.view(true)))
                } else {
                    j.retiring = true;
                    let view = wrap(j.view(true));
                    Next::Retry(j.prepared.take(), view, j.mode)
                }
            }
            // A disconnect is never cancellable: only its resume (above) exists.
            Some(j) if j.mode == Mode::Disconnect => {
                Next::Done(wrap(j.view(j.owned_by(a, session))))
            }
            Some(j) if !j.owned_by(a, session) => return Err(StatusCode::NOT_FOUND),
            Some(j) => {
                // Publication is atomic: past this point cancel cannot undo it.
                if !j.stage.terminal() && !j.committing {
                    j.cancel.cancel();
                }
                Next::Done(wrap(j.view(true)))
            }
        }
    };
    match next {
        Next::Done(v) => Ok(v),
        Next::Retry(prepared, view, Mode::Connect) => {
            tokio::spawn(cleanup(g.clone(), id, prepared));
            Ok(view)
        }
        Next::Retry(_, view, Mode::Disconnect) => {
            tokio::spawn(lifecycle::drive(g.clone(), id));
            Ok(view)
        }
        Next::Orphan => orphan(g, a, session, id).await,
    }
}
/// Recovery of a receipt without a live job. The orphan job occupies the
/// single setup slot, so a start cannot interleave, and it retires the host
/// record only after the config is read and does not name the request.
async fn orphan(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
) -> Result<Value, StatusCode> {
    let seen = stored(g).await?;
    let r = seen
        .receipt
        .filter(|r| r.request_id == id && r.owner_is(&a.allocation))
        .ok_or(StatusCode::NOT_FOUND)?;
    if !r.unresolved(seen.relation) {
        return Ok(wrap(r.view(seen.relation)));
    }
    // A disconnect receipt is judged by `lifecycle::drive` itself (target
    // config rechecked before every transition), not by the connect relation.
    let disconnect = r.mode == Mode::Disconnect;
    if !disconnect && !seen.relation.may_retire() && seen.relation != Relation::Exact {
        // Unreadable or mismatching config: nothing destructive is safe.
        return Ok(wrap(r.view_as(Stage::CleanupPending)));
    }
    let mut job = Job::orphan(&r, a, session);
    // A crash may have left a sign-in container. The exact inventory must be
    // confirmed empty before the host record is retired or the slot released.
    job.browser_unresolved = !disconnect && r.browser_pending;
    let view = job.view(true);
    {
        let mut jobs = lock(g)?;
        prune(&mut jobs);
        if jobs.iter().any(|j| !j.stage.terminal()) {
            return Err(StatusCode::CONFLICT);
        }
        while jobs.len() >= MAX_JOBS {
            jobs.remove(0);
        }
        jobs.push(job);
    }
    if !persist(g, id).await {
        lock(g)?.retain(|j| j.id != id);
        return Err(UNAVAILABLE);
    }
    if disconnect {
        tokio::spawn(lifecycle::drive(g.clone(), id));
    } else {
        tokio::spawn(cleanup(g.clone(), id, None));
    }
    Ok(wrap(view))
}
