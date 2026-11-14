//! Actual Runtime A/B -> unseen C, explicit DAG edges and structural-only sharing.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
fn graph() -> Value {
    json!({"input_count":5,"steps":[
     {"operation":"product","operands":[{"kind":"input","index":0},{"kind":"input","index":1}]},
     {"operation":"product","operands":[{"kind":"input","index":2},{"kind":"input","index":3}]},
     {"operation":"sum","operands":[{"kind":"result","index":0},{"kind":"result","index":1}]},
     {"operation":"product","operands":[{"kind":"result","index":2},{"kind":"input","index":4}]}
    ]})
}
struct Compound {
    requests: tokio::sync::Mutex<Vec<Value>>,
}
async fn response(
    State(state): State<Arc<Compound>>,
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
    let (inputs, answer) = if prompt.contains("batch-A") {
        (json!(["123.45", "2", "3", "10", "0.9"]), "249.21")
    } else if prompt.contains("batch-B") {
        (json!(["4", "5.25", "2", "7.5", "0.8"]), "28.8")
    } else {
        (json!(["6", "1.25", "3", "4.50", "1.1"]), "23.1")
    };
    let previous = result_values(&body);
    let mut calls = vec![];
    let mut text = String::new();
    if body["tools"][0]["function"]["name"] == "memory_command_decision" {
        calls.push(tool_call(0,"no-command".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null})));
    } else if previous.is_empty() {
        if prompt.contains("batch-C") {
            let metadata = body["messages"][0]["content"]
                .as_str()
                .unwrap()
                .lines()
                .find(|s| s.starts_with("private:"))
                .unwrap()
                .split_whitespace()
                .collect::<Vec<_>>();
            let pin = json!({"id":metadata[0],"revision":metadata[1].strip_prefix("revision=").unwrap(),"digest":metadata[2].strip_prefix("digest=").unwrap().trim_end_matches(':')});
            calls.push(tool_call(0,"fresh-workflow".into(),"learned_skill_execute",json!({"pin":pin,"arguments":{"kind":"workflow","text":null,"operands":null,"inputs":inputs}})));
        } else {
            calls.push(tool_call(
                0,
                "workflow".into(),
                "decimal_workflow",
                json!({"graph":graph(),"inputs":inputs}),
            ));
        }
    } else {
        let value = previous.last().unwrap();
        assert_eq!(value["kind"], "decimal_workflow");
        assert_eq!(value["value"]["results"].as_array().unwrap().len(), 4);
        assert_eq!(value["value"]["exact"], answer);
        text = format!("Exact current total: {answer}");
    }
    let done = calls.is_empty();
    let body = frame(
        json!({"id":"compound-workflow","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}]}),
    ) + &frame(
        json!({"id":"compound-workflow","model":"fixture-model","choices":[{"index":0,"delta":if done{json!({})}else{json!({"tool_calls":calls})},"finish_reason":if done{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(body))
        .unwrap()
}
#[tokio::test]
async fn runtime_learns_explicit_compound_graph_from_two_adopted_inputs_reuses_fresh_c_and_shares_no_trace()
 {
    let _lock = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(Compound {
        requests: tokio::sync::Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(response))
        .with_state(state.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, _) = tool_configured(&base).await;
    let runtime = Runtime::start(store.clone());
    let mut threads = vec![];
    for (label, values, expected) in [
        ("A", "123.45, 2, 3, 10, 0.9", "249.21"),
        ("B", "4, 5.25, 2, 7.5, 0.8", "28.8"),
        ("C", "6, 1.25, 3, 4.50, 1.1", "23.1"),
    ] {
        let thread = store
            .create_thread(&user, format!("PRIVATE_COMPOUND_TRACE_{label}"))
            .await
            .unwrap();
        store.submit_turn(&user,thread.id.clone(),uuid::Uuid::new_v4().to_string(),format!("batch-{label} PRIVATE_COMPOUND_TRACE: two line item products, sum, then multiply the total by the given final factor. Fresh input slots: {values}. Calculate the exact final amount.")).await.unwrap();
        runtime.wake();
        let done = wait_for(&store, &user, &thread.id, |t| !t.run.state.active()).await;
        assert_eq!(
            done.run.state,
            moosuri_core::RunState::Completed,
            "{} {:?}",
            label,
            done.run.error
        );
        assert!(done.assistant.iter().any(|m| m.text.contains(expected)));
        threads.push(thread.id);
        let cards = store.learned_skills(&user).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].readiness,
            if label == "A" { "candidate" } else { "ready" }
        );
    }
    let requests = state.requests.lock().await;
    let c = requests
        .iter()
        .filter(|v| {
            v["messages"].as_array().unwrap().iter().any(|m| {
                m["role"] == "user" && m["content"].as_str().is_some_and(|t| t.contains("batch-C"))
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(c.len(), 2);
    for request in c {
        let bytes = request.to_string();
        assert!(!bytes.contains("123.45"));
        assert!(!bytes.contains("5.25"));
    }
    drop(requests);
    let card = store.learned_skills(&user).await.unwrap().remove(0);
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
    let exported = serde_json::to_string(&preview.files).unwrap();
    for private in [
        "123.45",
        "5.25",
        "PRIVATE_COMPOUND_TRACE",
        user.account().id.as_str(),
        threads[0].as_str(),
        threads[1].as_str(),
    ] {
        assert!(!exported.contains(private));
    }
    assert!(exported.contains("decimal_workflow"));
    assert!(exported.contains("repository-owned synthetic"));
    moosuri_store::skill_author::check(&preview.slug, preview.files).unwrap();
    // Withdrawing B leaves one independent proof and revokes C's actual use.
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
    runtime.shutdown().await;
    server.abort();
}
