//! Synthetic actual Store authority, rollback, paging and receipt tests.
use super::*;
use crate::{
    ExternalToolStart, KnowledgeToolInput, KnowledgeToolResult, PendingKnowledgeCall,
    PrivateCheckpoint, RunLease, StepLease,
};
pub(super) async fn setup() -> (tempfile::TempDir, Store, UserContext, CalendarConnection) {
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let provider = s
        .create_provider(
            &u,
            crate::ProviderChange {
                name: "Synthetic calendar".into(),
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
    s.record_model_probe(&u, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.record_tool_probe(&u, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &u,
        Some(ModelSelection {
            source: Default::default(),
            provider_id: provider.id,
            provider_revision: 1,
            model: "synthetic".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
    let c = connect(&s, &u).await;
    (d, s, u, c)
}
pub(super) fn decision(text: &str) -> CalendarChatDecision {
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
pub(super) async fn source(
    s: &Store,
    u: &UserContext,
    text: &str,
    thread: Option<String>,
) -> (String, RunLease) {
    let thread = match thread {
        Some(t) => t,
        None => {
            s.create_thread(u, "Synthetic Calendar conversation".into())
                .await
                .unwrap()
                .id
        }
    };
    s.submit_turn_with_schedule_context(
        u,
        thread.clone(),
        Uuid::new_v4().to_string(),
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
    (thread, claim.lease)
}
pub(super) async fn commit(s: &Store, l: &RunLease, d: &CalendarChatDecision) -> StepLease {
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
const TEXT: &str = "Create Synthetic meeting on 2026-12-01 from 10:00 to 11:00";
#[tokio::test]
async fn calendar_schema30_model_ack_and_effect_admission_are_atomic_with_one_original_uuid() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = setup().await;
    let (_, l) = source(&s, &u, TEXT, None).await;
    let ctx = s.agent_context(&l).await.unwrap();
    let step = s.begin_agent_step(&l, ctx.checkpoint_seq).await.unwrap();
    let bytes = serde_json::json!({"kind":"mission_chat_decision","decision":decision(TEXT)})
        .to_string()
        .into_bytes();
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_cal_plan BEFORE INSERT ON calendar_command_plans BEGIN SELECT RAISE(ABORT,'synthetic');END;")?;Ok(())}).await.unwrap();
    assert!(
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: bytes.clone()
            },
            vec![]
        )
        .await
        .is_err()
    );
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM calendar_command_plans", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM agent_steps", [], |r| r
                    .get::<_, String>(0))?,
            "dispatched"
        );
        db.conn.execute_batch("DROP TRIGGER fail_cal_plan;")?;
        Ok(())
    })
    .await
    .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: bytes.clone(),
        },
        vec![],
    )
    .await
    .unwrap();
    let id = s
        .calendar_agent_command(&l)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    s.commit_agent_step(&step, PrivateCheckpoint { bytes }, vec![])
        .await
        .unwrap();
    assert_eq!(
        s.calendar_agent_command(&l)
            .await
            .unwrap()
            .unwrap()
            .action_id,
        id
    );
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_cal_link BEFORE INSERT ON calendar_agent_action_links BEGIN SELECT RAISE(ABORT,'synthetic');END;")?;Ok(())}).await.unwrap();
    assert!(
        s.admit_calendar_agent_change(&l, &b, primary(), None)
            .await
            .is_err()
    );
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
    s.call(|db| {
        db.conn.execute_batch("DROP TRIGGER fail_cal_link;")?;
        Ok(())
    })
    .await
    .unwrap();
    let CalendarAdmission::Dispatch(d) = s
        .admit_calendar_agent_change(&l, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!("expected first dispatch")
    };
    assert_eq!(d.change.request_id, id);
    let CalendarMutation::Create { fields } = &d.change.mutation else {
        panic!()
    };
    assert_eq!(fields.start, "2026-12-01T01:00:00+00:00");
    assert_eq!(fields.end, "2026-12-01T02:00:00+00:00");
    assert!(
        s.admit_calendar_agent_change(&l, &b, primary(), None)
            .await
            .is_err()
    );
    assert_eq!(s.calendar_actions(&u).await.unwrap().len(), 1);
    s.settle_calendar_change(
        &d,
        "outcome_unknown".into(),
        None,
        "synthetic_lost_ack".into(),
        None,
    )
    .await
    .unwrap();
    assert!(s.finish_calendar_chat(&l, None).await.unwrap());
}
#[tokio::test]
async fn calendar_missing_end_never_invents_duration_and_partial_or_duplicate_target_never_authorizes()
 {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = setup().await;
    let (_, l) = source(&s, &u, TEXT, None).await;
    let mut d = decision(TEXT);
    d.end = None;
    d.end_quote = None;
    commit(&s, &l, &d).await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    assert!(matches!(
        s.admit_calendar_agent_change(&l, &b, primary(), None).await,
        Err(StoreError::InvalidInput)
    ));
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
    s.finish_calendar_chat(&l, Some("clarify".into()))
        .await
        .unwrap();
    let text = "Delete Synthetic meeting on 2026-12-01";
    let (_, l) = source(&s, &u, text, None).await;
    let mut d = decision(text);
    d.operation = Some("delete".into());
    d.title_quote = None;
    d.target_quote = Some("Synthetic meeting".into());
    d.start = None;
    d.start_quote = None;
    d.end = None;
    d.end_quote = None;
    commit(&s, &l, &d).await;
    let mut e = CalendarEvent {
        id: "target1".into(),
        etag: "\"one\"".into(),
        status: "confirmed".into(),
        fields: fields(),
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    };
    e.fields.title = "Synthetic meeting".into();
    e.fields.start = "2026-12-01T10:00:00+09:00".into();
    for page in [
        CalendarEvents {
            events: vec![e.clone()],
            partial: true,
            next_page_token: Some("unseen-second-match".into()),
            primary_id: primary().id,
        },
        CalendarEvents {
            events: vec![e.clone(), e.clone()],
            partial: false,
            next_page_token: None,
            primary_id: primary().id,
        },
    ] {
        assert!(s.calendar_agent_target(&l, page).await.is_err());
    }
    assert!(
        s.admit_calendar_agent_change(&l, &b, primary(), Some(e))
            .await
            .is_err()
    );
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
}
async fn read(
    s: &Store,
    l: &RunLease,
    q: CalendarReadRequest,
) -> (StepLease, crate::ExternalToolPermit) {
    s.skip_mission_chat(l).await.unwrap();
    let ctx = s.agent_context(l).await.unwrap();
    let step = s.begin_agent_step(l, ctx.checkpoint_seq).await.unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"kind":"calendar_read"}"#.to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "calendar-read".into(),
            input: KnowledgeToolInput::External {
                request: ExternalToolRequest::Calendar(q),
            },
        }],
    )
    .await
    .unwrap();
    let ExternalToolStart::Dispatch(p) = s
        .prepare_external_tool(&step, "calendar-read".into())
        .await
        .unwrap()
    else {
        panic!("read not admitted")
    };
    (step, p)
}
#[tokio::test]
async fn calendar_observation_handles_paging_and_inherited_withdrawal_are_owner_bound() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = setup().await;
    let (thread, l) = source(&s, &u, "What is on my calendar?", None).await;
    let range = CalendarRange {
        connection_id: c.id.clone(),
        start: "2026-12-01T00:00:00Z".into(),
        end: "2026-12-02T00:00:00Z".into(),
        page_token: None,
    };
    let (_step, permit) = read(
        &s,
        &l,
        CalendarReadRequest::List {
            range: range.clone(),
        },
    )
    .await;
    let e = CalendarEvent {
        id: "synthetic1".into(),
        etag: "\"e\"".into(),
        status: "confirmed".into(),
        fields: fields(),
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    };
    let result = s
        .complete_external_tool(
            &permit,
            KnowledgeToolResult::External(
                serde_json::to_value(CalendarEvents {
                    events: vec![e],
                    partial: true,
                    next_page_token: Some("real-next".into()),
                    primary_id: primary().id,
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
    let KnowledgeToolResult::External(v) = result else {
        panic!()
    };
    let handle = v["events"][0]["observation_ref"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        s.calendar_read_event_id(&l, handle.clone())
            .await
            .unwrap()
            .1,
        "synthetic1"
    );
    assert!(s.calendar_read_event_id(&l, "forged".into()).await.is_err());
    let tenant = u.tenant.0.clone();
    let run = l.run_id().to_owned();
    let mut real = range.clone();
    real.page_token = Some("real-next".into());
    let mut forged = range;
    forged.page_token = Some("invented-next".into());
    s.call(move |db| {
        let r = crate::chat::load_run(&db.conn, &tenant, &run)?;
        assert!(crate::calendar::agent::allowed(
            &db.conn,
            &r,
            &CalendarReadRequest::List { range: real }
        )?);
        assert!(!crate::calendar::agent::allowed(
            &db.conn,
            &r,
            &CalendarReadRequest::List { range: forged }
        )?);
        Ok(())
    })
    .await
    .unwrap();
    let ctx = s.agent_context(&l).await.unwrap();
    let final_step = s.begin_agent_step(&l, ctx.checkpoint_seq).await.unwrap();
    s.commit_agent_step(
        &final_step,
        PrivateCheckpoint {
            bytes: br#"{"kind":"calendar_final"}"#.to_vec(),
        },
        vec![],
    )
    .await
    .unwrap();
    s.append_agent_output(
        &final_step,
        vec![crate::OutputPatch::Append {
            item: "answer".into(),
            text: "Synthetic calendar-derived answer".into(),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.finish_agent_run(&final_step, RunState::Completed, None)
        .await
        .unwrap();
    let (_, later) = source(&s, &u, "Explain that", Some(thread.clone())).await;
    assert!(s.calendar_persistence_context(&later).await.unwrap());
    assert!(s.calendar_read_event_id(&later, handle).await.is_err());
    s.disconnect_calendar(&u, c.id).await.unwrap();
    let page = s.conversation(&u, thread, None).await.unwrap();
    assert!(page.turns.iter().all(|t| t.assistant.is_empty()));
    assert!(s.check_run(&later).await.is_err());
}
#[tokio::test]
async fn calendar_logout_preserves_completed_history_and_never_revives_unapplied_command() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, c) = setup().await;
    let (thread, l) = source(&s, &u, TEXT, None).await;
    commit(&s, &l, &decision(TEXT)).await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    let CalendarAdmission::Dispatch(dispatch) = s
        .admit_calendar_agent_change(&l, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    s.settle_calendar_change(
        &dispatch,
        "applied".into(),
        None,
        "synthetic-receipt".into(),
        None,
    )
    .await
    .unwrap();
    s.finish_calendar_chat(&l, None).await.unwrap();
    let (_, pending) = source(&s, &u, TEXT, Some(thread.clone())).await;
    commit(&s, &pending, &decision(TEXT)).await;
    let id = s
        .calendar_agent_command(&pending)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    s.logout(&u).await.unwrap();
    let fresh = s
        .login(u.account.email.clone(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(s.calendar_agent_command(&pending).await.is_err());
    assert!(s.calendar_binding(&u, c.id, app()).await.is_err());
    let page = s.conversation(&fresh, thread, None).await.unwrap();
    assert!(
        page.turns[0]
            .assistant
            .iter()
            .any(|m| m.text.contains("Calendar change applied"))
    );
    assert_eq!(
        page.turns[0].run.calendar_receipt.as_ref().unwrap().state,
        "applied"
    );
    assert!(
        s.calendar_actions(&fresh)
            .await
            .unwrap()
            .iter()
            .all(|a| a.id != id)
    );
}

#[tokio::test]
async fn calendar_schema30_late_migration_failure_rolls_back_and_retries() {
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u, _c) = setup().await;
    let thread = s
        .create_thread(&u, "Retained migration evidence".into())
        .await
        .unwrap();
    drop(s);
    let c = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    c.execute_batch("DROP TABLE community_catalog_cache;DROP TRIGGER chatgpt_handoff_session_logout;DROP TRIGGER chatgpt_handoff_account_epoch;DROP TRIGGER chatgpt_handoff_binding_change;DROP TRIGGER session_purpose_valid_insert;DROP TRIGGER session_purpose_immutable;ALTER TABLE sessions DROP COLUMN absolute_deadline;ALTER TABLE sessions DROP COLUMN purpose;DROP TABLE chatgpt_handoffs;DROP TRIGGER audio_authority_no_delete;DROP TRIGGER audio_authority_monotonic;DROP TABLE audio_jobs;DROP TABLE audio_selection;DROP TABLE audio_authority;DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;CREATE TABLE calendar_agent_action_links(conflict INTEGER);PRAGMA user_version=29;").unwrap();
    drop(c);
    assert!(Store::open(dir.path()).is_err());
    let c = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        29
    );
    assert_eq!(c.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'calendar_agent_%'",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM threads WHERE id=?1",
            [thread.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    c.execute_batch("DROP TABLE calendar_agent_action_links;")
        .unwrap();
    drop(c);
    drop(Store::open(dir.path()).unwrap());
    let c = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::CURRENT_SCHEMA_VERSION
    );
}

#[tokio::test]
async fn calendar_pre30_and_mission_grants_do_not_gain_tools_and_app_replacement_withdraws_but_refresh_does_not()
 {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = setup().await;
    let (_, l) = source(&s, &u, "Calendar read", None).await;
    let range = CalendarRange {
        connection_id: c.id.clone(),
        start: "2026-12-01T00:00:00Z".into(),
        end: "2026-12-02T00:00:00Z".into(),
        page_token: None,
    };
    let q = CalendarReadRequest::List { range };
    let input = KnowledgeToolInput::External {
        request: ExternalToolRequest::Calendar(q.clone()),
    };
    assert!(!crate::missions::tool_allowed(&input));
    let tenant = u.tenant.0.clone();
    let run = l.run_id().to_owned();
    s.call(move |db| {
        db.conn.execute(
            "DELETE FROM calendar_agent_profiles WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    s.skip_mission_chat(&l).await.unwrap();
    let ctx = s.agent_context(&l).await.unwrap();
    let step = s.begin_agent_step(&l, ctx.checkpoint_seq).await.unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "old-grant".into(),
            input,
        }],
    )
    .await
    .unwrap();
    assert!(matches!(
        s.prepare_external_tool(&step, "old-grant".into())
            .await
            .unwrap(),
        ExternalToolStart::Complete(KnowledgeToolResult::Error(_))
    ));
    s.finish_agent_run(
        &step,
        RunState::Incomplete,
        Some(crate::RunFailure::Interrupted),
    )
    .await
    .unwrap();
    let (t, l) = source(&s, &u, TEXT, None).await;
    commit(&s, &l, &decision(TEXT)).await;
    let b = s.calendar_binding(&u, c.id.clone(), app()).await.unwrap();
    let CalendarAdmission::Dispatch(d) = s
        .admit_calendar_agent_change(&l, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    s.settle_calendar_change(&d, "applied".into(), None, "synthetic".into(), None)
        .await
        .unwrap();
    s.finish_calendar_chat(&l, None).await.unwrap();
    s.refresh_calendar(&b, tokens()).await.unwrap();
    assert!(
        !s.conversation(&u, t.clone(), None).await.unwrap().turns[0]
            .assistant
            .is_empty()
    );
    s.save_google_setup(
        &u,
        0,
        moosuri_connectors::gmail::OAuthClient {
            client_id: app().client_id,
            client_secret: token("synthetic-client-secret"),
            redirect_uri: app().redirect_uri,
        },
    )
    .await
    .unwrap();
    assert!(
        s.conversation(&u, t, None).await.unwrap().turns[0]
            .assistant
            .is_empty()
    );
    assert_eq!(s.calendar_actions(&u).await.unwrap()[0].state, "applied");
}
#[tokio::test]
async fn calendar_inherited_receipt_cannot_be_persisted_as_an_artifact() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = setup().await;
    let (t, l) = source(&s, &u, TEXT, None).await;
    commit(&s, &l, &decision(TEXT)).await;
    let b = s.calendar_binding(&u, c.id, app()).await.unwrap();
    let CalendarAdmission::Dispatch(d) = s
        .admit_calendar_agent_change(&l, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    s.settle_calendar_change(&d, "applied".into(), None, "synthetic".into(), None)
        .await
        .unwrap();
    s.finish_calendar_chat(&l, None).await.unwrap();
    let (_, later) = source(&s, &u, "Save that Calendar result", Some(t)).await;
    let tenant = u.tenant.0.clone();
    let run = later.run_id().to_owned();
    s.call(move |db| {
        let r = crate::chat::load_run(&db.conn, &tenant, &run)?;
        db.conn.execute(
            "UPDATE personal_artifact_run_profiles SET planned=1 WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
        )?;
        let value = crate::artifacts::agent::tool(
            &db.conn,
            &r,
            &ArtifactToolRequest::Write {
                title: "Derived".into(),
                body: "Calendar private content".into(),
                payload: ArtifactPayload::Document,
                anchor: None,
            },
        )?;
        assert_eq!(value["error"], "external_provenance_unavailable");
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn calendar_same_session_retry_resumes_original_plan_uuid_without_new_classifier() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = setup().await;
    let (_, l) = source(&s, &u, TEXT, None).await;
    commit(&s, &l, &decision(TEXT)).await;
    let id = s
        .calendar_agent_command(&l)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    s.finish_agent_execution(
        &l,
        RunState::Incomplete,
        Some(crate::RunFailure::Interrupted),
    )
    .await
    .unwrap();
    s.retry_run(&u, l.run_id().into(), l.attempt())
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(
        s.calendar_agent_command(&claim.lease)
            .await
            .unwrap()
            .unwrap()
            .action_id,
        id
    );
    assert!(
        s.mission_chat_planning(&claim.lease)
            .await
            .unwrap()
            .is_none()
    );
    let b = s.calendar_binding(&u, c.id, app()).await.unwrap();
    let CalendarAdmission::Dispatch(d) = s
        .admit_calendar_agent_change(&claim.lease, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    assert_eq!(d.change.request_id, id);
}

#[tokio::test]
async fn calendar_korean_shared_morning_range_preserves_explicit_duration_without_default() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = setup().await;
    let text = "2026-12-01 오전10시부터11시까지 팀 미팅 등록해 줘";
    let (_, l) = source(&s, &u, text, None).await;
    let mut d = decision(text);
    d.title_quote = Some("팀 미팅".into());
    d.start_quote = Some("오전10시".into());
    d.end_quote = Some("11시".into());
    commit(&s, &l, &d).await;
    let b = s.calendar_binding(&u, c.id, app()).await.unwrap();
    let CalendarAdmission::Dispatch(d) = s
        .admit_calendar_agent_change(&l, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    let CalendarMutation::Create { fields } = d.change.mutation else {
        panic!()
    };
    assert_eq!(fields.start, "2026-12-01T01:00:00+00:00");
    assert_eq!(fields.end, "2026-12-01T02:00:00+00:00");
}

#[tokio::test]
async fn calendar_explicit_iana_zone_in_current_text_overrides_browser_and_is_frozen_on_ack() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u, c) = setup().await;
    let text = "Create Synthetic meeting on 2026-12-01 from 10:00 to 11:00 America/New_York";
    let (_, l) = source(&s, &u, text, None).await;
    commit(&s, &l, &decision(text)).await;
    assert_eq!(
        s.calendar_agent_command(&l)
            .await
            .unwrap()
            .unwrap()
            .profile
            .timezone
            .as_deref(),
        Some("America/New_York")
    );
    let b = s.calendar_binding(&u, c.id, app()).await.unwrap();
    let CalendarAdmission::Dispatch(d) = s
        .admit_calendar_agent_change(&l, &b, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    let CalendarMutation::Create { fields } = d.change.mutation else {
        panic!()
    };
    assert_eq!(fields.start, "2026-12-01T15:00:00+00:00");
    assert_eq!(fields.end, "2026-12-01T16:00:00+00:00");
}

#[tokio::test]
async fn calendar_cancellation_withdrawal_and_private_purge_commit_atomically_and_never_reset() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u, connection) = setup().await;
    let (_thread, lease) = source(&s, &u, TEXT, None).await;
    let step = commit(&s, &lease, &decision(TEXT)).await;
    let binding = s.calendar_binding(&u, connection.id, app()).await.unwrap();
    let CalendarAdmission::Dispatch(dispatch) = s
        .admit_calendar_agent_change(&lease, &binding, primary(), None)
        .await
        .unwrap()
    else {
        panic!()
    };
    let mut observed = event(&dispatch);
    if let CalendarMutation::Create { fields } = &dispatch.change.mutation {
        observed.fields = fields.clone();
    }
    let stored_event = json(&observed).unwrap();
    s.settle_calendar_change(
        &dispatch,
        "applied".into(),
        Some(observed),
        "minimum-receipt".into(),
        None,
    )
    .await
    .unwrap();
    s.append_agent_output(
        &step,
        vec![crate::OutputPatch::Append {
            item: "synthetic-private-output".into(),
            text: "Synthetic private Calendar result".into(),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.call(|db|{db.conn.execute_batch("CREATE TRIGGER fail_calendar_cancel BEFORE UPDATE OF withdrawn ON calendar_agent_action_origins BEGIN SELECT RAISE(ABORT,'synthetic cancellation fault');END;")?;Ok(())}).await.unwrap();
    assert!(
        s.cancel_run(&u, lease.run_id().into(), lease.attempt())
            .await
            .is_err()
    );
    let tenant = u.tenant.0.clone();
    let run = lease.run_id().to_owned();
    let id = dispatch.change.request_id.clone();
    let retained_nonce = s.call({let tenant=tenant.clone();let run=run.clone();let id=id.clone();move |db| {
        assert_eq!(db.conn.query_row("SELECT state FROM chat_runs WHERE tenant_id=?1 AND id=?2",params![tenant,run],|r|r.get::<_,String>(0))?,"running");
        assert_eq!(db.conn.query_row("SELECT withdrawn FROM calendar_agent_action_origins WHERE tenant_id=?1 AND action_id=?2",params![tenant,id],|r|r.get::<_,i64>(0))?,0);
        assert!(db.conn.query_row("SELECT payload IS NOT NULL AND event IS NOT NULL FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|r.get::<_,bool>(0))?);
        assert_eq!(db.conn.query_row("SELECT count(*) FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|r.get::<_,i64>(0))?,1);
        db.conn.execute_batch("DROP TRIGGER fail_calendar_cancel;")?;
        Ok(db.conn.query_row("SELECT nonce FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|r.get::<_,Vec<u8>>(0))?)
    }}).await.unwrap();
    let canceled = s
        .cancel_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    assert!(canceled.assistant.is_empty());
    s.call({let tenant=tenant.clone();let run=run.clone();let id=id.clone();move |db| {
        let (state,payload,event,nonce,receipt):(String,Option<String>,Option<String>,Vec<u8>,Option<String>)=db.conn.query_row("SELECT state,payload,event,nonce,receipt FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
        assert_eq!(state,"applied");assert!(payload.is_none() && event.is_none());assert_eq!(nonce,retained_nonce);assert_eq!(receipt.as_deref(),Some("minimum-receipt"));
        assert_eq!(db.conn.query_row("SELECT withdrawn FROM calendar_agent_action_origins WHERE tenant_id=?1 AND action_id=?2",params![tenant,id],|r|r.get::<_,i64>(0))?,1);
        assert!(db.conn.execute("UPDATE calendar_agent_action_origins SET withdrawn=0 WHERE tenant_id=?1 AND action_id=?2",params![tenant,id]).is_err());
        for table in ["calendar_command_plans","calendar_agent_observations","chat_output"] {assert_eq!(db.conn.query_row(&format!("SELECT count(*) FROM {table} WHERE tenant_id=?1 AND run_id=?2"),params![tenant,run],|r|r.get::<_,i64>(0))?,0);}
        assert!(db.conn.query_row("SELECT state='passed' AND decision IS NULL FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|r.get::<_,bool>(0))?);
        assert_eq!(db.conn.query_row("SELECT count(*) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND checkpoint IS NOT NULL",params![tenant,run],|r|r.get::<_,i64>(0))?,0);
        // Isolate the projection guard with a synthetic stale persisted event. Even
        // if a future buggy writer leaves it behind, the withdrawn origin hides it.
        db.conn.execute("UPDATE calendar_actions SET event=?3 WHERE tenant_id=?1 AND id=?2",params![tenant,id,stored_event])?;
        Ok(())
    }}).await.unwrap();
    let action = s.calendar_actions(&u).await.unwrap().remove(0);
    assert_eq!(action.id, id);
    assert!(action.event.is_none());
    assert_eq!(action.receipt.as_deref(), Some("minimum-receipt"));
    s.call(move |db| {
        db.conn.execute(
            "UPDATE calendar_actions SET event=NULL WHERE tenant_id=?1 AND id=?2",
            params![tenant, id],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    s.retry_run(&u, lease.run_id().into(), lease.attempt())
        .await
        .unwrap();
    let retry = s.claim_agent_run().await.unwrap().unwrap();
    assert!(
        s.calendar_agent_command(&retry.lease)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        s.mission_chat_planning(&retry.lease)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn calendar_personal_disconnect_retires_private_storage_before_late_settlement() {
    let _g = TEST_LOCK.lock().await;
    for state in ["applied", "outcome_unknown"] {
        let (_dir, s, u, connection) = setup().await;
        let (_thread, lease) = source(&s, &u, TEXT, None).await;
        commit(&s, &lease, &decision(TEXT)).await;
        let binding = s.calendar_binding(&u, connection.id, app()).await.unwrap();
        let CalendarAdmission::Dispatch(dispatch) = s
            .admit_calendar_agent_change(&lease, &binding, primary(), None)
            .await
            .unwrap()
        else {
            panic!()
        };
        let registration = s
            .begin_subscription_login(&u, None, "Synthetic missing-token metadata".into())
            .await
            .unwrap();
        let personal = registration.binding;
        let run = lease.run_id().to_owned();
        let tenant = u.tenant.0.clone();
        s.call({ let b=personal.clone(); let run=run.clone(); move |db| {
            // Metadata-only lifecycle test: no session, fake cipher or verified
            // identity is created. Signed end-to-end auth is covered in agent.
            db.conn.execute("UPDATE chatgpt_bindings SET status='active' WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![b.tenant,b.account,b.id])?;
            let raw:String=db.conn.query_row("SELECT model FROM chat_runs WHERE tenant_id=?1 AND id=?2",params![b.tenant,run],|r|r.get(0))?;
            let mut choice:serde_json::Value=serde_json::from_str(&raw).unwrap();
            choice["source"]=serde_json::json!("personal_chat_gpt");
            choice["provider_id"]=serde_json::json!(b.id);
            choice["provider_revision"]=serde_json::json!(b.revision);
            db.conn.execute("UPDATE chat_runs SET model=?3 WHERE tenant_id=?1 AND id=?2",params![b.tenant,run,json(&choice)?])?;
            Ok(())
        }}).await.unwrap();
        assert!(
            s.begin_subscription_disconnect(&u, personal.id, personal.revision)
                .await
                .unwrap()
                .is_none()
        );
        let mut observed = event(&dispatch);
        if let CalendarMutation::Create { fields } = &dispatch.change.mutation {
            observed.fields = fields.clone();
        }
        let result = s
            .settle_calendar_change(
                &dispatch,
                state.into(),
                if state == "applied" {
                    Some(observed)
                } else {
                    None
                },
                "minimum-late-receipt".into(),
                None,
            )
            .await;
        let id = dispatch.change.request_id.clone();
        s.call(move |db| {
            let raw:(bool,bool,bool,String,bool)=db.conn.query_row("SELECT a.payload IS NULL,a.event IS NULL,o.withdrawn,r.state,e.invalidated FROM calendar_actions a JOIN calendar_agent_action_origins o ON o.tenant_id=a.tenant_id AND o.action_id=a.id JOIN chat_runs r ON r.tenant_id=o.tenant_id AND r.id=o.run_id JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE a.tenant_id=?1 AND a.id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
            assert_eq!(raw,(true,true,true,"incomplete".into(),true),"raw private storage and active origin must be retired before any conversation projection");
            let ledger:(String,String)=db.conn.query_row("SELECT state,receipt FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            assert_eq!(ledger,(state.into(),"minimum-late-receipt".into()));
            assert_eq!(db.conn.query_row("SELECT count(*) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|r.get::<_,i64>(0))?,0);
            Ok(())
        }).await.unwrap();
        assert!(matches!(result, Err(StoreError::Unauthorized)));
        assert!(
            s.retry_run(&u, lease.run_id().into(), lease.attempt())
                .await
                .is_err()
        );
    }
}
