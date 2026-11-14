//! Bounded personal SIWC orchestration. SQLite owns refresh admission and CAS.
mod handoff;
use crate::*;
use moosuri_connectors::chatgpt_oauth::{AuthError, ChatGptAuthClient};
use moosuri_store::{SubscriptionAccess, SubscriptionAuthority, SubscriptionRef, UserContext};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
#[derive(Clone)]
pub struct SubscriptionService {
    store: Store,
    signals: Signals,
    logins: Arc<tokio::sync::Semaphore>,
    #[cfg(any(test, feature = "test-fixtures"))]
    fixture: Option<(String, String)>,
}
/// Private launch payload for the native helper. Never Debug/log its URL.
#[derive(serde::Serialize)]
pub struct LocalLoginLaunch {
    pub attempt_id: String,
    pub binding_id: String,
    pub revision: i64,
    pub authorization_url: String,
}
impl SubscriptionService {
    async fn auth_client(&self) -> Result<ChatGptAuthClient, AuthError> {
        #[cfg(any(test, feature = "test-fixtures"))]
        if let Some((origin, _)) = &self.fixture {
            return ChatGptAuthClient::synthetic_fixture(origin).await;
        }
        ChatGptAuthClient::discover().await
    }
    pub async fn client(
        &self,
        authority: SubscriptionAuthority,
        binding: SubscriptionRef,
    ) -> Result<ModelClient, StoreError> {
        #[cfg(any(test, feature = "test-fixtures"))]
        let fixture = self.fixture.as_ref();
        #[cfg(not(any(test, feature = "test-fixtures")))]
        let fixture: Option<&(String, String)> = None;
        personal_client_inner(&self.store, authority, binding, fixture).await
    }

