//! Contract tests through the real supervisor, writer and loopback HTTP adapter.
//! The scripted provider sees tool results, never an injected retrieval answer.
use super::*;
use moosuri_core::{IngestBatch, KnowledgeKind, SourceInput};
use std::collections::BTreeSet;

const SOURCE_TITLE: &str = "합성 제주 여행 메모";
const SOURCE_TEXT: &str = "합성 제주 여행 일정은 목요일이며 준비물은 홍차입니다.";
const FOREIGN_TEXT: &str = "합성 다른 사용자 전용 비공개 사실";
const QUERY: &str = "제주여행";

#[derive(Clone, Copy)]
enum Scenario {
    Chain,
    Browse,
    Repair,
    DeleteWhileHeld,
    Loop,
    RetryBudget,
    Overflow,
}
struct Script {
    scenario: Scenario,
    calls: AtomicUsize,
    requests: tokio::sync::Mutex<Vec<Value>>,
    foreign: tokio::sync::Mutex<Option<(String, String)>>,
    release: Arc<Semaphore>,
}
struct ScriptFixture {
    state: Arc<Script>,
    base: String,
    task: JoinHandle<()>,
}
impl Drop for ScriptFixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn search_args(text: &str) -> Value {
    json!({"kind":"source","text":text,"collection_id":null,"subject":null,"speaker":null,"valid_at":null})
}
fn read_args(id: &str, revision: i64) -> Value {
    json!({"id":id,"revision":revision,"start_byte":0,"max_bytes":4096})
}
pub(super) fn tool_call(index: usize, id: String, name: &str, args: Value) -> Value {
    json!({"index":index,"id":id,"type":"function","function":{"name":name,"arguments":args.to_string()}})
}
pub(super) fn result_values(body: &Value) -> Vec<Value> {
    body["messages"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| {
            let mut value: Value = serde_json::from_str(m["content"].as_str().unwrap()).unwrap();
            if value["is_error"] == true {
                value = serde_json::from_str(value["output"].as_str().unwrap()).unwrap();
            }
            value
        })
        .collect()
}
async fn script_fixture(scenario: Scenario) -> ScriptFixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(Script {
        scenario,
        calls: AtomicUsize::new(0),
        requests: tokio::sync::Mutex::new(vec![]),
        foreign: tokio::sync::Mutex::new(None),
        release: Arc::new(Semaphore::new(0)),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(scripted_response))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    ScriptFixture { state, base, task }
}
async fn scripted_response(
    State(state): State<Arc<Script>>,
    axum::Json(body): axum::Json<Value>,
) -> Response {
    let step = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
    assert_eq!(body["model"], "fixture-model");
    assert_eq!(body["max_tokens"], 4096);
    let names = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|d| d["function"]["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        names,
        BTreeSet::from([
            "knowledge_search",
            "knowledge_read_source",
            "knowledge_read_memory",
            "skill_read",
            "decimal_calculate",
            "artifact_search",
            "artifact_read",
            "followup_briefing"
        ])
    );
    state.requests.lock().await.push(body.clone());
    let prior = result_values(&body);
    let mut calls = vec![];
    let mut answer = None;
    match state.scenario {
        Scenario::Browse => match step {
            1 => calls.push(tool_call(
                0,
                "browse-first".into(),
                "knowledge_search",
                json!({"kind":"memory","text":"","collection_id":null,"subject":null,
                    "speaker":null,"valid_at":null,"cursor":null}),
            )),
            2 => {
                let result = prior.last().unwrap();
                assert_eq!(result["kind"], "search");
                assert_eq!(result["value"]["hits"].as_array().unwrap().len(), 5);
                assert_eq!(result["value"]["truncated"], false);
                assert!(result["value"]["next_cursor"].is_string());
                calls.push(tool_call(
                    0,
                    "browse-next".into(),
                    "knowledge_search",
                    json!({"kind":"memory","text":"","collection_id":null,"subject":null,
                        "speaker":null,"valid_at":null,"cursor":result["value"]["next_cursor"]}),
                ));
            }
            3 => {
                let result = prior.last().unwrap();
                let hits = result["value"]["hits"].as_array().unwrap();
                assert_eq!(hits.len(), 1);
                assert!(result["value"]["next_cursor"].is_null());
                calls.push(tool_call(
                    0,
                    "browse-read".into(),
                    "knowledge_read_memory",
                    json!({"id":hits[0]["id"],"revision":hits[0]["revision"]}),
                ));
            }
            4 => {
                let result = prior.last().unwrap();
                assert_eq!(result["kind"], "memory");
                answer = Some(format!(
                    "저장된 선호: {}",
                    result["value"]["draft"]["text"].as_str().unwrap()
                ));
            }
            _ => panic!("unexpected extra browse continuation"),
        },
        Scenario::Loop | Scenario::RetryBudget => {
            assert!(step <= 6, "runtime exceeded persisted model budget");
            for i in 0..2 {
                let number = (step - 1) * 2 + i + 1;
                calls.push(tool_call(
                    i,
                    format!("loop-{number}"),
                    "knowledge_search",
                    search_args(&format!("loop-{number:02}")),
                ));
            }
        }
        Scenario::Overflow => {
            assert!(
                step <= 2,
                "overflow triggered an automatic extra model call"
            );
            let count = if step == 1 { 8 } else { 5 };
            for i in 0..count {
                let number = if step == 1 { i + 1 } else { 9 + i };
                calls.push(tool_call(
                    i,
                    format!("overflow-{number}"),
                    "knowledge_search",
                    search_args(&format!("loop-{number:02}")),
                ));
            }
        }
        Scenario::Repair if step == 1 => {
            calls.push(tool_call(
                0,
                "large-search".into(),
                "knowledge_search",
                search_args(&"x".repeat(5 * 1024)),
            ));
        }
        Scenario::Repair if step == 2 => {
            assert_eq!(
                prior.last().unwrap(),
                &json!({"kind":"error","value":"invalid_input"})
            );
            let foreign = state.foreign.lock().await.clone().unwrap();
            let mut args = read_args(&foreign.0, 1);
            args["tenant_id"] = json!(foreign.1);
            args["command"] = json!("untrusted-synthetic-command");
            calls.push(tool_call(
                0,
                "malformed-call".into(),
                "knowledge_read_source",
                args,
            ));
        }
        Scenario::Repair if step == 3 => {
            assert_eq!(
                prior.last().unwrap(),
                &json!({"kind":"error","value":"invalid_input"})
            );
            let foreign = state.foreign.lock().await.clone().unwrap();
            calls.push(tool_call(
                0,
                "foreign-call".into(),
                "knowledge_read_source",
                read_args(&foreign.0, 1),
            ));
        }
        _ => {
            let offset = if matches!(state.scenario, Scenario::Repair) {
                3
            } else {
                0
            };
            match step - offset {
                1 => {
                    if offset > 0 {
                        assert_eq!(
                            prior.last().unwrap(),
                            &json!({"kind":"error","value":"not_found"})
                        );
                    }
                    calls.push(tool_call(
                        0,
                        "search-call".into(),
                        "knowledge_search",
                        search_args(QUERY),
                    ));
                }
                2 => {
                    let result = prior.last().unwrap();
                    assert_eq!(result["kind"], "search");
                    let hits = result["value"]["hits"].as_array().unwrap();
                    assert_eq!(hits.len(), 1);
                    let hit = &hits[0];
                    calls.push(tool_call(
                        0,
                        "read-call".into(),
                        "knowledge_read_source",
                        read_args(
                            hit["id"].as_str().unwrap(),
                            hit["revision"].as_i64().unwrap(),
                        ),
                    ));
                }
                3 => {
                    let result = prior.last().unwrap();
                    assert_eq!(result["kind"], "source");
                    assert_eq!(result["value"]["start_byte"], 0);
                    assert_eq!(result["value"]["has_more"], false);
                    answer = Some(format!(
                        "기록: {}",
                        result["value"]["text"].as_str().unwrap()
                    ));
                }
                _ => panic!("unexpected extra continuation"),
            }
        }
    }
    let final_round = answer.is_some();
    let prefix_text = answer.unwrap_or_else(|| format!("합성 도구 단계 {step}."));
    let id = format!("fixture-round-{step}");
    let late_text = prefix_text.clone();
    let prefix = frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":prefix_text},"finish_reason":null}],"usage":{"prompt_tokens":step*10,"completion_tokens":step}}),
    );
    let mut tail = if final_round {
        frame(
            json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":step*10,"completion_tokens":step*2}}),
        ) + "data: [DONE]\n\n"
    } else {
        frame(
            json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{"tool_calls":calls},"finish_reason":null}]}),
        ) + &frame(
            json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}],"usage":{"prompt_tokens":step*10,"completion_tokens":step*2}}),
        ) + "data: [DONE]\n\n"
    };
    let held = final_round && matches!(state.scenario, Scenario::DeleteWhileHeld);
    if held {
        tail = frame(
            json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{"content":format!(" 늦은 자료: {late_text}")},"finish_reason":null}]}),
        ) + &tail;
    }
    let truncated = matches!(state.scenario, Scenario::RetryBudget) && step == 2;
    let stream = futures_util::stream::unfold(
        (0, state, prefix, tail),
        move |(stage, state, prefix, tail)| async move {
            let bytes = match stage {
                0 => prefix.clone(),
                1 => {
                    if truncated {
                        return None;
                    }
                    if held {
                        let permit = state.release.clone().acquire_owned().await.unwrap();
                        permit.forget();
                    }
                    tail.clone()
                }
                _ => return None,
            };
            Some((
                Ok::<_, std::io::Error>(Bytes::from(bytes)),
                (stage + 1, state, prefix, tail),
            ))
        },
    );
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}
pub(super) async fn tool_configured(base: &str) -> (tempfile::TempDir, Store, UserContext, String) {
    let value = configured(base).await;
    let provider = value
        .1
        .list_providers(&value.2)
        .await
        .unwrap()
        .providers
        .remove(0);
    value
        .1
        .record_tool_probe(
            &value.2,
            provider.id,
            provider.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    value
}
pub(super) async fn seed(
    store: &Store,
    user: &UserContext,
    title: &str,
    texts: Vec<String>,
) -> Vec<String> {
    let collection = store
        .create_collection(
            user,
            "합성 테스트 자료".into(),
            uuid::Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    let entries = texts
        .into_iter()
        .enumerate()
        .map(|(index, text)| SourceInput {
            external_id: format!("synthetic-{index}"),
            version: "v1".into(),
            expected_revision: None,
            title: if title == SOURCE_TITLE {
                title.into()
            } else {
                format!("{title} {index:02}")
            },
            text,
            speaker: Some("합성 본인".into()),
            observed_at: 1_700_000_000,
            valid_from: None,
            valid_until: None,
        })
        .collect();
    store
        .ingest_text(
            user,
            collection.id,
            IngestBatch {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries,
            },
        )
        .await
        .unwrap()
        .source_ids
}
async fn ask(store: &Store, user: &UserContext, thread: &str) -> ChatTurn {
    store
        .submit_turn(
            user,
            thread.into(),
            uuid::Uuid::new_v4().to_string(),
            "저장한 제주 여행 기록에서 일정과 준비물을 찾아 알려줘.".into(),
        )
        .await
        .unwrap()
}
fn evidence(turn: &ChatTurn, id: &str) {
    assert_eq!(turn.run.evidence.len(), 1);
    assert_eq!(turn.run.evidence[0].id, id);
    assert_eq!(turn.run.evidence[0].revision, 1);
    assert_eq!(turn.run.evidence[0].kind, KnowledgeKind::Source);
    assert_eq!(turn.run.evidence[0].title, SOURCE_TITLE);
}
async fn accounting(store: &Store, user: &UserContext, turn: &ChatTurn, calls: i64) {
    let sum = (calls * (calls + 1) / 2) as u64;
    let usage = store.usage_summary(user).await.unwrap();
    assert_eq!(usage.dispatched_calls, calls);
    assert_eq!(usage.unreported_calls, 0);
    assert_eq!(usage.reported_input_tokens, sum * 10);
    assert_eq!(usage.reported_output_tokens, sum * 2);
    assert_eq!(turn.run.usage.input_tokens, Some(sum * 10));
    assert_eq!(turn.run.usage.output_tokens, Some(sum * 2));
}

#[tokio::test]
async fn search_excerpt_final_chain_has_distinct_output_items_and_durable_step_receipts() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::Chain).await;
    let (dir, store, user, thread) = tool_configured(&fixture.base).await;
    let id = seed(&store, &user, SOURCE_TITLE, vec![SOURCE_TEXT.into()])
        .await
        .remove(0);
    let runtime = Runtime::start(store.clone());
    ask(&store, &user, &thread).await;
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 3);
    assert_eq!(done.assistant.len(), 3);
    assert_eq!(
        done.assistant
            .iter()
            .map(|m| &m.id)
            .collect::<BTreeSet<_>>()
            .len(),
        3
    );
    assert_eq!(done.assistant[0].text, "합성 도구 단계 1.");
    assert_eq!(done.assistant[1].text, "합성 도구 단계 2.");
    assert_eq!(done.assistant[2].text, format!("기록: {SOURCE_TEXT}"));
    evidence(&done, &id);
    accounting(&store, &user, &done, 3).await;
    let requests = fixture.state.requests.lock().await;
    assert_eq!(result_values(&requests[1])[0]["value"]["hits"][0]["id"], id);
    assert_eq!(
        result_values(&requests[2]).last().unwrap()["value"]["text"],
        SOURCE_TEXT
    );
    drop(requests);
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let saved = reopened
        .conversation(&user, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(saved.run.state, RunState::Completed);
    evidence(&saved, &id);
    accounting(&reopened, &user, &saved, 3).await;
}

