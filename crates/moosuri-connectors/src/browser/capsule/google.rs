//! Installation-only transport for one sealed Google authentication attempt.
//! Reuses the capsule's pinned engine, network, limits and cleanup ownership.
use super::*;
use crate::browser::google_auth::{Binding, Envelope, PROTOCOL, Recipient};
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

struct Request {
    body: Option<Value>,
    reply: oneshot::Sender<Result<GooglePhase, BrowserError>>,
}

pub struct GoogleAttempt {
    recipient: Recipient,
    requests: mpsc::Sender<Request>,
    cancel: CancellationToken,
    done: Option<oneshot::Receiver<Result<(), BrowserError>>>,
    phase: Option<GooglePhase>,
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
        match setup {
            Err(error) => failed_start = Some((started, error)),
            Ok((recipient, mut reader)) => {
                observed = true;
                if started.send(Ok(recipient)).is_ok() {
                    let deadline = tokio::time::Instant::now() + Duration::from_secs(22 * 60);
                    loop {
                        let request = tokio::select! { biased;
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep_until(deadline) => break,
                            request = requests.recv() => match request { Some(r) => r, None => break },
                        };
                        if request.reply.is_closed() {
                            break;
                        }
                        let operation = async {
                            if let Some(body) = request.body {
                                write(&mut child, &body).await?;
                            }
                            let reply: PhaseReply =
                                serde_json::from_value(read_line(&mut reader, 256).await?)
                                    .map_err(|_| BrowserError::Protocol)?;
                            Ok(reply.status)
                        };
                        let result = tokio::select! { biased;
                            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
                            _ = tokio::time::sleep_until(deadline) => Err(BrowserError::Expired),
                            result = tokio::time::timeout(Duration::from_secs(250), operation) => result.unwrap_or(Err(BrowserError::Timeout)),
                        };
                        if result.as_ref().is_ok_and(|p: &GooglePhase| p.terminal()) {
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
            .cleanup(&mut child, &name, identity, observed, false)
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
            .send(Request { body, reply })
            .await
            .map_err(|_| BrowserError::Expired)?;
        let result = response.await.map_err(|_| BrowserError::Expired)??;
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
        if self.phase != Some(GooglePhase::SignedIn) {
            return Err(BrowserError::Protocol);
        }
        self.exchange(
            Some(json!({"url":url,"client_id":client_id,"service":service})),
            &[GooglePhase::Returned],
        )
        .await
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
