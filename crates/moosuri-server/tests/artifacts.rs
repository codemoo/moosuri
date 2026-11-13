use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_server::{ServerConfig, router};
use moosuri_store::{LoginResult, Store};
use serde_json::{Value, json};
use tower::ServiceExt;
async fn send(
    app: &Router,
    login: &LoginResult,
    method: &str,
    path: &str,
    body: Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "artifact.example.test")
                .header("origin", "https://artifact.example.test")
                .header("content-type", "application/json")
                .header("cookie", format!("moosuri_session={}", login.token))
                .header("x-moosuri-account", &login.user.account().id)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn json_body(r: axum::response::Response) -> Value {
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
#[tokio::test]
async fn authenticated_editor_receipts_history_conflict_export_and_delete() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let pass = "Synthetic-artifact-HTTP-password";
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            pass.into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login("owner@example.test".into(), pass.into())
        .await
        .unwrap();
    let app = router(
        store.clone(),
        ServerConfig::new("https://artifact.example.test", false).unwrap(),
    );
    let mut input = json!({"request_id":"a0000000-0000-4000-8000-000000000001","id":null,"expected_revision":null,"draft":{"title":"합성 문서","body":"<script>never execute</script>\n![remote](https://invalid.example/x)","payload":{"kind":"document"},"references":[]}});
    let created = send(&app, &login, "POST", "/api/v1/artifacts", input.clone()).await;
    assert_eq!(created.status(), StatusCode::OK);
    let receipt = json_body(created).await;
    assert_eq!(
        json_body(send(&app, &login, "POST", "/api/v1/artifacts", input.clone()).await).await,
        receipt
    );
    let id = receipt["id"].as_str().unwrap();
    let path = format!("/api/v1/artifacts/{id}");
    let got = json_body(send(&app, &login, "GET", &path, Value::Null).await).await;
    assert_eq!(got["draft"]["title"], "합성 문서");
    input["request_id"] = json!("a0000000-0000-4000-8000-000000000002");
    input["id"] = json!(id);
    input["expected_revision"] = json!(1);
    input["draft"]["body"] = json!("사용자가 편집한 내용");
    assert_eq!(
        send(&app, &login, "POST", "/api/v1/artifacts", input.clone())
            .await
            .status(),
        StatusCode::OK
    );
    input["request_id"] = json!("a0000000-0000-4000-8000-000000000003");
    assert_eq!(
        send(&app, &login, "POST", "/api/v1/artifacts", input)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let history =
        json_body(send(&app, &login, "GET", &format!("{path}/history"), Value::Null).await).await;
    assert_eq!(history["items"].as_array().unwrap().len(), 2);
    let html = send(
        &app,
        &login,
        "GET",
        &format!("{path}/export?revision=1&format=html"),
        Value::Null,
    )
    .await;
    assert_eq!(html.status(), StatusCode::OK);
    assert!(
        html.headers()["content-security-policy"]
            .to_str()
            .unwrap()
            .contains("sandbox")
    );
    assert_eq!(html.headers()["cache-control"], "no-store");
    let text = String::from_utf8(
        html.into_body()
            .collect()
            .await
            .unwrap()
            .to_bytes()
            .to_vec(),
    )
    .unwrap();
    assert!(!text.contains("<script>"));
    assert!(text.contains("&lt;script&gt;"));
    let deletion =
        json!({"request_id":"a0000000-0000-4000-8000-000000000004","expected_revision":2});
    let a = json_body(send(&app, &login, "DELETE", &path, deletion.clone()).await).await;
    let b = json_body(send(&app, &login, "DELETE", &path, deletion).await).await;
    assert_eq!(a, b);
    assert_eq!(
        send(
            &app,
            &login,
            "GET",
            &format!("{path}/export?revision=1&format=html"),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let mut unknown = json!({"request_id":"a0000000-0000-4000-8000-000000000005","id":null,"expected_revision":null,"draft":{"title":"x","body":"x","payload":{"kind":"document"},"references":[]}});
    unknown["tenant_id"] = json!("injected");
    assert_eq!(
        send(&app, &login, "POST", "/api/v1/artifacts", unknown)
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}

#[derive(Clone)]
struct ModelScript {
    target: std::sync::Arc<tokio::sync::Mutex<Option<(String, i64)>>>,
}
async fn model_reply(
    axum::extract::State(s): axum::extract::State<ModelScript>,
    axum::Json(input): axum::Json<Value>,
) -> axum::response::Response {
    let planning = input["tools"][0]["function"]["name"] == "artifact_intent";
    let text = input["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    let target = s.target.lock().await.clone();
    let (name, args) = if planning {
        (
            "artifact_intent",
            json!({"decision":"command","action":if text.contains("수정"){"edit"}else{"create"},"kind":"document","intent_quote":text,"target_quote":null}),
        )
    } else if let Some((id, revision)) = target {
        let read = input["messages"]
            .as_array()
            .unwrap()
            .iter()
            .rev()
            .find(|m| m["role"] == "tool")
            .and_then(|m| serde_json::from_str::<Value>(m["content"].as_str().unwrap()).ok());
        if let Some(body) = read.as_ref().and_then(|v| v["value"]["body"].as_str()) {
            (
                "artifact_write",
                json!({"title":"견적 비교","body":"이후 대화에서 고친 결과","payload":{"kind":"document"},"anchor":body}),
            )
        } else {
            (
                "artifact_read",
                json!({"id":id,"revision":revision,"start_byte":0,"max_bytes":8192}),
            )
        }
    } else {
        (
            "artifact_write",
            json!({"title":"견적 비교","body":"A 15, B 14.25. B가 0.75 낮음.","payload":{"kind":"document"},"anchor":null}),
        )
    };
    let event = json!({"id":"http-artifact-model","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[{"index":0,"id":format!("http-{name}-call"),"type":"function","function":{"name":name,"arguments":args.to_string()}}]},"finish_reason":"tool_calls"}]});
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(format!("data: {event}\n\ndata: [DONE]\n\n")))
        .unwrap()
}
#[tokio::test]
async fn http_chat_receipt_editor_and_later_chat_share_one_durable_report() {
    use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
    use moosuri_store::{CredentialChange, ProviderChange};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = ModelScript {
        target: Default::default(),
    };
    let model = Router::new()
        .route("/v1/chat/completions", axum::routing::post(model_reply))
        .with_state(script.clone());
    let task = tokio::spawn(async move { axum::serve(listener, model).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let pass = "Synthetic-report-journey-password";
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            pass.into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login("owner@example.test".into(), pass.into())
        .await
        .unwrap();
    let p = store
        .create_provider(
            &login.user,
            ProviderChange {
                name: "loopback".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(base),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &login.user,
            p.id.clone(),
            p.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_tool_probe(
            &login.user,
            p.id.clone(),
            p.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &login.user,
            Some(ModelSelection {
                provider_id: p.id,
                provider_revision: p.revision,
                model: "fixture-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let runtime = moosuri_agent::Runtime::start(store.clone());
    let app = moosuri_server::router_with_runtime(
        store.clone(),
        ServerConfig::new("https://artifact.example.test", false).unwrap(),
        runtime.clone(),
    );
    async fn terminal(app: &Router, login: &LoginResult, thread: &str) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(6), async {
            loop {
                let p = json_body(
                    send(
                        app,
                        login,
                        "GET",
                        &format!("/api/v1/threads/{thread}/messages"),
                        Value::Null,
                    )
                    .await,
                )
                .await;
                let r = &p["turns"][0];
                if !matches!(r["run"]["state"].as_str(), Some("queued" | "running")) {
                    return r.clone();
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await
            }
        })
        .await
        .unwrap()
    }
    let thread = store
        .create_thread(&login.user, "first".into())
        .await
        .unwrap()
        .id;
    let body = json!({"request_id":"b0000000-0000-4000-8000-000000000001","text":"A 견적은15, B 견적은14.25입니다. 비교하고 보고서로 저장해 줘"});
    assert_eq!(
        send(
            &app,
            &login,
            "POST",
            &format!("/api/v1/threads/{thread}/messages"),
            body.clone()
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let done = terminal(&app, &login, &thread).await;
    assert_eq!(done["run"]["state"], "completed");
    let id = done["run"]["artifact_receipts"][0]["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        send(
            &app,
            &login,
            "POST",
            &format!("/api/v1/threads/{thread}/messages"),
            body
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let read = json_body(
        send(
            &app,
            &login,
            "GET",
            &format!("/api/v1/artifacts/{id}"),
            Value::Null,
        )
        .await,
    )
    .await;
    let mut draft = read["draft"].clone();
    draft["body"] = json!("편집기에서 확인한 내용");
    let edited = json!({"request_id":"b0000000-0000-4000-8000-000000000002","id":id,"expected_revision":1,"draft":draft});
    assert_eq!(
        send(&app, &login, "POST", "/api/v1/artifacts", edited)
            .await
            .status(),
        StatusCode::OK
    );
    *script.target.lock().await = Some((id.clone(), 2));
    let second = store
        .create_thread(&login.user, "later".into())
        .await
        .unwrap()
        .id;
    assert_eq!(send(&app,&login,"POST",&format!("/api/v1/threads/{second}/messages"),json!({"request_id":"b0000000-0000-4000-8000-000000000003","text":"선택한 문서를 수정해 줘","artifact_target":{"id":id,"revision":2}})).await.status(),StatusCode::CREATED);
    let done = terminal(&app, &login, &second).await;
    assert_eq!(done["run"]["state"], "completed");
    assert_eq!(done["run"]["artifact_receipts"][0]["id"], id);
    let actual = json_body(
        send(
            &app,
            &login,
            "GET",
            &format!("/api/v1/artifacts/{id}"),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(actual["artifact"]["revision"], 3);
    assert_eq!(actual["draft"]["body"], "이후 대화에서 고친 결과");
    runtime.shutdown().await;
    task.abort();
}
