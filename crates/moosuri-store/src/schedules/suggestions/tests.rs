use super::*;
use crate::push::PushEnrollment;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_connectors::push::{PushOutcome, PushSubscription};
fn request() -> ArtifactWrite {
    ArtifactWrite {
        request_id: Uuid::new_v4().to_string(),
        id: None,
        expected_revision: None,
        draft: ArtifactDraft {
            title: "Synthetic pending reply".into(),
            body: "Synthetic private canary".into(),
            references: vec![],
            payload: ArtifactPayload::Followup {
                record_kind: FollowupKind::WaitingForReply,
                person: "Synthetic person".into(),
                lifecycle: FollowupLifecycle::Waiting,
                progress: FollowupProgress::None,
                muted: false,
                due: None,
                review: None,
                snooze: None,
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
    let l = s.claim_scheduled_reminder_at(at).await.unwrap().unwrap();
    s.finish_scheduled_reminder_at(&l, at).await.unwrap()
}
async fn enroll(s: &Store, u: &UserContext) -> PushDeviceView {
    let (_, key) = moosuri_connectors::push::generate_vapid().unwrap();
    s.enroll_push(
        u,
        PushEnrollment {
            operation_revision: 1,
            request_id: Uuid::new_v4().to_string(),
            device_id: Uuid::new_v4().to_string(),
            subscription: PushSubscription {
                endpoint: format!("https://fcm.googleapis.com/fcm/send/{}", Uuid::new_v4()),
                p256dh: key,
                auth: "_ordMnz7uTCmrpBTeUV4Bw".into(), // gitleaks:allow -- fixed public synthetic Push fixture
            },
        },
        "https://synthetic.example.test".into(),
    )
    .await
    .unwrap()
}
#[tokio::test]
async fn saved_evidence_suggestion_is_owned_provider_free_and_edit_retires_dispatched_display_without_replay()
 {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let other = member(&s, &u, "suggestions-other@example.test").await.user;
    let b = enroll(&s, &u).await;
    let v = request();
    let r = s.write_artifact(&u, v.clone()).await.unwrap();
    assert_eq!(s.write_artifact(&u, v.clone()).await.unwrap().id, r.id);
    assert!(s.schedules(&u).await.unwrap().is_empty());
    let n = deliver(&s, now().unwrap() + 90).await;
    assert_eq!(n.suggestion.as_ref().unwrap().artifact_id, r.id);
    assert_eq!(n.suggestion.as_ref().unwrap().topic, TOPIC);
    assert_eq!(n.source.as_ref().unwrap().kind, "proactive_topic");
    assert!(n.followup.is_none());
    assert!(
        s.notifications(&other, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        s.mark_notification(&other, n.id.clone(), true)
            .await
            .is_err()
    );
    assert!(
        s.set_notification_source(
            &other,
            n.source.as_ref().unwrap().id.clone(),
            NotificationSourceChange {
                expected_revision: 0,
                muted: true
            }
        )
        .await
        .is_err()
    );
    assert!(
        s.control_schedule(
            &u,
            n.schedule_id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "resume".into()
            }
        )
        .await
        .is_err()
    );
    s.poll_push_queue().await.unwrap();
    let p = s.claim_push().await.unwrap().unwrap();
    assert!(!String::from_utf8_lossy(&p.payload).contains("Synthetic"));
    s.commit_push_dispatch(&p.lease).await.unwrap();
    s.authorize_push_display(&u, b.binding_id.clone(), n.id.clone())
        .await
        .unwrap();
    s.write_artifact(&u, edit(v, &r)).await.unwrap();
    assert!(
        s.authorize_push_display(&u, b.binding_id, n.id)
            .await
            .is_err()
    );
    s.finish_push(&p.lease, PushOutcome::Accepted)
        .await
        .unwrap();
    assert_eq!(
        s.enqueue_scheduled_reminders_at(now().unwrap() + 180)
            .await
            .unwrap(),
        0
    );
    assert!(
        s.notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let old = s.notifications(&u, None, true).await.unwrap();
    assert!(
        old.items
            .iter()
            .all(|n| n.suggestion.is_none() && !n.title.contains("Synthetic"))
    );
    s.call(|db| {
        for table in [
            "chat_runs",
            "missions",
            "mail_actions",
            "calendar_actions",
            "model_call_receipts",
        ] {
            let count: i64 =
                db.conn
                    .query_row(&format!("SELECT count(*) FROM {table}"), [], |r| r.get(0))?;
            assert_eq!(count, 0, "{table}");
        }
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn product_topic_mutes_distinct_records_but_not_date_alerts_and_unmute_never_replays() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    enroll(&s, &u).await;
    let at = now().unwrap() + 90;
    s.write_artifact(&u, request()).await.unwrap();
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
    s.write_artifact(&u, request()).await.unwrap();
    let muted = deliver(&s, at + 1).await;
    assert_eq!(muted.source.as_ref().unwrap().id, source.id);
    assert!(muted.source.unwrap().muted);
    assert!(
        s.notifications(&u, None, true)
            .await
            .unwrap()
            .items
            .iter()
            .all(|n| n.id != muted.id)
    );
    let mut dated = request();
    if let ArtifactPayload::Followup { due, .. } = &mut dated.draft.payload {
        *due = Some(ArtifactDate {
            utc: at + 120,
            timezone: "UTC".into(),
        })
    };
    s.write_artifact(&u, dated).await.unwrap();
    let date = deliver(&s, at + 120).await;
    assert!(date.followup.is_some());
    assert!(!date.source.unwrap().muted);
    assert!(
        s.set_notification_source(
            &u,
            source.id.clone(),
            NotificationSourceChange {
                expected_revision: 0,
                muted: false
            }
        )
        .await
        .is_err()
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
    assert!(s.muted_notification_sources(&u).await.unwrap().is_empty());
    assert!(
        s.notifications(&u, None, true)
            .await
            .unwrap()
            .items
            .iter()
            .all(|n| n.id != muted.id)
    );
    // Previously published suggestion Push stays latched even after unmute.
    s.poll_push_queue().await.unwrap();
    let p = s.claim_push().await.unwrap().unwrap();
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&p.payload).unwrap()["notification_id"]
            .as_str(),
        Some(date.id.as_str())
    );
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 180).await.unwrap(), 0);
}
#[tokio::test]
async fn quiet_inbox_hiding_releases_five_card_capacity_without_record_or_execution_changes() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    enroll(&s, &u).await;
    let at = now().unwrap() + 90;
    let minute = ((at % 86400) / 60) as u16;
    s.set_quiet_hours(
        &u,
        QuietHoursChange {
            expected_revision: 0,
            enabled: true,
            timezone: "UTC".into(),
            start_minute: minute,
            end_minute: (minute + 3) % 1440,
        },
    )
    .await
    .unwrap();
    let mut saved = vec![];
    for _ in 0..6 {
        let v = request();
        let r = s.write_artifact(&u, v.clone()).await.unwrap();
        saved.push((v, r));
    }
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    for _ in 0..5 {
        let l = s.claim_scheduled_reminder_at(at).await.unwrap().unwrap();
        assert!(s.finish_scheduled_reminder_at(&l, at).await.unwrap().quiet)
    }
    assert!(s.claim_scheduled_reminder_at(at).await.unwrap().is_none());
    let cards = s.notifications(&u, None, false).await.unwrap().items;
    assert_eq!(cards.len(), 5);
    s.poll_push_queue().await.unwrap();
    assert!(s.claim_push().await.unwrap().is_none());
    s.mark_notification(&u, cards[0].id.clone(), true)
        .await
        .unwrap();
    let (v, r) = saved.last().unwrap();
    let next = s.write_artifact(&u, edit(v.clone(), r)).await.unwrap();
    deliver(&s, at + 5).await;
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items.len(),
        5
    );
    assert_eq!(
        s.artifact(&u, r.id.clone(), None)
            .await
            .unwrap()
            .artifact
            .revision,
        next.revision
    );
    // An individually hidden identity never reappears from an edit.
    let id = cards[0].suggestion.as_ref().unwrap().artifact_id.clone();
    let (v, r) = saved.iter().find(|(_, r)| r.id == id).unwrap();
    s.write_artifact(&u, edit(v.clone(), r)).await.unwrap();
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 180).await.unwrap(), 0);
}
#[tokio::test]
async fn held_publication_and_transitive_evidence_withdrawal_remove_reference_and_push() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    enroll(&s, &u).await;
    let mut parent = request();
    parent.draft.payload = ArtifactPayload::Document;
    let pr = s.write_artifact(&u, parent.clone()).await.unwrap();
    let mut v = request();
    v.draft.references.push(ArtifactDependency {
        kind: ArtifactDependencyKind::Artifact,
        id: pr.id.clone(),
        revision: 1,
    });
    let r = s.write_artifact(&u, v.clone()).await.unwrap();
    let at = now().unwrap() + 90;
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    let l = s.claim_scheduled_reminder_at(at).await.unwrap().unwrap();
    s.write_artifact(&u, edit(v, &r)).await.unwrap();
    assert!(s.finish_scheduled_reminder_at(&l, at).await.is_err());
    let n = deliver(&s, at + 2).await;
    s.poll_push_queue().await.unwrap();
    let p = s.claim_push().await.unwrap().unwrap();
    s.write_artifact(&u, edit(parent, &pr)).await.unwrap();
    assert!(s.commit_push_dispatch(&p.lease).await.is_err());
    assert!(
        s.notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert!(
        s.notifications(&u, None, true)
            .await
            .unwrap()
            .items
            .iter()
            .find(|old| old.id == n.id)
            .unwrap()
            .suggestion
            .is_none()
    );
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 180).await.unwrap(), 0);
}
#[tokio::test]
async fn restore_quarantines_pending_suggestion_and_preserves_topic_preference_until_explicit_save()
{
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    s.write_artifact(&u, request()).await.unwrap();
    let at = now().unwrap() + 90;
    let n = deliver(&s, at).await;
    let source = n.source.unwrap();
    s.set_notification_source(
        &u,
        source.id,
        NotificationSourceChange {
            expected_revision: 0,
            muted: true,
        },
    )
    .await
    .unwrap();
    let v = request();
    let r = s.write_artifact(&u, v.clone()).await.unwrap();
    s.call(|db| {
        let tx = db.conn.transaction()?;
        quarantine_for_restore(&tx)?;
        crate::push::quarantine_for_restore(&tx)?;
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(s.enqueue_scheduled_reminders_at(at + 120).await.unwrap(), 0);
    assert_eq!(s.muted_notification_sources(&u).await.unwrap().len(), 1);
    s.write_artifact(&u, edit(v, &r)).await.unwrap();
    let n = deliver(&s, at + 121).await;
    assert!(n.source.unwrap().muted);
    assert!(
        s.notifications(&u, None, true)
            .await
            .unwrap()
            .items
            .iter()
            .all(|old| old.id != n.id)
    );
}
#[tokio::test]
async fn account_deletion_clears_suggestion_topic_and_record_sidecars_without_touching_other_owner()
{
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let admin = admin(&s).await.user;
    let u = member(&s, &admin, "suggestions-delete@example.test")
        .await
        .user;
    s.write_artifact(&admin, request()).await.unwrap();
    s.write_artifact(&u, request()).await.unwrap();
    let revision = s
        .managed_accounts(&admin)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == u.account.id)
        .unwrap()
        .revision;
    let p = s
        .prepare_account_management(
            &admin,
            u.account.id.clone(),
            AccountManagementInput {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: revision,
                action: AccountManagementAction::Delete,
            },
        )
        .await
        .unwrap();
    s.apply_account_management(p).await.unwrap();
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        for table in ["proactive_topics", "proactive_suggestions"] {
            let count: i64 = db.conn.query_row(
                &format!("SELECT count(*) FROM {table} WHERE tenant_id=?1"),
                [&tenant],
                |r| r.get(0),
            )?;
            assert_eq!(count, 0);
        }
        Ok(())
    })
    .await
    .unwrap();
    assert!(deliver(&s, now().unwrap() + 90).await.suggestion.is_some());
}

