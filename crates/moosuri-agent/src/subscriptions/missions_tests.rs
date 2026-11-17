//! Signed personal OAuth + official plan protocol through the actual Runtime.
use super::*;
use axum::{
    body::Body,
    http::{HeaderMap, Method, Request, StatusCode},
    response::Response,
};
use moosuri_core::*;
const REPEAT: &str = "Every minute calculate exact 0.1 plus 0.2";
const STOP: &str = "Pause that task";
fn wire_calls(calls: Vec<(&str, Value, &str)>) -> String {
    let mut frames = vec![
        json!({"type":"response.created","response":{"id":"mission-response","model":"fixture-model"}}),
    ];
    let mut output = vec![];
    for (index, (name, args, id)) in calls.into_iter().enumerate() {
        let args = args.to_string();
        let item = json!({"type":"function_call","id":format!("fc-{index}"),"call_id":id,"namespace":"moosuri","name":name,"arguments":args,"status":"completed"});
        frames.push(json!({"type":"response.output_item.added","output_index":index,"item":{"type":"function_call","id":format!("fc-{index}"),"call_id":id,"namespace":"moosuri","name":name,"arguments":"","status":"in_progress"}}));
        frames.push(json!({"type":"response.function_call_arguments.delta","output_index":index,"item_id":format!("fc-{index}"),"delta":args}));
        frames.push(json!({"type":"response.function_call_arguments.done","output_index":index,"item_id":format!("fc-{index}"),"arguments":args}));
        frames.push(json!({"type":"response.output_item.done","output_index":index,"item":item}));
        output.push(item);
    }
    frames.push(json!({"type":"response.completed","response":{"id":"mission-response","model":"fixture-model","status":"completed","output":output,"usage":{"input_tokens":8,"output_tokens":2}}}));
    frames.iter().map(|e| format!("data: {e}\n\n")).collect()
}
pub(super) async fn response(s: &Provider, headers: &HeaderMap, body: &Value) -> Response {
    s.models.fetch_add(1, Ordering::SeqCst);
    s.requests.lock().unwrap().push(body.clone());
    assert!(
        headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("Bearer synthetic-access-")
    );
    assert_eq!(body["model"], "fixture-model");
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    for forbidden in ["max_output_tokens", "previous_response_id"] {
        assert!(body.get(forbidden).is_none());
    }
    let input = body["input"].as_array().unwrap();
    assert_eq!(input[0]["role"], "developer");
    assert!(!input.iter().any(|m| m["role"] == "system"));
    let text = input.iter().rev().find(|m| m["role"] == "user").unwrap()["content"]
        .as_str()
        .unwrap();
    let defs = body.pointer("/tools/0/tools").and_then(Value::as_array);
    if let Some(defs) = defs {
        assert_eq!(body["tools"][0]["type"], "namespace");
        assert_eq!(body["tools"][0]["name"], "moosuri");
        if defs.iter().any(|d| d["name"] == "mission_chat_decision") {
            let d = if text == REPEAT {
                json!({"decision":"command","action":"create_scheduled","target_kind":null,"intent_quote":REPEAT,"task_quote":"calculate exact 0.1 plus 0.2","constraints_quote":null,"time_quote":"Every minute","target_quote":null,"instruction_quote":null,"time":{"kind":"interval","seconds":60,"date":null,"time":null,"days":null},"clarification":null})
            } else {
                assert_eq!(text, STOP);
                json!({"decision":"command","action":"pause","target_kind":"selected","intent_quote":STOP,"task_quote":null,"constraints_quote":null,"time_quote":null,"target_quote":null,"instruction_quote":null,"time":null,"clarification":null})
            };
            return (
                [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
                wire_calls(vec![("mission_chat_decision", d, "mission-delegation")]),
            )
                .into_response();
        }
        for forbidden in [
            "mail_prepare_draft",
            "mail_send",
            "calendar_action_status",
            "browser_open",
            "artifact_write",
            "artifact_read",
            "learned_skill_execute",
            "github_repository_metadata",
            "conversation_search",
        ] {
            assert!(
                !defs.iter().any(|d| d["name"] == forbidden),
                "mission exposed {forbidden}"
            );
        }
    }
    let context: Value = serde_json::from_str(text).unwrap();
    let task = context["original"]["task"].as_str().unwrap();
    let receipts = context["receipts"].as_array().unwrap();
    let result = |id: &str| -> Value {
        receipts.iter().find(|r| r["call_id"] == id).unwrap()["result"].clone()
    };
    if s.hold_tool.swap(0, Ordering::SeqCst) == 1 {
        s.tool_entered.notify_one();
        s.tool_release.notified().await;
    }
    let wire = if context["plan"].as_array().unwrap().is_empty() {
        assert!(defs.is_none());
        foreground_tools_tests::final_wire(
            "{\"steps\":[\"Read admitted current evidence\",\"Verify actual receipt\"]}",
        )
    } else if task.contains("Gmail") {
        let defs = defs.unwrap();
        let pin = |name: &str| {
            defs.iter().find(|d|d["name"]==name).unwrap()["parameters"]["properties"]["connection_id"]["enum"][0].clone()
        };
        match receipts.len() {
            0 => wire_calls(vec![
                (
                    "mail_search",
                    json!({"connection_id":pin("mail_search"),"query":"newer_than:1d","page_token":null}),
                    "mail-search",
                ),
                (
                    "calendar_list_events",
                    json!({"connection_id":pin("calendar_list_events"),"start":"2026-11-15T00:00:00Z","end":"2026-11-16T00:00:00Z","page_token":null}),
                    "calendar-list",
                ),
            ]),
            2 => wire_calls(vec![
                (
                    "mail_read",
                    json!({"connection_id":pin("mail_search"),"message_id":result("mail-search")["value"]["messages"][0]["id"]}),
                    "mail-read",
                ),
                (
                    "calendar_read_event",
                    json!({"observation_ref":result("calendar-list")["value"]["events"][0]["observation_ref"]}),
                    "calendar-read",
                ),
            ]),
            4 => {
                assert_eq!(
                    result("mail-read")["value"]["mail"]["text"],
                    "Synthetic owned mail"
                );
                assert_eq!(
                    result("calendar-read")["value"]["event"]["fields"]["title"],
                    "Synthetic owned event"
                );
                let evidence = receipts
                    .iter()
                    .filter(|r| {
                        matches!(r["call_id"].as_str(), Some("mail-read" | "calendar-read"))
                    })
                    .map(|r| json!({"step":r["step"],"call_id":r["call_id"]}))
                    .collect::<Vec<_>>();
                foreground_tools_tests::final_wire(&json!({"text":"Synthetic owned mail; Synthetic owned event. Review required.","evidence":evidence}).to_string())
            }
            _ => panic!("unexpected connected receipt count"),
        }
    } else {
        match receipts.len() {
            0 => {
                let mut calls = vec![(
                    "decimal_calculate",
                    json!({"operation":"sum","operands":["0.1","0.2"]}),
                    "exact",
                )];
                if task.contains("invoice") {
                    calls.push(("knowledge_search",json!({"kind":"source","text":"Synthetic invoice","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null}),"search"));
                }
                wire_calls(calls)
            }
            2 if task.contains("invoice") => {
                let search = result("search");
                let hit = &search["value"]["hits"][0];
                wire_calls(vec![(
                    "knowledge_read_source",
                    json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}),
                    "source",
                )])
            }
            _ => {
                assert_eq!(result("exact")["value"]["exact"], "0.3");
                if task.contains("invoice") {
                    assert_eq!(
                        result("source")["value"]["text"],
                        "Synthetic invoice total: 0.3."
                    );
                }
                let evidence = receipts
                    .iter()
                    .filter(|r| r["call_id"] != "search")
                    .map(|r| json!({"step":r["step"],"call_id":r["call_id"]}))
                    .collect::<Vec<_>>();
                foreground_tools_tests::final_wire(&json!({"text":"The exact total is 0.3. Synthetic invoice total: 0.3.","evidence":evidence}).to_string())
            }
        }
    };
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        wire,
    )
        .into_response()
}
fn request(task: &str, mode: MissionMode) -> MissionCreate {
    MissionCreate {
        request_id: uuid::Uuid::new_v4().to_string(),
        task: task.into(),
        constraints: String::new(),
        checks: vec![MissionCheck::DecimalEquals {
            expected: "0.3".into(),
        }],
        execution_mode: mode,
    }
}
async fn setup() -> (
    tempfile::TempDir,
    Store,
    UserContext,
    Fixture,
    SubscriptionService,
    LocalLoginLaunch,
) {
    let f = fixture().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    s.bootstrap(
        "Owner".into(),
        "owner@example.test".into(),
        PASSWORD.into(),
        false,
    )
    .await
    .unwrap();
    let u = s
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let service = service(s.clone(), &f);
    let l = login(&service, &u).await;
    let choice = proof(&service, &u, l.binding_id.clone(), l.revision).await;
    s.set_model_preference(
        &u,
        Some(ModelSelection {
            source: choice.source,
            provider_id: choice.provider_id,
            provider_revision: choice.provider_revision,
            model: choice.model,
            effort: choice.effort,
        }),
    )
    .await
    .unwrap();
    assert!(
        s.create_mission(&u, request("Calculate 0.1 plus 0.2", MissionMode::Durable))
            .await
            .is_err(),
        "text proof is not mission tool capability"
    );
    let (b, _) = s
        .subscription_tool_probe_binding(
            &u,
            l.binding_id.clone(),
            l.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    s.record_subscription_tool_probe(&u, b.subscription.unwrap())
        .await
        .unwrap();
    f.state.models.store(0, Ordering::SeqCst);
    f.state.requests.lock().unwrap().clear();
    f.state.tool_mode.store(30, Ordering::SeqCst);
    (d, s, u, f, service, l)
}
fn runtime(
    store: Store,
    mut subscriptions: SubscriptionService,
    google: Option<std::net::SocketAddr>,
) -> Runtime {
    let (changes, _) = watch::channel(0);
    let (done_tx, done) = watch::channel(false);
    let (run_activity_tx, run_activity) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mut mail = crate::mail::MailService::new(
        store.clone(),
        google.map(|_| {
            Arc::new(moosuri_connectors::gmail::OAuthClient {
                client_id: "synthetic-mission-app".into(),
                client_secret: moosuri_connectors::gmail::SecretString::new(
                    "synthetic-secret".into(),
                )
                .unwrap(),
                redirect_uri: "http://localhost/callback".into(),
            })
        }),
        signals.clone(),
    );
    if let Some(address) = google {
        mail = mail.with_synthetic_google(address);
    }
    let mut web = crate::web::Service::new(None);
    let browser = crate::browser::BrowserService::new(store.clone(), None, signals.stop.clone());
    web.set_workspaces(browser.clone());
    subscriptions.signals = signals.clone();
    subscriptions.store = store.clone();
    let supervisor = tokio::spawn(supervise(
        store,
        signals.clone(),
        done_tx,
        run_activity_tx,
        mail.clone(),
        web,
        subscriptions.clone(),
        None,
    ));
    Runtime {
        control: Arc::new(Control {
            signals,
            supervisor: Mutex::new(Some(supervisor)),
            done,
            run_activity,
            mail,
            browser,
            subscriptions,
        }),
    }
}
async fn finished(s: &Store, u: &UserContext, id: &str) -> MissionView {
    tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let v = s
                .missions(u)
                .await
                .unwrap()
                .into_iter()
                .find(|m| m.id == id)
                .unwrap();
            if !matches!(
                v.state.as_str(),
                "queued" | "planning" | "executing" | "verifying"
            ) {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn personal_missions_runtime_direct_and_natural_recurring_keep_owner_proof_receipts_and_idle_zero_calls()
 {
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, f, service, l) = setup().await;
    let collection = s
        .create_collection(&u, "Synthetic invoice".into(), "synthetic-invoice".into())
        .await
        .unwrap();
    let source = s
        .ingest_text(
            &u,
            collection.id,
            IngestBatch {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "invoice".into(),
                    version: "v1".into(),
                    expected_revision: None,
                    title: "Synthetic invoice".into(),
                    text: "Synthetic invoice total: 0.3.".into(),
                    speaker: None,
                    observed_at: 1700000000,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap()
        .source_ids
        .remove(0);
    let mut input = request(
        "Read Synthetic invoice and calculate 0.1 plus 0.2",
        MissionMode::Durable,
    );
    input.checks.push(MissionCheck::SourceQuote {
        id: source,
        revision: 1,
        quote: "Synthetic invoice total: 0.3.".into(),
    });
    let one = s.create_mission(&u, input).await.unwrap();
    s.expire_subscription_fixture(&u, l.binding_id.clone(), l.revision)
        .await
        .unwrap();
    f.state.model_peers.lock().unwrap().clear();
    let rt = runtime(s.clone(), service.clone(), None);
    assert_eq!(finished(&s, &u, &one.id).await.state, "achieved");
    assert_eq!(f.state.models.load(Ordering::SeqCst), 3);
    {
        let peers = f.state.model_peers.lock().unwrap();
        assert_eq!(peers.len(), 3);
        assert!(
            peers.iter().all(|peer| *peer == peers[0]),
            "mission rounds must reuse their current authorized transport"
        );
    }
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
    assert!(f.state.refreshes.load(Ordering::SeqCst) >= 1);
    let t = s
        .create_thread(&u, "Explicit repeat delegation".into())
        .await
        .unwrap();
    s.submit_turn_with_schedule_context(
        &u,
        t.id.clone(),
        uuid::Uuid::new_v4().to_string(),
        REPEAT.into(),
        None,
        Some(ScheduleChatContext {
            timezone: "UTC".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    rt.wake();
    let scheduled = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = s.conversation(&u, t.id.clone(), None).await.unwrap();
            if let Some(v) = s.recurring_missions(&u).await.unwrap().into_iter().next() {
                break v;
            }
            if let Some(turn) = page.turns.last() {
                assert!(matches!(turn.run.state, RunState::Queued | RunState::Running),
                    "delegation finished without a recurring parent: state={} error={:?} receipt={}",
                    serde_json::to_string(&turn.run.state).unwrap(), turn.run.error,
                    serde_json::to_string(&turn.run.mission_receipt).unwrap());
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(f.state.models.load(Ordering::SeqCst), 4);
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert_eq!(
        f.state.models.load(Ordering::SeqCst),
        4,
        "idle due polling must not infer"
    );
    // A durable original delegation survives ordinary logout without becoming a
    // shared family provider. The next authenticated session observes, not grants.
    s.logout(&u).await.unwrap();
    let u = s
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let at = scheduled.schedule.next_fire_at.unwrap();
    // Drive the same DB-only enqueue→mission admission order as the due worker.
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    rt.wake();
    let child = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id != one.id)
        .unwrap();
    assert_eq!(finished(&s, &u, &child.id).await.state, "partial");
    assert_eq!(f.state.models.load(Ordering::SeqCst), 6);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 6);
    // Natural control is a new explicit owner command, retaining target revision.
    let schedule = s.recurring_missions(&u).await.unwrap().remove(0);
    s.submit_turn_with_mission_context(
        &u,
        t.id.clone(),
        uuid::Uuid::new_v4().to_string(),
        STOP.into(),
        None,
        vec![],
        None,
        Some(MissionChatContext {
            target: Some(MissionChatTarget {
                kind: "recurring".into(),
                id: schedule.schedule.id.clone(),
                revision: schedule.schedule.revision,
            }),
        }),
    )
    .await
    .unwrap();
    rt.wake();
    let paused = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let v = s.recurring_missions(&u).await.unwrap().remove(0);
            if v.schedule.state == "paused" {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(paused.schedule.state, "paused");
    assert_eq!(f.state.models.load(Ordering::SeqCst), 7);
    rt.shutdown().await;
}
fn google_pin() -> moosuri_store::GoogleAppPin {
    moosuri_store::GoogleAppPin {
        revision: None,
        client_id: "synthetic-mission-app".into(),
        redirect_uri: "http://localhost/callback".into(),
    }
}
fn secret(v: &str) -> moosuri_connectors::gmail::SecretString {
    moosuri_connectors::gmail::SecretString::new(v.into()).unwrap()
}
async fn connect_google(s: &Store, u: &UserContext) -> (String, String) {
    use moosuri_connectors::gmail::TokenSet;
    let tokens = |calendar| TokenSet {
        access_token: secret(if calendar {
            "owned-calendar"
        } else {
            "owned-mail"
        }),
        refresh_token: Some(secret("refresh")),
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
    };
    let begin = s
        .start_mail_oauth_with_app(u, google_pin(), false)
        .await
        .unwrap();
    let h = s
        .capture_mail_oauth(begin.state, secret("code"))
        .await
        .unwrap();
    let e = s
        .claim_mail_oauth_with_app(u, h, google_pin())
        .await
        .unwrap();
    let mail = s
        .complete_mail_oauth(e, tokens(false), "owner@example.test".into())
        .await
        .unwrap();
    let begin = s.start_calendar_oauth(u, google_pin()).await.unwrap();
    let h = s
        .capture_calendar_oauth(begin.state, Some(secret("code")))
        .await
        .unwrap()
        .unwrap();
    let e = s.claim_calendar_oauth(u, h, google_pin()).await.unwrap();
    let cal = s
        .complete_calendar_oauth(
            e,
            tokens(true),
            CalendarInfo {
                id: "owner@example.test".into(),
                title: "Synthetic primary".into(),
                time_zone: "UTC".into(),
                primary: true,
                access_role: "owner".into(),
                email_notifications: false,
            },
        )
        .await
        .unwrap();
    (mail.id, cal.id)
}
#[derive(Default)]
struct Google {
    reads: AtomicUsize,
    writes: AtomicUsize,
}
async fn google_response(State(g): State<Arc<Google>>, request: Request<Body>) -> Response {
    if request.method() != Method::GET {
        g.writes.fetch_add(1, Ordering::SeqCst);
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    assert!(matches!(
        request.headers()["authorization"].to_str().unwrap(),
        "Bearer owned-mail" | "Bearer owned-calendar"
    ));
    g.reads.fetch_add(1, Ordering::SeqCst);
    let event = json!({"id":"ownedevent","etag":"\"owned-etag\"","status":"confirmed","summary":"Synthetic owned event","start":{"dateTime":"2026-11-15T10:00:00Z","timeZone":"UTC"},"end":{"dateTime":"2026-11-15T11:00:00Z","timeZone":"UTC"},"organizer":{"self":true,"email":"owner@example.test"},"eventType":"default"});
    let value = match request.uri().path() {
        "/messages" => {
            json!({"messages":[{"id":"ownedmessage","threadId":"ownedthread"}],"resultSizeEstimate":1})
        }
        "/messages/ownedmessage" => {
            assert!(request.uri().query().unwrap().contains("format=full"));
            let text = b"Synthetic owned mail";
            json!({"id":"ownedmessage","threadId":"ownedthread","labelIds":["INBOX"],"payload":{"partId":"","mimeType":"text/plain","filename":"","headers":[{"name":"From","value":"sender@example.test"},{"name":"To","value":"owner@example.test"},{"name":"Subject","value":"Owned briefing"},{"name":"Message-ID","value":"<owned-message@example.test>"}],"body":{"size":text.len(),"data":URL_SAFE_NO_PAD.encode(text)}}})
        }
        "/users/me/calendarList/primary" => {
            json!({"id":"owner@example.test","summary":"Synthetic primary","timeZone":"UTC","primary":true,"accessRole":"owner"})
        }
        "/calendars/primary/events" => json!({"items":[event]}),
        "/calendars/primary/events/ownedevent" => event,
        path => panic!("unexpected Google read {path}"),
    };
    Json(value).into_response()
}
#[tokio::test]
async fn personal_missions_runtime_delegated_connected_reads_and_connection_withdrawal_keep_scope()
{
    let _g = crate::tests::TEST_LOCK.lock().await;
    let (_d, s, u, f, service, _l) = setup().await;
    let (mail, _cal) = connect_google(&s, &u).await;
    let g = Arc::new(Google::default());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new()
        .fallback(google_response)
        .with_state(g.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let mut input = request(
        "Read my Gmail and primary Calendar for a private briefing",
        MissionMode::Durable,
    );
    input.checks = vec![MissionCheck::UserReview {
        description: "Review briefing".into(),
    }];
    let one = s.create_mission(&u, input.clone()).await.unwrap();
    let rt = runtime(s.clone(), service.clone(), Some(address));
    let result = finished(&s, &u, &one.id).await;
    assert_eq!(result.state, "partial");
    assert_eq!(result.receipts.len(), 2);
    assert_eq!(f.state.models.load(Ordering::SeqCst), 3);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
    assert_eq!(g.writes.load(Ordering::SeqCst), 0);
    assert!(g.reads.load(Ordering::SeqCst) >= 4);
    rt.shutdown().await;
    // An already delegated queued request cannot exchange withdrawn authority
    // for another connection or silently omit the requested private domain.
    input.request_id = uuid::Uuid::new_v4().to_string();
    let revoked = s.create_mission(&u, input).await.unwrap();
    s.disconnect_mail(&u, mail).await.unwrap();
    let before = f.state.models.load(Ordering::SeqCst);
    let rt = runtime(s.clone(), service, None);
    let result = finished(&s, &u, &revoked.id).await;
    assert_ne!(result.state, "achieved");
    assert_eq!(f.state.models.load(Ordering::SeqCst), before);
    assert_eq!(g.writes.load(Ordering::SeqCst), 0);
    rt.shutdown().await;
    task.abort();
}
#[tokio::test]
async fn personal_missions_runtime_cancel_revoke_and_restart_never_reenter_unknown_dispatch() {
    let _g = crate::tests::TEST_LOCK.lock().await;
    for boundary in ["cancel", "revoke", "restart", "session"] {
        let (d, s, u, f, service, l) = setup().await;
        let mode = if boundary == "session" {
            MissionMode::CurrentSession
        } else {
            MissionMode::Durable
        };
        let one = s
            .create_mission(&u, request("Calculate 0.1 plus 0.2", mode))
            .await
            .unwrap();
        if boundary == "session" {
            s.logout(&u).await.unwrap();
            let u = s
                .login("owner@example.test".into(), PASSWORD.into())
                .await
                .unwrap()
                .user;
            let rt = runtime(s.clone(), service, None);
            let v = finished(&s, &u, &one.id).await;
            assert_ne!(v.state, "achieved");
            assert_eq!(f.state.models.load(Ordering::SeqCst), 0);
            rt.shutdown().await;
            continue;
        }
        f.state.hold_tool.store(1, Ordering::SeqCst);
        let rt = runtime(s.clone(), service.clone(), None);
        tokio::time::timeout(Duration::from_secs(10), f.state.tool_entered.notified())
            .await
            .unwrap();
        assert_eq!(f.state.models.load(Ordering::SeqCst), 1);
        match boundary {
            "cancel" => {
                s.control_mission(
                    &u,
                    one.id.clone(),
                    MissionControl {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: one.revision,
                        action: MissionAction::Cancel,
                    },
                )
                .await
                .unwrap();
            }
            "revoke" => {
                service
                    .disconnect(&u, l.binding_id, l.revision)
                    .await
                    .unwrap();
            }
            "restart" => {
                rt.shutdown().await;
            }
            _ => unreachable!(),
        }
        f.state.tool_release.notify_one();
        if boundary != "restart" {
            rt.shutdown().await;
        }
        drop(rt);
        drop(service);
        drop(s);
        let reopened = Store::open(d.path()).unwrap();
        let rt = runtime(reopened.clone(), super::service(reopened.clone(), &f), None);
        tokio::time::sleep(Duration::from_millis(250)).await;
        assert_eq!(
            f.state.models.load(Ordering::SeqCst),
            1,
            "old dispatch reentered at {boundary}"
        );
        let v = reopened
            .missions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == one.id)
            .unwrap();
        assert!(v.uncertain || matches!(v.state.as_str(), "unknown" | "cancelled" | "failed"));
        assert!(
            reopened
                .control_mission(
                    &u,
                    one.id,
                    MissionControl {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        expected_revision: v.revision,
                        action: MissionAction::Resume
                    }
                )
                .await
                .is_err()
        );
        assert_eq!(
            reopened.usage_summary(&u).await.unwrap().dispatched_calls,
            1
        );
        rt.shutdown().await;
    }
}

#[tokio::test]
async fn personal_acknowledged_final_recovers_without_token_refresh_but_keeps_current_authority() {
    use moosuri_connectors::model::protocol::tools::{ToolCapabilities, ToolRequest};
    use moosuri_connectors::model::protocol::{
        FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole,
    };
    use moosuri_store::{KnowledgeToolInput, PendingKnowledgeCall, PrivateCheckpoint};
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for boundary in ["discovery_unavailable", "disconnected", "logout"] {
        let (dir, store, user, fixture, service, login) = setup().await;
        let mission = store
            .create_mission(
                &user,
                request(
                    "Calculate 0.1 plus 0.2",
                    if boundary == "logout" {
                        MissionMode::CurrentSession
                    } else {
                        MissionMode::Durable
                    },
                ),
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let first = store
            .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
            .await
            .unwrap();
        let result = {
            store
                .commit_agent_step(
                    &first,
                    PrivateCheckpoint {
                        bytes: b"{}".to_vec(),
                    },
                    vec![PendingKnowledgeCall {
                        call_id: "exact".into(),
                        input: KnowledgeToolInput::DecimalCalculate {
                            calculation: DecimalCalculation {
                                operation: DecimalOperation::Sum,
                                operands: vec!["0.1".into(), "0.2".into()],
                            },
                        },
                    }],
                )
                .await
                .unwrap();
            store
                .execute_knowledge_tool(&first, "exact".into())
                .await
                .unwrap()
        };
        store
            .mission_save_plan(&first, vec!["Calculate and verify".into()])
            .await
            .unwrap();
        let context = store.agent_context(&claim.lease).await.unwrap();
        let final_step = store
            .begin_agent_step(&claim.lease, context.checkpoint_seq)
            .await
            .unwrap();
        let client = service
            .client(
                moosuri_store::SubscriptionAuthority::Run(claim.lease.clone()),
                claim.binding.subscription.clone().unwrap(),
            )
            .await
            .unwrap();
        let request = TextRequest {
            model: claim.binding.model.clone(), effort: claim.binding.effort.clone(), max_output_tokens: 4096,
            messages: vec![TextMessage { role: TextRole::System, text: "Verify the original task from its actual receipt".into(), phase: None },
                TextMessage { role: TextRole::User, text: json!({"original":mission.request,"plan":["Calculate and verify"],"receipts":[{"step":1,"call_id":"exact","result":result}]}).to_string(), phase: None }],
        };
        let definitions = crate::knowledge_tools::definitions(claim.tool_profile.as_ref());
        let tools = ToolRequest {
            text: &request,
            tools: &definitions,
            continuation: None,
            results: &[],
        };
        let (tx, mut rx) = tokio::sync::mpsc::channel(32);
        let round = client
            .stream_tool_round(
                &tools,
                &ToolCapabilities {
                    function_tools: true,
                },
                &tx,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(round.outcome, FinishOutcome::Completed);
        let mut text = String::new();
        while let Ok(event) = rx.try_recv() {
            if let ModelEvent::TextDelta { text: delta, .. } = event {
                text.push_str(&delta);
            }
        }
        assert!(text.contains("0.3"));
        store.commit_agent_step(&final_step, PrivateCheckpoint { bytes: json!({"kind":"mission_round","terminal":"completed","planning":false,"text":text}).to_string().into_bytes() }, vec![]).await.unwrap();
        store
            .expire_subscription_fixture(&user, login.binding_id.clone(), login.revision)
            .await
            .unwrap();
        fixture.state.discovery_status.store(503, Ordering::SeqCst);
        if boundary == "disconnected" {
            service
                .disconnect(&user, login.binding_id, login.revision)
                .await
                .unwrap();
        }
        if boundary == "logout" {
            store.logout(&user).await.unwrap();
        }
        let before_models = fixture.state.models.load(Ordering::SeqCst);
        let before_refreshes = fixture.state.refreshes.load(Ordering::SeqCst);
        drop(client);
        drop(claim);
        drop(service);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let observer = if boundary == "logout" {
            store
                .login("owner@example.test".into(), PASSWORD.into())
                .await
                .unwrap()
                .user
        } else {
            user
        };
        let runtime = runtime(store.clone(), super::service(store.clone(), &fixture), None);
        let done = finished(&store, &observer, &mission.id).await;
        if boundary == "discovery_unavailable" {
            assert_eq!(done.state, "achieved", "{:?}", done.report);
            assert!(done.result.unwrap().text.contains("0.3"));
        } else {
            assert_ne!(done.state, "achieved", "{boundary}");
        }
        assert_eq!(fixture.state.models.load(Ordering::SeqCst), before_models);
        assert_eq!(
            fixture.state.refreshes.load(Ordering::SeqCst),
            before_refreshes
        );
        assert_eq!(
            store
                .usage_summary(&observer)
                .await
                .unwrap()
                .dispatched_calls,
            2
        );
        runtime.shutdown().await;
    }
}
