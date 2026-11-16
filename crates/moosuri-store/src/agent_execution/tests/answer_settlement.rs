//! Guidance acknowledgment and answer disposition share one transaction.
use super::*;

#[tokio::test]
async fn answer_settlement_rolls_back_guidance_when_completion_fails() {
    failed_settlement(false).await;
}

#[tokio::test]
async fn answer_settlement_rolls_back_guidance_when_superseding_fails() {
    failed_settlement(true).await;
}

#[tokio::test]
async fn answer_settlement_cannot_skip_tools_or_revive_a_cancelled_run() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let provider = store
        .list_providers(&user)
        .await
        .unwrap()
        .providers
        .remove(0);
    store
        .record_tool_probe(
            &user,
            provider.id,
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Synthetic unfinished tool".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    store.enable_run_instructions(&claim.lease).await.unwrap();
    let request = RunInstructionRequest {
        request_id: Uuid::new_v4().to_string(),
        expected_attempt: 1,
        text: "Preserve the synthetic tool result".into(),
    };
    store
        .submit_run_instruction(&user, claim.lease.run_id().into(), request.clone())
        .await
        .unwrap();
    let step = store
        .begin_instructed_agent_step(&claim.lease, claim.context.checkpoint_seq, 1)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            capsule("a tool call, not a final answer"),
            vec![PendingKnowledgeCall {
                call_id: "unfinished-calculation".into(),
                input: KnowledgeToolInput::DecimalCalculate {
                    calculation: DecimalCalculation {
                        operation: DecimalOperation::Sum,
                        operands: vec!["2".into(), "3".into()],
                    },
                },
            }],
        )
        .await
        .unwrap();
    assert!(matches!(
        store.settle_agent_answer(&step).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .run_instruction(
                &user,
                claim.lease.run_id().into(),
                request.request_id.clone()
            )
            .await
            .unwrap()
            .state,
        "received"
    );
    assert!(
        store.agent_context(&claim.lease).await.unwrap().calls[0]
            .result
            .is_none()
    );
    store
        .cancel_run(&user, claim.lease.run_id().into(), 1)
        .await
        .unwrap();
    assert!(store.settle_agent_answer(&step).await.is_err());
    let done = store
        .conversation(&user, thread.id, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.state, RunState::Cancelled);
    assert!(done.assistant.is_empty());
    assert_eq!(
        store
            .run_instruction(&user, claim.lease.run_id().into(), request.request_id)
            .await
            .unwrap()
            .state,
        "closed"
    );
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        1
    );
}

async fn failed_settlement(late: bool) {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let provider = store
        .list_providers(&user)
        .await
        .unwrap()
        .providers
        .remove(0);
    store
        .record_tool_probe(
            &user,
            provider.id,
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Synthetic answer settlement".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    store.enable_run_instructions(&claim.lease).await.unwrap();
    let instruction = RunInstructionRequest {
        request_id: Uuid::new_v4().to_string(),
        expected_attempt: 1,
        text: "Use the first synthetic constraint".into(),
    };
    store
        .submit_run_instruction(&user, claim.lease.run_id().into(), instruction.clone())
        .await
        .unwrap();
    let step = store
        .begin_instructed_agent_step(&claim.lease, claim.context.checkpoint_seq, 1)
        .await
        .unwrap();
    store
        .append_agent_output(
            &step,
            vec![OutputPatch::Append {
                item: "step-1-answer".into(),
                text: "First constrained answer".into(),
                phase: Some(MessagePhase::FinalAnswer),
                refusal: false,
            }],
        )
        .await
        .unwrap();
    let context = store
        .commit_agent_step(&step, capsule("first completed response"), vec![])
        .await
        .unwrap();
    let second = RunInstructionRequest {
        request_id: Uuid::new_v4().to_string(),
        expected_attempt: 1,
        text: "Use the later synthetic constraint too".into(),
    };
    if late {
        store
            .submit_run_instruction(&user, claim.lease.run_id().into(), second.clone())
            .await
            .unwrap();
    }
    let before = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    store.call(move |db| {
        // Fail the last domain write, after the instruction acknowledgment.
        // This connection-local trigger never exists in a production DB.
        db.conn.execute_batch(if late {
            "CREATE TEMP TRIGGER fail_answer_settlement BEFORE UPDATE OF phase ON chat_output WHEN NEW.phase='commentary' BEGIN SELECT RAISE(ABORT,'synthetic answer interruption'); END;"
        } else {
            "CREATE TEMP TRIGGER fail_answer_settlement BEFORE UPDATE OF state ON chat_runs WHEN NEW.state='completed' BEGIN SELECT RAISE(ABORT,'synthetic answer interruption'); END;"
        })?;
        Ok(())
    }).await.unwrap();
    assert!(
        store.settle_agent_answer(&step).await.is_err(),
        "failed disposition cannot authorize another model round"
    );
    let receipt = store
        .run_instruction(
            &user,
            claim.lease.run_id().into(),
            instruction.request_id.clone(),
        )
        .await
        .unwrap();
    assert_eq!(
        receipt.state, "received",
        "failed answer settlement must not acknowledge guidance"
    );
    let after = store
        .conversation(&user, thread.id.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap(),
        "failed transaction cannot publish an intermediate projection"
    );
    store
        .call(|db| {
            db.conn
                .execute_batch("DROP TRIGGER fail_answer_settlement")?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(store.settle_agent_answer(&step).await.unwrap(), !late);
    assert_eq!(
        store
            .run_instruction(&user, claim.lease.run_id().into(), instruction.request_id)
            .await
            .unwrap()
            .state,
        "applied"
    );
    if late {
        assert_eq!(
            store
                .run_instruction(
                    &user,
                    claim.lease.run_id().into(),
                    second.request_id.clone()
                )
                .await
                .unwrap()
                .state,
            "received"
        );
        let current = store
            .conversation(&user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns
            .remove(0);
        assert_eq!(current.run.state, RunState::Running);
        assert_eq!(current.assistant[0].phase, Some(MessagePhase::Commentary));
        let next = store
            .begin_instructed_agent_step(&claim.lease, context.checkpoint_seq, 2)
            .await
            .unwrap();
        assert!(
            store.settle_agent_answer(&step).await.is_err(),
            "old step cannot acknowledge new guidance"
        );
        assert!(
            store.settle_agent_answer(&next).await.is_err(),
            "uncommitted response cannot complete"
        );
        store
            .append_agent_output(
                &next,
                vec![OutputPatch::Append {
                    item: "step-2-answer".into(),
                    text: "Both constrained answers".into(),
                    phase: Some(MessagePhase::FinalAnswer),
                    refusal: false,
                }],
            )
            .await
            .unwrap();
        store
            .commit_agent_step(&next, capsule("second completed response"), vec![])
            .await
            .unwrap();
        assert!(store.settle_agent_answer(&next).await.unwrap());
        assert_eq!(
            store
                .run_instruction(&user, claim.lease.run_id().into(), second.request_id)
                .await
                .unwrap()
                .state,
            "applied"
        );
    }
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        if late { 2 } else { 1 }
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let done = store
        .conversation(&user, thread.id, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(
        done.assistant
            .iter()
            .filter(|m| m.phase == Some(MessagePhase::FinalAnswer))
            .count(),
        1
    );
}
