use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};

fn input(revision: i64, action: AccountManagementAction) -> AccountManagementInput {
    AccountManagementInput {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: revision,
        action,
    }
}
async fn current(store: &Store, admin: &UserContext, id: &str) -> ManagedAccount {
    store
        .managed_accounts(admin)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == id)
        .unwrap()
}
async fn apply(
    store: &Store,
    admin: &UserContext,
    id: &str,
    input: AccountManagementInput,
) -> Result<AccountManagementResult> {
    let p = store
        .prepare_account_management(admin, id.into(), input)
        .await?;
    store.apply_account_management(p).await
}

#[tokio::test]
async fn suspend_resume_fences_old_sessions_without_touching_other_accounts_or_content() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "b@example.test").await;
    let thread = store
        .create_thread(&b.user, "Synthetic private retained thread".into())
        .await
        .unwrap();
    let id = b.user.account.id.clone();
    let revision = current(&store, &a.user, &id).await.revision;
    let request = input(revision, AccountManagementAction::Suspend);
    let result = apply(&store, &a.user, &id, request.clone()).await.unwrap();
    assert_eq!(
        current(&store, &a.user, &id).await.status,
        AccountStatus::Suspended
    );
    assert!(matches!(
        store.authenticate(b.token.clone()).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.login("b@example.test".into(), PASSWORD.into()).await,
        Err(StoreError::Unauthorized)
    ));
    assert_eq!(store.managed_accounts(&a.user).await.unwrap().len(), 2);
    assert_eq!(
        apply(&store, &a.user, &id, request.clone())
            .await
            .unwrap()
            .receipt
            .revision,
        result.receipt.revision
    );
    let mut changed = request;
    changed.action = AccountManagementAction::Resume;
    assert!(matches!(
        apply(&store, &a.user, &id, changed).await,
        Err(StoreError::Conflict)
    ));
    apply(
        &store,
        &a.user,
        &id,
        input(result.receipt.revision, AccountManagementAction::Resume),
    )
    .await
    .unwrap();
    assert!(matches!(
        store.authenticate(b.token).await,
        Err(StoreError::Unauthorized)
    ));
    let b = store
        .login("b@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(store.list_threads(&b.user).await.unwrap()[0].id, thread.id);
    assert!(matches!(
        store.delete_thread(&a.user, thread.id).await,
        Err(StoreError::NotFound)
    ));
    let token = b.token;
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.authenticate(token).await.is_ok());
}