#[tokio::test]
async fn malformed_typed_input_and_foreign_source_read_are_sanitized_before_repair() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::Repair).await;
    let (_dir, store, user, thread) = tool_configured(&fixture.base).await;
    let own = seed(&store, &user, SOURCE_TITLE, vec![SOURCE_TEXT.into()])
        .await
        .remove(0);
    let invite = store
        .create_user(
            &user,
            "Synthetic member".into(),
            "knowledge-member@example.test".into(),
        )
        .await
        .unwrap();
    let member = store
        .login(
            "knowledge-member@example.test".into(),
            invite.temporary_password,
        )
        .await
        .unwrap();
    let member = store
        .change_password(&member.user, PASSWORD.into())
        .await
        .unwrap()
        .user;
    let foreign = seed(
        &store,
        &member,
        "다른 사용자 전용 제목",
        vec![format!("{QUERY} {FOREIGN_TEXT}")],
    )
    .await
    .remove(0);
    *fixture.state.foreign.lock().await = Some((foreign.clone(), member.account().id.clone()));
    let runtime = Runtime::start(store.clone());
    ask(&store, &user, &thread).await;
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 6);
    evidence(&done, &own);
    assert!(!text(&done).contains(FOREIGN_TEXT));
    accounting(&store, &user, &done, 6).await;
    let requests = fixture.state.requests.lock().await;
    let first = result_values(&requests[1]);
    assert_eq!(first[0], json!({"kind":"error","value":"invalid_input"}));
    let second = result_values(&requests[2]);
    assert_eq!(second[1], json!({"kind":"error","value":"invalid_input"}));
    let third = result_values(&requests[3]);
    assert_eq!(third[2], json!({"kind":"error","value":"not_found"}));
    for request in requests.iter() {
        for result in result_values(request) {
            let serialized = result.to_string();
            assert!(!serialized.contains(FOREIGN_TEXT));
            assert!(!serialized.contains("untrusted-synthetic-command"));
            assert!(!serialized.contains(&foreign));
        }
    }
    drop(requests);
    runtime.shutdown().await;
}

