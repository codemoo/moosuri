use super::*;

#[tokio::test]
async fn provider_quota_is_owner_scoped_admin_only_and_unknown_is_not_zero() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    let a = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invited = store
        .create_user(&a.user, "Member".into(), "quota-member@example.test".into())
        .await
        .unwrap();
    let member = store
        .login(
            "quota-member@example.test".into(),
            invited.temporary_password,
        )
        .await
        .unwrap();
    store
        .change_password(&member.user, "Synthetic-Quota-member-password".into())
        .await
        .unwrap();
    let owner = login(&app, "admin@example.test", PASSWORD).await;
    let other = login(
        &app,
        "quota-member@example.test",
        "Synthetic-Quota-member-password",
    )
    .await;
    let prepared = store
        .prepare_chatgpt_handoff(&a.user, None, "Private quota fixture".into())
        .await
        .unwrap();
    let personal = format!(
        "/api/v1/settings/chatgpt/{}/quota?revision=0",
        prepared.context.binding
    );
    assert_eq!(
        send(&app, "GET", &personal, None, Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&app, "GET", &personal, Some(&other), Value::Null)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let result = value(send(&app, "GET", &personal, Some(&owner), Value::Null).await).await;
    assert_eq!(result["status"], "unsupported");
    assert_eq!(result["scope"], "personal_subscription");
    assert_eq!(result["source"], "personal_chat_gpt");
    assert_eq!(result["metrics"], json!([]));
    assert_eq!(result["observed_at"], Value::Null);
    assert!(
        result["evidence_url"]
            .as_str()
            .unwrap()
            .starts_with("https://developers.openai.com/siwc/")
    );
    let wrong = personal.replace("revision=0", "revision=1");
    assert_eq!(
        send(&app, "GET", &wrong, Some(&owner), Value::Null)
            .await
            .status(),
        StatusCode::CONFLICT
    );
    let p = store
        .create_provider(
            &a.user,
            moosuri_store::ProviderChange {
                name: "Private shared fixture".into(),
                enabled: true,
                config: moosuri_core::ProviderConfig {
                    kind: moosuri_core::ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: moosuri_store::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    let shared = format!("/api/v1/admin/providers/{}/quota?revision=1", p.id);
    assert_eq!(
        send(&app, "GET", &shared, None, Value::Null).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(&app, "GET", &shared, Some(&other), Value::Null)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let result = value(send(&app, "GET", &shared, Some(&owner), Value::Null).await).await;
    assert_eq!(result["status"], "unsupported");
    assert_eq!(result["scope"], "shared_provider_key");
    assert_eq!(result["metrics"], json!([]));
    assert_eq!(result["observed_at"], Value::Null);
    assert_eq!(
        send(
            &app,
            "GET",
            &shared.replace("revision=1", "revision=2"),
            Some(&owner),
            Value::Null
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    // Neither unsupported query opens the compatible connection's listening port.
}
