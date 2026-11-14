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
    for operation in ["models", "probe"] {
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
