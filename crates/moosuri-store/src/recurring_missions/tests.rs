use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use crate::{PendingKnowledgeCall, PrivateCheckpoint};
fn input(at: i64) -> RecurringMissionCreate {
    RecurringMissionCreate {
        request_id: Uuid::new_v4().to_string(),
        template: MissionCreate {
            request_id: Uuid::new_v4().to_string(),
            task: "Calculate synthetic 0.1 plus 0.2".into(),
            constraints: String::new(),
            checks: vec![MissionCheck::DecimalEquals {
                expected: "0.3".into(),
            }],
            execution_mode: MissionMode::Durable,
        },
        schedule: ScheduleDraft {
            title: "Synthetic recurring task".into(),
            message: "Ignored; never published".into(),
            rule: ScheduleRule::Interval {
                anchor: at,
                seconds: 60,
            },
            valid_until: None,
        },
        changes_only: true,
    }
}
async fn setup() -> (tempfile::TempDir, Store, UserContext) {
    let d = directory();
    let s = Store::open(d.path()).unwrap();
    let u = admin(&s).await.user;
    let p = s
        .create_provider(
            &u,
            crate::ProviderChange {
                name: "Synthetic recurring fixture".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: crate::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    s.record_model_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.record_tool_probe(&u, p.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    s.set_default_model(
        &u,
        Some(ModelSelection {
            source: Default::default(),
            provider_id: p.id,
            provider_revision: 1,
            model: "synthetic".into(),
            effort: None,
        }),
    )
    .await
    .unwrap();
    (d, s, u)
}
async fn due(s: &Store, t: i64) -> u32 {
    s.enqueue_scheduled_reminders_at(t).await.unwrap();
    s.poll_recurring_missions_at(t).await.unwrap()
}
async fn exact(s: &Store, call_id: &str) -> String {
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"v":1}"#.to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: call_id.into(),
            input: crate::KnowledgeToolInput::DecimalCalculate {
                calculation: DecimalCalculation {
                    operation: DecimalOperation::Sum,
                    operands: vec!["0.1".into(), "0.2".into()],
                },
            },
        }],
    )
    .await
    .unwrap();
    s.mission_save_plan(&step, vec!["Calculate actual exact total".into()])
        .await
        .unwrap();
    s.execute_knowledge_tool(&step, call_id.into())
        .await
        .unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &final_step,
        PrivateCheckpoint {
            bytes: br#"{"v":1}"#.to_vec(),
        },
        vec![],
    )
    .await
    .unwrap();
    assert!(
        s.mission_verify(
            &final_step,
            MissionCandidate {
                text: "Exact total: 0.3".into(),
                evidence: vec![MissionEvidence {
                    step: step.step(),
                    call_id: call_id.into()
                }]
            }
        )
        .await
        .unwrap()
    );
    claim.lease.run_id().to_string()
}
#[tokio::test]
async fn recurring_atomic_rollback_duplicate_poll_restart_and_original_deadline() {
    let _g = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let mut request = input(at);
    request.schedule.valid_until = Some(at + 300);
    let saved = s
        .create_recurring_mission(&u, request.clone())
        .await
        .unwrap();
    assert_eq!(
        s.create_recurring_mission(&u, request.clone())
            .await
            .unwrap()
            .schedule
            .id,
        saved.schedule.id
    );
    request.template.task.push_str(" changed");
    assert!(matches!(
        s.create_recurring_mission(&u, request).await,
        Err(StoreError::Conflict)
    ));
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    // Simulated failure between child insertion and transaction commit must roll back both identities.
    s.call(move |db| {
        let tx = db.conn.transaction()?;
        assert_eq!(poll(&tx, &db.provider_key, at)?, 1);
        tx.rollback()?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.missions(&u).await.unwrap().is_empty());
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 0);
    let m = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(m.expires_at_ms, (at + 300) * 1000);
    assert_eq!(
        m.request.request_id,
        s.recurring_missions(&u).await.unwrap()[0].recent[0].occurrence_id
    );
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 0);
    assert_eq!(s.missions(&u).await.unwrap().len(), 1);
}
#[tokio::test]
async fn recurring_grants_are_isolated_from_every_generic_reminder_route() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let saved = s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert!(s.schedules(&u).await.unwrap().is_empty());
    assert!(matches!(
        s.control_schedule(
            &u,
            saved.schedule.id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "revoke".into()
            }
        )
        .await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        s.edit_schedule(
            &u,
            saved.schedule.id.clone(),
            ScheduleEdit {
                expected_revision: 1,
                draft: input(at).schedule
            }
        )
        .await,
        Err(StoreError::NotFound)
    ));
    assert!(matches!(
        s.schedule_history(&u, saved.schedule.id.clone()).await,
        Err(StoreError::NotFound)
    ));
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert!(s.claim_scheduled_reminder_at(at).await.unwrap().is_none());
    assert!(
        s.notifications(&u, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    let other = member(&s, &u, "other-recurring@example.test").await;
    assert!(matches!(
        s.control_recurring_mission(
            &other.user,
            saved.schedule.id,
            ScheduleControl {
                expected_revision: 1,
                action: "pause".into()
            }
        )
        .await,
        Err(StoreError::NotFound)
    ));
}
#[tokio::test]
async fn recurring_pinned_choice_busy_owner_fairness_and_latest_only() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    // A permanently busy owner with64 queued schedules must not hide the next eligible owner.
    for _ in 0..64 {
        s.create_recurring_mission(&u, input(at)).await.unwrap();
    }
    let other = member(&s, &u, "other-recurring@example.test").await;
    s.create_recurring_mission(&other.user, input(at))
        .await
        .unwrap();
    s.create_mission(&u, input(at).template).await.unwrap();
    // Changing default selection after explicit save cannot change the pinned recurring choice.
    s.set_default_model(&u, None).await.unwrap();
    s.enqueue_scheduled_reminders_at(at + 120).await.unwrap();
    s.enqueue_scheduled_reminders_at(at + 120).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at + 120).await.unwrap(), 1);
    let m = s.missions(&other.user).await.unwrap();
    assert_eq!(m.len(), 1);
    assert_eq!(m[0].expires_at_ms, (at + 120 + 86400) * 1000);
    let r = s.recurring_missions(&other.user).await.unwrap();
    assert_eq!(r[0].model.model, "synthetic");
    assert_eq!(r[0].recent[0].scheduled_for, at + 120);
    assert_eq!(s.poll_recurring_missions_at(at + 120).await.unwrap(), 0);
}
#[tokio::test]
async fn recurring_parent_pause_edit_and_retry_never_revive_child_authority() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let saved = s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    let paused = s
        .control_recurring_mission(
            &u,
            saved.schedule.id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "pause".into(),
            },
        )
        .await
        .unwrap();
    assert_eq!(paused.recent[0].state, "unknown");
    assert!(s.agent_context(&claim.lease).await.is_err());
    assert!(
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"v":1}"#.to_vec()
            },
            vec![]
        )
        .await
        .is_err()
    );
    let resume = ScheduleControl {
        expected_revision: 2,
        action: "resume".into(),
    };
    s.control_recurring_mission(&u, saved.schedule.id.clone(), resume.clone())
        .await
        .unwrap();
    assert_eq!(due(&s, at + 60).await, 1);
    // Lost-ACK resume retries cannot cancel a new occurrence admitted under its new revision.
    s.control_recurring_mission(&u, saved.schedule.id.clone(), resume)
        .await
        .unwrap();
    assert!(
        s.missions(&u)
            .await
            .unwrap()
            .iter()
            .any(|m| m.state == "queued")
    );
    let mut edit = input(at).schedule;
    edit.rule = ScheduleRule::Daily {
        time: "09:00".into(),
        timezone: "Asia/Seoul".into(),
    };
    s.edit_recurring_mission(
        &u,
        saved.schedule.id.clone(),
        ScheduleEdit {
            expected_revision: 3,
            draft: edit,
        },
    )
    .await
    .unwrap();
    assert!(
        s.missions(&u)
            .await
            .unwrap()
            .iter()
            .all(|m| m.state != "queued")
    );
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}
#[tokio::test]
async fn recurring_exact_comparison_preserves_history_and_never_hides_failure_or_uncertainty() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let saved = s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    exact(&s, "dispatch-A").await;
    s.poll_recurring_missions_at(at).await.unwrap();
    let inbox = s.notifications(&u, None, false).await.unwrap();
    assert_eq!(inbox.items.len(), 1);
    assert!(!inbox.items[0].message.contains("0.3"));
    assert!(!inbox.items[0].message.contains("Calculate synthetic"));
    assert!(inbox.items[0].mission_id.is_some());
    assert_eq!(due(&s, at + 60).await, 1);
    exact(&s, "different-dispatch-B").await;
    s.poll_recurring_missions_at(at + 60).await.unwrap();
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items.len(),
        1
    );
    assert!(s.recurring_missions(&u).await.unwrap()[0].recent[0].exact_unchanged);
    assert_eq!(s.missions(&u).await.unwrap().len(), 2);
    // Unknown dispatch is a truthful terminal state, never an exact-comparison match.
    assert_eq!(due(&s, at + 120).await, 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let _step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.call(|db| {
        crate::chat::restart(&mut db.conn)?;
        Ok(())
    })
    .await
    .unwrap();
    s.poll_recurring_missions_at(at + 120).await.unwrap();
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items.len(),
        2
    );
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].recent[0].state,
        "unknown"
    );
    // Withdrawn achieved baseline cannot be used as a cache to revive or suppress later evidence.
    let sid = saved.schedule.id;
    s.call(move|db|{db.conn.execute("UPDATE missions SET state='quarantined',candidate=NULL WHERE id IN(SELECT mission_id FROM recurring_mission_occurrences WHERE schedule_id=?1)",[sid])?;Ok(())}).await.unwrap();
    assert_eq!(due(&s, at + 180).await, 1);
    exact(&s, "dispatch-C").await;
    s.poll_recurring_missions_at(at + 180).await.unwrap();
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items.len(),
        3
    );
}
#[tokio::test]
async fn recurring_late_once_expiry_and_restore_do_not_grant_a_fresh_day() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let mut request = input(at);
    request.schedule.rule = ScheduleRule::Once { at };
    let saved = s.create_recurring_mission(&u, request).await.unwrap();
    assert_eq!(due(&s, at + 86400).await, 0);
    assert!(s.missions(&u).await.unwrap().is_empty());
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0]
            .history
            .occurrences[0]
            .reason
            .as_deref(),
        Some("mission_window_expired")
    );
    let recurring = s
        .create_recurring_mission(&u, input(at + 120))
        .await
        .unwrap();
    assert_eq!(due(&s, at + 120).await, 1);
    s.call(|db| {
        let tx = db.conn.transaction()?;
        crate::schedules::quarantine_for_restore(&tx)?;
        tx.commit()?;
        Ok(())
    })
    .await
    .unwrap();
    s.poll_recurring_missions_at(at + 120).await.unwrap();
    assert!(
        s.missions(&u)
            .await
            .unwrap()
            .iter()
            .all(|m| m.state != "queued")
    );
    assert!(
        s.recurring_missions(&u)
            .await
            .unwrap()
            .iter()
            .all(|r| matches!(r.schedule.state.as_str(), "paused" | "expired"))
    );
    let _ = (saved, recurring);
}
#[tokio::test]
async fn recurring_epoch_and_provider_withdrawal_require_fresh_owner_action_and_keep_original_pin()
{
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let saved = s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    let next = s
        .change_password(&u, "Synthetic-recurring-new-password".into())
        .await
        .unwrap();
    assert!(
        s.control_recurring_mission(
            &u,
            saved.schedule.id.clone(),
            ScheduleControl {
                expected_revision: 1,
                action: "resume".into()
            }
        )
        .await
        .is_err()
    );
    s.poll_recurring_missions_at(at).await.unwrap();
    assert!(
        s.missions(&next.user)
            .await
            .unwrap()
            .iter()
            .all(|m| m.state == "cancelled")
    );
    let paused = s.recurring_missions(&next.user).await.unwrap().remove(0);
    assert_eq!(paused.schedule.state, "paused");
    s.control_recurring_mission(
        &next.user,
        saved.schedule.id.clone(),
        ScheduleControl {
            expected_revision: 1,
            action: "resume".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(due(&s, at + 60).await, 1);
    let provider = saved.model.provider_id;
    s.update_provider(
        &next.user,
        provider,
        1,
        crate::ProviderChange {
            name: "Revoked synthetic binding".into(),
            config: ProviderConfig {
                kind: ProviderKind::Compatible,
                base_url: Some("http://127.0.0.1:9/v1".into()),
                allowed_ips: vec!["127.0.0.1".into()],
                router_providers: vec![],
                zero_data_retention: false,
            },
            enabled: false,
            credential: crate::CredentialChange::Keep,
        },
    )
    .await
    .unwrap();
    assert_eq!(due(&s, at + 120).await, 0);
    let r = s.recurring_missions(&next.user).await.unwrap();
    assert_eq!(
        r[0].schedule.reason.as_deref(),
        Some("provider_unavailable")
    );
    assert!(
        s.missions(&next.user)
            .await
            .unwrap()
            .iter()
            .all(|m| m.result.is_none())
    );
}
#[tokio::test]
async fn recurring_uses_existing_dst_gap_fold_weekly_and_catchup_oracles() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let gap = ScheduleRule::Daily {
        time: "02:30".into(),
        timezone: "America/New_York".into(),
    };
    let mut request = input(now().unwrap() + 60);
    request.schedule.rule = gap.clone();
    let recurring = s
        .create_recurring_mission(&u, request.clone())
        .await
        .unwrap();
    let reminder = s
        .create_schedule(
            &u,
            ScheduleCreate {
                request_id: Uuid::new_v4().to_string(),
                draft: request.schedule,
            },
        )
        .await
        .unwrap();
    let spring = chrono::DateTime::parse_from_rfc3339("2027-03-14T07:00:00Z")
        .unwrap()
        .timestamp();
    due(&s, spring).await;
    let actual = s.recurring_missions(&u).await.unwrap().remove(0).history;
    let oracle = s.schedule_history(&u, reminder.id).await.unwrap();
    assert_eq!(
        actual
            .occurrences
            .iter()
            .filter(|o| o.reason.as_deref() == Some("dst_gap"))
            .map(|o| (&o.local_slot, o.scheduled_for))
            .collect::<Vec<_>>(),
        oracle
            .occurrences
            .iter()
            .filter(|o| o.reason.as_deref() == Some("dst_gap"))
            .map(|o| (&o.local_slot, o.scheduled_for))
            .collect::<Vec<_>>()
    );
    assert_eq!(
        actual
            .occurrences
            .iter()
            .filter(|o| o.reason.as_deref() == Some("dst_gap"))
            .count(),
        1
    );
    assert_eq!(
        actual.skipped_ranges[0].count,
        oracle.skipped_ranges[0].count
    );
    // Clear only the first schedule's future authority before exercising an independent weekly fold.
    s.control_recurring_mission(
        &u,
        recurring.schedule.id,
        ScheduleControl {
            expected_revision: 1,
            action: "revoke".into(),
        },
    )
    .await
    .unwrap();
    let mut request = input(now().unwrap() + 60);
    request.schedule.rule = ScheduleRule::Weekly {
        days: vec![7],
        time: "01:30".into(),
        timezone: "America/New_York".into(),
    };
    s.create_recurring_mission(&u, request).await.unwrap();
    let fold = chrono::DateTime::parse_from_rfc3339("2027-11-07T05:30:00Z")
        .unwrap()
        .timestamp();
    assert_eq!(due(&s, fold).await, 1);
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].recent[0].scheduled_for,
        fold
    );
    assert_eq!(due(&s, fold + 3600).await, 0);
}
#[tokio::test]
async fn recurring_actual_source_withdrawal_erases_result_but_inbox_contains_no_revocable_evidence()
{
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let col = s
        .create_collection(
            &u,
            "Synthetic recurring evidence".into(),
            Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    s.ingest_text(
        &u,
        col.id.clone(),
        IngestBatch {
            request_id: Uuid::new_v4().to_string(),
            expected_cursor: 0,
            next_cursor: 1,
            complete_snapshot: false,
            entries: vec![SourceInput {
                external_id: "synthetic-note".into(),
                version: "v1".into(),
                expected_revision: None,
                title: "Private synthetic source title".into(),
                text: "Synthetic private exact quote".into(),
                speaker: None,
                observed_at: 100,
                valid_from: None,
                valid_until: None,
            }],
        },
    )
    .await
    .unwrap();
    let source = s.sources(&u, col.id, None).await.unwrap().items.remove(0);
    let mut request = input(at);
    request.template.checks = vec![MissionCheck::SourceQuote {
        id: source.id.clone(),
        revision: 1,
        quote: "Synthetic private exact quote".into(),
    }];
    s.create_recurring_mission(&u, request).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"v":1}"#.to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "source-read".into(),
            input: crate::KnowledgeToolInput::ReadSource {
                id: source.id.clone(),
                revision: 1,
                start_byte: 0,
                max_bytes: 4096,
            },
        }],
    )
    .await
    .unwrap();
    s.execute_knowledge_tool(&step, "source-read".into())
        .await
        .unwrap();
    assert!(
        s.mission_verify(
            &step,
            MissionCandidate {
                text: "Synthetic private exact quote".into(),
                evidence: vec![MissionEvidence {
                    step: step.step(),
                    call_id: "source-read".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    s.poll_recurring_missions_at(at).await.unwrap();
    let notice = s
        .notifications(&u, None, false)
        .await
        .unwrap()
        .items
        .remove(0);
    assert!(!notice.message.contains("private"));
    assert!(!notice.title.contains("Private"));
    s.delete_source(&u, source.id, 1).await.unwrap();
    let m = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(m.state, "quarantined");
    assert!(m.result.is_none());
    assert!(m.receipts.is_empty());
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items[0].message,
        notice.message
    );
    assert!(
        s.notifications(&u, None, false).await.unwrap().items[0]
            .mission_id
            .is_none()
    );
    // The immutable notice carries only an ID link; following it cannot revive withdrawn mission content.
    let id = m.id;
    let t = u.tenant.0.clone();
    s.call(move |db| {
        assert!(exact_result(&db.conn, &t, &id)?.is_none());
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn recurring_review_results_are_never_suppressed_by_exact_content_policy() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let mut request = input(at);
    request.template.checks = vec![MissionCheck::UserReview {
        description: "Review semantic quality".into(),
    }];
    s.create_recurring_mission(&u, request).await.unwrap();
    for n in 0..2 {
        assert_eq!(due(&s, at + n * 60).await, 1);
        exact(&s, &format!("review-{n}")).await;
        s.poll_recurring_missions_at(at + n * 60).await.unwrap();
    }
    assert_eq!(
        s.notifications(&u, None, false).await.unwrap().items.len(),
        2
    );
    assert!(
        s.missions(&u)
            .await
            .unwrap()
            .iter()
            .all(|m| m.state == "partial")
    );
    assert!(
        s.recurring_missions(&u).await.unwrap()[0]
            .recent
            .iter()
            .all(|o| !o.exact_unchanged)
    );
}
#[tokio::test]
async fn recurring_real_encrypted_restore_preserves_history_and_requires_new_owner_grant() {
    use std::{fs, os::unix::fs::PermissionsExt};
    let _g = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    let saved = s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    exact(&s, "before-restore").await;
    s.poll_recurring_missions_at(at).await.unwrap();
    assert_eq!(due(&s, at + 60).await, 1);
    let account = u.account.id.clone();
    drop(s);
    let kit = directory();
    let heads = directory();
    let dest_parent = directory();
    for dir in [&kit, &heads, &dest_parent] {
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity = kit.path().join("identity");
    let recipient = crate::backup::keygen(&identity).unwrap();
    crate::backup::setup(d.path(), &recipient, &identity).unwrap();
    let cipher = kit.path().join("recurring.age");
    let head = heads.path().join("head.json");
    crate::cutover::checkpoint(d.path(), &cipher, &head).unwrap();
    let dest = dest_parent.path().join("restored");
    crate::cutover::restore(&cipher, &head, &identity, &dest).unwrap();
    let restored = Store::open(&dest).unwrap();
    assert!(restored.claim_agent_run().await.unwrap().is_none());
    assert_eq!(
        restored.poll_recurring_missions_at(at + 120).await.unwrap(),
        0
    );
    drop(restored);
    crate::recover_account(
        &dest,
        account,
        Role::Admin,
        "Synthetic-recurring-restored-temporary".into(),
    )
    .await
    .unwrap();
    let restored = Store::open(&dest).unwrap();
    let restricted = restored
        .login(
            "admin@example.test".into(),
            "Synthetic-recurring-restored-temporary".into(),
        )
        .await
        .unwrap();
    let owner = restored
        .change_password(
            &restricted.user,
            "Synthetic-recurring-restored-final".into(),
        )
        .await
        .unwrap()
        .user;
    let records = restored.recurring_missions(&owner).await.unwrap();
    assert_eq!(records[0].schedule.id, saved.schedule.id);
    assert_eq!(records[0].schedule.state, "paused");
    let missions = restored.missions(&owner).await.unwrap();
    assert!(missions.iter().any(|m| {
        m.state == "achieved"
            && m.result
                .as_ref()
                .is_some_and(|v| v.text == "Exact total: 0.3")
    }));
    assert!(
        missions
            .iter()
            .any(|m| m.state == "quarantined" && m.result.is_none())
    );
    assert_eq!(
        restored
            .notifications(&owner, None, false)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert!(
        restored
            .notifications(&owner, None, false)
            .await
            .unwrap()
            .items[0]
            .mission_id
            .is_none()
    );
    assert_eq!(
        restored.poll_recurring_missions_at(at + 180).await.unwrap(),
        0
    );
}
#[tokio::test]
async fn recurring_schema28_late_failure_rolls_back_the_complete_new_authority_unit() {
    let _g = TEST_LOCK.lock().await;
    let d = directory();
    drop(Store::open(d.path()).unwrap());
    let c = Connection::open(d.path().join("assistant.sqlite")).unwrap();
    c.execute_batch("DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;DROP TABLE mission_chat_commands;DROP TABLE mission_chat_plans;DROP TABLE recurring_mission_occurrences;DROP TABLE recurring_missions;CREATE TABLE recurring_mission_occurrences(conflict INTEGER);PRAGMA user_version=27;").unwrap();
    drop(c);
    assert!(Store::open(d.path()).is_err());
    let c = Connection::open(d.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        27
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name='recurring_missions'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(c.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN('missions','reminder_schedules','vault_owner_authority')",[],|r|r.get::<_,i64>(0)).unwrap(),2);
    c.execute_batch("DROP TABLE recurring_mission_occurrences;")
        .unwrap();
    drop(c);
    drop(Store::open(d.path()).unwrap());
}

#[tokio::test]
async fn recurring_expiry_poll_and_list_order_preserve_unknown_accounting_and_notify_without_suppression()
 {
    let _g = TEST_LOCK.lock().await;
    for list_first in [false, true] {
        let (_d, s, u) = setup().await;
        let at = now().unwrap() + 60;
        s.create_recurring_mission(&u, input(at)).await.unwrap();
        assert_eq!(due(&s, at).await, 1);
        exact(&s, "earlier-checked-result").await;
        s.poll_recurring_missions_at(at).await.unwrap();
        assert_eq!(due(&s, at + 60).await, 1);
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let ctx = s.agent_context(&claim.lease).await.unwrap();
        let dispatched = s
            .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
            .await
            .unwrap();
        let run = claim.lease.run_id().to_string();
        s.call(move |db| {
            db.conn
                .execute("UPDATE missions SET expires_ms=1 WHERE run_id=?1", [run])?;
            Ok(())
        })
        .await
        .unwrap();
        if list_first {
            assert!(s.missions(&u).await.unwrap()[0].uncertain);
        }
        assert_eq!(s.poll_recurring_missions_at(at + 60).await.unwrap(), 0);
        let m = s.missions(&u).await.unwrap().remove(0);
        assert_eq!(m.state, "unknown");
        assert!(m.uncertain);
        assert!(m.result.is_none());
        assert!(
            s.commit_agent_step(
                &dispatched,
                PrivateCheckpoint {
                    bytes: br#"{"v":1}"#.to_vec()
                },
                vec![]
            )
            .await
            .is_err()
        );
        s.update_agent_step_usage(
            &dispatched,
            RunUsage {
                input_tokens: Some(17),
                output_tokens: Some(2),
                ..RunUsage::default()
            },
        )
        .await
        .unwrap();
        let recent = s.recurring_missions(&u).await.unwrap().remove(0).recent;
        assert_eq!(recent[0].state, "unknown");
        assert!(!recent[0].exact_unchanged);
        assert!(recent[0].notification_id.is_some());
        assert_eq!(
            s.notifications(&u, None, false).await.unwrap().items.len(),
            2
        );
        assert!(
            s.control_mission(
                &u,
                m.id.clone(),
                MissionControl {
                    request_id: Uuid::new_v4().to_string(),
                    expected_revision: m.revision,
                    action: MissionAction::Resume
                }
            )
            .await
            .is_err()
        );
        assert!(s.claim_agent_run().await.unwrap().is_none());
        let id = m.id;
        s.call(move|db|{let(state,grant,carrier):(String,bool,String)=db.conn.query_row("SELECT m.state,m.grant_enabled,r.state FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id WHERE m.id=?1",[id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;assert_eq!(state,"unknown");assert!(!grant);assert_eq!(carrier,"outcome_unknown");Ok(())}).await.unwrap();
        assert_eq!(s.usage_summary(&u).await.unwrap().reported_input_tokens, 17);
        assert_eq!(s.poll_recurring_missions_at(at + 60).await.unwrap(), 0);
        assert_eq!(
            s.notifications(&u, None, false).await.unwrap().items.len(),
            2
        );
        // A distinct future occurrence remains separately authorized; it never replays the old dispatch.
        assert_eq!(due(&s, at + 120).await, 1);
        let next = s.missions(&u).await.unwrap().remove(0);
        assert_eq!(next.state, "queued");
        assert_eq!(next.expires_at_ms, (at + 120 + 86400) * 1000);
        assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
    }
}

#[tokio::test]
async fn recurring_expiry_late_model_guard_before_poll_or_list_keeps_unknown_and_no_result_publication()
 {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let dispatched = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    // First post-expiry action is the actual provider completion guard, not listing or polling.
    assert!(matches!(
        s.commit_agent_step(
            &dispatched,
            PrivateCheckpoint {
                bytes: br#"{"v":1,"late":"unpublished synthetic candidate"}"#.to_vec()
            },
            vec![]
        )
        .await,
        Err(StoreError::BudgetExhausted)
    ));
    s.call(|db|{let(state,uncertain,grant,candidate,carrier):(String,bool,bool,Option<String>,String)=db.conn.query_row("SELECT m.state,m.uncertain,m.grant_enabled,m.candidate,r.state FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;assert_eq!(state,"unknown");assert!(uncertain);assert!(!grant);assert!(candidate.is_none());assert_eq!(carrier,"outcome_unknown");assert_eq!(db.conn.query_row("SELECT count(*) FROM local_notifications",[],|r|r.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT count(*) FROM agent_steps WHERE checkpoint IS NOT NULL",[],|r|r.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
    s.update_agent_step_usage(
        &dispatched,
        RunUsage {
            input_tokens: Some(19),
            output_tokens: Some(2),
            ..RunUsage::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 0);
    let m = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(m.state, "unknown");
    assert!(m.uncertain);
    assert!(m.result.is_none());
    let notices = s.notifications(&u, None, false).await.unwrap().items;
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].message, "실행 상태: unknown");
    assert!(!s.recurring_missions(&u).await.unwrap()[0].recent[0].exact_unchanged);
    assert_eq!(s.usage_summary(&u).await.unwrap().reported_input_tokens, 19);
    assert!(s.claim_agent_run().await.unwrap().is_none());
}

#[tokio::test]
async fn recurring_acknowledged_verify_expiry_commits_before_poll_and_notifies_reviewable_history()
{
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let at = now().unwrap() + 60;
    s.create_recurring_mission(&u, input(at)).await.unwrap();
    assert_eq!(due(&s, at).await, 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    let candidate = MissionCandidate {
        text: "Acknowledged but unverified synthetic proposal".into(),
        evidence: vec![],
    };
    s.commit_agent_step(&step,PrivateCheckpoint{bytes:serde_json::to_vec(&serde_json::json!({"kind":"mission_round","planning":false,"text":serde_json::to_string(&candidate).unwrap()})).unwrap()},vec![]).await.unwrap();
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(matches!(
        s.mission_verify(&step, candidate).await,
        Err(StoreError::BudgetExhausted)
    ));
    s.call(|db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT state,uncertain,grant_enabled FROM missions",
                [],
                |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, bool>(1)?,
                    r.get::<_, bool>(2)?
                ))
            )?,
            ("partial".into(), false, false)
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM chat_runs", [], |r| r.get::<_, String>(0))?,
            "incomplete"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM local_notifications", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    s.poll_recurring_missions_at(at).await.unwrap();
    let notice = s
        .notifications(&u, None, false)
        .await
        .unwrap()
        .items
        .remove(0);
    assert_eq!(notice.message, "실행 상태: partial");
    assert!(!notice.message.contains("Acknowledged"));
    let recent = s.recurring_missions(&u).await.unwrap().remove(0).recent;
    assert!(!recent[0].exact_unchanged);
    assert_eq!(recent[0].state, "partial");
    assert!(s.claim_agent_run().await.unwrap().is_none());
}
