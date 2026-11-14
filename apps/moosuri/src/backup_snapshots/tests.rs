use super::*;
use std::{fs, os::unix::fs::PermissionsExt};

#[tokio::test]
async fn periodic_worker_captures_live_store_and_drains_without_extra_snapshot() {
    let private = || {
        tempfile::Builder::new()
            .permissions(fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap()
    };
    let source = private();
    let exports = private();
    let kit = private();
    let store = Store::open(source.path()).unwrap();
    store
        .bootstrap(
            "Synthetic".into(),
            "snapshot@example.test".into(),
            "Synthetic-snapshot-password".into(),
            false,
        )
        .await
        .unwrap();
    assert!(
        Worker::start(store.clone(), source.path(), exports.path(), 300)
            .await
            .is_err()
    );
    drop(store);
    let key = kit.path().join("recovery.key");
    let recipient = backup::keygen(&key).unwrap();
    backup::setup(source.path(), &recipient, &key).unwrap();
    let store = Store::open(source.path()).unwrap();
    assert!(
        Worker::start(store.clone(), source.path(), exports.path(), 5)
            .await
            .is_err()
    );
    let worker = Worker::start(store.clone(), source.path(), exports.path(), 300)
        .await
        .unwrap();
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let ready = fs::read_dir(exports.path()).unwrap().any(|entry| {
                entry
                    .unwrap()
                    .path()
                    .extension()
                    .is_some_and(|e| e == "age")
            });
            if ready {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    worker.shutdown().await;
    assert_eq!(fs::read_dir(exports.path()).unwrap().count(), 1);
    assert!(store.initialized().await.unwrap());
    drop(store);
    assert!(
        Store::open(source.path())
            .unwrap()
            .initialized()
            .await
            .unwrap()
    );
}
