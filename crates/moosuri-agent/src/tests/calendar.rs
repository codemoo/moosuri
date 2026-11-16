//! Counted typed IO + actual durable chat ledger. No live model/Google calls.
use super::*;
use moosuri_store::{CredentialChange, PrivateCheckpoint, ProviderChange, RunLease};
use serde_json::{Value, json};
async fn configure(s: &Store, u: &UserContext) {
    configure_at(s, u, "http://127.0.0.1:11434/v1").await;
}
async fn configure_at(s: &Store, u: &UserContext, base: &str) {
    let p = s
        .create_provider(
            u,
            ProviderChange {
                name: "Synthetic Calendar model".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(base.into()),
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
            source: Default::default(),
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
        move_date_quote: None,
        move_date: None,
        all_day_quote: None,
        last_date_quote: None,
        last_date: None,
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
    sealed_in(s, u, text, d, None, "Asia/Seoul").await
}
async fn sealed_in(
    s: &Store,
    u: &UserContext,
    text: &str,
    d: CalendarChatDecision,
    thread: Option<String>,
    zone: &str,
) -> (String, RunLease) {
    let thread = match thread {
        Some(t) => t,
        None => {
            s.create_thread(u, "Synthetic Calendar chat".into())
                .await
                .unwrap()
                .id
        }
    };
    s.submit_turn_with_schedule_context(
        u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        text.into(),
        None,
        Some(ScheduleChatContext {
            timezone: zone.into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = s
        .begin_mission_chat_step(&claim.lease, claim.context.checkpoint_seq)
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
    (thread, claim.lease)
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
        .dispatch(binding.access, admission, None, CancellationToken::new())
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

async fn next_sealed(
    s: &Store,
    u: &UserContext,
    thread: &str,
    text: &str,
    d: CalendarChatDecision,
) -> RunLease {
    s.submit_turn_with_schedule_context(
        u,
        thread.into(),
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
    let lease = s.claim_agent_run().await.unwrap().unwrap().lease;
    let ctx = s.agent_context(&lease).await.unwrap();
    let step = s
        .begin_mission_chat_step(&lease, ctx.checkpoint_seq)
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
    lease
}
#[tokio::test]
async fn request_continuation_calendar_answer_creates_and_receipt_moves_preserving_duration() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    let initial = "Create Synthetic meeting on 2026-12-01 from 10:00";
    let mut partial = decision(initial);
    partial.decision = "clarify".into();
    partial.end = None;
    partial.end_quote = None;
    partial.clarification = Some("duration".into());
    let (thread, first) = sealed(&service.store, &u, initial, partial.clone()).await;
    service.chat(&first).await.unwrap();
    assert_eq!(io.write_count(), 0);
    let mut completed = partial;
    completed.decision = "command".into();
    completed.clarification = None;
    completed.duration_quote = Some("45 minutes".into());
    completed.duration_minutes = Some(45);
    let second = next_sealed(&service.store, &u, &thread, "45 minutes", completed).await;
    service.chat(&second).await.unwrap();
    assert_eq!(io.write_count(), 1);
    let turns = service
        .store
        .conversation(&u, thread.clone(), None)
        .await
        .unwrap()
        .turns;
    assert!(turns[0].assistant[0].text.contains("종료 시각이나 기간"));
    assert_eq!(
        turns[1].run.calendar_receipt.as_ref().unwrap().state,
        "applied"
    );
    let original_id = turns[1]
        .run
        .calendar_receipt
        .as_ref()
        .unwrap()
        .event_id
        .clone()
        .unwrap();
    let text = "그 일정 2026-12-02로 옮겨";
    let mut moved = decision(text);
    moved.operation = Some("update".into());
    moved.title_quote = None;
    moved.target_quote = Some("그 일정".into());
    moved.date = None;
    moved.date_quote = None;
    moved.start = None;
    moved.start_quote = None;
    moved.end = None;
    moved.end_quote = None;
    moved.move_date = Some("2026-12-02".into());
    moved.move_date_quote = Some("2026-12-02".into());
    let third = next_sealed(&service.store, &u, &thread, text, moved).await;
    service.chat(&third).await.unwrap();
    assert_eq!(io.write_count(), 2);
    let record = io.writes.lock().unwrap()[1].clone();
    assert_eq!(record.event_id, original_id);
    let CalendarMutation::Update { fields, .. } = record.mutation else {
        panic!("expected date move")
    };
    assert_eq!(fields.start, "2026-12-02T01:00:00+00:00");
    assert_eq!(fields.end, "2026-12-02T01:45:00+00:00");
    assert_eq!(fields.title, "Synthetic meeting");
}
#[tokio::test]
async fn request_continuation_calendar_withdrawal_drops_pending_authority() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, c, io) = fixture().await;
    configure(&service.store, &u).await;
    let initial = "Create Synthetic meeting on 2026-12-01 from 10:00";
    let mut partial = decision(initial);
    partial.decision = "clarify".into();
    partial.end = None;
    partial.end_quote = None;
    partial.clarification = Some("duration".into());
    let (thread, first) = sealed(&service.store, &u, initial, partial).await;
    service.chat(&first).await.unwrap();
    service.store.disconnect_calendar(&u, c.id).await.unwrap();
    service
        .store
        .submit_turn_with_schedule_context(
            &u,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "45 minutes".into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let next = service
        .store
        .claim_agent_run()
        .await
        .unwrap()
        .unwrap()
        .lease;
    assert!(
        service
            .store
            .mission_chat_planning(&next)
            .await
            .unwrap()
            .unwrap()
            .continuation
            .is_none()
    );
    assert_eq!(io.write_count(), 0);
}

#[tokio::test]
async fn request_continuation_receipt_stale_etag_never_writes_or_retargets() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    let (thread, first) = sealed(&service.store, &u, TEXT, decision(TEXT)).await;
    service.chat(&first).await.unwrap();
    assert_eq!(io.write_count(), 1);
    let id = service
        .store
        .conversation(&u, thread.clone(), None)
        .await
        .unwrap()
        .turns[0]
        .run
        .calendar_receipt
        .as_ref()
        .unwrap()
        .event_id
        .clone()
        .unwrap();
    io.lookup_mode.store(4, Ordering::SeqCst);
    let text = "그 일정 2026-12-02로 옮겨";
    let mut d = decision(text);
    d.operation = Some("update".into());
    d.title_quote = None;
    d.target_quote = Some("그 일정".into());
    d.date = None;
    d.date_quote = None;
    d.start = None;
    d.start_quote = None;
    d.end = None;
    d.end_quote = None;
    d.move_date = Some("2026-12-02".into());
    d.move_date_quote = Some("2026-12-02".into());
    let next = next_sealed(&service.store, &u, &thread, text, d).await;
    service.chat(&next).await.unwrap();
    assert_eq!(io.write_count(), 1);
    assert!(
        io.event_calls
            .lock()
            .unwrap()
            .iter()
            .all(|value| value == &id)
    );
    assert_eq!(
        service
            .store
            .conversation(&u, thread, None)
            .await
            .unwrap()
            .turns[1]
            .run
            .calendar_receipt
            .as_ref()
            .unwrap()
            .state,
        "clarify"
    );
}

#[tokio::test]
async fn all_day_natural_create_move_and_delete_keep_civil_span_and_exact_receipt_target() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _connection, io) = fixture().await;
    configure(&service.store, &u).await;
    let text = "Create all-day Synthetic meeting from 2027-03-14 through 2027-03-16";
    assert!(crate::mission_intent::candidate(text, false));
    let mut d = decision(text);
    d.date = Some("2027-03-14".into());
    d.date_quote = d.date.clone();
    d.start = None;
    d.start_quote = None;
    d.end = None;
    d.end_quote = None;
    d.all_day_quote = Some("all-day".into());
    d.last_date = Some("2027-03-16".into());
    d.last_date_quote = d.last_date.clone();
    let (thread, l) = sealed_in(&service.store, &u, text, d, None, "America/Los_Angeles").await;
    assert!(service.chat(&l).await.unwrap());
    let created = io.writes.lock().unwrap()[0].clone();
    let CalendarMutation::Create { fields } = &created.mutation else {
        panic!()
    };
    assert_eq!((&*fields.start, &*fields.end), ("2027-03-14", "2027-03-17"));
    assert!(fields.time_zone.is_none());

    let text = "Move that event to 2027-11-07";
    let mut d = decision(text);
    d.operation = Some("update".into());
    d.title_quote = None;
    d.target_quote = Some("that event".into());
    d.date = None;
    d.date_quote = None;
    d.start = None;
    d.start_quote = None;
    d.end = None;
    d.end_quote = None;
    d.move_date = Some("2027-11-07".into());
    d.move_date_quote = d.move_date.clone();
    let (_, l) = sealed_in(
        &service.store,
        &u,
        text,
        d.clone(),
        Some(thread.clone()),
        "America/Los_Angeles",
    )
    .await;
    assert!(service.chat(&l).await.unwrap());
    let moved = io.writes.lock().unwrap()[1].clone();
    assert_eq!(moved.event_id, created.event_id);
    let CalendarMutation::Update { fields, etag, .. } = moved.mutation else {
        panic!()
    };
    assert_eq!((&*fields.start, &*fields.end), ("2027-11-07", "2027-11-10"));
    assert!(fields.time_zone.is_none());
    assert_eq!(etag, "\"etag-prior\"");

    let text = "Delete that event";
    d.intent_quote = Some(text.into());
    d.operation = Some("delete".into());
    d.move_date = None;
    d.move_date_quote = None;
    let (_, l) = sealed_in(
        &service.store,
        &u,
        text,
        d,
        Some(thread),
        "America/Los_Angeles",
    )
    .await;
    assert!(service.chat(&l).await.unwrap());
    let removed = io.writes.lock().unwrap()[2].clone();
    assert_eq!(removed.event_id, created.event_id);
    assert!(matches!(removed.mutation, CalendarMutation::Delete { .. }));
    let actions = service.store.calendar_actions(&u).await.unwrap();
    assert_eq!(actions.len(), 3);
    assert!(
        actions
            .iter()
            .all(|a| a.state == "applied" && a.receipt.is_some())
    );
    assert!(
        actions
            .iter()
            .find(|a| a.id == created.id)
            .unwrap()
            .event
            .as_ref()
            .unwrap()
            .all_day
    );
    assert!(service.chat(&l).await.is_err());
    assert_eq!(io.write_count(), 3);
}

#[tokio::test]
async fn all_day_missing_last_day_continuation_preserves_frozen_kind_and_start_date() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _c, io) = fixture().await;
    configure(&service.store, &u).await;
    let text = "Create all-day Synthetic meeting on 2027-03-14 for several days";
    let mut d = decision(text);
    d.decision = "clarify".into();
    d.clarification = Some("duration".into());
    d.date = Some("2027-03-14".into());
    d.date_quote = d.date.clone();
    d.start = None;
    d.start_quote = None;
    d.end = None;
    d.end_quote = None;
    d.all_day_quote = Some("all-day".into());
    let (thread, l) = sealed_in(
        &service.store,
        &u,
        text,
        d.clone(),
        None,
        "America/Los_Angeles",
    )
    .await;
    assert!(service.chat(&l).await.unwrap());
    assert_eq!(io.write_count(), 0);
    d.decision = "command".into();
    d.clarification = None;
    d.last_date = Some("2027-03-16".into());
    d.last_date_quote = d.last_date.clone();
    let (_, l) = sealed_in(
        &service.store,
        &u,
        "2027-03-16",
        d,
        Some(thread),
        "America/Los_Angeles",
    )
    .await;
    assert!(service.chat(&l).await.unwrap());
    assert_eq!(io.write_count(), 1);
    let record = io.writes.lock().unwrap()[0].clone();
    let CalendarMutation::Create { fields } = record.mutation else {
        panic!()
    };
    assert_eq!((&*fields.start, &*fields.end), ("2027-03-14", "2027-03-17"));
}

