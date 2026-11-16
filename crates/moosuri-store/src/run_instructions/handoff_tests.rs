//! Synthetic owner-visible durable events, without model/provider IO.
use super::*;
use crate::PrivateCheckpoint;
use crate::agent_execution::{KnowledgeToolInput, PendingKnowledgeCall};
use crate::tests::TEST_LOCK;
use moosuri_core::*;

async fn origin(s: &Store, u: &UserContext, mission: &MissionView) -> (String, String, i64) {
    let thread = s
        .create_thread(u, "Synthetic task conversation".into())
        .await
        .unwrap();
    let turn = s
        .submit_turn(
            u,
            thread.id.clone(),
            Uuid::new_v4().to_string(),
            "Calculate the synthetic total".into(),
        )
        .await
        .unwrap();
    let tenant = u.tenant.0.clone();
    let id = turn.run.id.clone();
    let receipt = MissionChatReceipt {
        request_id: turn.run.request_id,
        action: "create_now".into(),
        target: Some(MissionChatTarget {
            kind: "one_off".into(),
            id: mission.id.clone(),
            revision: mission.revision,
        }),
        state: "queued".into(),
        next_local_slot: None,
        candidates: vec![],
    };
    // The classifier/admission is outside this regression. Seed its ordinary
    // typed receipt, then use the real origin transition and public events API.
    s.call(move |db| {
        let tx = db
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let run = chat::load_run(&tx, &tenant, &id)?;
        tx.execute(
            "INSERT INTO mission_chat_commands VALUES(?1,?2,?3)",
            params![tenant, id, serde_json::to_string(&receipt).unwrap()],
        )?;
        chat::transition(&tx, &run, RunState::Completed, None)?;
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
    let events = s.chat_events(u, thread.id.clone(), 0).await.unwrap();
    let initial = events
        .events
        .iter()
        .rev()
        .find_map(|e| match &e.change {
            ChatChange::State { run } if run.id == turn.run.id => Some(run),
            _ => None,
        })
        .unwrap();
    assert!(
        initial
            .instruction_target
            .as_ref()
            .unwrap()
            .supports_instructions
    );
    (thread.id, turn.run.id, events.cursor)
}

async fn changed(
    s: &Store,
    u: &UserContext,
    thread: &str,
    origin: &str,
    cursor: i64,
) -> (RunView, i64) {
    let events = s.chat_events(u, thread.into(), cursor).await.unwrap();
    assert!(!events.reset);
    let mut states = events.events.into_iter().filter_map(|e| match e.change {
        ChatChange::State { run } if run.id == origin => Some(run),
        _ => None,
    });
    let state = states
        .next()
        .expect("original conversation needs a durable eligibility event");
    assert!(
        states.next().is_none(),
        "one changed projection, no nested duplicate events"
    );
    (state, events.cursor)
}
async fn committed(
    s: &Store,
    lease: &RunLease,
    through: i64,
    calls: Vec<PendingKnowledgeCall>,
) -> crate::StepLease {
    let context = s.agent_context(lease).await.unwrap();
    let step = s
        .begin_instructed_agent_step(lease, context.checkpoint_seq, through)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"kind":"mission_receipt_checkpoint"}"#.to_vec(),
        },
        calls,
    )
    .await
    .unwrap();
    step
}

#[tokio::test]
async fn task_handoff_completion_updates_original_open_chat_with_zero_or_applied_guidance() {
    let _guard = TEST_LOCK.lock().await;
    for guided in [false, true] {
        let (_dir, s, u) = crate::missions::tests::setup().await;
        let mission = s
            .create_mission(&u, crate::missions::tests::input(MissionMode::Durable))
            .await
            .unwrap();
        let (thread, original, mut cursor) = origin(&s, &u, &mission).await;
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        if guided {
            s.submit_run_instruction(
                &u,
                claim.lease.run_id().into(),
                RunInstructionRequest {
                    request_id: Uuid::new_v4().to_string(),
                    expected_attempt: 1,
                    text: "Keep the literal exact total".into(),
                },
            )
            .await
            .unwrap();
        }
        let step = committed(
            &s,
            &claim.lease,
            i64::from(guided),
            vec![PendingKnowledgeCall {
                call_id: "sum".into(),
                input: KnowledgeToolInput::DecimalCalculate {
                    calculation: DecimalCalculation {
                        operation: DecimalOperation::Sum,
                        operands: vec!["0.1".into(), "0.2".into()],
                    },
                },
            }],
        )
        .await;
        s.apply_step_instructions(&step).await.unwrap();
        s.mission_save_plan(&step, vec!["Calculate and verify".into()])
            .await
            .unwrap();
        s.execute_knowledge_tool(&step, "sum".into()).await.unwrap();
        // Consume receipt-only notifications before observing the terminal edge.
        cursor = s
            .chat_events(&u, thread.clone(), cursor)
            .await
            .unwrap()
            .cursor;
        assert!(
            s.mission_verify(
                &step,
                MissionCandidate {
                    text: "Exact total 0.3".into(),
                    evidence: vec![MissionEvidence {
                        step: step.step(),
                        call_id: "sum".into()
                    }],
                }
            )
            .await
            .unwrap()
        );
        let (event, next) = changed(&s, &u, &thread, &original, cursor).await;
        assert!(!event.supports_instructions);
        assert!(!event.instruction_target.unwrap().supports_instructions);
        assert_eq!(
            event.state,
            RunState::Completed,
            "original admission identity remains completed"
        );
        for _ in 0..3 {
            s.poll_recurring_missions().await.unwrap();
        }
        assert!(
            s.chat_events(&u, thread.clone(), next)
                .await
                .unwrap()
                .events
                .is_empty()
        );
        // With the event's false eligibility, the composer's next-message path
        // admits a new ordinary turn instead of steering the completed carrier.
        let next_turn = s
            .submit_turn(
                &u,
                thread.clone(),
                Uuid::new_v4().to_string(),
                "Explain another synthetic topic".into(),
            )
            .await
            .unwrap();
        assert_ne!(next_turn.run.id, original);
        assert_eq!(next_turn.sequence, 2);
    }
}