#[tokio::test]
async fn deleting_delivered_source_purges_output_evidence_and_replay_before_late_final_bytes() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::DeleteWhileHeld).await;
    let (dir, store, user, thread) = tool_configured(&fixture.base).await;
    let id = seed(&store, &user, SOURCE_TITLE, vec![SOURCE_TEXT.into()])
        .await
        .remove(0);
    let runtime = Runtime::start(store.clone());
    ask(&store, &user, &thread).await;
    runtime.wake();
    let partial = wait_for(&store, &user, &thread, |turn| {
        text(turn).contains(SOURCE_TEXT)
    })
    .await;
    assert_eq!(partial.run.state, RunState::Running);
    evidence(&partial, &id);
    let cursor = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .cursor;
    store.delete_source(&user, id.clone(), 1).await.unwrap();
    fixture.state.release.add_permits(1);
    runtime.wake();
    tokio::time::sleep(Duration::from_millis(300)).await;
    let done = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.state, RunState::Incomplete);
    assert_eq!(done.run.error.as_deref(), Some("knowledge_invalidated"));
    assert!(!done.run.can_retry);
    assert!(done.assistant.is_empty());
    assert!(done.run.evidence.is_empty());
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 3);
    for after in [0, cursor] {
        let replay = store
            .chat_events(&user, thread.clone(), after)
            .await
            .unwrap();
        assert!(replay.reset);
        assert!(
            !serde_json::to_string(&replay)
                .unwrap()
                .contains(SOURCE_TEXT)
        );
    }
    assert!(matches!(
        store.source(&user, id, None).await,
        Err(moosuri_store::StoreError::NotFound)
    ));
    let usage = store.usage_summary(&user).await.unwrap();
    assert_eq!(usage.dispatched_calls, 3);
    assert_eq!(usage.reported_input_tokens, 60);
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let page = reopened.conversation(&user, thread, None).await.unwrap();
    assert!(page.turns[0].assistant.is_empty());
    assert!(page.turns[0].run.evidence.is_empty());
    assert_eq!(
        reopened
            .usage_summary(&user)
            .await
            .unwrap()
            .dispatched_calls,
        3
    );
}

