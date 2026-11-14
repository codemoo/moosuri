use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};

const RECOVERY_PASSWORD: &str = "Synthetic-operator-temporary-credential";

async fn gap(store: &Store, account: &str) {
    let account = account.to_owned();
    store
        .call(move |db| {
            let epoch: i64 = db.conn.query_row(
                "SELECT auth_epoch FROM accounts WHERE id=?1",
                [&account],
                |r| r.get(0),
            )?;
            db.journal.append(&account, epoch + 1)?;
            db.replay_journal()
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn recovery_replaces_role_requires_exchange_and_preserves_other_user() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "member@example.test").await;
    let thread = store
        .create_thread(&b.user, "Other user's retained thread".into())
        .await
        .unwrap();
    let account = a.user.account.id.clone();
    gap(&store, &account).await;
    drop(store);
    recover_account(
        dir.path(),
        account.clone(),
        Role::Member,
        RECOVERY_PASSWORD.into(),
    )
    .await
    .unwrap();
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.authenticate(a.token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await,
        Err(StoreError::Unauthorized)
    ));
    let restricted = store
        .login("admin@example.test".into(), RECOVERY_PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(restricted.user.account.role, Role::Member);
    assert!(restricted.user.password_change_required());
    assert!(matches!(
        store.list_threads(&restricted.user).await,
        Err(StoreError::Forbidden)
    ));
    let current = store
        .change_password(&restricted.user, "Synthetic-final-user-password".into())
        .await
        .unwrap();
    assert!(matches!(
        store.authenticate(restricted.token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.list_users(&current.user).await,
        Err(StoreError::Forbidden)
    ));
    let b = store.authenticate(b.token).await.unwrap();
    assert_eq!(store.list_threads(&b).await.unwrap()[0].id, thread.id);
    assert_eq!(
        store
            .recover_locked_account(
                account.clone(),
                Role::Admin,
                Zeroizing::new(RECOVERY_PASSWORD.into())
            )
            .await,
        Err(StoreError::Conflict)
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert_eq!(
        store
            .authenticate(current.token)
            .await
            .unwrap()
            .account
            .role,
        Role::Member
    );
    assert!(matches!(
        store
            .login("admin@example.test".into(), RECOVERY_PASSWORD.into())
            .await,
        Err(StoreError::Unauthorized)
    ));
}

#[tokio::test]
async fn append_before_failed_commit_poison_replay_retry_and_lost_ack_are_safe() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let account = a.user.account.id.clone();
    gap(&store, &account).await;
    store.call(|db| {
        // A SQLite abort occurs after the real append/fsync, without production
        // environment failpoints. TEMP trigger disappears when connection closes.
        db.conn.execute_batch("CREATE TEMP TRIGGER recovery_fail BEFORE UPDATE OF password_hash ON accounts BEGIN SELECT RAISE(ABORT,'synthetic commit interruption'); END;")?;
        Ok(())
    }).await.unwrap();
    assert!(
        store
            .recover_locked_account(
                account.clone(),
                Role::Admin,
                Zeroizing::new(RECOVERY_PASSWORD.into())
            )
            .await
            .is_err()
    );
    assert_eq!(store.initialized().await, Err(StoreError::Unavailable));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    for password in [PASSWORD, RECOVERY_PASSWORD] {
        assert!(matches!(
            store
                .login("admin@example.test".into(), password.into())
                .await,
            Err(StoreError::Unauthorized)
        ));
    }
    let epoch: i64 = store
        .call({
            let account = account.clone();
            move |db| locked_epoch(&db.conn, &account)
        })
        .await
        .unwrap();
    assert_eq!(epoch, 2);
    let hash = hash_sync("Synthetic-retry-operator-password").unwrap();
    let retry = account.clone();
    let result: Result<()> = store
        .call(move |db| {
            apply(db, &retry, Role::Admin, hash)?;
            // The commit succeeded but the command's response is lost.
            Err(StoreError::Unavailable)
        })
        .await;
    assert_eq!(result, Err(StoreError::Unavailable));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let login = store
        .login(
            "admin@example.test".into(),
            "Synthetic-retry-operator-password".into(),
        )
        .await
        .unwrap();
    assert_eq!(login.user.account.role, Role::Admin);
    assert_eq!(login.user.epoch, 3);
    assert!(login.user.password_change_required());
    assert_eq!(
        store
            .recover_locked_account(
                account,
                Role::Member,
                Zeroizing::new(RECOVERY_PASSWORD.into())
            )
            .await,
        Err(StoreError::Conflict)
    );
}

#[tokio::test]
async fn healthy_missing_invalid_and_competing_recovery_never_reset_healthy_accounts() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let account = a.user.account.id.clone();
    assert_eq!(
        store
            .recover_locked_account(
                account.clone(),
                Role::Member,
                Zeroizing::new(RECOVERY_PASSWORD.into())
            )
            .await,
        Err(StoreError::Conflict)
    );
    assert_eq!(
        store
            .recover_locked_account(
                Uuid::new_v4().to_string(),
                Role::Admin,
                Zeroizing::new(RECOVERY_PASSWORD.into())
            )
            .await,
        Err(StoreError::NotFound)
    );
    assert_eq!(
        store
            .recover_locked_account(
                "bad-id".into(),
                Role::Admin,
                Zeroizing::new(RECOVERY_PASSWORD.into())
            )
            .await,
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        store
            .recover_locked_account(account.clone(), Role::Admin, Zeroizing::new("short".into()))
            .await,
        Err(StoreError::InvalidInput)
    );
    assert_eq!(
        store.call(|db| Ok(db.journal.records.len())).await.unwrap(),
        0
    );
    gap(&store, &account).await;
    assert_eq!(
        store
            .recover_locked_account(
                account.clone(),
                Role::Admin,
                Zeroizing::new(PASSWORD.into())
            )
            .await,
        Err(StoreError::InvalidInput),
    );
    assert_eq!(
        store.call(|db| Ok(db.journal.records.len())).await.unwrap(),
        1
    );
    let (one, two) = tokio::join!(
        store.recover_locked_account(
            account.clone(),
            Role::Member,
            Zeroizing::new(RECOVERY_PASSWORD.into())
        ),
        store.recover_locked_account(
            account.clone(),
            Role::Admin,
            Zeroizing::new("Other-synthetic-temporary-password".into())
        ),
    );
    assert!(matches!(
        (&one, &two),
        (Ok(()), Err(StoreError::Conflict)) | (Err(StoreError::Conflict), Ok(()))
    ));
    assert_eq!(
        store.call(|db| Ok(db.journal.records.len())).await.unwrap(),
        2
    );
}

#[tokio::test]
async fn offline_wrapper_requires_existing_unlocked_installation() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let absent = dir.path().join("absent");
    assert_eq!(
        recover_account(
            &absent,
            Uuid::new_v4().to_string(),
            Role::Admin,
            RECOVERY_PASSWORD.into()
        )
        .await,
        Err(StoreError::NotFound)
    );
    assert!(!absent.exists());
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    assert_eq!(
        recover_account(
            dir.path(),
            a.user.account.id.clone(),
            Role::Admin,
            RECOVERY_PASSWORD.into()
        )
        .await,
        Err(StoreError::Busy)
    );
    store.authenticate(a.token).await.unwrap();
}

