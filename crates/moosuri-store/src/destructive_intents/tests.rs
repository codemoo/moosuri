use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::MemoryDraft;
fn draft(text: &str) -> MemoryDraft {
    MemoryDraft {
        text: text.into(),
        subject: "Synthetic fact".into(),
        speaker: None,
        valid_from: None,
        valid_until: None,
        evidence: vec![],
    }
}
fn downgrade_to_legacy(root: &Path) {
    let conn = Connection::open(root.join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE push_enrollment_receipts; DROP TABLE push_queue; DROP TABLE push_subscriptions; DROP TABLE push_vapid_meta; DROP TABLE push_owners; DROP TABLE push_worker_state; DROP TABLE schedule_chat_commands; DROP TABLE chat_media; DROP TABLE media_run_dependencies; DROP TABLE media_objects; DROP TABLE provider_vision_probes; DROP TABLE installation_restores; DROP TABLE installation_cutover; DROP TABLE local_notifications; DROP TABLE notification_settings; DROP TABLE reminder_clock; DROP TABLE reminder_owners; DROP TABLE reminder_skipped_ranges; DROP TABLE reminder_occurrences; DROP TABLE reminder_schedules; DROP TABLE personal_artifact_run_profiles; DROP TABLE personal_artifact_run_dependencies; DROP TABLE personal_artifact_command_plans; DROP TABLE personal_artifact_mutations; DROP TABLE personal_artifact_dependencies; DROP TABLE personal_artifact_versions; DROP TABLE personal_artifacts; DROP TABLE account_management_receipts; ALTER TABLE accounts DROP COLUMN status; ALTER TABLE accounts DROP COLUMN management_revision; DROP TABLE destructive_journal_meta; PRAGMA user_version=14;")
        .unwrap();
    drop(conn);
    std::fs::remove_file(root.join(FILE_NAME)).unwrap();
}
#[tokio::test]
async fn pending_legacy_prefix_is_replayed_then_frozen_before_successor_adoption() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let old = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            draft("Legacy private canary"),
        )
        .await
        .unwrap();
    let current = store
        .create_memory(
            &user,
            Uuid::new_v4().to_string(),
            draft("Successor private canary"),
        )
        .await
        .unwrap();
    drop(store);
    downgrade_to_legacy(dir.path());
    let mut legacy =
        knowledge_journal::Journal::open(&dir.path().join("knowledge-invalidations.journal"))
            .unwrap();
    legacy.append(&user.tenant.0, &old.id, 1).unwrap();
    drop(legacy);
    let original = std::fs::read(dir.path().join("knowledge-invalidations.journal")).unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.memory(&user, old.id, None).await,
        Err(StoreError::NotFound)
    ));
    store
        .forget_memory(&user, current.id.clone(), 1)
        .await
        .unwrap();
    store
        .call(move |db| {
            assert_eq!(db.destructive_journal.binding.legacy_sequence, 1);
            assert_eq!(db.destructive_journal.records.len(), 1);
            assert!(
                db.knowledge_journal
                    .append(&user.tenant.0, &current.id, 1)
                    .is_err()
            );
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    assert_eq!(
        std::fs::read(dir.path().join("knowledge-invalidations.journal")).unwrap(),
        original
    );
    Store::open(dir.path()).unwrap();
}
#[tokio::test]
async fn interrupted_migration_reuses_exact_header_and_never_resets_invalid_partial_file() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    drop(store);
    downgrade_to_legacy(dir.path());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("CREATE TABLE destructive_journal_meta(interrupted INTEGER);")
        .unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    let original = std::fs::read(dir.path().join(FILE_NAME)).unwrap();
    assert_eq!(original.len(), 96);
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        14
    );
    conn.execute_batch("DROP TABLE destructive_journal_meta;")
        .unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    drop(store);
    assert_eq!(std::fs::read(dir.path().join(FILE_NAME)).unwrap(), original);
    downgrade_to_legacy(dir.path());
    std::fs::write(dir.path().join(FILE_NAME), b"partial header").unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
    assert_eq!(
        std::fs::read(dir.path().join(FILE_NAME)).unwrap(),
        b"partial header"
    );
}
#[tokio::test]
async fn missing_wrong_installation_partial_and_changed_predecessor_are_rejected() {
    let _guard = TEST_LOCK.lock().await;
    for damage in 0..4 {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        drop(store);
        match damage {
            0 => std::fs::remove_file(dir.path().join(FILE_NAME)).unwrap(),
            1 => {
                std::fs::remove_file(dir.path().join(FILE_NAME)).unwrap();
                Journal::initialize(
                    &dir.path().join(FILE_NAME),
                    &Uuid::new_v4().to_string(),
                    0,
                    [0; 32],
                )
                .unwrap();
            }
            2 => {
                use std::io::Write;
                std::fs::OpenOptions::new()
                    .append(true)
                    .open(dir.path().join(FILE_NAME))
                    .unwrap()
                    .write_all(b"partial record")
                    .unwrap();
            }
            _ => {
                let mut legacy = knowledge_journal::Journal::open(
                    &dir.path().join("knowledge-invalidations.journal"),
                )
                .unwrap();
                legacy
                    .append(&user.tenant.0, &Uuid::new_v4().to_string(), 1)
                    .unwrap();
            }
        }
        assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
        if damage == 0 {
            assert!(!dir.path().join(FILE_NAME).exists());
        }
    }
}
#[tokio::test]
async fn failed_thread_delete_poison_replay_and_lost_ack_preserve_tenant_boundaries() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let other = member(&store, &user, "other@example.test").await.user;
    let mine = store
        .create_thread(&user, "Synthetic removed thread".into())
        .await
        .unwrap();
    let theirs = store
        .create_thread(&other, "Synthetic retained thread".into())
        .await
        .unwrap();
    assert_eq!(
        store.delete_thread(&user, theirs.id.clone()).await,
        Err(StoreError::NotFound)
    );
    store.call(|db|{assert!(db.destructive_journal.records.is_empty());db.conn.execute_batch("CREATE TEMP TRIGGER fail_thread BEFORE DELETE ON threads BEGIN SELECT RAISE(ABORT,'synthetic delete interruption'); END;")?;Ok(())}).await.unwrap();
    assert!(store.delete_thread(&user, mine.id).await.is_err());
    assert_eq!(store.initialized().await, Err(StoreError::Unavailable));
    assert_eq!(
        store.delete_thread(&other, theirs.id.clone()).await,
        Err(StoreError::Unavailable),
        "a live actor with an adopted unapplied intent must refuse another destructive mutation"
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.list_threads(&user).await.unwrap().is_empty());
    assert_eq!(store.list_threads(&other).await.unwrap()[0].id, theirs.id);
    let next = store
        .create_thread(&user, "Synthetic lost response".into())
        .await
        .unwrap();
    let tenant = user.tenant.0.clone();
    let id = next.id.clone();
    let lost: Result<()> = store
        .call(move |db| {
            commit(db, &tenant, Effect::DeleteThread { target: id })?;
            Err(StoreError::Unavailable)
        })
        .await;
    assert_eq!(lost, Err(StoreError::Unavailable));
    assert_eq!(
        store.delete_thread(&user, next.id).await,
        Err(StoreError::NotFound)
    );
    store
        .call(|db| {
            assert_eq!(db.destructive_journal.records.len(), 2);
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.list_threads(&user).await.unwrap().is_empty());
}