#[tokio::test]
async fn sealed_schema56_and_current_snapshots_upgrade_without_backfill_or_restored_publication() {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let _g = TEST_LOCK.lock().await;
    for captured in [56, CURRENT_SCHEMA_VERSION] {
        let d = tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap();
        let data = d.path().join("source");
        let kit = d.path().join("kit");
        std::fs::DirBuilder::new().mode(0o700).create(&kit).unwrap();
        let s = Store::open(&data).unwrap();
        let u = admin(&s).await.user;
        s.write_artifact(&u, request()).await.unwrap();
        let topic = s
            .call(|db| {
                Ok(db
                    .conn
                    .query_row("SELECT schedule_id FROM proactive_topics", [], |r| {
                        r.get::<_, String>(0)
                    })?)
            })
            .await
            .unwrap();
        s.set_notification_source(
            &u,
            topic,
            NotificationSourceChange {
                expected_revision: 0,
                muted: true,
            },
        )
        .await
        .unwrap();
        drop(s);
        let key = kit.join("identity");
        let recipient = crate::backup::keygen(&key).unwrap();
        crate::backup::setup(&data, &recipient, &key).unwrap();
        let cipher = kit.join("checkpoint.age");
        let heads = d.path().join("heads");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&heads)
            .unwrap();
        let head = heads.join("head.json");
        crate::cutover::checkpoint(&data, &cipher, &head).unwrap();
        // Build a genuine predecessor capture from the valid current checkpoint;
        // current capture's schema floor is never weakened for this fixture.
        let (cipher, head) = if captured == 56 {
            predecessor56(&cipher, &head, &key, &kit)
        } else {
            (cipher, head)
        };
        let dest = d.path().join("restored");
        crate::cutover::restore::restore(&cipher, &head, &key, &dest).unwrap();
        let s = Store::open(&dest).unwrap();
        s.call(move |db| {
            assert_eq!(
                db.conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                CURRENT_SCHEMA_VERSION
            );
            let count: i64 =
                db.conn
                    .query_row("SELECT count(*) FROM proactive_suggestions", [], |r| {
                        r.get(0)
                    })?;
            assert_eq!(count, if captured == 56 { 0 } else { 1 });
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM reminder_schedules WHERE state='active'",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            if captured == CURRENT_SCHEMA_VERSION {
                assert_eq!(
                    db.conn.query_row(
                        "SELECT count(*) FROM notification_source_preferences WHERE muted=1",
                        [],
                        |r| r.get::<_, i64>(0)
                    )?,
                    1
                );
            }
            Ok(())
        })
        .await
        .unwrap();
        assert_eq!(
            s.enqueue_scheduled_reminders_at(now().unwrap() + 3600)
                .await
                .unwrap(),
            0
        );
        assert!(
            s.claim_scheduled_reminder_at(now().unwrap() + 3600)
                .await
                .unwrap()
                .is_none()
        );
    }
}

