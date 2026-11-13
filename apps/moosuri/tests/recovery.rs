use moosuri_core::Role;
use moosuri_store::Store;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
};

const PASSWORD: &str = "Synthetic-operator-password-canary";

fn recover(data: &Path, password: &Path, account: &str, role: &str) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args([
            "recover-account",
            "--account-id",
            account,
            "--role",
            role,
            "--password-file",
        ])
        .arg(password)
        .output()
        .unwrap()
}

#[tokio::test]
async fn cli_requires_stopped_existing_locked_account_and_private_password_file() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let data = root.path().join("data");
    let password = root.path().join("password");
    fs::write(&password, PASSWORD).unwrap();
    fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
    let missing = recover(
        &data,
        &password,
        "00000000-0000-4000-8000-000000000001",
        "admin",
    );
    assert!(!missing.status.success());
    assert!(!data.exists());
    fs::create_dir(&data).unwrap();
    fs::set_permissions(&data, fs::Permissions::from_mode(0o700)).unwrap();
    let empty_database = data.join("assistant.sqlite");
    fs::write(&empty_database, []).unwrap();
    fs::set_permissions(&empty_database, fs::Permissions::from_mode(0o600)).unwrap();
    let uninitialized = recover(
        &data,
        &password,
        "00000000-0000-4000-8000-000000000001",
        "admin",
    );
    assert!(!uninitialized.status.success());
    assert_eq!(fs::metadata(&empty_database).unwrap().len(), 0);
    assert_eq!(fs::read_dir(&data).unwrap().count(), 1);
    assert!(!String::from_utf8_lossy(&uninitialized.stdout).contains(PASSWORD));
    assert!(!String::from_utf8_lossy(&uninitialized.stderr).contains(PASSWORD));
    fs::remove_file(empty_database).unwrap();
    let store = Store::open(&data).unwrap();
    let user = store
        .bootstrap(
            "Synthetic Admin".into(),
            "admin@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let held = recover(&data, &password, &user.id, "member");
    assert!(!held.status.success());
    assert!(String::from_utf8_lossy(&held.stderr).contains("store busy"));
    drop(store);
    let healthy = recover(&data, &password, &user.id, "member");
    assert!(!healthy.status.success());
    assert!(String::from_utf8_lossy(&healthy.stderr).contains("account_not_recovery_required"));
    let invalid_role = recover(&data, &password, &user.id, "owner");
    assert!(!invalid_role.status.success());
    let malformed = recover(&data, &password, "bad-uuid", "admin");
    assert!(!malformed.status.success());
    fs::set_permissions(&password, fs::Permissions::from_mode(0o640)).unwrap();
    let permissive = recover(&data, &password, &user.id, "admin");
    assert!(!permissive.status.success());
    fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
    let link = root.path().join("link");
    symlink(&password, &link).unwrap();
    let linked = recover(&data, &link, &user.id, "admin");
    assert!(!linked.status.success());
    for result in [
        missing,
        held,
        healthy,
        invalid_role,
        malformed,
        permissive,
        linked,
    ] {
        assert!(!String::from_utf8_lossy(&result.stdout).contains(PASSWORD));
        assert!(!String::from_utf8_lossy(&result.stderr).contains(PASSWORD));
    }
    let store = Store::open(&data).unwrap();
    let login = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(login.user.account().role, Role::Admin);
    assert!(!login.user.password_change_required());
}
