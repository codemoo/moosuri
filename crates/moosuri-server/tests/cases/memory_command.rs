//! Actual HTTP/SSE/account boundaries with a scripted local model, not an intent quality evaluation.
use super::*;
use axum::{extract::State, response::IntoResponse, routing::post};
use moosuri_agent::Runtime;
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
use moosuri_server::router_with_runtime;
use moosuri_store::{CredentialChange, ProviderChange, UserContext};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

const REMEMBER: &str = "나는 홍차를 좋아해. 이 사실을 기억해 줘.";
const CORRECT: &str = "홍차 취향을 수정해 줘. 나는 녹차를 좋아해.";
const FORGET: &str = "녹차 취향을 잊어 줘.";
const PRIVATE: &str = "SYNTHETIC-PRIVATE-PLANNER-PREVIEW";
const ANSWER: &str = "합성 근거 답변: 홍차를 좋아해요.";

fn request_id() -> String {
    static NEXT: AtomicUsize = AtomicUsize::new(1);
    format!(
        "00000000-0000-4000-8000-{:012x}",
        NEXT.fetch_add(1, Ordering::SeqCst)
    )
}
#[derive(Clone)]
struct ModelState {
    calls: Arc<AtomicUsize>,
    hold: Arc<AtomicBool>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
struct MemoryModel {
    base: String,
    state: ModelState,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for MemoryModel {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn tool(name: &str, args: Value, call: usize) -> Value {
    json!({"role":"assistant","tool_calls":[{"index":0,"id":format!("synthetic-{call}"),"type":"function",
        "function":{"name":name,"arguments":args.to_string()}}]})
}
async fn model_round(
    State(state): State<ModelState>,
    axum::Json(input): axum::Json<Value>,
) -> axum::response::Response {
    let call = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
    assert_eq!(input["model"], "memory-command-http-model");
    let messages = input["messages"].as_array().unwrap();
    let planning = input["tools"][0]["function"]["name"] == "memory_command_decision";
    let (delta, finish) = if planning {
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0]["role"], "system");
        assert_eq!(messages[1]["role"], "user");
        let (action, intent, fact, target) = match messages[1]["content"].as_str().unwrap() {
            REMEMBER => ("remember", "기억해 줘", Some("나는 홍차를 좋아해"), None),
            CORRECT => (
                "correct",
                "수정해 줘",
                Some("나는 녹차를 좋아해"),
                Some("홍차"),
            ),
            FORGET => ("forget", "잊어 줘", None, Some("녹차")),
            _ => panic!("unexpected synthetic command"),
        };
        if state.hold.load(Ordering::SeqCst) {
            state.entered.notify_one();
            state.release.notified().await;
        }
        (
            tool(
                "memory_command_decision",
                json!({"decision":"command","clarification":null,"action":action,
            "intent_quote":intent,"fact_quote":fact,"subject":fact.map(|_|"음료 취향"),"target_quote":target}),
                call,
            ),
            "tool_calls",
        )
    } else {
        assert!(!input.to_string().contains(PRIVATE));
        let result = messages.iter().rev().find(|m| m["role"] == "tool");
        if let Some(result) = result {
            let result: Value = serde_json::from_str(result["content"].as_str().unwrap()).unwrap();
            assert_eq!(result["kind"], "search");
            assert_eq!(result["value"]["hits"][0]["snippet"], "나는 홍차를 좋아해");
            (json!({"role":"assistant","content":ANSWER}), "stop")
        } else {
            (
                tool(
                    "knowledge_search",
                    json!({"kind":"memory","text":"홍차","collection_id":null,
                "subject":null,"speaker":null,"valid_at":null}),
                    call,
                ),
                "tool_calls",
            )
        }
    };
    let id = format!("memory-http-{call}");
    let preview = json!({"id":id,"model":"memory-command-http-model","choices":[{"index":0,
        "delta":{"role":"assistant","content":if planning {PRIVATE} else {""}},"finish_reason":null}]});
    let answer = json!({"id":id,"model":"memory-command-http-model","choices":[{"index":0,"delta":delta,"finish_reason":finish}]});
    let usage = json!({"id":id,"model":"memory-command-http-model","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":3}});
    let wire = format!("data: {preview}\n\ndata: {answer}\n\ndata: {usage}\n\ndata: [DONE]\n\n");
    ([("content-type", "text/event-stream")], wire).into_response()
}
async fn synthetic_model() -> MemoryModel {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = ModelState {
        calls: Arc::new(AtomicUsize::new(0)),
        hold: Arc::new(AtomicBool::new(false)),
        entered: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
    };
    let app = Router::new()
        .route("/v1/chat/completions", post(model_round))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    MemoryModel { base, state, task }
}
async fn configured(
    model: &MemoryModel,
) -> (
    tempfile::TempDir,
    Store,
    Runtime,
    Router,
    UserContext,
    String,
) {
    let dir = tempfile::Builder::new()
        .prefix("moosuri-memory-command-http-")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let store = Store::open(&dir.path().join("data")).unwrap();
    store
        .bootstrap(
            "Synthetic owner".into(),
            "admin@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    let app = router_with_runtime(
        store.clone(),
        ServerConfig::new(ORIGIN, false).unwrap(),
        runtime.clone(),
    );
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let user = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let provider = store
        .create_provider(
            &user,
            ProviderChange {
                name: "Synthetic memory command HTTP model".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(model.base.clone()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    // As in chat.rs, fixture-only Store setup attests the synthetic transport.
    // Separate provider boundary tests exercise the real two-round proof endpoint.
    store
        .record_model_probe(
            &user,
            provider.id.clone(),
            1,
            "memory-command-http-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_tool_probe(
            &user,
            provider.id.clone(),
            1,
            "memory-command-http-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                provider_id: provider.id,
                provider_revision: 1,
                model: "memory-command-http-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    (dir, store, runtime, app, user, cookie)
}
async fn page(app: &Router, cookie: &str, thread: &str) -> Value {
    let response = send(
        app,
        "GET",
        &format!("/api/v1/threads/{thread}/messages"),
        Some(cookie),
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let page = value(response).await;
    assert!(!page.to_string().contains(PRIVATE));
    page
}
async fn submit(app: &Router, cookie: &str, thread: &str, text: &str) -> Value {
    let response = send(
        app,
        "POST",
        &format!("/api/v1/threads/{thread}/messages"),
        Some(cookie),
        json!({"request_id":request_id(),"text":text}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    value(response).await
}
async fn completed(app: &Router, cookie: &str, thread: &str, run: &Value) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = page(app, cookie, thread).await;
            let turn = page["turns"]
                .as_array()
                .unwrap()
                .iter()
                .find(|turn| turn["run"]["id"] == *run)
                .unwrap();
            let state = turn["run"]["state"].as_str().unwrap();
            if state == "completed" {
                return page;
            }
            assert!(
                matches!(state, "queued" | "running"),
                "command ended in {state}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("HTTP submitted command completes")
}
fn confirmation(page: &Value, run: &Value, text: &str) {
    let turn = page["turns"]
        .as_array()
        .unwrap()
        .iter()
        .find(|turn| turn["run"]["id"] == *run)
        .unwrap();
    assert_eq!(turn["assistant"].as_array().unwrap().len(), 1);
    assert_eq!(turn["assistant"][0]["text"], text);
    assert_eq!(turn["run"]["state"], "completed");
}
async fn reset(app: &Router, cookie: &str, thread: &str, after: &Value) {
    let response = send(
        app,
        "GET",
        &format!("/api/v1/threads/{thread}/events?after={after}"),
        Some(cookie),
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert!(
        response.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let bytes = tokio::time::timeout(Duration::from_secs(2), response.into_body().collect())
        .await
        .expect("reset stream closes")
        .unwrap()
        .to_bytes();
    let wire = std::str::from_utf8(&bytes).unwrap();
    assert!(wire.contains("event: reset"), "old cursor must reset");
    assert!(!wire.contains(ANSWER));
    assert!(!wire.contains(PRIVATE));
    assert!(!wire.contains("\"kind\":\"delta\""));
}
fn revoked(page: &Value) {
    let turn = &page["turns"][0];
    assert!(turn["assistant"].as_array().unwrap().is_empty());
    assert!(turn["run"]["evidence"].as_array().unwrap().is_empty());
    assert_eq!(turn["run"]["state"], "incomplete");
    assert_eq!(turn["run"]["error"], "knowledge_invalidated");
    assert_eq!(turn["run"]["can_retry"], false);
}

#[tokio::test]
async fn memory_commands_reset_http_replay_and_logout_fences_a_held_planner() {
    let _guard = TEST_LOCK.lock().await;
    let model = synthetic_model().await;
    let (_dir, store, runtime, app, user, cookie) = configured(&model).await;
    let thread = store
        .create_thread(&user, "HTTP memory commands".into())
        .await
        .unwrap();
    let before = page(&app, &cookie, &thread.id).await;
    let remembered = submit(&app, &cookie, &thread.id, REMEMBER).await;
    let remembered_page = completed(&app, &cookie, &thread.id, &remembered["run"]["id"]).await;
    confirmation(
        &remembered_page,
        &remembered["run"]["id"],
        "기억을 저장했습니다.",
    );
    reset(&app, &cookie, &thread.id, &before["cursor"]).await;
    let memory = store.memories(&user, None).await.unwrap().items.remove(0);
    assert_eq!(memory.draft.text, "나는 홍차를 좋아해");
    assert_eq!(
        remembered_page["turns"][0]["run"]["evidence"][0]["id"],
        memory.id
    );

    // The old answer is derived by the actual knowledge tool, in another chat.
    // Command-driven revocation must reach that HTTP replay stream as well.
    let recall_thread = store
        .create_thread(&user, "HTTP memory evidence".into())
        .await
        .unwrap();
    let recall = submit(
        &app,
        &cookie,
        &recall_thread.id,
        "내가 좋아하는 음료는 무엇이야?",
    )
    .await;
    let recall_page = completed(&app, &cookie, &recall_thread.id, &recall["run"]["id"]).await;
    confirmation(&recall_page, &recall["run"]["id"], ANSWER);
    assert_eq!(
        recall_page["turns"][0]["run"]["evidence"][0]["id"],
        memory.id
    );

    let corrected = submit(&app, &cookie, &thread.id, CORRECT).await;
    let corrected_page = completed(&app, &cookie, &thread.id, &corrected["run"]["id"]).await;
    confirmation(
        &corrected_page,
        &corrected["run"]["id"],
        "기억을 바로잡았습니다.",
    );
    reset(&app, &cookie, &thread.id, &remembered_page["cursor"]).await;
    reset(&app, &cookie, &recall_thread.id, &recall_page["cursor"]).await;
    revoked(&page(&app, &cookie, &recall_thread.id).await);
    assert!(
        corrected_page["turns"][0]["assistant"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        store
            .memory(&user, memory.id.clone(), None)
            .await
            .unwrap()
            .draft
            .text,
        "나는 녹차를 좋아해"
    );

    let forgotten = submit(&app, &cookie, &thread.id, FORGET).await;
    let forgotten_page = completed(&app, &cookie, &thread.id, &forgotten["run"]["id"]).await;
    confirmation(
        &forgotten_page,
        &forgotten["run"]["id"],
        "기억과 수정 이력을 지웠습니다. 원문 자료는 유지됩니다.",
    );
    reset(&app, &cookie, &thread.id, &corrected_page["cursor"]).await;
    assert!(
        forgotten_page["turns"][2]["run"]["evidence"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(
        forgotten_page["turns"][1]["assistant"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 5);

    // HTTP logout holds the shared delivery writer gate while invalidating the
    // session. A provider response arriving later cannot reauthorize its old run.
    model.state.hold.store(true, Ordering::SeqCst);
    let held = submit(&app, &cookie, &thread.id, REMEMBER).await;
    tokio::time::timeout(Duration::from_secs(5), model.state.entered.notified())
        .await
        .expect("planner entered");
    let held_page = page(&app, &cookie, &thread.id).await;
    let held_stream = send(
        &app,
        "GET",
        &format!(
            "/api/v1/threads/{}/events?after={}",
            thread.id, held_page["cursor"]
        ),
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert_eq!(held_stream.status(), StatusCode::OK);
    let response = send(
        &app,
        "POST",
        "/api/v1/auth/logout",
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    assert!(!response.headers().contains_key(header::SET_COOKIE));
    model.state.release.notify_one();
    let closed = tokio::time::timeout(Duration::from_secs(2), held_stream.into_body().collect())
        .await
        .expect("logged-out SSE closes")
        .unwrap()
        .to_bytes();
    let closed = std::str::from_utf8(&closed).unwrap();
    assert!(closed.contains("event: closed"));
    assert!(closed.contains("unauthorized"));
    assert!(!closed.contains(PRIVATE));
    assert!(!closed.contains("기억을 저장했습니다."));
    assert!(!closed.contains("\"kind\":\"delta\""));
    let fresh_cookie = login(&app, "admin@example.test", PASSWORD).await;
    let fresh_user = store
        .authenticate(
            fresh_cookie
                .strip_prefix("moosuri_session=")
                .unwrap()
                .into(),
        )
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let current = page(&app, &fresh_cookie, &thread.id).await;
            let turn = current["turns"]
                .as_array()
                .unwrap()
                .iter()
                .find(|turn| turn["run"]["id"] == held["run"]["id"])
                .unwrap();
            if !matches!(turn["run"]["state"].as_str().unwrap(), "queued" | "running") {
                assert_ne!(turn["run"]["state"], "completed");
                assert!(turn["assistant"].as_array().unwrap().is_empty());
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("logged-out planner terminates");
    runtime.shutdown().await;
    assert!(
        store
            .memories(&fresh_user, None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 6);
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("/api/v1/threads/{}/messages", thread.id),
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let usage = value(
        send(
            &app,
            "GET",
            "/api/v1/usage",
            Some(&fresh_cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(usage["dispatched_calls"], 6);
}
