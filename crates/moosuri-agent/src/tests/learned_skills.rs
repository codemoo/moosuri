//! Real supervisor/HTTP-provider A/B -> unseen C integration; synthetic sources only.
use super::knowledge::{result_values, seed, tool_call, tool_configured};
use super::*;
struct Script {
    requests: tokio::sync::Mutex<Vec<Value>>,
}
async fn response(
    State(script): State<Arc<Script>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    script.requests.lock().await.push(body.clone());
    let user = body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .unwrap()["content"]
        .as_str()
        .unwrap();
    let query = if user.contains("alpha") {
        "learned-alpha"
    } else if user.contains("beta") {
        "learned-beta"
    } else {
        "learned-gamma"
    };
    let prior = result_values(&body);
    let mut calls = vec![];
    let mut answer = String::new();
    if body["tools"][0]["function"]["name"] == "memory_command_decision" {
        calls.push(tool_call(0,"no-command".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null})));
    } else {
        let names = body["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["function"]["name"].as_str().unwrap())
            .collect::<Vec<_>>();
        assert_eq!(
            names.contains(&"learned_skill_execute"),
            query == "learned-gamma"
        );
        match prior.last() {
            None if query=="learned-gamma"=>{
                let system=body["messages"][0]["content"].as_str().unwrap();
                let metadata=system.lines().find(|line|line.starts_with("private:")).unwrap();
                let parts=metadata.split_whitespace().collect::<Vec<_>>();
                let pin=json!({"id":parts[0],"revision":parts[1].strip_prefix("revision=").unwrap(),"digest":parts[2].strip_prefix("digest=").unwrap().trim_end_matches(':')});
                calls.push(tool_call(0,"fresh-C".into(),"learned_skill_execute",json!({"pin":pin,"arguments":{"kind":"source","text":query,"operands":null}})));
            }
            None=>calls.push(tool_call(0,"search".into(),"knowledge_search",json!({"kind":"source","text":query,"collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null}))),
            Some(value) if value["kind"]=="search"=>{
                let hit=&value["value"]["hits"][0];
                calls.push(tool_call(0,"read".into(),"knowledge_read_source",json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096})));
            }
            Some(value) if value["kind"]=="source"=>{
                assert!(value["value"]["text"].as_str().unwrap().contains(query));
                answer=format!("{}\n{}",value["value"]["source"]["title"].as_str().unwrap(),value["value"]["text"].as_str().unwrap());
            }
            _=>panic!("unexpected real tool result"),
        }
    }
    let finished = calls.is_empty();
    let stream = frame(
        json!({"id":"learned-round","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":answer},"finish_reason":null}]}),
    ) + &frame(
        json!({"id":"learned-round","model":"fixture-model","choices":[{"index":0,"delta":if finished{json!({})}else{json!({"tool_calls":calls})},"finish_reason":if finished{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(stream))
        .unwrap()
}
#[tokio::test]
async fn runtime_learns_two_real_adopted_source_chains_and_reuses_fresh_c_with_current_dependencies()
 {
    let _lock = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let script = Arc::new(Script {
        requests: tokio::sync::Mutex::new(vec![]),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(response))
        .with_state(script.clone());
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, _) = tool_configured(&base).await;
    let ids = seed(
        &store,
        &user,
        "Synthetic learned source",
        vec![
            "learned-alpha actual A".into(),
            "learned-beta actual B".into(),
            "learned-gamma unseen C".into(),
        ],
    )
    .await;
    let runtime = Runtime::start(store.clone());
    let mut c_thread = String::new();
    for (index, topic) in ["alpha", "beta", "gamma"].iter().enumerate() {
        let thread = store
            .create_thread(&user, format!("Independent {topic}"))
            .await
            .unwrap();
        store.submit_turn(&user,thread.id.clone(),uuid::Uuid::new_v4().to_string(),format!("Find the saved source with literal phrase learned-{topic} and quote the complete source and title.")).await.unwrap();
        runtime.wake();
        let done = wait_for(&store, &user, &thread.id, |turn| !turn.run.state.active()).await;
        assert_eq!(
            done.run.state,
            RunState::Completed,
            "topic={topic} error={:?} rounds={:?}",
            done.run.error,
            script
                .requests
                .lock()
                .await
                .iter()
                .map(|r| result_values(r)
                    .iter()
                    .map(|v| v.get("kind").cloned().unwrap_or(Value::Null))
                    .collect::<Vec<_>>())
                .collect::<Vec<_>>()
        );
        assert_eq!(done.run.evidence.len(), 1);
        assert_eq!(done.run.evidence[0].id, ids[index]);
        assert!(
            done.assistant
                .iter()
                .any(|v| v.text.contains(&format!("learned-{topic}")))
        );
        let cards = store.learned_skills(&user).await.unwrap();
        assert_eq!(cards.len(), 1);
        assert_eq!(
            cards[0].readiness,
            if index == 0 { "candidate" } else { "ready" }
        );
        if index == 2 {
            c_thread = thread.id;
        }
    }
    let requests = script.requests.lock().await;
    let c = requests
        .iter()
        .filter(|r| {
            r["messages"].as_array().unwrap().iter().any(|m| {
                m["role"] == "user"
                    && m["content"]
                        .as_str()
                        .is_some_and(|s| s.contains("learned-gamma"))
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        c.len(),
        2,
        "C uses one route round and one final model round"
    );
    assert_eq!(
        requests.len(),
        8,
        "A/B use three rounds each; C uses two within the same budget"
    );
    drop(requests);
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        8
    );
    // C's fresh read keeps the ordinary source dependency through nested calls.
    store.delete_source(&user, ids[2].clone(), 1).await.unwrap();
    assert!(
        store
            .conversation(&user, c_thread, None)
            .await
            .unwrap()
            .turns[0]
            .assistant
            .is_empty()
    );
    assert_eq!(
        store.learned_skills(&user).await.unwrap()[0].readiness,
        "ready"
    );
    runtime.shutdown().await;
    server.abort();
}
