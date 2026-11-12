use super::*;
use axum::{
    extract::State,
    http::HeaderMap,
    response::IntoResponse,
    routing::{get, post},
};
use std::sync::{
    Arc,
    atomic::{AtomicU8, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

struct ModelFixture {
    base: String,
    mode: Arc<AtomicU8>,
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
    task: tokio::task::JoinHandle<()>,
}
impl Drop for ModelFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
#[derive(Clone)]
struct ModelState {
    mode: Arc<AtomicU8>,
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    release: Arc<Notify>,
}
async fn upstream() -> ModelFixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = ModelState {
        mode: Arc::new(AtomicU8::new(0)),
        calls: Arc::new(AtomicUsize::new(0)),
        entered: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
    };
    let router=Router::new().route("/v1/models",get(|headers:HeaderMap| async move {
        assert!(headers["authorization"]=="Bearer synthetic-provider-key");
        axum::Json(json!({"data":[{"id":"fixture-model","name":"Synthetic local model"}]}))
    })).route("/v1/chat/completions",post(|State(state):State<ModelState>,headers:HeaderMap,axum::Json(body):axum::Json<Value>| async move {
        state.calls.fetch_add(1,Ordering::SeqCst);
        assert!(headers["authorization"]=="Bearer synthetic-provider-key");
        assert_eq!(body["model"],"fixture-model");
        if body.get("tools").is_some() {return tool_upstream(&state,&body).await;}
        assert_eq!(body["messages"][0]["content"],"Reply with the single word OK.");
        assert!(body.get("reasoning").is_none());
        let mode=state.mode.load(Ordering::SeqCst);
        if mode==3 {state.entered.notify_one();state.release.notified().await;}
        if mode==4 {return (StatusCode::UNAUTHORIZED,"synthetic-provider-key must not escape").into_response();}
        let end=if mode==2 {"length"} else {"stop"};
        let mut text=json!({"id":"probe-fixture","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":"OK"},"finish_reason":null}]});
        let mut finish=json!({"id":"probe-fixture","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":end}]});
        if mode==5 {text["model"]=json!("substituted-model"); finish["model"]=json!("substituted-model");}
        if mode==6 {text.as_object_mut().unwrap().remove("model"); finish.as_object_mut().unwrap().remove("model");}
        let stream=if mode==1 {format!("data: {text}\n\n")} else {format!("data: {text}\n\ndata: {finish}\n\ndata: [DONE]\n\n")};
        ([("content-type","text/event-stream")],stream).into_response()
    })).with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    ModelFixture {
        base,
        mode: state.mode,
        calls: state.calls,
        entered: state.entered,
        release: state.release,
        task,
    }
}
fn connection(base: &str) -> Value {
    json!({"name":"Local fixture","enabled":true,"config":{"kind":"compatible","base_url":base,"allowed_ips":["127.0.0.1"],"router_providers":[],"zero_data_retention":false},"credential":{"action":"replace","value":"synthetic-provider-key"}})
}
async fn add(app: &Router, cookie: &str, upstream: &ModelFixture) -> Value {
    let response = send(
        app,
        "POST",
        "/api/v1/admin/providers",
        Some(cookie),
        connection(&upstream.base),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let body = value(response).await;
    assert!(!body.to_string().contains("synthetic-provider-key"));
    assert!(body["has_credential"].as_bool().unwrap());
    body
}
fn path(profile: &Value, suffix: &str) -> String {
    format!(
        "/api/v1/admin/providers/{}{}",
        profile["id"].as_str().unwrap(),
        suffix
    )
}
fn probe(profile: &Value) -> Value {
    json!({"revision":profile["revision"],"model":"fixture-model","effort":null})
}
fn selection(profile: &Value) -> Value {
    json!({"selection":{"provider_id":profile["id"],"provider_revision":profile["revision"],"model":"fixture-model","effort":null}})
}

#[tokio::test]
async fn administrator_connects_catalog_probes_defaults_and_member_keeps_own_selection() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let upstream = upstream().await;
    let profile = add(&app, &admin, &upstream).await;
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/admin/providers/default",
            Some(&admin),
            selection(&profile)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let catalog = send(
        &app,
        "POST",
        &path(&profile, "/models"),
        Some(&admin),
        json!({"revision":profile["revision"]}),
    )
    .await;
    assert_eq!(catalog.status(), StatusCode::OK);
    assert_eq!(value(catalog).await["models"][0]["id"], "fixture-model");
    assert_eq!(
        send(
            &app,
            "POST",
            &path(&profile, "/probe"),
            Some(&admin),
            probe(&profile)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/admin/providers/default",
            Some(&admin),
            selection(&profile)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let admin_user = store
        .authenticate(admin.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let invite = store
        .create_user(&admin_user, "Member".into(), "member@example.test".into())
        .await
        .unwrap();
    let member = store
        .login("member@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let member = store
        .change_password(&member.user, PASSWORD.into())
        .await
        .unwrap();
    let member_cookie = format!("moosuri_session={}", member.token);
    for (method, route, body) in [
        ("GET", "/api/v1/admin/providers".into(), Value::Null),
        (
            "POST",
            "/api/v1/admin/providers".into(),
            connection(&upstream.base),
        ),
        ("POST", path(&profile, "/probe"), probe(&profile)),
        ("POST", path(&profile, "/tool-probe"), probe(&profile)),
        (
            "POST",
            path(&profile, "/models"),
            json!({"revision":profile["revision"]}),
        ),
    ] {
        assert_eq!(
            send(&app, method, &route, Some(&member_cookie), body)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    let inherited = value(
        send(
            &app,
            "GET",
            "/api/v1/settings/model",
            Some(&member_cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert!(inherited["selection"].is_null());
    assert_eq!(inherited["effective"]["model"], "fixture-model");
    assert!(!inherited.to_string().contains("127.0.0.1"));
    let chosen = send(
        &app,
        "PUT",
        "/api/v1/settings/model",
        Some(&member_cookie),
        selection(&profile),
    )
    .await;
    assert_eq!(chosen.status(), StatusCode::OK);
    let administrator = value(
        send(
            &app,
            "GET",
            "/api/v1/settings/model",
            Some(&admin),
            Value::Null,
        )
        .await,
    )
    .await;
    assert!(administrator["selection"].is_null());
    let mut edited = connection(&upstream.base);
    edited["expected_revision"] = profile["revision"].clone();
    edited["enabled"] = json!(false);
    edited["credential"] = json!({"action":"keep"});
    assert_eq!(
        send(&app, "PUT", &path(&profile, ""), Some(&admin), edited)
            .await
            .status(),
        StatusCode::OK
    );
    let unavailable = value(
        send(
            &app,
            "GET",
            "/api/v1/settings/model",
            Some(&member_cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(unavailable["selection"]["model"], "fixture-model");
    assert!(unavailable["effective"].is_null());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn partial_incomplete_and_auth_failed_probes_never_activate_or_echo_private_body() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let upstream = upstream().await;
    let profile = add(&app, &admin, &upstream).await;
    for mode in [1, 2, 4, 5, 6] {
        upstream.mode.store(mode, Ordering::SeqCst);
        let response = send(
            &app,
            "POST",
            &path(&profile, "/probe"),
            Some(&admin),
            probe(&profile),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY);
        let body = value(response).await.to_string();
        assert!(!body.contains("synthetic-provider-key"));
        assert!(!body.contains("must not escape"));
        let settings = value(
            send(
                &app,
                "GET",
                "/api/v1/settings/model",
                Some(&admin),
                Value::Null,
            )
            .await,
        )
        .await;
        assert!(settings["choices"].as_array().unwrap().is_empty());
    }
    let mut bad = probe(&profile);
    bad["effort"] = json!("high");
    assert_eq!(
        send(&app, "POST", &path(&profile, "/probe"), Some(&admin), bad)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 5);
    let mut retarget = connection("http://127.0.0.1:9999/v1");
    retarget["credential"] = json!({"action":"keep"});
    retarget["expected_revision"] = profile["revision"].clone();
    assert_eq!(
        send(&app, "PUT", &path(&profile, ""), Some(&admin), retarget)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn disabling_during_held_probe_prevents_late_activation_and_stops_local_wait() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let upstream = upstream().await;
    let profile = add(&app, &admin, &upstream).await;
    upstream.mode.store(3, Ordering::SeqCst);
    let copy = app.clone();
    let cookie = admin.clone();
    let profile_copy = profile.clone();
    let pending = tokio::spawn(async move {
        send(
            &copy,
            "POST",
            &path(&profile_copy, "/probe"),
            Some(&cookie),
            probe(&profile_copy),
        )
        .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        upstream.entered.notified(),
    )
    .await
    .unwrap();
    let mut edited = connection(&upstream.base);
    edited["expected_revision"] = profile["revision"].clone();
    edited["enabled"] = json!(false);
    edited["credential"] = json!({"action":"keep"});
    assert_eq!(
        send(&app, "PUT", &path(&profile, ""), Some(&admin), edited)
            .await
            .status(),
        StatusCode::OK
    );
    let response = tokio::time::timeout(std::time::Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    upstream.release.notify_one();
    let settings = value(
        send(
            &app,
            "GET",
            "/api/v1/settings/model",
            Some(&admin),
            Value::Null,
        )
        .await,
    )
    .await;
    assert!(settings["choices"].as_array().unwrap().is_empty());
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 1);
}

async fn tool_upstream(state: &ModelState, body: &Value) -> axum::response::Response {
    let mode = state.mode.load(Ordering::SeqCst);
    assert_eq!(
        body["tools"][0]["function"]["name"],
        "moosuri_verification_ping"
    );
    assert_eq!(
        body["tools"][0]["function"]["parameters"]["properties"],
        json!({})
    );
    assert_eq!(body["max_tokens"], 2048);
    let result = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["role"] == "tool");
    if result.is_none() {
        assert!(!body.to_string().contains("proof-"));
    }
    if mode == 17 && result.is_some() {
        state.entered.notify_one();
        state.release.notified().await;
    }
    let response_model = if mode == 16 && result.is_some() {
        "substituted-model"
    } else {
        "fixture-model"
    };
    let delta = if let Some(result) = result {
        let challenge = result["content"].as_str().unwrap();
        assert!(challenge.starts_with("proof-"));
        assert_eq!(result["tool_call_id"], "verification-call");
        if mode == 15 {
            json!({"tool_calls":[{"index":0,"id":"second-call","type":"function","function":{"name":"moosuri_verification_ping","arguments":"{}"}}]})
        } else {
            json!({"content":if mode==14{"false claim".into()}else if mode==22{format!(" {challenge}")}else{challenge.to_owned()}})
        }
    } else if mode == 11 || mode == 19 {
        json!({"content":"I claim tools work"})
    } else {
        let call = json!({"index":0,"id":"verification-call","type":"function","function":{"name":if mode==20{"unregistered_ping"}else{"moosuri_verification_ping"},"arguments":if mode==12{json!({"unexpected":true}).to_string()}else{"{}".to_owned()}}});
        if mode == 21 {
            let mut second = call.clone();
            second["index"] = json!(1);
            second["id"] = json!("extra-call");
            json!({"tool_calls":[call,second]})
        } else {
            json!({"tool_calls":[call]})
        }
    };
    let stop = if mode == 19 {
        "content_filter"
    } else if delta.get("tool_calls").is_some() {
        "tool_calls"
    } else {
        "stop"
    };
    let prefix = json!({"id":if result.is_some(){"tool-second"}else{"tool-first"},"model":response_model,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let finish = json!({"id":if result.is_some(){"tool-second"}else{"tool-first"},"model":response_model,"choices":[{"index":0,"delta":{},"finish_reason":stop}]});
    let wire = if (mode == 13 && result.is_none()) || (mode == 18 && result.is_some()) {
        format!("data: {prefix}\n\n")
    } else {
        format!("data: {prefix}\n\ndata: {finish}\n\ndata: [DONE]\n\n")
    };
    ([("content-type", "text/event-stream")], wire).into_response()
}

#[tokio::test]
async fn tool_probe_requires_text_proof_then_validates_unseen_challenge_roundtrip() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let upstream = upstream().await;
    let profile = add(&app, &admin, &upstream).await;
    let catalog = send(
        &app,
        "POST",
        &path(&profile, "/models"),
        Some(&admin),
        json!({"revision":profile["revision"]}),
    )
    .await;
    assert_eq!(catalog.status(), StatusCode::OK);
    assert_eq!(
        send(
            &app,
            "POST",
            &path(&profile, "/tool-probe"),
            Some(&admin),
            probe(&profile)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 0);
    let response = send(
        &app,
        "POST",
        &path(&profile, "/probe"),
        Some(&admin),
        probe(&profile),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(value(response).await["function_tools"], false);
    upstream.mode.store(10, Ordering::SeqCst);
    let response = send(
        &app,
        "POST",
        &path(&profile, "/tool-probe"),
        Some(&admin),
        probe(&profile),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body = value(response).await;
    assert_eq!(body["function_tools"], true);
    assert!(!body.to_string().contains("proof-"));
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
    let settings = value(
        send(
            &app,
            "GET",
            "/api/v1/settings/model",
            Some(&admin),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(settings["choices"][0]["function_tools"], true);
    let mut forged = probe(&profile);
    forged["function_tools"] = json!(true);
    assert_eq!(
        send(
            &app,
            "POST",
            &path(&profile, "/tool-probe"),
            Some(&admin),
            forged
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn false_tool_claims_wrong_calls_nonce_identity_and_loss_do_not_activate_capability() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let upstream = upstream().await;
    let profile = add(&app, &admin, &upstream).await;
    assert_eq!(
        send(
            &app,
            "POST",
            &path(&profile, "/probe"),
            Some(&admin),
            probe(&profile)
        )
        .await
        .status(),
        StatusCode::OK
    );
    for mode in [11, 12, 13, 14, 15, 16, 18, 19, 20, 21, 22] {
        upstream.mode.store(mode, Ordering::SeqCst);
        let before = upstream.calls.load(Ordering::SeqCst);
        let response = send(
            &app,
            "POST",
            &path(&profile, "/tool-probe"),
            Some(&admin),
            probe(&profile),
        )
        .await;
        assert_eq!(response.status(), StatusCode::BAD_GATEWAY, "mode {mode}");
        let text = value(response).await.to_string();
        assert!(!text.contains("synthetic-provider-key"));
        assert!(!text.contains("proof-"));
        let expected = if [14, 15, 16, 18, 22].contains(&mode) {
            2
        } else {
            1
        };
        assert_eq!(
            upstream.calls.load(Ordering::SeqCst) - before,
            expected,
            "mode {mode} did not retry/fallback"
        );
        let settings = value(
            send(
                &app,
                "GET",
                "/api/v1/settings/model",
                Some(&admin),
                Value::Null,
            )
            .await,
        )
        .await;
        assert_eq!(settings["choices"][0]["function_tools"], false);
    }
}

#[tokio::test]
async fn revocation_during_second_tool_probe_round_blocks_late_capability_record() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let upstream = upstream().await;
    let profile = add(&app, &admin, &upstream).await;
    assert_eq!(
        send(
            &app,
            "POST",
            &path(&profile, "/probe"),
            Some(&admin),
            probe(&profile)
        )
        .await
        .status(),
        StatusCode::OK
    );
    upstream.mode.store(17, Ordering::SeqCst);
    let copy = app.clone();
    let cookie = admin.clone();
    let target = profile.clone();
    let pending = tokio::spawn(async move {
        send(
            &copy,
            "POST",
            &path(&target, "/tool-probe"),
            Some(&cookie),
            probe(&target),
        )
        .await
    });
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        upstream.entered.notified(),
    )
    .await
    .unwrap();
    let mut edited = connection(&upstream.base);
    edited["expected_revision"] = profile["revision"].clone();
    edited["enabled"] = json!(false);
    edited["credential"] = json!({"action":"keep"});
    assert_eq!(
        send(&app, "PUT", &path(&profile, ""), Some(&admin), edited)
            .await
            .status(),
        StatusCode::OK
    );
    let response = tokio::time::timeout(std::time::Duration::from_secs(2), pending)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(response.status(), StatusCode::CONFLICT);
    upstream.release.notify_one();
    assert_eq!(upstream.calls.load(Ordering::SeqCst), 3);
    let settings = value(
        send(
            &app,
            "GET",
            "/api/v1/settings/model",
            Some(&admin),
            Value::Null,
        )
        .await,
    )
    .await;
    assert!(settings["choices"].as_array().unwrap().is_empty());
}
