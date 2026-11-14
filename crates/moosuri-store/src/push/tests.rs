use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_core::{ScheduleCreate, ScheduleDraft, ScheduleRule};
pub(super) fn enrollment(device: String) -> PushEnrollment {
    let (_, p256dh) = moosuri_connectors::push::generate_vapid().unwrap();
    PushEnrollment {
        operation_revision: 1,
        request_id: Uuid::new_v4().to_string(),
        device_id: device,
        subscription: PushSubscription {
            endpoint: format!("https://fcm.googleapis.com/fcm/send/{}", Uuid::new_v4()),
            p256dh,
            auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic Web Push fixture, not a live subscription
        },
    }
}
async fn enrolled(store: &Store, user: &UserContext) -> PushDeviceView {
    store
        .enroll_push(
            user,
            enrollment(Uuid::new_v4().to_string()),
            "https://push-owner.example.test".into(),
        )
        .await
        .unwrap()
}
pub(super) async fn notify(store: &Store, user: &UserContext) -> moosuri_core::LocalNotification {
    let at = now().unwrap() + 60;
    store
        .create_schedule(
            user,
            ScheduleCreate {
                request_id: Uuid::new_v4().to_string(),
                draft: ScheduleDraft {
                    title: "Synthetic private reminder canary".into(),
                    message: "Synthetic body must never enter a push payload".into(),
                    rule: ScheduleRule::Once { at },
                    valid_until: None,
                },
            },
        )
        .await
        .unwrap();
    store.enqueue_scheduled_reminders_at(at).await.unwrap();
    let lease = store
        .claim_scheduled_reminder_at(at)
        .await
        .unwrap()
        .unwrap();
    store
        .finish_scheduled_reminder_at(&lease, at)
        .await
        .unwrap()
}
async fn claim(store: &Store) -> PushClaim {
    store.poll_push_queue().await.unwrap();
    store.claim_push().await.unwrap().unwrap()
}
async fn state(store: &Store, lease: &PushLease) -> String {
    let lease = lease.clone();
    store
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT state FROM push_queue WHERE tenant_id=?1 AND id=?2",
                params![lease.tenant, lease.id],
                |r| r.get(0),
            )?)
        })
        .await
        .unwrap()
}
#[tokio::test]
async fn opt_in_starts_after_history_lost_ack_is_exact_and_keys_never_enter_dtos() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    assert!(!dir.path().join(KEY_FILE).exists());
    notify(&store, &user).await;
    let mut input = enrollment(Uuid::new_v4().to_string());
    let payload = json(&input).unwrap();
    let first = store
        .enroll_push(&user, input, "https://push-owner.example.test".into())
        .await
        .unwrap();
    let same = store
        .enroll_push(
            &user,
            serde_json::from_slice(&payload).unwrap(),
            "https://push-owner.example.test".into(),
        )
        .await
        .unwrap();
    assert_eq!(first.binding_id, same.binding_id);
    assert!(dir.path().join(KEY_FILE).exists());
    let dto = serde_json::to_string(&first).unwrap();
    assert!(!dto.contains("endpoint") && !dto.contains("auth") && !dto.contains("p256dh"));
    store.poll_push_queue().await.unwrap();
    assert!(store.claim_push().await.unwrap().is_none());
    input = serde_json::from_slice(&payload).unwrap();
    input.subscription.auth = "AAAAAAAAAAAAAAAAAAAAAA".into();
    assert!(matches!(
        store
            .enroll_push(&user, input, "https://push-owner.example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
    notify(&store, &user).await;
    let claimed = claim(&store).await;
    let body = String::from_utf8(claimed.payload).unwrap();
    assert!(
        !body.contains("canary")
            && !body.contains("Synthetic body")
            && !body.contains("title")
            && !body.contains("message")
    );
}
#[tokio::test]
async fn claim_is_not_dispatch_read_quiet_and_session_changes_fence_it() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let binding = enrolled(&store, &user).await;
    let n = notify(&store, &user).await;
    let c = claim(&store).await;
    store.mark_notification(&user, n.id, false).await.unwrap();
    assert!(matches!(
        store.commit_push_dispatch(&c.lease).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(state(&store, &c.lease).await, "suppressed");
    let n = notify(&store, &user).await;
    let c = claim(&store).await;
    store.logout(&user).await.unwrap();
    assert!(matches!(
        store.commit_push_dispatch(&c.lease).await,
        Err(StoreError::Conflict)
    ));
    assert!(
        store
            .authorize_push_display(&user, binding.binding_id, n.id)
            .await
            .is_err()
    );
}
#[tokio::test]
async fn stable_receipts_backoff_unknown_restart_and_expired_binding_are_distinct() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    enrolled(&store, &user).await;
    notify(&store, &user).await;
    let c = claim(&store).await;
    store.commit_push_dispatch(&c.lease).await.unwrap();
    store
        .finish_push(&c.lease, PushOutcome::Retry(120))
        .await
        .unwrap();
    assert_eq!(state(&store, &c.lease).await, "queued");
    assert!(store.claim_push().await.unwrap().is_none());
    let old = c.lease.clone();
    let update = old.clone();
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE push_queue SET ready_at=0 WHERE tenant_id=?1 AND id=?2",
                params![update.tenant, update.id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let c = store.claim_push().await.unwrap().unwrap();
    assert!(matches!(
        store.commit_push_dispatch(&old).await,
        Err(StoreError::Conflict)
    ));
    store.commit_push_dispatch(&c.lease).await.unwrap();
    store
        .finish_push(&c.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    store
        .finish_push(&c.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    assert_eq!(state(&store, &c.lease).await, "accepted");
    notify(&store, &user).await;
    let c = claim(&store).await;
    store.commit_push_dispatch(&c.lease).await.unwrap();
    store.call(|db| restart(&db.conn)).await.unwrap();
    assert_eq!(state(&store, &c.lease).await, "unknown");
    store.poll_push_queue().await.unwrap();
    assert!(store.claim_push().await.unwrap().is_none());
    notify(&store, &user).await;
    let c = claim(&store).await;
    store.commit_push_dispatch(&c.lease).await.unwrap();
    store
        .finish_push(&c.lease, PushOutcome::Expired)
        .await
        .unwrap();
    assert_eq!(state(&store, &c.lease).await, "expired");
    assert!(
        !store
            .call(move |db| eligible(&db.conn, &c.lease.binding, clock()?))
            .await
            .unwrap()
    );
}
#[tokio::test]
async fn shared_endpoint_rebinds_generation_without_exposing_another_owner() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "push-member@example.test").await.user;
    let input = enrollment(Uuid::new_v4().to_string());
    let saved = json(&input).unwrap();
    let first = store
        .enroll_push(&a, input, "https://push-owner.example.test".into())
        .await
        .unwrap();
    let n = notify(&store, &a).await;
    let claimed = claim(&store).await;
    let mut input: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    input.request_id = Uuid::new_v4().to_string();
    input.operation_revision = 2;
    let second = store
        .enroll_push(&b, input, "https://push-owner.example.test".into())
        .await
        .unwrap();
    assert_ne!(first.binding_id, second.binding_id);
    assert!(matches!(
        store.commit_push_dispatch(&claimed.lease).await,
        Err(StoreError::Conflict)
    ));
    assert!(
        store
            .authorize_push_display(&b, first.binding_id, n.id)
            .await
            .is_err()
    );
    store
        .disable_push(&a, second.binding_id.clone())
        .await
        .unwrap();
    assert!(
        store
            .call(move |db| eligible(&db.conn, &second.binding_id, clock()?))
            .await
            .unwrap()
    );
}
#[tokio::test]
async fn staged_export_retires_authority_without_generating_or_carrying_a_private_key() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let binding = enrolled(&store, &user).await;
    notify(&store, &user).await;
    let c = claim(&store).await;
    store.commit_push_dispatch(&c.lease).await.unwrap();
    let before = std::fs::read(dir.path().join(KEY_FILE)).unwrap();
    store.call(|db| strip_for_export(&db.conn)).await.unwrap();
    assert_eq!(state(&store, &c.lease).await, "quarantined");
    assert_eq!(std::fs::read(dir.path().join(KEY_FILE)).unwrap(), before);
    let copy = directory();
    assert!(validate_optional_file(copy.path()).is_ok());
    assert!(!copy.path().join(KEY_FILE).exists());
    let device = Uuid::new_v4().to_string();
    store
        .push_configuration(&user, device, "https://push-owner.example.test".into())
        .await
        .unwrap();
    assert_ne!(std::fs::read(dir.path().join(KEY_FILE)).unwrap(), before);
    assert!(
        !store
            .call(move |db| eligible(&db.conn, &binding.binding_id, clock()?))
            .await
            .unwrap()
    );
    std::fs::remove_file(dir.path().join(KEY_FILE)).unwrap();
    assert!(
        store
            .push_configuration(
                &user,
                Uuid::new_v4().to_string(),
                "https://push-owner.example.test".into()
            )
            .await
            .is_err()
    );
    assert!(!dir.path().join(KEY_FILE).exists());
    assert!(
        store
            .create_thread(&user, "Push outage does not block chat".into())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn owner_fairness_quiet_recheck_and_fresh_session_status_are_enforced() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "fair-push@example.test").await.user;
    let device = Uuid::new_v4().to_string();
    store
        .enroll_push(
            &a,
            enrollment(device.clone()),
            "https://push-owner.example.test".into(),
        )
        .await
        .unwrap();
    enrolled(&store, &b).await;
    notify(&store, &a).await;
    notify(&store, &b).await;
    store.poll_push_queue().await.unwrap();
    let first = store.claim_push().await.unwrap().unwrap();
    let second = store.claim_push().await.unwrap().unwrap();
    assert_ne!(first.lease.account, second.lease.account);
    assert!(store.claim_push().await.unwrap().is_none());
    let minute = {
        let t = chrono::Utc::now();
        (t.hour() * 60 + t.minute()) as u16
    };
    let settings = moosuri_core::QuietHoursChange {
        expected_revision: 0,
        enabled: true,
        timezone: "UTC".into(),
        start_minute: (minute + 1439) % 1440,
        end_minute: (minute + 2) % 1440,
    };
    let lease = if first.lease.account == a.account.id {
        first.lease
    } else {
        second.lease
    };
    store.set_quiet_hours(&a, settings).await.unwrap();
    assert!(matches!(
        store.commit_push_dispatch(&lease).await,
        Err(StoreError::Conflict)
    ));
    let fresh = store
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap();
    assert_eq!(
        store
            .push_configuration(
                &fresh.user,
                device,
                "https://push-owner.example.test".into()
            )
            .await
            .unwrap()
            .device
            .unwrap()
            .state,
        "reconnect"
    );
}
#[tokio::test]
async fn admin_suspension_and_password_epoch_changes_block_claimed_dispatch() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "suspend-push@example.test").await.user;
    enrolled(&store, &b).await;
    notify(&store, &b).await;
    let claimed = claim(&store).await;
    let target = store
        .managed_accounts(&a)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.user.id == b.account.id)
        .unwrap();
    let prepared = store
        .prepare_account_management(
            &a,
            b.account.id.clone(),
            moosuri_core::AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: target.revision,
                action: moosuri_core::AccountManagementAction::Suspend,
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap();
    assert!(matches!(
        store.commit_push_dispatch(&claimed.lease).await,
        Err(StoreError::Conflict)
    ));
    enrolled(&store, &a).await;
    notify(&store, &a).await;
    let claimed = claim(&store).await;
    store
        .change_password(&a, "Synthetic-push-new-password".into())
        .await
        .unwrap();
    assert!(matches!(
        store.commit_push_dispatch(&claimed.lease).await,
        Err(StoreError::Conflict)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn invalid_signing_file_never_rotates_active_authority_or_blocks_chat() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    enrolled(&store, &user).await;
    let path = dir.path().join(KEY_FILE);
    let original = std::fs::read(&path).unwrap();
    let config = || {
        store.push_configuration(
            &user,
            Uuid::new_v4().to_string(),
            "https://push-owner.example.test".into(),
        )
    };
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o644)).unwrap();
    assert!(validate_optional_file(dir.path()).is_err());
    assert!(config().await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let (other, _) = moosuri_connectors::push::generate_vapid().unwrap();
    std::fs::write(&path, &other).unwrap();
    assert!(config().await.is_err());
    assert_eq!(std::fs::read(&path).unwrap(), other.as_slice());
    std::fs::write(&path, &original).unwrap();
    let target = dir.path().join("synthetic-key-target");
    std::fs::rename(&path, &target).unwrap();
    symlink(&target, &path).unwrap();
    assert!(validate_optional_file(dir.path()).is_err());
    assert!(config().await.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), original);
    std::fs::remove_file(&path).unwrap();
    std::fs::rename(&target, &path).unwrap();
    assert!(config().await.is_ok());
    assert!(
        store
            .create_thread(&user, "Synthetic key failure isolation".into())
            .await
            .is_ok()
    );
    assert!(!std::fs::read_dir(dir.path()).unwrap().any(|v| {
        v.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".web-push-vapid-")
    }));
}

