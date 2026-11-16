use super::*;
use crate::tests::{PASSWORD, TEST_LOCK, admin, directory, member};
async fn fixture() -> (
    tempfile::TempDir,
    Store,
    UserContext,
    String,
    String,
    GoogleAppPin,
) {
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let thread = store
        .create_thread(&u, "Synthetic vendor reply".into())
        .await
        .unwrap()
        .id;
    let pin = GoogleAppPin {
        revision: None,
        client_id: "synthetic.apps.googleusercontent.com".into(),
        redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
    };
    let start = store
        .start_mail_oauth_with_app(&u, pin.clone(), false)
        .await
        .unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = store
        .claim_mail_oauth_with_app(&u, handle, pin.clone())
        .await
        .unwrap();
    let c = store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "owner@example.test".into(),
        )
        .await
        .unwrap();
    let action = store
        .create_mail_draft(
            &u,
            MailDraftInput {
                connection_id: c.id.clone(),
                content: MailDraftContent {
                    from: c.email,
                    to: vec!["vendor@example.test".into()],
                    cc: vec![],
                    bcc: vec![],
                    subject: "Synthetic inventory question".into(),
                    text: "Is the item available?".into(),
                    reply: None,
                },
                origin_thread_id: Some(thread.clone()),
            },
            None,
        )
        .await
        .unwrap();
    // Only a synthetic successful-send receipt; never send a real message.
    store
        .decide_mail(
            &u,
            action.id.clone(),
            MailApproval {
                revision: action.revision,
                digest: action.digest,
            },
            true,
        )
        .await
        .unwrap();
    let dispatch = store.next_mail_dispatch().await.unwrap().unwrap();
    store
        .finish_mail_dispatch(&dispatch, Some(("m0".into(), "t1".into())), false)
        .await
        .unwrap();
    (dir, store, u, thread, c.id, pin)
}
async fn input(store: &Store, u: &UserContext, thread: &str) -> MailReplyWatchCreate {
    let list = store
        .mail_reply_watches(u, Some(thread.into()))
        .await
        .unwrap();
    assert_eq!(list.targets.len(), 1);
    MailReplyWatchCreate {
        request_id: Uuid::new_v4().to_string(),
        thread_id: thread.into(),
        target_id: list.targets[0].id.clone(),
        cadence_seconds: 1800,
        duration_days: 7,
    }
}
fn reply() -> Vec<MailReplyPreview> {
    vec![MailReplyPreview {
        message_id: "m1".into(),
        from: vec!["vendor@example.test".into()],
        subject: "Re: inventory".into(),
        text: "Synthetic: back in stock on Friday.".into(),
        partial: false,
    }]
}
async fn due(store: &Store) {
    store
        .call(|db| {
            db.conn.execute(
                "UPDATE mail_reply_watches SET next_due=0,lease_deadline=0 WHERE state='active'",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap()
}
#[tokio::test]
async fn reply_watch_is_durable_idempotent_owner_scoped_and_unchanged_admits_no_model() {
    let _g = TEST_LOCK.lock().await;
    let (dir, store, u, thread, _, _) = fixture().await;
    let b = member(&store, &u, "other@example.test").await.user;
    let request = input(&store, &u, &thread).await;
    let watch = store
        .create_mail_reply_watch(&u, request.clone())
        .await
        .unwrap();
    assert_eq!(
        store
            .create_mail_reply_watch(&u, request.clone())
            .await
            .unwrap()
            .id,
        watch.id
    );
    let mut changed = request.clone();
    changed.duration_days = 1;
    assert!(matches!(
        store.create_mail_reply_watch(&u, changed).await,
        Err(StoreError::Conflict)
    ));
    assert!(store.create_mail_reply_watch(&b, request).await.is_err());
    assert!(
        store
            .mail_reply_watches(&b, Some(thread.clone()))
            .await
            .is_err()
    );
    assert!(
        store
            .stop_mail_reply_watch(&b, watch.id.clone())
            .await
            .is_err()
    );
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    store
        .complete_mail_reply_watch(&lease, vec!["m0".into()], vec![])
        .await
        .unwrap();
    assert!(store.next_mail_reply_watch().await.unwrap().is_none());
    assert!(store.claim_agent_run().await.unwrap().is_none());
    assert!(
        store
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    store.logout(&u).await.unwrap();
    due(&store).await;
    let old = store.next_mail_reply_watch().await.unwrap().unwrap();
    // A crash leaves a lease; the same durable grant is reclaimed only after its deadline.
    due(&store).await;
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    assert!(
        store
            .complete_mail_reply_watch(&old, vec!["m0".into(), "m1".into()], reply())
            .await
            .is_err()
    );
    store
        .complete_mail_reply_watch(&lease, vec!["m0".into(), "m1".into()], reply())
        .await
        .unwrap();
    assert!(
        store
            .complete_mail_reply_watch(&lease, vec!["m0".into(), "m1".into()], reply())
            .await
            .is_err()
    );
    let u = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let list = store
        .mail_reply_watches(&u, Some(thread.clone()))
        .await
        .unwrap();
    assert_eq!(list.watches[0].state, "replied");
    assert_eq!(list.watches[0].replies.len(), 1);
    let inbox = store.notifications(&u, None, false).await.unwrap();
    assert_eq!(inbox.items.len(), 1);
    assert_eq!(inbox.items[0].thread_id.as_deref(), Some(thread.as_str()));
    assert!(!inbox.items[0].message.contains("back in stock"));
    assert_eq!(
        inbox.items[0].source.as_ref().unwrap().kind,
        "mail_reply_watch"
    );
    store.stop_mail_reply_watch(&u, watch.id).await.unwrap();
    assert!(
        store
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        store
            .mail_reply_watches(&u, Some(thread))
            .await
            .unwrap()
            .watches[0]
            .replies
            .is_empty()
    );
}
#[tokio::test]
async fn reply_watch_observation_and_notification_commit_atomically() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, u, thread, _, _) = fixture().await;
    store
        .create_mail_reply_watch(&u, input(&store, &u, &thread).await)
        .await
        .unwrap();
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    store.call(|db|{db.conn.execute_batch("CREATE TRIGGER reject_reply_notice BEFORE INSERT ON local_notifications BEGIN SELECT RAISE(ABORT,'synthetic notice failure');END;")?;Ok(())}).await.unwrap();
    assert!(
        store
            .complete_mail_reply_watch(&lease, vec!["m0".into(), "m1".into()], reply())
            .await
            .is_err()
    );
    let view = store
        .mail_reply_watches(&u, Some(thread.clone()))
        .await
        .unwrap();
    assert_eq!(view.watches[0].state, "active");
    assert!(view.watches[0].last_checked_at.is_none());
    store
        .call(|db| {
            db.conn.execute_batch("DROP TRIGGER reject_reply_notice;")?;
            Ok(())
        })
        .await
        .unwrap();
    store
        .complete_mail_reply_watch(&lease, vec!["m0".into(), "m1".into()], reply())
        .await
        .unwrap();
    assert_eq!(
        store
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
}
#[tokio::test]
async fn reply_watch_errors_do_not_advance_evidence_and_stop_disconnect_expiry_fence_results() {
    let _g = TEST_LOCK.lock().await;
    for case in ["stop", "disconnect", "expired", "delete", "epoch"] {
        let (_dir, store, u, thread, c, _) = fixture().await;
        let watch = store
            .create_mail_reply_watch(&u, input(&store, &u, &thread).await)
            .await
            .unwrap();
        let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
        store
            .fail_mail_reply_watch(&lease, "check_failed", false)
            .await
            .unwrap();
        let v = store
            .mail_reply_watches(&u, Some(thread.clone()))
            .await
            .unwrap();
        assert!(v.watches[0].last_checked_at.is_none());
        assert_eq!(v.watches[0].reason.as_deref(), Some("check_failed"));
        due(&store).await;
        let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
        assert_eq!(lease.seen_ids, vec!["m0"]);
        match case {
            "stop" => {
                store.stop_mail_reply_watch(&u, watch.id).await.unwrap();
            }
            "disconnect" => {
                store.disconnect_mail(&u, c).await.unwrap();
            }
            "expired" => {
                store
                    .call(|db| {
                        db.conn
                            .execute("UPDATE mail_reply_watches SET expires_at=0", [])?;
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
            "delete" => {
                store.delete_thread(&u, thread).await.unwrap();
            }
            _ => {
                store
                    .call(|db| {
                        db.conn
                            .execute("UPDATE accounts SET auth_epoch=auth_epoch+1", [])?;
                        Ok(())
                    })
                    .await
                    .unwrap();
            }
        }
        assert!(
            store
                .complete_mail_reply_watch(&lease, vec!["m0".into(), "m1".into()], reply())
                .await
                .is_err(),
            "{case}"
        );
        assert!(
            store.next_mail_reply_watch().await.unwrap().is_none(),
            "{case}"
        );
    }
}
#[tokio::test]
async fn reply_watch_credentials_refresh_cas_pin_and_revocation_are_enforced() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, u, thread, c, pin) = fixture().await;
    store
        .create_mail_reply_watch(&u, input(&store, &u, &thread).await)
        .await
        .unwrap();
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    let before = store
        .mail_reply_credentials(&lease, pin.clone())
        .await
        .unwrap();
    let mut wrong = pin.clone();
    wrong.client_id = "other.apps.googleusercontent.com".into();
    assert!(store.mail_reply_credentials(&lease, wrong).await.is_err());
    let tokens = || TokenSet {
        access_token: SecretString::new("synthetic-rotated".into()).unwrap(),
        refresh_token: None,
        expires_in: 3600,
        scopes: None,
    };
    store
        .refresh_mail_reply_credentials(&lease, pin.clone(), before.revision, tokens())
        .await
        .unwrap();
    assert!(
        store
            .refresh_mail_reply_credentials(&lease, pin.clone(), before.revision, tokens())
            .await
            .is_err()
    );
    let after = store
        .mail_reply_credentials(&lease, pin.clone())
        .await
        .unwrap();
    assert_eq!(after.access.expose(), "synthetic-rotated");
    assert_eq!(after.refresh.expose(), before.refresh.expose());
    store.disconnect_mail(&u, c).await.unwrap();
    assert!(store.mail_reply_credentials(&lease, pin).await.is_err());
}
#[tokio::test]
async fn reply_watch_blocked_notice_is_visible_but_withdrawn_before_passive_push_display() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, u, thread, c, _) = fixture().await;
    store
        .create_mail_reply_watch(&u, input(&store, &u, &thread).await)
        .await
        .unwrap();
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    store
        .fail_mail_reply_watch(&lease, "too_many_replies", true)
        .await
        .unwrap();
    let inbox = store.notifications(&u, None, false).await.unwrap();
    assert_eq!(inbox.items.len(), 1);
    let id = inbox.items[0].id.clone();
    let tenant = u.tenant.0.clone();
    assert!(
        store
            .call(move |db| crate::schedules::notice_current(&db.conn, &tenant, &id))
            .await
            .unwrap()
    );
    store.disconnect_mail(&u, c).await.unwrap();
    assert!(
        store
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
}

#[tokio::test]
async fn reply_watch_schema73_update_creates_no_grants_and_notification_preferences_delete_with_thread()
 {
    let _g = TEST_LOCK.lock().await;
    let dir = directory();
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE mail_reply_watch_requests;DROP TABLE mail_reply_watches;PRAGMA user_version=73;").unwrap();
    drop(conn);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.next_mail_reply_watch().await.unwrap().is_none());
    drop(store);
    let (_dir, store, u, thread, _, _) = fixture().await;
    let w = store
        .create_mail_reply_watch(&u, input(&store, &u, &thread).await)
        .await
        .unwrap();
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    store
        .complete_mail_reply_watch(&lease, vec!["m0".into(), "m1".into()], reply())
        .await
        .unwrap();
    store
        .set_notification_source(
            &u,
            w.id,
            moosuri_core::NotificationSourceChange {
                expected_revision: 0,
                muted: true,
            },
        )
        .await
        .unwrap();
    assert_eq!(store.muted_notification_sources(&u).await.unwrap().len(), 1);
    store.delete_thread(&u, thread).await.unwrap();
    assert!(
        store
            .notifications(&u, None, true)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        store
            .muted_notification_sources(&u)
            .await
            .unwrap()
            .is_empty()
    );
}

#[tokio::test]
async fn reply_watch_baseline_keeps_sent_receipts_when_dispatch_order_differs_from_draft_order() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, store, u, thread, connection, _) = fixture().await;
    let draft = || MailDraftInput {
        connection_id: connection.clone(),
        content: MailDraftContent {
            from: "owner@example.test".into(),
            to: vec!["vendor@example.test".into()],
            cc: vec![],
            bcc: vec![],
            subject: "Synthetic follow-up".into(),
            text: "Synthetic approved follow-up".into(),
            reply: None,
        },
        origin_thread_id: Some(thread.clone()),
    };
    let older = store.create_mail_draft(&u, draft(), None).await.unwrap();
    let newer = store.create_mail_draft(&u, draft(), None).await.unwrap();
    // An older draft can be approved/dispatched later than a newer draft.
    for (action, message) in [(newer, "m1"), (older, "m2")] {
        store
            .decide_mail(
                &u,
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
        store
            .finish_mail_dispatch(&dispatch, Some((message.into(), "t1".into())), false)
            .await
            .unwrap();
    }
    store
        .create_mail_reply_watch(&u, input(&store, &u, &thread).await)
        .await
        .unwrap();
    let lease = store.next_mail_reply_watch().await.unwrap().unwrap();
    assert!(lease.baseline_ids.contains(&"m1".to_owned()));
    assert!(lease.baseline_ids.contains(&"m2".to_owned()));
}
