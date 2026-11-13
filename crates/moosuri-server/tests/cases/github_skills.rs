use super::*;
#[tokio::test]
async fn external_preview_requires_current_account_and_rejects_non_github_before_network() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let input = json!({"repository":"https://127.0.0.1/private","directory":"skills/example","reference":"main"});
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/community-skills/github/preview",
            None,
            input.clone()
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/community-skills/github/preview",
            Some(&cookie),
            input.clone()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/community-skills/github/preview")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, &cookie)
                .header("x-moosuri-account", "other-account")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(input.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);
    assert!(store.claim_agent_run().await.unwrap().is_none());
}

#[tokio::test]
#[ignore = "explicitly authorized read-only public GitHub fixture; synthetic local account only"]
async fn explicit_public_github_package_preview_install_roundtrip() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let commit = "7f91c0ad8ef38049c5e0e601658da9b99b5f824e";
    let response=send(&app,"POST","/api/v1/community-skills/github/preview",Some(&cookie),json!({"repository":"https://github.com/codemoo/moosuri","directory":"community/skills/source-briefing","reference":commit})).await;
    assert_eq!(response.status(), StatusCode::OK);
    let preview = value(response).await;
    assert_eq!(preview["source_commit"], commit);
    assert!(preview["id"].as_str().unwrap().starts_with("github-"));
    assert!(preview["installed_digest"].is_null());
    assert_eq!(preview["revision"], 0);
    assert_eq!(
        value(
            send(
                &app,
                "GET",
                "/api/v1/community-skills",
                Some(&cookie),
                json!(null)
            )
            .await
        )
        .await
        .as_array()
        .unwrap()
        .len(),
        6
    );
    assert!(store.claim_agent_run().await.unwrap().is_none());
    let path = format!(
        "/api/v1/community-skills/{}",
        preview["id"].as_str().unwrap()
    );
    let installed = send(
        &app,
        "POST",
        &path,
        Some(&cookie),
        json!({"action":"install","expected_revision":0,"expected_digest":preview["digest"]}),
    )
    .await;
    assert_eq!(installed.status(), StatusCode::OK);
    let cards = value(installed).await;
    let external = cards
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == preview["id"])
        .unwrap();
    assert_eq!(external["installed_digest"], preview["digest"]);
    assert_eq!(external["enabled"], true);
    assert!(store.claim_agent_run().await.unwrap().is_none());
}