#[test]
fn bounded_records_reject_invalid_adoption_and_damaged_sequence_chain_or_payload() {
    let dir = directory();
    let path = dir.path().join(FILE_NAME);
    let tenant = Uuid::new_v4().to_string();
    let mut journal = Journal::initialize(&path, &Uuid::new_v4().to_string(), 0, [0; 32]).unwrap();
    for targets in [
        vec![],
        vec![Uuid::new_v4().to_string(); 2],
        (0..33).map(|_| Uuid::new_v4().to_string()).collect(),
    ] {
        assert!(matches!(
            journal.prepare(&tenant, Effect::DeleteSources { targets }),
            Err(StoreError::InvalidInput)
        ));
        assert_eq!(std::fs::metadata(&path).unwrap().len(), 96);
    }
    let prepared = journal
        .prepare(
            &tenant,
            Effect::DeleteThread {
                target: Uuid::new_v4().to_string(),
            },
        )
        .unwrap();
    journal.append(prepared).unwrap();
    drop(journal);
    let original = std::fs::read(&path).unwrap();
    for damage in 0..5 {
        let mut bytes = original.clone();
        match damage {
            0 => bytes[96..100].copy_from_slice(&8193u32.to_be_bytes()),
            1 => bytes[100..108].copy_from_slice(&2i64.to_be_bytes()),
            2 => bytes[108] ^= 1,
            3 => bytes[140] ^= 1,
            _ => {
                bytes.pop();
            }
        }
        std::fs::write(&path, &bytes).unwrap();
        assert!(matches!(Journal::open(&path), Err(StoreError::Corrupt)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
    std::fs::write(&path, original).unwrap();
    assert_eq!(Journal::open(&path).unwrap().records.len(), 1);
}

#[tokio::test]
async fn sync_failure_on_complete_header_blocks_migration_until_successful_exact_reuse() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    drop(store);
    downgrade_to_legacy(dir.path());
    let path = dir.path().join(FILE_NAME);
    let fault = journal::fail_sync_for_test(&path);
    assert!(matches!(
        Store::open(dir.path()),
        Err(StoreError::Unavailable)
    ));
    let readable = std::fs::read(&path).unwrap();
    assert_eq!(readable.len(), 96);
    assert_eq!(&readable[..8], b"MSDSTR01");
    assert_eq!(&readable[64..], Sha256::digest(&readable[..64]).as_slice());
    // A readable complete header must not bypass syncing on the existing-file
    // initialize path, even when the previous create reached write_all.
    for _ in 0..2 {
        assert!(matches!(
            Store::open(dir.path()),
            Err(StoreError::Unavailable)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), readable);
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        assert_eq!(
            conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            14
        );
        assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='destructive_journal_meta'", [], |r|r.get::<_,i64>(0)).unwrap(), 0);
    }
    drop(fault);
    let store = Store::open(dir.path()).unwrap();
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                CURRENT_SCHEMA_VERSION
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT sequence FROM destructive_journal_meta", [], |r| r
                        .get::<_, i64>(
                        0
                    ))?,
                0
            );
            assert!(db.destructive_journal.records.is_empty());
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    assert_eq!(std::fs::read(path).unwrap(), readable);
}

