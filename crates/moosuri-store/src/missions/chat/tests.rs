use super::*;
use crate::missions::tests::setup;
use crate::tests::TEST_LOCK;
use crate::{PrivateCheckpoint, RunLease};
fn decision(text: &str, scheduled: bool) -> MissionChatDecision {
    MissionChatDecision {
        decision: "command".into(),
        action: Some(
            if scheduled {
                "create_scheduled"
            } else {
                "create_now"
            }
            .into(),
        ),
        target_kind: None,
        intent_quote: Some(text.into()),
        task_quote: Some(text.into()),
        constraints_quote: None,
        time_quote: scheduled.then(|| "매일 아침9시".into()),
        target_quote: None,
        instruction_quote: None,
        time: scheduled.then(|| MissionChatTime::Daily {
            time: "09:00".into(),
        }),
        clarification: None,
    }
}
async fn source(
    s: &Store,
    u: &UserContext,
    text: &str,
    target: Option<MissionChatTarget>,
) -> (String, RunLease) {
    let thread = s
        .create_thread(u, "Synthetic mission command".into())
        .await
        .unwrap();
    let request = Uuid::new_v4().to_string();
    s.submit_turn_with_mission_context(
        u,
        thread.id.clone(),
        request,
        text.into(),
        None,
        vec![],
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
        target.map(|target| MissionChatContext {
            target: Some(target),
        }),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    (thread.id, claim.lease)
}
async fn commit(s: &Store, l: &RunLease, d: MissionChatDecision) -> crate::StepLease {
    let ctx = s.agent_context(l).await.unwrap();
    let step = s.begin_agent_step(l, ctx.checkpoint_seq).await.unwrap();
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
    step
}
#[tokio::test]
async fn semantic_plan_receipt_and_real_mission_are_atomic_and_exact_retry_never_reclassifies() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let text = "매일 아침9시에 공개 뉴스 찾아서 요약해 줘";
    let (thread, l) = source(&s, &u, text, None).await;
    // Failed acknowledgement cannot leave a typed command without its completed model step.
    let ctx = s.agent_context(&l).await.unwrap();
    let step = s.begin_agent_step(&l, ctx.checkpoint_seq).await.unwrap();
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_plan BEFORE UPDATE OF decision ON mission_chat_plans BEGIN SELECT RAISE(ABORT,'synthetic plan fault');END;")?;Ok(())}).await.unwrap();
    let checkpoint =
        serde_json::json!({"kind":"mission_chat_decision","decision":decision(text,true)})
            .to_string()
            .into_bytes();
    assert!(
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: checkpoint.clone()
            },
            vec![]
        )
        .await
        .is_err()
    );
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM agent_steps", [], |r| r
                    .get::<_, String>(0))?,
            "dispatched"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM mission_chat_plans", [], |r| r
                    .get::<_, String>(0))?,
            "awaiting"
        );
        db.conn.execute_batch("DROP TRIGGER fail_plan;")?;
        Ok(())
    })
    .await
    .unwrap();
    s.commit_agent_step(&step, PrivateCheckpoint { bytes: checkpoint }, vec![])
        .await
        .unwrap();
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_receipt BEFORE INSERT ON mission_chat_commands BEGIN SELECT RAISE(ABORT,'synthetic command fault');END;")?;Ok(())}).await.unwrap();
    assert!(s.apply_mission_chat(&l).await.is_err());
    assert!(s.recurring_missions(&u).await.unwrap().is_empty());
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM mission_chat_plans", [], |r| r
                    .get::<_, String>(0))?,
            "decided"
        );
        db.conn.execute_batch("DROP TRIGGER fail_receipt;")?;
        Ok(())
    })
    .await
    .unwrap();
    assert_eq!(s.apply_mission_chat(&l).await.unwrap(), Some(true));
    let parent = s.recurring_missions(&u).await.unwrap().remove(0);
    assert!(s.schedules(&u).await.unwrap().is_empty());
    assert_eq!(parent.model.model, "synthetic");
    let chat = s
        .conversation(&u, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    let receipt = chat.run.mission_receipt.unwrap();
    assert_eq!(receipt.target.unwrap().id, parent.schedule.id);
    assert!(chat.assistant.iter().all(|v| !v.text.contains("뉴스")));
    let replay = s
        .submit_turn_with_mission_context(
            &u,
            thread.clone(),
            chat.run.request_id.clone(),
            text.into(),
            None,
            vec![],
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
            None,
        )
        .await
        .unwrap();
    assert_eq!(replay.run.id, chat.run.id);
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
    assert!(matches!(
        s.submit_turn_with_mission_context(
            &u,
            thread,
            chat.run.request_id,
            text.into(),
            None,
            vec![],
            Some(ScheduleChatContext {
                timezone: "UTC".into(),
                target: None
            }),
            None
        )
        .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn semantic_no_command_clarification_and_selected_control_preserve_real_identity() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let text = "매일 아침9시에 공개 소식 요약해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, true)).await;
    s.apply_mission_chat(&l).await.unwrap();
    let parent = s.recurring_missions(&u).await.unwrap().remove(0);
    let text = "그 업무 잠깐 멈춰 줘";
    let target = MissionChatTarget {
        kind: "recurring".into(),
        id: parent.schedule.id.clone(),
        revision: 1,
    };
    let (_, control) = source(&s, &u, text, Some(target)).await;
    let mut d = decision(text, false);
    d.action = Some("pause".into());
    d.task_quote = None;
    d.target_kind = Some("selected".into());
    commit(&s, &control, d).await;
    s.apply_mission_chat(&control).await.unwrap();
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "paused"
    );
    let text = "Could you explain what a weekly summary is?";
    let (_, ordinary) = source(&s, &u, text, None).await;
    let mut d = decision(text, false);
    d.decision = "no_command".into();
    d.action = None;
    d.intent_quote = None;
    d.task_quote = None;
    commit(&s, &ordinary, d).await;
    assert_eq!(s.apply_mission_chat(&ordinary).await.unwrap(), Some(false));
    assert_eq!(s.apply_mission_chat(&ordinary).await.unwrap(), Some(false));
    assert!(s.mission_chat_planning(&ordinary).await.unwrap().is_none());
}
#[tokio::test]
async fn semantic_current_session_epoch_quote_scope_and_direct_api_pins_are_rechecked() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let text = "시간 걸려도 이 일 맡아서 공개 자료 비교해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    let mut invalid = decision(text, false);
    invalid.task_quote = Some("model invented a different task".into());
    let ctx = s.agent_context(&l).await.unwrap();
    let step = s.begin_agent_step(&l, ctx.checkpoint_seq).await.unwrap();
    assert!(
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: serde_json::json!({"kind":"mission_chat_decision","decision":invalid})
                    .to_string()
                    .into_bytes()
            },
            vec![]
        )
        .await
        .is_err()
    );
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes:
                serde_json::json!({"kind":"mission_chat_decision","decision":decision(text,false)})
                    .to_string()
                    .into_bytes(),
        },
        vec![],
    )
    .await
    .unwrap();
    s.logout(&u).await.unwrap();
    assert!(s.apply_mission_chat(&l).await.is_err());
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM missions", [], |r| r.get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM mission_chat_commands", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn acknowledged_semantic_decision_survives_restart_and_default_change_without_replanning() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u) = setup().await;
    let text = "시간 걸려도 공개 자료 비교하는 일 맡아서 해 줘";
    let (thread, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, false)).await;
    let run = l.run_id().to_owned();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let before = s
        .conversation(&u, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(before.run.state, RunState::OutcomeUnknown);
    s.retry_run(&u, run, 1).await.unwrap();
    s.set_default_model(&u, None).await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(
        s.apply_mission_chat(&claim.lease).await.unwrap(),
        Some(true)
    );
    let m = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(m.state, "queued");
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}
#[test]
fn civil_proposals_cannot_change_explicit_clock_or_day_and_missing_details_do_not_get_defaults() {
    let text = "매일 아침9시에 공개 뉴스 요약해 줘";
    let mut d = decision(text, true);
    assert!(civil_consistent(&d, 1_800_000_000, "Asia/Seoul"));
    d.time = Some(MissionChatTime::Daily {
        time: "18:00".into(),
    });
    assert!(!civil_consistent(&d, 1_800_000_000, "Asia/Seoul"));
    d.time_quote = Some("every Monday at nine in the morning".into());
    d.time = Some(MissionChatTime::Weekly {
        days: vec![2],
        time: "09:00".into(),
    });
    assert!(!civil_consistent(&d, 1_800_000_000, "Asia/Seoul"));
    d.time = Some(MissionChatTime::Weekly {
        days: vec![1],
        time: "09:00".into(),
    });
    assert!(civil_consistent(&d, 1_800_000_000, "Asia/Seoul"));
    d.time_quote = Some("every Monday".into());
    assert!(!civil_consistent(&d, 1_800_000_000, "Asia/Seoul"));
}

