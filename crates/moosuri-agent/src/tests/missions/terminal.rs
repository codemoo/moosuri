//! A clean HTTP EOF does not turn a limited/refused mission into completed work.
use super::*;
use moosuri_connectors::model::protocol::tools::{ToolCapabilities, ToolRequest};
use moosuri_connectors::model::protocol::{TextMessage, TextRequest, TextRole};
use moosuri_store::{KnowledgeToolInput, PendingKnowledgeCall, PrivateCheckpoint};

#[tokio::test]
async fn mission_terminal_outcome_fences_candidate_clarification_and_tools() {
    let _guard = TEST_LOCK.lock().await;
    for (reason, response_kind) in [
        ("stop", "candidate"),
        ("length", "candidate"),
        ("content_filter", "candidate"),
        ("length", "report"),
        ("content_filter", "report"),
        ("length", "clarification"),
        ("content_filter", "clarification"),
        ("length", "tools"),
        ("content_filter", "tools"),
    ] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let app = Router::new().route("/v1/chat/completions", post(move |axum::Json(body): axum::Json<Value>| {
            let seen = seen.clone();
            async move {
                let n = seen.fetch_add(1, Ordering::SeqCst);
                assert!(n < 2, "non-successful terminal must not retry inference");
                let context: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
                let (delta, finish) = if n == 0 {
                    let mut calls = vec![knowledge::tool_call(0,"terminal-sum".into(),"decimal_calculate",json!({"operation":"sum","operands":["0.1","0.2"]}))];
                    if response_kind == "report" {
                        let source = context["accepted"]["checks"].as_array().unwrap().iter().find(|check| check["kind"] == "source_quote").unwrap();
                        calls.push(knowledge::tool_call(1,"terminal-source".into(),"knowledge_read_source",json!({"id":source["id"],"revision":1,"start_byte":0,"max_bytes":4096})));
                    }
                    (json!({"tool_calls":calls}), "tool_calls")
                } else {
                    let receipts = context["receipts"].as_array().unwrap();
                    assert!(receipts.iter().any(|r|r["result"]["value"]["exact"] == "0.3"));
                    (match response_kind {
                        "report" => json!({"content":json!({"text":"Synthetic invoice total is 0.3.","evidence":receipts.iter().map(|r|json!({"step":r["step"],"call_id":r["call_id"]})).collect::<Vec<_>>()} ).to_string()}),
                        "candidate" => json!({"content":json!({"text":"The exact result is 0.3.","evidence":[{"step":1,"call_id":"terminal-sum"}]}).to_string()}),
                        "clarification" => json!({"content":json!({"input_request":{"field":"context","question":"What criterion should this original result use?","candidates":[]}}).to_string()}),
                        "tools" => json!({"tool_calls":[knowledge::tool_call(0,"must-not-execute".into(),"decimal_calculate",json!({"operation":"sum","operands":["1","2"]}))]}),
                        _ => unreachable!(),
                    }, reason)
                };
                let wire = frame(json!({"id":format!("terminal-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))
                    + &frame(json!({"id":format!("terminal-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":finish}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))
                    + "data: [DONE]\n\n";
                Response::builder().header("content-type", "text/event-stream").body(Body::from(wire)).unwrap()
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (dir, store, user, _) = knowledge::tool_configured(&base).await;
        let mut checks = vec![MissionCheck::DecimalEquals {
            expected: "0.3".into(),
        }];
        if response_kind == "report" {
            let source = knowledge::seed(
                &store,
                &user,
                "Terminal invoice",
                vec!["Synthetic invoice total is 0.3.".into()],
            )
            .await
            .remove(0);
            checks.extend([
                MissionCheck::SourceQuote {
                    id: source,
                    revision: 1,
                    quote: "Synthetic invoice total is 0.3.".into(),
                },
                MissionCheck::PrivateReport {
                    title: "Non-successful model report".into(),
                },
            ]);
        }
        store
            .create_mission(
                &user,
                MissionCreate {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    task: if response_kind == "report" {
                        "Read Terminal invoice, compute 0.1 plus 0.2 and save one private report"
                    } else {
                        "Compute 0.1 plus 0.2"
                    }
                    .into(),
                    constraints: String::new(),
                    checks,
                    execution_mode: MissionMode::Durable,
                },
            )
            .await
            .unwrap();
        let runtime = Runtime::start(store.clone());
        let done = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let view = store.missions(&user).await.unwrap().remove(0);
                if !["queued", "planning", "executing", "verifying"].contains(&view.state.as_str())
                {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        if reason == "stop" {
            assert_eq!(done.state, "achieved");
            assert_eq!(done.result.unwrap().text, "The exact result is 0.3.");
        } else {
            assert_ne!(done.state, "achieved", "{reason}/{response_kind}");
            assert_ne!(done.state, "waiting", "{reason}/{response_kind}");
            assert!(
                done.result.is_none(),
                "non-successful model text is not an admitted candidate"
            );
            assert!(done.inputs.is_empty());
            assert_eq!(
                done.completed_steps,
                if response_kind == "report" { 2 } else { 1 },
                "{reason}/{response_kind}: prior results retained, no further tool executed"
            );
            assert!(
                store
                    .artifacts(&user, None, None)
                    .await
                    .unwrap()
                    .items
                    .is_empty(),
                "non-successful final response must not save a private report"
            );
        }
        assert_eq!(count.load(Ordering::SeqCst), 2);
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            2
        );
        runtime.shutdown().await;
        drop(runtime);
        drop(store);
        let reopened = Store::open(dir.path()).unwrap();
        let saved = reopened.missions(&user).await.unwrap().remove(0);
        let run = reopened.claim_agent_run().await.unwrap();
        assert!(
            run.is_none(),
            "terminal responses must not replay after restart"
        );
        if reason != "stop" {
            assert!(saved.result.is_none());
        }
        server.abort();
    }
}

#[tokio::test]
async fn mission_legacy_unverified_checkpoint_cannot_resume_candidate_question_or_pending_tool() {
    let _guard = TEST_LOCK.lock().await;
    for kind in ["candidate", "clarification", "tools"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let text = if kind == "candidate" {
            json!({"text":"The exact result is 0.3.","evidence":[{"step":1,"call_id":"legacy-sum"}]}).to_string()
        } else if kind == "clarification" {
            json!({"input_request":{"field":"context","question":"Which original criterion?","candidates":[]}}).to_string()
        } else {
            String::new()
        };
        let wire_text = text.clone();
        let app = Router::new().route("/v1/chat/completions", post(move || {
            let seen = seen.clone(); let text = wire_text.clone();
            async move {
                assert_eq!(seen.fetch_add(1, Ordering::SeqCst), 0, "legacy recovery must not dispatch inference");
                let (delta, reason) = if kind == "tools" {
                    (json!({"tool_calls":[knowledge::tool_call(0,"legacy-pending".into(),"decimal_calculate",json!({"operation":"sum","operands":["1","2"]}))]}), "tool_calls")
                } else { (json!({"content":text}), "length") };
                let wire = frame(json!({"id":"legacy-terminal","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}})) + "data: [DONE]\n\n";
                Response::builder().header("content-type", "text/event-stream").body(Body::from(wire)).unwrap()
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (dir, store, user, _) = knowledge::tool_configured(&base).await;
        store
            .create_mission(
                &user,
                MissionCreate {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    task: "Compute 0.1 plus 0.2".into(),
                    constraints: String::new(),
                    checks: vec![MissionCheck::DecimalEquals {
                        expected: "0.3".into(),
                    }],
                    execution_mode: MissionMode::Durable,
                },
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let step = store
            .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
            .await
            .unwrap();
        let sum = |call_id: &str| PendingKnowledgeCall {
            call_id: call_id.into(),
            input: KnowledgeToolInput::DecimalCalculate {
                calculation: moosuri_core::DecimalCalculation {
                    operation: moosuri_core::DecimalOperation::Sum,
                    operands: vec!["0.1".into(), "0.2".into()],
                },
            },
        };
        store
            .commit_agent_step(
                &step,
                PrivateCheckpoint {
                    bytes: b"{}".to_vec(),
                },
                vec![sum("legacy-sum")],
            )
            .await
            .unwrap();
        store
            .mission_save_plan(&step, vec!["Calculate and verify".into()])
            .await
            .unwrap();
        store
            .execute_knowledge_tool(&step, "legacy-sum".into())
            .await
            .unwrap();
        let context = store.agent_context(&claim.lease).await.unwrap();
        let final_step = store
            .begin_agent_step(&claim.lease, context.checkpoint_seq)
            .await
            .unwrap();
        let client =
            model_client(&claim.binding.config, None, TextCapabilities::default()).unwrap();
        let request = TextRequest {
            model: claim.binding.model.clone(),
            effort: None,
            max_output_tokens: 4096,
            messages: vec![TextMessage {
                role: TextRole::User,
                text: "Complete the original synthetic task".into(),
                phase: None,
            }],
        };
        let definitions = crate::knowledge_tools::definitions(claim.tool_profile.as_ref());
        let tools = ToolRequest {
            text: &request,
            tools: &definitions,
            continuation: None,
            results: &[],
        };
        let (events, _received) = tokio::sync::mpsc::channel(32);
        let round = client
            .stream_tool_round(
                &tools,
                &ToolCapabilities {
                    function_tools: true,
                },
                &events,
                &CancellationToken::new(),
            )
            .await
            .unwrap();
        assert_eq!(
            round.outcome,
            if kind == "tools" {
                moosuri_connectors::model::protocol::FinishOutcome::ContinuationRequired
            } else {
                moosuri_connectors::model::protocol::FinishOutcome::Incomplete
            }
        );
        // Reproduce the pre-fix driver precisely: acknowledge provider text/calls
        // without a verified terminal marker, then lose the process before use.
        store
            .commit_agent_step(
                &final_step,
                PrivateCheckpoint {
                    bytes: json!({"kind":"mission_round","planning":false,"text":text})
                        .to_string()
                        .into_bytes(),
                },
                if kind == "tools" {
                    vec![sum("legacy-pending")]
                } else {
                    vec![]
                },
            )
            .await
            .unwrap();
        drop(claim);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let runtime = Runtime::start(store.clone());
        let done = tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let view = store.missions(&user).await.unwrap().remove(0);
                if !["queued", "planning", "executing", "verifying"].contains(&view.state.as_str())
                {
                    break view;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(done.state, "unknown", "{kind}");
        assert!(done.uncertain);
        assert!(done.result.is_none());
        assert!(done.inputs.is_empty());
        assert_eq!(
            done.completed_steps, 1,
            "the old pending tool must not execute"
        );
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            2
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        runtime.shutdown().await;
        server.abort();
    }
}
