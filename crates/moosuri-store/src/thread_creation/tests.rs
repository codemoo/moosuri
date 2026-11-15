use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};

#[tokio::test]
async fn keyed_creation_is_atomic_canonical_and_returns_current_summary() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let request = Uuid::new_v4().to_string();
    let (a, b) = tokio::join!(
        store.create_thread_with_request_id(
            &user,
            "  Original title  ".into(),
            Some(request.clone())
        ),
        store.create_thread_with_request_id(&user, "Original title".into(), Some(request.clone()))
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.id, request);
    assert_eq!(a.id, b.id);
    assert_eq!(store.list_threads(&user).await.unwrap().len(), 1);
    assert!(matches!(
        store
            .create_thread_with_request_id(&user, "Different".into(), Some(request.clone()))
            .await,
        Err(StoreError::Conflict)
    ));
    let id = a.id.clone();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE threads SET title='Renamed title',updated_at=updated_at+1 WHERE id=?1",
                [id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let retry = store
        .create_thread_with_request_id(&user, "Original title".into(), Some(request))
        .await
        .unwrap();
    assert_eq!(retry.title, "Renamed title");
    assert!(retry.updated_at > a.updated_at);
    let legacy = store.create_thread(&user, "Legacy".into()).await.unwrap();
    assert!(matches!(
        store
            .create_thread_with_request_id(&user, "Legacy".into(), Some(legacy.id))
            .await,
        Err(StoreError::Conflict)
    ));
    let other = store.create_thread(&user, "Legacy".into()).await.unwrap();
    assert_ne!(other.id, retry.id);
    assert!(matches!(
        store
            .create_thread_with_request_id(&user, "Valid".into(), Some("invalid".into()))
            .await,
        Err(StoreError::InvalidInput)
    ));
    store.call(|db| {
        let (kind, bytes): (String, i64) = db.conn.query_row("SELECT typeof(title_sha256),length(title_sha256) FROM thread_creation_receipts LIMIT 1", [], |r| Ok((r.get(0)?,r.get(1)?)))?;
        assert_eq!((kind, bytes), ("blob".into(), 32));
        Ok(())
    }).await.unwrap();
}

#[tokio::test]
async fn creation_receipts_are_owner_scoped_and_logout_fences_replay() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let owner = admin(&store).await.user;
    let other = member(&store, &owner, "member@example.test").await.user;
    let key = Uuid::new_v4().to_string();
    let a = store
        .create_thread_with_request_id(&owner, "Owner".into(), Some(key.clone()))
        .await
        .unwrap();
    let b = store
        .create_thread_with_request_id(&other, "Other".into(), Some(key.clone()))
        .await
        .unwrap();
    assert_eq!(a.id, b.id);
    assert_eq!(store.list_threads(&owner).await.unwrap()[0].title, "Owner");
    assert_eq!(store.list_threads(&other).await.unwrap()[0].title, "Other");
    store.delete_thread(&owner, key.clone()).await.unwrap();
    assert!(matches!(
        store
            .create_thread_with_request_id(&owner, "Owner".into(), Some(key.clone()))
            .await,
        Err(StoreError::NotFound)
    ));
    assert!(
        store
            .create_thread_with_request_id(&other, "Other".into(), Some(key.clone()))
            .await
            .is_ok()
    );
    store.logout(&other).await.unwrap();
    assert!(matches!(
        store
            .create_thread_with_request_id(&other, "Other".into(), Some(key))
            .await,
        Err(StoreError::Unauthorized)
    ));
}

#[tokio::test]
async fn latest_deletion_replay_fences_snapshot_before_keyed_creation() {
    let _guard = TEST_LOCK.lock().await;
    for historical in [false, true] {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = admin(&store).await.user;
        let snapshots = directory();
        let snapshot = snapshots.path().join("before.sqlite");
        let copy = snapshot.clone();
        store
            .call(move |db| {
                let mut dest = Connection::open(copy)?;
                let backup = rusqlite::backup::Backup::new(&db.conn, &mut dest)?;
                while !matches!(backup.step(256)?, rusqlite::backup::StepResult::Done) {}
                Ok(())
            })
            .await
            .unwrap();
        std::fs::set_permissions(&snapshot, {
            use std::os::unix::fs::PermissionsExt;
            std::fs::Permissions::from_mode(0o600)
        })
        .unwrap();
        if historical {
            let conn = Connection::open(&snapshot).unwrap();
            conn.execute_batch("DROP TABLE thread_creation_receipts;PRAGMA user_version=70;")
                .unwrap();
        }
        let key = Uuid::new_v4().to_string();
        store
            .create_thread_with_request_id(&user, "Original".into(), Some(key.clone()))
            .await
            .unwrap();
        store.delete_thread(&user, key.clone()).await.unwrap();
        drop(store);
        // Keep the latest independent deletion journal while restoring a DB
        // snapshot that has never seen the request or its creation receipt.
        std::fs::copy(&snapshot, dir.path().join("assistant.sqlite")).unwrap();
        let store = Store::open(dir.path()).unwrap();
        assert!(store.list_threads(&user).await.unwrap().is_empty());
        assert!(matches!(
            store
                .create_thread_with_request_id(&user, "Original".into(), Some(key.clone()))
                .await,
            Err(StoreError::NotFound)
        ));
        store
            .call(move |db| {
                let value: Option<Vec<u8>> = db.conn.query_row(
                    "SELECT title_sha256 FROM thread_creation_receipts WHERE request_id=?1",
                    [key],
                    |r| r.get(0),
                )?;
                assert!(value.is_none());
                assert_eq!(
                    db.conn
                        .pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))?,
                    71
                );
                Ok(())
            })
            .await
            .unwrap();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        assert!(store.list_threads(&user).await.unwrap().is_empty());
    }
}

#[tokio::test]
async fn schema70_upgrade_preserves_threads_without_adopting_legacy_ids() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let legacy = store.create_thread(&user, "Existing".into()).await.unwrap();
    drop(store);
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE thread_creation_receipts;PRAGMA user_version=70;")
        .unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(store.list_threads(&user).await.unwrap()[0].id, legacy.id);
    assert!(matches!(
        store
            .create_thread_with_request_id(&user, "Existing".into(), Some(legacy.id))
            .await,
        Err(StoreError::Conflict)
    ));
    store
        .create_thread_with_request_id(&user, "New".into(), Some(Uuid::new_v4().to_string()))
        .await
        .unwrap();
}
