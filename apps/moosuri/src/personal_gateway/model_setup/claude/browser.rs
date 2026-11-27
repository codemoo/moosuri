//! The in-app official Claude sign-in. The gateway (already root) runs the
//! connector's `CapsuleReader` directly against the existing Google capsule
//! inventory, so there is no second pool, service or generic browser protocol.
//!
//! One supervisor task per login owns the `ClaudeAttempt`. It polls about once
//! a second whether or not any page is open, so the automatic callback finishes
//! without the UI. The job (behind a std mutex) holds only a bounded command
//! channel and the latest sanitized frame, never the browser itself. The
//! authorization URL, the code and every page content stay in memory: the URL
//! goes straight to the worker, the code straight into the existing CLI
//! channel, and none of them is logged or returned over HTTP.
//!
//! Startup shares the Google root broker's admission lock
//! (`/run/moosuri-google-browser/start.lock`) so engine inventory selection and
//! container creation cannot interleave across the two processes. The lock is
//! held only until `start_claude` returns.
//!
//! The browser is always shut down with an acknowledgement before the CLI
//! result, activation or the setup slot is released. An unconfirmed shutdown
//! sets `Job::browser_unresolved`: the job stays `cleanup_pending` and nothing
//! declares it cancelled or ready.
use super::*;
use crate::runtime_google::{private_bytes, private_dir};
use moosuri_connectors::browser::{
    BrowserError,
    capsule::{
        CapsuleConfig, CapsuleReader, ClaudeAttempt, ClaudeView, WorkspaceFrame, WorkspaceInput,
    },
};
use tokio::sync::oneshot;

/// Shared with the Google root broker's `--startup-lock` default.
const START_LOCK: &str = "/run/moosuri-google-browser/start.lock";
const CONFIG_LIMIT: usize = 16 * 1024;
const POLL: Duration = Duration::from_secs(1);
const CAPACITY_WAIT: Duration = Duration::from_secs(10);
const READY_LIMIT: Duration = Duration::from_secs(45);
const INPUT_WAIT: Duration = Duration::from_secs(20);
/// A browser that does not acknowledge its own shutdown in this time is not
/// presumed gone.
const STOP_LIMIT: Duration = Duration::from_secs(90);
const TOKEN_LIMIT: usize = 256;

#[derive(Clone)]
pub(in crate::personal_gateway) struct Runtime {
    reader: Arc<CapsuleReader>,
}
impl Runtime {
    /// `None` when the flag is absent (an installation without it offers no
    /// in-app sign-in and has no fallback path). A present flag must name the
    /// existing private root-owned capsule configuration of this installation.
    pub(in crate::personal_gateway) fn new(options: &Options) -> Result<Option<Self>, String> {
        let Some(path) = options.claude_browser_capsule_config.as_deref() else {
            return Ok(None);
        };
        let installation = options
            .google_runtime_installation
            .ok_or("claude_browser_configuration_invalid")?;
        if !options.model_setup
            || !path.is_absolute()
            || installation.is_nil()
            || unsafe { libc::geteuid() } != 0
        {
            return Err("claude_browser_configuration_invalid".into());
        }
        let bytes =
            private_bytes(path, CONFIG_LIMIT).map_err(|_| "claude_browser_config_invalid")?;
        let text = std::str::from_utf8(&bytes).map_err(|_| "claude_browser_config_invalid")?;
        let config = CapsuleConfig::from_json(text).map_err(|_| "claude_browser_config_invalid")?;
        if config.owner != installation {
            return Err("claude_browser_installation_mismatch".into());
        }
        let reader = CapsuleReader::new(config).map_err(|_| "claude_browser_config_invalid")?;
        Ok(Some(Self {
            reader: Arc::new(reader),
        }))
    }
}

/// Reconcile an interrupted browser without removing or adopting any container.
/// Missing configuration is not cleanup evidence for a durable browser intent.
/// The shared lock excludes new starts while two empty inventory reads agree.
pub(super) async fn inventory_confirmed(g: &Gateway) -> bool {
    let Some(runtime) = &g.claude_browser else {
        return false;
    };
    let cancel = CancellationToken::new();
    let check = async {
        let _lock = admission(&cancel).await?;
        for _ in 0..20 {
            if runtime
                .reader
                .authentication_inventory_empty(&cancel)
                .await?
            {
                sleep(Duration::from_millis(250)).await;
                if runtime
                    .reader
                    .authentication_inventory_empty(&cancel)
                    .await?
                {
                    return Ok::<bool, BrowserError>(true);
                }
            }
            sleep(Duration::from_secs(1)).await;
        }
        Ok(false)
    };
    matches!(timeout(Duration::from_secs(30), check).await, Ok(Ok(true)))
}

