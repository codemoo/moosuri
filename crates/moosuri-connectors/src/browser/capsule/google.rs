//! Installation-only transport for one sealed Google authentication attempt.
//! Reuses the capsule's pinned engine, network, limits and cleanup ownership.
use super::*;
use crate::browser::google_auth::{
    Binding, Envelope, PROTOCOL, Recipient, bootstrap,
    bootstrap::{
        BOOTSTRAP_SHUTDOWN_GRACE, PRIVATE_WIRE_LIMIT, Request as BootstrapRequest,
        Response as BootstrapResponse,
    },
};
use tokio::sync::{mpsc, oneshot};
use zeroize::Zeroizing;

#[derive(Clone, Copy, PartialEq, Eq, Debug, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum GooglePhase {
    Verified,
    Verifying,
    SignedIn,
    Returned,
    Blocked,
    NeedsHuman,
    Unavailable,
}
impl GooglePhase {
    fn terminal(self) -> bool {
        matches!(
            self,
            Self::Returned | Self::Blocked | Self::NeedsHuman | Self::Unavailable
        )
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PhaseReply {
    status: GooglePhase,
}

/// One worker answer: a phase, or (only for a bootstrap request) its typed
/// response. The response may carry a one-time OAuth client secret: Debug is
/// redacted and the type is deliberately not Clone.
#[derive(Debug)]
enum Answer {
    Phase(GooglePhase),
    Bootstrap(BootstrapResponse),
}

/// Result of a bootstrap command: its typed response (PRIVATE when it holds a
/// client secret; the caller must never log it or put it in a web response), or the
/// worker's existing terminal status (blocked / needs_human / unavailable).
#[derive(Debug)]
pub enum BootstrapOutcome {
    Reply(BootstrapResponse),
    Terminal(GooglePhase),
}

/// Bounded, exact-shape parse of one worker line. A bootstrap request accepts its
/// typed response or a terminal status only; any other request accepts phases only.
fn parse_answer(mut value: Value, bootstrap: bool) -> Result<Answer, BrowserError> {
    if value
        .as_object()
        .is_some_and(|m| m.contains_key("bootstrap"))
    {
        let parsed = if bootstrap {
            BootstrapResponse::from_wire(&value)
                .map(Answer::Bootstrap)
                .map_err(|_| BrowserError::Protocol)
        } else {
            Err(BrowserError::Protocol)
        };
        // The parsed line may have held the client secret.
        bootstrap::scrub(&mut value);
        return parsed;
    }
    let phase = serde_json::from_value::<PhaseReply>(value)
        .map_err(|_| BrowserError::Protocol)?
        .status;
    if bootstrap
        && !matches!(
            phase,
            GooglePhase::Blocked | GooglePhase::NeedsHuman | GooglePhase::Unavailable
        )
    {
        return Err(BrowserError::Protocol);
    }
    Ok(Answer::Phase(phase))
}

struct Request {
    body: Option<Value>,
    bootstrap: bool,
    reply: oneshot::Sender<Result<Answer, BrowserError>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Bootstrapping {
    No,
    Active,
    Finished,
}

pub struct GoogleAttempt {
    recipient: Recipient,
    requests: mpsc::Sender<Request>,
    cancel: CancellationToken,
    done: Option<oneshot::Receiver<Result<(), BrowserError>>>,
    phase: Option<GooglePhase>,
    bootstrapping: Bootstrapping,
}
struct Pending(Option<CancellationToken>);
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(token) = self.0.take() {
            token.cancel();
        }
    }
}
impl Drop for GoogleAttempt {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

impl CapsuleReader {
    pub async fn start_google(
        &self,
        binding: Binding,
        cancel: &CancellationToken,
    ) -> Result<GoogleAttempt, BrowserError> {
        if self.config.engine != Engine::Kiri
            || self.config.recover_workspaces
            || !binding.valid()
            || binding.installation != self.config.owner
            || !self.config.javascript_enabled
        {
            return Err(BrowserError::IsolationRequired);
        }
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| BrowserError::Capacity)?;
        let _slot_lock = self.member_start.lock().await;
        self.config.validate()?;
        self.validate_resources(cancel).await?;
        empty_docker_config(Path::new(DOCKER_CONFIG))?;
        // Engine inventory also bounds admission after a service restart. An
        // orphan is never silently adopted or evicted to make room.
        let prefix = format!("moosuri-google-{}-", self.config.owner.simple());
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
        let name = workspace::free_member_slot(&prefix, &names)?;
        let cancel = cancel.child_token();
        let mut pending = Pending(Some(cancel.clone()));
        let (requests, rx) = mpsc::channel(1);
        let (started, startup) = oneshot::channel();
        let (finished, done) = oneshot::channel();
        let this = self.clone();
        let control = cancel.clone();
        tokio::spawn(async move {
            let _capacity = capacity;
            let result = this.google_actor(binding, name, control, rx, started).await;
            let _ = finished.send(result);
        });
        let recipient = startup.await.map_err(|_| BrowserError::Setup)??;
        pending.0 = None;
        Ok(GoogleAttempt {
            recipient,
            requests,
            cancel,
            done: Some(done),
            phase: None,
            bootstrapping: Bootstrapping::No,
        })
    }

