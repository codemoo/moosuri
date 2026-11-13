use super::*;
use crate::Store;
use crate::tests::{TEST_LOCK, admin};
use age::secrecy::ExposeSecret;
use std::collections::BTreeMap;
use std::os::unix::fs::{PermissionsExt, symlink};

// tempfile defaults follow the process umask. Recovery-kit/export fixtures must
// meet the same explicit private-directory contract on Linux and macOS.
fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("moosuri-backup-")
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}

fn key(parent: &Path, name: &str) -> (age::x25519::Identity, PathBuf) {
    let identity = age::x25519::Identity::generate();
    let path = parent.join(name);
    let mut file = files::create_file(&path).unwrap();
    file.write_all(identity.to_string().expose_secret().as_bytes())
        .unwrap();
    file.sync_all().unwrap();
    (identity, path)
}
fn unpack(archive: &Path, identity: &age::x25519::Identity) -> BTreeMap<String, Vec<u8>> {
    let file = File::open(archive).unwrap();
    let decryptor = age::Decryptor::new(file).unwrap();
    let mut reader = decryptor
        .decrypt(std::iter::once(identity as &dyn age::Identity))
        .unwrap();
    let mut plain = Vec::new();
    reader.read_to_end(&mut plain).unwrap();
    let mut result = BTreeMap::new();
    for item in tar::Archive::new(plain.as_slice()).entries().unwrap() {
        let mut item = item.unwrap();
        assert!(item.header().entry_type().is_file());
        let name = item.path().unwrap().to_string_lossy().to_string();
        let mut bytes = Vec::new();
        item.read_to_end(&mut bytes).unwrap();
        assert!(result.insert(name, bytes).is_none());
    }
    result
}
fn snapshot(root: &Path) -> BTreeMap<String, String> {
    fs::read_dir(root)
        .unwrap()
        .map(|entry| {
            let path = entry.unwrap().path();
            (
                path.file_name().unwrap().to_string_lossy().to_string(),
                hex(&Sha256::digest(fs::read(path).unwrap())),
            )
        })
        .collect()
}
async fn installed(root: &Path) -> String {
    let store = Store::open(root).unwrap();
    let user = admin(&store).await;
    let id = user.user.account().id.clone();
    drop(store);
    id
}
#[tokio::test]
async fn encrypted_snapshot_manifest_epochs_and_provider_key_independence() {
    let _guard = TEST_LOCK.lock().await;
    let data = directory();
    let kit = directory();
    let exports = directory();
    installed(data.path()).await;
    let (a, apath) = key(kit.path(), "identity-a");
    let (b, bpath) = key(kit.path(), "identity-b");
    let initial = snapshot(data.path());
    assert!(matches!(
        create(data.path(), &exports.path().join("unconfigured.age")),
        Err(Error::Unconfigured)
    ));
    assert_eq!(snapshot(data.path()), initial);
    assert_eq!(
        setup(data.path(), &a.to_public().to_string(), &bpath).err(),
        Some(Error::DecryptionFailed)
    );
    assert_eq!(snapshot(data.path()), initial);
    let c1 = setup(data.path(), &a.to_public().to_string(), &apath).unwrap();
    assert_eq!(c1.active_epoch, Some(1));
    assert_eq!(
        setup(data.path(), &a.to_public().to_string(), &apath)
            .unwrap()
            .active_epoch,
        Some(1)
    );
    // Export never requires, includes, decrypts or silently recreates this key.
    fs::remove_file(data.path().join("provider-secrets.key")).unwrap();
    let before = snapshot(data.path());
    let output_a = exports.path().join("epoch-a.age");
    create(data.path(), &output_a).unwrap();
    assert_eq!(snapshot(data.path()), before);
    let entries = unpack(&output_a, &a);
    assert_eq!(
        entries.keys().cloned().collect::<Vec<_>>(),
        vec![
            "assistant.sqlite",
            "auth-epochs.journal",
            "knowledge-invalidations.journal",
            "manifest.json"
        ]
    );
    let manifest: Manifest = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(manifest.installation_id, c1.installation_id);
    assert_eq!(manifest.key_epoch, 1);
    assert_eq!(manifest.purpose, "assistant-export");
    assert!(manifest.authoritative_files.is_empty());
    for entry in &manifest.entries {
        assert_eq!(entries[&entry.name].len() as u64, entry.size);
        assert_eq!(hex(&Sha256::digest(&entries[&entry.name])), entry.sha256);
    }
    let c2 = setup(data.path(), &b.to_public().to_string(), &bpath).unwrap();
    assert_eq!(c2.installation_id, c1.installation_id);
    assert_eq!(c2.active_epoch, Some(2));
    assert_eq!(c2.retained_epochs.len(), 2);
    let output_b = exports.path().join("epoch-b.age");
    create(data.path(), &output_b).unwrap();
    assert_eq!(
        serde_json::from_slice::<Manifest>(&unpack(&output_b, &b)["manifest.json"])
            .unwrap()
            .key_epoch,
        2
    );
    assert!(age::decrypt(&b, &fs::read(output_a).unwrap()).is_err());
    assert!(age::decrypt(&a, &fs::read(output_b).unwrap()).is_err());
    let db_bytes = &entries["assistant.sqlite"];
    assert!(!db_bytes.windows(15).any(|w| w == b"AGE-SECRET-KEY-"));
    assert_eq!(
        fs::metadata(exports.path().join("epoch-a.age"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
}
#[tokio::test]
async fn rejection_does_not_initialize_mutate_follow_links_or_overwrite_output() {
    let _guard = TEST_LOCK.lock().await;
    let data = directory();
    let kit = directory();
    let exports = directory();
    let (key, identity) = key(kit.path(), "identity");
    let output = exports.path().join("archive.age");
    fs::write(data.path().join("assistant.sqlite"), []).unwrap();
    fs::set_permissions(
        data.path().join("assistant.sqlite"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let before = snapshot(data.path());
    assert!(setup(data.path(), &key.to_public().to_string(), &identity).is_err());
    assert!(create(data.path(), &output).is_err());
    assert_eq!(snapshot(data.path()), before);
    assert!(!output.exists());
    fs::remove_file(data.path().join("assistant.sqlite")).unwrap();
    installed(data.path()).await;
    setup(data.path(), &key.to_public().to_string(), &identity).unwrap();
    fs::write(&output, b"retained output").unwrap();
    let before = snapshot(data.path());
    assert!(matches!(
        create(data.path(), &output),
        Err(Error::OutputExists)
    ));
    assert_eq!(fs::read(&output).unwrap(), b"retained output");
    assert_eq!(snapshot(data.path()), before);
    let link = exports.path().join("link.age");
    symlink(&output, &link).unwrap();
    assert!(matches!(
        create(data.path(), &link),
        Err(Error::OutputExists)
    ));
    assert!(create(data.path(), &data.path().join("nested.age")).is_err());
    let inside = data.path().join("identity");
    fs::copy(&identity, &inside).unwrap();
    assert_eq!(
        setup(data.path(), &key.to_public().to_string(), &inside).err(),
        Some(Error::InvalidInput)
    );
    fs::remove_file(inside).unwrap();
    fs::set_permissions(&identity, fs::Permissions::from_mode(0o640)).unwrap();
    assert!(setup(data.path(), &key.to_public().to_string(), &identity).is_err());
    fs::set_permissions(&identity, fs::Permissions::from_mode(0o600)).unwrap();
    let hard = kit.path().join("hard");
    fs::hard_link(&identity, &hard).unwrap();
    assert!(setup(data.path(), &key.to_public().to_string(), &identity).is_err());
    fs::remove_file(hard).unwrap();
    let linked = kit.path().join("linked");
    symlink(&identity, &linked).unwrap();
    assert!(setup(data.path(), &key.to_public().to_string(), &linked).is_err());
    assert!(!fs::read_dir(exports.path()).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .ends_with("partial")
    }));
}
#[tokio::test]
async fn damaged_public_marker_or_journal_never_resets_configuration() {
    let _guard = TEST_LOCK.lock().await;
    for damage in 0..3 {
        let data = directory();
        let kit = directory();
        let exports = directory();
        installed(data.path()).await;
        let (key, identity) = key(kit.path(), "identity");
        setup(data.path(), &key.to_public().to_string(), &identity).unwrap();
        if damage < 2 {
            let db = Connection::open(data.path().join("assistant.sqlite")).unwrap();
            db.execute_batch("PRAGMA foreign_keys=OFF;").unwrap();
            db.execute(
                if damage == 0 {
                    "DELETE FROM backup_installation"
                } else {
                    "DELETE FROM backup_key_epochs"
                },
                [],
            )
            .unwrap();
        } else {
            fs::write(data.path().join("auth-epochs.journal"), b"truncated").unwrap();
        }
        let before = snapshot(data.path());
        assert!(setup(data.path(), &key.to_public().to_string(), &identity).is_err());
        assert!(create(data.path(), &exports.path().join("broken.age")).is_err());
        assert_eq!(snapshot(data.path()), before);
        assert_eq!(fs::read_dir(exports.path()).unwrap().count(), 0);
    }
}
#[tokio::test]
async fn committed_wal_and_distinct_process_writer_lock_are_preserved() {
    let _guard = TEST_LOCK.lock().await;
    if let Some(path) = std::env::var_os("MOOSURI_TEST_EXPORT_CHILD") {
        let data = PathBuf::from(path);
        let store = Store::open(&data).unwrap();
        admin(&store).await;
        std::process::exit(0);
    }
    let data = directory();
    let kit = directory();
    let exports = directory();
    let child = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "backup::tests::committed_wal_and_distinct_process_writer_lock_are_preserved",
            "--nocapture",
        ])
        .env("MOOSURI_TEST_EXPORT_CHILD", data.path())
        .output()
        .unwrap();
    assert!(child.status.success());
    assert_eq!(
        u32::from_be_bytes(
            fs::read(data.path().join("assistant.sqlite")).unwrap()[60..64]
                .try_into()
                .unwrap()
        ),
        0
    );
    let (key, identity) = key(kit.path(), "identity");
    setup(data.path(), &key.to_public().to_string(), &identity).unwrap();
    let output = exports.path().join("wal.age");
    create(data.path(), &output).unwrap();
    let entries = unpack(&output, &key);
    let snapshot = exports.path().join("inspected.sqlite");
    fs::write(&snapshot, &entries["assistant.sqlite"]).unwrap();
    let db = Connection::open(snapshot).unwrap();
    let count: i64 = db
        .query_row("SELECT count(*) FROM accounts", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count, 1);
    let _running = Store::open(data.path()).unwrap();
    assert_eq!(
        setup(data.path(), &key.to_public().to_string(), &identity).err(),
        Some(Error::Busy)
    );
    assert!(matches!(
        create(data.path(), &exports.path().join("busy.age")),
        Err(Error::Busy)
    ));
}
#[tokio::test]
async fn source_size_caps_and_stream_failure_never_publish_a_complete_archive() {
    let _guard = TEST_LOCK.lock().await;
    let data = directory();
    let kit = directory();
    let exports = directory();
    installed(data.path()).await;
    let (key, identity) = key(kit.path(), "identity");
    setup(data.path(), &key.to_public().to_string(), &identity).unwrap();
    let offline = Offline::open(data.path()).unwrap();
    let manifest = Manifest {
        format: 1,
        purpose: "assistant-export".into(),
        installation_id: configuration(&offline.conn).unwrap().installation_id,
        export_id: Uuid::new_v4().to_string(),
        schema: CURRENT_SCHEMA_VERSION,
        created_at: 0,
        key_epoch: 1,
        recipient: key.to_public().to_string(),
        entries: vec![],
        auth_journal: journal_heads(&offline.conn, &offline.auth, &offline.knowledge)
            .unwrap()
            .0,
        knowledge_journal: journal_heads(&offline.conn, &offline.auth, &offline.knowledge)
            .unwrap()
            .1,
        authoritative_files: vec![],
        limitations: vec![],
    };
    struct FailedWriter;
    impl Write for FailedWriter {
        fn write(&mut self, _: &[u8]) -> io::Result<usize> {
            Err(io::Error::other("synthetic full disk"))
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(
        write_archive(
            FailedWriter,
            &key.to_public(),
            &manifest,
            b"{}",
            &offline.temp.0.join("unused"),
            &offline.temp.0,
            Instant::now()
        ),
        Err(Error::Unavailable)
    ));
    drop(offline);
    let journal = OpenOptions::new()
        .write(true)
        .open(data.path().join("auth-epochs.journal"))
        .unwrap();
    journal.set_len(JOURNAL_LIMIT + 1).unwrap();
    assert!(matches!(
        create(data.path(), &exports.path().join("oversize.age")),
        Err(Error::LimitExceeded)
    ));
    assert_eq!(fs::read_dir(exports.path()).unwrap().count(), 0);
    assert_eq!(
        check_deadline(Instant::now() - DEADLINE - Duration::from_secs(1)),
        Err(Error::Busy)
    );
}
#[tokio::test]
#[ignore = "requires an explicitly selected reference Go age binary; synthetic interoperability only"]
async fn reference_go_age_reads_export_and_rust_reads_reference_ciphertext() {
    let _guard = TEST_LOCK.lock().await;
    let binary =
        std::env::var_os("MOOSURI_REFERENCE_AGE").expect("set path to reference Go age binary");
    let data = directory();
    let kit = directory();
    let exports = directory();
    installed(data.path()).await;
    let (key, identity) = key(kit.path(), "identity");
    setup(data.path(), &key.to_public().to_string(), &identity).unwrap();
    let output = exports.path().join("export.age");
    create(data.path(), &output).unwrap();
    let decoded = std::process::Command::new(&binary)
        .args(["--decrypt", "--identity"])
        .arg(&identity)
        .arg(&output)
        .output()
        .unwrap();
    assert!(decoded.status.success(), "reference decryption failed");
    let names = tar::Archive::new(decoded.stdout.as_slice())
        .entries()
        .unwrap()
        .map(|e| e.unwrap().path().unwrap().to_string_lossy().to_string())
        .collect::<Vec<_>>();
    assert_eq!(names.len(), 4);
    let plain = exports.path().join("synthetic-input");
    fs::write(&plain, CHALLENGE).unwrap();
    let encoded = std::process::Command::new(&binary)
        .args(["--encrypt", "--recipient", &key.to_public().to_string()])
        .arg(&plain)
        .output()
        .unwrap();
    assert!(encoded.status.success());
    assert_eq!(age::decrypt(&key, &encoded.stdout).unwrap(), CHALLENGE);
    let mut corrupt = fs::read(output).unwrap();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(age::decrypt(&key, &corrupt).is_err());
}

#[tokio::test]
async fn export_keeps_pending_journal_intent_without_replaying_or_mutating_source() {
    let _guard = TEST_LOCK.lock().await;
    let data = directory();
    let kit = directory();
    let exports = directory();
    let account = installed(data.path()).await;
    let (key, identity) = key(kit.path(), "identity");
    setup(data.path(), &key.to_public().to_string(), &identity).unwrap();
    let store = Store::open(data.path()).unwrap();
    store
        .call(move |db| {
            db.journal.append(&account, 1)?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let before = snapshot(data.path());
    let output = exports.path().join("pending.age");
    create(data.path(), &output).unwrap();
    assert_eq!(snapshot(data.path()), before);
    let entries = unpack(&output, &key);
    let manifest: Manifest = serde_json::from_slice(&entries["manifest.json"]).unwrap();
    assert_eq!(manifest.auth_journal.sequence, 1);
    assert_eq!(manifest.auth_journal.snapshot_sequence, 0);
    assert_eq!(entries["auth-epochs.journal"].len(), 104);
    let dbpath = exports.path().join("snapshot.sqlite");
    fs::write(&dbpath, &entries["assistant.sqlite"]).unwrap();
    let db = Connection::open(dbpath).unwrap();
    let locked: bool = db
        .query_row("SELECT recovery_required FROM accounts", [], |r| r.get(0))
        .unwrap();
    assert!(!locked, "offline export must not run auth replay");
}

#[tokio::test]
async fn unsupported_schema_uninitialized_account_and_missing_journal_are_unchanged() {
    let _guard = TEST_LOCK.lock().await;
    let kit = directory();
    let exports = directory();
    let (key, identity) = key(kit.path(), "identity");
    for fixture in 0..3 {
        let data = directory();
        if fixture == 0 {
            let empty = Store::open(data.path()).unwrap();
            drop(empty);
        } else {
            installed(data.path()).await;
        }
        if fixture == 1 {
            let conn = Connection::open(data.path().join("assistant.sqlite")).unwrap();
            conn.pragma_update(None, "user_version", CURRENT_SCHEMA_VERSION - 1)
                .unwrap();
        }
        if fixture == 2 {
            fs::remove_file(data.path().join("knowledge-invalidations.journal")).unwrap();
        }
        let before = snapshot(data.path());
        assert!(setup(data.path(), &key.to_public().to_string(), &identity).is_err());
        assert!(
            create(
                data.path(),
                &exports.path().join(format!("unsupported-{fixture}.age"))
            )
            .is_err()
        );
        assert_eq!(snapshot(data.path()), before);
    }
    assert_eq!(fs::read_dir(exports.path()).unwrap().count(), 0);
}
