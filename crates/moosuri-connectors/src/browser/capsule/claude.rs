//! Installation-only transport for one Claude sign-in inside the same Kiri.
//! Reuses the Google authentication image, its `moosuri-google-<installation>-[01]`
//! inventory, the shared two-slot capacity and the capsule's pinned engine, network,
//! limits and cleanup. There is no open/observe/script/CDP wire: the typed protocol is
//! `poll` (sanitized frame or the one captured code) and proof-bound human `input`.
//!
//! Cancellation: `Drop` (and `shutdown`) cancel an independently owned actor, which
//! always awaits graceful worker exit and exact owned-container reconciliation. A
//! caller that abandons a request mid-flight cancels the attempt (stale replies and
//! uncertain input dispatch are never adopted or replayed). Cleanup that cannot be
//! confirmed is reported (`CleanupUnconfirmed`/`Setup`), never swallowed.
use super::*;
use crate::browser::claude_auth::{self, AuthorizeUrl, Secret};
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroizing;

/// Graceful worker exit (Chrome and X torn down, exit 0) is the cleanup receipt.
const WORKER_GRACE: Duration = if cfg!(test) {
    Duration::from_millis(1500)
} else {
    Duration::from_secs(10)
};
/// A base64 frame (below 3 MiB) plus its JSON envelope.
const MAX_FRAME_LINE: usize = 3 * 1024 * 1024 + 8192;

/// One poll result. No Debug on the code.
pub enum ClaudeView {
    Frame(WorkspaceFrame),
    /// `code#state` for the exact attempt, handed out once. Never log or serialize it.
    Code(Zeroizing<String>),
}
impl std::fmt::Debug for ClaudeView {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Frame(_) => f.write_str("ClaudeView::Frame(..)"),
            Self::Code(_) => f.write_str("ClaudeView::Code(<redacted>)"),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Poll,
    Input,
}
enum Answer {
    Frame(WorkspaceFrame),
    Code(Zeroizing<String>),
    Accepted,
}
struct Request {
    /// A complete wire line (with newline). Input text may be a password: zeroized.
    line: Zeroizing<Vec<u8>>,
    kind: Kind,
    reply: oneshot::Sender<Result<Answer, BrowserError>>,
}

pub struct ClaudeAttempt {
    requests: mpsc::Sender<Request>,
    cancel: CancellationToken,
    done: Option<oneshot::Receiver<Result<(), BrowserError>>>,
    /// The code was handed out; nothing more is served.
    finished: bool,
}
struct Guard(Option<CancellationToken>);
impl Drop for Guard {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            token.cancel();
        }
    }
}
impl Drop for ClaudeAttempt {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    ok: bool,
    #[serde(default)]
    frame: Option<WorkspaceFrame>,
    #[serde(default)]
    code: Option<Secret>,
    #[serde(default)]
    accepted: Option<bool>,
    #[serde(default)]
    error: Option<String>,
}

fn validate_frame(frame: &WorkspaceFrame) -> Result<(), BrowserError> {
    if frame.width != 1280
        || frame.height != 900
        || !hex64(&frame.frame_token)
        || !claude_auth::is_sanitized_origin(&frame.url)
        || frame.png_base64.len() > 3 * 1024 * 1024
    {
        return Err(BrowserError::Protocol);
    }
    use base64::Engine;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&frame.png_base64)
        .map_err(|_| BrowserError::Protocol)?;
    if bytes.len() > 2 * 1024 * 1024
        || bytes.len() < 24
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes[12..16] != *b"IHDR"
        || bytes[16..20] != 1280u32.to_be_bytes()
        || bytes[20..24] != 900u32.to_be_bytes()
    {
        return Err(BrowserError::Protocol);
    }
    Ok(())
}

