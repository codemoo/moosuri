//! Protocol/context regression, not a measurement of model semantic quality.
use super::knowledge::{tool_call, tool_configured};
use super::*;
use moosuri_core::MessagePhase;
use moosuri_store::OutputPatch;
const CURRENT: &str = "새로 들어온 것들 중 가장 나중에 받은 항목부터 설명해 줄래? 이메일 얘기야.";
const HELPER_CURRENT: &str =
    "이전 대답은 기억하지 말고, 새로 들어온 걸 날짜 역순으로 말해 줄래? 내 받은편지함 이야기야.";
const PRIVATE: &str = "SYNTHETIC_DISPLAY_ONLY_MAIL_MUST_STAY_PRIVATE";
#[tokio::test]
async fn gmail_model_context_reaches_unmatched_current_answers_and_helpers_without_history_mail_or_buttons()
 {
    let _g = TEST_LOCK.lock().await;
    let requests = Arc::new(std::sync::Mutex::new(Vec::<Value>::new()));
    let captured = requests.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(move |axum::Json(body):axum::Json<Value>|{let captured=captured.clone();async move {
        let messages=body["messages"].as_array().unwrap();let system=messages[0]["content"].as_str().unwrap();
        assert!(system.contains("MOOSURI_GMAIL_CAPABILITY_V1"));
        assert!(system.contains("\"recovery\":\"admin_setup\""));
        assert!(system.contains("\"run_read_route\":\"none\""));
        assert!(system.contains("/admin/google"));assert!(system.contains("latest actual user request"));
        assert!(!body.to_string().contains(PRIVATE));
        let current=messages.last().unwrap()["content"].as_str().unwrap();assert!([CURRENT,HELPER_CURRENT].contains(&current));
        let helper=body["tools"][0]["function"]["name"]=="memory_command_decision";
        let delta=if helper {
            assert_eq!(messages.len(),2,"helper sees only its system and the original latest user request");
            json!({"role":"assistant","tool_calls":[tool_call(0,"decision".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}))]})
        }else{
            assert!(messages.iter().any(|m|m["role"]=="assistant" && m["content"]=="Earlier unrelated suggestion: https://mail.google.com/"));
            assert!(body["tools"].as_array().unwrap().iter().all(|t|!t["function"]["name"].as_str().unwrap().starts_with("mail_")));
            json!({"role":"assistant","content":"Google 설치 설정 /admin/google 후 본인의 Gmail 읽기 연결 /settings/mail이 필요해요. 아직 메일을 읽지 않았어요."})
        };
        captured.lock().unwrap().push(body);
        let event=json!({"id":"context-result","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":if helper{"tool_calls"}else{"stop"}}]});
        Response::builder().header("content-type","text/event-stream").body(Body::from(frame(event)+"data: [DONE]\n\n")).unwrap()
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, thread) = tool_configured(&base).await;
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "사이트 주소라는 말만 번역해줘".into(),
        )
        .await
        .unwrap();
    let earlier = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&earlier.lease, earlier.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            moosuri_store::PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .append_agent_output(
            &step,
            vec![OutputPatch::Append {
                item: "synthetic-history".into(),
                text: "Earlier unrelated suggestion: https://mail.google.com/".into(),
                phase: Some(MessagePhase::FinalAnswer),
                refusal: false,
            }],
        )
        .await
        .unwrap();
    store
        .finish_agent_run(&step, RunState::Completed, None)
        .await
        .unwrap();
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "show my latest email".into(),
        )
        .await
        .unwrap();
    let preview = store.claim_agent_run().await.unwrap().unwrap();
    store
        .finish_latest_inbox(&preview.lease, PRIVATE.into(), None)
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    for current in [CURRENT, HELPER_CURRENT] {
        assert!(!Store::mail_connection_help_intent(current));
        let sent = store
            .submit_turn(
                &user,
                thread.clone(),
                uuid::Uuid::new_v4().to_string(),
                current.into(),
            )
            .await
            .unwrap();
        runtime.wake();
        let done = wait_for(&store, &user, &thread, |t| {
            t.run.id == sent.run.id && !t.run.state.active()
        })
        .await;
        assert_eq!(done.run.state, RunState::Completed);
        assert!(
            done.run.mail_connection_recovery.is_none(),
            "semantic model answers cannot mint unsolicited setup buttons"
        );
        assert!(
            done.assistant
                .iter()
                .any(|m| m.text.contains("/admin/google"))
        );
    }
    runtime.shutdown().await;
    server.abort();
    let seen = requests.lock().unwrap();
    assert!(
        seen.iter()
            .any(|b| b["tools"][0]["function"]["name"] == "memory_command_decision")
    );
    assert!(store.mail_actions(&user).await.unwrap().is_empty());
}
