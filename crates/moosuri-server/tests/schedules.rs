//! Actual authenticated HTTP and supervised five-second local delivery; synthetic only.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::Store;
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tower::ServiceExt;
async fn send(
    app: &axum::Router,
    method: &str,
    path: &str,
    token: Option<&str>,
    account: Option<&str>,
    body: Value,
) -> (StatusCode, Value) {
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "assistant.example.test")
        .header("origin", "https://assistant.example.test")
        .header("content-type", "application/json");
    if let Some(token) = token {
        request = request.header("cookie", format!("moosuri_session={token}"));
    }
    if let Some(account) = account {
        request = request.header("x-moosuri-account", account);
    }
    let response = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = response.status();
    let bytes = response.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn owner_http_creation_control_and_worker_inbox_survive_actual_restart_without_send_or_agent_run()
 {
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = Store::open(dir.path()).unwrap();
    let password = "Synthetic-schedule-http-password";
    store
        .bootstrap(
            "Synthetic owner".into(),
            "reminder-owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let a = store
        .login("reminder-owner@example.test".into(), password.into())
        .await
        .unwrap();
    let invitation = store
        .create_user(
            &a.user,
            "Other owner".into(),
            "reminder-other@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = store
        .login(
            "reminder-other@example.test".into(),
            invitation.temporary_password,
        )
        .await
        .unwrap();
    let b = store
        .change_password(&restricted.user, password.into())
        .await
        .unwrap();
    let config = ServerConfig::new("https://assistant.example.test", false).unwrap();
    let runtime = config.start_runtime(store.clone());
    let app = router_with_runtime(store.clone(), config, runtime.clone());
    assert_eq!(
        send(&app, "GET", "/api/v1/schedules", None, None, Value::Null)
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let once = json!({"title":"Synthetic local reminder","message":"Review the synthetic notes; no agent task is performed.","rule":{"kind":"once","at":now+2},"valid_until":null});
    let request = json!({"request_id":"00000000-0000-4000-8000-000000000001","draft":once});
    let (status, schedule) = send(
        &app,
        "POST",
        "/api/v1/schedules",
        Some(&a.token),
        Some(&a.user.account().id),
        request.clone(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let id = schedule["id"].as_str().unwrap();
    let retried = send(
        &app,
        "POST",
        "/api/v1/schedules",
        Some(&a.token),
        Some(&a.user.account().id),
        request,
    )
    .await;
    assert_eq!(retried.0, StatusCode::OK);
    assert_eq!(retried.1["id"], schedule["id"]);
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/schedules",
            Some(&b.token),
            Some(&b.user.account().id),
            Value::Null
        )
        .await
        .1,
        json!([])
    );
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("/api/v1/schedules/{id}/history"),
            Some(&b.token),
            Some(&b.user.account().id),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let end = tokio::time::Instant::now() + Duration::from_secs(9);
    let notification = loop {
        let (status, page) = send(
            &app,
            "GET",
            "/api/v1/notifications",
            Some(&a.token),
            Some(&a.user.account().id),
            Value::Null,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        if let Some(item) = page["items"].as_array().unwrap().first() {
            break item.clone();
        }
        assert!(
            tokio::time::Instant::now() < end,
            "supervised local reminder did not reach the inbox"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    let notification_id = notification["id"].as_str().unwrap();
    assert_eq!(notification["schedule_id"], schedule["id"]);
    assert_eq!(notification["message"], once["message"]);
    assert!(store.list_threads(&a.user).await.unwrap().is_empty());
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/v1/notifications/{notification_id}/read"),
            Some(&b.token),
            Some(&b.user.account().id),
            Value::Null
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/v1/notifications/{notification_id}/read"),
            Some(&a.token),
            Some(&a.user.account().id),
            Value::Null
        )
        .await
        .1["read"],
        true
    );
    let interval = json!({"title":"Controlled synthetic reminder","message":"Local content only","rule":{"kind":"interval","anchor":now+120,"seconds":60},"valid_until":null});
    let request = json!({"request_id":"00000000-0000-4000-8000-000000000002","draft":interval});
    let s = send(
        &app,
        "POST",
        "/api/v1/schedules",
        Some(&a.token),
        Some(&a.user.account().id),
        request,
    )
    .await
    .1;
    let path = format!("/api/v1/schedules/{}", s["id"].as_str().unwrap());
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("{path}/control"),
            Some(&a.token),
            Some(&a.user.account().id),
            json!({"expected_revision":1,"action":"pause"})
        )
        .await
        .1["state"],
        "paused"
    );
    assert_eq!(
        send(
            &app,
            "PUT",
            &path,
            Some(&a.token),
            Some(&a.user.account().id),
            json!({"expected_revision":1,"draft":interval})
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            "PUT",
            &path,
            Some(&b.token),
            Some(&b.user.account().id),
            json!({"expected_revision":2,"draft":interval})
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let edited = send(
        &app,
        "PUT",
        &path,
        Some(&a.token),
        Some(&a.user.account().id),
        json!({"expected_revision":2,"draft":interval}),
    )
    .await;
    assert_eq!(edited.0, StatusCode::OK);
    assert_eq!(edited.1["state"], "paused");
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("{path}/control"),
            Some(&a.token),
            Some(&a.user.account().id),
            json!({"expected_revision":3,"action":"resume"})
        )
        .await
        .1["state"],
        "active"
    );
    let q = json!({"expected_revision":0,"enabled":true,"timezone":"America/New_York","start_minute":1320,"end_minute":420});
    assert_eq!(
        send(
            &app,
            "PUT",
            "/api/v1/notifications/settings",
            Some(&a.token),
            Some(&a.user.account().id),
            q
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/notifications/settings",
            Some(&b.token),
            Some(&b.user.account().id),
            Value::Null
        )
        .await
        .1["revision"],
        0
    );
    runtime.shutdown().await;
    drop(app);
    drop(runtime);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let config = ServerConfig::new("https://assistant.example.test", false).unwrap();
    let runtime = config.start_runtime(store.clone());
    let app = router_with_runtime(store.clone(), config, runtime.clone());
    let page = send(
        &app,
        "GET",
        "/api/v1/notifications",
        Some(&a.token),
        Some(&a.user.account().id),
        Value::Null,
    )
    .await
    .1;
    assert_eq!(page["items"].as_array().unwrap().len(), 1);
    assert_eq!(page["items"][0]["id"], notification["id"]);
    assert_eq!(page["items"][0]["read"], true);
    for _ in 0..2 {
        assert_eq!(
            send(
                &app,
                "POST",
                &format!("/api/v1/notifications/{notification_id}/dismiss"),
                Some(&a.token),
                Some(&a.user.account().id),
                Value::Null
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/notifications",
            Some(&a.token),
            Some(&a.user.account().id),
            Value::Null
        )
        .await
        .1["items"],
        json!([])
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/notifications?dismissed=true",
            Some(&a.token),
            Some(&a.user.account().id),
            Value::Null
        )
        .await
        .1["items"][0]["dismissed"],
        true
    );
    runtime.shutdown().await;
}
