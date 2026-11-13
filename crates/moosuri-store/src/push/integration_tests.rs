use super::*;
use crate::tests::{TEST_LOCK, admin, directory};
use age::secrecy::ExposeSecret;
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
};

const DROP_PUSH: &str = "DROP TABLE push_enrollment_receipts; DROP TABLE push_queue; DROP TABLE push_subscriptions; DROP TABLE push_vapid_meta; DROP TABLE push_owners; DROP TABLE push_worker_state;";
const CONTACT: &str = "https://push-owner.example.test";

#[tokio::test]
async fn actual_runtime_reopen_retains_binding_and_does_not_repair_missing_or_changed_keys() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let login = admin(&store).await;
    let input = tests::enrollment(Uuid::new_v4().to_string());
    let saved = serde_json::to_vec(&input).unwrap();
    let expected = store
        .enroll_push(&login.user, input, CONTACT.into())
        .await
        .unwrap();
    let key_path = dir.path().join(KEY_FILE);
    let original = fs::read(&key_path).unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let user = store.authenticate(login.token.clone()).await.unwrap();
    let receipt = store
        .enroll_push(
            &user,
            serde_json::from_slice(&saved).unwrap(),
            CONTACT.into(),
        )
        .await
        .unwrap();
    assert_eq!(receipt.binding_id, expected.binding_id);
    assert_eq!(fs::read(&key_path).unwrap(), original);
    drop(store);

    fs::remove_file(&key_path).unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(!key_path.exists());
    let user = store.authenticate(login.token.clone()).await.unwrap();
    assert!(store.list_threads(&user).await.is_ok());
    let device = serde_json::from_slice::<PushEnrollment>(&saved)
        .unwrap()
        .device_id;
    assert!(
        store
            .push_configuration(&user, device.clone(), CONTACT.into())
            .await
            .is_err()
    );
    assert!(
        !key_path.exists(),
        "ordinary open or failed config must not regenerate an active missing key"
    );
    drop(store);

    let (replacement, _) = moosuri_connectors::push::generate_vapid().unwrap();
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&key_path)
        .unwrap()
        .write_all(&replacement)
        .unwrap();
    let store = Store::open(dir.path()).unwrap();
    let user = store.authenticate(login.token).await.unwrap();
    assert!(store.list_threads(&user).await.is_ok());
    assert!(
        store
            .push_configuration(&user, device, CONTACT.into())
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(&key_path).unwrap().as_slice(),
        replacement.as_slice()
    );
}

#[tokio::test]
async fn strict_runtime_and_stopped_inventory_reject_invalid_key_carriers_and_crash_orphans() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let other = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let device = Uuid::new_v4().to_string();
    store
        .push_configuration(&user, device, CONTACT.into())
        .await
        .unwrap();
    drop(store);
    let path = dir.path().join(KEY_FILE);
    let valid = fs::read(&path).unwrap();
    for kind in ["short", "public_mode", "symlink", "hardlink", "orphan"] {
        match kind {
            "short" => fs::write(&path, &valid[..31]).unwrap(),
            "public_mode" => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
            "symlink" => {
                fs::write(other.path().join("key"), &valid).unwrap();
                fs::remove_file(&path).unwrap();
                symlink(other.path().join("key"), &path).unwrap();
            }
            "hardlink" => fs::hard_link(&path, other.path().join("linked")).unwrap(),
            "orphan" => {
                fs::write(dir.path().join(".web-push-vapid-interrupted.tmp"), &valid).unwrap()
            }
            _ => unreachable!(),
        }
        assert!(Store::open(dir.path()).is_err(), "runtime accepted {kind}");
        assert!(
            crate::backup::files::stopped_lock(dir.path()).is_err(),
            "offline admitted {kind}"
        );
        match kind {
            "hardlink" => fs::remove_file(other.path().join("linked")).unwrap(),
            "orphan" => {
                fs::remove_file(dir.path().join(".web-push-vapid-interrupted.tmp")).unwrap()
            }
            _ => {}
        }
        fs::remove_file(&path).unwrap();
        OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(&path)
            .unwrap()
            .write_all(&valid)
            .unwrap();
    }
    drop(Store::open(dir.path()).unwrap());
}

