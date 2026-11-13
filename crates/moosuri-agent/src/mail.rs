//! Personal Gmail IO outside the single SQLite writer. No agent approval API.
use moosuri_connectors::gmail::{GmailClient, GmailError, OAuthClient};
use moosuri_connectors::model::CancellationToken;
use moosuri_core::{ExternalToolRequest, MailToolRequest};
use moosuri_store::{
    ExternalToolPermit, KnowledgeToolResult, MailBinding, MailDispatch, Store, StoreError,
    ToolReadError, UserContext,
};
use std::{
    collections::HashMap,
    sync::{Arc, Weak},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::Mutex;
type MailLocks = Arc<Mutex<HashMap<String, Weak<Mutex<()>>>>>;
#[derive(Clone)]
pub struct MailService {
    store: Store,
    client: GmailClient,
    oauth: Option<Arc<OAuthClient>>,
    locks: MailLocks,
    signals: crate::Signals,
}
impl MailService {
    pub(crate) fn new(
        store: Store,
        oauth: Option<Arc<OAuthClient>>,
        signals: crate::Signals,
    ) -> Self {
        Self {
            store,
            client: GmailClient::google(),
            oauth,
            locks: Arc::new(Mutex::new(HashMap::new())),
            signals,
        }
    }
    pub fn configured(&self) -> bool {
        self.oauth.is_some()
    }
    pub fn oauth_client(&self) -> Option<&OAuthClient> {
        self.oauth.as_deref()
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
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if binding.expires_at <= now + 60 {
            let oauth = self.oauth.as_ref().ok_or(StoreError::Unavailable)?;
            let tokens = match self
                .client
                .refresh(oauth, &binding.refresh_token, cancel)
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
        let first = self.store.source_mail_credentials(cap).await?;
        let lock = self.lock(&first.mail_connection_id).await;
        drop(first);
        let _guard = lock.lock().await;
        let mut credentials = self.store.source_mail_credentials(cap).await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if credentials.expires_at <= now + 60 {
            let client = self.oauth.as_ref().ok_or(StoreError::Unavailable)?;
            let tokens = self
                .client
                .refresh(client, &credentials.refresh_token, cancel)
                .await
                .map_err(|error| {
                    if matches!(error, GmailError::Http(400 | 401 | 403)) {
                        StoreError::Unauthorized
                    } else {
                        StoreError::Unavailable
                    }
                })?;
            self.store
                .save_source_mail_refresh(cap, credentials.revision, tokens)
                .await?;
            credentials = self.store.source_mail_credentials(cap).await?;
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
        let value = match request {
            MailToolRequest::Search {
                query, page_token, ..
            } => serde_json::to_value(
                self.client
                    .list_messages(&b.access_token, query, page_token.as_deref(), 5, cancel)
                    .await
                    .map_err(map)?,
            )
            .map_err(|_| StoreError::Unavailable)?,
            MailToolRequest::Read { message_id, .. } => project(
                self.client
                    .get_message(&b.access_token, message_id, cancel)
                    .await
                    .map_err(map)?,
            ),
            MailToolRequest::Thread { thread_id, .. } => {
                let t = self
                    .client
                    .get_thread(&b.access_token, thread_id, cancel)
                    .await
                    .map_err(map)?;
                serde_json::json!({"id":t.id,"messages":t.messages.into_iter().map(project).collect::<Vec<_>>(),"truncated":t.truncated})
            }
            MailToolRequest::Draft { .. } => return Err(StoreError::InvalidInput),
        };
        // Revalidate after IO before returning any private data (including disconnect).
        let latest = self.store.mail_binding(user, b.id.clone()).await?;
        if latest.epoch != b.epoch {
            return Err(StoreError::Conflict);
        }
        Ok(value)
    }
    pub async fn external(
        &self,
        permit: &ExternalToolPermit,
        cancel: &CancellationToken,
    ) -> KnowledgeToolResult {
        let ExternalToolRequest::Mail(request) = permit.request() else {
            return KnowledgeToolResult::Error(ToolReadError::Unavailable);
        };
        match self.read(permit.user(), request, cancel).await {
            Ok(mut value) => {
                fn trim(value: &mut serde_json::Value) {
                    if let Some(text) = value
                        .pointer_mut("/mail/text")
                        .and_then(|v| v.as_str().map(str::to_owned))
                        && text.len() > 4096
                    {
                        let mut end = 4096;
                        while !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        value["mail"]["text"] = text[..end].into();
                        value["mail"]["text_truncated"] = true.into();
                    }
                }
                trim(&mut value);
                if let Some(messages) = value.get_mut("messages").and_then(|v| v.as_array_mut()) {
                    let truncated = messages.len() > 3;
                    if truncated {
                        messages.drain(..messages.len() - 3);
                    }
                    for m in messages {
                        trim(m)
                    }
                    if truncated {
                        value["truncated"] = true.into();
                    }
                }
                KnowledgeToolResult::External(value)
            }
            Err(StoreError::InvalidInput) => {
                KnowledgeToolResult::Error(ToolReadError::InvalidInput)
            }
            Err(_) => KnowledgeToolResult::Error(ToolReadError::Unavailable),
        }
    }
    pub(crate) async fn worker(self, stop: CancellationToken) {
        let mut tick = tokio::time::interval(Duration::from_millis(300));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {_ =stop.cancelled()=>break,_=tick.tick()=>{}}
            if !self.configured() {
                continue;
            }
            if let Ok(Some(dispatch)) = self.store.next_mail_dispatch().await {
                self.dispatch(dispatch, &stop).await;
            }
        }
    }
    async fn dispatch(&self, dispatch: MailDispatch, stop: &CancellationToken) {
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
        for _ in 0..5 {
            match self
                .store
                .finish_mail_dispatch(&dispatch, receipt.clone(), definite)
                .await
            {
                Err(StoreError::Busy) => tokio::time::sleep(Duration::from_millis(20)).await,
                _ => break,
            }
        }
    }
}
fn map(error: GmailError) -> StoreError {
    match error {
        GmailError::InvalidInput => StoreError::InvalidInput,
        _ => StoreError::Unavailable,
    }
}
fn project(m: moosuri_connectors::gmail::MailMessage) -> serde_json::Value {
    serde_json::json!({"id":m.id,"thread_id":m.thread_id,"labels":m.labels,"mail":m.mail})
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
