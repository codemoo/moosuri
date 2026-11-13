//! Synthetic ciphertext outcomes; no real credentials or provider calls.
use age::secrecy::ExposeSecret;
use moosuri_vault_core::{
    ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
};
use moosuri_vault_service::{Error, database::Database, protocol::*, recovery};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};
use uuid::Uuid;
fn owner() -> Owner {
    Owner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        installation: Uuid::new_v4(),
        epoch: 1,
    }
}
fn empty_inventory() -> String {
    moosuri_vault_core::inventory_digest(&[])
        .unwrap()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn setup(db: &mut Database, o: &Owner) -> moosuri_vault_core::ActiveVault {
    db.create_session("synthetic-cookie", o).unwrap();
    let binding = Binding {
        owner_id: o.account,
        tenant_id: o.tenant,
        vault_id: Uuid::new_v4(),
    };
    let password = SecretText::new("Synthetic vault password 2026".into()).unwrap();
    let mut p = PendingVault::prepare(binding, &password, &mut OsEntropy).unwrap();
    let recovery = RecoveryKey::parse(&p.recovery().expose_encoded()).unwrap();
    p.verify_recovery(&recovery).unwrap();
    let (id, c) = p.candidate().unwrap();
    let c: String = c.iter().map(|b| format!("{b:02x}")).collect();
    let op = Operation::Key {
        id,
        expected_header: None,
        expected_inventory: empty_inventory(),
        header: p.header().clone(),
        items: vec![],
        commitment: c.clone(),
    };
    let r = db.prepare(o, op.clone()).unwrap();
    assert_eq!(r.state, "pending");
    assert!(db.state(o).unwrap().header.is_none());
    assert_eq!(db.prepare(o, op).unwrap().payload_sha256, r.payload_sha256);
    let r = db.commit(o, id, &r.payload_sha256).unwrap();
    assert_eq!(r.state, "committed");
    assert_eq!(r.commitment.as_deref(), Some(c.as_str()));
    p.confirm_commit(StorageReceipt::from_verified_storage(
        id,
        moosuri_vault_service::unhex(&c).unwrap(),
    ))
    .unwrap();
    p.activate().unwrap()
}
fn fields() -> ItemFields {
    serde_json::from_value(serde_json::json!({"kind":"login","title":"SYNTHETIC PRIVATE TITLE CANARY","username":"SYNTHETIC USER CANARY","url":"https://example.test","secret":"SYNTHETIC PRIVATE FIELD CANARY","notes":"SYNTHETIC PRIVATE NOTE CANARY","favorite":true,"expires_at":"","totp":""})).unwrap()
}
struct Kit {
    identity: PathBuf,
    archive: PathBuf,
    head: PathBuf,
    recipient: String,
}
fn kit(parent: &Path) -> Kit {
    fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
    let identity = age::x25519::Identity::generate();
    let file = parent.join("identity");
    fs::write(&file, identity.to_string().expose_secret()).unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    Kit {
        identity: file,
        archive: parent.join("checkpoint.age"),
        head: parent.join("trusted-head.json"),
        recipient: identity.to_public().to_string(),
    }
}
fn capture(root: &Path, k: &Kit) -> Result<recovery::TrustedHead, Error> {
    recovery::checkpoint(root, &k.recipient, &k.archive, &k.head, true)
}
fn restore(root: &Path, k: &Kit) -> Result<(), Error> {
    recovery::restore(root, &k.archive, &k.head, &k.identity)
}
fn put(db: &mut Database, o: &Owner, item: EncryptedItem) -> Operation {
    Operation::Put {
        id: Uuid::new_v4(),
        expected_header: db.state(o).unwrap().header_sha256,
        expected_revision: 0,
        item,
    }
}
#[test]
fn exact_restore_quarantines_pending_sessions_preserves_receipts_tombstones_and_reservations() {
    let tmp = tempfile::tempdir().unwrap();
    let k = kit(tmp.path());
    let source = tmp.path().join("source");
    let destination = tmp.path().join("destination");
    let mut db = Database::open(&source).unwrap();
    let o = owner();
    let mut active = setup(&mut db, &o);
    let item = active
        .seal_item(Uuid::new_v4(), 1, &fields(), &mut OsEntropy)
        .unwrap();
    let op = put(&mut db, &o, item.clone());
    let r = db.prepare(&o, op.clone()).unwrap();
    db.commit(&o, op.id(), &r.payload_sha256).unwrap();
    let delete = Operation::Delete {
        id: Uuid::new_v4(),
        expected_header: db.state(&o).unwrap().header_sha256,
        item: item.id,
        expected_revision: 1,
    };
    let r = db.prepare(&o, delete.clone()).unwrap();
    db.commit(&o, delete.id(), &r.payload_sha256).unwrap();
    let pending_item = active
        .seal_item(Uuid::new_v4(), 1, &fields(), &mut OsEntropy)
        .unwrap();
    let pending = put(&mut db, &o, pending_item.clone());
    let pending_receipt = db.prepare(&o, pending.clone()).unwrap();
    let cancelled_item = active
        .seal_item(Uuid::new_v4(), 1, &fields(), &mut OsEntropy)
        .unwrap();
    let cancelled = put(&mut db, &o, cancelled_item.clone());
    let cancelled_receipt = db.cancel(&o, cancelled.id(), cancelled.clone()).unwrap();
    let source_state = serde_json::to_vec(&db.state(&o).unwrap().header).unwrap();
    drop(db);
    let journal_before = fs::read(source.join("vault.journal")).unwrap();
    capture(&source, &k).unwrap();
    assert!(Database::open(&source).is_err());
    assert_eq!(
        fs::read(source.join("vault.journal")).unwrap(),
        journal_before
    );
    let inputs = [
        fs::read(&k.archive).unwrap(),
        fs::read(&k.head).unwrap(),
        fs::read(&k.identity).unwrap(),
    ];
    restore(&destination, &k).unwrap();
    let mut db = Database::open(&destination).unwrap();
    assert!(db.session("synthetic-cookie").is_err());
    assert_eq!(
        serde_json::to_vec(&db.state(&o).unwrap().header).unwrap(),
        source_state
    );
    assert!(db.state(&o).unwrap().items.is_empty());
    assert!(db.state(&o).unwrap().pending.is_empty());
    assert_eq!(db.prepare(&o, pending.clone()).unwrap().state, "aborted");
    assert_eq!(
        db.commit(&o, pending.id(), &pending_receipt.payload_sha256)
            .unwrap()
            .state,
        "aborted"
    );
    assert_eq!(
        serde_json::to_vec(&db.operation(&o, cancelled.id()).unwrap().1).unwrap(),
        serde_json::to_vec(&cancelled_receipt).unwrap()
    );
    let revive = put(&mut db, &o, item);
    assert!(db.prepare(&o, revive).is_err());
    let mut changed = pending_item;
    changed.wrapped_key = "A".repeat(changed.wrapped_key.len());
    let changed = put(&mut db, &o, changed);
    assert!(db.prepare(&o, changed).is_err());
    db.create_session("fresh-cookie", &o).unwrap();
    drop(db);
    restore(&destination, &k).unwrap();
    assert!(
        Database::open(&destination)
            .unwrap()
            .session("fresh-cookie")
            .is_ok()
    );
    let another = tmp.path().join("another");
    restore(&another, &k).unwrap();
    let id = |root: &Path| {
        let c = rusqlite::Connection::open(root.join("vault.sqlite")).unwrap();
        c.query_row("SELECT installation FROM meta", [], |r| {
            r.get::<_, String>(0)
        })
        .unwrap()
    };
    assert_ne!(id(&destination), id(&another));
    assert_ne!(id(&destination), id(&source));
    for (path, bytes) in [&k.archive, &k.head, &k.identity].into_iter().zip(inputs) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
#[test]
fn active_lock_permissions_links_and_occupied_outputs_fail_without_sealing_or_overwrite() {
    let tmp = tempfile::tempdir().unwrap();
    let k = kit(tmp.path());
    let source = tmp.path().join("source");
    let db = Database::open(&source).unwrap();
    assert!(capture(&source, &k).is_err());
    assert!(!source.join("vault.sealed").exists());
    drop(db);
    fs::write(&k.archive, b"keep-output").unwrap();
    assert!(capture(&source, &k).is_err());
    assert_eq!(fs::read(&k.archive).unwrap(), b"keep-output");
    assert!(!source.join("vault.sealed").exists());
    fs::remove_file(&k.archive).unwrap();
    fs::set_permissions(
        source.join("vault.journal"),
        fs::Permissions::from_mode(0o644),
    )
    .unwrap();
    assert!(capture(&source, &k).is_err());
    assert!(!source.join("vault.sealed").exists());
    fs::set_permissions(
        source.join("vault.journal"),
        fs::Permissions::from_mode(0o600),
    )
    .unwrap();
    let linked = tmp.path().join("linked");
    std::os::unix::fs::symlink(&source, &linked).unwrap();
    assert!(capture(&linked, &k).is_err());
    assert!(Database::open(&linked).is_err());
    fs::hard_link(source.join("vault.journal"), tmp.path().join("hardlink")).unwrap();
    assert!(capture(&source, &k).is_err());
    fs::remove_file(tmp.path().join("hardlink")).unwrap();
    capture(&source, &k).unwrap();
    let destination = tmp.path().join("destination");
    fs::create_dir(&destination).unwrap();
    fs::set_permissions(&destination, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(restore(&destination, &k).is_err());
    assert!(fs::read_dir(&destination).unwrap().next().is_none());
    fs::remove_dir(&destination).unwrap();
    fs::set_permissions(&k.identity, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(restore(&destination, &k).is_err());
    assert!(!destination.exists());
}
#[test]
fn missing_truncated_ahead_and_semantically_modified_evidence_remain_fail_closed() {
    for mode in [
        "missing",
        "truncated",
        "ahead",
        "nonce",
        "receipt",
        "identity",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let k = kit(tmp.path());
        let source = tmp.path().join("source");
        let mut db = Database::open(&source).unwrap();
        let o = owner();
        let _ = setup(&mut db, &o);
        drop(db);
        let journal = source.join("vault.journal");
        match mode {
            "missing" => fs::remove_file(&journal).unwrap(),
            "truncated" => {
                let f = fs::OpenOptions::new().write(true).open(&journal).unwrap();
                f.set_len(f.metadata().unwrap().len() - 1).unwrap();
            }
            "ahead" => {
                use std::io::Write;
                fs::OpenOptions::new()
                    .append(true)
                    .open(&journal)
                    .unwrap()
                    .write_all(b"ahead")
                    .unwrap();
            }
            _ => {
                let c = rusqlite::Connection::open(source.join("vault.sqlite")).unwrap();
                c.execute_batch(match mode {
                    "nonce" => "DELETE FROM nonce_ledger",
                    "receipt" => "UPDATE operations SET receipt=x'7b7d'",
                    _ => "UPDATE meta SET installation='00000000-0000-0000-0000-000000000000'",
                })
                .unwrap();
            }
        }
        let before = fs::read(&journal).ok();
        assert!(capture(&source, &k).is_err(), "{mode}");
        assert_eq!(fs::read(&journal).ok(), before);
        assert_eq!(
            source.join("vault.sealed").exists(),
            matches!(mode, "nonce" | "receipt")
        );
        assert!(!k.archive.exists());
    }
}
#[test]
fn corruption_truncation_wrong_identity_digest_and_private_archive_limits_never_publish() {
    for mode in [
        "corrupt", "truncate", "digest", "identity", "head", "limit", "symlink",
    ] {
        let tmp = tempfile::tempdir().unwrap();
        let k = kit(tmp.path());
        let source = tmp.path().join("source");
        drop(Database::open(&source).unwrap());
        capture(&source, &k).unwrap();
        match mode {
            "corrupt" => {
                let mut b = fs::read(&k.archive).unwrap();
                let n = b.len() - 1;
                b[n] ^= 1;
                fs::write(&k.archive, b).unwrap();
            }
            "truncate" => {
                let f = fs::OpenOptions::new().write(true).open(&k.archive).unwrap();
                f.set_len(f.metadata().unwrap().len() - 1).unwrap();
            }
            "digest" | "head" => {
                let mut h: serde_json::Value =
                    serde_json::from_slice(&fs::read(&k.head).unwrap()).unwrap();
                if mode == "digest" {
                    h["ciphertext_sha256"] = "0".repeat(64).into();
                } else {
                    h["seal"]["source"] = Uuid::new_v4().to_string().into();
                }
                fs::write(&k.head, serde_json::to_vec(&h).unwrap()).unwrap();
            }
            "identity" => {
                let i = age::x25519::Identity::generate();
                fs::write(&k.identity, i.to_string().expose_secret()).unwrap();
            }
            "limit" => {
                fs::OpenOptions::new()
                    .write(true)
                    .open(&k.archive)
                    .unwrap()
                    .set_len(2 * 1024 * 1024 * 1024)
                    .unwrap();
            }
            _ => {
                let real = tmp.path().join("real.age");
                fs::rename(&k.archive, &real).unwrap();
                std::os::unix::fs::symlink(real, &k.archive).unwrap();
            }
        }
        let destination = tmp.path().join("destination");
        assert!(restore(&destination, &k).is_err(), "{mode}");
        assert!(!destination.exists());
        assert!(Database::open(&source).is_err());
    }
}

#[test]
fn nonsticky_writable_ancestors_reject_runtime_source_inputs_and_destinations_without_mutation() {
    for mode in [0o770, 0o777] {
        let tmp = tempfile::tempdir().unwrap();
        let k = kit(tmp.path());
        let shared = tmp.path().join("shared");
        let nested = shared.join("nested");
        fs::create_dir(&shared).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(0o700)).unwrap();
        fs::create_dir(&nested).unwrap();
        fs::set_permissions(&nested, fs::Permissions::from_mode(0o700)).unwrap();
        let source = nested.join("source");
        drop(Database::open(&source).unwrap());
        let database = fs::read(source.join("vault.sqlite")).unwrap();
        let journal = fs::read(source.join("vault.journal")).unwrap();
        let foreign_output = nested.join("output");
        fs::create_dir(&foreign_output).unwrap();
        fs::set_permissions(&foreign_output, fs::Permissions::from_mode(0o700)).unwrap();
        fs::set_permissions(&shared, fs::Permissions::from_mode(mode)).unwrap();
        assert!(Database::open(&source).is_err(), "mode {mode:o}");
        assert!(capture(&source, &k).is_err());
        assert!(!source.join("vault.sealed").exists());
        assert_eq!(fs::read(source.join("vault.sqlite")).unwrap(), database);
        assert_eq!(fs::read(source.join("vault.journal")).unwrap(), journal);
        assert!(!k.archive.exists());
        let safe_source = tmp.path().join("safe-source");
        drop(Database::open(&safe_source).unwrap());
        let out = foreign_output.join("checkpoint.age");
        let head = foreign_output.join("head.json");
        assert!(recovery::checkpoint(&safe_source, &k.recipient, &out, &head, true).is_err());
        assert!(!safe_source.join("vault.sealed").exists());
        assert!(!out.exists());
        assert!(!head.exists());
        capture(&safe_source, &k).unwrap();
        let inputs = [
            fs::read(&k.archive).unwrap(),
            fs::read(&k.head).unwrap(),
            fs::read(&k.identity).unwrap(),
        ];
        let destination = nested.join("destination");
        assert!(restore(&destination, &k).is_err());
        assert!(!destination.exists());
        let relocated = nested.join("input.age");
        fs::write(&relocated, &inputs[0]).unwrap();
        fs::set_permissions(&relocated, fs::Permissions::from_mode(0o600)).unwrap();
        let safe_destination = tmp.path().join("safe-destination");
        assert!(recovery::restore(&safe_destination, &relocated, &k.head, &k.identity).is_err());
        assert!(!safe_destination.exists());
        assert_eq!(fs::read(&relocated).unwrap(), inputs[0]);
        for (path, bytes) in [&k.archive, &k.head, &k.identity].into_iter().zip(inputs) {
            assert_eq!(fs::read(path).unwrap(), bytes);
        }
    }
}

#[test]
fn owned_sticky_ancestors_and_canonical_sticky_temp_support_exact_cutover() {
    let temp_root = if cfg!(target_os = "linux") {
        Path::new("/tmp")
    } else {
        Path::new("/private/tmp")
    };
    let tmp = tempfile::Builder::new()
        .prefix("vault-sticky-recovery-")
        .tempdir_in(temp_root)
        .unwrap();
    let k = kit(tmp.path());
    let sticky = tmp.path().join("sticky");
    fs::create_dir(&sticky).unwrap();
    fs::set_permissions(&sticky, fs::Permissions::from_mode(0o1777)).unwrap();
    let private = sticky.join("private");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    let source = private.join("source");
    drop(Database::open(&source).unwrap());
    capture(&source, &k).unwrap();
    let destination = private.join("destination");
    restore(&destination, &k).unwrap();
    drop(Database::open(&destination).unwrap());
    assert!(Database::open(&source).is_err());
}

#[test]
fn reserved_stage_destination_is_rejected_before_publication_and_preserves_inputs() {
    let tmp = tempfile::tempdir().unwrap();
    let k = kit(tmp.path());
    let source = tmp.path().join("source");
    drop(Database::open(&source).unwrap());
    capture(&source, &k).unwrap();
    let inputs = [
        fs::read(&k.archive).unwrap(),
        fs::read(&k.head).unwrap(),
        fs::read(&k.identity).unwrap(),
    ];
    let destination = tmp.path().join(".vault-recovery-stage-user-selected");
    assert_eq!(restore(&destination, &k), Err(Error::Invalid));
    assert!(!destination.exists());
    assert!(Database::open(&destination).is_err());
    assert!(!destination.exists());
    assert!(fs::read_dir(tmp.path()).unwrap().all(|entry| {
        !entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".vault-recovery-stage-")
    }));
    for (path, bytes) in [&k.archive, &k.head, &k.identity].into_iter().zip(inputs) {
        assert_eq!(fs::read(path).unwrap(), bytes);
    }
}
