use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, directory};
use moosuri_core::{ChatTurn, ModelSelection, ProviderConfig, ProviderKind};
const BASE: i64 = 1_830_556_800; // Synthetic 2028 UTC reference, kept outside user clocks.
async fn configured(s: &Store) -> UserContext {
    let u = crate::tests::admin(s).await.user;
    let p = s
        .create_provider(
            &u,
            crate::ProviderChange {
                name: "Synthetic followup reminders".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    s.record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &u,
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
    u
}
async fn at(s: &Store, at: i64) {
    s.call(move |_| {
        super::super::super::TEST_TIME.with(|v| v.set(Some(at)));
        Ok(())
    })
    .await
    .unwrap();
}
async fn send(
    s: &Store,
    u: &UserContext,
    t: &str,
    id: &str,
    text: &str,
    zone: &str,
) -> Result<ChatTurn> {
    s.submit_turn_with_schedule_context(
        u,
        t.into(),
        id.into(),
        text.into(),
        None,
        Some(ScheduleChatContext {
            timezone: zone.into(),
            target: None,
        }),
    )
    .await
}
async fn turn(s: &Store, u: &UserContext, t: &str, text: &str) -> ChatTurn {
    send(s, u, t, &Uuid::new_v4().to_string(), text, "Asia/Seoul")
        .await
        .unwrap_or_else(|e| panic!("synthetic turn {text}: {e:?}"))
}
async fn ignored(s: &Store, u: &UserContext, t: &str, text: &str) {
    let answer = turn(s, u, t, text).await;
    assert!(answer.run.schedule_receipts.is_none());
    if matches!(answer.run.state, RunState::Queued | RunState::Running) {
        s.cancel_run(u, answer.run.id, answer.run.attempt)
            .await
            .unwrap();
    }
}
async fn thread(s: &Store, u: &UserContext) -> String {
    s.create_thread(u, "Synthetic reminder followups".into())
        .await
        .unwrap()
        .id
}
fn receipt(t: &ChatTurn) -> &ScheduleChatReceipt {
    &t.run.schedule_receipts.as_ref().unwrap()[0]
}
#[tokio::test]
async fn reminder_followup_reopen_exact_retry_anchors_answer_time_and_preserves_source_zone() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    at(&s, BASE).await;
    let t = thread(&s, &u).await;
    let start = turn(&s, &u, &t, "물 마시라고 알려줘").await;
    assert_eq!(start.run.state, RunState::Completed);
    assert!(start.assistant.iter().any(|m| m.text.contains("언제")));
    assert!(s.schedules(&u).await.unwrap().is_empty());
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    at(&s, BASE + 300).await;
    let id = Uuid::new_v4().to_string();
    let done = send(&s, &u, &t, &id, "in 10 minutes", "America/New_York")
        .await
        .unwrap();
    let result = receipt(&done);
    assert_eq!(result.timezone, "Asia/Seoul");
    let schedules = s.schedules(&u).await.unwrap();
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0].message, "물 마시라고");
    assert_eq!(schedules[0].rule, ScheduleRule::Once { at: BASE + 900 });
    at(&s, BASE + 700).await;
    let retry = send(&s, &u, &t, &id, "in 10 minutes", "America/New_York")
        .await
        .unwrap();
    assert_eq!(done.run.id, retry.run.id);
    assert_eq!(
        serde_json::to_value(&done).unwrap(),
        serde_json::to_value(&retry).unwrap()
    );
    assert!(matches!(
        send(&s, &u, &t, &id, "in 11 minutes", "America/New_York").await,
        Err(StoreError::Conflict)
    ));
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    at(&s, BASE + 800).await;
    assert_eq!(
        send(&s, &u, &t, &id, "in 10 minutes", "America/New_York")
            .await
            .unwrap()
            .run
            .id,
        done.run.id
    );
    assert_eq!(s.schedules(&u).await.unwrap().len(), 1);
    let t = thread(&s, &u).await;
    let start = turn(&s, &u, &t, "remind me in 10 minutes").await;
    assert!(start.assistant.iter().any(|m| m.text.contains("어떤 문구")));
    at(&s, BASE + 1000).await;
    let done = turn(
        &s,
        &u,
        &t,
        "내용: Translate the quoted sentence, do not send mail.",
    )
    .await;
    assert_eq!(receipt(&done).action, "create");
    let schedules = s.schedules(&u).await.unwrap();
    let saved = schedules
        .iter()
        .find(|s| s.id == receipt(&done).id)
        .unwrap();
    assert_eq!(
        saved.message,
        "Translate the quoted sentence, do not send mail."
    );
    assert_eq!(saved.rule, ScheduleRule::Once { at: BASE + 1400 });
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert!(s.claim_run().await.unwrap().is_none());
}
#[tokio::test]
async fn reminder_followup_missing_body_and_partial_civil_time_keep_exact_supplied_fields() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    at(&s, BASE).await;
    let t = thread(&s, &u).await;
    let start = turn(&s, &u, &t, "내일 오전 9시에 알려줘").await;
    assert!(start.assistant.iter().any(|m| m.text.contains("어떤 문구")));
    at(&s, BASE + 500).await;
    let done = turn(&s, &u, &t, "물 마시기").await;
    let schedules = s.schedules(&u).await.unwrap();
    let saved = schedules
        .iter()
        .find(|s| s.id == receipt(&done).id)
        .unwrap();
    let expected = parser::rule("내일 오전 9시", Some("Asia/Seoul"), BASE)
        .unwrap()
        .0;
    assert_eq!(saved.rule, expected);
    assert_eq!(saved.message, "물 마시기");
    let t = thread(&s, &u).await;
    at(&s, BASE).await;
    let start = turn(&s, &u, &t, "내일 9시에 약 먹으라고 알려줘").await;
    assert!(start.assistant.iter().any(|m| m.text.contains("오전")));
    at(&s, BASE + 500).await;
    let done = turn(&s, &u, &t, "오후").await;
    let schedules = s.schedules(&u).await.unwrap();
    let saved = schedules
        .iter()
        .find(|s| s.id == receipt(&done).id)
        .unwrap();
    assert_eq!(saved.message, "약 먹으라고");
    assert_eq!(
        saved.rule,
        parser::rule("내일 오후 9시", Some("Asia/Seoul"), BASE)
            .unwrap()
            .0
    );
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me to Stretch! Keep Case.").await;
    turn(&s, &u, &t, "tomorrow at 9").await;
    let done = turn(&s, &u, &t, "AM").await;
    let saved = s
        .schedules(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == receipt(&done).id)
        .unwrap();
    assert_eq!(saved.message, "Stretch! Keep Case.");
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "오후 9시에 알려줘").await;
    turn(&s, &u, &t, "내일").await;
    let done = turn(&s, &u, &t, "stretch").await;
    let saved = s
        .schedules(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == receipt(&done).id)
        .unwrap();
    assert_eq!(
        saved.rule,
        parser::rule("내일 오후 9시", Some("Asia/Seoul"), BASE + 500)
            .unwrap()
            .0
    );
}
#[tokio::test]
async fn reminder_followup_cancellation_unrelated_new_command_expiry_session_thread_and_restore_fences()
 {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    at(&s, BASE).await;
    for unrelated in [
        "취소",
        "never mind",
        "오늘 날씨 어때?",
        "translate: stretch",
        "send an email to Sam",
        "내일 뉴스 조사해줘",
    ] {
        let t = thread(&s, &u).await;
        turn(&s, &u, &t, "remind me in 10 minutes").await;
        ignored(&s, &u, &t, unrelated).await;
        ignored(&s, &u, &t, "stretch").await;
    }
    assert!(s.schedules(&u).await.unwrap().is_empty());
    for text in [
        "번역: 물 마시라고 알려줘",
        "문서 내용: 물 마시라고 알려줘",
        "인용문 물 마시라고 알려줘",
        "'물 마시라고 알려줘",
    ] {
        let t = thread(&s, &u).await;
        ignored(&s, &u, &t, text).await;
        ignored(&s, &u, &t, "in 10 minutes").await;
    }
    assert!(s.schedules(&u).await.unwrap().is_empty());
    let foreign = crate::tests::member(&s, &u, "synthetic-other-reminder@example.test")
        .await
        .user;
    let owned_thread = thread(&s, &u).await;
    turn(&s, &u, &owned_thread, "remind me to stretch").await;
    assert!(
        send(
            &s,
            &foreign,
            &owned_thread,
            &Uuid::new_v4().to_string(),
            "in 10 minutes",
            "UTC"
        )
        .await
        .is_err()
    );
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me to stretch").await;
    let other = thread(&s, &u).await;
    ignored(&s, &u, &other, "in 10 minutes").await;
    let other_session = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    ignored(&s, &other_session, &t, "in 10 minutes").await;
    ignored(&s, &u, &t, "in 10 minutes").await;
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me to stretch").await;
    at(&s, BASE + 901).await;
    ignored(&s, &u, &t, "in 10 minutes").await;
    at(&s, BASE).await;
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me to stretch").await;
    let independent = turn(&s, &u, &t, "remind me in 20 minutes to drink water").await;
    assert!(independent.run.schedule_receipts.is_some());
    ignored(&s, &u, &t, "in 10 minutes").await;
    assert_eq!(s.schedules(&u).await.unwrap().len(), 1);
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me to stretch").await;
    s.call(|db| super::super::super::quarantine_for_restore(&db.conn))
        .await
        .unwrap();
    ignored(&s, &u, &t, "in 10 minutes").await;
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me to stretch").await;
    s.delete_thread(&u, t).await.unwrap();
    s.call(|db|{let missing:i64=db.conn.query_row("SELECT count(*) FROM schedule_chat_drafts d WHERE NOT EXISTS(SELECT 1 FROM chat_runs r WHERE r.tenant_id=d.tenant_id AND r.id=d.run_id)",[],|r|r.get(0))?;assert_eq!(missing,0);Ok(())}).await.unwrap();
}

