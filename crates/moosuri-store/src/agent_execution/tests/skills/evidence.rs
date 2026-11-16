//! Outcome and authority failures for the common source-bound arithmetic tool.
use super::*;
use moosuri_core::{
    DecimalGraph, DecimalReference, DecimalStep, EvidenceBinding, EvidenceCalculation,
    EvidenceOrigin, EvidencePlan, ToolReceiptReference,
};

fn fresh_input(query: &str, scalar: &str) -> KnowledgeToolInput {
    KnowledgeToolInput::SourceCalculate {
        calculation: moosuri_core::SourceCalculation {
            plan: plan(),
            parameters: moosuri_core::EvidenceParameters {
                query: query.into(),
                prefix: "price:".into(),
                scalars: vec![scalar.into()],
            },
        },
    }
}

#[tokio::test]
async fn source_field_label_separators_preserve_exact_numeric_evidence() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    for (index, field, valid) in [
        (0, "재료 단가: 13.75", true),
        (1, "재료 단가 ：\t13.75", true),
        (2, "재료 단가 = 13.75", true),
        (3, "재료 단가 13.75", true),
        (4, "재료 단가:: 13.75", false),
        (5, "재료 단가:\n13.75", false),
        (6, "재료 단가: 약 13.75", false),
        (7, "재료 단가: 13.75e2", false),
        (8, "재료 단가: 13.75\n재료 단가: 9", false),
        (9, "재료 단가: 13.75+9", false),
    ] {
        let query = format!("FieldCase{index}");
        let source_text = format!("{query}\n{field}\n통화: USD");
        source(&s, &u, &source_text).await;
        let request = format!("{query}에서 재료 단가를 읽고 4를 곱해줘.");
        let run = start(&s, &u, &request).await;
        let mut input = fresh_input(&query, "4");
        if let KnowledgeToolInput::SourceCalculate { calculation } = &mut input {
            calculation.parameters.prefix = "재료 단가".into();
        }
        let (_, result) = execute(&s, &run, "fresh", input).await;
        if valid {
            let KnowledgeToolResult::EvidenceCalculation {
                exact, evidence, ..
            } = result
            else {
                panic!("field {index} should bind exact source evidence");
            };
            assert_eq!(exact, "55");
            assert_eq!(evidence.literal, "13.75");
            assert_eq!(
                &source_text[evidence.start_byte as usize..evidence.end_byte as usize],
                "13.75"
            );
            assert_eq!(evidence.search.ordinal, Some(0));
            assert_eq!(evidence.source.ordinal, Some(1));
        } else {
            assert!(
                matches!(
                    result,
                    KnowledgeToolResult::Error(ToolReadError::EvidenceFieldInvalid)
                ),
                "field {index}"
            );
        }
        stop(&s, &run).await;
    }
}

#[tokio::test]
async fn fresh_source_calculation_repeated_selector_retry_cost_and_owner_isolation() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    source(&s, &u, "솔잎표찰 price: 4.86").await;
    let text =
        "솔잎표찰 단가 price: 뒤에 수량 5를 곱해. 별도 금액 0.65. 보고서 솔잎표찰 price: 검토";
    let r = start(&s, &u, text).await;
    let (step, result) = execute(&s, &r, "fresh", fresh_input("솔잎표찰", "5")).await;
    let KnowledgeToolResult::EvidenceCalculation {
        exact, evidence, ..
    } = &result
    else {
        panic!("fresh evidence")
    };
    assert_eq!(exact, "24.3");
    assert_eq!(evidence.literal, "4.86");
    assert_eq!(evidence.search.ordinal, Some(0));
    assert_eq!(evidence.source.ordinal, Some(1));
    let replay = s
        .execute_knowledge_tool(&step, "fresh".into())
        .await
        .unwrap();
    assert_eq!(
        serde_json::to_string(&result).unwrap(),
        serde_json::to_string(&replay).unwrap()
    );
    let run = r.lease.run_id().to_string();
    let counts = s.call(move |db| Ok(db.conn.query_row(
        "SELECT tool_calls,(SELECT count(*) FROM learned_skill_receipts WHERE run_id=?1) FROM agent_execution WHERE run_id=?1", [run],
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))?)).await.unwrap();
    assert_eq!(counts, (3, 3));
    stop(&s, &r).await;
    let other = member(&s, &u, "fresh-source-outsider@example.test")
        .await
        .user;
    let r = start(&s, &other, text).await;
    let (_, result) = execute(&s, &r, "foreign", fresh_input("솔잎표찰", "5")).await;
    assert!(matches!(
        result,
        KnowledgeToolResult::Error(ToolReadError::NotFound)
    ));
    stop(&s, &r).await;
}

