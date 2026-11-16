//! Real Runtime model transport -> typed native slot -> authenticated broker -> next model step.
use super::knowledge::{tool_call, tool_configured};
use super::*;
use moosuri_credential_broker::{
    auth::{SigningKey, authenticated_bytes, projection_digest},
    google::GoogleIdentity,
    policy::LocalGrant,
};
use moosuri_vault_protocol::broker::*;
use uuid::Uuid;
async fn command(s: &Store, d: &Device, k: &SigningKey, command: Command) -> Reply {
    let c = s.broker_auth_challenge(d.id).await.unwrap();
    s.broker_command(SignedCommand {
        challenge: c.nonce,
        signature: k.sign(&authenticated_bytes(&c, "POST", DEVICE_PATH, &command).unwrap()),
        command,
    })
    .await
    .unwrap()
}
fn clock() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
async fn deliver(s: &Store, d: &Device, k: &SigningKey, run: &str) {
    let Reply::Waiting { requests, .. } = command(s, d, k, Command::Poll).await else {
        panic!("poll")
    };
    assert_eq!(requests.len(), 1);
    let request = &requests[0];
    assert_eq!(request.invocation.run.to_string(), run);
    let Reply::ModelMailPayload { payload } = command(
        s,
        d,
        k,
        Command::ModelMailPayload {
            invocation: request.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(request)),
        },
    )
    .await
    else {
        panic!("payload")
    };
    assert!(matches!(payload.read, MailModelRead::Search { .. }));
    let Reply::Commitment { commitment } = command(
        s,
        d,
        k,
        Command::CommitCurrent {
            invocation: request.invocation.clone(),
            request_digest: moosuri_credential_broker::auth::hash(&request_bytes(request)),
        },
    )
    .await
    else {
        panic!("commit")
    };
    let projection = Projection::GoogleMailModelRead {
        result: Box::new(MailModelResult {
            version: 1,
            status: MailModelStatus::Ready,
            messages: vec![MailModelMessage {
                selection: Uuid::new_v4(),
                thread: Uuid::new_v4(),
                observation: Uuid::new_v4(),
                subject: "Synthetic native invoice".into(),
                from: "source@example.test".into(),
                text: "Synthetic amount 42".into(),
                partial: false,
                content_digest: [9; 32],
                attachments: vec![],
            }],
            next_cursor: None,
            partial: false,
            attachment: None,
        }),
    };
    let receipt = Receipt {
        commitment: *commitment,
        outcome: Outcome::ProjectedSuccess,
        projection_digest: projection_digest(projection.clone()),
        projection,
    };
    let signed = SignedReceipt {
        signature: k.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    assert!(matches!(
        command(
            s,
            d,
            k,
            Command::Settle {
                receipt: Box::new(signed)
            }
        )
        .await,
        Reply::Settled
    ));
}
#[tokio::test]
async fn native_mail_runtime_current_chat_and_background_mission_continue_from_signed_real_slots() {
    let _lock = TEST_LOCK.lock().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let seen=seen.clone();async move{
        seen.fetch_add(1,Ordering::SeqCst);let all=body.to_string();assert!(!all.contains("SYNTHETIC-NATIVE-SECRET"));
        let user=body["messages"].as_array().unwrap().iter().rev().find(|m|m["role"]=="user").unwrap()["content"].as_str().unwrap();let context=serde_json::from_str::<Value>(user).ok();let mission=context.as_ref().is_some_and(|v|v.get("accepted").is_some());
        // Every original and resumed inference keeps the same admitted read catalog.
        for name in ["native_mail_search", "native_mail_message", "native_mail_thread", "native_mail_attachment"] {
            assert!(body["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]==name), "missing {name}");
        }
        let has_result=all.contains("Synthetic amount 42");
        let (delta,reason)=if has_result{
            let text=if mission{json!({"text":"Synthetic amount 42 from the authenticated native read","evidence":[{"step":1,"call_id":"native-real-call"}]}).to_string()}else{"Synthetic amount 42 from the authenticated native read".into()};(json!({"role":"assistant","content":text}),"stop")
        }else{(json!({"role":"assistant","tool_calls":[tool_call(0,"native-real-call".into(),"native_mail_search",json!({"query":"newer_than:7d","cursor":null}))]}),"tool_calls")};
        let data=frame(json!({"id":"native-model-step","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":reason}],"usage":{"prompt_tokens":5,"completion_tokens":2}}))+"data: [DONE]\n\n";Response::builder().header("content-type","text/event-stream").body(Body::from(data)).unwrap()
    }}));
    let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, thread) = tool_configured(&base).await;
    let k = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let pair = store
        .broker_pair_challenge(
            &user,
            "https://assistant.example.test".into(),
            k.public_key(),
        )
        .await
        .unwrap();
    let device = store
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: k.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    let grant = LocalGrant::select_google_model_read(
        device.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        GoogleIdentity {
            subject: "native-runtime-sub".into(),
            email: "native-private@example.test".into(),
        },
        1,
        1,
        clock() + 600,
        true,
    )
    .unwrap();
    command(
        &store,
        &device,
        &k,
        Command::RegisterGoogleModelRead {
            binding: grant.binding.clone(),
            background: true,
        },
    )
    .await;
    let first = store
        .submit_turn(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "Read native Gmail recent invoice".into(),
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    let waiting = wait_for(&store, &user, &thread, |t| t.run.connection_wait.is_some()).await;
    assert_eq!(waiting.run.id, first.run.id);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    deliver(&store, &device, &k, &first.run.id).await;
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |t| {
        t.run.state == RunState::Completed
    })
    .await;
    assert_eq!(done.run.id, first.run.id);
    assert!(
        done.assistant
            .iter()
            .any(|m| m.text.contains("Synthetic amount 42"))
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let mission = store
        .create_mission(
            &user,
            moosuri_core::MissionCreate {
                request_id: Uuid::new_v4().to_string(),
                task: "Read native Gmail and summarize the recent invoice".into(),
                constraints: String::new(),
                checks: vec![moosuri_core::MissionCheck::UserReview {
                    description: "Review the authenticated result".into(),
                }],
                execution_mode: moosuri_core::MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    runtime.wake();
    let waiting = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let m = store
                .missions(&user)
                .await
                .unwrap()
                .into_iter()
                .find(|m| m.id == mission.id)
                .unwrap();
            if m.state == "waiting" {
                break m;
            }
            tokio::time::sleep(Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    let run = waiting.wait.as_ref().unwrap().run_id.clone();
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    tokio::time::sleep(Duration::from_millis(700)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    deliver(&store, &device, &k, &run).await;
    runtime.wake();
    let finished = tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let m = store
                .missions(&user)
                .await
                .unwrap()
                .into_iter()
                .find(|m| m.id == mission.id)
                .unwrap();
            if m.state == "partial" {
                break m;
            }
            tokio::time::sleep(Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    assert!(
        finished
            .result
            .unwrap()
            .text
            .contains("Synthetic amount 42")
    );
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    runtime.shutdown().await;
    upstream.abort();
}

#[tokio::test]
async fn native_mixed_source_results_keep_receipts_for_bound_calculation_without_rereads() {
    let _lock = TEST_LOCK.lock().await;
    // Both already-saved local results and reads deferred until native reclaim
    // must carry the original tool-slot address into the next actual inference.
    for native_first in [false, true] {
        let requests = Arc::new(tokio::sync::Mutex::new(Vec::<Value>::new()));
        let seen = requests.clone();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = Router::new().route(
            "/v1/chat/completions",
            post(move |axum::Json(body): axum::Json<Value>| {
                let seen = seen.clone();
                async move {
                    let round = {
                        let mut requests = seen.lock().await;
                        requests.push(body.clone());
                        requests.len()
                    };
                    let prior = super::knowledge::result_values(&body);
                    let mut calls = vec![];
                    let mut answer = None;
                    match round {
                        1 => {
                            calls.push(tool_call(0, "saved-search".into(), "knowledge_search", json!({"kind":"source","text":"NativeMixedLookup","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null})));
                        }
                        2 => {
                            let search = prior.iter().find(|v| v["kind"] == "search").unwrap();
                            assert_eq!(search["receipt"]["call_id"], "saved-search");
                            assert!(search["receipt"]["step"].is_u64(), "resumed search needs its actual receipt address");
                            assert_eq!(search["value"]["hits"].as_array().unwrap().len(), 1);
                            let hit = &search["value"]["hits"][0];
                            calls.push(tool_call(0, "saved-source".into(), "knowledge_read_source", json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096})));
                        }
                        3 => {
                            let search = prior.iter().find(|v| v["kind"] == "search").unwrap();
                            let source = prior.iter().find(|v| v["kind"] == "source").unwrap();
                            assert_eq!(source["receipt"]["call_id"], "saved-source");
                            assert!(source["receipt"]["step"].is_u64(), "resumed source needs its actual receipt address");
                            // Consume only addresses actually returned by the two
                            // resumed model requests; no inferred dispatch numbers.
                            calls.push(tool_call(0, "bound".into(), "evidence_calculate", json!({
                                "plan":{"origins":["source","current","current"],"graph":{"input_count":3,"steps":[{"operation":"product","operands":[{"kind":"input","index":0},{"kind":"input","index":1}]},{"operation":"sum","operands":[{"kind":"result","index":0},{"kind":"input","index":2}]}]}},
                                "binding":{"search":search["receipt"],"source":source["receipt"],"query":"NativeMixedLookup","prefix":"mixed_price:","scalars":["4","0.60"]}
                            })));
                        }
                        4 => {
                            let calculated = prior.iter().find(|v| v["kind"] == "evidence_calculation").expect("returned addresses must resolve to actual saved receipts");
                            assert_eq!(calculated["value"]["exact"], "10");
                            assert_eq!(calculated["value"]["evidence"]["literal"], "2.35");
                            assert_eq!(calculated["value"]["evidence"]["source"]["call_id"], "saved-source");
                            assert!(body.to_string().contains("Synthetic amount 42"));
                            answer = Some("Native observations retained; exact saved-source total 10.");
                        }
                        _ => panic!("no repeated searches or extra repair inference is needed"),
                    }
                    if round <= 2 {
                        let native = tool_call(1, format!("native-{round}"), "native_mail_search", json!({"query":if round == 1{"newer_than:7d subject:invoice"}else{"newer_than:7d subject:receipt"},"cursor":null}));
                        if native_first {
                            calls.insert(0, native);
                        } else {
                            calls.push(native);
                        }
                        for (index, call) in calls.iter_mut().enumerate() {
                            call["index"] = json!(index);
                        }
                    }
                    let done = answer.is_some();
                    let data = frame(json!({"id":"native-mixed-source","model":"fixture-model","choices":[{"index":0,"delta":if let Some(text)=answer{json!({"role":"assistant","content":text})}else{json!({"role":"assistant","tool_calls":calls})},"finish_reason":if done{"stop"}else{"tool_calls"}}],"usage":{"prompt_tokens":5,"completion_tokens":2}})) + "data: [DONE]\n\n";
                    Response::builder().header("content-type", "text/event-stream").body(Body::from(data)).unwrap()
                }
            }),
        );
        let upstream = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (_dir, store, user, thread) = tool_configured(&base).await;
        super::knowledge::seed(
            &store,
            &user,
            "Synthetic mixed source",
            vec!["NativeMixedLookup mixed_price: 2.35".into()],
        )
        .await;
        let k = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
        let pair = store
            .broker_pair_challenge(
                &user,
                "https://assistant.example.test".into(),
                k.public_key(),
            )
            .await
            .unwrap();
        let device = store
            .broker_pair(PairProof {
                challenge: pair.nonce,
                signature: k.sign(&pairing_bytes(&pair).unwrap()),
            })
            .await
            .unwrap();
        let grant = LocalGrant::select_google_model_read(
            device.clone(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            GoogleIdentity {
                subject: "mixed-native-sub".into(),
                email: "mixed-native@example.test".into(),
            },
            1,
            1,
            clock() + 600,
            true,
        )
        .unwrap();
        command(
            &store,
            &device,
            &k,
            Command::RegisterGoogleModelRead {
                binding: grant.binding.clone(),
                background: true,
            },
        )
        .await;
        let first = store.submit_turn(&user, thread.clone(), Uuid::new_v4().to_string(), "Read native Gmail invoices and receipts. 자료 NativeMixedLookup; 숫자 앞 문구 mixed_price:; 수량 4; 수수료 0.60. Calculate source number times quantity plus fee.".into()).await.unwrap();
        let runtime = Runtime::start(store.clone());
        for expected_round in 1..=2 {
            let waiting = wait_for(&store, &user, &thread, |turn| {
                turn.run.connection_wait.is_some()
                    && requests
                        .try_lock()
                        .is_ok_and(|seen| seen.len() == expected_round)
            })
            .await;
            assert_eq!(waiting.run.id, first.run.id);
            tokio::time::sleep(Duration::from_millis(100)).await;
            assert_eq!(
                requests.lock().await.len(),
                expected_round,
                "waiting must not poll the model"
            );
            deliver(&store, &device, &k, &first.run.id).await;
            runtime.wake();
        }
        let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
        assert_eq!(done.run.state, RunState::Completed, "{:?}", done.run.error);
        assert_eq!(done.run.id, first.run.id);
        assert!(text(&done).contains("exact saved-source total 10"));
        let requests = requests.lock().await;
        assert_eq!(requests.len(), 4);
        let mut searches = 0;
        let mut source_reads = 0;
        // Each outgoing continuation includes earlier assistant calls. The last
        // request is therefore the complete call ledger, not a sum of repeats.
        for message in requests.last().unwrap()["messages"].as_array().unwrap() {
            if let Some(calls) = message["tool_calls"].as_array() {
                for call in calls {
                    searches += usize::from(call["function"]["name"] == "knowledge_search");
                    source_reads +=
                        usize::from(call["function"]["name"] == "knowledge_read_source");
                }
            }
        }
        assert_eq!((searches, source_reads), (1, 1));
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            4
        );
        runtime.shutdown().await;
        upstream.abort();
    }
}
