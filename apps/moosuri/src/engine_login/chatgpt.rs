//! Host worker for one officially enrolled Sign in with ChatGPT registration.

use moosuri_connectors::model::{
    CancellationToken, ModelClient, TransportLimits,
    chatgpt::encode_text_request,
    protocol::{ModelEvent, TextCapabilities, TextRequest},
};
use moosuri_core::ModelSource;
use moosuri_store::runtime_registry::engine::ModelChoice;
use moosuri_store::{LoginResult, Store, StoreError, SubscriptionAuthority, SubscriptionRef};
use serde::Deserialize;
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tokio::sync::{Mutex, mpsc};
use uuid::Uuid;
use zeroize::Zeroizing;

#[derive(Clone, clap::Args, Deserialize)]
#[serde(deny_unknown_fields)]
struct Enrollment {
    /// Dedicated credential store created under the ChatGPT worker's UID.
    #[arg(long)]
    store_dir: PathBuf,
    /// Local enrollment account email, independent of the provider identity.
    #[arg(long)]
    enrollment_email: String,
    /// Private file containing the local enrollment password.
    #[arg(long)]
    password_file: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Config {
    enrollment: Enrollment,
    enrollment_account: Uuid,
    binding: Uuid,
    revision: i64,
}

#[derive(clap::Args)]
pub(crate) struct InspectOptions {
    #[command(flatten)]
    enrollment: Enrollment,
}

struct Session {
    token: Zeroizing<String>,
    user: moosuri_store::UserContext,
    renew_at: Instant,
}

impl From<LoginResult> for Session {
    fn from(login: LoginResult) -> Self {
        Self {
            token: Zeroizing::new(login.token),
            user: login.user,
            renew_at: Instant::now() + Duration::from_secs(24 * 60 * 60),
        }
    }
}

fn private_parent(path: &Path) -> bool {
    let uid = unsafe { libc::geteuid() };
    path.is_absolute()
        && fs::canonicalize(path).ok().as_deref() == Some(path)
        && super::trusted_parents(path, uid)
}

impl Enrollment {
    fn open_store(&self) -> Result<Store, String> {
        let private = private_parent(&self.store_dir)
            && fs::symlink_metadata(&self.store_dir).is_ok_and(|info| {
                info.is_dir()
                    && info.uid() == unsafe { libc::geteuid() }
                    && info.mode() & 0o777 == 0o700
            });
        if !private
            || !fs::symlink_metadata(self.store_dir.join("assistant.sqlite"))
                .is_ok_and(|info| info.is_file() && info.len() > 0)
        {
            return Err("chatgpt_enrollment_store_invalid".into());
        }
        // The existing Store rejects member volumes and retains the exclusive
        // writer lock. The enrollment server must be stopped before opening.
        Store::open(&self.store_dir).map_err(|_| "chatgpt_enrollment_store_unavailable".into())
    }

    async fn login(&self, store: &Store) -> Result<Session, String> {
        if !private_parent(&self.password_file) || self.password_file.starts_with(&self.store_dir) {
            return Err("chatgpt_enrollment_password_file_invalid".into());
        }
        let password = crate::read_password_file(&self.password_file)
            .map_err(|_| "chatgpt_enrollment_password_file_invalid")?;
        let login = store
            .login(self.enrollment_email.clone(), password)
            .await
            .map_err(|_| "chatgpt_enrollment_login_failed")?;
        let session = Session::from(login);
        if session.user.password_change_required() {
            let _ = store.logout(&session.user).await;
            return Err("chatgpt_enrollment_password_change_required".into());
        }
        Ok(session)
    }
}

pub(crate) async fn inspect(options: &InspectOptions) -> Result<(), String> {
    let store = options.enrollment.open_store()?;
    let session = options.enrollment.login(&store).await?;
    let result = async {
        let bindings = store.subscriptions(&session.user).await?;
        let choices = store.model_settings(&session.user).await?.choices;
        Ok::<_, StoreError>(serde_json::json!({
            "enrollment_account": session.user.account().id,
            "bindings": bindings.into_iter().map(|binding| serde_json::json!({
                "binding": binding.id,
                "revision": binding.revision,
                "status": binding.status,
            })).collect::<Vec<_>>(),
            "models": choices.iter().filter(|choice|
                choice.source == ModelSource::PersonalChatGpt && choice.effort.is_none()
            ).map(|choice| serde_json::json!({
                "binding": choice.provider_id,
                "revision": choice.provider_revision,
                "model": choice.model,
            })).collect::<Vec<_>>(),
            "choices": choices.iter().filter(|choice| {
                choice.source == ModelSource::PersonalChatGpt
            }).map(|choice| serde_json::json!({
                "binding": choice.provider_id,
                "revision": choice.provider_revision,
                "model": choice.model,
                "effort": choice.effort,
            })).collect::<Vec<_>>(),
        }))
    }
    .await;
    let logout = store.logout(&session.user).await;
    let report = result
        .and_then(|report| logout.map(|()| report))
        .map_err(|_| "chatgpt_enrollment_inspection_failed")?;
    println!("{report}");
    Ok(())
}

pub(super) struct Adapter {
    store: Store,
    enrollment: Enrollment,
    session: Mutex<Session>,
    bindings: Vec<SubscriptionRef>,
    #[cfg(all(test, feature = "test-fixtures"))]
    fixture: Option<String>,
}

impl Adapter {
    pub(super) async fn open(config: Config, choices: &[ModelChoice]) -> Result<Self, String> {
        if config.revision < 1 || choices.iter().any(|choice| choice.model == "catalog") {
            return Err("chatgpt_engine_model_invalid".into());
        }
        let store = config.enrollment.open_store()?;
        let session = config.enrollment.login(&store).await?;
        let bindings = match Self::owned_models(&store, &session, &config, choices).await {
            Ok(bindings) => bindings,
            Err(error) => {
                let _ = store.logout(&session.user).await;
                return Err(error);
            }
        };
        Ok(Self {
            store,
            enrollment: config.enrollment,
            session: Mutex::new(session),
            bindings,
            #[cfg(all(test, feature = "test-fixtures"))]
            fixture: None,
        })
    }

