use super::*;
use moosuri_connectors::gmail::{SecretString, TokenSet};

fn client_json(origin: &str) -> String {
    json!({"web":{"client_id":"synthetic.apps.googleusercontent.com",
        "client_secret":"synthetic-google-setup-canary",
        "redirect_uris":[format!("{origin}/api/v1/mail/oauth/callback")],
        "token_uri":"https://untrusted.example/token"}})
    .to_string()
}
async fn setup(app: &Router, cookie: &str, revision: i64) -> Value {
    let response = send(
        app,
        "PUT",
        "/api/v1/admin/google",
        Some(cookie),
        json!({"expected_revision":revision,"client_json":client_json(ORIGIN)}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    value(response).await
}
async fn callback(app: &Router, query: &str) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/v1/mail/oauth/callback?{query}"))
                .header(header::HOST, HOST)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn admin_setup_local_validation_hot_availability_and_personal_authority() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let before = value(
        send(
            &app,
            "GET",
            "/api/v1/admin/google",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(before["mode"], "unconfigured");
    assert_eq!(before["configured"], false);
    assert_eq!(
        before["redirect_uri"],
        format!("{ORIGIN}/api/v1/mail/oauth/callback")
    );
    let valid = value(
        send(
            &app,
            "POST",
            "/api/v1/admin/google/validate",
            Some(&cookie),
            json!({"client_json":client_json(ORIGIN)}),
        )
        .await,
    )
    .await;
    assert_eq!(valid["local_valid"], true);
    assert!(!valid.to_string().contains("canary"));
    assert_eq!(
        value(
            send(
                &app,
                "GET",
                "/api/v1/mail/connections",
                Some(&cookie),
                Value::Null
            )
            .await
        )
        .await["configured"],
        false
    );
    let bad = value(
        send(
            &app,
            "POST",
            "/api/v1/admin/google/validate",
            Some(&cookie),
            json!({"client_json":client_json("https://wrong.example.test")}),
        )
        .await,
    )
    .await;
    assert_eq!(bad["code"], "google_redirect_uri_mismatch");
    let huge = send(
        &app,
        "POST",
        "/api/v1/admin/google/validate",
        Some(&cookie),
        json!({"client_json":"x".repeat(16385)}),
    )
    .await;
    assert_eq!(huge.status(), StatusCode::BAD_REQUEST);
    let saved = setup(&app, &cookie, 0).await;
    assert_eq!(saved["revision"], 1);
    assert_eq!(saved["mode"], "managed");
    assert_eq!(saved["configured"], true);
    assert!(!saved.to_string().contains("canary"));
    assert_eq!(
        value(
            send(
                &app,
                "GET",
                "/api/v1/mail/connections",
                Some(&cookie),
                Value::Null
            )
            .await
        )
        .await["configured"],
        true
    );
    assert_eq!(
        send(
            &app,
            "PUT",
            "/api/v1/admin/google",
            Some(&cookie),
            json!({"expected_revision":0,"client_json":client_json(ORIGIN)})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let user = store
        .authenticate(cookie.split_once('=').unwrap().1.into())
        .await
        .unwrap();
    let invite = store
        .create_user(&user, "Member".into(), "member@example.test".into())
        .await
        .unwrap();
    let temporary = store
        .login("member@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let member = store
        .change_password(&temporary.user, "Member-Setup-Password-2026".into())
        .await
        .unwrap();
    let member_cookie = format!("moosuri_session={}", member.token);
    for (method, path, payload) in [
        ("GET", "/api/v1/admin/google", Value::Null),
        (
            "POST",
            "/api/v1/admin/google/validate",
            json!({"client_json":client_json(ORIGIN)}),
        ),
        (
            "PUT",
            "/api/v1/admin/google",
            json!({"client_json":client_json(ORIGIN),"expected_revision":1}),
        ),
        ("POST", "/api/v1/admin/google/oauth/start", json!({})),
        (
            "POST",
            "/api/v1/admin/google/check",
            json!({"connection_id":"unknown"}),
        ),
    ] {
        assert_eq!(
            send(&app, method, path, Some(&member_cookie), payload)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    // A known current member binding is still not an administrator's mailbox.
    let pin = store
        .google_app(format!("{ORIGIN}/api/v1/mail/oauth/callback"))
        .await
        .unwrap()
        .unwrap()
        .pin;
    let start = store
        .start_mail_oauth_with_app(&member.user, pin.clone(), false)
        .await
        .unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = store
        .claim_mail_oauth_with_app(&member.user, handle, pin)
        .await
        .unwrap();
    let connection = store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "member@example.test".into(),
        )
        .await
        .unwrap();
    let status = value(
        send(
            &app,
            "GET",
            "/api/v1/admin/google",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(status["personal_connections"], json!([]));
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/admin/google/check",
            Some(&cookie),
            json!({"connection_id":connection.id})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
}

#[tokio::test]
async fn setup_oauth_return_is_stored_and_denial_releases_edit_without_secrets() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    setup(&app, &cookie, 0).await;
    let start = value(
        send(
            &app,
            "POST",
            "/api/v1/admin/google/oauth/start",
            Some(&cookie),
            json!({}),
        )
        .await,
    )
    .await;
    let url = url::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap();
    assert_eq!(
        url.origin().ascii_serialization(),
        "https://accounts.google.com"
    );
    assert!(!url.as_str().contains("canary"));
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let busy = value(
        send(
            &app,
            "PUT",
            "/api/v1/admin/google",
            Some(&cookie),
            json!({"expected_revision":1,"client_json":client_json(ORIGIN)}),
        )
        .await,
    )
    .await;
    assert_eq!(busy["code"], "google_authorization_in_progress");
    let denied = callback(
        &app,
        &format!("state={state}&error=access_denied&error_description=secret-canary"),
    )
    .await;
    assert_eq!(denied.status(), StatusCode::SEE_OTHER);
    assert_eq!(
        denied.headers()[header::LOCATION],
        "/admin/google?oauth_error=denied"
    );
    setup(&app, &cookie, 1).await;
    let start = value(
        send(
            &app,
            "POST",
            "/api/v1/admin/google/oauth/start",
            Some(&cookie),
            json!({}),
        )
        .await,
    )
    .await;
    let url = url::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap();
    let state = url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let captured = callback(
        &app,
        &format!("state={state}&code=synthetic-code&return_to=https://untrusted.example"),
    )
    .await;
    assert_eq!(captured.status(), StatusCode::SEE_OTHER);
    let location = captured.headers()[header::LOCATION].to_str().unwrap();
    assert!(location.starts_with("/admin/google?oauth="));
    assert!(!location.contains("synthetic-code"));
    let handle = location.split_once("oauth=").unwrap().1;
    let second = login(&app, "admin@example.test", PASSWORD).await;
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/mail/oauth/complete",
            Some(&second),
            json!({"handle":handle})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let user = store
        .authenticate(cookie.split_once('=').unwrap().1.into())
        .await
        .unwrap();
    let pin = store
        .google_app(format!("{ORIGIN}/api/v1/mail/oauth/callback"))
        .await
        .unwrap()
        .unwrap()
        .pin;
    let exchange = store
        .claim_mail_oauth_with_app(&user, handle.into(), pin)
        .await
        .unwrap();
    store.cancel_mail_oauth_exchange(&exchange).await.unwrap();
    assert_eq!(setup(&app, &cookie, 2).await["revision"], 3);
}

#[tokio::test]
async fn operator_file_mode_is_explicit_and_web_import_cannot_override_it() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    setup(&app, &cookie, 0).await;
    let config = ServerConfig::new(ORIGIN, false)
        .unwrap()
        .with_google_client_json(&client_json(ORIGIN))
        .unwrap();
    let app = router(store, config);
    let status = value(
        send(
            &app,
            "GET",
            "/api/v1/admin/google",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(status["mode"], "operator_file");
    assert_eq!(status["configured"], true);
    let error = value(
        send(
            &app,
            "PUT",
            "/api/v1/admin/google",
            Some(&cookie),
            json!({"expected_revision":1,"client_json":client_json(ORIGIN)}),
        )
        .await,
    )
    .await;
    assert_eq!(error["code"], "google_managed_by_operator");
}
