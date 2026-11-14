//! Signed personal OAuth + real plan classifier + existing counted Calendar IO.
use super::*;
use moosuri_core::*;
use moosuri_store::{PrivateCheckpoint, RunFailure};
const CREATE: &str = "Create Synthetic meeting on 2026-12-01 from 10:00 to 11:00";
const UPDATE: &str = "Update Synthetic meeting on 2026-12-01 from 11:00 to 12:00";
const DELETE: &str = "Delete Synthetic meeting on 2026-12-01";
fn final_wire(text: &str) -> String {
    let item = json!({"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":text,"annotations":[]}]});
    [json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"m","role":"assistant","content":[]}}),json!({"type":"response.content_part.added","item_id":"m","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}),json!({"type":"response.output_text.delta","item_id":"m","output_index":0,"content_index":0,"delta":text}),json!({"type":"response.output_text.done","item_id":"m","output_index":0,"content_index":0,"text":text}),json!({"type":"response.content_part.done","item_id":"m","output_index":0,"content_index":0,"part":{"type":"output_text","text":text,"annotations":[]}}),json!({"type":"response.output_item.done","output_index":0,"item":item}),json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}})].iter().map(|e|format!("data: {e}\n\n")).collect()
}
fn call_wire(name: &str, args: Value, namespace: &str) -> String {
    let args = args.to_string();
    let item = json!({"type":"function_call","id":"fc","call_id":"call-foreground","namespace":namespace,"name":name,"arguments":args,"status":"completed"});
    [json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc","call_id":"call-foreground","namespace":namespace,"name":name,"arguments":"","status":"in_progress"}}),json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc","delta":args}),json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc","arguments":args}),json!({"type":"response.output_item.done","output_index":0,"item":item}),json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}})].iter().map(|e|format!("data: {e}\n\n")).collect()
}

