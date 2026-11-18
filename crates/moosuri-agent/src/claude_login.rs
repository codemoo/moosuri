//! Ephemeral, session-owned bridge to official Claude subscription login.
use crate::subscriptions::SubscriptionService;
use moosuri_connectors::{
    claude_code::{LoginCode, LoginEvent},
    model::CancellationToken,
};
use moosuri_store::{ClaudeExecutionRef, StoreError, UserContext};
use std::{
    collections::HashMap,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc};

pub(crate) type Logins = Arc<Mutex<HashMap<String, Arc<Attempt>>>>;
pub(crate) struct Attempt {
    owner: UserContext,
    reference: ClaudeExecutionRef,
    created: Instant,
    state: Mutex<Progress>,
    codes: mpsc::Sender<LoginCode>,
    cancel: CancellationToken,
}
struct Progress {
    status: &'static str,
    url: Option<String>,
}
/// Intentionally not Debug: the authorization URL contains ephemeral OAuth state.
#[derive(serde::Serialize)]
pub struct ClaudeLoginView {
    pub attempt_id: String,
    pub status: &'static str,
    pub authorization_url: Option<String>,
    pub expires_in_seconds: u64,
}
impl Attempt {
    async fn view(&self, id: String) -> ClaudeLoginView {
        let state = self.state.lock().await;
        ClaudeLoginView {
            attempt_id: id,
            status: state.status,
            authorization_url: state.url.clone(),
            expires_in_seconds: 300u64.saturating_sub(self.created.elapsed().as_secs()),
        }
    }
}
impl SubscriptionService {
    pub async fn claude_login_start(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<ClaudeLoginView, StoreError> {
        let _sync = self
            .claude_model_sync
            .try_lock()
            .map_err(|_| StoreError::Busy)?;
        let reference = self.store.claude_login_ref(u, id, revision).await?;
        let profile = self.claude_profile().await?;
        if profile.digest() != reference.profile_digest || profile.root_id() != reference.root_id {
            return Err(StoreError::Conflict);
        }
        let mut logins = self.claude_logins.lock().await;
        logins.retain(|_, a| a.created.elapsed() < Duration::from_secs(360));
        for (key, attempt) in logins.iter() {
            if attempt.reference.environment_id == reference.environment_id {
                let active = matches!(
                    attempt.state.lock().await.status,
                    "starting" | "awaiting_code" | "verifying"
                );
                if active {
                    return if u.same_session(&attempt.owner) {
                        Ok(attempt.view(key.clone()).await)
                    } else {
                        Err(StoreError::Busy)
                    };
                }
            }
        }
        if logins.len() >= 16 {
            return Err(StoreError::Busy);
        }
        let permit = self
            .logins
            .clone()
            .try_acquire_owned()
            .map_err(|_| StoreError::Busy)?;
        let (codes, mut input) = mpsc::channel(1);
        let (events, mut output) = mpsc::channel(1);
        let key = uuid::Uuid::new_v4().to_string();
        let attempt = Arc::new(Attempt {
            owner: u.clone(),
            reference,
            created: Instant::now(),
            state: Mutex::new(Progress {
                status: "starting",
                url: None,
            }),
            codes,
            cancel: CancellationToken::new(),
        });
        logins.insert(key.clone(), attempt.clone());
        let service = self.clone();
        let running = attempt.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let operation = profile.login_web(
                &running.reference.environment_id,
                &events,
                &mut input,
                &running.cancel,
            );
            tokio::pin!(operation);
            let mut tick = tokio::time::interval(Duration::from_millis(500));
            let result = loop {
                tokio::select! {biased;
                    _=service.signals.stopping().cancelled(), if !running.cancel.is_cancelled()=>running.cancel.cancel(),
                    _=tick.tick()=>{
                        if service.store.claude_login_ref(&running.owner,running.reference.id.clone(),running.reference.revision).await.is_err()
                            || running.created.elapsed()>=Duration::from_secs(300) { running.cancel.cancel(); }
                    },
                    Some(LoginEvent::AuthorizationUrl(url))=output.recv()=>{
                        let mut state=running.state.lock().await;
                        if state.status=="starting" && !running.cancel.is_cancelled() {
                            state.status="awaiting_code";state.url=Some(url);
                        }
                    },
                    result=&mut operation=>break result,
                }
            };
            let mut state = running.state.lock().await;
            state.url = None;
            state.status = if running.created.elapsed() >= Duration::from_secs(300) {
                "expired"
            } else if running.cancel.is_cancelled() {
                "cancelled"
            } else if result.is_ok() {
                "succeeded"
            } else {
                "failed"
            };
        });
        Ok(attempt.view(key).await)
    }
    async fn claude_login_attempt(
        &self,
        u: &UserContext,
        binding: &str,
        key: &str,
    ) -> Result<Arc<Attempt>, StoreError> {
        let attempt = self
            .claude_logins
            .lock()
            .await
            .get(key)
            .cloned()
            .ok_or(StoreError::NotFound)?;
        if !u.same_session(&attempt.owner) || binding != attempt.reference.id {
            return Err(StoreError::NotFound);
        }
        if let Err(error) = self
            .store
            .claude_login_ref(u, binding.into(), attempt.reference.revision)
            .await
        {
            attempt.cancel.cancel();
            return Err(error);
        }
        if attempt.created.elapsed() >= Duration::from_secs(360) {
            attempt.cancel.cancel();
            return Err(StoreError::NotFound);
        }
        Ok(attempt)
    }
    pub async fn claude_login_status(
        &self,
        u: &UserContext,
        binding: &str,
        key: String,
    ) -> Result<ClaudeLoginView, StoreError> {
        Ok(self
            .claude_login_attempt(u, binding, &key)
            .await?
            .view(key)
            .await)
    }
    pub async fn claude_login_code(
        &self,
        u: &UserContext,
        binding: &str,
        key: String,
        code: LoginCode,
    ) -> Result<ClaudeLoginView, StoreError> {
        let attempt = self.claude_login_attempt(u, binding, &key).await?;
        {
            let mut state = attempt.state.lock().await;
            if state.status != "awaiting_code"
                || attempt.cancel.is_cancelled()
                || attempt.created.elapsed() >= Duration::from_secs(300)
            {
                return Err(StoreError::Conflict);
            }
            state.status = "verifying";
            state.url = None;
            if attempt.codes.try_send(code).is_err() {
                state.status = "failed";
                attempt.cancel.cancel();
                return Err(StoreError::Unavailable);
            }
        }
        Ok(attempt.view(key).await)
    }
    pub async fn claude_login_cancel(
        &self,
        u: &UserContext,
        binding: &str,
        key: String,
    ) -> Result<ClaudeLoginView, StoreError> {
        let attempt = self.claude_login_attempt(u, binding, &key).await?;
        attempt.cancel.cancel();
        {
            let mut state = attempt.state.lock().await;
            state.url = None;
            state.status = "cancelled";
        }
        Ok(attempt.view(key).await)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Runtime;
    use moosuri_store::Store;
    async fn status(
        service: &SubscriptionService,
        u: &UserContext,
        binding: &str,
        key: &str,
        wanted: &str,
    ) {
        tokio::time::timeout(Duration::from_secs(6), async {
            loop {
                if service
                    .claude_login_status(u, binding, key.into())
                    .await
                    .unwrap()
                    .status
                    == wanted
                {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
    }
    #[tokio::test]
    async fn claude_web_login_is_session_bound_single_delivery_and_not_a_model_probe() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Synthetic".into(),
                "owner@example.test".into(),
                "Synthetic-password-2026".into(),
                false,
            )
            .await
            .unwrap();
        let u = store
            .login(
                "owner@example.test".into(),
                "Synthetic-password-2026".into(),
            )
            .await
            .unwrap()
            .user;
        let other = store
            .login(
                "owner@example.test".into(),
                "Synthetic-password-2026".into(),
            )
            .await
            .unwrap()
            .user;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        f.behavior("web-login");
        let r = store
            .create_claude_binding(
                &u,
                "Synthetic".into(),
                f.profile.digest().into(),
                f.profile.root_id(),
            )
            .await
            .unwrap();
        let runtime = Runtime::start_with_google_setup_claude(
            store.clone(),
            None,
            None,
            None,
            "https://example.test/callback".into(),
            Some(Arc::new(f.profile.clone())),
        );
        let service = runtime.subscription_service();
        let started = service
            .claude_login_start(&u, r.id.clone(), r.revision)
            .await
            .unwrap();
        let repeated = service
            .claude_login_start(&u, r.id.clone(), r.revision)
            .await
            .unwrap();
        assert_eq!(started.attempt_id, repeated.attempt_id);
        status(&service, &u, &r.id, &started.attempt_id, "awaiting_code").await;
        assert!(
            service
                .claude_login_status(&other, &r.id, started.attempt_id.clone())
                .await
                .is_err()
        );
        assert!(
            service
                .claude_login_start(&other, r.id.clone(), r.revision)
                .await
                .is_err()
        );
        let code = || LoginCode::new("synthetic-code#state".to_owned().into()).unwrap();
        service
            .claude_login_code(&u, &r.id, started.attempt_id.clone(), code())
            .await
            .unwrap();
        assert!(
            service
                .claude_login_code(&u, &r.id, started.attempt_id.clone(), code())
                .await
                .is_err()
        );
        status(&service, &u, &r.id, &started.attempt_id, "succeeded").await;
        assert!(
            service
                .claude_login_status(&u, &r.id, started.attempt_id)
                .await
                .unwrap()
                .authorization_url
                .is_none()
        );
        assert_eq!(
            store.claude_bindings(&u).await.unwrap()[0].status,
            "pending"
        );
        assert_eq!(f.launches(), 1);
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn claude_web_login_logout_cancels_and_cleans_original_process() {
        let _guard = crate::tests::TEST_LOCK.lock().await;
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Synthetic".into(),
                "owner@example.test".into(),
                "Synthetic-password-2026".into(),
                false,
            )
            .await
            .unwrap();
        let u = store
            .login(
                "owner@example.test".into(),
                "Synthetic-password-2026".into(),
            )
            .await
            .unwrap()
            .user;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        f.behavior("web-login");
        let r = store
            .create_claude_binding(
                &u,
                "Synthetic".into(),
                f.profile.digest().into(),
                f.profile.root_id(),
            )
            .await
            .unwrap();
        let runtime = Runtime::start_with_google_setup_claude(
            store.clone(),
            None,
            None,
            None,
            "https://example.test/callback".into(),
            Some(Arc::new(f.profile.clone())),
        );
        let service = runtime.subscription_service();
        let started = service
            .claude_login_start(&u, r.id.clone(), r.revision)
            .await
            .unwrap();
        status(&service, &u, &r.id, &started.attempt_id, "awaiting_code").await;
        store.logout(&u).await.unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if f.directory.path().join("removed").exists() {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap();
        assert!(
            service
                .claude_login_status(&u, &r.id, started.attempt_id)
                .await
                .is_err()
        );
        runtime.shutdown().await;
    }
}