#[tokio::test]
async fn recovery_never_reauthorizes_held_run_source_or_mail_dispatch() {
    use moosuri_connectors::gmail::{SecretString, TokenSet};
    use moosuri_core::{
        CreateSourceConnection, MailApproval, MailDraftContent, MailDraftInput, ModelSelection,
        ProviderConfig, ProviderKind, SourceConnectionKind,
    };
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let connection = store
        .create_source_connection(
            &a.user,
            CreateSourceConnection {
                name: "Synthetic producer".into(),
                kind: SourceConnectionKind::CustomHttp,
                scope: "one synthetic scope".into(),
                remote_account: "producer@example.test".into(),
                mail_connection_id: None,
                sync_enabled: false,
                poll_interval_seconds: 60,
            },
        )
        .await
        .unwrap();
    let token = store
        .issue_source_token(&a.user, connection.id, 3600)
        .await
        .unwrap()
        .token;
    let cap = store
        .authenticate_source_token(token.clone())
        .await
        .unwrap();
    let source_lease = store.begin_source_run(&cap).await.unwrap();
    let provider = store
        .create_provider(
            &a.user,
            ProviderChange {
                name: "Synthetic provider".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &a.user,
            provider.id.clone(),
            1,
            "synthetic-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &a.user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: provider.id.clone(),
                provider_revision: 1,
                model: "synthetic-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&a.user, "Synthetic run".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &a.user,
            thread.id,
            Uuid::new_v4().to_string(),
            "synthetic question".into(),
        )
        .await
        .unwrap();
    let run = store.claim_run().await.unwrap().unwrap();
    let start = store.start_mail_oauth(&a.user).await.unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = store.claim_mail_oauth(&a.user, handle).await.unwrap();
    let mail = store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: Some(
                    moosuri_connectors::gmail::MINIMAL_SCOPES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap();
    let action = store
        .create_mail_draft(
            &a.user,
            MailDraftInput {
                connection_id: mail.id,
                content: MailDraftContent {
                    from: mail.email,
                    to: vec!["recipient@example.test".into()],
                    cc: vec![],
                    bcc: vec![],
                    subject: "Synthetic mail".into(),
                    text: "Synthetic body".into(),
                    reply: None,
                },
                origin_thread_id: None,
            },
            None,
        )
        .await
        .unwrap();
    store
        .decide_mail(
            &a.user,
            action.id,
            MailApproval {
                revision: action.revision,
                digest: action.digest,
            },
            true,
        )
        .await
        .unwrap();
    let dispatch = store.next_mail_dispatch().await.unwrap().unwrap();
    gap(&store, &a.user.account.id).await;
    store
        .recover_locked_account(
            a.user.account.id.clone(),
            Role::Admin,
            Zeroizing::new(RECOVERY_PASSWORD.into()),
        )
        .await
        .unwrap();
    let restricted = store
        .login("admin@example.test".into(), RECOVERY_PASSWORD.into())
        .await
        .unwrap();
    let current = store
        .change_password(&restricted.user, "Synthetic-final-password".into())
        .await
        .unwrap();
    assert!(matches!(
        store.check_run(&run.lease).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(store.begin_source_run(&cap).await.is_err());
    assert!(store.source_ingest_status(&cap).await.is_err());
    assert!(store.authenticate_source_token(token).await.is_err());
    assert!(store.source_heartbeat(&source_lease).await.is_err());
    assert!(store.mail_dispatch_binding(&dispatch).await.is_err());
    // Current live provider policy is unchanged; recovery did not manufacture a revision.
    let providers = store.list_providers(&current.user).await.unwrap();
    assert_eq!(providers.providers[0].id, provider.id);
    assert_eq!(providers.providers[0].revision, 1);
}

#[tokio::test]
async fn rejected_uninitialized_database_never_initializes_or_changes_installation_files() {
    use std::os::unix::fs::PermissionsExt;
    let _guard = TEST_LOCK.lock().await;
    for fixture in 0..3 {
        let dir = directory();
        let db_path = dir.path().join("assistant.sqlite");
        match fixture {
            0 => {
                std::fs::write(&db_path, []).unwrap();
            }
            1 => {
                let db = Connection::open(&db_path).unwrap();
                db.execute_batch("CREATE TABLE unrelated(value TEXT); INSERT INTO unrelated VALUES('synthetic');").unwrap();
            }
            _ => {
                let empty = Store::open(dir.path()).unwrap();
                drop(empty);
            }
        }
        std::fs::set_permissions(&db_path, std::fs::Permissions::from_mode(0o600)).unwrap();
        let snapshot = |path: &Path| {
            let mut entries = std::fs::read_dir(path)
                .unwrap()
                .map(|e| {
                    let path = e.unwrap().path();
                    (
                        path.file_name().unwrap().to_owned(),
                        Sha256::digest(std::fs::read(&path).unwrap()).to_vec(),
                    )
                })
                .collect::<Vec<_>>();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            entries
        };
        let before = snapshot(dir.path());
        assert_eq!(
            recover_account(
                dir.path(),
                Uuid::new_v4().to_string(),
                Role::Admin,
                RECOVERY_PASSWORD.into()
            )
            .await,
            Err(StoreError::NotFound)
        );
        assert_eq!(
            snapshot(dir.path()),
            before,
            "rejected fixture {fixture} changed installation files"
        );
        let db = Connection::open_with_flags(&db_path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
        let version: i64 = db
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            version,
            if fixture < 2 {
                0
            } else {
                CURRENT_SCHEMA_VERSION
            }
        );
    }
}

#[tokio::test]
async fn preflight_includes_committed_wal_after_process_exit_without_database_drop() {
    let _guard = TEST_LOCK.lock().await;
    if let Some(root) = std::env::var_os("MOOSURI_TEST_RECOVERY_PREFLIGHT_ROOT") {
        let root = PathBuf::from(root);
        let store = Store::open(&root.join("data")).unwrap();
        let user = admin(&store).await;
        gap(&store, &user.user.account.id).await;
        std::fs::write(root.join("synthetic-account-id"), user.user.account.id).unwrap();
        // Test-only child exits without Rust drops, leaving committed WAL pages.
        // No production environment hook or external operation is involved.
        std::process::exit(0);
    }
    let root = directory();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "recovery::tests::preflight_includes_committed_wal_after_process_exit_without_database_drop", "--nocapture"])
        .env("MOOSURI_TEST_RECOVERY_PREFLIGHT_ROOT", root.path())
        .output().unwrap();
    assert!(output.status.success(), "synthetic preflight child failed");
    let data = root.path().join("data");
    let header = std::fs::read(data.join("assistant.sqlite")).unwrap();
    assert_eq!(
        u32::from_be_bytes(header[60..64].try_into().unwrap()),
        0,
        "fixture must keep initialization in WAL, so immutable mode would be wrong"
    );
    assert!(
        std::fs::metadata(data.join("assistant.sqlite-wal"))
            .unwrap()
            .len()
            > 32
    );
    let account = std::fs::read_to_string(root.path().join("synthetic-account-id")).unwrap();
    recover_account(&data, account, Role::Member, RECOVERY_PASSWORD.into())
        .await
        .unwrap();
    let store = Store::open(&data).unwrap();
    let login = store
        .login("admin@example.test".into(), RECOVERY_PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(login.user.account.role, Role::Member);
    assert!(login.user.password_change_required());
    assert!(matches!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await,
        Err(StoreError::Unauthorized)
    ));
}
