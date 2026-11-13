//! Synthetic ciphertext outcomes; no real credentials or provider calls.
use moosuri_vault_core::{
    ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
};
use moosuri_vault_service::{Error, database::Database, protocol::*};
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
#[test]
fn exact_cas_receipts_restart_and_owner_isolation_preserve_ciphertext_only() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("vault");
    let mut db = Database::open(&path).unwrap();
    let a = owner();
    let b = owner();
    let mut v = setup(&mut db, &a);
    db.create_session("other-cookie", &b).unwrap();
    let id = Uuid::new_v4();
    let item = v.seal_item(id, 1, &fields(), &mut OsEntropy).unwrap();
    let opid = Uuid::new_v4();
    let h = db.state(&a).unwrap().header_sha256;
    let op = Operation::Put {
        id: opid,
        expected_header: h.clone(),
        expected_revision: 0,
        item: item.clone(),
    };
    let receipt = db.prepare(&a, op.clone()).unwrap();
    assert!(db.state(&a).unwrap().items.is_empty());
    assert!(db.operation(&b, opid).is_err());
    let committed = db.commit(&a, opid, &receipt.payload_sha256).unwrap();
    assert_eq!(committed.state, "committed");
    assert_eq!(
        db.prepare(&a, op).unwrap().payload_sha256,
        receipt.payload_sha256
    );
    assert_eq!(db.state(&a).unwrap().items.len(), 1);
    assert!(db.state(&b).unwrap().items.is_empty());
    let stale = v.seal_item(id, 2, &fields(), &mut OsEntropy).unwrap();
    let first = Operation::Put {
        id: Uuid::new_v4(),
        expected_header: h.clone(),
        expected_revision: 1,
        item: stale,
    };
    let r = db.prepare(&a, first).unwrap();
    let second = v.seal_item(id, 2, &fields(), &mut OsEntropy).unwrap();
    let r2 = db
        .prepare(
            &a,
            Operation::Put {
                id: Uuid::new_v4(),
                expected_header: h,
                expected_revision: 1,
                item: second,
            },
        )
        .unwrap();
    assert_eq!(
        db.commit(&a, r.operation, &r.payload_sha256).unwrap().state,
        "committed"
    );
    assert_eq!(
        db.commit(&a, r2.operation, &r2.payload_sha256)
            .unwrap()
            .state,
        "conflict"
    );
    drop(db);
    let db = Database::open(&path).unwrap();
    let s = db.state(&a).unwrap();
    assert_eq!(s.items[0].revision, 2);
    assert_eq!(db.operation(&a, opid).unwrap().1.state, "committed");
    for p in std::fs::read_dir(&path).unwrap() {
        let p = p.unwrap().path();
        if p.is_file() {
            let bytes = std::fs::read(p).unwrap();
            for marker in [
                b"SYNTHETIC PRIVATE TITLE CANARY".as_slice(),
                b"SYNTHETIC PRIVATE FIELD CANARY",
                b"SYNTHETIC USER CANARY",
                b"SYNTHETIC PRIVATE NOTE CANARY",
            ] {
                assert!(!bytes.windows(marker.len()).any(|v| v == marker));
            }
        }
    }
}
#[test]
fn cancellation_retains_nonce_reservation_and_deleted_id_cannot_revive() {
    let tmp = tempfile::tempdir().unwrap();
    let mut db = Database::open(&tmp.path().join("vault")).unwrap();
    let a = owner();
    let mut v = setup(&mut db, &a);
    let h = db.state(&a).unwrap().header_sha256;
    let item = v
        .seal_item(Uuid::new_v4(), 1, &fields(), &mut OsEntropy)
        .unwrap();
    let op = Operation::Put {
        id: Uuid::new_v4(),
        expected_header: h.clone(),
        expected_revision: 0,
        item: item.clone(),
    };
    let r = db.prepare(&a, op.clone()).unwrap();
    assert_eq!(
        db.cancel(&a, r.operation, op.clone()).unwrap().state,
        "aborted"
    );
    assert_eq!(
        db.commit(&a, r.operation, &r.payload_sha256).unwrap().state,
        "aborted"
    );
    assert!(db.state(&a).unwrap().items.is_empty());
    let early_id = Uuid::new_v4();
    let early = Operation::Put {
        id: early_id,
        expected_header: h.clone(),
        expected_revision: 0,
        item: item.clone(),
    };
    assert_eq!(
        db.cancel(&a, early_id, early.clone()).unwrap().state,
        "aborted"
    );
    assert_eq!(db.prepare(&a, early).unwrap().state, "aborted");
    let mut tampered = item.clone();
    tampered.wrapped_key = "A".repeat(tampered.wrapped_key.len());
    assert!(matches!(
        db.prepare(
            &a,
            Operation::Put {
                id: Uuid::new_v4(),
                expected_header: h.clone(),
                expected_revision: 0,
                item: tampered
            }
        ),
        Err(Error::Conflict) | Err(Error::Invalid)
    ));
    let r = db
        .prepare(
            &a,
            Operation::Put {
                id: Uuid::new_v4(),
                expected_header: h.clone(),
                expected_revision: 0,
                item: item.clone(),
            },
        )
        .unwrap();
    db.commit(&a, r.operation, &r.payload_sha256).unwrap();
    let r = db
        .prepare(
            &a,
            Operation::Delete {
                id: Uuid::new_v4(),
                expected_header: h.clone(),
                item: item.id,
                expected_revision: 1,
            },
        )
        .unwrap();
    db.commit(&a, r.operation, &r.payload_sha256).unwrap();
    assert!(
        db.prepare(
            &a,
            Operation::Put {
                id: Uuid::new_v4(),
                expected_header: h,
                expected_revision: 0,
                item
            }
        )
        .is_err()
    );
}
#[test]
fn missing_truncated_or_ahead_journal_never_reopens_and_releases_lock() {
    use std::io::Write;
    for mode in ["missing", "truncated", "ahead"] {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("vault");
        let mut db = Database::open(&path).unwrap();
        let a = owner();
        let _ = setup(&mut db, &a);
        drop(db);
        let journal = path.join("vault.journal");
        match mode {
            "missing" => std::fs::remove_file(&journal).unwrap(),
            "truncated" => {
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&journal)
                    .unwrap();
                f.set_len(f.metadata().unwrap().len() - 1).unwrap()
            }
            _ => std::fs::OpenOptions::new()
                .append(true)
                .open(&journal)
                .unwrap()
                .write_all(&[0, 0, 0, 5, b'x'])
                .unwrap(),
        }
        assert!(Database::open(&path).is_err());
        assert!(Database::open(&path).is_err());
    }
}

