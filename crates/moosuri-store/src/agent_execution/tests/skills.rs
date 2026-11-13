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
