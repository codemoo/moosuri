use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
fn ts(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}
fn parsed(text: &str, tz: Option<&str>) -> parser::Parsed {
    match parser::parse(text, tz, ts("2027-01-04T00:00:00Z"), false) {
        parser::Decision::Command(c) => c,
        _ => panic!("not a command: {text}"),
    }
}
#[test]
fn current_user_english_korean_grammar_and_body_preservation() {
    let cases = [
        (
            "Remind me in 10 MINUTES to Review My Notes",
            ScheduleRule::Once {
                at: ts("2027-01-04T00:10:00Z"),
            },
            "Review My Notes",
        ),
        (
            "10분 뒤에 물 마시라고 알려줘",
            ScheduleRule::Once {
                at: ts("2027-01-04T00:10:00Z"),
            },
            "물 마시라고",
        ),
        (
            "내일 오전 아홉시에 합성 자료 확인하라고 알려줘",
            ScheduleRule::Once {
                at: ts("2027-01-05T00:00:00Z"),
            },
            "합성 자료 확인하라고",
        ),
        (
            "remind me every Monday and Wednesday at 9AM to Stretch",
            ScheduleRule::Weekly {
                days: vec![1, 3],
                time: "09:00".into(),
                timezone: "Asia/Seoul".into(),
            },
            "Stretch",
        ),
        (
            "매주 월요일,수요일 오전 9시에 스트레칭 알려줘",
            ScheduleRule::Weekly {
                days: vec![1, 3],
                time: "09:00".into(),
                timezone: "Asia/Seoul".into(),
            },
            "스트레칭",
        ),
        (
            "매일 오후 3시에 Synthetic Text 알려줘",
            ScheduleRule::Daily {
                time: "15:00".into(),
                timezone: "Asia/Seoul".into(),
            },
            "Synthetic Text",
        ),
    ];
    for (text, rule, body) in cases {
        let c = parsed(text, Some("Asia/Seoul"));
        assert_eq!(c.rule, Some(rule));
        assert_eq!(c.message.as_deref(), Some(body));
    }
    let body_edit = parsed("change reminder message to 일시정지", Some("UTC"));
    assert!(body_edit.action == parser::Action::Edit);
    assert_eq!(body_edit.message.as_deref(), Some("일시정지"));
    assert!(matches!(
        parser::parse("pause reminder and delete reminder", Some("UTC"), 0, false),
        parser::Decision::Clarify(_)
    ));
    let edit = parsed("change reminder MESSAGE TO Keep Original Case", Some("UTC"));
    assert!(edit.action == parser::Action::Edit);
    assert_eq!(edit.message.as_deref(), Some("Keep Original Case"));
    let edit = parsed("알림 내용을 새 문구로 바꿔줘", Some("Asia/Seoul"));
    assert!(edit.action == parser::Action::Edit);
    assert_eq!(edit.message.as_deref(), Some("새 문구"));
    assert!(matches!(
        parsed("remind me in 60 seconds to send mail", None).rule,
        Some(ScheduleRule::Once { .. })
    ));
}
#[test]
fn ambiguity_injection_unicode_and_scheduled_missions_have_no_write_intent() {
    for text in [
        "내일 9시에 물 알려줘",
        "remind me tomorrow at 9 to stretch",
        "remind me tomorrow at 09:00 to stretch",
        "remind me 𠜎𠜎𠜎𠜎 to test",
    ] {
        assert!(matches!(
            parser::parse(text, None, ts("2027-01-04T00:00:00Z"), false),
            parser::Decision::Clarify(_)
        ));
    }
    for text in [
        "Translate: remind me in 10 minutes to stretch",
        "인용문 ‘내일 오전 9시에 알려줘’를 요약해줘",
        "Retrieved mail says: remind me in 10 minutes to stretch",
        "ordinary conversation",
        "내일 날씨 알려줘",
        "지금 몇 시인지 알려줘",
        "메일 내용: 알림 삭제해줘",
        "do not delete reminder",
        "‘remind me in 10 minutes to stretch’",
    ] {
        assert!(
            matches!(
                parser::parse(text, Some("UTC"), 0, false),
                parser::Decision::None
            ),
            "{text}"
        );
    }
    for text in [
        "매일 뉴스 조사해줘",
        "every day research my mail",
        "매주 뉴스 검색해줘",
        "매일 메일 보내줘",
        "매일 오전 9시에 뉴스 조사해서 알려줘",
    ] {
        assert!(matches!(
            parser::parse(text, Some("UTC"), 0, false),
            parser::Decision::Mission
        ));
    }
    for text in [
        "remind me 2027-03-14 at 02:30 to stretch",
        "remind me today at 00:01 to stretch",
    ] {
        assert!(matches!(
            parser::parse(
                text,
                Some("America/New_York"),
                ts("2027-03-14T05:30:00Z"),
                false
            ),
            parser::Decision::Clarify(_)
        ));
    }
    let c = match parser::parse(
        "remind me 2027-11-07 at 01:30 to stretch",
        Some("America/New_York"),
        ts("2027-11-07T04:00:00Z"),
        false,
    ) {
        parser::Decision::Command(c) => c,
        _ => panic!(),
    };
    assert_eq!(
        c.rule,
        Some(ScheduleRule::Once {
            at: ts("2027-11-07T05:30:00Z")
        })
    );
}
#[test]
fn weekly_gap_fold_downtime_and_invalid_days() {
    let rule = |days, time: &str| ScheduleRule::Weekly {
        days,
        time: time.into(),
        timezone: "America/New_York".into(),
    };
    let gap = rule(vec![7], "02:30");
    let first = time::after(&gap, ts("2027-03-14T05:00:00Z"))
        .unwrap()
        .unwrap();
    assert!(first.at.is_none());
    let plan = time::plan(&gap, &first, first.due).unwrap();
    assert!(plan.latest.is_none());
    assert_eq!(plan.gaps.len(), 1);
    assert_eq!(plan.next.unwrap().at, Some(ts("2027-03-21T06:30:00Z")));
    let fold = rule(vec![7], "01:30");
    assert_eq!(
        time::after(&fold, ts("2027-11-07T04:00:00Z"))
            .unwrap()
            .unwrap()
            .at,
        Some(ts("2027-11-07T05:30:00Z"))
    );
    assert_eq!(
        time::after(&fold, ts("2027-11-07T05:31:00Z"))
            .unwrap()
            .unwrap()
            .at,
        Some(ts("2027-11-14T06:30:00Z"))
    );
    let multi = rule(vec![1, 3, 5], "09:00");
    let first = time::after(&multi, ts("2027-01-01T00:00:00Z"))
        .unwrap()
        .unwrap();
    let plan = time::plan(&multi, &first, ts("2027-02-01T15:00:00Z")).unwrap();
    assert_eq!(plan.latest.unwrap().at, Some(ts("2027-02-01T14:00:00Z")));
    assert_eq!(plan.skipped.unwrap().2, 13);
    assert_eq!(plan.next.unwrap().at, Some(ts("2027-02-03T14:00:00Z")));
    assert!(
        time::after(&rule(vec![1, 3, 5], "09:00"), 4_102_444_800)
            .unwrap()
            .is_none()
    );
    for days in [vec![], vec![0], vec![8], vec![3, 1], vec![1, 1]] {
        assert!(time::preview(&rule(days, "09:00"), 0).is_err());
    }
}
async fn configured(store: &Store) -> UserContext {
    let u = admin(store).await.user;
    let p = store
        .create_provider(
            &u,
            crate::ProviderChange {
                name: "Synthetic reminder only".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&u, p.id.clone(), 1, "synthetic-model".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &u,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: p.id,
                provider_revision: 1,
                model: "synthetic-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    u
}
fn context(target: Option<ScheduleTarget>) -> Option<ScheduleChatContext> {
    Some(ScheduleChatContext {
        timezone: "Asia/Seoul".into(),
        target,
    })
}
async fn send(
    store: &Store,
    u: &UserContext,
    thread: &str,
    text: &str,
    target: Option<ScheduleTarget>,
) -> moosuri_core::ChatTurn {
    store
        .submit_turn_with_schedule_context(
            u,
            thread.into(),
            Uuid::new_v4().to_string(),
            text.into(),
            None,
            context(target),
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn atomic_receipts_retry_restart_and_no_model_claim() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let thread = store
        .create_thread(&u, "Synthetic".into())
        .await
        .unwrap()
        .id;
    let request = Uuid::new_v4().to_string();
    let submit = || {
        store.submit_turn_with_schedule_context(
            &u,
            thread.clone(),
            request.clone(),
            "remind me in 10 minutes to stretch".into(),
            None,
            context(None),
        )
    };
    let a = submit().await.unwrap();
    let b = submit().await.unwrap();
    assert_eq!(a.run.state, RunState::Completed);
    assert!(!a.run.can_cancel);
    assert_eq!(
        serde_json::to_value(&a).unwrap(),
        serde_json::to_value(&b).unwrap()
    );
    assert_eq!(store.schedules(&u).await.unwrap().len(), 1);
    assert!(store.claim_run().await.unwrap().is_none());
    let receipts = a.run.schedule_receipts.clone().unwrap();
    assert_eq!(receipts.len(), 1);
    assert_eq!(receipts[0].revision, 1);
    assert!(matches!(
        store
            .submit_turn_with_schedule_context(
                &u,
                thread.clone(),
                request.clone(),
                "remind me in 11 minutes to stretch".into(),
                None,
                context(None)
            )
            .await,
        Err(StoreError::Conflict)
    ));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let b = store
        .submit_turn_with_schedule_context(
            &u,
            thread,
            request,
            "remind me in 10 minutes to stretch".into(),
            None,
            context(None),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&b.run.schedule_receipts).unwrap(),
        serde_json::to_value(&receipts).unwrap()
    );
    assert_eq!(store.schedules(&u).await.unwrap().len(), 1);
}
#[tokio::test]
async fn selected_revision_ownership_controls_and_pending_delivery_are_fenced() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = configured(&store).await;
    let b = member(&store, &a, "synthetic-other@example.test")
        .await
        .user;
    let t = store
        .create_thread(&a, "Synthetic".into())
        .await
        .unwrap()
        .id;
    let other = store
        .create_thread(&b, "Synthetic other".into())
        .await
        .unwrap()
        .id;
    let first = send(
        &store,
        &a,
        &t,
        "매주 월요일,수요일 오전 9시에 스트레칭 알려줘",
        None,
    )
    .await;
    let r = &first.run.schedule_receipts.unwrap()[0];
    let target = ScheduleTarget {
        id: r.id.clone(),
        revision: r.revision,
    };
    assert!(matches!(
        store
            .submit_turn_with_schedule_context(
                &b,
                other,
                Uuid::new_v4().to_string(),
                "pause reminder".into(),
                None,
                context(Some(target.clone()))
            )
            .await,
        Err(StoreError::NotFound)
    ));
    let paused = send(&store, &a, &t, "pause reminder", Some(target.clone())).await;
    assert_eq!(paused.run.schedule_receipts.unwrap()[0].state, "paused");
    assert!(matches!(
        store
            .submit_turn_with_schedule_context(
                &a,
                t.clone(),
                Uuid::new_v4().to_string(),
                "delete reminder".into(),
                None,
                context(Some(target))
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let target = ScheduleTarget {
        id: r.id.clone(),
        revision: 2,
    };
    let edited = send(
        &store,
        &a,
        &t,
        "change reminder to tomorrow at 9am",
        Some(target),
    )
    .await;
    let r = &edited.run.schedule_receipts.unwrap()[0];
    assert_eq!(r.state, "paused");
    assert_eq!(r.revision, 3);
    let resumed = send(
        &store,
        &a,
        &t,
        "resume reminder",
        Some(ScheduleTarget {
            id: r.id.clone(),
            revision: 3,
        }),
    )
    .await;
    assert_eq!(resumed.run.schedule_receipts.unwrap()[0].revision, 4);
    let removed = send(
        &store,
        &a,
        &t,
        "delete reminder",
        Some(ScheduleTarget {
            id: r.id.clone(),
            revision: 4,
        }),
    )
    .await;
    assert_eq!(removed.run.schedule_receipts.unwrap()[0].state, "revoked");
    assert!(store.schedules(&a).await.unwrap().is_empty());
    let created = send(
        &store,
        &a,
        &t,
        "remind me in 1 second to synthetic tick",
        None,
    )
    .await;
    let schedule = created.run.schedule_receipts.unwrap()[0].id.clone();
    let now = clock().unwrap() + 3;
    store.enqueue_scheduled_reminders_at(now).await.unwrap();
    let lease = store
        .claim_scheduled_reminder_at(now)
        .await
        .unwrap()
        .unwrap();
    send(
        &store,
        &a,
        &t,
        "pause reminder",
        Some(ScheduleTarget {
            id: schedule,
            revision: 1,
        }),
    )
    .await;
    assert!(
        store
            .finish_scheduled_reminder_at(&lease, now)
            .await
            .is_err()
    );
    assert!(
        store
            .notifications(&a, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}
#[tokio::test]
async fn failed_confirmation_rolls_back_every_effect_and_logout_rejects_admission() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let t = store
        .create_thread(&u, "Synthetic".into())
        .await
        .unwrap()
        .id;
    store.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_confirmation BEFORE INSERT ON chat_output BEGIN SELECT RAISE(ABORT,'synthetic fault'); END;")?;Ok(())}).await.unwrap();
    let request = Uuid::new_v4().to_string();
    assert!(
        store
            .submit_turn_with_schedule_context(
                &u,
                t.clone(),
                request.clone(),
                "remind me in 10 minutes to stretch".into(),
                None,
                context(None)
            )
            .await
            .is_err()
    );
    assert!(store.schedules(&u).await.unwrap().is_empty());
    assert!(
        store
            .conversation(&u, t.clone(), None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    store
        .call(|db| {
            let n: i64 =
                db.conn
                    .query_row("SELECT count(*) FROM schedule_chat_commands", [], |r| {
                        r.get(0)
                    })?;
            assert_eq!(n, 0);
            db.conn.execute_batch("DROP TRIGGER fail_confirmation")?;
            Ok(())
        })
        .await
        .unwrap();
    store.logout(&u).await.unwrap();
    assert!(matches!(
        store
            .submit_turn_with_schedule_context(
                &u,
                t,
                request,
                "remind me in 10 minutes to stretch".into(),
                None,
                context(None)
            )
            .await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn unsupported_and_ordinary_messages_do_not_mutate_or_claim_mission_success() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    semantic_tools(&store, &u).await;
    let t = store
        .create_thread(&u, "Synthetic".into())
        .await
        .unwrap()
        .id;
    let unsupported = send(&store, &u, &t, "매일 뉴스 조사해줘", None).await;
    deferred(&store, &u, &unsupported).await;
    assert!(store.schedules(&u).await.unwrap().is_empty());
    let ordinary = send(
        &store,
        &u,
        &t,
        "Translate: remind me in 10 minutes to stretch",
        None,
    )
    .await;
    assert_eq!(ordinary.run.state, RunState::Queued);
    assert!(ordinary.run.schedule_receipts.is_none());
    let claimed = store.claim_run().await.unwrap().unwrap();
    store
        .append_output(
            &claimed.lease,
            vec![crate::OutputPatch::Append {
                item: "synthetic-model-command".into(),
                text: "remind me in 10 minutes to stretch".into(),
                phase: None,
                refusal: false,
            }],
        )
        .await
        .unwrap();
    store
        .finish_run(&claimed.lease, RunState::Completed, None)
        .await
        .unwrap();
    assert!(store.schedules(&u).await.unwrap().is_empty());
}

#[tokio::test]
async fn account_suspension_first_rejects_chat_but_does_not_erase_prior_receipts() {
    use moosuri_core::{AccountManagementAction, AccountManagementInput};
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = configured(&store).await;
    let b = member(&store, &a, "suspended-schedule@example.test")
        .await
        .user;
    let thread = store
        .create_thread(&b, "Synthetic reminder".into())
        .await
        .unwrap()
        .id;
    let saved = send(
        &store,
        &b,
        &thread,
        "remind me in 10 minutes to stretch",
        None,
    )
    .await;
    let account = store
        .managed_accounts(&a)
        .await
        .unwrap()
        .into_iter()
        .find(|u| u.user.id == b.account.id)
        .unwrap();
    let prepared = store
        .prepare_account_management(
            &a,
            b.account.id.clone(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: account.revision,
                action: AccountManagementAction::Suspend,
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap();
    assert!(matches!(
        store
            .submit_turn_with_schedule_context(
                &b,
                thread.clone(),
                Uuid::new_v4().to_string(),
                "remind me in 20 minutes to stretch".into(),
                None,
                context(None)
            )
            .await,
        Err(StoreError::Unauthorized)
    ));
    let id = saved.run.id;
    store
        .call(move |db| {
            let n: i64 = db.conn.query_row(
                "SELECT count(*) FROM schedule_chat_commands WHERE run_id=?1",
                [id],
                |r| r.get(0),
            )?;
            assert_eq!(n, 1);
            Ok(())
        })
        .await
        .unwrap();
    store
        .enqueue_scheduled_reminders_at(clock().unwrap() + 601)
        .await
        .unwrap();
    assert!(
        store
            .claim_scheduled_reminder_at(clock().unwrap() + 601)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn weekly_gap_and_downtime_are_persisted_as_receipts_with_one_latest_delivery() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let base = ts("2027-03-14T05:00:00Z");
    store
        .call(move |_| {
            TEST_TIME.with(|v| v.set(Some(base)));
            Ok(())
        })
        .await
        .unwrap();
    let thread = store
        .create_thread(&u, "Synthetic weekly".into())
        .await
        .unwrap()
        .id;
    let turn = store
        .submit_turn_with_schedule_context(
            &u,
            thread,
            Uuid::new_v4().to_string(),
            "remind me every Sunday at 02:30 to stretch".into(),
            None,
            Some(ScheduleChatContext {
                timezone: "America/New_York".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let id = turn.run.schedule_receipts.unwrap()[0].id.clone();
    store
        .enqueue_scheduled_reminders_at(ts("2027-03-14T07:00:00Z"))
        .await
        .unwrap();
    assert!(
        store
            .claim_scheduled_reminder_at(ts("2027-03-14T07:00:00Z"))
            .await
            .unwrap()
            .is_none()
    );
    let h = store.schedule_history(&u, id.clone()).await.unwrap();
    assert_eq!(h.occurrences[0].reason.as_deref(), Some("dst_gap"));
    let now = ts("2027-04-04T12:00:00Z");
    store.enqueue_scheduled_reminders_at(now).await.unwrap();
    let lease = store
        .claim_scheduled_reminder_at(now)
        .await
        .unwrap()
        .unwrap();
    let n = store
        .finish_scheduled_reminder_at(&lease, now)
        .await
        .unwrap();
    assert_eq!(n.schedule_id, id);
    let h = store.schedule_history(&u, id).await.unwrap();
    assert_eq!(h.skipped_ranges[0].count, 2);
    assert_eq!(
        h.occurrences
            .iter()
            .filter(|o| o.state == "delivered")
            .count(),
        1
    );
    assert!(
        store
            .claim_scheduled_reminder_at(now)
            .await
            .unwrap()
            .is_none()
    );
}

// SC1: a selected identity must not turn a question/condition/alternative into authority.
#[tokio::test]
async fn repair_controls_require_a_complete_current_imperative_and_exact_owned_title() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    let thread = store
        .create_thread(&u, "Synthetic authority repair".into())
        .await
        .unwrap()
        .id;
    let first = send(
        &store,
        &u,
        &thread,
        "remind me in 10 minutes to water",
        None,
    )
    .await;
    let receipt = &first.run.schedule_receipts.unwrap()[0];
    let selected = ScheduleTarget {
        id: receipt.id.clone(),
        revision: receipt.revision,
    };
    let rejected = [
        "delete reminder if I ask you later",
        "delete reminder? What would happen?",
        "delete water reminder or keep it?",
        "pause reminder when I say so later",
        "cancel water reminder unless I object",
        "delete water reminder and resume it",
        "delete water reminder please discuss the consequences",
        "delete waterish reminder",
        "delete \"water reminder\"",
        "나중에 요청하면 알림 삭제해줘",
        "알림 삭제 또는 재개해줘",
        "알림 삭제하면 어떻게 돼?",
        "알림 삭제하지 마",
        "알림 다시 켜줘?",
        "나중에 요청하면 알림 꺼줘",
        "알림 켜거나 꺼줘",
        "알림 꺼주지 마",
        "알림 꺼줘도 돼?",
    ];
    for target in [None, Some(selected.clone())] {
        for text in rejected {
            let turn = send(&store, &u, &thread, text, target.clone()).await;
            assert_eq!(turn.run.state, RunState::Completed, "{text}");
            assert!(turn.run.schedule_receipts.is_none(), "{text}");
            let rows = store.schedules(&u).await.unwrap();
            assert_eq!(rows.len(), 1, "{text}");
            assert_eq!(rows[0].state, "active", "{text}");
            assert_eq!(rows[0].revision, 1, "{text}");
            assert!(store.claim_run().await.unwrap().is_none(), "{text}");
        }
    }
    let paused = send(&store, &u, &thread, "pause water reminder", None).await;
    let r = &paused.run.schedule_receipts.unwrap()[0];
    assert_eq!(r.state, "paused");
    assert_eq!(r.revision, 2);
    let resumed = send(
        &store,
        &u,
        &thread,
        "resume reminder",
        Some(ScheduleTarget {
            id: r.id.clone(),
            revision: 2,
        }),
    )
    .await;
    let r = &resumed.run.schedule_receipts.unwrap()[0];
    assert_eq!(r.state, "active");
    assert_eq!(r.revision, 3);
    let literal = "summarize my notes, then quote: 'do not delete this reminder?'";
    let edited = send(
        &store,
        &u,
        &thread,
        &format!("change water reminder message to {literal}"),
        None,
    )
    .await;
    assert_eq!(edited.run.schedule_receipts.unwrap()[0].revision, 4);
    assert_eq!(store.schedules(&u).await.unwrap()[0].message, literal);
    let edited = send(
        &store,
        &u,
        &thread,
        "change water reminder to tomorrow at 09:00",
        None,
    )
    .await;
    assert_eq!(edited.run.schedule_receipts.unwrap()[0].revision, 5);
    let deleted = send(&store, &u, &thread, "delete water reminder", None).await;
    assert_eq!(deleted.run.schedule_receipts.unwrap()[0].state, "revoked");
    assert!(store.schedules(&u).await.unwrap().is_empty());
    assert!(store.claim_run().await.unwrap().is_none());
    send(
        &store,
        &u,
        &thread,
        "remind me in 10 minutes to water",
        None,
    )
    .await;
    send(
        &store,
        &u,
        &thread,
        "remind me in 10 minutes to the water",
        None,
    )
    .await;
    let changed = send(&store, &u, &thread, "pause the water reminder", None).await;
    let r = &changed.run.schedule_receipts.unwrap()[0];
    assert_eq!(r.title, "the water");
    assert_eq!(r.state, "paused");
    assert_eq!(
        store
            .schedules(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.title == "water")
            .unwrap()
            .state,
        "active"
    );
}

#[tokio::test]
async fn everyday_korean_on_off_controls_preserve_one_owned_reminder() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = configured(&store).await;
    let thread = store
        .create_thread(&user, "Synthetic reminder continuity".into())
        .await
        .unwrap()
        .id;
    let created = send(&store, &user, &thread, "10분 뒤에 물 마시라고 알려줘", None).await;
    let mut receipt = created.run.schedule_receipts.unwrap().remove(0);
    for (text, selected, state) in [
        ("알림 꺼줘", false, "paused"),
        ("알림 다시 켜줘", false, "active"),
        ("물 마시라고 알림을 꺼 주세요", false, "paused"),
        ("물 마시라고 알림 다시 켜 주세요", false, "active"),
        ("꺼줘", true, "paused"),
        ("다시 켜 줘", true, "active"),
    ] {
        let target = selected.then(|| ScheduleTarget {
            id: receipt.id.clone(),
            revision: receipt.revision,
        });
        let turn = send(&store, &user, &thread, text, target).await;
        assert_eq!(turn.run.state, RunState::Completed, "{text}");
        let next = turn.run.schedule_receipts.unwrap().remove(0);
        assert_eq!(next.id, receipt.id, "{text}");
        assert_eq!(next.revision, receipt.revision + 1, "{text}");
        assert_eq!(next.state, state, "{text}");
        assert_eq!(store.schedules(&user).await.unwrap().len(), 1);
        assert!(store.claim_run().await.unwrap().is_none());
        receipt = next;
    }
}

// SC2/SC3: known task requests are unsupported, while reminder-message bodies are literal data.
#[tokio::test]
async fn scheduled_missions_defer_to_isolated_planner_and_literal_meta_words_still_save_reminders()
{
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    semantic_tools(&store, &u).await;
    store
        .call(|_| {
            TEST_TIME.with(|v| v.set(Some(ts("2027-01-04T00:00:00Z"))));
            Ok(())
        })
        .await
        .unwrap();
    let thread = store
        .create_thread(&u, "Synthetic shell/body repair".into())
        .await
        .unwrap()
        .id;
    for text in [
        "내일 오전 9시에 뉴스 조사해서 알려줘",
        "매일 오전 9시에 뉴스 조사해서 알려줘",
        "10분 뒤에 뉴스 조사해서 알려줘",
        "내일 오전 9시에 메일 확인해서 알려줘",
        "매일 메일 보내줘",
    ] {
        let turn = send(&store, &u, &thread, text, None).await;
        deferred(&store, &u, &turn).await;
        assert!(store.schedules(&u).await.unwrap().is_empty(), "{text}");
        assert!(store.claim_run().await.unwrap().is_none(), "{text}");
    }
    let cases = [
        (
            "remind me in 10 minutes to summarize my notes",
            "summarize my notes",
        ),
        (
            "remind me in 10 minutes to translate: 'do not delete this reminder?'",
            "translate: 'do not delete this reminder?'",
        ),
        (
            "remind me in 10 minutes to quote the example; never send mail.",
            "quote the example; never send mail.",
        ),
        ("10분 뒤에 요약하라고 알려줘", "요약하라고"),
        ("내일 오전 9시에 뉴스 조사하라고 알려줘", "뉴스 조사하라고"),
        (
            "내일 오전 9시에 뉴스 조사해서 알림 설정해줘",
            "뉴스 조사해서",
        ),
    ];
    let mut target = None;
    for (i, (text, body)) in cases.into_iter().enumerate() {
        let turn = send(&store, &u, &thread, text, None).await;
        assert_eq!(turn.run.state, RunState::Completed, "{text}");
        let receipts = turn.run.schedule_receipts.unwrap();
        assert_eq!(receipts.len(), 1);
        assert_eq!(receipts[0].action, "create");
        let rows = store.schedules(&u).await.unwrap();
        assert_eq!(rows.len(), i + 1);
        assert_eq!(
            rows.iter()
                .find(|s| s.id == receipts[0].id)
                .unwrap()
                .message,
            body,
            "{text}"
        );
        assert!(store.claim_run().await.unwrap().is_none(), "{text}");
        target = Some(ScheduleTarget {
            id: receipts[0].id.clone(),
            revision: 1,
        });
    }
    let turn = send(
        &store,
        &u,
        &thread,
        "10분 뒤에 알림 삭제하지 말라고 알려줘",
        target,
    )
    .await;
    assert_eq!(turn.run.schedule_receipts.unwrap()[0].action, "create");
    assert!(
        store
            .schedules(&u)
            .await
            .unwrap()
            .iter()
            .any(|s| s.message == "알림 삭제하지 말라고")
    );
    let count = store.schedules(&u).await.unwrap().len();
    let quoted = send(
        &store,
        &u,
        &thread,
        "Translate: remind me in 10 minutes to stretch",
        None,
    )
    .await;
    assert_eq!(quoted.run.state, RunState::Queued);
    assert!(quoted.run.schedule_receipts.is_none());
    let claimed = store.claim_run().await.unwrap().unwrap();
    store
        .finish_run(&claimed.lease, RunState::Completed, None)
        .await
        .unwrap();
    assert_eq!(store.schedules(&u).await.unwrap().len(), count);
}

// SC4: restore the documented absolute-date Korean reminder without widening other frames.
#[tokio::test]
async fn sc4_iso_date_korean_store_receipt_and_invalid_meta_conditional_guards() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = configured(&store).await;
    semantic_tools(&store, &u).await;
    store
        .call(|_| {
            TEST_TIME.with(|v| v.set(Some(ts("2027-01-04T00:00:00Z"))));
            Ok(())
        })
        .await
        .unwrap();
    let thread = store
        .create_thread(&u, "Synthetic ISO date reminder".into())
        .await
        .unwrap()
        .id;
    let text = "2027-01-05 오전 9시에 물 마시라고 알려줘";
    let saved = send(&store, &u, &thread, text, None).await;
    assert_eq!(saved.run.state, RunState::Completed);
    let receipt = &saved.run.schedule_receipts.as_ref().unwrap()[0];
    assert_eq!(receipt.action, "create");
    assert_eq!(receipt.revision, 1);
    let rows = store.schedules(&u).await.unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].id, receipt.id);
    assert_eq!(rows[0].rule, ScheduleRule::Once { at: 1_799_107_200 });
    assert_eq!(rows[0].message, "물 마시라고");
    assert_eq!(rows[0].next_fire_at, Some(ts("2027-01-05T00:00:00Z")));
    assert!(store.claim_run().await.unwrap().is_none());
    let replay = store
        .submit_turn_with_schedule_context(
            &u,
            thread.clone(),
            saved.run.request_id.clone(),
            text.into(),
            None,
            context(None),
        )
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_value(&saved).unwrap(),
        serde_json::to_value(&replay).unwrap()
    );
    for text in [
        "2027-01-05 오전 9시에 뉴스 조사해서 알려줘",
        "2027-01-05 오전 9시에 뉴스 검색해줘",
    ] {
        let mission = send(&store, &u, &thread, text, None).await;
        deferred(&store, &u, &mission).await;
        assert!(store.claim_run().await.unwrap().is_none());
    }
    let selected = Some(ScheduleTarget {
        id: receipt.id.clone(),
        revision: 1,
    });
    for text in [
        "2027-02-30 오전 9시에 물 마시라고 알려줘",
        "2027-13-05 오전 9시에 물 마시라고 알려줘",
        "2027-01-05 뉴스 알려줘",
        "Translate: 2027-01-05 오전 9시에 물 마시라고 알려줘",
        "‘2027-01-05 오전 9시에 물 마시라고 알려줘’",
        "2027-01-05 if I ask later 오전 9시에 물 마시라고 알려줘",
        "만약 부탁하면 2027-01-05 오전 9시에 물 마시라고 알려줘",
        "delete reminder if I ask you on 2027-01-05 later",
        "𠜎𠜎𠜎𠜎 오전 9시에 물 마시라고 알려줘",
    ] {
        let turn = send(&store, &u, &thread, text, selected.clone()).await;
        assert!(turn.run.schedule_receipts.is_none(), "{text}");
        // Ordinary invalid/meta frames can enter ordinary chat, but never schedule.
        // Settle that synthetic lease without invoking a provider so each case is isolated.
        if turn.run.state == RunState::Queued {
            let lease = store.claim_run().await.unwrap().unwrap().lease;
            store
                .finish_run(&lease, RunState::Completed, None)
                .await
                .unwrap();
        } else {
            assert_eq!(turn.run.state, RunState::Completed, "{text}");
            assert!(store.claim_run().await.unwrap().is_none(), "{text}");
        }
        let rows = store.schedules(&u).await.unwrap();
        assert_eq!(rows.len(), 1, "{text}");
        assert_eq!(rows[0].revision, 1, "{text}");
        assert_eq!(rows[0].state, "active", "{text}");
    }
}

async fn semantic_tools(store: &Store, u: &UserContext) {
    let p = store.list_providers(u).await.unwrap().providers.remove(0);
    store
        .record_tool_probe(u, p.id, 1, "synthetic-model".into(), None)
        .await
        .unwrap();
}
async fn deferred(store: &Store, u: &UserContext, turn: &moosuri_core::ChatTurn) {
    assert_eq!(turn.run.state, RunState::Queued);
    assert!(turn.run.schedule_receipts.is_none());
    let id = turn.run.id.clone();
    store
        .call(move |db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT state FROM mission_chat_plans WHERE run_id=?1",
                    [id],
                    |r| r.get::<_, String>(0)
                )?,
                "awaiting"
            );
            Ok(())
        })
        .await
        .unwrap();
    store.cancel_run(u, turn.run.id.clone(), 1).await.unwrap();
}