#[test]
fn cancelled_and_lost_ack_rotations_keep_old_state_until_exact_restart_commit() {
    use moosuri_vault_core::{ClientVault, inventory_digest};
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("vault");
    let mut db = Database::open(&path).unwrap();
    let mut o = owner();
    o.epoch = 0; // Actual bootstrap account epochs begin at zero.
    let mut active = setup(&mut db, &o);
    let item = active
        .seal_item(Uuid::new_v4(), 1, &fields(), &mut OsEntropy)
        .unwrap();
    let initial = db.state(&o).unwrap();
    let op = Operation::Put {
        id: Uuid::new_v4(),
        expected_header: initial.header_sha256,
        expected_revision: 0,
        item,
    };
    let r = db.prepare(&o, op.clone()).unwrap();
    db.commit(&o, op.id(), &r.payload_sha256).unwrap();
    let before = db.state(&o).unwrap();
    let old_head = before.header_sha256.clone();
    let old_inventory = before.inventory_sha256.clone();
    let mut client = ClientVault::new(active);
    let password = SecretText::new("Synthetic rotated vault password".into()).unwrap();
    let candidate = |c: &ClientVault| {
        let (id, digest) = c.pending_candidate().unwrap();
        Operation::Key {
            id,
            expected_header: Some(old_head.clone()),
            expected_inventory: old_inventory.clone(),
            header: c.pending_header().unwrap().clone(),
            items: c.pending_items().unwrap().to_vec(),
            commitment: digest.iter().map(|b| format!("{b:02x}")).collect(),
        }
    };
    client
        .prepare_rotation(
            &password,
            &before.items,
            inventory_digest(&before.items).unwrap(),
            &mut OsEntropy,
        )
        .unwrap();
    let key = candidate(&client);
    let r = db.prepare(&o, key.clone()).unwrap();
    assert_eq!(r.state, "pending");
    assert_eq!(db.state(&o).unwrap().header_sha256, old_head);
    assert_eq!(db.state(&o).unwrap().inventory_sha256, old_inventory);
    assert!(client.active().is_err());
    drop(db);
    let mut db = Database::open(&path).unwrap();
    assert_eq!(
        db.cancel(&o, key.id(), key.clone()).unwrap().state,
        "aborted"
    );
    assert_eq!(db.prepare(&o, key).unwrap().state, "aborted");
    client.abort_pending();
    assert_eq!(
        client
            .active()
            .unwrap()
            .open_item(&before.items[0])
            .unwrap()
            .secret
            .expose(),
        "SYNTHETIC PRIVATE FIELD CANARY"
    );
    client
        .prepare_rotation(
            &password,
            &before.items,
            inventory_digest(&before.items).unwrap(),
            &mut OsEntropy,
        )
        .unwrap();
    let recovery =
        RecoveryKey::parse(&client.pending_recovery().unwrap().unwrap().expose_encoded()).unwrap();
    client.verify_recovery(&recovery).unwrap();
    let key = candidate(&client);
    let r = db.prepare(&o, key.clone()).unwrap();
    let committed = db.commit(&o, key.id(), &r.payload_sha256).unwrap(); // First ACK is lost.
    drop(db);
    let mut db = Database::open(&path).unwrap();
    let repeated = db.commit(&o, key.id(), &r.payload_sha256).unwrap();
    assert_eq!(
        serde_json::to_vec(&repeated).unwrap(),
        serde_json::to_vec(&committed).unwrap()
    );
    let after = db.state(&o).unwrap();
    assert_eq!(after.items.len(), 1);
    assert_ne!(after.header_sha256, old_head);
    assert_eq!(after.items[0].generation, before.items[0].generation + 1);
    assert_eq!(after.items[0].revision, before.items[0].revision + 1);
    let (id, digest) = client.pending_candidate().unwrap();
    client
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    client.activate().unwrap();
    assert_eq!(
        client
            .active()
            .unwrap()
            .open_item(&after.items[0])
            .unwrap()
            .secret
            .expose(),
        "SYNTHETIC PRIVATE FIELD CANARY"
    );
    assert!(
        client
            .active()
            .unwrap()
            .open_item(&before.items[0])
            .is_err()
    );
}
