//! Entirely synthetic loopback provider: answers are computed from received pixels.
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::post,
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use http_body_util::BodyExt;
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind, RunState};
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{CredentialChange, LoginResult, ProviderChange, Store};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;
use uuid::Uuid;
struct Script {
    calls: AtomicUsize,
    mode: AtomicUsize,
    release: tokio::sync::Semaphore,
}
impl Default for Script {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            mode: AtomicUsize::new(0),
            release: tokio::sync::Semaphore::new(0),
        }
    }
}
fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}
fn pixels(body: &Value) -> String {
    let image = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find_map(|m| {
            m["content"]
                .as_array()
                .and_then(|c| c.iter().find_map(|v| v["image_url"]["url"].as_str()))
        })
        .expect("actual image input");
    let encoded = image.strip_prefix("data:image/jpeg;base64,").unwrap();
    let bytes = STANDARD.decode(encoded).unwrap();
    let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
    let mut answer = String::new();
    for y in 0..4 {
        for x in 0..4 {
            let p = image
                .get_pixel(
                    (x * 2 + 1) * image.width() / 8,
                    (y * 2 + 1) * image.height() / 8,
                )
                .0;
            answer.push(if p[0] > 150 && p[1] > 150 {
                'Y'
            } else if p[0] > p[1] && p[0] > p[2] {
                'R'
            } else if p[1] > p[2] {
                'G'
            } else {
                'B'
            });
        }
    }
    answer
}
async fn provider(
    State(state): State<Arc<Script>>,
    Json(body): Json<Value>,
) -> axum::response::Response {
    state.calls.fetch_add(1, Ordering::SeqCst);
    let answer = pixels(&body);
    assert!(body.get("tools").is_none());
    let probe = body["max_tokens"] == 256;
    let mode = if probe {
        0
    } else {
        state.mode.load(Ordering::SeqCst)
    };
    let text = if probe {
        answer
    } else {
        format!("Observed synthetic tile pixels {answer}")
    };
    let first = frame(
        json!({"id":"fixture-image-response","model":"image-fixture","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
    );
    let last = if mode == 1 {
        frame(json!({"error":{"message":"synthetic failure"}}))
    } else {
        frame(
            json!({"id":"fixture-image-response","model":"image-fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":9,"completion_tokens":4}}),
        ) + "data: [DONE]\n\n"
    };
    let stream = futures_util::stream::unfold((0, state), move |(n, state)| {
        let first = first.clone();
        let last = last.clone();
        async move {
            match n {
                0 => Some((Ok::<_, std::io::Error>(first), (1, state))),
                1 => {
                    if mode == 2 {
                        state.release.acquire().await.unwrap().forget();
                    }
                    Some((Ok(last), (2, state)))
                }
                _ => None,
            }
        }
    });
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}
async fn send(
    app: &Router,
    login: &LoginResult,
    method: &str,
    path: &str,
    body: Vec<u8>,
    binary: bool,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "media.example.test")
                .header("origin", "https://media.example.test")
                .header(
                    "content-type",
                    if binary {
                        "application/octet-stream"
                    } else {
                        "application/json"
                    },
                )
                .header("cookie", format!("moosuri_session={}", login.token))
                .header("x-moosuri-account", &login.user.account().id)
                .body(Body::from(body))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn call(app: &Router, login: &LoginResult, method: &str, path: &str, body: Value) -> Value {
    let result = send(
        app,
        login,
        method,
        path,
        body.to_string().into_bytes(),
        false,
    )
    .await;
    assert!(result.status().is_success(), "{}", result.status());
    serde_json::from_slice(&result.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn wait(
    store: &Store,
    login: &LoginResult,
    thread: &str,
    partial: bool,
) -> moosuri_core::ChatTurn {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let page = store
                .conversation(&login.user, thread.into(), None)
                .await
                .unwrap();
            if let Some(t) = page.turns.last()
                && if partial {
                    !t.assistant.is_empty()
                } else {
                    !t.run.state.active()
                }
            {
                return t.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn upload_probe_chat_retry_reload_library_and_delete_during_response() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = Arc::new(Script::default());
    let app = Router::new()
        .route("/v1/chat/completions", post(provider))
        .with_state(script.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let password = "Synthetic-media-password-42";
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login("owner@example.test".into(), password.into())
        .await
        .unwrap();
    let p = store
        .create_provider(
            &login.user,
            ProviderChange {
                name: "Scripted pixels".into(),
                enabled: true,
                credential: CredentialChange::Clear,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(base),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&login.user, p.id.clone(), 1, "image-fixture".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&login.user, p.id.clone(), 1, "image-fixture".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &login.user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: p.id.clone(),
                provider_revision: 1,
                model: "image-fixture".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let runtime = moosuri_agent::Runtime::start(store.clone());
    let app = router_with_runtime(
        store.clone(),
        ServerConfig::new("https://media.example.test", false).unwrap(),
        runtime.clone(),
    );
    let (bytes, _) = moosuri_store::media::vision_challenge().unwrap();
    let id = Uuid::new_v4().to_string();
    let route = format!("/api/v1/media/{id}");
    let upload = send(&app, &login, "POST", &route, bytes.clone(), true).await;
    assert_eq!(upload.status(), StatusCode::OK);
    let receipt: Value =
        serde_json::from_slice(&upload.into_body().collect().await.unwrap().to_bytes()).unwrap();
    assert_eq!(receipt["state"], "ready");
    assert!(receipt.get("data").is_none());
    let dup = send(&app, &login, "POST", &route, bytes, true).await;
    assert_eq!(dup.status(), StatusCode::OK);
    assert_eq!(
        send(&app, &login, "POST", &route, b"conflict".to_vec(), true)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let thread = store
        .create_thread(&login.user, "images".into())
        .await
        .unwrap()
        .id;
    let request = Uuid::new_v4().to_string();
    let input = json!({"request_id":request,"text":"Describe the tiles","attachments":[{"id":id,"revision":1}]});
    let messages = format!("/api/v1/threads/{thread}/messages");
    assert_eq!(
        send(
            &app,
            &login,
            "POST",
            &messages,
            input.to_string().into_bytes(),
            false
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let probe = call(
        &app,
        &login,
        "POST",
        &format!("/api/v1/admin/providers/{}/vision-probe", p.id),
        json!({"revision":1,"model":"image-fixture","effort":null}),
    )
    .await;
    assert_eq!(probe["vision"], true);
    script.mode.store(1, Ordering::SeqCst);
    let admitted = call(&app, &login, "POST", &messages, input.clone()).await;
    let failed = wait(&store, &login, &thread, false).await;
    assert_eq!(failed.run.state, RunState::Failed);
    assert_eq!(failed.attachments.as_ref().unwrap()[0].id, id);
    script.mode.store(0, Ordering::SeqCst);
    call(
        &app,
        &login,
        "POST",
        &format!(
            "/api/v1/runs/{}/retry",
            admitted["run"]["id"].as_str().unwrap()
        ),
        json!({"expected_attempt":1}),
    )
    .await;
    let done = wait(&store, &login, &thread, false).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert!(
        done.assistant
            .iter()
            .any(|m| m.text.contains("Observed synthetic tile pixels"))
    );
    assert_eq!(done.attachments.as_ref().unwrap()[0].id, id);
    assert_eq!(done.run.usage.input_tokens, None); // Failed first dispatch has unknown usage; totals remain honestly unknown.
    assert_eq!(script.calls.load(Ordering::SeqCst), 3); // probe, failed image round, explicit retry; no mutator/intent round.
    let page = call(&app, &login, "GET", &messages, Value::Null).await;
    assert_eq!(page["turns"][0]["attachments"][0]["id"], id);
    let library = call(&app, &login, "GET", "/api/v1/media", Value::Null).await;
    assert_eq!(library["items"][0]["id"], id);
    let preview = send(
        &app,
        &login,
        "GET",
        &format!("{route}/thumbnail"),
        vec![],
        false,
    )
    .await;
    assert_eq!(preview.status(), StatusCode::OK);
    assert_eq!(preview.headers()["cache-control"], "no-store");
    assert_eq!(preview.headers()["content-type"], "image/jpeg");
    let invite = store
        .create_user(
            &login.user,
            "Synthetic second owner".into(),
            "second@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = store
        .login("second@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let second = store
        .change_password(
            &restricted.user,
            "Synthetic-second-media-password-42".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        send(
            &app,
            &second,
            "GET",
            &format!("{route}/content"),
            vec![],
            false
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            &login,
            "POST",
            &format!("/api/v1/media/{}", Uuid::new_v4()),
            b"<svg onload=attack>".to_vec(),
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        send(
            &app,
            &login,
            "POST",
            &format!("/api/v1/media/{}", Uuid::new_v4()),
            vec![0; moosuri_store::media::MAX_BYTES + 1],
            true
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    script.mode.store(2, Ordering::SeqCst);
    let input = json!({"request_id":Uuid::new_v4().to_string(),"text":"Describe again","attachments":[{"id":id,"revision":1}]});
    call(&app, &login, "POST", &messages, input).await;
    wait(&store, &login, &thread, true).await;
    assert_eq!(
        send(&app, &login, "DELETE", &route, vec![], false)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    script.release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(350)).await;
    let page = store.conversation(&login.user, thread, None).await.unwrap();
    assert!(
        page.turns
            .iter()
            .all(|t| t.assistant.is_empty() && !t.run.can_retry)
    );
    assert_eq!(
        send(
            &app,
            &login,
            "GET",
            &format!("{route}/content"),
            vec![],
            false
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    runtime.shutdown().await;
    task.abort();
}