#[tokio::test]
async fn absolute_date_and_clock_keep_their_boundary_and_exact_utc_receipt() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let date = chrono::Utc::now().date_naive().succ_opt().unwrap();
    let quote = format!("{date} 23:15 UTC");
    let text = format!("{quote} 공개 항만 공지를 읽는 일을 맡길게요.");
    let (_, lease) = source(&s, &u, &text, None).await;
    let mut d = decision(&text, true);
    d.time_quote = Some(quote.clone());
    d.time = Some(MissionChatTime::LocalOnce {
        date: date.to_string(),
        time: "23:15".into(),
    });
    commit(&s, &lease, d.clone()).await;
    s.apply_mission_chat(&lease).await.unwrap();
    assert_eq!(
        s.mission_chat_receipt_for_test(&lease).await.state,
        "active"
    );
    let rows = s.recurring_missions(&u).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].schedule.rule,
        ScheduleRule::Once {
            at: date.and_hms_opt(23, 15, 0).unwrap().and_utc().timestamp(),
        }
    );
    let anchor = chrono::Utc::now().timestamp();
    d.time = Some(MissionChatTime::LocalOnce {
        date: date.to_string(),
        time: "23:00".into(),
    });
    assert!(!civil_consistent(&d, anchor, "UTC"));
    d.time = Some(MissionChatTime::LocalOnce {
        date: date.succ_opt().unwrap().to_string(),
        time: "23:15".into(),
    });
    assert!(!civil_consistent(&d, anchor, "UTC"));
    d.time = Some(MissionChatTime::Daily {
        time: "23:15".into(),
    });
    assert!(!civil_consistent(&d, anchor, "UTC"));
    d.time_quote = Some(format!("{date} at noon UTC"));
    d.time = Some(MissionChatTime::LocalOnce {
        date: date.to_string(),
        time: "12:00".into(),
    });
    assert!(civil_consistent(&d, anchor, "UTC"));
}

