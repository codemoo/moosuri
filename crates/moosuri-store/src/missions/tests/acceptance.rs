use super::*;
#[tokio::test]
async fn mission_acceptance_pending_local_slots_cannot_block_or_resume_the_old_plan() {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let old = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(&s, &old.lease, vec![calc()]).await;
    let paused = s
        .control_mission(&u, m.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    s.revise_mission_acceptance(&u, m.id.clone(), update(paused.revision))
        .await
        .unwrap();
    let fresh = s.claim_agent_run().await.unwrap().unwrap();
    assert!(fresh.context.calls.is_empty());
    assert!(matches!(
        s.mission_step_lease(&fresh.lease, step.step()).await,
        Err(StoreError::Conflict)
    ));
    assert!(s.execute_knowledge_tool(&step, "sum".into()).await.is_err());
    let new = begin(&s, &fresh.lease, vec![]).await;
    s.mission_save_plan_at(&new, 2, vec!["Only the new accepted plan".into()])
        .await
        .unwrap();
    assert_eq!(counters(&s, &m.id).await.1, 0);
    s.finish_agent_execution(
        &fresh.lease,
        RunState::Cancelled,
        Some(RunFailure::Interrupted),
    )
    .await
    .unwrap();
}
fn update(revision: i64) -> MissionAcceptanceUpdate {
    MissionAcceptanceUpdate {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: revision,
        task: "Compute 2 plus 3".into(),
        constraints: String::new(),
        checks: vec![MissionCheck::DecimalEquals {
            expected: "5".into(),
        }],
    }
}
async fn counters(s: &Store, id: &str) -> (i64, i64, i64, i64, i64) {
    let id = id.to_owned();
    s.call(move|db|Ok(db.conn.query_row("SELECT a.remote_calls,a.tool_calls,m.repairs,a.deadline_ms,m.expires_ms FROM missions m JOIN agent_execution a ON a.tenant_id=m.tenant_id AND a.run_id=m.run_id WHERE m.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?)).await.unwrap()
}
#[tokio::test]
async fn mission_acceptance_revision_replans_fences_old_work_preserves_budget_and_reopens() {
    let _lock = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let old = begin(&s, &claim.lease, vec![calc()]).await;
    s.mission_save_plan(&old, vec!["Original plan".into()])
        .await
        .unwrap();
    s.execute_knowledge_tool(&old, "sum".into()).await.unwrap();
    let final_step = begin(&s, &claim.lease, vec![]).await;
    assert!(
        !s.mission_verify(
            &final_step,
            MissionCandidate {
                text: "Wrong 0.4".into(),
                evidence: vec![]
            }
        )
        .await
        .unwrap()
    );
    let paused = s
        .control_mission(&u, m.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    let before = counters(&s, &m.id).await;
    let change = update(paused.revision);
    let revised = s
        .revise_mission_acceptance(&u, m.id.clone(), change.clone())
        .await
        .unwrap();
    assert_eq!(counters(&s, &m.id).await, before);
    assert_eq!(revised.state, "queued");
    assert!(revised.plan.is_empty() && revised.result.is_none());
    let history = revised.acceptance_history.as_ref().unwrap();
    assert_eq!(history.len(), 2);
    assert_eq!(history[0].request.task, m.request.task);
    assert_eq!(revised.request.task, "Compute 2 plus 3");
    assert!(
        s.mission_save_plan(&old, vec!["Late old plan".into()])
            .await
            .is_err()
    );
    assert_eq!(
        s.revise_mission_acceptance(&u, m.id.clone(), change.clone())
            .await
            .unwrap()
            .revision,
        revised.revision
    );
    let mut conflict = change.clone();
    conflict.task.push('!');
    assert!(matches!(
        s.revise_mission_acceptance(&u, m.id.clone(), conflict)
            .await,
        Err(StoreError::Conflict)
    ));
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(
        s.revise_mission_acceptance(&u, m.id.clone(), change)
            .await
            .unwrap()
            .acceptance_history
            .unwrap()
            .len(),
        2
    );
    let next = s.claim_agent_run().await.unwrap().unwrap();
    assert!(next.context.calls.is_empty() && next.context.checkpoint.is_none());
    assert_eq!(
        s.mission_execution(&next.lease)
            .await
            .unwrap()
            .unwrap()
            .receipts
            .as_array()
            .unwrap()
            .len(),
        0
    );
    let step = begin(
        &s,
        &next.lease,
        vec![PendingKnowledgeCall {
            call_id: "new-sum".into(),
            input: KnowledgeToolInput::DecimalCalculate {
                calculation: DecimalCalculation {
                    operation: DecimalOperation::Sum,
                    operands: vec!["2".into(), "3".into()],
                },
            },
        }],
    )
    .await;
    assert!(matches!(
        s.mission_save_plan_at(&step, 1, vec!["Stale criteria".into()])
            .await,
        Err(StoreError::Conflict)
    ));
    s.mission_save_plan_at(&step, 2, vec!["New accepted plan".into()])
        .await
        .unwrap();
    assert!(
        matches!(s.execute_knowledge_tool(&step,"new-sum".into()).await.unwrap(),KnowledgeToolResult::Decimal{exact}if exact=="5")
    );
    let final_new = begin(&s, &next.lease, vec![]).await;
    let candidate = MissionCandidate {
        text: "5".into(),
        evidence: vec![MissionEvidence {
            step: step.step(),
            call_id: "new-sum".into(),
        }],
    };
    assert!(matches!(
        s.mission_verify_at(&final_new, 1, candidate.clone()).await,
        Err(StoreError::Conflict)
    ));
    assert!(
        !s.mission_verify_at(
            &final_new,
            2,
            MissionCandidate {
                text: "5".into(),
                evidence: vec![MissionEvidence {
                    step: old.step(),
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    assert_eq!(counters(&s, &m.id).await.2, 2);
    let last = begin(&s, &next.lease, vec![]).await;
    assert!(s.mission_verify_at(&last, 2, candidate).await.unwrap());
    let current = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(current.state, "achieved");
    assert_eq!(
        current.acceptance_history.unwrap()[0].request.task,
        m.request.task
    );
    s.delete_mission(&u, m.id.clone()).await.unwrap();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM mission_acceptances WHERE mission_id=?1",
                [m.id],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        assert!(!db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?);
        Ok(())
    })
    .await
    .unwrap();
    let at = now().unwrap() + 60;
    let scheduled = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: input(MissionMode::Durable),
                schedule: ScheduleDraft {
                    title: "Fixed recurring acceptance".into(),
                    message: "Read only".into(),
                    rule: ScheduleRule::Once { at },
                    valid_until: None,
                },
                changes_only: false,
            },
        )
        .await
        .unwrap();
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    let child = s
        .recurring_missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.schedule.id == scheduled.schedule.id)
        .unwrap()
        .recent
        .remove(0);
    let paused = s
        .control_mission(&u, child.mission_id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    assert!(matches!(
        s.revise_mission_acceptance(&u, child.mission_id, update(paused.revision))
            .await,
        Err(StoreError::Forbidden)
    ));
}
#[tokio::test]
async fn mission_acceptance_schema51_and_current_owner_scope_guards() {
    let _lock = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    s.call(|db| {
        db.conn
            .execute_batch("DROP TABLE mission_acceptances;PRAGMA user_version=51;")?;
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    assert!(
        s.revise_mission_acceptance(&u, m.id.clone(), update(1))
            .await
            .is_err()
    );
    let paused = s
        .control_mission(&u, m.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    for task in [
        "Read my Gmail",
        "Read Calendar",
        "Send my Gmail to Sam",
        "Use the browser",
        "Read my vault",
        "Send this result to Slack",
    ] {
        let mut q = update(paused.revision);
        q.task = task.into();
        assert!(
            matches!(
                s.revise_mission_acceptance(&u, m.id.clone(), q).await,
                Err(StoreError::Forbidden)
            ),
            "{task}"
        );
    }
    let foreign = member(&s, &u, "revision-other@example.test").await.user;
    assert!(matches!(
        s.revise_mission_acceptance(&foreign, m.id.clone(), update(paused.revision))
            .await,
        Err(StoreError::NotFound)
    ));
    let other = s
        .create_mission(&foreign, input(MissionMode::Durable))
        .await
        .unwrap();
    let other_paused = s
        .control_mission(&foreign, other.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    s.revise_mission_acceptance(&foreign, other.id, update(other_paused.revision))
        .await
        .unwrap();
    let managed = s
        .managed_accounts(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == foreign.account.id)
        .unwrap();
    let prepared = s
        .prepare_account_management(
            &u,
            foreign.account.id.clone(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: managed.revision,
                action: AccountManagementAction::Delete,
            },
        )
        .await
        .unwrap();
    s.apply_account_management(prepared).await.unwrap();
    let removed = foreign.tenant.0.clone();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM mission_acceptances WHERE tenant_id=?1",
                [removed],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    let mut prose = update(paused.revision);
    prose.constraints = "Check delivery conditions manually".into();
    let out = s
        .revise_mission_acceptance(&u, m.id.clone(), prose)
        .await
        .unwrap();
    assert_eq!(out.acceptance_history.unwrap().len(), 2);
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
            52
        );
        assert!(!db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?);
        Ok(())
    })
    .await
    .unwrap();
}
