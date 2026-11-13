//! Counted typed IO + actual durable chat ledger. No live model/Google calls.
use super::*;
use moosuri_store::{CredentialChange, PrivateCheckpoint, ProviderChange, RunLease};
async fn configure(s: &Store, u: &UserContext) {
    let p = s
        .create_provider(
            u,
            ProviderChange {
                name: "Synthetic Calendar model".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    s.record_model_probe(u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.record_tool_probe(u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        u,
        Some(ModelSelection {
            provider_id: p.id,
            provider_revision: 1,
            model: "synthetic".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
}
const TEXT: &str = "Create Synthetic meeting on 2026-12-01 from 10:00 to 11:00";
fn decision(text: &str) -> CalendarChatDecision {
    CalendarChatDecision {
        domain: "calendar".into(),
        decision: "command".into(),
        operation: Some("create".into()),
        connection_quote: None,
        intent_quote: Some(text.into()),
        title_quote: Some("Synthetic meeting".into()),
        target_quote: None,
        date_quote: Some("2026-12-01".into()),
        date: Some("2026-12-01".into()),
        start_quote: Some("10:00".into()),
        start: Some("10:00".into()),
        end_quote: Some("11:00".into()),
        end: Some("11:00".into()),
        duration_quote: None,
        duration_minutes: None,
        clarification: None,
    }
}
async fn sealed(
    s: &Store,
    u: &UserContext,
    text: &str,
    d: CalendarChatDecision,
) -> (String, RunLease) {
    let t = s
        .create_thread(u, "Synthetic Calendar chat".into())
        .await
        .unwrap();
    s.submit_turn_with_schedule_context(
        u,
        t.id.clone(),
        uuid::Uuid::new_v4().to_string(),
        text.into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = s
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: serde_json::json!({"kind":"mission_chat_decision","decision":d})
                .to_string()
                .into_bytes(),
        },
        vec![],
    )
    .await
    .unwrap();
    (t.id, claim.lease)
}
#[tokio::test]
async fn sealed_calendar_create_uses_one_exact_write_and_trusted_receipt_without_final_model() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    for mode in [0, 1, 2] {
        io.write_mode.store(mode, Ordering::SeqCst);
        let (t, l) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
        let before = io.write_count();
        let id = service
            .store
            .calendar_agent_command(&l)
            .await
            .unwrap()
            .unwrap()
            .action_id;
        assert!(service.chat(&l).await.unwrap());
        assert_eq!(io.write_count(), before + 1);
        let turn = service
            .store
            .conversation(&u, t, None)
            .await
            .unwrap()
            .turns
            .remove(0);
        let receipt = turn.run.calendar_receipt.unwrap();
        assert_eq!(receipt.action_id.as_deref(), Some(id.as_str()));
        assert_eq!(
            receipt.state,
            match mode {
                0 => "applied",
                1 => "outcome_unknown",
                _ => "conflict",
            }
        );
        assert!(turn.assistant.iter().any(|m| m.text == receipt.message));
        assert!(service.chat(&l).await.is_err());
        assert_eq!(io.write_count(), before + 1);
    }
}
#[tokio::test]
async fn incomplete_calendar_request_has_zero_io_writes_and_no_extra_approval() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    let mut d = decision(TEXT);
    d.end = None;
    d.end_quote = None;
    let (t, l) = sealed(&service.store, &u, TEXT, d).await;
    assert!(service.chat(&l).await.unwrap());
    assert_eq!(io.write_count(), 0);
    let turn = service
        .store
        .conversation(&u, t, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(turn.run.calendar_receipt.unwrap().state, "clarify");
    assert!(turn.assistant.iter().all(|m| !m.text.contains("approve")));
}
#[tokio::test]
async fn late_calendar_result_after_disconnect_privately_settles_same_effect_once() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, c, io) = fixture().await;
    configure(&service.store, &u).await;
    io.hold_mutation.store(true, Ordering::SeqCst);
    let (t, l) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
    let id = service
        .store
        .calendar_agent_command(&l)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    let worker = service.clone();
    let running = tokio::spawn(async move { worker.chat(&l).await });
    io.reached().await;
    service.store.disconnect_calendar(&u, c.id).await.unwrap();
    io.release.add_permits(1);
    assert!(running.await.unwrap().is_err());
    assert_eq!(io.write_count(), 1);
    let action = service
        .store
        .calendar_actions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    assert_eq!(action.state, "applied");
    assert!(action.event.is_none());
    assert!(action.receipt.is_some());
    assert!(
        service.store.conversation(&u, t, None).await.unwrap().turns[0]
            .assistant
            .is_empty()
    );
}
#[test]
fn current_intent_gate_admits_calendar_variants_but_normal_chat_stays_free() {
    for text in [
        "내일 오전10시부터 오전11시까지 팀 미팅 등록해 줘",
        "Create Synthetic meeting tomorrow from10am to11am",
        "Delete the calendar event",
        "약속을 옮겨 줘",
    ] {
        assert!(crate::mission_intent::candidate(text, false));
    }
    for text in ["Hello", "What is two plus two?", "Explain Rust ownership"] {
        assert!(!crate::mission_intent::candidate(text, false));
    }
}

#[tokio::test]
async fn sealed_calendar_update_and_delete_use_unique_full_target_fresh_etag_and_one_dispatch() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    for operation in ["update", "delete"] {
        // Independent source snapshots: no prior applied operation supplies new authority.
        io.writes.lock().unwrap().clear();
        *io.observed.lock().unwrap() = None;
        let text = if operation == "update" {
            "Rename Synthetic meeting on 2026-12-01 to Renamed meeting"
        } else {
            "Delete Synthetic meeting on 2026-12-01"
        };
        let mut d = decision(text);
        d.operation = Some(operation.into());
        d.title_quote = if operation == "update" {
            Some("Renamed meeting".into())
        } else {
            None
        };
        d.target_quote = Some("Synthetic meeting".into());
        d.start = None;
        d.start_quote = None;
        d.end = None;
        d.end_quote = None;
        let (t, l) = sealed(&service.store, &u, text, d).await;
        assert!(service.chat(&l).await.unwrap());
        assert_eq!(io.write_count(), 1);
        let record = io.writes.lock().unwrap()[0].clone();
        assert_eq!(record.event_id, "synthetic1");
        match record.mutation {
            CalendarMutation::Update { fields, .. } => {
                assert_eq!(fields.title, "Renamed meeting");
                assert_eq!(fields.description, "Synthetic only");
                assert_eq!(fields.location, "Test room")
            }
            CalendarMutation::Delete { .. } => {}
            _ => panic!(),
        };
        assert_eq!(
            service.store.conversation(&u, t, None).await.unwrap().turns[0]
                .run
                .calendar_receipt
                .as_ref()
                .unwrap()
                .state,
            "applied"
        );
    }
}

#[tokio::test]
async fn late_calendar_result_after_thread_deletion_keeps_only_origin_tombstone_and_minimum_receipt()
 {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    io.hold_mutation.store(true, Ordering::SeqCst);
    let (t, l) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
    let id = service
        .store
        .calendar_agent_command(&l)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    let worker = service.clone();
    let running = tokio::spawn(async move { worker.chat(&l).await });
    io.reached().await;
    service.store.delete_thread(&u, t).await.unwrap();
    io.release.add_permits(1);
    assert!(running.await.unwrap().is_err());
    assert_eq!(io.write_count(), 1);
    let action = service
        .store
        .calendar_actions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    assert_eq!(action.state, "applied");
    assert!(action.event.is_none());
    assert!(action.receipt.is_some());
}

#[tokio::test]
async fn calendar_cancellation_retry_claim_before_late_provider_receipt_never_revives_private_result()
 {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    io.hold_mutation.store(true, Ordering::SeqCst);
    let (thread, lease) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
    let id = service
        .store
        .calendar_agent_command(&lease)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    let worker = service.clone();
    let old_lease = lease.clone();
    let running = tokio::spawn(async move { worker.chat(&old_lease).await });
    io.reached().await;
    assert_eq!(io.write_count(), 1);
    service
        .store
        .cancel_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    service
        .store
        .retry_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    let retry = service.store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(retry.lease.run_id(), lease.run_id());
    assert!(
        service
            .store
            .calendar_agent_command(&retry.lease)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .store
            .mission_chat_planning(&retry.lease)
            .await
            .unwrap()
            .is_none()
    );
    io.release.add_permits(1);
    assert!(running.await.unwrap().is_err());
    let action = service
        .store
        .calendar_actions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    assert_eq!(action.id, id);
    assert_eq!(action.state, "applied");
    assert!(
        action.event.is_none(),
        "canceled origin may retain only the minimum receipt after Retry"
    );
    assert!(action.receipt.is_some());
    assert!(!service.chat(&retry.lease).await.unwrap());
    assert_eq!(io.write_count(), 1);
    let turn = service
        .store
        .conversation(&u, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(
        turn.run.calendar_receipt.unwrap().action_id.as_deref(),
        Some(id.as_str())
    );
    assert!(turn.assistant.is_empty());
}

#[tokio::test]
async fn calendar_cancellation_after_settlement_before_finish_purges_already_stored_private_event()
{
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, connection, io) = fixture().await;
    configure(&service.store, &u).await;
    let (_thread, lease) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
    let binding = service.binding(&u, connection.id).await.unwrap();
    let admission = service
        .store
        .admit_calendar_agent_change(&lease, &binding, primary(), None)
        .await
        .unwrap();
    // Complete the actual typed provider IO and ledger settlement; hold chat finish at this seam.
    let applied = service
        .dispatch(binding, admission, None, CancellationToken::new())
        .await
        .unwrap();
    assert!(applied.event.is_some());
    assert_eq!(io.write_count(), 1);
    let canceled = service
        .store
        .cancel_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    assert!(canceled.assistant.is_empty());
    assert!(
        service
            .store
            .finish_calendar_chat(&lease, None)
            .await
            .is_err()
    );
    let retained = service.store.calendar_actions(&u).await.unwrap().remove(0);
    assert_eq!(retained.id, applied.id);
    assert_eq!(retained.state, "applied");
    assert!(
        retained.event.is_none(),
        "settle→cancel must also erase the already retained private event"
    );
    assert_eq!(retained.receipt, applied.receipt);
}

#[tokio::test]
async fn calendar_cancellation_before_dispatch_retry_stays_ordinary_without_reclassification_or_write()
 {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    let (_thread, lease) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
    service
        .store
        .cancel_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    service
        .store
        .retry_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    let retry = service.store.claim_agent_run().await.unwrap().unwrap();
    assert!(
        service
            .store
            .calendar_agent_command(&retry.lease)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        service
            .store
            .mission_chat_planning(&retry.lease)
            .await
            .unwrap()
            .is_none()
    );
    assert!(!service.chat(&retry.lease).await.unwrap());
    assert_eq!(io.write_count(), 0);
    assert!(service.store.calendar_actions(&u).await.unwrap().is_empty());
}
