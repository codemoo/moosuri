//! Actual held HTTP streams, protocol continuations and durable local tools.
use super::*;
use moosuri_core::{MissionCheck, MissionCreate, MissionMode, RunInstructionRequest};

#[tokio::test]
async fn held_stream_live_guidance_continues_same_run_preserves_tool_and_closes_invalid_results() {
    let _guard = TEST_LOCK.lock().await;
    for scenario in ["final", "tool", "refused", "malformed", "mission"] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let calls = Arc::new(AtomicUsize::new(0));
        let seen = calls.clone();
        let release = Arc::new(Semaphore::new(0));
        let gate = release.clone();
        let records = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
        let recorded = records.clone();
        let has_tool = matches!(scenario, "tool" | "mission");
        let held = usize::from(has_tool);
        let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{
            let seen=seen.clone();let gate=gate.clone();let records=recorded.clone();async move {
                let n=seen.fetch_add(1,Ordering::SeqCst);records.lock().await.push(body.clone());assert!(n<4);
                let (delta,reason)=if has_tool && n==0 {
                    (json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"original-sum".into(),"decimal_calculate",json!({"operation":"sum","operands":["2","3"]}))]}),"tool_calls")
                } else if n>held && scenario=="refused" {(json!({"role":"assistant","refusal":"Cannot apply synthetic guidance"}),"content_filter")}
                else {
                    let text=if scenario=="mission" {json!({"text":if n==held{"Exact result 5"}else{"Revised exact result 5"},"evidence":[{"step":1,"call_id":"original-sum"}]}).to_string()}
                        else {if n==held{"Stale synthetic answer"}else{"Revised synthetic answer"}.into()};
                    (json!({"role":"assistant","content":text}),"stop")
                };
                if n>held {
                    assert!(body["messages"].as_array().unwrap().iter().any(|m|m["role"]=="user" && m["content"]=="Use the revised constraint exactly."));
                    if has_tool {
                        if scenario=="mission" {assert_eq!(body["messages"][1]["content"].as_str().unwrap().matches("original-sum").count(),1);}
                        else {assert_eq!(body["messages"].as_array().unwrap().iter().filter(|m|m["role"]=="tool").count(),1);}
                    }
                }
                let first=frame(json!({"id":format!("instruction-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}));
                let last=if n>held && scenario=="malformed" {"data: {malformed\n\n".into()}
                    else {frame(json!({"id":format!("instruction-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n"};
                let stream=futures_util::stream::unfold((0,gate,first,last),move |(index,gate,first,last)|async move {
                    let text=match index {0=>first.clone(),1=>{if n==held {gate.clone().acquire_owned().await.unwrap().forget();}last.clone()},_=>return None};
                    Some((Ok::<_,std::io::Error>(Bytes::from(text)),(index+1,gate,first,last)))
                });
                Response::builder().header("content-type","text/event-stream").body(Body::from_stream(stream)).unwrap()
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (_dir, store, user, thread) = knowledge::tool_configured(&base).await;
        let (run_id, thread, mission_id) = if scenario == "mission" {
            let mission = store
                .create_mission(
                    &user,
                    MissionCreate {
                        request_id: uuid::Uuid::new_v4().to_string(),
                        task: "Compute 2 plus 3".into(),
                        constraints: String::new(),
                        checks: vec![MissionCheck::DecimalEquals {
                            expected: "5".into(),
                        }],
                        execution_mode: MissionMode::Durable,
                    },
                )
                .await
                .unwrap();
            let target = mission.instruction_target.unwrap();
            assert!(target.supports_instructions);
            (target.run_id, target.thread_id, Some(mission.id))
        } else {
            let turn = store
                .submit_turn(
                    &user,
                    thread.clone(),
                    uuid::Uuid::new_v4().to_string(),
                    "Explain a synthetic result.".into(),
                )
                .await
                .unwrap();
            assert!(
                !turn.run.supports_instructions,
                "ordinary queued path has not selected continuation support"
            );
            (turn.run.id, thread, None)
        };
        let runtime = Runtime::start(store.clone());
        tokio::time::timeout(Duration::from_secs(4), async {
            while calls.load(Ordering::SeqCst) <= held {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        if mission_id.is_none() {
            let active = store
                .conversation(&user, thread.clone(), None)
                .await
                .unwrap()
                .turns
                .remove(0);
            assert!(active.run.supports_instructions, "{scenario}");
        }
        let request = RunInstructionRequest {
            request_id: uuid::Uuid::new_v4().to_string(),
            expected_attempt: 1,
            text: "Use the revised constraint exactly.".into(),
        };
        let ack = store
            .submit_run_instruction(&user, run_id.clone(), request.clone())
            .await
            .unwrap();
        assert_eq!(ack.state, "received");
        assert_eq!(calls.load(Ordering::SeqCst), held + 1, "ACK must not infer");
        assert_eq!(
            store
                .submit_run_instruction(&user, run_id.clone(), request.clone())
                .await
                .unwrap()
                .sequence,
            ack.sequence
        );
        release.add_permits(1);
        let done = if let Some(id) = mission_id.as_ref() {
            tokio::time::timeout(Duration::from_secs(4), async {
                loop {
                    let actual = store
                        .missions(&user)
                        .await
                        .unwrap()
                        .into_iter()
                        .find(|m| &m.id == id)
                        .unwrap();
                    if !matches!(
                        actual.state.as_str(),
                        "queued" | "planning" | "executing" | "verifying"
                    ) {
                        assert_eq!(actual.state, "achieved");
                        assert!(actual.result.unwrap().text.contains("Revised"));
                        let target = actual.instruction_target.unwrap();
                        assert_eq!(target.run_id, run_id);
                        assert_eq!(target.attempt, 1);
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(10)).await;
                }
            })
            .await
            .unwrap();
            None
        } else {
            Some(wait_for(&store, &user, &thread, |t| !t.run.state.active()).await)
        };
        let receipt = store
            .run_instruction(&user, run_id.clone(), request.request_id.clone())
            .await
            .unwrap();
        assert_eq!(receipt.run_id, run_id);
        assert_eq!(receipt.attempt, 1);
        if matches!(scenario, "refused" | "malformed") {
            assert_ne!(done.as_ref().unwrap().run.state, RunState::Completed);
            assert_eq!(receipt.state, "closed", "{scenario}");
        } else {
            assert_eq!(receipt.state, "applied");
            if let Some(done) = done {
                assert_eq!(done.run.id, run_id);
                assert_eq!(done.run.attempt, 1);
                assert_eq!(
                    done.run.state,
                    RunState::Completed,
                    "{scenario}: {:?}",
                    done.run.error
                );
                assert!(done.assistant.iter().any(|m| m.text.contains("Revised")));
                assert!(
                    done.assistant
                        .iter()
                        .filter(|m| m.text.contains("Stale"))
                        .all(|m| m.phase == Some(moosuri_core::MessagePhase::Commentary))
                );
            }
        }
        assert_eq!(calls.load(Ordering::SeqCst), held + 2, "{scenario}");
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            (held + 2) as i64
        );
        runtime.shutdown().await;
        server.abort();
    }
}
