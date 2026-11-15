use super::compound_learning::{fixture, plan};
use super::*;
use moosuri_connectors::gmail::SecretString;
async fn mailbox(s: &Store, u: &UserContext) -> String {
    let app = GoogleAppPin {
        revision: None,
        client_id: "synthetic-draft-learning".into(),
        redirect_uri: "http://localhost/callback".into(),
    };
    let start = s
        .start_mail_oauth_with_app(u, app.clone(), false)
        .await
        .unwrap();
    let handle = s
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = s.claim_mail_oauth_with_app(u, handle, app).await.unwrap();
    s.complete_mail_oauth(
        exchange,
        moosuri_connectors::gmail::TokenSet {
            access_token: SecretString::new("synthetic-access".into()).unwrap(),
            refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
            expires_in: 3600,
            scopes: Some(
                moosuri_connectors::gmail::MINIMAL_SCOPES
                    .iter()
                    .map(|s| s.to_string())
                    .collect(),
            ),
        },
        "owner@example.test".into(),
    )
    .await
    .unwrap()
    .id
}
fn request(key: &str, scalar: &str, total: &str, recipient: &str) -> MissionCreate {
    MissionCreate {
        request_id: Uuid::new_v4().to_string(),
        task: format!(
            "Read {key}; prefix cost:; quantity {scalar}; and prepare Gmail draft to {recipient}"
        ),
        constraints: String::new(),
        checks: vec![
            MissionCheck::DecimalEquals {
                expected: total.into(),
            },
            MissionCheck::UserReview {
                description: "Review factual draft and recipient".into(),
            },
        ],
        execution_mode: MissionMode::Durable,
    }
}
#[allow(clippy::too_many_arguments)] // Explicit fixture source, draft and expected outcome.
async fn execute(
    s: &Store,
    u: &UserContext,
    key: &str,
    src: &SourceSummary,
    scalar: &str,
    amount: &str,
    total: &str,
    recipient: &str,
    connection: &str,
    pin: Option<SkillPin>,
) -> MissionView {
    let req = request(key, scalar, total, recipient);
    let task = req.task.clone();
    let m = s.create_mission(u, req).await.unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let lease = &claim.lease;
    let calc = if let Some(pin) = pin {
        let step = begin(
            s,
            lease,
            vec![PendingKnowledgeCall {
                call_id: "calc".into(),
                input: KnowledgeToolInput::LearnedSkill {
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
                },
            }],
        )
        .await;
        s.mission_save_plan(
            &step,
            vec![
                "Read fresh source and calculate".into(),
                "Prepare local draft for review".into(),
            ],
        )
        .await
        .unwrap();
        step
    } else {
        let search = begin(
            s,
            lease,
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
                        observed_from: None,
                        observed_until: None,
                        valid_at: None,
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
                "Prepare local draft for review".into(),
            ],
        )
        .await
        .unwrap();
        s.execute_knowledge_tool(&search, "search".into())
            .await
            .unwrap();
        let read = begin(
            s,
            lease,
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
        begin(
            s,
            lease,
            vec![PendingKnowledgeCall {
                call_id: "calc".into(),
                input: KnowledgeToolInput::EvidenceCalculate {
                    calculation: EvidenceCalculation {
                        plan: plan(),
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
                },
            }],
        )
        .await
    };
    assert!(
        matches!(s.execute_knowledge_tool(&calc,"calc".into()).await.unwrap(),KnowledgeToolResult::EvidenceCalculation{exact,..} if exact==total)
    );
    let body = format!(
        "Synthetic {key}; cost: {amount}; total {total}. Narrative and recipient need review."
    );
    let prepared = begin(
        s,
        lease,
        vec![PendingKnowledgeCall {
            call_id: "prepare".into(),
            input: KnowledgeToolInput::MissionAction {
                request: MissionActionRequest::PrepareMail {
                    intent_quote: task,
                    input: MailDraftInput {
                        connection_id: connection.into(),
                        origin_thread_id: None,
                        content: MailDraftContent {
                            from: "owner@example.test".into(),
                            to: vec![recipient.into()],
                            cc: vec![],
                            bcc: vec![],
                            subject: "Synthetic calculation draft".into(),
                            text: body.clone(),
                            reply: None,
                        },
                    }
                    .into(),
                    evidence: vec![MissionEvidence {
                        step: calc.step(),
                        call_id: "calc".into(),
                    }],
                },
            },
        }],
    )
    .await;
    let result = s
        .execute_knowledge_tool(&prepared, "prepare".into())
        .await
        .unwrap();
    let serialized = serde_json::to_string(&result).unwrap();
    assert!(
        matches!(&result,KnowledgeToolResult::External(v) if v["local_draft"]==true&&v["requires_user_approval"]==true&&v["sent"]==false),
        "{serialized}"
    );
    assert_eq!(
        serde_json::to_string(
            &s.execute_knowledge_tool(&prepared, "prepare".into())
                .await
                .unwrap()
        )
        .unwrap(),
        serialized
    );
    let final_step = begin(s, lease, vec![]).await;
    assert!(
        s.mission_verify(
            &final_step,
            MissionCandidate {
                text: body,
                evidence: vec![
                    MissionEvidence {
                        step: calc.step(),
                        call_id: "calc".into()
                    },
                    MissionEvidence {
                        step: prepared.step(),
                        call_id: "prepare".into()
                    }
                ]
            }
        )
        .await
        .unwrap()
    );
    let run = lease.run_id().to_owned();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                [run],
                |r| r.get::<_, i64>(0)
            )?,
            4
        );
        Ok(())
    })
    .await
    .unwrap();
    s.missions(u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap()
}
fn adoption(m: &MissionView) -> CompoundDraftAdoption {
    let a = m.actions.iter().find(|a| a.kind == "mail").unwrap();
    CompoundDraftAdoption {
        action_id: a.id.clone(),
        revision: a.revision,
        digest: a.digest.clone(),
    }
}
#[tokio::test]
async fn compound_draft_learning_adopts_two_fresh_outputs_reuses_current_values_and_revokes() {
    let _lock = TEST_LOCK.lock().await;
    let (_d, s, u) = setup().await;
    let connection = mailbox(&s, &u).await;
    let a = fixture(&s, &u, "DraftOfferAlpha", "2").await;
    let ma = execute(
        &s,
        &u,
        "DraftOfferAlpha",
        &a,
        "3",
        "2",
        "6",
        "alpha@example.test",
        &connection,
        None,
    )
    .await;
    assert_eq!(ma.state, "partial");
    let aa = adoption(&ma);
    let id = s
        .adopt_compound_draft(&u, ma.id.clone(), aa.clone())
        .await
        .unwrap();
    assert_eq!(
        s.adopt_compound_draft(&u, ma.id.clone(), aa.clone())
            .await
            .unwrap(),
        id
    );
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|v| v.id == id)
            .unwrap()
            .readiness,
        "candidate"
    );
    let other = member(&s, &u, "draft-learning-other@example.test")
        .await
        .user;
    assert!(matches!(
        s.adopt_compound_draft(&other, ma.id.clone(), aa.clone())
            .await,
        Err(StoreError::NotFound)
    ));
    let b = fixture(&s, &u, "DraftOfferBeta", "4").await;
    let mb = execute(
        &s,
        &u,
        "DraftOfferBeta",
        &b,
        "5",
        "4",
        "20",
        "beta@example.test",
        &connection,
        None,
    )
    .await;
    s.adopt_compound_draft(&u, mb.id.clone(), adoption(&mb))
        .await
        .unwrap();
    let cards = s.learned_skills(&u).await.unwrap();
    let card = cards.iter().find(|v| v.id == id).unwrap();
    assert_eq!(card.readiness, "ready");
    assert!(card.capabilities.contains(&"compound_draft_recipe".into()));
    s.decide_mail(
        &u,
        aa.action_id.clone(),
        MailApproval {
            revision: aa.revision,
            digest: aa.digest.clone(),
        },
        true,
    )
    .await
    .unwrap();
    let c = fixture(&s, &u, "DraftOfferGamma", "6").await;
    let mc = execute(
        &s,
        &u,
        "DraftOfferGamma",
        &c,
        "7",
        "6",
        "42",
        "fresh@example.test",
        &connection,
        Some(SkillPin {
            id: id.clone(),
            revision: card.version.clone(),
            digest: card.digest.clone(),
        }),
    )
    .await;
    let ac = adoption(&mc);
    assert_ne!(ac.action_id, aa.action_id);
    let current = s.mail_action(&u, ac.action_id.clone()).await.unwrap();
    assert_eq!(current.state, "pending_approval");
    assert_eq!(
        current.content.as_ref().unwrap().to,
        vec!["fresh@example.test"]
    );
    assert!(current.content.as_ref().unwrap().text.contains("42"));
    assert!(
        !current
            .content
            .as_ref()
            .unwrap()
            .text
            .contains("DraftOfferAlpha")
    );
    assert!(matches!(
        s.adopt_compound_draft(&u, mc.id.clone(), ac).await,
        Err(StoreError::InvalidInput)
    ));
    let share = s
        .preview_learned_skill_share(
            &u,
            id.clone(),
            SkillShareInput {
                expected_revision: card.revision,
            },
        )
        .await
        .unwrap();
    let serialized = serde_json::to_string(&share).unwrap();
    for private in [
        &u.account.id,
        &connection,
        &a.id,
        "alpha@example.test",
        "DraftOfferAlpha",
        "DraftOfferBeta",
        "fresh@example.test",
    ] {
        assert!(!serialized.contains(private));
    }
    crate::community_skills::author::check(&share.slug, share.files).unwrap();
    let at = now().unwrap() + 60;
    let recurring = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: request("DraftOfferGamma", "7", "42", "scheduled@example.test"),
                schedule: ScheduleDraft {
                    title: "Synthetic draft recipe".into(),
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
    let mut unrelated_request = request("DraftOfferGamma", "7", "42", "scheduled@example.test");
    unrelated_request.task = "Compute unrelated decimal sum".into();
    let unrelated = s
        .create_recurring_mission(
            &u,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: unrelated_request,
                schedule: ScheduleDraft {
                    title: "Unrelated schedule".into(),
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
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = begin(
        &s,
        &claim.lease,
        vec![PendingKnowledgeCall {
            call_id: "scheduled-calc".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: SkillPin {
                        id: id.clone(),
                        revision: card.version.clone(),
                        digest: card.digest.clone(),
                    },
                    arguments: LearnedSkillArguments::Evidence {
                        parameters: EvidenceParameters {
                            query: "DraftOfferGamma".into(),
                            prefix: "cost:".into(),
                            scalars: vec!["7".into()],
                        },
                    },
                },
            },
        }],
    )
    .await;
    s.mission_save_plan(
        &step,
        vec!["Read and calculate before fresh local draft preparation".into()],
    )
    .await
    .unwrap();
    assert!(
        matches!(s.execute_knowledge_tool(&step,"scheduled-calc".into()).await.unwrap(),KnowledgeToolResult::EvidenceCalculation{exact,..} if exact=="42")
    );
    let old = s.mail_action(&u, aa.action_id.clone()).await.unwrap();
    let mut content = old.content.unwrap();
    content.text = "Synthetic owner changed facts".into();
    s.create_mail_draft(
        &u,
        MailDraftInput {
            connection_id: connection.clone(),
            origin_thread_id: None,
            content,
        },
        Some((aa.action_id, aa.revision)),
    )
    .await
    .unwrap();
    assert_eq!(
        s.learned_skills(&u)
            .await
            .unwrap()
            .iter()
            .find(|v| v.id == id)
            .unwrap()
            .readiness,
        "invalidated"
    );
    assert!(
        s.mail_action(&u, current.id)
            .await
            .unwrap()
            .content
            .is_none()
    );
    let schedules = s.recurring_missions(&u).await.unwrap();
    assert_eq!(
        schedules
            .iter()
            .find(|v| v.schedule.id == recurring.schedule.id)
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
}

#[tokio::test]
async fn compound_draft_learning_source_account_and_recipe_revision_boundaries() {
    let _lock = TEST_LOCK.lock().await;
    for boundary in ["source", "account", "unknown", "recipe"] {
        let (_dir, s, u) = setup().await;
        let connection = mailbox(&s, &u).await;
        let a = fixture(&s, &u, "BoundaryAlpha", "2").await;
        let ma = execute(
            &s,
            &u,
            "BoundaryAlpha",
            &a,
            "3",
            "2",
            "6",
            "alpha@example.test",
            &connection,
            None,
        )
        .await;
        let id = s
            .adopt_compound_draft(&u, ma.id.clone(), adoption(&ma))
            .await
            .unwrap();
        let b = fixture(&s, &u, "BoundaryBeta", "4").await;
        let mb = execute(
            &s,
            &u,
            "BoundaryBeta",
            &b,
            "5",
            "4",
            "20",
            "beta@example.test",
            &connection,
            None,
        )
        .await;
        s.adopt_compound_draft(&u, mb.id.clone(), adoption(&mb))
            .await
            .unwrap();
        let cards = s.learned_skills(&u).await.unwrap();
        let card = cards.iter().find(|v| v.id == id).unwrap();
        assert_eq!(card.readiness, "ready");
        match boundary {
            "source" => {
                s.delete_source(&u, a.id, a.revision).await.unwrap();
            }
            "account" => {
                s.disconnect_mail(&u, connection).await.unwrap();
            }
            "unknown" => {
                let a = adoption(&ma);
                let approval = MailApproval {
                    revision: a.revision,
                    digest: a.digest.clone(),
                };
                s.decide_mail(&u, a.action_id.clone(), approval.clone(), true)
                    .await
                    .unwrap();
                let dispatch = s.next_mail_dispatch().await.unwrap().unwrap();
                // Synthetic settlement only: no connector/provider call occurs.
                s.finish_mail_dispatch(&dispatch, None, false)
                    .await
                    .unwrap();
                assert!(s.next_mail_dispatch().await.unwrap().is_none());
                assert_eq!(
                    s.mail_action(&u, a.action_id.clone()).await.unwrap().state,
                    "outcome_unknown"
                );
                assert!(
                    s.decide_mail(&u, a.action_id, approval, true)
                        .await
                        .is_err()
                );
            }
            _ => {
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
                let route = s.compound_recipe(&u, edited.clone()).await.unwrap();
                assert_eq!(route.revision, 2);
                let cards = s.learned_skills(&u).await.unwrap();
                assert!(cards.iter().all(|v| v.id != id));
                let card = cards.iter().find(|v| v.id == edited).unwrap();
                assert_eq!(card.readiness, "candidate");
                assert!(card.capabilities.contains(&"compound_draft_recipe".into()));
                assert_eq!(
                    s.adopt_compound_draft(&u, ma.id.clone(), adoption(&ma))
                        .await
                        .unwrap(),
                    id
                );
                let tenant = u.tenant.0.clone();
                s.call(move|db|{assert_eq!(db.conn.query_row("SELECT count(*) FROM learned_skill_proofs WHERE tenant_id=?1 AND skill_id=?2",params![tenant,edited],|v|v.get::<_,i64>(0))?,0);Ok(())}).await.unwrap();
                continue;
            }
        }
        assert_eq!(
            s.learned_skills(&u)
                .await
                .unwrap()
                .iter()
                .find(|v| v.id == id)
                .unwrap()
                .readiness,
            "invalidated"
        );
        assert!(
            s.adopt_compound_draft(&u, ma.id.clone(), adoption(&ma))
                .await
                .is_err()
        );
    }
}

#[tokio::test]
async fn compound_draft_learning_preserves_preparation_proof_during_confirmed_send() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let connection = mailbox(&s, &u).await;
    let a = fixture(&s, &u, "SendingAlpha", "2").await;
    let ma = execute(
        &s,
        &u,
        "SendingAlpha",
        &a,
        "3",
        "2",
        "6",
        "alpha@example.test",
        &connection,
        None,
    )
    .await;
    let aa = adoption(&ma);
    let id = s
        .adopt_compound_draft(&u, ma.id.clone(), aa.clone())
        .await
        .unwrap();
    let b = fixture(&s, &u, "SendingBeta", "4").await;
    let mb = execute(
        &s,
        &u,
        "SendingBeta",
        &b,
        "5",
        "4",
        "20",
        "beta@example.test",
        &connection,
        None,
    )
    .await;
    s.adopt_compound_draft(&u, mb.id.clone(), adoption(&mb))
        .await
        .unwrap();
    let cards = s.learned_skills(&u).await.unwrap();
    let card = cards.iter().find(|v| v.id == id).unwrap();
    let pin = SkillPin {
        id: id.clone(),
        revision: card.version.clone(),
        digest: card.digest.clone(),
    };
    let approval = MailApproval {
        revision: aa.revision,
        digest: aa.digest.clone(),
    };
    s.decide_mail(&u, aa.action_id.clone(), approval.clone(), true)
        .await
        .unwrap();
    let dispatch = s.next_mail_dispatch().await.unwrap().unwrap();
    assert_eq!(dispatch.action_id, aa.action_id);
    assert_eq!(
        s.mail_action(&u, aa.action_id.clone()).await.unwrap().state,
        "dispatch_committed"
    );
    // Deliberately hold the synthetic dispatch across the normal refresh and
    // pin-resolution paths that previously erased both original proofs.
    for settling in [false, true] {
        if settling {
            // Store settlement only; no provider/connector or external send.
            s.finish_mail_dispatch(
                &dispatch,
                Some((
                    "synthetic-confirmed-message".into(),
                    "synthetic-confirmed-thread".into(),
                )),
                true,
            )
            .await
            .unwrap();
            assert_eq!(
                s.mail_action(&u, aa.action_id.clone()).await.unwrap().state,
                "sent"
            );
        }
        let refreshed = s.learned_skills(&u).await.unwrap();
        let current = refreshed.iter().find(|v| v.id == id).unwrap();
        assert_eq!(current.readiness, "ready");
        assert_eq!(current.digest, card.digest);
        assert_eq!(current.revision, card.revision);
        let tenant = u.tenant.0.clone();
        let account = u.account.id.clone();
        let expected = pin.clone();
        s.call(move|db|{
            assert!(crate::learned_skills::mission_eligible(&db.conn,&tenant,&account,&expected)?);
            assert_eq!(db.conn.query_row("SELECT count(*) FROM learned_skill_proofs WHERE tenant_id=?1 AND account_id=?2 AND skill_id=?3",params![tenant,account,expected.id],|v|v.get::<_,i64>(0))?,2);
            Ok(())
        }).await.unwrap();
        // Approval is not replayable and committed work cannot dispatch again.
        assert!(
            s.decide_mail(&u, aa.action_id.clone(), approval.clone(), true)
                .await
                .is_err()
        );
        assert!(s.next_mail_dispatch().await.unwrap().is_none());
    }
    // Re-observing the same original adopted output adds no training example.
    assert_eq!(
        s.adopt_compound_draft(&u, ma.id.clone(), aa).await.unwrap(),
        id
    );
}