#[tokio::test]
async fn durable_intent_order_fences_request_ack_replay_and_off_on_reordering() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let first = enrollment(Uuid::new_v4().to_string());
    let saved = json(&first).unwrap();
    let mut newer: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    newer.request_id = Uuid::new_v4().to_string();
    newer.operation_revision = 2;
    let newer_saved = json(&newer).unwrap();
    let latest = store
        .enroll_push(&user, newer, "https://push-owner.example.test".into())
        .await
        .unwrap();
    // The older HTTP request arrives only after the newer request committed.
    assert!(matches!(
        store
            .enroll_push(&user, first, "https://push-owner.example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
    let replay = store
        .enroll_push(
            &user,
            serde_json::from_slice(&newer_saved).unwrap(),
            "https://push-owner.example.test".into(),
        )
        .await
        .unwrap();
    assert_eq!(replay.binding_id, latest.binding_id);
    assert_eq!(replay.state, "active");
    let off = PushDeactivation {
        request_id: Uuid::new_v4().to_string(),
        device_id: serde_json::from_slice::<PushEnrollment>(&saved)
            .unwrap()
            .device_id,
        operation_revision: 3,
    };
    let off_saved = json(&off).unwrap();
    store.deactivate_push(&user, off).await.unwrap();
    assert_eq!(
        store
            .enroll_push(
                &user,
                serde_json::from_slice(&newer_saved).unwrap(),
                "https://push-owner.example.test".into()
            )
            .await
            .unwrap()
            .state,
        "off"
    );
    let mut late: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    late.request_id = Uuid::new_v4().to_string();
    late.operation_revision = 2;
    assert!(matches!(
        store
            .enroll_push(&user, late, "https://push-owner.example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
    let mut next: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    next.request_id = Uuid::new_v4().to_string();
    next.operation_revision = 4;
    let active = store
        .enroll_push(&user, next, "https://push-owner.example.test".into())
        .await
        .unwrap();
    // An uncertain old off ACK can be checked exactly, without disabling new on.
    store
        .deactivate_push(&user, serde_json::from_slice(&off_saved).unwrap())
        .await
        .unwrap();
    let binding = active.binding_id.clone();
    assert!(
        store
            .call(move |db| eligible(&db.conn, &binding, clock()?))
            .await
            .unwrap()
    );
    // Durable receipt ordering survives restart and subscription-row replacement.
    assert!(matches!(
        store
            .enroll_push(
                &user,
                serde_json::from_slice(&saved).unwrap(),
                "https://push-owner.example.test".into()
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let mut conflict: PushDeactivation = serde_json::from_slice(&off_saved).unwrap();
    conflict.operation_revision = 5;
    assert!(matches!(
        store.deactivate_push(&user, conflict).await,
        Err(StoreError::Conflict)
    ));
    // Reopen the actual SQLite database, rather than merely rerun restart SQL.
    // Root-owned strict runtime inventory must admit/validate the optional key
    // before integrated Store::open can exercise the same case.
    drop(store);
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    let first: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    assert!(matches!(
        require_new_intent(&conn, &user, &first.device_id, 3),
        Err(StoreError::Conflict)
    ));
    require_new_intent(&conn, &user, &first.device_id, 5).unwrap();
    let receipt: (String, Vec<u8>) = conn.query_row("SELECT binding_id,request_digest FROM push_enrollment_receipts WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![user.tenant.0,user.account.id,serde_json::from_slice::<PushEnrollment>(&newer_saved).unwrap().request_id],|r|Ok((r.get(0)?,r.get(1)?))).unwrap();
    assert_eq!(receipt.0, latest.binding_id);
    assert_eq!(receipt.1, Sha256::digest(&newer_saved).to_vec());
}

#[tokio::test]
async fn browser_intent_order_preserves_account_session_and_foreign_off_boundaries() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "intent-b@example.test").await.user;
    let first = enrollment(Uuid::new_v4().to_string());
    let saved = json(&first).unwrap();
    let mut next: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    next.request_id = Uuid::new_v4().to_string();
    next.operation_revision = 2;
    let active = store
        .enroll_push(&b, next, "https://push-owner.example.test".into())
        .await
        .unwrap();
    assert!(matches!(
        store
            .enroll_push(&a, first, "https://push-owner.example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
    let device = serde_json::from_slice::<PushEnrollment>(&saved)
        .unwrap()
        .device_id;
    // Metadata-only A off never retires B or raises B's own off watermark.
    store
        .deactivate_push(
            &a,
            PushDeactivation {
                request_id: Uuid::new_v4().to_string(),
                device_id: device.clone(),
                operation_revision: 100,
            },
        )
        .await
        .unwrap();
    let binding = active.binding_id.clone();
    assert!(
        store
            .call(move |db| eligible(&db.conn, &binding, clock()?))
            .await
            .unwrap()
    );
    let mut next: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    next.request_id = Uuid::new_v4().to_string();
    next.operation_revision = 3;
    store
        .enroll_push(&b, next, "https://push-owner.example.test".into())
        .await
        .unwrap();
    let fresh = store
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    let mut next: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    next.request_id = Uuid::new_v4().to_string();
    next.operation_revision = 101;
    let latest = store
        .enroll_push(&fresh, next, "https://push-owner.example.test".into())
        .await
        .unwrap();
    assert_eq!(
        store
            .push_configuration(&a, device.clone(), "https://push-owner.example.test".into())
            .await
            .unwrap()
            .device
            .unwrap()
            .state,
        "reconnect"
    );
    let mut old_session: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    old_session.request_id = Uuid::new_v4().to_string();
    old_session.operation_revision = 99;
    assert!(matches!(
        store
            .enroll_push(&a, old_session, "https://push-owner.example.test".into())
            .await,
        Err(StoreError::Conflict)
    ));
    store.logout(&fresh).await.unwrap();
    let mut revoked: PushEnrollment = serde_json::from_slice(&saved).unwrap();
    revoked.request_id = Uuid::new_v4().to_string();
    revoked.operation_revision = 102;
    assert!(
        store
            .enroll_push(&fresh, revoked, "https://push-owner.example.test".into())
            .await
            .is_err()
    );
    assert!(
        !store
            .call(move |db| eligible(&db.conn, &latest.binding_id, clock()?))
            .await
            .unwrap()
    );
}

// Catches queue-only suppression that loses unscanned notifications or revives on unmute.
#[tokio::test]
async fn source_mute_latches_unscanned_queued_claimed_and_dispatched_notices_without_replay() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let b = member(&s, &u, "mute-push-other@example.test").await.user;
    let binding = enrolled(&s, &u).await;
    enrolled(&s, &b).await;
    let unscanned = notify(&s, &u).await;
    let mutate = |revision, muted| moosuri_core::NotificationSourceChange {
        expected_revision: revision,
        muted,
    };
    s.set_notification_source(&u, unscanned.schedule_id.clone(), mutate(0, true))
        .await
        .unwrap();
    s.set_notification_source(&u, unscanned.schedule_id.clone(), mutate(1, false))
        .await
        .unwrap();
    s.poll_push_queue().await.unwrap();
    assert!(s.claim_push().await.unwrap().is_none());
    let queued = notify(&s, &u).await;
    s.poll_push_queue().await.unwrap();
    s.set_notification_source(&u, queued.schedule_id.clone(), mutate(0, true))
        .await
        .unwrap();
    s.set_notification_source(&u, queued.schedule_id.clone(), mutate(1, false))
        .await
        .unwrap();
    assert!(s.claim_push().await.unwrap().is_none());
    let held_note = notify(&s, &u).await;
    let held = claim(&s).await;
    s.set_notification_source(&u, held_note.schedule_id.clone(), mutate(0, true))
        .await
        .unwrap();
    assert_eq!(state(&s, &held.lease).await, "suppressed");
    s.set_notification_source(&u, held_note.schedule_id.clone(), mutate(1, false))
        .await
        .unwrap();
    assert!(s.commit_push_dispatch(&held.lease).await.is_err());
    let sent_note = notify(&s, &u).await;
    let sent = claim(&s).await;
    s.commit_push_dispatch(&sent.lease).await.unwrap();
    s.authorize_push_display(&u, binding.binding_id.clone(), sent_note.id.clone())
        .await
        .unwrap();
    s.set_notification_source(&u, sent_note.schedule_id.clone(), mutate(0, true))
        .await
        .unwrap();
    assert_eq!(state(&s, &sent.lease).await, "dispatched");
    s.finish_push(&sent.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    assert_eq!(state(&s, &sent.lease).await, "accepted");
    assert!(
        s.authorize_push_display(&u, binding.binding_id.clone(), sent_note.id.clone())
            .await
            .is_err()
    );
    s.set_notification_source(&u, sent_note.schedule_id.clone(), mutate(1, false))
        .await
        .unwrap();
    assert!(
        s.authorize_push_display(&u, binding.binding_id.clone(), sent_note.id)
            .await
            .is_err()
    );
    let retry_note = notify(&s, &u).await;
    let retry = claim(&s).await;
    s.commit_push_dispatch(&retry.lease).await.unwrap();
    s.set_notification_source(&u, retry_note.schedule_id.clone(), mutate(0, true))
        .await
        .unwrap();
    s.set_notification_source(&u, retry_note.schedule_id, mutate(1, false))
        .await
        .unwrap();
    s.finish_push(&retry.lease, PushOutcome::Retry(30))
        .await
        .unwrap();
    assert_eq!(state(&s, &retry.lease).await, "rejected");
    // Independent owner and source still dispatch; no provider/device is called.
    let other = notify(&s, &b).await;
    let unrelated = notify(&s, &u).await;
    let first = claim(&s).await;
    s.commit_push_dispatch(&first.lease).await.unwrap();
    s.finish_push(&first.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    let second = claim(&s).await;
    s.commit_push_dispatch(&second.lease).await.unwrap();
    s.finish_push(&second.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    let mut actual = vec![first.lease.notification, second.lease.notification];
    actual.sort();
    let mut expected = vec![other.id, unrelated.id];
    expected.sort();
    assert_eq!(actual, expected);
    drop(s);
    let s = Store::open(d.path()).unwrap();
    s.poll_push_queue().await.unwrap();
    assert!(s.claim_push().await.unwrap().is_none());
    let n = unscanned.id;
    s.call(move |db| {
        assert!(
            db.conn
                .execute(
                    "UPDATE local_notifications SET push_suppressed=0 WHERE id=?1",
                    [n]
                )
                .is_err()
        );
        Ok(())
    })
    .await
    .unwrap();
}
