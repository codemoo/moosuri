use super::*;
use std::os::unix::fs::PermissionsExt;
fn private(path: &std::path::Path, value: &str) {
    std::fs::write(path, value).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
#[tokio::test]
async fn gws_inbox_http_exposes_only_current_owner_descriptor_and_observes_revocation() {
    let _g = TEST_LOCK.lock().await;
    let (dir, store, old) = fixture(false).await;
    let admin = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &admin.user,
            "Member".into(),
            "gws-member@example.test".into(),
        )
        .await
        .unwrap();
    let member = store
        .login("gws-member@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    store
        .change_password(&member.user, "Synthetic-GWS-Member-Password42".into())
        .await
        .unwrap();
    let key = dir.path().join("gws.key");
    private(&key, &"a".repeat(64));
    let config = dir.path().join("gws.json");
    private(&config,&json!([{"account_id":member.user.account().id,"mailbox":"private-member@example.test","endpoint":"http://127.0.0.1:1/v1/inbox/latest","key_file":key}]).to_string());
    drop(old);
    let app = router(
        store,
        ServerConfig::new(ORIGIN, false)
            .unwrap()
            .with_gws_inbox_file(config.clone())
            .unwrap(),
    );
    let a = login(&app, "admin@example.test", PASSWORD).await;
    let b = login(
        &app,
        "gws-member@example.test",
        "Synthetic-GWS-Member-Password42",
    )
    .await;
    let path = "/api/v1/mail/gws-inbox";
    assert_eq!(
        send(&app, "GET", path, None, Value::Null).await.status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        value(send(&app, "GET", path, Some(&a), Value::Null).await).await,
        json!({"configured":false})
    );
    let status = value(send(&app, "GET", path, Some(&b), Value::Null).await).await;
    assert_eq!(
        status,
        json!({"configured":true,"mailbox":"private-member@example.test"})
    );
    assert!(!status.to_string().contains("127.0.0.1"));
    assert!(!status.to_string().contains("key_file"));
    private(&config, "[]");
    assert_eq!(
        value(send(&app, "GET", path, Some(&b), Value::Null).await).await,
        json!({"configured":false})
    );
}
