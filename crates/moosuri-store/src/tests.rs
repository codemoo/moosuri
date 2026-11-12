use super::*;
use std::{
    fs::{self, OpenOptions},
    io::Write,
};
const PASSWORD: &str = "Synthetic-test-password-2026";
fn directory() -> tempfile::TempDir {
    tempfile::Builder::new()
        .prefix("moosuri-store-")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap()
}
async fn admin(store: &Store) -> LoginResult {
    store
        .bootstrap(
            "Synthetic Admin".into(),
            "ADMIN@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    store
        .login(" admin@EXAMPLE.test ".into(), PASSWORD.into())
        .await
        .unwrap()
}
async fn member(store: &Store, admin: &UserContext, email: &str) -> LoginResult {
    let invite = store
        .create_user(admin, "Synthetic Member".into(), email.into())
        .await
        .unwrap();
    let restricted = store
        .login(email.into(), invite.temporary_password)
        .await
        .unwrap();
    store
        .change_password(&restricted.user, PASSWORD.into())
        .await
        .unwrap()
}
// The global KDF limit is intentionally exercised by a race within its own test.
// Serializing fixtures keeps unrelated tests from spending each other's two permits.
static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
#[tokio::test]
async fn tenant_scope_restriction_revocation_and_durable_reopen() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let member = member(&store, &admin.user, "member@example.test").await;
    let a = store
        .create_thread(&admin.user, "Admin private".into())
        .await
        .unwrap();
    let b = store
        .create_thread(&member.user, "Member private".into())
        .await
        .unwrap();
    assert_eq!(store.list_threads(&admin.user).await.unwrap()[0].id, a.id);
    assert_eq!(store.list_threads(&member.user).await.unwrap()[0].id, b.id);
    assert_eq!(
        store.delete_thread(&admin.user, b.id.clone()).await,
        Err(StoreError::NotFound)
    );
    assert_eq!(
        store.delete_thread(&member.user, a.id.clone()).await,
        Err(StoreError::NotFound)
    );
    assert!(matches!(
        store.list_users(&member.user).await,
        Err(StoreError::Forbidden)
    ));
    assert_eq!(store.list_users(&admin.user).await.unwrap().len(), 2);
    let admin_token = admin.token;
    let member_token = member.token;
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let admin = reopened.authenticate(admin_token).await.unwrap();
    let member = reopened.authenticate(member_token).await.unwrap();
    assert_eq!(
        reopened.list_threads(&admin).await.unwrap()[0].title,
        "Admin private"
    );
    reopened.delete_thread(&member, b.id).await.unwrap();
    assert!(reopened.list_threads(&member).await.unwrap().is_empty());
    let changed = reopened
        .change_password(&admin, "A-new-synthetic-password".into())
        .await
        .unwrap();
    assert!(matches!(
        reopened.list_users(&admin).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        reopened
            .login("admin@example.test".into(), PASSWORD.into())
            .await,
        Err(StoreError::Unauthorized)
    ));
    assert_eq!(reopened.list_users(&changed.user).await.unwrap().len(), 2);
    reopened.logout(&changed.user).await.unwrap();
    assert!(matches!(
        reopened.authenticate(changed.token).await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn temporary_password_exchange_is_atomic_and_expires() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let invite = store
        .create_user(&admin.user, "Temporary".into(), "temp@example.test".into())
        .await
        .unwrap();
    let old_password = invite.temporary_password;
    let one = store
        .login("temp@example.test".into(), old_password.clone())
        .await
        .unwrap();
    let two = store
        .login("temp@example.test".into(), old_password.clone())
        .await
        .unwrap();
    assert!(one.user.password_change_required());
    assert!(matches!(
        store.create_thread(&one.user, "no".into()).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.list_threads(&one.user).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store.list_users(&one.user).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store
            .create_user(&one.user, "no".into(), "no@example.test".into())
            .await,
        Err(StoreError::Forbidden)
    ));
    assert_eq!(
        store
            .delete_thread(&one.user, Uuid::new_v4().to_string())
            .await,
        Err(StoreError::Forbidden)
    );
    let (left, right) = tokio::join!(
        store.change_password(&one.user, "First-new-password".into()),
        store.change_password(&two.user, "Second-new-password".into())
    );
    assert_ne!(left.is_ok(), right.is_ok());
    assert!(matches!(
        left.as_ref().err().or(right.as_ref().err()),
        Some(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.authenticate(one.token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.login("temp@example.test".into(), old_password).await,
        Err(StoreError::Unauthorized)
    ));
    let expired = store
        .create_user(&admin.user, "Expired".into(), "expired@example.test".into())
        .await
        .unwrap();
    store
        .call(|db| {
            db.conn.execute(
                "UPDATE accounts SET temporary_expires_at=0 WHERE email='expired@example.test'",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store
            .login("expired@example.test".into(), expired.temporary_password)
            .await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn bootstrap_validation_normalization_and_foreign_keys() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    assert!(!store.initialized().await.unwrap());
    for email in ["", "no-at", "a@@example.test", "a @example.test"] {
        assert!(matches!(
            store
                .bootstrap("Admin".into(), email.into(), PASSWORD.into(), false)
                .await,
            Err(StoreError::InvalidInput)
        ));
    }
    assert!(matches!(
        store
            .bootstrap(
                "Admin".into(),
                "admin@example.test".into(),
                "short".into(),
                false
            )
            .await,
        Err(StoreError::InvalidInput)
    ));
    let (a, b) = tokio::join!(
        store.bootstrap(
            "A".into(),
            "first@example.test".into(),
            PASSWORD.into(),
            false
        ),
        store.bootstrap(
            "B".into(),
            "second@example.test".into(),
            PASSWORD.into(),
            false
        )
    );
    assert_ne!(a.is_ok(), b.is_ok());
    assert!(matches!(
        a.as_ref().err().or(b.as_ref().err()),
        Some(StoreError::AlreadyInitialized)
    ));
    let account = a.ok().or(b.ok()).unwrap();
    let admin = store.login(account.email, PASSWORD.into()).await.unwrap();
    let member = member(&store, &admin.user, "Mixed.Name+tag@example.test").await;
    assert_eq!(member.user.account().email, "mixed.name+tag@example.test");
    assert!(matches!(
        store
            .create_user(
                &admin.user,
                "Duplicate".into(),
                " MIXED.NAME+TAG@EXAMPLE.TEST ".into()
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let tenant = member.user.tenant.0.clone();
    let account = admin.user.account.id.clone();
    store.call(move |db| {
        assert_eq!(db.conn.query_row("PRAGMA foreign_keys",[],|r|r.get::<_,i64>(0))?,1);
        assert!(db.conn.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) VALUES (?1,'shared-id',?2,'crossing',0,0)",params![tenant,account]).is_err());
        assert!(db.conn.execute("INSERT INTO sessions(digest,tenant_id,account_id,auth_epoch,password_change_only,expires_at) VALUES (zeroblob(32),?1,?2,0,0,100)",params![tenant,account]).is_err());
        Ok(())
    }).await.unwrap();
    assert!(matches!(
        store.create_thread(&admin.user, " \n ".into()).await,
        Err(StoreError::InvalidInput)
    ));
    assert_eq!(
        store
            .delete_thread(&admin.user, "../../escape".into())
            .await,
        Err(StoreError::InvalidInput)
    );
    assert!(matches!(
        store.authenticate("bad".into()).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store
            .login("missing@example.test".into(), PASSWORD.into())
            .await,
        Err(StoreError::Unauthorized)
    ));
}
#[tokio::test]
async fn journal_gap_revokes_old_password_and_truncation_fails_closed() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let account = admin.user.account.id.clone();
    // Simulate process death immediately after durable revocation, before password DB commit.
    store
        .call(move |db| {
            db.journal.append(&account, 1)?;
            Ok(())
        })
        .await
        .unwrap();
    let token = admin.token;
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    assert!(matches!(
        reopened.authenticate(token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        reopened
            .login("admin@example.test".into(), PASSWORD.into())
            .await,
        Err(StoreError::Unauthorized)
    ));
    drop(reopened);
    // Removing a whole committed record is detected by the DB watermark.
    OpenOptions::new()
        .write(true)
        .open(dir.path().join("auth-epochs.journal"))
        .unwrap()
        .set_len(0)
        .unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Corrupt)));
    let fresh = directory();
    let empty = Store::open(fresh.path()).unwrap();
    drop(empty);
    OpenOptions::new()
        .append(true)
        .open(fresh.path().join("auth-epochs.journal"))
        .unwrap()
        .write_all(b"partial record")
        .unwrap();
    assert!(matches!(
        Store::open(fresh.path()),
        Err(StoreError::Corrupt)
    ));
}
#[tokio::test]
async fn committed_journal_reopens_without_locking_valid_password() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let admin = admin(&store).await;
    let updated = store
        .change_password(&admin.user, "A-new-synthetic-password".into())
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    store.authenticate(updated.token).await.unwrap();
    store
        .login(
            "admin@example.test".into(),
            "A-new-synthetic-password".into(),
        )
        .await
        .unwrap();
}
#[test]
fn exclusive_writer_private_files_and_symlink_rejection() {
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(Store::open(dir.path()), Err(StoreError::Busy)));
    drop(store);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{PermissionsExt, symlink};
        assert_eq!(
            fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
            0o700
        );
        for name in ["writer.lock", "assistant.sqlite", "auth-epochs.journal"] {
            assert_eq!(
                fs::metadata(dir.path().join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        let outside = directory();
        let links = directory();
        symlink(outside.path(), links.path().join("linked")).unwrap();
        assert!(Store::open(&links.path().join("linked")).is_err());
        let malicious = directory();
        symlink(
            outside.path().join("secret"),
            malicious.path().join("assistant.sqlite"),
        )
        .unwrap();
        assert!(Store::open(malicious.path()).is_err());
        assert!(!outside.path().join("secret").exists());
    }
}
#[test]
fn separate_process_cannot_take_writer_lock() {
    // The child runs only this helper, without opening a second store in the parent.
    if let Some(path) = std::env::var_os("MOOSURI_SYNTHETIC_LOCK_PATH") {
        assert!(matches!(
            Store::open(Path::new(&path)),
            Err(StoreError::Busy)
        ));
        return;
    }
    let dir = directory();
    let _store = Store::open(dir.path()).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "tests::separate_process_cannot_take_writer_lock",
            "--nocapture",
        ])
        .env("MOOSURI_SYNTHETIC_LOCK_PATH", dir.path())
        .status()
        .unwrap();
    assert!(status.success());
}

#[tokio::test]
async fn restricted_admin_and_bounded_admission() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Temporary admin".into(),
            "temporary-admin@example.test".into(),
            PASSWORD.into(),
            true,
        )
        .await
        .unwrap();
    let login = store
        .login("temporary-admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert!(matches!(
        store.list_users(&login.user).await,
        Err(StoreError::Forbidden)
    ));
    assert!(matches!(
        store
            .create_user(&login.user, "Member".into(), "member@example.test".into())
            .await,
        Err(StoreError::Forbidden)
    ));
    let permits = KDF_LIMIT.clone().acquire_many_owned(2).await.unwrap();
    assert!(matches!(
        store
            .change_password(&login.user, "A-new-synthetic-password".into())
            .await,
        Err(StoreError::Busy)
    ));
    assert!(store.initialized().await.unwrap());
    drop(permits);
    let (entered_tx, entered_rx) = oneshot::channel();
    let (release_tx, release_rx) = std::sync::mpsc::channel();
    // Hold a synthetic job to make saturation deterministic rather than relying on timing.
    store
        .inner
        .sender
        .as_ref()
        .unwrap()
        .try_send(Box::new(move |_| {
            let _ = entered_tx.send(());
            let _ = release_rx.recv();
        }))
        .unwrap_or_else(|_| panic!("empty mailbox rejected command"));
    entered_rx.await.unwrap();
    for _ in 0..64 {
        store
            .inner
            .sender
            .as_ref()
            .unwrap()
            .try_send(Box::new(|_| {}))
            .unwrap_or_else(|_| panic!("mailbox filled early"));
    }
    assert_eq!(store.initialized().await, Err(StoreError::Busy));
    release_tx.send(()).unwrap();
    // Drop drains the bounded mailbox and releases the installation lock.
    drop(store);
    assert!(
        Store::open(dir.path())
            .unwrap()
            .initialized()
            .await
            .unwrap()
    );
}

#[test]
fn unrelated_nonempty_directory_is_untouched() {
    use std::os::unix::fs::PermissionsExt;
    let dir = directory();
    fs::write(dir.path().join("unrelated.txt"), b"keep me").unwrap();
    fs::write(
        dir.path().join("assistant.sqlite"),
        b"not a moosuri database",
    )
    .unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        Store::open(dir.path()),
        Err(StoreError::InvalidInput)
    ));
    assert_eq!(
        fs::metadata(dir.path()).unwrap().permissions().mode() & 0o777,
        0o755
    );
    assert_eq!(
        fs::read(dir.path().join("unrelated.txt")).unwrap(),
        b"keep me"
    );
    assert!(!dir.path().join("writer.lock").exists());
}
