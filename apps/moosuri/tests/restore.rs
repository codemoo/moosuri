use moosuri_core::*;
use moosuri_store::{Store, backup};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    process::{Command, Output},
};
fn cli(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .unwrap()
}
#[tokio::test]
async fn native_checkpoint_restore_enroll_login_read_and_retry_are_usable() {
    let root = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let data = root.path().join("source");
    let dest = root.path().join("restored");
    let kit = root.path().join("kit");
    fs::DirBuilder::new().mode(0o700).create(&kit).unwrap();
    let store = Store::open(&data).unwrap();
    store
        .bootstrap(
            "Synthetic".into(),
            "admin@example.test".into(),
            "Old-checkpoint-password".into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login(
            "admin@example.test".into(),
            "Old-checkpoint-password".into(),
        )
        .await
        .unwrap();
    let account = login.user.account().id.clone();
    let memory = store
        .create_memory(
            &login.user,
            "00000000-0000-4000-8000-000000000001".into(),
            MemoryDraft {
                text: "Restored current content".into(),
                subject: "synthetic".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    drop(store);
    let identity = kit.join("identity");
    let recipient = backup::keygen(&identity).unwrap();
    backup::setup(&data, &recipient, &identity).unwrap();
    let cipher = kit.join("checkpoint.age");
    let heads = root.path().join("heads");
    fs::DirBuilder::new().mode(0o700).create(&heads).unwrap();
    let head = heads.join("trusted-head.json");
    let seal = cli(
        &data,
        &[
            "backup",
            "checkpoint",
            "--seal-installation",
            "--output",
            cipher.to_str().unwrap(),
            "--head-output",
            head.to_str().unwrap(),
        ],
    );
    assert!(
        seal.status.success(),
        "{}",
        String::from_utf8_lossy(&seal.stderr)
    );
    let args = [
        "backup",
        "restore",
        "--checkpoint",
        cipher.to_str().unwrap(),
        "--trusted-head",
        head.to_str().unwrap(),
        "--identity-file",
        identity.to_str().unwrap(),
    ];
    let restored = cli(&dest, &args);
    assert!(
        restored.status.success(),
        "{}",
        String::from_utf8_lossy(&restored.stderr)
    );
    let stdout = String::from_utf8_lossy(&restored.stdout);
    assert!(stdout.contains(&account));
    assert!(stdout.contains("recover-account"));
    assert!(stdout.contains("source stays sealed"));
    let password = kit.join("new-password");
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&password)
        .unwrap()
        .write_all(b"New-enrollment-password")
        .unwrap();
    let enrolled = cli(
        &dest,
        &[
            "recover-account",
            "--account-id",
            &account,
            "--role",
            "admin",
            "--password-file",
            password.to_str().unwrap(),
        ],
    );
    assert!(
        enrolled.status.success(),
        "{}",
        String::from_utf8_lossy(&enrolled.stderr)
    );
    let store = Store::open(&dest).unwrap();
    assert!(
        store
            .login(
                "admin@example.test".into(),
                "Old-checkpoint-password".into()
            )
            .await
            .is_err()
    );
    let restricted = store
        .login(
            "admin@example.test".into(),
            "New-enrollment-password".into(),
        )
        .await
        .unwrap();
    assert!(restricted.user.password_change_required());
    let current = store
        .change_password(&restricted.user, "New-restored-password".into())
        .await
        .unwrap();
    assert_eq!(
        store
            .memory(&current.user, memory.id, None)
            .await
            .unwrap()
            .draft
            .text,
        "Restored current content"
    );
    drop(store);
    let again = cli(&dest, &args);
    assert!(again.status.success());
    let again = String::from_utf8_lossy(&again.stdout);
    assert!(again.contains("Matching restore already published"));
    assert!(again.contains("preserved current destination enrollment"));
    assert!(!again.contains("All accounts require fresh local enrollment"));
    assert!(!cli(&data, &["doctor"]).status.success());
    assert!(cipher.exists());
    assert!(head.exists());
    assert!(identity.exists());
}
#[test]
fn missing_restore_inputs_never_initialize_a_destination() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let dest = root.path().join("not-created");
    let missing = root.path().join("missing");
    let out = cli(
        &dest,
        &[
            "backup",
            "restore",
            "--checkpoint",
            missing.to_str().unwrap(),
            "--trusted-head",
            missing.to_str().unwrap(),
            "--identity-file",
            missing.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    assert!(!dest.exists());
}
