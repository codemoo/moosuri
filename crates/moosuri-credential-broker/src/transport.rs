//! HTTPS origin pin, no proxy/redirect/cookie inheritance, signed device commands.
use crate::*;
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
pub struct Transport {
    client: reqwest::Client,
    origin: String,
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
        })
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
            .post(format!("{}{path}", self.origin))
            .header("content-type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| Error::Authentication)?;
        if !r.status().is_success() {
            return Err(Error::Authentication);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = r.chunk().await.map_err(|_| Error::Authentication)? {
            if bytes.len() + chunk.len() > 262144 {
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
    /// Locked callers do not invoke this: waiting server jobs use no worker/model slot.
    pub async fn poll(
        &self,
        engine: &mut Engine,
        archive: &mut moosuri_vault_core::UnlockedArchive,
        adapter: &github::GitHubAdapter,
    ) -> Result<usize> {
        self.sync_retained(engine).await?;
        let Reply::Waiting {
            requests,
            revocation_pending,
        } = self.command(engine, Command::Poll).await?
        else {
            return Err(Error::Authentication);
        };
        if revocation_pending {
            engine.revoke()?;
            self.command(engine, Command::AckRevoke).await?;
            return Ok(0);
        }
        let mut completed = 0;
        for request in requests {
            let invocation = request.invocation.clone();
            engine.prepare_github(archive, request, current_time()?)?;
            let Reply::Commitment { commitment } = self
                .command(
                    engine,
                    Command::Commit {
                        invocation: invocation.clone(),
                    },
                )
                .await?
            else {
                return Err(Error::Authentication);
            };
            engine.accept_commitment(*commitment)?;
            let receipt = engine.execute_github(archive, &invocation, adapter).await?;
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
