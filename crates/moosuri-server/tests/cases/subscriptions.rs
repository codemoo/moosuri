use super::*;
#[tokio::test]
async fn personal_chatgpt_settings_require_owner_and_shared_default_rejects_personal_source() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, _store, app) = fixture(false).await;
    assert_eq!(
        send(&app, "GET", "/api/v1/settings/chatgpt", None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let signed = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":"admin@example.test","password":PASSWORD}),
    )
    .await;
    let cookie = session_cookie(&signed);
    assert_eq!(
        value(
            send(
                &app,
                "GET",
                "/api/v1/settings/chatgpt",
                Some(&cookie),
                Value::Null
            )
            .await
        )
        .await,
        json!([])
    );
    for operation in ["models", "probe", "tool-probe"] {
        let r = send(
            &app,
            "POST",
            &format!("/api/v1/settings/chatgpt/guessed-binding/{operation}"),
            Some(&cookie),
            json!({"revision":1,"model":"fixture-model","effort":null}),
        )
        .await;
        assert_eq!(r.status(), StatusCode::NOT_FOUND);
        let b = value(r).await;
        assert_eq!(b, json!({"code":"not_found"}));
    }
    let default=send(&app,"POST","/api/v1/admin/providers/default",Some(&cookie),json!({"selection":{"source":"personal_chat_gpt","provider_id":"guessed-binding","provider_revision":1,"model":"fixture-model","effort":null}})).await;
    assert_eq!(default.status(), StatusCode::CONFLICT);
    let launch = send(
        &app,
        "POST",
        "/api/v1/settings/chatgpt/local-login",
        Some(&cookie),
        json!({"label":"Remote attempt","binding_id":null,"expected_revision":null}),
    )
    .await;
    assert_eq!(launch.status(), StatusCode::CONFLICT);
    assert_eq!(
        value(launch).await,
        json!({"code":"same_host_login_required"})
    );
}

