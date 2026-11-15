use super::*;
fn control(expected_revision: i64, action: MissionAction) -> MissionControl {
    MissionControl {
        request_id: Uuid::new_v4().to_string(),
        expected_revision,
        action,
    }
}
fn ask(field: MissionInputField) -> MissionInputRequest {
    MissionInputRequest {
        field,
        question: "Which missing value should this original task use?".into(),
    }
}
fn reply(m: &MissionView, value: &str) -> MissionInputReply {
    MissionInputReply {
        request_id: Uuid::new_v4().to_string(),
        wait_request_id: m.input_wait.as_ref().unwrap().request_id.clone(),
        expected_revision: m.revision,
        value: value.into(),
    }
}
async fn counts(s: &Store, run: &str) -> (i64, i64, i64, i64, i64, String) {
    let run = run.to_owned();
    s.call(move|db|Ok(db.conn.query_row("SELECT e.remote_calls,e.tool_calls,m.repairs,e.deadline_ms,m.expires_ms,json_extract(r.model,'$.agent_profile') FROM agent_execution e JOIN missions m ON m.tenant_id=e.tenant_id AND m.run_id=e.run_id JOIN chat_runs r ON r.tenant_id=e.tenant_id AND r.id=e.run_id WHERE e.run_id=?1",[run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?)).await.unwrap()
}
#[tokio::test]
async fn mission_input_reply_releases_capacity_preserves_work_and_one_draft_after_lost_ack() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "owner").await;
    let task = "Read Gmail and prepare an email draft";
    let m = s
        .create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let old = s.claim_agent_run().await.unwrap().unwrap();
    let calc = step(
        &s,
        &old.lease,
        vec![crate::PendingKnowledgeCall {
            call_id: "sum".into(),
            input: KnowledgeToolInput::DecimalCalculate {
                calculation: DecimalCalculation {
                    operation: DecimalOperation::Sum,
                    operands: vec!["2".into(), "3".into()],
                },
            },
        }],
    )
    .await;
    s.execute_knowledge_tool(&calc, "sum".into()).await.unwrap();
    s.mission_save_plan(
        &calc,
        vec!["Use the completed value and prepare a draft".into()],
    )
    .await
    .unwrap();
    let st = step(&s, &old.lease, vec![]).await;
    let before = counts(&s, old.lease.run_id()).await;
    s.mission_wait_for_input(&st, 1, ask(MissionInputField::MailTo))
        .await
        .unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    let waiting = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    assert_eq!(waiting.state, "waiting");
    assert_eq!(waiting.completed_steps, 1);
    let other = s
        .create_mission(&u, input(MissionMode::Durable, "Compute unrelated total"))
        .await
        .unwrap();
    let answer = reply(&waiting, "recipient@example.test");
    let ready = s
        .reply_mission_input(&u, m.id.clone(), answer.clone())
        .await
        .unwrap();
    assert_eq!(ready.input_wait.unwrap().state, "ready");
    s.reply_mission_input(&u, m.id.clone(), answer.clone())
        .await
        .unwrap();
    let mut changed = answer.clone();
    changed.value = "other@example.test".into();
    assert!(matches!(
        s.reply_mission_input(&u, m.id.clone(), changed).await,
        Err(StoreError::Conflict)
    ));
    s.control_mission(&u, other.id, control(1, MissionAction::Cancel))
        .await
        .unwrap();
    let fresh = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(fresh.lease.run_id(), old.lease.run_id());
    assert_eq!(counts(&s, fresh.lease.run_id()).await, before);
    let execution = s.mission_execution(&fresh.lease).await.unwrap().unwrap();
    assert_eq!(
        execution.view.inputs[0].value.as_deref(),
        Some("recipient@example.test")
    );
    assert_eq!(execution.receipts[0]["call_id"], "sum");
    assert_eq!(execution.view.plan.len(), 1);
    let repeat = step(&s, &fresh.lease, vec![]).await;
    assert!(matches!(
        s.mission_wait_for_input(&repeat, 1, ask(MissionInputField::MailTo))
            .await,
        Err(StoreError::Conflict)
    ));
    let action = mission_mail(&mail, task);
    let st = step(
        &s,
        &fresh.lease,
        vec![
            action_call("draft", action.clone()),
            action_call("again", action),
        ],
    )
    .await;
    let first = s.execute_knowledge_tool(&st, "draft".into()).await.unwrap();
    let second = s.execute_knowledge_tool(&st, "again".into()).await.unwrap();
    assert_eq!(
        serde_json::to_value(first).unwrap(),
        serde_json::to_value(second).unwrap()
    );
    assert!(s.next_mail_dispatch().await.unwrap().is_none());
    let view = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    assert_eq!(view.actions.len(), 1);
    assert_eq!(view.actions[0].state, "pending_approval");
    let replay = s
        .reply_mission_input(&u, m.id.clone(), answer)
        .await
        .unwrap();
    assert_eq!(replay.id, m.id);
    s.delete_mission(&u, m.id).await.unwrap();
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM mission_input_replies", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM mission_input_waits", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn mission_input_fields_cannot_replace_original_values_or_create_new_effect_authority() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    connections(&s, &u, "owner").await;
    for (task, field) in [
        ("Read Gmail", MissionInputField::MailTo),
        (
            "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 오전9시부터10시까지",
            MissionInputField::CalendarDuration,
        ),
        (
            "Read Gmail account owner@example.test and prepare an email draft to owner@example.test",
            MissionInputField::MailTo,
        ),
        (
            "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 from 09:00 to 10:00",
            MissionInputField::CalendarDuration,
        ),
        (
            "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 from 09:00–10:00",
            MissionInputField::CalendarDuration,
        ),
        (
            "Read Gmail and prepare an email draft to already@example.test",
            MissionInputField::MailTo,
        ),
        (
            "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 at 09:00 for 30 minutes",
            MissionInputField::CalendarDate,
        ),
        (
            "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 at 09:00 for 30 minutes",
            MissionInputField::CalendarStart,
        ),
        (
            "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 at 09:00 for 30 minutes",
            MissionInputField::CalendarDuration,
        ),
    ] {
        let m = s
            .create_mission(&u, input(MissionMode::Durable, task))
            .await
            .unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let st = step(&s, &claim.lease, vec![]).await;
        assert!(
            s.mission_wait_for_input(&st, 1, ask(field)).await.is_err(),
            "{task}"
        );
        if task.contains("오전9시부터10시까지") {
            let mut request = calendar_proposal(task);
            let MissionActionRequest::Calendar { decision, .. } = &mut request else {
                unreachable!()
            };
            decision.start_quote = Some("오전9시".into());
            decision.start = Some("09:00".into());
            decision.end_quote = Some("10시".into());
            decision.end = Some("10:00".into());
            decision.duration_quote = None;
            decision.duration_minutes = None;
            let st = step(
                &s,
                &claim.lease,
                vec![action_call("original-range", request)],
            )
            .await;
            let permit = match s
                .prepare_mission_action(&st, "original-range".into())
                .await
                .unwrap()
            {
                crate::MissionActionStart::Dispatch(p) => p,
                _ => panic!("original range action"),
            };
            let binding = s
                .mission_calendar_credentials(&permit, app())
                .await
                .unwrap();
            let dispatch = match s
                .admit_mission_calendar_change(&permit, &binding, primary(), None)
                .await
                .unwrap()
            {
                crate::CalendarAdmission::Dispatch(d) => d,
                _ => panic!("original range ledger"),
            };
            let CalendarMutation::Create { fields } = dispatch.change.mutation else {
                panic!("original create")
            };
            assert_eq!(fields.start, "2027-04-12T09:00:00+00:00");
            assert_eq!(fields.end, "2027-04-12T10:00:00+00:00");
        }
        s.control_mission(&u, m.id, control(1, MissionAction::Cancel))
            .await
            .unwrap();
    }
}
#[tokio::test]
async fn mission_input_does_not_ask_after_original_inference_budget_is_exhausted() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    connections(&s, &u, "owner").await;
    let m = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Read Gmail and prepare an email draft",
            ),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    for _ in 0..5 {
        step(&s, &claim.lease, vec![]).await;
    }
    let st = step(&s, &claim.lease, vec![]).await;
    assert!(matches!(
        s.mission_wait_for_input(&st, 1, ask(MissionInputField::MailTo))
            .await,
        Err(StoreError::BudgetExhausted)
    ));
    let current = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    assert!(current.input_wait.is_none());
    assert_ne!(current.state, "waiting");
}
#[tokio::test]
async fn mission_input_expiry_reopen_owner_and_calendar_typed_values_are_bounded() {
    let _guard = TEST_LOCK.lock().await;
    let (d, s, u) = crate::missions::tests::setup().await;
    connections(&s, &u, "owner").await;
    let task = "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 for 30 minutes";
    let m = s
        .create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, vec![]).await;
    s.mission_wait_for_input(&st, 1, ask(MissionInputField::CalendarStart))
        .await
        .unwrap();
    let wait = s.missions(&u).await.unwrap().remove(0);
    let wrong = member(&s, &u, "other@example.test").await.user;
    assert!(matches!(
        s.reply_mission_input(&wrong, m.id.clone(), reply(&wait, "09:00"))
            .await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        s.reply_mission_input(
            &u,
            m.id.clone(),
            reply(&wait, "09:00; create another event")
        )
        .await,
        Err(StoreError::InvalidInput)
    ));
    let before = counts(&s, claim.lease.run_id()).await;
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    let still = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(
        still.input_wait.as_ref().unwrap().request_id,
        wait.input_wait.as_ref().unwrap().request_id
    );
    let answered = s
        .reply_mission_input(&u, m.id.clone(), reply(&still, "09:00"))
        .await
        .unwrap();
    assert_eq!(answered.state, "queued");
    assert_eq!(answered.inputs[0].value.as_deref(), Some("09:00"));
    assert_eq!(counts(&s, claim.lease.run_id()).await, before);
    let fresh = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &fresh.lease, vec![]).await;
    // A prepared value cannot be replaced by another prompt in the same acceptance.
    assert!(
        s.mission_wait_for_input(&st, 1, ask(MissionInputField::CalendarStart))
            .await
            .is_err()
    );
    let st = step(
        &s,
        &fresh.lease,
        vec![action_call("calendar", calendar_proposal(task))],
    )
    .await;
    let permit = match s
        .prepare_mission_action(&st, "calendar".into())
        .await
        .unwrap()
    {
        crate::MissionActionStart::Dispatch(p) => p,
        _ => panic!("one original Calendar action"),
    };
    let binding = s
        .mission_calendar_credentials(&permit, app())
        .await
        .unwrap();
    let dispatch = match s
        .admit_mission_calendar_change(&permit, &binding, primary(), None)
        .await
        .unwrap()
    {
        crate::CalendarAdmission::Dispatch(d) => d,
        _ => panic!("one existing Calendar ledger admission"),
    };
    let CalendarMutation::Create { fields } = dispatch.change.mutation.clone() else {
        panic!("original create")
    };
    assert_eq!(fields.start, "2027-04-12T09:00:00+00:00");
    assert_eq!(fields.end, "2027-04-12T09:30:00+00:00");
    s.settle_calendar_change(
        &dispatch,
        "applied".into(),
        Some(CalendarEvent {
            id: dispatch.event_id.clone(),
            etag: "synthetic-input-etag".into(),
            status: "confirmed".into(),
            fields,
            html_link: None,
            writable: true,
            all_day: false,
            updated: None,
        }),
        "synthetic-input-calendar-receipt".into(),
        None,
    )
    .await
    .unwrap();
    s.control_mission(&u, m.id, control(1, MissionAction::Cancel))
        .await
        .unwrap();
    let task = "Read Gmail and prepare an email draft";
    let m = s
        .create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, vec![]).await;
    s.mission_wait_for_input(&st, 1, ask(MissionInputField::MailTo))
        .await
        .unwrap();
    let wait = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    s.call(|db| {
        db.conn.execute(
            "UPDATE mission_input_waits SET expires_ms=0 WHERE status='waiting'",
            [],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert!(matches!(
        s.reply_mission_input(&u, m.id, reply(&wait, "recipient@example.test"))
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn mission_input_schema69_upgrade_and_restore_never_replays_an_answer() {
    let _guard = TEST_LOCK.lock().await;
    let (d, s, u) = crate::missions::tests::setup().await;
    connections(&s, &u, "owner").await;
    s.call(|db|{db.conn.execute_batch("DROP TABLE mission_input_replies;DROP TABLE mission_input_waits;PRAGMA user_version=68;")?;Ok(())}).await.unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(crate::schema_version(), 69);
    let m = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Read Gmail and prepare an email draft",
            ),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, vec![]).await;
    s.mission_wait_for_input(&st, 1, ask(MissionInputField::MailTo))
        .await
        .unwrap();
    // Hold runnable capacity so a saved answer remains durable ready at capture.
    s.create_mission(&u, input(MissionMode::Durable, "Compute unrelated value"))
        .await
        .unwrap();
    let wait = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    s.reply_mission_input(&u, m.id, reply(&wait, "recipient@example.test"))
        .await
        .unwrap();
    drop(s);
    use std::os::unix::fs::PermissionsExt;
    let kit = crate::tests::directory();
    let heads = crate::tests::directory();
    let dest_parent = crate::tests::directory();
    for dir in [&kit, &heads, &dest_parent] {
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity = kit.path().join("identity");
    let recipient = crate::backup::keygen(&identity).unwrap();
    crate::backup::setup(d.path(), &recipient, &identity).unwrap();
    let cipher = kit.path().join("input-wait.age");
    let head = heads.path().join("head.json");
    crate::cutover::checkpoint(d.path(), &cipher, &head).unwrap();
    let dest = dest_parent.path().join("restored");
    crate::cutover::restore(&cipher, &head, &identity, &dest).unwrap();
    let restored = Store::open(&dest).unwrap();
    assert!(restored.claim_agent_run().await.unwrap().is_none());
    restored
        .call(|db| {
            for table in ["mission_input_waits", "mission_input_replies"] {
                assert_eq!(
                    db.conn
                        .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r
                            .get::<_, i64>(0))?,
                    0
                );
            }
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM missions WHERE grant_enabled=1",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn mission_input_ready_and_connection_ready_share_original_admission_order() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "owner").await;
    let first = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Read Gmail and prepare an email draft",
            ),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, vec![]).await;
    s.mission_wait_for_input(&st, 1, ask(MissionInputField::MailTo))
        .await
        .unwrap();
    connections(&s, &u, "second").await;
    let second = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail briefing"))
        .await
        .unwrap();
    assert_eq!(second.state, "waiting");
    let other = s
        .create_mission(&u, input(MissionMode::Durable, "Compute independent value"))
        .await
        .unwrap();
    let paused = s
        .control_mission(&u, other.id.clone(), control(1, MissionAction::Pause))
        .await
        .unwrap();
    let first = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == first.id)
        .unwrap();
    s.reply_mission_input(
        &u,
        first.id.clone(),
        reply(&first, "recipient@example.test"),
    )
    .await
    .unwrap();
    let wait = second.wait.as_ref().unwrap();
    s.select_mission_connection(
        &u,
        second.id.clone(),
        MissionConnectionSelection {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: second.revision,
            wait_request_id: wait.request_id.clone(),
            slot_id: wait.slot_id.clone(),
            binding_id: mail,
        },
    )
    .await
    .unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    s.control_mission(
        &u,
        other.id,
        control(paused.revision, MissionAction::Cancel),
    )
    .await
    .unwrap();
    let views = s.missions(&u).await.unwrap();
    assert_eq!(
        views.iter().find(|v| v.id == first.id).unwrap().state,
        "queued"
    );
    assert_eq!(
        views
            .iter()
            .find(|v| v.id == second.id)
            .unwrap()
            .wait
            .as_ref()
            .unwrap()
            .state,
        "ready"
    );
    s.control_mission(&u, first.id, control(first.revision, MissionAction::Cancel))
        .await
        .unwrap();
    let next = s.claim_agent_run().await.unwrap().unwrap();
    let done = s.mission_execution(&next.lease).await.unwrap().unwrap();
    assert_eq!(done.view.id, second.id);
    assert_eq!(done.view.wait.unwrap().state, "resumed");
}