fn no_command() -> Value {
    json!({"decision":"no_command","action":null,"target_kind":null,"intent_quote":null,"task_quote":null,"constraints_quote":null,"time_quote":null,"target_quote":null,"instruction_quote":null,"time":null,"clarification":null})
}
fn decision(text: &str, mode: usize) -> CalendarChatDecision {
    CalendarChatDecision {
        domain: "calendar".into(),
        decision: if mode == 25 { "unsupported" } else { "command" }.into(),
        operation: if mode == 25 {
            None
        } else {
            Some(
                if mode == 21 {
                    "update"
                } else if mode == 23 {
                    "delete"
                } else {
                    "create"
                }
                .into(),
            )
        },
        connection_quote: None,
        intent_quote: if mode == 25 { None } else { Some(text.into()) },
        title_quote: if mode == 20 {
            Some("Synthetic meeting".into())
        } else {
            None
        },
        target_quote: if mode == 21 || mode == 23 {
            Some("Synthetic meeting".into())
        } else {
            None
        },
        date_quote: if mode == 25 {
            None
        } else {
            Some("2026-12-01".into())
        },
        date: if mode == 25 {
            None
        } else {
            Some("2026-12-01".into())
        },
        start_quote: if mode == 20 {
            Some("10:00".into())
        } else if mode == 21 {
            Some("11:00".into())
        } else {
            None
        },
        start: if mode == 20 {
            Some("10:00".into())
        } else if mode == 21 {
            Some("11:00".into())
        } else {
            None
        },
        end_quote: if mode == 20 {
            Some("11:00".into())
        } else if mode == 21 {
            Some("12:00".into())
        } else {
            None
        },
        end: if mode == 20 {
            Some("11:00".into())
        } else if mode == 21 {
            Some("12:00".into())
        } else {
            None
        },
        duration_quote: None,
        duration_minutes: None,
        clarification: if mode == 25 {
            Some("unsupported".into())
        } else {
            None
        },
    }
}
fn decision_wire(name: &str, args: Value) -> String {
    const PREVIEW: &str = "SYNTHETIC-PRIVATE-CALENDAR-CLASSIFIER";
    let parse = |s: &str| {
        s.lines()
            .filter_map(|l| l.strip_prefix("data: "))
            .map(|l| serde_json::from_str::<Value>(l).unwrap())
            .collect::<Vec<_>>()
    };
    let mut preview = parse(&final_wire(PREVIEW));
    preview.retain(|v| v["type"] != "response.created" && v["type"] != "response.completed");
    for v in &mut preview {
        if v["item"]["type"] == "message" {
            v["item"]["phase"] = json!("commentary");
        }
    }
    let mut frames = parse(&call_wire(name, args, "moosuri"));
    let mut message = json!({"id":"m","type":"message","role":"assistant","phase":"commentary","status":"completed","content":[{"type":"output_text","text":PREVIEW,"annotations":[]}]});
    for v in &mut frames {
        if v.get("output_index").is_some() {
            v["output_index"] = json!(1);
        }
        if v["type"] == "response.completed" {
            v["response"]["output"]
                .as_array_mut()
                .unwrap()
                .insert(0, message.take());
        }
    }
    frames.splice(1..1, preview);
    frames.iter().map(|v| format!("data: {v}\n\n")).collect()
}
pub(super) async fn response(
    s: &Provider,
    h: &axum::http::HeaderMap,
    b: &Value,
) -> axum::response::Response {
    s.models.fetch_add(1, Ordering::SeqCst);
    s.requests.lock().unwrap().push(b.clone());
    s.authorizations
        .lock()
        .unwrap()
        .push(h["authorization"].to_str().unwrap().into());
    assert_eq!(b["model"], "fixture-model");
    assert_eq!(b["store"], false);
    assert_eq!(b["stream"], true);
    assert!(b.get("max_output_tokens").is_none());
    assert!(b.get("previous_response_id").is_none());
    assert_eq!(b["tools"][0]["name"], "moosuri");
    let defs = b["tools"][0]["tools"].as_array().unwrap();
    let input = b["input"].as_array().unwrap();
    assert_eq!(input[0]["role"], "developer");
    let wire = if defs[0]["name"] == "calendar_chat_decision" {
        assert_eq!(defs.len(), 2);
        assert_eq!(
            defs[1]["parameters"]["properties"]["decision"]["enum"],
            json!(["no_command"])
        );
        assert_eq!(input.len(), 2);
        assert!(
            input[0]["content"]
                .as_str()
                .unwrap()
                .contains("PERSONAL CALENDAR-ONLY DOMAIN")
        );
        if s.hold_tool.load(Ordering::SeqCst) == 1 {
            s.tool_entered.notify_one();
            s.tool_release.notified().await;
        }
        let text = input[1]["content"].as_str().unwrap();
        let mode = s.tool_mode.load(Ordering::SeqCst);
        if mode == 22 || mode == 26 {
            let mut d = no_command();
            d["decision"] = json!("command");
            d["action"] = json!(if mode == 22 { "pause" } else { "create_now" });
            d["intent_quote"] = json!(text);
            if mode == 22 {
                d["target_kind"] = json!("one_off");
                d["target_quote"] = json!("Synthetic meeting");
            } else {
                d["task_quote"] = json!(text);
            }
            decision_wire("mission_chat_decision", d)
        } else if mode == 24 {
            decision_wire("mission_chat_decision", no_command())
        } else {
            decision_wire(
                "calendar_chat_decision",
                serde_json::to_value(decision(text, mode)).unwrap(),
            )
        }
    } else {
        final_wire("No Calendar mutation requested.")
    };
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        wire,
    )
        .into_response()
}
type Setup = (
    tempfile::TempDir,
    Store,
    UserContext,
    Fixture,
    SubscriptionService,
    LocalLoginLaunch,
    crate::calendar::CalendarService,
    CalendarConnection,
    Arc<crate::calendar::tests::FixtureIo>,
);
async fn configured() -> Setup {
    let f = fixture().await;
    let d = tempfile::tempdir().unwrap();
    let store = Store::open(d.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let admin = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &admin.user,
            "Calendar member".into(),
            "calendar-member@example.test".into(),
        )
        .await
        .unwrap();
    let member = store
        .login(
            "calendar-member@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap();
    let u = store
        .change_password(&member.user, PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert_eq!(u.account().role, Role::Member);
    let s = service(store.clone(), &f);
    let l = login(&s, &u).await;
    let c = proof(&s, &u, l.binding_id.clone(), l.revision).await;
    let (b, _) = store
        .subscription_tool_probe_binding(
            &u,
            l.binding_id.clone(),
            l.revision,
            c.model.clone(),
            c.effort.clone(),
        )
        .await
        .unwrap();
    store
        .record_subscription_tool_probe(&u, b.subscription.unwrap())
        .await
        .unwrap();
    store
        .set_model_preference(
            &u,
            Some(ModelSelection {
                source: c.source,
                provider_id: c.provider_id,
                provider_revision: c.provider_revision,
                model: c.model,
                effort: c.effort,
            }),
        )
        .await
        .unwrap();
    let (cal, conn, io) =
        crate::calendar::tests::fixture_for_owner(store.clone(), &u, s.signals.clone()).await;
    f.state.tool_mode.store(20, Ordering::SeqCst);
    (d, store, u, f, s, l, cal, conn, io)
}
async fn claim(
    store: &Store,
    u: &UserContext,
    text: &str,
) -> (String, ChatTurn, moosuri_store::AgentClaim) {
    let t = store
        .create_thread(u, "Personal Calendar".into())
        .await
        .unwrap();
    let v = store
        .submit_turn_with_schedule_context(
            u,
            t.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            text.into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let c = store.claim_agent_run().await.unwrap().unwrap();
    (t.id, v, c)
}
async fn execute(
    store: Store,
    s: SubscriptionService,
    c: moosuri_store::AgentClaim,
    cal: crate::calendar::CalendarService,
) {
    crate::runner::execute_with_calendar(
        store.clone(),
        c,
        s.signals.clone(),
        crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
        crate::web::Service::new(None),
        s,
        cal,
    )
    .await;
}
async fn ask(
    store: &Store,
    u: &UserContext,
    s: &SubscriptionService,
    cal: &crate::calendar::CalendarService,
    text: &str,
) -> ChatTurn {
    let (t, _, c) = claim(store, u, text).await;
    execute(store.clone(), s.clone(), c, cal.clone()).await;
    store
        .conversation(u, t, None)
        .await
        .unwrap()
        .turns
        .remove(0)
}
#[tokio::test]
async fn signed_personal_calendar_create_same_day_update_delete_use_typed_io_and_receipt_reload() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, _, cal, _, io) = configured().await;
    for (mode, text) in [(20, CREATE), (21, UPDATE), (23, DELETE)] {
        f.state.tool_mode.store(mode, Ordering::SeqCst);
        let done = ask(&store, &u, &s, &cal, text).await;
        assert_eq!(
            done.run.state,
            RunState::Completed,
            "error={:?}",
            done.run.error
        );
        let r = done.run.calendar_receipt.unwrap();
        assert_eq!(r.state, "applied");
        assert!(r.action_id.is_some());
        assert_eq!(done.assistant.len(), 1);
        assert_eq!(done.assistant[0].text, r.message);
        assert!(done.run.mission_receipt.is_none());
    }
    assert_eq!(io.write_count(), 3);
    assert_eq!(f.state.requests.lock().unwrap().len(), 3);
    assert!(store.missions(&u).await.unwrap().is_empty());
    assert!(store.recurring_missions(&u).await.unwrap().is_empty());
}
#[tokio::test]
async fn personal_calendar_rejects_mission_command_or_control_even_from_declared_no_command_name() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for mode in [22, 26] {
        let (_d, store, u, f, s, _, cal, _, io) = configured().await;
        f.state.tool_mode.store(mode, Ordering::SeqCst);
        let done = ask(&store, &u, &s, &cal, CREATE).await;
        assert_ne!(done.run.state, RunState::Completed);
        assert!(done.run.calendar_receipt.is_none());
        assert!(done.run.mission_receipt.is_none());
        assert_eq!(io.write_count(), 0);
        assert!(store.missions(&u).await.unwrap().is_empty());
    }
}
#[tokio::test]
async fn personal_calendar_no_command_and_unsupported_have_zero_write_without_approval() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for mode in [24, 25] {
        let (_d, store, u, f, s, _, cal, _, io) = configured().await;
        f.state.tool_mode.store(mode, Ordering::SeqCst);
        let done = ask(&store, &u, &s, &cal, CREATE).await;
        assert_eq!(done.run.state, RunState::Completed);
        assert_eq!(io.write_count(), 0);
        if mode == 25 {
            assert_eq!(done.run.calendar_receipt.unwrap().state, "unsupported");
            assert_eq!(f.state.requests.lock().unwrap().len(), 1);
        } else {
            assert!(done.run.calendar_receipt.is_none());
            assert_eq!(f.state.requests.lock().unwrap().len(), 2);
        }
    }
}
#[tokio::test]
async fn personal_calendar_current_token_refresh_and_classifier_authority_races_are_fenced() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for scenario in [0, 1, 2, 3] {
        let (_d, store, u, f, s, l, cal, conn, io) = configured().await;
        if scenario == 0 || scenario == 3 {
            store
                .expire_subscription_fixture(&u, l.binding_id.clone(), l.revision)
                .await
                .unwrap();
            if scenario == 3 {
                f.state.refresh_id.store(1, Ordering::SeqCst);
                f.state.jwks_status.store(503, Ordering::SeqCst);
            }
            let done = ask(&store, &u, &s, &cal, CREATE).await;
            assert_eq!(io.write_count(), if scenario == 0 { 1 } else { 0 });
            assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
            if scenario == 3 {
                assert_ne!(done.run.state, RunState::Completed);
                assert!(f.state.requests.lock().unwrap().is_empty());
            }
            continue;
        }
        f.state.hold_tool.store(1, Ordering::SeqCst);
        let (t, v, c) = claim(&store, &u, CREATE).await;
        let job = tokio::spawn(execute(store.clone(), s.clone(), c, cal));
        tokio::time::timeout(Duration::from_secs(5), f.state.tool_entered.notified())
            .await
            .unwrap();
        if scenario == 1 {
            store.cancel_run(&u, v.run.id, 1).await.unwrap();
        } else {
            store.disconnect_calendar(&u, conn.id).await.unwrap();
        }
        f.state.tool_release.notify_one();
        job.await.unwrap();
        assert_eq!(io.write_count(), 0);
        let done = store
            .conversation(&u, t, None)
            .await
            .unwrap()
            .turns
            .remove(0);
        assert_ne!(done.run.state, RunState::Completed);
    }
}
#[tokio::test]
async fn personal_sealed_calendar_retry_precedes_chatgpt_client_and_unknown_receipt_never_resends()
{
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, l, cal, _, io) = configured().await;
    let (t, v, c) = claim(&store, &u, CREATE).await;
    let step = store
        .begin_agent_step(&c.lease, c.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: json!({"kind":"mission_chat_decision","decision":decision(CREATE,20)})
                    .to_string()
                    .into_bytes(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .finish_agent_execution(
            &c.lease,
            RunState::OutcomeUnknown,
            Some(RunFailure::Interrupted),
        )
        .await
        .unwrap();
    store.retry_run(&u, v.run.id.clone(), 1).await.unwrap();
    store
        .expire_subscription_fixture(&u, l.binding_id, l.revision)
        .await
        .unwrap();
    io.set_write_mode(1);
    let c = store.claim_agent_run().await.unwrap().unwrap();
    let id = store
        .calendar_agent_command(&c.lease)
        .await
        .unwrap()
        .unwrap()
        .action_id;
    execute(store.clone(), s.clone(), c, cal.clone()).await;
    let done = store
        .conversation(&u, t.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.calendar_receipt.unwrap().state, "outcome_unknown");
    assert_eq!(io.write_count(), 1);
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 0);
    assert!(f.state.requests.lock().unwrap().is_empty());
    let action = store
        .calendar_actions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.id == id)
        .unwrap();
    assert_eq!(action.state, "outcome_unknown");
    cal.lookup(&u, id.clone()).await.unwrap();
    assert_eq!(io.write_count(), 1);
    assert_eq!(
        store
            .calendar_actions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|a| a.id == id)
            .unwrap()
            .state,
        "applied"
    );
}
#[tokio::test]
async fn personal_calendar_cancel_after_dispatch_retains_minimum_receipt_without_retry_write() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, _, cal, _, io) = configured().await;
    io.hold_next_mutation();
    let (t, v, c) = claim(&store, &u, CREATE).await;
    let job = tokio::spawn(execute(store.clone(), s.clone(), c, cal.clone()));
    io.reached().await;
    store.cancel_run(&u, v.run.id.clone(), 1).await.unwrap();
    io.release_mutation();
    job.await.unwrap();
    assert_eq!(io.write_count(), 1);
    store.retry_run(&u, v.run.id, 1).await.unwrap();
    f.state.tool_mode.store(24, Ordering::SeqCst);
    let c = store.claim_agent_run().await.unwrap().unwrap();
    execute(store.clone(), s, c, cal).await;
    let done = store
        .conversation(&u, t, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert!(
        done.run.calendar_receipt.is_some(),
        "minimum prior effect receipt remains visible"
    );
    assert!(
        store
            .calendar_actions(&u)
            .await
            .unwrap()
            .iter()
            .all(|a| a.event.is_none()),
        "canceled private event must stay purged"
    );
    assert_eq!(io.write_count(), 1);
    assert_eq!(
        f.state
            .requests
            .lock()
            .unwrap()
            .iter()
            .filter(|r| r["tools"][0]["tools"][0]["name"] == "calendar_chat_decision")
            .count(),
        1
    );
}

