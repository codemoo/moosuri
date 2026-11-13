use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
fn ts(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}
async fn set_clock(store: &Store, t: i64) {
    store
        .call(move |_| {
            TEST_TIME.with(|v| v.set(Some(t)));
            Ok(())
        })
        .await
        .unwrap();
}
fn input(rule: ScheduleRule) -> ScheduleCreate {
    ScheduleCreate {
        request_id: Uuid::new_v4().to_string(),
        draft: ScheduleDraft {
            title: "Synthetic reminder".into(),
            message: "Inspect the synthetic garden notes. This is only a local reminder.".into(),
            rule,
            valid_until: None,
        },
    }
}
async fn deliver(store: &Store, t: i64) -> LocalNotification {
    store.enqueue_scheduled_reminders_at(t).await.unwrap();
    let lease = store.claim_scheduled_reminder_at(t).await.unwrap().unwrap();
    store.finish_scheduled_reminder_at(&lease, t).await.unwrap()
}

#[test]
fn utc_oracle_preserves_anchor_and_new_york_gap_fold_without_backlog_materialization() {
    let gap = ScheduleRule::Daily {
        time: "02:30".into(),
        timezone: "America/New_York".into(),
    };
    let first = time::after(&gap, ts("2027-03-14T05:00:00Z"))
        .unwrap()
        .unwrap();
    assert!(first.at.is_none());
    assert_eq!(first.due, ts("2027-03-14T07:00:00Z"));
    let plan = time::plan(&gap, &first, first.due).unwrap();
    assert!(plan.latest.is_none());
    assert_eq!(plan.gaps.len(), 1);
    assert_eq!(plan.next.unwrap().at, Some(ts("2027-03-15T06:30:00Z")));
    let fold = ScheduleRule::Daily {
        time: "01:30".into(),
        timezone: "America/New_York".into(),
    };
    let first = time::after(&fold, ts("2027-11-07T04:00:00Z"))
        .unwrap()
        .unwrap();
    assert_eq!(first.at, Some(ts("2027-11-07T05:30:00Z")));
    assert_eq!(
        time::after(&fold, ts("2027-11-07T05:31:00Z"))
            .unwrap()
            .unwrap()
            .at,
        Some(ts("2027-11-08T06:30:00Z"))
    );
    let edge = ScheduleRule::Interval {
        anchor: 4_102_444_800,
        seconds: 60,
    };
    let slot = time::after(&edge, 4_102_444_799).unwrap().unwrap();
    assert!(
        time::plan(&edge, &slot, 4_102_444_800)
            .unwrap()
            .next
            .is_none()
    );
    let anchor = ts("2027-01-01T00:00:00Z");
    let rule = ScheduleRule::Interval {
        anchor,
        seconds: 60,
    };
    let first = time::after(&rule, anchor).unwrap().unwrap();
    let now = ts("2050-01-01T00:00:17Z");
    let plan = time::plan(&rule, &first, now).unwrap();
    assert_eq!(plan.latest.unwrap().at, Some(now - 17));
    assert_eq!(plan.next.unwrap().at, Some(now + 43));
    assert!(plan.skipped.unwrap().2 > 10_000_000);
    assert!(
        time::after(
            &ScheduleRule::Interval {
                anchor,
                seconds: 59
            },
            anchor
        )
        .is_err()
    );
    assert!(
        time::after(
            &ScheduleRule::Daily {
                time: "25:00".into(),
                timezone: "America/New_York".into()
            },
            anchor
        )
        .is_err()
    );
    assert!(
        time::after(
            &ScheduleRule::Daily {
                time: "02:30".into(),
                timezone: "Not/A_Zone".into()
            },
            anchor
        )
        .is_err()
    );
}

