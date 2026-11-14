use super::*;
use crate::tests::{TEST_LOCK, admin, member};
#[tokio::test]
async fn staged_tickets_owner_session_epoch_and_registration_retry_are_fenced() {
    let _g = TEST_LOCK.lock().await;
    let dir = tempfile::tempdir().unwrap();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await;
    let b = member(&s, &a.user, "other@example.test").await;
    let p = s
        .prepare_chatgpt_handoff(&a.user, None, "Synthetic VM".into())
        .await
        .unwrap();
    assert!(
        s.chatgpt_handoff_status(&b.user, p.context.ticket.clone())
            .await
            .is_err()
    );
    assert!(
        s.cancel_chatgpt_handoff(&b.user, p.context.ticket.clone())
            .await
            .is_err()
    );
    assert!(
        s.admit_chatgpt_handoff(&b.user, p.context.ticket.clone(), [1; 32])
            .await
            .is_err()
    );
    let second = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    assert!(
        s.commit_chatgpt_handoff_nonce(
            &second.user,
            p.context.ticket.clone(),
            vec![1; 32],
            "source".into()
        )
        .await
        .is_err()
    );
    s.commit_chatgpt_handoff_nonce(
        &a.user,
        p.context.ticket.clone(),
        vec![1; 32],
        "source".into(),
    )
    .await
    .unwrap();
    s.save_chatgpt_handoff_registration(
        &a.user,
        p.context.ticket.clone(),
        "issued-synthetic-client".into(),
    )
    .await
    .unwrap();
    s.cancel_chatgpt_handoff(&a.user, p.context.ticket.clone())
        .await
        .unwrap();
    let p2 = s
        .prepare_chatgpt_handoff(
            &a.user,
            Some((p.context.binding.clone(), 0)),
            "Retry".into(),
        )
        .await
        .unwrap();
    assert_eq!(p2.client_id.as_deref(), Some("issued-synthetic-client"));
    assert_eq!(p2.context.target_host, p.context.target_host);
    s.commit_chatgpt_handoff_nonce(
        &a.user,
        p2.context.ticket.clone(),
        vec![2; 32],
        "source".into(),
    )
    .await
    .unwrap();
    s.save_chatgpt_handoff_registration(
        &a.user,
        p2.context.ticket.clone(),
        "issued-synthetic-client".into(),
    )
    .await
    .unwrap();
    assert!(matches!(
        s.admit_chatgpt_handoff(&a.user, p2.context.ticket.clone(), [3; 32])
            .await
            .unwrap(),
        HandoffAdmission::Validate(_)
    ));
    assert!(
        s.admit_chatgpt_handoff(&a.user, p2.context.ticket.clone(), [3; 32])
            .await
            .is_err()
    );
    let ticket = p2.context.ticket.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE chatgpt_handoffs SET lease_until=0 WHERE id=?1",
            [ticket],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.admit_chatgpt_handoff(&a.user, p2.context.ticket.clone(), [4; 32])
            .await
            .is_err()
    );
    assert!(matches!(
        s.admit_chatgpt_handoff(&a.user, p2.context.ticket.clone(), [3; 32])
            .await
            .unwrap(),
        HandoffAdmission::Validate(_)
    ));
    s.logout(&a.user).await.unwrap();
    let phase = s
        .chatgpt_handoff_status(&second.user, p2.context.ticket.clone())
        .await;
    assert!(phase.is_err());
    let id = p2.context.ticket.clone();
    s.call(move |db| {
        let (phase, key): (String, Option<Vec<u8>>) = db.conn.query_row(
            "SELECT phase,transfer_key FROM chatgpt_handoffs WHERE id=?1",
            [id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(phase, "cancelled");
        assert!(key.is_none());
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn active_ticket_cap_and_existing_revision_are_not_mutated_by_failed_staging() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let mut tickets = Vec::new();
    for i in 0..4 {
        tickets.push(
            s.prepare_chatgpt_handoff(&a.user, None, format!("Synthetic {i}"))
                .await
                .unwrap(),
        );
    }
    assert!(
        s.prepare_chatgpt_handoff(&a.user, None, "Over cap".into())
            .await
            .is_err()
    );
    let p = &tickets[0];
    assert!(
        s.prepare_chatgpt_handoff(
            &a.user,
            Some((p.context.binding.clone(), 0)),
            "Duplicate".into()
        )
        .await
        .is_err()
    );
    assert_eq!(
        s.subscriptions(&a.user)
            .await
            .unwrap()
            .iter()
            .find(|b| b.id == p.context.binding)
            .unwrap()
            .revision,
        0
    );
    s.cancel_chatgpt_handoff(&a.user, p.context.ticket.clone())
        .await
        .unwrap();
    assert!(
        s.prepare_chatgpt_handoff(
            &a.user,
            Some((p.context.binding.clone(), 9)),
            "Stale revision".into()
        )
        .await
        .is_err()
    );
}

#[tokio::test]
async fn helper_issued_session_is_absolute_and_cannot_create_lasting_authority() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let admin = admin(&s).await;
    let member = member(&s, &admin.user, "helper-member@example.test").await;
    for email in ["admin@example.test", "helper-member@example.test"] {
        let helper = s
            .login_chatgpt_helper(email.into(), crate::tests::PASSWORD.into())
            .await
            .unwrap();
        assert!(helper.user.is_chatgpt_helper());
        assert!(helper.user.chat_draft_scope().is_none());
        assert!(matches!(
            s.change_password(&helper.user, "Synthetic-forbidden-password-42".into())
                .await,
            Err(StoreError::Forbidden)
        ));
        assert!(matches!(
            s.create_user(
                &helper.user,
                "forbidden".into(),
                "forbidden@example.test".into()
            )
            .await,
            Err(StoreError::Forbidden)
        ));
        assert!(matches!(
            s.create_thread(&helper.user, "forbidden".into()).await,
            Err(StoreError::Forbidden)
        ));
        // Even if internal memory were relabelled, persisted session purpose is
        // authoritative at the default operational loader.
        let mut relabelled = helper.user.clone();
        relabelled.helper = false;
        relabelled.absolute_deadline = None;
        assert!(matches!(
            s.create_thread(&relabelled, "forged label".into()).await,
            Err(StoreError::Forbidden)
        ));
        assert!(
            s.begin_subscription_login(&helper.user, None, "P1 forbidden".into())
                .await
                .is_err()
        );
        assert!(matches!(
            s.vault_handoff(&helper.user, "https://vault.example.test".into())
                .await,
            Err(StoreError::Forbidden)
        ));
        assert!(matches!(
            s.issue_source_token(&helper.user, "guess".into(), 3600)
                .await,
            Err(StoreError::Forbidden)
        ));
        assert!(s.source_connections(&helper.user).await.is_err());
        assert!(s.managed_accounts(&helper.user).await.is_err());
        let id = helper.user.digest;
        let deadline = helper.user.absolute_deadline.unwrap();
        s.authenticate(helper.token.clone()).await.unwrap();
        s.call(move|db|{let (purpose,cap,expiry):(String,i64,i64)=db.conn.query_row("SELECT purpose,absolute_deadline,expires_at FROM sessions WHERE digest=?1",[id.as_slice()],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;assert_eq!(purpose,"chatgpt_helper");assert_eq!(cap,deadline);assert_eq!(expiry,cap);assert!(db.conn.execute("UPDATE sessions SET absolute_deadline=absolute_deadline+3600 WHERE digest=?1",[id.as_slice()]).is_err());Ok(())}).await.unwrap();
        let p = s
            .prepare_chatgpt_handoff(&helper.user, None, "Allowed".into())
            .await
            .unwrap();
        s.cancel_chatgpt_handoff(&helper.user, p.context.ticket.clone())
            .await
            .unwrap();
        s.logout(&helper.user).await.unwrap();
        assert!(s.authenticate(helper.token).await.is_err());
    }
    assert!(
        s.create_thread(&member.user, "Normal member remains usable".into())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn helper_deadline_survives_undelivered_login_preprepare_and_recovery_crashes() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    // Drop the entire response: no caller cleanup is needed to establish purpose/cap.
    drop(
        s.login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
            .await
            .unwrap(),
    );
    let recovery = s
        .login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    // Recovery only reads a receipt/status and never prepares a new ticket.
    assert!(
        s.chatgpt_handoff_status(&recovery.user, "missing-ticket".into())
            .await
            .is_err()
    );
    let digest = recovery.user.digest;
    s.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM sessions WHERE purpose='chatgpt_helper' AND absolute_deadline IS NOT NULL AND expires_at=absolute_deadline",[],|r|r.get::<_,i64>(0))?,2);assert_eq!(db.conn.query_row("SELECT count(*) FROM chatgpt_handoffs",[],|r|r.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    s.authenticate(recovery.token.clone()).await.unwrap();
    s.call(move |db| {
        // Synthetic elapsed clock: immutable cap itself cannot be edited by runtime SQL.
        db.conn
            .execute_batch("DROP TRIGGER session_purpose_immutable;")?;
        db.conn.execute(
            "UPDATE sessions SET absolute_deadline=0,expires_at=9999999999 WHERE digest=?1",
            [digest.as_slice()],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.authenticate(recovery.token).await.is_err());
    assert!(
        s.authenticate(a.token).await.is_ok(),
        "ordinary browser session remains unaffected"
    );
    let fresh = s
        .login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    assert!(s.authenticate(fresh.token).await.is_ok());
}

#[tokio::test]
async fn p1_and_p2_share_monotonic_reservation_and_cas_fences() {
    let _g = TEST_LOCK.lock().await;
    let d = tempfile::tempdir().unwrap();
    let s = Store::open(d.path()).unwrap();
    let a = admin(&s).await;
    let p = s
        .prepare_chatgpt_handoff(&a.user, None, "Initial staged".into())
        .await
        .unwrap();
    s.commit_chatgpt_handoff_nonce(
        &a.user,
        p.context.ticket.clone(),
        vec![1; 32],
        "laptop".into(),
    )
    .await
    .unwrap();
    s.save_chatgpt_handoff_registration(
        &a.user,
        p.context.ticket.clone(),
        "issued-only-in-staging".into(),
    )
    .await
    .unwrap();
    s.cancel_chatgpt_handoff(&a.user, p.context.ticket.clone())
        .await
        .unwrap();
    let b = p.context.binding.clone();
    let (p1, p2) = tokio::join!(
        s.begin_subscription_login(&a.user, Some((b.clone(), 0)), "P1".into()),
        s.prepare_chatgpt_handoff(&a.user, Some((b.clone(), 0)), "P2".into())
    );
    let p1 = p1.unwrap();
    assert!(p1.binding.revision > p.context.staged_revision);
    assert_eq!(p1.issued.unwrap().client_id(), "issued-only-in-staging");
    if let Ok(p2) = p2 {
        assert!(p1.binding.revision > p2.context.staged_revision);
        assert!(
            s.commit_chatgpt_handoff_nonce(
                &a.user,
                p2.context.ticket.clone(),
                vec![2; 32],
                "laptop".into()
            )
            .await
            .is_err()
        );
    }
    let id = b.clone();
    s.call(move|db|{let duplicates:i64=db.conn.query_row("SELECT count(*) FROM (SELECT revision,count(*) AS n FROM chatgpt_binding_revisions WHERE binding_id=?1 GROUP BY revision HAVING n>1)",[id],|r|r.get(0))?;assert_eq!(duplicates,0);Ok(())}).await.unwrap();
}

#[tokio::test]
async fn abrupt_helper_issuance_preprepare_and_recovery_exit_keep_persisted_purpose_deadline() {
    const CHILD_ROOT: &str = "MOOSURI_SYNTHETIC_HELPER_ISSUANCE_ROOT";
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let s = Store::open(std::path::Path::new(&root)).unwrap();
        let a = admin(&s).await;
        let issued = s
            .login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
            .await
            .unwrap();
        let mode = std::env::var("MOOSURI_SYNTHETIC_HELPER_ISSUANCE_PHASE").unwrap();
        if mode != "issuance" {
            s.prepare_chatgpt_handoff(&issued.user, None, "Pre-crash".into())
                .await
                .unwrap();
        }
        if mode == "recovery" {
            let recovery = s
                .login_chatgpt_helper("admin@example.test".into(), crate::tests::PASSWORD.into())
                .await
                .unwrap();
            assert!(
                s.chatgpt_handoff_status(&recovery.user, "missing-ticket".into())
                    .await
                    .is_err()
            );
        }
        drop(a);
        std::process::exit(73);
    }
    let _g = TEST_LOCK.lock().await;
    for mode in ["issuance", "prepared", "recovery"] {
        let dir = tempfile::tempdir().unwrap();
        let output=std::process::Command::new(std::env::current_exe().unwrap()).args(["--exact","subscriptions::handoff_tests::abrupt_helper_issuance_preprepare_and_recovery_exit_keep_persisted_purpose_deadline","--nocapture"]).env(CHILD_ROOT,dir.path()).env("MOOSURI_SYNTHETIC_HELPER_ISSUANCE_PHASE",mode).output().unwrap();
        assert_eq!(
            output.status.code(),
            Some(73),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let s = Store::open(dir.path()).unwrap();
        let expected = if mode == "recovery" { 2 } else { 1 };
        s.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM sessions WHERE purpose='chatgpt_helper' AND absolute_deadline IS NOT NULL AND expires_at=absolute_deadline AND absolute_deadline<=?1",[now()?+600],|r|r.get::<_,i64>(0))?,expected);assert_eq!(db.conn.query_row("SELECT count(*) FROM sessions WHERE purpose='normal' AND absolute_deadline IS NULL",[],|r|r.get::<_,i64>(0))?,1);Ok(())}).await.unwrap();
    }
}
