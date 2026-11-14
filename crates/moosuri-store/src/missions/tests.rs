use super::*;
use crate::tests::{TEST_LOCK, admin, directory, member};
use crate::{PendingKnowledgeCall, PrivateCheckpoint, StepLease};
use moosuri_core::*;
use std::fs;
use std::os::unix::fs::PermissionsExt;
pub(crate) async fn setup() -> (tempfile::TempDir, Store, UserContext) {
    let d = directory();
    let s = Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let u = admin(&s).await.user;
    let p = s
        .create_provider(
            &u,
            ProviderChange {
                name: "Synthetic mission fixture".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
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
pub(crate) fn input(mode: MissionMode) -> MissionCreate {
    MissionCreate {
        request_id: Uuid::new_v4().to_string(),
        task: "Compute the synthetic total from 0.1 and 0.2".into(),
        constraints: String::new(),
        checks: vec![MissionCheck::DecimalEquals {
            expected: "0.3".into(),
        }],
        execution_mode: mode,
    }
}
fn checkpoint() -> PrivateCheckpoint {
    PrivateCheckpoint {
        bytes: br#"{"v":1,"kind":"mission_receipt_checkpoint"}"#.to_vec(),
    }
}
fn calc() -> PendingKnowledgeCall {
    PendingKnowledgeCall {
        call_id: "sum".into(),
        input: KnowledgeToolInput::DecimalCalculate {
            calculation: DecimalCalculation {
                operation: DecimalOperation::Sum,
                operands: vec!["0.1".into(), "0.2".into()],
            },
        },
    }
}
async fn begin(s: &Store, l: &RunLease, calls: Vec<PendingKnowledgeCall>) -> StepLease {
    let ctx = s.agent_context(l).await.unwrap();
    let st = s.begin_agent_step(l, ctx.checkpoint_seq).await.unwrap();
    s.commit_agent_step(&st, checkpoint(), calls).await.unwrap();
    st
}
fn ctl(rev: i64, action: MissionAction) -> MissionControl {
    MissionControl {
        request_id: Uuid::new_v4().to_string(),
        expected_revision: rev,
        action,
    }
}
#[tokio::test]
async fn mission_exact_receipt_verifies_and_wrong_run_receipt_cannot() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    assert!(s.list_threads(&u).await.unwrap().is_empty());
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &c.lease, vec![calc()]).await;
    s.mission_save_plan(
        &st,
        vec!["Calculate exact total".into(), "Verify receipt".into()],
    )
    .await
    .unwrap();
    let v = s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    assert!(matches!(v,KnowledgeToolResult::Decimal{exact} if exact=="0.3"));
    let final_step = begin(&s, &c.lease, vec![]).await;
    let wrong = MissionCandidate {
        text: "0.3".into(),
        evidence: vec![MissionEvidence {
            step: 99,
            call_id: "sum".into(),
        }],
    };
    assert!(!s.mission_verify(&final_step, wrong).await.unwrap());
    let next = begin(&s, &c.lease, vec![]).await;
    assert!(
        s.mission_verify(
            &next,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: st.step(),
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    let result = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(result.id, m.id);
    assert_eq!(result.state, "achieved");
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
}
#[tokio::test]
async fn mission_missing_evidence_exhausts_two_repairs_without_success() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    for n in 0..3 {
        let st = begin(&s, &c.lease, vec![]).await;
        assert_eq!(
            s.mission_verify(
                &st,
                MissionCandidate {
                    text: "0.3".into(),
                    evidence: vec![]
                }
            )
            .await
            .unwrap(),
            n == 2
        );
    }
    let result = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(result.state, "missing_evidence");
    assert!(!result.report.is_empty());
}
#[tokio::test]
async fn mission_logout_separates_explicit_grant_from_current_session() {
    let _g = TEST_LOCK.lock().await;
    for mode in [MissionMode::CurrentSession, MissionMode::Durable] {
        let (_d, s, u) = setup().await;
        s.create_mission(&u, input(mode.clone())).await.unwrap();
        s.logout(&u).await.unwrap();
        let claim = s.claim_agent_run().await.unwrap();
        assert_eq!(claim.is_some(), matches!(mode, MissionMode::Durable));
    }
}
#[tokio::test]
async fn mission_pause_resume_preserves_deadline_budget_and_cas() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    let pause = ctl(1, MissionAction::Pause);
    let p = s
        .control_mission(&u, m.id.clone(), pause.clone())
        .await
        .unwrap();
    assert_eq!(p.state, "paused");
    assert_eq!(
        s.control_mission(&u, m.id.clone(), pause)
            .await
            .unwrap()
            .revision,
        2
    );
    assert!(s.check_run(&c.lease).await.is_err());
    assert!(matches!(
        s.control_mission(&u, m.id.clone(), ctl(1, MissionAction::Resume))
            .await,
        Err(StoreError::Conflict)
    ));
    let r = s
        .control_mission(
            &u,
            m.id.clone(),
            ctl(
                2,
                MissionAction::Steer {
                    instruction: "Explain briefly".into(),
                },
            ),
        )
        .await
        .unwrap();
    assert_eq!(r.expires_at_ms, m.expires_at_ms);
    assert_eq!(r.request.constraints, m.request.constraints);
    assert_eq!(r.steering.len(), 1);
    let new = s.claim_agent_run().await.unwrap().unwrap();
    assert_ne!(new.lease.attempt(), c.lease.attempt());
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
    let adopted = s.mission_step_lease(&new.lease, st.step()).await.unwrap();
    assert!(
        matches!(s.execute_knowledge_tool(&adopted,"sum".into()).await.unwrap(),KnowledgeToolResult::Decimal{exact} if exact=="0.3")
    );
}
#[tokio::test]
async fn mission_committed_pause_is_uncertain_and_never_resumes() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    s.begin_agent_step(&c.lease, 0).await.unwrap();
    let p = s
        .control_mission(&u, m.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    assert!(p.uncertain);
    assert!(matches!(
        s.control_mission(&u, m.id, ctl(p.revision, MissionAction::Resume))
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn mission_restart_stable_continues_unknown_dispatch_never_replays() {
    let _g = TEST_LOCK.lock().await;
    for dispatched in [false, true] {
        let (d, s, u) = setup().await;
        s.create_mission(&u, input(MissionMode::Durable))
            .await
            .unwrap();
        let c = s.claim_agent_run().await.unwrap().unwrap();
        if dispatched {
            s.begin_agent_step(&c.lease, 0).await.unwrap();
        } else {
            let st = begin(&s, &c.lease, vec![calc()]).await;
            s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
        }
        drop(c);
        drop(s);
        let s = Store::open(d.path()).unwrap();
        let m = s.missions(&u).await.unwrap().remove(0);
        assert_eq!(m.state, if dispatched { "unknown" } else { "queued" });
        assert_eq!(s.claim_agent_run().await.unwrap().is_some(), !dispatched);
        assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
    }
}
#[tokio::test]
async fn mission_isolation_deletion_idempotence_and_no_resurrection() {
    let _g = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    let other = member(&s, &u, "mission-other@example.test").await.user;
    let request = input(MissionMode::Durable);
    let m = s.create_mission(&u, request.clone()).await.unwrap();
    assert!(s.missions(&other).await.unwrap().is_empty());
    assert!(matches!(
        s.delete_mission(&other, m.id.clone()).await,
        Err(StoreError::NotFound)
    ));
    let mut conflicting = request.clone();
    conflicting.task.push('!');
    assert!(matches!(
        s.create_mission(&u, conflicting).await,
        Err(StoreError::Conflict)
    ));
    s.delete_mission(&u, m.id).await.unwrap();
    assert!(matches!(
        s.create_mission(&u, request).await,
        Err(StoreError::NotFound)
    ));
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert!(s.missions(&u).await.unwrap().is_empty());
    assert!(s.claim_agent_run().await.unwrap().is_none());
}
#[tokio::test]
async fn mission_epoch_revocation_and_original_expiry_cannot_be_reauthorized() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE accounts SET auth_epoch=auth_epoch+1 WHERE tenant_id=?1",
            [tenant],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    let u = s
        .login("admin@example.test".into(), crate::tests::PASSWORD.into())
        .await
        .unwrap()
        .user;
    assert!(matches!(
        s.control_mission(&u, m.id, ctl(1, MissionAction::Resume))
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn mission_original_budget_and_tool_failures_cannot_be_reset() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    for n in 0..3 {
        let st = begin(
            &s,
            &c.lease,
            vec![PendingKnowledgeCall {
                call_id: format!("bad-{n}"),
                input: KnowledgeToolInput::InvalidArguments,
            }],
        )
        .await;
        assert!(matches!(
            s.execute_knowledge_tool(&st, format!("bad-{n}"))
                .await
                .unwrap(),
            KnowledgeToolResult::Error(_)
        ));
        assert_eq!(s.mission_tool_failure(&st).await.unwrap(), n == 2);
    }
    let v = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(v.state, "failed");
    let l = c.lease.clone();
    assert!(s.agent_context(&l).await.is_err());
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
}
#[tokio::test]
async fn mission_model_and_absolute_expiry_budgets_are_enforced() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    for _ in 0..6 {
        begin(&s, &c.lease, vec![]).await;
    }
    let seq = s.agent_context(&c.lease).await.unwrap().checkpoint_seq;
    assert!(matches!(
        s.begin_agent_step(&c.lease, seq).await,
        Err(StoreError::BudgetExhausted)
    ));
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE missions SET expires_ms=1 WHERE tenant_id=?1",
            [tenant],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.check_run(&c.lease).await.is_err());
}
#[tokio::test]
async fn mission_provider_withdrawal_purges_candidate_and_fences_lease() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    assert!(
        s.mission_verify(
            &st,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: 1,
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    let provider = s.list_providers(&u).await.unwrap().providers.remove(0);
    s.update_provider(
        &u,
        provider.id,
        1,
        ProviderChange {
            name: "Withdrawn synthetic".into(),
            enabled: false,
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
    let v = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(v.state, "quarantined");
    assert!(v.result.is_none());
    assert!(s.check_run(&c.lease).await.is_err());
}
#[tokio::test]
async fn mission_failed_tool_receipt_and_semantic_prose_never_prove_success() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let mut request = input(MissionMode::Durable);
    request.checks = vec![
        MissionCheck::OutputContains {
            text: "done".into(),
        },
        MissionCheck::UserReview {
            description: "Verify semantic meaning".into(),
        },
    ];
    s.create_mission(&u, request).await.unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(
        &s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "unsupported".into(),
            input: KnowledgeToolInput::Artifact {
                request: ArtifactToolRequest::Search {
                    after: None,
                    kind: None,
                },
            },
        }],
    )
    .await;
    assert!(matches!(
        s.execute_knowledge_tool(&st, "unsupported".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(_)
    ));
    assert!(
        !s.mission_verify(
            &st,
            MissionCandidate {
                text: "done".into(),
                evidence: vec![MissionEvidence {
                    step: 1,
                    call_id: "unsupported".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    assert_ne!(s.missions(&u).await.unwrap()[0].state, "achieved");
}
#[tokio::test]
async fn mission_owned_search_source_receipt_withdrawal_erases_result_and_retry_receipts() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let col = s
        .create_collection(
            &u,
            "Synthetic mission sources".into(),
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
                external_id: "invoice".into(),
                version: "v1".into(),
                expected_revision: None,
                title: "Synthetic invoice".into(),
                text: "Apples cost 0.1.".into(),
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
    let mut req = input(MissionMode::Durable);
    req.checks = vec![MissionCheck::SourceQuote {
        id: source.id.clone(),
        revision: 1,
        quote: "Apples cost 0.1.".into(),
    }];
    let m = s.create_mission(&u, req).await.unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(
        &s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "read".into(),
            input: KnowledgeToolInput::ReadSource {
                id: source.id.clone(),
                revision: 1,
                start_byte: 0,
                max_bytes: 4096,
            },
        }],
    )
    .await;
    s.execute_knowledge_tool(&st, "read".into()).await.unwrap();
    let pause = ctl(1, MissionAction::Pause);
    s.control_mission(&u, m.id.clone(), pause.clone())
        .await
        .unwrap();
    let resumed = s
        .control_mission(&u, m.id.clone(), ctl(2, MissionAction::Resume))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = s.mission_step_lease(&claim.lease, 1).await.unwrap();
    s.mission_verify(
        &st,
        MissionCandidate {
            text: "Apples cost 0.1.".into(),
            evidence: vec![MissionEvidence {
                step: 1,
                call_id: "read".into(),
            }],
        },
    )
    .await
    .unwrap();
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "achieved");
    s.delete_source(&u, source.id, 1).await.unwrap();
    let hidden = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(hidden.state, "quarantined");
    assert!(hidden.result.is_none());
    let replay = s.control_mission(&u, m.id, pause).await.unwrap();
    assert!(replay.result.is_none());
    assert_eq!(resumed.request.constraints, hidden.request.constraints);
}
#[tokio::test]
async fn mission_actual_encrypted_restore_quarantines_unfinished_grants_and_keeps_readable_history()
{
    let _guard = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    let done = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    assert!(
        s.mission_verify(
            &st,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: 1,
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    let pending = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let account = u.account.id.clone();
    drop(c);
    drop(s);
    let kit = directory();
    let heads = directory();
    let dest_parent = directory();
    // Key generation/restore intentionally reject public directories. Do not
    // depend on the developer shell's umask, which differs on Linux.
    for dir in [&kit, &heads, &dest_parent] {
        fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let identity = kit.path().join("identity");
    let recipient = crate::backup::keygen(&identity).unwrap();
    crate::backup::setup(d.path(), &recipient, &identity).unwrap();
    let cipher = kit.path().join("mission-checkpoint.age");
    let head = heads.path().join("head.json");
    crate::cutover::checkpoint(d.path(), &cipher, &head).unwrap();
    let dest = dest_parent.path().join("restored");
    crate::cutover::restore(&cipher, &head, &identity, &dest).unwrap();
    let restored = Store::open(&dest).unwrap();
    assert!(restored.claim_agent_run().await.unwrap().is_none());
    restored
        .call(|db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM missions WHERE grant_enabled=1",
                    [],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    drop(restored);
    crate::recover_account(
        &dest,
        account,
        Role::Admin,
        "Synthetic-restored-temporary-password".into(),
    )
    .await
    .unwrap();
    let restored = Store::open(&dest).unwrap();
    let restricted = restored
        .login(
            "admin@example.test".into(),
            "Synthetic-restored-temporary-password".into(),
        )
        .await
        .unwrap();
    let owner = restored
        .change_password(&restricted.user, "Synthetic-restored-final-password".into())
        .await
        .unwrap()
        .user;
    let results = restored.missions(&owner).await.unwrap();
    let historical = results.iter().find(|m| m.id == done.id).unwrap();
    assert_eq!(historical.state, "achieved");
    assert_eq!(historical.result.as_ref().unwrap().text, "0.3");
    assert_eq!(historical.receipts[0].detail, "0.3");
    let unfinished = results.iter().find(|m| m.id == pending.id).unwrap();
    assert_eq!(unfinished.state, "quarantined");
    assert!(matches!(
        restored
            .control_mission(
                &owner,
                pending.id,
                ctl(unfinished.revision, MissionAction::Resume)
            )
            .await,
        Err(StoreError::Conflict)
    ));
}
#[tokio::test]
async fn mission_pinned_community_skill_removal_fences_unread_and_completed_results() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let card = s.community_skills(&u).await.unwrap().remove(0);
    s.mutate_community_skill(
        &u,
        card.id.clone(),
        CommunitySkillMutation {
            action: "install".into(),
            expected_revision: 0,
            expected_digest: Some(card.digest),
        },
    )
    .await
    .unwrap();
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    s.mutate_community_skill(
        &u,
        card.id,
        CommunitySkillMutation {
            action: "remove".into(),
            expected_revision: 1,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    assert!(s.check_run(&c.lease).await.is_err());
    let v = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(v.state, "quarantined");
    assert!(v.result.is_none());
}
#[tokio::test]
async fn mission_public_http_owner_permit_has_no_session_and_unknown_restart_never_replays() {
    let _guard = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(
        &s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "http".into(),
            input: KnowledgeToolInput::External {
                request: ExternalToolRequest::Web(WebToolRequest::Read {
                    url: "https://example.test/synthetic".into(),
                }),
            },
        }],
    )
    .await;
    let permit = match s.prepare_external_tool(&st, "http".into()).await.unwrap() {
        crate::ExternalToolStart::Dispatch(p) => p,
        _ => panic!("public HTTP should be admitted"),
    };
    assert!(permit.user().is_none());
    drop(permit);
    drop(c);
    drop(s);
    let s = Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let v = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(v.state, "unknown");
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}
#[tokio::test]
async fn mission_pause_before_and_after_external_commit_fences_results_but_keeps_late_usage() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(
        &s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "http".into(),
            input: KnowledgeToolInput::External {
                request: ExternalToolRequest::Web(WebToolRequest::Read {
                    url: "https://example.test/synthetic".into(),
                }),
            },
        }],
    )
    .await;
    let p = match s.prepare_external_tool(&st, "http".into()).await.unwrap() {
        crate::ExternalToolStart::Dispatch(p) => p,
        _ => panic!("public HTTP should be admitted"),
    };
    s.control_mission(&u, m.id, ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    assert!(
        s.complete_external_tool(
            &p,
            KnowledgeToolResult::External(
                serde_json::json!({"web_page":{"text":"late synthetic text"}})
            )
        )
        .await
        .is_err()
    );
    s.update_agent_step_usage(
        &st,
        RunUsage {
            input_tokens: Some(9),
            output_tokens: Some(3),
            ..RunUsage::default()
        },
    )
    .await
    .unwrap();
    assert!(s.missions(&u).await.unwrap()[0].uncertain);
    let summary = s.usage_summary(&u).await.unwrap();
    assert_eq!(summary.dispatched_calls, 1);
}
#[tokio::test]
async fn mission_semantic_review_is_usable_partial_without_pointless_model_repairs() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let mut req = input(MissionMode::Durable);
    req.constraints = "Check that the recommendation fits all delivery terms".into();
    req.checks.push(MissionCheck::UserReview {
        description: "Confirm the recommendation suits the request".into(),
    });
    s.create_mission(&u, req).await.unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    assert!(
        s.mission_verify(
            &st,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: 1,
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    let v = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(v.state, "partial");
    assert!(v.result.is_some());
    assert_eq!(
        v.report.len(),
        1,
        "explicit owner review must not be duplicated"
    );
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}

#[tokio::test]
async fn freeform_constraints_require_review_without_mutating_payload_or_extra_repairs() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let mut request = input(MissionMode::Durable);
    request.constraints = "Check all delivery conditions, not just the price".into();
    let original = serde_json::to_value(&request).unwrap();
    s.create_mission(&u, request).await.unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&step, "sum".into()).await.unwrap();
    assert!(
        s.mission_verify(
            &step,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: step.step(),
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    let view = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(view.state, "partial");
    assert_eq!(serde_json::to_value(view.request).unwrap(), original);
    assert_eq!(
        view.report,
        vec!["Additional constraints need owner review"]
    );
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT repairs FROM missions", [], |r| r.get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn mission_cancel_before_external_dispatch_prevents_any_commit_or_stale_result() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(
        &s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "http".into(),
            input: KnowledgeToolInput::External {
                request: ExternalToolRequest::Web(WebToolRequest::Read {
                    url: "https://example.test/synthetic".into(),
                }),
            },
        }],
    )
    .await;
    let command = ctl(1, MissionAction::Cancel);
    let v = s
        .control_mission(&u, m.id.clone(), command.clone())
        .await
        .unwrap();
    assert_eq!(v.state, "cancelled");
    assert!(!v.uncertain);
    assert!(s.prepare_external_tool(&st, "http".into()).await.is_err());
    assert!(
        s.mission_verify(
            &st,
            MissionCandidate {
                text: "fabricated late result".into(),
                evidence: vec![]
            }
        )
        .await
        .is_err()
    );
    assert_eq!(
        s.control_mission(&u, m.id, command).await.unwrap().state,
        "cancelled"
    );
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM external_tool_dispatches", [], |r| r
                    .get::<_, i64>(
                    0
                ))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
}
#[tokio::test]
async fn mission_excluded_tool_is_truthfully_unsupported_without_a_repair_loop() {
    let _guard = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(
        &s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "browser".into(),
            input: KnowledgeToolInput::External {
                request: ExternalToolRequest::Web(WebToolRequest::BrowserRead {
                    url: "https://example.test/synthetic".into(),
                }),
            },
        }],
    )
    .await;
    assert!(matches!(
        s.prepare_external_tool(&st, "browser".into())
            .await
            .unwrap(),
        crate::ExternalToolStart::Complete(KnowledgeToolResult::Error(_))
    ));
    assert!(s.mission_tool_failure(&st).await.unwrap());
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "unsupported");
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}
#[tokio::test]
async fn mission_completed_typed_plan_and_final_proposal_recover_without_new_model_dispatch() {
    let _guard = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let st = s.begin_agent_step(&c.lease, 0).await.unwrap();
    let text = serde_json::json!({"steps":["Calculate exact total","Verify receipt"]}).to_string();
    s.commit_agent_step(
        &st,
        PrivateCheckpoint {
            bytes: serde_json::json!({"kind":"mission_round","planning":true,"text":text})
                .to_string()
                .into_bytes(),
        },
        vec![],
    )
    .await
    .unwrap();
    drop(c);
    drop(s);
    let s = Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let pending = s
        .mission_execution(&c.lease)
        .await
        .unwrap()
        .unwrap()
        .pending_round
        .unwrap();
    assert_eq!(pending.step, 1);
    assert!(pending.planning);
    let st = s.mission_step_lease(&c.lease, pending.step).await.unwrap();
    let plan = serde_json::from_str::<serde_json::Value>(&pending.text).unwrap()["steps"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    s.mission_save_plan(&st, plan).await.unwrap();
    assert!(
        s.mission_execution(&c.lease)
            .await
            .unwrap()
            .unwrap()
            .pending_round
            .is_none()
    );
    let calc_step = begin(&s, &c.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&calc_step, "sum".into())
        .await
        .unwrap();
    let context = s.agent_context(&c.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&c.lease, context.checkpoint_seq)
        .await
        .unwrap();
    let candidate = MissionCandidate {
        text: "0.3".into(),
        evidence: vec![MissionEvidence {
            step: 2,
            call_id: "sum".into(),
        }],
    };
    s.commit_agent_step(&final_step,PrivateCheckpoint{bytes:serde_json::json!({"kind":"mission_round","planning":false,"text":serde_json::to_string(&candidate).unwrap()}).to_string().into_bytes()},vec![]).await.unwrap();
    drop(c);
    drop(s);
    let s = Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let pending = s
        .mission_execution(&c.lease)
        .await
        .unwrap()
        .unwrap()
        .pending_round
        .unwrap();
    assert!(!pending.planning);
    let step = s.mission_step_lease(&c.lease, pending.step).await.unwrap();
    assert!(
        s.mission_verify(&step, serde_json::from_str(&pending.text).unwrap())
            .await
            .unwrap()
    );
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "achieved");
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
}

async fn recover_final_at_capacity(
    models: usize,
    tools: usize,
    leave_pending_tool: bool,
    attempt: i32,
    constraints: bool,
) {
    let (d, s, u) = setup().await;
    let mut request = input(MissionMode::Durable);
    if constraints {
        request.constraints = "Check that the result fits the owner's delivery conditions".into();
    }
    s.create_mission(&u, request).await.unwrap();
    s.call(move |db| {
        db.conn
            .execute("UPDATE chat_runs SET attempt=?1", [attempt])?;
        Ok(())
    })
    .await
    .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let calls = (0..tools)
        .map(|i| {
            let mut call = calc();
            call.call_id = format!("sum{i}");
            call
        })
        .collect();
    let first = begin(&s, &c.lease, calls).await;
    s.mission_save_plan(&first, vec!["Calculate and verify actual receipt".into()])
        .await
        .unwrap();
    for i in 0..tools {
        s.execute_knowledge_tool(&first, format!("sum{i}"))
            .await
            .unwrap();
    }
    for _ in 1..models - 1 {
        begin(&s, &c.lease, vec![]).await;
    }
    let context = s.agent_context(&c.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&c.lease, context.checkpoint_seq)
        .await
        .unwrap();
    let candidate = MissionCandidate {
        text: "0.3".into(),
        evidence: vec![MissionEvidence {
            step: 1,
            call_id: "sum0".into(),
        }],
    };
    s.commit_agent_step(&final_step, PrivateCheckpoint { bytes: serde_json::json!({"kind":"mission_round","planning":false,"text":serde_json::to_string(&candidate).unwrap()}).to_string().into_bytes() }, if leave_pending_tool { vec![calc()] } else { vec![] }).await.unwrap();
    let usage = s.usage_summary(&u).await.unwrap().dispatched_calls;
    let old_lease = c.lease.clone();
    let run_id = c.lease.run_id().to_owned();
    let owner = u.clone();
    let original_run = run_id.clone();
    let counts = s.call(move |db| {
        assert!(!crate::agent_execution::retry_allowed(&db.conn, &owner.tenant.0, &original_run)?);
        Ok(db.conn.query_row("SELECT remote_calls,tool_calls,(SELECT count(*) FROM external_tool_dispatches) FROM agent_execution WHERE run_id=?1", [&original_run], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?)))?)
    }).await.unwrap();
    drop(final_step);
    drop(first);
    drop(c);
    drop(s);
    let s = Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "queued");
    let owner = u.clone();
    let recovery_run = run_id.clone();
    s.call(move |db| {
        assert_eq!(
            super::local_final_recovery(&db.conn, &owner.tenant.0, &recovery_run)?,
            !leave_pending_tool
        );
        Ok(())
    })
    .await
    .unwrap();
    let claimed = s.claim_agent_run().await.unwrap();
    if leave_pending_tool {
        assert!(claimed.is_none());
        assert_eq!(s.missions(&u).await.unwrap()[0].state, "partial");
    } else {
        let c = claimed
            .expect("acknowledged final proposal needs local verification, not another dispatch");
        assert_ne!(old_lease.nonce, c.lease.nonce);
        assert!(s.check_run(&old_lease).await.is_err());
        let pending = s
            .mission_execution(&c.lease)
            .await
            .unwrap()
            .unwrap()
            .pending_round
            .unwrap();
        assert!(!pending.planning);
        if models == 6 {
            let ctx = s.agent_context(&c.lease).await.unwrap();
            assert!(matches!(
                s.begin_agent_step(&c.lease, ctx.checkpoint_seq).await,
                Err(StoreError::BudgetExhausted)
            ));
        }
        let step = s.mission_step_lease(&c.lease, pending.step).await.unwrap();
        assert!(
            s.mission_verify(&step, serde_json::from_str(&pending.text).unwrap())
                .await
                .unwrap()
        );
        let view = s.missions(&u).await.unwrap().remove(0);
        assert_eq!(view.state, if constraints { "partial" } else { "achieved" });
        assert_eq!(view.result.unwrap().text, "0.3");
        let ctx = s.agent_context(&c.lease).await;
        assert!(ctx.is_err());
    }
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, usage);
    let after = s.call(move |db| Ok(db.conn.query_row("SELECT remote_calls,tool_calls,(SELECT count(*) FROM external_tool_dispatches) FROM agent_execution WHERE run_id=?1", [&run_id], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?)))?)).await.unwrap();
    assert_eq!(after, counts);
    assert_eq!(counts, (models as i64, tools as i64, 0));
}
#[tokio::test]
async fn acknowledged_final_reopens_at_model_capacity_without_new_dispatch() {
    let _g = TEST_LOCK.lock().await;
    recover_final_at_capacity(6, 1, false, 1, false).await;
}
#[tokio::test]
async fn acknowledged_final_reopens_at_tool_capacity_without_new_dispatch() {
    let _g = TEST_LOCK.lock().await;
    recover_final_at_capacity(2, 12, false, 1, false).await;
}
#[tokio::test]
async fn capacity_exception_never_admits_an_unfinished_tool_proposal() {
    let _g = TEST_LOCK.lock().await;
    recover_final_at_capacity(6, 1, true, 1, false).await;
}

