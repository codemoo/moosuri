//! Official CLI text setup/probe. No credentials are read by this service.
use crate::*;
use moosuri_connectors::{
    claude_code::Profile,
    model::protocol::{FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole},
};
use moosuri_store::{ClaudeExecutionRef, UserContext};
#[derive(serde::Serialize)]
pub struct ClaudeModelsView {
    pub status: &'static str,
    pub models: Vec<String>,
}
impl crate::subscriptions::SubscriptionService {
    pub async fn claude_profile(&self) -> Result<Arc<Profile>, StoreError> {
        let p = self.claude.clone().ok_or(StoreError::Unavailable)?;
        let ready = self
            .claude_ready
            .get_or_init(|| async { p.cleanup_stale().await.map_err(|_| ()) })
            .await;
        if ready.is_err() {
            return Err(StoreError::Unavailable);
        }
        Ok(p)
    }
    /// Recover already signed-in pending bindings as well as newly completed logins.
    /// A catalog row alone never becomes a selectable model.
    pub async fn claude_models(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<ClaudeModelsView, StoreError> {
        self.claude_models_refresh(u, id, revision, false).await
    }
    pub async fn claude_models_refresh(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        force: bool,
    ) -> Result<ClaudeModelsView, StoreError> {
        // Serialize discovery/probes before reading saved successes, so refreshes
        // and concurrent tabs cannot duplicate inference for this runtime.
        let _sync = self
            .claude_model_sync
            .try_lock()
            .map_err(|_| StoreError::Busy)?;
        let r = self
            .store
            .claude_launch_ref(u, id.clone(), revision)
            .await?;
        let p = self.claude_profile().await?;
        if p.digest() != r.profile_digest || p.root_id() != r.root_id {
            return Err(StoreError::Conflict);
        }
        let mut models = self
            .store
            .model_settings(u)
            .await?
            .choices
            .into_iter()
            .filter(|c| {
                c.source == moosuri_core::ModelSource::PersonalClaudeCode
                    && c.provider_id == id
                    && c.provider_revision == revision
            })
            .map(|c| c.model)
            .collect::<Vec<_>>();
        if !models.is_empty() && !force {
            return Ok(ClaudeModelsView {
                status: "ready",
                models,
            });
        }
        let cancel = CancellationToken::new();
        let operation = p.account_models(&r.environment_id, &cancel);
        tokio::pin!(operation);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let catalog = loop {
            tokio::select! {biased;
                _=self.signals.stopping().cancelled(),if !cancel.is_cancelled()=>cancel.cancel(),
                _=tick.tick()=>{if self.store.claude_launch_ref(u,r.id.clone(),r.revision).await.is_err(){cancel.cancel();}},
                result=&mut operation=>break result,
            }
        };
        // Recheck owner/session/revision after the subprocess before any result.
        self.store
            .claude_launch_ref(u, id.clone(), revision)
            .await?;
        let catalog = match catalog {
            Ok(catalog) if !cancel.is_cancelled() => catalog,
            _ => {
                return Ok(ClaudeModelsView {
                    status: "unavailable",
                    models: vec![],
                });
            }
        };
        if catalog.login_required {
            return Ok(ClaudeModelsView {
                status: "login_required",
                models: vec![],
            });
        }
        // Connector emits at most three unique standard-tier concrete IDs.
        // Failed/unknown attempts are still admitted by the durable probe budget.
        for model in catalog.models.into_iter().take(3) {
            if models.contains(&model) {
                continue;
            }
            if self
                .claude_probe_inner(u, id.clone(), revision, model.clone())
                .await
                .is_ok()
            {
                models.push(model);
            }
        }
        self.store.claude_launch_ref(u, id, revision).await?;
        Ok(ClaudeModelsView {
            status: if models.is_empty() {
                "unavailable"
            } else {
                "ready"
            },
            models,
        })
    }
    pub async fn claude_probe(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        model: String,
    ) -> Result<moosuri_core::ModelChoice, StoreError> {
        let _sync = self
            .claude_model_sync
            .try_lock()
            .map_err(|_| StoreError::Busy)?;
        self.claude_probe_inner(u, id, revision, model).await
    }
    async fn claude_probe_inner(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
        model: String,
    ) -> Result<moosuri_core::ModelChoice, StoreError> {
        // Ownership precedes profile verification/launch.
        let r = self.store.claude_launch_ref(u, id, revision).await?;
        let p = self.claude_profile().await?;
        if p.digest() != r.profile_digest || p.root_id() != r.root_id {
            return Err(StoreError::Conflict);
        }
        if let Some(choice) = self
            .store
            .model_settings(u)
            .await?
            .choices
            .into_iter()
            .find(|c| {
                c.source == moosuri_core::ModelSource::PersonalClaudeCode
                    && c.provider_id == r.id
                    && c.provider_revision == revision
                    && c.model == model
            })
        {
            return Ok(choice);
        }
        let request = TextRequest {
            model: model.clone(),
            effort: None,
            max_output_tokens: 2048,
            messages: vec![TextMessage {
                role: TextRole::User,
                text: "Reply with the single word OK.".into(),
                phase: None,
            }],
        };
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::channel(16);
        moosuri_connectors::claude_code::protocol::input(
            &request,
            "00000000-0000-4000-8000-000000000001",
            "00000000-0000-4000-8000-000000000002",
        )
        .map_err(|_| StoreError::InvalidInput)?;
        self.store.reserve_claude_probe(u, &r).await?;
        let operation = p.stream_text(&r.environment_id, &request, &tx, &cancel);
        tokio::pin!(operation);
        let mut tick = tokio::time::interval(Duration::from_millis(250));
        let (mut identity, mut text, mut complete) = (false, false, false);
        let result = loop {
            tokio::select! {biased;_=self.signals.stopping().cancelled(),if !cancel.is_cancelled()=>{cancel.cancel();},_=tick.tick()=>{if self.store.claude_launch_ref(u,r.id.clone(),r.revision).await.is_err(){cancel.cancel();}},Some(e)=rx.recv()=>match e{ModelEvent::Started{model:Some(actual),..}=>identity=actual==model,ModelEvent::TextDelta{text:t,..}=>text|=!t.trim().is_empty(),ModelEvent::Finished{outcome:FinishOutcome::Completed,..}=>complete=true,_=>{}},r=&mut operation=>break r}
        };
        while let Ok(e) = rx.try_recv() {
            match e {
                ModelEvent::Started {
                    model: Some(actual),
                    ..
                } => identity = actual == model,
                ModelEvent::TextDelta { text: t, .. } => text |= !t.trim().is_empty(),
                ModelEvent::Finished {
                    outcome: FinishOutcome::Completed,
                    ..
                } => complete = true,
                _ => {}
            }
        }
        if result.is_err() || !identity || !text || !complete || cancel.is_cancelled() {
            return Err(StoreError::Unavailable);
        }
        self.store.record_claude_probe(u, r.clone(), model).await
    }
    pub async fn claude_disconnect(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<(), StoreError> {
        let reference = self
            .store
            .claude_launch_ref(u, id.clone(), revision)
            .await?;
        self.store.disconnect_claude(u, id, revision).await?;
        if let Some(p) = &self.claude
            && p.root_id() == reference.root_id
        {
            let _ = p.block_environment(&reference.environment_id);
        }
        self.signals.changed();
        self.signals.wake.notify_one();
        Ok(())
    }
    pub(crate) async fn claude_retirement_worker(&self) {
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            tokio::select! {_=self.signals.stopping().cancelled()=>break,_=tick.tick()=>{}}
            let Some(p) = &self.claude else { continue };
            let Ok(environments) = self.store.claude_retirements(p.root_id()).await else {
                continue;
            };
            for env in environments {
                if self.signals.stopping().is_cancelled() {
                    return;
                }
                let retired = tokio::select! {_=self.signals.stopping().cancelled()=>return,r=p.retire_environment(&env)=>r};
                if retired.is_ok() {
                    let _ = self
                        .store
                        .acknowledge_claude_retirement(env, p.root_id())
                        .await;
                    self.signals.changed();
                }
            }
        }
    }
    pub async fn claude_native_ref(
        &self,
        u: &UserContext,
        id: String,
        revision: i64,
    ) -> Result<ClaudeExecutionRef, StoreError> {
        let r = self.store.claude_login_ref(u, id, revision).await?;
        let p = self.claude_profile().await?;
        if p.digest() != r.profile_digest || p.root_id() != r.root_id {
            return Err(StoreError::Conflict);
        }
        Ok(r)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moosuri_core::{ModelSelection, ModelSource, RunState};
    #[path = "recovery_test.rs"]
    mod recovery;
    async fn setup() -> (tempfile::TempDir, Store, moosuri_store::UserContext) {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Synthetic Claude user".into(),
                "claude@example.test".into(),
                "Synthetic-Claude-password-2026".into(),
                false,
            )
            .await
            .unwrap();
        let u = store
            .login(
                "claude@example.test".into(),
                "Synthetic-Claude-password-2026".into(),
            )
            .await
            .unwrap()
            .user;
        (dir, store, u)
    }
    async fn select(
        store: &Store,
        u: &moosuri_store::UserContext,
        p: &Profile,
    ) -> moosuri_store::ClaudeExecutionRef {
        let r = store
            .create_claude_binding(u, "Official fixture".into(), p.digest().into(), p.root_id())
            .await
            .unwrap();
        let c = store
            .record_claude_probe(u, r.clone(), "claude-fixture-v1".into())
            .await
            .unwrap();
        store
            .set_model_preference(
                u,
                Some(ModelSelection {
                    source: ModelSource::PersonalClaudeCode,
                    provider_id: c.provider_id,
                    provider_revision: c.provider_revision,
                    model: c.model,
                    effort: None,
                }),
            )
            .await
            .unwrap();
        r
    }
    async fn terminal(
        store: &Store,
        u: &moosuri_store::UserContext,
        thread: &str,
    ) -> moosuri_core::ChatTurn {
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let page = store.conversation(u, thread.into(), None).await.unwrap();
                if !page.turns[0].run.state.active() {
                    return page.turns[0].clone();
                }
                tokio::time::sleep(Duration::from_millis(25)).await;
            }
        })
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn account_models_recover_pending_and_reuse_verified_choices_without_inference() {
        let _g = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, u) = setup().await;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        let r = store
            .create_claude_binding(
                &u,
                "Signed in before repair".into(),
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
        let result = service
            .claude_models(&u, r.id.clone(), r.revision)
            .await
            .unwrap();
        assert_eq!(result.status, "ready");
        assert_eq!(
            result.models,
            [
                "claude-fixture-v1",
                "claude-fixture-sonnet",
                "claude-fixture-haiku"
            ]
        );
        assert_eq!(
            f.launches(),
            4,
            "one non-inference catalog and three exact probes"
        );
        assert_eq!(store.claude_bindings(&u).await.unwrap()[0].status, "active");
        let repeat = service
            .claude_models(&u, r.id.clone(), r.revision)
            .await
            .unwrap();
        assert_eq!(repeat.status, "ready");
        assert_eq!(repeat.models.len(), 3);
        assert_eq!(f.launches(), 4);
        assert!(
            service
                .claude_models(&u, r.id.clone(), r.revision + 1)
                .await
                .is_err()
        );
        assert_eq!(f.launches(), 4);
        runtime.shutdown().await;
        // Saved eligibility survives runtime restart without another catalog/probe.
        let runtime = Runtime::start_with_google_setup_claude(
            store.clone(),
            None,
            None,
            None,
            "https://example.test/callback".into(),
            Some(Arc::new(f.profile.clone())),
        );
        assert_eq!(
            runtime
                .subscription_service()
                .claude_models(&u, r.id, r.revision)
                .await
                .unwrap()
                .status,
            "ready"
        );
        assert_eq!(f.launches(), 4);
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn logged_out_catalog_and_wrong_identity_never_create_eligible_models() {
        let _g = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, u) = setup().await;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        let r = store
            .create_claude_binding(
                &u,
                "Pending".into(),
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
        f.behavior("logged-out");
        for _ in 0..2 {
            let result = service
                .claude_models(&u, r.id.clone(), r.revision)
                .await
                .unwrap();
            assert_eq!(result.status, "login_required");
            assert!(result.models.is_empty());
        }
        assert_eq!(
            f.launches(),
            2,
            "refresh only initializes; no unauthenticated inference"
        );
        f.behavior("identity-mismatch");
        let result = service.claude_models(&u, r.id, r.revision).await.unwrap();
        assert_eq!(result.status, "unavailable");
        assert!(result.models.is_empty());
        assert!(store.model_settings(&u).await.unwrap().choices.is_empty());
        assert_eq!(
            store.claude_bindings(&u).await.unwrap()[0].status,
            "pending"
        );
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn manual_and_forced_sync_reuse_proof_and_repair_only_missing_tiers() {
        let _g = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, u) = setup().await;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        let r = store
            .create_claude_binding(
                &u,
                "Partial".into(),
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
        let old = store
            .create_claude_binding(
                &u,
                "Previously verified".into(),
                f.profile.digest().into(),
                f.profile.root_id(),
            )
            .await
            .unwrap();
        store
            .record_claude_probe(&u, old.clone(), "claude-fixture-v1".into())
            .await
            .unwrap();
        assert!(
            service
                .claude_native_ref(&u, old.id.clone(), old.revision)
                .await
                .is_err()
        );
        assert!(
            service
                .claude_login_start(&u, old.id, old.revision)
                .await
                .is_err()
        );
        assert_eq!(
            f.launches(),
            0,
            "legacy proof is guarded before any official login launch"
        );
        f.behavior("partial");
        let first = service
            .claude_models(&u, r.id.clone(), r.revision)
            .await
            .unwrap();
        assert_eq!(first.models.len(), 2);
        assert_eq!(f.launches(), 4);
        service
            .claude_probe(&u, r.id.clone(), r.revision, first.models[0].clone())
            .await
            .unwrap();
        assert_eq!(f.launches(), 4, "manual probe reuses exact success");
        // A verified binding cannot mutate its credential environment via either
        // login entry point, including proofs created before marker support.
        assert!(
            service
                .claude_native_ref(&u, r.id.clone(), r.revision)
                .await
                .is_err()
        );
        assert!(
            service
                .claude_login_start(&u, r.id.clone(), r.revision)
                .await
                .is_err()
        );
        {
            let _guard = service.claude_model_sync.lock().await;
            assert!(
                service
                    .claude_probe(&u, r.id.clone(), r.revision, "claude-fixture-v1".into())
                    .await
                    .is_err()
            );
            assert!(
                service
                    .claude_models(&u, r.id.clone(), r.revision)
                    .await
                    .is_err()
            );
        }
        f.behavior("ok");
        let repaired = service
            .claude_models_refresh(&u, r.id.clone(), r.revision, true)
            .await
            .unwrap();
        assert_eq!(repaired.models.len(), 3);
        assert_eq!(
            f.launches(),
            6,
            "one catalog plus only previously failed tier"
        );
        service
            .claude_models_refresh(&u, r.id.clone(), r.revision, true)
            .await
            .unwrap();
        assert_eq!(
            f.launches(),
            7,
            "explicit refresh reads catalog but reuses all proof"
        );
        runtime.shutdown().await;
        let incompatible = moosuri_connectors::claude_code::fixture::Fixture::new();
        let runtime = Runtime::start_with_google_setup_claude(
            store,
            None,
            None,
            None,
            "https://example.test/callback".into(),
            Some(Arc::new(incompatible.profile.clone())),
        );
        assert!(
            runtime
                .subscription_service()
                .claude_models(&u, r.id.clone(), r.revision)
                .await
                .is_err()
        );
        assert_eq!(
            incompatible.launches(),
            0,
            "profile/root mismatch cannot return saved eligibility"
        );
        runtime.shutdown().await;
    }
    #[tokio::test]
    async fn saved_runtime_text_uses_real_supervisor_and_retains_usage() {
        let _g = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, u) = setup().await;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        select(&store, &u, &f.profile).await;
        let thread = store
            .create_thread(&u, "Saved Claude conversation".into())
            .await
            .unwrap();
        store
            .submit_turn(
                &u,
                thread.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Hello".into(),
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
        let turn = terminal(&store, &u, &thread.id).await;
        assert_eq!(turn.run.state, RunState::Completed);
        assert_eq!(
            turn.assistant
                .iter()
                .map(|v| v.text.as_str())
                .collect::<String>(),
            "OK"
        );
        assert_eq!(turn.run.model.source, ModelSource::PersonalClaudeCode);
        assert_eq!(turn.run.usage.input_tokens, Some(8));
        assert_eq!(f.launches(), 1);
        runtime.shutdown().await;
        assert_eq!(
            store.conversation(&u, thread.id, None).await.unwrap().turns[0]
                .run
                .state,
            RunState::Completed
        );
    }
    #[tokio::test]
    async fn truncated_result_is_unknown_and_never_automatically_replayed() {
        let _g = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, u) = setup().await;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        f.behavior("truncated");
        select(&store, &u, &f.profile).await;
        let thread = store
            .create_thread(&u, "Unknown Claude invocation".into())
            .await
            .unwrap();
        store
            .submit_turn(
                &u,
                thread.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Hello".into(),
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
        let turn = terminal(&store, &u, &thread.id).await;
        assert_eq!(turn.run.state, RunState::OutcomeUnknown);
        assert_eq!(turn.run.usage.input_tokens, None);
        runtime.shutdown().await;
        let runtime = Runtime::start_with_google_setup_claude(
            store.clone(),
            None,
            None,
            None,
            "https://example.test/callback".into(),
            Some(Arc::new(f.profile.clone())),
        );
        tokio::time::sleep(Duration::from_millis(150)).await;
        runtime.shutdown().await;
        assert_eq!(f.launches(), 1);
    }
    #[tokio::test]
    async fn binding_revoke_cancels_running_child_without_late_output() {
        let _g = crate::tests::TEST_LOCK.lock().await;
        let (_dir, store, u) = setup().await;
        let f = moosuri_connectors::claude_code::fixture::Fixture::new();
        f.behavior("hold");
        let binding = select(&store, &u, &f.profile).await;
        let thread = store
            .create_thread(&u, "Revoked Claude invocation".into())
            .await
            .unwrap();
        store
            .submit_turn(
                &u,
                thread.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Hello".into(),
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
        tokio::time::timeout(Duration::from_secs(5), async {
            while !f.directory.path().join("entered").exists() {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        runtime
            .subscription_service()
            .claude_disconnect(&u, binding.id, 1)
            .await
            .unwrap();
        let turn = terminal(&store, &u, &thread.id).await;
        assert_ne!(turn.run.state, RunState::Completed);
        assert!(turn.assistant.is_empty());
        runtime.shutdown().await;
        assert!(f.directory.path().join("removed").exists());
        assert_eq!(f.launches(), 1);
    }
}