/// Same unpack/repack pattern as cutover::restore's accepted predecessor fixture.
fn predecessor56(
    cipher: &std::path::Path,
    head: &std::path::Path,
    key: &std::path::Path,
    kit: &std::path::Path,
) -> (std::path::PathBuf, std::path::PathBuf) {
    use crate::backup::files;
    use sha2::{Digest, Sha256};
    use std::io::{Read, Write};
    let hash = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let identity: age::x25519::Identity = std::fs::read_to_string(key)
        .unwrap()
        .lines()
        .find(|l| !l.starts_with('#') && !l.trim().is_empty())
        .unwrap()
        .parse()
        .unwrap();
    let plain = age::decrypt(&identity, &std::fs::read(cipher).unwrap()).unwrap();
    let mut items = tar::Archive::new(plain.as_slice())
        .entries()
        .unwrap()
        .map(|e| {
            let mut e = e.unwrap();
            let name = e.path().unwrap().to_string_lossy().into_owned();
            let mut bytes = vec![];
            e.read_to_end(&mut bytes).unwrap();
            (name, bytes)
        })
        .collect::<Vec<_>>();
    let mut manifest: crate::cutover::Manifest = serde_json::from_slice(&items[0].1).unwrap();
    assert_eq!(manifest.seal.schema, CURRENT_SCHEMA_VERSION);
    manifest.seal.schema = 56;
    let db = kit.join("predecessor56.sqlite");
    files::create_file(&db)
        .unwrap()
        .write_all(&items[1].1)
        .unwrap();
    let mut c = Connection::open(&db).unwrap();
    crate::schema::tests::replace_with_predecessor(&mut c, 56);
    c.execute_batch("DROP TRIGGER installation_cutover_monotonic;")
        .unwrap();
    c.execute(
        "UPDATE installation_cutover SET seal=?1",
        [serde_json::to_string(&manifest.seal).unwrap()],
    )
    .unwrap();
    c.execute_batch("CREATE TRIGGER installation_cutover_monotonic BEFORE UPDATE ON installation_cutover WHEN NOT ((OLD.phase='active' AND NEW.phase='sealed' AND NEW.singleton=1) OR (OLD.phase='sealed' AND NEW.phase='checkpoint_ready' AND NEW.singleton=1 AND NEW.seal=OLD.seal)) BEGIN SELECT RAISE(ABORT,'cutover transition forbidden'); END;").unwrap();
    drop(c);
    items[1].1 = std::fs::read(db).unwrap();
    items[5].1 = serde_json::to_vec(&manifest.seal).unwrap();
    manifest.seal_sha256 = hash(&items[5].1);
    for (entry, (_, bytes)) in manifest.entries.iter_mut().zip(items.iter().skip(1)) {
        entry.sha256 = hash(bytes);
        entry.size = bytes.len() as u64;
    }
    items[0].1 = serde_json::to_vec(&manifest).unwrap();
    let mut tar = tar::Builder::new(vec![]);
    for (name, bytes) in &items {
        let mut h = tar::Header::new_ustar();
        h.set_path(name).unwrap();
        h.set_mode(0o600);
        h.set_uid(0);
        h.set_gid(0);
        h.set_mtime(0);
        h.set_size(bytes.len() as u64);
        h.set_entry_type(tar::EntryType::Regular);
        h.set_cksum();
        tar.append(&h, bytes.as_slice()).unwrap();
    }
    tar.finish().unwrap();
    let encrypted = age::encrypt(&identity.to_public(), &tar.into_inner().unwrap()).unwrap();
    let mut trusted: crate::cutover::TrustedHead =
        serde_json::from_slice(&std::fs::read(head).unwrap()).unwrap();
    trusted.schema = 56;
    trusted.seal_sha256 = manifest.seal_sha256;
    trusted.manifest_sha256 = hash(&items[0].1);
    trusted.ciphertext_sha256 = hash(&encrypted);
    trusted.ciphertext_size = encrypted.len() as u64;
    let cipher = kit.join("predecessor56.age");
    let head = head.with_file_name("predecessor56.json");
    files::create_file(&cipher)
        .unwrap()
        .write_all(&encrypted)
        .unwrap();
    files::create_file(&head)
        .unwrap()
        .write_all(&serde_json::to_vec(&trusted).unwrap())
        .unwrap();
    (cipher, head)
}