#[tokio::test]
async fn password_reissue_is_one_time_idempotent_and_keeps_suspension() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "b@example.test").await;
    let id = b.user.account.id.clone();
    let revision = current(&store, &a.user, &id).await.revision;
    let reset = input(revision, AccountManagementAction::ResetPassword);
    let p1 = store
        .prepare_account_management(&a.user, id.clone(), reset.clone())
        .await
        .unwrap();
    let p2 = store
        .prepare_account_management(&a.user, id.clone(), reset.clone())
        .await
        .unwrap();
    let first = store.apply_account_management(p1).await.unwrap();
    let second = store.apply_account_management(p2).await.unwrap();
    assert!(second.temporary_password.is_none());
    assert_eq!(first.receipt.revision, second.receipt.revision);
    assert!(
        apply(&store, &a.user, &id, reset)
            .await
            .unwrap()
            .temporary_password
            .is_none()
    );
    assert!(matches!(
        store.authenticate(b.token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.login("b@example.test".into(), PASSWORD.into()).await,
        Err(StoreError::Unauthorized)
    ));
    let secret = first.temporary_password.unwrap();
    let restricted = store
        .login("b@example.test".into(), secret.to_string())
        .await
        .unwrap();
    assert!(restricted.user.password_change_required());
    assert!(matches!(
        store
            .create_thread(&restricted.user, "Cannot use".into())
            .await,
        Err(StoreError::Forbidden)
    ));
    let account = current(&store, &a.user, &id).await;
    assert!(
        (now().unwrap() + TEMP_SECONDS - 30..=now().unwrap() + TEMP_SECONDS)
            .contains(&account.temporary_expires_at.unwrap())
    );
    let revision = apply(
        &store,
        &a.user,
        &id,
        input(account.revision, AccountManagementAction::Suspend),
    )
    .await
    .unwrap()
    .receipt
    .revision;
    let reset = apply(
        &store,
        &a.user,
        &id,
        input(revision, AccountManagementAction::ResetPassword),
    )
    .await
    .unwrap();
    assert_eq!(
        current(&store, &a.user, &id).await.status,
        AccountStatus::Suspended
    );
    assert!(matches!(
        store
            .login(
                "b@example.test".into(),
                reset.temporary_password.unwrap().to_string()
            )
            .await,
        Err(StoreError::Unauthorized)
    ));
    let secret_copy = secret.to_string();
    store
        .call(move |db| {
            let mut stmt = db
                .conn
                .prepare("SELECT request_digest FROM account_management_receipts")?;
            assert_eq!(
                stmt.query_map([], |r| r.get::<_, Vec<u8>>(0))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
                    .len(),
                3
            );
            let stored: String = db.conn.query_row(
                "SELECT group_concat(password_hash,' ') FROM accounts",
                [],
                |r| r.get(0),
            )?;
            assert!(!stored.contains(&secret_copy));
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn role_change_revokes_prepared_admin_authority_and_self_lockout_is_rejected() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "b@example.test").await;
    let c = member(&store, &a.user, "c@example.test").await;
    let id = b.user.account.id.clone();
    let rev = current(&store, &a.user, &id).await.revision;
    assert!(matches!(
        store.managed_accounts(&b.user).await,
        Err(StoreError::Forbidden)
    ));
    apply(
        &store,
        &a.user,
        &id,
        input(
            rev,
            AccountManagementAction::ChangeRole { role: Role::Admin },
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        store.managed_accounts(&b.user).await,
        Err(StoreError::Unauthorized)
    ));
    let b = store
        .login("b@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(b.user.account.role, Role::Admin);
    let c_before = current(&store, &b.user, &c.user.account.id).await;
    let pending = store
        .prepare_account_management(
            &b.user,
            c.user.account.id.clone(),
            input(c_before.revision, AccountManagementAction::ResetPassword),
        )
        .await
        .unwrap();
    let rev = current(&store, &a.user, &id).await.revision;
    apply(
        &store,
        &a.user,
        &id,
        input(
            rev,
            AccountManagementAction::ChangeRole { role: Role::Member },
        ),
    )
    .await
    .unwrap();
    assert!(matches!(
        store.apply_account_management(pending).await,
        Err(StoreError::Unauthorized)
    ));
    assert_eq!(
        current(&store, &a.user, &c.user.account.id).await.revision,
        c_before.revision
    );
    let self_rev = current(&store, &a.user, &a.user.account.id).await.revision;
    for action in [
        AccountManagementAction::Suspend,
        AccountManagementAction::ResetPassword,
        AccountManagementAction::ChangeRole { role: Role::Member },
    ] {
        assert!(matches!(
            apply(&store, &a.user, &a.user.account.id, input(self_rev, action)).await,
            Err(StoreError::InvalidInput)
        ));
    }
    apply(
        &store,
        &a.user,
        &a.user.account.id,
        input(self_rev, AccountManagementAction::EndSessions),
    )
    .await
    .unwrap();
    assert!(matches!(
        store.authenticate(a.token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(
        store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .is_ok()
    );
    assert!(store.authenticate(c.token).await.is_ok());
}

#[tokio::test]
async fn concurrent_password_change_fences_prepared_reset_without_new_epoch_adoption() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "b@example.test").await;
    let id = b.user.account.id.clone();
    let rev = current(&store, &a.user, &id).await.revision;
    let pending = store
        .prepare_account_management(
            &a.user,
            id.clone(),
            input(rev, AccountManagementAction::ResetPassword),
        )
        .await
        .unwrap();
    let b = store
        .change_password(&b.user, "Synthetic independently changed password".into())
        .await
        .unwrap();
    let count = std::fs::metadata(dir.path().join("auth-epochs.journal"))
        .unwrap()
        .len();
    assert!(matches!(
        store.apply_account_management(pending).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        std::fs::metadata(dir.path().join("auth-epochs.journal"))
            .unwrap()
            .len(),
        count
    );
    assert!(store.authenticate(b.token).await.is_ok());
}

#[tokio::test]
async fn post_journal_transaction_failure_reopens_locked_without_replaying_a_password() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "b@example.test").await;
    let id = b.user.account.id.clone();
    let rev = current(&store, &a.user, &id).await.revision;
    let p = store
        .prepare_account_management(
            &a.user,
            id.clone(),
            input(rev, AccountManagementAction::Suspend),
        )
        .await
        .unwrap();
    store.call(|db| {db.conn.execute_batch("CREATE TEMP TRIGGER fail_management BEFORE INSERT ON account_management_receipts BEGIN SELECT RAISE(ABORT,'synthetic failure'); END;")?;Ok(())}).await.unwrap();
    assert!(store.apply_account_management(p).await.is_err());
    assert!(matches!(
        store.managed_accounts(&a.user).await,
        Err(StoreError::Unavailable)
    ));
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(matches!(
        store.authenticate(b.token).await,
        Err(StoreError::Unauthorized)
    ));
    assert!(matches!(
        store.login("b@example.test".into(), PASSWORD.into()).await,
        Err(StoreError::Unauthorized)
    ));
    let a = store.authenticate(a.token).await.unwrap();
    let target = current(&store, &a, &id).await;
    assert!(target.recovery_required);
    assert!(matches!(
        apply(
            &store,
            &a,
            &id,
            input(target.revision, AccountManagementAction::Resume)
        )
        .await,
        Err(StoreError::Conflict)
    ));
    assert!(store.list_threads(&a).await.is_ok());
}

#[tokio::test]
async fn permanent_deletion_purges_only_owned_content_and_reports_exact_vault_ack() {
    use moosuri_vault_protocol::AuthorityRequest;
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "delete-owner@example.test").await.user;
    store
        .create_thread_with_request_id(
            &b,
            "Synthetic deleted private text".into(),
            Some(Uuid::new_v4().to_string()),
        )
        .await
        .unwrap();
    let retained = store
        .create_thread_with_request_id(
            &a,
            "Synthetic retained private text".into(),
            Some(Uuid::new_v4().to_string()),
        )
        .await
        .unwrap();
    let (b_seed, a_seed) = (b.clone(), a.clone());
    store.call(move |db| {
        for u in [&b_seed,&a_seed] {
            db.conn.execute("INSERT INTO media_objects(tenant_id,account_id,id,state,auth_epoch,created_at,bytes,extracted_text) VALUES(?1,?2,?3,'ready',?4,0,?5,'Synthetic private file text')",params![u.tenant.0,u.account.id,Uuid::new_v4().to_string(),u.epoch,b"synthetic-private-file".as_slice()])?;
            db.conn.execute("INSERT INTO mail_connections(tenant_id,account_id,id,email,status,credential,expires_at,scopes) VALUES(?1,?2,?3,'synthetic@example.test','active',?4,0,'synthetic')",params![u.tenant.0,u.account.id,Uuid::new_v4().to_string(),b"synthetic-encrypted-token".as_slice()])?;
            db.conn.execute("INSERT INTO knowledge_fts(tenant_key,kind,object_id,revision,normalized) VALUES(?1,'source',?2,1,'syntheticprivatecanary')",params![format!("t{}",u.tenant.0.replace('-',"")),Uuid::new_v4().to_string()])?;
        }
        db.conn.execute("INSERT INTO mail_actions(tenant_id,account_id,id,revision,connection_id,connection_epoch,auth_epoch,content,raw,digest,message_id,state,expires_at) VALUES(?1,?2,?3,1,?4,1,?5,'Synthetic private outgoing body',?6,'synthetic-digest','private-message-id','outcome_unknown',0)",params![b_seed.tenant.0,b_seed.account.id,Uuid::new_v4().to_string(),Uuid::new_v4().to_string(),b_seed.epoch,b"synthetic-private-mime".as_slice()])?;
        db.conn.execute("INSERT INTO model_call_receipts(tenant_id,account_id,run_id,attempt,nonce,usage,dispatched_at) VALUES(?1,?2,?3,1,zeroblob(32),'{}',0)",params![b_seed.tenant.0,b_seed.account.id,Uuid::new_v4().to_string()])?;
        Ok(())
    }).await.unwrap();
    let req = input(
        current(&store, &a, &b.account.id).await.revision,
        AccountManagementAction::Delete,
    );
    assert!(matches!(
        apply(
            &store,
            &a,
            &a.account.id,
            input(1, AccountManagementAction::Delete)
        )
        .await,
        Err(StoreError::InvalidInput)
    ));
    assert_eq!(
        apply(&store, &a, &b.account.id, req.clone())
            .await
            .unwrap()
            .receipt
            .revision,
        3
    );
    assert!(store.create_thread(&b, "late result".into()).await.is_err());
    assert!(
        store
            .login("delete-owner@example.test".into(), PASSWORD.into())
            .await
            .is_err()
    );
    assert!(
        store
            .list_threads(&a)
            .await
            .unwrap()
            .iter()
            .any(|t| t.id == retained.id)
    );
    let deleted = current(&store, &a, &b.account.id).await;
    assert_eq!(deleted.user.name, "Deleted account");
    let status = deleted.deletion.unwrap();
    assert!(status.local_complete);
    assert!(!status.vault_acknowledged);
    let owner = b.clone();
    store
        .call(move |db| {
            for table in [
                "threads",
                "thread_creation_receipts",
                "media_objects",
                "mail_connections",
            ] {
                assert_eq!(
                    db.conn.query_row(
                        &format!("SELECT count(*) FROM {table} WHERE tenant_id=?1"),
                        [&owner.tenant.0],
                        |r| r.get::<_, i64>(0)
                    )?,
                    0
                );
            }
            assert_eq!(
                db.conn.query_row(
                    "SELECT state FROM deleted_account_effects WHERE account_id=?1 AND kind='mail'",
                    [&owner.account.id],
                    |r| r.get::<_, String>(0)
                )?,
                "outcome_unknown"
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM model_call_receipts WHERE tenant_id=?1",
                    [&owner.tenant.0],
                    |r| r.get::<_, i64>(0)
                )?,
                1
            );
            assert!(!db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?);
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM thread_creation_receipts", [], |r| r
                        .get::<_, i64>(
                        0
                    ))?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
    let delivery = store
        .vault_authority(AuthorityRequest::LifecycleNext)
        .await
        .unwrap();
    assert_eq!(delivery.owner.account.to_string(), b.account.id);
    assert_eq!(delivery.digest.as_deref(), Some("delete"));
    let event = delivery.operation.unwrap();
    for _ in 0..2 {
        store
            .vault_authority(AuthorityRequest::LifecycleAck { event })
            .await
            .unwrap();
    }
    assert!(
        current(&store, &a, &b.account.id)
            .await
            .deletion
            .unwrap()
            .vault_acknowledged
    );
    assert_eq!(
        apply(&store, &a, &b.account.id, req)
            .await
            .unwrap()
            .receipt
            .revision,
        3
    );
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let a = reopened
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        current(&reopened, &a, &b.account.id)
            .await
            .deletion
            .unwrap()
            .local_complete
    );
}