#[tokio::test]
async fn expired_and_paused_past_once_are_skips_not_false_delivery_and_restore_quarantines_future_work()
 {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let base = ts("2027-01-01T00:00:00Z");
    set_clock(&store, base).await;
    let mut create = input(ScheduleRule::Once { at: base + 60 });
    create.draft.valid_until = Some(base + 70);
    let expired = store.create_schedule(&u, create).await.unwrap();
    store
        .enqueue_scheduled_reminders_at(base + 80)
        .await
        .unwrap();
    assert!(
        store
            .claim_scheduled_reminder_at(base + 80)
            .await
            .unwrap()
            .is_none()
    );
    let h = store.schedule_history(&u, expired.id).await.unwrap();
    assert_eq!(h.occurrences[0].state, "expired");
    assert!(h.occurrences[0].notification_id.is_none());
    let paused = store
        .create_schedule(&u, input(ScheduleRule::Once { at: base + 120 }))
        .await
        .unwrap();
    store
        .control_schedule(
            &u,
            paused.id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "pause".into(),
            },
        )
        .await
        .unwrap();
    set_clock(&store, base + 180).await;
    let result = store
        .control_schedule(
            &u,
            paused.id.clone(),
            ScheduleControl {
                expected_revision: 2,
                action: "resume".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(result.state, "expired");
    assert_eq!(result.reason.as_deref(), Some("pause_period"));
    assert_eq!(
        store
            .control_schedule(
                &u,
                paused.id,
                ScheduleControl {
                    expected_revision: 2,
                    action: "resume".into()
                }
            )
            .await
            .unwrap()
            .revision,
        result.revision
    );
    assert!(
        store
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    store
        .create_schedule(&u, input(ScheduleRule::Once { at: base + 240 }))
        .await
        .unwrap();
    let delivered = deliver(&store, base + 240).await;
    set_clock(&store, base + 241).await;
    let pending = store
        .create_schedule(
            &u,
            input(ScheduleRule::Interval {
                anchor: base + 300,
                seconds: 60,
            }),
        )
        .await
        .unwrap();
    store
        .enqueue_scheduled_reminders_at(base + 300)
        .await
        .unwrap();
    let held = store
        .claim_scheduled_reminder_at(base + 300)
        .await
        .unwrap()
        .unwrap();
    store
        .call(|db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            quarantine_for_restore(&tx)?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 301)
            .await
            .is_err()
    );
    assert_eq!(
        store.notifications(&u, None, false).await.unwrap().items[0].id,
        delivered.id
    );
    let view = store
        .schedules(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|s| s.id == pending.id)
        .unwrap();
    assert_eq!(view.state, "paused");
    assert_eq!(view.reason.as_deref(), Some("restore_quarantine"));
}

#[tokio::test]
async fn once_late_receipt_idempotency_and_actual_reopen_never_duplicate_notification() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let base = ts("2027-01-01T00:00:00Z");
    set_clock(&store, base).await;
    let request = input(ScheduleRule::Once { at: base + 60 });
    let schedule = store.create_schedule(&u, request.clone()).await.unwrap();
    assert_eq!(
        store.create_schedule(&u, request).await.unwrap().id,
        schedule.id
    );
    store
        .enqueue_scheduled_reminders_at(base + 900)
        .await
        .unwrap();
    let lease = store
        .claim_scheduled_reminder_at(base + 900)
        .await
        .unwrap()
        .unwrap();
    let n = store
        .finish_scheduled_reminder_at(&lease, base + 900)
        .await
        .unwrap();
    assert!(n.delayed);
    assert_eq!(n.scheduled_for, base + 60);
    assert_eq!(
        store
            .finish_scheduled_reminder_at(&lease, base + 901)
            .await
            .unwrap()
            .id,
        n.id
    );
    assert_eq!(store.schedules(&u).await.unwrap()[0].state, "completed");
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    set_clock(&store, base + 1200).await;
    assert_eq!(
        store
            .enqueue_scheduled_reminders_at(base + 1200)
            .await
            .unwrap(),
        0
    );
    assert!(
        store
            .claim_scheduled_reminder_at(base + 1200)
            .await
            .unwrap()
            .is_none()
    );
    let page = store.notifications(&u, None, false).await.unwrap();
    assert_eq!(page.items.len(), 1);
    assert_eq!(page.items[0].id, n.id);
    let h = store.schedule_history(&u, schedule.id).await.unwrap();
    assert_eq!(h.occurrences.len(), 1);
    assert_eq!(
        h.occurrences[0].notification_id.as_deref(),
        Some(n.id.as_str())
    );
}