#[tokio::test]
async fn acknowledged_final_reopens_at_attempt32_without_new_dispatch_or_old_lease() {
    let _g = TEST_LOCK.lock().await;
    recover_final_at_capacity(6, 1, false, 32, false).await;
}

#[tokio::test]
async fn attempt32_unknown_dispatch_stays_unknown_on_reopen() {
    let _g = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    s.call(|db| {
        db.conn.execute("UPDATE chat_runs SET attempt=32", [])?;
        Ok(())
    })
    .await
    .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let step = s.begin_agent_step(&c.lease, 0).await.unwrap();
    drop(c);
    drop(s);
    let s = Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap();
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "unknown");
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert!(
        s.commit_agent_step(&step, checkpoint(), vec![])
            .await
            .is_err()
    );
    s.update_agent_step_usage(
        &step,
        RunUsage {
            input_tokens: Some(19),
            ..Default::default()
        },
    )
    .await
    .unwrap();
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 1);
}

#[tokio::test]
async fn stable_controls_cross_attempt32_without_stranding_or_reviving_old_leases() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let mut mission = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let expiry = mission.expires_at_ms;
    let mut claim = s.claim_agent_run().await.unwrap().unwrap();
    let first = begin(&s, &claim.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&first, "sum".into())
        .await
        .unwrap();
    let mut cap_receipt: Option<StepLease> = None;
    for cycle in 1..=40 {
        let old = claim.lease.clone();
        let latest_step = cap_receipt.as_ref().map_or(first.step(), StepLease::step);
        let prior_step = s.mission_step_lease(&old, latest_step).await.unwrap();
        mission = s
            .control_mission(
                &u,
                mission.id.clone(),
                ctl(mission.revision, MissionAction::Pause),
            )
            .await
            .unwrap();
        assert_eq!(mission.state, "paused");
        mission = s
            .control_mission(
                &u,
                mission.id.clone(),
                ctl(mission.revision, MissionAction::Resume),
            )
            .await
            .unwrap();
        assert_eq!(mission.state, "queued");
        claim = s
            .claim_agent_run()
            .await
            .unwrap()
            .expect("accepted Resume must be claimable");
        assert_eq!(claim.lease.attempt(), (cycle + 1).min(32));
        assert_ne!(claim.lease.nonce, old.nonce);
        assert!(s.check_run(&old).await.is_err());
        assert!(s.begin_agent_step(&old, 0).await.is_err());
        assert!(
            s.commit_agent_step(&prior_step, checkpoint(), vec![])
                .await
                .is_err()
        );
        assert!(
            s.execute_knowledge_tool(&prior_step, "sum".into())
                .await
                .is_err()
        );
        assert!(
            s.mission_verify(
                &prior_step,
                MissionCandidate {
                    text: "0.3".into(),
                    evidence: vec![]
                }
            )
            .await
            .is_err()
        );
        assert!(
            s.finish_agent_execution(&old, RunState::Cancelled, None)
                .await
                .is_err()
        );
        assert!(
            s.finish_agent_run(&prior_step, RunState::Cancelled, None)
                .await
                .is_err()
        );
        if cycle <= 31 {
            let adopted = s
                .mission_step_lease(&claim.lease, first.step())
                .await
                .unwrap();
            assert!(
                matches!(s.execute_knowledge_tool(&adopted, "sum".into()).await.unwrap(), KnowledgeToolResult::Decimal { exact } if exact == "0.3")
            );
        }
        if cycle == 31 {
            cap_receipt = Some(begin(&s, &claim.lease, vec![]).await);
        }
        if cycle == 32 {
            let receipt = cap_receipt.as_ref().unwrap();
            assert!(
                s.finish_agent_run(receipt, RunState::Cancelled, None)
                    .await
                    .is_err()
            );
            s.update_agent_step_usage(
                receipt,
                RunUsage {
                    input_tokens: Some(17),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        }
        assert_eq!(mission.expires_at_ms, expiry);
    }
    let receipt = cap_receipt.unwrap();
    let final_step = s
        .mission_step_lease(&claim.lease, receipt.step())
        .await
        .unwrap();
    assert!(
        s.mission_verify(
            &final_step,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: first.step(),
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    assert_eq!(s.missions(&u).await.unwrap()[0].state, "achieved");
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 2);
    s.call(|db| {
        let counts: (i64, i64) = db.conn.query_row(
            "SELECT remote_calls,tool_calls FROM agent_execution",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        assert_eq!(counts, (2, 1));
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn migration25_rolls_back_late_failure_and_preserves_prior_authority_tables() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let dir = crate::tests::directory();
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    conn.execute_batch("DROP TABLE community_catalog_cache;DROP TRIGGER chatgpt_handoff_session_logout;DROP TRIGGER chatgpt_handoff_account_epoch;DROP TRIGGER chatgpt_handoff_binding_change;DROP TRIGGER session_purpose_valid_insert;DROP TRIGGER session_purpose_immutable;ALTER TABLE sessions DROP COLUMN absolute_deadline;ALTER TABLE sessions DROP COLUMN purpose;DROP TABLE chatgpt_handoffs;DROP TRIGGER audio_authority_no_delete;DROP TRIGGER audio_authority_monotonic;DROP TABLE audio_jobs;DROP TABLE audio_selection;DROP TABLE audio_authority;DROP TRIGGER personal_subscription_exclusive;DROP TRIGGER personal_shared_exclusive;DROP TRIGGER personal_shared_exclusive_update;DROP TABLE personal_subscription_selections;DROP TABLE chatgpt_model_probes;DROP TABLE chatgpt_auth_attempts;DROP TABLE chatgpt_sessions;DROP TABLE chatgpt_binding_revisions;DROP TABLE chatgpt_bindings;DROP TABLE subscription_runtime;DROP TABLE mail_reconciliations;DROP TABLE mail_send_evidence;DROP TABLE calendar_agent_action_origins;DROP TABLE calendar_agent_action_links;DROP TABLE calendar_command_plans;DROP TABLE calendar_agent_observations;DROP TABLE calendar_run_dependencies;DROP TABLE calendar_agent_profiles;DROP TABLE mission_chat_commands;DROP TABLE mission_chat_plans;DROP TABLE recurring_mission_occurrences;DROP TABLE recurring_missions;ALTER TABLE media_objects DROP COLUMN filename;ALTER TABLE media_objects DROP COLUMN media_type;ALTER TABLE media_objects DROP COLUMN extracted_text;DROP TABLE calendar_actions;DROP TABLE calendar_oauth_sessions;DROP TABLE calendar_connections;DROP TABLE mission_controls;DROP TABLE mission_revisions;DROP TABLE missions;ALTER TABLE threads DROP COLUMN mission_internal; CREATE TABLE mission_controls(conflict INTEGER); PRAGMA user_version=24;").unwrap();
    drop(conn);
    assert!(Store::open(dir.path()).is_err());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        24
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM pragma_table_info('threads') WHERE name='mission_internal'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        0
    );
    assert_eq!(conn.query_row("SELECT count(*) FROM sqlite_schema WHERE type='table' AND name IN ('missions','mission_revisions','mission_controls')",[],|r|r.get::<_,i64>(0)).unwrap(),1);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name LIKE 'vault_%'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        3
    );
    conn.execute_batch("DROP TABLE mission_controls;").unwrap();
    drop(conn);
    drop(Store::open(dir.path()).unwrap());
    let conn = Connection::open(dir.path().join("assistant.sqlite")).unwrap();
    assert_eq!(
        conn.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        crate::CURRENT_SCHEMA_VERSION
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM pragma_table_info('threads') WHERE name='mission_internal'",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
}

#[tokio::test]
async fn restored_final_with_freeform_constraints_remains_reviewable_at_capacity() {
    let _g = TEST_LOCK.lock().await;
    recover_final_at_capacity(6, 1, false, 32, true).await;
}

#[tokio::test]
async fn mission_expiry_distinguishes_safe_pending_prior_uncertainty_and_known_final() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let queued = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    let partial = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(partial.id, queued.id);
    assert_eq!(partial.state, "partial");
    assert!(!partial.uncertain);
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM chat_runs", [], |r| r.get::<_, String>(0))?,
            "incomplete"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT grant_enabled FROM missions", [], |r| r
                    .get::<_, i64>(0))?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    let done = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let st = begin(&s, &claim.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&st, "sum".into()).await.unwrap();
    assert!(
        s.mission_verify(
            &st,
            MissionCandidate {
                text: "0.3".into(),
                evidence: vec![MissionEvidence {
                    step: st.step(),
                    call_id: "sum".into()
                }]
            }
        )
        .await
        .unwrap()
    );
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    let final_view = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == done.id)
        .unwrap();
    assert_eq!(final_view.state, "achieved");
    assert_eq!(final_view.result.unwrap().text, "0.3");
    assert!(!final_view.uncertain);
    let prior = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let id = prior.id.clone();
    s.call(move |db| {
        db.conn.execute(
            "UPDATE missions SET state='paused',uncertain=1,expires_ms=1 WHERE id=?1",
            [id],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let unknown = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|m| m.id == prior.id)
        .unwrap();
    assert_eq!(unknown.state, "unknown");
    assert!(unknown.uncertain);
    assert!(
        s.control_mission(&u, prior.id, ctl(unknown.revision, MissionAction::Resume))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn mission_list_expiry_is_atomic_preserves_unsettled_accounting_and_fences_late_model_result()
{
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let m = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let dispatched = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.call(|db|{db.conn.execute("UPDATE missions SET expires_ms=1",[])?;db.conn.execute_batch("CREATE TRIGGER fail_expiry BEFORE UPDATE OF state ON chat_runs WHEN NEW.state='outcome_unknown' BEGIN SELECT RAISE(ABORT,'synthetic expiry failure'); END;")?;Ok(())}).await.unwrap();
    assert!(s.missions(&u).await.is_err());
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state,grant_enabled FROM missions", [], |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, i64>(1)?
                )))?,
            ("planning".into(), 1)
        );
        db.conn.execute_batch("DROP TRIGGER fail_expiry;")?;
        Ok(())
    })
    .await
    .unwrap();
    let unknown = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(unknown.state, "unknown");
    assert!(unknown.uncertain);
    assert!(
        s.commit_agent_step(&dispatched, checkpoint(), vec![])
            .await
            .is_err()
    );
    s.update_agent_step_usage(
        &dispatched,
        RunUsage {
            input_tokens: Some(9),
            output_tokens: Some(3),
            ..RunUsage::default()
        },
    )
    .await
    .unwrap();
    assert!(
        s.control_mission(&u, m.id, ctl(unknown.revision, MissionAction::Resume))
            .await
            .is_err()
    );
    assert!(s.claim_agent_run().await.unwrap().is_none());
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state,error FROM chat_runs", [], |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?
                )))?,
            ("outcome_unknown".into(), "budget_exhausted".into())
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM agent_steps", [], |r| r
                    .get::<_, String>(0))?,
            "dispatched"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT count(*) FROM model_call_receipts", [], |r| r
                    .get::<_, i64>(0))?,
            1
        );
        Ok(())
    })
    .await
    .unwrap();
    let after = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(after.state, "unknown");
    assert!(after.uncertain);
    assert!(after.result.is_none());
    assert_eq!(s.usage_summary(&u).await.unwrap().reported_input_tokens, 9);
}

