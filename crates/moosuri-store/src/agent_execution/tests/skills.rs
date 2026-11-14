use super::*;

async fn enabled(store: &Store) -> UserContext {
    let user = configured(store).await;
    let provider = store
        .list_providers(&user)
        .await
        .unwrap()
        .providers
        .remove(0);
    store
        .record_tool_probe(
            &user,
            provider.id,
            provider.revision,
            "synthetic".into(),
            None,
        )
        .await
        .unwrap();
    user
}

fn skill_call(id: &str, pin: SkillPin) -> PendingKnowledgeCall {
    PendingKnowledgeCall {
        call_id: id.into(),
        input: KnowledgeToolInput::SkillRead { pin },
    }
}
fn math_call(id: &str) -> PendingKnowledgeCall {
    PendingKnowledgeCall {
        call_id: id.into(),
        input: KnowledgeToolInput::DecimalCalculate {
            calculation: DecimalCalculation {
                operation: DecimalOperation::Sum,
                operands: vec!["0.1".into(), "0.2".into()],
            },
        },
    }
}

#[tokio::test]
async fn profile_and_skill_receipts_survive_retry_without_upgrading_legacy_runs() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    let thread = store
        .create_thread(&user, "Recipe retry".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let profile = claim.tool_profile.clone().unwrap();
    let pin = profile.skills[0].clone();
    let mut wrong = pin.clone();
    wrong.digest = "0".repeat(64);
    let mut missing = pin.clone();
    missing.id = "not-bundled".into();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            capsule("private recipe capsule"),
            vec![
                skill_call("valid", pin.clone()),
                skill_call("wrong", wrong),
                skill_call("missing", missing),
                math_call("math"),
            ],
        )
        .await
        .unwrap();
    for _ in 0..2 {
        match store
            .execute_knowledge_tool(&step, "valid".into())
            .await
            .unwrap()
        {
            KnowledgeToolResult::Skill(skill) => {
                assert!(skill.pin == pin);
                assert!(skill.instructions.contains("source-briefing"));
            }
            _ => panic!("pinned recipe not delivered"),
        }
    }
    for id in ["wrong", "missing"] {
        assert!(matches!(
            store
                .execute_knowledge_tool(&step, id.into())
                .await
                .unwrap(),
            KnowledgeToolResult::Error(ToolReadError::Unavailable)
        ));
    }
    assert!(
        matches!(store.execute_knowledge_tool(&step, "math".into()).await.unwrap(), KnowledgeToolResult::Decimal { exact } if exact == "0.3")
    );
    let run_id = claim.lease.run_id().to_owned();
    let lookup = run_id.clone();
    assert_eq!(
        store
            .call(move |db| Ok(db.conn.query_row(
                "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                [lookup],
                |r| r.get::<_, i64>(0)
            )?))
            .await
            .unwrap(),
        4
    );
    store
        .finish_agent_execution(
            &claim.lease,
            RunState::Failed,
            Some(RunFailure::ConnectionFailed),
        )
        .await
        .unwrap();
    store.retry_run(&user, run_id.clone(), 1).await.unwrap();
    let retry = store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(
        encoded(&retry.tool_profile).unwrap(),
        encoded(&Some(profile)).unwrap()
    );
    assert!(!retry.context.memory_planning_allowed);
    assert!(retry.context.calls.is_empty());
    let step = tool_step(&store, &retry, skill_call("again", pin)).await;
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "again".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Skill(_)
    ));
    store
        .finish_agent_execution(
            &retry.lease,
            RunState::Failed,
            Some(RunFailure::ConnectionFailed),
        )
        .await
        .unwrap();

    // Old persisted ModelChoice JSON has no profile. It must not gain the new
    // vocabulary merely because a later binary has bundled recipes.
    let legacy_thread = store.create_thread(&user, "Legacy".into()).await.unwrap();
    let admitted = store
        .submit_turn(
            &user,
            legacy_thread.id,
            Uuid::new_v4().to_string(),
            "legacy request".into(),
        )
        .await
        .unwrap();
    let legacy_id = admitted.run.id;
    store
        .call(move |db| {
            db.conn.execute(
                "UPDATE chat_runs SET model=json_remove(model,'$.agent_profile') WHERE id=?1",
                [legacy_id],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    let legacy = store.claim_agent_run().await.unwrap().unwrap();
    assert!(legacy.tool_profile.is_none());
    let step = tool_step(&store, &legacy, math_call("unavailable")).await;
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "unavailable".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::Unavailable)
    ));
    store
        .finish_agent_execution(
            &legacy.lease,
            RunState::Failed,
            Some(RunFailure::ConnectionFailed),
        )
        .await
        .unwrap();

    // A once-admitted digest whose bytes are absent from this binary must not
    // silently resolve to the current recipe with the same name and revision.
    let old_thread = store
        .create_thread(&user, "Old recipe bytes".into())
        .await
        .unwrap();
    let old = store
        .submit_turn(
            &user,
            old_thread.id,
            Uuid::new_v4().to_string(),
            "old revision request".into(),
        )
        .await
        .unwrap();
    let old_id = old.run.id;
    store.call(move |db| { db.conn.execute("UPDATE chat_runs SET model=json_set(model,'$.agent_profile.skills[0].digest',?2) WHERE id=?1", params![old_id, "f".repeat(64)])?; Ok(()) }).await.unwrap();
    let old = store.claim_agent_run().await.unwrap().unwrap();
    let old_pin = old.tool_profile.as_ref().unwrap().skills[0].clone();
    let step = tool_step(&store, &old, skill_call("old-bytes", old_pin)).await;
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "old-bytes".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::Unavailable)
    ));
}

