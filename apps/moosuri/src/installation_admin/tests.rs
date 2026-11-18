use super::*;
use std::os::unix::fs::PermissionsExt;
fn dir() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}
#[tokio::test]
async fn actual_installed_private_login_bootstrap_and_temporary_password_boundaries() {
    let registry = dir();
    let ipc_root = dir();
    let root = dir();
    let a = runtime_registry::reserve(registry.path(), Uuid::new_v4(), "a@example.test".into())
        .unwrap();
    runtime_registry::set_routing(registry.path(), &a, true).unwrap();
    let owner = moosuri_store::personal_volume::VolumeOwner {
        account: Uuid::parse_str(&a.account_id).unwrap(),
        tenant: Uuid::parse_str(&a.tenant_id).unwrap(),
        volume: Uuid::parse_str(&a.volume_id).unwrap(),
        generation: 1,
    };
    owner
        .initialize(
            root.path(),
            "Synthetic Admin Owner".into(),
            "a@example.test".into(),
            "Synthetic-temporary-password-42".into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(root.path()).await.unwrap();
    let child = ipc_root.path().join(&a.account_id);
    std::fs::create_dir(&child).unwrap();
    std::fs::set_permissions(&child, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = child.join("personal.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let health = axum::routing::get(move || async move {
        (
            StatusCode::NO_CONTENT,
            [
                ("x-moosuri-account", owner.account.to_string()),
                ("x-moosuri-tenant", owner.tenant.to_string()),
                ("x-moosuri-volume", owner.volume.to_string()),
                ("x-moosuri-generation", owner.generation.to_string()),
            ],
        )
    });
    let router = moosuri_server::personal_auth_router(store.clone()).route("/health/ready", health);
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let id = Uuid::new_v4();
    let uid = unsafe { libc::geteuid() };
    assert!(
        bootstrap(
            registry.path(),
            id,
            "a@example.test",
            ipc_root.path(),
            uid,
            Zeroizing::new("Synthetic-temporary-password-42".into())
        )
        .await
        .is_err()
    );
    let initial = store
        .login(
            "a@example.test".into(),
            "Synthetic-temporary-password-42".into(),
        )
        .await
        .unwrap();
    store
        .change_password(&initial.user, "Synthetic-changed-password-42".into())
        .await
        .unwrap();
    assert!(
        bootstrap(
            registry.path(),
            id,
            "a@example.test",
            ipc_root.path(),
            uid.wrapping_add(1),
            Zeroizing::new("Synthetic-changed-password-42".into())
        )
        .await
        .is_err()
    );
    let member = bootstrap(
        registry.path(),
        id,
        "a@example.test",
        ipc_root.path(),
        uid,
        Zeroizing::new("Synthetic-changed-password-42".into()),
    )
    .await
    .unwrap();
    assert_eq!(member.role, "admin");
    assert_eq!(
        bootstrap(
            registry.path(),
            id,
            "a@example.test",
            ipc_root.path(),
            uid,
            Zeroizing::new("Synthetic-changed-password-42".into())
        )
        .await
        .unwrap()
        .role_revision,
        member.role_revision
    );
    let login = store
        .login(
            "a@example.test".into(),
            "Synthetic-changed-password-42".into(),
        )
        .await
        .unwrap();
    assert_eq!(login.user.account().role, moosuri_core::Role::Member);
    let bytes = std::fs::read(registry.path().join("account-runtimes.sqlite")).unwrap();
    for secret in ["Synthetic-changed-password-42", "moosuri_session"] {
        assert!(!bytes.windows(secret.len()).any(|b| b == secret.as_bytes()));
    }
    task.abort();
}
