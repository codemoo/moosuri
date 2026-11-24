//! Personal Gmail IO outside the single SQLite writer. No agent approval API.
use moosuri_connectors::gmail::{GmailClient, GmailError, OAuthClient};
use moosuri_connectors::model::CancellationToken;
use moosuri_core::{ExternalToolRequest, MailToolRequest};
use moosuri_store::{
    ExternalToolPermit, GoogleApp, GoogleAppPin, KnowledgeToolResult, MailBinding, MailDispatch,
    Store, StoreError, ToolReadError, UserContext,
};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
mod attachments;
mod connection_help;
pub(crate) use connection_help::{CONNECTION_HELP, connection_help_definition};
mod drafts;
mod gws;
mod latest;
mod model_context;
mod projection;
mod reconcile;
mod reply_watches;
pub type FenceFuture<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + 'a>>;
pub trait DispatchFence: Send + Sync {
    fn permit<'a>(
        &'a self,
        dispatch: &'a MailDispatch,
        binding: &'a MailBinding,
    ) -> FenceFuture<'a>;
}
type MailLocks = Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>;
#[derive(Clone)]
pub struct MailService {
    store: Store,
    gws_inbox: Option<Arc<moosuri_connectors::gws_inbox::GwsInboxRegistry>>,
    pdf_extractor: Option<std::path::PathBuf>,
    client: GmailClient,
    oauth: Option<Arc<OAuthClient>>,
    managed_redirect: Option<String>,
    locks: MailLocks,
    signals: crate::Signals,
    #[cfg(test)]
    synthetic_google: Option<std::net::SocketAddr>,
}
impl MailService {
    pub(crate) fn new(
        store: Store,
        oauth: Option<Arc<OAuthClient>>,
        signals: crate::Signals,
    ) -> Self {
        Self {
            store,
            gws_inbox: None,
            pdf_extractor: None,
            #[cfg(test)]
            synthetic_google: None,
            client: GmailClient::google(),
            oauth,
            managed_redirect: None,
            locks: Arc::new(Mutex::new(HashMap::new())),
            signals,
        }
    }
    pub fn for_member_relay(store: Store, oauth: Arc<OAuthClient>, client: GmailClient) -> Self {
        let (changes, _) = tokio::sync::watch::channel(0);
        let signals = crate::Signals {
            stop: CancellationToken::new(),
            wake: Default::default(),
            changes,
            delivery: Default::default(),
        };
        let mut service = Self::new(store, Some(oauth), signals);
        service.client = client;
        service
    }
    pub(crate) fn with_gws_inbox(
        mut self,
        registry: Option<Arc<moosuri_connectors::gws_inbox::GwsInboxRegistry>>,
    ) -> Self {
        self.gws_inbox = registry;
        self
    }
    pub(crate) fn with_managed_google(mut self, redirect_uri: Option<String>) -> Self {
        self.managed_redirect = redirect_uri;
        self
    }
    pub async fn configured(&self) -> bool {
        matches!(self.oauth_client().await, Ok(Some(_)))
    }
    /// Resolve one immutable configuration per operation. The writer checks its
    /// pin again before any OAuth/refresh success or failure can mutate state.
    pub async fn oauth_client(&self) -> Result<Option<GoogleApp>, StoreError> {
        if let Some(client) = &self.oauth {
            return Ok(Some(GoogleApp {
                client: client.clone(),
                pin: GoogleAppPin {
                    revision: None,
                    client_id: client.client_id.clone(),
                    redirect_uri: client.redirect_uri.clone(),
                },
            }));
        }
        match &self.managed_redirect {
            Some(redirect) => self.store.google_app(redirect.clone()).await,
            None => Ok(None),
        }
    }
    #[cfg(test)]
    pub(crate) fn with_synthetic_google(mut self, address: std::net::SocketAddr) -> Self {
        self.client = GmailClient::synthetic_loopback(address).unwrap();
        self.synthetic_google = Some(address);
        self
    }
    pub(crate) fn calendar_client(&self) -> moosuri_connectors::calendar::CalendarClient {
        #[cfg(test)]
        if let Some(address) = self.synthetic_google {
            return moosuri_connectors::calendar::CalendarClient::synthetic_loopback(address)
                .unwrap();
        }
        moosuri_connectors::calendar::CalendarClient::google()
    }
    pub fn client(&self) -> &GmailClient {
        &self.client
    }
    async fn lock(&self, id: &str) -> Arc<Mutex<()>> {
        let mut locks = self.locks.lock().await;
        locks.retain(|_, lock| lock.strong_count() > 0);
        if let Some(lock) = locks.get(id).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        locks.insert(id.into(), Arc::downgrade(&lock));
        lock
    }
    // Only the local revoke/purge mutation is ordered against SSE delivery.
    // OAuth refresh network IO has already returned before taking this gate.
    async fn invalidate_binding(&self, binding: &MailBinding) -> Result<(), StoreError> {
        let result = {
            let _delivery = self.signals.delivery.write().await;
            self.store.mark_mail_needs_reconnect(binding).await
        };
        self.signals.changed();
        result
    }
    async fn refresh_binding(
        &self,
        mut binding: MailBinding,
        cancel: &CancellationToken,
        dispatch: Option<&MailDispatch>,
    ) -> Result<MailBinding, StoreError> {
        let app = self.oauth_client().await?.ok_or(StoreError::Unavailable)?;
        binding = self
            .store
            .pin_mail_binding(binding, app.pin.clone())
            .await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if binding.expires_at <= now + 60 {
            let tokens = match self
                .client
                .refresh(&app.client, &binding.refresh_token, cancel)
                .await
            {
                Ok(tokens) => tokens,
                Err(error) => {
                    if matches!(error, GmailError::Http(400 | 401)) {
                        let _ = self.invalidate_binding(&binding).await;
                    }
                    return Err(StoreError::Unavailable);
                }
            };
            self.store.save_mail_refresh(&binding, tokens).await?;
            binding = if let Some(d) = dispatch {
                self.store.mail_dispatch_binding(d).await?
            } else {
                self.store
                    .mail_binding(binding.user(), binding.id.clone())
                    .await?
            };
            binding = self.store.pin_mail_binding(binding, app.pin).await?;
        }
        Ok(binding)
    }
    pub async fn binding(
        &self,
        user: &UserContext,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MailBinding, StoreError> {
        let lock = self.lock(id).await;
        let _guard = lock.lock().await;
        let b = self.store.mail_binding(user, id.into()).await?;
        self.refresh_binding(b, cancel, None).await
    }
    /// Background collection uses a durable source grant, never a fabricated browser session.
    /// Share the mailbox refresh lock with interactive reads and approved dispatches.
    pub(crate) async fn source_credentials(
        &self,
        cap: &moosuri_store::SourceIngestCapability,
        cancel: &CancellationToken,
    ) -> Result<moosuri_store::SourceGmailCredentials, StoreError> {
        let app = self.oauth_client().await?.ok_or(StoreError::Unavailable)?;
        let first = self
            .store
            .source_mail_credentials_with_app(cap, app.pin.clone())
            .await?;
        let lock = self.lock(&first.mail_connection_id).await;
        drop(first);
        let _guard = lock.lock().await;
        let mut credentials = self
            .store
            .source_mail_credentials_with_app(cap, app.pin.clone())
            .await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if credentials.expires_at <= now + 60 {
            let tokens = self
                .client
                .refresh(&app.client, &credentials.refresh_token, cancel)
                .await
                .map_err(|error| {
                    if matches!(error, GmailError::Http(400 | 401 | 403)) {
                        StoreError::Unauthorized
                    } else {
                        StoreError::Unavailable
                    }
                })?;
            self.store
                .save_source_mail_refresh_with_app(
                    cap,
                    credentials.revision,
                    tokens,
                    app.pin.clone(),
                )
                .await?;
            credentials = self
                .store
                .source_mail_credentials_with_app(cap, app.pin)
                .await?;
        }
        Ok(credentials)
    }
    pub async fn read(
        &self,
        user: &UserContext,
        request: &MailToolRequest,
        cancel: &CancellationToken,
    ) -> Result<serde_json::Value, StoreError> {
        let b = self.binding(user, request.connection_id(), cancel).await?;
        let value = self
            .read_with_token(&b.access_token, request, cancel)
            .await?;
        // Revalidate after IO before returning any private data (including disconnect).
        let latest = self.store.mail_binding(user, b.id.clone()).await?;
        if latest.epoch != b.epoch {
            return Err(StoreError::Conflict);
        }
        Ok(value)
    }
    async fn read_with_token(
        &self,
        token: &moosuri_connectors::gmail::SecretString,
        request: &MailToolRequest,
        cancel: &CancellationToken,
    ) -> Result<serde_json::Value, StoreError> {
        let value = match request {
            MailToolRequest::Search {
                query, page_token, ..
            } => serde_json::to_value(
                self.client
                    .list_messages(token, query, page_token.as_deref(), 5, cancel)
                    .await
                    .map_err(map)?,
            )
            .map_err(|_| StoreError::Unavailable)?,
            MailToolRequest::Read { message_id, .. } => project(
                self.client
                    .get_message(token, message_id, cancel)
                    .await
                    .map_err(map)?,
            ),
            MailToolRequest::Attachments {
                message_id,
                start_index,
                ..
            } => {
                let m = self
                    .client
                    .get_message(token, message_id, cancel)
                    .await
                    .map_err(map)?;
                let start = *start_index as usize;
                if start > m.attachments.len() {
                    return Err(StoreError::InvalidInput);
                }
                let mut end = (start + 4).min(m.attachments.len());
                loop {
                    let result = serde_json::json!({"message_id":m.id,"thread_id":m.thread_id,"attachments":m.attachments[start..end],"total":m.attachments.len(),"next_start_index":if end<m.attachments.len(){Some(end)}else{None}});
                    if serde_json::to_vec(&KnowledgeToolResult::External(result.clone()))
                        .map_err(|_| StoreError::Unavailable)?
                        .len()
                        <= 12 * 1024
                    {
                        break result;
                    }
                    if end == start {
                        return Err(StoreError::InvalidInput);
                    }
                    end -= 1;
                }
            }
            MailToolRequest::Attachment { .. } => self.attachment(token, request, cancel).await?,
            MailToolRequest::Thread { thread_id, .. } => {
                let t = self
                    .client
                    .get_thread(token, thread_id, cancel)
                    .await
                    .map_err(map)?;
                serde_json::json!({"id":t.id,"messages":t.messages.into_iter().map(project).collect::<Vec<_>>(),"truncated":t.truncated})
            }
            MailToolRequest::Draft { .. } => return Err(StoreError::InvalidInput),
        };
        Ok(value)
    }
    pub(crate) async fn mission_credentials(
        &self,
        permit: &ExternalToolPermit,
        cancel: &CancellationToken,
    ) -> Result<moosuri_store::MissionReadCredentials, StoreError> {
        let app = self.oauth_client().await?.ok_or(StoreError::Unavailable)?;
        let first = self
            .store
            .mission_read_credentials(permit, app.pin.clone())
            .await?;
        let lock = self.lock(&first.connection_id).await;
        drop(first);
        let _guard = lock.lock().await;
        let mut b = self
            .store
            .mission_read_credentials(permit, app.pin.clone())
            .await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if b.expires_at <= now + 60 {
            let tokens = match self.client.refresh(&app.client, &b.refresh, cancel).await {
                Ok(tokens) => tokens,
                Err(error) => {
                    if matches!(error, GmailError::Http(400 | 401 | 403)) {
                        let _delivery = self.signals.delivery.write().await;
                        let _ = self
                            .store
                            .mark_mission_read_needs_reconnect(permit, app.pin.clone(), b.revision)
                            .await;
                        self.signals.changed();
                    }
                    return Err(map(error));
                }
            };
            self.store
                .refresh_mission_read(permit, app.pin.clone(), b.revision, tokens)
                .await?;
            b = self.store.mission_read_credentials(permit, app.pin).await?;
        }
        self.store.check_external_tool(permit).await?;
        Ok(b)
    }
    pub async fn external(
        &self,
        permit: &ExternalToolPermit,
        cancel: &CancellationToken,
    ) -> KnowledgeToolResult {
        let ExternalToolRequest::Mail(request) = permit.request() else {
            return KnowledgeToolResult::Error(ToolReadError::Unavailable);
        };
        let result = if let Some(user) = permit.user() {
            self.read(user, request, cancel).await
        } else {
            async {
                let b = self.mission_credentials(permit, cancel).await?;
                self.store.check_external_tool(permit).await?;
                let value = self.read_with_token(&b.access, request, cancel).await?;
                self.store.check_external_tool(permit).await?;
                Ok(value)
            }
            .await
        };
        match result {
            Ok(value) => projection::bounded(value),
            Err(StoreError::InvalidInput) => {
                KnowledgeToolResult::Error(ToolReadError::InvalidInput)
            }
            Err(_) => KnowledgeToolResult::Error(ToolReadError::Unavailable),
        }
    }
    pub(crate) async fn worker(self, stop: CancellationToken) {
        self.delivery_loop(stop, None).await;
    }
    pub async fn member_worker(self, fence: Arc<dyn DispatchFence>, stop: CancellationToken) {
        self.delivery_loop(stop, Some(fence)).await;
    }
    async fn delivery_loop(self, stop: CancellationToken, fence: Option<Arc<dyn DispatchFence>>) {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {_ =stop.cancelled()=>break,_=tick.tick()=>{}}
            if !self.configured().await {
                continue;
            }
            if let Ok(Some(dispatch)) = self.store.next_mail_dispatch().await {
                self.dispatch(dispatch, &stop, fence.as_deref()).await;
            }
            if let Ok(Some(permit)) = self.store.next_mail_reconciliation().await {
                self.reconcile(permit, &stop).await;
            }
        }
    }
    async fn dispatch(
        &self,
        dispatch: MailDispatch,
        stop: &CancellationToken,
        fence: Option<&dyn DispatchFence>,
    ) {
        let lock = self.lock(&dispatch.connection_id).await;
        let _guard = lock.lock().await;
        let b = match self.store.mail_dispatch_binding(&dispatch).await {
            Ok(b) => b,
            Err(_) => {
                let _ = self.store.finish_mail_dispatch(&dispatch, None, true).await;
                return;
            }
        };
        let b = match self.refresh_binding(b, stop, Some(&dispatch)).await {
            Ok(b) => b,
            Err(_) => {
                let _ = self.store.finish_mail_dispatch(&dispatch, None, true).await;
                return;
            }
        };
        // A fresh current binding check closes disconnect/refresh races before IO;
        // after this check the committed action is in flight, not retractable.
        if self.store.mail_dispatch_binding(&dispatch).await.is_err() {
            let _ = self.store.finish_mail_dispatch(&dispatch, None, true).await;
            return;
        }
        if let Some(fence) = fence {
            if !fence.permit(&dispatch, &b).await {
                let _ = self.store.finish_mail_dispatch(&dispatch, None, true).await;
                return;
            }
        }
        let result = self
            .client
            .send_frozen(&b.access_token, &dispatch.frozen, stop)
            .await;
        let (receipt, definite) = match result {
            Ok(r) => (Some((r.id, r.thread_id)), false),
            Err(GmailError::Http(400 | 401 | 403 | 404 | 413)) => (None, true),
            Err(_) => (None, false),
        };
        // Retry only local receipt persistence, never the network send.
        let _ = crate::store_retry::local(|| {
            self.store
                .finish_mail_dispatch(&dispatch, receipt.clone(), definite)
        })
        .await;
    }
}
fn map(error: GmailError) -> StoreError {
    match error {
        GmailError::InvalidInput => StoreError::InvalidInput,
        _ => StoreError::Unavailable,
    }
}
fn project(m: moosuri_connectors::gmail::MailMessage) -> serde_json::Value {
    serde_json::json!({"id":m.id,"thread_id":m.thread_id,"labels":m.labels,"mail":m.mail,"attachments":m.attachments})
}