    async fn google_actor(
        &self,
        binding: Binding,
        name: String,
        cancel: CancellationToken,
        mut requests: mpsc::Receiver<Request>,
        started: oneshot::Sender<Result<Recipient, BrowserError>>,
    ) -> Result<(), BrowserError> {
        let identity = Uuid::new_v4();
        let mut command = self.worker_command_mode(binding.tenant, identity, &name, None, true);
        command
            .arg("--google-auth")
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
        let mut observed = false;
        // Cancellation can race a final bootstrap reply. Keep stdout owned until
        // cleanup finishes, so Docker can report the worker's actual exit status.
        let mut reader = child.stdout.take().map(BufReader::new);
        let setup = async {
            let reader = reader.as_mut().ok_or(BrowserError::Setup)?;
            write(&mut child, &json!({"protocol":PROTOCOL,"binding":binding})).await?;
            let value = read_line(reader, 4096).await?;
            if value.as_object().is_none_or(|m| m.len() != 2) || value["status"] != "vault" {
                return Err(BrowserError::Protocol);
            }
            let recipient: Recipient = serde_json::from_value(value["recipient"].clone())
                .map_err(|_| BrowserError::Protocol)?;
            use base64::Engine;
            if recipient.binding != binding
                || recipient.protocol != PROTOCOL
                || base64::engine::general_purpose::STANDARD
                    .decode(&recipient.spki)
                    .ok()
                    .is_none_or(|p| p.len() != 422)
            {
                return Err(BrowserError::Protocol);
            }
            Ok(recipient)
        };
        let setup = tokio::select! { biased;
            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(45), setup) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        let mut failed_start = None;
        let mut terminal_reply = None;
        // Once any bootstrap command has been sent the worker may be inside SDK
        // credential cleanup, so shutdown gets the long grace instead of the short one.
        let mut bootstrap_sent = false;
        match (setup, reader.as_mut()) {
            (Err(error), _) => failed_start = Some((started, error)),
            (Ok(_), None) => failed_start = Some((started, BrowserError::Setup)),
            (Ok(recipient), Some(reader)) => {
                observed = true;
                if started.send(Ok(recipient)).is_ok() {
                    let deadline = tokio::time::Instant::now() + bootstrap::ATTEMPT_LIFETIME;
                    let mut liveness = tokio::time::interval(Duration::from_secs(2));
                    loop {
                        let request = tokio::select! { biased;
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep_until(deadline) => break,
                            // Child::wait closes its stdin before waiting. This
                            // worker's stdin must remain open for later Deliver.
                            _ = liveness.tick() => {
                                if !matches!(child.try_wait(), Ok(None)) { break; }
                                continue;
                            },
                            request = requests.recv() => match request { Some(r) => r, None => break },
                        };
                        if request.reply.is_closed() {
                            break;
                        }
                        let bootstrap = request.bootstrap;
                        bootstrap_sent |= bootstrap;
                        let operation = async {
                            if let Some(body) = request.body {
                                write(&mut child, &body).await?;
                            }
                            // Bootstrap lines may carry the client secret and are
                            // bounded by the private wire limit; phases stay tiny.
                            let limit = if bootstrap { PRIVATE_WIRE_LIMIT } else { 256 };
                            parse_answer(read_line(reader, limit).await?, bootstrap)
                        };
                        // The worker's longest bootstrap action is bounded at 600 s; an
                        // abandoned wait still ends in the awaited graceful cleanup below.
                        let limit = Duration::from_secs(if bootstrap { 660 } else { 250 });
                        let result = tokio::select! { biased;
                            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
                            _ = tokio::time::sleep_until(deadline) => Err(BrowserError::Expired),
                            result = tokio::time::timeout(limit, operation) => result.unwrap_or(Err(BrowserError::Timeout)),
                        };
                        if result
                            .as_ref()
                            .is_ok_and(|a| matches!(a, Answer::Phase(p) if p.terminal()))
                        {
                            terminal_reply = Some((request.reply, result));
                            break;
                        }
                        let failed = result.is_err();
                        if request.reply.send(result).is_err() || failed {
                            break;
                        }
                    }
                }
            }
        }
        let cleanup = self.cleanup_grace(
            &mut child,
            &name,
            identity,
            observed,
            false,
            bootstrap_sent.then_some(BOOTSTRAP_SHUTDOWN_GRACE),
        );
        let drain = async {
            if let Some(mut reader) = reader {
                // Late OAuth replies may contain a secret. Discard them privately;
                // no late reply is parsed or delivered after cancellation.
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
        // The existing shutdown grace bounds the drain. SDK cleanup still needs a
        // successful worker exit; EOF or container removal alone is not that proof.
        let cleanup = tokio::select! {
            result = &mut cleanup => result,
            _ = &mut drain => cleanup.await,
        };
        if let Some((started, error)) = failed_start {
            let _ = started.send(Err(cleanup.err().unwrap_or(error)));
            return Ok(());
        }
        if let Some((reply, result)) = terminal_reply {
            let _ = reply.send(if cleanup.is_ok() {
                result
            } else {
                Err(BrowserError::CleanupUnconfirmed)
            });
        }
        cleanup
    }
}

async fn write(child: &mut Child, body: &Value) -> Result<(), BrowserError> {
    let mut bytes = serde_json::to_vec(body).map_err(|_| BrowserError::Protocol)?;
    if bytes.len() > 12 * 1024 {
        return Err(BrowserError::OutputTooLarge);
    }
    bytes.push(b'\n');
    let input = child.stdin.as_mut().ok_or(BrowserError::Protocol)?;
    input
        .write_all(&bytes)
        .await
        .map_err(|_| BrowserError::Protocol)?;
    input.flush().await.map_err(|_| BrowserError::Protocol)
}
impl GoogleAttempt {
    /// Resolves when the owning actor exits; does not consume cleanup proof.
    pub async fn closed(&self) {
        self.requests.closed().await;
    }
    pub fn recipient(&self) -> &Recipient {
        &self.recipient
    }
    async fn exchange(
        &mut self,
        body: Option<Value>,
        expected: &[GooglePhase],
    ) -> Result<GooglePhase, BrowserError> {
        let mut pending = Pending(Some(self.cancel.clone()));
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request {
                body,
                bootstrap: false,
                reply,
            })
            .await
            .map_err(|_| BrowserError::Expired)?;
        let Answer::Phase(result) = response.await.map_err(|_| BrowserError::Expired)?? else {
            return Err(BrowserError::Protocol);
        };
        if !expected.contains(&result)
            && !matches!(
                result,
                GooglePhase::Blocked | GooglePhase::NeedsHuman | GooglePhase::Unavailable
            )
        {
            return Err(BrowserError::Protocol);
        }
        self.phase = Some(result);
        pending.0 = None;
        Ok(result)
    }
    pub async fn deliver(
        &mut self,
        email: &str,
        envelope: Envelope,
    ) -> Result<GooglePhase, BrowserError> {
        if self.phase.is_some() {
            return Err(BrowserError::Protocol);
        }
        self.exchange(
            Some(json!({"google_email":email,"envelope":envelope})),
            &[GooglePhase::Verified],
        )
        .await
    }
    pub async fn proceed(&mut self) -> Result<GooglePhase, BrowserError> {
        if self.phase != Some(GooglePhase::Verified) {
            return Err(BrowserError::Protocol);
        }
        self.exchange(
            Some(json!({"proceed":self.recipient.binding.challenge})),
            &[GooglePhase::Verifying, GooglePhase::SignedIn],
        )
        .await
    }
    pub async fn verification(&mut self) -> Result<GooglePhase, BrowserError> {
        if self.phase != Some(GooglePhase::Verifying) {
            return Err(BrowserError::Protocol);
        }
        self.exchange(None, &[GooglePhase::SignedIn]).await
    }
    pub async fn consent(
        &mut self,
        url: &str,
        client_id: &str,
        service: &str,
    ) -> Result<GooglePhase, BrowserError> {
        // Consent is valid before any bootstrap or after its Finish, never between.
        if self.phase != Some(GooglePhase::SignedIn) || self.bootstrapping == Bootstrapping::Active
        {
            return Err(BrowserError::Protocol);
        }
        self.exchange(
            Some(json!({"url":url,"client_id":client_id,"service":service})),
            &[GooglePhase::Returned],
        )
        .await
    }
    /// One fixed bootstrap command after `SignedIn`. The first must be `Authorize`
    /// (once); the worker enforces the full sequence, and this side mirrors the
    /// parts it can: nothing after `Finish`, and a failed or terminal exchange ends
    /// the attempt. The caller (gateway) must already hold durable intent and a fresh
    /// admin heartbeat; this transport establishes neither.
    pub async fn bootstrap(
        &mut self,
        request: BootstrapRequest,
    ) -> Result<BootstrapOutcome, BrowserError> {
        use bootstrap::Action;
        let binding = &self.recipient.binding;
        if self.phase != Some(GooglePhase::SignedIn)
            || self.bootstrapping == Bootstrapping::Finished
            || (self.bootstrapping == Bootstrapping::No) != (request.action == Action::Authorize)
            || request.validate(binding).is_err()
        {
            return Err(BrowserError::Protocol);
        }
        let mut pending = Pending(Some(self.cancel.clone()));
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request {
                body: Some(request.to_wire()),
                bootstrap: true,
                reply,
            })
            .await
            .map_err(|_| BrowserError::Expired)?;
        let answer = response.await.map_err(|_| BrowserError::Expired)??;
        let outcome = match answer {
            // A response of the wrong kind or bound to another setup/callback is a
            // protocol violation; dropping `pending` cancels (and cleans up) the
            // attempt. The secret-bearing value is dropped (zeroized) here.
            Answer::Bootstrap(reply) if !reply.answers(&request) => {
                return Err(BrowserError::Protocol);
            }
            Answer::Bootstrap(reply) => BootstrapOutcome::Reply(reply),
            Answer::Phase(phase) => {
                self.phase = Some(phase);
                BootstrapOutcome::Terminal(phase)
            }
        };
        if matches!(outcome, BootstrapOutcome::Reply(_)) {
            self.bootstrapping = if request.action == Action::Finish {
                Bootstrapping::Finished
            } else {
                Bootstrapping::Active
            };
        }
        pending.0 = None;
        Ok(outcome)
    }
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
mod google_bootstrap_tests {
    use super::*;

