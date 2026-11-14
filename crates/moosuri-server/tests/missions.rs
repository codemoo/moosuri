//! Authenticated mission API admission/control, using a stopped synthetic runtime.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{CredentialChange, ProviderChange, Store};
use serde_json::{Value, json};
use tower::ServiceExt;
async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    account: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut req = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "assistant.example.test")
        .header("origin", "https://assistant.example.test")
        .header("content-type", "application/json");
    if let Some(t) = token {
        req = req.header("cookie", format!("moosuri_session={t}"));
    }
    if let Some(a) = account {
        req = req.header("x-moosuri-account", a);
    }
    let res = app
        .clone()
        .oneshot(req.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = res.status();
    let bytes = res.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn mission_http_owner_admission_deduplication_controls_and_scope() {
    let d = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let s = Store::open(d.path()).unwrap();
    let pass = "Synthetic-mission-http-password";
    s.bootstrap(
        "Synthetic owner".into(),
        "owner@example.test".into(),
        pass.into(),
        false,
    )
    .await
    .unwrap();
    let u = s
        .login("owner@example.test".into(), pass.into())
        .await
        .unwrap();
    let p = s
        .create_provider(
            &u.user,
            ProviderChange {
                name: "Synthetic".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    s.record_model_probe(&u.user, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.record_tool_probe(&u.user, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &u.user,
        Some(ModelSelection {
            source: Default::default(),
            provider_id: p.id,
            provider_revision: 1,
            model: "synthetic".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
    let config = ServerConfig::new("https://assistant.example.test", false).unwrap();
    let runtime = config.start_runtime(s.clone());
    runtime.shutdown().await;
    let app = router_with_runtime(s.clone(), config, runtime);
    let (status, _) = send(&app, "GET", "/api/v1/missions", None, None, Value::Null).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let req = json!({"request_id":"00000000-0000-4000-8000-000000000001","task":"Compute 0.1 + 0.2","constraints":"Use exact receipts","checks":[{"kind":"decimal_equals","expected":"0.3"}],"execution_mode":"durable"});
    let (status, m) = send(
        &app,
        "POST",
        "/api/v1/missions",
        Some(&u.token),
        Some(&u.user.account().id),
        req.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{m}");
    let (_, same) = send(
        &app,
        "POST",
        "/api/v1/missions",
        Some(&u.token),
        Some(&u.user.account().id),
        req.clone(),
    )
    .await;
    assert_eq!(same["id"], m["id"]);
    let mut conflicting = req;
    conflicting["task"] = json!("Different request");
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/missions",
            Some(&u.token),
            Some(&u.user.account().id),
            conflicting
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let control = format!("/api/v1/missions/{}/control", m["id"].as_str().unwrap());
    let pause = json!({"request_id":"00000000-0000-4000-8000-000000000002","expected_revision":1,"action":{"kind":"pause"}});
    let (status, p) = send(
        &app,
        "POST",
        &control,
        Some(&u.token),
        Some(&u.user.account().id),
        pause.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(p["state"], "paused");
    assert_eq!(
        send(
            &app,
            "POST",
            &control,
            Some(&u.token),
            Some(&u.user.account().id),
            pause
        )
        .await
        .1["revision"],
        2
    );
    assert_eq!(send(&app,"POST",&control,Some(&u.token),Some("00000000-0000-4000-8000-000000000099"),json!({"request_id":"00000000-0000-4000-8000-000000000003","expected_revision":2,"action":{"kind":"resume"}})).await.0,StatusCode::UNAUTHORIZED);
    let recurring = json!({"request_id":"00000000-0000-4000-8000-000000000010","template":{"request_id":"00000000-0000-4000-8000-000000000011","task":"Compute synthetic total","constraints":"","checks":[{"kind":"decimal_equals","expected":"0.3"}],"execution_mode":"durable"},"schedule":{"title":"Synthetic scheduled task","message":"Not notification content","rule":{"kind":"daily","time":"09:00","timezone":"Asia/Seoul"},"valid_until":null},"changes_only":true});
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/recurring-missions",
            None,
            None,
            recurring.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/recurring-missions",
            Some(&u.token),
            Some("wrong-account"),
            recurring.clone()
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, r) = send(
        &app,
        "POST",
        "/api/v1/recurring-missions",
        Some(&u.token),
        Some(&u.user.account().id),
        recurring.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{r}");
    let (_, same) = send(
        &app,
        "POST",
        "/api/v1/recurring-missions",
        Some(&u.token),
        Some(&u.user.account().id),
        recurring,
    )
    .await;
    assert_eq!(same["schedule"]["id"], r["schedule"]["id"]);
    let id = r["schedule"]["id"].as_str().unwrap();
    let generic = format!("/api/v1/schedules/{id}/control");
    assert_eq!(
        send(
            &app,
            "POST",
            &generic,
            Some(&u.token),
            Some(&u.user.account().id),
            json!({"expected_revision":1,"action":"pause"})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let path = format!("/api/v1/recurring-missions/{id}/control");
    let (status, paused) = send(
        &app,
        "POST",
        &path,
        Some(&u.token),
        Some(&u.user.account().id),
        json!({"expected_revision":1,"action":"pause"}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(paused["schedule"]["state"], "paused");
    assert!(s.list_threads(&u.user).await.unwrap().is_empty());
    assert_eq!(s.usage_summary(&u.user).await.unwrap().dispatched_calls, 0);
    let config = ServerConfig::new("https://assistant.example.test", false).unwrap();
    let runtime = config.start_runtime(s.clone());
    let app = router_with_runtime(s.clone(), config, runtime.clone());
    let thread = s
        .create_thread(&u.user, "Synthetic semantic controls".into())
        .await
        .unwrap();
    let path = format!("/api/v1/threads/{}/messages", thread.id);
    let command = json!({"request_id":"00000000-0000-4000-8000-000000000020","text":"그 업무 멈춰 줘","schedule_context":{"timezone":"Asia/Seoul","target":null},"mission_context":{"target":{"kind":"recurring","id":id,"revision":2}}});
    let (status, accepted) = send(
        &app,
        "POST",
        &path,
        Some(&u.token),
        Some(&u.user.account().id),
        command.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED, "{accepted}");
    assert_eq!(accepted["run"]["state"], "queued");
    let (status, retry) = send(
        &app,
        "POST",
        &path,
        Some(&u.token),
        Some(&u.user.account().id),
        command.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::CREATED);
    assert_eq!(retry["run"]["id"], accepted["run"]["id"]);
    let mut changed = command;
    changed["mission_context"]["target"]["revision"] = json!(99);
    assert_eq!(
        send(
            &app,
            "POST",
            &path,
            Some(&u.token),
            Some(&u.user.account().id),
            changed
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    runtime.shutdown().await;
}