#[tokio::test]
async fn handoff_routes_are_owner_authenticated_and_private_material_stays_out_of_status() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, _store, app) = fixture(false).await;
    for (method, path) in [
        ("POST", "/api/v1/settings/chatgpt/handoffs"),
        ("GET", "/api/v1/settings/chatgpt/handoffs/guessed"),
        ("DELETE", "/api/v1/settings/chatgpt/handoffs/guessed"),
    ] {
        let r = send(
            &app,
            method,
            path,
            None,
            json!({"label":"VM","binding_id":null,"expected_revision":null}),
        )
        .await;
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    }
    let login = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":"admin@example.test","password":PASSWORD}),
    )
    .await;
    let cookie = session_cookie(&login);
    let prepared = send(
        &app,
        "POST",
        "/api/v1/settings/chatgpt/handoffs",
        Some(&cookie),
        json!({"label":"VM","binding_id":null,"expected_revision":null}),
    )
    .await;
    assert_eq!(prepared.status(), StatusCode::OK);
    let private = value(prepared).await;
    assert!(private["transfer_key"].is_string());
    let id = private["context"]["ticket"].as_str().unwrap();
    let safe = value(
        send(
            &app,
            "GET",
            &format!("/api/v1/settings/chatgpt/handoffs/{id}"),
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(
        safe,
        json!({"phase":"prepared","receipt":null,"receipt_mac":null})
    );
    for key in [
        "transfer_key",
        "cookie",
        "access_token",
        "refresh_token",
        "authorization_url",
    ] {
        assert!(safe.get(key).is_none());
    }
    let invalid = send(
        &app,
        "PUT",
        &format!("/api/v1/settings/chatgpt/handoffs/{id}/nonce"),
        Some(&cookie),
        json!({"commitment":[1,2],"source_host":"source"}),
    )
    .await;
    assert_eq!(invalid.status(), StatusCode::BAD_REQUEST);
    let cancelled = send(
        &app,
        "DELETE",
        &format!("/api/v1/settings/chatgpt/handoffs/{id}"),
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert_eq!(cancelled.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn password_issued_helper_sessions_allow_only_handoff_session_status_and_logout() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    // Password-authenticated purpose exists at issuance, before prepare or a delivered response.
    let login = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":"admin@example.test","password":PASSWORD,"purpose":"chatgpt_helper"}),
    )
    .await;
    assert_eq!(login.status(), StatusCode::OK);
    let cookie = session_cookie(&login);
    assert!(
        login.headers()["set-cookie"]
            .to_str()
            .unwrap()
            .contains("Max-Age=600")
    );
    let view = value(login).await;
    assert!(view["chat_draft_scope"].is_null());
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );
    for (method, path, input) in [
        (
            "POST",
            "/api/v1/auth/password",
            json!({"password":"Synthetic-forbidden-password-42"}),
        ),
        (
            "POST",
            "/api/v1/admin/users",
            json!({"name":"Forbidden","email":"forbidden@example.test"}),
        ),
        ("GET", "/api/v1/admin/users", Value::Null),
        ("GET", "/api/v1/vault", Value::Null),
        ("POST", "/api/v1/vault/handoff", Value::Null),
        ("GET", "/api/v1/source-connections", Value::Null),
        (
            "POST",
            "/api/v1/source-connections/guessed/token",
            json!({"ttl_seconds":3600}),
        ),
        (
            "POST",
            "/api/v1/settings/chatgpt/guessed/tool-probe",
            json!({"revision":1,"model":"fixture-model","effort":null}),
        ),
        ("GET", "/api/v1/threads", Value::Null),
        (
            "POST",
            "/api/v1/shares/origin",
            json!({"origin_scope":"0".repeat(64),"expected_current_scope":"0".repeat(64)}),
        ),
        (
            "POST",
            "/api/v1/settings/chatgpt/local-login",
            json!({"label":"forbidden","binding_id":null,"expected_revision":null}),
        ),
    ] {
        let r = send(&app, method, path, Some(&cookie), input).await;
        assert_eq!(r.status(), StatusCode::FORBIDDEN, "{method} {path}");
        assert!(r.headers().get("set-cookie").is_none());
    }
    // A member helper has the identical narrow purpose; role does not widen it.
    let admin = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &admin.user,
            "Member".into(),
            "helper-member@example.test".into(),
        )
        .await
        .unwrap();
    let temporary = store
        .login(
            "helper-member@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap();
    store
        .change_password(&temporary.user, PASSWORD.into())
        .await
        .unwrap();
    let member=send(&app,"POST","/api/v1/auth/login",None,json!({"email":"helper-member@example.test","password":PASSWORD,"purpose":"chatgpt_helper"})).await;
    assert_eq!(member.status(), StatusCode::OK);
    let member_cookie = session_cookie(&member);
    for (method, path, input) in [
        (
            "POST",
            "/api/v1/auth/password",
            json!({"password":"Synthetic-forbidden-password-42"}),
        ),
        ("GET", "/api/v1/admin/users", Value::Null),
        ("GET", "/api/v1/vault", Value::Null),
        ("GET", "/api/v1/source-connections", Value::Null),
        (
            "POST",
            "/api/v1/settings/chatgpt/guessed/tool-probe",
            json!({"revision":1,"model":"fixture-model","effort":null}),
        ),
        ("GET", "/api/v1/threads", Value::Null),
    ] {
        assert_eq!(
            send(&app, method, path, Some(&member_cookie), input)
                .await
                .status(),
            StatusCode::FORBIDDEN,
            "member {path}"
        );
    }
    let prepared = send(
        &app,
        "POST",
        "/api/v1/settings/chatgpt/handoffs",
        Some(&cookie),
        json!({"label":"VM","binding_id":null,"expected_revision":null}),
    )
    .await;
    assert_eq!(prepared.status(), StatusCode::OK);
    let p = value(prepared).await;
    let id = p["context"]["ticket"].as_str().unwrap();
    assert_eq!(
        send(
            &app,
            "GET",
            &format!("/api/v1/settings/chatgpt/handoffs/{id}"),
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/auth/logout",
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    // No replacement password, account or lasting successor was created.
    assert!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .is_ok()
    );
}
