use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, member};
use moosuri_core::*;
fn secret(s: &str) -> SecretString {
    SecretString::new(s.into()).unwrap()
}
fn app() -> GoogleAppPin {
    GoogleAppPin {
        revision: None,
        client_id: "synthetic-mission-app".into(),
        redirect_uri: "http://localhost/callback".into(),
    }
}
fn tokens(name: &str, calendar: bool) -> TokenSet {
    TokenSet {
        access_token: secret(name),
        refresh_token: Some(secret("synthetic-refresh")),
        expires_in: 3600,
        scopes: Some(if calendar {
            moosuri_connectors::calendar::SCOPES
                .iter()
                .map(|s| s.to_string())
                .collect()
        } else {
            moosuri_connectors::gmail::MINIMAL_SCOPES
                .iter()
                .map(|s| s.to_string())
                .collect()
        }),
    }
}
async fn connections(s: &Store, u: &UserContext, label: &str) -> (String, String) {
    let begin = s.start_mail_oauth_with_app(u, app(), false).await.unwrap();
    let handle = s
        .capture_mail_oauth(begin.state, secret("code"))
        .await
        .unwrap();
    let e = s.claim_mail_oauth_with_app(u, handle, app()).await.unwrap();
    let m = s
        .complete_mail_oauth(e, tokens(label, false), format!("{label}@example.test"))
        .await
        .unwrap();
    let begin = s.start_calendar_oauth(u, app()).await.unwrap();
    let handle = s
        .capture_calendar_oauth(begin.state, Some(secret("code")))
        .await
        .unwrap()
        .unwrap();
    let e = s.claim_calendar_oauth(u, handle, app()).await.unwrap();
    let c = s
        .complete_calendar_oauth(
            e,
            tokens(label, true),
            CalendarInfo {
                id: format!("{label}@example.test"),
                title: label.into(),
                time_zone: "UTC".into(),
                primary: true,
                access_role: "owner".into(),
                email_notifications: false,
            },
        )
        .await
        .unwrap();
    (m.id, c.id)
}
fn input(mode: MissionMode, task: &str) -> MissionCreate {
    MissionCreate {
        task: task.into(),
        checks: vec![MissionCheck::UserReview {
            description: "Review actual briefing".into(),
        }],
        ..crate::missions::tests::input(mode)
    }
}
async fn step(
    s: &Store,
    l: &crate::RunLease,
    calls: Vec<crate::PendingKnowledgeCall>,
) -> crate::StepLease {
    let context = s.agent_context(l).await.unwrap();
    let st = s.begin_agent_step(l, context.checkpoint_seq).await.unwrap();
    s.commit_agent_step(
        &st,
        crate::PrivateCheckpoint {
            bytes: br#"{"kind":"mission_receipt_checkpoint","private":"synthetic"}"#.to_vec(),
        },
        calls,
    )
    .await
    .unwrap();
    st
}
fn call(id: &str, q: ExternalToolRequest) -> crate::PendingKnowledgeCall {
    crate::PendingKnowledgeCall {
        call_id: id.into(),
        input: KnowledgeToolInput::External { request: q },
    }
}
fn search(id: &str) -> ExternalToolRequest {
    ExternalToolRequest::Mail(MailToolRequest::Search {
        connection_id: id.into(),
        query: "newer_than:1d".into(),
        page_token: None,
    })
}
async fn permit(s: &Store, st: &crate::StepLease, id: &str) -> crate::ExternalToolPermit {
    match s.prepare_external_tool(st, id.into()).await.unwrap() {
        crate::ExternalToolStart::Dispatch(p) => p,
        _ => panic!("expected a read permit"),
    }
}
#[tokio::test]
async fn connected_mission_owner_grant_survives_logout_but_session_only_does_not_and_refresh_is_cas()
 {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (m, c) = connections(&s, &u, "owner").await;
    let foreign = member(&s, &u, "mission-decoy@example.test").await.user;
    let (fm, _) = connections(&s, &foreign, "foreign").await;
    s.create_mission(
        &u,
        input(MissionMode::Durable, "Read my Gmail and Calendar briefing"),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![
            call("own", search(&m)),
            call("foreign", search(&fm)),
            call(
                "invented",
                ExternalToolRequest::Mail(MailToolRequest::Read {
                    connection_id: m.clone(),
                    message_id: "foreign-message".into(),
                }),
            ),
        ],
    )
    .await;
    s.logout(&u).await.unwrap();
    s.check_run(&claim.lease).await.unwrap();
    let p = permit(&s, &st, "own").await;
    assert!(p.user().is_none());
    let credentials = s.mission_read_credentials(&p, app()).await.unwrap();
    assert_eq!(credentials.access.expose(), "owner");
    assert_eq!(credentials.revision, 1);
    for id in ["foreign", "invented"] {
        assert!(matches!(
            s.prepare_external_tool(&st, id.into()).await.unwrap(),
            crate::ExternalToolStart::Complete(KnowledgeToolResult::Error(_))
        ));
    }
    let mut wrong = app();
    wrong.client_id = "foreign-app".into();
    assert!(s.mission_read_credentials(&p, wrong).await.is_err());
    s.refresh_mission_read(&p, app(), 1, tokens("owner-refreshed", false))
        .await
        .unwrap();
    assert!(matches!(
        s.refresh_mission_read(&p, app(), 1, tokens("stale", false))
            .await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        s.mission_read_credentials(&p, app())
            .await
            .unwrap()
            .access
            .expose(),
        "owner-refreshed"
    );
    let fresh = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    s.disconnect_mail(&fresh, m).await.unwrap();
    assert!(s.check_external_tool(&p).await.is_err());
    assert!(s.mission_read_credentials(&p, app()).await.is_err());
    assert!(
        s.complete_external_tool(
            &p,
            KnowledgeToolResult::External(serde_json::json!({"private":"late"}))
        )
        .await
        .is_err()
    );
    let tenant = fresh.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    s.call(move|db|{let n:i64=db.conn.query_row("SELECT count(*) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND checkpoint IS NOT NULL",params![tenant,run],|r|r.get(0))?;assert_eq!(n,0);Ok(())}).await.unwrap();
    let current = s
        .create_mission(
            &fresh,
            input(MissionMode::CurrentSession, "Read my Calendar events"),
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![call(
            "calendar",
            ExternalToolRequest::Calendar(CalendarReadRequest::List {
                range: CalendarRange {
                    connection_id: c.clone(),
                    start: "2026-11-14T00:00:00Z".into(),
                    end: "2026-11-15T00:00:00Z".into(),
                    page_token: None,
                },
            }),
        )],
    )
    .await;
    let paused = s
        .control_mission(
            &fresh,
            current.id.clone(),
            MissionControl {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: current.revision,
                action: MissionAction::Pause,
            },
        )
        .await
        .unwrap();
    assert!(!paused.uncertain);
    s.control_mission(
        &fresh,
        current.id,
        MissionControl {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: paused.revision,
            action: MissionAction::Resume,
        },
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    s.check_run(&claim.lease).await.unwrap();
    assert!(
        s.prepare_external_tool(&st, "calendar".into())
            .await
            .is_err()
    );
    let st = step(
        &s,
        &claim.lease,
        vec![call(
            "calendar",
            ExternalToolRequest::Calendar(CalendarReadRequest::List {
                range: CalendarRange {
                    connection_id: c,
                    start: "2026-11-14T00:00:00Z".into(),
                    end: "2026-11-15T00:00:00Z".into(),
                    page_token: None,
                },
            }),
        )],
    )
    .await;
    s.logout(&fresh).await.unwrap();
    assert!(s.check_run(&claim.lease).await.is_err());
    let _new = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(
        s.prepare_external_tool(&st, "calendar".into())
            .await
            .is_err()
    );
}
#[tokio::test]
async fn connected_mission_missing_domains_and_recurring_withdrawal_never_reduce_or_repin_the_task()
{
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    assert!(matches!(
        s.create_mission(&u, input(MissionMode::Durable, "Read Gmail and Calendar"))
            .await,
        Err(StoreError::Unavailable)
    ));
    assert!(s.missions(&u).await.unwrap().is_empty());
    let (m, _) = connections(&s, &u, "owner").await;
    let at = now().unwrap() + 60;
    let schedule = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: input(MissionMode::Durable, "Read Gmail and Calendar every day"),
                schedule: ScheduleDraft {
                    title: "Daily connected briefing".into(),
                    message: String::new(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: true,
            },
        )
        .await
        .unwrap();
    s.logout(&u).await.unwrap();
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    s.check_run(&claim.lease).await.unwrap();
    assert!(claim.tool_profile.as_ref().unwrap().mission_reads.is_some());
    assert_eq!(claim.calendar_profile.as_ref().unwrap().reference, at);
    let fresh = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    s.disconnect_mail(&fresh, m).await.unwrap();
    assert!(s.check_run(&claim.lease).await.is_err());
    s.enqueue_scheduled_reminders_at(at + 60).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at + 60).await.unwrap(), 0);
    let view = s.recurring_missions(&fresh).await.unwrap().remove(0);
    assert_eq!(view.schedule.state, "paused");
    assert_eq!(
        view.schedule.reason.as_deref(),
        Some("connection_unavailable")
    );
    connections(&s, &fresh, "owner").await;
    assert!(
        s.control_recurring_mission(
            &fresh,
            schedule.schedule.id,
            ScheduleControl {
                expected_revision: view.schedule.revision,
                action: "resume".into()
            }
        )
        .await
        .is_err()
    );
    assert_eq!(s.poll_recurring_missions_at(at + 120).await.unwrap(), 0);
}
#[tokio::test]
async fn connected_mission_pause_keeps_dispatch_identity_but_removes_private_checkpoint_and_blocks_late_result()
 {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (m, _) = connections(&s, &u, "owner").await;
    let mission = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail briefing"))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(&s, &claim.lease, vec![call("read", search(&m))]).await;
    let p = permit(&s, &st, "read").await;
    s.control_mission(
        &u,
        mission.id.clone(),
        MissionControl {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: mission.revision,
            action: MissionAction::Pause,
        },
    )
    .await
    .unwrap();
    assert!(s.check_external_tool(&p).await.is_err());
    assert!(
        s.complete_external_tool(
            &p,
            KnowledgeToolResult::External(serde_json::json!({"private":"late"}))
        )
        .await
        .is_err()
    );
    let view = s.missions(&u).await.unwrap().remove(0);
    assert!(view.uncertain && view.result.is_none());
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    s.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started'",params![tenant,run],|r|r.get::<_,i64>(0))?,1);assert_eq!(db.conn.query_row("SELECT count(*) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND checkpoint IS NOT NULL",params![tenant,run],|r|r.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    assert!(!tool_allowed(&KnowledgeToolInput::External {
        request: ExternalToolRequest::Calendar(CalendarReadRequest::Status {
            action_id: Uuid::new_v4().to_string()
        })
    }));
}

