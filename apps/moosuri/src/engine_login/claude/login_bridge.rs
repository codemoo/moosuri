//! Private noninteractive child-login bridge for the owner-bound Claude worker.
//!
//! A root gateway starts this command as the dedicated worker UID. stdin/stdout
//! are a bounded JSON-lines channel, never a terminal or a log:
//!
//! * out: `{"event":"authorization_url","url":"https://claude.com/cai/oauth/authorize?..."}`
//!   at most once, then exactly one `{"event":"result","result":"<outcome>"}`.
//! * in: `{"command":"code","code":"..."}` once after the URL, or
//!   `{"command":"cancel"}` at any time.
//!
//! The code goes to the same running official login process owned by
//! `Profile::login_web`; no raw CLI output, account detail or token is exposed.

use super::{choices_available, prepare};
use clap::Args;
use moosuri_connectors::{
    claude_code::{AccountModels, LoginCode, LoginEvent, Profile},
    model::{CancellationToken, TransportError},
};
use moosuri_store::runtime_registry::engine::ModelChoice;
use serde::{Deserialize, Serialize};
use std::{future::Future, path::PathBuf, pin::Pin, time::Duration};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    sync::mpsc,
    time::Sleep,
};
use zeroize::Zeroizing;

/// Hard bound for the whole command. `Profile::login_web` also bounds itself.
const TOTAL_LIMIT: Duration = Duration::from_secs(360);
const WRITE_LIMIT: Duration = Duration::from_secs(5);
const MAX_LINE: usize = 8192;

#[derive(Args)]
pub(crate) struct BridgeOptions {
    /// Private worker configuration (mode 0600) of the exact Claude worker.
    #[arg(long)]
    pub(crate) config: PathBuf,
}

/// Enumerated terminal results; the only information the parent receives.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
enum Outcome {
    Ready,
    Cancelled,
    Timeout,
    ProtocolError,
    InvalidInput,
    LoginFailed,
    AccountUnavailable,
    ModelUnavailable,
    CatalogUnavailable,
    ProfileUnavailable,
    OutputFailed,
}

