//! Synthetic official-CLI protocol through the real mission supervisor and Store.
//! These tests do not invoke a real CLI/account or establish provider eligibility.
use super::*;
use crate::Runtime;
use moosuri_core::{MissionCheck, MissionCreate, MissionMode, ModelSelection, ModelSource};
use moosuri_store::UserContext;
use std::sync::Arc;

const EMAIL: &str = "mission-engine@example.test";
const PASSWORD: &str = "Synthetic-mission-engine-password-2026";

async fn setup() -> (
    tempfile::TempDir,
    Store,
    UserContext,
    moosuri_connectors::claude_code::fixture::Fixture,
) {
    let directory = tempfile::tempdir().unwrap();
    let store = Store::open(directory.path()).unwrap();
    store
        .bootstrap(
            "Mission engine fixture".into(),
            EMAIL.into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let user = store
        .login(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let fixture = moosuri_connectors::claude_code::fixture::Fixture::new();
    let reference = store
        .create_claude_binding(
            &user,
            "Synthetic official CLI".into(),
            fixture.profile.digest().into(),
            fixture.profile.root_id(),
        )
        .await
        .unwrap();
    let choice = store
        .record_claude_probe(&user, reference.clone(), "claude-fixture-v1".into())
        .await
        .unwrap();
    store
        .set_model_preference(
            &user,
            Some(ModelSelection {
                source: ModelSource::PersonalClaudeCode,
                provider_id: choice.provider_id,
                provider_revision: choice.provider_revision,
                model: choice.model,
                effort: None,
            }),
        )
        .await
        .unwrap();
    let (changes, _) = tokio::sync::watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(tokio::sync::Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mut service = crate::subscriptions::SubscriptionService::new(store.clone(), signals);
    service.claude = Some(Arc::new(fixture.profile.clone()));
    fixture.behavior("tool-bridge-receipt");
    assert!(
        service
            .claude_tool_probe(
                &user,
                reference.id,
                reference.revision,
                "claude-fixture-v1".into(),
                None,
            )
            .await
            .unwrap()
            .function_tools
    );
    assert_eq!(fixture.launches(), 2);
    // Probe rounds and mission rounds have independent synthetic decisions.
    std::fs::write(fixture.directory.path().join("bridge-turns"), "0").unwrap();
    fixture.behavior("tool-bridge-mission");
    (directory, store, user, fixture)
}

async fn create(store: &Store, user: &UserContext, mode: MissionMode) -> MissionView {
    store
        .create_mission(
            user,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: "Calculate the exact sum of 917.13 and 409.15".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::DecimalEquals {
                    expected: "1326.28".into(),
                }],
                execution_mode: mode,
            },
        )
        .await
        .unwrap()
}

async fn terminal(store: &Store, user: &UserContext) -> MissionView {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = store.missions(user).await.unwrap().remove(0);
            if !["queued", "planning", "executing", "verifying"].contains(&view.state.as_str()) {
                return view;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}

fn runtime(
    store: Store,
    profile: Option<Arc<moosuri_connectors::claude_code::Profile>>,
) -> Runtime {
    Runtime::start_with_google_setup_claude(
        store,
        None,
        None,
        None,
        "https://example.test/callback".into(),
        profile,
    )
}

#[tokio::test]
async fn verified_claude_runs_current_and_durable_missions_from_actual_receipts() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for durable in [false, true] {
        let (_directory, store, user, fixture) = setup().await;
        let created = create(
            &store,
            &user,
            if durable {
                MissionMode::Durable
            } else {
                MissionMode::CurrentSession
            },
        )
        .await;
        let user = if durable {
            store.logout(&user).await.unwrap();
            store
                .login(EMAIL.into(), PASSWORD.into())
                .await
                .unwrap()
                .user
        } else {
            user
        };
        let before = store.usage_summary(&user).await.unwrap().dispatched_calls;
        let runtime = runtime(store.clone(), Some(Arc::new(fixture.profile.clone())));
        runtime.wake();
        let done = terminal(&store, &user).await;
        assert_eq!(done.id, created.id);
        assert_eq!(done.expires_at_ms, created.expires_at_ms);
        assert_eq!(done.state, "achieved", "{:?}", done.report);
        let candidate = done.result.unwrap();
        assert_eq!(candidate.text, "The exact sum is 1326.28.");
        assert_eq!(candidate.evidence.len(), 1);
        assert_eq!(candidate.evidence[0].step, 1);
        assert!(candidate.evidence[0].call_id.starts_with("moosuri_"));
        assert!(
            done.receipts
                .iter()
                .any(|receipt| receipt.detail.contains("1326.28"))
        );
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls - before,
            2
        );
        assert_eq!(fixture.launches(), 4);
        runtime.shutdown().await;
    }
}

#[tokio::test]
async fn claude_mission_rejects_unavailable_profile_and_unbound_round_without_tool_receipts() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for missing_profile in [true, false] {
        let (_directory, store, user, fixture) = setup().await;
        create(&store, &user, MissionMode::Durable).await;
        fixture.behavior("tool-bridge-wrong-id");
        let before = store.usage_summary(&user).await.unwrap().dispatched_calls;
        let profile = (!missing_profile).then(|| Arc::new(fixture.profile.clone()));
        let runtime = runtime(store.clone(), profile);
        runtime.wake();
        let done = terminal(&store, &user).await;
        assert_eq!(done.state, "failed", "{:?}", done.report);
        assert!(done.result.is_none());
        assert!(done.receipts.is_empty());
        let dispatched = i64::from(!missing_profile);
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls - before,
            dispatched
        );
        assert_eq!(fixture.launches(), 2 + dispatched as usize);
        runtime.shutdown().await;
    }
}
