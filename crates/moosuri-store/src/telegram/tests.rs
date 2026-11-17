use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use moosuri_connectors::telegram::{Chat, Message};
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind, RunState};
fn update(id: i64, person: i64, text: &str) -> Update {
    Update {
        update_id: id,
        message: Some(Message {
            message_id: id + 1,
            date: now().unwrap(),
            from: Some(Identity {
                id: person,
                is_bot: false,
                username: None,
            }),
            chat: Chat {
                id: person,
                kind: "private".into(),
            },
            text: Some(text.into()),
        }),
    }
}
async fn setup() -> (tempfile::TempDir, Store, UserContext) {
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let a = admin(&s).await.user;
    s.configure_telegram(
        &a,
        0,
        Identity {
            id: 12345,
            is_bot: true,
            username: Some("SyntheticMoosuriBot".into()),
        },
        Zeroizing::new("12345:synthetic_token_for_tests_only".into()),
    )
    .await
    .unwrap();
    let p = s
        .create_provider(
            &a,
            ProviderChange {
                name: "synthetic".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    s.record_model_probe(&a, p.id.clone(), p.revision, "fixture".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &a,
        Some(ModelSelection {
            source: Default::default(),
            provider_id: p.id,
            provider_revision: p.revision,
            model: "fixture".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
    (dir, s, a)
}
async fn pair(s: &Store, u: &UserContext, id: i64, person: i64) {
    let p = s
        .pair_telegram(
            u,
            PairInput {
                timezone: "Asia/Seoul".into(),
                notifications: true,
            },
        )
        .await
        .unwrap();
    s.ingest_telegram(1, update(id, person, &format!("/start {}", p.code)))
        .await
        .unwrap();
}
async fn flush(s: &Store) {
    s.call(|db| {
        db.conn.execute("DELETE FROM telegram_outbox", [])?;
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn telegram_pairing_is_private_one_use_and_account_scoped_with_atomic_input_replay() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, a) = setup().await;
    let b = member(&s, &a, "second@example.test").await.user;
    let p = s
        .pair_telegram(
            &a,
            PairInput {
                timezone: "UTC".into(),
                notifications: false,
            },
        )
        .await
        .unwrap();
    let mut group = update(1, 111, &format!("/start {}", p.code));
    group.message.as_mut().unwrap().chat.kind = "group".into();
    s.ingest_telegram(1, group).await.unwrap();
    assert!(
        s.telegram_settings(&a, false)
            .await
            .unwrap()
            .binding
            .is_none()
    );
    s.ingest_telegram(1, update(2, 111, &format!("/start {}", p.code)))
        .await
        .unwrap();
    s.ingest_telegram(1, update(3, 222, &format!("/start {}", p.code)))
        .await
        .unwrap();
    assert!(
        s.telegram_settings(&b, false)
            .await
            .unwrap()
            .binding
            .is_none()
    );
    pair(&s, &b, 4, 222).await;
    assert!(
        s.ingest_telegram(1, update(5, 111, "private first account"))
            .await
            .unwrap()
    );
    assert!(
        !s.ingest_telegram(1, update(5, 111, "private first account"))
            .await
            .unwrap()
    );
    let ta = s
        .telegram_settings(&a, false)
        .await
        .unwrap()
        .binding
        .unwrap()
        .thread_id
        .unwrap();
    assert_eq!(
        s.conversation(&a, ta.clone(), None)
            .await
            .unwrap()
            .turns
            .len(),
        1
    );
    assert!(matches!(
        s.conversation(&b, ta, None).await,
        Err(StoreError::NotFound)
    ));
    s.ingest_telegram(1, update(6, 222, "private second account"))
        .await
        .unwrap();
    let tb = s
        .telegram_settings(&b, false)
        .await
        .unwrap()
        .binding
        .unwrap()
        .thread_id
        .unwrap();
    assert_eq!(
        s.conversation(&b, tb, None).await.unwrap().turns[0].user_text,
        "private second account"
    );
    // Browser logout leaves the independently paired session working.
    s.logout(&a).await.unwrap();
    s.ingest_telegram(1, update(7, 111, "/stop")).await.unwrap();
    let restored = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    s.disconnect_telegram(&restored).await.unwrap();
    assert!(
        !s.ingest_telegram(1, update(8, 111, "must never be admitted"))
            .await
            .unwrap()
    );
    assert!(
        s.telegram_settings(&restored, false)
            .await
            .unwrap()
            .binding
            .is_none()
    );
    assert!(
        s.telegram_settings(&b, false)
            .await
            .unwrap()
            .binding
            .is_some()
    );
    assert!(matches!(
        s.disable_telegram(&b, 1).await,
        Err(StoreError::Forbidden)
    ));
}
#[tokio::test]
async fn telegram_delivery_unknown_disconnect_and_recovery_do_not_replay_private_messages() {
    let _g = TEST_LOCK.lock().await;
    let (d, s, a) = setup().await;
    pair(&s, &a, 1, 111).await;
    flush(&s).await;
    s.ingest_telegram(1, update(2, 111, "Synthetic request"))
        .await
        .unwrap();
    let claim = s.claim_run().await.unwrap().unwrap();
    s.append_output(
        &claim.lease,
        vec![crate::OutputPatch::Append {
            item: "result".into(),
            text: "Synthetic private answer".into(),
            phase: None,
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.finish_run(&claim.lease, RunState::Completed, None)
        .await
        .unwrap();
    s.poll_telegram_delivery(1).await.unwrap();
    let dispatch = s.claim_telegram_dispatch(1, None).await.unwrap().unwrap();
    assert_eq!(dispatch.text, "Synthetic private answer");
    s.finish_telegram_dispatch(&dispatch, Err(TransportError::Unknown))
        .await
        .unwrap();
    assert!(s.claim_telegram_dispatch(1, None).await.unwrap().is_none());
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert!(s.claim_telegram_dispatch(1, None).await.unwrap().is_none());
    let a = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert_eq!(
        s.telegram_settings(&a, false)
            .await
            .unwrap()
            .binding
            .unwrap()
            .last_delivery
            .as_deref(),
        Some("unknown")
    );
    s.ingest_telegram(1, update(3, 111, "/help")).await.unwrap();
    s.disconnect_telegram(&a).await.unwrap();
    assert!(s.claim_telegram_dispatch(1, None).await.unwrap().is_none());
    pair(&s, &a, 4, 111).await;
    s.call(|db| {
        quarantine(&db.conn)?;
        assert!(!db.conn.prepare("PRAGMA foreign_key_check")?.exists([])?);
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.telegram_runtime().await.unwrap().is_none());
    assert!(
        s.telegram_settings(&a, false)
            .await
            .unwrap()
            .binding
            .is_none()
    );
}
#[tokio::test]
async fn telegram_expired_pairing_epoch_and_muted_notifications_are_fenced() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, a) = setup().await;
    let p = s
        .pair_telegram(
            &a,
            PairInput {
                timezone: "UTC".into(),
                notifications: true,
            },
        )
        .await
        .unwrap();
    s.call(|db| {
        db.conn
            .execute("UPDATE telegram_pairings SET expires_at=0", [])?;
        Ok(())
    })
    .await
    .unwrap();
    s.ingest_telegram(1, update(1, 111, &format!("/start {}", p.code)))
        .await
        .unwrap();
    assert!(
        s.telegram_settings(&a, false)
            .await
            .unwrap()
            .binding
            .is_none()
    );
    pair(&s, &a, 2, 111).await;
    flush(&s).await;
    let schedule = s
        .create_schedule(
            &a,
            moosuri_core::ScheduleCreate {
                request_id: Uuid::new_v4().to_string(),
                draft: moosuri_core::ScheduleDraft {
                    title: "Synthetic notification".into(),
                    message: "Private reminder body".into(),
                    rule: moosuri_core::ScheduleRule::Interval {
                        anchor: now().unwrap(),
                        seconds: 3600,
                    },
                    valid_until: None,
                },
            },
        )
        .await
        .unwrap();
    let at = now().unwrap() + 3601;
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    let lease = s.claim_scheduled_reminder_at(at).await.unwrap().unwrap();
    s.finish_scheduled_reminder_at(&lease, at).await.unwrap();
    s.poll_telegram_delivery(1).await.unwrap();
    let d = s
        .claim_telegram_dispatch(1, Some("https://assistant.example.test".into()))
        .await
        .unwrap()
        .unwrap();
    assert!(!d.text.contains("synthetic-private"));
    assert!(
        d.text
            .contains("https://assistant.example.test/app/schedules")
    );
    s.finish_telegram_dispatch(&d, Ok(())).await.unwrap();
    flush(&s).await;
    s.set_notification_source(
        &a,
        schedule.id.clone(),
        moosuri_core::NotificationSourceChange {
            expected_revision: 0,
            muted: true,
        },
    )
    .await
    .unwrap();
    let at = now().unwrap() + 7201;
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    let lease = s.claim_scheduled_reminder_at(at).await.unwrap().unwrap();
    s.finish_scheduled_reminder_at(&lease, at).await.unwrap();
    s.poll_telegram_delivery(1).await.unwrap();
    assert!(s.claim_telegram_dispatch(1, None).await.unwrap().is_none());
    let renewed = s
        .change_password(&a, "Changed-telegram-test-password".into())
        .await
        .unwrap()
        .user;
    assert!(
        s.telegram_settings(&renewed, false)
            .await
            .unwrap()
            .binding
            .is_none()
    );
    assert!(
        !s.ingest_telegram(1, update(3, 111, "revoked input"))
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn telegram_stop_targets_the_delegated_task_after_its_parent_has_completed() {
    use moosuri_core::*;
    let _g = TEST_LOCK.lock().await;
    let (_d, s, a) = crate::missions::tests::setup().await;
    s.configure_telegram(
        &a,
        0,
        Identity {
            id: 12345,
            is_bot: true,
            username: Some("SyntheticMoosuriBot".into()),
        },
        Zeroizing::new("12345:synthetic_token_for_tests_only".into()),
    )
    .await
    .unwrap();
    pair(&s, &a, 1, 111).await;
    flush(&s).await;
    s.ingest_telegram(1, update(2, 111, "Calculate synthetic total"))
        .await
        .unwrap();
    let thread = s
        .telegram_settings(&a, false)
        .await
        .unwrap()
        .binding
        .unwrap()
        .thread_id
        .unwrap();
    let origin = s
        .conversation(&a, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    let mission = s
        .create_mission(&a, crate::missions::tests::input(MissionMode::Durable))
        .await
        .unwrap();
    let receipt = MissionChatReceipt {
        request_id: origin.run.request_id,
        action: "create_now".into(),
        target: Some(MissionChatTarget {
            kind: "one_off".into(),
            id: mission.id.clone(),
            revision: mission.revision,
        }),
        state: "queued".into(),
        next_local_slot: None,
        candidates: vec![],
    };
    let t = a.tenant.0.clone();
    let id = origin.run.id;
    s.call(move |db| {
        let r = crate::chat::load_run(&db.conn, &t, &id)?;
        db.conn.execute(
            "INSERT INTO mission_chat_commands VALUES(?1,?2,?3)",
            params![t, id, serde_json::to_string(&receipt).unwrap()],
        )?;
        crate::chat::transition(&db.conn, &r, RunState::Completed, None)?;
        Ok(())
    })
    .await
    .unwrap();
    s.ingest_telegram(1, update(3, 111, "/status"))
        .await
        .unwrap();
    s.call(|db| {
        let body: String = db.conn.query_row(
            "SELECT body FROM telegram_outbox WHERE dedupe='update:3'",
            [],
            |r| r.get(0),
        )?;
        assert!(body.contains("진행 중"));
        Ok(())
    })
    .await
    .unwrap();
    s.ingest_telegram(1, update(4, 111, "/stop")).await.unwrap();
    let mid = mission.id;
    s.call(move|db|{let state:(String,bool,String)=db.conn.query_row("SELECT m.state,m.grant_enabled,r.state FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id WHERE m.id=?1",[mid],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;assert_eq!(state,("cancelled".into(),false,"cancelled".into()));Ok(())}).await.unwrap();
}
#[tokio::test]
async fn telegram_web_retry_tracks_only_the_new_attempt_and_sends_its_result() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, a) = setup().await;
    pair(&s, &a, 1, 111).await;
    flush(&s).await;
    s.ingest_telegram(1, update(2, 111, "Synthetic retry question"))
        .await
        .unwrap();
    let claim = s.claim_run().await.unwrap().unwrap();
    s.finish_run(&claim.lease, RunState::Failed, None)
        .await
        .unwrap();
    s.poll_telegram_delivery(1).await.unwrap();
    flush(&s).await;
    let r = s
        .retry_run(&a, claim.lease.run_id().into(), 1)
        .await
        .unwrap();
    assert_eq!(r.run.attempt, 2);
    let c = s.claim_run().await.unwrap().unwrap();
    s.append_output(
        &c.lease,
        vec![crate::OutputPatch::Append {
            item: "retry".into(),
            text: "Recovered answer".into(),
            phase: None,
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.finish_run(&c.lease, RunState::Completed, None)
        .await
        .unwrap();
    s.poll_telegram_delivery(1).await.unwrap();
    assert_eq!(
        s.claim_telegram_dispatch(1, None)
            .await
            .unwrap()
            .unwrap()
            .text,
        "Recovered answer"
    );
}
