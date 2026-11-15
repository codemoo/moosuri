//! Real supervisor -> search/read receipts -> bound arithmetic -> A/B/C reuse.
use super::knowledge::{result_values, seed, tool_call, tool_configured};
use super::*;
fn plan() -> Value {
    json!({"origins":["source","current","current"],"graph":{"input_count":3,"steps":[{"operation":"product","operands":[{"kind":"input","index":0},{"kind":"input","index":1}]},{"operation":"sum","operands":[{"kind":"result","index":0},{"kind":"input","index":2}]}]}})
}
struct EvidenceScript {
    requests: tokio::sync::Mutex<Vec<Value>>,
}
async fn response(
    State(state): State<Arc<EvidenceScript>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    state.requests.lock().await.push(body.clone());
    let prompt = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    let (label, query, prefix, qty, fee, literal, answer) = if prompt.contains("AlphaLookup") {
        (
            "A",
            "AlphaLookup",
            "PRIVATE_ALPHA_PRICE:",
            "4",
            "0.60",
            "2.35",
            "10",
        )
    } else if prompt.contains("BravoLookup") {
        (
            "B",
            "BravoLookup",
            "PRIVATE_BRAVO_RATE:",
            "2",
            "0.50",
            "1.25",
            "3",
        )
    } else {
        (
            "C",
            "CharlieLookup",
            "current_amount:",
            "3",
            "0.25",
            "4.50",
            "13.75",
        )
    };
    let parameters = json!({"query":query,"prefix":prefix,"scalars":[qty,fee]});
    let previous = result_values(&body);
    let mut calls = vec![];
    let mut text = String::new();
    if body["tools"][0]["function"]["name"] == "memory_command_decision" {
        calls.push(tool_call(0,"no-command".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null})));
    } else if previous.is_empty() && label == "C" {
        let fields = body["messages"][0]["content"]
            .as_str()
            .unwrap()
            .lines()
            .find(|s| s.starts_with("private:") && s.contains("evidence-decimal-v1:"))
            .unwrap()
            .split_whitespace()
            .collect::<Vec<_>>();
        let pin = json!({"id":fields[0],"revision":fields[1].strip_prefix("revision=").unwrap(),"digest":fields[2].strip_prefix("digest=").unwrap().trim_end_matches(':')});
        assert_eq!(pin["revision"], "3");
        calls.push(tool_call(0,"fresh-bound".into(),"learned_skill_execute",json!({"pin":pin,"arguments":{"kind":"evidence","text":null,"operands":null,"inputs":null,"tracking_number":null,"parameters":parameters}})));
    } else if previous.is_empty() && label == "B" {
        calls.push(tool_call(
            0,
            "fresh-source".into(),
            "source_calculate",
            json!({"plan":plan(),"parameters":parameters}),
        ));
    } else if previous.is_empty() {
        calls.push(tool_call(0,"search".into(),"knowledge_search",json!({"kind":"source","text":query,"collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null})));
    } else if previous.last().unwrap()["kind"] == "search" {
        let page = &previous.last().unwrap()["value"];
        assert_eq!(page["hits"].as_array().unwrap().len(), 1);
        let hit = &page["hits"][0];
        calls.push(tool_call(
            0,
            "source".into(),
            "knowledge_read_source",
            json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}),
        ));
    } else if previous.last().unwrap()["kind"] == "source" {
        let binding = json!({"search":previous[0]["receipt"],"source":previous[1]["receipt"],"query":parameters["query"],"prefix":parameters["prefix"],"scalars":parameters["scalars"]});
        calls.push(tool_call(
            0,
            "bound".into(),
            "evidence_calculate",
            json!({"plan":plan(),"binding":binding}),
        ));
    } else {
        let result = previous.last().unwrap();
        assert_eq!(result["kind"], "evidence_calculation");
        let value = &result["value"];
        assert_eq!(value["exact"], answer);
        assert_eq!(value["evidence"]["literal"], literal);
        let at = value["evidence"]["current_query"]["start_byte"]
            .as_u64()
            .unwrap() as usize;
        let end = value["evidence"]["current_query"]["end_byte"]
            .as_u64()
            .unwrap() as usize;
        assert_eq!(&prompt[at..end], query);
        assert!(
            at > prompt[..at].chars().count(),
            "server resolved UTF8 bytes after Korean/emoji"
        );
        assert_eq!(value["results"].as_array().unwrap().len(), 2);
        if label != "A" {
            assert_eq!(value["evidence"]["source"]["ordinal"], 1);
        }
        text = format!(
            "{}: {prefix} {literal}; exact current total {answer}",
            value["evidence"]["source_title"].as_str().unwrap()
        );
    }
    let done = calls.is_empty();
    let payload = frame(
        json!({"id":"evidence-flow","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
    ) + &frame(
        json!({"id":"evidence-flow","model":"fixture-model","choices":[{"index":0,"delta":if done{json!({})}else{json!({"tool_calls":calls})},"finish_reason":if done{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(payload))
        .unwrap()
}
#[tokio::test]
async fn runtime_evidence_calculation_learns_two_sources_reuses_fresh_c_and_withdraws_exported_structure_only()
 {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(EvidenceScript {
        requests: tokio::sync::Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(response))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, _) = tool_configured(&base).await;
    for (title, text) in [
        ("Synthetic A", "AlphaLookup PRIVATE_ALPHA_PRICE: 2.35"),
        ("Synthetic B", "BravoLookup PRIVATE_BRAVO_RATE: 1.25"),
        ("Synthetic C", "CharlieLookup current_amount: 4.50"),
    ] {
        seed(&store, &user, title, vec![text.into()]).await;
    }
    let runtime = Runtime::start(store.clone());
    let mut threads = vec![];
    for (q, prefix, qty, fee, answer) in [
        ("AlphaLookup", "PRIVATE_ALPHA_PRICE:", "4", "0.60", "10"),
        ("BravoLookup", "PRIVATE_BRAVO_RATE:", "2", "0.50", "3"),
        ("CharlieLookup", "current_amount:", "3", "0.25", "13.75"),
    ] {
        let thread = store
            .create_thread(&user, "PRIVATE_EVIDENCE_TRACE".into())
            .await
            .unwrap();
        let request = uuid::Uuid::new_v4().to_string();
        let mut text = format!(
            "🌱 한글 요청입니다. 자료 {q}; 숫자 앞 문구 {prefix}; 수량 {qty}; 수수료 {fee}. Calculate source number times quantity plus fee."
        );
        if q == "BravoLookup" {
            text.push_str(" 결과 제목은 BravoLookup 검토.");
        }
        store
            .submit_turn(&user, thread.id.clone(), request.clone(), text.clone())
            .await
            .unwrap();
        runtime.wake();
        let done = wait_for(&store, &user, &thread.id, |t| !t.run.state.active()).await;
        assert_eq!(
            done.run.state,
            moosuri_core::RunState::Completed,
            "{q}: {:?}",
            done.run.error
        );
        assert!(done.assistant.iter().any(|m| m.text.contains(answer)));
        let cards = store.learned_skills(&user).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].readiness,
            if threads.is_empty() {
                "candidate"
            } else {
                "ready"
            }
        );
        // UUID retry returns the already committed run without another IO/model pass.
        let retry = store
            .submit_turn(&user, thread.id.clone(), request, text)
            .await
            .unwrap();
        assert_eq!(retry.run.id, done.run.id);
        threads.push(thread.id);
    }
    let requests = state.requests.lock().await;
    let fresh_rounds = requests
        .iter()
        .filter(|v| {
            v["tools"][0]["function"]["name"] != "memory_command_decision"
                && v["messages"].as_array().unwrap().iter().any(|m| {
                    m["role"] == "user"
                        && m["content"]
                            .as_str()
                            .is_some_and(|s| s.contains("BravoLookup"))
                })
        })
        .count();
    assert_eq!(
        fresh_rounds, 2,
        "fresh source route uses one tool round and one final round"
    );
    let c = requests
        .iter()
        .filter(|v| {
            v["tools"][0]["function"]["name"] != "memory_command_decision"
                && v["messages"].as_array().unwrap().iter().any(|m| {
                    m["role"] == "user"
                        && m["content"]
                            .as_str()
                            .is_some_and(|s| s.contains("CharlieLookup"))
                })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        c.len(),
        2,
        "C uses one learned tool round and one final round"
    );
    for request in c {
        let raw = request.to_string();
        for old in [
            "AlphaLookup",
            "BravoLookup",
            "PRIVATE_ALPHA_PRICE",
            "PRIVATE_BRAVO_RATE",
            "2.35",
            "1.25",
        ] {
            assert!(!raw.contains(old), "{old}");
        }
    }
    drop(requests);
    runtime.shutdown().await;
    let card = store.learned_skills(&user).await.unwrap().remove(0);
    // Current evidence ambiguity is not a defect in the learned graph. Three
    // real failed field bindings retain readiness/revision, so no quarantine.
    for (n, prefix, quantity, scalar) in [
        (0, "missing_prefix:", "3", "3"),
        (1, "current_amount:", "1,234", "1"),
        (2, "current_amount:", "1/3", "3"),
    ] {
        let rejected = store
            .create_thread(&user, "Synthetic invalid binding".into())
            .await
            .unwrap();
        store
            .submit_turn(
                &user,
                rejected.id,
                uuid::Uuid::new_v4().to_string(),
                format!("🌱 자료 CharlieLookup; prefix {prefix}; quantity {quantity}; fee 0.25"),
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let context = store.agent_context(&claim.lease).await.unwrap();
        let step = store
            .begin_agent_step(&claim.lease, context.checkpoint_seq)
            .await
            .unwrap();
        let id = format!("missing-{n}");
        let input = moosuri_store::KnowledgeToolInput::LearnedSkill {
            invocation: moosuri_core::LearnedSkillInvocation {
                pin: moosuri_core::SkillPin {
                    id: card.id.clone(),
                    revision: card.version.clone(),
                    digest: card.digest.clone(),
                },
                arguments: moosuri_core::LearnedSkillArguments::Evidence {
                    parameters: moosuri_core::EvidenceParameters {
                        query: "CharlieLookup".into(),
                        prefix: prefix.into(),
                        scalars: vec![scalar.into(), "0.25".into()],
                    },
                },
            },
        };
        store
            .commit_agent_step(
                &step,
                moosuri_store::PrivateCheckpoint {
                    bytes: b"{}".to_vec(),
                },
                vec![moosuri_store::PendingKnowledgeCall {
                    call_id: id.clone(),
                    input,
                }],
            )
            .await
            .unwrap();
        assert!(matches!(
            store.execute_knowledge_tool(&step, id).await.unwrap(),
            moosuri_store::KnowledgeToolResult::Error(moosuri_store::ToolReadError::InvalidInput)
        ));
        let after = store.learned_skills(&user).await.unwrap().remove(0);
        assert_eq!(after.readiness, "ready");
        assert_eq!(after.revision, card.revision);
        store
            .finish_agent_execution(
                &claim.lease,
                moosuri_core::RunState::Cancelled,
                Some(moosuri_store::RunFailure::Interrupted),
            )
            .await
            .unwrap();
    }
    let preview = store
        .preview_learned_skill_share(
            &user,
            card.id.clone(),
            moosuri_core::SkillShareInput {
                expected_revision: card.revision,
            },
        )
        .await
        .unwrap();
    let raw = serde_json::to_string(&preview.files).unwrap();
    for private in [
        "AlphaLookup",
        "BravoLookup",
        "PRIVATE_ALPHA_PRICE",
        "PRIVATE_BRAVO_RATE",
        "2.35",
        "1.25",
        "PRIVATE_EVIDENCE_TRACE",
        user.account().id.as_str(),
        threads[0].as_str(),
        threads[1].as_str(),
    ] {
        assert!(!raw.contains(private), "{private}");
    }
    assert!(
        raw.contains("evidence_calculate")
            && raw.contains("synthetic_amount:")
            && raw.contains("precondition")
    );
    moosuri_store::skill_author::check(&preview.slug, preview.files).unwrap();
    store
        .delete_thread(&user, threads[1].clone())
        .await
        .unwrap();
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "candidate"
    );
    assert!(
        store
            .conversation(&user, threads[2].clone(), None)
            .await
            .unwrap()
            .turns[0]
            .assistant
            .is_empty()
    );
    server.abort();
}
