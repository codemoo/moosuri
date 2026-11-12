//! Runtime outcomes with a scripted provider, not semantic classifier evaluation.
use super::*;
use moosuri_core::{MemoryCommandAction, MemoryCommandIntent, MemoryDraft};
use moosuri_store::PrivateCheckpoint;

const REMEMBER: &str = "나는 홍차를 좋아해. 이 사실을 기억해 줘.";
const CORRECT: &str = "홍차 취향을 수정해 줘. 나는 녹차를 좋아해.";
const FORGET: &str = "녹차 취향을 잊어 줘.";
const PRIVATE_PREVIEW: &str = "INTERNAL-SYNTHETIC-PLANNER-PREVIEW";

#[derive(Clone, Copy)]
enum Mode {
    Commands,
    NoCommand,
    ForgedQuote,
    RetryAfterRead,
    Hold,
}
struct MemoryScript {
    mode: Mode,
    calls: AtomicUsize,
    requests: tokio::sync::Mutex<Vec<Value>>,
    release: Arc<Semaphore>,
}
struct MemoryServer {
    base: String,
    state: Arc<MemoryScript>,
    task: JoinHandle<()>,
}
impl Drop for MemoryServer {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn no_command() -> Value {
    json!({"decision":"no_command","clarification":null,"action":null,
        "intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null})
}
fn command(
    action: &str,
    intent: &str,
    fact: Option<&str>,
    subject: Option<&str>,
    target: Option<&str>,
) -> Value {
    json!({"decision":"command","clarification":null,"action":action,
        "intent_quote":intent,"fact_quote":fact,"subject":subject,"target_quote":target})
}
async fn memory_model(mode: Mode) -> MemoryServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(MemoryScript {
        mode,
        calls: AtomicUsize::new(0),
        requests: tokio::sync::Mutex::new(vec![]),
        release: Arc::new(Semaphore::new(0)),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(memory_response))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    MemoryServer { base, state, task }
}
async fn memory_response(
    State(state): State<Arc<MemoryScript>>,
    axum::Json(input): axum::Json<Value>,
) -> Response {
    let call = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
    state.requests.lock().await.push(input.clone());
    assert_eq!(input["model"], "fixture-model");
    let planning = input["tools"][0]["function"]["name"] == "memory_command_decision";
    let messages = input["messages"].as_array().unwrap();
    let current = messages.iter().rev().find(|v| v["role"] == "user").unwrap()["content"]
        .as_str()
        .unwrap();
    if matches!(state.mode, Mode::RetryAfterRead) && call == 3 {
        return Response::builder().status(503).body(Body::empty()).unwrap();
    }
    let (delta, finish) = if planning {
        assert_eq!(input["max_tokens"], 2048);
        assert_eq!(
            messages.len(),
            2,
            "intent model must see only system plus current user"
        );
        assert_eq!(messages[0]["role"], "system");
        let args = match state.mode {
            Mode::NoCommand | Mode::RetryAfterRead => no_command(),
            Mode::ForgedQuote => command(
                "remember",
                "기억해 줘",
                Some("absent synthetic fact"),
                Some("취향"),
                None,
            ),
            Mode::Commands | Mode::Hold => match current {
                REMEMBER => command(
                    "remember",
                    "기억해 줘",
                    Some("나는 홍차를 좋아해"),
                    Some("음료 취향"),
                    None,
                ),
                CORRECT => command(
                    "correct",
                    "수정해 줘",
                    Some("나는 녹차를 좋아해"),
                    Some("음료 취향"),
                    Some("홍차"),
                ),
                FORGET => command("forget", "잊어 줘", None, None, Some("녹차")),
                "홍차 기억을 지워 줘." => {
                    command("forget", "지워 줘", None, None, Some("홍차"))
                }
                _ => panic!("unexpected synthetic command"),
            },
        };
        (
            json!({"tool_calls":[{"index":0,"id":format!("decision-{call}"),"type":"function",
            "function":{"name":"memory_command_decision","arguments":args.to_string()}}]}),
            "tool_calls",
        )
    } else {
        assert_eq!(input["max_tokens"], 4096);
        assert!(!input.to_string().contains(PRIVATE_PREVIEW));
        assert!(!input.to_string().contains("memory_command_decision"));
        let results = messages
            .iter()
            .filter(|m| m["role"] == "tool")
            .collect::<Vec<_>>();
        if matches!(state.mode, Mode::NoCommand) {
            assert!(
                results.is_empty(),
                "no-command must discard the classifier call/capsule"
            );
            (json!({"content":"합성 일반 대화 답변입니다."}), "stop")
        } else if results.is_empty() {
            (
                json!({"tool_calls":[{"index":0,"id":"search-memory","type":"function","function":{
                "name":"knowledge_search","arguments":json!({"kind":"memory","text":"홍차",
                    "collection_id":null,"speaker":null,"subject":null,"valid_at":null}).to_string()}}]}),
                "tool_calls",
            )
        } else {
            let value: Value =
                serde_json::from_str(results.last().unwrap()["content"].as_str().unwrap()).unwrap();
            assert_eq!(value["kind"], "search");
            assert_eq!(value["value"]["hits"][0]["snippet"], "나는 홍차를 좋아해");
            (
                json!({"content":"홍차를 좋아한다고 저장되어 있어요."}),
                "stop",
            )
        }
    };
    let id = format!("memory-response-{call}");
    let head = frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{
        "role":"assistant","content":if planning {PRIVATE_PREVIEW}else{""}},"finish_reason":null}],
        "usage":{"prompt_tokens":7,"completion_tokens":1}}),
    );
    let tail = frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":finish}]}),
    ) + &frame(
        json!({"id":id,"model":"fixture-model","choices":[],"usage":{"prompt_tokens":13,"completion_tokens":7}}),
    ) + "data: [DONE]\n\n";
    let stream = futures_util::stream::unfold(
        (0, head, tail, state),
        |(index, head, tail, state)| async move {
            let bytes = match index {
                0 => head.clone(),
                1 => {
                    if matches!(state.mode, Mode::Hold) {
                        let permit = state.release.clone().acquire_owned().await.unwrap();
                        permit.forget();
                    }
                    tail.clone()
                }
                _ => return None,
            };
            Some((
                Ok::<_, std::io::Error>(Bytes::from(bytes)),
                (index + 1, head, tail, state),
            ))
        },
    );
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}
async fn enabled(base: &str) -> (tempfile::TempDir, Store, UserContext, String) {
    let result = configured(base).await;
    let provider = result
        .1
        .list_providers(&result.2)
        .await
        .unwrap()
        .providers
        .remove(0);
    result
        .1
        .record_tool_probe(
            &result.2,
            provider.id,
            provider.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    result
}
async fn ask_memory(store: &Store, user: &UserContext, thread: &str, text: &str) -> ChatTurn {
    store
        .submit_turn(
            user,
            thread.into(),
            uuid::Uuid::new_v4().to_string(),
            text.into(),
        )
        .await
        .unwrap()
}
async fn completed(store: &Store, user: &UserContext, thread: &str, run: &str) -> ChatTurn {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = store.conversation(user, thread.into(), None).await.unwrap();
            if let Some(turn) = page
                .turns
                .into_iter()
                .find(|turn| turn.run.id == run && !turn.run.state.active())
            {
                assert_eq!(turn.run.state, RunState::Completed);
                assert!(
                    !turn
                        .assistant
                        .iter()
                        .any(|v| v.text.contains(PRIVATE_PREVIEW))
                );
                return turn;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn explicit_commands_recall_correct_self_context_forget_and_reopen_without_duplicate_write() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::Commands).await;
    let (dir, store, user, thread) = enabled(&model.base).await;
    let runtime = Runtime::start(store.clone());
    let submitted = ask_memory(&store, &user, &thread, REMEMBER).await;
    runtime.wake();
    let first = completed(&store, &user, &thread, &submitted.run.id).await;
    let memories = store.memories(&user, None).await.unwrap().items;
    assert_eq!(memories.len(), 1);
    assert_eq!(memories[0].draft.text, "나는 홍차를 좋아해");
    let memory_id = memories[0].id.clone();
    assert_eq!(first.run.evidence[0].id, memory_id);
    store
        .submit_turn(
            &user,
            thread.clone(),
            submitted.run.request_id,
            REMEMBER.into(),
        )
        .await
        .unwrap();
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 1);
    let another = store
        .create_thread(&user, "Separate recall".into())
        .await
        .unwrap();
    let recall = ask_memory(&store, &user, &another.id, "내가 좋아하는 음료는 무엇이야?").await;
    runtime.wake();
    let recall = completed(&store, &user, &another.id, &recall.run.id).await;
    assert_eq!(recall.run.evidence[0].id, memory_id);
    let correction = ask_memory(&store, &user, &thread, CORRECT).await;
    runtime.wake();
    let correction = completed(&store, &user, &thread, &correction.run.id).await;
    assert_eq!(correction.run.evidence[0].revision, 2);
    assert_eq!(
        store
            .memory(&user, memory_id.clone(), None)
            .await
            .unwrap()
            .draft
            .text,
        "나는 녹차를 좋아해"
    );
    let prior = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap();
    assert!(
        prior.turns[0].assistant.is_empty(),
        "old remembered fact confirmation must be revoked"
    );
    assert!(
        store
            .conversation(&user, another.id, None)
            .await
            .unwrap()
            .turns[0]
            .assistant
            .is_empty()
    );
    let forgotten = ask_memory(&store, &user, &thread, FORGET).await;
    runtime.wake();
    let forgotten = completed(&store, &user, &thread, &forgotten.run.id).await;
    assert!(forgotten.run.evidence.is_empty());
    assert_eq!(forgotten.assistant.len(), 1);
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    let receipt = store
        .memory_command_receipt(&user, forgotten.run.id.clone())
        .await
        .unwrap();
    assert!(receipt.is_some());
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    assert!(
        store
            .memory_command_receipt(&user, forgotten.run.id)
            .await
            .unwrap()
            .is_some()
    );
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 5);
}

