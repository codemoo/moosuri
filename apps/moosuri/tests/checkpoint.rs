use moosuri_store::{Store, backup};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::Path,
    process::{Command, Output},
};
fn root() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
fn run(data: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .unwrap()
}
#[test]
fn checkpoint_requires_deliberate_flag_and_never_initializes_missing_data() {
    let root = root();
    let data = root.path().join("missing");
    let output = root.path().join("out.age");
    let head = root.path().join("head.json");
    let out = run(
        &data,
        &[
            "backup",
            "checkpoint",
            "--output",
            output.to_str().unwrap(),
            "--head-output",
            head.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("--seal-installation"));
    assert!(!data.exists());
    let out = run(
        &data,
        &[
            "backup",
            "checkpoint",
            "--seal-installation",
            "--output",
            output.to_str().unwrap(),
            "--head-output",
            head.to_str().unwrap(),
        ],
    );
    assert!(!out.status.success());
    assert!(!data.exists());
    assert!(!output.exists());
    assert!(!head.exists());
}
#[tokio::test]
async fn native_checkpoint_retires_cli_entrypoints_and_retries_identical_outputs() {
    let root = root();
    let data = root.path().join("data");
    let kit = root.path().join("kit");
    let outputs = root.path().join("outputs");
    let heads = root.path().join("heads");
    for p in [&kit, &outputs, &heads] {
        fs::DirBuilder::new().mode(0o700).create(p).unwrap()
    }
    let store = Store::open(&data).unwrap();
    store
        .bootstrap(
            "Synthetic".into(),
            "admin@example.test".into(),
            "Synthetic-cutover-password".into(),
            false,
        )
        .await
        .unwrap();
    let account = store
        .login(
            "admin@example.test".into(),
            "Synthetic-cutover-password".into(),
        )
        .await
        .unwrap()
        .user
        .account()
        .id
        .clone();
    drop(store);
    let identity = kit.join("identity");
    let recipient = backup::keygen(&identity).unwrap();
    backup::setup(&data, &recipient, &identity).unwrap();
    let output = outputs.join("checkpoint.age");
    let head = heads.join("head.json");
    let args = [
        "backup",
        "checkpoint",
        "--seal-installation",
        "--output",
        output.to_str().unwrap(),
        "--head-output",
        head.to_str().unwrap(),
    ];
    let out = run(&data, &args);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.contains("Source permanently sealed"));
    assert!(text.contains("locally complete"));
    assert!(text.contains("No off-host transfer, restore or ongoing disaster protection"));
    let status = run(&data, &["backup", "checkpoint-status"]);
    assert!(status.status.success());
    assert!(String::from_utf8_lossy(&status.stdout).contains("checkpoint_ready"));
    let cipher = fs::read(&output).unwrap();
    let metadata = fs::read(&head).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&metadata).unwrap();
    assert_eq!(value["purpose"], "assistant-sealed-cutover-head");
    assert_eq!(value["closure_kind"], "sealed_installation_cutover_v1");
    assert!(value.get("latest").is_none());
    assert!(run(&data, &args).status.success());
    assert_eq!(fs::read(&output).unwrap(), cipher);
    assert_eq!(fs::read(&head).unwrap(), metadata);
    let password = kit.join("password");
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&password)
        .unwrap();
    file.write_all(b"New-synthetic-password-2026\n").unwrap();
    file.sync_all().unwrap();
    for args in [
        vec!["serve", "--origin", "https://assistant.example.test"],
        vec!["doctor"],
        vec![
            "init",
            "--name",
            "Synthetic",
            "--email",
            "new@example.test",
            "--password-file",
            password.to_str().unwrap(),
        ],
        vec![
            "recover-account",
            "--account-id",
            &account,
            "--role",
            "admin",
            "--password-file",
            password.to_str().unwrap(),
        ],
        vec![
            "backup",
            "setup",
            "--recipient",
            &recipient,
            "--identity-file",
            identity.to_str().unwrap(),
        ],
        vec![
            "backup",
            "create",
            "--output",
            outputs.join("ordinary.age").to_str().unwrap(),
        ],
    ] {
        let out = run(&data, &args);
        assert!(!out.status.success(), "{args:?}");
    }
    // Installation-independent keygen still works after retirement.
    assert!(
        run(
            &data,
            &[
                "backup",
                "keygen",
                "--output",
                kit.join("new-identity").to_str().unwrap()
            ]
        )
        .status
        .success()
    );
    assert_eq!(fs::read(&output).unwrap(), cipher);
    assert_eq!(fs::read(&head).unwrap(), metadata);
}
