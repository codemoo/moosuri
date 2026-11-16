//! Real Store pressure at the boundary between one model response and its tools.
use super::*;
use moosuri_core::{MissionAction, MissionCheck, MissionControl, MissionCreate, MissionMode};
use moosuri_store::{KnowledgeToolInput, PendingKnowledgeCall};

#[tokio::test]
async fn round_checkpoint_pressure_preserves_response_and_resumes_after_reopen() {
    checkpoint_pressure(false).await;
}

#[tokio::test]
async fn round_checkpoint_pressure_observes_owner_cancellation_before_retry() {
    checkpoint_pressure(true).await;
}

async fn checkpoint_pressure(cancel: bool) {
    let _guard = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let count = Arc::new(AtomicUsize::new(0));
    let seen = count.clone();
    let app = Router::new().route("/v1/chat/completions", post(move |axum::Json(body): axum::Json<Value>| {
        let seen = seen.clone();
        async move {
            assert_eq!(seen.fetch_add(1, Ordering::SeqCst), 0, "never repeat the saved model round");
            let context: Value = serde_json::from_str(body["messages"][1]["content"].as_str().unwrap()).unwrap();
            let receipts = context["receipts"].as_array().unwrap();
            assert_eq!(receipts.len(), 1);
            assert_eq!(receipts[0]["call_id"], "saved-sum");
            assert_eq!(receipts[0]["result"]["value"]["exact"], "5");
            let text = json!({"text":"The exact total is 5.","evidence":[{"step":1,"call_id":"saved-sum"}]}).to_string();
            let wire = frame(json!({"id":"after-reopen","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":"stop"}],"usage":{"prompt_tokens":10,"completion_tokens":5}})) + "data: [DONE]\n\n";
            Response::builder().header("content-type", "text/event-stream").body(Body::from(wire)).unwrap()
        }
    }));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, _) = knowledge::tool_configured(&base).await;
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
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    let current = store.missions(&user).await.unwrap().remove(0);
    let bytes = json!({"kind":"mission_round","terminal":"continuation_required","acceptance_revision":1,"planning":false,"text":""}).to_string().into_bytes();
    let calls = vec![PendingKnowledgeCall {
        call_id: "saved-sum".into(),
        input: KnowledgeToolInput::DecimalCalculate {
            calculation: moosuri_core::DecimalCalculation {
                operation: moosuri_core::DecimalOperation::Sum,
                operands: vec!["2".into(), "3".into()],
            },
        },
    }];
    {
        // Hold the actor and fill its bounded queue by polling each read exactly
        // once. No timing race, SQLite timeout or new production failpoint.
        let (entered, at_actor) = tokio::sync::oneshot::channel();
        let (release, released) = std::sync::mpsc::channel();
        let held_store = store.clone();
        let holder = tokio::spawn(async move {
            held_store
                .hold_subscription_actor_fixture(entered, released)
                .await
        });
        at_actor.await.unwrap();
        let mut cancellation = Box::pin(store.control_mission(
            &user,
            mission.id.clone(),
            MissionControl {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: current.revision,
                action: MissionAction::Cancel,
            },
        ));
        if cancel {
            assert!(futures_util::poll!(cancellation.as_mut()).is_pending());
        }
        let mut queued = Vec::new();
        for _ in 0..(64 - usize::from(cancel)) {
            let mut read = Box::pin(store.initialized());
            assert!(futures_util::poll!(read.as_mut()).is_pending());
            queued.push(read);
        }
        assert_eq!(store.initialized().await, Err(StoreError::Busy));
        let mut pending = Box::pin(crate::runner::commit_round(&store, &step, &bytes, &calls));
        let first_poll = futures_util::poll!(pending.as_mut());
        release.send(()).unwrap();
        holder.await.unwrap().unwrap();
        for read in queued {
            assert!(read.await.unwrap());
        }
        if cancel {
            assert_eq!(cancellation.await.unwrap().state, "cancelled");
        }
        assert!(
            first_poll.is_pending(),
            "temporary queue pressure discarded the received model round"
        );
        let result = pending.await;
        if cancel {
            assert!(matches!(
                result,
                Err(StoreError::Unauthorized
                    | StoreError::Forbidden
                    | StoreError::NotFound
                    | StoreError::Conflict)
            ));
        } else {
            let committed = result.unwrap();
            assert_eq!(committed.checkpoint.as_ref().unwrap().bytes, bytes);
            assert_eq!(committed.calls.len(), 1);
            assert_eq!(committed.calls[0].call_id, "saved-sum");
            assert!(committed.calls[0].result.is_none());
            assert_eq!(
                store.usage_summary(&user).await.unwrap().dispatched_calls,
                1
            );
        }
    }
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let runtime = Runtime::start(store.clone());
    let view = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let view = store.missions(&user).await.unwrap().remove(0);
            if !["queued", "planning", "executing", "verifying"].contains(&view.state.as_str()) {
                break view;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(view.id, mission.id);
    if cancel {
        assert_eq!(view.state, "cancelled");
        assert!(view.result.is_none());
    } else {
        assert_eq!(view.state, "achieved");
        assert_eq!(view.result.unwrap().text, "The exact total is 5.");
    }
    runtime.shutdown().await;
    assert_eq!(count.load(Ordering::SeqCst), usize::from(!cancel));
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        if cancel { 1 } else { 2 }
    );
    let conn = rusqlite::Connection::open_with_flags(
        dir.path().join("assistant.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let (slots, finished): (i64, i64) = conn.query_row("SELECT count(*),coalesce(sum(result IS NOT NULL),0) FROM agent_tool_slots WHERE run_id=?1", [claim.lease.run_id()], |r| Ok((r.get(0)?, r.get(1)?))).unwrap();
    assert_eq!((slots, finished), if cancel { (0, 0) } else { (1, 1) });
    server.abort();
}
