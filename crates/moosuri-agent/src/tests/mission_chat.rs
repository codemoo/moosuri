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
        d.clarification = (mode != "no_command").then(|| {
            if mode == "unsupported" {
                "unsupported"
            } else {
                "time"
            }
            .into()
        });
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
        let(delta,reason)=if intent{assert_eq!(body["messages"].as_array().unwrap().len(),2);let text=body["messages"][1]["content"].as_str().unwrap();let mode=if text.contains("택시"){"unsupported"}else if text.contains("Monday"){"clarify"}else if text.contains("explain") || text.contains("답변") || text.contains("설명") || text=="한국어 인사말 세 가지 부탁해."{"no_command"}else{"command"};(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("decision-{n}"),"mission_chat_decision",serde_json::to_value(semantic(text,mode)).unwrap())]}),"tool_calls")}else{(json!({"role":"assistant","content":"Synthetic ordinary answer"}),"stop")};
        let content=frame(json!({"id":format!("semantic-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("semantic-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
    }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, s, u, thread) = knowledge::tool_configured(&base).await;
    let text = "매일 아침9시에 '기억해 줘' 관련 공개 뉴스 찾아서 요약해 줘";
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
    for text in [
        "두 번째 답변을 이어줘",
        "이어서 설명해 줘",
        "방금 말한 두 가지를 한 문장으로 이어서 설명해 줘.",
        "앞서 말한 내용을 짧게 이어서 설명해 주세요.",
        "산책이 몸과 기분에 도움이 되는 이유를 알려줘.",
        "이 성분에 대해 설명해 줘.",
        "수업 시간에 집중하는 방법을 설명해 줘.",
        "업무 시간에 쉬는 방법을 알려줘.",
    ] {
        let before = count.load(Ordering::SeqCst);
        s.submit_turn(
            &u,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            text.into(),
        )
        .await
        .unwrap();
        let ordinary = wait(&s, &u, &thread).await;
        assert_eq!(ordinary.run.state, moosuri_core::RunState::Completed);
        assert!(ordinary.run.mission_receipt.is_none());
        assert_eq!(count.load(Ordering::SeqCst), before + 1, "{text}");
        assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    }
    s.submit_turn(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "What is 2 plus 2?".into(),
    )
    .await
    .unwrap();
    wait(&s, &u, &thread).await;
    assert_eq!(count.load(Ordering::SeqCst), 10);
    // A polite foreground request after a real saved-task receipt still needs
    // only its answer call, and must not create or control another mission.
    s.submit_turn(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "한국어 인사말 세 가지 부탁해.".into(),
    )
    .await
    .unwrap();
    let ordinary = wait(&s, &u, &thread).await;
    assert_eq!(ordinary.run.state, moosuri_core::RunState::Completed);
    assert!(ordinary.run.mission_receipt.is_none());
    assert_eq!(count.load(Ordering::SeqCst), 11);
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
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
    assert_eq!(count.load(Ordering::SeqCst), 12);
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "내일 오전 8시에 택시를 자동으로 예약해 줘.".into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let unsupported = wait(&s, &u, &thread).await;
    assert_eq!(unsupported.run.state, moosuri_core::RunState::Completed);
    assert!(unsupported.run.error.is_none());
    assert_eq!(
        unsupported.run.mission_receipt.unwrap().state,
        "unsupported"
    );
    assert!(unsupported.assistant.iter().any(|m| !m.text.is_empty()));
    assert_eq!(count.load(Ordering::SeqCst), 13);
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    assert!(s.missions(&u).await.unwrap().is_empty());
    assert!(
        s.schedules(&u)
            .await
            .unwrap()
            .iter()
            .all(|v| v.title != "택시")
    );
    let requests = requests.lock().await;
    let schema = requests.last().unwrap()["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["function"]["name"] == "mission_chat_decision")
        .unwrap();
    let codes = schema["function"]["parameters"]["properties"]["clarification"]["enum"]
        .as_array()
        .unwrap();
    assert!(codes.contains(&json!("unsupported")));
    assert!(codes.contains(&Value::Null));
    assert!(!codes.contains(&json!("택시 자동 예약은 지원되지 않습니다.")));
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
        "매일 아침9시에 공개 뉴스 요약 부탁해",
        "매일 아침9시에 '기억해 줘' 관련 공개 뉴스를 찾아서 요약해 줘",
        "매일 아침9시에 기억해 줘 관련 공개 뉴스를 찾아서 요약해 줘",
        "1분 뒤에 저장한 자료를 읽고 남은 준비를 정리해서 알려 줘.",
        "두 시간 후에 저장한 자료를 읽어서 요약해 줘",
        "In 1 minute read my saved notes and summarize them",
        "시간 걸려도 맡아서 공개 기사 조사 부탁해",
        "아침마다 아홉 시가 되면 공개 경제 소식 정리해 줘",
        "아침에 공개 뉴스를 요약해 줘",
        "아침9시에 공개 뉴스를 요약해 줘",
        "아침 아홉 시에 공개 뉴스를 요약해 줘",
        "내일 아침에 공개 뉴스를 요약해 줘",
        "On Mondays at nine in the morning give me a public paper digest",
        "시간 걸려도 창 닫아도 계속 맡아서 해 줘",
        "내 아침 뉴스 업무 쉬게 해 줘",
        "두 시간 에 한 번씩 공개 교통 소식을 확인해 주세요",
        "십 분마다 공개 경보를 읽어 주세요",
        "2시간에 1번 공개 뉴스를 확인해 줘",
        "십 분에 한 번 공개 경보를 읽어 주세요",
        "20분에 한 번 공지를 확인해 줘",
        "스물한 분에 한 번 공지를 확인해 줘",
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
        "그 업무 이어줘",
        "공개 뉴스 비교 작업을 계속해 줘",
        "그 일 이어서 해 줘",
        "Coastal digest 이어줘",
        "고객 답변 조사 이어줘",
        "방금 말한 두 가지 업무를 이어서 설명해 줘",
        "앞서 말한 내용을 짧게 이어서 설명하고 내일 업무로 맡아 줘",
        "계속해 줘",
    ] {
        assert!(crate::mission_intent::candidate(text, false), "{text}")
    }
    assert!(!crate::mission_intent::candidate(
        "What is 2 plus 2?",
        false
    ));
    assert!(!crate::mission_intent::candidate("안녕 잘 지냈어?", false));
    for text in [
        "아침 운동에 대한 기억만 지워 줘.",
        "아침 운동은 산책이야. 기억해 줘.",
        "두 번째 답변을 이어줘",
        "이어서 설명해 줘",
        "방금 말한 두 가지를 한 문장으로 이어서 설명해 줘.",
        "앞서 말한 내용을 짧게 이어서 설명해 주세요.",
        "산책이 몸과 기분에 도움이 되는 이유를 알려줘.",
        "이 성분에 대해 설명해 줘.",
        "수업 시간에 집중하는 방법을 설명해 줘.",
        "업무 시간에 쉬는 방법을 알려줘.",
    ] {
        assert!(!crate::mission_intent::candidate(text, false), "{text}");
    }
    assert!(crate::mission_intent::candidate(
        "그 답변 업무 이어줘",
        false
    ));
    assert!(crate::mission_intent::candidate("답변을 이어줘", true));
    assert!(crate::mission_intent::candidate(
        "내일 답변을 이어서 작성해 줘",
        false
    ));
}

// A scripted decision exercises routing/storage only, not real-model language quality.
#[tokio::test]
async fn delayed_saved_read_schedules_future_work_without_early_reads_or_literal_reminder_models() {
    let _g = TEST_LOCK.lock().await;
    const TASK: &str = "저장한 ‘자작 회의 준비’ 자료를 읽고, 끝난 검토와 남은 준비를 정리해서";
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |axum::Json(body): axum::Json<Value>| {
        let count = counted.clone();
        async move {
            let n = count.fetch_add(1, Ordering::SeqCst);
            let intent = body["tools"].as_array().is_some_and(|tools| tools.iter().any(|tool| tool["function"]["name"] == "mission_chat_decision"));
            let (delta, reason) = if intent {
                assert_eq!(body["messages"].as_array().unwrap().len(), 2);
                let text = body["messages"][1]["content"].as_str().unwrap();
                let noncommand = text.contains("하지 마") || text.starts_with('“');
                let mut decision = semantic(text, if noncommand { "no_command" } else { "command" });
                if !noncommand {
                    decision.task_quote = Some(TASK.into());
                    decision.time_quote = Some("1분 뒤".into());
                    decision.time = Some(MissionChatTime::Relative { seconds: 60 });
                }
                // Deliberately propose a reduced read for the email request too;
                // Store must retain/reject the owner's external-delivery effect.
                (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("delay-{n}"),"mission_chat_decision",serde_json::to_value(decision).unwrap())]}), "tool_calls")
            } else {
                (json!({"role":"assistant","content":"Synthetic ordinary answer"}), "stop")
            };
            let content = frame(json!({"id":format!("delay-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}})) + "data: [DONE]\n\n";
            Response::builder().header("content-type", "text/event-stream").body(Body::from(content)).unwrap()
        }
    }));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, s, u, thread) = knowledge::tool_configured(&base).await;
    let admitted = s
        .submit_turn_with_schedule_context(
            &u,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            format!("1분 뒤에 {TASK} 알려 줘."),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(admitted.run.state, RunState::Queued);
    assert!(admitted.run.schedule_receipts.is_none());
    let runtime = Runtime::start(s.clone());
    let saved = wait(&s, &u, &thread).await;
    assert_eq!(
        saved.run.state,
        RunState::Completed,
        "{:?}",
        saved.run.error
    );
    assert_eq!(saved.run.mission_receipt.unwrap().state, "active");
    assert!(saved.run.schedule_receipts.is_none());
    let scheduled = s.recurring_missions(&u).await.unwrap();
    assert_eq!(scheduled.len(), 1);
    assert_eq!(scheduled[0].template.task, TASK);
    let moosuri_core::ScheduleRule::Once { at } = scheduled[0].schedule.rule else {
        panic!("one future occurrence required")
    };
    assert!(at >= admitted.run.created_at + 60);
    assert!(at <= saved.run.created_at + 61);
    assert!(scheduled[0].recent.is_empty());
    assert_eq!(s.poll_recurring_missions_at(at - 1).await.unwrap(), 0);
    assert!(
        s.missions(&u).await.unwrap().is_empty(),
        "no early read execution"
    );
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "only the isolated decision, no read/answer call"
    );

    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "1분 뒤 약 먹으라고 알려줘".into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let literal = wait(&s, &u, &thread).await;
    assert!(literal.run.mission_receipt.is_none());
    assert_eq!(literal.run.schedule_receipts.unwrap().len(), 1);
    assert_eq!(
        count.load(Ordering::SeqCst),
        1,
        "literal reminders stay deterministic"
    );

    for text in [
        "1분 뒤에 자료를 읽고 요약하는 일은 하지 마.",
        "“1분 뒤에 자료를 읽고 요약해서 알려줘”를 번역해 줘.",
    ] {
        s.submit_turn(
            &u,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            text.into(),
        )
        .await
        .unwrap();
        let ordinary = wait(&s, &u, &thread).await;
        assert_eq!(ordinary.run.state, RunState::Completed);
        assert!(ordinary.run.mission_receipt.is_none(), "{text}");
        assert!(ordinary.run.schedule_receipts.is_none(), "{text}");
    }
    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        format!("1분 뒤에 {TASK} 예시@example.test로 이메일 보내 줘."),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let unsupported = wait(&s, &u, &thread).await;
    assert_eq!(unsupported.run.state, RunState::Completed);
    assert_eq!(
        unsupported.run.mission_receipt.unwrap().state,
        "unsupported"
    );
    assert!(unsupported.run.schedule_receipts.is_none());
    assert_eq!(
        s.recurring_missions(&u).await.unwrap().len(),
        1,
        "no reduced email task saved"
    );
    assert!(s.missions(&u).await.unwrap().is_empty());
    assert!(s.mail_actions(&u).await.unwrap().is_empty());
    assert!(s.calendar_actions(&u).await.unwrap().is_empty());
    assert!(s.next_mail_dispatch().await.unwrap().is_none());
    assert_eq!(count.load(Ordering::SeqCst), 6);
    runtime.shutdown().await;
    upstream.abort();
}