#[tokio::test]
async fn missing_name_cannot_select_the_only_other_owned_task() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let title = "매일 아침9시에 공개 항만 소식을 정리해 줘";
    let (_, lease) = source(&s, &u, title, None).await;
    commit(&s, &lease, decision(title, true)).await;
    s.apply_mission_chat(&lease).await.unwrap();
    for text in [
        "Please pause the Unrelated Digest task",
        "Please pause the task",
    ] {
        let (_, lease) = source(&s, &u, text, None).await;
        let mut d = decision(text, false);
        d.action = Some("pause".into());
        d.task_quote = None;
        d.target_kind = Some("recurring".into());
        commit(&s, &lease, d).await;
        s.apply_mission_chat(&lease).await.unwrap();
        let receipt = s.mission_chat_receipt_for_test(&lease).await;
        assert_eq!(receipt.state, "clarify");
        assert_eq!(receipt.candidates.len(), 1);
        assert_eq!(
            s.recurring_missions(&u).await.unwrap()[0].schedule.state,
            "active"
        );
    }
    let text = format!("Please pause {title}");
    let (_, lease) = source(&s, &u, &text, None).await;
    let mut d = decision(&text, false);
    d.action = Some("pause".into());
    d.task_quote = None;
    d.target_kind = Some("recurring".into());
    d.target_quote = Some(title.into());
    commit(&s, &lease, d).await;
    s.apply_mission_chat(&lease).await.unwrap();
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "paused"
    );
}
#[tokio::test]
async fn mission_chat_schema29_rolls_back_late_failure_without_recreating_prior_authority() {
    let _g = TEST_LOCK.lock().await;
    let d = crate::tests::directory();
    drop(Store::open(d.path()).unwrap());
    let c = Connection::open(d.path().join("assistant.sqlite")).unwrap();
    c.execute_batch("DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;DROP TABLE mission_chat_commands;DROP TABLE mission_chat_plans;CREATE TABLE mission_chat_commands(conflict INTEGER);PRAGMA user_version=28;").unwrap();
    drop(c);
    assert!(Store::open(d.path()).is_err());
    let c = Connection::open(d.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        28
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name='mission_chat_plans'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    c.execute_batch("DROP TABLE mission_chat_commands;")
        .unwrap();
    drop(c);
    drop(Store::open(d.path()).unwrap());
}

#[tokio::test]
async fn semantic_plan_is_not_an_account_or_cross_owner_capability_and_ambiguous_target_has_actual_cards()
 {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let other = crate::tests::member(&s, &u, "semantic-other@example.test")
        .await
        .user;
    let text = "매일 아침9시에 공개 뉴스 요약해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, true)).await;
    s.apply_mission_chat(&l).await.unwrap();
    let parent = s.recurring_missions(&u).await.unwrap().remove(0);
    let text = "그 업무 멈춰줘";
    let target = MissionChatTarget {
        kind: "recurring".into(),
        id: parent.schedule.id.clone(),
        revision: 1,
    };
    let (_, foreign) = source(&s, &other, text, Some(target)).await;
    let mut d = decision(text, false);
    d.action = Some("pause".into());
    d.task_quote = None;
    d.target_kind = Some("selected".into());
    commit(&s, &foreign, d).await;
    assert_eq!(s.apply_mission_chat(&foreign).await.unwrap(), Some(true));
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "active"
    );
    assert!(s.recurring_missions(&other).await.unwrap().is_empty());
    let text = "매일 아침9시에 공개 논문 소식 요약해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, true)).await;
    s.apply_mission_chat(&l).await.unwrap();
    let text = "반복 업무 멈춰 줘";
    let (thread, l) = source(&s, &u, text, None).await;
    let mut d = decision(text, false);
    d.action = Some("pause".into());
    d.task_quote = None;
    d.target_kind = Some("recurring".into());
    commit(&s, &l, d).await;
    s.apply_mission_chat(&l).await.unwrap();
    let turn = s
        .conversation(&u, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    let receipt = turn.run.mission_receipt.unwrap();
    assert_eq!(receipt.state, "clarify");
    assert_eq!(receipt.candidates.len(), 2);
    assert!(receipt.candidates.iter().all(|c| !c.title.is_empty()));
    assert!(
        s.recurring_missions(&u)
            .await
            .unwrap()
            .iter()
            .all(|v| v.schedule.state == "active")
    );
}
#[tokio::test]
async fn encrypted_restore_keeps_semantic_receipts_but_unapplied_plans_are_quarantined() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u) = setup().await;
    let text = "매일 아침9시에 공개 소식 요약해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, true)).await;
    s.apply_mission_chat(&l).await.unwrap();
    let text = "시간 걸려도 공개 자료 조사하는 일 맡아서 해 줘";
    let (_, pending) = source(&s, &u, text, None).await;
    commit(&s, &pending, decision(text, false)).await;
    drop(s);
    let kit = crate::tests::directory();
    let heads = crate::tests::directory();
    let dest_parent = crate::tests::directory();
    for d in [&kit, &heads, &dest_parent] {
        fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity = kit.path().join("identity");
    let recipient = crate::backup::keygen(&identity).unwrap();
    crate::backup::setup(dir.path(), &recipient, &identity).unwrap();
    let cipher = kit.path().join("semantic.age");
    let head = heads.path().join("head.json");
    crate::cutover::checkpoint(dir.path(), &cipher, &head).unwrap();
    let dest = dest_parent.path().join("restored");
    crate::cutover::restore(&cipher, &head, &identity, &dest).unwrap();
    let restored = Store::open(&dest).unwrap();
    assert!(restored.claim_agent_run().await.unwrap().is_none());
    restored
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM mission_chat_commands", [], |r| r
                        .get::<_, i64>(0))?,
                1
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM mission_chat_plans WHERE state='quarantined'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT state FROM reminder_schedules", [], |r| r
                        .get::<_, String>(0))?,
                "paused"
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM missions", [], |r| r.get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
}
#[tokio::test]
async fn unknown_classifier_dispatch_cannot_reclassify_or_create_after_explicit_retry() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u) = setup().await;
    let text = "매일 아침9시에 공개 소식 요약해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    let ctx = s.agent_context(&l).await.unwrap();
    let _step = s.begin_agent_step(&l, ctx.checkpoint_seq).await.unwrap();
    let id = l.run_id().to_string();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    s.retry_run(&u, id, 1).await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    assert!(s.mission_chat_planning(&claim.lease).await.is_err());
    assert!(s.recurring_missions(&u).await.unwrap().is_empty());
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}

