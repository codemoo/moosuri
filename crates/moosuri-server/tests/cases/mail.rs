use super::*;
use moosuri_connectors::gmail::{SecretString, TokenSet};
async fn seed_mail(store: &Store, cookie: &str) -> String {
    let user = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let start = store.start_mail_oauth(&user).await.unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let ex = store.claim_mail_oauth(&user, handle).await.unwrap();
    store
        .complete_mail_oauth(
            ex,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap()
        .id
}
#[tokio::test]
async fn personal_http_preview_then_explicit_approval_admits_exactly_one_snapshot() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let connection = seed_mail(&store, &cookie).await;
    let body = json!({"connection_id":connection,"content":{"from":"sender@example.test","to":["to@example.test"],"cc":["cc@example.test"],"bcc":["hidden@example.test"],"subject":"합성 메일 ✓","text":"전체 합성 본문","reply":null},"origin_thread_id":null});
    let response = send(
        &app,
        "POST",
        "/api/v1/mail/actions",
        Some(&cookie),
        body.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let action = value(response).await;
    assert_eq!(action["content"]["bcc"][0], "hidden@example.test");
    assert_eq!(action["content"]["text"], "전체 합성 본문");
    assert!(action.get("raw").is_none());
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let path = format!(
        "/api/v1/mail/actions/{}/approve",
        action["id"].as_str().unwrap()
    );
    let mut approval = json!({"revision":action["revision"],"digest":action["digest"]});
    approval["text"] = json!("changed after preview");
    assert_eq!(
        send(&app, "POST", &path, Some(&cookie), approval)
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let approval = json!({"revision":action["revision"],"digest":action["digest"]});
    assert_eq!(
        send(&app, "POST", &path, Some(&cookie), approval.clone())
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "POST", &path, Some(&cookie), approval)
            .await
            .status(),
        StatusCode::OK
    );
    let dispatch = store.next_mail_dispatch().await.unwrap().unwrap();
    let parsed = moosuri_connectors::gmail::parse_mail(dispatch.frozen.raw_mime()).unwrap();
    assert_eq!(parsed.bcc, vec!["hidden@example.test"]);
    assert_eq!(parsed.text.trim_end(), "전체 합성 본문");
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    store
        .finish_mail_dispatch(&dispatch, None, false)
        .await
        .unwrap();
    let status = value(
        send(
            &app,
            "GET",
            "/api/v1/mail/actions",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(status["actions"][0]["state"], "outcome_unknown");
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let mut unsupported = body;
    unsupported["content"]["attachments"] = json!(["private-file"]);
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/mail/actions",
            Some(&cookie),
            unsupported
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
}
#[tokio::test]
async fn oauth_callback_works_without_strict_cookie_but_complete_requires_original_session() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, _app) = fixture(false).await;
    let config=ServerConfig::new(ORIGIN,false).unwrap().with_google_client_json(&json!({"web":{"client_id":"synthetic-client","client_secret":"synthetic-secret","redirect_uris":[format!("{ORIGIN}/api/v1/mail/oauth/callback")]}}).to_string()).unwrap();
    let app = router(store.clone(), config);
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let start = value(
        send(
            &app,
            "POST",
            "/api/v1/mail/oauth/start",
            Some(&cookie),
            json!({}),
        )
        .await,
    )
    .await;
    let u = url::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap();
    let state = u
        .query_pairs()
        .find(|(k, _)| k == "state")
        .unwrap()
        .1
        .into_owned();
    let request = Request::builder()
        .uri(format!(
            "/api/v1/mail/oauth/callback?state={state}&code=synthetic-code&hd=example.test"
        ))
        .header(header::HOST, HOST)
        .body(Body::empty())
        .unwrap();
    let response = app.clone().oneshot(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response.headers()[header::LOCATION].to_str().unwrap();
    assert!(!location.contains("synthetic-code"));
    let handle = location.split("oauth=").nth(1).unwrap().to_owned();
    let other = login(&app, "admin@example.test", PASSWORD).await;
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/mail/oauth/complete",
            Some(&other),
            json!({"handle":handle})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let user = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    assert!(store.claim_mail_oauth(&user, handle.clone()).await.is_ok());
    assert!(store.claim_mail_oauth(&user, handle).await.is_err());
}
#[tokio::test]
async fn missing_operator_config_and_stale_identity_are_honest() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, _store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let c = value(
        send(
            &app,
            "GET",
            "/api/v1/mail/connections",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(c["configured"], false);
    let start = send(
        &app,
        "POST",
        "/api/v1/mail/oauth/start",
        Some(&cookie),
        json!({}),
    )
    .await;
    assert_eq!(start.status(), StatusCode::CONFLICT);
    assert_eq!(value(start).await["code"], "gmail_setup_required");
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/v1/mail/connections")
                .header(header::HOST, HOST)
                .header(header::COOKIE, cookie)
                .header("x-moosuri-account", "another-account")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn disconnect_waits_for_held_delivery_and_notifies_after_local_purge() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, old_app) = fixture(false).await;
    let cookie = login(&old_app, "admin@example.test", PASSWORD).await;
    let id = seed_mail(&store, &cookie).await;
    let runtime = moosuri_agent::Runtime::start(store.clone());
    let app = moosuri_server::router_with_runtime(
        store.clone(),
        ServerConfig::new(ORIGIN, false).unwrap(),
        runtime.clone(),
    );
    let user = store
        .authenticate(cookie.strip_prefix("moosuri_session=").unwrap().into())
        .await
        .unwrap();
    let gate = runtime.delivery_gate();
    let delivery = gate.read().await;
    let mut changed = runtime.subscribe();
    changed.borrow_and_update();
    let path = format!("/api/v1/mail/connections/{id}");
    let mut request =
        tokio::spawn(async move { send(&app, "DELETE", &path, Some(&cookie), Value::Null).await });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut request)
            .await
            .is_err()
    );
    assert_eq!(
        store.mail_connections(&user).await.unwrap()[0].status,
        "active"
    );
    drop(delivery);
    assert_eq!(request.await.unwrap().status(), StatusCode::NO_CONTENT);
    tokio::time::timeout(std::time::Duration::from_secs(1), changed.changed())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        store.mail_connections(&user).await.unwrap()[0].status,
        "disconnected"
    );
    runtime.shutdown().await;
}