/// Exact-shape parse of one worker line for the request kind that was sent.
fn parse_reply(bytes: &[u8], kind: Kind, authorize: &AuthorizeUrl) -> Result<Answer, BrowserError> {
    let reply: Reply = serde_json::from_slice(bytes).map_err(|_| BrowserError::Protocol)?;
    if !reply.ok {
        if reply.frame.is_some() || reply.code.is_some() || reply.accepted.is_some() {
            return Err(BrowserError::Protocol);
        }
        return Err(match reply.error.as_deref() {
            Some("stale_page") => BrowserError::StalePage,
            Some("invalid_handle") => BrowserError::InvalidHandle,
            Some("capacity") => BrowserError::Capacity,
            _ => BrowserError::Protocol,
        });
    }
    if reply.error.is_some() {
        return Err(BrowserError::Protocol);
    }
    match (kind, reply.frame, reply.code, reply.accepted) {
        (Kind::Poll, Some(frame), None, None) => {
            validate_frame(&frame)?;
            Ok(Answer::Frame(frame))
        }
        (Kind::Poll, None, Some(code), None) => {
            if !authorize.owns_code(code.as_str()) {
                return Err(BrowserError::Protocol);
            }
            Ok(Answer::Code(code.into_zeroizing()))
        }
        (Kind::Input, None, None, Some(true)) => Ok(Answer::Accepted),
        _ => Err(BrowserError::Protocol),
    }
}

/// One bounded reply line in a pre-sized zeroizing buffer (it may hold the code).
async fn read_reply<R: tokio::io::AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> Result<Zeroizing<Vec<u8>>, BrowserError> {
    let mut bytes = Zeroizing::new(Vec::with_capacity(limit + 1));
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
    Ok(bytes)
}
async fn write_line(child: &mut Child, bytes: &[u8]) -> Result<(), BrowserError> {
    let input = child.stdin.as_mut().ok_or(BrowserError::Protocol)?;
    input
        .write_all(bytes)
        .await
        .map_err(|_| BrowserError::Protocol)?;
    input.flush().await.map_err(|_| BrowserError::Protocol)
}

impl CapsuleReader {
    fn fixture_worker(&self) -> bool {
        #[cfg(test)]
        let fixture = self.fixture.is_some();
        #[cfg(not(test))]
        let fixture = false;
        fixture
    }

