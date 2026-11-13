use moosuri_store::Store;
use std::{fs, os::unix::fs::PermissionsExt, path::Path, process::Command};
fn create(data: &Path, output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args(["backup", "create", "--output"])
        .arg(output)
        .output()
        .unwrap()
}
fn keygen(data: &Path, output: &Path) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args(["backup", "keygen", "--output"])
        .arg(output)
        .output()
        .unwrap()
}
#[test]
fn cli_keygen_is_private_nonoverwriting_and_independent_of_installation() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let data = root.path().join("uninitialized");
    let kit = root.path().join("kit");
    fs::create_dir(&kit).unwrap();
    fs::set_permissions(&kit, fs::Permissions::from_mode(0o700)).unwrap();
    let identity = kit.join("identity");
    let generated = keygen(&data, &identity);
    assert!(generated.status.success());
    assert!(!data.exists());
    let recipient = String::from_utf8(generated.stdout).unwrap();
    assert!(recipient.starts_with("age1"));
    assert_eq!(recipient.lines().count(), 1);
    assert!(generated.stderr.is_empty());
    assert_eq!(
        fs::metadata(&identity).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let before = fs::read(&identity).unwrap();
    assert!(
        before
            .windows(b"AGE-SECRET-KEY-1".len())
            .any(|w| w == b"AGE-SECRET-KEY-1")
    );
    assert!(!recipient.contains("AGE-SECRET"));
    let again = keygen(&data, &identity);
    assert!(!again.status.success());
    assert!(String::from_utf8_lossy(&again.stderr).contains("backup_output_exists"));
    assert!(again.stdout.is_empty());
    assert_eq!(fs::read(&identity).unwrap(), before);
    assert_eq!(fs::read_dir(&kit).unwrap().count(), 1);

    let linked = root.path().join("linked");
    std::os::unix::fs::symlink(&kit, &linked).unwrap();
    assert!(!keygen(&data, &linked.join("new")).status.success());
    fs::set_permissions(&kit, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(!keygen(&data, &kit.join("new")).status.success());
    fs::set_permissions(&kit, fs::Permissions::from_mode(0o700)).unwrap();
    for marker in [
        "Cargo.toml",
        "package.json",
        ".git",
        "assistant.sqlite",
        "writer.lock",
    ] {
        fs::write(kit.join(marker), b"").unwrap();
        assert!(!keygen(&data, &kit.join("new")).status.success());
        assert!(!kit.join("new").exists());
        fs::remove_file(kit.join(marker)).unwrap();
    }
}
#[tokio::test]
async fn cli_export_requires_an_existing_stopped_configured_installation() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let data = root.path().join("data");
    let output = root.path().join("export.age");
    let missing = create(&data, &output);
    assert!(!missing.status.success());
    assert!(!data.exists());
    assert!(!output.exists());
    let store = Store::open(&data).unwrap();
    store
        .bootstrap(
            "Synthetic Admin".into(),
            "admin@example.test".into(),
            "Synthetic-backup-password".into(),
            false,
        )
        .await
        .unwrap();
    let running = create(&data, &output);
    assert!(!running.status.success());
    assert!(String::from_utf8_lossy(&running.stderr).contains("backup_busy"));
    drop(store);
    let unconfigured = create(&data, &output);
    assert!(!unconfigured.status.success());
    // The parent/root itself is deliberately not a valid export directory because
    // it also contains the installation. Admission fails before publication.
    assert!(!output.exists());
    assert!(!String::from_utf8_lossy(&unconfigured.stdout).contains("Synthetic-backup-password"));
}
#[tokio::test]
#[ignore = "requires explicitly selected reference Go age and age-keygen binaries"]
async fn cli_setup_create_and_reference_decrypt_work_without_a_server_key() {
    let age = std::env::var_os("MOOSURI_REFERENCE_AGE").expect("reference age path required");
    let keygen = std::env::var_os("MOOSURI_REFERENCE_AGE_KEYGEN")
        .expect("reference age-keygen path required");
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let data = root.path().join("data");
    let kit = root.path().join("kit");
    let exports = root.path().join("exports");
    for path in [&kit, &exports] {
        fs::create_dir(path).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity = kit.join("identity");
    let generated = self::keygen(&data, &identity);
    assert!(generated.status.success());
    assert!(!data.exists());
    let recipient = Command::new(keygen)
        .arg("-y")
        .arg(&identity)
        .output()
        .unwrap();
    assert!(recipient.status.success());
    assert_eq!(recipient.stdout, generated.stdout);
    let recipient = String::from_utf8(recipient.stdout).unwrap();
    let store = Store::open(&data).unwrap();
    store
        .bootstrap(
            "Synthetic Admin".into(),
            "admin@example.test".into(),
            "Synthetic-backup-password".into(),
            false,
        )
        .await
        .unwrap();
    drop(store);
    let setup = Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(&data)
        .args([
            "backup",
            "setup",
            "--recipient",
            recipient.trim(),
            "--identity-file",
        ])
        .arg(&identity)
        .output()
        .unwrap();
    assert!(setup.status.success());
    fs::remove_file(data.join("provider-secrets.key")).unwrap();
    let output = exports.join("export.age");
    assert!(create(&data, &output).status.success());
    let decrypted = Command::new(age)
        .args(["--decrypt", "--identity"])
        .arg(identity)
        .arg(output)
        .output()
        .unwrap();
    assert!(decrypted.status.success());
    assert!(
        decrypted
            .stdout
            .windows(b"manifest.json".len())
            .any(|w| w == b"manifest.json")
    );
}