#[cfg(test)]
mod delivery_tests {
    use super::*;
    use moosuri_connectors::gmail::{SecretString, TokenSet};
    #[tokio::test]
    async fn refresh_failure_purge_waits_for_held_delivery_and_notifies() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Synthetic".into(),
                "owner@example.test".into(),
                "Synthetic-Account-Password-42".into(),
                false,
            )
            .await
            .unwrap();
        let user = store
            .login(
                "owner@example.test".into(),
                "Synthetic-Account-Password-42".into(),
            )
            .await
            .unwrap()
            .user;
        let start = store.start_mail_oauth(&user).await.unwrap();
        let handle = store
            .capture_mail_oauth(
                start.state,
                SecretString::new("synthetic-code".into()).unwrap(),
            )
            .await
            .unwrap();
        let exchange = store.claim_mail_oauth(&user, handle).await.unwrap();
        let connection = store
            .complete_mail_oauth(
                exchange,
                TokenSet {
                    access_token: SecretString::new("synthetic-access".into()).unwrap(),
                    refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                    expires_in: 3600,
                    scopes: None,
                },
                "owner@example.test".into(),
            )
            .await
            .unwrap();
        let runtime = crate::Runtime::start(store.clone());
        let mail = runtime.mail_service();
        let binding = store
            .mail_binding(&user, connection.id.clone())
            .await
            .unwrap();
        let gate = runtime.delivery_gate();
        let delivery = gate.read().await;
        let mut changed = runtime.subscribe();
        changed.borrow_and_update();
        // Invoke the exact local mutation helper used after HTTP refresh failure;
        // there is no network call or synthetic production endpoint override.
        let mut invalidation = tokio::spawn(async move { mail.invalidate_binding(&binding).await });
        assert!(
            tokio::time::timeout(Duration::from_millis(100), &mut invalidation)
                .await
                .is_err()
        );
        assert_eq!(
            store.mail_connections(&user).await.unwrap()[0].status,
            "active"
        );
        drop(delivery);
        invalidation.await.unwrap().unwrap();
        tokio::time::timeout(Duration::from_secs(1), changed.changed())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store.mail_connections(&user).await.unwrap()[0].status,
            "needs_reconnect"
        );
        runtime.shutdown().await;
    }
}