#[tokio::test]
async fn reminder_followup_schema50_forward_migration_rollback_fk_and_quarantine() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    s.call(|db| {
        db.conn
            .execute_batch("ALTER TABLE browser_workspaces DROP COLUMN idle_until;ALTER TABLE browser_workspaces DROP COLUMN owner_session;DROP TABLE learned_page_dispatches;DROP TABLE artifact_external_evidence;DROP TABLE artifact_external_context;DROP TABLE artifact_external_profiles;DROP TABLE mission_acceptances;DROP TABLE schedule_chat_drafts;PRAGMA user_version=50;")?;
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    at(&s, BASE).await;
    s.call(|db| {
        let version: i64 = db.conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        assert_eq!(version, crate::CURRENT_SCHEMA_VERSION);
        assert!(!db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?);
        Ok(())
    })
    .await
    .unwrap();
    let t = thread(&s, &u).await;
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER synthetic_followup_rollback BEFORE INSERT ON schedule_chat_drafts BEGIN SELECT RAISE(ABORT,'synthetic bounded rollback');END;")?;Ok(())}).await.unwrap();
    let id = Uuid::new_v4().to_string();
    assert!(
        send(&s, &u, &t, &id, "remind me to stretch", "UTC")
            .await
            .is_err()
    );
    let request = id.clone();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM chat_runs WHERE request_id=?1",
                [request],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        db.conn
            .execute_batch("DROP TRIGGER synthetic_followup_rollback;")?;
        Ok(())
    })
    .await
    .unwrap();
    let prompt = send(&s, &u, &t, &id, "remind me to stretch", "UTC")
        .await
        .unwrap();
    assert_eq!(prompt.run.state, RunState::Completed);
    let source = prompt.run.id.clone();
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER synthetic_schedule_rollback BEFORE INSERT ON schedule_chat_commands BEGIN SELECT RAISE(ABORT,'synthetic receipt rollback');END;")?;Ok(())}).await.unwrap();
    let answer = Uuid::new_v4().to_string();
    assert!(
        send(&s, &u, &t, &answer, "in 10 minutes", "UTC")
            .await
            .is_err()
    );
    assert!(s.schedules(&u).await.unwrap().is_empty());
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT pending FROM schedule_chat_drafts WHERE run_id=?1",
                [source],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        db.conn
            .execute_batch("DROP TRIGGER synthetic_schedule_rollback;")?;
        Ok(())
    })
    .await
    .unwrap();
    let done = send(&s, &u, &t, &answer, "in 10 minutes", "UTC")
        .await
        .unwrap();
    assert!(done.run.schedule_receipts.is_some());
    assert_eq!(s.schedules(&u).await.unwrap().len(), 1);
    s.delete_thread(&u, t).await.unwrap();
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM schedule_chat_drafts", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert!(!db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?);
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn reminder_followup_repair_cancel_meta_iso_and_exhaustion_do_not_expand_authority() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    at(&s, BASE).await;
    let t = thread(&s, &u).await;
    let saved = turn(&s, &u, &t, "remind me in 30 minutes to drink water").await;
    let saved_id = receipt(&saved).id.clone();
    let before = s.schedules(&u).await.unwrap();
    for cancel in ["cancel reminder", "cancel reminder.", "cancel reminder!"] {
        let t = thread(&s, &u).await;
        turn(&s, &u, &t, "remind me to stretch").await;
        let answer = turn(&s, &u, &t, cancel).await;
        assert!(answer.run.schedule_receipts.is_none());
        assert!(
            answer
                .assistant
                .iter()
                .any(|m| m.text.contains("초안을 취소"))
        );
        let after = s.schedules(&u).await.unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(after[0].id, saved_id);
        assert_eq!(after[0].revision, before[0].revision);
        assert_eq!(after[0].state, "active");
        ignored(&s, &u, &t, "in 10 minutes").await;
    }
    for meta in [
        "번역해 줘 물 마시기",
        "요약해줘 약 먹기",
        "설명해줘 물 주기",
        "문서 분석해줘 스트레칭",
    ] {
        let t = thread(&s, &u).await;
        turn(&s, &u, &t, "remind me in 10 minutes").await;
        ignored(&s, &u, &t, meta).await;
        ignored(&s, &u, &t, "stretch").await;
        assert_eq!(s.schedules(&u).await.unwrap().len(), 1);
    }
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me in 10 minutes").await;
    let literal = turn(&s, &u, &t, "내용: 번역해 줘 물 마시기").await;
    assert_eq!(
        s.schedules(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|s| s.id == receipt(&literal).id)
            .unwrap()
            .message,
        "번역해 줘 물 마시기"
    );
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "2028-01-05 9시에 약 먹으라고 알려줘").await;
    let complete = turn(&s, &u, &t, "오전").await;
    let actual = s
        .schedules(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == receipt(&complete).id)
        .unwrap();
    assert_eq!(actual.message, "약 먹으라고");
    assert_eq!(
        actual.rule,
        parser::rule("2028-01-05 오전 9시", Some("Asia/Seoul"), BASE)
            .unwrap()
            .0
    );
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me tomorrow at 9").await;
    let first = turn(&s, &u, &t, "AM").await;
    assert!(first.assistant.iter().any(|m| m.text.contains("어떤 문구")));
    let done = turn(&s, &u, &t, "stretch").await;
    assert!(done.run.schedule_receipts.is_some());
    // Seed the persisted answer-count boundary to verify that one last answer
    // may complete, but cannot create an unanswerable new question.
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me in 10 minutes").await;
    let run = s
        .conversation(&u, t.clone(), None)
        .await
        .unwrap()
        .turns
        .pop()
        .unwrap()
        .run
        .id;
    s.call(move |db| {
        let raw: String = db.conn.query_row(
            "SELECT draft FROM schedule_chat_drafts WHERE run_id=?1",
            [&run],
            |r| r.get(0),
        )?;
        let mut d: Draft = decode(&raw)?;
        d.answers = 1;
        db.conn.execute(
            "UPDATE schedule_chat_drafts SET draft=?1 WHERE run_id=?2",
            params![encode(&d)?, run],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let done = turn(&s, &u, &t, "stretch").await;
    assert!(done.run.schedule_receipts.is_some());
    let t = thread(&s, &u).await;
    turn(&s, &u, &t, "remind me at 09:00").await;
    let run = s
        .conversation(&u, t.clone(), None)
        .await
        .unwrap()
        .turns
        .pop()
        .unwrap()
        .run
        .id;
    s.call(move |db| {
        let raw: String = db.conn.query_row(
            "SELECT draft FROM schedule_chat_drafts WHERE run_id=?1",
            [&run],
            |r| r.get(0),
        )?;
        let mut d: Draft = decode(&raw)?;
        d.answers = 1;
        db.conn.execute(
            "UPDATE schedule_chat_drafts SET draft=?1 WHERE run_id=?2",
            params![encode(&d)?, run],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let ended = turn(&s, &u, &t, "tomorrow").await;
    assert!(ended.run.schedule_receipts.is_none());
    assert!(ended.assistant.iter().any(|m| m.text.contains("새 요청")));
    assert!(!ended.assistant.iter().any(|m| m.text.contains("어떤 문구")));
    ignored(&s, &u, &t, "stretch").await;
}

#[tokio::test]
async fn conversational_civil_time_reminder_outcomes_followup_reopen_timezone_and_dst_oracles() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = configured(&s).await;
    let anchor = chrono::DateTime::parse_from_rfc3339("2026-12-31T15:30:00Z")
        .unwrap()
        .timestamp();
    at(&s, anchor).await;
    // Fixed external calendar oracle: Seoul is Jan1 (Friday), LA is Dec31 (Thursday).
    for (command, zone, utc) in [
        (
            "내일 오전9:30에 60분 동안 물 마시라고 알려줘",
            "Asia/Seoul",
            "2027-01-02T00:30:00Z",
        ),
        (
            "모레 오후 세 시 반에 물 마시라고 알려줘",
            "Asia/Seoul",
            "2027-01-03T06:30:00Z",
        ),
        (
            "다음 주 금요일 오전 아홉 시 십오 분에 합성 자료 확인하라고 알려줘",
            "Asia/Seoul",
            "2027-01-08T00:15:00Z",
        ),
        (
            "remind me this Friday at 9:30AM to stretch",
            "America/Los_Angeles",
            "2027-01-01T17:30:00Z",
        ),
        (
            "remind me day after tomorrow at midnight to stretch",
            "America/Los_Angeles",
            "2027-01-02T08:00:00Z",
        ),
    ] {
        let t = thread(&s, &u).await;
        let done = send(&s, &u, &t, &Uuid::new_v4().to_string(), command, zone)
            .await
            .unwrap();
        assert_eq!(done.run.state, RunState::Completed, "{command}");
        let r = receipt(&done);
        let saved = s
            .schedules(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == r.id)
            .unwrap();
        if command == "내일 오전9:30에 60분 동안 물 마시라고 알려줘" {
            assert_eq!(saved.message, "60분 동안 물 마시라고");
        }
        assert_eq!(
            saved.rule,
            ScheduleRule::Once {
                at: chrono::DateTime::parse_from_rfc3339(utc)
                    .unwrap()
                    .timestamp()
            },
            "{command}"
        );
    }
    // A supplied native-word clock keeps its original weekday/date while AM/PM is answered.
    let t = thread(&s, &u).await;
    let initial = turn(&s, &u, &t, "다음 주 금요일 아홉 시에 물 마시라고 알려줘").await;
    assert!(initial.run.schedule_receipts.is_none());
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    at(&s, anchor + 300).await;
    let id = Uuid::new_v4().to_string();
    let done = send(&s, &u, &t, &id, "오전", "America/New_York")
        .await
        .unwrap();
    assert_eq!(receipt(&done).timezone, "Asia/Seoul");
    let saved = s
        .schedules(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == receipt(&done).id)
        .unwrap();
    assert_eq!(
        saved.rule,
        ScheduleRule::Once {
            at: chrono::DateTime::parse_from_rfc3339("2027-01-08T00:00:00Z")
                .unwrap()
                .timestamp()
        }
    );
    assert_eq!(
        send(&s, &u, &t, &id, "오전", "America/New_York")
            .await
            .unwrap()
            .run
            .id,
        done.run.id
    );
    assert!(matches!(
        parser::parse("내일 날씨 알려줘", Some("Asia/Seoul"), anchor, false),
        parser::Decision::None
    ));
    assert!(matches!(
        parser::parse(
            "아침에 물 마시라고 알려줘",
            Some("Asia/Seoul"),
            anchor,
            false
        ),
        parser::Decision::Clarify(_)
    ));
    let before = s.schedules(&u).await.unwrap().len();
    for command in [
        "내일 오전9시60분 물 마시라고 알려줘",
        "내일 오전9시 육십분에 물 마시라고 알려줘",
        "내일 오전13시 물 마시라고 알려줘",
        "내일 09:60 물 마시라고 알려줘",
        "내일 오전9시 오후 물 마시라고 알려줘",
    ] {
        let t = thread(&s, &u).await;
        let result = send(
            &s,
            &u,
            &t,
            &Uuid::new_v4().to_string(),
            command,
            "Asia/Seoul",
        )
        .await
        .unwrap();
        assert!(result.run.schedule_receipts.is_none(), "{command}");
        assert_eq!(result.run.state, RunState::Completed, "{command}");
    }
    for (base, command) in [
        (
            "2027-03-13T12:00:00Z",
            "remind me tomorrow at 2:30AM to stretch",
        ),
        (
            "2027-11-06T12:00:00Z",
            "remind me tomorrow at 1:30AM to stretch",
        ),
        (
            "2026-12-31T15:30:00Z",
            "remind me next Friday at 9AM to stretch",
        ),
        (
            "2026-12-31T15:30:00Z",
            "remind me next week in the morning to stretch",
        ),
    ] {
        at(
            &s,
            chrono::DateTime::parse_from_rfc3339(base)
                .unwrap()
                .timestamp(),
        )
        .await;
        let t = thread(&s, &u).await;
        let result = send(
            &s,
            &u,
            &t,
            &Uuid::new_v4().to_string(),
            command,
            "America/Los_Angeles",
        )
        .await
        .unwrap();
        assert!(result.run.schedule_receipts.is_none(), "{command}");
        assert_eq!(result.run.state, RunState::Completed);
    }
    assert_eq!(s.schedules(&u).await.unwrap().len(), before);
    assert!(s.claim_agent_run().await.unwrap().is_none()); // zero model admission for reminders/clarifications.
}