    /// Starts one disposable Claude sign-in browser. `authorization` is the official
    /// authorization URL printed by the CLI; it is validated here (exact origin/path, one
    /// state, the official redirect) and again in the worker, and is never logged.
    ///
    /// The caller (gateway) must already hold `/run/moosuri-google-browser/start.lock`
    /// and release it only after this returns; this transport neither takes nor checks it.
    /// Capacity, container-name inventory and cleanup are shared with Google attempts, so
    /// an orphan or a Google worker consumes the same two slots and is never evicted.
    pub async fn start_claude(
        &self,
        tenant: Uuid,
        authorization: Zeroizing<String>,
        cancel: &CancellationToken,
    ) -> Result<ClaudeAttempt, BrowserError> {
        let fixture = self.fixture_worker();
        if tenant.is_nil()
            || (!fixture
                && (self.config.engine != Engine::Kiri
                    || self.config.recover_workspaces
                    || !self.config.javascript_enabled))
        {
            return Err(BrowserError::IsolationRequired);
        }
        let authorize = AuthorizeUrl::parse(&authorization)?;
        drop(authorization);
        if cancel.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| BrowserError::Capacity)?;
        let _slot_lock = tokio::select! { biased;
            _ = cancel.cancelled() => return Err(BrowserError::Cancelled),
            lock = self.member_start.lock() => lock,
        };
        let prefix = format!("moosuri-google-{}-", self.config.owner.simple());
        let name = if fixture {
            format!("{prefix}0")
        } else {
            self.config.validate()?;
            self.validate_resources(cancel).await?;
            empty_docker_config(Path::new(DOCKER_CONFIG))?;
            // Engine inventory bounds admission after a restart: an orphan is never
            // adopted or evicted to make room.
            let names = self
                .docker_output(
                    &[
                        "container",
                        "ls",
                        "--all",
                        "--format",
                        "{{.Names}}",
                        "--filter",
                        &format!("name=^/{prefix}[01]$"),
                    ],
                    cancel,
                    Duration::from_secs(3),
                )
                .await?;
            workspace::free_member_slot(&prefix, &names)?
        };
        let cancel = cancel.child_token();
        // Dropping this future before the actor reports startup cancels the actor,
        // which still awaits its own cleanup and releases the capacity permit itself.
        let mut pending = Guard(Some(cancel.clone()));
        let (requests, rx) = mpsc::channel(1);
        let (started, startup) = oneshot::channel();
        let (finished, done) = oneshot::channel();
        let this = self.clone();
        let control = cancel.clone();
        tokio::spawn(async move {
            let _capacity = capacity;
            let result = this
                .claude_actor(tenant, authorize, name, control, rx, started)
                .await;
            let _ = finished.send(result);
        });
        // The actor may already have spawned a container. Losing its receipt
        // never proves absence, including a panic during startup or cleanup.
        startup
            .await
            .map_err(|_| BrowserError::CleanupUnconfirmed)??;
        pending.0 = None;
        Ok(ClaudeAttempt {
            requests,
            cancel,
            done: Some(done),
            finished: false,
        })
    }

    async fn claude_actor(
        &self,
        tenant: Uuid,
        authorize: AuthorizeUrl,
        name: String,
        cancel: CancellationToken,
        mut requests: mpsc::Receiver<Request>,
        started: oneshot::Sender<Result<(), BrowserError>>,
    ) -> Result<(), BrowserError> {
        let identity = Uuid::new_v4();
        let mut command = self.worker_command_mode(tenant, identity, &name, None, true);
        command
            .arg("--claude-auth")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = started.send(Err(BrowserError::Setup));
                return Ok(());
            }
        };
        // A line from the worker proves its container exists, even if it only reports
        // failure; this lets cleanup treat an empty inventory as removal, not as doubt.
        let mut observed = false;
        // Keep stdout open through shutdown. Cancellation can abandon a poll while
        // the worker is still writing its frame; closing or merely retaining an
        // unread pipe can make the Docker client fail or block before its exit receipt.
        let mut reader = child.stdout.take().map(BufReader::new);
        let setup = async {
            let reader = reader.as_mut().ok_or(BrowserError::Setup)?;
            let init = claude_auth::init_line(tenant, &authorize)?;
            write_line(&mut child, &init).await?;
            drop(init);
            let ready = read_line(reader, 512).await?;
            observed = true;
            if ready != json!({"status":"ready","tenant":tenant}) {
                return Err(BrowserError::Setup);
            }
            Ok(())
        };
        let setup = tokio::select! { biased;
            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(75), setup) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        let mut failed_start = None;
        match (setup, reader.as_mut()) {
            (Err(error), _) => failed_start = Some((started, error)),
            (Ok(()), None) => failed_start = Some((started, BrowserError::Setup)),
            (Ok(()), Some(reader)) => {
                if started.send(Ok(())).is_ok() {
                    let deadline = tokio::time::Instant::now()
                        + Duration::from_secs(claude_auth::ATTEMPT_SECONDS + 60);
                    let mut liveness = tokio::time::interval(Duration::from_secs(2));
                    let mut delivered = false;
                    loop {
                        let request = tokio::select! { biased;
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep_until(deadline) => break,
                            // Child::wait would close the worker's stdin.
                            _ = liveness.tick() => {
                                if !matches!(child.try_wait(), Ok(None)) { break; }
                                continue;
                            },
                            request = requests.recv() => match request { Some(r) => r, None => break },
                        };
                        // A disconnected caller has no authority to dispatch a delayed action.
                        if request.reply.is_closed() {
                            break;
                        }
                        if delivered {
                            let _ = request.reply.send(Err(BrowserError::Expired));
                            continue;
                        }
                        let kind = request.kind;
                        let operation = async {
                            write_line(&mut child, &request.line).await?;
                            let limit = if kind == Kind::Poll {
                                MAX_FRAME_LINE
                            } else {
                                1024
                            };
                            let bytes = read_reply(reader, limit).await?;
                            parse_reply(&bytes, kind, &authorize)
                        };
                        let result = tokio::select! { biased;
                            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
                            _ = tokio::time::sleep_until(deadline) => Err(BrowserError::Expired),
                            result = tokio::time::timeout(Duration::from_secs(40), operation) => result.unwrap_or(Err(BrowserError::Timeout)),
                        };
                        let recoverable = matches!(
                            &result,
                            Err(BrowserError::StalePage
                                | BrowserError::InvalidHandle
                                | BrowserError::Capacity)
                        );
                        let terminal = result.is_err() && !recoverable;
                        delivered |= matches!(&result, Ok(Answer::Code(_)));
                        // An abandoned request's reply (even a code) is dropped and wiped,
                        // and ends the attempt: nothing stale is ever matched to a later one.
                        if request.reply.send(result).is_err() || terminal {
                            break;
                        }
                    }
                }
            }
        }
        // Further requests fail at once instead of waiting for cleanup to finish.
        requests.close();
        // The worker must exit zero after stopping Chrome and X; a killed worker is
        // never a cleanup receipt even if the container is then found removed.
        let cleanup = self.cleanup_grace(
            &mut child,
            &name,
            identity,
            observed,
            false,
            Some(WORKER_GRACE),
        );
        let drain = async {
            if let Some(mut reader) = reader {
                // Discard late frames/codes privately; never parse or deliver them.
                let mut bytes = Zeroizing::new(vec![0u8; 8192]);
                while let Ok(count) = reader.read(&mut bytes).await {
                    if count == 0 {
                        break;
                    }
                    bytes.fill(0);
                }
            }
        };
        tokio::pin!(cleanup, drain);
        // The existing worker grace bounds this drain too, including a writer that
        // never closes stdout. A failed/killed exit still cannot confirm cleanup.
        let cleanup = tokio::select! {
            result = &mut cleanup => result,
            _ = &mut drain => cleanup.await,
        }
        .map_err(|_| BrowserError::CleanupUnconfirmed);
        #[cfg(test)]
        if let Some(f) = &self.fixture {
            f.cleaned.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some((started, error)) = failed_start {
            let _ = started.send(Err(cleanup.err().unwrap_or(error)));
            return Ok(());
        }
        cleanup
    }
}