impl Outcome {
    fn code(self) -> &'static str {
        match self {
            Self::Ready => "ready",
            Self::Cancelled => "cancelled",
            Self::Timeout => "timeout",
            Self::ProtocolError => "protocol_error",
            Self::InvalidInput => "invalid_input",
            Self::LoginFailed => "login_failed",
            Self::AccountUnavailable => "account_unavailable",
            Self::ModelUnavailable => "model_unavailable",
            Self::CatalogUnavailable => "catalog_unavailable",
            Self::ProfileUnavailable => "profile_unavailable",
            Self::OutputFailed => "output_failed",
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "event", rename_all = "snake_case")]
enum Output<'a> {
    AuthorizationUrl { url: &'a str },
    Result { result: Outcome },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawCommand {
    command: String,
    #[serde(default)]
    code: Option<Zeroizing<String>>,
}

/// Deliberately neither Debug nor Serialize: a code never reaches a log.
enum Item {
    Code(Zeroizing<String>),
    Cancel,
    Bad,
    Eof,
}

fn parse(line: &[u8]) -> Item {
    let Ok(raw) = serde_json::from_slice::<RawCommand>(line) else {
        return Item::Bad;
    };
    match (raw.command.as_str(), raw.code) {
        ("code", Some(code)) => Item::Code(code),
        ("cancel", None) => Item::Cancel,
        _ => Item::Bad,
    }
}

/// Cancellation-safe bounded line reader (state lives in the struct).
struct Input<R> {
    reader: R,
    buffer: Zeroizing<Vec<u8>>,
}

impl<R: AsyncRead + Unpin> Input<R> {
    fn new(reader: R) -> Self {
        Self {
            reader,
            buffer: Zeroizing::new(Vec::new()),
        }
    }

    async fn next(&mut self) -> Item {
        loop {
            if let Some(end) = self.buffer.iter().position(|byte| *byte == b'\n') {
                let line: Zeroizing<Vec<u8>> = Zeroizing::new(self.buffer.drain(..=end).collect());
                if end > MAX_LINE {
                    return Item::Bad;
                }
                return parse(&line[..end]);
            }
            if self.buffer.len() > MAX_LINE {
                return Item::Bad;
            }
            let mut chunk = Zeroizing::new([0u8; 1024]);
            match self.reader.read(&mut chunk[..]).await {
                // A partial line at EOF is parent loss, not a command.
                Ok(0) | Err(_) => return Item::Eof,
                Ok(n) => self.buffer.extend_from_slice(&chunk[..n]),
            }
        }
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
enum State {
    AwaitUrl,
    AwaitCode,
    CodeSent,
}

enum Action<T> {
    Done(T),
    Deadline,
    Url(LoginEvent),
    Input(Item),
}

/// Supervises every phase: one code or cancel, strict order, hard deadline.
/// A violation cancels the original operation; the phase future is still driven
/// to completion so the official login container is removed first.
struct Session<R, W> {
    input: Input<R>,
    writer: W,
    events: mpsc::Receiver<LoginEvent>,
    _events_alive: mpsc::Sender<LoginEvent>,
    codes: mpsc::Sender<LoginCode>,
    cancel: CancellationToken,
    deadline: Pin<Box<Sleep>>,
    state: State,
    abort: Option<Outcome>,
}

impl<R: AsyncRead + Unpin, W: AsyncWrite + Unpin> Session<R, W> {
    fn fail(&mut self, outcome: Outcome) {
        self.abort.get_or_insert(outcome);
        self.cancel.cancel();
    }

    async fn emit(&mut self, value: &Output<'_>) -> Result<(), ()> {
        let mut line = Zeroizing::new(serde_json::to_vec(value).map_err(|_| ())?);
        line.push(b'\n');
        let writer = &mut self.writer;
        tokio::time::timeout(WRITE_LIMIT, async {
            writer.write_all(&line).await?;
            writer.flush().await
        })
        .await
        .map_err(|_| ())?
        .map_err(|_| ())
    }

    fn command(&mut self, item: Item) {
        match (item, self.state) {
            (Item::Cancel | Item::Eof, _) => self.fail(Outcome::Cancelled),
            (Item::Bad, _) => self.fail(Outcome::InvalidInput),
            (Item::Code(code), State::AwaitCode) => match LoginCode::new(code) {
                Ok(code) => {
                    if self.codes.try_send(code).is_ok() {
                        self.state = State::CodeSent;
                    } else {
                        self.fail(Outcome::ProtocolError);
                    }
                }
                Err(_) => self.fail(Outcome::InvalidInput),
            },
            // Before the URL (out of order) or after the one allowed code.
            (Item::Code(_), _) => self.fail(Outcome::ProtocolError),
        }
    }

    async fn phase<T>(&mut self, operation: impl Future<Output = T>) -> T {
        tokio::pin!(operation);
        loop {
            let live = self.abort.is_none();
            let awaiting_url = live && self.state == State::AwaitUrl;
            let action = tokio::select! {
                biased;
                _ = &mut self.deadline, if live => Action::Deadline,
                item = self.input.next(), if live => Action::Input(item),
                Some(event) = self.events.recv(), if awaiting_url => Action::Url(event),
                value = &mut operation => Action::Done(value),
            };
            match action {
                Action::Done(value) => return value,
                Action::Deadline => self.fail(Outcome::Timeout),
                Action::Url(LoginEvent::AuthorizationUrl(url)) => {
                    self.state = State::AwaitCode;
                    if self
                        .emit(&Output::AuthorizationUrl { url: &url })
                        .await
                        .is_err()
                    {
                        self.fail(Outcome::OutputFailed);
                    }
                }
                Action::Input(item) => self.command(item),
            }
        }
    }
}

async fn catalog(
    profile: &Profile,
    environment: &str,
    email: &str,
    cancel: &CancellationToken,
) -> Result<AccountModels, String> {
    profile
        .account_models_for_email(environment, email, cancel)
        .await
        .map_err(|_| "claude_worker_catalog_unavailable".into())
}

/// Initialize-catalog verification only: never a text/inference request.
fn verdict(models: &AccountModels, choices: &[ModelChoice]) -> Outcome {
    if models.login_required {
        Outcome::AccountUnavailable
    } else if choices_available(models, choices) {
        Outcome::Ready
    } else {
        Outcome::ModelUnavailable
    }
}

async fn drive<R, W, C, F>(
    profile: &Profile,
    environment: &str,
    choices: &[ModelChoice],
    reader: R,
    writer: W,
    limit: Duration,
    check: C,
) -> Result<(), String>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
    C: Fn(CancellationToken) -> F,
    F: Future<Output = Result<AccountModels, String>>,
{
    let cancel = CancellationToken::new();
    let (events_tx, events) = mpsc::channel(1);
    let (codes, mut codes_rx) = mpsc::channel(1);
    let mut session = Session {
        input: Input::new(reader),
        writer,
        events,
        _events_alive: events_tx.clone(),
        codes,
        cancel: cancel.clone(),
        deadline: Box::pin(tokio::time::sleep(limit)),
        state: State::AwaitUrl,
        abort: None,
    };
    let outcome = 'done: {
        // No login was launched yet. Stop preflight on parent loss/deadline;
        // stale cleanup can resume next time without retaining an OS lock or
        // spending another per-container timeout after cancellation.
        let cleanup = async {
            tokio::select! {
                biased;
                _ = cancel.cancelled() => Err(TransportError::Cancelled),
                result = profile.cleanup_stale_environment(environment) => result,
            }
        };
        if session.phase(cleanup).await.is_err() {
            break 'done Outcome::ProfileUnavailable;
        }
        if session.abort.is_some() {
            break 'done Outcome::Cancelled;
        }
        match session.phase(check(cancel.clone())).await {
            Ok(models) if !models.login_required => break 'done verdict(&models, choices),
            Ok(_) => {}
            Err(_) => break 'done Outcome::CatalogUnavailable,
        }
        if session.abort.is_some() {
            break 'done Outcome::Cancelled;
        }
        // The only login attempt. Its result is never retried.
        let login = profile.login_web(environment, &events_tx, &mut codes_rx, &cancel);
        match session.phase(login).await {
            Ok(()) => {}
            Err(TransportError::Cancelled) => break 'done Outcome::Cancelled,
            Err(TransportError::Timeout) => break 'done Outcome::Timeout,
            Err(_) => break 'done Outcome::LoginFailed,
        }
        if session.abort.is_some() {
            break 'done Outcome::Cancelled;
        }
        match session.phase(check(cancel.clone())).await {
            Ok(models) => verdict(&models, choices),
            Err(_) => Outcome::CatalogUnavailable,
        }
    };
    // A recorded protocol/cancel/timeout reason is more precise than its effect.
    let outcome = session.abort.unwrap_or(outcome);
    let sent = session
        .emit(&Output::Result { result: outcome })
        .await
        .is_ok();
    match (outcome, sent) {
        (Outcome::Ready, true) => Ok(()),
        (_, false) => Err("claude_login_output_failed".into()),
        (outcome, true) => Err(format!("claude_login_{}", outcome.code())),
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) async fn run(_: &BridgeOptions) -> Result<(), String> {
    Err("claude_login_requires_unprivileged_linux_user".into())
}

/// Only anonymous/private parent pipes are supported. Tokio's ordinary stdin
/// uses an uncancellable blocking read, which can prevent runtime shutdown even
/// after a completed login. These descriptors use the reactor and bounded I/O.
#[cfg(target_os = "linux")]
fn private_pipes() -> Result<
    (
        tokio::net::unix::pipe::Receiver,
        tokio::net::unix::pipe::Sender,
    ),
    String,
> {
    use std::os::fd::AsFd;
    use tokio::net::unix::pipe::{Receiver, Sender};
    let input = std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| "claude_login_private_pipe_required")?;
    let output = std::io::stdout()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| "claude_login_private_pipe_required")?;
    let reader =
        Receiver::from_owned_fd(input).map_err(|_| "claude_login_private_pipe_required")?;
    let writer = Sender::from_owned_fd(output).map_err(|_| "claude_login_private_pipe_required")?;
    Ok((reader, writer))
}

#[cfg(target_os = "linux")]
pub(crate) async fn run(options: &BridgeOptions) -> Result<(), String> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("claude_login_requires_unprivileged_linux_user".into());
    }
    let config = super::super::read_config(&options.config)?;
    let choices = super::super::validate_config(&config)?;
    let super::super::Settings::ClaudeCode(settings) = config.adapter else {
        return Err("claude_login_config_not_claude".into());
    };
    let (reader, writer) = private_pipes()?;
    let (profile, environment, email) = prepare(settings).await?;
    drive(
        &profile,
        &environment,
        &choices,
        reader,
        writer,
        TOTAL_LIMIT,
        |cancel| {
            let (profile, environment, email) = (&profile, &environment, &email);
            async move { catalog(profile, environment, email, &cancel).await }
        },
    )
    .await
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests {
    use super::*;
    use moosuri_connectors::claude_code::fixture::Fixture;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::io::{AsyncBufReadExt, BufReader, duplex};
    use uuid::Uuid;

    const EMAIL: &str = "owner@example.test";
    const CODE: &[u8] = b"{\"command\":\"code\",\"code\":\"synthetic-code#state\"}\n";

    enum Script {
        /// Read only; already logged in.
        Passive,
        /// Read URL, then stay silent until the hard deadline.
        Quiet,
        Code,
        CodeTwice,
        Cancel,
        Eof,
        OutputGone,
        Raw(Vec<u8>),
        /// Code before any URL was emitted.
        EarlyCode,
    }

    fn choice(model: &str) -> ModelChoice {
        ModelChoice {
            model: model.into(),
            effort: Some("xhigh".into()),
        }
    }

    /// `plan` = fixture behavior for the pre-login and post-login catalog checks.
    /// The login process itself always sees `web-login`.
    async fn exercise(
        f: &Fixture,
        selected: &ModelChoice,
        plan: [&'static str; 2],
        limit: Duration,
        script: Script,
    ) -> (Vec<serde_json::Value>, Result<(), String>) {
        let (mut commands, command_rx) = duplex(16384);
        let (output_tx, output_rx) = duplex(16384);
        let environment = Uuid::new_v4().to_string();
        let calls = AtomicUsize::new(0);
        let (calls_ref, env) = (&calls, environment.as_str());
        let check = move |cancel: CancellationToken| {
            let n = calls_ref.fetch_add(1, Ordering::SeqCst);
            async move {
                f.behavior(plan[n.min(1)]);
                let result = catalog(&f.profile, env, EMAIL, &cancel).await;
                if n == 0 {
                    f.behavior("web-login");
                }
                result
            }
        };
        let interact = async move {
            let mut lines = BufReader::new(output_rx).lines();
            let mut seen = Vec::new();
            if matches!(script, Script::OutputGone) {
                drop(lines);
                // Keep input live, so this specifically tests output loss.
                tokio::time::sleep(Duration::from_secs(1)).await;
                return seen;
            }
            let mut read = |line: String| seen.push(serde_json::from_str(&line).unwrap());
            match &script {
                Script::Passive => {}
                Script::EarlyCode => {
                    let _ = commands.write_all(CODE).await;
                }
                _ => {
                    read(lines.next_line().await.unwrap().unwrap());
                    match &script {
                        Script::Code => {
                            let _ = commands.write_all(CODE).await;
                        }
                        Script::CodeTwice => {
                            let _ = commands.write_all(&[CODE, CODE].concat()).await;
                        }
                        Script::Cancel => {
                            let _ = commands.write_all(b"{\"command\":\"cancel\"}\n").await;
                        }
                        Script::Raw(bytes) => {
                            let _ = commands.write_all(bytes).await;
                        }
                        _ => {}
                    }
                    if matches!(script, Script::Eof) {
                        drop(std::mem::replace(&mut commands, duplex(1).0));
                    }
                }
            }
            while let Some(line) = lines.next_line().await.unwrap() {
                read(line);
            }
            seen
        };
        let (seen, result) = tokio::join!(
            interact,
            drive(
                &f.profile,
                &environment,
                std::slice::from_ref(selected),
                command_rx,
                output_tx,
                limit,
                check
            )
        );
        (seen, result)
    }

    fn assert_terminal(seen: &[serde_json::Value], expected: &str) {
        let results: Vec<_> = seen.iter().filter(|v| v["event"] == "result").collect();
        assert_eq!(results.len(), 1, "{seen:?}");
        assert_eq!(seen.last().unwrap()["result"], expected, "{seen:?}");
        assert!(
            seen.iter()
                .filter(|v| v["event"] == "authorization_url")
                .count()
                <= 1
        );
        assert!(
            seen.iter()
                .all(|v| v["event"] == "authorization_url" || v["event"] == "result")
        );
    }

    fn removed(f: &Fixture) -> bool {
        f.directory.path().join("removed").exists()
    }
    fn no_inference(f: &Fixture) {
        assert!(!f.directory.path().join("bound-user").exists());
        assert!(!f.directory.path().join("usage-command").exists());
        assert!(!f.directory.path().join("runtime-model.json").exists());
    }
    const LOGIN: [&str; 2] = ["logged-out", "web-login"];
    const LONG: Duration = Duration::from_secs(60);

    #[tokio::test]
    async fn code_reaches_original_process_then_catalog_makes_ready() {
        let f = Fixture::new();
        let (seen, result) =
            exercise(&f, &choice("claude-fixture-v1"), LOGIN, LONG, Script::Code).await;
        assert!(result.is_ok(), "{seen:?}");
        assert_terminal(&seen, "ready");
        assert_eq!(seen.len(), 2);
        assert!(
            seen[0]["url"]
                .as_str()
                .unwrap()
                .starts_with("https://claude.com/cai/oauth/authorize?")
        );
        // Pre-check, exactly one original login process, post-check.
        assert_eq!(f.launches(), 3);
        assert!(removed(&f));
        no_inference(&f);
    }

    #[tokio::test]
    async fn existing_exact_account_and_model_skips_login() {
        let f = Fixture::new();
        let (seen, result) = exercise(
            &f,
            &choice("claude-fixture-v1"),
            ["ok", "ok"],
            LONG,
            Script::Passive,
        )
        .await;
        assert!(result.is_ok());
        assert_terminal(&seen, "ready");
        assert_eq!(seen.len(), 1);
        assert_eq!(f.launches(), 1);
        no_inference(&f);

        let f = Fixture::new();
        let (seen, result) = exercise(
            &f,
            &choice("claude-unverified"),
            ["ok", "ok"],
            LONG,
            Script::Passive,
        )
        .await;
        assert!(result.is_err());
        assert_terminal(&seen, "model_unavailable");
        assert_eq!(f.launches(), 1);
    }

    #[tokio::test]
    async fn cancel_and_parent_eof_clean_up_the_original_login() {
        for script in [Script::Cancel, Script::Eof] {
            let f = Fixture::new();
            let (seen, result) =
                exercise(&f, &choice("claude-fixture-v1"), LOGIN, LONG, script).await;
            assert!(result.is_err());
            assert_terminal(&seen, "cancelled");
            assert_eq!(f.launches(), 2);
            assert!(removed(&f));
            no_inference(&f);
        }
    }

    #[tokio::test]
    async fn malformed_duplicate_and_out_of_order_commands_are_fixed_failures() {
        let cases: Vec<(Script, &str)> = vec![
            (Script::Raw(b"not json\n".to_vec()), "invalid_input"),
            (
                Script::Raw(b"{\"command\":\"code\",\"code\":\"short\"}\n".to_vec()),
                "invalid_input",
            ),
            (
                Script::Raw(b"{\"command\":\"code\",\"code\":\"abcdefgh\\n\"}\n".to_vec()),
                "invalid_input",
            ),
            (
                Script::Raw(b"{\"command\":\"cancel\",\"extra\":1}\n".to_vec()),
                "invalid_input",
            ),
            (
                Script::Raw(b"{\"command\":\"code\"}\n".to_vec()),
                "invalid_input",
            ),
            (Script::Raw(vec![b'a'; 20000]), "invalid_input"),
            (Script::CodeTwice, "protocol_error"),
            (Script::EarlyCode, "protocol_error"),
        ];
        for (script, expected) in cases {
            let f = Fixture::new();
            let (seen, result) =
                exercise(&f, &choice("claude-fixture-v1"), LOGIN, LONG, script).await;
            assert!(result.is_err());
            assert_terminal(&seen, expected);
            no_inference(&f);
        }
    }

    #[tokio::test]
    async fn wrong_identity_or_selected_model_never_becomes_ready() {
        for (model, post, expected) in [
            (
                "claude-fixture-v1",
                "different-account",
                "account_unavailable",
            ),
            ("claude-fixture-v1", "email-missing", "account_unavailable"),
            ("claude-fixture-v1", "unsubscribed", "account_unavailable"),
            ("claude-fixture-v1", "effort-withdrawn", "model_unavailable"),
            ("claude-unverified", "web-login", "model_unavailable"),
        ] {
            let f = Fixture::new();
            let (seen, result) =
                exercise(&f, &choice(model), ["logged-out", post], LONG, Script::Code).await;
            assert!(result.is_err(), "{post}");
            assert_terminal(&seen, expected);
            assert_eq!(f.launches(), 3, "{post}");
            no_inference(&f);
        }
    }

    #[tokio::test]
    async fn hard_deadline_cancels_and_removes_the_login_process() {
        let f = Fixture::new();
        let (seen, result) = exercise(
            &f,
            &choice("claude-fixture-v1"),
            LOGIN,
            Duration::from_secs(4),
            Script::Quiet,
        )
        .await;
        assert!(result.is_err());
        assert_terminal(&seen, "timeout");
        assert!(removed(&f));
    }

    #[tokio::test]
    async fn closed_output_cancels_without_exposing_raw_login_output() {
        let f = Fixture::new();
        let (seen, result) = exercise(
            &f,
            &choice("claude-fixture-v1"),
            LOGIN,
            LONG,
            Script::OutputGone,
        )
        .await;
        assert!(seen.is_empty());
        assert_eq!(result, Err("claude_login_output_failed".into()));
        assert!(removed(&f));
        no_inference(&f);
    }

    #[test]
    fn command_grammar_is_closed() {
        assert!(matches!(parse(b"{\"command\":\"cancel\"}"), Item::Cancel));
        assert!(matches!(
            parse(b"{\"command\":\"code\",\"code\":\"x\"}"),
            Item::Code(_)
        ));
        for bad in [
            &b"{}"[..],
            b"{\"command\":\"code\"}",
            b"{\"command\":\"cancel\",\"code\":\"x\"}",
            b"{\"command\":\"other\"}",
            b"[]",
        ] {
            assert!(matches!(parse(bad), Item::Bad));
        }
        assert_eq!(
            serde_json::to_string(&Output::Result {
                result: Outcome::Ready
            })
            .unwrap(),
            "{\"event\":\"result\",\"result\":\"ready\"}"
        );
    }
}
