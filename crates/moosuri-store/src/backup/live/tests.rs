use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, member};
use moosuri_core::MemoryDraft;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
fn dir() -> tempfile::TempDir {
    tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
fn child(p: &Path, name: &str) -> PathBuf {
    let d = p.join(name);
    fs::DirBuilder::new().mode(0o700).create(&d).unwrap();
    d
}
fn draft(t: &str) -> MemoryDraft {
    MemoryDraft {
        text: t.into(),
        subject: "Synthetic live capture".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
fn bytes(c: &Captured) -> Vec<u8> {
    let mut b = vec![];
    c.write_to(&mut b).unwrap();
    b
}
#[tokio::test]
async fn live_capture_receiver_prefix_crash_ack_retry_and_source_free_restore() {
    let _guard = TEST_LOCK.lock().await;
    let temp = dir();
    let source = temp.path().join("source");
    let kit = child(temp.path(), "kit");
    let output = child(temp.path(), "output");
    let store = Store::open(&source).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "live-other@example.test").await.user;
    let forgotten = store
        .create_memory(
            &a,
            Uuid::new_v4().to_string(),
            draft("Withdraw after baseline"),
        )
        .await
        .unwrap();
    let retained = store
        .create_memory(
            &b,
            Uuid::new_v4().to_string(),
            draft("Other tenant retained"),
        )
        .await
        .unwrap();
    drop(store);
    let identity = kit.join("age.key");
    let recipient = keygen(&identity).unwrap();
    let cfg = setup(&source, &recipient, &identity).unwrap();
    let export = output.join("base.age");
    create(&source, &export).unwrap();
    let fork = child(temp.path(), "fork");
    for e in fs::read_dir(&source).unwrap() {
        let e = e.unwrap();
        files::copy_file(&e.path(), &fork.join(e.file_name()), DB_LIMIT).unwrap();
    }
    let binding = ExportBinding::open(&export).unwrap();
    let receiver = child(temp.path(), "receiver");
    let artifacts = child(temp.path(), "artifacts");
    let ledger = child(temp.path(), "ledger");
    init_target(
        &receiver,
        &artifacts,
        &ledger,
        &export,
        &cfg.installation_id,
    )
    .unwrap();
    let store = Store::open(&source).unwrap();
    let a = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let first = store
        .capture_recovery_journals(binding.clone())
        .await
        .unwrap();
    let frame = bytes(&first);
    let a1 = receive(&receiver, &frame[..]).unwrap();
    assert_eq!(a1.generation, 1);
    // Same heads with a new capture UUID do not advance accepted generation.
    let capture = store
        .capture_recovery_journals(binding.clone())
        .await
        .unwrap();
    assert_eq!(
        receive(&receiver, &bytes(&capture)[..]).unwrap().generation,
        1
    );
    let (during, deleted) = tokio::join!(
        store.capture_recovery_journals(binding.clone()),
        store.forget_memory(&a, forgotten.id.clone(), 1)
    );
    deleted.unwrap();
    let during = during.unwrap();
    assert_eq!(
        during.manifest.coverage.successor_journal.sequence,
        first.manifest.coverage.successor_journal.sequence
    );
    assert_eq!(
        receive(&receiver, &bytes(&during)[..]).unwrap().generation,
        1
    );
    let next = store
        .capture_recovery_journals(binding.clone())
        .await
        .unwrap();
    // Original frozen capture cannot include subsequent deletes. Later one does.
    assert_eq!(
        first.manifest.coverage.successor_journal.sequence + 1,
        next.manifest.coverage.successor_journal.sequence
    );
    let next = bytes(&next);
    for point in [
        "before_generation",
        "after_generation",
        "before_receipt",
        "after_receipt",
    ] {
        FAILURES
            .lock()
            .unwrap()
            .push((receiver.clone(), point.into()));
        assert!(receive(&receiver, &next[..]).is_err());
        FAILURES.lock().unwrap().clear();
        let status = target_status(&receiver).unwrap();
        assert_eq!(
            status.current.as_ref().unwrap().generation,
            if point == "after_receipt" { 2 } else { 1 }
        );
    }
    let a2 = receive(&receiver, &next[..]).unwrap();
    assert_eq!(a2.generation, 2);
    assert_eq!(
        fs::metadata(ledger.join(format!("{}.json", generation(a2.generation))))
            .unwrap()
            .nlink(),
        1
    );
    assert!(receive(&receiver, &frame[..]).is_err());
    let mut corrupt = next.clone();
    let last = corrupt.len() - 1;
    corrupt[last] ^= 1;
    assert!(receive(&receiver, &corrupt[..]).is_err());
    assert!(receive(&receiver, &next[..next.len() / 2]).is_err());
    let branch = Store::open(&fork).unwrap();
    let bu = branch
        .login("live-other@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    branch
        .forget_memory(&bu, retained.id.clone(), 1)
        .await
        .unwrap();
    let fork_capture = branch
        .capture_recovery_journals(binding.clone())
        .await
        .unwrap();
    assert!(receive(&receiver, &bytes(&fork_capture)[..]).is_err());
    drop(branch);
    // Key rotation while source is stopped; receiver never needs either private key.
    drop(store);
    let new_identity = kit.join("new-age.key");
    let recipient = keygen(&new_identity).unwrap();
    setup(&source, &recipient, &new_identity).unwrap();
    let store = Store::open(&source).unwrap();
    let rotated = store.capture_recovery_journals(binding).await.unwrap();
    let a3 = receive(&receiver, &bytes(&rotated)[..]).unwrap();
    assert_eq!(a3.generation, 3);
    // Recovery input is copied target state; source tree can disappear entirely.
    drop(store);
    fs::remove_dir_all(&source).unwrap();
    let status = target_status(&receiver).unwrap();
    let dest = temp.path().join("restored");
    crate::backup::restore_historical(
        &status.export,
        status.journal.as_ref().unwrap(),
        status.trusted_head.as_ref().unwrap(),
        &a3.trusted_head_sha256,
        &identity,
        &new_identity,
        &dest,
    )
    .unwrap();
    let c = Connection::open(dest.join("assistant.sqlite")).unwrap();
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
            [forgotten.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM knowledge_memory_versions WHERE memory_id=?1",
            [retained.id],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}
