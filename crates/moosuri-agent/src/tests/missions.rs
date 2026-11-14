//! Synthetic full mission through the real supervisor, model HTTP parser and writer.
use super::*;
use moosuri_core::{MissionCheck, MissionCreate, MissionMode};
#[tokio::test]
async fn mission_acceptance_runtime_replans_and_repairs_under_spent_original_budget() {
    revised_mission_runtime(false).await;
}
#[tokio::test]
async fn mission_acceptance_runtime_restarts_after_acknowledged_revised_plan_without_replanning() {
    revised_mission_runtime(true).await;
}
async fn revised_mission_runtime(restart_after_revised_plan: bool) {
    use moosuri_core::{MissionAcceptanceUpdate, MissionAction, MissionCandidate, MissionControl};
    use moosuri_store::{KnowledgeToolInput, PendingKnowledgeCall, PrivateCheckpoint};
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let seen=seen.clone();async move{
        let n=seen.fetch_add(1,Ordering::SeqCst);let context:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
        assert_eq!(context["acceptance_revision"],2);assert_eq!(context["accepted"]["task"],"Compute 2 plus 3");assert_eq!(context["original"]["task"],"Compute 0.1 plus 0.2");
        assert!(!context["receipts"].to_string().contains("old-sum"));
        let (delta,reason)=match n {
            0=>(json!({"role":"assistant","content":"{\"steps\":[\"Calculate the changed sum\",\"Verify current criteria\"]}"}),"stop"),
            1=>(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"new-sum".into(),"decimal_calculate",json!({"operation":"sum","operands":["2","3"]}))]}),"tool_calls"),
            2|3=>{let slot=&context["receipts"].as_array().unwrap()[0];(json!({"role":"assistant","content":json!({"text":if n==2{"Wrong 6"}else{"Exact result 5"},"evidence":[{"step":slot["step"],"call_id":"new-sum"}]}).to_string()}),"stop")},
            _=>panic!("extra model dispatch")
        };
        let data=frame(json!({"id":format!("revised-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("revised-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(data)).unwrap()
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (d, s, u, _) = knowledge::tool_configured(&base).await;
    let m = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: "Compute 0.1 plus 0.2".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::DecimalEquals {
                    expected: "0.3".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let claim = s.claim_agent_run().await.unwrap().unwrap();
    let context = s.agent_context(&claim.lease).await.unwrap();
    let step = s
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &step,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![PendingKnowledgeCall {
            call_id: "old-sum".into(),
            input: KnowledgeToolInput::DecimalCalculate {
                calculation: moosuri_core::DecimalCalculation {
                    operation: moosuri_core::DecimalOperation::Sum,
                    operands: vec!["0.1".into(), "0.2".into()],
                },
            },
        }],
    )
    .await
    .unwrap();
    s.mission_save_plan(&step, vec!["Original plan".into()])
        .await
        .unwrap();
    s.execute_knowledge_tool(&step, "old-sum".into())
        .await
        .unwrap();
    let context = s.agent_context(&claim.lease).await.unwrap();
    let final_old = s
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    s.commit_agent_step(
        &final_old,
        PrivateCheckpoint {
            bytes: b"{}".to_vec(),
        },
        vec![],
    )
    .await
    .unwrap();
    assert!(
        !s.mission_verify(
            &final_old,
            MissionCandidate {
                text: "Wrong result".into(),
                evidence: vec![]
            }
        )
        .await
        .unwrap()
    );
    let paused = s
        .control_mission(
            &u,
            m.id.clone(),
            MissionControl {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: 1,
                action: MissionAction::Pause,
            },
        )
        .await
        .unwrap();
    let updated = s
        .revise_mission_acceptance(
            &u,
            m.id.clone(),
            MissionAcceptanceUpdate {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: paused.revision,
                task: "Compute 2 plus 3".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::DecimalEquals {
                    expected: "5".into(),
                }],
            },
        )
        .await
        .unwrap();
    assert!(updated.plan.is_empty());
    if restart_after_revised_plan {
        use moosuri_connectors::model::protocol::{
            FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole,
        };
        let claim = s.claim_agent_run().await.unwrap().unwrap();
        let step = s
            .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
            .await
            .unwrap();
        // Real loopback transport/parser, then the same durable acknowledgement
        // as the runner. Stop precisely before mission_save_plan_at adopts it.
        let client =
            model_client(&claim.binding.config, None, TextCapabilities::default()).unwrap();
        let request = TextRequest {
            model: claim.binding.model.clone(),
            effort: None,
            max_output_tokens: 2048,
            messages: vec![
                TextMessage { role: TextRole::System, text: "Plan the accepted synthetic mission.".into(), phase: None },
                TextMessage { role: TextRole::User, text: json!({"acceptance_revision":2,"accepted":updated.request,"original":m.request,"receipts":[]}).to_string(), phase: None },
            ],
        };
        let (events, mut received) = tokio::sync::mpsc::channel(32);
        client
            .stream_text(&request, &events, &CancellationToken::new())
            .await
            .unwrap();
        let mut text = String::new();
        let mut finished = false;
        while let Ok(event) = received.try_recv() {
            match event {
                ModelEvent::TextDelta { text: delta, .. } => text.push_str(&delta),
                ModelEvent::UsageSnapshot(usage) => s
                    .update_agent_step_usage(
                        &step,
                        moosuri_core::RunUsage {
                            input_tokens: usage.input_tokens,
                            output_tokens: usage.output_tokens,
                            cache_read_tokens: usage.cache_read_tokens,
                            cache_write_tokens: usage.cache_write_tokens,
                        },
                    )
                    .await
                    .unwrap(),
                ModelEvent::Finished { outcome, .. } => {
                    assert_eq!(outcome, FinishOutcome::Completed);
                    finished = true;
                }
                _ => {}
            }
        }
        assert!(finished);
        s.commit_agent_step(&step, PrivateCheckpoint {
            bytes: json!({"kind":"mission_round","acceptance_revision":2,"planning":true,"text":text}).to_string().into_bytes(),
        }, vec![]).await.unwrap();
        let execution = s.mission_execution(&claim.lease).await.unwrap().unwrap();
        let pending = execution.pending_round.unwrap();
        assert!(pending.planning);
        assert_eq!(pending.acceptance_revision, 2);
        assert!(execution.view.plan.is_empty());
        assert_eq!(count.load(Ordering::SeqCst), 1);
        assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 3);
    }
    drop(s);
    let s = Store::open(d.path()).unwrap();
    assert_eq!(
        s.usage_summary(&u).await.unwrap().dispatched_calls,
        if restart_after_revised_plan { 3 } else { 2 }
    );
    let runtime = Runtime::start(s.clone());
    runtime.wake();
    let done = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let v = s.missions(&u).await.unwrap().remove(0);
            if v.state == "achieved" {
                break v;
            }
            if ["failed", "partial", "missing_evidence", "unknown"].contains(&v.state.as_str()) {
                panic!("{:?}", v.report)
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(done.result.unwrap().text, "Exact result 5");
    assert_eq!(count.load(Ordering::SeqCst), 4);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 6);
    assert_eq!(done.acceptance_history.unwrap().len(), 2);
    runtime.shutdown().await;
    server.abort();
}
#[tokio::test]
async fn mission_model_plan_two_tools_and_deterministic_result_survive_reopen() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let requests = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
    let count_handler = count.clone();
    let requests_handler = requests.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let count=count_handler.clone();let requests=requests_handler.clone();async move {
 let n=count.fetch_add(1,Ordering::SeqCst);requests.lock().await.push(body.clone());
 let (delta,reason)=match n {
 0=>(json!({"role":"assistant","content":"{\"steps\":[\"Read an admitted evidence recipe\",\"Calculate the exact total\",\"Verify the result\"]}"}),"stop"),
 1=>{let message=body["messages"][1]["content"].as_str().unwrap();let original:Value=serde_json::from_str(message).unwrap();assert_eq!(original["original"]["execution_mode"],"durable");let defs=body["tools"].as_array().unwrap();assert!(defs.iter().all(|d|!d["function"]["name"].as_str().unwrap().contains("mail")));let system=body["messages"][0]["content"].as_str().unwrap();let pins:Value=serde_json::from_str(system.split("Admitted recipe metadata: ").nth(1).unwrap()).unwrap();let pin=pins.as_array().unwrap().iter().find(|p|p["id"]=="evidence-comparison").unwrap();(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"exact-total".into(),"decimal_calculate",json!({"operation":"sum","operands":["0.1","0.2"]})),knowledge::tool_call(1,"find-source".into(),"knowledge_search",json!({"kind":"source","text":"Synthetic invoice","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null})),knowledge::tool_call(2,"recipe".into(),"skill_read",pin.clone())]}),"tool_calls")},
 2=>{let context:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();let slots=context["receipts"].as_array().unwrap();let receipt=|name:&str|->Value{serde_json::from_str(slots.iter().find(|v|v["call_id"]==name).unwrap()["result"].as_str().unwrap()).unwrap()};assert_eq!(receipt("exact-total")["value"]["exact"],"0.3");assert_eq!(receipt("recipe")["kind"],"skill");let search=receipt("find-source");let hit=&search["value"]["hits"][0];(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"invoice".into(),"knowledge_read_source",json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}))]}),"tool_calls")},
 3=>{let context:Value=serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();let slot=context["receipts"].as_array().unwrap().iter().find(|v|v["call_id"]=="invoice").unwrap();let source:Value=serde_json::from_str(slot["result"].as_str().unwrap()).unwrap();assert_eq!(source["value"]["text"],"Synthetic invoice: apples 0.1, tea 0.2.");(json!({"role":"assistant","content":json!({"text":"The exact total is 0.3. Synthetic invoice: apples 0.1, tea 0.2.","evidence":[{"step":2,"call_id":"exact-total"},{"step":2,"call_id":"recipe"},{"step":3,"call_id":"invoice"}]}).to_string()}),"stop")},
 _=>panic!("mission exceeded its necessary model calls"),
 };
 let content=frame(json!({"id":format!("mission-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("mission-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
 Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
 }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (d, s, u, thread) = knowledge::tool_configured(&base).await;
    let source = knowledge::seed(
        &s,
        &u,
        "Synthetic invoice",
        vec!["Synthetic invoice: apples 0.1, tea 0.2.".into()],
    )
    .await
    .remove(0);
    let m = s
        .create_mission(
            &u,
            MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: "Calculate synthetic total 0.1 + 0.2".into(),
                constraints: String::new(),
                checks: vec![
                    MissionCheck::DecimalEquals {
                        expected: "0.3".into(),
                    },
                    MissionCheck::SourceQuote {
                        id: source,
                        revision: 1,
                        quote: "Synthetic invoice: apples 0.1, tea 0.2.".into(),
                    },
                ],
                execution_mode: MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let runtime = Runtime::start(s.clone());
    let result = tokio::time::timeout(Duration::from_secs(15), async {
        loop {
            let v = s.missions(&u).await.unwrap().remove(0);
            if !["queued", "planning", "executing", "verifying"].contains(&v.state.as_str()) {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    assert_eq!(result.state, "achieved", "{:?}", result.report);
    assert_eq!(
        result.result.unwrap().text,
        "The exact total is 0.3. Synthetic invoice: apples 0.1, tea 0.2."
    );
    assert_eq!(count.load(Ordering::SeqCst), 4);
    assert_eq!(s.usage_summary(&u).await.unwrap().dispatched_calls, 4);
    assert!(
        s.conversation(&u, thread, None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    assert_eq!(s.list_threads(&u).await.unwrap().len(), 1);
    runtime.shutdown().await;
    drop(runtime);
    drop(s);
    let s = Store::open(d.path()).unwrap();
    let historical = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(historical.id, m.id);
    assert_eq!(historical.state, "achieved");
    assert_eq!(count.load(Ordering::SeqCst), 4);
    upstream.abort();
}

#[tokio::test]
async fn recurring_worker_is_idle_before_due_then_executes_and_publishes_actual_mission() {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let counted = count.clone();
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let count=counted.clone();async move{
        let n=count.fetch_add(1,Ordering::SeqCst);
        let(delta,reason)=match n{
            0=>(json!({"role":"assistant","content":"{\"steps\":[\"Calculate the exact synthetic total\",\"Verify the actual receipt\"]}"}),"stop"),
            1=>{assert!(body["tools"].as_array().unwrap().iter().all(|d|!d["function"]["name"].as_str().unwrap().contains("mail")));(json!({"role":"assistant","tool_calls":[knowledge::tool_call(0,"scheduled-total".into(),"decimal_calculate",json!({"operation":"sum","operands":["0.1","0.2"]}))]}),"tool_calls")},
            2=>(json!({"role":"assistant","content":json!({"text":"Scheduled exact total: 0.3","evidence":[{"step":2,"call_id":"scheduled-total"}]}).to_string()}),"stop"),
            _=>panic!("scheduled occurrence was replayed"),
        };
        let content=frame(json!({"id":format!("scheduled-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]}))+&frame(json!({"id":format!("scheduled-{n}"),"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":reason}],"usage":{"prompt_tokens":10,"completion_tokens":5}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(content)).unwrap()
    }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_d, s, u, _thread) = knowledge::tool_configured(&base).await;
    let at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
        + 2;
    s.create_recurring_mission(
        &u,
        moosuri_core::RecurringMissionCreate {
            request_id: uuid::Uuid::new_v4().to_string(),
            template: MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: "Calculate scheduled 0.1 plus 0.2".into(),
                constraints: String::new(),
                checks: vec![MissionCheck::DecimalEquals {
                    expected: "0.3".into(),
                }],
                execution_mode: MissionMode::Durable,
            },
            schedule: moosuri_core::ScheduleDraft {
                title: "Scheduled synthetic calculation".into(),
                message: "Never copy evidence".into(),
                rule: moosuri_core::ScheduleRule::Once { at },
                valid_until: None,
            },
            changes_only: true,
        },
    )
    .await
    .unwrap();
    let runtime = Runtime::start(s.clone());
    tokio::time::sleep(Duration::from_millis(250)).await;
    assert_eq!(count.load(Ordering::SeqCst), 0);
    let notice = tokio::time::timeout(Duration::from_secs(16), async {
        loop {
            let items = s.notifications(&u, None, false).await.unwrap().items;
            if let Some(notice) = items.into_iter().next() {
                break notice;
            }
            tokio::time::sleep(Duration::from_millis(30)).await
        }
    })
    .await
    .unwrap();
    let result = s.missions(&u).await.unwrap().remove(0);
    assert_eq!(result.state, "achieved");
    assert_eq!(notice.mission_id.as_deref(), Some(result.id.as_str()));
    assert!(!notice.message.contains("0.3"));
    assert!(!notice.message.contains("Calculate scheduled"));
    assert_eq!(count.load(Ordering::SeqCst), 3);
    assert_eq!(
        s.recurring_missions(&u).await.unwrap()[0].schedule.state,
        "completed"
    );
    assert_eq!(s.poll_recurring_missions().await.unwrap(), 0);
    assert_eq!(count.load(Ordering::SeqCst), 3);
    runtime.shutdown().await;
    upstream.abort();
}