#[tokio::test]
async fn repeated_tool_rounds_stop_at_six_model_calls_and_twelve_executed_slots_after_reopen() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::Loop).await;
    let (dir, store, user, thread) = tool_configured(&fixture.base).await;
    let ids = seed(
        &store,
        &user,
        "합성 반복 항목",
        (1..=12)
            .map(|i| format!("loop-{i:02} 독립 합성 자료"))
            .collect(),
    )
    .await;
    let runtime = Runtime::start(store.clone());
    let submitted = ask(&store, &user, &thread).await;
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Incomplete);
    assert_eq!(done.run.error.as_deref(), Some("budget_exhausted"));
    assert!(!done.run.can_retry);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 6);
    assert_eq!(
        done.run
            .evidence
            .iter()
            .map(|e| e.id.clone())
            .collect::<BTreeSet<_>>(),
        ids.into_iter().collect::<BTreeSet<_>>()
    );
    accounting(&store, &user, &done, 6).await;
    let requests = fixture.state.requests.lock().await;
    for (index, request) in requests.iter().enumerate() {
        assert_eq!(result_values(request).len(), index * 2);
    }
    drop(requests);
    assert!(matches!(
        store.retry_run(&user, submitted.run.id.clone(), 1).await,
        Err(moosuri_store::StoreError::Conflict)
    ));
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let saved = reopened
        .conversation(&user, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert!(!saved.run.can_retry);
    assert!(matches!(
        reopened.retry_run(&user, submitted.run.id, 1).await,
        Err(moosuri_store::StoreError::Conflict)
    ));
    accounting(&reopened, &user, &saved, 6).await;
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 6);
}

