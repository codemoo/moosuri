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