    pub(crate) fn new(store: Store, signals: Signals) -> Self {
        Self {
            store,
            signals,
            logins: Arc::new(tokio::sync::Semaphore::new(2)),
            #[cfg(test)]
            fixture: None,
            #[cfg(all(not(test), feature = "test-fixtures"))]
            fixture: std::env::var("MOOSURI_TEST_CHATGPT_ORIGIN")
                .ok()
                .map(|origin| (origin.clone(), origin)),
        }
    }
    pub async fn local_login(
        &self,
        user: &UserContext,
        existing: Option<(String, i64)>,
        label: String,
    ) -> Result<LocalLoginLaunch, StoreError> {
        let permit = self
            .logins
            .clone()
            .try_acquire_owned()
            .map_err(|_| StoreError::Busy)?;
        // Bind exact loopback first. Remote browsers require the separate VM helper.
        let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
            .await
            .map_err(|_| StoreError::Unavailable)?;
        let port = listener
            .local_addr()
            .map_err(|_| StoreError::Unavailable)?
            .port();
        // Owner check precedes any provider HTTP, including discovery.
        let ticket = self
            .store
            .begin_subscription_login(user, existing, label)
            .await?;
        let client = match self.auth_client().await {
            Ok(v) => v,
            Err(_) => {
                self.store.fail_subscription_login(&ticket).await?;
                return Err(StoreError::Unavailable);
            }
        };
        let started = if let Some(i) = ticket.identity.as_ref() {
            client.begin_returning(i, port)
        } else if let Some(i) = ticket.issued.as_ref() {
            client.begin_reauthorization(i.clone(), port)
        } else {
            client.begin_registration(&ticket.host_id, port)
        };
        let (attempt, url) = match started {
            Ok(v) => v,
            Err(_) => {
                self.store.fail_subscription_login(&ticket).await?;
                return Err(StoreError::Unavailable);
            }
        };
        let launch = LocalLoginLaunch {
            attempt_id: ticket.attempt_id.clone(),
            binding_id: ticket.binding.id.clone(),
            revision: ticket.binding.revision,
            authorization_url: url.for_system_browser().to_owned(),
        };
        let store = self.store.clone();
        let signals = self.signals.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let operation = async {
                let (mut stream, _) = listener
                    .accept()
                    .await
                    .map_err(|_| StoreError::Unavailable)?;
                let mut bytes = Vec::new();
                let mut chunk = [0; 1024];
                loop {
                    let n = stream
                        .read(&mut chunk)
                        .await
                        .map_err(|_| StoreError::Unavailable)?;
                    if n == 0 {
                        return Err(StoreError::InvalidInput);
                    }
                    bytes.extend_from_slice(&chunk[..n]);
                    if bytes.len() > 16384 {
                        return Err(StoreError::InvalidInput);
                    }
                    if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
                        break;
                    }
                }
                let text = std::str::from_utf8(&bytes).map_err(|_| StoreError::InvalidInput)?;
                let line = text.lines().next().ok_or(StoreError::InvalidInput)?;
                let mut parts = line.split_whitespace();
                if parts.next() != Some("GET") {
                    return Err(StoreError::InvalidInput);
                }
                let path = parts.next().ok_or(StoreError::InvalidInput)?;
                if !path.starts_with("/auth/callback?")
                    || parts.next() != Some("HTTP/1.1")
                    || parts.next().is_some()
                {
                    return Err(StoreError::InvalidInput);
                }
                let expected_host = format!("127.0.0.1:{port}");
                let hosts = text
                    .lines()
                    .skip(1)
                    .filter_map(|l| l.split_once(':'))
                    .filter(|(k, _)| k.eq_ignore_ascii_case("host"))
                    .map(|(_, v)| v.trim())
                    .collect::<Vec<_>>();
                if hosts != [expected_host.as_str()] {
                    return Err(StoreError::InvalidInput);
                }
                let pending = attempt
                    .accept_callback(&format!("http://127.0.0.1:{port}{path}"))
                    .map_err(|_| StoreError::InvalidInput)?;
                // Save issued registration durably before the one consuming redemption.
                store
                    .save_subscription_registration(&ticket, pending.registration())
                    .await?;
                let bundle = client
                    .exchange(pending)
                    .await
                    .map_err(|_| StoreError::Unavailable)?;
                if signals.stopping().is_cancelled() {
                    return Err(StoreError::Unauthorized);
                }
                store.complete_subscription_login(&ticket, bundle).await?;
                let response=b"HTTP/1.1 200 OK\r\nContent-Type: text/plain; charset=utf-8\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\nMoosuri sign-in complete. Return to the app.";
                let _ = stream.write_all(response).await;
                Ok::<(), StoreError>(())
            };
            let result = tokio::select! {_ = signals.stopping().cancelled()=>Err(StoreError::Unavailable),result=tokio::time::timeout(Duration::from_secs(300),operation)=>result.unwrap_or(Err(StoreError::Unavailable))};
            if result.is_err() {
                let _ = store.fail_subscription_login(&ticket).await;
            }
            signals.changed();
        });
        Ok(launch)
    }
    pub async fn disconnect(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<bool, StoreError> {
        let bundle = self
            .store
            .begin_subscription_disconnect(u, id.clone(), revision)
            .await?;
        self.signals.changed();
        self.signals.wake.notify_one();
        let confirmed = if let Some(b) = bundle {
            match self.auth_client().await {
                Ok(client) => client.revoke(&b).await.is_ok(),
                Err(_) => false,
            }
        } else {
            false
        };
        self.store
            .finish_subscription_disconnect(u, id, revision, confirmed)
            .await?;
        Ok(confirmed)
    }
}
/// Every call is authorized against the durable owned binding before fixed-origin IO.
pub async fn personal_client(
    store: &Store,
    authority: SubscriptionAuthority,
    binding: SubscriptionRef,
) -> Result<ModelClient, StoreError> {
    personal_client_inner(store, authority, binding, None).await
}
async fn personal_client_inner(
    store: &Store,
    authority: SubscriptionAuthority,
    binding: SubscriptionRef,
    fixture: Option<&(String, String)>,
) -> Result<ModelClient, StoreError> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(35);
    loop {
        match store
            .subscription_access(authority.clone(), binding.clone())
            .await?
        {
            SubscriptionAccess::Ready(token) => {
                let token = Secret::new(token.to_string()).map_err(|_| StoreError::Corrupt)?;
                let caps = TextCapabilities {
                    efforts: binding.effort.iter().cloned().collect(),
                };
                #[cfg(any(test, feature = "test-fixtures"))]
                if let Some((_, origin)) = fixture {
                    return ModelClient::synthetic_chatgpt_plan(origin, token, caps)
                        .map_err(|_| StoreError::Unavailable);
                }
                return ModelClient::chatgpt_plan(token, caps).map_err(|_| StoreError::Unavailable);
            }
            SubscriptionAccess::Wait => {
                if tokio::time::Instant::now() >= deadline {
                    return Err(StoreError::Busy);
                }
                tokio::time::sleep(Duration::from_millis(50)).await;
            }
            SubscriptionAccess::Refresh(lease, bundle) => {
                #[cfg(any(test, feature = "test-fixtures"))]
                let discovered = if let Some((origin, _)) = fixture {
                    ChatGptAuthClient::synthetic_fixture(origin).await
                } else {
                    ChatGptAuthClient::discover().await
                };
                #[cfg(not(any(test, feature = "test-fixtures")))]
                let discovered = {
                    let _ = fixture;
                    ChatGptAuthClient::discover().await
                };
                let client = match discovered {
                    Ok(v) => v,
                    Err(_) => return Err(StoreError::Unavailable),
                };
                store.dispatch_subscription_refresh(&lease).await?;
                match client.refresh(&bundle).await {
                    Ok(next) => store.complete_subscription_refresh(*lease, next).await?,
                    Err(error) => {
                        // refresh() exposes TemporaryHttp only for the token
                        // endpoint's known rejection. All failures after a 200
                        // replacement (including JWKS) are rotation-unknown.
                        let temporary = matches!(error, AuthError::TemporaryHttp(_));
                        store.fail_subscription_refresh(*lease, temporary).await?;
                        return Err(StoreError::Unavailable);
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;
