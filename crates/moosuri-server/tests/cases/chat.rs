use super::*;
use axum::{response::IntoResponse, routing::post};
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
use moosuri_store::{CredentialChange, ProviderChange};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;
use tokio::sync::Notify;

struct SyntheticModel {
    base: String,
    calls: Arc<AtomicUsize>,
    release: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for SyntheticModel {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn synthetic_model() -> SyntheticModel {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let release = Arc::new(Notify::new());
    let count = calls.clone();
    let gate = release.clone();
    let app=Router::new().route("/v1/chat/completions",post(move |axum::Json(input):axum::Json<Value>| {
        let count=count.clone();let gate=gate.clone();
        async move {
            count.fetch_add(1,Ordering::SeqCst);
            assert_eq!(input["model"],"fixture-model");
            assert!(!input["messages"].as_array().unwrap().last().unwrap()["content"].as_str().unwrap().is_empty());
            let stream=futures_util::stream::unfold((0,gate),|(step,gate)| async move {
                let text=match step {
                    0 => "data: {\"id\":\"fixture-chat\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Synthetic partial\"},\"finish_reason\":null}]}\n\n".to_owned(),
                    1 => {
                        gate.notified().await;
                        concat!("data: {\"id\":\"fixture-chat\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{\"content\":\" answer\"},\"finish_reason\":null}]}\n\n",
                        "data: {\"id\":\"fixture-chat\",\"model\":\"fixture-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
                        "data: {\"id\":\"fixture-chat\",\"model\":\"fixture-model\",\"choices\":[],\"usage\":{\"prompt_tokens\":13,\"completion_tokens\":5}}\n\n",
                        "data: [DONE]\n\n").to_owned()
                    },
                    _ => return None,
                };
                Some((Ok::<_,std::io::Error>(text),(step+1,gate)))
            });
            ([("content-type","text/event-stream")],Body::from_stream(stream)).into_response()
        }
    }));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    SyntheticModel {
        base,
        calls,
        release,
        task,
    }
}
async fn ready_thread(store: &Store, app: &Router, model: &SyntheticModel) -> (String, String) {
    let cookie = login(app, "admin@example.test", PASSWORD).await;
    let user = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let profile = store
        .create_provider(
            &user,
            ProviderChange {
                name: "Chat fixture".into(),
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
    // Synthetic setup attests this fixture; the provider HTTP suite tests the real
    // administrator probe endpoint separately. No hosted credential is involved.
    store
        .record_model_probe(
            &user,
            profile.id.clone(),
            profile.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                provider_id: profile.id,
                provider_revision: profile.revision,
                model: "fixture-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "New conversation".into())
        .await
        .unwrap();
    (cookie, thread.id)
}
async fn page(app: &Router, cookie: &str, id: &str) -> Value {
    value(
        send(
            app,
            "GET",
            &format!("/api/v1/threads/{id}/messages"),
            Some(cookie),
            Value::Null,
        )
        .await,
    )
    .await
}
async fn wait_state(app: &Router, cookie: &str, id: &str, state: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let value = page(app, cookie, id).await;
            if value["turns"][0]["run"]["state"] == state {
                return value;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn real_chat_sse_disconnect_keeps_one_saved_call_and_reports_observed_usage() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let model = synthetic_model().await;
    let (cookie, thread) = ready_thread(&store, &app, &model).await;
    let stream = send(
        &app,
        "GET",
        &format!("/api/v1/threads/{thread}/events?after=0"),
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert_eq!(stream.status(), StatusCode::OK);
    assert!(
        stream.headers()["content-type"]
            .to_str()
            .unwrap()
            .starts_with("text/event-stream")
    );
    let payload = json!({"request_id":"33333333-3333-4333-8333-333333333333","text":"Synthetic chat question"});
    let url = format!("/api/v1/threads/{thread}/messages");
    let first = send(&app, "POST", &url, Some(&cookie), payload.clone()).await;
    assert_eq!(first.status(), StatusCode::CREATED);
    let first = value(first).await;
    let duplicate = value(send(&app, "POST", &url, Some(&cookie), payload).await).await;
    assert_eq!(first["run"]["id"], duplicate["run"]["id"]);
    let mut body = stream.into_body();
    let mut received = String::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while !received.contains("Synthetic partial") {
            let frame = body.frame().await.unwrap().unwrap();
            if let Some(data) = frame.data_ref() {
                received.push_str(std::str::from_utf8(data).unwrap());
            }
        }
    })
    .await
    .unwrap();
    assert!(received.contains("event: change"));
    assert!(received.contains("\"kind\":\"delta\""));
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
    drop(body); // Closing only the browser stream must not cancel model execution.
    model.release.notify_one();
    let finished = wait_state(&app, &cookie, &thread, "completed").await;
    assert_eq!(finished["turns"].as_array().unwrap().len(), 1);
    assert_eq!(
        finished["turns"][0]["assistant"][0]["text"],
        "Synthetic partial answer"
    );
    let usage = value(send(&app, "GET", "/api/v1/usage", Some(&cookie), Value::Null).await).await;
    assert_eq!(usage["dispatched_calls"], 1);
    assert_eq!(usage["unreported_calls"], 0);
    assert_eq!(usage["reported_input_tokens"], 13);
    assert_eq!(usage["reported_output_tokens"], 5);
}

#[tokio::test]
async fn cancelled_and_deleted_thread_is_not_recreated_by_late_model_result() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let model = synthetic_model().await;
    let (cookie, thread) = ready_thread(&store, &app, &model).await;
    let turn=value(send(&app,"POST",&format!("/api/v1/threads/{thread}/messages"),Some(&cookie),
        json!({"request_id":"44444444-4444-4444-8444-444444444444","text":"Synthetic chat question"})).await).await;
    wait_state(&app, &cookie, &thread, "running").await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while model.calls.load(Ordering::SeqCst) == 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    let run = turn["run"]["id"].as_str().unwrap();
    let cancelled = send(
        &app,
        "POST",
        &format!("/api/v1/runs/{run}/cancel"),
        Some(&cookie),
        json!({"expected_attempt":1}),
    )
    .await;
    assert_eq!(cancelled.status(), StatusCode::OK);
    assert_eq!(value(cancelled).await["run"]["state"], "cancelled");
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/threads/{thread}"),
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    model.release.notify_one();
    for suffix in ["messages", "events?after=0"] {
        assert_eq!(
            send(
                &app,
                "GET",
                &format!("/api/v1/threads/{thread}/{suffix}"),
                Some(&cookie),
                Value::Null
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/v1/runs/{run}/retry"),
            Some(&cookie),
            json!({"expected_attempt":1})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let usage = value(send(&app, "GET", "/api/v1/usage", Some(&cookie), Value::Null).await).await;
    assert_eq!(usage["dispatched_calls"], 1);
    assert!(usage["unreported_calls"].as_i64().unwrap() <= 1);
    assert_eq!(model.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn decoded_prompt_limit_accepts_json_escaped_text_and_rejects_excess() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let model = synthetic_model().await;
    let (cookie, thread) = ready_thread(&store, &app, &model).await;
    let text = "\u{0001}".repeat(8192);
    let payload = json!({"request_id":"55555555-5555-4555-8555-555555555555","text":text});
    assert!(payload.to_string().len() > 48 * 1024);
    let path = format!("/api/v1/threads/{thread}/messages");
    let response = send(&app, "POST", &path, Some(&cookie), payload).await;
    assert_eq!(response.status(), StatusCode::CREATED);
    assert_eq!(value(response).await["user_text"], text);
    let response = send(
        &app,
        "POST",
        &path,
        Some(&cookie),
        json!({
            "request_id":"66666666-6666-4666-8666-666666666666","text":"a".repeat(8193)
        }),
    )
    .await;
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}
