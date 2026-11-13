use super::*;

async fn producer_request(
    app: &Router,
    method: &str,
    path: &str,
    token: &str,
    body: Value,
) -> axum::response::Response {
    app.clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, HOST)
                .header(header::AUTHORIZATION, format!("Bearer {token}"))
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap()
}
async fn custom(app: &Router, cookie: &str) -> Value {
    let response = send(
        app,
        "POST",
        "/api/v1/source-connections",
        Some(cookie),
        json!({"kind":"custom_http","name":"Personal notes","scope":"notes/v1"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    value(response).await
}
#[tokio::test]
async fn custom_http_commit_replay_provenance_and_revocation() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let connection = custom(&app, &cookie).await;
    let id = connection["id"].as_str().unwrap();
    let grant = value(
        send(
            &app,
            "POST",
            &format!("/api/v1/source-connections/{id}/token"),
            Some(&cookie),
            json!({"ttl_seconds":3600}),
        )
        .await,
    )
    .await;
    let token = grant["token"].as_str().unwrap();
    let status = value(
        producer_request(
            &app,
            "GET",
            "/api/v1/source-ingest/status",
            token,
            json!(null),
        )
        .await,
    )
    .await;
    let run =
        value(producer_request(&app, "POST", "/api/v1/source-ingest/runs", token, json!({})).await)
            .await;
    let batch = json!({"protocol_version":1,"request_id":"11000000-0000-4000-8000-000000000001",
        "config_revision":status["config_revision"],"run_id":run["run_id"],"generation":run["generation"],
        "expected_sequence":status["local_sequence"],"checkpoint_before":null,"checkpoint_after":"opaque-provider-page-1",
        "complete_snapshot":false,"entries":[{"external_id":"note-1","version":"v1","expected_revision":null,"title":"A note","text":"Synthetic source text","speaker":null,"observed_at":1700000000,"valid_from":null,"valid_until":null}],
        "metadata":[{"external_id":"note-1","thread_id":"thread-1","parent_id":null}]});
    let response = producer_request(
        &app,
        "POST",
        "/api/v1/source-ingest/batches",
        token,
        batch.clone(),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()[header::CACHE_CONTROL], "no-store");
    let receipt = value(response).await;
    assert_eq!(receipt["sequence"], 1);
    assert_eq!(
        value(
            producer_request(
                &app,
                "POST",
                "/api/v1/source-ingest/batches",
                token,
                batch.clone()
            )
            .await
        )
        .await,
        receipt
    );
    let mut conflict = batch.clone();
    conflict["entries"][0]["text"] = json!("Changed request bytes");
    assert_eq!(
        producer_request(
            &app,
            "POST",
            "/api/v1/source-ingest/batches",
            token,
            conflict
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let source = receipt["source_ids"][0].as_str().unwrap();
    let doc = value(
        send(
            &app,
            "GET",
            &format!("/api/v1/knowledge/sources/{source}"),
            Some(&cookie),
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(doc["provenance"]["connection_id"], id);
    assert_eq!(doc["provenance"]["thread_id"], "thread-1");
    assert_eq!(doc["provenance"]["request_id"], batch["request_id"]);
    let managed = value(
        send(
            &app,
            "GET",
            "/api/v1/knowledge/collections",
            Some(&cookie),
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(managed["collections"][0]["source_connection_id"], id);
    let bypass = json!({"request_id":"11000000-0000-4000-8000-000000000099","expected_cursor":1,"next_cursor":2,"complete_snapshot":false,"entries":batch["entries"]});
    assert_ne!(
        send(
            &app,
            "POST",
            &format!(
                "/api/v1/knowledge/collections/{}/batches",
                connection["collection_id"].as_str().unwrap()
            ),
            Some(&cookie),
            bypass
        )
        .await
        .status(),
        StatusCode::CREATED
    );
    let finish = json!({"run_id":run["run_id"],"generation":run["generation"],"outcome":"completed","error":null,"retry_after_seconds":null});
    assert_eq!(
        producer_request(
            &app,
            "POST",
            "/api/v1/source-ingest/finish",
            token,
            finish.clone()
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        producer_request(&app, "POST", "/api/v1/source-ingest/finish", token, finish)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        value(
            producer_request(
                &app,
                "POST",
                "/api/v1/source-ingest/batches",
                token,
                batch.clone()
            )
            .await
        )
        .await,
        receipt
    );
    let history = value(
        send(
            &app,
            "GET",
            &format!("/api/v1/source-connections/{id}/runs"),
            Some(&cookie),
            json!(null),
        )
        .await,
    )
    .await;
    assert_eq!(history["runs"][0]["producer_report"], true);
    assert!(!history.to_string().contains("opaque-provider-page-1"));
    assert!(!history.to_string().contains(token));
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/v1/source-connections/{id}/revoke-token"),
            Some(&cookie),
            json!({})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        producer_request(
            &app,
            "GET",
            "/api/v1/source-ingest/status",
            token,
            json!(null)
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        producer_request(&app, "POST", "/api/v1/source-ingest/batches", token, batch)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
}
#[tokio::test]
async fn collector_is_not_a_browser_session_and_cannot_claim_official_or_other_tenant() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let owner = store
        .authenticate(cookie.split_once('=').unwrap().1.into())
        .await
        .unwrap();
    let invited = store
        .create_user(&owner, "Member".into(), "member@example.test".into())
        .await
        .unwrap();
    let temporary = store
        .login(
            "member@example.test".into(),
            invited.temporary_password.clone(),
        )
        .await
        .unwrap();
    let logged = store
        .change_password(&temporary.user, "Member-Password-2026".into())
        .await
        .unwrap();
    let member_cookie = format!("moosuri_session={}", logged.token);
    let connection = custom(&app, &cookie).await;
    let id = connection["id"].as_str().unwrap();
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("/api/v1/source-connections/{id}"),
            Some(&member_cookie),
            json!(null)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("/api/v1/source-connections/{id}/token"),
            Some(&member_cookie),
            json!({"ttl_seconds":3600})
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let grant = value(
        send(
            &app,
            "POST",
            &format!("/api/v1/source-connections/{id}/token"),
            Some(&cookie),
            json!({"ttl_seconds":3600}),
        )
        .await,
    )
    .await;
    let token = grant["token"].as_str().unwrap();
    for path in [
        "/api/v1/knowledge/collections",
        "/api/v1/threads",
        "/api/v1/source-connections",
    ] {
        assert_eq!(
            producer_request(&app, "GET", path, token, json!(null))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/source-ingest/runs",
            Some(&cookie),
            json!({})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let forged = json!({"kind":"custom_http","name":"Fake official","scope":"notes/v1","remote_account":"someone","tenant_id":owner.account().id});
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/source-connections",
            Some(&cookie),
            forged
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let state = value(
        producer_request(
            &app,
            "GET",
            "/api/v1/source-ingest/status",
            token,
            json!(null),
        )
        .await,
    )
    .await;
    let run =
        value(producer_request(&app, "POST", "/api/v1/source-ingest/runs", token, json!({})).await)
            .await;
    let fake_completion = json!({"run_id":run["run_id"],"generation":run["generation"],"outcome":"completed","error":null});
    assert_eq!(
        producer_request(
            &app,
            "POST",
            "/api/v1/source-ingest/finish",
            token,
            fake_completion
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let current = value(
        send(
            &app,
            "GET",
            &format!("/api/v1/source-connections/{id}"),
            Some(&cookie),
            json!(null),
        )
        .await,
    )
    .await;
    assert!(current["last_commit_at"].is_null());
    assert!(current["last_completed_at"].is_null());
    assert_eq!(state["local_sequence"], 0);
}