#[tokio::test]
async fn retry_keeps_persisted_model_budget_and_reports_exhaustion_instead_of_revocation() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::RetryBudget).await;
    let (dir, store, user, thread) = tool_configured(&fixture.base).await;
    seed(
        &store,
        &user,
        "합성 재시도 항목",
        (1..=12)
            .map(|i| format!("loop-{i:02} 독립 합성 자료"))
            .collect(),
    )
    .await;
    let runtime = Runtime::start(store.clone());
    let first = ask(&store, &user, &thread).await;
    runtime.wake();
    let interrupted = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(interrupted.run.state, RunState::OutcomeUnknown);
    assert!(interrupted.run.can_retry);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 2);
    store
        .retry_run(&user, first.run.id.clone(), 1)
        .await
        .unwrap();
    runtime.wake();
    let exhausted = wait_for(&store, &user, &thread, |turn| {
        turn.run.attempt == 2 && !turn.run.state.active()
    })
    .await;
    runtime.shutdown().await;
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 6);
    assert!(!exhausted.run.can_retry);
    let usage = store.usage_summary(&user).await.unwrap();
    assert_eq!(usage.dispatched_calls, 6);
    drop(runtime);
    drop(store);
    let reopened = Store::open(dir.path()).unwrap();
    let saved = reopened
        .conversation(&user, thread, None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert!(!saved.run.can_retry);
    assert_eq!(
        reopened
            .usage_summary(&user)
            .await
            .unwrap()
            .dispatched_calls,
        6
    );
    assert_eq!(
        exhausted.run.state,
        RunState::Incomplete,
        "six persisted model dispatches were consumed across attempts (error {:?}, can_retry {})",
        exhausted.run.error,
        exhausted.run.can_retry
    );
    assert_eq!(exhausted.run.error.as_deref(), Some("budget_exhausted"));
}

