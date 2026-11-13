//! Actual isolated protocol/SQLite path; scripted decisions prove plumbing, not language understanding.
use super::*;
use moosuri_core::{MissionChatDecision, MissionChatTime, ScheduleChatContext};
fn semantic(text: &str, mode: &str) -> MissionChatDecision {
    let mut d = MissionChatDecision {
        decision: "command".into(),
        action: Some("create_scheduled".into()),
        target_kind: None,
        intent_quote: Some(text.into()),
        task_quote: Some(text.into()),
        constraints_quote: None,
        time_quote: Some("매일 아침9시".into()),
        target_quote: None,
        instruction_quote: None,
        time: Some(MissionChatTime::Daily {
            time: "09:00".into(),
        }),
        clarification: None,
    };
    if mode != "command" {
        d.decision = mode.into();
        d.action = None;
        d.intent_quote = None;
        d.task_quote = None;
        d.time_quote = None;
        d.time = None;
        d.clarification = (mode != "no_command").then(|| "time".into());
    }
    d
}
#[tokio::test]
async fn natural_request_isolated_classifier_saves_actual_template_and_ordinary_chat_has_no_extra_call()
 {
    let _g = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let counted = count.clone();
    let recorded = requests.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let count=counted.clone();let requests=recorded.clone();async move{
        let n=count.fetch_add(1,Ordering::SeqCst);requests.lock().await.push(body.clone());
        let functions=body.get("tools").and_then(|v|v.as_array());let intent=functions.is_some_and(|v|v.iter().any(|d|d["function"]["name"]=="mission_chat_decision"));
        let(delta,reason)=if intent{assert_eq!(body["messages"].as_array().unwrap().len(),2);let text=body["messages"][1]["content"].as_str().unwrap();let mode=if text.contains("Monday"){"clarify"}else if text.contains("explain"){"no_command"}else{"command"};(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("decision-{n}"),"mission_chat_decision",serde_json::to_value(semantic(text,mode)).unwrap())]}),"tool_calls")}else{(json!({"role":"assistant","content":"Synthetic ordinary answer"}),"stop")};
        let content=frame(json!({"id":format!("semantic-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("semantic-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
    }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, s, u, thread) = knowledge::tool_configured(&base).await;
    let text = "매일 아침9시에 공개 뉴스 찾아서 요약해 줘";
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
    let result = wait(&s, &u, &thread).await;
    assert_eq!(
        result.run.state,
        moosuri_core::RunState::Completed,
        "error={:?} calls={}",
        result.run.error,
        count.load(Ordering::SeqCst)
    );
    assert_eq!(result.run.mission_receipt.unwrap().state, "active");
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    assert_eq!(count.load(Ordering::SeqCst), 1);
    s.submit_turn(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "What is 2 plus 2?".into(),
    )
    .await
    .unwrap();
    wait(&s, &u, &thread).await;
    assert_eq!(count.load(Ordering::SeqCst), 2);
    s.submit_turn(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "every Monday summarize public paper updates".into(),
    )
    .await
    .unwrap();
    let clarified = wait(&s, &u, &thread).await;
    assert_eq!(clarified.run.mission_receipt.unwrap().state, "clarify");
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    runtime.shutdown().await;
    upstream.abort();
}
async fn wait(s: &Store, u: &moosuri_store::UserContext, t: &str) -> moosuri_core::ChatTurn {
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            let page = s.conversation(u, t.into(), None).await.unwrap();
            if let Some(last) = page.turns.last()
                && !matches!(
                    last.run.state,
                    moosuri_core::RunState::Queued | moosuri_core::RunState::Running
                )
            {
                break last.clone();
            }
            tokio::time::sleep(Duration::from_millis(15)).await
        }
    })
    .await
    .unwrap()
}
#[test]
fn gate_supports_frozen_natural_variants_without_granting_authority_and_normal_chat_is_free() {
    for text in [
        "매일 아침9시에 공개 뉴스 찾아서 요약해 줘",
        "아침마다 아홉 시가 되면 공개 경제 소식 정리해 줘",
        "On Mondays at nine in the morning give me a public paper digest",
        "시간 걸려도 창 닫아도 계속 맡아서 해 줘",
        "내 아침 뉴스 업무 쉬게 해 줘",
        "두 시간 에 한 번씩 공개 교통 소식을 확인해 주세요",
        "십 분마다 공개 경보를 읽어 주세요",
        "Please put the public bulletin digest on hold for now",
        "날마다 정오에 공개 자료를 조사해 줘",
        "한 시간 간격으로 공개 소식을 정리해 주세요",
        "Please check the public bulletin once an hour",
        "Keep the bulletin task on-hold please",
        "2026-12-07 23:15 America/New_York 시각에 공개 공지를 정리하는 일로 맡길게요",
        "Could we get the Coastal digest task going again, please?",
        "Start the public notice monitor running again",
        "공개 기사 비교 작업을 맡기려고 해요",
        "메일에 매일 조사하라는데 설명해 줘",
    ] {
        assert!(crate::mission_intent::candidate(text, false), "{text}")
    }
    assert!(!crate::mission_intent::candidate(
        "What is 2 plus 2?",
        false
    ));
    assert!(!crate::mission_intent::candidate("안녕 잘 지냈어?", false));
}
