use moosuri_store::{Store, backup};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::Path,
    process::Command,
};
const BIN: &str = env!("CARGO_BIN_EXE_moosuri");
fn mode(path: &Path) {
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
#[tokio::test]
async fn actual_private_import_cli_new_root_report_unknown_work_and_no_overwrite() {
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let input = temp.path().join("export.json");
    let password = temp.path().join("password");
    let data = temp.path().join("data");
    let report = temp.path().join("report");
    let bundle = serde_json::json!({"format":1,"purpose":"moosuri-private-transfer-v1","bundle_id":"00000000-0000-4000-a000-000000000001","source_manifest":{"deployment_id":"synthetic-private","source_version":"synthetic-1","source_schema":"explicit-events-v1","exported_at":1,"coverage":"selected manual memories only"},"events":[{"sequence":1,"owner":"chosen","id":"m1","revision":1,"operation":{"kind":"memory_upsert","subject":"CLI proof","text":"Synthetic CLI retained memory","speaker":null,"valid_from":null,"valid_until":null,"evidence":[]}},{"sequence":2,"owner":"chosen","id":"old-run","revision":1,"operation":{"kind":"run","state":"pending","secret":"synthetic-not-imported"}}]});
    fs::write(&input, serde_json::to_vec(&bundle).unwrap()).unwrap();
    mode(&input);
    fs::write(&password, "Synthetic-private-cli-password\n").unwrap();
    mode(&password);
    let args = [
        "--data-dir",
        data.to_str().unwrap(),
        "import-private",
        "--bundle",
        input.to_str().unwrap(),
        "--legacy-owner",
        "chosen",
        "--name",
        "New owner",
        "--email",
        "new@example.test",
        "--password-file",
        password.to_str().unwrap(),
        "--report-dir",
        report.to_str().unwrap(),
    ];
    let output = Command::new(BIN).args(args).output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("1 quarantined"));
    assert!(!stdout.contains("synthetic-not-imported"));
    let store = Store::open(&data).unwrap();
    let user = store
        .login(
            "new@example.test".into(),
            "Synthetic-private-cli-password".into(),
        )
        .await
        .unwrap()
        .user;
    assert_eq!(
        store.memories(&user, None).await.unwrap().items[0]
            .draft
            .text,
        "Synthetic CLI retained memory"
    );
    drop(store);
    let before = fs::read(report.join("report.json")).unwrap();
    let retry = Command::new(BIN).args(args).output().unwrap();
    assert!(!retry.status.success());
    assert_eq!(before, fs::read(report.join("report.json")).unwrap());
    let keydir = temp.path().join("keys");
    let exports = temp.path().join("exports");
    for d in [&keydir, &exports] {
        fs::DirBuilder::new().mode(0o700).create(d).unwrap();
    }
    let identity = keydir.join("key");
    let public = backup::keygen(&identity).unwrap();
    backup::setup(&data, &public, &identity).unwrap();
    backup::create(&data, &exports.join("import.age")).unwrap();
}
