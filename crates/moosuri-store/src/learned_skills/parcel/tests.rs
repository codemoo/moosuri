//! Synthetic typed observations through actual durable dispatch/finalization.
use super::*;
use crate::{
    agent_execution::*,
    tests::{TEST_LOCK, admin, directory, member},
};
use moosuri_core::*;
use serde_json::json;
const A: &str = "AA123456789JP";
const B: &str = "BB123456789JP";
const C: &str = "CC123456789JP";
async fn setup() -> (tempfile::TempDir, Store, UserContext) {
    let dir = directory();
    let s = Store::open_with_web(
        dir.path(),
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
            crate::ProviderChange {
                name: "Synthetic learned parcel".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
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
    (dir, s, u)
}
fn observed(number: &str, state: &str) -> Output {
    Output::External(
        json!({"parcel":{"carrier":"japan_post","tracking_number":number,"state":state,"coverage":"latest_summary_only","events":if state=="observed"{json!([{"reported_time_text":"2026/11/15","local_date":"2026-11-15","occurred_at":null,"status_text":"PRIVATE_STATUS_CANARY","handling_office":"PRIVATE_OFFICE_CANARY","prefecture":"PRIVATE_PREFECTURE_CANARY"}])}else{json!([])},"source":{"url":moosuri_connectors::parcel::url(number).unwrap().to_string(),"title":"Japan Post synthetic summary","retrieved_at_unix_ms":12345,"truncated":false,"visible_excerpt":format!("{number} | 2026/11/15 | PRIVATE_STATUS_CANARY | PRIVATE_OFFICE_CANARY")}}}),
    )
}
fn answer(number: &str) -> String {
    format!(
        "{number}: 2026/11/15 PRIVATE_STATUS_CANARY {} (latest summary only)",
        moosuri_connectors::parcel::url(number).unwrap()
    )
}
async fn start(
    s: &Store,
    u: &UserContext,
    text: &str,
    input: Input,
) -> (String, AgentClaim, StepLease) {
    let thread = s
        .create_thread(u, "PRIVATE_PARCEL_TITLE_CANARY".into())
        .await
        .unwrap()
        .id;
    s.submit_turn(u, thread.clone(), Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = s
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"synthetic":true}"#.to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "parcel".into(),
            input,
        }],
    )
    .await
    .unwrap();
    (thread, claim, step)
}
async fn finish(s: &Store, claim: &AgentClaim, text: &str, phase: Option<MessagePhase>) {
    let context = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: br#"{"synthetic_final":true}"#.to_vec(),
        },
        vec![],
    )
    .await
    .unwrap();
    s.append_agent_output(
        &step,
        vec![crate::OutputPatch::Append {
            item: format!("step-{}-answer", step.step()),
            text: text.into(),
            phase,
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.finish_agent_run(&step, RunState::Completed, None)
        .await
        .unwrap();
}
fn direct(number: &str) -> Input {
    Input::External {
        request: ExternalToolRequest::Web(WebToolRequest::Parcel {
            carrier: "japan_post".into(),
            tracking_number: Some(number.into()),
        }),
    }
}
async fn example(
    s: &Store,
    u: &UserContext,
    number: &str,
    state: &str,
    final_answer: Option<String>,
) -> String {
    let (thread, claim, step) = start(
        s,
        u,
        &format!("Japan Post {number} delivery status"),
        direct(number),
    )
    .await;
    let crate::ExternalToolStart::Dispatch(permit) = s
        .prepare_external_tool(&step, "parcel".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    s.complete_external_tool(&permit, observed(number, state))
        .await
        .unwrap();
    if let Some(text) = final_answer {
        finish(s, &claim, &text, Some(MessagePhase::FinalAnswer)).await;
    } else {
        s.finish_agent_execution(&claim.lease, RunState::Cancelled, None)
            .await
            .unwrap();
    }
    thread
}
#[tokio::test]
async fn learned_parcel_requires_two_independent_final_observations_not_tool_flags() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    for state in ["not_found", "provider_changed"] {
        example(&s, &u, A, state, Some(answer(A))).await;
    }
    example(&s, &u, A, "observed", Some("Completed".into())).await;
    example(&s, &u, A, "observed", None).await;
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
    let first = example(&s, &u, A, "observed", Some(answer(A))).await;
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    assert_eq!(card.readiness, "candidate");
    assert_eq!(
        card.version,
        moosuri_core::BUILTIN_TOOLSET_VERSION.to_string()
    );
    example(&s, &u, A, "observed", Some(answer(A))).await;
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    let second = example(&s, &u, B, "observed", Some(answer(B))).await;
    assert_eq!(s.learned_skills(&u).await.unwrap()[0].readiness, "ready");
    let other = member(&s, &u, "learned-parcel-other@example.test")
        .await
        .user;
    assert!(s.learned_skills(&other).await.unwrap().is_empty());
    s.delete_thread(&u, first).await.unwrap();
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    s.delete_thread(&u, second).await.unwrap();
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
}
#[tokio::test]
async fn learned_parcel_fresh_dispatch_private_receipt_budget_export_and_revocation() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let proof_thread = example(&s, &u, A, "observed", Some(answer(A))).await;
    example(&s, &u, B, "observed", Some(answer(B))).await;
    let pin = super::super::pin(PARCEL);
    let input = Input::LearnedSkill {
        invocation: LearnedSkillInvocation {
            pin: pin.clone(),
            arguments: LearnedSkillArguments::Parcel {
                tracking_number: C.into(),
            },
        },
    };
    let (thread, claim, step) =
        start(&s, &u, &format!("Japan Post {C} delivery status"), input).await;
    let crate::ExternalToolStart::Dispatch(permit) = s
        .prepare_external_tool(&step, "parcel".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    assert!(
        matches!(permit.request(),ExternalToolRequest::Web(WebToolRequest::Parcel{tracking_number:Some(n),..}) if n==C)
    );
    let result = s
        .complete_external_tool(&permit, observed(C, "observed"))
        .await
        .unwrap();
    assert!(matches!(result,Output::External(ref v) if v["parcel"]["tracking_number"]==C));
    assert!(matches!(
        s.prepare_external_tool(&step, "parcel".into())
            .await
            .unwrap(),
        crate::ExternalToolStart::Complete(_)
    ));
    let run = claim.lease.run_id().to_owned();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM learned_skill_receipts WHERE run_id=?1",
                [&run],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        assert_eq!(
            db.conn.query_row(
                "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                [&run],
                |r| r.get::<_, i64>(0)
            )?,
            1
        );
        Ok(())
    })
    .await
    .unwrap();
    finish(&s, &claim, &answer(C), Some(MessagePhase::FinalAnswer)).await;
    assert_eq!(
        s.conversation(&u, thread.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .run
            .web_evidence
            .as_ref()
            .unwrap()
            .len(),
        1
    );
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    let preview = s
        .preview_learned_skill_share(
            &u,
            card.id.clone(),
            SkillShareInput {
                expected_revision: card.revision,
            },
        )
        .await
        .unwrap();
    let files = serde_json::to_string(&preview.files).unwrap();
    for canary in [
        A,
        B,
        C,
        "PRIVATE_STATUS_CANARY",
        "PRIVATE_OFFICE_CANARY",
        "PRIVATE_PARCEL_TITLE_CANARY",
        &u.account.id,
        &u.tenant.0,
        &thread,
        claim.lease.run_id(),
    ] {
        assert!(!files.contains(canary), "{canary}");
    }
    assert!(files.contains("parcel_track"));
    assert!(files.contains("AA000000000JP"));
    crate::community_skills::author::check(&preview.slug, preview.files).unwrap();
    // Removing an original proof demotes the route and purges this actually used
    // derivative, not merely its discovery pin.
    s.delete_thread(&u, proof_thread).await.unwrap();
    assert!(
        s.conversation(&u, thread.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .assistant
            .is_empty()
    );
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    assert_eq!(card.readiness, "candidate");
    s.mutate_learned_skill(
        &u,
        card.id,
        CommunitySkillMutation {
            action: "remove".into(),
            expected_revision: card.revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    assert!(
        s.conversation(&u, thread, None).await.unwrap().turns[0]
            .assistant
            .is_empty()
    );
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
}
#[tokio::test]
async fn learned_parcel_rejects_missing_ambiguous_old_or_foreign_number_before_io() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    example(&s, &u, A, "observed", Some(answer(A))).await;
    example(&s, &u, B, "observed", Some(answer(B))).await;
    let pin = super::super::pin(PARCEL);
    let other = member(&s, &u, "parcel-isolation@example.test").await.user;
    // Another account cannot use the original owner's private pin, even with a
    // fresh explicit number and normal public-read permissions.
    let other_thread = s
        .create_thread(&other, "Other owner".into())
        .await
        .unwrap()
        .id;
    s.submit_turn(
        &other,
        other_thread,
        Uuid::new_v4().to_string(),
        format!("Japan Post {C} status"),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let context = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "parcel".into(),
            input: Input::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Parcel {
                        tracking_number: C.into(),
                    },
                },
            },
        }],
    )
    .await
    .unwrap();
    assert!(matches!(
        s.prepare_external_tool(&step, "parcel".into())
            .await
            .unwrap(),
        crate::ExternalToolStart::Complete(Output::Error(ToolReadError::Unavailable))
    ));
    s.finish_agent_execution(&claim.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    for (text, number) in [
        ("Check that parcel", A),
        ("Japan Post shipping status", A),
        ("Japan Post CC123456789JP status", A),
        ("Japan Post CC123456789JP DD123456789JP status", C),
    ] {
        let (_, claim, step) = start(
            &s,
            &u,
            text,
            Input::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Parcel {
                        tracking_number: number.into(),
                    },
                },
            },
        )
        .await;
        assert!(matches!(
            s.prepare_external_tool(&step, "parcel".into())
                .await
                .unwrap(),
            crate::ExternalToolStart::Complete(Output::Error(
                ToolReadError::InvalidInput | ToolReadError::Unavailable
            ))
        ));
        s.finish_agent_execution(&claim.lease, RunState::Cancelled, None)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn learned_parcel_commentary_or_unbacked_success_result_cannot_become_proof() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let (_, claim, step) = start(&s, &u, &format!("Japan Post {A} status"), direct(A)).await;
    let crate::ExternalToolStart::Dispatch(permit) = s
        .prepare_external_tool(&step, "parcel".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    s.complete_external_tool(&permit, observed(A, "observed"))
        .await
        .unwrap();
    finish(&s, &claim, &answer(A), Some(MessagePhase::Commentary)).await;
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
    let (_, early, step) = start(&s, &u, &format!("Japan Post {A} status"), direct(A)).await;
    let crate::ExternalToolStart::Dispatch(permit) = s
        .prepare_external_tool(&step, "parcel".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    s.complete_external_tool(&permit, observed(A, "observed"))
        .await
        .unwrap();
    s.append_agent_output(
        &step,
        vec![crate::OutputPatch::Append {
            item: format!("step-{}-early", step.step()),
            text: answer(A),
            phase: None,
            refusal: false,
        }],
    )
    .await
    .unwrap();
    finish(&s, &early, "Completed", Some(MessagePhase::FinalAnswer)).await;
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
    let (_, claim, _step) = start(&s, &u, &format!("Japan Post {B} status"), direct(B)).await;
    let tenant = u.tenant.0.clone();
    let run = claim.lease.run_id().to_owned();
    s.call(move|db|{
        let r=crate::chat::load_run(&db.conn,&tenant,&run)?;
        let Output::External(mut value)=observed(B,"observed") else{unreachable!()};
        value["parcel"]["provenance"]=json!({"carrier":"japan_post","tracking_number":B,"current_request_id":r.request,"number_source_request_id":r.request,"number_source_run_id":r.id,"used_previous_number":false});
        db.conn.execute("UPDATE agent_tool_slots SET result=?3 WHERE tenant_id=?1 AND run_id=?2",params![tenant,run,encode(&Output::External(value))?])?;Ok(())
    }).await.unwrap();
    finish(&s, &claim, &answer(B), Some(MessagePhase::FinalAnswer)).await;
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
}

#[tokio::test]
async fn learned_parcel_same_thread_history_is_not_fresh_authority_and_late_result_stays_revoked() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let thread = example(&s, &u, A, "observed", Some(answer(A))).await;
    example(&s, &u, B, "observed", Some(answer(B))).await;
    let pin = super::super::pin(PARCEL);
    s.submit_turn(
        &u,
        thread,
        Uuid::new_v4().to_string(),
        "Check that parcel again".into(),
    )
    .await
    .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let step = s
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "parcel".into(),
            input: Input::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Parcel {
                        tracking_number: A.into(),
                    },
                },
            },
        }],
    )
    .await
    .unwrap();
    assert!(matches!(
        s.prepare_external_tool(&step, "parcel".into())
            .await
            .unwrap(),
        crate::ExternalToolStart::Complete(Output::Error(ToolReadError::InvalidInput))
    ));
    s.finish_agent_execution(&claim.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    let (_, claim, step) = start(
        &s,
        &u,
        &format!("Japan Post {C} status"),
        Input::LearnedSkill {
            invocation: LearnedSkillInvocation {
                pin,
                arguments: LearnedSkillArguments::Parcel {
                    tracking_number: C.into(),
                },
            },
        },
    )
    .await;
    let crate::ExternalToolStart::Dispatch(permit) = s
        .prepare_external_tool(&step, "parcel".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    s.mutate_learned_skill(
        &u,
        card.id,
        CommunitySkillMutation {
            action: "remove".into(),
            expected_revision: card.revision,
            expected_digest: None,
        },
    )
    .await
    .unwrap();
    assert!(
        s.complete_external_tool(&permit, observed(C, "observed"))
            .await
            .is_err()
    );
    let run = claim.lease.run_id().to_owned();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM learned_skill_receipts WHERE run_id=?1",
                [&run],
                |r| r.get::<_, i64>(0)
            )?,
            0
        );
        Ok(())
    })
    .await
    .unwrap();
    example(&s, &u, C, "observed", Some(answer(C))).await;
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
}
