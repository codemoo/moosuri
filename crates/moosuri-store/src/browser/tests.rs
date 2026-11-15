use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
async fn setup() -> (
    tempfile::TempDir,
    Store,
    UserContext,
    String,
    BrowserWorkspaceView,
) {
    let dir = directory();
    let store = Store::open_with_web(
        dir.path(),
        moosuri_core::WebToolProfile {
            read: false,
            search: false,
            browser_pin: Some("fixture-pin".into()),
        },
    )
    .unwrap();
    let user = admin(&store).await.user;
    let thread = store
        .create_thread(&user, "Private browser task".into())
        .await
        .unwrap()
        .id;
    let view = store
        .begin_browser_workspace(&user, thread.clone(), "fixture-pin".into())
        .await
        .unwrap();
    let view = store
        .activate_browser_workspace(
            &user,
            thread.clone(),
            view.id,
            "https://example.com/".into(),
            "Fixture".into(),
        )
        .await
        .unwrap();
    (dir, store, user, thread, view)
}
#[tokio::test]
async fn takeover_fences_model_other_tab_and_other_account_then_resumes_without_navigation() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread, view) = setup().await;
    let other = member(&store, &user, "other@example.test").await.user;
    assert!(matches!(
        store.browser_workspace(&other, thread.clone()).await,
        Err(StoreError::NotFound)
    ));
    let first = store
        .take_browser_control(&user, thread.clone(), view.id.clone(), view.revision)
        .await
        .unwrap();
    let v = first.workspace;
    for access in [
        BrowserAccess::AgentObserve,
        BrowserAccess::AgentNavigate,
        BrowserAccess::Watch,
    ] {
        assert!(
            store
                .check_browser_workspace(&user, thread.clone(), v.id.clone(), v.revision, access)
                .await
                .is_err()
        );
    }
    let second = store
        .take_browser_control(&user, thread.clone(), v.id.clone(), v.revision)
        .await
        .unwrap();
    assert!(
        store
            .check_browser_workspace(
                &user,
                thread.clone(),
                v.id.clone(),
                v.revision,
                BrowserAccess::Human(first.control_token)
            )
            .await
            .is_err()
    );
    let v = second.workspace;
    let new_session = store
        .login(user.account.email.clone(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        store
            .check_browser_workspace(
                &new_session,
                thread.clone(),
                v.id.clone(),
                v.revision,
                BrowserAccess::Human(second.control_token.clone())
            )
            .await
            .is_err()
    );
    // No verified model: admission must roll back the control transition as well.
    assert!(
        store
            .resume_browser(
                &user,
                thread.clone(),
                v.id.clone(),
                v.revision,
                second.control_token.clone()
            )
            .await
            .is_err()
    );
    let still_controlled = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(still_controlled.state, State::UserControl);
    assert_eq!(still_controlled.revision, v.revision);
    assert!(
        store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    model(&store, &user).await;
    let first_resume = store
        .resume_browser(
            &user,
            thread.clone(),
            v.id.clone(),
            v.revision,
            second.control_token.clone(),
        )
        .await
        .unwrap();
    let second_resume = store
        .resume_browser(
            &user,
            thread.clone(),
            v.id.clone(),
            v.revision,
            second.control_token,
        )
        .await
        .unwrap();
    assert_eq!(first_resume.run.id, second_resume.run.id);
    assert_eq!(
        store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .len(),
        1
    );
    let ready = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ready.state, State::Ready);
    assert!(
        store
            .check_browser_workspace(
                &user,
                thread.clone(),
                ready.id.clone(),
                ready.revision,
                BrowserAccess::AgentObserve
            )
            .await
            .is_ok()
    );
    assert!(
        store
            .check_browser_workspace(
                &user,
                thread,
                ready.id,
                ready.revision,
                BrowserAccess::AgentNavigate
            )
            .await
            .is_err()
    );
}
#[tokio::test]
async fn expired_control_waits_for_human_and_restart_never_replays_a_browser() {
    let _guard = TEST_LOCK.lock().await;
    let (dir, store, user, thread, view) = setup().await;
    let controlled = store
        .take_browser_control(&user, thread.clone(), view.id, view.revision)
        .await
        .unwrap();
    store
        .call(|db| {
            db.conn
                .execute("UPDATE browser_workspaces SET control_until=0", [])?;
            Ok(())
        })
        .await
        .unwrap();
    let waiting = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(waiting.state, State::WaitingForUser);
    assert!(
        store
            .check_browser_workspace(
                &user,
                thread.clone(),
                waiting.id.clone(),
                waiting.revision,
                BrowserAccess::AgentObserve
            )
            .await
            .is_err()
    );
    assert!(
        store
            .resume_browser(
                &user,
                thread.clone(),
                waiting.id,
                controlled.workspace.revision,
                controlled.control_token
            )
            .await
            .is_err()
    );
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let view = reopened
        .browser_workspace(&user, thread)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(view.state, State::OutcomeUnknown);
    assert!(view.url.is_none() && view.title.is_none());
}
#[tokio::test]
async fn session_revocation_thread_deletion_and_old_workspace_ids_do_not_reacquire_control() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread, view) = setup().await;
    let closed = store
        .end_browser_workspace(
            &user,
            thread.clone(),
            view.id.clone(),
            view.revision,
            State::Closed,
        )
        .await
        .unwrap();
    let fresh = store
        .begin_browser_workspace(&user, thread.clone(), "fixture-pin".into())
        .await
        .unwrap();
    assert_ne!(fresh.id, closed.id);
    assert!(
        store
            .take_browser_control(&user, thread.clone(), closed.id, closed.revision)
            .await
            .is_err()
    );
    store.delete_thread(&user, thread.clone()).await.unwrap();
    assert!(matches!(
        store.browser_workspace(&user, thread).await,
        Err(StoreError::NotFound)
    ));
    store.logout(&user).await.unwrap();
    assert!(matches!(
        store
            .begin_browser_workspace(&user, Uuid::new_v4().to_string(), "fixture-pin".into())
            .await,
        Err(StoreError::Unauthorized)
    ));
}

