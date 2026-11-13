//! Actual Router/Runtime/Store knowledge revocation with a synthetic loopback model.
use super::*;
use axum::{extract::State, response::IntoResponse, routing::post};
use moosuri_agent::Runtime;
use moosuri_core::{IngestBatch, ModelSelection, ProviderConfig, ProviderKind, SourceInput};
use moosuri_server::router_with_runtime;
use moosuri_store::{CredentialChange, ProviderChange, UserContext};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

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
    hold_answer: Arc<AtomicBool>,
    answer_entered: Arc<Notify>,
    answer_release: Arc<Notify>,
}
struct KnowledgeModel {
    base: String,
    state: ModelState,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for KnowledgeModel {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn tool_delta(id: &str, name: &str, arguments: Value) -> Value {
    json!({"role":"assistant","tool_calls":[{"index":0,"id":id,"type":"function","function":{"name":name,"arguments":arguments.to_string()}}]})
}
async fn model_round(
    State(state): State<ModelState>,
    axum::Json(input): axum::Json<Value>,
) -> axum::response::Response {
    let call = state.calls.fetch_add(1, Ordering::SeqCst);
    assert_eq!(input["model"], "knowledge-http-model");
    let tools: std::collections::BTreeSet<_> = input["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["function"]["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        tools,
        [
            "artifact_read",
            "artifact_search",
            "decimal_calculate",
            "followup_briefing",
            "knowledge_read_memory",
            "knowledge_read_source",
            "knowledge_search",
            "skill_read",
        ]
        .into_iter()
        .collect()
    );
    let results = input["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|message| message["role"] == "tool")
        .map(|message| serde_json::from_str::<Value>(message["content"].as_str().unwrap()).unwrap())
        .collect::<Vec<_>>();
    let (delta, finish) = match results.as_slice() {
        [] => (
            tool_delta(
                "search-call",
                "knowledge_search",
                json!({"kind":"source","text":"Friday","collection_id":null,"subject":null,"speaker":null,"valid_at":null}),
            ),
            "tool_calls",
        ),
        [search] => {
            assert_eq!(search["kind"], "search");
            let hits = search["value"]["hits"].as_array().unwrap();
            assert_eq!(hits.len(), 1);
            (
                tool_delta(
                    "read-call",
                    "knowledge_read_source",
                    json!({"id":hits[0]["id"],"revision":hits[0]["revision"],"start_byte":0,"max_bytes":4096}),
                ),
                "tool_calls",
            )
        }
        [search, source] => {
            assert_eq!(source["kind"], "source");
            assert_eq!(
                source["value"]["source"]["id"],
                search["value"]["hits"][0]["id"]
            );
            assert_eq!(source["value"]["source"]["revision"], 1);
            assert_eq!(source["value"]["start_byte"], 0);
            assert_eq!(source["value"]["next_start_byte"], Value::Null);
            let text = source["value"]["text"].as_str().unwrap();
            assert!(text.contains("Friday"));
            if state.hold_answer.load(Ordering::SeqCst) {
                // Both local reads have committed before this request can exist.
                state.answer_entered.notify_one();
                state.answer_release.notified().await;
            }
            (
                json!({"role":"assistant","content":format!("Synthetic grounded answer: {text}")}),
                "stop",
            )
        }
        _ => panic!("unexpected automatic extra round"),
    };
    let id = format!("knowledge-http-round-{call}");
    let first = json!({"id":id,"model":"knowledge-http-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let last = json!({"id":id,"model":"knowledge-http-model","choices":[{"index":0,"delta":{},"finish_reason":finish}]});
    let usage = json!({"id":id,"model":"knowledge-http-model","choices":[],"usage":{"prompt_tokens":11,"completion_tokens":3}});
    let wire = format!("data: {first}\n\ndata: {last}\n\ndata: {usage}\n\ndata: [DONE]\n\n");
    ([("content-type", "text/event-stream")], wire).into_response()
}
async fn synthetic_model() -> KnowledgeModel {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = ModelState {
        calls: Arc::new(AtomicUsize::new(0)),
        hold_answer: Arc::new(AtomicBool::new(false)),
        answer_entered: Arc::new(Notify::new()),
        answer_release: Arc::new(Notify::new()),
    };
    let app = Router::new()
        .route("/v1/chat/completions", post(model_round))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    KnowledgeModel { base, state, task }
}
async fn configured(
    model: &KnowledgeModel,
) -> (
    tempfile::TempDir,
    Store,
    Runtime,
    Router,
    UserContext,
    String,
) {
    let dir = tempfile::Builder::new()
        .prefix("moosuri-knowledge-http-")
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
                name: "Synthetic knowledge HTTP model".into(),
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
            "knowledge-http-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_tool_probe(
            &user,
            provider.id.clone(),
            1,
            "knowledge-http-model".into(),
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
                model: "knowledge-http-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    (dir, store, runtime, app, user, cookie)
}
async fn source(store: &Store, user: &UserContext, text: &str) -> String {
    let collection = store
        .create_collection(user, "Synthetic source collection".into(), request_id())
        .await
        .unwrap();
    store
        .ingest_text(
            user,
            collection.id,
            IngestBatch {
                request_id: request_id(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "synthetic-friday".into(),
                    version: "1".into(),
                    expected_revision: None,
                    title: "Synthetic Friday note".into(),
                    text: text.into(),
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
        .remove(0)
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
    value(response).await
}
async fn wait_completed(app: &Router, cookie: &str, thread: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = page(app, cookie, thread).await;
            let state = page["turns"][0]["run"]["state"].as_str().unwrap();
            if state == "completed" {
                return page;
            }
            assert!(
                matches!(state, "queued" | "running"),
                "synthetic agent stopped before completion: {state}"
            );
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("synthetic search/read/final answer completes")
}
async fn submit(app: &Router, cookie: &str, thread: &str) {
    let response = send(
        app,
        "POST",
        &format!("/api/v1/threads/{thread}/messages"),
        Some(cookie),
        json!({"request_id":request_id(),"text":"What does my Friday note say?"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
}
async fn delete_and_check_reset(
    app: &Router,
    cookie: &str,
    thread: &str,
    source: &str,
    cursor: &Value,
) {
    let response = send(
        app,
        "DELETE",
        &format!("/api/v1/knowledge/sources/{source}"),
        Some(cookie),
        json!({"expected_revision":1}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::NO_CONTENT);
    let response = send(
        app,
        "GET",
        &format!("/api/v1/threads/{thread}/events?after={cursor}"),
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
    assert!(wire.contains("event: reset"));
    assert!(!wire.contains("Synthetic grounded answer"));
    assert!(!wire.contains("Synthetic Friday note"));
    let response = send(
        app,
        "GET",
        &format!("/api/v1/knowledge/sources/{source}?revision=1"),
        Some(cookie),
        Value::Null,
    )
    .await;
    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
fn assert_purged(page: &Value) {
    let turn = &page["turns"][0];
    assert_eq!(turn["user_text"], "What does my Friday note say?");
    assert!(turn["assistant"].as_array().unwrap().is_empty());
    assert!(turn["run"]["evidence"].as_array().unwrap().is_empty());
    assert_eq!(turn["run"]["state"], "incomplete");
    assert_eq!(turn["run"]["error"], "knowledge_invalidated");
    assert_eq!(turn["run"]["can_retry"], false);
}

#[tokio::test]
async fn search_read_answer_http_delete_resets_replay_and_blocks_a_held_late_answer() {
    let _guard = TEST_LOCK.lock().await;
    let model = synthetic_model().await;
    let (_dir, store, runtime, app, user, cookie) = configured(&model).await;
    let text = "Friday synthetic meeting is at 09:00. 🌱";
    let first_source = source(&store, &user, text).await;
    let first_thread = store
        .create_thread(&user, "Grounded answer".into())
        .await
        .unwrap();
    submit(&app, &cookie, &first_thread.id).await;
    let completed = wait_completed(&app, &cookie, &first_thread.id).await;
    let turn = &completed["turns"][0];
    assert_eq!(
        turn["assistant"][0]["text"],
        format!("Synthetic grounded answer: {text}")
    );
    assert_eq!(turn["run"]["model"]["function_tools"], true);
    assert_eq!(
        turn["run"]["evidence"],
        json!([{"kind":"source","id":first_source,"revision":1,"title":"Synthetic Friday note"}])
    );
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 3);
    delete_and_check_reset(
        &app,
        &cookie,
        &first_thread.id,
        &first_source,
        &completed["cursor"],
    )
    .await;
    assert_purged(&page(&app, &cookie, &first_thread.id).await);

    // A second request reaches the final provider call with saved search/read
    // dependencies, then the owner deletes its source before the answer arrives.
    let second_source = source(&store, &user, "Friday held synthetic private note").await;
    let second_thread = store
        .create_thread(&user, "Held late answer".into())
        .await
        .unwrap();
    model.state.hold_answer.store(true, Ordering::SeqCst);
    submit(&app, &cookie, &second_thread.id).await;
    tokio::time::timeout(
        Duration::from_secs(5),
        model.state.answer_entered.notified(),
    )
    .await
    .expect("provider received both saved read results");
    let before = page(&app, &cookie, &second_thread.id).await;
    assert_eq!(before["turns"][0]["run"]["state"], "running");
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 6);
    let mut changed = runtime.subscribe();
    let previous = *changed.borrow_and_update();
    delete_and_check_reset(
        &app,
        &cookie,
        &second_thread.id,
        &second_source,
        &before["cursor"],
    )
    .await;
    model.state.answer_release.notify_one();
    // The deletion wakes once; wait for additional worker progress. Either it
    // consumes the released answer and rejects publication or cancels locally
    // on its authority tick. Both must happen before the final snapshot check.
    tokio::time::timeout(Duration::from_secs(3), async {
        while *changed.borrow_and_update() < previous + 2 {
            changed.changed().await.unwrap();
        }
    })
    .await
    .expect("worker observes late answer or authority fence");
    assert_purged(&page(&app, &cookie, &second_thread.id).await);
    runtime.shutdown().await; // Drain the worker, then check again for late publication.
    assert_purged(&page(&app, &cookie, &second_thread.id).await);
    assert_purged(&page(&app, &cookie, &first_thread.id).await);
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 6); // No replay/fallback.
    let usage = value(send(&app, "GET", "/api/v1/usage", Some(&cookie), Value::Null).await).await;
    assert_eq!(usage["dispatched_calls"], 6); // Deletion retains accounting.
}
