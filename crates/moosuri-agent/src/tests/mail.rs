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
