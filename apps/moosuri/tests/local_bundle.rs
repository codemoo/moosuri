//! Synthetic native bundle lifecycle. No service manager, network provisioning or live provider.
use ring::signature::{Ed25519KeyPair, KeyPair};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};
struct ChildGuard(std::process::Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn native(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .args(args)
        .output()
        .unwrap()
}
fn success(o: std::process::Output) -> Vec<u8> {
    assert!(
        o.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&o.stdout),
        String::from_utf8_lossy(&o.stderr)
    );
    o.stdout
}
fn private(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir(&p).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    p
}
fn signed_bundle(parent: &Path, name: &str, seq: u64, key: &Ed25519KeyPair) -> PathBuf {
    let b = private(parent, name);
    let binary = b.join("moosuri");
    fs::copy(env!("CARGO_BIN_EXE_moosuri"), &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    success(native(&[
        "installation",
        "manifest",
        "--binary",
        binary.to_str().unwrap(),
        "--sequence",
        &seq.to_string(),
        "--output",
        b.join("manifest.json").to_str().unwrap(),
    ]));
    fs::write(
        b.join("manifest.ed25519"),
        key.sign(&fs::read(b.join("manifest.json")).unwrap())
            .as_ref(),
    )
    .unwrap();
    b
}
fn digest(path: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(path).unwrap()).to_vec()
}
#[tokio::test]
async fn installation_signed_fresh_init_served_shell_stopped_update_recovery_and_data_preserving_uninstall()
 {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[34; 32]).unwrap();
    let pin = root.path().join("public-pin");
    let hex: String = key
        .public_key()
        .as_ref()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    fs::write(&pin, hex).unwrap();
    fs::set_permissions(&pin, fs::Permissions::from_mode(0o600)).unwrap();
    let bundle1 = signed_bundle(root.path(), "bundle1", 1, &key);
    let prefix = root.path().join("runtime");
    let data = root.path().join("private-data");
    success(native(&[
        "installation",
        "install",
        "--bundle",
        bundle1.to_str().unwrap(),
        "--trust-key",
        pin.to_str().unwrap(),
        "--prefix",
        prefix.to_str().unwrap(),
        "--data-dir",
        data.to_str().unwrap(),
        "--origin",
        "https://assistant.example.test",
        "--network",
        "https",
    ]));
    assert!(
        !data.exists(),
        "install does not initialize or touch private data"
    );
    let manager = prefix.join("manager");
    let password = root.path().join("chosen-password");
    fs::write(&password, "Synthetic-install-password-2026").unwrap();
    fs::set_permissions(&password, fs::Permissions::from_mode(0o600)).unwrap();
    let initialized = Command::new(&manager)
        .args([
            "installation",
            "run",
            "--prefix",
            prefix.to_str().unwrap(),
            "--",
            "init",
            "--name",
            "Synthetic operator",
            "--email",
            "operator@example.test",
            "--password-file",
            password.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(!String::from_utf8_lossy(&initialized.stdout).contains("Synthetic-install-password"));
    assert!(!String::from_utf8_lossy(&initialized.stderr).contains("Synthetic-install-password"));
    success(initialized);
    assert!(data.join("assistant.sqlite").is_file());
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = reservation.local_addr().unwrap();
    drop(reservation);
    let origin = format!("http://{addr}");
    let mut child = ChildGuard(
        Command::new(&manager)
            .args([
                "installation",
                "run",
                "--prefix",
                prefix.to_str().unwrap(),
                "--",
                "serve",
                "--origin",
                &origin,
                "--bind",
                &addr.to_string(),
                "--dev",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if let Ok(response) = client.get(format!("{origin}/app")).send().await
                && response.status().is_success()
            {
                let html = response.text().await.unwrap();
                assert!(html.contains("<html") || html.contains("<!doctype html"));
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await
        }
    })
    .await
    .unwrap();
    let bundle2 = signed_bundle(root.path(), "bundle2", 2, &key);
    let exports = private(root.path(), "exports");
    let backup = exports.join("before-update.age");
    let update = || {
        native(&[
            "installation",
            "update",
            "--bundle",
            bundle2.to_str().unwrap(),
            "--trust-key",
            pin.to_str().unwrap(),
            "--prefix",
            prefix.to_str().unwrap(),
            "--backup-output",
            backup.to_str().unwrap(),
        ])
    };
    let active = update();
    assert!(!active.status.success());
    assert!(String::from_utf8_lossy(&active.stderr).contains("installation_runtime_busy"));
    assert!(!backup.exists());
    assert_eq!(unsafe { libc::kill(child.0.id() as i32, libc::SIGTERM) }, 0);
    assert!(child.0.wait().unwrap().success());
    let identity = exports.join("identity");
    let recipient = moosuri_store::backup::keygen(&identity).unwrap();
    moosuri_store::backup::setup(&data, &recipient, &identity).unwrap();
    let old: serde_json::Value =
        serde_json::from_slice(&fs::read(prefix.join("current.json")).unwrap()).unwrap();
    success(update());
    assert!(backup.is_file());
    let new: serde_json::Value =
        serde_json::from_slice(&fs::read(prefix.join("current.json")).unwrap()).unwrap();
    assert_eq!(new["sequence"], 2);
    let before = digest(&data.join("assistant.sqlite"));
    // Durable tail after current-state rename but before pending-intent retirement.
    fs::write(prefix.join("pending.json"),serde_json::to_vec(&serde_json::json!({"from":old,"to":new,"backup_output":backup,"export_id":uuid::Uuid::new_v4().to_string()})).unwrap()).unwrap();
    fs::set_permissions(
        prefix.join("pending.json"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let blocked = Command::new(&manager)
        .args([
            "installation",
            "run",
            "--prefix",
            prefix.to_str().unwrap(),
            "--",
            "doctor",
        ])
        .output()
        .unwrap();
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("installation_activation_pending"));
    success(native(&[
        "installation",
        "recover",
        "--prefix",
        prefix.to_str().unwrap(),
        "--choose",
        "old",
    ]));
    assert_eq!(digest(&data.join("assistant.sqlite")), before);
    assert!(!prefix.join("pending.json").exists());
    fs::write(prefix.join("operator-notes.txt"), "unrelated operator note").unwrap();
    let custom = prefix.join("deployment/moosuri.service");
    fs::write(&custom, "operator changed this review file").unwrap();
    success(native(&[
        "installation",
        "uninstall",
        "--prefix",
        prefix.to_str().unwrap(),
    ]));
    assert!(data.join("assistant.sqlite").is_file());
    assert_eq!(digest(&data.join("assistant.sqlite")), before);
    assert_eq!(
        fs::read_to_string(prefix.join("operator-notes.txt")).unwrap(),
        "unrelated operator note"
    );
    assert!(custom.is_file());
    assert!(!manager.exists());
    assert!(!prefix.join("installation.json").exists());
}
