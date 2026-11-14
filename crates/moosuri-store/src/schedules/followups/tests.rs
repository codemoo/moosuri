use super::*;
use crate::push::PushEnrollment;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_connectors::push::{PushOutcome, PushSubscription};
fn input(due: Option<i64>, review: Option<i64>, snooze: Option<i64>) -> ArtifactWrite {
    let date = |utc| ArtifactDate {
        utc,
        timezone: "UTC".into(),
    };
    ArtifactWrite {
        request_id: Uuid::new_v4().to_string(),
        id: None,
        expected_revision: None,
        draft: ArtifactDraft {
            title: "Synthetic followup private canary".into(),
            body: "Synthetic body must not enter Push".into(),
            references: vec![],
            payload: ArtifactPayload::Followup {
                record_kind: FollowupKind::Request,
                person: "Synthetic person".into(),
                lifecycle: FollowupLifecycle::Requested,
                progress: FollowupProgress::None,
                muted: false,
                due: due.map(date),
                review: review.map(date),
                snooze: snooze.map(date),
            },
        },
    }
}
fn edit(mut v: ArtifactWrite, r: &ArtifactReceipt) -> ArtifactWrite {
    v.request_id = Uuid::new_v4().to_string();
    v.id = Some(r.id.clone());
    v.expected_revision = Some(r.revision);
    v
}
async fn deliver(s: &Store, at: i64) -> LocalNotification {
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    let lease = s.claim_scheduled_reminder_at(at).await.unwrap().unwrap();
    s.finish_scheduled_reminder_at(&lease, at).await.unwrap()
}
async fn enroll(s: &Store, u: &UserContext) -> PushDeviceView {
    let (_, p256dh) = moosuri_connectors::push::generate_vapid().unwrap();
    s.enroll_push(
        u,
        PushEnrollment {
            operation_revision: 1,
            request_id: Uuid::new_v4().to_string(),
            device_id: Uuid::new_v4().to_string(),
            subscription: PushSubscription {
                endpoint: format!("https://fcm.googleapis.com/fcm/send/{}", Uuid::new_v4()),
                p256dh,
                auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- synthetic Push fixture
            },
        },
        "https://push-owner.example.test".into(),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn saved_revision_reaches_inbox_push_and_edit_fences_dispatch_and_held_reminder() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let other = member(&s, &u, "followup-other@example.test").await.user;
    let binding = enroll(&s, &u).await;
    let at = now().unwrap() + 60;
    let request = input(Some(at), Some(at + 60), None);
    let r = s.write_artifact(&u, request.clone()).await.unwrap();
    assert_eq!(
        s.write_artifact(&u, request.clone()).await.unwrap().id,
        r.id
    );
    assert!(s.schedules(&u).await.unwrap().is_empty());
    assert!(s.artifact(&other, r.id.clone(), None).await.is_err());
    let saved = s.artifact(&u, r.id.clone(), None).await.unwrap();
    assert_eq!(saved.reminders.unwrap().len(), 2);
    let n = deliver(&s, at).await;
    let target = n.followup.as_ref().unwrap();
    assert_eq!(
        (
            &target.artifact_id,
            target.artifact_revision,
            target.date_kind.as_str(),
            target.date.utc
        ),
        (&r.id, 1, "due", at)
    );
    assert_eq!(n.source.as_ref().unwrap().kind, "followup");
    assert!(
        s.notifications(&other, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        s.control_schedule(
            &u,
            n.schedule_id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "pause".into()
            }
        )
        .await
        .is_err()
    );
    s.poll_push_queue().await.unwrap();
    let push = s.claim_push().await.unwrap().unwrap();
    let payload = String::from_utf8_lossy(&push.payload);
    assert!(!payload.contains("canary") && !payload.contains("Synthetic body"));
    s.commit_push_dispatch(&push.lease).await.unwrap();
    s.authorize_push_display(&u, binding.binding_id.clone(), n.id.clone())
        .await
        .unwrap();
    s.enqueue_scheduled_reminders_at(at + 60).await.unwrap();
    let held = s
        .claim_scheduled_reminder_at(at + 60)
        .await
        .unwrap()
        .unwrap();
    let mut changed = edit(request, &r);
    changed.draft.title = "Current revision".into();
    s.write_artifact(&u, changed).await.unwrap();
    assert!(
        s.finish_scheduled_reminder_at(&held, at + 60)
            .await
            .is_err()
    );
    assert!(
        s.authorize_push_display(&u, binding.binding_id, n.id)
            .await
            .is_err()
    );
    s.finish_push(&push.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    assert!(
        s.notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let history = s.notifications(&u, None, true).await.unwrap();
    assert!(
        history
            .items
            .iter()
            .all(|n| !n.title.contains("canary") && n.followup.is_none())
    );
}
#[tokio::test]
async fn past_dates_are_briefing_only_snooze_rearms_and_offline_dates_coalesce_without_restart_replay()
 {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let at = now().unwrap();
    let past = input(Some(at - 60), Some(at - 30), None);
    let r = s.write_artifact(&u, past.clone()).await.unwrap();
    assert!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .reminders
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 5).await.unwrap(), 0);
    let future = at + 90;
    let r2 = s
        .write_artifact(
            &u,
            edit(input(Some(at - 60), Some(at - 30), Some(future)), &r),
        )
        .await
        .unwrap();
    assert_eq!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .reminders
            .unwrap()
            .len(),
        1
    );
    let n = deliver(&s, future).await;
    assert_eq!(n.followup.unwrap().date_kind, "snooze");
    assert!(
        s.claim_scheduled_reminder_at(future)
            .await
            .unwrap()
            .is_none()
    );
    let both = s
        .write_artifact(&u, input(Some(at + 120), Some(at + 150), None))
        .await
        .unwrap();
    s.enqueue_scheduled_reminders_at(at + 180).await.unwrap();
    let held = s
        .claim_scheduled_reminder_at(at + 180)
        .await
        .unwrap()
        .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert!(
        s.finish_scheduled_reminder_at(&held, at + 180)
            .await
            .is_err()
    );
    let n = deliver(&s, at + 180).await;
    assert_eq!(n.followup.unwrap().date_kind, "review");
    assert!(
        s.claim_scheduled_reminder_at(at + 180)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 180).await.unwrap(), 0);
    assert_eq!(
        s.artifact(&u, both.id, None)
            .await
            .unwrap()
            .reminders
            .unwrap()
            .len(),
        2
    );
    // An old mutation receipt is idempotent even after its event was delivered.
    assert_eq!(
        s.artifact(&u, r2.id, None).await.unwrap().artifact.revision,
        2
    );
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 180).await.unwrap(), 0);
    assert!(
        s.claim_scheduled_reminder_at(at + 180)
            .await
            .unwrap()
            .is_none()
    );
}
#[tokio::test]
async fn artifact_controls_delete_and_recursive_withdrawal_retire_unscanned_queued_and_claimed_push()
 {
    let _g = TEST_LOCK.lock().await;
    for (i, mode) in [
        "resolved",
        "cancelled",
        "muted",
        "snoozed",
        "deleted",
        "withdrawn",
    ]
    .into_iter()
    .enumerate()
    {
        let d = directory();
        let s = Store::open(d.path()).unwrap();
        let u = admin(&s).await.user;
        enroll(&s, &u).await;
        let at = now().unwrap() + 60;
        let mut v = input(Some(at), None, None);
        let parent = if mode == "withdrawn" {
            let mut p = input(None, None, None);
            p.draft.payload = ArtifactPayload::Document;
            let r = s.write_artifact(&u, p).await.unwrap();
            v.draft.references.push(ArtifactDependency {
                kind: ArtifactDependencyKind::Artifact,
                id: r.id.clone(),
                revision: 1,
            });
            Some(r)
        } else {
            None
        };
        let r = s.write_artifact(&u, v.clone()).await.unwrap();
        let n = deliver(&s, at).await;
        let held = if i % 3 == 0 {
            None
        } else {
            s.poll_push_queue().await.unwrap();
            if i % 3 == 2 {
                Some(s.claim_push().await.unwrap().unwrap())
            } else {
                None
            }
        };
        if mode == "deleted" {
            s.delete_artifact(
                &u,
                r.id.clone(),
                ArtifactDelete {
                    request_id: Uuid::new_v4().to_string(),
                    expected_revision: 1,
                },
            )
            .await
            .unwrap();
        } else if let Some(parent) = parent {
            let mut p = input(None, None, None);
            p.draft.payload = ArtifactPayload::Document;
            s.write_artifact(&u, edit(p, &parent)).await.unwrap();
        } else {
            let mut v = edit(v, &r);
            if let ArtifactPayload::Followup {
                lifecycle,
                muted,
                snooze,
                ..
            } = &mut v.draft.payload
            {
                match mode {
                    "resolved" => *lifecycle = FollowupLifecycle::Resolved,
                    "cancelled" => *lifecycle = FollowupLifecycle::Cancelled,
                    "muted" => *muted = true,
                    _ => {
                        *snooze = Some(ArtifactDate {
                            utc: at + 120,
                            timezone: "UTC".into(),
                        })
                    }
                }
            }
            s.write_artifact(&u, v).await.unwrap();
        }
        if let Some(held) = held {
            assert!(s.commit_push_dispatch(&held.lease).await.is_err(), "{mode}");
        }
        s.poll_push_queue().await.unwrap();
        assert!(s.claim_push().await.unwrap().is_none(), "{mode}");
        assert!(
            s.notifications(&u, None, false)
                .await
                .unwrap()
                .items
                .is_empty(),
            "{mode}"
        );
        let historical = s.notifications(&u, None, true).await.unwrap();
        assert_eq!(historical.items[0].id, n.id);
        assert!(historical.items[0].followup.is_none());
        if mode == "snoozed" {
            let later = deliver(&s, at + 120).await;
            assert_eq!(later.followup.unwrap().date_kind, "snooze");
        } else {
            assert_eq!(
                s.enqueue_scheduled_reminders_at(at + 180).await.unwrap(),
                0,
                "{mode}"
            );
        }
    }
}
#[tokio::test]
async fn stable_record_source_mute_survives_new_dates_edits_and_unmute_never_replays() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    enroll(&s, &u).await;
    let at = now().unwrap() + 60;
    let v = input(Some(at), Some(at + 60), None);
    let r = s.write_artifact(&u, v.clone()).await.unwrap();
    let n = deliver(&s, at).await;
    let source = n.source.unwrap();
    s.set_notification_source(
        &u,
        source.id.clone(),
        NotificationSourceChange {
            expected_revision: 0,
            muted: true,
        },
    )
    .await
    .unwrap();
    let r2 = s
        .write_artifact(&u, edit(input(Some(at + 120), None, None), &r))
        .await
        .unwrap();
    let n2 = deliver(&s, at + 120).await;
    assert!(n2.source.as_ref().unwrap().muted);
    assert_eq!(n2.source.unwrap().id, source.id);
    assert!(
        s.notifications(&u, None, true)
            .await
            .unwrap()
            .items
            .iter()
            .all(|n| n.id != n2.id)
    );
    s.set_notification_source(
        &u,
        source.id.clone(),
        NotificationSourceChange {
            expected_revision: 1,
            muted: false,
        },
    )
    .await
    .unwrap();
    s.poll_push_queue().await.unwrap();
    assert!(s.claim_push().await.unwrap().is_none());
    s.write_artifact(&u, edit(input(Some(at + 180), None, None), &r2))
        .await
        .unwrap();
    let n3 = deliver(&s, at + 180).await;
    assert!(!n3.source.unwrap().muted);
    s.poll_push_queue().await.unwrap();
    assert!(s.claim_push().await.unwrap().is_some());
}
#[tokio::test]
async fn quiet_hours_keep_inbox_without_push_and_equal_dates_form_one_receipt() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    enroll(&s, &u).await;
    let at = now().unwrap() + 60;
    let minute = ((at % 86400) / 60) as u16;
    s.set_quiet_hours(
        &u,
        QuietHoursChange {
            expected_revision: 0,
            enabled: true,
            timezone: "UTC".into(),
            start_minute: minute,
            end_minute: (minute + 2) % 1440,
        },
    )
    .await
    .unwrap();
    let r = s
        .write_artifact(&u, input(Some(at), Some(at), None))
        .await
        .unwrap();
    assert_eq!(
        s.artifact(&u, r.id, None)
            .await
            .unwrap()
            .reminders
            .unwrap()
            .len(),
        1
    );
    let n = deliver(&s, at).await;
    assert!(n.quiet);
    assert_eq!(n.followup.unwrap().date_kind, "due_review");
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items.len(),
        1
    );
    s.poll_push_queue().await.unwrap();
    assert!(s.claim_push().await.unwrap().is_none());
}
#[tokio::test]
async fn upgrade_never_backfills_existing_dates_until_explicit_resave() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let at = now().unwrap() + 3600;
    let v = input(Some(at), None, None);
    let r = s.write_artifact(&u, v.clone()).await.unwrap();
    // Reconstruct the pre-followup snapshot: the record exists without its scheduler.
    s.call(|db| {
        db.conn.execute_batch("DELETE FROM followup_reminders;DELETE FROM reminder_schedules;DROP TABLE conversation_recall_dependencies;DROP TABLE followup_reminders;")?;
        db.conn.pragma_update(None,"user_version",45)?;
        Ok(())
    }).await.unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .reminders
            .unwrap()
            .is_empty()
    );
    assert_eq!(s.enqueue_scheduled_reminders_at(at).await.unwrap(), 0);
    s.write_artifact(&u, edit(v, &r)).await.unwrap();
    let n = deliver(&s, at).await;
    assert_eq!(n.followup.unwrap().artifact_revision, 2);
}