#[tokio::test]
async fn independent_session_revoked_plan_cannot_be_rebound_by_retry() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let text = "Keep working in the background on a comparison of public port notices.";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, false)).await;
    s.logout(&u).await.unwrap();
    assert!(s.apply_mission_chat(&l).await.is_err());
    let fresh = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    let result = s.retry_run(&fresh, l.run_id().into(), 1).await;
    assert!(matches!(result, Err(StoreError::Unauthorized)));
    let actual = s.missions(&fresh).await.unwrap();
    println!(
        "REBOUND_PLAN retry_refused={} missions={}",
        result.is_err(),
        actual.len()
    );
    assert!(
        actual.is_empty(),
        "Revoked original source session must not be replaced with the later retry login to mint the old durable grant"
    );
}
#[tokio::test]
async fn independent_civil_proposals_reject_wrong_minute_extra_day_and_wrong_date() {
    let _g = TEST_LOCK.lock().await;
    let mut bad = vec![];
    for (label, text, time_quote, time) in [
        (
            "minute",
            "Every Monday at 9:45 AM summarize public port notices.",
            "Every Monday at 9:45 AM",
            MissionChatTime::Weekly {
                days: vec![1],
                time: "09:00".into(),
            },
        ),
        (
            "extra_day",
            "Every Monday at 9 AM summarize public port notices.",
            "Every Monday at 9 AM",
            MissionChatTime::Weekly {
                days: vec![1, 2],
                time: "09:00".into(),
            },
        ),
        (
            "date",
            "Tomorrow at 9 AM summarize public port notices.",
            "Tomorrow at 9 AM",
            MissionChatTime::LocalOnce {
                date: "2099-01-01".into(),
                time: "09:00".into(),
            },
        ),
    ] {
        let (_dir, s, u) = setup().await;
        let (_, l) = source(&s, &u, text, None).await;
        let mut d = decision(text, true);
        d.time_quote = Some(time_quote.into());
        d.time = Some(time);
        commit(&s, &l, d).await;
        s.apply_mission_chat(&l).await.unwrap();
        let parents = s.recurring_missions(&u).await.unwrap();
        println!("CIVIL {label} admitted={}", parents.len());
        if let Some(parent) = parents.first() {
            println!(
                "ACTUAL_RULE {}",
                serde_json::to_string(&parent.schedule.rule).unwrap()
            );
            bad.push(label);
        }
    }
    assert!(
        bad.is_empty(),
        "Explicit source time contradictions admitted: {bad:?}"
    );
}
#[tokio::test]
async fn independent_selected_target_cannot_override_different_explicit_owned_name() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let mut parents = vec![];
    for text in [
        "매일 아침9시에 항만 소식 정리해 줘",
        "매일 아침9시에 철도 소식 정리해 줘",
    ] {
        let (_, l) = source(&s, &u, text, None).await;
        commit(&s, &l, decision(text, true)).await;
        s.apply_mission_chat(&l).await.unwrap();
        parents.push(
            s.recurring_missions(&u)
                .await
                .unwrap()
                .into_iter()
                .find(|p| p.template.task == text)
                .unwrap(),
        );
    }
    let text = "Please pause 매일 아침9시에 철도 소식 정리해 줘";
    let selected = MissionChatTarget {
        kind: "recurring".into(),
        id: parents[0].schedule.id.clone(),
        revision: 1,
    };
    let (_, l) = source(&s, &u, text, Some(selected)).await;
    let mut d = decision(text, false);
    d.action = Some("pause".into());
    d.task_quote = None;
    d.target_kind = Some("recurring".into());
    d.target_quote = Some(parents[1].schedule.title.clone());
    commit(&s, &l, d).await;
    s.apply_mission_chat(&l).await.unwrap();
    let rows = s.recurring_missions(&u).await.unwrap();
    let old = rows
        .iter()
        .find(|p| p.schedule.id == parents[0].schedule.id)
        .unwrap();
    let named = rows
        .iter()
        .find(|p| p.schedule.id == parents[1].schedule.id)
        .unwrap();
    println!(
        "SELECTED_VS_NAMED selected={},named={}",
        old.schedule.state, named.schedule.state
    );
    assert_eq!(
        old.schedule.state, "active",
        "Explicit different name cannot silently pause old selected identity"
    );
}

