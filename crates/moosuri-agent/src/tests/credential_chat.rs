//! Actual chat tool HTTP plan → durable wait → original authenticated broker receipt.
use super::knowledge::{tool_call, tool_configured};
use super::*;
use moosuri_credential_broker::{
    auth::{SigningKey, authenticated_bytes, projection_digest},
    policy::LocalGrant,
};
use moosuri_vault_protocol::broker::*;
use uuid::Uuid;
fn clock() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
async fn command(s: &Store, d: &Device, key: &SigningKey, command: Command) -> Reply {
    let c = s.broker_auth_challenge(d.id).await.unwrap();
    s.broker_command(SignedCommand {
        challenge: c.nonce,
        signature: key.sign(&authenticated_bytes(&c, "POST", DEVICE_PATH, &command).unwrap()),
        command,
    })
    .await
    .unwrap()
}
async fn projected(s: &Store, d: &Device, key: &SigningKey, commitment: Commitment) {
    let projection = Projection::GitHubRepository {
        id: 71,
        private: true,
        archived: false,
        stars: 12,
        open_issues: 3,
    };
    let receipt = Receipt {
        commitment,
        outcome: Outcome::ProjectedSuccess,
        projection,
        projection_digest: projection_digest(projection),
    };
    let signed = SignedReceipt {
        signature: key.sign(&receipt_bytes(&receipt)),
        receipt,
    };
    assert!(
        !serde_json::to_string(&signed)
            .unwrap()
            .contains("SYNTHETIC-TOKEN-CANARY")
    );
    command(
        s,
        d,
        key,
        Command::Settle {
            receipt: Box::new(signed),
        },
    )
    .await;
}
#[tokio::test]
async fn credential_chat_wait_restart_receipt_reuse_and_cancel_late_never_redispatch() {
    let _serial = TEST_LOCK.lock().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let seen=seen.clone();async move{
  seen.fetch_add(1,Ordering::SeqCst);assert!(!body.to_string().contains("SYNTHETIC-TOKEN-CANARY"));
  assert!(body["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]=="github_repository_metadata"));
  let current=body["messages"].as_array().unwrap().iter().rev().find(|m|m["role"]=="user").unwrap()["content"].as_str().unwrap();
  if current=="ordinary independent question" {let data=frame(json!({"id":"ordinary","model":"fixture-model","choices":[{"index":0,"delta":{"content":"Synthetic independent answer"},"finish_reason":"stop"}]}))+"data: [DONE]\n\n";return Response::builder().header("content-type","text/event-stream").body(Body::from(data)).unwrap()}
  let args=json!({"owner":"synthetic-owner","repository":"synthetic-repo"});
  let data=frame(json!({"id":"credential-plan","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[tool_call(0,"original-model-call".into(),"github_repository_metadata",args)]},"finish_reason":null}]}))+&frame(json!({"id":"credential-plan","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}))+"data: [DONE]\n\n";
  Response::builder().header("content-type","text/event-stream").body(Body::from(data)).unwrap()
 }}));
    let provider = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, thread) = tool_configured(&base).await;
    let first = store
        .submit_turn(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "GitHub synthetic-owner/synthetic-repo 메타데이터를 읽어 줘".into(),
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    let waiting = wait_for(&store, &user, &thread, |t| {
        t.run
            .credential
            .as_ref()
            .is_some_and(|s| s.status == "waiting")
    })
    .await;
    assert_eq!(waiting.run.id, first.run.id);
    assert!(waiting.run.can_cancel);
    assert!(!waiting.run.can_retry);
    assert!(
        store.claim_agent_run().await.unwrap().is_none(),
        "credential waiting must not reclaim a worker slot"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let other = store
        .create_thread(&user, "Independent chat".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &user,
            other.id.clone(),
            Uuid::new_v4().to_string(),
            "ordinary independent question".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    wait_for(&store, &user, &other.id, |t| {
        t.run.state == RunState::Completed
    })
    .await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "waiting returns the same user's worker capacity for another thread"
    );
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let runtime = Runtime::start(store.clone());
    tokio::time::sleep(Duration::from_millis(350)).await;
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "restart must not repeat the model tool plan"
    );
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let pair = store
        .broker_pair_challenge(
            &user,
            "https://assistant.example.test".into(),
            key.public_key(),
        )
        .await
        .unwrap();
    let device = store
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: key.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    let repo = GitHubRepository {
        owner: "synthetic-owner".into(),
        repository: "synthetic-repo".into(),
    };
    let grant = LocalGrant::select_github(
        device.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        repo.clone(),
        1,
        1,
        clock() + 600,
    )
    .unwrap();
    command(
        &store,
        &device,
        &key,
        Command::RegisterRepository {
            binding: grant.binding.clone(),
            repository: repo,
        },
    )
    .await;
    let Reply::Waiting { mut requests, .. } = command(&store, &device, &key, Command::Poll).await
    else {
        panic!("not waiting")
    };
    assert_eq!(requests.len(), 1);
    let request = requests.remove(0);
    assert_eq!(request.invocation.run.to_string(), first.run.id);
    assert_eq!(request.invocation.step, 1);
    assert!(
        request.expires_at <= clock() + 120,
        "credential wait must inherit the original chat deadline"
    );
    let Reply::Waiting { requests, .. } = command(&store, &device, &key, Command::Poll).await
    else {
        panic!("not waiting")
    };
    assert!(
        requests[0] == request,
        "poll preserves original mapped call and nonce"
    );
    let Reply::Commitment { commitment } = command(
        &store,
        &device,
        &key,
        Command::Commit {
            invocation: request.invocation.clone(),
        },
    )
    .await
    else {
        panic!("not committed")
    };
    projected(&store, &device, &key, *commitment).await;
    let completed = wait_for(&store, &user, &thread, |t| {
        t.run.state == RunState::Completed
    })
    .await;
    assert_eq!(completed.run.id, first.run.id);
    assert_eq!(completed.run.attempt, 1);
    assert!(
        completed
            .assistant
            .iter()
            .any(|m| m.text.contains("Stars: 12"))
    );
    assert_eq!(
        calls.load(Ordering::SeqCst),
        2,
        "fixed receipt summary must not need another LLM call"
    );
    assert_eq!(
        store.settle_credential_chats().await.unwrap(),
        0,
        "same receipt finalizes only once"
    );
    // A second explicit chat uses the same local binding, then cancellation keeps a
    // later authenticated receipt private rather than resurrecting chat output.
    let second = store
        .submit_turn(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "GitHub synthetic-owner/synthetic-repo metadata again".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    wait_for(&store, &user, &thread, |t| {
        t.run.id == second.run.id && t.run.credential.is_some()
    })
    .await;
    let Reply::Waiting { mut requests, .. } = command(&store, &device, &key, Command::Poll).await
    else {
        panic!("not waiting")
    };
    let request = requests.remove(0);
    assert_eq!(request.binding.id, grant.binding.id);
    assert_eq!(request.invocation.run.to_string(), second.run.id);
    let Reply::Commitment { commitment } = command(
        &store,
        &device,
        &key,
        Command::Commit {
            invocation: request.invocation,
        },
    )
    .await
    else {
        panic!("not committed")
    };
    store
        .cancel_run(&user, second.run.id.clone(), 1)
        .await
        .unwrap();
    projected(&store, &device, &key, *commitment).await;
    assert_eq!(store.settle_credential_chats().await.unwrap(), 0);
    let cancelled = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .pop()
        .unwrap();
    assert_eq!(cancelled.run.state, RunState::Cancelled);
    assert!(cancelled.assistant.is_empty());
    assert!(!cancelled.run.can_retry);
    assert!(store.retry_run(&user, second.run.id, 1).await.is_err());
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    runtime.shutdown().await;
    provider.abort();
}

#[tokio::test]
async fn google_inbox_chat_wait_restart_reuse_cancel_and_numeric_receipt_use_original_run() {
    let _serial = TEST_LOCK.lock().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(move|axum::Json(body):axum::Json<Value>|{let seen=seen.clone();async move{
        seen.fetch_add(1,Ordering::SeqCst);assert!(!body.to_string().contains("CANARY"));assert!(body["tools"].as_array().unwrap().iter().any(|t|t["function"]["name"]=="google_inbox_counts"));
        let data=frame(json!({"id":"google-plan","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[tool_call(0,"original-google-call".into(),"google_inbox_counts",json!({}))]},"finish_reason":null}]}))+&frame(json!({"id":"google-plan","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":4,"completion_tokens":2}}))+"data: [DONE]\n\n";
        Response::builder().header("content-type","text/event-stream").body(Body::from(data)).unwrap()
    }}));
    let provider = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, thread) = tool_configured(&base).await;
    let first = store
        .submit_turn(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "내 Gmail INBOX 메시지와 스레드 개수를 알려줘".into(),
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    let waiting = wait_for(&store, &user, &thread, |t| {
        t.run
            .credential
            .as_ref()
            .is_some_and(|c| c.status == "waiting")
    })
    .await;
    assert_eq!(waiting.run.id, first.run.id);
    assert_eq!(
        waiting.run.credential.unwrap().service.as_deref(),
        Some("google")
    );
    assert!(store.claim_agent_run().await.unwrap().is_none());
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let runtime = Runtime::start(store.clone());
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let pair = store
        .broker_pair_challenge(
            &user,
            "https://assistant.example.test".into(),
            key.public_key(),
        )
        .await
        .unwrap();
    let device = store
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: key.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    let grant = LocalGrant::select_google(
        device.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        moosuri_credential_broker::google::GoogleIdentity {
            subject: "synthetic-subject".into(),
            email: "synthetic@example.test".into(),
        },
        1,
        1,
        clock() + 600,
    )
    .unwrap();
    command(
        &store,
        &device,
        &key,
        Command::RegisterGoogleInbox {
            binding: grant.binding.clone(),
        },
    )
    .await;
    for pass in 0..2 {
        let target = if pass == 0 {
            first.run.id.clone()
        } else {
            let next = store
                .submit_turn(
                    &user,
                    thread.clone(),
                    Uuid::new_v4().to_string(),
                    "내 Gmail 받은편지함 읽지 않은 개수 알려줘".into(),
                )
                .await
                .unwrap();
            runtime.wake();
            wait_for(&store, &user, &thread, |t| {
                t.run.id == next.run.id && t.run.credential.is_some()
            })
            .await;
            next.run.id
        };
        let Reply::Waiting { mut requests, .. } =
            command(&store, &device, &key, Command::Poll).await
        else {
            panic!("waiting")
        };
        assert_eq!(requests.len(), 1);
        let request = requests.remove(0);
        assert!(request.operation == Operation::GoogleInboxRead);
        assert!(request.repository.is_none());
        assert_eq!(request.binding.id, grant.binding.id);
        assert_eq!(request.invocation.run.to_string(), target);
        assert!(request.expires_at <= clock() + 120);
        let Reply::Waiting { requests, .. } = command(&store, &device, &key, Command::Poll).await
        else {
            panic!("waiting")
        };
        assert!(requests[0] == request);
        let Reply::Commitment { commitment } = command(
            &store,
            &device,
            &key,
            Command::CommitCurrent {
                invocation: request.invocation.clone(),
                request_digest: moosuri_credential_broker::auth::hash(&request_bytes(&request)),
            },
        )
        .await
        else {
            panic!("commit")
        };
        let projection = Projection::GoogleInbox {
            messages_total: 21,
            messages_unread: 7,
            threads_total: 13,
            threads_unread: 4,
        };
        let receipt = Receipt {
            commitment: *commitment,
            outcome: Outcome::ProjectedSuccess,
            projection,
            projection_digest: projection_digest(projection),
        };
        let signed = SignedReceipt {
            signature: key.sign(&receipt_bytes(&receipt)),
            receipt,
        };
        if pass == 1 {
            store.cancel_run(&user, target.clone(), 1).await.unwrap();
        }
        command(
            &store,
            &device,
            &key,
            Command::Settle {
                receipt: Box::new(signed.clone()),
            },
        )
        .await;
        command(
            &store,
            &device,
            &key,
            Command::Settle {
                receipt: Box::new(signed),
            },
        )
        .await;
        if pass == 0 {
            let done = wait_for(&store, &user, &thread, |t| {
                t.run.state == RunState::Completed
            })
            .await;
            assert_eq!(done.run.id, target);
            assert!(
                done.assistant
                    .iter()
                    .any(|m| m.text.contains("메시지 전체: 21 · 읽지 않음: 7"))
            );
            assert!(
                !serde_json::to_string(&done)
                    .unwrap()
                    .contains("synthetic@example.test")
            );
        } else {
            assert_eq!(store.settle_credential_chats().await.unwrap(), 0);
            let conversation = store
                .conversation(&user, thread.clone(), None)
                .await
                .unwrap();
            let cancelled = conversation
                .turns
                .iter()
                .find(|t| t.run.id == target)
                .unwrap();
            assert_eq!(cancelled.run.state, RunState::Cancelled);
            assert!(cancelled.assistant.is_empty());
        }
    }
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert!(store.github_metadata_jobs(&user).await.unwrap().is_empty());
    assert_eq!(store.google_inbox_jobs(&user).await.unwrap().len(), 2);
    runtime.shutdown().await;
    provider.abort();
}

#[tokio::test]
async fn google_inbox_nonaffirmative_model_calls_create_no_job_or_dispatch() {
    let _serial = TEST_LOCK.lock().await;
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/chat/completions",post(|axum::Json(body):axum::Json<Value>|async move{
        let tool_result=body["messages"].as_array().unwrap().iter().any(|m|m["role"]=="tool");
        let data=if tool_result {frame(json!({"id":"denied-final","model":"fixture-model","choices":[{"index":0,"delta":{"content":"No credential read was authorized."},"finish_reason":"stop"}]}))}else{
            frame(json!({"id":"forced-plan","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":[tool_call(0,"forced-google-call".into(),"google_inbox_counts",json!({}))]},"finish_reason":null}]}))+&frame(json!({"id":"forced-plan","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}))
        }+"data: [DONE]\n\n";Response::builder().header("content-type","text/event-stream").body(Body::from(data)).unwrap()
    }));
    let provider = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (_dir, store, user, thread) = tool_configured(&base).await;
    let key = SigningKey::from_pkcs8(&SigningKey::generate_pkcs8().unwrap()).unwrap();
    let pair = store
        .broker_pair_challenge(
            &user,
            "https://assistant.example.test".into(),
            key.public_key(),
        )
        .await
        .unwrap();
    let device = store
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: key.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    let grant = LocalGrant::select_google(
        device.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        moosuri_credential_broker::google::GoogleIdentity {
            subject: "synthetic-subject".into(),
            email: "synthetic@example.test".into(),
        },
        1,
        1,
        clock() + 600,
    )
    .unwrap();
    command(
        &store,
        &device,
        &key,
        Command::RegisterGoogleInbox {
            binding: grant.binding,
        },
    )
    .await;
    let runtime = Runtime::start(store.clone());
    for text in [
        "Do not read my Gmail INBOX unread count",
        "Please don't show my Gmail inbox counts",
        "Translate my Gmail inbox unread count",
        "Explain the phrase show my Gmail inbox counts",
        "내 Gmail INBOX 개수 알려주지 마",
        "내 Gmail INBOX 개수 알려줘 문장을 번역해줘",
        "문서 내용: 내 Gmail INBOX 개수 알려줘",
        "내 Gmail 받은편지함 개수에 대한 문장을 분석해줘",
    ] {
        let turn = store
            .submit_turn(
                &user,
                thread.clone(),
                Uuid::new_v4().to_string(),
                text.into(),
            )
            .await
            .unwrap();
        runtime.wake();
        let done = wait_for(&store, &user, &thread, |t| {
            t.run.id == turn.run.id && t.run.state == RunState::Completed
        })
        .await;
        assert!(done.run.credential.is_none());
        assert!(store.google_inbox_jobs(&user).await.unwrap().is_empty());
        let Reply::Waiting { requests, .. } = command(&store, &device, &key, Command::Poll).await
        else {
            panic!("waiting")
        };
        assert!(requests.is_empty());
    }
    let affirmative = store
        .submit_turn(
            &user,
            thread.clone(),
            Uuid::new_v4().to_string(),
            "Please check my Gmail inbox unread count.".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    wait_for(&store, &user, &thread, |t| {
        t.run.id == affirmative.run.id && t.run.credential.is_some()
    })
    .await;
    let Reply::Waiting { requests, .. } = command(&store, &device, &key, Command::Poll).await
    else {
        panic!("waiting")
    };
    assert_eq!(requests.len(), 1);
    assert_eq!(requests[0].invocation.run.to_string(), affirmative.run.id);
    store
        .cancel_run(&user, affirmative.run.id, 1)
        .await
        .unwrap();
    runtime.shutdown().await;
    provider.abort();
}
