use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::{AccountManagementAction, AccountManagementInput, ManagedAccount};
fn input(target: &ManagedAccount, password: Option<String>) -> InvitationPreviewInput {
    InvitationPreviewInput {
        request_id: Uuid::new_v4().to_string(),
        account_id: target.user.id.clone(),
        expected_revision: target.revision,
        temporary_password: password,
    }
}
async fn preview(s: &Store, u: &UserContext, i: InvitationPreviewInput) -> InvitationDelivery {
    s.preview_invitation(
        u,
        i,
        "system@example.test".into(),
        "https://assistant.example.test".into(),
        "synthetic-sender-pin".into(),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn invitation_full_preview_encrypted_snapshot_exact_approval_and_no_repeat_after_unknown() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await;
    let created = s
        .create_user(
            &a.user,
            "Synthetic Invitee".into(),
            "invitee@example.test".into(),
        )
        .await
        .unwrap();
    let request = input(
        &created.management,
        Some(created.temporary_password.clone()),
    );
    let p = preview(&s, &a.user, request.clone()).await;
    assert_eq!(p.state, "delivery_pending");
    let text = p.text.as_ref().unwrap();
    assert!(
        text.contains("Synthetic Invitee")
            && text.contains("invitee@example.test")
            && text.contains("https://assistant.example.test/app")
            && text.contains("expires at UTC")
            && text.contains(&created.temporary_password)
    );
    assert_eq!(preview(&s, &a.user, request).await.id, p.id);
    let password = created.temporary_password.clone();
    let id = p.id.clone();
    s.call(move |db| {
        let cipher: Vec<u8> = db.conn.query_row(
            "SELECT payload FROM invitation_deliveries WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        assert!(
            !cipher
                .windows(password.len())
                .any(|v| v == password.as_bytes())
        );
        Ok(())
    })
    .await
    .unwrap();
    assert!(
        s.commit_invitation(
            &a.user,
            p.id.clone(),
            "wrong-digest".into(),
            "synthetic-sender-pin".into()
        )
        .await
        .is_err()
    );
    let d = s
        .commit_invitation(
            &a.user,
            p.id.clone(),
            p.digest.clone(),
            "synthetic-sender-pin".into(),
        )
        .await
        .unwrap()
        .unwrap();
    assert!(s.validate_invitation_dispatch(&d).await.is_ok());
    assert!(
        s.commit_invitation(
            &a.user,
            p.id.clone(),
            p.digest.clone(),
            "synthetic-sender-pin".into()
        )
        .await
        .unwrap()
        .is_none()
    );
    s.finish_invitation(&d, None, false).await.unwrap();
    assert!(
        s.commit_invitation(
            &a.user,
            p.id.clone(),
            p.digest.clone(),
            "synthetic-sender-pin".into()
        )
        .await
        .unwrap()
        .is_none()
    );
    let state = s.invitation_deliveries(&a.user).await.unwrap().remove(0);
    assert_eq!(state.state, "delivery_unknown");
    assert!(state.text.is_none());
    s.finish_invitation(&d, Some("synthetic-late-receipt".into()), false)
        .await
        .unwrap();
    assert_eq!(
        s.invitation_deliveries(&a.user).await.unwrap()[0].state,
        "sent"
    );
    assert!(
        s.managed_accounts(&a.user)
            .await
            .unwrap()
            .iter()
            .any(|m| m.user.id == created.user.id)
    );
    // A late receipt must preserve completion evidence without restoring erased
    // invitation metadata after target account deletion.
    let second = s
        .create_user(
            &a.user,
            "Deleted Invitee".into(),
            "deleted-invitee@example.test".into(),
        )
        .await
        .unwrap();
    let preview = preview(&s, &a.user, input(&second.management, None)).await;
    let dispatch = s
        .commit_invitation(
            &a.user,
            preview.id.clone(),
            preview.digest,
            "synthetic-sender-pin".into(),
        )
        .await
        .unwrap()
        .unwrap();
    let deletion = s
        .prepare_account_management(
            &a.user,
            second.user.id,
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: second.management.revision,
                action: AccountManagementAction::Delete,
            },
        )
        .await
        .unwrap();
    s.apply_account_management(deletion).await.unwrap();
    s.finish_invitation(
        &dispatch,
        Some("private-provider-reference-after-delete".into()),
        false,
    )
    .await
    .unwrap();
    let erased = s.invitation_delivery(&a.user, preview.id).await.unwrap();
    assert_eq!(erased.state, "sent");
    assert!(erased.provider_message_id.is_none() && erased.text.is_none());
    assert!(erased.to.is_empty() && erased.subject.is_empty());
}
#[tokio::test]
async fn invitation_actor_session_scope_password_check_reset_delete_and_expiry_fence_sends() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await;
    let b = member(&s, &a.user, "member@example.test").await;
    for mode in ["reset", "delete", "expiry", "session", "sender"] {
        let created = s
            .create_user(
                &a.user,
                "Synthetic".into(),
                format!("invite-{mode}@example.test"),
            )
            .await
            .unwrap();
        let request = input(
            &created.management,
            Some(created.temporary_password.clone()),
        );
        assert!(
            s.preview_invitation(
                &b.user,
                request.clone(),
                "system@example.test".into(),
                "https://assistant.example.test".into(),
                "synthetic-sender-pin".into()
            )
            .await
            .is_err()
        );
        let mut wrong = request.clone();
        wrong.temporary_password = Some("not-the-current-password".into());
        assert!(
            s.preview_invitation(
                &a.user,
                wrong,
                "system@example.test".into(),
                "https://assistant.example.test".into(),
                "synthetic-sender-pin".into()
            )
            .await
            .is_err()
        );
        let p = preview(&s, &a.user, request).await;
        if mode == "reset" || mode == "delete" {
            let prepared = s
                .prepare_account_management(
                    &a.user,
                    created.user.id.clone(),
                    AccountManagementInput {
                        request_id: Uuid::new_v4().to_string(),
                        expected_revision: created.management.revision,
                        action: if mode == "reset" {
                            AccountManagementAction::ResetPassword
                        } else {
                            AccountManagementAction::Delete
                        },
                    },
                )
                .await
                .unwrap();
            s.apply_account_management(prepared).await.unwrap();
        } else if mode == "expiry" {
            let id = p.id.clone();
            s.call(move |db| {
                db.conn.execute(
                    "UPDATE invitation_deliveries SET expires_at=0 WHERE id=?1",
                    [id],
                )?;
                Ok(())
            })
            .await
            .unwrap();
        }
        let user = if mode == "session" {
            s.login("admin@example.test".into(), crate::tests::PASSWORD.into())
                .await
                .unwrap()
                .user
        } else {
            a.user.clone()
        };
        let pin = if mode == "sender" {
            "changed-sender"
        } else {
            "synthetic-sender-pin"
        };
        assert!(
            s.commit_invitation(&user, p.id.clone(), p.digest, pin.into())
                .await
                .is_err(),
            "mode {mode}"
        );
        if mode == "reset" || mode == "delete" || mode == "expiry" {
            let v = s
                .invitation_deliveries(&a.user)
                .await
                .unwrap()
                .into_iter()
                .find(|v| v.id == p.id)
                .unwrap();
            assert!(v.text.is_none());
            assert_eq!(v.state, "expired");
        }
    }
}
#[tokio::test]
async fn invitation_restart_keeps_unknown_without_payload_or_send_authority() {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await;
    let created = s
        .create_user(&a.user, "Synthetic".into(), "restart@example.test".into())
        .await
        .unwrap();
    let p = preview(&s, &a.user, input(&created.management, None)).await;
    let d = s
        .commit_invitation(
            &a.user,
            p.id.clone(),
            p.digest.clone(),
            "synthetic-sender-pin".into(),
        )
        .await
        .unwrap()
        .unwrap();
    drop(d);
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let v = s.invitation_deliveries(&a.user).await.unwrap().remove(0);
    assert_eq!(v.state, "delivery_unknown");
    assert!(v.text.is_none());
    assert!(
        s.commit_invitation(&a.user, p.id, p.digest, "synthetic-sender-pin".into())
            .await
            .unwrap()
            .is_none()
    );
}