#[tokio::test]
async fn deletion_journal_crash_gap_and_old_snapshot_never_resurrect_content_or_credentials() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "gap-delete@example.test").await.user;
    store
        .create_thread(&b, "Synthetic crash-gap private content".into())
        .await
        .unwrap();
    let snapshot = directory();
    let saved = snapshot.path().join("snapshot.sqlite");
    let save = saved.clone();
    store
        .call(move |db| {
            let mut dest = Connection::open(save)?;
            rusqlite::backup::Backup::new(&db.conn, &mut dest)?.run_to_completion(
                128,
                std::time::Duration::ZERO,
                None,
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let owner = b.clone();
    store
        .call(move |db| {
            db.journal
                .append_delete(&owner.account.id, owner.epoch + 1)?;
            db.poisoned = true;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let aa = reopened
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        current(&reopened, &aa, &b.account.id)
            .await
            .deletion
            .unwrap()
            .local_complete
    );
    drop(reopened);
    std::fs::copy(saved, dir.path().join("assistant.sqlite")).unwrap();
    let restored = Store::open(dir.path()).unwrap();
    assert!(
        restored
            .login("gap-delete@example.test".into(), PASSWORD.into())
            .await
            .is_err()
    );
    let aa = restored
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(
        current(&restored, &aa, &b.account.id)
            .await
            .deletion
            .unwrap()
            .local_complete
    );
    let id = b.account.id.clone();
    drop(restored);
    assert!(matches!(
        crate::recovery::recover_account(
            dir.path(),
            id,
            Role::Admin,
            "Synthetic-new-recovery-password-2026".into()
        )
        .await,
        Err(StoreError::NotFound)
    ));
}

#[tokio::test]
async fn cleanup_failure_is_pending_while_access_stays_revoked_and_exact_retry_finishes() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "pending-delete@example.test").await.user;
    store
        .create_thread(&b, "synthetic cleanup interruption".into())
        .await
        .unwrap();
    store.call(|db| {db.conn.execute_batch("CREATE TEMP TRIGGER fail_cleanup BEFORE DELETE ON threads BEGIN SELECT RAISE(ABORT,'synthetic interruption'); END;")?;Ok(())}).await.unwrap();
    let req = input(
        current(&store, &a, &b.account.id).await.revision,
        AccountManagementAction::Delete,
    );
    apply(&store, &a, &b.account.id, req.clone()).await.unwrap();
    assert!(
        !current(&store, &a, &b.account.id)
            .await
            .deletion
            .unwrap()
            .local_complete
    );
    assert!(store.list_threads(&b).await.is_err());
    store
        .call(|db| {
            db.conn.execute_batch("DROP TRIGGER fail_cleanup;")?;
            Ok(())
        })
        .await
        .unwrap();
    apply(&store, &a, &b.account.id, req).await.unwrap();
    assert!(
        current(&store, &a, &b.account.id)
            .await
            .deletion
            .unwrap()
            .local_complete
    );
}