    async fn owned_models(
        store: &Store,
        session: &Session,
        config: &Config,
        configured: &[ModelChoice],
    ) -> Result<Vec<SubscriptionRef>, String> {
        if session.user.account().id != config.enrollment_account.to_string() {
            return Err("chatgpt_engine_owner_mismatch".into());
        }
        let binding_id = config.binding.to_string();
        let choices = store
            .model_settings(&session.user)
            .await
            .map_err(|_| "chatgpt_engine_binding_unavailable")?
            .choices;
        let mut bindings = Vec::with_capacity(configured.len());
        for configured in configured {
            if !choices.iter().any(|choice| {
                choice.source == ModelSource::PersonalChatGpt
                    && choice.provider_id == binding_id
                    && choice.provider_revision == config.revision
                    && choice.model == configured.model
                    && choice.effort == configured.effort
            }) {
                return Err("chatgpt_engine_model_unverified".into());
            }
            let binding = store
                .subscription_ref(
                    &session.user,
                    binding_id.clone(),
                    config.revision,
                    configured.model.clone(),
                    configured.effort.clone(),
                )
                .await
                .map_err(|_| "chatgpt_engine_binding_unavailable")?;
            bindings.push(binding);
        }
        Ok(bindings)
    }

    async fn authority(&self) -> Result<SubscriptionAuthority, String> {
        let mut session = self.session.lock().await;
        // Keep the engine's local application session renewed independently of
        // model credential rotation. Store revalidates it during each lookup.
        if Instant::now() >= session.renew_at {
            match self.store.authenticate(session.token.to_string()).await {
                Ok(user) => session.user = user,
                // A worker that was idle past the normal session lifetime can
                // open a fresh local login using its private service password.
                Err(StoreError::Unauthorized) => {
                    let fresh = self.enrollment.login(&self.store).await?;
                    if fresh.user.account().id != session.user.account().id {
                        let _ = self.store.logout(&fresh.user).await;
                        return Err("chatgpt_engine_owner_mismatch".into());
                    }
                    *session = fresh;
                }
                Err(_) => return Err("chatgpt_enrollment_login_failed".into()),
            }
            session.renew_at = Instant::now() + Duration::from_secs(24 * 60 * 60);
        }
        Ok(SubscriptionAuthority::User(session.user.clone()))
    }

    async fn client(
        &self,
        authority: SubscriptionAuthority,
        binding: SubscriptionRef,
    ) -> Result<ModelClient, StoreError> {
        #[cfg(all(test, feature = "test-fixtures"))]
        if let Some(origin) = &self.fixture {
            return moosuri_agent::subscriptions::personal_client_fixture(
                &self.store,
                authority,
                binding,
                origin,
            )
            .await;
        }
        moosuri_agent::subscriptions::personal_client(&self.store, authority, binding).await
    }

    pub(super) async fn stream(
        &self,
        request: &TextRequest,
        tx: &mpsc::Sender<ModelEvent>,
        cancel: &CancellationToken,
    ) -> Result<(), String> {
        if cancel.is_cancelled() || tx.is_closed() {
            return Err("chatgpt_engine_cancelled".into());
        }
        let binding = self
            .bindings
            .iter()
            .find(|binding| binding.model == request.model && binding.effort == request.effort)
            .ok_or("chatgpt_engine_request_invalid")?;
        let caps = TextCapabilities {
            efforts: binding.effort.iter().cloned().collect(),
        };
        if encode_text_request(request, &caps).is_err() {
            return Err("chatgpt_engine_request_invalid".into());
        }
        let authority = self.authority().await?;
        if cancel.is_cancelled() || tx.is_closed() {
            return Err("chatgpt_engine_cancelled".into());
        }
        // Resolve the current sealed subscription and its single-flight refresh
        // to completion. Caller cancellation fences inference below rather than
        // abandoning a refresh that may already have rotated at OpenAI.
        let client = self
            .client(authority, binding.clone())
            .await
            .map_err(|_| "chatgpt_engine_credential_unavailable")?
            .with_limits(TransportLimits {
                connect: Duration::from_secs(5),
                idle: Duration::from_secs(20),
                total: Duration::from_secs(60),
                response_bytes: 512 * 1024,
            })
            .map_err(|_| "chatgpt_engine_model_invalid")?;
        if cancel.is_cancelled() || tx.is_closed() {
            return Err("chatgpt_engine_cancelled".into());
        }
        client
            .stream_text(request, tx, cancel)
            .await
            .map_err(|_| "chatgpt_engine_inference_failed".into())
    }
}

#[cfg(all(test, feature = "test-fixtures"))]
mod tests;