#[tokio::test]
async fn independent_literal_precedence_and_failed_confirmation_preserve_atomic_boundaries() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let thread = s
        .create_thread(&u, "synthetic evaluator".into())
        .await
        .unwrap();
    let turn = s
        .submit_turn_with_mission_context(
            &u,
            thread.id,
            Uuid::new_v4().to_string(),
            "내일 오전 10시에 공개 공지를 조사해서 팀원에게 메일을 보내라고 알려줘.".into(),
            None,
            vec![],
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
            None,
        )
        .await
        .unwrap();
    assert_eq!(turn.run.state, RunState::Completed);
    assert_eq!(s.schedules(&u).await.unwrap().len(), 1);
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM mission_chat_plans", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM model_call_receipts", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM missions", [], |r| r.get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    let text = "Take this on in the background: compare public ferry bulletins.";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, false)).await;
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_semantic_confirmation BEFORE INSERT ON chat_output BEGIN SELECT RAISE(ABORT,'synthetic confirmation failure');END;")?;Ok(())}).await.unwrap();
    assert!(s.apply_mission_chat(&l).await.is_err());
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM missions", [], |r| r.get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM mission_chat_commands", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM mission_chat_plans", [], |r| r
                    .get::<_, String>(0))?,
            "decided"
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn independent_control_savepoint_keeps_target_budget_guard_transition() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let m = s
        .create_mission(&u, crate::missions::tests::input(MissionMode::Durable))
        .await
        .unwrap();
    let child = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&child.lease).await.unwrap();
    let step = s
        .begin_agent_step(&child.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"v":1}"#.to_vec(),
        },
        vec![],
    )
    .await
    .unwrap();
    s.control_mission(
        &u,
        m.id.clone(),
        MissionControl {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: 1,
            action: MissionAction::Pause,
        },
    )
    .await
    .unwrap();
    let child_run = child.lease.run_id().to_string();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE agent_execution SET deadline_ms=1 WHERE run_id=?1",
            [child_run],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let text = "Resume the selected task, please.";
    let (_, l) = source(
        &s,
        &u,
        text,
        Some(MissionChatTarget {
            kind: "one_off".into(),
            id: m.id.clone(),
            revision: 2,
        }),
    )
    .await;
    let mut d = decision(text, false);
    d.task_quote = None;
    d.action = Some("resume".into());
    d.target_kind = Some("selected".into());
    commit(&s, &l, d).await;
    s.apply_mission_chat(&l).await.unwrap();
    let target = m.id;
    let persisted=s.call(move|db|{Ok(db.conn.query_row("SELECT m.state,r.state FROM missions m JOIN chat_runs r ON r.id=m.run_id WHERE m.id=?1",[target],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?)}).await.unwrap();
    println!("TARGET_BUDGET_AFTER_SAVEPOINT {persisted:?}");
    assert_eq!(
        persisted.1, "incomplete",
        "Guard-only budget transition on the actual target must survive semantic action rollback"
    );
}