#[tokio::test]
async fn exact_source_authority_and_deletion_cover_recipe_math_and_final_payloads() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    let other = member(&store, &user, "recipe-other@example.test")
        .await
        .user;
    let (_, own) = source(&store, &user, "합성 견적A 12.50 배송비2.50").await;
    let (_, foreign) = source(&store, &other, "foreign-price-canary").await;
    let thread = store
        .create_thread(&user, "Comparison".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let mut stale = read(&own, 0, 4096);
    stale.call_id = "stale".into();
    if let KnowledgeToolInput::ReadSource { revision, .. } = &mut stale.input {
        *revision = 2;
    }
    let mut foreign = read(&foreign, 0, 4096);
    foreign.call_id = "foreign".into();
    let mut missing = read(&Uuid::new_v4().to_string(), 0, 4096);
    missing.call_id = "missing".into();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            capsule("pinned recipe and evidence"),
            vec![
                skill_call(
                    "recipe",
                    claim.tool_profile.as_ref().unwrap().skills[2].clone(),
                ),
                stale,
                foreign,
                missing,
                read(&own, 0, 4096),
                math_call("total"),
            ],
        )
        .await
        .unwrap();
    for id in ["recipe", "stale", "foreign", "missing", "read-1", "total"] {
        let result = store
            .execute_knowledge_tool(&step, id.into())
            .await
            .unwrap();
        if ["stale", "foreign", "missing"].contains(&id) {
            assert!(matches!(
                result,
                KnowledgeToolResult::Error(ToolReadError::NotFound)
            ));
        }
    }
    finish(&store, &claim, "derived-math-answer-canary 0.3").await;
    store.delete_source(&user, own, 1).await.unwrap();
    let page = store.conversation(&user, thread.id, None).await.unwrap();
    assert!(page.turns[0].assistant.is_empty());
    assert!(page.turns[0].run.evidence.is_empty());
    let run = claim.lease.run_id().to_owned();
    store
        .call(move |db| {
            for sql in [
                "SELECT count(*) FROM agent_tool_slots WHERE run_id=?1",
                "SELECT count(*) FROM agent_steps WHERE run_id=?1 AND checkpoint IS NOT NULL",
                "SELECT count(*) FROM chat_output WHERE run_id=?1",
            ] {
                assert_eq!(db.conn.query_row(sql, [&run], |r| r.get::<_, i64>(0))?, 0);
            }
            // Pins contain only public identity/digest and may remain after purge.
            let stored: String =
                db.conn
                    .query_row("SELECT model FROM chat_runs WHERE id=?1", [run], |r| {
                        r.get(0)
                    })?;
            assert!(!stored.contains("derived-math-answer-canary"));
            assert!(stored.contains("agent_profile"));
            Ok(())
        })
        .await
        .unwrap();
}