#[tokio::test]
async fn task_handoff_input_wait_resume_and_cancel_publish_same_carrier_without_poll_storms() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let mission = s
        .create_mission(&u, crate::missions::tests::input(MissionMode::Durable))
        .await
        .unwrap();
    let (thread, original, cursor) = origin(&s, &u, &mission).await;
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let carrier = claim.lease.run_id().to_owned();
    let step = committed(&s, &claim.lease, 0, vec![]).await;
    s.mission_save_plan(&step, vec!["Calculate and verify".into()])
        .await
        .unwrap();
    s.mission_wait_for_input(
        &step,
        1,
        MissionInputRequest {
            field: MissionInputField::Context,
            question: "Which synthetic criterion?".into(),
            candidates: vec![],
        },
    )
    .await
    .unwrap();
    let (waiting, cursor) = changed(&s, &u, &thread, &original, cursor).await;
    assert!(!waiting.instruction_target.unwrap().supports_instructions);
    let wait = s.missions(&u).await.unwrap().remove(0);
    let resumed = s
        .reply_mission_input(
            &u,
            mission.id.clone(),
            MissionInputReply {
                request_id: Uuid::new_v4().to_string(),
                wait_request_id: wait.input_wait.unwrap().request_id,
                expected_revision: wait.revision,
                value: "Use the original total".into(),
            },
        )
        .await
        .unwrap();
    let (ready, cursor) = changed(&s, &u, &thread, &original, cursor).await;
    let target = ready.instruction_target.unwrap();
    assert!(target.supports_instructions);
    assert_eq!(target.run_id, carrier);
    assert_eq!(target.attempt, 2);
    let cancel = MissionControl {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: resumed.revision,
        action: MissionAction::Cancel,
    };
    s.control_mission(&u, mission.id.clone(), cancel.clone())
        .await
        .unwrap();
    let (cancelled, cursor) = changed(&s, &u, &thread, &original, cursor).await;
    assert!(!cancelled.instruction_target.unwrap().supports_instructions);
    s.control_mission(&u, mission.id, cancel).await.unwrap();
    for _ in 0..3 {
        s.poll_recurring_missions().await.unwrap();
    }
    assert!(
        s.chat_events(&u, thread, cursor)
            .await
            .unwrap()
            .events
            .is_empty()
    );
}

#[tokio::test]
async fn task_handoff_model_capacity_and_deleted_target_emit_once_after_retention_loss() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let mission = s
        .create_mission(&u, crate::missions::tests::input(MissionMode::Durable))
        .await
        .unwrap();
    let (thread, original, cursor) = origin(&s, &u, &mission).await;
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    for _ in 0..5 {
        committed(&s, &claim.lease, 0, vec![]).await;
    }
    assert!(
        s.chat_events(&u, thread.clone(), cursor)
            .await
            .unwrap()
            .events
            .is_empty()
    );
    committed(&s, &claim.lease, 0, vec![]).await;
    let (capacity, _) = changed(&s, &u, &thread, &original, cursor).await;
    assert!(!capacity.instruction_target.unwrap().supports_instructions);
    // Prune the origin's old projection exactly as retained output churn can.
    // The mutation must still emit a replacement, never depend on SSE reads.
    let tenant = u.tenant.0.clone();
    let tid = thread.clone();
    s.call(move |db| {
        db.conn.execute(
            "DELETE FROM chat_events WHERE tenant_id=?1 AND thread_id=?2",
            params![tenant, tid],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let cursor = s
        .chat_events(&u, thread.clone(), i64::MAX)
        .await
        .unwrap()
        .cursor;
    s.delete_mission(&u, mission.id).await.unwrap();
    let (deleted, cursor) = changed(&s, &u, &thread, &original, cursor).await;
    assert!(deleted.instruction_target.is_none());
    assert!(deleted.mission_receipt.unwrap().target.is_none());
    for _ in 0..3 {
        s.poll_recurring_missions().await.unwrap();
    }
    assert!(
        s.chat_events(&u, thread, cursor)
            .await
            .unwrap()
            .events
            .is_empty()
    );
}
