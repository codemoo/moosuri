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
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
}
