//! Actual authenticated Calendar API admission/receipt routes, no Google network.
use axum::{
    body::Body,
    http::{Request, StatusCode},
};
use http_body_util::BodyExt;
use moosuri_connectors::gmail::{SecretString, TokenSet};
use moosuri_core::*;
use moosuri_server::{ServerConfig, router_with_runtime};
use moosuri_store::{GoogleAppPin, Store};
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
    let mut request = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "assistant.example.test")
        .header("origin", "https://assistant.example.test")
        .header("content-type", "application/json");
    if let Some(t) = token {
        request = request.header("cookie", format!("moosuri_session={t}"))
    }
    if let Some(a) = account {
        request = request.header("x-moosuri-account", a)
    }
    let r = app
        .clone()
        .oneshot(request.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap();
    let status = r.status();
    let bytes = r.into_body().collect().await.unwrap().to_bytes();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
#[tokio::test]
async fn calendar_http_separate_scope_real_state_and_owner_retry_receipt() {
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let password = "Synthetic-calendar-http-password";
    store
        .bootstrap(
            "Synthetic owner".into(),
            "calendar-owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("calendar-owner@example.test".into(), password.into())
        .await
        .unwrap();
    let config=ServerConfig::new("https://assistant.example.test",false).unwrap().with_google_client_json(&json!({"web":{"client_id":"synthetic-calendar.apps.googleusercontent.com","client_secret":"synthetic-client-secret","redirect_uris":["https://assistant.example.test/api/v1/mail/oauth/callback"]}}).to_string()).unwrap();
    let runtime = config.start_runtime(store.clone());
    runtime.shutdown().await;
    let app = router_with_runtime(store.clone(), config, runtime);
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/calendar/connections",
            None,
            None,
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let (status, connections) = send(
        &app,
        "GET",
        "/api/v1/calendar/connections",
        Some(&owner.token),
        Some(&owner.user.account().id),
        Value::Null,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(connections["configured"], true);
    assert_eq!(connections["connections"], json!([]));
    let (status, started) = send(
        &app,
        "POST",
        "/api/v1/calendar/oauth/start",
        Some(&owner.token),
        Some(&owner.user.account().id),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let u = url::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
    let query: std::collections::HashMap<_, _> = u.query_pairs().collect();
    assert_eq!(
        query["scope"],
        moosuri_connectors::calendar::SCOPES.join(" ")
    );
    assert!(!query["scope"].contains("gmail"));
    let state = query["state"].to_string();
    assert!(state.starts_with("cal."));
    let callback = format!("/api/v1/mail/oauth/callback?state={state}&code=synthetic-code");
    let (status, _) = send(&app, "GET", &callback, None, None, Value::Null).await;
    assert_eq!(status, StatusCode::SEE_OTHER);
    // Complete a separate synthetic OAuth carrier locally; the API's later exact
    // retry must return the committed receipt before any Google network request.
    let pin = GoogleAppPin {
        revision: None,
        client_id: "synthetic-calendar.apps.googleusercontent.com".into(),
        redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
    };
    let start = store
        .start_calendar_oauth(&owner.user, pin.clone())
        .await
        .unwrap();
    let handle = store
        .capture_calendar_oauth(
            start.state,
            Some(SecretString::new("synthetic-code".into()).unwrap()),
        )
        .await
        .unwrap()
        .unwrap();
    let exchange = store
        .claim_calendar_oauth(&owner.user, handle, pin.clone())
        .await
        .unwrap();
    let primary = CalendarInfo {
        id: "calendar-owner@example.test".into(),
        title: "Synthetic Primary".into(),
        time_zone: "UTC".into(),
        primary: true,
        access_role: "owner".into(),
        email_notifications: false,
    };
    let conn = store
        .complete_calendar_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-calendar-access".into()).unwrap(),
                refresh_token: Some(
                    SecretString::new("synthetic-calendar-refresh".into()).unwrap(),
                ),
                expires_in: 3600,
                scopes: Some(
                    moosuri_connectors::calendar::SCOPES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            primary.clone(),
        )
        .await
        .unwrap();
    let b = store
        .calendar_binding(&owner.user, conn.id.clone(), pin)
        .await
        .unwrap();
    let change = CalendarChange {
        request_id: uuid::Uuid::new_v4().to_string(),
        connection_id: conn.id,
        expected_epoch: 1,
        expected_revision: 1,
        mutation: CalendarMutation::Create {
            fields: CalendarFields {
                title: "Synthetic exact request".into(),
                description: "".into(),
                location: "".into(),
                start: "2026-11-15T10:00:00Z".into(),
                end: "2026-11-15T11:00:00Z".into(),
                time_zone: Some("UTC".into()),
            },
        },
    };
    let dispatch = match store
        .admit_calendar_change(&b, change.clone(), primary, None)
        .await
        .unwrap()
    {
        moosuri_store::CalendarAdmission::Dispatch(d) => d,
        _ => panic!(),
    };
    store
        .settle_calendar_change(
            &dispatch,
            "outcome_unknown".into(),
            None,
            "synthetic_lost_ack".into(),
            Some("lookup_required".into()),
        )
        .await
        .unwrap();
    let (status, recorded) = send(
        &app,
        "POST",
        "/api/v1/calendar/actions",
        Some(&owner.token),
        Some(&owner.user.account().id),
        serde_json::to_value(&change).unwrap(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(recorded["state"], "outcome_unknown");
    assert_eq!(recorded["event_id"], dispatch.event_id);
    let mut altered = serde_json::to_value(change).unwrap();
    altered["mutation"]["fields"]["title"] = json!("Conflicting payload");
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/calendar/actions",
            Some(&owner.token),
            Some(&owner.user.account().id),
            altered
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/calendar/actions",
            Some(&owner.token),
            Some("00000000-0000-4000-8000-000000000099"),
            Value::Null
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert!(
        store
            .mail_connections(&owner.user)
            .await
            .unwrap()
            .is_empty()
    );
}