/// Admission lock shared with the Google root broker. Same file checks and the
/// same cancellable poll; a lock held elsewhere only delays, never fails.
async fn admission(cancel: &CancellationToken) -> Result<std::fs::File, BrowserError> {
    let path = std::path::Path::new(START_LOCK);
    private_dir(path.parent().ok_or(BrowserError::Setup)?).map_err(|_| BrowserError::Setup)?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| BrowserError::Setup)?;
    let meta = file.metadata().map_err(|_| BrowserError::Setup)?;
    if !meta.is_file()
        || meta.uid() != 0
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
        || meta.len() != 0
    {
        return Err(BrowserError::Setup);
    }
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(_) => return Err(BrowserError::Setup),
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err(BrowserError::Cancelled),
            _ = sleep(Duration::from_millis(100)) => {}
        }
    }
}
async fn start(
    rt: &Runtime,
    tenant: Uuid,
    url: &Zeroizing<String>,
    cancel: &CancellationToken,
) -> Result<ClaudeAttempt, BrowserError> {
    let _admission = admission(cancel).await?;
    let deadline = Instant::now() + CAPACITY_WAIT;
    loop {
        let fresh = Zeroizing::new(String::clone(url));
        match rt.reader.start_claude(tenant, fresh, cancel).await {
            // A just-closed attempt may still be removing its container.
            Err(BrowserError::Capacity) if Instant::now() < deadline => tokio::select! {
                _ = cancel.cancelled() => return Err(BrowserError::Cancelled),
                _ = sleep(Duration::from_millis(500)) => {}
            },
            other => return other,
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub(super) enum InputError {
    /// The page moved on; the user may look again and retry by hand.
    Stale,
    /// The browser is closing or no longer accepting input.
    Closed,
    /// Dispatch may or may not have happened. Never replayed.
    Uncertain,
}
pub(super) enum Command {
    Input {
        token: String,
        input: WorkspaceInput,
        reply: oneshot::Sender<Result<(), InputError>>,
    },
}

/// The running supervisor. `finish` always awaits it.
pub(super) struct Pump {
    stop: CancellationToken,
    task: tokio::task::JoinHandle<()>,
}
impl Pump {
    /// Stops the supervisor and waits for its acknowledged shutdown. A task
    /// that panics or does not answer in time leaves the browser unconfirmed.
    pub(super) async fn finish(self, g: &Gateway, id: Uuid) {
        self.stop.cancel();
        let mut task = self.task;
        if !matches!(timeout(STOP_LIMIT, &mut task).await, Ok(Ok(()))) {
            with_job(g, id, |j| j.browser_unresolved = true);
        }
    }
}

/// Starts the sealed browser for the CLI's authorization URL and returns once
/// its first frame exists (the job is then `awaiting_code` with a live browser).
/// Every failure path has already awaited the browser's own cleanup.
pub(super) async fn launch(
    g: &Gateway,
    id: Uuid,
    url: Zeroizing<String>,
    cancel: &CancellationToken,
) -> Result<Pump, Stop> {
    let rt = g.claude_browser.clone().ok_or(Stop::Failed)?;
    let tenant = with_job(g, id, |j| owner_of(&j.allocation))
        .flatten()
        .ok_or(Stop::Failed)?
        .tenant_id;
    let stop = cancel.child_token();
    // Persist before a process can be created, including crashes during startup.
    with_job(g, id, |j| j.browser_unresolved = true);
    if !persist(g, id).await {
        report("launch", Some("receipt_failed"));
        return Err(Stop::Failed);
    }
    let attempt = match start(&rt, tenant, &url, &stop).await {
        Ok(attempt) => attempt,
        Err(BrowserError::CleanupUnconfirmed) => {
            report("launch", Some("cleanup_unconfirmed"));
            with_job(g, id, |j| j.browser_unresolved = true);
            return Err(Stop::Failed);
        }
        Err(error) => {
            report(
                "launch",
                Some(match &error {
                    BrowserError::Timeout => "timeout",
                    BrowserError::Cancelled => "cancelled",
                    BrowserError::Capacity => "capacity",
                    BrowserError::Protocol => "protocol",
                    BrowserError::Setup => "setup",
                    _ => "start_failed",
                }),
            );
            // Only these classified failures prove no unacknowledged process.
            // Unexpected errors require an independent empty inventory receipt.
            let clean = matches!(
                error,
                BrowserError::InvalidUrl
                    | BrowserError::Capacity
                    | BrowserError::IsolationRequired
                    | BrowserError::Cancelled
            ) || inventory_confirmed(g).await;
            with_job(g, id, |j| j.browser_unresolved = !clean);
            persist(g, id).await;
            return Err(if cancel.is_cancelled() {
                Stop::Cancelled
            } else {
                Stop::Failed
            });
        }
    };
    drop(url);
    let (tx, rx) = mpsc::channel(1);
    let (ready_tx, ready_rx) = oneshot::channel();
    let task = tokio::spawn(supervise(
        g.clone(),
        id,
        attempt,
        (tx, rx),
        ready_tx,
        cancel.clone(),
        stop.clone(),
    ));
    let pump = Pump { stop, task };
    let outcome = tokio::select! {
        ready = ready_rx => ready.map_err(|_| { report("launch", Some("supervisor_ended")); Stop::Failed }),
        _ = cancel.cancelled() => Err(Stop::Cancelled),
        _ = sleep(READY_LIMIT) => { report("launch", Some("ready_timeout")); Err(Stop::Failed) },
    };
    match outcome {
        Ok(()) => Ok(pump),
        Err(why) => {
            pump.finish(g, id).await;
            Err(why)
        }
    }
}

/// One bounded terminal diagnostic line: a static stage and the connector's fixed
/// static cause label. No error text, URL, page, input, code or identifier is ever
/// formatted, and it is emitted only for a terminal event, never per frame.
fn report(stage: &'static str, cause: Option<&'static str>) {
    use std::io::Write as _;
    let _ = writeln!(
        std::io::stderr().lock(),
        "moosuri: claude_browser_{stage}_{}",
        cause.unwrap_or("unclassified")
    );
}

async fn serve(g: &Gateway, id: Uuid, attempt: &mut ClaudeAttempt, command: Command) {
    let Command::Input {
        token,
        input,
        reply,
    } = command;
    // Authorization is re-read right before dispatch: the supervision task
    // cancels the job on revocation, and only an awaiting login takes input.
    let live = with_job(g, id, |j| {
        j.stage == Stage::AwaitingCode && !j.cancel.is_cancelled()
    })
    .unwrap_or(false);
    let cancel = with_job(g, id, |j| j.cancel.clone());
    let authorized = match cancel {
        Some(ref cancel) if live => check(g, id, cancel).await.is_ok(),
        _ => false,
    };
    let result = if !authorized || reply.is_closed() {
        Err(InputError::Closed)
    } else {
        match attempt.input(&token, input).await {
            Ok(()) => Ok(()),
            Err(BrowserError::StalePage | BrowserError::InvalidHandle) => Err(InputError::Stale),
            Err(_) => {
                if let Some(cause) = attempt.cause().filter(|cause| *cause != "host_cancelled") {
                    report("input", Some(cause));
                }
                Err(InputError::Uncertain)
            }
        }
    };
    let _ = reply.send(result);
}

/// Owns the browser from first frame to acknowledged shutdown.
async fn supervise(
    g: Gateway,
    id: Uuid,
    mut attempt: ClaudeAttempt,
    (tx, mut rx): (mpsc::Sender<Command>, mpsc::Receiver<Command>),
    ready: oneshot::Sender<()>,
    job: CancellationToken,
    stop: CancellationToken,
) {
    let (mut tx, mut ready) = (Some(tx), Some(ready));
    let mut tick = interval(POLL);
    tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut fault = false;
    loop {
        tokio::select! { biased;
            _ = stop.cancelled() => break,
            command = rx.recv() => match command {
                Some(command) => serve(&g, id, &mut attempt, command).await,
                None => break,
            },
            // The selected branches above are cancel-safe; the poll itself is
            // awaited to completion in the body, never dropped half-way.
            _ = tick.tick() => match attempt.poll().await {
                Ok(ClaudeView::Frame(frame)) => {
                    let first = ready.take();
                    let channel = if first.is_some() { tx.take() } else { None };
                    with_job(&g, id, |j| {
                        if let Some(channel) = channel {
                            if j.stage == Stage::Preparing {
                                j.stage = Stage::AwaitingCode;
                            }
                            j.browser = Some(channel);
                        }
                        j.frame = Some(frame);
                    });
                    if let Some(first) = first {
                        let _ = first.send(());
                    }
                }
                Ok(ClaudeView::Code(code)) => {
                    // Exactly one hand-over to the existing CLI channel.
                    let valid = protocol::valid_code(&code);
                    let sent = valid
                        && matches!(
                            with_job(&g, id, |j| j.accept_code(code)),
                            Some(CodeStep::Sent)
                        );
                    if sent {
                        persist(&g, id).await;
                    } else {
                        report("handoff", Some("code_not_accepted"));
                        fault = true;
                    }
                    break;
                }
                // A screenshot can race navigation/compositing; no action was sent.
                // Keep the attempt alive within its original deadline and ask again.
                Err(BrowserError::StalePage) => {
                    with_job(&g, id, |j| j.frame = None);
                }
                Err(error) => {
                    // A user/deadline cancellation can interrupt an in-flight poll.
                    // Preserve its cause instead of relabelling it a browser fault.
                    fault = !stop.is_cancelled() && !matches!(error, BrowserError::Cancelled);
                    if fault {
                        report("poll", attempt.cause());
                    }
                    break;
                }
            },
        }
    }
    // Nothing stale is offered while the browser closes.
    with_job(&g, id, |j| {
        j.browser = None;
        j.frame = None;
        j.faulted |= fault;
    });
    if fault {
        job.cancel();
    }
    rx.close();
    let unresolved = attempt.shutdown().await.is_err();
    with_job(&g, id, |j| j.browser_unresolved = unresolved);
    persist(&g, id).await;
}

fn token_ok(token: &str) -> bool {
    (1..=TOKEN_LIMIT).contains(&token.len()) && token.bytes().all(|b| b.is_ascii_graphic())
}

/// `op:frame`: only the exact initiating session of a live awaiting login.
pub(super) fn frame(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
) -> Result<Value, StatusCode> {
    let jobs = lock(g)?;
    let j = jobs
        .iter()
        .find(|j| j.id == id && j.owned_by(a, session))
        .ok_or(StatusCode::NOT_FOUND)?;
    if j.stage != Stage::AwaitingCode || j.browser.is_none() || j.cancel.is_cancelled() {
        return Err(StatusCode::CONFLICT);
    }
    let frame: &WorkspaceFrame = j.frame.as_ref().ok_or(StatusCode::CONFLICT)?;
    // Enforce the HTTP boundary independently of the worker's origin-only contract.
    let origin = url::Url::parse(&frame.url)
        .ok()
        .filter(|u| u.scheme() == "https" && u.username().is_empty() && u.password().is_none())
        .map(|u| u.origin().ascii_serialization())
        .unwrap_or_default();
    Ok(json!({"supported":true,"operation":j.view(true),"frame":{
        "png_base64":frame.png_base64, "width":frame.width, "height":frame.height,
        "url":origin, "frame_token":frame.frame_token,
    }}))
}

/// `op:input`: one human input to the live frame. A full queue was never
/// accepted (retry is safe); an accepted input whose reply is lost is uncertain
/// and reported as unavailable, never replayed by the gateway.
pub(super) async fn input(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    id: Uuid,
    token: String,
    input: WorkspaceInput,
) -> Result<Value, StatusCode> {
    if !token_ok(&token) || input.validate().is_err() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let sender = {
        let jobs = lock(g)?;
        let j = jobs
            .iter()
            .find(|j| j.id == id && j.owned_by(a, session))
            .ok_or(StatusCode::NOT_FOUND)?;
        if j.stage != Stage::AwaitingCode || j.cancel.is_cancelled() {
            return Err(StatusCode::CONFLICT);
        }
        j.browser.clone().ok_or(StatusCode::CONFLICT)?
    };
    let (reply, answer) = oneshot::channel();
    sender
        .try_send(Command::Input {
            token,
            input,
            reply,
        })
        .map_err(|_| UNAVAILABLE)?;
    match timeout(INPUT_WAIT, answer).await {
        Ok(Ok(Ok(()))) => {}
        Ok(Ok(Err(InputError::Stale | InputError::Closed))) => return Err(StatusCode::CONFLICT),
        _ => return Err(UNAVAILABLE),
    }
    with_job(g, id, |j| wrap(j.view(true))).ok_or(StatusCode::NOT_FOUND)
}