#[tokio::test]
async fn personal_selected_mission_is_rejected_and_missing_connection_has_typed_settings_guidance()
{
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, _, cal, conn, _io) = configured().await;
    let t = store
        .create_thread(&u, "No task authority".into())
        .await
        .unwrap();
    let rejected = store
        .submit_turn_with_mission_context(
            &u,
            t.id,
            uuid::Uuid::new_v4().to_string(),
            CREATE.into(),
            None,
            vec![],
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
            Some(MissionChatContext {
                target: Some(MissionChatTarget {
                    kind: "one_off".into(),
                    id: uuid::Uuid::new_v4().to_string(),
                    revision: 1,
                }),
            }),
        )
        .await;
    assert!(matches!(
        rejected,
        Err(moosuri_store::StoreError::Forbidden)
    ));
    store.disconnect_calendar(&u, conn.id).await.unwrap();
    let t = store
        .create_thread(&u, "Admitted before reconnect".into())
        .await
        .unwrap();
    store
        .submit_turn_with_schedule_context(
            &u,
            t.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            CREATE.into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let planning = store
        .mission_chat_planning(&claim.lease)
        .await
        .unwrap()
        .unwrap();
    assert!(planning.calendar_only);
    assert!(planning.calendar.unwrap().connections.is_empty());
    execute(store.clone(), s.clone(), claim, cal).await;
    let done = store
        .conversation(&u, t.id.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.state, RunState::Completed);
    let receipt = done.run.calendar_receipt.unwrap();
    assert_eq!(receipt.state, "connection_needed");
    assert!(receipt.message.contains("/settings/calendar"));
    assert!(receipt.action_id.is_none());
    assert!(store.calendar_actions(&u).await.unwrap().is_empty());
    let (_reconnected, _conn, io) =
        crate::calendar::tests::fixture_for_owner(store.clone(), &u, s.signals.clone()).await;
    assert_eq!(io.write_count(), 0);
    let reloaded = store
        .conversation(&u, t.id, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(
        reloaded.run.calendar_receipt.unwrap().state,
        "connection_needed"
    );
    assert_eq!(f.state.requests.lock().unwrap().len(), 1);
    assert!(store.missions(&u).await.unwrap().is_empty());
}

#[tokio::test]
async fn personal_calendar_disconnect_during_dispatch_keeps_only_minimum_receipt_after_reconnect() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for mode in [0, 1] {
        let (_d, store, u, f, s, l, cal, _, io) = configured().await;
        io.set_write_mode(mode);
        io.hold_next_mutation();
        let (t, v, c) = claim(&store, &u, CREATE).await;
        // Reenter through runner's sealed-command resume branch, before a new
        // ChatGPT client/classifier or its ordinary failure finalizer exists.
        let step = store
            .begin_agent_step(&c.lease, c.context.checkpoint_seq)
            .await
            .unwrap();
        store
            .commit_agent_step(
                &step,
                PrivateCheckpoint {
                    bytes: json!({"kind":"mission_chat_decision","decision":decision(CREATE,20)})
                        .to_string()
                        .into_bytes(),
                },
                vec![],
            )
            .await
            .unwrap();
        store
            .finish_agent_execution(
                &c.lease,
                RunState::OutcomeUnknown,
                Some(RunFailure::Interrupted),
            )
            .await
            .unwrap();
        store.retry_run(&u, v.run.id.clone(), 1).await.unwrap();
        let c = store.claim_agent_run().await.unwrap().unwrap();
        let job = tokio::spawn(execute(store.clone(), s.clone(), c, cal.clone()));
        io.reached().await;
        let before = store.calendar_actions(&u).await.unwrap();
        assert_eq!(before.len(), 1);
        assert_eq!(before[0].state, "dispatch_committed");
        let id = before[0].id.clone();
        store
            .begin_subscription_disconnect(&u, l.binding_id.clone(), l.revision)
            .await
            .unwrap();
        io.release_mutation();
        job.await.unwrap();
        let done = store
            .conversation(&u, t.clone(), None)
            .await
            .unwrap()
            .turns
            .remove(0);
        assert!(
            !done.run.state.active(),
            "disconnect cannot strand a running Calendar run"
        );
        assert!(
            done.assistant.is_empty(),
            "late private output must not return"
        );
        let receipt = done.run.calendar_receipt.unwrap();
        assert_eq!(receipt.action_id.as_deref(), Some(id.as_str()));
        let actions = store.calendar_actions(&u).await.unwrap();
        assert_eq!(actions.len(), 1);
        assert_eq!(actions[0].id, id);
        assert_eq!(
            actions[0].state,
            if mode == 0 {
                "applied"
            } else {
                "outcome_unknown"
            }
        );
        assert!(
            actions[0].receipt.is_some(),
            "external effect evidence must survive"
        );
        assert!(
            actions[0].event.is_none(),
            "late private event must be purged"
        );
        assert_eq!(io.write_count(), 1);
        if mode == 1 {
            assert!(
                cal.lookup(&u, id.clone()).await.is_err(),
                "withdrawn unknown payload cannot be reused for lookup"
            );
        }
        store
            .finish_subscription_disconnect(&u, l.binding_id.clone(), l.revision, false)
            .await
            .unwrap();
        let reconnected = login_binding(&s, &u, Some((l.binding_id.clone(), l.revision))).await;
        assert_eq!(reconnected.binding_id, l.binding_id);
        assert!(reconnected.revision > l.revision);
        proof(&s, &u, reconnected.binding_id.clone(), reconnected.revision).await;
        assert!(
            store.retry_run(&u, v.run.id, 2).await.is_err(),
            "withdrawn in-flight command is permanently retired"
        );
        assert!(store.claim_agent_run().await.unwrap().is_none());
        let reloaded = store
            .conversation(&u, t, None)
            .await
            .unwrap()
            .turns
            .remove(0);
        assert!(!reloaded.run.state.active());
        assert!(reloaded.assistant.is_empty());
        assert!(store.calendar_actions(&u).await.unwrap()[0].event.is_none());
        assert_eq!(io.write_count(), 1);
        assert!(
            f.state.requests.lock().unwrap().is_empty(),
            "sealed resume never reclassifies"
        );
    }
}

