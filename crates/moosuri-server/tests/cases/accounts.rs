use super::*;

#[tokio::test]
async fn administrative_management_is_private_fenced_and_never_replays_a_password() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let a = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &a.user,
            "Synthetic Member".into(),
            "member@example.test".into(),
        )
        .await
        .unwrap();
    let member = store
        .login("member@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    store
        .change_password(&member.user, PASSWORD.into())
        .await
        .unwrap();
    let member_cookie = login(&app, "member@example.test", PASSWORD).await;
    let response = send(
        &app,
        "GET",
        "/api/v1/admin/users",
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert!(
        response
            .headers()
            .get(header::CACHE_CONTROL)
            .unwrap()
            .to_str()
            .unwrap()
            .contains("no-store")
    );
    let list = value(response).await;
    let id = invite.user.id;
    let row = list["management"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["user"]["id"] == id)
        .unwrap();
    assert_eq!(row["status"], "active");
    assert!(!list.to_string().contains("password_hash"));
    let route = format!("/api/v1/admin/users/{id}/manage");
    let request = json!({"request_id":"00000000-0000-4000-8000-000000000001","expected_revision":row["revision"],"action":{"kind":"reset_password"}});
    assert_eq!(
        send(&app, "POST", &route, Some(&member_cookie), request.clone())
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(&route)
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, &cookie)
                .header("x-moosuri-account", &id)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(request.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);
    let mut unknown = request.clone();
    unknown["unrecognized"] = json!(true);
    assert_eq!(
        send(&app, "POST", &route, Some(&cookie), unknown)
            .await
            .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let response = send(&app, "POST", &route, Some(&cookie), request.clone()).await;
    assert_eq!(response.status(), StatusCode::OK);
    let result = value(response).await;
    let password = result["temporary_password"].as_str().unwrap().to_owned();
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/threads",
            Some(&member_cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let replay = value(send(&app, "POST", &route, Some(&cookie), request).await).await;
    assert!(replay["temporary_password"].is_null());
    assert_eq!(result["receipt"], replay["receipt"]);
    let new_cookie = login(&app, "member@example.test", &password).await;
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/threads",
            Some(&new_cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let suspend = json!({"request_id":"00000000-0000-4000-8000-000000000002","expected_revision":result["receipt"]["revision"],"action":{"kind":"suspend"}});
    assert_eq!(
        send(&app, "POST", &route, Some(&cookie), suspend)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&new_cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let list = value(
        send(
            &app,
            "GET",
            "/api/v1/admin/users",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert!(!list.to_string().contains(&password));
    assert_eq!(
        list["management"]
            .as_array()
            .unwrap()
            .iter()
            .find(|r| r["user"]["id"] == id)
            .unwrap()["status"],
        "suspended"
    );
}
