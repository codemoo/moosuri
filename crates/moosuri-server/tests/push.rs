//! Synthetic account/HTTP boundary with runtime stopped before any push enrollment.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{LoginResult, Store};
use serde_json::{Value, json};
use std::time::{SystemTime, UNIX_EPOCH};
use tower::ServiceExt;
fn id(v: u64) -> String {
    format!("00000000-0000-4000-8000-{v:012}")
}
async fn send(
    app: &Router,
    login: &LoginResult,
    method: &str,
    path: &str,
    input: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "push.example.test")
                .header("origin", "https://push.example.test")
                .header("content-type", "application/json")
                .header("cookie", format!("moosuri_session={}", login.token))
                .header("x-moosuri-account", &login.user.account().id)
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.headers()["cache-control"], "no-store");
    let status = response.status();
    let body = response.into_body().collect().await.unwrap().to_bytes();
    (status, serde_json::from_slice(&body).unwrap_or(Value::Null))
}
#[tokio::test]
async fn current_account_enrollment_display_gate_disable_and_private_endpoint_rejection() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let password = "Synthetic-push-HTTP-password";
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let a = store
        .login("owner@example.test".into(), password.into())
        .await
        .unwrap();
    let invite = store
        .create_user(&a.user, "second".into(), "second@example.test".into())
        .await
        .unwrap();
    let restricted = store
        .login("second@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let b = store
        .change_password(&restricted.user, "Synthetic-second-push-password".into())
        .await
        .unwrap();
    let runtime = moosuri_agent::Runtime::start(store.clone());
    runtime.shutdown().await;
    let app = router_with_runtime(
        store.clone(),
        ServerConfig::new("https://push.example.test", false).unwrap(),
        runtime,
    );
    let config = send(
        &app,
        &a,
        "GET",
        &format!("/api/v1/notifications/push?device_id={}", id(1)),
        Value::Null,
    )
    .await;
    assert_eq!(config.0, StatusCode::OK);
    assert!(config.1["application_server_key"].is_string());
    assert!(config.1["device"].is_null());
    let (_, public) = moosuri_connectors::push::generate_vapid().unwrap();
    let input = json!({"request_id":id(2),"device_id":id(1),"operation_revision":1,"subscription":{"endpoint":"https://fcm.googleapis.com/fcm/send/synthetic-http-only","p256dh":public,"auth":"_ordMnz7uTCmrpBTeUV4Bw"}}); // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
    let registered = send(
        &app,
        &a,
        "POST",
        "/api/v1/notifications/push",
        input.clone(),
    )
    .await;
    assert_eq!(registered.0, StatusCode::OK);
    assert_eq!(registered.1["state"], "active");
    let receipt = send(
        &app,
        &a,
        "POST",
        "/api/v1/notifications/push",
        input.clone(),
    )
    .await;
    assert_eq!(registered.1, receipt.1);
    assert!(registered.1.get("endpoint").is_none() && registered.1.get("auth").is_none());
    let binding = registered.1["binding_id"].as_str().unwrap();
    let at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 60;
    store
        .create_schedule(
            &a.user,
            moosuri_core::ScheduleCreate {
                request_id: id(3),
                draft: moosuri_core::ScheduleDraft {
                    title: "Synthetic PRIVATE title".into(),
                    message: "Synthetic PRIVATE body".into(),
                    rule: moosuri_core::ScheduleRule::Once { at },
                    valid_until: None,
                },
            },
        )
        .await
        .unwrap();
    store.enqueue_scheduled_reminders_at(at).await.unwrap();
    let lease = store
        .claim_scheduled_reminder_at(at)
        .await
        .unwrap()
        .unwrap();
    let notification = store
        .finish_scheduled_reminder_at(&lease, at)
        .await
        .unwrap();
    store.poll_push_queue().await.unwrap();
    let claim = store.claim_push().await.unwrap().unwrap();
    let display = json!({"binding_id":binding,"notification_id":notification.id});
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/authorize",
            display.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    store.commit_push_dispatch(&claim.lease).await.unwrap();
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/authorize",
            display.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            &b,
            "POST",
            "/api/v1/notifications/push/authorize",
            display.clone()
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            &b,
            "DELETE",
            &format!("/api/v1/notifications/push/{binding}"),
            Value::Null
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/authorize",
            display.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    store
        .mark_notification(&a.user, notification.id, false)
        .await
        .unwrap();
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/authorize",
            display
        )
        .await
        .0,
        StatusCode::NOT_FOUND
    );
    let mut invalid = input.clone();
    invalid["request_id"] = json!(id(4));
    invalid["subscription"]["endpoint"] = json!("https://127.0.0.1/private-canary");
    assert_eq!(
        send(&app, &a, "POST", "/api/v1/notifications/push", invalid)
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let off = json!({"request_id":id(5),"device_id":id(1),"operation_revision":2});
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/disable",
            off.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            &a,
            "GET",
            &format!("/api/v1/notifications/push?device_id={}", id(1)),
            Value::Null
        )
        .await
        .1["device"]["state"],
        "off"
    );
    // A never-accepted earlier request is stale; exact accepted replay is off.
    let mut late = input.clone();
    late["request_id"] = json!(id(6));
    assert_eq!(
        send(&app, &a, "POST", "/api/v1/notifications/push", late)
            .await
            .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push",
            input.clone()
        )
        .await
        .1["state"],
        "off"
    );
    let mut next = input;
    next["request_id"] = json!(id(7));
    next["operation_revision"] = json!(3);
    let newer = send(&app, &a, "POST", "/api/v1/notifications/push", next).await;
    assert_eq!(newer.0, StatusCode::OK);
    assert_eq!(newer.1["state"], "active");
    // Lost off ACK replay is harmless after a newer enrollment commits.
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/disable",
            off.clone()
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            &a,
            "GET",
            &format!("/api/v1/notifications/push?device_id={}", id(1)),
            Value::Null
        )
        .await
        .1["device"]["binding_id"],
        newer.1["binding_id"]
    );
    let mut changed = off;
    changed["operation_revision"] = json!(4);
    assert_eq!(
        send(
            &app,
            &a,
            "POST",
            "/api/v1/notifications/push/disable",
            changed
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
}
