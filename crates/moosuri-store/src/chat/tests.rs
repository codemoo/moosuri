use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};

async fn configured(store: &Store) -> (UserContext, String) {
    let user = admin(store).await.user;
    let config = ProviderConfig {
        kind: ProviderKind::Compatible,
        base_url: Some("http://127.0.0.1:11434/v1".into()),
        allowed_ips: vec!["127.0.0.1".into()],
        router_providers: vec![],
        zero_data_retention: false,
    };
    let provider = store
        .create_provider(
            &user,
            ProviderChange {
                name: "Synthetic only".into(),
                config,
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &user,
            provider.id.clone(),
            1,
            "synthetic-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                provider_id: provider.id.clone(),
                provider_revision: 1,
                model: "synthetic-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    (user, provider.id)
}
async fn submit(store: &Store, user: &UserContext, thread: &str, text: &str) -> ChatTurn {
    store
        .submit_turn(user, thread.into(), Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap()
}
fn append(item: &str, text: &str) -> OutputPatch {
    OutputPatch::Append {
        item: item.into(),
        text: text.into(),
        phase: None,
        refusal: false,
    }
}
#[tokio::test]
async fn lost_ack_idempotency_and_tenant_scoped_content() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (a, _) = configured(&store).await;
    let b = member(&store, &a, "member@example.test").await.user;
    let thread = store.create_thread(&a, "A private".into()).await.unwrap();
    let other = store.create_thread(&b, "B private".into()).await.unwrap();
    let request = Uuid::new_v4().to_string();
    let first = store
        .submit_turn(
            &a,
            thread.id.clone(),
            request.clone(),
            "private prompt".into(),
        )
        .await
        .unwrap();
    // Changing defaults after a lost acknowledgement must not change identity/binding.
    store.set_default_model(&a, None).await.unwrap();
    let replay = store
        .submit_turn(
            &a,
            thread.id.clone(),
            request.clone(),
            "private prompt".into(),
        )
        .await
        .unwrap();
    assert_eq!(first.run.id, replay.run.id);
    assert!(matches!(
        store
            .submit_turn(&a, thread.id.clone(), request.clone(), "different".into())
            .await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .submit_turn(&a, other.id.clone(), request, "private prompt".into())
            .await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.conversation(&b, thread.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.conversation(&a, other.id.clone(), None).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.chat_events(&b, thread.id.clone(), 0).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.cancel_run(&b, first.run.id.clone(), 1).await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        store.retry_run(&b, first.run.id, 1).await,
        Err(StoreError::NotFound)
    ));
    assert_eq!(
        store
            .conversation(&a, thread.id, None)
            .await
            .unwrap()
            .turns
            .len(),
        1
    );
    assert!(matches!(
        store.account_usage(&b).await,
        Err(StoreError::Forbidden)
    ));
}
#[tokio::test]
async fn retry_cancel_delete_fence_text_but_keep_one_receipt_per_dispatch() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (user, _) = configured(&store).await;
    let thread = store.create_thread(&user, "Private".into()).await.unwrap();
    let original = submit(&store, &user, &thread.id, "one prompt").await;
    let first = store.claim_run().await.unwrap().unwrap();
    store
        .append_output(&first.lease, vec![append("item", "failed partial")])
        .await
        .unwrap();
    store
        .finish_run(
            &first.lease,
            RunState::Failed,
            Some(RunFailure::ConnectionFailed),
        )
        .await
        .unwrap();
    let retried = store
        .retry_run(&user, original.run.id.clone(), 1)
        .await
        .unwrap();
    assert_eq!(retried.run.id, original.run.id);
    assert_eq!(retried.run.attempt, 2);
    assert!(retried.assistant.is_empty());
    assert!(matches!(
        store.retry_run(&user, original.run.id.clone(), 1).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        store
            .append_output(&first.lease, vec![append("item", "late")])
            .await,
        Err(StoreError::Conflict)
    ));
    let second = store.claim_run().await.unwrap().unwrap();
    assert_eq!(second.messages.len(), 1);
    store
        .cancel_run(&user, original.run.id.clone(), 2)
        .await
        .unwrap();
    assert!(matches!(
        store
            .finish_run(&second.lease, RunState::Completed, None)
            .await,
        Err(StoreError::Conflict)
    ));
    let observed = RunUsage {
        input_tokens: Some(10),
        output_tokens: Some(4),
        cache_read_tokens: None,
        cache_write_tokens: None,
    };
    store
        .update_run_usage(&second.lease, observed.clone())
        .await
        .unwrap();
    store
        .update_run_usage(&second.lease, observed)
        .await
        .unwrap();
    let usage = store.usage_summary(&user).await.unwrap();
    assert_eq!(usage.dispatched_calls, 2);
    assert_eq!(usage.unreported_calls, 1);
    assert_eq!(usage.reported_input_tokens, 10);
    store.delete_thread(&user, thread.id.clone()).await.unwrap();
    assert!(matches!(
        store
            .append_output(&second.lease, vec![append("item", "late")])
            .await,
        Err(StoreError::NotFound)
    ));
    store
        .update_run_usage(
            &second.lease,
            RunUsage {
                input_tokens: Some(12),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    let usage = store.usage_summary(&user).await.unwrap();
    assert_eq!(usage.reported_input_tokens, 12);
    assert_eq!(usage.reported_output_tokens, 4);
    let mut forged = second.lease;
    forged.nonce[0] ^= 1;
    assert_eq!(
        store.update_run_usage(&forged, RunUsage::default()).await,
        Err(StoreError::Unauthorized)
    );
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM chat_runs", [], |r| r.get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM chat_events", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn provider_and_session_revocation_stop_claim_and_publication() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (user, provider) = configured(&store).await;
    let thread = store.create_thread(&user, "Private".into()).await.unwrap();
    submit(&store, &user, &thread.id, "private prompt").await;
    let claimed = store.claim_run().await.unwrap().unwrap();
    let current = store
        .list_providers(&user)
        .await
        .unwrap()
        .providers
        .remove(0);
    store
        .update_provider(
            &user,
            provider,
            1,
            ProviderChange {
                name: current.name,
                config: current.config,
                enabled: false,
                credential: CredentialChange::Keep,
            },
        )
        .await
        .unwrap();
    assert!(store.check_run(&claimed.lease).await.is_err());
    assert!(
        store
            .append_output(&claimed.lease, vec![append("item", "must never persist")])
            .await
            .is_err()
    );
    let page = store.conversation(&user, thread.id, None).await.unwrap();
    assert_eq!(page.turns[0].run.state, RunState::Failed);
    assert!(page.turns[0].assistant.is_empty());
    assert_eq!(
        page.turns[0].run.error.as_deref(),
        Some("provider_unavailable")
    );
    let dir2 = directory();
    let other = Store::open(dir2.path()).unwrap();
    let (user, _) = configured(&other).await;
    let a = other.create_thread(&user, "queued".into()).await.unwrap();
    let b = other.create_thread(&user, "running".into()).await.unwrap();
    submit(&other, &user, &a.id, "first").await;
    submit(&other, &user, &b.id, "second").await;
    let claimed = other.claim_run().await.unwrap().unwrap();
    other.logout(&user).await.unwrap();
    assert_eq!(
        other.check_run(&claimed.lease).await,
        Err(StoreError::Unauthorized)
    );
    assert!(other.claim_run().await.unwrap().is_none());
    assert!(matches!(
        other.conversation(&user, a.id, None).await,
        Err(StoreError::Unauthorized)
    ));
    other
        .update_run_usage(
            &claimed.lease,
            RunUsage {
                input_tokens: Some(2),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    other.call(|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM chat_runs WHERE state='cancelled' AND error='authority_revoked'",[],|r|r.get::<_,i64>(0))?,2);Ok(())}).await.unwrap();
}
#[tokio::test]
async fn restart_never_replays_dispatched_attempt_and_keeps_queued_work() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (user, _) = configured(&store).await;
    let a = store.create_thread(&user, "first".into()).await.unwrap();
    let b = store.create_thread(&user, "second".into()).await.unwrap();
    submit(&store, &user, &a.id, "dispatched").await;
    submit(&store, &user, &b.id, "not dispatched").await;
    let claimed = store.claim_run().await.unwrap().unwrap();
    store
        .append_output(&claimed.lease, vec![append("item", "checkpoint")])
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let page = store.conversation(&user, a.id, None).await.unwrap();
    assert_eq!(page.turns[0].run.state, RunState::OutcomeUnknown);
    assert_eq!(page.turns[0].assistant[0].text, "checkpoint");
    assert_eq!(page.turns[0].run.error.as_deref(), Some("server_restarted"));
    let next = store.claim_run().await.unwrap().unwrap();
    assert_ne!(next.lease.run_id(), claimed.lease.run_id());
    store
        .finish_run(
            &next.lease,
            RunState::OutcomeUnknown,
            Some(RunFailure::Interrupted),
        )
        .await
        .unwrap();
    assert!(store.claim_run().await.unwrap().is_none());
    assert!(store.check_run(&claimed.lease).await.is_err());
    store
        .update_run_usage(
            &claimed.lease,
            RunUsage {
                output_tokens: Some(3),
                ..Default::default()
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        2
    );
}
#[tokio::test]
async fn events_phase_snapshot_pagination_and_retained_window() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (user, _) = configured(&store).await;
    let thread = store.create_thread(&user, "Stream".into()).await.unwrap();
    let run = submit(&store, &user, &thread.id, "hello").await;
    let claimed = store.claim_run().await.unwrap().unwrap();
    store
        .append_output(
            &claimed.lease,
            vec![
                OutputPatch::Phase {
                    item: "answer".into(),
                    phase: Some(MessagePhase::FinalAnswer),
                },
                append("answer", "hello"),
            ],
        )
        .await
        .unwrap();
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    let message = &page.turns[0].assistant[0];
    assert_eq!(message.phase, Some(MessagePhase::FinalAnswer));
    let events = store
        .chat_events(&user, thread.id.clone(), 0)
        .await
        .unwrap();
    assert!(!events.reset);
    assert!(
        events
            .events
            .iter()
            .enumerate()
            .all(|(i, e)| e.sequence == i as i64 + 1)
    );
    let delta = events
        .events
        .iter()
        .find_map(|e| {
            if let ChatChange::Delta { message_id, .. } = &e.change {
                Some(message_id)
            } else {
                None
            }
        })
        .unwrap();
    assert_eq!(delta, &message.id);
    for _ in 0..510 {
        store
            .append_output(&claimed.lease, vec![append("answer", "x")])
            .await
            .unwrap();
    }
    assert!(
        store
            .chat_events(&user, thread.id.clone(), 0)
            .await
            .unwrap()
            .reset
    );
    let page = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    let replay = store
        .chat_events(&user, thread.id.clone(), page.cursor - 70)
        .await
        .unwrap();
    assert_eq!(replay.events.len(), 64);
    assert_eq!(replay.cursor, page.cursor - 6);
    store
        .finish_run(&claimed.lease, RunState::Completed, None)
        .await
        .unwrap();
    assert!(
        !store
            .conversation(&user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .run
            .can_retry
    );
    for _ in 0..21 {
        let next = submit(&store, &user, &thread.id, "next").await;
        store.cancel_run(&user, next.run.id, 1).await.unwrap();
    }
    let newest = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap();
    assert_eq!(newest.turns.len(), 20);
    assert!(newest.next_before.is_some());
    let oldest = store
        .conversation(&user, thread.id, newest.next_before)
        .await
        .unwrap();
    assert_eq!(oldest.turns.len(), 2);
    assert_eq!(oldest.turns[0].run.id, run.run.id);
    assert!(oldest.next_before.is_none());
}

#[tokio::test]
async fn fair_claim_queue_and_output_limits_are_transactional() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (a, _) = configured(&store).await;
    let b = member(&store, &a, "member@example.test").await.user;
    let mut a_runs = Vec::new();
    for i in 0..8 {
        let thread = store.create_thread(&a, format!("A {i}")).await.unwrap();
        a_runs.push(submit(&store, &a, &thread.id, "queued").await);
    }
    let overflow = store.create_thread(&a, "overflow".into()).await.unwrap();
    assert!(matches!(
        store
            .submit_turn(
                &a,
                overflow.id,
                Uuid::new_v4().to_string(),
                "too many".into()
            )
            .await,
        Err(StoreError::Busy)
    ));
    assert!(matches!(
        store
            .submit_turn(
                &a,
                a_runs[0].run.thread_id.clone(),
                Uuid::new_v4().to_string(),
                "duplicate active turn".into()
            )
            .await,
        Err(StoreError::Busy) | Err(StoreError::Conflict)
    ));
    let b_thread = store.create_thread(&b, "B".into()).await.unwrap();
    let b_run = submit(&store, &b, &b_thread.id, "B eligible").await;
    let first = store.claim_run().await.unwrap().unwrap();
    let second = store.claim_run().await.unwrap().unwrap();
    assert_eq!(first.lease.run_id(), a_runs[0].run.id);
    assert_eq!(second.lease.run_id(), b_run.run.id);
    assert!(store.claim_run().await.unwrap().is_none());
    assert_eq!(
        store
            .append_output(
                &first.lease,
                vec![
                    append("first", "prefix"),
                    append("second", &"x".repeat(8193))
                ]
            )
            .await,
        Err(StoreError::InvalidInput)
    );
    let page = store
        .conversation(&a, a_runs[0].run.thread_id.clone(), None)
        .await
        .unwrap();
    assert!(page.turns[0].assistant.is_empty());
    let patches = (0..128)
        .map(|i| OutputPatch::Phase {
            item: format!("item-{i}"),
            phase: Some(MessagePhase::Commentary),
        })
        .collect();
    store.append_output(&first.lease, patches).await.unwrap();
    assert_eq!(
        store
            .append_output(&first.lease, vec![append("overflow", "x")])
            .await,
        Err(StoreError::InvalidInput)
    );
    for _ in 0..32 {
        store
            .append_output(&first.lease, vec![append("item-0", &"x".repeat(8192))])
            .await
            .unwrap();
    }
    assert_eq!(
        store
            .append_output(&first.lease, vec![append("item-0", "x")])
            .await,
        Err(StoreError::InvalidInput)
    );
    store
        .finish_run(&first.lease, RunState::Completed, None)
        .await
        .unwrap();
    let next = store.claim_run().await.unwrap().unwrap();
    assert_eq!(next.lease.run_id(), a_runs[1].run.id);
    assert_eq!(store.usage_summary(&a).await.unwrap().unreported_calls, 2);
    assert_eq!(store.usage_summary(&b).await.unwrap().dispatched_calls, 1);
}

#[tokio::test]
async fn context_uses_successful_whole_turns_and_preserves_phase() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let (user, _) = configured(&store).await;
    let thread = store.create_thread(&user, "Context".into()).await.unwrap();
    submit(&store, &user, &thread.id, "successful user").await;
    let successful = store.claim_run().await.unwrap().unwrap();
    store
        .append_output(
            &successful.lease,
            vec![OutputPatch::Append {
                item: "answer".into(),
                text: "successful answer".into(),
                phase: Some(MessagePhase::FinalAnswer),
                refusal: false,
            }],
        )
        .await
        .unwrap();
    store
        .finish_run(&successful.lease, RunState::Completed, None)
        .await
        .unwrap();
    submit(&store, &user, &thread.id, "failed user").await;
    let failed = store.claim_run().await.unwrap().unwrap();
    store
        .append_output(&failed.lease, vec![append("item", "must not be context")])
        .await
        .unwrap();
    store
        .finish_run(&failed.lease, RunState::Failed, None)
        .await
        .unwrap();
    submit(&store, &user, &thread.id, "large old user").await;
    let large = store.claim_run().await.unwrap().unwrap();
    // Before the size boundary, successful history retains phase; failed partial
    // output is excluded. The later oversized turn must then fence older context.
    assert!(
        large
            .messages
            .iter()
            .any(|m| m.text == "successful answer" && m.phase == Some(MessagePhase::FinalAnswer))
    );
    assert!(
        !large
            .messages
            .iter()
            .any(|m| m.text == "must not be context")
    );
    for _ in 0..9 {
        store
            .append_output(&large.lease, vec![append("large", &"x".repeat(8192))])
            .await
            .unwrap();
    }
    store
        .finish_run(&large.lease, RunState::Completed, None)
        .await
        .unwrap();
    let latest = submit(&store, &user, &thread.id, "new question").await;
    assert!(matches!(
        store.retry_run(&user, failed.lease.run.clone(), 1).await,
        Err(StoreError::Conflict)
    ));
    let current = store.claim_run().await.unwrap().unwrap();
    assert_eq!(current.lease.run_id(), latest.run.id);
    assert!(
        current
            .messages
            .iter()
            .any(|m| m.role == ContextRole::System)
    );
    assert!(
        !current
            .messages
            .iter()
            .any(|m| m.text == "must not be context"
                || m.text == "failed user"
                || m.text == "large old user")
    );
    assert!(
        !current
            .messages
            .iter()
            .any(|m| m.role == ContextRole::Assistant)
    );
    assert!(
        !current
            .messages
            .iter()
            .any(|m| m.text == "successful user" || m.text == "successful answer")
    );
    assert_eq!(current.messages.last().unwrap().text, "new question");
    assert!(current.messages.len() <= 128);
    assert!(current.messages.iter().map(|m| m.text.len()).sum::<usize>() <= 256 * 1024);
}

#[tokio::test]
async fn schema_two_migrates_in_place_and_restricted_sessions_never_admit_text() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch(include_str!(
        "../../../../migrations/assistant/0001_accounts_threads.sql"
    ))
    .unwrap();
    conn.execute_batch(include_str!(
        "../../../../migrations/assistant/0002_providers.sql"
    ))
    .unwrap();
    let account = insert_account(
        &conn,
        "Old temporary admin".into(),
        "old@example.test".into(),
        hash_sync(crate::tests::PASSWORD).unwrap(),
        Role::Admin,
        true,
    )
    .unwrap();
    let thread = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) SELECT tenant_id,?1,id,'Old thread',0,0 FROM accounts WHERE id=?2",params![thread,account.id]).unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    let login = store
        .login("old@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    assert!(matches!(
        store
            .submit_turn(
                &login.user,
                thread.clone(),
                Uuid::new_v4().to_string(),
                "restricted text".into()
            )
            .await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.conversation(&login.user, thread, None).await,
        Err(StoreError::Forbidden)
    ));
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                6
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM threads", [], |r| r.get::<_, i64>(0))?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
}