#[tokio::test]
async fn no_command_drops_private_planner_history_but_retains_both_call_receipts() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::NoCommand).await;
    let (_dir, store, user, thread) = enabled(&model.base).await;
    let runtime = Runtime::start(store.clone());
    let run = ask_memory(&store, &user, &thread, "무수리는 기억 기능이 있어?").await;
    runtime.wake();
    let result = completed(&store, &user, &thread, &run.run.id).await;
    assert_eq!(result.assistant.len(), 1);
    assert_eq!(result.assistant[0].text, "합성 일반 대화 답변입니다.");
    assert_eq!(result.run.usage.input_tokens, Some(26));
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 2);
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    runtime.shutdown().await;
}

#[tokio::test]
async fn malformed_intent_cannot_save_a_fact_absent_from_the_current_user_text() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::ForgedQuote).await;
    let (_dir, store, user, thread) = enabled(&model.base).await;
    let runtime = Runtime::start(store.clone());
    ask_memory(&store, &user, &thread, REMEMBER).await;
    runtime.wake();
    let result = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(result.run.state, RunState::Failed);
    assert!(result.assistant.is_empty());
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 1);
    runtime.shutdown().await;
}

#[tokio::test]
async fn planner_previews_are_hidden_and_cancel_before_commit_prevents_mutation() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::Hold).await;
    let (_dir, store, user, thread) = enabled(&model.base).await;
    let runtime = Runtime::start(store.clone());
    let run = ask_memory(&store, &user, &thread, REMEMBER).await;
    runtime.wake();
    let partial = wait_for(&store, &user, &thread, |turn| {
        turn.run.usage.input_tokens == Some(7)
    })
    .await;
    assert!(partial.assistant.is_empty());
    let replay = store.chat_events(&user, thread.clone(), 0).await.unwrap();
    assert!(
        !serde_json::to_string(&replay)
            .unwrap()
            .contains(PRIVATE_PREVIEW)
    );
    store.cancel_run(&user, run.run.id, 1).await.unwrap();
    runtime.wake();
    model.state.release.add_permits(1);
    runtime.shutdown().await;
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    assert!(
        store.conversation(&user, thread, None).await.unwrap().turns[0]
            .assistant
            .is_empty()
    );
}