impl ClaudeAttempt {
    /// Resolves when the owning actor exits; does not consume cleanup proof.
    pub async fn closed(&self) {
        self.requests.closed().await;
    }
    async fn exchange(
        &mut self,
        kind: Kind,
        line: Zeroizing<Vec<u8>>,
    ) -> Result<Answer, BrowserError> {
        if self.cancel.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        // Dropping this future mid-request cancels the attempt (fail closed).
        let mut guard = Guard(Some(self.cancel.clone()));
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request { line, kind, reply })
            .await
            .map_err(|_| BrowserError::Expired)?;
        let answer = response.await.map_err(|_| BrowserError::Expired)?;
        guard.0 = None;
        answer
    }
    /// Next sanitized frame (origin-only URL), or, once, the captured `code#state`.
    /// `StalePage`/`Capacity`/`InvalidHandle` are recoverable: poll again. Any other
    /// error means the actor is already cleaning up; call `shutdown` for its outcome.
    /// Do not drop or time out this future to "try again": that cancels the attempt.
    pub async fn poll(&mut self) -> Result<ClaudeView, BrowserError> {
        if self.finished {
            return Err(BrowserError::Expired);
        }
        let line = Zeroizing::new(claude_auth::POLL_LINE.to_vec());
        match self.exchange(Kind::Poll, line).await? {
            Answer::Frame(frame) => Ok(ClaudeView::Frame(frame)),
            Answer::Code(code) => {
                self.finished = true;
                Ok(ClaudeView::Code(code))
            }
            Answer::Accepted => Err(BrowserError::Protocol),
        }
    }
    /// Human input bound to a frame token. A stale token (`StalePage`) dispatches
    /// nothing and is recoverable. Any other error leaves dispatch uncertain: it is
    /// never replayed and the attempt ends.
    pub async fn input(
        &mut self,
        frame_token: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError> {
        if self.finished {
            return Err(BrowserError::Expired);
        }
        input.validate()?;
        if !hex64(frame_token) {
            return Err(BrowserError::StalePage);
        }
        let line = claude_auth::input_line(frame_token, &input)?;
        drop(input);
        match self.exchange(Kind::Input, line).await? {
            Answer::Accepted => Ok(()),
            _ => Err(BrowserError::Protocol),
        }
    }
    /// Cancels and awaits the actor's graceful worker exit and exact container
    /// reconciliation. An error means cleanup is unconfirmed and must stay recoverable.
    pub async fn shutdown(mut self) -> Result<(), BrowserError> {
        self.cancel.cancel();
        self.done
            .take()
            .ok_or(BrowserError::Expired)?
            .await
            .map_err(|_| BrowserError::CleanupUnconfirmed)?
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::Engine;
    use std::sync::atomic::Ordering;

    const STATE: &str = "state_VALUE-123~abcdefgh";
    const CODE: &str = "CANARYcode123456";
    fn authorization() -> Zeroizing<String> {
        Zeroizing::new(format!(
            "https://claude.com/cai/oauth/authorize?response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&state={STATE}"
        ))
    }
    fn capsule_with(script: String) -> CapsuleReader {
        let mut reader = CapsuleReader::new(CapsuleConfig {
            engine: Default::default(),
            image_id: format!("sha256:{}", "a".repeat(64)),
            network_id: "b".repeat(64),
            proxy_container_id: "c".repeat(64),
            proxy_ip: "172.29.0.2".into(),
            owner: Uuid::new_v4(),
            project: "test-browser".into(),
            seccomp_file: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../deployment/browser/seccomp.json"),
            verified: true,
            smoke_evidence_sha256: "d".repeat(64),
            recover_workspaces: false,
            javascript_enabled: true,
        })
        .unwrap();
        reader.fixture = Some(Arc::new(Fixture {
            script,
            cleaned: 0.into(),
        }));
        reader
    }
    fn cleaned(reader: &CapsuleReader) -> usize {
        reader
            .fixture
            .as_ref()
            .unwrap()
            .cleaned
            .load(Ordering::SeqCst)
    }
    /// A fake worker: reads the init line, announces readiness (or `first`), then
    /// answers each request line with the next scripted reply, and exits 0 on the
    /// shutdown line or EOF (the cleanup receipt).
    fn worker(first: &str, replies: &[String]) -> String {
        let quote = |text: &str| format!("'{}'", text.replace('\'', "'\\''"));
        let mut script = format!("read -r init; printf '%s\\n' {}; ", quote(first));
        for reply in replies {
            script += &format!(
                "read -r req; case \"$req\" in *shutdown*) exit 0;; esac; printf '%s\\n' {}; ",
                quote(reply)
            );
        }
        script + "while read -r req; do case \"$req\" in *shutdown*) exit 0;; esac; done; exit 0"
    }
    fn ready(tenant: Uuid) -> String {
        json!({"status":"ready","tenant":tenant}).to_string()
    }
    fn frame_reply(url: &str) -> String {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1280u32.to_be_bytes());
        png.extend(900u32.to_be_bytes());
        json!({"ok":true,"frame":{
            "png_base64":base64::engine::general_purpose::STANDARD.encode(png),
            "width":1280,"height":900,"url":url,"frame_token":"a".repeat(64)}})
        .to_string()
    }
    fn error_reply(code: &str) -> String {
        json!({"ok":false,"error":code}).to_string()
    }
    fn key() -> WorkspaceInput {
        WorkspaceInput::Key {
            key: WorkspaceKey::Enter,
        }
    }

    #[tokio::test]
    async fn invalid_authorization_or_tenant_never_starts_a_worker() {
        let tenant = Uuid::new_v4();
        let good = authorization();
        for bad in [
            good.replace("https://claude.com", "https://claude.example"),
            good.replace(STATE, "x"),
            format!("{}&state=another_state_value", good.as_str()),
            "https://claude.com/cai/oauth/authorize?state=aaaaaaaa".to_owned(),
        ] {
            let reader = capsule_with(worker(&ready(tenant), &[]));
            assert!(matches!(
                reader
                    .start_claude(tenant, Zeroizing::new(bad), &CancellationToken::new())
                    .await,
                Err(BrowserError::InvalidUrl)
            ));
            assert_eq!(cleaned(&reader), 0);
        }
        let reader = capsule_with(worker(&ready(tenant), &[]));
        assert!(matches!(
            reader
                .start_claude(Uuid::nil(), authorization(), &CancellationToken::new())
                .await,
            Err(BrowserError::IsolationRequired)
        ));
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(matches!(
            reader
                .start_claude(tenant, authorization(), &cancelled)
                .await,
            Err(BrowserError::Cancelled)
        ));
        assert_eq!(cleaned(&reader), 0);
    }

    #[tokio::test]
    async fn frames_are_validated_stale_is_recoverable_and_shutdown_proves_cleanup() {
        let tenant = Uuid::new_v4();
        let reader = capsule_with(worker(
            &ready(tenant),
            &[
                frame_reply("https://claude.com"),
                error_reply("stale_page"),
                frame_reply("about:blank"),
            ],
        ));
        let mut attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        match attempt.poll().await.unwrap() {
            ClaudeView::Frame(frame) => assert_eq!(frame.url, "https://claude.com"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(attempt.poll().await, Err(BrowserError::StalePage)));
        assert!(matches!(
            attempt.poll().await.unwrap(),
            ClaudeView::Frame(_)
        ));
        attempt.shutdown().await.unwrap();
        assert_eq!(cleaned(&reader), 1);
    }

    #[tokio::test]
    async fn frames_with_a_path_extra_field_or_bad_image_end_the_attempt() {
        let tenant = Uuid::new_v4();
        let mut extra: serde_json::Value =
            serde_json::from_str(&frame_reply("https://claude.com")).unwrap();
        extra["frame"]["page_text"] = json!("secret");
        let mut small: serde_json::Value =
            serde_json::from_str(&frame_reply("https://claude.com")).unwrap();
        small["frame"]["png_base64"] = json!("AAAA");
        for reply in [
            frame_reply("https://claude.com/cai/oauth/authorize?state=x"),
            frame_reply("https://platform.claude.com/oauth/code/callback?code=SECRET"),
            extra.to_string(),
            small.to_string(),
            json!({"ok":true}).to_string(),
            json!({"ok":true,"accepted":true}).to_string(),
            json!({"ok":true,"code":"nohash123"}).to_string(),
            json!({"ok":false,"error":"stale_page","frame":null,"extra":1}).to_string(),
            "not json".to_owned(),
        ] {
            let reader = capsule_with(worker(&ready(tenant), std::slice::from_ref(&reply)));
            let mut attempt = reader
                .start_claude(tenant, authorization(), &CancellationToken::new())
                .await
                .unwrap();
            assert!(
                matches!(
                    attempt.poll().await,
                    Err(BrowserError::Protocol | BrowserError::OutputTooLarge)
                ),
                "{}",
                &reply[..reply.len().min(60)]
            );
            // Terminal: the actor is cleaning up, nothing more is served.
            assert!(attempt.poll().await.is_err());
            attempt.shutdown().await.unwrap();
            assert_eq!(cleaned(&reader), 1);
        }
    }

    #[tokio::test]
    async fn the_code_is_bound_to_this_attempts_state_and_handed_out_once() {
        let tenant = Uuid::new_v4();
        let reader = capsule_with(worker(
            &ready(tenant),
            &[json!({"ok":true,"code":format!("{CODE}#{STATE}")}).to_string()],
        ));
        let mut attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        let view = attempt.poll().await.unwrap();
        assert!(!format!("{view:?}").contains("CANARY"));
        let ClaudeView::Code(code) = view else {
            panic!("expected the code")
        };
        assert_eq!(code.as_str(), format!("{CODE}#{STATE}"));
        // Once only; the browser stays until the caller acknowledges shutdown.
        assert!(matches!(attempt.poll().await, Err(BrowserError::Expired)));
        assert!(matches!(
            attempt.input(&"a".repeat(64), key()).await,
            Err(BrowserError::Expired)
        ));
        assert_eq!(cleaned(&reader), 0);
        attempt.shutdown().await.unwrap();
        assert_eq!(cleaned(&reader), 1);

        // A code for another state is a protocol violation that ends the attempt.
        let reader = reader_for_wrong_state(tenant);
        let mut attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(attempt.poll().await, Err(BrowserError::Protocol)));
        attempt.shutdown().await.unwrap();
    }
    fn reader_for_wrong_state(tenant: Uuid) -> CapsuleReader {
        capsule_with(worker(
            &ready(tenant),
            &[json!({"ok":true,"code":format!("{CODE}#another_state_value")}).to_string()],
        ))
    }

    #[tokio::test]
    async fn input_is_acknowledged_stale_is_recoverable_and_uncertain_dispatch_is_not_replayed() {
        let tenant = Uuid::new_v4();
        let accepted = json!({"ok":true,"accepted":true}).to_string();
        let reader = capsule_with(worker(
            &ready(tenant),
            &[
                accepted.clone(),
                error_reply("stale_page"),
                error_reply("failed"),
                accepted,
            ],
        ));
        let mut attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        // Local validation never reaches the worker (the script has replies queued).
        assert!(matches!(
            attempt.input("not-a-token", key()).await,
            Err(BrowserError::StalePage)
        ));
        assert!(matches!(
            attempt
                .input(&"a".repeat(64), WorkspaceInput::Click { x: 1280, y: 0 })
                .await,
            Err(BrowserError::InvalidHandle)
        ));
        attempt
            .input(
                &"a".repeat(64),
                WorkspaceInput::InsertText {
                    text: "it's a \"pass\" word".into(),
                },
            )
            .await
            .unwrap();
        assert!(matches!(
            attempt.input(&"a".repeat(64), key()).await,
            Err(BrowserError::StalePage)
        ));
        // Unknown failure: dispatch may have happened. No replay, attempt over.
        assert!(matches!(
            attempt.input(&"a".repeat(64), key()).await,
            Err(BrowserError::Protocol)
        ));
        assert!(attempt.input(&"a".repeat(64), key()).await.is_err());
        attempt.shutdown().await.unwrap();
        assert_eq!(cleaned(&reader), 1);
    }

    #[tokio::test]
    async fn dropping_the_attempt_cancels_and_reaps_the_actor() {
        let tenant = Uuid::new_v4();
        let reader = capsule_with(worker(&ready(tenant), &[]));
        let attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        drop(attempt);
        for _ in 0..100 {
            if cleaned(&reader) == 1 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("abandoned attempt leaked its actor");
    }

    #[tokio::test]
    async fn abandoned_request_cancels_instead_of_adopting_a_stale_reply() {
        let tenant = Uuid::new_v4();
        // The worker never answers the poll.
        let reader = capsule_with(format!(
            "read -r init; printf '%s\\n' '{}'; read -r req; exec /bin/sleep 30",
            ready(tenant)
        ));
        let mut attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        assert!(
            tokio::time::timeout(Duration::from_millis(300), attempt.poll())
                .await
                .is_err()
        );
        // The dropped poll cancelled the attempt; the actor reaps (killed worker is
        // honestly reported as unconfirmed, never as a clean receipt).
        assert!(matches!(attempt.poll().await, Err(BrowserError::Cancelled)));
        assert!(matches!(
            attempt.shutdown().await,
            Err(BrowserError::CleanupUnconfirmed)
        ));
        assert_eq!(cleaned(&reader), 1);
    }

    #[tokio::test]
    async fn cancellation_drains_an_inflight_frame_before_worker_exit() {
        let tenant = Uuid::new_v4();
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("poll-started");
        let reader = capsule_with(format!(
            "read -r init; printf '%s\\n' '{}'; read -r req; touch '{}'; \
             sleep 0.2; head -c 262144 /dev/zero || exit 9; \
             while read -r req; do case \"$req\" in *shutdown*) exit 0;; esac; done; exit 0",
            ready(tenant),
            marker.display()
        ));
        let cancel = CancellationToken::new();
        let mut attempt = reader
            .start_claude(tenant, authorization(), &cancel)
            .await
            .unwrap();
        let trigger = cancel.clone();
        let cancellation = tokio::spawn(async move {
            tokio::time::timeout(Duration::from_secs(3), async {
                while !marker.exists() {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            trigger.cancel();
        });
        assert!(matches!(attempt.poll().await, Err(BrowserError::Cancelled)));
        cancellation.await.unwrap();
        attempt.shutdown().await.unwrap();
        assert_eq!(cleaned(&reader), 1);
    }

    #[tokio::test]
    async fn failed_wrong_or_cancelled_startup_is_reaped_before_start_returns() {
        let tenant = Uuid::new_v4();
        for first in [
            "garbage".to_owned(),
            json!({"status":"unavailable"}).to_string(),
            ready(Uuid::new_v4()),
            json!({"status":"ready","tenant":tenant,"url":"x"}).to_string(),
        ] {
            let reader = capsule_with(worker(&first, &[]));
            assert!(
                reader
                    .start_claude(tenant, authorization(), &CancellationToken::new())
                    .await
                    .is_err(),
                "{first}"
            );
            assert_eq!(cleaned(&reader), 1);
        }
        // Cancelled while the worker is still starting.
        let reader = capsule_with("read -r init; exec /bin/sleep 30".to_owned());
        let cancel = CancellationToken::new();
        let trigger = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(200)).await;
            trigger.cancel();
        });
        assert!(matches!(
            reader.start_claude(tenant, authorization(), &cancel).await,
            Err(BrowserError::CleanupUnconfirmed)
        ));
        assert_eq!(cleaned(&reader), 1);
    }

    #[tokio::test]
    async fn oversized_input_acknowledgement_ends_the_attempt() {
        let tenant = Uuid::new_v4();
        let reader = capsule_with(worker(&ready(tenant), &["x".repeat(2000)]));
        let mut attempt = reader
            .start_claude(tenant, authorization(), &CancellationToken::new())
            .await
            .unwrap();
        assert!(matches!(
            attempt.input(&"a".repeat(64), key()).await,
            Err(BrowserError::OutputTooLarge)
        ));
        attempt.shutdown().await.unwrap();
    }

    #[tokio::test]
    async fn dropping_start_mid_flight_still_reaps_and_releases_capacity() {
        let tenant = Uuid::new_v4();
        let reader = capsule_with("read -r init; exec /bin/sleep 30".to_owned());
        let cancel = CancellationToken::new();
        let start = reader.start_claude(tenant, authorization(), &cancel);
        // The caller gives up while the worker is still starting.
        assert!(
            tokio::time::timeout(Duration::from_millis(300), start)
                .await
                .is_err()
        );
        for _ in 0..100 {
            if cleaned(&reader) == 1 && reader.capacity.available_permits() == 2 {
                return;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!("abandoned startup leaked its actor or capacity");
    }
}
