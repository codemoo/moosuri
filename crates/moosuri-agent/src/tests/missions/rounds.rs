//! Outcomes at the same committed-round boundary, before and after reopen.
use super::*;
use moosuri_core::RunInstructionRequest;
use moosuri_store::{PrivateCheckpoint, RunLease, UserContext};

async fn mission(store: &Store, user: &UserContext) -> moosuri_core::MissionView {
    store
        .create_mission(
            user,
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
        .unwrap()
}
async fn terminal(store: &Store, user: &UserContext) -> moosuri_core::MissionView {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = store.missions(user).await.unwrap().remove(0);
            if !["queued", "planning", "executing", "verifying"].contains(&view.state.as_str()) {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("invalid admitted round must settle without spending another model call")
}
async fn guidance(
    store: &Store,
    user: &UserContext,
    lease: &RunLease,
) -> moosuri_core::RunInstructionReceipt {
    store
        .submit_run_instruction(
            user,
            lease.run_id().into(),
            RunInstructionRequest {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_attempt: 1,
                text: "Keep the literal revised criterion".into(),
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn fresh_and_reopened_blank_final_settle_without_repair_inference() {
    let _guard = TEST_LOCK.lock().await;
    for reopen in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let count = Arc::new(AtomicUsize::new(0));
        let seen = count.clone();
        let app = Router::new().route("/v1/chat/completions", post(move || {
            let seen = seen.clone(); async move {
                assert_eq!(seen.fetch_add(1, Ordering::SeqCst), 0);
                let wire = frame(json!({"id":"blank-final","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":"{\"text\":\"  \",\"evidence\":[]}"},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}})) + "data: [DONE]\n\n";
                Response::builder().header("content-type", "text/event-stream").body(Body::from(wire)).unwrap()
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let (dir, store, user, _) = knowledge::tool_configured(&base).await;
        mission(&store, &user).await;
        let store = if reopen {
            let claim = store.claim_agent_run().await.unwrap().unwrap();
            let step = store
                .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
                .await
                .unwrap();
            store.commit_agent_step(&step, PrivateCheckpoint {
                bytes: json!({"kind":"mission_round","terminal":"completed","acceptance_revision":1,"planning":false,"text":"{\"text\":\"  \",\"evidence\":[]}"}).to_string().into_bytes(),
            }, vec![]).await.unwrap();
            drop(store);
            Store::open(dir.path()).unwrap()
        } else {
            store
        };
        let runtime = Runtime::start(store.clone());
        runtime.wake();
        let result = terminal(&store, &user).await;
        assert_eq!(result.state, "failed");
        assert!(result.result.is_none());
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            1
        );
        assert_eq!(count.load(Ordering::SeqCst), usize::from(!reopen));
        runtime.shutdown().await;
        server.abort();
    }
}

#[tokio::test]
async fn reopened_invalid_plan_and_guided_malformed_round_settle_without_inference() {
    let _guard = TEST_LOCK.lock().await;
    for guided in [false, true] {
        let (dir, store, user, _) = knowledge::tool_configured("http://127.0.0.1:1/v1").await;
        mission(&store, &user).await;
        let claim = store.claim_agent_run().await.unwrap().unwrap();
        let receipt = if guided {
            Some(guidance(&store, &user, &claim.lease).await)
        } else {
            None
        };
        let step = store
            .begin_instructed_agent_step(
                &claim.lease,
                claim.context.checkpoint_seq,
                i64::from(guided),
            )
            .await
            .unwrap();
        store.commit_agent_step(&step, PrivateCheckpoint {
            bytes: json!({"kind":"mission_round","terminal":"completed","acceptance_revision":1,"planning":!guided,"text":if guided {"Malformed guided response"} else {"{\"steps\":[]}"}}).to_string().into_bytes(),
        }, vec![]).await.unwrap();
        let run_id = claim.lease.run_id().to_owned();
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let runtime = Runtime::start(store.clone());
        runtime.wake();
        let result = terminal(&store, &user).await;
        assert_eq!(result.state, "failed", "guided={guided}");
        assert!(result.result.is_none());
        assert_eq!(
            store.usage_summary(&user).await.unwrap().dispatched_calls,
            1
        );
        if let Some(receipt) = receipt {
            let receipt = store
                .run_instruction(&user, run_id, receipt.request_id)
                .await
                .unwrap();
            assert_eq!(receipt.state, "closed");
            assert_ne!(
                receipt.state, "applied",
                "malformed output cannot acknowledge guidance"
            );
        }
        runtime.shutdown().await;
    }
}

#[tokio::test]
async fn exact_step_guidance_read_excludes_unassigned_applied_and_stale_steps() {
    let _guard = TEST_LOCK.lock().await;
    let (_dir, store, user, _) = knowledge::tool_configured("http://127.0.0.1:1/v1").await;
    mission(&store, &user).await;
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let first = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    guidance(&store, &user, &claim.lease).await;
    assert!(
        !store.step_has_run_instructions(&first).await.unwrap(),
        "late receipt is unassigned"
    );
    let context = store
        .commit_agent_step(
            &first,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    let second = store
        .begin_instructed_agent_step(&claim.lease, context.checkpoint_seq, 1)
        .await
        .unwrap();
    assert!(store.step_has_run_instructions(&second).await.unwrap());
    assert!(
        store.step_has_run_instructions(&first).await.is_err(),
        "old-step guard remains authoritative"
    );
    store
        .commit_agent_step(
            &second,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store.apply_step_instructions(&second).await.unwrap();
    assert!(!store.step_has_run_instructions(&second).await.unwrap());
}
