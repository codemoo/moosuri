#![cfg(feature = "test-fixtures")]
//! Real local Bot API + model transports, normal Store and supervised agent.
use axum::{
    Json, Router,
    body::Body,
    http::{Request, StatusCode},
    response::IntoResponse,
    routing::post,
};
use http_body_util::BodyExt;
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{CredentialChange, LoginResult, ProviderChange, Store};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tower::ServiceExt;
async fn api(
    app: &Router,
    login: &LoginResult,
    method: &str,
    path: &str,
    input: Value,
) -> (StatusCode, Value) {
    let r = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "telegram.example.test")
                .header("origin", "https://telegram.example.test")
                .header("content-type", "application/json")
                .header("cookie", format!("moosuri_session={}", login.token))
                .header("x-moosuri-account", &login.user.account().id)
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = r.status();
    assert_eq!(r.headers()["cache-control"], "no-store");
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn update(id: i64, text: &str) -> Value {
    json!({"update_id":id,"message":{"message_id":id+1,"date":SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs(),"from":{"id":111,"is_bot":false},"chat":{"id":111,"type":"private"},"text":text}})
}
#[tokio::test]
async fn telegram_http_pair_poll_model_reply_and_disconnect_use_one_owned_conversation() {
    let updates = Arc::new(Mutex::new(Vec::<Value>::new()));
    let sent = Arc::new(Mutex::new(Vec::<Value>::new()));
    let model_calls = Arc::new(AtomicUsize::new(0));
    let typing = Arc::new(AtomicUsize::new(0));
    let floods = Arc::new(AtomicUsize::new(0));
    let (queue, outputs, actions, flood) = (
        updates.clone(),
        sent.clone(),
        typing.clone(),
        floods.clone(),
    );
    let bot=Router::new().route("/bot12345:synthetic_token_for_tests_only/{method}",post(move|axum::extract::Path(method):axum::extract::Path<String>,Json(input):Json<Value>|{
        let(queue,outputs,actions,flood)=(queue.clone(),outputs.clone(),actions.clone(),flood.clone());async move{
            let result=match method.as_str(){
                "getMe"=>json!({"id":12345,"is_bot":true,"username":"SyntheticMoosuriBot"}),
                "getWebhookInfo"=>json!({"url":""}),
                "getUpdates"=>{tokio::time::sleep(Duration::from_millis(200)).await;let offset=input["offset"].as_i64().unwrap();Value::Array(queue.lock().unwrap().iter().filter(|u|u["update_id"].as_i64().unwrap()>=offset).cloned().collect())},
                "sendChatAction"=>{actions.fetch_add(1,Ordering::SeqCst);json!(true)},
                "sendMessage"=>{
                    if input["text"].as_str().unwrap().contains("Synthetic Telegram answer")&&flood.fetch_add(1,Ordering::SeqCst)==0 {
                        return (StatusCode::TOO_MANY_REQUESTS,Json(json!({"ok":false,"error_code":429,"parameters":{"retry_after":1}}))).into_response()
                    }
                    outputs.lock().unwrap().push(input);json!({"message_id":42})
                },
                _=>panic!("unexpected method"),
            };Json(json!({"ok":true,"result":result})).into_response()
        }
    }));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", l.local_addr().unwrap());
    let bot_task = tokio::spawn(async move { axum::serve(l, bot).await.unwrap() });
    let calls = model_calls.clone();
    let model=Router::new().route("/v1/chat/completions",post(move|Json(input):Json<Value>|{let calls=calls.clone();async move{
        calls.fetch_add(1,Ordering::SeqCst);assert!(input["messages"].as_array().unwrap().iter().any(|m|m["content"].as_str().is_some_and(|s|s.contains("synthetic Telegram question"))));
        tokio::time::sleep(Duration::from_secs(3)).await;
        ([("content-type","text/event-stream")],concat!("data: {\"id\":\"fixture\",\"model\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"Synthetic Telegram answer\"},\"finish_reason\":null}]}\n\n","data: {\"id\":\"fixture\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n","data: [DONE]\n\n"))
    }}));
    let l = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let model_base = format!("http://{}/v1", l.local_addr().unwrap());
    let model_task = tokio::spawn(async move { axum::serve(l, model).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store.telegram_test_endpoint(base).await.unwrap();
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            "Synthetic-Telegram-Password".into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login(
            "owner@example.test".into(),
            "Synthetic-Telegram-Password".into(),
        )
        .await
        .unwrap();
    let p = store
        .create_provider(
            &login.user,
            ProviderChange {
                name: "Synthetic model".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(model_base),
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
            "fixture".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &login.user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: p.id,
                provider_revision: p.revision,
                model: "fixture".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let config = ServerConfig::new("https://telegram.example.test", false).unwrap();
    let runtime = config.start_runtime(store.clone());
    let app = router_with_runtime(store.clone(), config, runtime.clone());
    let configured = api(
        &app,
        &login,
        "PUT",
        "/api/v1/admin/telegram",
        json!({"expected_revision":0,"token":"12345:synthetic_token_for_tests_only"}),
    )
    .await;
    assert_eq!(configured.0, StatusCode::OK);
    assert!(!configured.1.to_string().contains("synthetic_token"));
    let (status, pair) = api(
        &app,
        &login,
        "POST",
        "/api/v1/settings/telegram/pair",
        json!({"timezone":"Asia/Seoul","notifications":false}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    updates.lock().unwrap().push(update(
        1,
        &format!("/start {}", pair["code"].as_str().unwrap()),
    ));
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if store
                .telegram_settings(&login.user, false)
                .await
                .unwrap()
                .binding
                .is_some()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    let question = update(2, "synthetic Telegram question");
    updates.lock().unwrap().extend([question.clone(), question]);
    tokio::time::timeout(Duration::from_secs(20), async {
        loop {
            if sent.lock().unwrap().iter().any(|m| {
                m["text"]
                    .as_str()
                    .unwrap()
                    .contains("Synthetic Telegram answer")
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(model_calls.load(Ordering::SeqCst), 1);
    assert!(typing.load(Ordering::SeqCst) > 0);
    assert_eq!(floods.load(Ordering::SeqCst), 2);
    let thread = store
        .telegram_settings(&login.user, false)
        .await
        .unwrap()
        .binding
        .unwrap()
        .thread_id
        .unwrap();
    let page = store
        .conversation(&login.user, thread.clone(), None)
        .await
        .unwrap();
    assert_eq!(page.turns.len(), 1);
    assert_eq!(page.turns[0].assistant[0].text, "Synthetic Telegram answer");
    let sent_answer = sent
        .lock()
        .unwrap()
        .iter()
        .find(|m| {
            m["text"]
                .as_str()
                .unwrap()
                .contains("Synthetic Telegram answer")
        })
        .unwrap()
        .clone();
    assert_eq!(sent_answer["chat_id"], 111);
    assert!(
        sent_answer["text"]
            .as_str()
            .unwrap()
            .contains(&format!("/app?thread={thread}"))
    );
    assert_eq!(
        api(
            &app,
            &login,
            "DELETE",
            "/api/v1/settings/telegram",
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    updates
        .lock()
        .unwrap()
        .push(update(3, "must not invoke model"));
    tokio::time::sleep(Duration::from_secs(2)).await;
    assert_eq!(model_calls.load(Ordering::SeqCst), 1);
    runtime.shutdown().await;
    bot_task.abort();
    model_task.abort();
}
