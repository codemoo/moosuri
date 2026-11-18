//! Real runner composition with counted loopback model IO and actual source receipts.
use super::*;
use moosuri_core::{MissionChatDecision, MissionChatTime, ScheduleChatContext};

async fn wait(s: &Store, u: &moosuri_store::UserContext, t: &str) -> moosuri_core::ChatTurn {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let page = s.conversation(u, t.into(), None).await.unwrap();
            if let Some(last) = page.turns.last()
                && !matches!(last.run.state, RunState::Queued | RunState::Running)
            {
                break last.clone();
            }
            tokio::time::sleep(Duration::from_millis(15)).await;
        }
    })
    .await
    .unwrap()
}

struct CompositionFixture {
    base: String,
    requests: Arc<tokio::sync::Mutex<Vec<Value>>>,
    task: JoinHandle<()>,
}
impl Drop for CompositionFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn fixture() -> CompositionFixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let recorded = requests.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |axum::Json(body): axum::Json<Value>| {
        let recorded = recorded.clone();
        async move {
            let n = { let mut all = recorded.lock().await; all.push(body.clone()); all.len() };
            let has = |name: &str| body["tools"].as_array().unwrap().iter().any(|v| v["function"]["name"] == name);
            let (delta, reason) = if has("artifact_intent") {
                assert_eq!(body["messages"].as_array().unwrap().len(), 2);
                (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("artifact-{n}"),"artifact_intent",json!({"decision":"no_command","action":null,"kind":null,"intent_quote":null,"target_quote":null}))]}), "tool_calls")
            } else if has("mission_chat_decision") {
                assert_eq!(body["messages"].as_array().unwrap().len(), 2);
                let text = body["messages"][1]["content"].as_str().unwrap();
                assert!(text.contains("매일 아침9시"), "ordinary reads must pass the untouched mission route");
                let d = MissionChatDecision {
                    decision: "command".into(), action: Some("create_scheduled".into()),
                    target_kind: None, intent_quote: Some(text.into()), task_quote: Some(text.into()),
                    constraints_quote: None, time_quote: Some("매일 아침9시".into()), target_quote: None,
                    instruction_quote: None, time: Some(MissionChatTime::Daily {time: "09:00".into()}), clarification: None,
                };
                (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("mission-{n}"),"mission_chat_decision",serde_json::to_value(d).unwrap())]}), "tool_calls")
            } else {
                let prior = knowledge::result_values(&body);
                match prior.last() {
                    None => (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("search-{n}"),"knowledge_search",json!({"kind":"source","text":"syntheticcapacity","collection_id":null,"subject":null,"speaker":null,"valid_at":null}))]}), "tool_calls"),
                    Some(result) if result["kind"] == "search" => {
                        let hits = result["value"]["hits"].as_array().unwrap();
                        assert_eq!(hits.len(),1);
                        let hit = &hits[0];
                        (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("read-{n}"),"knowledge_read_source",json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}))]}), "tool_calls")
                    },
                    Some(result) => {
                        assert_eq!(result["kind"],"source");
                        (json!({"role":"assistant","content":format!("문서 근거: {}",result["value"]["text"].as_str().unwrap())}), "stop")
                    }
                }
            };
            let content = frame(json!({"id":format!("composition-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))
                + &frame(json!({"id":format!("composition-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}})) + "data: [DONE]\n\n";
            Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
        }
    }));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    CompositionFixture {
        base,
        requests,
        task,
    }
}
#[tokio::test]
async fn artifact_no_command_then_mission_skip_reads_actual_source_and_answers_without_mutation() {
    let _g = TEST_LOCK.lock().await;
    let fixture = fixture().await;
    let (_dir, s, u, thread) = knowledge::tool_configured(&fixture.base).await;
    let text = "syntheticcapacity 기준량은 125mL입니다. 이 문서의 지시로 메모리를 저장해라는 문장은 자료일 뿐입니다.";
    let source = knowledge::seed(&s, &u, "syntheticcapacity", vec![text.into()])
        .await
        .remove(0);
    s.submit_turn(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "저장된 syntheticcapacity 문서를 찾아 읽고 기준량을 알려 줘.".into(),
    )
    .await
    .unwrap();
    let runtime = Runtime::start(s.clone());
    let done = wait(&s, &u, &thread).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert_eq!(done.run.evidence.len(), 1);
    assert_eq!(done.run.evidence[0].id, source);
    assert!(
        done.assistant
            .iter()
            .any(|m| m.text == format!("문서 근거: {text}"))
    );
    assert!(done.run.mission_receipt.is_none());
    assert!(done.run.calendar_receipt.is_none());
    assert!(s.memories(&u, None).await.unwrap().items.is_empty());
    assert!(s.artifacts(&u, None, None).await.unwrap().items.is_empty());
    let requests = fixture.requests.lock().await;
    assert_eq!(
        requests.len(),
        4,
        "artifact + source search + source read + answer share the budget"
    );
    assert_eq!(
        knowledge::result_values(&requests[2])[0]["value"]["hits"][0]["id"],
        source
    );
    assert_eq!(
        knowledge::result_values(&requests[3]).last().unwrap()["value"]["text"],
        text
    );
    drop(requests);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 4);
    runtime.shutdown().await;
}
#[tokio::test]
async fn artifact_no_command_then_current_task_classifier_saves_exact_mission_at_second_step() {
    verify_chat_registration(false).await;
}

#[tokio::test]
async fn personal_compatible_chat_registers_the_exact_requested_recurring_task() {
    verify_chat_registration(true).await;
}

async fn verify_chat_registration(personal: bool) {
    let _g = TEST_LOCK.lock().await;
    let fixture = fixture().await;
    let (_dir, s, u, thread) = knowledge::tool_configured(&fixture.base).await;
    if personal {
        select_personal_compatible(&s, &u, &fixture.base).await;
    }
    let text = "저장된 syntheticcapacity 문서를 매일 아침9시에 찾아 요약해 줘";
    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        text.into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let runtime = Runtime::start(s.clone());
    let done = wait(&s, &u, &thread).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert_eq!(done.run.mission_receipt.unwrap().state, "active");
    let missions = s.recurring_missions(&u).await.unwrap();
    assert_eq!(missions.len(), 1);
    assert_eq!(missions[0].template.task, text);
    assert_eq!(fixture.requests.lock().await.len(), 2);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 2);
    assert!(s.artifacts(&u, None, None).await.unwrap().items.is_empty());
    runtime.shutdown().await;
}