#[tokio::test]
async fn personal_calendar_completed_history_survives_model_disconnect() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, _f, s, l, cal, _, io) = configured().await;
    let (thread, _, claim) = claim(&store, &u, CREATE).await;
    execute(store.clone(), s, claim, cal).await;
    let before = store
        .conversation(&u, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(before.run.state, RunState::Completed);
    let actions = store.calendar_actions(&u).await.unwrap();
    assert_eq!(actions.len(), 1);
    let event = actions[0]
        .event
        .clone()
        .expect("applied private event exists before model disconnect");
    let receipt = before.run.calendar_receipt.unwrap();
    store
        .begin_subscription_disconnect(&u, l.binding_id, l.revision)
        .await
        .unwrap();
    let after = store
        .conversation(&u, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(after.run.state, RunState::Completed);
    assert_eq!(after.assistant[0].text, before.assistant[0].text);
    assert_eq!(
        after.run.calendar_receipt.unwrap().action_id,
        receipt.action_id
    );
    let retained = store.calendar_actions(&u).await.unwrap();
    assert_eq!(retained[0].id, actions[0].id);
    assert_eq!(retained[0].state, "applied");
    assert_eq!(retained[0].event.as_ref().unwrap().id, event.id);
    assert_eq!(io.write_count(), 1);
}

#[tokio::test]
async fn personal_calendar_finish_commits_failed_guard_after_binding_replacement() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, store, u, f, s, l, _cal, _, io) = configured().await;
    let (thread, _, claim) = claim(&store, &u, CREATE).await;
    // A separately completed signed reauthentication replaces the frozen source.
    // This path intentionally does not use explicit-disconnect retirement.
    let newer = login_binding(&s, &u, Some((l.binding_id, l.revision))).await;
    assert!(newer.revision > l.revision);
    assert!(
        store
            .finish_calendar_chat(&claim.lease, None)
            .await
            .is_err()
    );
    let done = store
        .conversation(&u, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(
        done.run.state,
        RunState::Failed,
        "guard failure must not roll its state change back"
    );
    assert!(done.assistant.is_empty());
    assert_eq!(io.write_count(), 0);
    assert!(f.state.requests.lock().unwrap().is_empty());
}