#[tokio::test]
async fn oversized_next_tool_batch_is_atomic_and_reports_budget_exhaustion() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::Overflow).await;
    let (_dir, store, user, thread) = tool_configured(&fixture.base).await;
    let ids = seed(
        &store,
        &user,
        "합성 호출 상한 항목",
        (1..=13)
            .map(|i| format!("loop-{i:02} 독립 합성 자료"))
            .collect(),
    )
    .await;
    let runtime = Runtime::start(store.clone());
    ask(&store, &user, &thread).await;
    runtime.wake();
    let stopped = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    runtime.shutdown().await;
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 2);
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        2
    );
    assert_eq!(
        stopped
            .run
            .evidence
            .iter()
            .map(|e| e.id.clone())
            .collect::<BTreeSet<_>>(),
        ids[..8].iter().cloned().collect::<BTreeSet<_>>(),
        "over-budget call batch must not execute a prefix"
    );
    assert_eq!(
        stopped.run.state,
        RunState::Incomplete,
        "eight committed tool calls plus five proposed calls exceeds twelve (error {:?}, can_retry {})",
        stopped.run.error,
        stopped.run.can_retry
    );
    assert_eq!(stopped.run.error.as_deref(), Some("budget_exhausted"));
}

#[test]
fn memory_browse_query_mapping_preserves_invalid_argument_boundaries() {
    use crate::knowledge_tools::{Invocation, parse};
    let args = |kind: &str, text: &str| {
        json!({"kind":kind,"text":text,
        "collection_id":null,"subject":null,"speaker":null,"valid_at":null})
    };
    for (kind, text) in [
        ("source", ""),
        ("memory", " "),
        ("memory", "\n"),
        ("source", " "),
    ] {
        assert!(matches!(
            parse("knowledge_search", &args(kind, text).to_string()),
            Invocation::InvalidArguments
        ));
    }
    let mut browse = args("memory", "");
    browse["cursor"] = json!("returned-scoped-cursor");
    match parse("knowledge_search", &browse.to_string()) {
        Invocation::Search(query) => {
            assert_eq!(query.kind, KnowledgeKind::Memory);
            assert_eq!(query.text, "");
            assert_eq!(query.cursor.as_deref(), Some("returned-scoped-cursor"));
        }
        _ => panic!("valid read-only browse rejected"),
    }
    browse["cursor"] = json!(42);
    assert!(matches!(
        parse("knowledge_search", &browse.to_string()),
        Invocation::InvalidArguments
    ));
    let mut unknown = args("memory", "");
    unknown["tenant_id"] = json!("untrusted");
    assert!(matches!(
        parse("knowledge_search", &unknown.to_string()),
        Invocation::InvalidArguments
    ));
}