#[tokio::test]
async fn community_runs_pin_installed_bytes_updates_do_not_expand_tools_and_removal_revokes_late_work()
 {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    let card = store.community_skills(&user).await.unwrap().remove(0);
    store
        .mutate_community_skill(
            &user,
            card.id.clone(),
            CommunitySkillMutation {
                action: "install".into(),
                expected_revision: 0,
                expected_digest: Some(card.digest.clone()),
            },
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Pinned community".into())
        .await
        .unwrap();
    let old = claimed(&store, &user, &thread.id).await;
    let original = old.tool_profile.as_ref().unwrap();
    assert_eq!(original.skills.len(), BUNDLED_SKILLS.len());
    assert!(original.skills.iter().all(|p| p.id != "source-briefing"));
    let pin = original
        .skills
        .iter()
        .find(|p| p.id.starts_with("community:"))
        .unwrap()
        .clone();
    let step = tool_step(&store, &old, skill_call("community", pin.clone())).await;
    let owner = user.clone();
    store
        .call(move |db| crate::community_skills::tests::activate_v2(db, owner))
        .await
        .unwrap();
    match store
        .execute_knowledge_tool(&step, "community".into())
        .await
        .unwrap()
    {
        KnowledgeToolResult::Skill(doc) => {
            assert!(doc.pin == pin);
            assert!(!doc.instructions.contains("Synthetic v2 guidance"));
        }
        _ => panic!("original installed revision unavailable"),
    }
    let second_thread = store
        .create_thread(&user, "Updated community".into())
        .await
        .unwrap();
    let new = store
        .submit_turn(
            &user,
            second_thread.id,
            Uuid::new_v4().to_string(),
            "Synthetic updated request".into(),
        )
        .await
        .unwrap();
    let owner = user.clone();
    let run_id = new.run.id.clone();
    let updated = store
        .call(move |db| {
            Ok(chat::load_run(&db.conn, &owner.tenant.0, &run_id)?
                .tool_profile
                .unwrap())
        })
        .await
        .unwrap();
    let changed = updated
        .skills
        .iter()
        .find(|p| p.id.starts_with("community:"))
        .unwrap();
    assert_ne!(changed.digest, pin.digest);
    assert_eq!(updated.version, original.version);
    assert!(original.web.is_none() && updated.web.is_none());
    assert!(
        !original.community_metadata[0]
            .description
            .contains("Synthetic updated")
    );
    assert!(
        updated.community_metadata[0]
            .description
            .contains("Synthetic updated")
    );
    assert_eq!(
        updated.mail_connections.len(),
        original.mail_connections.len()
    );
    let current = store.community_skills(&user).await.unwrap().remove(0);
    store
        .mutate_community_skill(
            &user,
            card.id.clone(),
            CommunitySkillMutation {
                action: "disable".into(),
                expected_revision: current.revision,
                expected_digest: None,
            },
        )
        .await
        .unwrap();
    let third_thread = store
        .create_thread(&user, "Disabled community".into())
        .await
        .unwrap();
    let fresh_turn = store
        .submit_turn(
            &user,
            third_thread.id,
            Uuid::new_v4().to_string(),
            "Synthetic disabled request".into(),
        )
        .await
        .unwrap();
    let owner = user.clone();
    let run_id = fresh_turn.run.id.clone();
    let fresh_profile = store
        .call(move |db| {
            Ok(chat::load_run(&db.conn, &owner.tenant.0, &run_id)?
                .tool_profile
                .unwrap())
        })
        .await
        .unwrap();
    assert!(
        fresh_profile
            .skills
            .iter()
            .all(|p| !p.id.starts_with("community:"))
    );
    assert!(
        fresh_profile
            .skills
            .iter()
            .all(|p| p.id != "source-briefing")
    );
    let context_before_remove = store.agent_context(&old.lease).await.unwrap();
    let late_step = store
        .begin_agent_step(&old.lease, context_before_remove.checkpoint_seq)
        .await
        .unwrap();
    let current = store.community_skills(&user).await.unwrap().remove(0);
    store
        .mutate_community_skill(
            &user,
            card.id.clone(),
            CommunitySkillMutation {
                action: "remove".into(),
                expected_revision: current.revision,
                expected_digest: None,
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .begin_agent_step(&old.lease, old.context.checkpoint_seq)
            .await
            .is_err()
    );
    assert!(
        store
            .append_agent_output(&late_step, patch("synthetic late community answer"))
            .await
            .is_err()
    );
    let owner = user.clone();
    let run = old.lease.run_id().to_owned();
    store
        .call(move |db| {
            assert!(!retry_allowed(&db.conn, &owner.tenant.0, &run)?);
            Ok(())
        })
        .await
        .unwrap();
    let fresh = store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(fresh.lease.run_id(), fresh_turn.run.id);
    assert!(
        store
            .begin_agent_step(&fresh.lease, fresh.context.checkpoint_seq)
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn external_source_pins_survive_updates_and_remove_revokes_held_output_without_disabling_bundled_tools()
 {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    let owner = user.clone();
    let id = store
        .call(move |db| crate::community_skills::external::tests::activate_fixture(db, owner, 1))
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Synthetic external run".into())
        .await
        .unwrap();
    let old = claimed(&store, &user, &thread.id).await;
    let profile = old.tool_profile.as_ref().unwrap();
    assert_eq!(profile.skills.len(), BUNDLED_SKILLS.len() + 1);
    let pin = profile
        .skills
        .iter()
        .find(|p| p.id.starts_with("community:github-"))
        .unwrap()
        .clone();
    let step = tool_step(&store, &old, skill_call("external", pin.clone())).await;
    let owner = user.clone();
    store
        .call(move |db| crate::community_skills::external::tests::activate_fixture(db, owner, 2))
        .await
        .unwrap();
    match store
        .execute_knowledge_tool(&step, "external".into())
        .await
        .unwrap()
    {
        KnowledgeToolResult::Skill(document) => {
            assert!(document.pin == pin);
            assert!(
                !document
                    .instructions
                    .contains("Synthetic external version two")
            );
        }
        _ => panic!("pinned external instructions were not read"),
    }
    let next_thread = store
        .create_thread(&user, "Synthetic updated external run".into())
        .await
        .unwrap();
    let queued = store
        .submit_turn(
            &user,
            next_thread.id,
            Uuid::new_v4().to_string(),
            "Synthetic external request".into(),
        )
        .await
        .unwrap();
    let owner = user.clone();
    let run_id = queued.run.id;
    let updated = store
        .call(move |db| {
            Ok(chat::load_run(&db.conn, &owner.tenant.0, &run_id)?
                .tool_profile
                .unwrap())
        })
        .await
        .unwrap();
    let new_pin = updated.skills.iter().find(|p| p.id == pin.id).unwrap();
    assert_ne!(new_pin.digest, pin.digest);
    assert_eq!(updated.version, profile.version);
    assert!(updated.web.is_none() && profile.web.is_none());
    let context = store.agent_context(&old.lease).await.unwrap();
    let late = store
        .begin_agent_step(&old.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .mutate_community_skill(
            &user,
            id,
            CommunitySkillMutation {
                action: "remove".into(),
                expected_revision: 2,
                expected_digest: None,
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .append_agent_output(&late, patch("Synthetic late external answer"))
            .await
            .is_err()
    );
    let owner = user.clone();
    store
        .call(move |db| {
            let mut fresh = crate::agent_execution::skills::profile();
            crate::community_skills::admit(&db.conn, &owner, &mut fresh)?;
            assert_eq!(fresh.skills.len(), BUNDLED_SKILLS.len());
            assert!(fresh.skills.iter().any(|p| p.id == "source-briefing"));
            Ok(())
        })
        .await
        .unwrap();
}

async fn learned_math_example(
    store: &Store,
    user: &UserContext,
    operands: &[&str],
    answer: Option<&str>,
) -> AgentClaim {
    let thread = store
        .create_thread(user, "Private exact route".into())
        .await
        .unwrap();
    let claim = claimed(store, user, &thread.id).await;
    let step = tool_step(
        store,
        &claim,
        PendingKnowledgeCall {
            call_id: "exact".into(),
            input: KnowledgeToolInput::DecimalCalculate {
                calculation: DecimalCalculation {
                    operation: DecimalOperation::Sum,
                    operands: operands.iter().map(|s| (*s).into()).collect(),
                },
            },
        },
    )
    .await;
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "exact".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Decimal { .. }
    ));
    if let Some(answer) = answer {
        finish(store, &claim, answer).await;
    }
    claim
}
#[tokio::test]
async fn learned_private_routes_require_independent_adopted_final_receipts_and_bind_fresh_c() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    learned_math_example(&store, &user, &["1", "2"], Some("3")).await;
    let cards = store.learned_skills(&user).await.unwrap();
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].readiness, "candidate");
    // Numeric formatting and commutative permutations are the same A input.
    learned_math_example(&store, &user, &["2.00", "01.0"], Some("3")).await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate"
    );
    // A successful tool unused by the final output is not an adopted proof.
    learned_math_example(&store, &user, &["4", "5"], Some("Completed")).await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate"
    );
    let pending = learned_math_example(&store, &user, &["6", "7"], None).await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate"
    );
    store
        .finish_agent_execution(&pending.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    let other = member(&store, &user, "learned-other@example.test")
        .await
        .user;
    assert!(store.learned_skills(&other).await.unwrap().is_empty());
    learned_math_example(&store, &user, &["4", "5"], Some("9")).await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "ready"
    );
    let thread = store.create_thread(&user, "Fresh C".into()).await.unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let pin = claim
        .tool_profile
        .as_ref()
        .unwrap()
        .skills
        .iter()
        .find(|p| p.id.starts_with("private:"))
        .unwrap()
        .clone();
    let step = tool_step(
        &store,
        &claim,
        PendingKnowledgeCall {
            call_id: "reuse".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Decimal {
                        operands: vec!["10.25".into(), "0.75".into()],
                    },
                },
            },
        },
    )
    .await;
    for _ in 0..2 {
        assert!(
            matches!(store.execute_knowledge_tool(&step,"reuse".into()).await.unwrap(),KnowledgeToolResult::Decimal{exact} if exact=="11")
        );
    }
    let run = claim.lease.run_id().to_owned();
    store
        .call(move |db| {
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
    // Removing the route stops a pinned run before its delayed output.
    let card = store.learned_skills(&user).await.unwrap().remove(0);
    store
        .mutate_learned_skill(
            &user,
            card.id,
            CommunitySkillMutation {
                action: "remove".into(),
                expected_revision: card.revision,
                expected_digest: None,
            },
        )
        .await
        .unwrap();
    assert!(store.append_agent_output(&step, patch("11")).await.is_err());
    assert!(store.learned_skills(&user).await.unwrap().is_empty());
    learned_math_example(&store, &user, &["8", "9"], Some("17")).await;
    assert!(store.learned_skills(&user).await.unwrap().is_empty());
}

async fn learned_source_example(
    store: &Store,
    user: &UserContext,
    text: &str,
) -> (AgentClaim, String) {
    let thread = store
        .create_thread(user, "Private lookup".into())
        .await
        .unwrap();
    let claim = claimed(store, user, &thread.id).await;
    let step = tool_step(
        store,
        &claim,
        PendingKnowledgeCall {
            call_id: "search".into(),
            input: KnowledgeToolInput::Search { query: query(text) },
        },
    )
    .await;
    let KnowledgeToolResult::Search(page) = store
        .execute_knowledge_tool(&step, "search".into())
        .await
        .unwrap()
    else {
        panic!("search receipt missing")
    };
    let hit = &page.hits[0];
    let id = hit.id.clone();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let step = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            capsule("read linked first hit"),
            vec![read(&id, 0, 4096)],
        )
        .await
        .unwrap();
    let KnowledgeToolResult::Source(excerpt) = store
        .execute_knowledge_tool(&step, "read-1".into())
        .await
        .unwrap()
    else {
        panic!("read receipt missing")
    };
    finish(
        store,
        &claim,
        &format!("{}\n{}", excerpt.source.title, excerpt.text),
    )
    .await;
    (claim, id)
}
#[tokio::test]
async fn learned_source_route_reads_fresh_ids_and_source_deletion_erases_proofs_and_derived_outputs()
 {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    source(&store, &user, "synthetic-alpha source A").await;
    source(&store, &user, "synthetic-beta source B").await;
    let (_, c) = source(&store, &user, "synthetic-gamma source C").await;
    let (_, a) = learned_source_example(&store, &user, "synthetic-alpha").await;
    learned_source_example(&store, &user, "source A").await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate",
        "another phrase for the same A source is not independent B"
    );
    let (b, _) = learned_source_example(&store, &user, "synthetic-beta").await;
    let card = store.learned_skills(&user).await.unwrap().remove(0);
    assert_eq!(card.readiness, "ready");
    let thread = store
        .create_thread(&user, "Fresh lookup C".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let pin = claim
        .tool_profile
        .as_ref()
        .unwrap()
        .skills
        .iter()
        .find(|p| p.id == card.id)
        .unwrap()
        .clone();
    let step = tool_step(
        &store,
        &claim,
        PendingKnowledgeCall {
            call_id: "reuse".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin,
                    arguments: LearnedSkillArguments::Source {
                        text: "synthetic-gamma".into(),
                    },
                },
            },
        },
    )
    .await;
    for _ in 0..2 {
        assert!(
            matches!(store.execute_knowledge_tool(&step,"reuse".into()).await.unwrap(),KnowledgeToolResult::Source(v) if v.source.id==c && v.text.contains("source C"))
        );
    }
    finish(&store, &claim, "Synthetic source\nsynthetic-gamma source C").await;
    let run = claim.lease.run_id().to_owned();
    let lookup = run.clone();
    store
        .call(move |db| {
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM learned_skill_receipts WHERE run_id=?1",
                    [&lookup],
                    |r| r.get::<_, i64>(0)
                )?,
                2
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT tool_calls FROM agent_execution WHERE run_id=?1",
                    [&lookup],
                    |r| r.get::<_, i64>(0)
                )?,
                2
            );
            Ok(())
        })
        .await
        .unwrap();
    // D discovers the same private pin but does not use it. Deleting training
    // source A must preserve D and its unrelated current source evidence.
    let (_, d) = source(&store, &user, "unrelated-D remains visible").await;
    let d_thread = store
        .create_thread(&user, "Unrelated D".into())
        .await
        .unwrap();
    let d_claim = claimed(&store, &user, &d_thread.id).await;
    let d_step = tool_step(&store, &d_claim, read(&d, 0, 4096)).await;
    store
        .execute_knowledge_tool(&d_step, "read-1".into())
        .await
        .unwrap();
    finish(&store, &d_claim, "unrelated-D remains visible").await;
    store.delete_source(&user, a, 1).await.unwrap();
    let d_page = store.conversation(&user, d_thread.id, None).await.unwrap();
    assert!(!d_page.turns[0].assistant.is_empty());
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "invalidated"
    );
    store
        .call(move |db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM learned_skill_proofs", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM learned_skill_receipts WHERE run_id=?1",
                    [&run],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            assert_eq!(
                db.conn.query_row(
                    "SELECT count(*) FROM chat_output WHERE run_id=?1",
                    [&run],
                    |r| r.get::<_, i64>(0)
                )?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    // Withdrawn finalized B cannot be reharvested as a new proof.
    let b_id = b.lease.run_id().to_owned();
    let tenant = user.tenant.0.clone();
    store
        .call(move |db| {
            let run = chat::load_run(&db.conn, &tenant, &b_id)?;
            crate::learned_skills::finalize(&db.conn, &run)
        })
        .await
        .unwrap();
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "invalidated"
    );
    source(&store, &user, "synthetic-delta fresh A-prime").await;
    source(&store, &user, "synthetic-epsilon fresh B-prime").await;
    learned_source_example(&store, &user, "synthetic-delta").await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate"
    );
    learned_source_example(&store, &user, "synthetic-epsilon").await;
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "ready"
    );
}

