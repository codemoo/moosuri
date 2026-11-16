//! Composed real Runtime + real typed loopback Gmail/Calendar reads, no Google account.
use super::*;
use axum::{
    http::{Method, Request, StatusCode},
    response::IntoResponse,
};
use base64::Engine;
use moosuri_connectors::gmail::{OAuthClient, SecretString, TokenSet};
use moosuri_core::*;
use moosuri_store::GoogleAppPin;
struct Io {
    reads: AtomicUsize,
    searches: AtomicUsize,
    writes: AtomicUsize,
    hold: AtomicU8,
    started: tokio::sync::Notify,
    release: Semaphore,
}
fn secret(s: &str) -> SecretString {
    SecretString::new(s.into()).unwrap()
}
fn pin() -> GoogleAppPin {
    GoogleAppPin {
        revision: None,
        client_id: "synthetic-connected-app".into(),
        redirect_uri: "http://localhost/callback".into(),
    }
}
async fn connections(s: &Store, u: &UserContext, owner: &str) -> (String, String) {
    let begin = s.start_mail_oauth_with_app(u, pin(), false).await.unwrap();
    let handle = s
        .capture_mail_oauth(begin.state, secret("code"))
        .await
        .unwrap();
    let ex = s.claim_mail_oauth_with_app(u, handle, pin()).await.unwrap();
    let m = s
        .complete_mail_oauth(
            ex,
            TokenSet {
                access_token: secret(&format!("{owner}-mail")),
                refresh_token: Some(secret("refresh")),
                expires_in: 3600,
                scopes: Some(
                    moosuri_connectors::gmail::MINIMAL_SCOPES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            format!("{owner}@example.test"),
        )
        .await
        .unwrap();
    let begin = s.start_calendar_oauth(u, pin()).await.unwrap();
    let handle = s
        .capture_calendar_oauth(begin.state, Some(secret("code")))
        .await
        .unwrap()
        .unwrap();
    let ex = s.claim_calendar_oauth(u, handle, pin()).await.unwrap();
    let c = s
        .complete_calendar_oauth(
            ex,
            TokenSet {
                access_token: secret(&format!("{owner}-calendar")),
                refresh_token: Some(secret("refresh")),
                expires_in: 3600,
                scopes: Some(
                    moosuri_connectors::calendar::SCOPES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            CalendarInfo {
                id: format!("{owner}@example.test"),
                title: "Synthetic primary".into(),
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
fn event() -> Value {
    json!({"id":"ownedevent","etag":"\"owned-etag\"","status":"confirmed","summary":"Owned meeting","description":"Synthetic owned calendar evidence","start":{"dateTime":"2026-11-14T10:00:00Z","timeZone":"UTC"},"end":{"dateTime":"2026-11-14T11:00:00Z","timeZone":"UTC"},"organizer":{"self":true,"email":"owner@example.test"},"eventType":"default"})
}
async fn io_response(State(s): State<Arc<Io>>, req: Request<Body>) -> Response {
    if req.method() != Method::GET {
        s.writes.fetch_add(1, Ordering::SeqCst);
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let authorization = req
        .headers()
        .get("authorization")
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        matches!(authorization, "Bearer owner-mail" | "Bearer owner-calendar"),
        "foreign credentials must never reach this owner's fixture"
    );
    s.reads.fetch_add(1, Ordering::SeqCst);
    let path = req.uri().path();
    let value = match path {
        "/messages" => {
            assert!(req.uri().query().unwrap().contains("newer_than"));
            let n = s.searches.fetch_add(1, Ordering::SeqCst) + 1;
            json!({"messages":[{"id":format!("ownedmessage{n}"),"threadId":format!("ownedthread{n}")}],"resultSizeEstimate":1})
        }
        p if p.starts_with("/messages/") => {
            let text = b"Owned mail body";
            json!({"id":p.trim_start_matches("/messages/"),"threadId":"ownedthread1","labelIds":["INBOX"],"payload":{"partId":"","mimeType":"text/plain","filename":"","headers":[{"name":"From","value":"sender@example.test"},{"name":"To","value":"owner@example.test"},{"name":"Subject","value":"Owned briefing memo"},{"name":"Message-ID","value":"<owned-source@example.test>"}],"body":{"size":text.len(),"data":base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(text)}}})
        }
        "/users/me/calendarList/primary" => {
            json!({"id":"owner@example.test","summary":"Owned primary","timeZone":"UTC","primary":true,"accessRole":"owner"})
        }
        "/calendars/primary/events" => json!({"items":[event()]}),
        "/calendars/primary/events/ownedevent" => {
            if s.hold.load(Ordering::SeqCst) == 1 {
                s.started.notify_one();
                s.release.acquire().await.unwrap().forget();
            }
            event()
        }
        _ => panic!("unexpected read endpoint {path}"),
    };
    axum::Json(value).into_response()
}
struct Model {
    calls: AtomicUsize,
}
async fn model(State(s): State<Arc<Model>>, axum::Json(body): axum::Json<Value>) -> Response {
    s.calls.fetch_add(1, Ordering::SeqCst);
    assert!(!body.to_string().contains("FOREIGN_CONNECTOR_HISTORY"));
    if body["tools"].as_array().is_some_and(|v| {
        v.iter()
            .any(|d| d["function"]["name"] == "mission_chat_decision")
    }) {
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        let text = body["messages"][1]["content"].as_str().unwrap();
        let unsupported = text.contains("발송") && !text.contains("실제로");
        let decision = MissionChatDecision {
            decision: if unsupported {
                "unsupported"
            } else {
                "command"
            }
            .into(),
            action: (!unsupported).then(|| "create_scheduled".into()),
            target_kind: None,
            intent_quote: (!unsupported).then(|| text.into()),
            task_quote: (!unsupported).then(|| {
                if text.contains("빠뜨려") {
                    "요약해".into()
                } else if text.contains("실제로") {
                    "Gmail과 Calendar를 읽고 요약해".into()
                } else {
                    text.into()
                }
            }),
            constraints_quote: None,
            time_quote: (!unsupported).then(|| "매일 아침9시".into()),
            target_quote: None,
            instruction_quote: None,
            time: (!unsupported).then(|| MissionChatTime::Daily {
                time: "09:00".into(),
            }),
            clarification: unsupported.then(|| "unsupported".into()),
        };
        let call = knowledge::tool_call(
            0,
            "connected-delegation".into(),
            "mission_chat_decision",
            serde_json::to_value(decision).unwrap(),
        );
        let data = frame(
            json!({"id":"connected-intent","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[call]},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
        ) + "data: [DONE]\n\n";
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from(data))
            .unwrap();
    }
    let context: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    let plan = context["plan"].as_array().unwrap();
    let receipts = context["receipts"].as_array().unwrap();
    let (delta, reason) = if plan.is_empty() {
        assert!(
            body.get("tools").is_none() || body["tools"].as_array().is_some_and(|v| v.is_empty())
        );
        (
            json!({"role":"assistant","content":"{\"steps\":[\"Read fresh Gmail and primary Calendar evidence\",\"Prepare private daily briefing\"]}"}),
            "stop",
        )
    } else {
        let tools = body["tools"].as_array().unwrap();
        for denied in [
            "mail_prepare_draft",
            "mail_send",
            "calendar_action_status",
            "browser_open",
            "conversation_search",
        ] {
            assert!(!tools.iter().any(|d| d["function"]["name"] == denied))
        }
        let find = |name: &str| {
            tools
                .iter()
                .find(|d| d["function"]["name"] == name)
                .unwrap()
        };
        let mail =
            find("mail_search")["function"]["parameters"]["properties"]["connection_id"]["enum"][0]
                .clone();
        let cal=find("calendar_list_events")["function"]["parameters"]["properties"]["connection_id"]["enum"][0].clone();
        let result = |id: &str| -> Value {
            receipts.iter().find(|r| r["call_id"] == id).unwrap()["result"].clone()
        };
        match receipts.len() {
            0 => (
                json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"fresh-mail-search".into(),"mail_search",json!({"connection_id":mail,"query":"newer_than:1d","page_token":null})),knowledge::tool_call(1,"fresh-calendar-list".into(),"calendar_list_events",json!({"connection_id":cal,"start":"2026-11-14T00:00:00Z","end":"2026-11-15T00:00:00Z","page_token":null}))]}),
                "tool_calls",
            ),
            2 => {
                let search = result("fresh-mail-search");
                assert_eq!(search["kind"], "external");
                let calendar = result("fresh-calendar-list");
                assert_eq!(calendar["kind"], "external");
                (
                    json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"fresh-mail-read".into(),"mail_read",json!({"connection_id":mail,"message_id":search["value"]["messages"][0]["id"]})),knowledge::tool_call(1,"fresh-event-read".into(),"calendar_read_event",json!({"observation_ref":calendar["value"]["events"][0]["observation_ref"]}))]}),
                    "tool_calls",
                )
            }
            4 => {
                let mail = result("fresh-mail-read");
                assert_eq!(mail["value"]["mail"]["text"], "Owned mail body");
                let calendar = result("fresh-event-read");
                assert_eq!(
                    calendar["value"]["event"]["fields"]["title"],
                    "Owned meeting"
                );
                let evidence = receipts
                    .iter()
                    .filter(|r| {
                        matches!(
                            r["call_id"].as_str(),
                            Some("fresh-mail-read" | "fresh-event-read")
                        )
                    })
                    .map(|r| json!({"step":r["step"],"call_id":r["call_id"]}))
                    .collect::<Vec<_>>();
                (
                    json!({"role":"assistant","content":json!({"text":"Daily briefing: Owned mail body; Owned meeting. Fresh owner evidence, review required.","evidence":evidence}).to_string()}),
                    "stop",
                )
            }
            n => panic!("unexpected receipt count {n}"),
        }
    };
    let data = frame(
        json!({"id":"connected-mission","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
    ) + &frame(
        json!({"id":"connected-mission","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(data))
        .unwrap()
}
fn runtime(s: Store, address: std::net::SocketAddr) -> Runtime {
    let (changes, _) = watch::channel(0);
    let (done_tx, done) = watch::channel(false);
    let (run_activity_tx, run_activity) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mail = crate::mail::MailService::new(
        s.clone(),
        Some(Arc::new(OAuthClient {
            client_id: pin().client_id,
            client_secret: secret("synthetic-secret"),
            redirect_uri: pin().redirect_uri,
        })),
        signals.clone(),
    )
    .with_synthetic_google(address);
    let mut web = crate::web::Service::new(None);
    let browser = crate::browser::BrowserService::new(s.clone(), None, signals.stop.clone());
    web.set_workspaces(browser.clone());
    let subscriptions = crate::subscriptions::SubscriptionService::new(s.clone(), signals.clone());
    let supervisor = tokio::spawn(supervise(
        s,
        signals.clone(),
        done_tx,
        run_activity_tx,
        mail.clone(),
        web,
        subscriptions.clone(),
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
fn input() -> MissionCreate {
    MissionCreate {
        request_id: uuid::Uuid::new_v4().to_string(),
        task: "Read my Gmail and Calendar for a daily briefing".into(),
        constraints: "Read only; keep results in private task history".into(),
        checks: vec![MissionCheck::UserReview {
            description: "Review daily briefing".into(),
        }],
        execution_mode: MissionMode::Durable,
    }
}
async fn finished(s: &Store, u: &UserContext, id: &str) -> MissionView {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let v = s
                .missions(u)
                .await
                .unwrap()
                .into_iter()
                .find(|v| v.id == id)
                .unwrap();
            if !matches!(
                v.state.as_str(),
                "queued" | "planning" | "executing" | "verifying"
            ) {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn connected_missions_runtime_private_briefing_and_late_calendar_disconnect_use_real_loopback_reads()
 {
    let _lock = TEST_LOCK.lock().await;
    let model_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", model_listener.local_addr().unwrap());
    let script = Arc::new(Model {
        calls: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(model))
        .with_state(script.clone());
    let model_server = tokio::spawn(async move { axum::serve(model_listener, app).await.unwrap() });
    let io_listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = io_listener.local_addr().unwrap();
    let io = Arc::new(Io {
        reads: AtomicUsize::new(0),
        searches: AtomicUsize::new(0),
        writes: AtomicUsize::new(0),
        hold: AtomicU8::new(0),
        started: tokio::sync::Notify::new(),
        release: Semaphore::new(0),
    });
    let app = Router::new().fallback(io_response).with_state(io.clone());
    let io_server = tokio::spawn(async move { axum::serve(io_listener, app).await.unwrap() });
    let (_d, s, u, _) = knowledge::tool_configured(&base).await;
    let (_, cal) = connections(&s, &u, "owner").await;
    let invite = s
        .create_user(
            &u,
            "Foreign".into(),
            "connected-foreign@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = s
        .login(
            "connected-foreign@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap();
    let foreign = s
        .change_password(&restricted.user, PASSWORD.into())
        .await
        .unwrap()
        .user;
    connections(&s, &foreign, "FOREIGN_CONNECTOR_HISTORY").await;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 60;
    let schedule = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                template: input(),
                schedule: ScheduleDraft {
                    title: "Daily owner briefing".into(),
                    message: String::new(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 86400,
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
    let fresh = s
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let id = s.missions(&fresh).await.unwrap().remove(0).id;
    let runtime = runtime(s.clone(), address);
    runtime.wake();
    let done = finished(&s, &fresh, &id).await;
    assert_eq!(done.state, "partial", "{:?}", done.report);
    assert!(
        done.result
            .unwrap()
            .text
            .contains("Owned mail body; Owned meeting")
    );
    let visible = serde_json::to_string(&done.receipts).unwrap();
    assert!(visible.contains("Owned briefing memo") && visible.contains("Owned meeting"));
    for private in [
        "observation_ref",
        "next_page_token",
        "ownedmessage",
        "ownedevent",
        "connection_id",
    ] {
        assert!(!visible.contains(private));
    }
    assert_eq!(script.calls.load(Ordering::SeqCst), 3);
    assert_eq!(io.searches.load(Ordering::SeqCst), 1);
    assert_eq!(io.writes.load(Ordering::SeqCst), 0);
    assert!(s.mail_actions(&fresh).await.unwrap().is_empty());
    assert!(s.calendar_actions(&fresh).await.unwrap().is_empty());
    io.hold.store(1, Ordering::SeqCst);
    let second = s.create_mission(&fresh, input()).await.unwrap();
    runtime.wake();
    tokio::time::timeout(Duration::from_secs(5), io.started.notified())
        .await
        .unwrap();
    assert_eq!(io.searches.load(Ordering::SeqCst), 2);
    s.disconnect_calendar(&fresh, cal).await.unwrap();
    io.release.add_permits(1);
    let gone = finished(&s, &fresh, &second.id).await;
    assert!(gone.result.is_none());
    assert!(matches!(
        gone.state.as_str(),
        "quarantined" | "unknown" | "partial"
    ));
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(script.calls.load(Ordering::SeqCst), 5);
    assert_eq!(io.writes.load(Ordering::SeqCst), 0);
    assert!(
        s.missions(&fresh)
            .await
            .unwrap()
            .iter()
            .all(|m| m.result.is_none())
    );
    assert!(
        s.control_recurring_mission(
            &fresh,
            schedule.schedule.id,
            ScheduleControl {
                expected_revision: schedule.schedule.revision,
                action: "resume".into()
            }
        )
        .await
        .is_err()
    );
    runtime.shutdown().await;
    model_server.abort();
    io_server.abort();
}
#[tokio::test]
async fn connected_missions_current_delegation_preserves_required_domains_and_rejects_send_without_approval_dialog()
 {
    let _lock = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = Arc::new(Model {
        calls: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(model))
        .with_state(script.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, s, u, thread) = knowledge::tool_configured(&base).await;
    let runtime = Runtime::start(s.clone());
    async fn ask(
        s: &Store,
        u: &UserContext,
        thread: &str,
        text: &str,
        runtime: &Runtime,
    ) -> ChatTurn {
        s.submit_turn_with_schedule_context(
            u,
            thread.into(),
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
        runtime.wake();
        wait_for(s, u, thread, |t| !t.run.state.active()).await
    }
    let request = "매일 아침9시에 Gmail과 Calendar를 읽고 요약해 줘";
    let missing = ask(&s, &u, &thread, request, &runtime).await;
    assert_eq!(
        missing.run.mission_receipt.unwrap().state,
        "needs_connection"
    );
    assert!(s.recurring_missions(&u).await.unwrap().is_empty());
    connections(&s, &u, "owner").await;
    let omitted = ask(
        &s,
        &u,
        &thread,
        "매일 아침9시에 Gmail과 Calendar를 읽고 요약해. 빠뜨려",
        &runtime,
    )
    .await;
    assert!(omitted.run.mission_receipt.is_some());
    assert!(s.recurring_missions(&u).await.unwrap().is_empty());
    let saved = ask(&s, &u, &thread, request, &runtime).await;
    assert_eq!(saved.run.mission_receipt.unwrap().state, "active");
    let schedules = s.recurring_missions(&u).await.unwrap();
    assert_eq!(schedules.len(), 1);
    assert_eq!(schedules[0].template.task, request);
    assert!(
        matches!(&schedules[0].schedule.rule,ScheduleRule::Daily{time,timezone} if time=="09:00"&&timezone=="Asia/Seoul")
    );
    let denied = ask(
        &s,
        &u,
        &thread,
        "매일 아침9시에 Gmail을 요약하고 a@example.test에 발송해 줘",
        &runtime,
    )
    .await;
    assert_eq!(denied.run.mission_receipt.unwrap().state, "unsupported");
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    assert!(s.mail_actions(&u).await.unwrap().is_empty());
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
    let stripped = ask(
        &s,
        &u,
        &thread,
        "매일 아침9시에 Gmail과 Calendar를 읽고 요약해, 실제로 a@example.test에 발송해",
        &runtime,
    )
    .await;
    assert_eq!(stripped.run.mission_receipt.unwrap().state, "unsupported");
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    assert_eq!(script.calls.load(Ordering::SeqCst), 5);
    runtime.shutdown().await;
    server.abort();
}

struct CompoundIo {
    read: Arc<Io>,
    drafts: AtomicUsize,
    calendars: AtomicUsize,
    sends: AtomicUsize,
}
async fn compound_io(State(s): State<Arc<CompoundIo>>, req: Request<Body>) -> Response {
    if req.method() == Method::GET {
        return io_response(State(s.read.clone()), req).await;
    }
    let path = req.uri().path().to_owned();
    if path.contains("send") {
        s.sends.fetch_add(1, Ordering::SeqCst);
        return StatusCode::METHOD_NOT_ALLOWED.into_response();
    }
    let mut body: Value = serde_json::from_slice(
        &axum::body::to_bytes(req.into_body(), 1000000)
            .await
            .unwrap(),
    )
    .unwrap();
    match path.as_str() {
        "/drafts" => {
            s.drafts.fetch_add(1, Ordering::SeqCst);
            assert!(body["message"]["raw"].is_string());
            axum::Json(json!({"id":"compounddraft","message":{"id":"compoundmessage","threadId":"compoundthread"}})).into_response()
        }
        "/calendars/primary/events" => {
            s.calendars.fetch_add(1, Ordering::SeqCst);
            body["etag"] = json!("\"compound-etag\"");
            body["status"] = json!("confirmed");
            body["organizer"] = json!({"self":true,"email":"owner@example.test"});
            body["eventType"] = json!("default");
            axum::Json(body).into_response()
        }
        _ => panic!("unexpected effect endpoint {path}"),
    }
}
fn compound_decision(task: &str) -> Value {
    json!({"domain":"calendar","decision":"command","operation":"create","connection_quote":null,"intent_quote":task,"title_quote":"Synthetic","target_quote":null,"date_quote":"2027-04-12","date":"2027-04-12","move_date_quote":null,"move_date":null,"all_day_quote":null,"last_date_quote":null,"last_date":null,"start_quote":"09:00","start":"09:00","end_quote":null,"end":null,"duration_quote":"30 minutes","duration_minutes":30,"clarification":null})
}
async fn compound_model(
    State(s): State<Arc<Model>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    s.calls.fetch_add(1, Ordering::SeqCst);
    let context: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    let task = context["accepted"]["task"].as_str().unwrap();
    let receipts = context["receipts"].as_array().unwrap();
    let tools = body["tools"].as_array();
    let definition = |name: &str| {
        tools
            .unwrap()
            .iter()
            .find(|t| t["function"]["name"] == name)
            .unwrap()
    };
    let result = |id: &str| -> Value {
        receipts.iter().find(|r| r["call_id"] == id).unwrap()["result"].clone()
    };
    let reference = |id: &str| -> Value {
        let r = receipts.iter().find(|r| r["call_id"] == id).unwrap();
        json!({"step":r["step"],"call_id":id})
    };
    let (delta, reason) = if context["plan"].as_array().unwrap().is_empty() {
        (
            json!({"role":"assistant","content":json!({"steps":["Read the requested evidence","Prepare exact mail and Calendar actions","Save the requested Gmail draft for later final approval"]}).to_string()}),
            "stop",
        )
    } else {
        match receipts.len() {
            0 => {
                let mail=definition("mail_search")["function"]["parameters"]["properties"]["connection_id"]["enum"][0].clone();
                let cal=definition("calendar_list_events")["function"]["parameters"]["properties"]["connection_id"]["enum"][0].clone();
                (
                    json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"search".into(),"mail_search",json!({"connection_id":mail,"query":"newer_than:1d","page_token":null})),knowledge::tool_call(1,"list".into(),"calendar_list_events",json!({"connection_id":cal,"start":"2026-11-14T00:00:00Z","end":"2026-11-15T00:00:00Z","page_token":null}))]}),
                    "tool_calls",
                )
            }
            2 => {
                let mail=definition("mail_read")["function"]["parameters"]["properties"]["connection_id"]["enum"][0].clone();
                (
                    json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"read".into(),"mail_read",json!({"connection_id":mail,"message_id":result("search")["value"]["messages"][0]["id"]}))]}),
                    "tool_calls",
                )
            }
            3 if !task.contains("recipient@example.test")
                && context["owner_inputs"].as_array().unwrap().is_empty() =>
            {
                (
                    json!({"role":"assistant","content":json!({"input_request":{"field":"mail_to","question":"회신 초안을 받을 이메일 주소를 알려 주세요."}}).to_string()}),
                    "stop",
                )
            }
            3 => {
                if !task.contains("recipient@example.test") {
                    assert_eq!(
                        context["owner_inputs"][0]["value"],
                        "recipient@example.test"
                    );
                }
                let mail=definition("mission_prepare_mail")["function"]["parameters"]["properties"]["input"]["properties"]["connection_id"]["enum"][0].clone();
                (
                    json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"prepare".into(),"mission_prepare_mail",json!({"intent_quote":task,"input":{"connection_id":mail,"origin_thread_id":null,"content":{"from":"owner@example.test","to":["recipient@example.test"],"cc":[],"bcc":[],"subject":"Synthetic compound draft","text":"Owned mail body","reply":null}},"evidence":[reference("read")]})),knowledge::tool_call(1,"calendar".into(),"mission_calendar_action",json!({"decision":compound_decision(task),"evidence":[reference("list")]}))]}),
                    "tool_calls",
                )
            }
            5 if !task.contains("recipient@example.test") => (
                json!({"role":"assistant","content":json!({"text":"Prepared the original draft and Calendar event with the owner recipient. Final mail approval is still required.","evidence":[reference("read"),reference("prepare"),reference("calendar")]}).to_string()}),
                "stop",
            ),
            5 => {
                let p = result("prepare");
                assert_eq!(
                    p["kind"],
                    "external",
                    "prepare={p}, read={}",
                    result("read")
                );
                assert_eq!(result("calendar")["value"]["state"], "applied");
                (
                    json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"provider".into(),"mission_save_provider_draft",json!({"intent_quote":task,"action_id":p["value"]["action_id"],"expected":{"revision":p["value"]["revision"],"digest":p["value"]["digest"]}}))]}),
                    "tool_calls",
                )
            }
            6 => {
                assert_eq!(
                    result("provider")["value"]["state"],
                    "saved",
                    "provider={}",
                    result("provider")
                );
                (
                    json!({"role":"assistant","content":json!({"text":"Prepared the exact local and Gmail drafts; Calendar creation has its actual receipt. Mail remains pending final owner approval.","evidence":[reference("read"),reference("prepare"),reference("calendar"),reference("provider")]}).to_string()}),
                    "stop",
                )
            }
            n => panic!("unexpected compound receipt count {n}"),
        }
    };
    let data = frame(
        json!({"id":"compound-actions","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
    ) + &frame(
        json!({"id":"compound-actions","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(data))
        .unwrap()
}
#[tokio::test]
async fn compound_actions_runtime_reads_prepares_gmail_and_creates_calendar_without_sending() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = Arc::new(Model {
        calls: AtomicUsize::new(0),
    });
    let model_app = Router::new()
        .route("/v1/chat/completions", post(compound_model))
        .with_state(script.clone());
    let model_server = tokio::spawn(async move { axum::serve(listener, model_app).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let io = Arc::new(CompoundIo {
        read: Arc::new(Io {
            reads: AtomicUsize::new(0),
            searches: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            hold: AtomicU8::new(0),
            started: tokio::sync::Notify::new(),
            release: Semaphore::new(0),
        }),
        drafts: AtomicUsize::new(0),
        calendars: AtomicUsize::new(0),
        sends: AtomicUsize::new(0),
    });
    let io_app = Router::new().fallback(compound_io).with_state(io.clone());
    let io_server = tokio::spawn(async move { axum::serve(listener, io_app).await.unwrap() });
    let (_dir, s, u, _) = knowledge::tool_configured(&base).await;
    let (read_only_mail, _) = connections(&s, &u, "owner").await;
    s.disconnect_mail(&u, read_only_mail).await.unwrap();
    let begin = s
        .start_mail_compose_oauth_with_app(&u, pin())
        .await
        .unwrap();
    let handle = s
        .capture_mail_oauth(begin.state, secret("code"))
        .await
        .unwrap();
    let exchange = s
        .claim_mail_oauth_with_app(&u, handle, pin())
        .await
        .unwrap();
    s.complete_mail_oauth(
        exchange,
        TokenSet {
            access_token: secret("owner-mail"),
            refresh_token: Some(secret("refresh")),
            expires_in: 3600,
            scopes: Some(
                moosuri_connectors::gmail::COMPOSE_SCOPES
                    .iter()
                    .map(|v| v.to_string())
                    .collect(),
            ),
        },
        "owner@example.test".into(),
    )
    .await
    .unwrap();
    let task = "Read Gmail and Calendar, prepare an email draft to recipient@example.test and create primary Calendar event named Synthetic on 2027-04-12 at 09:00 for 30 minutes and save Gmail draft";
    let m = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: task.into(),
                constraints: String::new(),
                checks: vec![MissionCheck::UserReview {
                    description: "Review the exact draft".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let runtime = runtime(s.clone(), address);
    runtime.wake();
    let done = finished(&s, &u, &m.id).await;
    assert_eq!(done.state, "partial", "{:?}", done.report);
    assert_eq!(done.actions.len(), 2);
    assert!(done.actions.iter().any(|a| a.kind == "mail"
        && a.state == "pending_approval"
        && a.provider_receipt.is_some()));
    assert!(
        done.actions
            .iter()
            .any(|a| a.kind == "calendar" && a.state == "applied" && a.provider_receipt.is_some())
    );
    assert_eq!(io.drafts.load(Ordering::SeqCst), 1);
    assert_eq!(io.calendars.load(Ordering::SeqCst), 1);
    assert_eq!(io.sends.load(Ordering::SeqCst), 0);
    assert_eq!(script.calls.load(Ordering::SeqCst), 5);
    assert!(s.next_mail_dispatch().await.unwrap().is_none());
    runtime.shutdown().await;
    model_server.abort();
    io_server.abort();
}

#[tokio::test]
async fn mission_input_runtime_reads_once_waits_without_polling_and_continues_same_run() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = Arc::new(Model {
        calls: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(compound_model))
        .with_state(script.clone());
    let model_server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let io = Arc::new(CompoundIo {
        read: Arc::new(Io {
            reads: AtomicUsize::new(0),
            searches: AtomicUsize::new(0),
            writes: AtomicUsize::new(0),
            hold: AtomicU8::new(0),
            started: tokio::sync::Notify::new(),
            release: Semaphore::new(0),
        }),
        drafts: AtomicUsize::new(0),
        calendars: AtomicUsize::new(0),
        sends: AtomicUsize::new(0),
    });
    let app = Router::new().fallback(compound_io).with_state(io.clone());
    let io_server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, s, u, _) = knowledge::tool_configured(&base).await;
    connections(&s, &u, "owner").await;
    let task = "Read Gmail account owner@example.test and Calendar, prepare an email draft and create primary Calendar event named Synthetic on 2027-04-12 at 09:00 for 30 minutes";
    let m = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: task.into(),
                constraints: String::new(),
                checks: vec![MissionCheck::UserReview {
                    description: "Review the exact draft".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let engine = runtime(s.clone(), address);
    engine.wake();
    let wait = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = s
                .missions(&u)
                .await
                .unwrap()
                .into_iter()
                .find(|v| v.id == m.id)
                .unwrap();
            if view.state == "waiting" {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(script.calls.load(Ordering::SeqCst), 3);
    assert_eq!(wait.completed_steps, 3);
    assert!(wait.actions.is_empty());
    // The original engine can be stopped and restarted while the owner answers.
    engine.shutdown().await;
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert_eq!(script.calls.load(Ordering::SeqCst), 3);
    let answer = moosuri_core::MissionInputReply {
        request_id: uuid::Uuid::new_v4().to_string(),
        wait_request_id: wait.input_wait.as_ref().unwrap().request_id.clone(),
        expected_revision: wait.revision,
        value: "recipient@example.test".into(),
    };
    let queued = s
        .reply_mission_input(&u, m.id.clone(), answer.clone())
        .await
        .unwrap();
    assert_eq!(queued.state, "queued");
    assert_eq!(queued.id, m.id);
    assert_eq!(queued.request.request_id, m.request.request_id);
    s.reply_mission_input(&u, m.id.clone(), answer)
        .await
        .unwrap();
    let resumed = runtime(s.clone(), address);
    resumed.wake();
    let done = finished(&s, &u, &m.id).await;
    assert_eq!(done.state, "partial", "{:?}", done.report);
    assert_eq!(done.actions.len(), 2);
    assert_eq!(done.inputs.len(), 1);
    assert_eq!(io.read.searches.load(Ordering::SeqCst), 1);
    assert_eq!(io.read.reads.load(Ordering::SeqCst), 5);
    assert_eq!(io.calendars.load(Ordering::SeqCst), 1);
    assert_eq!(io.drafts.load(Ordering::SeqCst), 0);
    assert_eq!(io.sends.load(Ordering::SeqCst), 0);
    assert_eq!(script.calls.load(Ordering::SeqCst), 5);
    assert!(s.next_mail_dispatch().await.unwrap().is_none());
    resumed.shutdown().await;
    model_server.abort();
    io_server.abort();
}

async fn flexible_context_model(
    State(s): State<Arc<Model>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    s.calls.fetch_add(1, Ordering::SeqCst);
    let context: Value =
        serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
    let receipts = context["receipts"].as_array().unwrap();
    let (delta, reason) = if context["plan"].as_array().unwrap().is_empty() {
        (
            json!({"role":"assistant","content":json!({"steps":["Calculate the exact total","Ask for the original comparison criterion","Present saved evidence"]}).to_string()}),
            "stop",
        )
    } else if receipts.is_empty() {
        (
            json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"saved-sum".into(),"decimal_calculate",json!({"operation":"sum","operands":["0.1","0.2"]}))]}),
            "tool_calls",
        )
    } else if context["owner_inputs"].as_array().unwrap().is_empty() {
        (
            json!({"role":"assistant","content":json!({"input_request":{"field":"context","question":"Which original comparison criterion should the answer emphasize?","candidates":[]}}).to_string()}),
            "stop",
        )
    } else {
        assert_eq!(context["owner_inputs"][0]["value"], "Emphasize exact cost");
        assert_eq!(context["original"]["task"], context["accepted"]["task"]);
        let r = &receipts[0];
        (
            json!({"role":"assistant","content":json!({"text":"Exact cost is 0.3", "evidence":[{"step":r["step"],"call_id":r["call_id"]}]}).to_string()}),
            "stop",
        )
    };
    let data = frame(
        json!({"id":"flexible-context","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}),
    ) + &frame(
        json!({"id":"flexible-context","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(data))
        .unwrap()
}
#[tokio::test]
async fn mission_flexible_context_runtime_pauses_without_polling_and_resumes_completed_work() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = Arc::new(Model {
        calls: AtomicUsize::new(0),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(flexible_context_model))
        .with_state(script.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, s, u, _) = knowledge::tool_configured(&base).await;
    let m = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: "Compute the synthetic exact cost".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::DecimalEquals {
                    expected: "0.3".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let engine = Runtime::start(s.clone());
    engine.wake();
    let wait = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let view = s.missions(&u).await.unwrap().remove(0);
            if view.state == "waiting" {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(script.calls.load(Ordering::SeqCst), 2);
    assert_eq!(wait.completed_steps, 1);
    engine.shutdown().await;
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert_eq!(script.calls.load(Ordering::SeqCst), 2);
    let answer = MissionInputReply {
        request_id: uuid::Uuid::new_v4().to_string(),
        wait_request_id: wait.input_wait.unwrap().request_id,
        expected_revision: wait.revision,
        value: "Emphasize exact cost".into(),
    };
    s.reply_mission_input(&u, m.id.clone(), answer.clone())
        .await
        .unwrap();
    s.reply_mission_input(&u, m.id.clone(), answer)
        .await
        .unwrap();
    let engine = Runtime::start(s.clone());
    engine.wake();
    let done = finished(&s, &u, &m.id).await;
    assert_eq!(done.state, "partial", "{:?}", done.report);
    assert!(done.report.iter().any(|r| r.contains("owner review")));
    assert_eq!(done.completed_steps, 1);
    assert_eq!(script.calls.load(Ordering::SeqCst), 3);
    engine.shutdown().await;
    server.abort();
}
