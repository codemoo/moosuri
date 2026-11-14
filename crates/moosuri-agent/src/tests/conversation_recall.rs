//! Real Runtime discovery/read/answer and destructive provenance, synthetic only.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
use moosuri_store::{KnowledgeToolInput, OutputPatch, PendingKnowledgeCall, PrivateCheckpoint};
struct RecallScript {
    calls: AtomicUsize,
    source_id: tokio::sync::Mutex<String>,
}
async fn response(
    State(state): State<Arc<RecallScript>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    state.calls.fetch_add(1, Ordering::SeqCst);
    let encoded = body.to_string();
    assert!(!encoded.contains("FOREIGN_RECALL_SECRET"));
    let names = body["tools"].as_array().unwrap();
    if names[0]["function"]["name"] == "memory_command_decision" {
        assert_eq!(body["messages"].as_array().unwrap().len(), 2);
        let call = tool_call(
            0,
            "recall-is-no-memory-write".into(),
            "memory_command_decision",
            json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}),
        );
        let stream = frame(
            json!({"id":"recall-plan","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[call]},"finish_reason":null}]}),
        ) + &frame(
            json!({"id":"recall-plan","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
        ) + "data: [DONE]\n\n";
        return Response::builder()
            .header("content-type", "text/event-stream")
            .body(Body::from(stream))
            .unwrap();
    }
    assert!(
        names
            .iter()
            .any(|d| d["function"]["name"] == "conversation_search")
    );
    assert!(
        names
            .iter()
            .any(|d| d["function"]["name"] == "conversation_read")
    );
    let results = result_values(&body);
    let mut calls = vec![];
    let mut answer = None;
    if body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap()
        .contains("text descendant")
    {
        answer = Some("Synthetic text-only descendant of recalled evidence".to_string());
    } else {
        match results.len() {
            0 => calls.push(tool_call(
                0,
                "local-history".into(),
                "conversation_search",
                json!({"query":"recall-anchor","scope":"current_thread"}),
            )),
            1 => {
                let v = &results[0];
                assert_eq!(v["kind"], "conversation_search");
                let hits = v["value"]["hits"].as_array().unwrap();
                assert_eq!(hits.len(), 1);
                assert_eq!(hits[0]["turn_id"], *state.source_id.lock().await);
                assert_eq!(
                    hits[0]["user_excerpt"],
                    "recall-anchor original user assertion"
                );
                calls.push(tool_call(
                    0,
                    "owned-history".into(),
                    "conversation_search",
                    json!({"query":"recall-anchor","scope":"owned_conversations"}),
                ));
            }
            2 => {
                let hits = results[1]["value"]["hits"].as_array().unwrap();
                assert!(!hits.is_empty());
                let id = state.source_id.lock().await;
                let first = hits.iter().find(|h| h["turn_id"] == *id).unwrap();
                calls.push(tool_call(
                    0,
                    "whole-turn".into(),
                    "conversation_read",
                    json!({"turn_id":first["turn_id"],"revision":first["revision"]}),
                ));
            }
            3 => {
                let v = &results[2];
                assert_eq!(v["kind"], "conversation_turn");
                assert_eq!(v["value"]["messages"][0]["role"], "user");
                assert_eq!(v["value"]["messages"][1]["role"], "assistant");
                assert!(v["value"]["created_at"].as_i64().unwrap() > 0);
                assert_eq!(
                    v["value"]["messages"][1]["text"],
                    "recall-anchor assistant inference from saved source"
                );
                answer=Some("Historical user assertion and assistant inference were different; recalled synthetic evidence.".into());
            }
            _ => panic!("unexpected recall continuation"),
        }
    }
    let id = "recall-fixture";
    let final_round = answer.is_some();
    let mut s = frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":answer.unwrap_or_else(||"Checking earlier conversation evidence.".into())},"finish_reason":null}]}),
    );
    if !final_round {
        s += &frame(
            json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":null}]}),
        );
    }
    s += &frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":if final_round{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
    );
    s += "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(s))
        .unwrap()
}
async fn synthetic_turn(
    s: &Store,
    u: &UserContext,
    t: &str,
    user: &str,
    answer: &str,
    source: Option<&str>,
) -> ChatTurn {
    let turn = s
        .submit_turn(u, t.into(), uuid::Uuid::new_v4().to_string(), user.into())
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(claim.lease.run_id(), turn.run.id);
    let mut seq = claim.context.checkpoint_seq;
    if let Some(id) = source {
        let step = s.begin_agent_step(&claim.lease, seq).await.unwrap();
        s.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "source".into(),
                input: KnowledgeToolInput::ReadSource {
                    id: id.into(),
                    revision: 1,
                    start_byte: 0,
                    max_bytes: 4096,
                },
            }],
        )
        .await
        .unwrap();
        s.execute_knowledge_tool(&step, "source".into())
            .await
            .unwrap();
        seq = s.agent_context(&claim.lease).await.unwrap().checkpoint_seq;
    }
    let step = s.begin_agent_step(&claim.lease, seq).await.unwrap();
    s.append_agent_output(
        &step,
        vec![OutputPatch::Append {
            item: "answer".into(),
            text: answer.into(),
            phase: Some(moosuri_core::MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )
    .await
    .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![],
    )
    .await
    .unwrap();
    s.finish_agent_run(&step, RunState::Completed, None)
        .await
        .unwrap();
    turn
}
#[tokio::test]
async fn runtime_recall_after_130_turns_revokes_unread_candidate_and_upstream_context_without_resurrection()
 {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(RecallScript {
        calls: AtomicUsize::new(0),
        source_id: tokio::sync::Mutex::new(String::new()),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(response))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, s, u, thread) = tool_configured(&base).await;
    let saved = knowledge::seed(
        &s,
        &u,
        "Synthetic recalled upstream",
        vec!["Synthetic upstream assertion".into()],
    )
    .await
    .remove(0);
    let first = synthetic_turn(
        &s,
        &u,
        &thread,
        "recall-anchor original user assertion",
        "recall-anchor assistant inference from saved source",
        Some(&saved),
    )
    .await;
    *state.source_id.lock().await = first.run.id;
    for i in 0..130 {
        synthetic_turn(
            &s,
            &u,
            &thread,
            &format!("padding user {i}"),
            "unrelated synthetic padding",
            None,
        )
        .await;
    }
    let other = s
        .create_thread(&u, "unread candidate".into())
        .await
        .unwrap();
    synthetic_turn(
        &s,
        &u,
        &other.id,
        "recall-anchor unread original user",
        "recall-anchor unread assistant",
        None,
    )
    .await;
    let invite = s
        .create_user(
            &u,
            "Synthetic foreign".into(),
            "foreign-recall@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = s
        .login(
            "foreign-recall@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap();
    let foreign = s
        .change_password(&restricted.user, PASSWORD.into())
        .await
        .unwrap()
        .user;
    let ft = s
        .create_thread(&foreign, "Foreign decoy".into())
        .await
        .unwrap();
    // Foreign account has an explicitly shared fixture model, but its history stays private.
    synthetic_turn(
        &s,
        &foreign,
        &ft.id,
        "recall-anchor FOREIGN_RECALL_SECRET",
        "FOREIGN_RECALL_SECRET",
        None,
    )
    .await;
    let runtime = Runtime::start(s.clone());
    let ask = |s: Store, u: UserContext, t: String| async move {
        s.submit_turn(
            &u,
            t,
            uuid::Uuid::new_v4().to_string(),
            "Use recall-anchor earlier conversations to compare the original statement and answer"
                .into(),
        )
        .await
        .unwrap()
    };
    ask(s.clone(), u.clone(), thread.clone()).await;
    runtime.wake();
    let done = wait_for(&s, &u, &thread, |t| !t.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert!(text(&done).contains("Historical user assertion"));
    assert_eq!(state.calls.load(Ordering::SeqCst), 5);
    s.delete_thread(&u, other.id).await.unwrap();
    let page = s.conversation(&u, thread.clone(), None).await.unwrap();
    let last = page.turns.last().unwrap();
    assert!(last.assistant.is_empty() && !last.run.can_retry);
    assert!(s.retry_run(&u, done.run.id, 1).await.is_err());
    ask(s.clone(), u.clone(), thread.clone()).await;
    runtime.wake();
    let second = wait_for(&s, &u, &thread, |t| !t.run.state.active()).await;
    assert_eq!(second.run.state, RunState::Completed);
    s.submit_turn(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "text descendant followup".into(),
    )
    .await
    .unwrap();
    runtime.wake();
    let descendant = wait_for(&s, &u, &thread, |t| !t.run.state.active()).await;
    assert_eq!(descendant.run.state, RunState::Completed);
    s.delete_source(&u, saved, 1).await.unwrap();
    let page = s.conversation(&u, thread.clone(), None).await.unwrap();
    for id in [&second.run.id, &descendant.run.id] {
        let turn = page.turns.iter().find(|t| &t.run.id == id).unwrap();
        assert!(turn.assistant.is_empty() && !turn.run.can_retry);
        assert!(s.retry_run(&u, id.clone(), 1).await.is_err());
    }
    runtime.shutdown().await;
    drop(runtime);
    drop(s);
    let reopened = Store::open(dir.path()).unwrap();
    assert!(
        reopened
            .conversation(&u, thread, None)
            .await
            .unwrap()
            .turns
            .iter()
            .rev()
            .take(3)
            .all(|t| t.assistant.is_empty())
    );
    server.abort();
}
