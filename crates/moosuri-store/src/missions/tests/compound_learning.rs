use super::*;
use moosuri_core::{
    CompoundRecipeEdit, CompoundReportAdoption, LearnedSkillArguments, LearnedSkillInvocation,
};
pub(super) fn plan() -> EvidencePlan {
    EvidencePlan {
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
    }
}
pub(super) async fn fixture(s: &Store, u: &UserContext, key: &str, amount: &str) -> SourceSummary {
    let col = s
        .create_collection(u, format!("Synthetic {key}"), Uuid::new_v4().to_string())
        .await
        .unwrap();
    s.ingest_text(
        u,
        col.id.clone(),
        IngestBatch {
            request_id: Uuid::new_v4().to_string(),
            expected_cursor: 0,
            next_cursor: 1,
            complete_snapshot: false,
            entries: vec![SourceInput {
                external_id: key.into(),
                version: "1".into(),
                expected_revision: None,
                title: format!("Synthetic {key}"),
                text: format!("{key} cost: {amount} Synthetic evidence only"),
                speaker: None,
                observed_at: 100,
                valid_from: None,
                valid_until: None,
            }],
        },
    )
    .await
    .unwrap();
    s.sources(u, col.id, None).await.unwrap().items.remove(0)
}
async fn example(
    s: &Store,
    u: &UserContext,
    key: &str,
    src: &SourceSummary,
    scalar: &str,
    amount: &str,
    total: &str,
    learned: Option<SkillPin>,
) -> MissionView {
    example_with_plan(s, u, key, src, scalar, amount, total, learned, plan()).await
}
async fn example_with_plan(
    s: &Store,
    u: &UserContext,
    key: &str,
    src: &SourceSummary,
    scalar: &str,
    amount: &str,
    total: &str,
    learned: Option<SkillPin>,
    calculation_plan: EvidencePlan,
) -> MissionView {
    let m = s
        .create_mission(
            u,
            MissionCreate {
                request_id: Uuid::new_v4().to_string(),
                task: format!(
                    "Read {key}; prefix cost:; quantity {scalar}; calculate and save private report"
                ),
                constraints: String::new(),
                checks: vec![
                    MissionCheck::PrivateReport {
                        title: "Synthetic calculation report".into(),
                    },
                    MissionCheck::DecimalEquals {
                        expected: total.into(),
                    },
                ],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    let mut evidence = vec![];
    let tool = if let Some(pin) = learned {
        KnowledgeToolInput::LearnedSkill {
            invocation: LearnedSkillInvocation {
                pin,
                arguments: LearnedSkillArguments::Evidence {
                    parameters: EvidenceParameters {
                        query: key.into(),
                        prefix: "cost:".into(),
                        scalars: vec![scalar.into()],
                    },
                },
            },
        }
    } else {
        let search = begin(
            s,
            &c.lease,
            vec![PendingKnowledgeCall {
                call_id: "search".into(),
                input: KnowledgeToolInput::Search {
                    query: KnowledgeQuery {
                        kind: KnowledgeKind::Source,
                        text: key.into(),
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
                "Read fresh source and calculate".into(),
                "Save requested report".into(),
            ],
        )
        .await
        .unwrap();
        s.execute_knowledge_tool(&search, "search".into())
            .await
            .unwrap();
        let read = begin(
            s,
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
        s.execute_knowledge_tool(&read, "read".into())
            .await
            .unwrap();
        evidence.push(MissionEvidence {
            step: read.step(),
            call_id: "read".into(),
        });
        KnowledgeToolInput::EvidenceCalculate {
            calculation: EvidenceCalculation {
                plan: calculation_plan,
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
                    query: key.into(),
                    prefix: "cost:".into(),
                    scalars: vec![scalar.into()],
                },
            },
        }
    };
    let calc = begin(
        s,
        &c.lease,
        vec![PendingKnowledgeCall {
            call_id: "calculate".into(),
            input: tool,
        }],
    )
    .await;
    if evidence.is_empty() {
        s.mission_save_plan(
            &calc,
            vec![
                "Read and calculate fresh inputs".into(),
                "Save report".into(),
            ],
        )
        .await
        .unwrap();
    }
    let result = s
        .execute_knowledge_tool(&calc, "calculate".into())
        .await
        .unwrap();
    assert!(
        matches!(&result,KnowledgeToolResult::EvidenceCalculation{exact,..} if exact==total),
        "{}",
        serde_json::to_string(&result).unwrap()
    );
    evidence.push(MissionEvidence {
        step: calc.step(),
        call_id: "calculate".into(),
    });
    let terminal = begin(s, &c.lease, vec![]).await;
    assert!(
        s.mission_verify(
            &terminal,
            MissionCandidate {
                text: format!(
                    "Synthetic {key}; cost: {amount}; total {total}. Narrative requires review."
                ),
                evidence
            }
        )
        .await
        .unwrap()
    );
    s.missions(u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap()
}
fn adoption(m: &MissionView) -> CompoundReportAdoption {
    let out = m.compound.as_ref().unwrap().output.as_ref().unwrap();
    CompoundReportAdoption {
        artifact_id: out.id.clone(),
        artifact_revision: out.revision,
    }
}
#[tokio::test]
async fn compound_learning_two_adoptions_fresh_replay_edit_and_withdrawal() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let a = fixture(&s, &u, "OfferAlpha", "2.5").await;
    let ma = example(&s, &u, "OfferAlpha", &a, "3", "2.5", "7.5", None).await;
    assert!(ma.compound.as_ref().unwrap().review_required);
    assert_eq!(ma.state, "partial");
    assert!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .all(|c| !c.capabilities.contains(&"compound_report_recipe".into()))
    );
    let id = s
        .adopt_compound_report(&u, ma.id.clone(), adoption(&ma))
        .await
        .unwrap();
    let cards = s.learned_skills(&u).await.unwrap();
    let card = cards.iter().find(|c| c.id == id).unwrap();
    assert_eq!(card.readiness, "candidate");
    assert_eq!(
        s.adopt_compound_report(&u, ma.id.clone(), adoption(&ma))
            .await
            .unwrap(),
        id
    );
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .readiness,
        "candidate"
    );
    let outsider = member(&s, &u, "compound-outsider@example.test").await.user;
    assert!(matches!(
        s.adopt_compound_report(&outsider, ma.id.clone(), adoption(&ma))
            .await,
        Err(StoreError::NotFound)
    ));
    let repeat = example(&s, &u, "OfferAlpha", &a, "3", "2.5", "7.5", None).await;
    s.adopt_compound_report(&u, repeat.id.clone(), adoption(&repeat))
        .await
        .unwrap();
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|c| c.id == id)
            .unwrap()
            .readiness,
        "candidate"
    );
    let b = fixture(&s, &u, "OfferBeta", "4").await;
    let mb = example(&s, &u, "OfferBeta", &b, "6", "4", "24", None).await;
    assert_eq!(
        s.adopt_compound_report(&u, mb.id.clone(), adoption(&mb))
            .await
            .unwrap(),
        id
    );
    let cards = s.learned_skills(&u).await.unwrap();
    let card = cards.iter().find(|c| c.id == id).unwrap();
    assert_eq!(card.readiness, "ready");
    let pin = SkillPin {
        id: id.clone(),
        revision: card.version.clone(),
        digest: card.digest.clone(),
    };
    let c = fixture(&s, &u, "OfferGamma", "8").await;
    let mc = example(&s, &u, "OfferGamma", &c, "9", "8", "72", Some(pin.clone())).await;
    assert!(mc.compound.as_ref().unwrap().review_required);
    assert!(mc.compound.as_ref().unwrap().output.is_some());
    assert!(matches!(
        s.adopt_compound_report(&u, mc.id.clone(), adoption(&mc))
            .await,
        Err(StoreError::InvalidInput)
    ));
    let share = s
        .preview_learned_skill_share(
            &u,
            id.clone(),
            moosuri_core::SkillShareInput {
                expected_revision: card.revision,
            },
        )
        .await
        .unwrap();
    let exported = serde_json::to_string(&share).unwrap();
    for private in [
        &u.account.id,
        &a.id,
        &b.id,
        "OfferAlpha",
        "OfferBeta",
        "OfferGamma",
    ] {
        assert!(!exported.contains(private));
    }
    crate::community_skills::author::check(&share.slug, share.files).unwrap();
    let at = now().unwrap() + 60;
    let schedule = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: MissionCreate {
                    request_id: Uuid::new_v4().to_string(),
                    task: "Read OfferGamma; prefix cost:; quantity 9; save report".into(),
                    constraints: String::new(),
                    checks: vec![MissionCheck::PrivateReport {
                        title: "Synthetic scheduled report".into(),
                    }],
                    execution_mode: MissionMode::Durable,
                },
                schedule: ScheduleDraft {
                    title: "Synthetic compound schedule".into(),
                    message: "Synthetic".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: false,
            },
        )
        .await
        .unwrap();
    let unrelated = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: MissionCreate {
                    request_id: Uuid::new_v4().to_string(),
                    task: "Compute unrelated total".into(),
                    constraints: String::new(),
                    checks: vec![MissionCheck::DecimalEquals {
                        expected: "2".into(),
                    }],
                    execution_mode: MissionMode::Durable,
                },
                schedule: ScheduleDraft {
                    title: "Unrelated synthetic schedule".into(),
                    message: "Synthetic".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at + 30,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: false,
            },
        )
        .await
        .unwrap();
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    let scheduled = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(
        &s,
        &scheduled.lease,
        vec![PendingKnowledgeCall {
            call_id: "fresh".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Evidence {
                        parameters: EvidenceParameters {
                            query: "OfferGamma".into(),
                            prefix: "cost:".into(),
                            scalars: vec!["9".into()],
                        },
                    },
                },
            },
        }],
    )
    .await;
    s.mission_save_plan(&step, vec!["Read fresh values, calculate, report".into()])
        .await
        .unwrap();
    assert!(
        matches!(s.execute_knowledge_tool(&step,"fresh".into()).await.unwrap(),KnowledgeToolResult::EvidenceCalculation{exact,..} if exact=="72")
    );
    let run = scheduled.lease.run_id().to_owned();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                [run],
                |v| v.get::<_, i64>(0)
            )?,
            3
        );
        Ok(())
    })
    .await
    .unwrap();
    s.mutate_learned_skill(
        &u,
        id.clone(),
        moosuri_core::CommunitySkillMutation {
            action: "disable".into(),
            expected_revision: card.revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    let schedules = s.recurring_missions(&u).await.unwrap();
    assert_eq!(
        schedules
            .iter()
            .find(|v| v.schedule.id == schedule.schedule.id)
            .unwrap()
            .schedule
            .reason
            .as_deref(),
        Some("needs_skill")
    );
    assert_eq!(
        schedules
            .iter()
            .find(|v| v.schedule.id == unrelated.schedule.id)
            .unwrap()
            .schedule
            .state,
        "active"
    );
    let revision = s
        .learned_skills(&u)
        .await
        .unwrap()
        .iter()
        .find(|v| v.id == id)
        .unwrap()
        .revision;
    s.mutate_learned_skill(
        &u,
        id.clone(),
        moosuri_core::CommunitySkillMutation {
            action: "enable".into(),
            expected_revision: revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    let paused = s
        .recurring_missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.schedule.id == schedule.schedule.id)
        .unwrap();
    assert_eq!(paused.schedule.state, "paused");
    s.control_recurring_mission(
        &u,
        paused.schedule.id.clone(),
        ScheduleControl {
            expected_revision: paused.schedule.revision,
            action: "resume".into(),
        },
    )
    .await
    .unwrap();
    let cards = s.learned_skills(&u).await.unwrap();
    let card = cards.iter().find(|v| v.id == id).unwrap();
    let edited = s
        .edit_compound_recipe(
            &u,
            id.clone(),
            CompoundRecipeEdit {
                expected_revision: card.revision,
                calculation: plan(),
            },
        )
        .await
        .unwrap();
    let cards = s.learned_skills(&u).await.unwrap();
    assert!(cards.iter().all(|c| c.id != id));
    assert_eq!(
        cards.iter().find(|c| c.id == edited).unwrap().readiness,
        "candidate"
    );
    // A/B replay must not populate the edited revision.
    s.adopt_compound_report(&u, ma.id.clone(), adoption(&ma))
        .await
        .unwrap();
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|c| c.id == edited)
            .unwrap()
            .readiness,
        "candidate"
    );
    let d = fixture(&s, &u, "OfferDelta", "10").await;
    let md = example(&s, &u, "OfferDelta", &d, "11", "10", "110", None).await;
    s.adopt_compound_report(&u, md.id.clone(), adoption(&md))
        .await
        .unwrap();
    let e = fixture(&s, &u, "OfferEpsilon", "12").await;
    let me = example(&s, &u, "OfferEpsilon", &e, "13", "12", "156", None).await;
    s.adopt_compound_report(&u, me.id.clone(), adoption(&me))
        .await
        .unwrap();
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|c| c.id == edited)
            .unwrap()
            .readiness,
        "ready"
    );
    let doc = adoption(&md);
    s.write_artifact(
        &u,
        ArtifactWrite {
            request_id: Uuid::new_v4().to_string(),
            id: Some(doc.artifact_id),
            expected_revision: Some(doc.artifact_revision),
            draft: ArtifactDraft {
                title: "Synthetic owner edit".into(),
                body: "Changed factual output".into(),
                payload: ArtifactPayload::Document,
                references: vec![],
            },
        },
    )
    .await
    .unwrap();
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|c| c.id == edited)
            .unwrap()
            .readiness,
        "invalidated"
    );
}