#[tokio::test]
async fn schema23_failure_rolls_back_and_retries_without_creating_signing_authority() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch(&format!("DROP TABLE vault_handoffs;DROP TABLE vault_grants;DROP TABLE vault_operation_commitments;{DROP_PUSH} CREATE TABLE push_enrollment_receipts(conflict INTEGER); PRAGMA user_version=22;")).unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        22
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'push_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    conn.execute_batch("DROP TABLE push_enrollment_receipts;")
        .unwrap();
    drop(conn);
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::CURRENT_SCHEMA_VERSION
    );
    assert!(!dir.path().join(KEY_FILE).exists());
}

#[tokio::test]
async fn actual_encrypted_export_strips_push_authority_only_from_staged_database() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let kit = directory();
    fs::set_permissions(kit.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    store
        .enroll_push(
            &user,
            tests::enrollment(Uuid::new_v4().to_string()),
            CONTACT.into(),
        )
        .await
        .unwrap();
    tests::notify(&store, &user).await;
    store.poll_push_queue().await.unwrap();
    let claim = store.claim_push().await.unwrap().unwrap();
    drop(claim);
    drop(store);
    let signing_key = fs::read(dir.path().join(KEY_FILE)).unwrap();
    let identity = age::x25519::Identity::generate();
    let identity_path = kit.path().join("identity");
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&identity_path)
        .unwrap()
        .write_all(identity.to_string().expose_secret().as_bytes())
        .unwrap();
    crate::backup::setup(
        dir.path(),
        &identity.to_public().to_string(),
        &identity_path,
    )
    .unwrap();
    let output = kit.path().join("export.age");
    crate::backup::create(dir.path(), &output).unwrap();
    assert_eq!(fs::read(dir.path().join(KEY_FILE)).unwrap(), signing_key);
    let original = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(original.query_row("SELECT count(*) FROM push_subscriptions WHERE state='active' AND credentials IS NOT NULL AND session_digest IS NOT NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(
        original
            .query_row(
                "SELECT count(*) FROM push_queue WHERE state='claimed'",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    drop(original);
    let decrypted = age::decrypt(&identity, &fs::read(output).unwrap()).unwrap();
    let mut database = None;
    let mut names = vec![];
    for entry in tar::Archive::new(decrypted.as_slice()).entries().unwrap() {
        let mut entry = entry.unwrap();
        let name = entry.path().unwrap().to_string_lossy().into_owned();
        assert_ne!(name, KEY_FILE);
        if name == "assistant.sqlite" {
            let mut bytes = vec![];
            entry.read_to_end(&mut bytes).unwrap();
            database = Some(bytes);
        }
        names.push(name);
    }
    assert_eq!(names.len(), 5);
    let snapshot = kit.path().join("staged.sqlite");
    fs::write(&snapshot, database.unwrap()).unwrap();
    let snapshot = Connection::open(snapshot).unwrap();
    assert_eq!(snapshot.query_row("SELECT count(*) FROM push_subscriptions WHERE state='restore_required' AND credentials IS NULL AND session_digest IS NULL AND endpoint_hash IS NULL", [], |r| r.get::<_, i64>(0)).unwrap(), 1);
    assert_eq!(
        snapshot
            .query_row(
                "SELECT count(*) FROM push_queue WHERE state='quarantined' AND nonce IS NULL",
                [],
                |r| r.get::<_, i64>(0)
            )
            .unwrap(),
        1
    );
    assert_eq!(
        snapshot
            .query_row("SELECT retired FROM push_vapid_meta", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        snapshot
            .query_row("SELECT count(*) FROM push_enrollment_receipts", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
        1
    );
    assert_eq!(
        snapshot
            .query_row("SELECT count(*) FROM local_notifications", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        1
    );
    fs::remove_file(dir.path().join(KEY_FILE)).unwrap();
    crate::backup::create(dir.path(), &kit.path().join("missing-key.age")).unwrap();
    assert!(!dir.path().join(KEY_FILE).exists());
}