#[tokio::test]
async fn pause_edit_revoke_fence_held_claims_and_resume_skips_paused_time() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let base = ts("2027-01-01T00:00:00Z");
    set_clock(&store, base).await;
    let s = store
        .create_schedule(
            &u,
            input(ScheduleRule::Interval {
                anchor: base + 60,
                seconds: 60,
            }),
        )
        .await
        .unwrap();
    store
        .enqueue_scheduled_reminders_at(base + 60)
        .await
        .unwrap();
    let old = store
        .claim_scheduled_reminder_at(base + 60)
        .await
        .unwrap()
        .unwrap();
    set_clock(&store, base + 61).await;
    let paused = store
        .control_schedule(
            &u,
            s.id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "pause".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.state, "paused");
    assert!(matches!(
        store.finish_scheduled_reminder_at(&old, base + 62).await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(
        store
            .enqueue_scheduled_reminders_at(base + 500)
            .await
            .unwrap(),
        0
    );
    set_clock(&store, base + 500).await;
    let resumed = store
        .control_schedule(
            &u,
            s.id.clone(),
            ScheduleControl {
                expected_revision: 2,
                action: "resume".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(resumed.next_fire_at, Some(base + 540));
    assert!(
        store
            .schedule_history(&u, s.id.clone())
            .await
            .unwrap()
            .skipped_ranges
            .iter()
            .any(|r| r.reason == "pause_period")
    );
    store
        .enqueue_scheduled_reminders_at(base + 540)
        .await
        .unwrap();
    let held = store
        .claim_scheduled_reminder_at(base + 540)
        .await
        .unwrap()
        .unwrap();
    set_clock(&store, base + 541).await;
    let mut changed = input(ScheduleRule::Once { at: base + 900 }).draft;
    changed.title = "Edited synthetic reminder".into();
    let edited = store
        .edit_schedule(
            &u,
            s.id.clone(),
            ScheduleEdit {
                expected_revision: 3,
                draft: changed,
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.revision, 4);
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 542)
            .await
            .is_err()
    );
    store
        .enqueue_scheduled_reminders_at(base + 900)
        .await
        .unwrap();
    let held = store
        .claim_scheduled_reminder_at(base + 900)
        .await
        .unwrap()
        .unwrap();
    set_clock(&store, base + 901).await;
    store
        .control_schedule(
            &u,
            s.id,
            ScheduleControl {
                expected_revision: 4,
                action: "revoke".into(),
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 902)
            .await
            .is_err()
    );
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
async fn latest_queue_bounds_and_persistent_round_robin_keep_other_owner_progress() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "reminder-other@example.test").await.user;
    let base = ts("2027-01-01T00:00:00Z");
    set_clock(&store, base).await;
    for u in [&a, &a, &b] {
        store
            .create_schedule(
                u,
                input(ScheduleRule::Interval {
                    anchor: base + 60,
                    seconds: 60,
                }),
            )
            .await
            .unwrap();
    }
    store
        .enqueue_scheduled_reminders_at(base + 60)
        .await
        .unwrap();
    let first = store
        .claim_scheduled_reminder_at(base + 170)
        .await
        .unwrap()
        .unwrap();
    let second = store
        .claim_scheduled_reminder_at(base + 170)
        .await
        .unwrap()
        .unwrap();
    assert_ne!(first.account, second.account);
    store
        .enqueue_scheduled_reminders_at(base + 180)
        .await
        .unwrap();
    store.call(|db|{let bad:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM reminder_occurrences WHERE state IN('claimed','queued') GROUP BY tenant_id,schedule_id,state HAVING count(*)>1)",[],|r|r.get(0))?;assert!(!bad);Ok(())}).await.unwrap();
    let n = store
        .finish_scheduled_reminder_at(&first, base + 181)
        .await
        .unwrap();
    assert_eq!(n.scheduled_for, base + 60);
    let h = store
        .schedule_history(
            if first.account == a.account.id {
                &a
            } else {
                &b
            },
            n.schedule_id,
        )
        .await
        .unwrap();
    assert_eq!(
        h.occurrences.iter().filter(|o| o.state == "queued").count(),
        1
    );
    assert!(
        h.occurrences
            .iter()
            .any(|o| o.scheduled_for == Some(base + 180))
    );
}

#[tokio::test]
async fn account_epoch_revokes_background_authority_but_logout_does_not_mint_a_session() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let base = ts("2027-01-01T00:00:00Z");
    set_clock(&store, base).await;
    store
        .create_schedule(
            &u,
            input(ScheduleRule::Interval {
                anchor: base + 60,
                seconds: 60,
            }),
        )
        .await
        .unwrap();
    store
        .enqueue_scheduled_reminders_at(base + 60)
        .await
        .unwrap();
    let held = store
        .claim_scheduled_reminder_at(base + 60)
        .await
        .unwrap()
        .unwrap();
    let new = store
        .change_password(&u, "Synthetic-new-reminder-password".into())
        .await
        .unwrap()
        .user;
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 61)
            .await
            .is_err()
    );
    let displayed = store.schedules(&new).await.unwrap().remove(0);
    assert_eq!(
        displayed.state, "paused",
        "epoch revocation must be visible before the next due poll"
    );
    set_clock(&store, base + 61).await;
    let rebound = store
        .control_schedule(
            &new,
            displayed.id,
            ScheduleControl {
                expected_revision: displayed.revision,
                action: "resume".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(rebound.next_fire_at, Some(base + 120));
    store
        .enqueue_scheduled_reminders_at(base + 120)
        .await
        .unwrap();
    let held = store
        .claim_scheduled_reminder_at(base + 120)
        .await
        .unwrap()
        .unwrap();
    let new = store
        .change_password(&new, "Synthetic-third-reminder-password".into())
        .await
        .unwrap()
        .user;
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 121)
            .await
            .is_err()
    );
    store
        .enqueue_scheduled_reminders_at(base + 180)
        .await
        .unwrap();
    assert!(
        store
            .claim_scheduled_reminder_at(base + 180)
            .await
            .unwrap()
            .is_none()
    );
    set_clock(&store, base + 181).await;
    let old = store.schedules(&new).await.unwrap().remove(0);
    let rebound = store
        .control_schedule(
            &new,
            old.id,
            ScheduleControl {
                expected_revision: old.revision,
                action: "resume".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(rebound.next_fire_at, Some(base + 240));
    store.logout(&new).await.unwrap();
    assert_eq!(deliver(&store, base + 240).await.scheduled_for, base + 240);
}

#[tokio::test]
async fn failed_local_publication_rolls_back_receipt_and_restart_recovers_only_uncommitted_claim() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    let base = ts("2027-01-01T00:00:00Z");
    set_clock(&store, base).await;
    store
        .create_schedule(&u, input(ScheduleRule::Once { at: base + 60 }))
        .await
        .unwrap();
    store
        .enqueue_scheduled_reminders_at(base + 60)
        .await
        .unwrap();
    let held = store
        .claim_scheduled_reminder_at(base + 60)
        .await
        .unwrap()
        .unwrap();
    store.call(|db|{db.conn.execute_batch("CREATE TEMP TRIGGER interrupt_notification BEFORE UPDATE OF state ON reminder_occurrences WHEN NEW.state='delivered' BEGIN SELECT RAISE(ABORT,'synthetic receipt interruption after inbox insert'); END;")?;Ok(())}).await.unwrap();
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 60)
            .await
            .is_err()
    );
    assert!(
        store
            .notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let fresh = store
        .claim_scheduled_reminder_at(base + 61)
        .await
        .unwrap()
        .unwrap();
    assert!(
        store
            .finish_scheduled_reminder_at(&held, base + 61)
            .await
            .is_err()
    );
    let n = store
        .finish_scheduled_reminder_at(&fresh, base + 61)
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(
        store
            .claim_scheduled_reminder_at(base + 62)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        store.notifications(&u, None, false).await.unwrap().items[0].id,
        n.id
    );
}