#[tokio::test]
async fn sync_failure_on_complete_record_blocks_replay_watermark_until_successful_reopen() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let thread = store
        .create_thread(&user, "Synthetic sync interruption".into())
        .await
        .unwrap();
    let path = dir.path().join(FILE_NAME);
    let fault = journal::fail_sync_for_test(&path);
    assert_eq!(
        store.delete_thread(&user, thread.id.clone()).await,
        Err(StoreError::Unavailable)
    );
    assert_eq!(store.initialized().await, Err(StoreError::Unavailable));
    drop(store);
    let readable = std::fs::read(&path).unwrap();
    assert!(readable.len() > 96);
    assert!(
        matches!(Journal::open(&path), Err(StoreError::Unavailable)),
        "the complete record parses but cannot become durable authority"
    );
    for _ in 0..2 {
        assert!(matches!(
            Store::open(dir.path()),
            Err(StoreError::Unavailable)
        ));
        assert_eq!(std::fs::read(&path).unwrap(), readable);
        let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
        assert_eq!(
            conn.query_row("SELECT sequence FROM destructive_journal_meta", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        assert_eq!(
            conn.query_row(
                "SELECT count(*) FROM threads WHERE tenant_id=?1 AND id=?2",
                params![user.tenant.0, thread.id],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
            1
        );
    }
    drop(fault);
    for _ in 0..2 {
        let store = Store::open(dir.path()).unwrap();
        assert!(store.list_threads(&user).await.unwrap().is_empty());
        store
            .call(|db| {
                assert_eq!(db.destructive_journal.records.len(), 1);
                let record = &db.destructive_journal.records[0];
                let (sequence, head): (i64, Vec<u8>) = db.conn.query_row(
                    "SELECT sequence,head FROM destructive_journal_meta",
                    [],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )?;
                assert_eq!(sequence, 1);
                assert_eq!(head, record.hash);
                Ok(())
            })
            .await
            .unwrap();
        drop(store);
        assert_eq!(std::fs::read(&path).unwrap(), readable);
    }
}