#[tokio::test]
async fn calendar_domain_uses_same_single_current_intent_round_and_preserves_literal_reminder_precedence()
 {
    let _g = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let count=counted.clone();async move{
        let n=count.fetch_add(1,Ordering::SeqCst);assert_eq!(body["messages"].as_array().unwrap().len(),2);let functions=body["tools"].as_array().unwrap();assert!(functions.iter().any(|f|f["function"]["name"]=="mission_chat_decision"));assert!(functions.iter().any(|f|f["function"]["name"]=="calendar_chat_decision"));let text=body["messages"][1]["content"].as_str().unwrap();
        let d=moosuri_core::CalendarChatDecision{domain:"calendar".into(),decision:"clarify".into(),operation:None,connection_quote:None,intent_quote:None,title_quote:None,target_quote:None,date_quote:None,date:None,move_date_quote:None,move_date:None,all_day_quote:None,last_date_quote:None,last_date:None,start_quote:None,start:None,end_quote:None,end:None,duration_quote:None,duration_minutes:None,clarification:Some("time".into())};assert!(text.contains("meeting"));
        let delta=json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("calendar-decision-{n}"),"calendar_chat_decision",serde_json::to_value(d).unwrap())]});let content=frame(json!({"id":format!("cal-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("cal-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
    }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, s, u, t) = knowledge::tool_configured(&base).await;
    s.submit_turn_with_schedule_context(
        &u,
        t.clone(),
        uuid::Uuid::new_v4().to_string(),
        "Create a meeting tomorrow".into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let runtime = Runtime::start(s.clone());
    let result = wait(&s, &u, &t).await;
    assert_eq!(result.run.state, moosuri_core::RunState::Completed);
    assert_eq!(result.run.calendar_receipt.unwrap().state, "clarify");
    assert!(result.run.mission_receipt.is_none());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    s.submit_turn_with_schedule_context(
        &u,
        t.clone(),
        uuid::Uuid::new_v4().to_string(),
        "내일 오전9시에 회의 등록하라고 알려줘".into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let reminder = wait(&s, &u, &t).await;
    assert!(reminder.run.calendar_receipt.is_none());
    assert_eq!(count.load(Ordering::SeqCst), 1);
    runtime.shutdown().await;
    upstream.abort();
}

#[tokio::test]
async fn request_continuation_runtime_clock_answer_saves_original_weekly_task_and_controls_actual_receipt()
 {
    let _g = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let initial = "every Monday summarize public paper updates";
    let complete = format!("{initial}\nat 9AM");
    let expected = complete.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{
        let count=counted.clone();let complete=expected.clone();async move {
        let n=count.fetch_add(1,Ordering::SeqCst);assert!(n<3,"exactly one interpretation per explicit turn");
        assert_eq!(body["messages"].as_array().unwrap().len(),2,"arbitrary history must never enter interpreter");
        let text=body["messages"][1]["content"].as_str().unwrap();
        let mut d=semantic(initial,"command");d.task_quote=Some("summarize public paper updates".into());
        if n==0 {assert_eq!(text,initial);d.decision="clarify".into();d.time=None;d.time_quote=None;d.clarification=Some("time".into())}
        else if n==1 {assert_eq!(text,complete);assert!(body["messages"][0]["content"].as_str().unwrap().contains("initiating_request"));d.time=Some(MissionChatTime::Weekly{days:vec![1],time:"09:00".into()});d.time_quote=Some(complete)}
        else {assert_eq!(text,"pause that task");d.action=Some("pause".into());d.intent_quote=Some(text.into());d.target_kind=Some("selected".into());d.task_quote=None;d.time=None;d.time_quote=None;}
        let content=frame(json!({"id":format!("continuation-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("decision-{n}"),"mission_chat_decision",serde_json::to_value(d).unwrap())]},"finish_reason":null}]}))+&frame(json!({"id":format!("continuation-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
    }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, s, u, thread) = knowledge::tool_configured(&base).await;
    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        initial.into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let runtime = Runtime::start(s.clone());
    let first = wait(&s, &u, &thread).await;
    assert_eq!(first.run.mission_receipt.unwrap().state, "clarify");
    assert!(s.recurring_missions(&u).await.unwrap().is_empty());
    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "at 9AM".into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let second = wait(&s, &u, &thread).await;
    assert_eq!(
        second.run.state,
        moosuri_core::RunState::Completed,
        "{:?}",
        second.run.error
    );
    assert_eq!(second.run.mission_receipt.unwrap().state, "active");
    let saved = s.recurring_missions(&u).await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].template.task, "summarize public paper updates");
    assert_eq!(count.load(Ordering::SeqCst), 2);
    s.submit_turn_with_schedule_context(
        &u,
        thread.clone(),
        uuid::Uuid::new_v4().to_string(),
        "pause that task".into(),
        None,
        Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let controlled = wait(&s, &u, &thread).await;
    assert_eq!(controlled.run.mission_receipt.unwrap().state, "paused");
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "paused"
    );
    assert_eq!(count.load(Ordering::SeqCst), 3);
    runtime.shutdown().await;
    upstream.abort();
}

#[tokio::test]
async fn conversational_civil_time_runtime_missing_clock_reopen_preserves_original_week_and_zone() {
    let _g = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let first = "next week Friday summarize public notices";
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let counted=counted.clone();async move{
        let n=counted.fetch_add(1,Ordering::SeqCst);
        let intent=body["tools"].as_array().is_some_and(|v|v.iter().any(|d|d["function"]["name"]=="mission_chat_decision"));
        let(delta,reason)=if intent {
            assert_eq!(body["messages"].as_array().unwrap().len(),2);
            let text=body["messages"][1]["content"].as_str().unwrap();let answer=text.contains("오전 아홉 시 십오 분");
            assert!(text.contains(first));
            let d=MissionChatDecision{decision:if answer{"command"}else{"clarify"}.into(),action:Some("create_scheduled".into()),target_kind:None,intent_quote:Some(first.into()),task_quote:Some("summarize public notices".into()),constraints_quote:None,time_quote:answer.then(||"오전 아홉 시 십오 분".into()),target_quote:None,instruction_quote:None,time:answer.then(||MissionChatTime::LocalOnce{date:"next week Friday".into(),time:"09:15".into()}),clarification:(!answer).then(||"time".into())};
            (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,format!("civil-{n}"),"mission_chat_decision",serde_json::to_value(d).unwrap())]}),"tool_calls")
        }else{(json!({"role":"assistant","content":"Synthetic ordinary answer"}),"stop")};
        let body=frame(json!({"id":format!("civil-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(body)).unwrap()
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, s, u, t) = knowledge::tool_configured(&base).await;
    let start = s
        .submit_turn_with_schedule_context(
            &u,
            t.clone(),
            uuid::Uuid::new_v4().to_string(),
            first.into(),
            None,
            Some(ScheduleChatContext {
                timezone: "Asia/Seoul".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    let runtime = Runtime::start(s.clone());
    let question = wait(&s, &u, &t).await;
    assert_eq!(
        question.run.mission_receipt.as_ref().unwrap().state,
        "clarify"
    );
    assert!(s.recurring_missions(&u).await.unwrap().is_empty());
    runtime.shutdown().await;
    drop(runtime);
    drop(s);
    let s = Store::open(dir.path()).unwrap();
    let answer_id = uuid::Uuid::new_v4().to_string();
    let answer = "오전 아홉 시 십오 분";
    s.submit_turn_with_schedule_context(
        &u,
        t.clone(),
        answer_id.clone(),
        answer.into(),
        None,
        Some(ScheduleChatContext {
            timezone: "America/New_York".into(),
            target: None,
        }),
    )
    .await
    .unwrap();
    let runtime = Runtime::start(s.clone());
    let done = wait(&s, &u, &t).await;
    assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
    assert_eq!(done.run.mission_receipt.as_ref().unwrap().state, "active");
    let saved = s.recurring_missions(&u).await.unwrap();
    assert_eq!(saved.len(), 1);
    assert_eq!(saved[0].template.task, "summarize public notices");
    // Independent weekday construction from the original admission instant; no
    // call to the production expression parser or proposed model calendar date.
    let local_day = (start.run.created_at + 9 * 3600).div_euclid(86400);
    // Unix epoch was Thursday (Monday-based index3); Seoul has no DST.
    let monday = local_day - (local_day + 3).rem_euclid(7);
    let expected = (monday + 11) * 86400 + 15 * 60; // next Friday09:15KST = Friday00:15UTC
    assert_eq!(
        saved[0].schedule.rule,
        moosuri_core::ScheduleRule::Once { at: expected }
    );
    let replay = s
        .submit_turn_with_schedule_context(
            &u,
            t.clone(),
            answer_id,
            answer.into(),
            None,
            Some(ScheduleChatContext {
                timezone: "America/New_York".into(),
                target: None,
            }),
        )
        .await
        .unwrap();
    assert_eq!(replay.run.id, done.run.id);
    assert_eq!(count.load(Ordering::SeqCst), 2);
    s.submit_turn(
        &u,
        t.clone(),
        uuid::Uuid::new_v4().to_string(),
        "What is 2 plus 2?".into(),
    )
    .await
    .unwrap();
    runtime.wake();
    wait(&s, &u, &t).await;
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert_eq!(s.recurring_missions(&u).await.unwrap().len(), 1);
    runtime.shutdown().await;
    drop(runtime);
    server.abort();
}

#[tokio::test]
async fn reminder_receipt_followup_moves_and_revokes_same_reminder_without_task_inference() {
    use moosuri_core::{ScheduleCreate, ScheduleDraft, ScheduleRule};
    let _guard = TEST_LOCK.lock().await;
    for reference in ["그 알림", "방금 그 알림"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move || {
                seen.fetch_add(1, Ordering::SeqCst);
                async {
                    (
                        axum::http::StatusCode::SERVICE_UNAVAILABLE,
                        "synthetic unexpected inference",
                    )
                }
            }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (_dir, store, user, thread) = knowledge::tool_configured(&base).await;
        let unrelated = store
            .create_schedule(
                &user,
                ScheduleCreate {
                    request_id: uuid::Uuid::new_v4().to_string(),
                    draft: ScheduleDraft {
                        title: "Unrelated existing reminder".into(),
                        message: "Keep this reminder".into(),
                        rule: ScheduleRule::Daily {
                            time: "11:00".into(),
                            timezone: "Asia/Seoul".into(),
                        },
                        valid_until: None,
                    },
                },
            )
            .await
            .unwrap();
        let context = Some(ScheduleChatContext {
            timezone: "Asia/Seoul".into(),
            target: None,
        });
        let runtime = Runtime::start(store.clone());
        let first = store
            .submit_turn_with_schedule_context(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                "내일 오후 3시에 클로버 시안 검토하라고 알려줘.".into(),
                None,
                context.clone(),
            )
            .await
            .unwrap();
        assert_eq!(first.run.state, RunState::Completed);
        let original = first.run.schedule_receipts.unwrap().remove(0);
        let request = uuid::Uuid::new_v4().to_string();
        let prompt = format!("{reference}을 내일 오후 4시로 옮겨줘.");
        let moved = store
            .submit_turn_with_schedule_context(
                &user,
                thread.clone(),
                request.clone(),
                prompt.clone(),
                None,
                context.clone(),
            )
            .await
            .unwrap();
        let done = wait(&store, &user, &thread).await;
        assert_eq!(done.run.id, moved.run.id);
        assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
        assert!(done.run.mission_receipt.is_none());
        let edited = done.run.schedule_receipts.unwrap().remove(0);
        assert_eq!(edited.id, original.id);
        assert_eq!(edited.revision, original.revision + 1);
        assert_eq!(edited.action, "edit");
        let schedules = store.schedules(&user).await.unwrap();
        assert_eq!(schedules.len(), 2);
        let current = schedules.iter().find(|s| s.id == original.id).unwrap();
        let at = current.next_fire_at.unwrap();
        let hour = chrono_hour_seoul(at);
        assert_eq!(hour, "16:00");
        let replay = store
            .submit_turn_with_schedule_context(
                &user,
                thread.clone(),
                request,
                prompt,
                None,
                context.clone(),
            )
            .await
            .unwrap();
        assert_eq!(replay.run.id, moved.run.id);
        assert_eq!(
            store
                .schedules(&user)
                .await
                .unwrap()
                .iter()
                .find(|s| s.id == original.id)
                .unwrap()
                .revision,
            edited.revision
        );
        let cancelled = store
            .submit_turn_with_schedule_context(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                format!("{reference} 취소해 줘."),
                None,
                context,
            )
            .await
            .unwrap();
        assert_eq!(cancelled.run.state, RunState::Completed);
        let revoked = cancelled.run.schedule_receipts.unwrap().remove(0);
        assert_eq!(revoked.id, original.id);
        assert_eq!(revoked.action, "revoke");
        assert_eq!(revoked.state, "revoked");
        let remaining = store.schedules(&user).await.unwrap();
        assert_eq!(remaining.len(), 1);
        assert_eq!(remaining[0].id, unrelated.id);
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert!(store.missions(&user).await.unwrap().is_empty());
        assert!(store.recurring_missions(&user).await.unwrap().is_empty());
        runtime.shutdown().await;
        server.abort();
    }
}

fn chrono_hour_seoul(at: i64) -> String {
    // Schedule view's UTC timestamp: Seoul is UTC+09 with no DST.
    let seconds = (at + 9 * 3600).rem_euclid(86400);
    format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60)
}