#[tokio::test]
async fn learned_schema_drift_and_attributable_failures_stop_reuse_but_infrastructure_does_not() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    learned_math_example(&store, &user, &["1", "2"], Some("3")).await;
    learned_math_example(&store, &user, &["4", "5"], Some("9")).await;
    let thread = store
        .create_thread(&user, "Infrastructure interruption".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let pin = claim
        .tool_profile
        .as_ref()
        .unwrap()
        .skills
        .iter()
        .find(|p| p.id.starts_with("private:"))
        .unwrap()
        .clone();
    let step = tool_step(
        &store,
        &claim,
        PendingKnowledgeCall {
            call_id: "reuse".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Decimal {
                        operands: vec!["7".into(), "8".into()],
                    },
                },
            },
        },
    )
    .await;
    store
        .execute_knowledge_tool(&step, "reuse".into())
        .await
        .unwrap();
    store
        .finish_agent_execution(
            &claim.lease,
            RunState::Failed,
            Some(RunFailure::ConnectionFailed),
        )
        .await
        .unwrap();
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT failures FROM learned_skills", [], |r| r
                        .get::<_, i64>(0))?,
                0
            );
            Ok(())
        })
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Attributable argument failures".into())
        .await
        .unwrap();
    let claim = claimed(&store, &user, &thread.id).await;
    let step = store.begin_agent_step(&claim.lease, 0).await.unwrap();
    let calls = (0..3)
        .map(|i| PendingKnowledgeCall {
            call_id: format!("bad-{i}"),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin: pin.clone(),
                    arguments: LearnedSkillArguments::Decimal {
                        operands: vec!["not-a-number".into(), "2".into()],
                    },
                },
            },
        })
        .collect();
    store
        .commit_agent_step(&step, capsule("bad typed inputs"), calls)
        .await
        .unwrap();
    for i in 0..3 {
        assert!(matches!(
            store
                .execute_knowledge_tool(&step, format!("bad-{i}"))
                .await
                .unwrap(),
            KnowledgeToolResult::Error(ToolReadError::InvalidInput)
        ));
    }
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "quarantined"
    );
    assert!(
        store
            .append_agent_output(&step, patch("delayed successful answer"))
            .await
            .is_err()
    );

    // A new schema generation cannot silently reuse a prior usable pin. Fixture
    // reset only isolates schema drift from the separate quarantine condition.
    store
        .call(|db| {
            db.conn.execute(
                "UPDATE learned_skills SET state='usable',failures=0,schema_version=2",
                [],
            )?;
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "invalidated"
    );
    let thread = store
        .create_thread(&user, "New schema".into())
        .await
        .unwrap();
    let fresh = claimed(&store, &user, &thread.id).await;
    assert!(
        !fresh
            .tool_profile
            .as_ref()
            .unwrap()
            .skills
            .iter()
            .any(|p| p.id == pin.id)
    );
    let step = tool_step(&store, &fresh, skill_call("stale", pin)).await;
    assert!(matches!(
        store
            .execute_knowledge_tool(&step, "stale".into())
            .await
            .unwrap(),
        KnowledgeToolResult::Error(ToolReadError::Unavailable)
    ));
}