#[tokio::test]
async fn mission_expiry_with_started_http_is_unknown_even_after_model_acknowledgement() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(
        &s,
        &claim.lease,
        vec![PendingKnowledgeCall {
            call_id: "expiring-http".into(),
            input: KnowledgeToolInput::External {
                request: ExternalToolRequest::Web(WebToolRequest::Read {
                    url: "https://example.test/synthetic-expiry".into(),
                }),
            },
        }],
    )
    .await;
    let permit = match s
        .prepare_external_tool(&step, "expiring-http".into())
        .await
        .unwrap()
    {
        crate::ExternalToolStart::Dispatch(p) => p,
        _ => panic!("synthetic HTTP must be admitted"),
    };
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    let m = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(m.state, "unknown");
    assert!(m.uncertain);
    assert!(
        s.complete_external_tool(
            &permit,
            KnowledgeToolResult::External(
                serde_json::json!({"web_page":{"text":"late synthetic result"}})
            )
        )
        .await
        .is_err()
    );
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM agent_steps", [], |r| r
                    .get::<_, String>(0))?,
            "completed"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM external_tool_dispatches", [], |r| r
                    .get::<_, String>(
                    0
                ))?,
            "started"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM chat_runs", [], |r| r.get::<_, String>(0))?,
            "outcome_unknown"
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn mission_expiry_guard_rejects_old_and_foreign_leases_before_exact_current_run_mutation() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let other = member(&s, &u, "expiry-other@example.test").await.user;
    let mine = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let original = s.claim_agent_run().await.unwrap().unwrap();
    begin(&s, &original.lease, vec![]).await;
    s.control_mission(&u, mine.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    s.control_mission(&u, mine.id.clone(), ctl(2, MissionAction::Resume))
        .await
        .unwrap();
    let current = s.claim_agent_run().await.unwrap().unwrap();
    let theirs = s
        .create_mission(&other, input(MissionMode::Durable))
        .await
        .unwrap();
    let their_run = theirs.id.clone();
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.check_run(&original.lease).await.is_err());
    let mut foreign = current.lease.clone();
    foreign.tenant = other.tenant.0.clone();
    foreign.run = s
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT run_id FROM missions WHERE id=?1",
                [their_run],
                |r| r.get::<_, String>(0),
            )?)
        })
        .await
        .unwrap();
    assert!(s.check_run(&foreign).await.is_err());
    s.call(|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM missions WHERE grant_enabled=1 AND state IN('queued','planning','executing')",[],|r|r.get::<_,i64>(0))?,2);Ok(())}).await.unwrap();
    assert!(matches!(
        s.check_run(&current.lease).await,
        Err(StoreError::BudgetExhausted)
    ));
    let mine_id = mine.id;
    let theirs_id = theirs.id;
    s.call(move |db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM missions WHERE id=?1", [mine_id], |r| {
                    r.get::<_, String>(0)
                })?,
            "partial"
        );
        assert_eq!(
            db.conn.query_row(
                "SELECT state,grant_enabled FROM missions WHERE id=?1",
                [theirs_id],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, bool>(1)?))
            )?,
            ("queued".into(), true)
        );
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn mission_guard_rejection_commits_expiry_for_every_transactional_mission_caller() {
    let _g = TEST_LOCK.lock().await;
    for operation in ["save_plan", "verify", "tool_failure", "clear_round"] {
        let (_d, s, u) = setup().await;
        s.create_mission(&u, input(MissionMode::Durable))
            .await
            .unwrap();
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let ctx = s.agent_context(&claim.lease).await.unwrap();
        let step = s
            .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
            .await
            .unwrap();
        s.call(|db| {
            db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
            Ok(())
        })
        .await
        .unwrap();
        let result = match operation {
            "save_plan" => {
                s.mission_save_plan(&step, vec!["Must not save after expiry".into()])
                    .await
            }
            "verify" => s
                .mission_verify(
                    &step,
                    MissionCandidate {
                        text: "Must not publish after expiry".into(),
                        evidence: vec![],
                    },
                )
                .await
                .map(|_| ()),
            "tool_failure" => s.mission_tool_failure(&step).await.map(|_| ()),
            "clear_round" => s.mission_clear_round(&step).await,
            _ => unreachable!(),
        };
        assert!(
            matches!(result, Err(StoreError::BudgetExhausted)),
            "{operation}"
        );
        // No listing/poll is allowed before checking committed writer state.
        s.call(move|db|{let(mission,uncertain,grant,carrier,repairs,plan):(String,bool,bool,String,i64,String)=db.conn.query_row("SELECT m.state,m.uncertain,m.grant_enabled,r.state,m.repairs,m.plan FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;assert_eq!(mission,"unknown","{operation}");assert!(uncertain);assert!(!grant);assert_eq!(carrier,"outcome_unknown");assert_eq!(repairs,0);assert_eq!(plan,"[]");Ok(())}).await.unwrap();
        s.update_agent_step_usage(
            &step,
            RunUsage {
                input_tokens: Some(23),
                ..RunUsage::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(s.usage_summary(&u).await.unwrap().reported_input_tokens, 23);
        assert!(s.claim_agent_run().await.unwrap().is_none());
    }
}

#[tokio::test]
async fn mission_acknowledged_final_proposal_expiry_commits_safe_partial_without_losing_receipts() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let first = begin(&s, &claim.lease, vec![calc()]).await;
    s.execute_knowledge_tool(&first, "sum".into())
        .await
        .unwrap();
    let ctx = s.agent_context(&claim.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&claim.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    let candidate = MissionCandidate {
        text: "0.3".into(),
        evidence: vec![MissionEvidence {
            step: first.step(),
            call_id: "sum".into(),
        }],
    };
    s.commit_agent_step(&final_step,PrivateCheckpoint{bytes:serde_json::to_vec(&serde_json::json!({"kind":"mission_round","planning":false,"text":serde_json::to_string(&candidate).unwrap()})).unwrap()},vec![]).await.unwrap();
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(matches!(
        s.mission_verify(&final_step, candidate).await,
        Err(StoreError::BudgetExhausted)
    ));
    s.call(|db|{let(state,uncertain,grant,candidate,round,carrier):(String,bool,bool,Option<String>,Option<String>,String)=db.conn.query_row("SELECT m.state,m.uncertain,m.grant_enabled,m.candidate,m.round_text,r.state FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?)))?;assert_eq!(state,"partial");assert!(!uncertain);assert!(!grant);assert!(candidate.is_none());assert!(round.is_some());assert_eq!(carrier,"incomplete");assert_eq!(db.conn.query_row("SELECT count(*) FROM model_call_receipts",[],|r|r.get::<_,i64>(0))?,2);assert_eq!(db.conn.query_row("SELECT count(*) FROM agent_tool_slots WHERE result IS NOT NULL",[],|r|r.get::<_,i64>(0))?,1);Ok(())}).await.unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
}

#[tokio::test]
async fn mission_guard_commit_does_not_commit_postguard_invalid_work_or_weaken_stale_lease() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let mission = s
        .create_mission(&u, input(MissionMode::Durable))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(&s, &claim.lease, vec![]).await;
    assert!(matches!(
        s.mission_save_plan(&step, vec![]).await,
        Err(StoreError::InvalidInput)
    ));
    assert!(matches!(
        s.mission_verify(
            &step,
            MissionCandidate {
                text: String::new(),
                evidence: vec![]
            }
        )
        .await,
        Err(StoreError::InvalidInput)
    ));
    s.control_mission(&u, mission.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    s.control_mission(&u, mission.id, ctl(2, MissionAction::Resume))
        .await
        .unwrap();
    let _current = s.claim_agent_run().await.unwrap().unwrap();
    s.call(|db| {
        db.conn.execute("UPDATE missions SET expires_ms=1", [])?;
        Ok(())
    })
    .await
    .unwrap();
    assert!(s.mission_clear_round(&step).await.is_err());
    s.call(|db| {
        let (grant, state, repairs, plan): (bool, String, i64, String) = db.conn.query_row(
            "SELECT grant_enabled,state,repairs,plan FROM missions",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )?;
        assert!(grant);
        assert_eq!(state, "planning");
        assert_eq!(repairs, 0);
        assert_eq!(plan, "[]");
        Ok(())
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn mission_transactional_guard_keeps_current_session_revocation_as_cancellation() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    s.create_mission(&u, input(MissionMode::CurrentSession))
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(&s, &claim.lease, vec![]).await;
    s.logout(&u).await.unwrap();
    assert!(matches!(
        s.mission_clear_round(&step).await,
        Err(StoreError::Unauthorized)
    ));
    s.call(|db| {
        assert_eq!(
            db.conn
                .query_row("SELECT state FROM missions", [], |r| r.get::<_, String>(0))?,
            "cancelled"
        );
        assert_eq!(
            db.conn
                .query_row("SELECT state,error FROM chat_runs", [], |r| Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?
                )))?,
            ("cancelled".into(), "authority_revoked".into())
        );
        Ok(())
    })
    .await
    .unwrap();
}