#[tokio::test]
async fn broad_personal_recall_browses_scoped_pages_and_forget_revokes_grounded_answer() {
    let _guard = TEST_LOCK.lock().await;
    let fixture = script_fixture(Scenario::Browse).await;
    let (_dir, store, user, thread) = tool_configured(&fixture.base).await;
    let mut memories = vec![];
    for i in 0..6 {
        memories.push(
            store
                .create_memory(
                    &user,
                    uuid::Uuid::new_v4().to_string(),
                    moosuri_core::MemoryDraft {
                        text: format!("합성 별도 사실 {i}"),
                        subject: "합성 항목".into(),
                        speaker: None,
                        valid_from: None,
                        valid_until: None,
                        evidence: vec![],
                    },
                )
                .await
                .unwrap(),
        );
    }
    memories.sort_by(|a, b| a.id.cmp(&b.id));
    let target = memories.last().unwrap();
    let fact = "나는 홍차를 좋아해.";
    store
        .correct_memory(
            &user,
            target.id.clone(),
            1,
            uuid::Uuid::new_v4().to_string(),
            moosuri_core::MemoryDraft {
                text: fact.into(),
                subject: "홍차 선호".into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    let runtime = Runtime::start(store.clone());
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "내가 좋아하는 음료는 무엇이야?".into(),
        )
        .await
        .unwrap();
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(fixture.state.calls.load(Ordering::SeqCst), 4);
    assert!(text(&done).contains(fact));
    assert!(
        done.run
            .evidence
            .iter()
            .any(|e| e.kind == KnowledgeKind::Memory && e.id == target.id && e.revision == 2)
    );
    let before = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .cursor;
    store
        .forget_memory(&user, target.id.clone(), 2)
        .await
        .unwrap();
    let revoked = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert!(revoked.assistant.is_empty());
    assert!(revoked.run.evidence.is_empty());
    for cursor in [0, before] {
        let events = store
            .chat_events(&user, thread.clone(), cursor)
            .await
            .unwrap();
        assert!(events.reset);
        assert!(!serde_json::to_string(&events).unwrap().contains(fact));
    }
    let mut browse = moosuri_core::KnowledgeQuery {
        kind: KnowledgeKind::Memory,
        text: "".into(),
        collection_id: None,
        subject: None,
        speaker: None,
        valid_at: None,
        cursor: None,
    };
    let after = store.search_knowledge(&user, browse.clone()).await.unwrap();
    assert!(!after.hits.iter().any(|h| h.id == target.id));
    // The cursor captured from the first five-hit page cannot survive forgetting.
    let requests = fixture.state.requests.lock().await;
    browse.cursor = Some(
        result_values(&requests[1])[0]["value"]["next_cursor"]
            .as_str()
            .unwrap()
            .into(),
    );
    assert!(matches!(
        store.search_knowledge(&user, browse).await,
        Err(moosuri_store::StoreError::Conflict)
    ));
    drop(requests);
    runtime.shutdown().await;
}