#[tokio::test]
async fn compound_learning_cross_graph_edit_preserves_untouched_proof_and_used_schedule() {
    let _g = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let mut sum_plan = plan();
    sum_plan.graph.steps[0].operation = DecimalOperation::Sum;
    let mut product_id = String::new();
    let mut sum_id = String::new();
    let mut sum_outputs = vec![];
    for (key, amount, scalar, total, calculation) in [
        ("ProductOne", "5", "2", "10", plan()),
        ("ProductTwo", "6", "3", "18", plan()),
        ("SumOne", "7", "4", "11", sum_plan.clone()),
        ("SumTwo", "8", "5", "13", sum_plan.clone()),
    ] {
        let source = fixture(&s, &u, key, amount).await;
        let output = example_with_plan(
            &s,
            &u,
            key,
            &source,
            scalar,
            amount,
            total,
            None,
            calculation,
        )
        .await;
        let id = s
            .adopt_compound_report(&u, output.id.clone(), adoption(&output))
            .await
            .unwrap();
        if key.starts_with("Product") {
            product_id = id;
        } else {
            sum_id = id;
            sum_outputs.push(output);
        }
    }
    let cards = s.learned_skills(&u).await.unwrap();
    let product = cards.iter().find(|c| c.id == product_id).unwrap();
    let sum = cards.iter().find(|c| c.id == sum_id).unwrap();
    assert_eq!(product.readiness, "ready");
    assert_eq!(sum.readiness, "ready");
    let source = fixture(&s, &u, "SumCurrent", "9").await;
    let at = now().unwrap() + 60;
    let scheduled = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: MissionCreate {
                    request_id: Uuid::new_v4().to_string(),
                    task: "Read SumCurrent; prefix cost:; quantity 6; calculate and save report"
                        .into(),
                    constraints: String::new(),
                    checks: vec![MissionCheck::PrivateReport {
                        title: "Synthetic sum report".into(),
                    }],
                    execution_mode: MissionMode::Durable,
                },
                schedule: ScheduleDraft {
                    title: "Untouched synthetic sum".into(),
                    message: "Synthetic".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: false,
            },
        )
        .await
        .unwrap();
    s.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(s.poll_recurring_missions_at(at).await.unwrap(), 1);
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(
        &s,
        &claim.lease,
        vec![PendingKnowledgeCall {
            call_id: "untouched-sum".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: SkillPin {
                        id: sum_id.clone(),
                        revision: sum.version.clone(),
                        digest: sum.digest.clone(),
                    },
                    arguments: LearnedSkillArguments::Evidence {
                        parameters: EvidenceParameters {
                            query: "SumCurrent".into(),
                            prefix: "cost:".into(),
                            scalars: vec!["6".into()],
                        },
                    },
                },
            },
        }],
    )
    .await;
    s.mission_save_plan(
        &step,
        vec!["Read current source, calculate sum and save report".into()],
    )
    .await
    .unwrap();
    assert!(
        matches!(s.execute_knowledge_tool(&step, "untouched-sum".into()).await.unwrap(), KnowledgeToolResult::EvidenceCalculation { exact, evidence, .. } if exact == "15" && evidence.source_id == source.id)
    );
    let edited_id = s
        .edit_compound_recipe(
            &u,
            product_id.clone(),
            CompoundRecipeEdit {
                expected_revision: product.revision,
                calculation: sum_plan,
            },
        )
        .await
        .unwrap();
    assert_ne!(edited_id, sum_id);
    let refreshed = s.learned_skills(&u).await.unwrap();
    assert_eq!(
        refreshed.iter().find(|c| c.id == sum_id).unwrap().readiness,
        "ready"
    );
    assert_eq!(
        refreshed
            .iter()
            .find(|c| c.id == edited_id)
            .unwrap()
            .readiness,
        "candidate"
    );
    assert_eq!(
        s.recurring_missions(&u)
            .await
            .unwrap()
            .iter()
            .find(|m| m.schedule.id == scheduled.schedule.id)
            .unwrap()
            .schedule
            .state,
        "active"
    );
    // Replaying the original adoptions must retain Sum-v1, never populate Sum-v2.
    for output in sum_outputs {
        assert_eq!(
            s.adopt_compound_report(&u, output.id.clone(), adoption(&output))
                .await
                .unwrap(),
            sum_id
        );
    }
    let tenant = u.tenant.0.clone();
    let account = u.account.id.clone();
    s.call(move |db| {
        assert_eq!(db.conn.query_row("SELECT count(*) FROM learned_skill_proofs WHERE tenant_id=?1 AND account_id=?2 AND skill_id=?3", params![tenant,account,sum_id], |r|r.get::<_,i64>(0))?, 2);
        assert_eq!(db.conn.query_row("SELECT count(*) FROM learned_skill_proofs WHERE tenant_id=?1 AND account_id=?2 AND skill_id=?3", params![tenant,account,edited_id], |r|r.get::<_,i64>(0))?, 0);
        assert_eq!(db.conn.query_row("SELECT count(*) FROM compound_report_adoptions d JOIN learned_skills s ON s.tenant_id=d.tenant_id AND s.account_id=d.account_id AND s.route=d.recipe_route JOIN learned_skill_proofs p ON p.tenant_id=s.tenant_id AND p.account_id=s.account_id AND p.skill_id=s.id AND p.run_id=d.run_id WHERE s.tenant_id=?1 AND s.account_id=?2 AND s.id=?3", params![tenant,account,sum_id], |r|r.get::<_,i64>(0))?, 2);
        Ok(())
    }).await.unwrap();
    // The already executed untouched slot also stays readable after the edit.
    assert!(
        matches!(s.execute_knowledge_tool(&step, "untouched-sum".into()).await.unwrap(), KnowledgeToolResult::EvidenceCalculation { exact, .. } if exact == "15")
    );
}