#[tokio::test]
async fn quiet_inbox_read_dismiss_and_pagination_are_owner_scoped_and_idempotent() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let a = admin(&store).await.user;
    let b = member(&store, &a, "inbox-other@example.test").await.user;
    let base = ts("2027-01-01T22:00:00Z");
    set_clock(&store, base).await;
    store
        .set_quiet_hours(
            &a,
            QuietHoursChange {
                expected_revision: 0,
                enabled: true,
                timezone: "UTC".into(),
                start_minute: 1320,
                end_minute: 420,
            },
        )
        .await
        .unwrap();
    store
        .create_schedule(
            &a,
            input(ScheduleRule::Interval {
                anchor: base + 60,
                seconds: 60,
            }),
        )
        .await
        .unwrap();
    let n = deliver(&store, base + 60).await;
    assert!(n.quiet);
    assert!(!n.read);
    assert!(matches!(
        store.mark_notification(&b, n.id.clone(), true).await,
        Err(StoreError::NotFound)
    ));
    let read = store
        .mark_notification(&a, n.id.clone(), false)
        .await
        .unwrap();
    assert!(read.read);
    assert!(!read.dismissed);
    store
        .mark_notification(&a, n.id.clone(), true)
        .await
        .unwrap();
    store
        .mark_notification(&a, n.id.clone(), false)
        .await
        .unwrap();
    assert!(
        store
            .notifications(&a, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(store.notifications(&a, None, true).await.unwrap().items[0].dismissed);
    assert!(
        store
            .notifications(&b, None, true)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    for i in 2..=106 {
        deliver(&store, base + i * 60).await;
    }
    let page = store.notifications(&a, None, false).await.unwrap();
    assert_eq!(page.items.len(), 100);
    let second = store
        .notifications(&a, page.next_cursor, false)
        .await
        .unwrap();
    assert_eq!(second.items.len(), 5);
    assert!(second.next_cursor.is_none());
    assert!(
        page.items
            .iter()
            .all(|a| second.items.iter().all(|b| a.id != b.id))
    );
    let settings = store.quiet_hours(&a).await.unwrap();
    assert!(time::quiet(&settings, ts("2027-01-02T06:59:00Z")).unwrap());
    assert!(!time::quiet(&settings, ts("2027-01-02T07:00:00Z")).unwrap());
}

#[tokio::test]
async fn stored_daily_gap_and_fold_receipts_match_exact_utc_without_second_fold() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let u = admin(&store).await.user;
    set_clock(&store, ts("2027-03-13T05:00:00Z")).await;
    let s = store
        .create_schedule(
            &u,
            input(ScheduleRule::Daily {
                time: "02:30".into(),
                timezone: "America/New_York".into(),
            }),
        )
        .await
        .unwrap();
    assert_eq!(s.next_fire_at, Some(ts("2027-03-13T07:30:00Z")));
    deliver(&store, ts("2027-03-13T07:30:00Z")).await;
    store
        .enqueue_scheduled_reminders_at(ts("2027-03-14T07:00:00Z"))
        .await
        .unwrap();
    assert!(
        store
            .claim_scheduled_reminder_at(ts("2027-03-14T07:00:00Z"))
            .await
            .unwrap()
            .is_none()
    );
    let h = store.schedule_history(&u, s.id.clone()).await.unwrap();
    assert!(
        h.occurrences
            .iter()
            .any(|o| o.reason.as_deref() == Some("dst_gap")
                && o.scheduled_for.is_none()
                && o.local_slot.starts_with("2027-03-14T02:30"))
    );
    assert_eq!(
        store.schedules(&u).await.unwrap()[0].next_fire_at,
        Some(ts("2027-03-15T06:30:00Z"))
    );
    store
        .control_schedule(
            &u,
            s.id,
            ScheduleControl {
                expected_revision: 1,
                action: "revoke".into(),
            },
        )
        .await
        .unwrap();
    set_clock(&store, ts("2027-11-07T04:00:00Z")).await;
    store
        .create_schedule(
            &u,
            input(ScheduleRule::Daily {
                time: "01:30".into(),
                timezone: "America/New_York".into(),
            }),
        )
        .await
        .unwrap();
    let n = deliver(&store, ts("2027-11-07T05:30:00Z")).await;
    assert_eq!(n.scheduled_for, ts("2027-11-07T05:30:00Z"));
    assert_eq!(
        store
            .enqueue_scheduled_reminders_at(ts("2027-11-07T06:30:00Z"))
            .await
            .unwrap(),
        0
    );
}
