//! Actual synthetic Store outcomes. No provider/model or external URL is contacted.
use super::*;
use crate::{
    agent_execution::{AgentClaim, PendingKnowledgeCall, PrivateCheckpoint},
    tests::{TEST_LOCK, admin, directory, member},
};
use moosuri_core::*;
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
            ProviderChange {
                name: "Synthetic pages".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:9/v1".into()),
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
    (dir, s, u)
}
fn urls(prefix: &str) -> Vec<String> {
    vec![
        format!("https://example.com/{prefix}-one"),
        format!("https://example.org/{prefix}-two"),
    ]
}
fn text(urls: &[String]) -> String {
    format!(
        "Collect and compare these public pages: {}",
        urls.iter()
            .map(|u| format!("`{u}`"))
            .collect::<Vec<_>>()
            .join(" ")
    )
}
fn capsule() -> PrivateCheckpoint {
    PrivateCheckpoint {
        bytes: br#"{"v":1,"synthetic":"page collection"}"#.to_vec(),
    }
}
async fn admission(s: &Store, u: &UserContext, urls: &[String]) -> (String, AgentClaim) {
    let t = s
        .create_thread(u, "Synthetic collection".into())
        .await
        .unwrap()
        .id;
    s.submit_turn(u, t.clone(), Uuid::new_v4().to_string(), text(urls))
        .await
        .unwrap();
    let c = s.claim_agent_run().await.unwrap().unwrap();
    (t, c)
}
async fn slot(s: &Store, c: &AgentClaim, input: Input) -> StepLease {
    let ctx = s.agent_context(&c.lease).await.unwrap();
    let step = s
        .begin_agent_step(&c.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        capsule(),
        vec![PendingKnowledgeCall {
            call_id: "collect".into(),
            input,
        }],
    )
    .await
    .unwrap();
    step
}
fn page(url: &str) -> Output {
    Output::External(
        json!({"web_page":{"url":url,"title":format!("Observed {url}"),"text":format!("FRESH_CURRENT_PAGE {url}"),"retrieved_at_unix_ms":12345,"truncated":false,"warning":"http_only"}}),
    )
}
async fn collect(s: &Store, step: &StepLease, urls: &[String]) -> (PageCollectionPermit, Output) {
    let PageCollectionStart::Dispatch(p) = s
        .prepare_page_collection(step, "collect".into())
        .await
        .unwrap()
    else {
        panic!("no dispatch")
    };
    for (i, url) in urls.iter().enumerate() {
        let read = s.begin_collection_page(&p, i).await.unwrap();
        assert_eq!(read.url(), url);
        let a = s.complete_collection_page(&read, page(url)).await.unwrap();
        let b = s.complete_collection_page(&read, page(url)).await.unwrap();
        assert_eq!(encode(&a).unwrap(), encode(&b).unwrap());
        assert!(s.begin_collection_page(&p, i).await.is_err());
    }
    let result = s.finish_page_collection(&p).await.unwrap();
    assert_eq!(
        encode(&result).unwrap(),
        encode(&s.finish_page_collection(&p).await.unwrap()).unwrap()
    );
    (p, result)
}
async fn terminal(s: &Store, c: &AgentClaim, value: &Output) {
    let Output::External(v) = value else { panic!() };
    let answer = v["public_pages"]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| {
            let p = &v["result"]["web_page"];
            format!(
                "{} {} {}",
                p["url"].as_str().unwrap(),
                p["title"].as_str().unwrap(),
                p["text"].as_str().unwrap()
            )
        })
        .collect::<Vec<_>>()
        .join("\n");
    let ctx = s.agent_context(&c.lease).await.unwrap();
    let step = s
        .begin_agent_step(&c.lease, ctx.checkpoint_seq)
        .await
        .unwrap();
    s.append_agent_output(
        &step,
        vec![crate::OutputPatch::Append {
            item: format!("step-{}-final", step.step()),
            text: answer,
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.commit_agent_step(&step, capsule(), vec![]).await.unwrap();
    s.finish_agent_run(&step, RunState::Completed, None)
        .await
        .unwrap();
}
async fn train(s: &Store, u: &UserContext, prefix: &str) -> String {
    train_urls(s, u, urls(prefix)).await
}
async fn train_urls(s: &Store, u: &UserContext, urls: Vec<String>) -> String {
    let (t, c) = admission(s, u, &urls).await;
    let step = slot(s, &c, Input::CollectPages { urls: urls.clone() }).await;
    let (_, result) = collect(s, &step, &urls).await;
    terminal(s, &c, &result).await;
    t
}
#[tokio::test]
async fn public_page_collection_two_actual_proofs_fresh_foreground_and_mission_reuse_share_then_withdraw()
 {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let a = train(&s, &u, "PRIVATE-A-CANARY").await;
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    train(&s, &u, "PRIVATE-A-CANARY").await; // same canonical input is not independent
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    let b = train(&s, &u, "PRIVATE-B-CANARY").await;
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    assert_eq!(card.readiness, "ready");
    let pin = super::super::pin(&route(2));
    let shared = s
        .preview_learned_skill_share(
            &u,
            card.id.clone(),
            SkillShareInput {
                expected_revision: card.revision,
            },
        )
        .await
        .unwrap();
    let package = encode(&shared.files).unwrap();
    assert!(
        !package.contains("PRIVATE-A")
            && !package.contains("PRIVATE-B")
            && !package.contains("FRESH_CURRENT_PAGE")
    );
    assert!(package.contains("synthetic-option"));
    let fresh = urls("fresh-C");
    let (t, c) = admission(&s, &u, &fresh).await;
    let step = slot(
        &s,
        &c,
        Input::LearnedSkill {
            invocation: LearnedSkillInvocation {
                pin: pin.clone(),
                arguments: LearnedSkillArguments::Pages {
                    urls: fresh.clone(),
                },
            },
        },
    )
    .await;
    let (_, result) = collect(&s, &step, &fresh).await;
    terminal(&s, &c, &result).await;
    let page = s.conversation(&u, t.clone(), None).await.unwrap();
    assert_eq!(page.turns[0].run.web_evidence.as_ref().unwrap().len(), 2);
    let tenant = u.tenant.0.clone();
    let run = c.lease.run_id().to_owned();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
                params![tenant, run],
                |r| r.get::<_, i64>(0)
            )?,
            2
        );
        Ok(())
    })
    .await
    .unwrap();
    // The same route is available only under a new explicit accepted owner mission.
    let mission_urls = urls("fresh-mission");
    s.create_mission(
        &u,
        MissionCreate {
            request_id: Uuid::new_v4().to_string(),
            task: text(&mission_urls),
            constraints: String::new(),
            checks: vec![MissionCheck::UserReview {
                description: "Review evidence and comparison".into(),
            }],
            execution_mode: MissionMode::Durable,
        },
    )
    .await
    .unwrap();
    let mission = s.claim_agent_run().await.unwrap().unwrap();
    assert!(mission.tool_profile.as_ref().unwrap().skills.contains(&pin));
    let step = slot(
        &s,
        &mission,
        Input::LearnedSkill {
            invocation: LearnedSkillInvocation {
                pin: pin.clone(),
                arguments: LearnedSkillArguments::Pages {
                    urls: mission_urls.clone(),
                },
            },
        },
    )
    .await;
    let (_, observed) = collect(&s, &step, &mission_urls).await;
    assert!(encode(&observed).unwrap().contains("fresh-mission"));
    let other = member(&s, &u, "other-pages@example.test").await.user;
    assert!(s.learned_skills(&other).await.unwrap().is_empty());
    // Removing one training conversation demotes the recipe and withdraws actual uses.
    s.delete_thread(&u, b).await.unwrap();
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    assert!(
        s.conversation(&u, t, None).await.unwrap().turns[0]
            .assistant
            .is_empty()
    );
    assert!(s.check_run(&mission.lease).await.is_err());
    s.delete_thread(&u, a).await.unwrap();
}
#[tokio::test]
async fn public_page_collection_fresh_targets_partial_unknown_restart_and_cancel_do_not_repeat_io()
{
    let _g = TEST_LOCK.lock().await;
    let (dir, s, u) = setup().await;
    let current = urls("current");
    let (_, c) = admission(&s, &u, &current).await;
    let wrong = slot(&s, &c, Input::CollectPages { urls: urls("old") }).await;
    assert!(matches!(
        s.prepare_page_collection(&wrong, "collect".into())
            .await
            .unwrap(),
        PageCollectionStart::Complete(Output::Error(ToolReadError::InvalidInput))
    ));
    // Failed page remains visible as a distinct entry, with no adopted proof.
    let step = slot(
        &s,
        &c,
        Input::CollectPages {
            urls: current.clone(),
        },
    )
    .await;
    let PageCollectionStart::Dispatch(p) = s
        .prepare_page_collection(&step, "collect".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    let first = s.begin_collection_page(&p, 0).await.unwrap();
    s.complete_collection_page(&first, page(&current[0]))
        .await
        .unwrap();
    let second = s.begin_collection_page(&p, 1).await.unwrap();
    s.complete_collection_page(&second, Output::Error(ToolReadError::Unavailable))
        .await
        .unwrap();
    let partial = s.finish_page_collection(&p).await.unwrap();
    let Output::External(v) = partial else {
        panic!()
    };
    assert_eq!(v["public_pages"]["all_reads_observed"], false);
    assert_eq!(
        v["public_pages"]["entries"][1]["result"]["error"],
        "unavailable"
    );
    s.finish_agent_execution(
        &c.lease,
        RunState::Failed,
        Some(crate::RunFailure::ProtocolFailed),
    )
    .await
    .unwrap();
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
    let (_, held) = admission(&s, &u, &current).await;
    let step = slot(
        &s,
        &held,
        Input::CollectPages {
            urls: current.clone(),
        },
    )
    .await;
    let PageCollectionStart::Dispatch(p) = s
        .prepare_page_collection(&step, "collect".into())
        .await
        .unwrap()
    else {
        panic!()
    };
    let page = s.begin_collection_page(&p, 0).await.unwrap();
    // A lost dispatch acknowledgement permanently settles the parent as unknown.
    assert!(matches!(
        s.prepare_page_collection(&step, "collect".into())
            .await
            .unwrap(),
        PageCollectionStart::Complete(Output::Error(ToolReadError::OutcomeUnknown))
    ));
    assert!(
        s.complete_collection_page(&page, page_result(&current[0]))
            .await
            .is_err()
    );
    s.cancel_run(&u, held.lease.run_id().into(), 1)
        .await
        .unwrap();
    assert!(s.begin_collection_page(&p, 1).await.is_err());
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    assert!(s.claim_agent_run().await.unwrap().is_none());
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
}
fn page_result(url: &str) -> Output {
    page(url)
}
#[test]
fn public_page_collection_literal_targets_cannot_expand_or_normalize_into_duplicate_authority() {
    let a = urls("one");
    assert!(targets(&text(&a), &a));
    assert!(!targets(&text(&a), &[a[1].clone(), a[0].clone()]));
    for invalid in [
        vec![a[0].clone(), a[0].clone()],
        vec!["https://name:password@example.com/a".into(), a[1].clone()],
        vec!["file:///private".into(), a[1].clone()],
        vec![
            "https://example.com/a#one".into(),
            "https://example.com/a#two".into(),
        ],
    ] {
        assert!(!targets(&text(&invalid), &invalid));
    }
    assert!(!targets(
        &format!("{} https://example.net/extra", text(&a)),
        &a
    ));
}

#[tokio::test]
async fn public_page_collection_equivalent_url_spellings_are_not_independent_training_proofs() {
    let _g = TEST_LOCK.lock().await;
    let (_dir, s, u) = setup().await;
    let original = urls("same-target-A");
    train_urls(&s, &u, original.clone()).await;
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    // Different literal current inputs still address exactly the original targets.
    let equivalent: Vec<String> = vec![
        "https://EXAMPLE.COM:443/intermediate/../same-target-A-one".into(),
        "https://EXAMPLE.ORG:443/another/../same-target-A-two".into(),
    ];
    for (a, b) in original.iter().zip(&equivalent) {
        assert_ne!(a, b);
        assert_eq!(
            moosuri_connectors::web::validate_public_url(a).unwrap(),
            moosuri_connectors::web::validate_public_url(b).unwrap()
        );
    }
    train_urls(&s, &u, equivalent).await;
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    train_urls(&s, &u, urls("distinct-target-C")).await;
    assert_eq!(s.learned_skills(&u).await.unwrap()[0].readiness, "ready");
}
