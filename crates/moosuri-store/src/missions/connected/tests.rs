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

fn selection(m: &MissionView, binding: &str) -> MissionConnectionSelection {
    let w = m.wait.as_ref().unwrap();
    MissionConnectionSelection {
        request_id: Uuid::new_v4().to_string(),
        wait_request_id: w.request_id.clone(),
        expected_revision: m.revision,
        slot_id: w.slot_id.clone(),
        binding_id: binding.into(),
    }
}
async fn control(
    s: &Store,
    u: &UserContext,
    m: &MissionView,
    action: MissionAction,
) -> MissionView {
    s.control_mission(
        u,
        m.id.clone(),
        MissionControl {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: m.revision,
            action,
        },
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn connected_mission_waits_choose_once_retain_ready_and_promote_oldest_without_displacing_pause()
 {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (first, _) = connections(&s, &u, "first-private").await;
    let (second, _) = connections(&s, &u, "second-private").await;
    let a = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail first task"))
        .await
        .unwrap();
    let b = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail second task"))
        .await
        .unwrap();
    assert_eq!(a.state, "waiting");
    assert_eq!(b.state, "waiting");
    let choices = serde_json::to_string(&a.wait.as_ref().unwrap().choices).unwrap();
    assert!(!choices.contains("example.test"));
    assert!(!choices.contains("first-private"));
    assert!(matches!(
        s.create_mission(&u, input(MissionMode::Durable, "Read Gmail third task"))
            .await,
        Err(StoreError::Busy)
    ));
    let local = s
        .create_mission(&u, input(MissionMode::Durable, "Compute synthetic total"))
        .await
        .unwrap();
    let paused = control(&s, &u, &local, MissionAction::Pause).await;
    let choose_a = selection(&a, &first);
    let choose_b = selection(&b, &second);
    let ready_a = s
        .select_mission_connection(&u, a.id.clone(), choose_a.clone())
        .await
        .unwrap();
    let ready_b = s
        .select_mission_connection(&u, b.id.clone(), choose_b)
        .await
        .unwrap();
    assert_eq!(ready_a.wait.as_ref().unwrap().state, "ready");
    assert_eq!(ready_b.wait.as_ref().unwrap().state, "ready");
    assert!(s.claim_agent_run().await.unwrap().is_none());
    let cancel = control(&s, &u, &paused, MissionAction::Cancel).await;
    assert_eq!(cancel.state, "cancelled");
    let all = s.missions(&u).await.unwrap();
    let promoted = all.iter().find(|m| m.id == a.id).unwrap();
    let retained = all.iter().find(|m| m.id == b.id).unwrap();
    assert_eq!(promoted.state, "queued");
    assert_eq!(retained.state, "waiting");
    assert_eq!(retained.wait.as_ref().unwrap().state, "ready");
    let duplicate = s
        .select_mission_connection(&u, a.id.clone(), choose_a)
        .await
        .unwrap();
    assert_eq!(duplicate.id, a.id);
    assert!(matches!(
        s.select_mission_connection(&u, a.id.clone(), selection(&a, &second))
            .await,
        Err(StoreError::Conflict)
    ));
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(claim.lease.run_id(), a.wait.as_ref().unwrap().run_id);
    let st = step(
        &s,
        &claim.lease,
        vec![call("selected", search(crate::connection_waits::MAIL_SLOT))],
    )
    .await;
    let p = permit(&s, &st, "selected").await;
    assert_eq!(
        s.mission_read_credentials(&p, app())
            .await
            .unwrap()
            .connection_id,
        first
    );
}
#[tokio::test]
async fn connected_mission_combined_completed_read_yields_and_preserves_calendar_budget_and_schema()
{
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "bounded").await;
    let m = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Read Gmail and Calendar and calculate",
            ),
        )
        .await
        .unwrap();
    assert_eq!(m.state, "queued");
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![call("observed", search(crate::connection_waits::MAIL_SLOT))],
    )
    .await;
    let permit = permit(&s, &st, "observed").await;
    let pending = MissionConnectionWait {
        request_id: Uuid::new_v4().to_string(),
        slot_id: crate::connection_waits::MAIL_SLOT.into(),
        acceptance_revision: 1,
        cause: "vault_unlock".into(),
        operation_expires_at_ms: m.expires_at_ms - 1000,
    };
    assert!(matches!(
        s.mission_wait_for_connection(&claim.lease, pending.clone())
            .await,
        Err(StoreError::Conflict)
    ));
    s.complete_external_tool(
        &permit,
        KnowledgeToolResult::External(serde_json::json!({"messages":[{"id":"observed-message"}]})),
    )
    .await
    .unwrap();
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    let before=s.call(move|db|Ok(db.conn.query_row("SELECT r.model,a.remote_calls,a.tool_calls,a.deadline_ms,m.repairs,(SELECT profile FROM calendar_agent_profiles WHERE tenant_id=r.tenant_id AND run_id=r.id) FROM chat_runs r JOIN agent_execution a ON a.tenant_id=r.tenant_id AND a.run_id=r.id JOIN missions m ON m.tenant_id=r.tenant_id AND m.run_id=r.id WHERE r.tenant_id=?1 AND r.id=?2",params![tenant,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,String>(5)?)))?)).await.unwrap();
    let w = s
        .mission_wait_for_connection(&claim.lease, pending)
        .await
        .unwrap();
    assert_eq!(w.step, 2);
    assert_eq!(w.run_id, claim.lease.run_id());
    assert_eq!(w.expires_at_ms, m.expires_at_ms - 1000);
    assert!(s.check_run(&claim.lease).await.is_err());
    assert!(s.claim_agent_run().await.unwrap().is_none());
    let waiting = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    let resumed = s
        .select_mission_connection(&u, m.id.clone(), selection(&waiting, &mail))
        .await
        .unwrap();
    assert_eq!(resumed.state, "queued");
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(resumed.lease.run_id(), claim.lease.run_id());
    let tenant = u.tenant.0.clone();
    let run = resumed.lease.run_id().to_owned();
    let after=s.call(move|db|Ok(db.conn.query_row("SELECT r.model,a.remote_calls,a.tool_calls,a.deadline_ms,m.repairs,(SELECT profile FROM calendar_agent_profiles WHERE tenant_id=r.tenant_id AND run_id=r.id) FROM chat_runs r JOIN agent_execution a ON a.tenant_id=r.tenant_id AND a.run_id=r.id JOIN missions m ON m.tenant_id=r.tenant_id AND m.run_id=r.id WHERE r.tenant_id=?1 AND r.id=?2",params![tenant,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?,r.get::<_,i64>(4)?,r.get::<_,String>(5)?)))?)).await.unwrap();
    assert_eq!(before, after);
    let context = s.agent_context(&resumed.lease).await.unwrap();
    let next = s
        .begin_agent_step(&resumed.lease, context.checkpoint_seq)
        .await
        .unwrap();
    assert_eq!(next.step, 2);
}
#[tokio::test]
async fn connected_mission_ready_wait_expires_without_ui_and_revocation_never_selects_another_account()
 {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (first, _) = connections(&s, &u, "selected").await;
    let (_other, _) = connections(&s, &u, "not-selected").await;
    let a = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail"))
        .await
        .unwrap();
    let local = s
        .create_mission(&u, input(MissionMode::Durable, "Compute total"))
        .await
        .unwrap();
    let ready = s
        .select_mission_connection(&u, a.id.clone(), selection(&a, &first))
        .await
        .unwrap();
    assert_eq!(ready.wait.as_ref().unwrap().state, "ready");
    s.disconnect_mail(&u, first).await.unwrap();
    let paused = control(&s, &u, &local, MissionAction::Pause).await;
    control(&s, &u, &paused, MissionAction::Cancel).await;
    assert!(s.claim_agent_run().await.unwrap().is_none());
    let current = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == a.id)
        .unwrap();
    assert_ne!(current.state, "queued");
    let (second, _) = connections(&s, &u, "second-eligible").await;
    let b = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail again"))
        .await
        .unwrap();
    assert_eq!(b.state, "waiting");
    let local = s
        .create_mission(&u, input(MissionMode::Durable, "Compute total again"))
        .await
        .unwrap();
    let ready = s
        .select_mission_connection(&u, b.id.clone(), selection(&b, &second))
        .await
        .unwrap();
    assert_eq!(ready.wait.as_ref().unwrap().state, "ready");
    s.poll_recurring_missions_at(b.expires_at_ms / 1000 + 1)
        .await
        .unwrap();
    let id = b.id.clone();
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        let status: String = db.conn.query_row(
            "SELECT status FROM mission_connection_waits WHERE tenant_id=?1 AND mission_id=?2",
            params![tenant, id],
            |r| r.get(0),
        )?;
        assert_eq!(status, "expired");
        Ok(())
    })
    .await
    .unwrap();
    assert!(matches!(
        s.select_mission_connection(&u, b.id.clone(), selection(&b, &second))
            .await,
        Err(StoreError::Conflict)
    ));
    let _ = local;
}
#[tokio::test]
async fn connected_mission_explicit_account_is_not_sender_filter_and_wait_cancel_frees_capacity() {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (first, _) = connections(&s, &u, "account-a").await;
    for text in [
        "Use Gmail account account-b@example.test to read my inbox",
        "account-b@example.test 계정으로 Gmail 읽어 줘",
        "Gmail 계정은 account-b@example.test 이고 최근 메일 읽어 줘",
    ] {
        assert!(matches!(
            s.create_mission(&u, input(MissionMode::Durable, text))
                .await,
            Err(StoreError::Unavailable)
        ));
    }
    assert!(s.missions(&u).await.unwrap().is_empty());
    let sender = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Read Gmail from:account-b@example.test",
            ),
        )
        .await
        .unwrap();
    assert_eq!(sender.state, "queued");
    let run = sender.wait.as_ref().unwrap().run_id.clone();
    let tenant = u.tenant.0.clone();
    let actual = s
        .call(move |db| {
            let r = crate::chat::load_run(&db.conn, &tenant, &run)?;
            Ok(r.tool_profile.unwrap().mission_reads.unwrap().mail[0]
                .connection
                .id
                .clone())
        })
        .await
        .unwrap();
    assert_eq!(actual, first);
    control(&s, &u, &sender, MissionAction::Cancel).await;
    let (second, _) = connections(&s, &u, "account-b").await;
    let named = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Use Gmail account account-b@example.test to read my inbox",
            ),
        )
        .await
        .unwrap();
    assert_eq!(named.state, "queued");
    let run = named.wait.as_ref().unwrap().run_id.clone();
    let tenant = u.tenant.0.clone();
    let actual = s
        .call(move |db| {
            let r = crate::chat::load_run(&db.conn, &tenant, &run)?;
            Ok(r.tool_profile.unwrap().mission_reads.unwrap().mail[0]
                .connection
                .id
                .clone())
        })
        .await
        .unwrap();
    assert_eq!(actual, second);
    control(&s, &u, &named, MissionAction::Cancel).await;
    let a = s
        .create_mission(
            &u,
            input(
                MissionMode::Durable,
                "Read Gmail from:account-b@example.test",
            ),
        )
        .await
        .unwrap();
    assert_eq!(a.state, "waiting");
    assert_eq!(a.wait.as_ref().unwrap().choices.len(), 2);
    let _b = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail recent"))
        .await
        .unwrap();
    assert!(matches!(
        s.create_mission(&u, input(MissionMode::Durable, "Read Gmail extra"))
            .await,
        Err(StoreError::Busy)
    ));
    assert_eq!(
        control(&s, &u, &a, MissionAction::Cancel).await.state,
        "cancelled"
    );
    assert_eq!(
        s.create_mission(
            &u,
            input(MissionMode::Durable, "Read Gmail after cancellation")
        )
        .await
        .unwrap()
        .state,
        "waiting"
    );
}
#[tokio::test]
async fn connected_mission_acceptance_two_wait_promotes_exact_current_record_with_history_retained()
{
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "revision").await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable, "Read Gmail"))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    step(&s, &claim.lease, vec![]).await;
    let paused = control(&s, &u, &m, MissionAction::Pause).await;
    let updated = s
        .revise_mission_acceptance(
            &u,
            m.id.clone(),
            MissionAcceptanceUpdate {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: paused.revision,
                task: "Read Gmail with revised criteria".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::UserReview {
                    description: "Review the revised output".into(),
                }],
            },
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![call("fresh", search(crate::connection_waits::MAIL_SLOT))],
    )
    .await;
    let permit = permit(&s, &st, "fresh").await;
    s.complete_external_tool(
        &permit,
        KnowledgeToolResult::External(serde_json::json!({"messages":[]})),
    )
    .await
    .unwrap();
    let run = claim.lease.run_id().to_owned();
    let tenant = u.tenant.0.clone();
    let before=s.call(move|db|Ok(db.conn.query_row("SELECT remote_calls,tool_calls,deadline_ms,checkpoint_seq FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?)).await.unwrap();
    s.mission_wait_for_connection(
        &claim.lease,
        MissionConnectionWait {
            request_id: Uuid::new_v4().to_string(),
            slot_id: crate::connection_waits::MAIL_SLOT.into(),
            acceptance_revision: 2,
            cause: "connection_ready".into(),
            operation_expires_at_ms: updated.expires_at_ms,
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
    assert_eq!(waiting.wait.as_ref().unwrap().acceptance_revision, 2);
    let resumed = s
        .select_mission_connection(&u, m.id.clone(), selection(&waiting, &mail))
        .await
        .unwrap();
    assert_eq!(resumed.state, "queued");
    for _ in 0..2 {
        s.poll_recurring_missions().await.unwrap();
    }
    let run = claim.lease.run_id().to_owned();
    let tenant = u.tenant.0.clone();
    let id = m.id.clone();
    s.call(move|db|{let after=db.conn.query_row("SELECT remote_calls,tool_calls,deadline_ms,checkpoint_seq FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,i64>(3)?)))?;assert_eq!(before,after);let rows=db.conn.prepare("SELECT acceptance_revision,status FROM mission_connection_waits WHERE tenant_id=?1 AND mission_id=?2 ORDER BY acceptance_revision")?.query_map(params![tenant,id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;assert_eq!(rows,vec![(1,"resumed".into()),(2,"resumed".into())]);Ok(())}).await.unwrap();
    let next = s.claim_agent_run().await.unwrap().unwrap();
    let c = s.agent_context(&next.lease).await.unwrap();
    assert_eq!(
        s.begin_agent_step(&next.lease, c.checkpoint_seq)
            .await
            .unwrap()
            .step(),
        3
    );
}

fn mission_mail(connection: &str, task: &str) -> MissionActionRequest {
    MissionActionRequest::PrepareMail {
        intent_quote: task.into(),
        input: MailDraftInput {
            connection_id: connection.into(),
            origin_thread_id: None,
            content: MailDraftContent {
                from: "owner@example.test".into(),
                to: vec!["recipient@example.test".into()],
                cc: vec![],
                bcc: vec![],
                subject: "Synthetic compound draft".into(),
                text: "Complete synthetic body".into(),
                reply: None,
            },
        }
        .into(),
        evidence: vec![],
    }
}
fn action_call(id: &str, request: MissionActionRequest) -> crate::PendingKnowledgeCall {
    crate::PendingKnowledgeCall {
        call_id: id.into(),
        input: KnowledgeToolInput::MissionAction { request },
    }
}
#[tokio::test]
async fn compound_draft_survives_logout_has_one_identity_and_later_approval_does_not_revive_mission()
 {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "owner").await;
    let task = "Read Gmail and prepare an email draft to recipient@example.test";
    let m = s
        .create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    s.logout(&u).await.unwrap();
    let request = mission_mail(&mail, task);
    let st = step(
        &s,
        &claim.lease,
        vec![
            action_call("prepare", request.clone()),
            action_call("same-intent", request),
        ],
    )
    .await;
    let first = s
        .execute_knowledge_tool(&st, "prepare".into())
        .await
        .unwrap();
    let second = s
        .execute_knowledge_tool(&st, "same-intent".into())
        .await
        .unwrap();
    let replay = s
        .execute_knowledge_tool(&st, "prepare".into())
        .await
        .unwrap();
    let KnowledgeToolResult::External(first) = first else {
        panic!("expected actual draft")
    };
    assert_eq!(
        serde_json::to_value(second).unwrap()["value"]["action_id"],
        first["action_id"]
    );
    assert_eq!(
        serde_json::to_value(replay).unwrap()["value"]["action_id"],
        first["action_id"]
    );
    assert!(s.next_mail_dispatch().await.unwrap().is_none());
    let fresh = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let action_id = first["action_id"].as_str().unwrap().to_owned();
    let action = s.mail_action(&fresh, action_id.clone()).await.unwrap();
    let tenant = fresh.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    let before=s.call(move|db|{
        let count:i64=db.conn.query_row("SELECT count(*) FROM mail_actions WHERE tenant_id=?1",[&tenant],|r|r.get(0))?;assert_eq!(count,1);
        let counts:(i64,i64)=db.conn.query_row("SELECT remote_calls,tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?)))?;
        db.conn.execute("UPDATE missions SET expires_ms=0 WHERE tenant_id=?1 AND run_id=?2",params![tenant,run])?;
        Ok(counts)
    }).await.unwrap();
    assert!(s.check_run(&claim.lease).await.is_err());
    let preview = s.mail_action(&fresh, action_id.clone()).await.unwrap();
    assert!(preview.content.is_some());
    s.decide_mail(
        &fresh,
        action_id.clone(),
        MailApproval {
            revision: action.revision,
            digest: action.digest.clone(),
        },
        true,
    )
    .await
    .unwrap();
    let view = s
        .missions(&fresh)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    assert_eq!(view.actions[0].id, action_id);
    assert_eq!(view.actions[0].state, "approved");
    assert!(!["queued", "planning", "executing"].contains(&view.state.as_str()));
    let tenant = fresh.tenant.0;
    let run = claim.lease.run_id().to_owned();
    s.call(move |db| {
        let after: (i64, i64) = db.conn.query_row(
            "SELECT remote_calls,tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(before, after);
        Ok(())
    })
    .await
    .unwrap();
}
fn calendar_proposal(task: &str) -> MissionActionRequest {
    MissionActionRequest::Calendar{decision:serde_json::from_value(serde_json::json!({"domain":"calendar","decision":"command","operation":"create","connection_quote":null,"intent_quote":task,"title_quote":"Synthetic","target_quote":null,"date_quote":"2027-04-12","date":"2027-04-12","move_date_quote":null,"move_date":null,"all_day_quote":null,"last_date_quote":null,"last_date":null,"start_quote":"09:00","start":"09:00","end_quote":null,"end":null,"duration_quote":"30 minutes","duration_minutes":30,"clarification":null})).unwrap(),evidence:vec![]}
}
fn primary() -> CalendarInfo {
    CalendarInfo {
        id: "owner@example.test".into(),
        title: "owner".into(),
        time_zone: "UTC".into(),
        primary: true,
        access_role: "owner".into(),
        email_notifications: false,
    }
}
#[tokio::test]
async fn compound_calendar_reuses_ledger_after_missing_ack_and_settles_after_inference_expiry() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    connections(&s, &u, "owner").await;
    let task = "Read Calendar and create primary Calendar event named Synthetic on 2027-04-12 at 09:00 for 30 minutes";
    s.create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![action_call("calendar", calendar_proposal(task))],
    )
    .await;
    let permit = match s
        .prepare_mission_action(&st, "calendar".into())
        .await
        .unwrap()
    {
        crate::MissionActionStart::Dispatch(p) => p,
        _ => panic!("expected one initial action"),
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
        _ => panic!("expected first ledger admission"),
    };
    // Lost provider ACK must return the original committed receipt, never IO authority.
    assert!(
        matches!(s.prepare_mission_action(&st,"calendar".into()).await.unwrap(),crate::MissionActionStart::Complete(KnowledgeToolResult::External(v)) if v["state"]=="dispatch_committed")
    );
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE missions SET expires_ms=0 WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.check_run(&claim.lease).await.is_err());
    let CalendarMutation::Create { fields } = dispatch.change.mutation.clone() else {
        panic!("exact creation")
    };
    let event = CalendarEvent {
        id: dispatch.event_id.clone(),
        etag: "applied-etag".into(),
        status: "confirmed".into(),
        fields,
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    };
    s.settle_calendar_change(
        &dispatch,
        "applied".into(),
        Some(event),
        "synthetic-provider-receipt".into(),
        None,
    )
    .await
    .unwrap();
    let view = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(view.actions.len(), 1);
    assert_eq!(view.actions[0].state, "applied");
    assert_eq!(
        view.actions[0].provider_receipt.as_deref(),
        Some("synthetic-provider-receipt")
    );
    assert!(
        s.mission_calendar_credentials(&permit, app())
            .await
            .is_err()
    );
    assert!(
        s.calendar_actions(&u)
            .await
            .unwrap()
            .iter()
            .any(|a| a.event.is_some())
    );
    s.disconnect_calendar(&u, binding.connection.id)
        .await
        .unwrap();
    assert!(
        s.calendar_actions(&u)
            .await
            .unwrap()
            .iter()
            .all(|a| a.event.is_none())
    );
}
#[tokio::test]
async fn compound_source_words_and_old_missions_cannot_mint_action_rights() {
    let _guard = TEST_LOCK.lock().await;
    assert!(
        !crate::missions::actions::requested("Read Calendar notes about create event instructions")
            .1
    );
    assert!(!crate::missions::actions::requested("Do not create Calendar events").1);
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "owner").await;
    s.create_mission(&u, input(MissionMode::Durable, "Read Gmail messages"))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![action_call(
            "unrequested",
            mission_mail(&mail, "prepare an email draft to recipient@example.test"),
        )],
    )
    .await;
    assert!(matches!(
        s.execute_knowledge_tool(&st, "unrequested".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(_)
    ));
    assert!(s.mail_actions(&u).await.unwrap().is_empty());
}

#[tokio::test]
async fn compound_calendar_date_move_requires_unique_current_target_and_fresh_etag() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (_, calendar) = connections(&s, &u, "owner").await;
    let task = "Read Gmail and create an email draft to recipient@example.test and move Calendar event named Synthetic to 2027-04-13";
    s.create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let event = CalendarEvent {
        id: "event1".into(),
        etag: "original-etag".into(),
        status: "confirmed".into(),
        fields: CalendarFields {
            title: "Synthetic".into(),
            description: String::new(),
            location: String::new(),
            start: "2027-04-12T09:00:00Z".into(),
            end: "2027-04-12T09:45:00Z".into(),
            time_zone: Some("UTC".into()),
        },
        html_link: None,
        writable: true,
        all_day: false,
        updated: None,
    };
    let mut decision = match calendar_proposal(task) {
        MissionActionRequest::Calendar { decision, .. } => decision,
        _ => unreachable!(),
    };
    decision.operation = Some("update".into());
    decision.title_quote = None;
    decision.target_quote = Some("Synthetic".into());
    decision.date = None;
    decision.date_quote = None;
    decision.start = None;
    decision.start_quote = None;
    decision.duration_minutes = None;
    decision.duration_quote = None;
    decision.move_date = Some("2027-04-13".into());
    decision.move_date_quote = decision.move_date.clone();
    for duplicate in [true, false] {
        let read = step(
            &s,
            &claim.lease,
            vec![call(
                "list",
                ExternalToolRequest::Calendar(CalendarReadRequest::List {
                    range: CalendarRange {
                        connection_id: calendar.clone(),
                        start: "2027-04-12T00:00:00Z".into(),
                        end: "2027-04-14T00:00:00Z".into(),
                        page_token: None,
                    },
                }),
            )],
        )
        .await;
        let p = permit(&s, &read, "list").await;
        let mut events = vec![event.clone()];
        if duplicate {
            let mut other = event.clone();
            other.id = "event2".into();
            events.push(other);
        }
        s.complete_external_tool(
            &p,
            KnowledgeToolResult::External(
                serde_json::to_value(CalendarEvents {
                    events,
                    next_page_token: None,
                    partial: false,
                    primary_id: "owner@example.test".into(),
                })
                .unwrap(),
            ),
        )
        .await
        .unwrap();
        let action = MissionActionRequest::Calendar {
            decision: decision.clone(),
            evidence: vec![MissionEvidence {
                step: read.step(),
                call_id: "list".into(),
            }],
        };
        let st = step(&s, &claim.lease, vec![action_call("move", action)]).await;
        let prepared = s.prepare_mission_action(&st, "move".into()).await.unwrap();
        if duplicate {
            assert!(matches!(
                prepared,
                crate::MissionActionStart::Complete(KnowledgeToolResult::Error(_))
            ));
            continue;
        }
        let crate::MissionActionStart::Dispatch(p) = prepared else {
            panic!("one exact target")
        };
        let b = s.mission_calendar_credentials(&p, app()).await.unwrap();
        let mut stale = event.clone();
        stale.etag = "concurrent-edit".into();
        assert!(
            s.admit_mission_calendar_change(&p, &b, primary(), Some(stale))
                .await
                .is_err()
        );
        let crate::CalendarAdmission::Dispatch(dispatch) = s
            .admit_mission_calendar_change(&p, &b, primary(), Some(event.clone()))
            .await
            .unwrap()
        else {
            panic!("first exact move")
        };
        let CalendarMutation::Update {
            event_id,
            etag,
            fields,
        } = dispatch.change.mutation
        else {
            panic!("date move")
        };
        assert_eq!(event_id, "event1");
        assert_eq!(etag, "original-etag");
        assert_eq!(fields.start, "2027-04-13T09:00:00+00:00");
        assert_eq!(fields.end, "2027-04-13T09:45:00+00:00");
        let tenant = u.tenant.0.clone();
        s.call(move|db|{let counts:(i64,i64)=db.conn.query_row("SELECT count(*) FILTER(WHERE kind='update'),count(*) FILTER(WHERE kind='create') FROM calendar_actions WHERE tenant_id=?1",[tenant],|r|Ok((r.get(0)?,r.get(1)?)))?;assert_eq!(counts,(1,0),"one PATCH commitment and zero event POST commitments");Ok(())}).await.unwrap();
    }
}

#[tokio::test]
async fn compound_calendar_other_output_verbs_never_authorize_event_creation() {
    let _guard = TEST_LOCK.lock().await;
    for task in [
        "Read Calendar and create a private report about the meetings",
        "Read Calendar and create a table of Calendar events",
        "Read Calendar and create a meeting summary",
        "Read Calendar and create a Calendar event report",
        "Read Calendar and create a note about the Calendar event",
        "Read Calendar and create an email draft to recipient@example.test",
        "Read Gmail and create an email draft to recipient@example.test and move Calendar event named Synthetic to 2027-04-13",
    ] {
        let (_dir, s, u) = crate::missions::tests::setup().await;
        connections(&s, &u, "owner").await;
        s.create_mission(&u, input(MissionMode::Durable, task))
            .await
            .unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let st = step(
            &s,
            &claim.lease,
            vec![action_call("unrequested-create", calendar_proposal(task))],
        )
        .await;
        assert!(matches!(
            s.prepare_mission_action(&st, "unrequested-create".into())
                .await
                .unwrap(),
            crate::MissionActionStart::Complete(KnowledgeToolResult::Error(_))
        ));
        assert!(
            s.calendar_actions(&u).await.unwrap().is_empty(),
            "other output verbs cannot dispatch a Calendar POST"
        );
    }
}
#[tokio::test]
async fn compound_prepared_action_blocks_acceptance_reinterpretation_and_keeps_original_receipt() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, s, u) = crate::missions::tests::setup().await;
    let (mail, _) = connections(&s, &u, "owner").await;
    let task = "Read Gmail and prepare an email draft to recipient@example.test";
    let mission = s
        .create_mission(&u, input(MissionMode::Durable, task))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = step(
        &s,
        &claim.lease,
        vec![action_call("prepare", mission_mail(&mail, task))],
    )
    .await;
    let KnowledgeToolResult::External(receipt) = s
        .execute_knowledge_tool(&st, "prepare".into())
        .await
        .unwrap()
    else {
        panic!("actual prepared draft")
    };
    let before = s
        .control_mission(
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
    for revised_task in [
        "Read Gmail and prepare an email draft to other@example.test with a different body",
        "Read Gmail and return completed",
    ] {
        assert!(matches!(
            s.revise_mission_acceptance(
                &u,
                mission.id.clone(),
                MissionAcceptanceUpdate {
                    request_id: Uuid::new_v4().to_string(),
                    expected_revision: before.revision,
                    task: revised_task.into(),
                    constraints: String::new(),
                    checks: vec![MissionCheck::OutputContains {
                        text: "completed".into()
                    }]
                }
            )
            .await,
            Err(StoreError::Conflict)
        ));
        let current = s.missions(&u).await.unwrap().remove(0);
        assert_eq!(current.request.task, task);
        assert_eq!(current.state, "paused");
        assert_eq!(current.actions.len(), 1);
        assert_eq!(
            current.actions[0].id,
            receipt["action_id"].as_str().unwrap()
        );
        let actions = s.mail_actions(&u).await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(
            actions[0].content.as_ref().unwrap().to,
            ["recipient@example.test"]
        );
    }
    // Defensive verification fences historical/raced snapshots too, even if a
    // pre-fix acceptance row already exists without a current action contract.
    let tenant = u.tenant.0.clone();
    let id = mission.id;
    let run = claim.lease.run_id().to_owned();
    s.call(move |db| {
        let original: String = db.conn.query_row(
            "SELECT payload FROM missions WHERE tenant_id=?1 AND id=?2",
            params![tenant, id],
            |r| r.get(0),
        )?;
        db.conn.execute(
            "INSERT INTO mission_acceptances VALUES(?1,?2,2,?3,?4,?4,1)",
            params![tenant, id, Uuid::new_v4().to_string(), original],
        )?;
        let r = crate::chat::load_run(&db.conn, &tenant, &run)?;
        let (report, _) = crate::missions::actions::verification(&db.conn, &r)?;
        assert!(report.iter().any(|v| v.contains("different acceptance")));
        Ok(())
    })
    .await
    .unwrap();
}