#[test]
fn civil_components_preserve_minutes_meridiem_dates_and_supported_zones() {
    // 2026-11-13 23:30 UTC is already Nov14 in Seoul. Civil dates use the frozen zone.
    let anchor = chrono::DateTime::parse_from_rfc3339("2026-11-13T23:30:00Z")
        .unwrap()
        .timestamp();
    for (quote, time) in [
        (
            "Every Monday at 9:45 AM",
            MissionChatTime::Weekly {
                days: vec![1],
                time: "09:45".into(),
            },
        ),
        (
            "매주 월요일 오전 아홉 시 45분",
            MissionChatTime::Weekly {
                days: vec![1],
                time: "09:45".into(),
            },
        ),
        (
            "매일 오후 아홉 시 십오 분",
            MissionChatTime::Daily {
                time: "21:15".into(),
            },
        ),
        (
            "Every day at half past nine in the morning",
            MissionChatTime::Daily {
                time: "09:30".into(),
            },
        ),
        (
            "매일 오후 열두 시 반",
            MissionChatTime::Daily {
                time: "12:30".into(),
            },
        ),
        (
            "every day at 12 AM",
            MissionChatTime::Daily {
                time: "00:00".into(),
            },
        ),
        (
            "every day at 12 PM",
            MissionChatTime::Daily {
                time: "12:00".into(),
            },
        ),
        (
            "every day at midnight",
            MissionChatTime::Daily {
                time: "00:00".into(),
            },
        ),
        (
            "매일 정오",
            MissionChatTime::Daily {
                time: "12:00".into(),
            },
        ),
        (
            "매일 자정",
            MissionChatTime::Daily {
                time: "00:00".into(),
            },
        ),
        (
            "Today at 09:45",
            MissionChatTime::LocalOnce {
                date: "2026-11-14".into(),
                time: "09:45".into(),
            },
        ),
        (
            "Tomorrow at 9 AM",
            MissionChatTime::LocalOnce {
                date: "2026-11-15".into(),
                time: "09:00".into(),
            },
        ),
        (
            "2026-12-20 at 09:45",
            MissionChatTime::LocalOnce {
                date: "2026-12-20".into(),
                time: "09:45".into(),
            },
        ),
        (
            "한 시간에 한 번씩",
            MissionChatTime::Interval { seconds: 3600 },
        ),
        (
            "every 1 hour 30 minutes",
            MissionChatTime::Interval { seconds: 5400 },
        ),
    ] {
        let mut d = decision(quote, true);
        d.time_quote = Some(quote.into());
        d.time = Some(time);
        assert!(civil_consistent(&d, anchor, "Asia/Seoul"), "{quote}");
    }
    let mut d = decision("Every day at 09:00 UTC", true);
    d.time_quote = Some("Every day at 09:00 UTC".into());
    d.time = Some(MissionChatTime::Daily {
        time: "09:00".into(),
    });
    assert!(
        matches!(rule(&d,anchor,Some("Asia/Seoul"),"Every day at 09:00 UTC"),Ok(ScheduleRule::Daily{timezone,..}) if timezone=="UTC")
    );
    assert!(
        matches!(rule(&d,anchor,Some("UTC"),"Every day at 09:00 America/New_York"),Ok(ScheduleRule::Daily{timezone,..}) if timezone=="America/New_York")
    );
    for text in [
        "Every day at 09:00 UTC+09:00",
        "Every day at 09:00 KST",
        "Every day at 09:00 +09:00",
        "Every day at 09:00 +0900",
        "Every day at 09:00 UTC Asia/Seoul",
    ] {
        assert!(
            rule(&d, anchor, Some("Asia/Seoul"), text).is_err(),
            "{text}"
        );
    }
    for (quote, wrong) in [
        ("매일 오후 아홉 시 십오 분", "21:00"),
        ("Every day at half past nine in the morning", "09:00"),
    ] {
        d.time_quote = Some(quote.into());
        d.time = Some(MissionChatTime::Daily { time: wrong.into() });
        assert!(!civil_consistent(&d, anchor, "UTC"));
    }
    d.time_quote = Some("Today at 09:00".into());
    d.time = Some(MissionChatTime::LocalOnce {
        date: "2026-11-14".into(),
        time: "09:00".into(),
    });
    assert!(civil_consistent(&d, anchor, "Asia/Seoul"));
    assert!(!civil_consistent(&d, anchor, "UTC"));
}

