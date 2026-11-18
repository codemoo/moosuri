//! Scripted model over real HTTP drives the actual supervisor and local draft
//! tool. This tests execution boundaries, never model comprehension or Gmail.
use super::knowledge::{result_values, tool_call, tool_configured};
use super::*;
use axum::{
    http::{StatusCode, header},
    response::IntoResponse,
};
#[tokio::test]
async fn delegated_mail_request_creates_reviewable_draft_but_cannot_send() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(input):axum::Json<Value>|{let count=count.clone();async move{count.fetch_add(1,Ordering::SeqCst);let planning=input["tools"][0]["function"]["name"]=="memory_command_decision";let previous=result_values(&input);let tool=if planning{Some(tool_call(0,"decision".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null})))}else if previous.iter().any(|v|v["kind"]=="external"){None}else{let tools=input["tools"].as_array().unwrap();assert!(!tools.iter().any(|v|v["function"]["name"]=="mail_approve"||v["function"]["name"]=="mail_send"));let definition=tools.iter().find(|v|v["function"]["name"]=="mail_prepare_draft").unwrap();let connection=definition["function"]["parameters"]["properties"]["connection_id"]["enum"][0].clone();let system=input["messages"][0]["content"].as_str().unwrap();assert!(!system.contains("You may search and read only this user's saved sources"));Some(tool_call(0,"mail-draft".into(),"mail_prepare_draft",json!({"connection_id":connection,"from":"sender@example.test","to":["to@example.test"],"cc":[],"bcc":["hidden@example.test"],"subject":"합성 초안","text":"전체 합성 본문"})))};let final_round=tool.is_none();let delta=if let Some(tool)=tool{json!({"role":"assistant","tool_calls":[tool]})}else{json!({"role":"assistant","content":"로컬 초안을 준비했어요. 내 Gmail · 발송 확인에서 전체 내용을 확인하고 허락해 주세요. 아직 발송하지 않았어요."})};let body=frame(json!({"id":"mail-round","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":if final_round{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}))+"data: [DONE]\n\n";(StatusCode::OK,[(header::CONTENT_TYPE,"text/event-stream")],body).into_response()}}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, thread) = tool_configured(&base).await;
    use moosuri_connectors::gmail::{SecretString, TokenSet};
    let start = store.start_mail_oauth(&user).await.unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let ex = store.claim_mail_oauth(&user, handle).await.unwrap();
    store
        .complete_mail_oauth(
            ex,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "담당자에게 이 내용을 메일로 보내줘.".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |t| !t.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    let actions = store.mail_actions(&user).await.unwrap();
    assert_eq!(actions.len(), 1);
    assert_eq!(actions[0].state, "pending_approval");
    assert_eq!(
        actions[0].content.as_ref().unwrap().bcc,
        vec!["hidden@example.test"]
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    assert!(calls.load(Ordering::SeqCst) >= 2);
    runtime.shutdown().await;
    server.abort();
}

#[tokio::test]
async fn latest_inbox_unconfigured_turn_finishes_without_any_model_call() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread) = tool_configured("http://127.0.0.1:1/v1").await;
    let runtime = Runtime::start(store.clone());
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "내 이메일 가장 최근에 온게 뭐야?".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |t| !t.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    let text = done
        .assistant
        .iter()
        .map(|m| m.text.as_str())
        .collect::<String>();
    assert!(text.contains("Google 앱이 아직 설정되지"));
    assert!(text.contains("/settings/mail"));
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn unverified_compatible_model_does_not_respond_to_an_explicit_tool_request() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, thread) = configured("http://127.0.0.1:1/v1").await;
    let runtime = Runtime::start(store.clone());
    let admitted = store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "내 Gmail 받은편지함을 요약해 줘".into(),
        )
        .await
        .unwrap();
    assert!(!admitted.run.model.function_tools);
    runtime.wake();
    let blocked = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(blocked.run.error.as_deref(), Some("tools_required"));
    assert!(!blocked.run.can_retry);
    assert!(
        store
            .retry_run(&user, blocked.run.id.clone(), blocked.run.attempt)
            .await
            .is_err()
    );
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn latest_inbox_scans_metadata_across_pages_and_keeps_literal_result_out_of_models() {
    use moosuri_connectors::gmail::{OAuthClient, SecretString, TokenSet};
    let _guard = TEST_LOCK.lock().await;
    let requests = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
    let captured = requests.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let app = Router::new().fallback(axum::routing::any(move |request: axum::extract::Request| {
        let captured = captured.clone();
        async move {
            assert_eq!(request.method(), "GET");
            captured.lock().unwrap().push(request.uri().to_string());
            let path = request.uri().path();
            let value = if path == "/messages" {
                assert!(request.uri().query().unwrap().contains("q=in%3Ainbox"));
                if request.uri().query().unwrap().contains("pageToken=page2") {
                    json!({"messages":[{"id":"newer","threadId":"thread"}]})
                } else {
                    json!({"messages":[{"id":"old","threadId":"thread"}],"nextPageToken":"page2"})
                }
            } else {
                assert!(request.uri().query().unwrap().contains("format=metadata"));
                let id = path.trim_start_matches("/messages/");
                json!({"id":id,"internalDate":if id=="newer"{"3000"}else{"1000"},"labelIds":["INBOX"],"payload":{"headers":[{"name":"Subject","value":if id=="newer"{"PRIVATE_NEWEST_LITERAL"}else{"PRIVATE_OLDER_LITERAL"}},{"name":"From","value":"sender@example.test"},{"name":"Date","value":"synthetic date"}]}})
            };
            axum::Json(value).into_response()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, thread) = tool_configured(&format!("http://{address}/v1")).await;
    let start = store.start_mail_oauth(&user).await.unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = store.claim_mail_oauth(&user, handle).await.unwrap();
    store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: Some(
                    moosuri_connectors::gmail::MINIMAL_SCOPES
                        .iter()
                        .map(|s| s.to_string())
                        .collect(),
                ),
            },
            "owner@example.test".into(),
        )
        .await
        .unwrap();
    let (changes, _) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mail = crate::mail::MailService::new(
        store.clone(),
        Some(Arc::new(OAuthClient {
            client_id: "synthetic.apps.googleusercontent.com".into(),
            client_secret: SecretString::new("synthetic-secret".into()).unwrap(),
            redirect_uri: "https://assistant.example.test/callback".into(),
        })),
        signals,
    )
    .with_synthetic_google(address);
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "내 이메일 가장 최근에 온 게 뭐야?".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    assert!(mail.latest_inbox_chat(&claim.lease).await.unwrap());
    let history = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap();
    let text = serde_json::to_string(&history).unwrap();
    assert!(text.contains("PRIVATE_NEWEST_LITERAL"));
    assert!(!text.contains("PRIVATE_OLDER_LITERAL"));
    assert!(text.contains("전체에서 가장 최근"));
    assert_eq!(requests.lock().unwrap().len(), 4);
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    store
        .submit_turn(
            &user,
            thread,
            uuid::Uuid::new_v4().to_string(),
            "ordinary question".into(),
        )
        .await
        .unwrap();
    let next = store.claim_agent_run().await.unwrap().unwrap();
    assert!(
        !next
            .messages
            .iter()
            .any(|m| m.text.contains("PRIVATE_NEWEST_LITERAL"))
    );
    server.abort();
}