#[tokio::test]
async fn gmail_attachment_mission_uses_observed_message_and_real_owner_grant_without_session() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (connection, _) = connections(&s, &u, "owner").await;
    s.create_mission(
        &u,
        input(MissionMode::Durable, "Read my Gmail attachment briefing"),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let attachment = |message: &str| {
        ExternalToolRequest::Mail(MailToolRequest::Attachment {
            connection_id: connection.clone(),
            message_id: message.into(),
            part_id: "1".into(),
            attachment_id: Some("a1".into()),
            start_byte: 0,
            max_bytes: 4096,
            expected_digest: None,
        })
    };
    let first = step(
        &s,
        &claim.lease,
        vec![
            call("search", search(&connection)),
            call("invented", attachment("invented")),
        ],
    )
    .await;
    assert!(matches!(
        s.prepare_external_tool(&first, "invented".into())
            .await
            .unwrap(),
        crate::ExternalToolStart::Complete(KnowledgeToolResult::Error(_))
    ));
    let search = permit(&s, &first, "search").await;
    s.complete_external_tool(
        &search,
        KnowledgeToolResult::External(
            serde_json::json!({"messages":[{"id":"m1","threadId":"t1"}]}),
        ),
    )
    .await
    .unwrap();
    let next = step(
        &s,
        &claim.lease,
        vec![
            call("attachment", attachment("m1")),
            call(
                "list",
                ExternalToolRequest::Mail(MailToolRequest::Attachments {
                    connection_id: connection.clone(),
                    message_id: "m1".into(),
                    start_index: 0,
                }),
            ),
        ],
    )
    .await;
    s.logout(&u).await.unwrap();
    let read = permit(&s, &next, "attachment").await;
    let list = permit(&s, &next, "list").await;
    assert!(read.user().is_none() && list.user().is_none());
    assert_eq!(
        s.mission_read_credentials(&read, app())
            .await
            .unwrap()
            .access
            .expose(),
        "owner"
    );
    s.complete_external_tool(&read,KnowledgeToolResult::External(serde_json::json!({"status":"ok","message_id":"m1","attachment":{"part_id":"1"},"text":"Synthetic attachment","content_digest":"synthetic","text_digest":"synthetic"}))).await.unwrap();
    let fresh = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    s.disconnect_mail(&fresh, connection).await.unwrap();
    assert!(s.mission_read_credentials(&list, app()).await.is_err());
    assert!(
        s.complete_external_tool(
            &list,
            KnowledgeToolResult::External(serde_json::json!({"private":"late metadata"}))
        )
        .await
        .is_err()
    );
}