#[tokio::test]
async fn ambiguous_forget_has_a_durable_clarification_and_removes_nothing() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::Commands).await;
    let (_dir, store, user, thread) = enabled(&model.base).await;
    for text in ["나는 홍차를 좋아해", "홍차 선물은 금요일에 주문"] {
        store
            .create_memory(
                &user,
                uuid::Uuid::new_v4().to_string(),
                MemoryDraft {
                    text: text.into(),
                    subject: "합성 취향".into(),
                    speaker: None,
                    valid_from: None,
                    valid_until: None,
                    evidence: vec![],
                },
            )
            .await
            .unwrap();
    }
    let runtime = Runtime::start(store.clone());
    let run = ask_memory(&store, &user, &thread, "홍차 기억을 지워 줘.").await;
    runtime.wake();
    let result = completed(&store, &user, &thread, &run.run.id).await;
    assert_eq!(result.assistant.len(), 1);
    assert!(result.run.evidence.is_empty());
    assert_eq!(store.memories(&user, None).await.unwrap().items.len(), 2);
    assert!(matches!(
        store
            .memory_command_receipt(&user, run.run.id)
            .await
            .unwrap(),
        Some(moosuri_store::MemoryCommandOutcome::Clarify { .. })
    ));
    runtime.shutdown().await;
}

