//! Installation-only transport for one sealed Google authentication attempt.
//! Reuses the capsule's pinned engine, network, limits and cleanup ownership.
use super::*;
use crate::browser::google_auth::{
    Binding, Envelope, PROTOCOL, Recipient, bootstrap,
    bootstrap::{BOOTSTRAP_SHUTDOWN_GRACE, Reply as BootstrapReply, Request as BootstrapRequest},
};
use tokio::sync::{mpsc, oneshot};

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

/// One worker answer: a phase, or (only for a bootstrap request) its typed reply.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Answer {
    Phase(GooglePhase),
    Bootstrap(BootstrapReply),
}

/// Result of a bootstrap command: its typed reply, or the worker's existing
/// terminal status (blocked / needs_human / unavailable).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BootstrapOutcome {
    Reply(BootstrapReply),
    Terminal(GooglePhase),
}

/// Bounded, exact-shape parse of one worker line. A bootstrap request accepts its
/// typed reply or a terminal status only; any other request accepts phases only.
fn parse_answer(value: Value, bootstrap: bool) -> Result<Answer, BrowserError> {
    if value
        .as_object()
        .is_some_and(|m| m.contains_key("bootstrap"))
    {
        if !bootstrap {
            return Err(BrowserError::Protocol);
        }
        return BootstrapReply::from_wire(&value)
            .map(Answer::Bootstrap)
            .map_err(|_| BrowserError::Protocol);
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
        let setup = async {
            let mut reader = BufReader::new(child.stdout.take().ok_or(BrowserError::Setup)?);
            write(&mut child, &json!({"protocol":PROTOCOL,"binding":binding})).await?;
            let value = read_line(&mut reader, 4096).await?;
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
            Ok((recipient, reader))
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
        match setup {
            Err(error) => failed_start = Some((started, error)),
            Ok((recipient, mut reader)) => {
                observed = true;
                if started.send(Ok(recipient)).is_ok() {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(22 * 60);
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
                            parse_answer(read_line(&mut reader, 256).await?, bootstrap)
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
        let cleanup = self
            .cleanup_grace(
                &mut child,
                &name,
                identity,
                observed,
                false,
                bootstrap_sent.then_some(BOOTSTRAP_SHUTDOWN_GRACE),
            )
            .await;
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

    #[test]
    fn bootstrap_answers_are_exact_and_not_passthrough() {
        let ready = json!({"bootstrap":"ready"});
        assert_eq!(
            parse_answer(ready.clone(), true).unwrap(),
            Answer::Bootstrap(BootstrapReply::Ready)
        );
        assert_eq!(
            parse_answer(json!({"bootstrap":"absent"}), true).unwrap(),
            Answer::Bootstrap(BootstrapReply::Absent)
        );
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