#[tokio::test]
async fn runtime_artifact_no_command_then_calendar_uses_second_completed_classifier_once() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, u, _connection, io) = fixture().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let counted = requests.clone();
    let app = axum::Router::new().route("/v1/chat/completions",axum::routing::post(move |axum::Json(body): axum::Json<serde_json::Value>| {
        let counted = counted.clone();
        async move {
            let n = counted.fetch_add(1,Ordering::SeqCst);
            assert_eq!(body["messages"].as_array().unwrap().len(),2);
            let text = body["messages"][1]["content"].as_str().unwrap();
            let (name,args) = if n == 0 {
                assert_eq!(body["tools"][0]["function"]["name"],"artifact_intent");
                ("artifact_intent",serde_json::json!({"decision":"no_command","action":null,"kind":null,"intent_quote":null,"target_quote":null}))
            } else {
                assert_eq!(n,1,"no replay or final model call after sealed Calendar command");
                assert!(body["tools"].as_array().unwrap().iter().any(|v|v["function"]["name"]=="mission_chat_decision"));
                ("calendar_chat_decision",serde_json::to_value(decision(text)).unwrap())
            };
            let frame = |v: serde_json::Value| format!("data: {v}\n\n");
            let content = frame(serde_json::json!({"id":format!("composition-{n}"),"model":"synthetic","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("intent-{n}"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":null}]}))
                + &frame(serde_json::json!({"id":format!("composition-{n}"),"model":"synthetic","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5}})) + "data: [DONE]\n\n";
            axum::response::Response::builder().header("content-type","text/event-stream").body(axum::body::Body::from(content)).unwrap()
        }
    }));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    configure_at(&service.store, &u, &base).await;
    let thread = service
        .store
        .create_thread(&u, "Synthetic planner composition".into())
        .await
        .unwrap();
    let text = format!("문서 저장 기능과 별개로 {TEXT}");
    service
        .store
        .submit_turn_with_schedule_context(
            &u,
            thread.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            text,
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let claim = service.store.claim_agent_run().await.unwrap().unwrap();
    let (changes, _) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let subscriptions =
        crate::subscriptions::SubscriptionService::new(service.store.clone(), signals.clone());
    crate::runner::execute_with_calendar(
        service.store.clone(),
        claim,
        signals,
        service.mail.clone(),
        crate::web::Service::new(None),
        subscriptions,
        service.clone(),
    )
    .await;
    let done = service
        .store
        .conversation(&u, thread.id, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert_eq!(done.run.calendar_receipt.unwrap().state, "applied");
    assert!(done.run.mission_receipt.is_none());
    assert_eq!(requests.load(Ordering::SeqCst), 2);
    assert_eq!(
        service
            .store
            .usage_summary(&u)
            .await
            .unwrap()
            .dispatched_calls,
        2
    );
    assert_eq!(io.write_count(), 1);
    assert!(
        service
            .store
            .artifacts(&u, None, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    upstream.abort();
}

#[tokio::test]
async fn calendar_model_reason_contract_keeps_the_same_pending_action_until_duration_is_supplied() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let (_dir, service, user, _connection, io) = fixture().await;
    let store = &service.store;
    configure(store, &user).await;
    let initial = "Create Synthetic meeting on 2026-12-01 from 10:00";
    let thread = store
        .create_thread(&user, "Synthetic intent contract".into())
        .await
        .unwrap()
        .id;
    store
        .submit_turn_with_schedule_context(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            initial.into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_mission_chat_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    let mut partial = decision(initial);
    partial.decision = "clarify".into();
    partial.end = None;
    partial.end_quote = None;
    partial.clarification = Some("duration".into());
    let raw = serde_json::to_value(&partial).unwrap();
    let checkpoint = |decision: Value| PrivateCheckpoint {
        bytes: json!({"kind":"mission_chat_decision","decision":decision})
            .to_string()
            .into_bytes(),
    };
    // These were schema-valid strings but could never become an answerable
    // continuation. Both the current-model parser and Store must reject them.
    for reason in ["When should it end?", "duration ", ""] {
        let mut bad = raw.clone();
        bad["clarification"] = json!(reason);
        assert!(crate::calendar_intent::parse(&bad.to_string()).is_none());
        assert!(matches!(
            store
                .commit_agent_step(&step, checkpoint(bad), vec![])
                .await,
            Err(StoreError::InvalidInput)
        ));
    }
    let mut missing_reason = raw.clone();
    missing_reason["clarification"] = Value::Null;
    assert!(crate::calendar_intent::parse(&missing_reason.to_string()).is_none());
    assert!(matches!(
        store
            .commit_agent_step(&step, checkpoint(missing_reason), vec![])
            .await,
        Err(StoreError::InvalidInput)
    ));
    for key in moosuri_core::CALENDAR_CHAT_FIELDS {
        let mut bad = raw.clone();
        bad.as_object_mut().unwrap().remove(*key);
        assert!(
            crate::calendar_intent::parse(&bad.to_string()).is_none(),
            "missing {key}"
        );
        assert!(
            matches!(
                store
                    .commit_agent_step(&step, checkpoint(bad), vec![])
                    .await,
                Err(StoreError::InvalidInput)
            ),
            "missing {key}"
        );
    }
    assert!(store.calendar_actions(&user).await.unwrap().is_empty());
    let parsed = crate::calendar_intent::parse(&raw.to_string()).unwrap();
    store
        .commit_agent_step(
            &step,
            checkpoint(serde_json::to_value(parsed).unwrap()),
            vec![],
        )
        .await
        .unwrap();
    store
        .finish_calendar_chat(&claim.lease, Some("clarify".into()))
        .await
        .unwrap();
    assert_eq!(io.write_count(), 0);

    let answer = "45 minutes";
    store
        .submit_turn_with_schedule_context(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            answer.into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let planning = store
        .mission_chat_planning(&claim.lease)
        .await
        .unwrap()
        .unwrap();
    let continuation = planning.continuation.unwrap();
    assert_eq!(continuation.initiating_request, initial);
    assert_eq!(continuation.answers, [answer]);
    assert_eq!(continuation.missing_field, "duration");
    let CurrentChatDecision::Calendar(draft) = continuation.draft else {
        panic!("same Calendar draft required")
    };
    assert_eq!(draft.intent_quote, partial.intent_quote);
    assert_eq!(draft.start, partial.start);
    assert_eq!(draft.title_quote, partial.title_quote);
    let mut complete = partial;
    complete.decision = "command".into();
    complete.clarification = None;
    complete.duration_quote = Some(answer.into());
    complete.duration_minutes = Some(45);
    let parsed = crate::calendar_intent::parse(&serde_json::to_string(&complete).unwrap()).unwrap();
    let step = store
        .begin_mission_chat_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            checkpoint(serde_json::to_value(parsed).unwrap()),
            vec![],
        )
        .await
        .unwrap();
    assert!(service.chat(&claim.lease).await.unwrap());
    assert_eq!(io.write_count(), 1);
    let actions = store.calendar_actions(&user).await.unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].state, "applied");
    assert!(store.mail_actions(&user).await.unwrap().is_empty());
}