#[tokio::test]
async fn retry_resumes_a_sealed_local_intent_without_a_replacement_model_call() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::Commands).await;
    let (dir, store, user, thread) = enabled(&model.base).await;
    let run = ask_memory(&store, &user, &thread, REMEMBER).await;
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"synthetic":"completed planning"}"#.to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .prepare_memory_command(
            &step,
            MemoryCommandIntent {
                action: MemoryCommandAction::Remember,
                intent_quote: "기억해 줘".into(),
                fact_quote: Some("나는 홍차를 좋아해".into()),
                subject: Some("음료 취향".into()),
                target_quote: None,
            },
        )
        .await
        .unwrap();
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(
        store.memories(&user, None).await.unwrap().items.is_empty(),
        "prepared is not committed"
    );
    store.retry_run(&user, run.run.id.clone(), 1).await.unwrap();
    let runtime = Runtime::start(store.clone());
    runtime.wake();
    let result = completed(&store, &user, &thread, &run.run.id).await;
    assert_eq!(result.run.attempt, 2);
    assert_eq!(
        model.state.calls.load(Ordering::SeqCst),
        0,
        "sealed plan must not be generated again"
    );
    assert_eq!(store.memories(&user, None).await.unwrap().items.len(), 1);
    runtime.shutdown().await;
}

#[tokio::test]
async fn retry_after_a_knowledge_read_skips_new_memory_planning() {
    let _guard = TEST_LOCK.lock().await;
    let model = memory_model(Mode::RetryAfterRead).await;
    let (_dir, store, user, thread) = enabled(&model.base).await;
    store
        .create_memory(
            &user,
            uuid::Uuid::new_v4().to_string(),
            MemoryDraft {
                text: "나는 홍차를 좋아해".into(),
                subject: "음료 취향".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    let submitted = ask_memory(&store, &user, &thread, "홍차 기억을 찾아 줘.").await;
    runtime.wake();
    let first = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_ne!(first.run.state, RunState::Completed);
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 3);
    store
        .retry_run(&user, submitted.run.id.clone(), 1)
        .await
        .unwrap();
    runtime.wake();
    let result = completed(&store, &user, &thread, &submitted.run.id).await;
    assert_eq!(result.run.attempt, 2);
    assert_eq!(result.run.evidence.len(), 1);
    assert_eq!(model.state.calls.load(Ordering::SeqCst), 5);
    let requests = model.state.requests.lock().await;
    assert_eq!(
        requests
            .iter()
            .filter(|r| r["tools"][0]["function"]["name"] == "memory_command_decision")
            .count(),
        1
    );
    drop(requests);
    assert_eq!(store.memories(&user, None).await.unwrap().items.len(), 1);
    runtime.shutdown().await;
}
