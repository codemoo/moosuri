use super::*;
use crate::backup::{
    live,
    tests::{directory, key, unpack},
};
use crate::tests::{PASSWORD, TEST_LOCK, admin};
use moosuri_core::MemoryDraft;

fn memory(text: &str) -> MemoryDraft {
    MemoryDraft {
        text: text.into(),
        subject: "Synthetic snapshot".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
#[tokio::test]
async fn live_full_snapshot_is_immutable_and_later_deletions_win_on_restore() {
    let _lock = TEST_LOCK.lock().await;
    let data = directory();
    let exports = directory();
    let kit = directory();
    let store = Store::open(data.path()).unwrap();
    let user = admin(&store).await.user;
    assert!(matches!(
        store.capture_export().await,
        Err(Error::Unconfigured)
    ));
    let forget = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            memory("Snapshot removed canary"),
        )
        .await
        .unwrap();
    let keep = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            memory("Snapshot retained canary"),
        )
        .await
        .unwrap();
    drop(store);
    let (identity, identity_file) = key(kit.path(), "recovery.key");
    let config = setup(
        data.path(),
        &identity.to_public().to_string(),
        &identity_file,
    )
    .unwrap();
    let store = Store::open(data.path()).unwrap();
    let user = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let captured = store.capture_export().await.unwrap();
    let stage = captured.temp.0.clone();
    // The returned capture releases the live writer BEFORE encryption starts.
    store
        .forget_memory(&user, forget.id.clone(), 1)
        .await
        .unwrap();
    let later = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            memory("After snapshot canary"),
        )
        .await
        .unwrap();
    let output = exports.path().join("live.age");
    captured.encrypt(&output).unwrap();
    assert!(!stage.exists());
    let archive = unpack(&output, &identity);
    assert_eq!(archive.len(), 5);
    let DecodedManifest::V2(manifest) = decode_manifest(&archive["manifest.json"]).unwrap() else {
        panic!()
    };
    assert_eq!(manifest.installation_id, config.installation_id);
    assert_eq!(
        manifest.auth_journal.sequence,
        manifest.auth_journal.snapshot_sequence
    );
    assert!(manifest.authoritative_files.is_empty());
    let inspected = directory();
    let snapshot_path = inspected.path().join("snapshot.sqlite");
    let mut f = files::create_file(&snapshot_path).unwrap();
    f.write_all(&archive["assistant.sqlite"]).unwrap();
    f.sync_all().unwrap();
    let snapshot = Connection::open(&snapshot_path).unwrap();
    let count = |id: &str| {
        snapshot
            .query_row(
                "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(count(&forget.id), 1);
    assert_eq!(count(&keep.id), 1);
    assert_eq!(count(&later.id), 0);
    assert!(matches!(
        store.capture_export().await.unwrap().encrypt(&output),
        Err(Error::OutputExists)
    ));

    // Real receiver + historical restore consume the exact live-created export.
    let receiver = directory();
    let artifacts = directory();
    let ledger = directory();
    live::init_target(
        receiver.path(),
        artifacts.path(),
        ledger.path(),
        &output,
        &config.installation_id,
    )
    .unwrap();
    let journals = store
        .capture_recovery_journals(live::ExportBinding::open(&output).unwrap())
        .await
        .unwrap();
    let mut frame = Vec::new();
    journals.write_to(&mut frame).unwrap();
    let ack = live::receive(receiver.path(), &frame[..]).unwrap();
    drop(store);
    let target = live::target_status(receiver.path()).unwrap();
    let restore_parent = directory();
    let destination = restore_parent.path().join("restored");
    restore_historical(
        &target.export,
        target.journal.as_ref().unwrap(),
        target.trusted_head.as_ref().unwrap(),
        &ack.trusted_head_sha256,
        &identity_file,
        &identity_file,
        &destination,
    )
    .unwrap();
    let restored = Connection::open(destination.join("assistant.sqlite")).unwrap();
    let count = |id: &str| {
        restored
            .query_row(
                "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
                [id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap()
    };
    assert_eq!(count(&forget.id), 0);
    assert_eq!(count(&keep.id), 1);
    assert_eq!(count(&later.id), 0);
}

#[tokio::test]
async fn damaged_stage_and_unsafe_destination_never_publish_plaintext() {
    let _lock = TEST_LOCK.lock().await;
    let data = directory();
    let exports = directory();
    let kit = directory();
    let store = Store::open(data.path()).unwrap();
    admin(&store).await;
    drop(store);
    let (identity, key_file) = key(kit.path(), "key");
    setup(data.path(), &identity.to_public().to_string(), &key_file).unwrap();
    let store = Store::open(data.path()).unwrap();
    assert!(validate_directory(data.path(), data.path()).is_err());
    let captured = store.capture_export().await.unwrap();
    fs::write(
        captured.temp.0.join(destructive_intents::FILE_NAME),
        b"truncated",
    )
    .unwrap();
    let output = exports.path().join("bad.age");
    assert!(captured.encrypt(&output).is_err());
    assert!(!output.exists());
    assert_eq!(fs::read_dir(exports.path()).unwrap().count(), 0);
    let captured = store.capture_export().await.unwrap();
    fs::remove_file(captured.temp.0.join("auth-epochs.journal")).unwrap();
    assert!(captured.encrypt(&output).is_err());
    assert!(store.initialized().await.unwrap());
}
