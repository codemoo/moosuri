//! Stable provider-connection continuation must retain the same owner guidance.
use super::*;

async fn snapshot(s: &Store, tenant: &str, run: &str) -> serde_json::Value {
    let tenant = tenant.to_owned();
    let run = run.to_owned();
    s.call(move |db| {
        let row = db.conn.query_row(
            "SELECT r.model,a.remote_calls,a.tool_calls,a.started_ms,a.deadline_ms,a.checkpoint_seq,m.repairs,(SELECT count(*) FROM agent_steps WHERE tenant_id=r.tenant_id AND run_id=r.id),(SELECT count(*) FROM agent_tool_slots WHERE tenant_id=r.tenant_id AND run_id=r.id),(SELECT result FROM agent_tool_slots WHERE tenant_id=r.tenant_id AND run_id=r.id AND call_id='observed') FROM chat_runs r JOIN agent_execution a ON a.tenant_id=r.tenant_id AND a.run_id=r.id JOIN missions m ON m.tenant_id=r.tenant_id AND m.run_id=r.id WHERE r.tenant_id=?1 AND r.id=?2",
            params![tenant, run],
            |r| Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,i64>(5)?,r.get::<_,i64>(6)?,r.get::<_,i64>(7)?,r.get::<_,i64>(8)?,r.get::<_,String>(9)?)),
        )?;
        Ok(serde_json::to_value(row).unwrap())
    }).await.unwrap()
}

async fn continuity(applied: bool) {
    let _lock = TEST_LOCK.lock().await;
    for original_attempt in [1, 32] {
        let (_dir, s, u) = crate::missions::tests::setup().await;
        let (mail, _) = connections(&s, &u, "guidance-continuity").await;
        let m = s
            .create_mission(
                &u,
                input(
                    MissionMode::Durable,
                    "Read Gmail and preserve the original criteria",
                ),
            )
            .await
            .unwrap();
        if original_attempt == 32 {
            let tenant = u.tenant.0.clone();
            let id = m.id.clone();
            s.call(move |db| { db.conn.execute("UPDATE chat_runs SET attempt=32 WHERE tenant_id=?1 AND id=(SELECT run_id FROM missions WHERE tenant_id=?1 AND id=?2)", params![tenant,id])?; Ok(()) }).await.unwrap();
        }
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let run = claim.lease.run_id().to_owned();
        let request = RunInstructionRequest {
            request_id: Uuid::new_v4().to_string(),
            expected_attempt: original_attempt,
            text: "Keep the synthetic owner criterion in the resumed result".into(),
        };
        let mut receipt = if applied {
            Some(
                s.submit_run_instruction(&u, run.clone(), request.clone())
                    .await
                    .unwrap(),
            )
        } else {
            None
        };
        let context = s.agent_context(&claim.lease).await.unwrap();
        let st = s
            .begin_instructed_agent_step(
                &claim.lease,
                context.checkpoint_seq,
                if applied { 1 } else { 0 },
            )
            .await
            .unwrap();
        s.commit_agent_step(
            &st,
            crate::PrivateCheckpoint {
                bytes: br#"{"kind":"mission_receipt_checkpoint"}"#.to_vec(),
            },
            vec![call("observed", search(crate::connection_waits::MAIL_SLOT))],
        )
        .await
        .unwrap();
        let permit = permit(&s, &st, "observed").await;
        s.complete_external_tool(
            &permit,
            KnowledgeToolResult::External(
                serde_json::json!({"messages":[{"id":"synthetic-observed"}]}),
            ),
        )
        .await
        .unwrap();
        if applied {
            s.apply_step_instructions(&st).await.unwrap();
        } else {
            // Accepted after the completed read: still unassigned when the task yields.
            receipt = Some(
                s.submit_run_instruction(&u, run.clone(), request.clone())
                    .await
                    .unwrap(),
            );
        }
        let receipt = receipt.unwrap();
        let before = snapshot(&s, &u.tenant.0, &run).await;
        s.mission_wait_for_connection(
            &claim.lease,
            MissionConnectionWait {
                request_id: Uuid::new_v4().to_string(),
                slot_id: crate::connection_waits::MAIL_SLOT.into(),
                acceptance_revision: 1,
                cause: "vault_unlock".into(),
                operation_expires_at_ms: m.expires_at_ms - 1000,
            },
        )
        .await
        .unwrap();
        let waiting = s
            .missions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == m.id)
            .unwrap();
        assert_eq!(waiting.state, "waiting");
        s.select_mission_connection(&u, m.id.clone(), selection(&waiting, &mail))
            .await
            .unwrap();
        let resumed = s.claim_agent_run().await.unwrap().unwrap();
        assert_eq!(resumed.lease.run_id(), run);
        assert_eq!(
            snapshot(&s, &u.tenant.0, &run).await,
            before,
            "same model, receipt, checkpoint and original budgets; no read replay"
        );
        let retained = if applied {
            s.applied_run_instructions(&resumed.lease).await.unwrap()
        } else {
            s.pending_run_instructions(&resumed.lease).await.unwrap()
        };
        assert_eq!(
            retained.len(),
            1,
            "accepted guidance must reach the resumed model context"
        );
        assert_eq!(retained[0].request_id, receipt.request_id);
        assert_eq!(retained[0].text, request.text);
        assert_eq!(
            retained[0].attempt, original_attempt,
            "the original admission receipt remains immutable"
        );
        assert!(
            s.apply_step_instructions(&st).await.is_err(),
            "retired lease cannot apply guidance, including at attempt32"
        );
        let context = s.agent_context(&resumed.lease).await.unwrap();
        let next = s
            .begin_instructed_agent_step(
                &resumed.lease,
                context.checkpoint_seq,
                if applied { 0 } else { receipt.sequence },
            )
            .await
            .unwrap();
        s.commit_agent_step(
            &next,
            crate::PrivateCheckpoint {
                bytes: br#"{"kind":"mission_receipt_checkpoint"}"#.to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
        s.apply_step_instructions(&next).await.unwrap();
        let retained = s.applied_run_instructions(&resumed.lease).await.unwrap();
        assert_eq!(retained.len(), 1);
        assert_eq!(retained[0].request_id, receipt.request_id);
        let current = s
            .missions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == m.id)
            .unwrap();
        let attempt = current.instruction_target.as_ref().unwrap().attempt;
        assert_eq!(attempt, (original_attempt + 1).min(32));
        let later = s
            .submit_run_instruction(
                &u,
                run.clone(),
                RunInstructionRequest {
                    request_id: Uuid::new_v4().to_string(),
                    expected_attempt: attempt,
                    text: "Synthetic guidance cancelled before dispatch".into(),
                },
            )
            .await
            .unwrap();
        s.control_mission(
            &u,
            m.id,
            MissionControl {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: current.revision,
                action: MissionAction::Cancel,
            },
        )
        .await
        .unwrap();
        assert!(s.apply_step_instructions(&next).await.is_err());
        let cancelled = s.run_instruction(&u, run, later.request_id).await.unwrap();
        assert_eq!(cancelled.state, "closed");
        assert_eq!(cancelled.reason.as_deref(), Some("mission_stopped"));
    }
}

#[tokio::test]
async fn connection_guidance_pending_survives_same_carrier_promotion() {
    continuity(false).await;
}
#[tokio::test]
async fn connection_guidance_applied_survives_same_carrier_promotion() {
    continuity(true).await;
}
