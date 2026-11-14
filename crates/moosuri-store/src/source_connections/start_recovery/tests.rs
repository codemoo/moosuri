use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
async fn setup(s: &Store, u: &UserContext) -> (SourceConnection, SourceIngestCapability, String) {
    let c = s
        .create_source_connection(
            u,
            CreateSourceConnection {
                name: "Synthetic recoverable source".into(),
                kind: SourceConnectionKind::CustomHttp,
                scope: "synthetic/start/v1".into(),
                remote_account: "external_producer".into(),
                mail_connection_id: None,
                sync_enabled: false,
                poll_interval_seconds: 60,
            },
        )
        .await
        .unwrap();
    let token = s
        .issue_source_token(u, c.id.clone(), 3600)
        .await
        .unwrap()
        .token;
    let cap = s.authenticate_source_token(token.clone()).await.unwrap();
    (c, cap, token)
}
async fn intent(s: &Store, cap: &SourceIngestCapability) -> SourceRunStartIntent {
    let status = s.source_ingest_status(cap).await.unwrap();
    SourceRunStartIntent {
        start_binding: status.start_binding,
        request_id: Uuid::new_v4().to_string(),
        config_revision: status.config_revision,
        expected_run_generation: status.run_generation,
        expected_sequence: status.local_sequence,
        checkpoint_before: status.checkpoint,
    }
}
#[tokio::test]
async fn exact_start_replay_restart_expiry_and_pruning_never_allocate_an_old_composite_key() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let (c, cap, token) = setup(&s, &u).await;
    let first = intent(&s, &cap).await;
    let ack = s
        .begin_source_run_recoverable(&cap, first.clone())
        .await
        .unwrap();
    let replay = s
        .begin_source_run_recoverable(&cap, first.clone())
        .await
        .unwrap();
    assert_eq!(
        (&ack.run_id, ack.generation),
        (&replay.run_id, replay.generation)
    );
    assert_eq!(s.source_sync_runs(&u, c.id.clone()).await.unwrap().len(), 1);
    let mut changed = first.clone();
    changed.checkpoint_before = Some("different".into());
    assert!(s.begin_source_run_recoverable(&cap, changed).await.is_err());
    let mut competing = first.clone();
    competing.request_id = Uuid::new_v4().to_string();
    assert!(
        s.begin_source_run_recoverable(&cap, competing)
            .await
            .is_err()
    );
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let cap = s.authenticate_source_token(token).await.unwrap();
    let unknown = s
        .begin_source_run_recoverable(&cap, first.clone())
        .await
        .unwrap();
    assert_eq!(unknown.run_id, ack.run_id);
    assert_eq!(unknown.state, "unknown");
    assert_eq!(
        s.source_ingest_status(&cap).await.unwrap().run_generation,
        1
    );
    let second = intent(&s, &cap).await;
    let live = s
        .begin_source_run_recoverable(&cap, second.clone())
        .await
        .unwrap();
    assert_eq!(live.generation, 2);
    let run = live.run_id.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE source_sync_runs SET lease_until=0 WHERE id=?1",
            [run],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let expired = s
        .begin_source_run_recoverable(&cap, second.clone())
        .await
        .unwrap();
    assert_eq!(expired.run_id, live.run_id);
    assert_eq!(expired.state, "unknown");
    assert_eq!(expired.generation, 2);
    // Existing {} / scheduler API still starts a distinct run and uses its old lease.
    let legacy = s.begin_source_run(&cap).await.unwrap();
    assert_eq!(legacy.generation(), 3);
    s.finish_source_run(&legacy, SourceRunOutcome::Partial, None, None)
        .await
        .unwrap();
    let tenant = u.tenant.0.clone();
    let connection = c.id.clone();
    s.call(move |db| {
        prune_source_history(&db.conn, &tenant, &connection, 0, 0)?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.source_sync_runs(&u, c.id.clone())
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        s.begin_source_run_recoverable(&cap, first.clone())
            .await
            .is_err()
    );
    assert!(s.begin_source_run_recoverable(&cap, second).await.is_err());
    assert_eq!(
        s.source_ingest_status(&cap).await.unwrap().run_generation,
        3
    );
    // UUID alone is deliberately not global forever: a new expected generation
    // is a distinct explicit composite intent even after old history was pruned.
    let mut fresh = intent(&s, &cap).await;
    fresh.request_id = first.request_id;
    let next = s.begin_source_run_recoverable(&cap, fresh).await.unwrap();
    assert_eq!(next.generation, 4);
}
#[tokio::test]
async fn current_owner_grant_config_checkpoint_and_generation_bind_every_start_dispatch() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await.user;
    let b = member(&s, &a, "start-other@example.test").await.user;
    let (ca, cap, old_token) = setup(&s, &a).await;
    let (cb, other, _) = setup(&s, &b).await;
    let original = intent(&s, &cap).await;
    let ack = s
        .begin_source_run_recoverable(&cap, original.clone())
        .await
        .unwrap();
    assert!(
        s.begin_source_run_recoverable(&other, original.clone())
            .await
            .is_err()
    );
    let mut peer_intent = intent(&s, &other).await;
    peer_intent.request_id = original.request_id.clone();
    let peer = s
        .begin_source_run_recoverable(&other, peer_intent)
        .await
        .unwrap();
    assert_ne!(ack.run_id, peer.run_id);
    assert_eq!(s.source_sync_runs(&b, cb.id).await.unwrap().len(), 1);
    assert!(s.source_sync_runs(&b, ca.id.clone()).await.is_err());
    s.revoke_source_token(&a, ca.id.clone()).await.unwrap();
    assert!(s.authenticate_source_token(old_token).await.is_err());
    assert!(
        s.begin_source_run_recoverable(&cap, original.clone())
            .await
            .is_err()
    );
    let token = s
        .issue_source_token(&a, ca.id.clone(), 3600)
        .await
        .unwrap()
        .token;
    let fresh_cap = s.authenticate_source_token(token.clone()).await.unwrap();
    assert!(
        s.begin_source_run_recoverable(&fresh_cap, original.clone())
            .await
            .is_err()
    );
    let before = s.source_ingest_status(&fresh_cap).await.unwrap();
    s.configure_source_connection(
        &a,
        ca.id.clone(),
        before.config_revision,
        "Changed local scope config".into(),
        false,
        60,
    )
    .await
    .unwrap();
    assert!(
        s.begin_source_run_recoverable(&fresh_cap, original)
            .await
            .is_err()
    );
    let fresh_cap = s.authenticate_source_token(token.clone()).await.unwrap();
    let config_intent = intent(&s, &fresh_cap).await;
    let reset = s
        .reset_source_checkpoint(&a, ca.id.clone(), config_intent.config_revision)
        .await
        .unwrap();
    assert!(
        s.begin_source_run_recoverable(&fresh_cap, config_intent.clone())
            .await
            .is_err()
    );
    let fresh_cap = s.authenticate_source_token(token).await.unwrap();
    assert!(
        s.begin_source_run_recoverable(&fresh_cap, config_intent)
            .await
            .is_err()
    );
    assert_eq!(
        s.source_ingest_status(&fresh_cap)
            .await
            .unwrap()
            .run_generation,
        before.run_generation
    );
    assert!(reset.config_revision > before.config_revision);
    let fresh = intent(&s, &fresh_cap).await;
    let run = s
        .begin_source_run_recoverable(&fresh_cap, fresh)
        .await
        .unwrap();
    let lease = s
        .source_run_lease(&fresh_cap, run.run_id.clone(), run.generation)
        .await
        .unwrap();
    s.commit_source_batch(
        &lease,
        SourceBatch {
            protocol_version: 1,
            request_id: Uuid::new_v4().to_string(),
            config_revision: reset.config_revision,
            run_id: run.run_id,
            generation: run.generation,
            expected_sequence: 0,
            checkpoint_before: None,
            checkpoint_after: Some("committed-page".into()),
            entries: vec![],
            metadata: vec![],
            tombstones: vec![],
            complete_snapshot: false,
        },
    )
    .await
    .unwrap();
    s.finish_source_run(&lease, SourceRunOutcome::Partial, None, None)
        .await
        .unwrap();
    let mut wrong = intent(&s, &fresh_cap).await;
    wrong.expected_sequence = 0;
    wrong.checkpoint_before = None;
    assert!(
        s.begin_source_run_recoverable(&fresh_cap, wrong)
            .await
            .is_err()
    );
    assert_eq!(
        s.source_ingest_status(&fresh_cap)
            .await
            .unwrap()
            .local_sequence,
        1
    );
}
#[tokio::test]
async fn schema47_upgrade_retains_legacy_generation_and_cannot_reuse_its_missing_key() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let (c, cap, token) = setup(&s, &u).await;
    let legacy = s.begin_source_run(&cap).await.unwrap();
    assert_eq!(legacy.generation(), 1);
    s.call(|db| {
        db.conn
            .execute_batch("DROP TABLE source_run_starts;PRAGMA user_version=47;")?;
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let cap = s.authenticate_source_token(token).await.unwrap();
    assert_eq!(
        s.source_ingest_status(&cap).await.unwrap().run_generation,
        1
    );
    let mut stale = intent(&s, &cap).await;
    stale.expected_run_generation = 0;
    assert!(s.begin_source_run_recoverable(&cap, stale).await.is_err());
    let next = s
        .begin_source_run_recoverable(&cap, intent(&s, &cap).await)
        .await
        .unwrap();
    assert_eq!(next.generation, 2);
    assert_eq!(s.source_sync_runs(&u, c.id).await.unwrap().len(), 2);
}