#[tokio::test]
async fn learned_training_thread_deletion_demotes_route_and_preserves_unrelated_discovery() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    let a = learned_math_example(&store, &user, &["1", "2"], Some("3")).await;
    learned_math_example(&store, &user, &["4", "5"], Some("9")).await;
    let d_thread = store
        .create_thread(&user, "Unrelated discovery D".into())
        .await
        .unwrap();
    let d = claimed(&store, &user, &d_thread.id).await;
    finish(&store, &d, "unrelated ordinary reply").await;
    let c_thread = store
        .create_thread(&user, "Pending adopted route C".into())
        .await
        .unwrap();
    let c = claimed(&store, &user, &c_thread.id).await;
    let pin = c
        .tool_profile
        .as_ref()
        .unwrap()
        .skills
        .iter()
        .find(|p| p.id.starts_with("private:"))
        .unwrap()
        .clone();
    let step = tool_step(
        &store,
        &c,
        PendingKnowledgeCall {
            call_id: "pending".into(),
            input: KnowledgeToolInput::LearnedSkill {
                invocation: LearnedSkillInvocation {
                    pin,
                    arguments: LearnedSkillArguments::Decimal {
                        operands: vec!["7".into(), "8".into()],
                    },
                },
            },
        },
    )
    .await;
    let a_run = a.lease.run_id().to_owned();
    let a_thread = store
        .call(move |db| {
            Ok(db.conn.query_row(
                "SELECT thread_id FROM chat_runs WHERE id=?1",
                [a_run],
                |r| r.get::<_, String>(0),
            )?)
        })
        .await
        .unwrap();
    store.delete_thread(&user, a_thread).await.unwrap();
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate"
    );
    store
        .call(|db| {
            assert_eq!(
                db.conn
                    .query_row("SELECT count(*) FROM learned_skill_proofs", [], |r| r
                        .get::<_, i64>(0))?,
                1
            );
            Ok(())
        })
        .await
        .unwrap();
    assert!(
        store
            .execute_knowledge_tool(&step, "pending".into())
            .await
            .is_err()
    );
    assert!(
        store
            .append_agent_output(&step, patch("late answer"))
            .await
            .is_err()
    );
    assert!(
        !store
            .conversation(&user, d_thread.id, None)
            .await
            .unwrap()
            .turns[0]
            .assistant
            .is_empty()
    );
}

