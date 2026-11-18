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
    for request in [
        "이메일 가장 최근꺼 뭐야",
        "내 이메일은 가장 최근 꺼가 뭐야?",
        "가장 최근의 이메일이 뭐야?",
        "GmAiL 제일 최근 거 보여 줘",
        "내 이메일 최근에 온거 뭐냐고",
    ] {
        store
            .submit_turn(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                request.into(),
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
        assert!(text.contains("Google 앱"));
        assert!(text.contains("관리자인 본인"));
        assert!(matches!(
            done.run.mail_connection_recovery,
            Some(moosuri_core::MailConnectionRecovery::AdminSetup)
        ));
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            0
        );
    }
    runtime.shutdown().await;
}

#[tokio::test]
async fn unconnected_inbox_request_recovers_before_unverified_model_tools_error() {
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
    assert_eq!(blocked.run.state, RunState::Completed);
    assert!(matches!(
        blocked.run.mail_connection_recovery,
        Some(moosuri_core::MailConnectionRecovery::AdminSetup)
    ));
    assert!(
        blocked
            .assistant
            .iter()
            .any(|m| m.text.contains("Google 앱"))
    );
    assert!(!blocked.run.can_retry);
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
    for request in [
        "이메일 가장 최근꺼 뭐야 그리고 답장해줘",
        "내가 가장 최근에 보낸 이메일이 뭐야",
        "다른 사람의 이메일 가장 최근꺼 뭐야",
        "이메일 가장 최근꺼 뭐야라는 문장 번역해줘",
    ] {
        let excluded = store
            .create_thread(&user, "Excluded mailbox request".into())
            .await
            .unwrap();
        let admitted = store
            .submit_turn(
                &user,
                excluded.id,
                uuid::Uuid::new_v4().to_string(),
                request.into(),
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        assert!(
            !mail.latest_inbox_chat(&claim.lease).await.unwrap(),
            "{request}"
        );
        assert!(
            requests.lock().unwrap().is_empty(),
            "excluded request cannot read Gmail: {request}"
        );
        store
            .cancel_run(&user, admitted.run.id, admitted.run.attempt)
            .await
            .unwrap();
    }
    for (index, request) in [
        "이메일 가장 최근꺼 뭐야",
        "가장 최근의 이메일이 뭐야?",
        "내 GmAiL 가장 최근 꺼는 뭔데",
        "이메일 중에서 최근 거 보여줘",
        "내 이메일 가장 최근에 온 게 뭐야?",
    ]
    .into_iter()
    .enumerate()
    {
        store
            .submit_turn(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                request.into(),
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
        assert_eq!(requests.lock().unwrap().len(), (index + 1) * 4);
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            0
        );
    }
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

#[tokio::test]
async fn inbox_recovery_uses_actual_google_setup_and_current_members_own_connection() {
    use moosuri_connectors::gmail::{OAuthClient, SecretString};
    use moosuri_core::MailConnectionRecovery;
    let _guard = TEST_LOCK.lock().await;
    for configured in [false, true] {
        let (_dir, store, admin, _) = tool_configured("http://127.0.0.1:1/v1").await;
        let redirect = "https://assistant.example.test/callback";
        if configured {
            store
                .save_google_setup(
                    &admin,
                    0,
                    OAuthClient {
                        client_id: "synthetic.apps.googleusercontent.com".into(),
                        client_secret: SecretString::new("synthetic-secret".into()).unwrap(),
                        redirect_uri: redirect.into(),
                    },
                )
                .await
                .unwrap();
        }
        let invite = store
            .create_user(
                &admin,
                "Synthetic Member".into(),
                "member@example.test".into(),
            )
            .await
            .unwrap();
        let restricted = store
            .login("member@example.test".into(), invite.temporary_password)
            .await
            .unwrap();
        let member = store
            .change_password(&restricted.user, PASSWORD.into())
            .await
            .unwrap()
            .user;
        let thread = store
            .create_thread(&member, "Synthetic mail request".into())
            .await
            .unwrap();
        store
            .submit_turn(
                &member,
                thread.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "내 이메일 최근에 온거 뭐냐고".into(),
            )
            .await
            .unwrap();
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let (changes, _) = watch::channel(0);
        let signals = Signals {
            stop: CancellationToken::new(),
            wake: Arc::new(Notify::new()),
            changes,
            delivery: Arc::new(tokio::sync::RwLock::new(())),
        };
        let mail = crate::mail::MailService::new(store.clone(), None, signals)
            .with_managed_google(Some(redirect.into()));
        assert!(mail.latest_inbox_chat(&claim.lease).await.unwrap());
        let turn = store
            .conversation(&member, thread.id, None)
            .await
            .unwrap()
            .turns
            .remove(0);
        let text = turn
            .assistant
            .iter()
            .map(|m| m.text.as_str())
            .collect::<String>();
        assert!(match turn.run.mail_connection_recovery {
            Some(MailConnectionRecovery::Connect) => configured,
            Some(MailConnectionRecovery::MemberSetup) => !configured,
            _ => false,
        });
        assert!(text.contains("본인 Google 계정"));
        if !configured {
            assert!(text.contains("설치 관리자"));
        }
        assert!(!text.contains("관리자인 본인"));
        assert_eq!(turn.run.state, RunState::Completed);
        assert_eq!(
            store.usage_summary(&member).await.unwrap().dispatched_calls,
            0
        );
    }
}
