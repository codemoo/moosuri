//! HTTPS origin pin, no proxy/redirect/cookie inheritance, signed device commands.
use crate::*;
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
#[derive(Clone)]
pub struct Transport {
    client: reqwest::Client,
    origin: String,
    endpoint: String,
}
impl Transport {
    pub fn new(origin: &str) -> Result<Self> {
        if !auth::valid_origin(origin) {
            return Err(Error::Authentication);
        }
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| Error::PrivateState)?,
            origin: origin.into(),
            endpoint: origin.into(),
        })
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn synthetic_loopback(origin: &str, endpoint: &str) -> Result<Self> {
        let url = url::Url::parse(endpoint).map_err(|_| Error::Policy)?;
        if url.scheme() != "http"
            || !matches!(url.host_str(), Some("127.0.0.1" | "localhost"))
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.origin().ascii_serialization() != endpoint
        {
            return Err(Error::Policy);
        }
        let mut transport = Self::new(origin)?;
        transport.endpoint = endpoint.into();
        Ok(transport)
    }
    pub async fn post<I: Serialize, O: DeserializeOwned>(
        &self,
        path: &str,
        input: &I,
    ) -> Result<O> {
        if !matches!(
            path,
            "/api/v1/credential-broker/device/challenge"
                | "/api/v1/credential-broker/device/pair"
                | DEVICE_PATH
        ) {
            return Err(Error::Policy);
        }
        let body = zeroize::Zeroizing::new(serde_json::to_vec(input).map_err(|_| Error::Policy)?);
        if body.len() > MAX_SIGNED_BYTES {
            return Err(Error::Policy);
        }
        let mut r = self
            .client
            .post(format!("{}{path}", self.endpoint))
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        if !r.status().is_success() {
            return Err(if matches!(r.status().as_u16(), 401 | 403) {
                Error::Authentication
            } else {
                Error::Unavailable
            });
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk) = r.chunk().await.map_err(|_| Error::Unavailable)? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(Error::Authentication);
            }
            bytes.extend(chunk);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Authentication)
    }
    pub async fn command(&self, engine: &Engine, command: Command) -> Result<Reply> {
        if engine.origin() != self.origin {
            return Err(Error::Authentication);
        }
        let c: AuthChallenge = self
            .post(
                "/api/v1/credential-broker/device/challenge",
                &serde_json::json!({"device":engine.device().id}),
            )
            .await?;
        let signed = engine.sign_command(&c, "POST", DEVICE_PATH, command, current_time()?)?;
        self.post(DEVICE_PATH, &signed).await
    }
    pub async fn sync_retained(&self, engine: &mut Engine) -> Result<()> {
        for request in engine.retained() {
            let invocation = request.invocation;
            if let Some(receipt) = engine.receipt(&invocation)? {
                self.command(
                    engine,
                    Command::Settle {
                        receipt: Box::new(receipt),
                    },
                )
                .await?;
                engine.delivered(invocation)?;
                continue;
            }
            if let Ok(Reply::Commitment { commitment }) = self
                .command(
                    engine,
                    Command::Recover {
                        invocation: invocation.clone(),
                    },
                )
                .await
            {
                let _ = engine.accept_commitment(*commitment);
                if let Ok(receipt) = engine.settle_without_retry(&invocation) {
                    self.command(
                        engine,
                        Command::Settle {
                            receipt: Box::new(receipt),
                        },
                    )
                    .await?;
                    engine.delivered(invocation)?;
                }
            } else if matches!(
                self.command(
                    engine,
                    Command::Abandon {
                        invocation: invocation.clone()
                    },
                )
                .await,
                Ok(Reply::Abandoned)
            ) {
                engine.delivered(invocation)?;
            }
        }
        Ok(())
    }
    async fn waiting(&self, engine: &Engine) -> Result<(Vec<Request>, bool)> {
        let native = engine.grants().iter().any(|g| g.google_send());
        let mut requests = Vec::new();
        let mut revoke = false;
        if native {
            let Reply::Waiting {
                requests: mail,
                revocation_pending,
            } = self.command(engine, Command::PollMail).await?
            else {
                return Err(Error::Authentication);
            };
            requests.extend(mail);
            revoke |= revocation_pending;
        }
        match self.command(engine, Command::Poll).await {
            Ok(Reply::Waiting {
                requests: read,
                revocation_pending,
            }) => {
                requests.extend(read);
                revoke |= revocation_pending;
            }
            Err(Error::Authentication) if native => {}
            Err(e) => return Err(e),
            _ => return Err(Error::Authentication),
        }
        Ok((requests, revoke))
    }
    async fn mail_payload(&self, engine: &mut Engine, invocation: &Invocation) -> Result<()> {
        let request = engine
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request
            .clone();
        if !matches!(
            request.operation,
            Operation::GoogleMailSend { .. } | Operation::GoogleMailLookup { .. }
        ) {
            return Ok(());
        }
        let Reply::MailPayload { payload } = self
            .command(
                engine,
                Command::MailPayload {
                    invocation: invocation.clone(),
                    request_digest: auth::hash(&request_bytes(&request)),
                },
            )
            .await?
        else {
            return Err(Error::Authentication);
        };
        engine.attach_mail_payload(invocation, *payload)
    }
    pub async fn register_live(
        &self,
        engine: &mut Engine,
        binding: &Binding,
        repository: &GitHubRepository,
    ) -> Result<()> {
        let grant = engine
            .grants()
            .iter()
            .find(|g| {
                &g.binding == binding && g.repository() == Some(repository) && g.live().is_some()
            })
            .ok_or(Error::Policy)?;
        let refresh = grant.pending_refresh().cloned();
        let command = match &refresh {
            Some(expected) => Command::RefreshRepository {
                expected: expected.clone(),
                binding: binding.clone(),
                repository: repository.clone(),
            },
            None => Command::RegisterRepository {
                binding: binding.clone(),
                repository: repository.clone(),
            },
        };
        if !matches!(self.command(engine, command).await?, Reply::Registered) {
            return Err(Error::Authentication);
        }
        if refresh.is_some() {
            engine.confirm_live_refresh(binding)?;
        }
        Ok(())
    }
    pub async fn register_google(&self, engine: &mut Engine, binding: &Binding) -> Result<()> {
        let grant = engine
            .grants()
            .iter()
            .find(|g| &g.binding == binding && g.google().is_some())
            .ok_or(Error::Policy)?;
        let preview = grant.google_preview();
        let refresh = grant.pending_refresh().cloned();
        let command = if grant.google_send() {
            let email = grant.google().ok_or(Error::Policy)?.email.clone();
            match &refresh {
                Some(expected) => Command::RefreshGoogleSend {
                    expected: expected.clone(),
                    binding: binding.clone(),
                    email,
                },
                None => Command::RegisterGoogleSend {
                    binding: binding.clone(),
                    email,
                },
            }
        } else if grant.google_body() {
            match &refresh {
                Some(expected) => Command::RefreshGoogleBody {
                    expected: expected.clone(),
                    binding: binding.clone(),
                },
                None => Command::RegisterGoogleBody {
                    binding: binding.clone(),
                },
            }
        } else {
            match (&refresh, preview) {
                (Some(expected), true) => Command::RefreshGooglePreview {
                    expected: expected.clone(),
                    binding: binding.clone(),
                },
                (None, true) => Command::RegisterGooglePreview {
                    binding: binding.clone(),
                },
                (Some(expected), false) => Command::RefreshGoogleInbox {
                    expected: expected.clone(),
                    binding: binding.clone(),
                },
                (None, false) => Command::RegisterGoogleInbox {
                    binding: binding.clone(),
                },
            }
        };
        if !matches!(self.command(engine, command).await?, Reply::Registered) {
            return Err(Error::Authentication);
        }
        if refresh.is_some() {
            engine.confirm_live_refresh(binding)?
        }
        Ok(())
    }
    async fn live_pending(
        &self,
        engine: &mut Engine,
        vault: &mut live_vault::LiveVault,
        adapter: &github::GitHubAdapter,
        google: &google::GoogleAdapter,
    ) -> Result<usize> {
        let invocation = vault.pending.clone().ok_or(Error::Policy)?;
        // A dropped poll future can leave pending after provider admission or
        // receipt persistence. Only a same-process Committed phase is proof of
        // non-entry; every other phase must reconcile without adapter retry.
        let phase = engine
            .state
            .entries
            .iter()
            .find(|e| e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .phase
            .clone();
        let operation = engine
            .state
            .entries
            .iter()
            .find(|e| e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request
            .operation;
        let result = match phase {
            Phase::Committed(_) => match operation {
                Operation::GoogleInboxRead
                | Operation::GoogleInboxPreview
                | Operation::GoogleInboxBody { .. }
                | Operation::GoogleMailSend { .. }
                | Operation::GoogleMailLookup { .. } => {
                    engine.execute_live_google(vault, &invocation, google).await
                }
                Operation::GitHubRepositoryRead => {
                    engine
                        .execute_live_github(vault, &invocation, adapter)
                        .await
                }
                _ => Err(Error::Policy),
            },
            _ => engine.settle_without_retry(&invocation),
        };
        let receipt = match result {
            Ok(receipt) => receipt,
            Err(Error::Unavailable) => return Err(Error::Unavailable),
            Err(Error::Reconnect) => {
                vault.retire();
                let receipt = engine.settle_without_retry(&invocation)?;
                let result = self
                    .command(
                        engine,
                        Command::Settle {
                            receipt: Box::new(receipt),
                        },
                    )
                    .await;
                if result.is_ok() {
                    engine.delivered(invocation)?;
                }
                return Err(Error::Reconnect);
            }
            // A terminal local admission failure (for example request expiry
            // while offline) must not strand pending ahead of reconciliation.
            // The durable phase determines NotDispatched vs Unknown; neither
            // branch grants a second provider entry.
            Err(_) => engine.settle_without_retry(&invocation)?,
        };
        self.command(
            engine,
            Command::Settle {
                receipt: Box::new(receipt),
            },
        )
        .await?;
        engine.delivered(invocation)?;
        vault.pending = None;
        Ok(1)
    }
    /// Transient loss suspends; an acknowledged, not-entered commitment can
    /// revalidate in this same unlocked process. Restart retains no such permit.
    pub async fn poll_live(
        &self,
        engine: &mut Engine,
        vault: &mut live_vault::LiveVault,
        adapter: &github::GitHubAdapter,
    ) -> Result<usize> {
        self.poll_live_with_google(engine, vault, adapter, &google::GoogleAdapter::new()?)
            .await
    }
    pub async fn poll_live_with_google(
        &self,
        engine: &mut Engine,
        vault: &mut live_vault::LiveVault,
        adapter: &github::GitHubAdapter,
        google: &google::GoogleAdapter,
    ) -> Result<usize> {
        let result = self.poll_live_inner(engine, vault, adapter, google).await;
        if matches!(result, Err(Error::Authentication | Error::Reconnect)) {
            vault.retire();
        }
        result
    }
    async fn poll_live_inner(
        &self,
        engine: &mut Engine,
        vault: &mut live_vault::LiveVault,
        adapter: &github::GitHubAdapter,
        google: &google::GoogleAdapter,
    ) -> Result<usize> {
        let mut completed = 0;
        if vault.pending.is_some() {
            completed += self.live_pending(engine, vault, adapter, google).await?;
        }
        if !vault.is_unlocked() {
            return Err(Error::Reconnect);
        }
        engine.mail_transport = Some(self.clone());
        vault.validate().await?;
        self.sync_retained(engine).await?;
        let (requests, revocation_pending) = self.waiting(engine).await?;
        if revocation_pending {
            engine.revoke()?;
            vault.retire();
            self.command(engine, Command::AckRevoke).await?;
            return Ok(completed);
        }
        for request in requests {
            let invocation = request.invocation.clone();
            let request_digest = auth::hash(&request_bytes(&request));
            engine.prepare_live_github(vault, request).await?;
            self.mail_payload(engine, &invocation).await?;
            let Reply::Commitment { commitment } = self
                .command(
                    engine,
                    Command::CommitCurrent {
                        invocation: invocation.clone(),
                        request_digest,
                    },
                )
                .await?
            else {
                return Err(Error::Authentication);
            };
            engine.accept_commitment(*commitment)?;
            vault.pending = Some(invocation);
            completed += self.live_pending(engine, vault, adapter, google).await?;
        }
        Ok(completed)
    }
    /// Locked callers do not invoke this: waiting server jobs use no worker/model slot.
    pub async fn poll(
        &self,
        engine: &mut Engine,
        archive: &mut moosuri_vault_core::UnlockedArchive,
        adapter: &github::GitHubAdapter,
    ) -> Result<usize> {
        self.poll_with_google(engine, archive, adapter, &google::GoogleAdapter::new()?)
            .await
    }
    pub async fn poll_with_google(
        &self,
        engine: &mut Engine,
        archive: &mut moosuri_vault_core::UnlockedArchive,
        adapter: &github::GitHubAdapter,
        google: &google::GoogleAdapter,
    ) -> Result<usize> {
        engine.mail_transport = Some(self.clone());
        self.sync_retained(engine).await?;
        let (requests, revocation_pending) = self.waiting(engine).await?;
        if revocation_pending {
            engine.revoke()?;
            self.command(engine, Command::AckRevoke).await?;
            return Ok(0);
        }
        let mut completed = 0;
        for request in requests {
            let invocation = request.invocation.clone();
            let operation = request.operation;
            match operation {
                Operation::GoogleInboxRead
                | Operation::GoogleInboxPreview
                | Operation::GoogleInboxBody { .. }
                | Operation::GoogleMailSend { .. }
                | Operation::GoogleMailLookup { .. } => {
                    engine.prepare_google(archive, request, current_time()?)?
                }
                Operation::GitHubRepositoryRead => {
                    engine.prepare_github(archive, request, current_time()?)?
                }
                _ => return Err(Error::Policy),
            }
            self.mail_payload(engine, &invocation).await?;
            let request = engine
                .state
                .entries
                .iter()
                .find(|e| e.request.invocation == invocation)
                .ok_or(Error::Policy)?
                .request
                .clone();
            let Reply::Commitment { commitment } = self
                .command(
                    engine,
                    Command::CommitCurrent {
                        invocation: invocation.clone(),
                        request_digest: auth::hash(&request_bytes(&request)),
                    },
                )
                .await?
            else {
                return Err(Error::Authentication);
            };
            engine.accept_commitment(*commitment)?;
            let receipt = match operation {
                Operation::GoogleInboxRead
                | Operation::GoogleInboxPreview
                | Operation::GoogleInboxBody { .. }
                | Operation::GoogleMailSend { .. }
                | Operation::GoogleMailLookup { .. } => {
                    engine.execute_google(archive, &invocation, google).await?
                }
                Operation::GitHubRepositoryRead => {
                    engine.execute_github(archive, &invocation, adapter).await?
                }
                _ => return Err(Error::Policy),
            };
            self.command(
                engine,
                Command::Settle {
                    receipt: Box::new(receipt),
                },
            )
            .await?;
            engine.delivered(invocation)?;
            completed += 1;
        }
        Ok(completed)
    }
}