#[tokio::test]
async fn learned_schema37_upgrade_preserves_ordinary_thread_deletion() {
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    let thread = store
        .create_thread(&user, "Existing schema37 thread".into())
        .await
        .unwrap();
    store.call(|db|{db.conn.execute_batch("DROP TABLE learned_skill_receipts; DROP TABLE learned_skill_observations; DROP TABLE learned_skill_uses; DROP TABLE learned_skill_proofs; DROP TABLE learned_skills; PRAGMA user_version=37;")?;Ok(())}).await.unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    store.delete_thread(&user, thread.id).await.unwrap();
    assert!(store.learned_skills(&user).await.unwrap().is_empty());
}

#[tokio::test]
async fn learned_foreground_admission_keeps_mission_and_recurring_profiles_within_supported_tools()
{
    let _lock = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = enabled(&store).await;
    learned_math_example(&store, &user, &["1", "2"], Some("3")).await;
    learned_math_example(&store, &user, &["4", "5"], Some("9")).await;
    let thread = store
        .create_thread(&user, "Foreground discovery".into())
        .await
        .unwrap();
    let foreground = claimed(&store, &user, &thread.id).await;
    assert!(
        foreground
            .tool_profile
            .as_ref()
            .unwrap()
            .skills
            .iter()
            .any(|p| p.id.starts_with("private:"))
    );
    store
        .finish_agent_execution(&foreground.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    let template = || MissionCreate {
        request_id: Uuid::new_v4().to_string(),
        task: "Compute 0.1 plus 0.2".into(),
        constraints: String::new(),
        checks: vec![MissionCheck::DecimalEquals {
            expected: "0.3".into(),
        }],
        execution_mode: MissionMode::Durable,
    };
    store.create_mission(&user, template()).await.unwrap();
    let mission = store.claim_agent_run().await.unwrap().unwrap();
    let supported = |profile: &RunToolProfile| {
        assert!(!profile.skills.iter().any(|p| p.id.starts_with("private:")));
        assert!(
            !profile
                .community_metadata
                .iter()
                .any(|m| m.pin.id.starts_with("private:"))
        );
        assert!(profile.skills.iter().any(|p| p.id == "source-briefing"));
    };
    supported(mission.tool_profile.as_ref().unwrap());
    store
        .finish_agent_execution(&mission.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    let at = now().unwrap() + 60;
    store
        .create_recurring_mission(
            &user,
            RecurringMissionCreate {
                request_id: Uuid::new_v4().to_string(),
                template: template(),
                schedule: ScheduleDraft {
                    title: "Bounded recurring read".into(),
                    message: "Synthetic".into(),
                    rule: ScheduleRule::Interval {
                        anchor: at,
                        seconds: 60,
                    },
                    valid_until: None,
                },
                changes_only: true,
            },
        )
        .await
        .unwrap();
    store.enqueue_scheduled_reminders_at(at).await.unwrap();
    assert_eq!(store.poll_recurring_missions_at(at).await.unwrap(), 1);
    let recurring = store.claim_agent_run().await.unwrap().unwrap();
    supported(recurring.tool_profile.as_ref().unwrap());
}

async fn learned_text_followup(
    store: &Store,
    user: &UserContext,
    thread: &str,
) -> crate::ClaimedRun {
    let turn = store
        .submit_turn(
            user,
            thread.into(),
            Uuid::new_v4().to_string(),
            "Restate the previous exact result.".into(),
        )
        .await
        .unwrap();
    let id = turn.run.id;
    // A previously admitted text-only model has no tool authority/profile. Its
    // actual context must still carry withdrawal of delivered ancestor output.
    store.call(move|db|{db.conn.execute("UPDATE chat_runs SET model=json_set(json_remove(model,'$.agent_profile'),'$.function_tools',json('false')) WHERE id=?1",[&id])?;Ok(())}).await.unwrap();
    let claim = store.claim_run().await.unwrap().unwrap();
    assert!(
        claim
            .messages
            .iter()
            .any(|m| matches!(m.role, crate::ContextRole::Assistant) && m.text.contains("11")),
        "real selected model context contains C/D's learned result"
    );
    claim
}
#[tokio::test]
async fn learned_withdrawal_follows_delivered_context_into_text_only_completed_and_held_descendants()
 {
    let _lock = TEST_LOCK.lock().await;
    for withdrawal in ["remove", "disable", "training_thread"] {
        let dir = directory();
        let store = Store::open(dir.path()).unwrap();
        let user = enabled(&store).await;
        let a = learned_math_example(&store, &user, &["1", "2"], Some("3")).await;
        learned_math_example(&store, &user, &["4", "5"], Some("9")).await;
        let other = member(&store, &user, &format!("context-{withdrawal}@example.test"))
            .await
            .user;
        let foreign = store
            .create_thread(&other, "Foreign ordinary context".into())
            .await
            .unwrap();
        let foreign_claim = claimed(&store, &other, &foreign.id).await;
        assert!(
            !foreign_claim
                .tool_profile
                .as_ref()
                .unwrap()
                .skills
                .iter()
                .any(|p| p.id.starts_with("private:"))
        );
        finish(&store, &foreign_claim, "Foreign ordinary response").await;
        let unrelated = store
            .create_thread(&user, "Unrelated discovery F".into())
            .await
            .unwrap();
        let f = claimed(&store, &user, &unrelated.id).await;
        finish(&store, &f, "Independent ordinary response").await;
        let thread = store
            .create_thread(&user, "Learned C and actual descendants".into())
            .await
            .unwrap();
        let c = claimed(&store, &user, &thread.id).await;
        let pin = c
            .tool_profile
            .as_ref()
            .unwrap()
            .skills
            .iter()
            .find(|p| p.id.starts_with("private:"))
            .unwrap()
            .clone();
        let step = tool_step(
            &store,
            &c,
            PendingKnowledgeCall {
                call_id: "C".into(),
                input: KnowledgeToolInput::LearnedSkill {
                    invocation: LearnedSkillInvocation {
                        pin,
                        arguments: LearnedSkillArguments::Decimal {
                            operands: vec!["10.25".into(), "0.75".into()],
                        },
                    },
                },
            },
        )
        .await;
        assert!(
            matches!(store.execute_knowledge_tool(&step,"C".into()).await.unwrap(),KnowledgeToolResult::Decimal{exact} if exact=="11")
        );
        finish(&store, &c, "11").await;
        let d = learned_text_followup(&store, &user, &thread.id).await;
        // Text-only D must work before withdrawal, without receiving tool pins.
        store
            .append_output(&d.lease, patch("The prior result is 11."))
            .await
            .unwrap();
        store
            .finish_run(&d.lease, RunState::Completed, None)
            .await
            .unwrap();
        let interrupted = learned_text_followup(&store, &user, &thread.id).await;
        store
            .append_output(&interrupted.lease, patch("Interrupted derived result: "))
            .await
            .unwrap();
        store
            .finish_run(
                &interrupted.lease,
                RunState::Failed,
                Some(RunFailure::ConnectionFailed),
            )
            .await
            .unwrap();
        let retried = store
            .retry_run(&user, interrupted.lease.run_id().into(), 1)
            .await
            .unwrap();
        assert_eq!(retried.run.attempt, 2);
        let e = store.claim_run().await.unwrap().unwrap();
        assert!(
            e.messages
                .iter()
                .any(|m| matches!(m.role, crate::ContextRole::Assistant) && m.text.contains("11"))
        );
        store
            .append_output(&e.lease, patch("From the previous result: "))
            .await
            .unwrap();
        let card = store.learned_skills(&user).await.unwrap().remove(0);
        if withdrawal == "training_thread" {
            let a_id = a.lease.run_id().to_owned();
            let training = store
                .call(move |db| {
                    Ok(db.conn.query_row(
                        "SELECT thread_id FROM chat_runs WHERE id=?1",
                        [a_id],
                        |r| r.get::<_, String>(0),
                    )?)
                })
                .await
                .unwrap();
            store.delete_thread(&user, training).await.unwrap();
        } else {
            store
                .mutate_learned_skill(
                    &user,
                    card.id,
                    CommunitySkillMutation {
                        action: withdrawal.into(),
                        expected_revision: card.revision,
                        expected_digest: None,
                    },
                )
                .await
                .unwrap();
        }
        let page = store
            .conversation(&user, thread.id.clone(), None)
            .await
            .unwrap();
        assert!(
            page.turns.iter().all(|turn| turn.assistant.is_empty()),
            "{withdrawal} must purge actual derived C/D/E output, including completed text-only D"
        );
        assert!(
            store.append_output(&e.lease, patch("11")).await.is_err(),
            "{withdrawal} blocks held late output"
        );
        assert!(
            store
                .finish_run(&e.lease, RunState::Completed, None)
                .await
                .is_err()
        );
        assert!(
            store
                .retry_run(&user, e.lease.run_id().into(), 2)
                .await
                .is_err(),
            "withdrawal cannot be replayed through an explicit retry"
        );
        let unrelated = store.conversation(&user, unrelated.id, None).await.unwrap();
        assert!(
            !unrelated.turns[0].assistant.is_empty(),
            "discovery alone creates no dependency"
        );
        assert!(
            !store
                .conversation(&other, foreign.id, None)
                .await
                .unwrap()
                .turns[0]
                .assistant
                .is_empty(),
            "another owner is unaffected by withdrawal"
        );
    }
}