#[tokio::test]
async fn revoked_epoch_cannot_resume_pending_plan_but_fresh_current_command_can() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let text = "Take on a comparison of public ferry notices in the background.";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, decision(text, false)).await;
    let fresh = s
        .change_password(&u, crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(s.apply_mission_chat(&l).await.is_err());
    assert!(s.retry_run(&fresh, l.run_id().into(), 1).await.is_err());
    assert!(s.missions(&fresh).await.unwrap().is_empty());
    let (_, new) = source(&s, &fresh, text, None).await;
    commit(&s, &new, decision(text, false)).await;
    assert_eq!(s.apply_mission_chat(&new).await.unwrap(), Some(true));
    assert_eq!(s.missions(&fresh).await.unwrap().len(), 1);
}

#[tokio::test]
async fn explicit_zone_is_saved_in_actual_parent_and_unsupported_offset_clarifies() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    for (quote, zone) in [
        ("Every day at 09:45 UTC", Some("UTC")),
        (
            "Every day at 10:15 America/New_York",
            Some("America/New_York"),
        ),
        ("Every day at 09:45 UTC+09:00", None),
    ] {
        let text = format!("{quote} summarize public harbor notices.");
        let (_, l) = source(&s, &u, &text, None).await;
        let mut d = decision(&text, true);
        d.time_quote = Some(quote.into());
        d.time = Some(MissionChatTime::Daily {
            time: if quote.contains("10:15") {
                "10:15"
            } else {
                "09:45"
            }
            .into(),
        });
        commit(&s, &l, d).await;
        s.apply_mission_chat(&l).await.unwrap();
        let receipt = s.mission_chat_receipt_for_test(&l).await;
        if let Some(zone) = zone {
            assert_eq!(receipt.state, "active");
            let rows = s.recurring_missions(&u).await.unwrap();
            let row = rows
                .iter()
                .find(|p| p.schedule.id == receipt.target.as_ref().unwrap().id)
                .unwrap();
            assert!(
                matches!(&row.schedule.rule,ScheduleRule::Daily{timezone,..} if timezone==zone)
            );
        } else {
            assert_eq!(receipt.state, "clarify");
            assert!(receipt.target.is_none());
        }
    }
}

impl Store {
    async fn mission_chat_receipt_for_test(&self, l: &RunLease) -> MissionChatReceipt {
        let l = l.clone();
        self.call(move |db| Ok(receipt(&db.conn, &l.tenant, &l.run)?.unwrap()))
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn named_and_selected_targets_must_agree_including_cross_kind_and_current_revision() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let single = s
        .create_mission(&u, crate::missions::tests::input(MissionMode::Durable))
        .await
        .unwrap();
    s.control_mission(
        &u,
        single.id.clone(),
        MissionControl {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: 1,
            action: MissionAction::Pause,
        },
    )
    .await
    .unwrap();
    let title = "매일 아침9시에 항만 현황 정리해 줘";
    let (_, l) = source(&s, &u, title, None).await;
    commit(&s, &l, decision(title, true)).await;
    s.apply_mission_chat(&l).await.unwrap();
    let parent = s.recurring_missions(&u).await.unwrap().remove(0);
    let selected = MissionChatTarget {
        kind: "recurring".into(),
        id: parent.schedule.id.clone(),
        revision: 1,
    };
    let text = format!("Please resume {}", single.request.task);
    let (_, l) = source(&s, &u, &text, Some(selected.clone())).await;
    let mut d = decision(&text, false);
    d.task_quote = None;
    d.action = Some("resume".into());
    d.target_kind = Some("one_off".into());
    d.target_quote = Some(single.request.task);
    commit(&s, &l, d).await;
    s.apply_mission_chat(&l).await.unwrap();
    let receipt = s.mission_chat_receipt_for_test(&l).await;
    assert_eq!(receipt.state, "clarify");
    assert_eq!(receipt.candidates.len(), 2);
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "paused");
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "active"
    );
    // A matching complete name and selected identity are accepted.
    let text = format!("Please pause {title}");
    let (_, l) = source(&s, &u, &text, Some(selected.clone())).await;
    let mut d = decision(&text, false);
    d.task_quote = None;
    d.action = Some("pause".into());
    d.target_kind = Some("recurring".into());
    d.target_quote = Some(title.into());
    commit(&s, &l, d).await;
    s.apply_mission_chat(&l).await.unwrap();
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "paused"
    );
    // The old selected revision cannot resume the now-paused parent.
    let text = "Resume the selected task.";
    let (_, l) = source(&s, &u, text, Some(selected)).await;
    let mut d = decision(text, false);
    d.task_quote = None;
    d.action = Some("resume".into());
    d.target_kind = Some("selected".into());
    commit(&s, &l, d).await;
    s.apply_mission_chat(&l).await.unwrap();
    assert_eq!(s.mission_chat_receipt_for_test(&l).await.state, "clarify");
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "paused"
    );
}

