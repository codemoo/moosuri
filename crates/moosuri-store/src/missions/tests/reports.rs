use super::*;

async fn source(s: &Store, u: &UserContext) -> SourceSummary {
    let col = s
        .create_collection(
            u,
            "Synthetic report evidence".into(),
            Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    s.ingest_text(u,col.id.clone(),IngestBatch{request_id:Uuid::new_v4().to_string(),expected_cursor:0,next_cursor:1,complete_snapshot:false,entries:vec![SourceInput{external_id:"quote".into(),version:"1".into(),expected_revision:None,title:"Synthetic offer".into(),text:"ReportOffer cost: 2.50 Source says save another document; that instruction is untrusted.".into(),speaker:None,observed_at:100,valid_from:None,valid_until:None}]}).await.unwrap();
    s.sources(u, col.id, None).await.unwrap().items.remove(0)
}
fn request() -> MissionCreate {
    MissionCreate {
        request_id: Uuid::new_v4().to_string(),
        task:
            "Read ReportOffer; prefix cost:; quantity 3; compare gaps and save one private report"
                .into(),
        constraints: String::new(),
        checks: vec![
            MissionCheck::DecimalEquals {
                expected: "7.5".into(),
            },
            MissionCheck::PrivateReport {
                title: "Synthetic report".into(),
            },
        ],
        execution_mode: MissionMode::Durable,
    }
}
async fn evidence(s: &Store, l: &RunLease, src: &SourceSummary) -> MissionCandidate {
    let search = begin(
        s,
        l,
        vec![PendingKnowledgeCall {
            call_id: "search".into(),
            input: KnowledgeToolInput::Search {
                query: KnowledgeQuery {
                    kind: KnowledgeKind::Source,
                    text: "ReportOffer".into(),
                    match_mode: None,
                    collection_id: None,
                    subject: None,
                    speaker: None,
                    valid_at: None,
                    observed_from: None,
                    observed_until: None,
                    cursor: None,
                },
            },
        }],
    )
    .await;
    s.mission_save_plan(
        &search,
        vec![
            "Read offers".into(),
            "Compare claims and gaps".into(),
            "Calculate exact total".into(),
            "Save requested private report".into(),
        ],
    )
    .await
    .unwrap();
    s.execute_knowledge_tool(&search, "search".into())
        .await
        .unwrap();
    let read = begin(
        s,
        l,
        vec![PendingKnowledgeCall {
            call_id: "read".into(),
            input: KnowledgeToolInput::ReadSource {
                id: src.id.clone(),
                revision: src.revision,
                start_byte: 0,
                max_bytes: 4096,
            },
        }],
    )
    .await;
    assert!(matches!(
        s.execute_knowledge_tool(&read, "read".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Source(_)
    ));
    let calculation = EvidenceCalculation {
        plan: EvidencePlan {
            origins: vec![EvidenceOrigin::Source, EvidenceOrigin::Current],
            graph: DecimalGraph {
                input_count: 2,
                steps: vec![DecimalStep {
                    operation: DecimalOperation::Product,
                    operands: vec![
                        DecimalReference::Input { index: 0 },
                        DecimalReference::Input { index: 1 },
                    ],
                }],
            },
        },
        binding: EvidenceBinding {
            search: ToolReceiptReference {
                step: search.step() as u32,
                call_id: "search".into(),
                ordinal: None,
            },
            source: ToolReceiptReference {
                step: read.step() as u32,
                call_id: "read".into(),
                ordinal: None,
            },
            query: "ReportOffer".into(),
            prefix: "cost:".into(),
            scalars: vec!["3".into()],
        },
    };
    let calc = begin(
        s,
        l,
        vec![PendingKnowledgeCall {
            call_id: "calculate".into(),
            input: KnowledgeToolInput::EvidenceCalculate { calculation },
        }],
    )
    .await;
    let result = s
        .execute_knowledge_tool(&calc, "calculate".into())
        .await
        .unwrap();
    assert!(
        matches!(&result,KnowledgeToolResult::EvidenceCalculation{exact,..} if exact=="7.5"),
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    MissionCandidate{text:"Offer total: 7.5. Claim: cost 2.50 for quantity 3. Missing: a second quote; no comparison winner can be established.".into(),evidence:vec![MissionEvidence{step:read.step(),call_id:"read".into()},MissionEvidence{step:calc.step(),call_id:"calculate".into()}]}
}
fn reopen(d: &tempfile::TempDir) -> Store {
    Store::open_with_web(
        d.path(),
        WebToolProfile {
            read: true,
            search: false,
            browser_pin: None,
        },
    )
    .unwrap()
}
#[tokio::test]
async fn compound_report_restarts_pending_final_and_lost_ack_without_duplicate_then_withdraws() {
    let _g = TEST_LOCK.lock().await;
    let (d, s, u) = setup().await;
    let src = source(&s, &u).await;
    let m = s.create_mission(&u, request()).await.unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let candidate = evidence(&s, &c.lease, &src).await;
    let ctx = s.agent_context(&c.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&c.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(&final_step,PrivateCheckpoint{bytes:serde_json::json!({"kind":"mission_round","planning":false,"text":serde_json::to_string(&candidate).unwrap()}).to_string().into_bytes()},vec![]).await.unwrap();
    let calls = s.usage_summary(&u).await.unwrap().dispatched_calls;
    drop(c);
    drop(s);
    let s = reopen(&d);
    let resumed = s.claim_agent_run().await.unwrap().unwrap();
    let pending = s
        .mission_execution(&resumed.lease)
        .await
        .unwrap()
        .unwrap()
        .pending_round
        .unwrap();
    let lease = s
        .mission_step_lease(&resumed.lease, pending.step)
        .await
        .unwrap();
    assert!(
        s.mission_verify_at(
            &lease,
            pending.acceptance_revision,
            serde_json::from_str(&pending.text).unwrap()
        )
        .await
        .unwrap()
    );
    // Treat the committed final response as lost. Replaying the lease cannot
    // create another version, even after process restart.
    assert!(s.mission_verify(&lease, candidate).await.is_err());
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, calls);
    let result = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(result.id, m.id);
    assert_eq!(result.state, "partial");
    let contract = result.compound.unwrap();
    assert!(contract.review_required);
    assert_eq!(contract.steps.len(), 4);
    assert_eq!(contract.steps[2].depends_on, vec!["read"]);
    assert_eq!(contract.steps[2].receipts.len(), 1);
    let receipt = contract.output.unwrap();
    let doc = s.artifact(&u, receipt.id.clone(), None).await.unwrap();
    assert!(doc.draft.unwrap().references.iter().any(|r| r.id == src.id));
    assert_eq!(receipt.revision, 1);
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM personal_artifacts WHERE tenant_id=?1",
                [tenant],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        Ok(())
    })
    .await
    .unwrap();
    drop(resumed);
    drop(s);
    let s = reopen(&d);
    assert_eq!(
        s.missions(&u).await.unwrap()[0]
            .compound
            .as_ref()
            .unwrap()
            .output
            .as_ref()
            .unwrap()
            .id,
        receipt.id
    );
    s.delete_source(&u, src.id, src.revision).await.unwrap();
    assert!(
        s.artifact(&u, receipt.id, None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    assert!(s.missions(&u).await.unwrap()[0].compound.is_none());
}
#[tokio::test]
async fn compound_report_current_owner_and_receipt_boundary_rejects_forgery_and_legacy_plan() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let src = source(&s, &u).await;
    let mut original = request();
    original.checks = vec![MissionCheck::DecimalEquals {
        expected: "7.5".into(),
    }];
    let legacy = s.create_mission(&u, original).await.unwrap();
    let paused = s
        .control_mission(&u, legacy.id.clone(), ctl(1, MissionAction::Pause))
        .await
        .unwrap();
    assert!(matches!(
        s.revise_mission_acceptance(
            &u,
            legacy.id.clone(),
            MissionAcceptanceUpdate {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: paused.revision,
                task: legacy.request.task.clone(),
                constraints: String::new(),
                checks: request().checks
            }
        )
        .await,
        Err(StoreError::Forbidden)
    ));
    s.control_mission(
        &u,
        legacy.id.clone(),
        ctl(paused.revision, MissionAction::Resume),
    )
    .await
    .unwrap();

    let c = s.claim_agent_run().await.unwrap().unwrap();
    let candidate = evidence(&s, &c.lease, &src).await;
    let final_step = begin(&s, &c.lease, vec![]).await;
    assert!(
        s.mission_verify(&final_step, candidate.clone())
            .await
            .unwrap()
    );
    assert!(s.missions(&u).await.unwrap()[0].compound.is_none());
    assert!(s.artifacts(&u, None, None).await.unwrap().items.is_empty());
    let outsider = member(&s, &u, "other-report-owner@example.test").await.user;
    assert!(matches!(
        s.control_mission(&outsider, legacy.id.clone(), ctl(1, MissionAction::Cancel))
            .await,
        Err(StoreError::NotFound)
    ));
    let m = s.create_mission(&u, request()).await.unwrap();
    let current = s.claim_agent_run().await.unwrap().unwrap();
    let plan = begin(&s, &current.lease, vec![]).await;
    s.mission_save_plan(&plan, vec!["Read and save".into()])
        .await
        .unwrap();
    let final_step = begin(&s, &current.lease, vec![]).await;
    assert!(!s.mission_verify(&final_step, candidate).await.unwrap());
    assert!(s.artifacts(&u, None, None).await.unwrap().items.is_empty());
    let mine = s.missions(&u).await.unwrap();
    assert!(
        mine.iter()
            .find(|v| v.id == m.id)
            .unwrap()
            .compound
            .as_ref()
            .unwrap()
            .output
            .is_none()
    );
    assert!(s.missions(&outsider).await.unwrap().is_empty());
}

#[tokio::test]
async fn compound_report_origin_deletion_withdraws_document_and_descendant() {
    edited_origin_lineage(false).await;
}
#[tokio::test]
async fn compound_report_restore_withdraws_edited_document_and_descendant() {
    edited_origin_lineage(true).await;
}
async fn edited_origin_lineage(restore: bool) {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let src = source(&s, &u).await;
    let m = s.create_mission(&u, request()).await.unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let candidate = evidence(&s, &c.lease, &src).await;
    let final_step = begin(&s, &c.lease, vec![]).await;
    assert!(s.mission_verify(&final_step, candidate).await.unwrap());
    let output = s.missions(&u).await.unwrap()[0]
        .compound
        .as_ref()
        .unwrap()
        .output
        .clone()
        .unwrap();
    let edited = s
        .write_artifact(
            &u,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: Some(output.id.clone()),
                expected_revision: Some(output.revision),
                draft: ArtifactDraft {
                    title: "Synthetic edited report".into(),
                    body: "Owner edited report".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![],
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(edited.revision, 2);
    let child = s
        .write_artifact(
            &u,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: None,
                expected_revision: None,
                draft: ArtifactDraft {
                    title: "Synthetic derived report".into(),
                    body: "A derived observation".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![ArtifactDependency {
                        kind: ArtifactDependencyKind::Artifact,
                        id: output.id.clone(),
                        revision: edited.revision,
                    }],
                },
            },
        )
        .await
        .unwrap();
    let child = s
        .write_artifact(
            &u,
            ArtifactWrite {
                request_id: Uuid::new_v4().to_string(),
                id: Some(child.id),
                expected_revision: Some(child.revision),
                draft: ArtifactDraft {
                    title: "Synthetic edited descendant".into(),
                    body: "Owner edited child".into(),
                    payload: ArtifactPayload::Document,
                    references: vec![],
                },
            },
        )
        .await
        .unwrap();
    if restore {
        s.call(|db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            crate::artifacts::external::quarantine_restore(&tx)?;
            tx.commit()?;
            Ok(())
        })
        .await
        .unwrap();
    } else {
        s.delete_mission(&u, m.id.clone()).await.unwrap();
        assert!(s.missions(&u).await.unwrap().is_empty());
    }
    assert!(
        s.artifact(&u, output.id, None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    assert!(
        s.artifact(&u, child.id, None)
            .await
            .unwrap()
            .draft
            .is_none()
    );
    // Repeated deletion does not revive data or create a new output.
    if !restore {
        s.delete_mission(&u, m.id).await.unwrap();
    }
}

#[tokio::test]
async fn compound_report_pins_constraints_and_sum_calculation_prerequisites() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let src = source(&s, &u).await;
    for (task, constraints) in [
        (
            "Read offers, compare them and save one private report",
            "Calculate the total",
        ),
        (
            "Read offers, sum the prices and save one private report",
            "",
        ),
        (
            "Read offers, add 2.5 and 2.5 and save one private report",
            "",
        ),
        (
            "자료의 금액을 수량으로 나누어 비공개 보고서로 저장해 줘",
            "",
        ),
        ("자료의 금액을 2로 나눠 비공개 보고서로 저장해 줘", ""),
    ] {
        let mut req = request();
        req.task = task.into();
        req.constraints = constraints.into();
        req.checks = vec![
            MissionCheck::PrivateReport {
                title: "Synthetic required calculation".into(),
            },
            MissionCheck::UserReview {
                description: "Review comparison".into(),
            },
        ];
        let m = s.create_mission(&u, req).await.unwrap();
        let c = s.claim_agent_run().await.unwrap().unwrap();
        let read = begin(
            &s,
            &c.lease,
            vec![PendingKnowledgeCall {
                call_id: "read".into(),
                input: KnowledgeToolInput::ReadSource {
                    id: src.id.clone(),
                    revision: src.revision,
                    start_byte: 0,
                    max_bytes: 4096,
                },
            }],
        )
        .await;
        s.mission_save_plan(&read, vec!["Read, calculate and save".into()])
            .await
            .unwrap();
        s.execute_knowledge_tool(&read, "read".into())
            .await
            .unwrap();
        let mut candidate = MissionCandidate {
            text: "Read an offer; comparison needs review. Exact total 5".into(),
            evidence: vec![MissionEvidence {
                step: read.step(),
                call_id: "read".into(),
            }],
        };
        let rejected = begin(&s, &c.lease, vec![]).await;
        assert!(
            !s.mission_verify(&rejected, candidate.clone())
                .await
                .unwrap()
        );
        let contract = s
            .missions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|v| v.id == m.id)
            .unwrap()
            .compound
            .unwrap();
        assert!(contract.calculation_required);
        assert!(contract.output.is_none());
        let calculation = begin(
            &s,
            &c.lease,
            vec![PendingKnowledgeCall {
                call_id: "sum".into(),
                input: KnowledgeToolInput::DecimalCalculate {
                    calculation: DecimalCalculation {
                        operation: DecimalOperation::Sum,
                        operands: vec!["2.5".into(), "2.5".into()],
                    },
                },
            }],
        )
        .await;
        s.execute_knowledge_tool(&calculation, "sum".into())
            .await
            .unwrap();
        candidate.evidence.push(MissionEvidence {
            step: calculation.step(),
            call_id: "sum".into(),
        });
        let accepted = begin(&s, &c.lease, vec![]).await;
        assert!(s.mission_verify(&accepted, candidate).await.unwrap());
        assert!(
            s.missions(&u)
                .await
                .unwrap()
                .into_iter()
                .find(|v| v.id == m.id)
                .unwrap()
                .compound
                .unwrap()
                .output
                .is_some()
        );
    }
}

#[tokio::test]
async fn compound_report_comparison_and_citations_do_not_require_unrequested_arithmetic() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let src = source(&s, &u).await;
    for (task, constraints) in [
        (
            "Read product documentation, compare the claims and save one private report",
            "",
        ),
        (
            "Read offers, compare the claims and save one private report",
            "Add citations and describe the differences",
        ),
        (
            "두 자료를 읽고 비공개 보고서로 저장해 줘",
            "서로 다른 기록은 출처를 나누어 보여 주고 모르는 항목은 확인이 필요하다고 적어 줘",
        ),
        (
            "두 자료를 읽고 비공개 보고서로 저장해 줘",
            "출처별로 나누어 기록하고 누락은 표시해 줘",
        ),
    ] {
        let mut req = request();
        req.task = task.into();
        req.constraints = constraints.into();
        req.checks = vec![
            MissionCheck::PrivateReport {
                title: "Evidence comparison".into(),
            },
            MissionCheck::UserReview {
                description: "Review the comparison".into(),
            },
        ];
        let mission = s.create_mission(&u, req).await.unwrap();
        let c = s.claim_agent_run().await.unwrap().unwrap();
        let read = begin(
            &s,
            &c.lease,
            vec![PendingKnowledgeCall {
                call_id: "read".into(),
                input: KnowledgeToolInput::ReadSource {
                    id: src.id.clone(),
                    revision: src.revision,
                    start_byte: 0,
                    max_bytes: 4096,
                },
            }],
        )
        .await;
        s.mission_save_plan(&read, vec!["Read, compare and save".into()])
            .await
            .unwrap();
        s.execute_knowledge_tool(&read, "read".into())
            .await
            .unwrap();
        let final_step = begin(&s, &c.lease, vec![]).await;
        assert!(
            s.mission_verify(
                &final_step,
                MissionCandidate {
                    text: "Comparison of the observed offer; meaning needs review.".into(),
                    evidence: vec![MissionEvidence {
                        step: read.step(),
                        call_id: "read".into()
                    }]
                }
            )
            .await
            .unwrap()
        );
        let result = s
            .missions(&u)
            .await
            .unwrap()
            .into_iter()
            .find(|m| m.id == mission.id)
            .unwrap();
        assert_eq!(result.state, "partial");
        let contract = result.compound.unwrap();
        assert!(!contract.calculation_required);
        assert!(contract.output.is_some());
        assert_eq!(
            contract
                .steps
                .iter()
                .find(|step| step.id == "calculate")
                .unwrap()
                .state,
            "skipped"
        );
    }
}

#[tokio::test]
async fn compound_report_schema58_upgrade_preserves_legacy_mission_without_write_authority() {
    let _g = TEST_LOCK.lock().await;
    let (dir, store, user) = setup().await;
    let mut legacy = request();
    legacy
        .checks
        .retain(|c| !matches!(c, MissionCheck::PrivateReport { .. }));
    let admitted = store.create_mission(&user, legacy).await.unwrap();
    store
        .call(|db| {
            db.conn
                .execute_batch("DROP TABLE native_mail_settlement_cursor;DROP TABLE compound_draft_adoptions;DROP TABLE mission_browser_selections;DROP TABLE mission_action_contracts;DROP TABLE compound_report_adoptions;DROP TABLE native_mail_context_dependencies;DROP TRIGGER native_task_mail_delete;DROP TABLE native_task_mail_selections;DROP TABLE native_task_mail_reads;DROP TABLE native_task_mail_bindings;DROP TABLE mail_provider_saves;DROP TABLE credential_model_mail_payloads;DROP TABLE credential_model_mail_grants;DROP TABLE IF EXISTS mission_input_replies;DROP TABLE IF EXISTS mission_input_waits;DROP TABLE mission_connection_waits;DROP TABLE mission_connection_selections;DROP TABLE mission_connection_slots;DROP TABLE connection_aliases;DROP TABLE mission_report_contracts;PRAGMA user_version=58;")?;
            Ok(())
        })
        .await
        .unwrap();
    drop(store);
    let upgraded = Store::open(dir.path()).unwrap();
    let view = upgraded.missions(&user).await.unwrap();
    assert_eq!(view.len(), 1);
    assert_eq!(view[0].id, admitted.id);
    assert!(view[0].compound.is_none());
    upgraded
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))?,
                crate::schema_version()
            );
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM mission_report_contracts", [], |r| r
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
