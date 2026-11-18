use super::*;
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
