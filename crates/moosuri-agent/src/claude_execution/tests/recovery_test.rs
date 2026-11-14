use super::*;
use std::os::unix::fs::PermissionsExt;

#[tokio::test]
async fn claude_restored_probe_admission_precedes_process_and_last_chat_can_finish() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let source = root.path().join("source");
    let dest = root.path().join("restored");
    let kit = root.path().join("kit");
    let output = root.path().join("output");
    for dir in [&kit, &output] {
        std::fs::create_dir(dir).unwrap();
        std::fs::set_permissions(dir, std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let password = "Synthetic-Claude-recovery-password-2026";
    let store = Store::open(&source).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), password.into())
        .await
        .unwrap()
        .user;
    let account = owner.account().id.clone();
    drop(store);
    let identity = kit.join("identity");
    let recipient = moosuri_store::backup::keygen(&identity).unwrap();
    moosuri_store::backup::setup(&source, &recipient, &identity).unwrap();
    let export = output.join("source.age");
    moosuri_store::backup::create(&source, &export).unwrap();
    let journal = output.join("journal.age");
    let head = kit.join("head.json");
    let published =
        moosuri_store::backup::publish_journal(&source, &export, &journal, &head).unwrap();
    moosuri_store::backup::restore_historical(
        &export,
        &journal,
        &head,
        &published.trusted_head_sha256,
        &identity,
        &identity,
        &dest,
    )
    .unwrap();
    moosuri_store::recover_account(
        &dest,
        account,
        moosuri_core::Role::Admin,
        "Synthetic-temporary-password-2026".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&dest).unwrap();
    let restricted = store
        .login(
            "owner@example.test".into(),
            "Synthetic-temporary-password-2026".into(),
        )
        .await
        .unwrap();
    let owner = store
        .change_password(&restricted.user, password.into())
        .await
        .unwrap()
        .user;
    let f = moosuri_connectors::claude_code::fixture::Fixture::new();
    let runtime = Runtime::start_with_google_setup_claude(
        store.clone(),
        None,
        None,
        None,
        "https://example.test/callback".into(),
        Some(Arc::new(f.profile.clone())),
    );
    let service = runtime.subscription_service();
    let binding = store
        .create_claude_binding(
            &owner,
            "Synthetic CLI".into(),
            f.profile.digest().into(),
            f.profile.root_id(),
        )
        .await
        .unwrap();
    assert!(matches!(
        service
            .claude_probe(&owner, binding.id.clone(), 1, "claude-fixture-v1".into())
            .await,
        Err(StoreError::RecoveryAllocationRequired)
    ));
    assert_eq!(f.launches(), 0);
    let restore = store
        .recovery_budget_status(&owner)
        .await
        .unwrap()
        .restore_id
        .unwrap();
    runtime.shutdown().await;
    drop(service);
    drop(runtime);
    drop(store);
    moosuri_store::recovery_budget::allocate(
        &dest,
        &restore,
        &uuid::Uuid::new_v4().to_string(),
        3,
        0,
    )
    .await
    .unwrap();
    let store = Store::open(&dest).unwrap();
    let owner = store
        .login("owner@example.test".into(), password.into())
        .await
        .unwrap()
        .user;
    let runtime = Runtime::start_with_google_setup_claude(
        store.clone(),
        None,
        None,
        None,
        "https://example.test/callback".into(),
        Some(Arc::new(f.profile.clone())),
    );
    let service = runtime.subscription_service();
    // Invalid local input and a stale environment never spend a slot or launch.
    assert!(matches!(
        service
            .claude_probe(&owner, binding.id.clone(), 1, "invalid model".into())
            .await,
        Err(StoreError::InvalidInput)
    ));
    let mut stale = binding.clone();
    stale.environment_id = uuid::Uuid::new_v4().to_string();
    assert!(matches!(
        store.reserve_claude_probe(&owner, &stale).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .recovery_budget_status(&owner)
            .await
            .unwrap()
            .committed_calls,
        0
    );
    assert_eq!(f.launches(), 0);
    // Unknown outcomes consume; they cannot manufacture a verified model.
    f.behavior("truncated");
    assert!(
        service
            .claude_probe(&owner, binding.id.clone(), 1, "claude-fixture-v1".into())
            .await
            .is_err()
    );
    assert_eq!(
        store
            .recovery_budget_status(&owner)
            .await
            .unwrap()
            .committed_calls,
        1
    );
    assert!(
        store
            .model_settings(&owner)
            .await
            .unwrap()
            .effective
            .is_none()
    );
    f.behavior("ok");
    let choice = service
        .claude_probe(&owner, binding.id.clone(), 1, "claude-fixture-v1".into())
        .await
        .unwrap();
    store
        .set_model_preference(
            &owner,
            Some(ModelSelection {
                source: choice.source,
                provider_id: choice.provider_id,
                provider_revision: choice.provider_revision,
                model: choice.model,
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&owner, "Last admitted Claude chat".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &owner,
            thread.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Hello".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        terminal(&store, &owner, &thread.id).await.run.state,
        RunState::Completed
    );
    assert!(matches!(
        service
            .claude_probe(&owner, binding.id, 1, "claude-fixture-v1".into())
            .await,
        Err(StoreError::RecoveryAllowanceExhausted)
    ));
    assert_eq!(f.launches(), 3);
    assert_eq!(
        store
            .recovery_budget_status(&owner)
            .await
            .unwrap()
            .committed_calls,
        3
    );
    assert!(store.usage_summary(&owner).await.is_err());
    runtime.shutdown().await;
}