#[tokio::test]
async fn target_guard_survives_failed_confirmation_and_revocation_purge_is_not_an_action() {
    let _g = TEST_LOCK.lock().await;
    for mode in 0..3 {
        let revoked = mode == 1;
        let (_dir, s, u) = setup().await;
        let m = s
            .create_mission(&u, crate::missions::tests::input(MissionMode::Durable))
            .await
            .unwrap();
        let target = s.claim_agent_run().await.unwrap().unwrap();
        let ctx = s.agent_context(&target.lease).await.unwrap();
        let step = s
            .begin_agent_step(&target.lease, ctx.checkpoint_seq)
            .await
            .unwrap();
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"v":1}"#.to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
        s.control_mission(
            &u,
            m.id.clone(),
            MissionControl {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: 1,
                action: MissionAction::Pause,
            },
        )
        .await
        .unwrap();
        let run = target.lease.run_id().to_owned();
        s.call(move |db| {
            db.conn.execute(
                if revoked {
                    "UPDATE agent_execution SET generation=-1 WHERE run_id=?1"
                } else if mode==0 {
                    "UPDATE agent_execution SET deadline_ms=1 WHERE run_id=?1"
                } else {"UPDATE agent_execution SET generation=generation WHERE run_id=?1"},
                [&run],
            )?;
            if mode==2 {
                // Preflight observes a valid deadline; its clock update makes the deadline
                // expire just before the actual control guard, modelling the boundary race.
                db.conn.execute_batch(&format!("CREATE TRIGGER expire_after_preflight AFTER UPDATE OF last_clock_ms ON agent_execution WHEN NEW.run_id='{run}' BEGIN UPDATE agent_execution SET deadline_ms=1 WHERE run_id=NEW.run_id;END;"))?;
            }
            Ok(())
        })
        .await
        .unwrap();
        let text = "Resume the selected task.";
        let (_, l) = source(
            &s,
            &u,
            text,
            Some(MissionChatTarget {
                kind: "one_off".into(),
                id: m.id.clone(),
                revision: 2,
            }),
        )
        .await;
        let mut d = decision(text, false);
        d.task_quote = None;
        d.action = Some("resume".into());
        d.target_kind = Some("selected".into());
        commit(&s, &l, d).await;
        s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_guard_confirmation BEFORE INSERT ON chat_output BEGIN SELECT RAISE(ABORT,'synthetic confirmation failure');END;")?;Ok(())}).await.unwrap();
        assert!(s.apply_mission_chat(&l).await.is_err());
        let run = target.lease.run_id().to_owned();
        s.call(move|db|{
            let (state,invalid):(String,bool)=db.conn.query_row("SELECT r.state,e.invalidated FROM chat_runs r JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE r.id=?1",[run],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if revoked {assert!(invalid)}else{assert_eq!(state,"incomplete")}
            assert_eq!(db.conn.query_row("SELECT count(*) FROM mission_chat_commands",[],|r|r.get::<_,i64>(0))?,0);
            assert_eq!(db.conn.query_row("SELECT count(*) FROM mission_controls",[],|r|r.get::<_,i64>(0))?,1);
            db.conn.execute_batch("DROP TRIGGER fail_guard_confirmation")?;Ok(())
        }).await.unwrap();
        s.apply_mission_chat(&l).await.unwrap();
        let receipt = s.mission_chat_receipt_for_test(&l).await;
        assert_eq!(receipt.state, if revoked { "revoked" } else { "expired" });
        assert!(receipt.target.is_none());
    }
}