async fn model(store: &Store, user: &UserContext) {
    use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
    let provider = store
        .create_provider(
            user,
            crate::ProviderChange {
                name: "Synthetic".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn recovery_keeps_original_deadlines_and_fences_previous_control_without_replaying_a_turn() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread, view) = setup().await;
    // Enablement is only legal in Starting, after a capable worker is ready.
    assert!(
        store
            .enable_browser_recovery(&user, view.clone())
            .await
            .is_err()
    );
    store
        .call(|db| {
            db.conn
                .execute("UPDATE browser_workspaces SET state='starting'", [])?;
            Ok(())
        })
        .await
        .unwrap();
    store
        .enable_browser_recovery(&user, view.clone())
        .await
        .unwrap();
    let view = store
        .activate_browser_workspace(
            &user,
            thread.clone(),
            view.id,
            "https://example.com/".into(),
            "".into(),
        )
        .await
        .unwrap();
    let old = store
        .take_browser_control(&user, thread.clone(), view.id.clone(), view.revision)
        .await
        .unwrap();
    let before = store
        .call(|db| {
            Ok(db.conn.query_row(
                "SELECT expires_at,idle_until FROM browser_workspaces",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )?)
        })
        .await
        .unwrap();
    store.call(|db| restart(&db.conn)).await.unwrap();
    let interrupted = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert!(interrupted.recoverable);
    assert_eq!(interrupted.state, State::OutcomeUnknown);
    assert!(interrupted.url.is_none());
    let new_session = store
        .login(user.account.email.clone(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        !store
            .browser_workspace(&new_session, thread.clone())
            .await
            .unwrap()
            .unwrap()
            .recoverable
    );
    assert!(
        store
            .recover_browser_workspace(
                &new_session,
                thread.clone(),
                interrupted.id.clone(),
                interrupted.revision,
                "fixture-pin".into()
            )
            .await
            .is_err()
    );
    assert!(
        store
            .recover_browser_workspace(
                &user,
                thread.clone(),
                interrupted.id.clone(),
                interrupted.revision,
                "changed-pin".into()
            )
            .await
            .is_err()
    );
    let recovered = store
        .recover_browser_workspace(
            &user,
            thread.clone(),
            interrupted.id.clone(),
            interrupted.revision,
            "fixture-pin".into(),
        )
        .await
        .unwrap();
    assert!(recovered.workspace.human_touched);
    assert_eq!(recovered.workspace.state, State::UserControl);
    assert_ne!(old.control_token, recovered.control_token);
    assert!(
        store
            .check_browser_workspace(
                &new_session,
                thread.clone(),
                interrupted.id.clone(),
                recovered.workspace.revision,
                BrowserAccess::Human(recovered.control_token.clone())
            )
            .await
            .is_err()
    );
    assert!(recovered.workspace.url.is_none());
    let after = store
        .call(|db| {
            Ok(db.conn.query_row(
                "SELECT expires_at,idle_until FROM browser_workspaces",
                [],
                |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)),
            )?)
        })
        .await
        .unwrap();
    assert_eq!(before, after);
    assert!(
        store
            .recover_browser_workspace(
                &user,
                thread.clone(),
                interrupted.id.clone(),
                interrupted.revision,
                "fixture-pin".into()
            )
            .await
            .is_err()
    );
    assert!(
        store
            .check_browser_workspace(
                &user,
                thread.clone(),
                interrupted.id.clone(),
                recovered.workspace.revision,
                BrowserAccess::Human(old.control_token)
            )
            .await
            .is_err()
    );
    assert!(
        store
            .check_browser_workspace(
                &user,
                thread.clone(),
                interrupted.id.clone(),
                recovered.workspace.revision,
                BrowserAccess::AgentNavigate
            )
            .await
            .is_err()
    );
    assert!(
        store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    model(&store, &user).await;
    assert!(matches!(
        store
            .resume_browser(
                &user,
                thread.clone(),
                interrupted.id.clone(),
                recovered.workspace.revision,
                recovered.control_token.clone()
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let blank = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(blank.state, State::UserControl);
    assert_eq!(blank.revision, recovered.workspace.revision);
    assert!(
        store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    let opened = store
        .browser_location(
            &user,
            blank,
            BrowserAccess::Human(recovered.control_token.clone()),
            "https://example.com/result".into(),
            "Result".into(),
        )
        .await
        .unwrap();
    store
        .resume_browser(
            &user,
            thread.clone(),
            opened.id.clone(),
            opened.revision,
            recovered.control_token,
        )
        .await
        .unwrap();
    let completed = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(completed.state, State::Ready);
    assert_eq!(
        store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .len(),
        1
    );
    store
        .end_browser_workspace(
            &user,
            thread.clone(),
            interrupted.id,
            completed.revision,
            State::Closed,
        )
        .await
        .unwrap();
    store.call(|db| restart(&db.conn)).await.unwrap();
    assert!(
        store
            .browser_profile_leases("fixture-pin".into())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        !store
            .browser_workspace(&user, thread)
            .await
            .unwrap()
            .unwrap()
            .recoverable
    );
}

#[tokio::test]
async fn expired_idle_profile_cannot_revive_after_restart_or_token_loss() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread, view) = setup().await;
    store.call(|db|{db.conn.execute("UPDATE browser_workspaces SET owner_session=(SELECT digest FROM sessions LIMIT 1),idle_until=0",[])?;restart(&db.conn)}).await.unwrap();
    let expired = store
        .browser_workspace(&user, thread.clone())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(expired.state, State::Expired);
    assert!(!expired.recoverable);
    assert!(
        store
            .recover_browser_workspace(
                &user,
                thread,
                view.id,
                expired.revision,
                "fixture-pin".into()
            )
            .await
            .is_err()
    );
    assert!(
        store
            .browser_profile_leases("fixture-pin".into())
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn profile_cleanup_authority_excludes_revoked_deleted_and_changed_deployment_leases() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread, view) = setup().await;
    store
        .call(|db| {
            db.conn
                .execute("UPDATE browser_workspaces SET state='starting'", [])?;
            Ok(())
        })
        .await
        .unwrap();
    store
        .enable_browser_recovery(&user, view.clone())
        .await
        .unwrap();
    assert_eq!(
        store
            .browser_profile_leases("fixture-pin".into())
            .await
            .unwrap()
            .len(),
        1
    );
    assert!(
        store
            .browser_profile_leases("changed-pin".into())
            .await
            .unwrap()
            .is_empty()
    );
    store.delete_thread(&user, thread).await.unwrap();
    assert!(
        store
            .browser_profile_leases("fixture-pin".into())
            .await
            .unwrap()
            .is_empty()
    );
    let thread = store
        .create_thread(&user, "Revocation".into())
        .await
        .unwrap()
        .id;
    let view = store
        .begin_browser_workspace(&user, thread.clone(), "fixture-pin".into())
        .await
        .unwrap();
    store.enable_browser_recovery(&user, view).await.unwrap();
    assert_eq!(
        store
            .browser_profile_leases("fixture-pin".into())
            .await
            .unwrap()
            .len(),
        1
    );
    store.logout(&user).await.unwrap();
    assert!(
        store
            .browser_profile_leases("fixture-pin".into())
            .await
            .unwrap()
            .is_empty()
    );
    store.call(|db| restart(&db.conn)).await.unwrap();
    assert!(store.browser_workspace(&user, thread).await.is_err());
}

#[tokio::test]
async fn revoked_profile_leases_do_not_consume_capacity_for_a_new_login_session() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, _thread, view) = setup().await;
    store
        .call(|db| {
            db.conn
                .execute("UPDATE browser_workspaces SET state='starting'", [])?;
            Ok(())
        })
        .await
        .unwrap();
    store.enable_browser_recovery(&user, view).await.unwrap();
    let second = store
        .create_thread(&user, "Second profile".into())
        .await
        .unwrap()
        .id;
    let view = store
        .begin_browser_workspace(&user, second, "fixture-pin".into())
        .await
        .unwrap();
    store.enable_browser_recovery(&user, view).await.unwrap();
    let third = store
        .create_thread(&user, "Fresh session capacity".into())
        .await
        .unwrap()
        .id;
    assert!(matches!(
        store
            .begin_browser_workspace(&user, third.clone(), "fixture-pin".into())
            .await,
        Err(StoreError::Busy)
    ));
    store.logout(&user).await.unwrap();
    let fresh = store
        .login(user.account.email.clone(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        store
            .begin_browser_workspace(&fresh, third, "fixture-pin".into())
            .await
            .is_ok()
    );
}
