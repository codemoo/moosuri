//! Isolated actual CLI receiver/status/historical restore. Synthetic transcript;
//! no SSH destination, provider, credential, private deployment or real data.
use age::secrecy::ExposeSecret;
use moosuri_vault_service::{database::Database, hash, recovery};
use serde_json::{Value, json};
use std::{
    fs,
    io::Write,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
fn private(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    p
}
fn cli(home: &Path) -> Command {
    let mut c = Command::new(env!("CARGO_BIN_EXE_moosuri-vault-service"));
    c.env("HOME", home);
    c
}
#[test]
fn publisher_actual_receiver_cli_status_pin_restores_without_source_or_receiver_identity() {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let home = private(temp.path(), "receiver-home");
    let artifacts = private(temp.path(), "artifacts");
    let ledger = private(temp.path(), "ledger");
    let kit = private(temp.path(), "offline-kit");
    let key = age::x25519::Identity::generate();
    let recipient = key.to_public().to_string();
    let identity = kit.join("offline-identity");
    fs::write(&identity, key.to_string().expose_secret()).unwrap();
    fs::set_permissions(&identity, fs::Permissions::from_mode(0o600)).unwrap();
    let source = temp.path().join("source");
    let mut db = Database::open(&source).unwrap();
    db.close_session("first").unwrap();
    let snapshot = kit.join("snapshot.age");
    let snapshot_head = kit.join("snapshot.json");
    db.capture_snapshot(&kit)
        .unwrap()
        .encrypt(&recipient, &snapshot, &snapshot_head)
        .unwrap();
    db.close_session("later").unwrap();
    drop(db);
    let journal = kit.join("journal.age");
    let head = kit.join("journal.json");
    recovery::historical::journal_checkpoint(&source, &recipient, &journal, &head).unwrap();
    let binding: Value = serde_json::from_slice(&fs::read(&head).unwrap()).unwrap();
    let journal_bytes = fs::read(source.join("vault.journal")).unwrap();
    let request = json!({"format":1,"recipient":recipient,"baseline_sha256":hash(&fs::read(&snapshot_head).unwrap()),"binding":binding["journal"],"journal_size":journal_bytes.len(),"journal_sha256":hash(&journal_bytes)});
    let meta = serde_json::to_vec(&request).unwrap();
    let mut frame = b"MVJP0001".to_vec();
    frame.extend((meta.len() as u32).to_be_bytes());
    frame.extend(meta);
    frame.extend(journal_bytes);
    let output = cli(&home)
        .args([
            "recovery",
            "target-init",
            "--target-id",
            "synthetic",
            "--artifacts",
        ])
        .arg(&artifacts)
        .arg("--ledger")
        .arg(&ledger)
        .arg("--snapshot")
        .arg(&snapshot)
        .arg("--snapshot-head")
        .arg(&snapshot_head)
        .arg("--recipient")
        .arg(&recipient)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let send = || {
        let mut child = cli(&home)
            .args(["recovery", "receive-journal", "--target-id", "synthetic"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(&frame).unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice::<Value>(&output.stdout).unwrap()
    };
    let first = send();
    assert_eq!(send(), first);
    let output = cli(&home)
        .args(["recovery", "target-status", "--target-id", "synthetic"])
        .output()
        .unwrap();
    assert!(output.status.success());
    let status: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(status["current"], first);
    let remote_journal = Path::new(status["journal"].as_str().unwrap());
    let remote_head = Path::new(status["journal_head"].as_str().unwrap());
    assert_eq!(
        hash(&fs::read(remote_head).unwrap()),
        status["current"]["trusted_head_sha256"].as_str().unwrap()
    );
    // The selected source and source-local archives are no longer available.
    fs::rename(&source, temp.path().join("retired-test-source")).unwrap();
    fs::remove_file(journal).unwrap();
    fs::remove_file(head).unwrap();
    let destination = temp.path().join("restored");
    recovery::historical::restore_historical(
        &destination,
        Path::new(status["snapshot"].as_str().unwrap()),
        Path::new(status["snapshot_head"].as_str().unwrap()),
        &identity,
        &recovery::historical::JournalInputs {
            checkpoint: remote_journal,
            trusted_head: remote_head,
            trusted_head_sha256: status["current"]["trusted_head_sha256"].as_str().unwrap(),
            identity_file: &identity,
        },
    )
    .unwrap();
    Database::open(&destination)
        .unwrap()
        .close_session("fresh")
        .unwrap();
    let generation = remote_journal.parent().unwrap();
    let mut names = fs::read_dir(generation)
        .unwrap()
        .map(|v| v.unwrap().file_name().into_string().unwrap())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, vec!["head.json", "journal.age"]);
    assert!(!home.join("identity").exists());
    assert!(!artifacts.join("vault.journal").exists());
}