#[tokio::test]
async fn fresh_source_calculation_reports_recoverable_failures_and_keeps_legacy_profiles_closed() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    for (query, body, expected) in [
        (
            "TwoSources",
            "price: 2.35",
            ToolReadError::EvidenceSourceNotUnique,
        ),
        (
            "TwoFields",
            "price: 2.35\nprice: 4.5",
            ToolReadError::EvidenceFieldInvalid,
        ),
        (
            "BadDecimal",
            "price: 2.35e2",
            ToolReadError::EvidenceFieldInvalid,
        ),
        (
            "Noncurrent",
            "price: 2.35",
            ToolReadError::EvidenceParametersInvalid,
        ),
    ] {
        source(&s, &u, &format!("{query} {body}")).await;
        if query == "TwoSources" {
            source(&s, &u, "TwoSources price: 8.5").await;
        }
        let text = format!("Find {query}; prefix price:; quantity 4");
        let r = start(&s, &u, &text).await;
        let scalar = if query == "Noncurrent" { "5" } else { "4" };
        let (_, result) = execute(&s, &r, "fresh", fresh_input(query, scalar)).await;
        assert!(
            matches!(result, KnowledgeToolResult::Error(error) if error == expected),
            "{query}"
        );
        stop(&s, &r).await;
    }
    let r = start(&s, &u, "Find Noncurrent; prefix price:; quantity 4").await;
    let run = r.lease.run_id().to_string();
    s.call(move |db| {
        db.conn.execute("UPDATE chat_runs SET model=json_remove(model,'$.agent_profile.source_calculation') WHERE id=?1", [run])?;
        Ok(())
    }).await.unwrap();
    let (_, result) = execute(&s, &r, "legacy", fresh_input("Noncurrent", "4")).await;
    assert!(matches!(
        result,
        KnowledgeToolResult::Error(ToolReadError::Unavailable)
    ));
    stop(&s, &r).await;
}
fn plan() -> EvidencePlan {
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
async fn start(s: &Store, u: &UserContext, text: &str) -> AgentClaim {
    let t = s
        .create_thread(u, "Synthetic evidence calculation".into())
        .await
        .unwrap();
    s.submit_turn(u, t.id, Uuid::new_v4().to_string(), text.into())
        .await
        .unwrap();
    s.claim_agent_run().await.unwrap().unwrap()
}
async fn execute(
    s: &Store,
    r: &AgentClaim,
    id: &str,
    input: KnowledgeToolInput,
) -> (StepLease, KnowledgeToolResult) {
    let context = s.agent_context(&r.lease).await.unwrap();
    let step = s
        .begin_agent_step(&r.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        capsule("synthetic source binding"),
        vec![PendingKnowledgeCall {
            call_id: id.into(),
            input,
        }],
    )
    .await
    .unwrap();
    let result = s.execute_knowledge_tool(&step, id.into()).await.unwrap();
    (step, result)
}
async fn prepare(
    s: &Store,
    r: &AgentClaim,
    _text: &str,
    q: &str,
    prefix: &str,
    number: &str,
) -> EvidenceCalculation {
    prepare_mode(s, r, q, prefix, number, None).await
}
async fn prepare_mode(
    s: &Store,
    r: &AgentClaim,
    q: &str,
    prefix: &str,
    number: &str,
    match_mode: Option<KnowledgeMatchMode>,
) -> EvidenceCalculation {
    let mut query = query(q);
    query.match_mode = match_mode;
    let (search, page) = execute(s, r, "search", KnowledgeToolInput::Search { query }).await;
    let KnowledgeToolResult::Search(page) = page else {
        panic!("search result")
    };
    let hit = page.hits.first().unwrap();
    let (read, _) = execute(
        s,
        r,
        "source",
        KnowledgeToolInput::ReadSource {
            id: hit.id.clone(),
            revision: hit.revision,
            start_byte: 0,
            max_bytes: 4096,
        },
    )
    .await;
    EvidenceCalculation {
        plan: plan(),
        binding: EvidenceBinding {
            search: ToolReceiptReference {
                step: search.step() as u32,
                call_id: "search".into(),
                ordinal: None,
            },
            source: ToolReceiptReference {
                step: read.step() as u32,
                call_id: "source".into(),
                ordinal: None,
            },
            query: q.into(),
            prefix: prefix.into(),
            scalars: vec![number.into()],
        },
    }
}
async fn stop(s: &Store, r: &AgentClaim) {
    s.finish_agent_execution(&r.lease, RunState::Cancelled, Some(RunFailure::Interrupted))
        .await
        .unwrap();
}
#[tokio::test]
async fn evidence_calculation_accepts_complete_korean_number_suffixes_not_substrings() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    source(&s, &u, "KoreanSuffixLookup price: 2.35").await;
    for (current, literal, expected) in [
        ("4.08을", "4.08", Some("9.588")),
        ("2.64야", "2.64", Some("6.204")),
        ("4개를", "4", Some("9.4")),
        ("4상자씩", "4", Some("9.4")),
        ("4개만", "4", Some("9.4")),
        ("4개를; 별도 금액 0.64", "4", Some("9.4")),
        ("4개를; 별도 금액 -14", "4", Some("9.4")),
        ("4개를; 다른 수량 4", "4", None),
        ("4개를; 다른 수량 4개", "4", None),
        ("-4.08원으로", "-4.08", Some("-9.588")),
        ("4만", "4", None),
        ("4.08만", "4.08", None),
        ("14개를", "4", None),
        ("4,080원을", "4", None),
        ("4.08원을", "4", None),
        ("-4원을", "4", None),
        ("4/2개를", "4", None),
        ("4개×2", "4", None),
        ("4개별", "4", None),
        ("4을림", "4", None),
    ] {
        let text = format!("자료 KoreanSuffixLookup; 숫자 앞 문구 price:; 수량 {current} 곱해 줘.");
        let r = start(&s, &u, &text).await;
        let calculation = prepare(&s, &r, &text, "KoreanSuffixLookup", "price:", literal).await;
        let (_, result) = execute(
            &s,
            &r,
            "korean-quantity",
            KnowledgeToolInput::EvidenceCalculate { calculation },
        )
        .await;
        match (expected, result) {
            (Some(expected), KnowledgeToolResult::EvidenceCalculation { exact, .. }) => {
                assert_eq!(exact, expected, "{current}");
            }
            (None, KnowledgeToolResult::Error(ToolReadError::InvalidInput)) => {}
            _ => panic!("unexpected evidence outcome for {current}"),
        }
        stop(&s, &r).await;
    }
}
#[tokio::test]
async fn evidence_calculation_declines_ambiguous_incomplete_stale_forged_and_noncurrent_origins() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    // Ambiguous current identities, duplicate fields and bounded incomplete reads
    // are real source/read outcomes, not model assertions or parser-only tests.
    for (q, body) in [
        ("AmbiguousLookup", "price: 2.35".to_string()),
        ("DuplicateLookup", "price: 2.35\nprice: 4.50".into()),
        ("LongLookup", format!("price: 2.35 {}", "z".repeat(4200))),
        ("ExponentLookup", "price: 2.35e2".into()),
        ("GroupedLookup", "price: 2.35".into()),
    ] {
        source(&s, &u, &format!("{q} {body}")).await;
        if q == "AmbiguousLookup" {
            source(&s, &u, &format!("{q} price: 9.00")).await;
        }
        let quantity = if q == "GroupedLookup" { "1,234" } else { "4" };
        let quoted = if q == "GroupedLookup" { "1" } else { "4" };
        let text = format!("Find {q}; prefix price:; quantity {quantity}");
        let r = start(&s, &u, &text).await;
        let calc = prepare(&s, &r, &text, q, "price:", quoted).await;
        let (_, result) = execute(
            &s,
            &r,
            "bound",
            KnowledgeToolInput::EvidenceCalculate { calculation: calc },
        )
        .await;
        assert!(
            matches!(
                result,
                KnowledgeToolResult::Error(ToolReadError::InvalidInput)
            ),
            "{q}"
        );
        stop(&s, &r).await;
    }
    let (collection, id) = source(&s, &u, "FreshLookup price: 2.35").await;
    let text = "Find FreshLookup; prefix price:; quantity 4; ambiguous ambiguous";
    let r = start(&s, &u, text).await;
    let calc = prepare(&s, &r, text, "FreshLookup", "price:", "4").await;
    // Reject a forged address and overlapping/current-substring scalar before
    // evaluating any arithmetic. The source-derived amount is never an argument.
    for variant in 0..3 {
        let mut bad = calc.clone();
        match variant {
            0 => bad.binding.source.call_id = "not-receipted".into(),
            1 => bad.binding.scalars[0] = bad.binding.prefix.clone(),
            _ => bad.binding.prefix = "ambiguous".into(),
        }
        let (_, result) = execute(
            &s,
            &r,
            &format!("bad-{variant}"),
            KnowledgeToolInput::EvidenceCalculate { calculation: bad },
        )
        .await;
        assert!(matches!(
            result,
            KnowledgeToolResult::Error(ToolReadError::InvalidInput)
        ));
    }
    let (valid, result) = execute(
        &s,
        &r,
        "valid",
        KnowledgeToolInput::EvidenceCalculate {
            calculation: calc.clone(),
        },
    )
    .await;
    assert!(matches!(result,KnowledgeToolResult::EvidenceCalculation{exact,..} if exact=="9.4"));
    assert!(
        matches!(s.execute_knowledge_tool(&valid,"valid".into()).await.unwrap(),KnowledgeToolResult::EvidenceCalculation{exact,..} if exact=="9.4")
    );
    let run = r.lease.run_id().to_owned();
    let counts=s.call(move|db|Ok(db.conn.query_row("SELECT tool_calls,(SELECT count(*) FROM learned_skill_receipts WHERE run_id=?1) FROM agent_execution WHERE run_id=?1",[run],|v|Ok((v.get::<_,i64>(0)?,v.get::<_,i64>(1)?)))?)).await.unwrap();
    assert_eq!(counts, (6, 1));
    // New run/owner cannot point back to this run's otherwise valid receipts.
    stop(&s, &r).await;
    let outsider = member(&s, &u, "evidence-other@example.test").await.user;
    let other = start(&s, &outsider, text).await;
    let (_, result) = execute(
        &s,
        &other,
        "foreign",
        KnowledgeToolInput::EvidenceCalculate {
            calculation: calc.clone(),
        },
    )
    .await;
    assert!(matches!(
        result,
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    stop(&s, &other).await;
    let r = start(&s, &u, text).await;
    let calc = prepare(&s, &r, text, "FreshLookup", "price:", "4").await;
    // A source revision change between read and execution revokes the run before
    // any new calculation or late output can be published.
    let mut update = input("one", "FreshLookup price: 8.00");
    update.expected_revision = Some(1);
    update.version = "v2".into();
    s.ingest_text(&u, collection.id, batch(1, vec![update]))
        .await
        .unwrap();
    assert!(s.agent_context(&r.lease).await.is_err());
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
    assert!(!id.is_empty() && calc.plan.valid());
}
#[tokio::test]
async fn evidence_calculation_old_profiles_stay_unavailable_and_generation_drift_requires_fresh_proofs()
 {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    source(&s, &u, "OldLookup price: 2.35").await;
    let text = "Find OldLookup; prefix price:; quantity 4";
    let r = start(&s, &u, text).await;
    let calc = prepare(&s, &r, text, "OldLookup", "price:", "4").await;
    let run = r.lease.run_id().to_owned();
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        let mut model: serde_json::Value =
            serde_json::from_str(&db.conn.query_row::<String, _, _>(
                "SELECT model FROM chat_runs WHERE tenant_id=?1 AND id=?2",
                params![tenant, run],
                |row| row.get(0),
            )?)
            .map_err(|_| StoreError::Corrupt)?;
        model["agent_profile"]["evidence_calculation"] = serde_json::Value::Bool(false);
        db.conn.execute(
            "UPDATE chat_runs SET model=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, run, model.to_string()],
        )?;
        Ok(())
    })
    .await
    .unwrap();
    let (_, result) = execute(
        &s,
        &r,
        "old",
        KnowledgeToolInput::EvidenceCalculate { calculation: calc },
    )
    .await;
    assert!(matches!(
        result,
        KnowledgeToolResult::Error(ToolReadError::Unavailable)
    ));
    stop(&s, &r).await;
    learned_math_example(&s, &u, &["1", "2"], Some("3")).await;
    learned_math_example(&s, &u, &["3", "4"], Some("7")).await;
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    let id = card.id.clone();
    let account = u.account.id.clone();
    let tenant = u.tenant.0.clone();
    s.call(move|db| {db.conn.execute("UPDATE learned_skills SET schema_version=2 WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![tenant,account,id])?;Ok(())}).await.unwrap();
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "invalidated"
    );
    learned_math_example(&s, &u, &["5", "6"], Some("11")).await;
    assert_eq!(
        s.learned_skills(&u).await.unwrap()[0].readiness,
        "candidate"
    );
    learned_math_example(&s, &u, &["7", "8"], Some("15")).await;
    assert_eq!(s.learned_skills(&u).await.unwrap()[0].readiness, "ready");
}

#[tokio::test]
async fn evidence_calculation_export_example_executes_with_many_unique_current_scalars() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    source(&s, &u, "PrivateExportTraining PRIVATE_EXPORT_FIELD: 17").await;
    let training_scalars = (201..212).map(|v| v.to_string()).collect::<Vec<_>>();
    let training = format!(
        "Search PrivateExportTraining; prefix PRIVATE_EXPORT_FIELD:; scalars {}",
        serde_json::to_string(&training_scalars).unwrap()
    );
    let r = start(&s, &u, &training).await;
    let mut calculation = prepare(
        &s,
        &r,
        &training,
        "PrivateExportTraining",
        "PRIVATE_EXPORT_FIELD:",
        &training_scalars[0],
    )
    .await;
    calculation.plan = EvidencePlan {
        origins: std::iter::once(EvidenceOrigin::Source)
            .chain(std::iter::repeat_n(EvidenceOrigin::Current, 11))
            .collect(),
        graph: DecimalGraph {
            input_count: 12,
            steps: vec![DecimalStep {
                operation: DecimalOperation::Sum,
                operands: (0..12)
                    .map(|index| DecimalReference::Input { index })
                    .collect(),
            }],
        },
    };
    calculation.binding.scalars = training_scalars.clone();
    let (_, result) = execute(
        &s,
        &r,
        "large-plan",
        KnowledgeToolInput::EvidenceCalculate { calculation },
    )
    .await;
    let KnowledgeToolResult::EvidenceCalculation {
        exact, evidence, ..
    } = result
    else {
        panic!("actual training calculation");
    };
    let context = s.agent_context(&r.lease).await.unwrap();
    let final_step = s
        .begin_agent_step(&r.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.append_agent_output(
        &final_step,
        vec![OutputPatch::Append {
            item: format!("step-{}-answer", final_step.step()),
            text: format!(
                "{} PRIVATE_EXPORT_FIELD: {}; exact result {exact}",
                evidence.source_title, evidence.literal
            ),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.commit_agent_step(
        &final_step,
        capsule("final synthetic export training"),
        vec![],
    )
    .await
    .unwrap();
    s.finish_agent_run(&final_step, RunState::Completed, None)
        .await
        .unwrap();
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    assert_eq!(card.readiness, "candidate");
    let preview = s
        .preview_learned_skill_share(
            &u,
            card.id,
            SkillShareInput {
                expected_revision: card.revision,
            },
        )
        .await
        .unwrap();
    crate::skill_author::check(&preview.slug, preview.files.clone()).unwrap();
    let examples: serde_json::Value =
        serde_json::from_str(&preview.files["examples.json"]).unwrap();
    let example = examples["examples"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["kind"] == "normal")
        .unwrap();
    let request = example["request"].as_str().unwrap();
    let instructions = &preview.files["SKILL.md"];
    let exported: EvidencePlan = serde_json::from_str(
        instructions
            .split("with this structure: ")
            .nth(1)
            .unwrap()
            .split(". Bind its")
            .next()
            .unwrap(),
    )
    .unwrap();
    let quoted = request.split('`').skip(1).step_by(2).collect::<Vec<_>>();
    let (query, prefix) = (quoted[0], quoted[1]);
    let scalars: Vec<String> = serde_json::from_str(
        request
            .split("fresh scalar slots ")
            .nth(1)
            .unwrap()
            .split(". Apply")
            .next()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(scalars.len(), 11);
    let synthetic_source = instructions
        .split("contains `")
        .nth(1)
        .unwrap()
        .split('`')
        .next()
        .unwrap();
    let collection = s
        .create_collection(
            &u,
            "Separate synthetic export fixture".into(),
            Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    let mut fixture = input("export-fixture", synthetic_source);
    fixture.title = query.into();
    s.ingest_text(&u, collection.id, batch(0, vec![fixture]))
        .await
        .unwrap();
    // Execute the exact exported current request and its exported graph, rather
    // than restating or patching the example into a passing handwritten request.
    let fresh = start(&s, &u, request).await;
    let mut calculation = prepare(&s, &fresh, request, query, prefix, &scalars[0]).await;
    calculation.plan = exported;
    calculation.binding.scalars = scalars;
    let (_, result) = execute(
        &s,
        &fresh,
        "exported-example",
        KnowledgeToolInput::EvidenceCalculate { calculation },
    )
    .await;
    let KnowledgeToolResult::EvidenceCalculation {
        exact, evidence, ..
    } = result
    else {
        panic!("exported normal example must execute");
    };
    assert_eq!(evidence.literal, "100");
    assert!(
        example["expected"]
            .as_str()
            .unwrap()
            .contains(&format!("exact result is {exact}"))
    );
    let raw = serde_json::to_string(&preview.files).unwrap();
    for private in [
        "PrivateExportTraining",
        "PRIVATE_EXPORT_FIELD",
        "201",
        "211",
    ] {
        assert!(!raw.contains(private), "{private}");
    }
    stop(&s, &fresh).await;
}

#[tokio::test]
async fn korean_aliases_evidence_binding_declines_even_when_literal_fallback_found_one_source() {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    source(&s, &u, "AliasPrice price: 2.35").await;
    let r = start(&s, &u, "Find AliasPrice; prefix price:; quantity 4").await;
    let calculation = prepare_mode(
        &s,
        &r,
        "AliasPrice",
        "price:",
        "4",
        Some(KnowledgeMatchMode::KoreanAliasesV1),
    )
    .await;
    let (_, result) = execute(
        &s,
        &r,
        "bound",
        KnowledgeToolInput::EvidenceCalculate { calculation },
    )
    .await;
    assert!(matches!(
        result,
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    assert!(s.learned_skills(&u).await.unwrap().is_empty());
    stop(&s, &r).await;
}

#[tokio::test]
async fn mission_evidence_recipe_uses_accepted_task_and_fresh_source_then_withdraws() {
    mission_evidence_recipe_outcome(false).await;
}
#[tokio::test]
async fn compound_report_learned_evidence_recipe_retains_actual_child_read_then_withdraws() {
    mission_evidence_recipe_outcome(true).await;
}
async fn mission_evidence_recipe_outcome(report: bool) {
    let _guard = TEST_LOCK.lock().await;
    let dir = directory();
    let s = Store::open(dir.path()).unwrap();
    let u = enabled(&s).await;
    for (q, price, count) in [
        ("MissionProofAlpha", "2.35", "4"),
        ("MissionProofBravo", "1.25", "2"),
    ] {
        source(&s, &u, &format!("{q} cost: {price}")).await;
        let text = format!("Find {q}; prefix cost:; quantity {count}");
        let r = start(&s, &u, &text).await;
        let calculation = prepare(&s, &r, &text, q, "cost:", count).await;
        let (_, output) = execute(
            &s,
            &r,
            "bound",
            KnowledgeToolInput::EvidenceCalculate { calculation },
        )
        .await;
        let KnowledgeToolResult::EvidenceCalculation {
            exact, evidence, ..
        } = output
        else {
            panic!("training receipt")
        };
        let context = s.agent_context(&r.lease).await.unwrap();
        let final_step = s
            .begin_agent_step(&r.lease, context.checkpoint_seq)
            .await
            .unwrap();
        s.append_agent_output(
            &final_step,
            vec![OutputPatch::Append {
                item: format!("step-{}-answer", final_step.step()),
                text: format!(
                    "{} cost: {}; exact {exact}",
                    evidence.source_title, evidence.literal
                ),
                phase: Some(MessagePhase::FinalAnswer),
                refusal: false,
            }],
        )
        .await
        .unwrap();
        s.commit_agent_step(
            &final_step,
            capsule("terminal adopted source calculation"),
            vec![],
        )
        .await
        .unwrap();
        s.finish_agent_run(&final_step, RunState::Completed, None)
            .await
            .unwrap();
    }
    let card = s.learned_skills(&u).await.unwrap().remove(0);
    assert_eq!(card.readiness, "ready");
    let pin = SkillPin {
        id: card.id,
        revision: card.version,
        digest: card.digest,
    };
    let (_, source_id) = source(&s, &u, "MissionFreshLookup cost: 2.50").await;
    let mut original =
        super::missions::template("Find SupersededLookup; prefix retired:; quantity 8", "64");
    if report {
        original.checks.push(MissionCheck::PrivateReport {
            title: "Synthetic learned report".into(),
        });
    }
    let m = s.create_mission(&u, original).await.unwrap();
    let paused = s
        .control_mission(
            &u,
            m.id.clone(),
            MissionControl {
                request_id: Uuid::new_v4().to_string(),
                expected_revision: m.revision,
                action: MissionAction::Pause,
            },
        )
        .await
        .unwrap();
    s.revise_mission_acceptance(
        &u,
        m.id.clone(),
        moosuri_core::MissionAcceptanceUpdate {
            request_id: Uuid::new_v4().to_string(),
            expected_revision: paused.revision,
            task: "Find MissionFreshLookup; prefix cost:; quantity 3".into(),
            constraints: String::new(),
            checks: {
                let mut checks = vec![MissionCheck::DecimalEquals {
                    expected: "7.5".into(),
                }];
                if report {
                    checks.push(MissionCheck::PrivateReport {
                        title: "Synthetic learned report".into(),
                    });
                }
                checks
            },
        },
    )
    .await
    .unwrap();
    let r = s.claim_agent_run().await.unwrap().unwrap();
    let (_, superseded) = super::missions::reuse(
        &s,
        &r,
        pin.clone(),
        LearnedSkillArguments::Evidence {
            parameters: moosuri_core::EvidenceParameters {
                query: "SupersededLookup".into(),
                prefix: "retired:".into(),
                scalars: vec!["8".into()],
            },
        },
    )
    .await;
    assert!(matches!(
        superseded,
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    // A verification answer in serialized metadata is not a current-task scalar.
    let (_, bad) = super::missions::reuse(
        &s,
        &r,
        pin.clone(),
        LearnedSkillArguments::Evidence {
            parameters: moosuri_core::EvidenceParameters {
                query: "MissionFreshLookup".into(),
                prefix: "cost:".into(),
                scalars: vec!["7.5".into()],
            },
        },
    )
    .await;
    assert!(matches!(
        bad,
        KnowledgeToolResult::Error(ToolReadError::InvalidInput)
    ));
    let (step, output) = super::missions::reuse(
        &s,
        &r,
        pin.clone(),
        LearnedSkillArguments::Evidence {
            parameters: moosuri_core::EvidenceParameters {
                query: "MissionFreshLookup".into(),
                prefix: "cost:".into(),
                scalars: vec!["3".into()],
            },
        },
    )
    .await;
    assert!(
        matches!(output, KnowledgeToolResult::EvidenceCalculation { exact, evidence, .. } if exact=="7.5" && evidence.source_id==source_id && evidence.literal=="2.50")
    );
    super::missions::verify(&s, &r, &step, "Calculated from current cost: 7.5").await;
    let completed = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    assert_eq!(completed.state, if report { "partial" } else { "achieved" });
    let saved = if report {
        let contract = completed.compound.as_ref().unwrap();
        let read = &contract.steps[0].receipts[0];
        assert_eq!(read.step, step.step());
        assert_eq!(read.call_id, "fresh-reuse");
        assert_eq!(read.ordinal, Some(1));
        assert_eq!(contract.steps[2].receipts[0].ordinal, None);
        let output = contract.output.clone().unwrap();
        assert!(
            s.artifact(&u, output.id.clone(), None)
                .await
                .unwrap()
                .draft
                .is_some()
        );
        Some(output)
    } else {
        None
    };
    assert!(
        completed
            .receipts
            .iter()
            .any(|v| v.detail.contains("2.50 → 7.5"))
    );
    // Completed delegated runs are consumers, never extra independent training proofs.
    let tenant = u.tenant.0.clone();
    s.call(move |db| {
        assert_eq!(
            db.conn.query_row(
                "SELECT count(*) FROM learned_skill_proofs WHERE tenant_id=?1",
                [tenant],
                |row| row.get::<_, i64>(0)
            )?,
            2
        );
        Ok(())
    })
    .await
    .unwrap();
    s.delete_source(&u, source_id, 1).await.unwrap();
    if let Some(output) = saved {
        assert!(
            s.artifact(&u, output.id, None)
                .await
                .unwrap()
                .draft
                .is_none()
        );
    }
    let withdrawn = s
        .missions(&u)
        .await
        .unwrap()
        .into_iter()
        .find(|v| v.id == m.id)
        .unwrap();
    assert_eq!(withdrawn.state, "quarantined");
    assert!(withdrawn.result.is_none() && withdrawn.receipts.is_empty());
    let outsider = member(&s, &u, "mission-outsider@example.test").await.user;
    s.create_mission(
        &outsider,
        super::missions::template("Find MissionFreshLookup; prefix cost:; quantity 3", "7.5"),
    )
    .await
    .unwrap();
    let other = s.claim_agent_run().await.unwrap().unwrap();
    assert!(!other.tool_profile.as_ref().unwrap().skills.contains(&pin));
    let (_, refused) = super::missions::reuse(
        &s,
        &other,
        pin,
        LearnedSkillArguments::Evidence {
            parameters: moosuri_core::EvidenceParameters {
                query: "MissionFreshLookup".into(),
                prefix: "cost:".into(),
                scalars: vec!["3".into()],
            },
        },
    )
    .await;
    assert!(matches!(
        refused,
        KnowledgeToolResult::Error(ToolReadError::Unavailable)
    ));
}
