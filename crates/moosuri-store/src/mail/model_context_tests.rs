use super::*;
const SEMANTIC_READ: &str = "새로 도착한 것부터 차근차근 말해 줄래? 내 받은편지함 얘기야.";

#[tokio::test]
async fn structured_gmail_helper_rechecks_owner_connection_and_cannot_authorize_reads() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let foreign = member(&store, &a.user, "foreign-help@example.test").await;
    configured(&store, &a.user).await;
    connect(&store, &foreign.user).await;
    let t = store
        .create_thread(&a.user, "Synthetic setup receipt".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &a.user,
            t.id.clone(),
            Uuid::new_v4().to_string(),
            SEMANTIC_READ.into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    assert_eq!(
        store
            .finish_model_mail_connection_help(
                &step,
                MailConnectionHelpIntent {
                    current_request: "old request".into()
                },
                None
            )
            .await
            .err(),
        Some(StoreError::Forbidden)
    );
    // Snapshot was disconnected. A connection established during inference must
    // prevent stale guidance; another owner's connection never satisfies ours.
    assert!(
        !store
            .mail_model_connection_context(&claim.lease)
            .await
            .unwrap()
            .server_read_connected
    );
    let own = connect(&store, &a.user).await;
    assert_eq!(
        store
            .finish_model_mail_connection_help(
                &step,
                MailConnectionHelpIntent {
                    current_request: SEMANTIC_READ.into()
                },
                None
            )
            .await
            .err(),
        Some(StoreError::Conflict)
    );
    assert_eq!(
        store
            .pin_latest_inbox(&claim.lease, own.id.clone(), own.epoch)
            .await
            .err(),
        Some(StoreError::Forbidden)
    );
    let b = store.mail_binding(&a.user, own.id).await.unwrap();
    store.mark_mail_needs_reconnect(&b).await.unwrap();
    store
        .finish_model_mail_connection_help(
            &step,
            MailConnectionHelpIntent {
                current_request: SEMANTIC_READ.into(),
            },
            Some(GoogleAppPin {
                revision: None,
                client_id: "synthetic".into(),
                redirect_uri: "https://example.test/callback".into(),
            }),
        )
        .await
        .unwrap();
    let turn = store
        .conversation(&a.user, t.id, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert!(matches!(
        turn.run.mail_connection_recovery,
        Some(MailConnectionRecovery::Reconnect)
    ));
    assert_eq!(turn.run.state, RunState::Completed);
    assert!(store.mail_actions(&a.user).await.unwrap().is_empty());
    assert!(
        store
            .finish_model_mail_connection_help(
                &step,
                MailConnectionHelpIntent {
                    current_request: SEMANTIC_READ.into()
                },
                None
            )
            .await
            .is_err()
    );
}

#[tokio::test]
async fn structured_gmail_helper_derives_admin_and_member_setup_without_model_urls() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "member-help@example.test").await;
    configured(&store, &a.user).await;
    for (user, expected) in [
        (&a.user, MailConnectionRecovery::AdminSetup),
        (&b.user, MailConnectionRecovery::MemberSetup),
    ] {
        let t = store
            .create_thread(user, "Synthetic role guidance".into())
            .await
            .unwrap();
        store
            .submit_turn(
                user,
                t.id.clone(),
                Uuid::new_v4().to_string(),
                SEMANTIC_READ.into(),
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let step = store
            .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
            .await
            .unwrap();
        store
            .finish_model_mail_connection_help(
                &step,
                MailConnectionHelpIntent {
                    current_request: SEMANTIC_READ.into(),
                },
                None,
            )
            .await
            .unwrap();
        let turn = store
            .conversation(user, t.id, None)
            .await
            .unwrap()
            .turns
            .remove(0);
        assert_eq!(
            serde_json::to_value(turn.run.mail_connection_recovery).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(turn.run.state, RunState::Completed);
    }
    assert!(
        serde_json::from_str::<MailConnectionHelpIntent>(
            r#"{"current_request":"x","url":"https://example.test"}"#
        )
        .is_err()
    );
}

#[tokio::test]
async fn gmail_model_context_is_current_owner_scoped_and_never_upgrades_frozen_read_admission() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "foreign@example.test").await;
    configured(&store, &a.user).await;
    connect(&store, &b.user).await;
    let t = store
        .create_thread(&a.user, "Synthetic context".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &a.user,
            t.id.clone(),
            Uuid::new_v4().to_string(),
            "받은편지함 이야기인데, 새로 도착한 것부터 차근차근 말해 줄래?".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let c = store
        .mail_model_connection_context(&claim.lease)
        .await
        .unwrap();
    assert!(c.admin);
    assert!(!c.server_read_connected);
    assert!(!c.server_read_admitted);
    assert!(!c.native_read_admitted);
    let own = connect(&store, &a.user).await;
    let c = store
        .mail_model_connection_context(&claim.lease)
        .await
        .unwrap();
    assert!(c.server_read_connected);
    assert!(
        !c.server_read_admitted,
        "a connection created after admission grants no new run tools"
    );
    store
        .finish_agent_execution(&claim.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    assert!(
        store
            .mail_model_connection_context(&claim.lease)
            .await
            .is_err()
    );
    store
        .submit_turn(
            &a.user,
            t.id,
            Uuid::new_v4().to_string(),
            "새로 도착한 것부터 말해 줄래? 받은편지함 얘기야.".into(),
        )
        .await
        .unwrap();
    let next = store.claim_agent_run().await.unwrap().unwrap();
    let c = store
        .mail_model_connection_context(&next.lease)
        .await
        .unwrap();
    assert!(c.server_read_connected);
    assert!(c.server_read_admitted);
    let binding = store.mail_binding(&a.user, own.id).await.unwrap();
    store.mark_mail_needs_reconnect(&binding).await.unwrap();
    let c = store
        .mail_model_connection_context(&next.lease)
        .await
        .unwrap();
    assert!(!c.server_read_connected);
    assert!(!c.server_read_admitted);
    assert!(c.needs_reconnect);
}
