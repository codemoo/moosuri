use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
use moosuri_core::{AccountManagementAction, AccountManagementInput};
async fn manage(
    store: &Store,
    actor: &UserContext,
    id: &str,
    action: AccountManagementAction,
) -> AccountManagementResult {
    let revision = store
        .managed_accounts(actor)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == id)
        .unwrap()
        .revision;
    let prepared = store
        .prepare_account_management(
            actor,
            id.into(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: revision,
                action,
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap()
}
#[tokio::test]
async fn share_origin_natural_expiry_survives_but_targeted_logout_and_password_change_do_not() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let old = admin(&store).await;
    let origin = old.user.chat_draft_scope().unwrap();
    let digest = old.user.digest;
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE sessions SET expires_at=0 WHERE digest=?1",
                [digest.as_slice()],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(matches!(
        store.authenticate(old.token).await,
        Err(StoreError::Unauthorized)
    ));
    let current = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store
            .validate_share_origin(&current.user, origin.clone())
            .await,
        Ok(())
    );
    // No renewal of the original expired session is performed by the proof.
    store
        .call(move |db| {
            let expires: i64 = db.conn.query_row(
                "SELECT expires_at FROM sessions WHERE digest=?1",
                [digest.as_slice()],
                |r| r.get(0),
            )?;
            assert_eq!(expires, 0);
            Ok(())
        })
        .await
        .unwrap();
    let logout_origin = current.user.chat_draft_scope().unwrap();
    store.logout(&current.user).await.unwrap();
    let next = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&next.user, logout_origin).await,
        Err(StoreError::Forbidden)
    );
    let changed = store
        .change_password(&next.user, "Synthetic-changed-password-42".into())
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&changed.user, origin).await,
        Err(StoreError::Forbidden)
    );
    assert_eq!(
        store
            .validate_share_origin(&next.user, changed.user.chat_draft_scope().unwrap())
            .await,
        Err(StoreError::Unauthorized)
    );
}
#[tokio::test]
async fn share_origin_admin_revoke_reset_suspend_resume_and_cross_owner_are_closed() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    let b = member(&store, &a.user, "b@example.test").await;
    let id = b.user.account.id.clone();
    let origin = b.user.chat_draft_scope().unwrap();
    assert_eq!(
        store.validate_share_origin(&a.user, origin.clone()).await,
        Err(StoreError::Forbidden)
    );
    manage(&store, &a.user, &id, AccountManagementAction::EndSessions).await;
    let b = store
        .login("b@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&b.user, origin).await,
        Err(StoreError::Forbidden)
    );
    let origin = b.user.chat_draft_scope().unwrap();
    manage(&store, &a.user, &id, AccountManagementAction::Suspend).await;
    assert_eq!(
        store.validate_share_origin(&b.user, origin.clone()).await,
        Err(StoreError::Unauthorized)
    );
    manage(&store, &a.user, &id, AccountManagementAction::Resume).await;
    let b = store
        .login("b@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&b.user, origin).await,
        Err(StoreError::Forbidden)
    );
    let origin = b.user.chat_draft_scope().unwrap();
    let reset = manage(&store, &a.user, &id, AccountManagementAction::ResetPassword).await;
    let restricted = store
        .login(
            "b@example.test".into(),
            reset.temporary_password.unwrap().to_string(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .validate_share_origin(&restricted.user, origin.clone())
            .await,
        Err(StoreError::Forbidden)
    );
    let b = store
        .change_password(&restricted.user, PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&b.user, origin).await,
        Err(StoreError::Forbidden)
    );
}
#[tokio::test]
async fn share_origin_epoch_recovery_origin_restriction_corruption_and_scan_bound_fail_closed() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await;
    for origin in ["".into(), "a".repeat(63), "G".repeat(64), "a".repeat(65)] {
        assert_eq!(
            store.validate_share_origin(&a.user, origin).await,
            Err(StoreError::InvalidInput)
        );
    }
    assert_eq!(
        store.validate_share_origin(&a.user, "0".repeat(64)).await,
        Err(StoreError::Forbidden)
    );
    let origin = a.user.chat_draft_scope().unwrap();
    let account = a.user.account.id.clone();
    let tenant = a.user.tenant.0.clone();
    let epoch = a.user.epoch;
    store
        .call(move |db| {
            // Foreign-epoch and restricted rows cannot act as an origin.
            let digest = [23u8; 32];
            db.conn.execute(
                "INSERT INTO sessions VALUES(?1,?2,?3,?4,0,0)",
                params![digest.as_slice(), tenant, account, epoch - 1],
            )?;
            let digest = [24u8; 32];
            db.conn.execute(
                "INSERT INTO sessions VALUES(?1,?2,?3,?4,1,0)",
                params![digest.as_slice(), tenant, account, epoch],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        store
            .validate_share_origin(
                &a.user,
                draft_scope(&[23u8; 32], &a.user.account.id, a.user.epoch - 1)
            )
            .await,
        Err(StoreError::Forbidden)
    );
    assert_eq!(
        store
            .validate_share_origin(
                &a.user,
                draft_scope(&[24u8; 32], &a.user.account.id, a.user.epoch)
            )
            .await,
        Err(StoreError::Forbidden)
    );
    let account = a.user.account.id.clone();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE accounts SET recovery_required=1 WHERE id=?1",
                [account],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&a.user, origin.clone()).await,
        Err(StoreError::Unauthorized)
    );
    let account = a.user.account.id.clone();
    let tenant = a.user.tenant.0.clone();
    let epoch = a.user.epoch;
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE accounts SET recovery_required=0 WHERE id=?1",
                [&account],
            )?;
            for n in 0..MAX_ORIGIN_SESSIONS {
                let mut digest = [0u8; 32];
                digest[..8].copy_from_slice(&(n as u64).to_be_bytes());
                db.conn.execute(
                    "INSERT INTO sessions VALUES(?1,?2,?3,?4,0,0)",
                    params![digest.as_slice(), tenant, account, epoch],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&a.user, origin).await,
        Err(StoreError::Forbidden)
    );
}

#[tokio::test]
async fn share_origin_actual_journal_recovery_and_new_login_never_revive_prior_namespace() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let old = admin(&store).await;
    let origin = old.user.chat_draft_scope().unwrap();
    let id = old.user.account.id.clone();
    let account = id.clone();
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
    assert_eq!(
        store.validate_share_origin(&old.user, origin.clone()).await,
        Err(StoreError::Unauthorized)
    );
    drop(store);
    recover_account(
        dir.path(),
        id,
        Role::Admin,
        "Synthetic-recovery-temporary-42".into(),
    )
    .await
    .unwrap();
    let store = Store::open(dir.path()).unwrap();
    let restricted = store
        .login(
            "admin@example.test".into(),
            "Synthetic-recovery-temporary-42".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        store
            .validate_share_origin(&restricted.user, origin.clone())
            .await,
        Err(StoreError::Forbidden)
    );
    let next = store
        .change_password(
            &restricted.user,
            "Synthetic-recovered-new-password-42".into(),
        )
        .await
        .unwrap();
    assert_eq!(
        store.validate_share_origin(&next.user, origin).await,
        Err(StoreError::Forbidden)
    );
}

#[test]
fn share_origin_preserves_existing_chat_namespace_hash_contract() {
    assert_eq!(
        draft_scope(&[1u8; 32], "synthetic-account", 7),
        "3c5a1e452862a89f877b98f27d09dd9876ff0640852059f3aa52ba8973ccd415"
    );
}