    async fn cancel_with_late_reply(exit: u8) -> Result<(), BrowserError> {
        use base64::Engine;
        let binding = Binding {
            installation: Uuid::new_v4(),
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
            epoch: 1,
            challenge: Uuid::new_v4(),
            email: "owner@example.test".into(),
            origin: "https://assistant.example.test".into(),
        };
        let directory = tempfile::tempdir().unwrap();
        let marker = directory.path().join("bootstrap-started");
        let ready = json!({"status":"vault","recipient":{
            "protocol":PROTOCOL,"binding":binding,
            "spki":base64::engine::general_purpose::STANDARD.encode([0u8;422])
        }});
        // Only emit the late reply after the host writes shutdown: no timing race
        // decides whether the original stdout reader has already been dropped.
        let script = format!(
            "read -r init; printf '%s\\n' '{}'; read -r req; touch '{}'; \
             read -r stop; case \"$stop\" in *shutdown*) ;; *) exit 8;; esac; \
             printf '%s\\n' '{{\"status\":\"unavailable\"}}' || exit 9; exit {exit}",
            ready,
            marker.display()
        );
        let mut reader = CapsuleReader::new(CapsuleConfig {
            engine: super::super::Engine::Kiri,
            image_id: format!("sha256:{}", "a".repeat(64)),
            network_id: "b".repeat(64),
            proxy_container_id: "c".repeat(64),
            proxy_ip: "172.29.0.2".into(),
            owner: binding.installation,
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
        let cancel = CancellationToken::new();
        let (requests, incoming) = mpsc::channel(1);
        let (started, startup) = oneshot::channel();
        let control = cancel.clone();
        let actor = tokio::spawn(async move {
            reader
                .google_actor(
                    binding,
                    "synthetic-google".into(),
                    control,
                    incoming,
                    started,
                )
                .await
        });
        startup.await.unwrap().unwrap();
        let (reply, response) = oneshot::channel();
        requests
            .send(Request {
                body: Some(json!({"synthetic_bootstrap":true})),
                bootstrap: true,
                reply,
            })
            .await
            .unwrap();
        tokio::time::timeout(Duration::from_secs(3), async {
            while !marker.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        cancel.cancel();
        assert!(matches!(
            response.await.unwrap(),
            Err(BrowserError::Cancelled)
        ));
        actor.await.unwrap()
    }

    #[tokio::test]
    async fn cancelled_bootstrap_drains_late_status_before_accepting_exit() {
        cancel_with_late_reply(0).await.unwrap();
    }

    #[tokio::test]
    async fn drained_late_status_does_not_override_failed_sdk_cleanup() {
        assert!(matches!(
            cancel_with_late_reply(7).await,
            Err(BrowserError::CleanupUnconfirmed)
        ));
    }

    #[test]
    fn bootstrap_answers_are_exact_and_not_passthrough() {
        use bootstrap::Reply;
        let ready = json!({"bootstrap":"ready"});
        assert!(matches!(
            parse_answer(ready.clone(), true).unwrap(),
            Answer::Bootstrap(BootstrapResponse::Status(Reply::Ready))
        ));
        assert!(matches!(
            parse_answer(json!({"bootstrap":"absent"}), true).unwrap(),
            Answer::Bootstrap(BootstrapResponse::Status(Reply::Absent))
        ));
        // Private client lines are typed, exact and redacted in Debug.
        let setup = Uuid::new_v4();
        let created = json!({"bootstrap":{"client_created":{
            "setup_id":setup,
            "client_id":"123-abc.apps.googleusercontent.com",
            "redirect_uri":"https://moosuri.example.test/api/v1/mail/oauth/callback",
            "client_secret":"GOCSPX-canary-secret-value"}}});
        let answer = parse_answer(created.clone(), true).unwrap();
        assert!(!format!("{answer:?}").contains("canary"));
        assert!(matches!(
            answer,
            Answer::Bootstrap(BootstrapResponse::Client(_))
        ));
        assert!(parse_answer(created, false).is_err());
        // A bootstrap reply to a non-bootstrap request is a protocol error.
        assert!(parse_answer(ready, false).is_err());
        for bad in [
            json!({"bootstrap":"ready","extra":1}),
            json!({"bootstrap":{"project":"x"}}),
            json!({"bootstrap":"maybe"}),
            json!({"bootstrap":null}),
        ] {
            assert!(parse_answer(bad, true).is_err());
        }
        // Terminal worker statuses are the only phases a bootstrap request accepts.
        for status in ["blocked", "needs_human", "unavailable"] {
            assert!(matches!(
                parse_answer(json!({"status":status}), true),
                Ok(Answer::Phase(_))
            ));
        }
        for status in ["signed_in", "verified", "returned", "verifying"] {
            assert!(parse_answer(json!({"status":status}), true).is_err());
            assert!(parse_answer(json!({"status":status}), false).is_ok());
        }
        assert!(parse_answer(json!({"status":"blocked","note":"x"}), true).is_err());
    }
}
