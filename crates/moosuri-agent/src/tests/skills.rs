//! Delegated results through the actual supervisor, Store and HTTP tool transport.
//! Scripted upstream behavior proves execution boundaries, not model comprehension.
use super::knowledge::{result_values, seed, tool_call, tool_configured};
use super::*;

#[derive(Clone, Copy)]
enum Mode {
    Compare,
    HoldFinal,
    RetryAfterSkill,
}
struct Script {
    mode: Mode,
    calls: AtomicUsize,
    requests: tokio::sync::Mutex<Vec<Value>>,
    release: Arc<Semaphore>,
}
struct Server {
    base: String,
    state: Arc<Script>,
    task: JoinHandle<()>,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}
async fn server(mode: Mode) -> Server {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(Script {
        mode,
        calls: AtomicUsize::new(0),
        requests: tokio::sync::Mutex::new(vec![]),
        release: Arc::new(Semaphore::new(0)),
    });
    let app = Router::new()
        .route("/v1/chat/completions", post(response))
        .with_state(state.clone());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Server { base, state, task }
}
fn recipe_args(input: &Value) -> Value {
    let system = input["messages"][0]["content"].as_str().unwrap();
    assert!(
        !system.contains("# Evidence-based comparison"),
        "body must load progressively"
    );
    let metadata = system
        .lines()
        .find(|line| line.starts_with("evidence-comparison revision="))
        .unwrap();
    let mut parts = metadata.split_whitespace();
    let id = parts.next().unwrap();
    let revision = parts.next().unwrap().strip_prefix("revision=").unwrap();
    let digest = parts
        .next()
        .unwrap()
        .strip_prefix("digest=")
        .unwrap()
        .trim_end_matches(':');
    assert_eq!(digest.len(), 64);
    json!({"id":id,"revision":revision,"digest":digest})
}
async fn response(
    State(state): State<Arc<Script>>,
    axum::Json(input): axum::Json<Value>,
) -> Response {
    let step = state.calls.fetch_add(1, Ordering::SeqCst) + 1;
    state.requests.lock().await.push(input.clone());
    let planning = input["tools"][0]["function"]["name"] == "memory_command_decision";
    let prior = result_values(&input);
    let mut calls = vec![];
    let mut answer = None;
    if planning {
        assert!(matches!(state.mode, Mode::RetryAfterSkill));
        assert_eq!(
            step, 1,
            "retrieved recipe must not enable another intent plan"
        );
        assert_eq!(input["messages"].as_array().unwrap().len(), 2);
        calls.push(tool_call(0,"decision".into(),"memory_command_decision",json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null})));
    } else if matches!(state.mode, Mode::RetryAfterSkill) {
        match step {
            2 => calls.push(tool_call(
                0,
                "recipe".into(),
                "skill_read",
                recipe_args(&input),
            )),
            3 => {
                assert_eq!(prior.last().unwrap()["kind"], "skill");
                return Response::builder().status(503).body(Body::empty()).unwrap();
            }
            4 => {
                assert!(
                    prior.is_empty(),
                    "explicit retry must clear old private payloads"
                );
                answer=Some("비교에 필요한 저장 자료가 없어 견적 내용을 알려 주세요. 외부 작업은 수행하지 않았습니다.".to_owned());
            }
            _ => panic!("unexpected retry round"),
        }
    } else {
        match step {
            1 => {
                calls.push(tool_call(
                    0,
                    "recipe".into(),
                    "skill_read",
                    recipe_args(&input),
                ));
                calls.push(tool_call(1,"search".into(),"knowledge_search",json!({"kind":"source","text":"견적","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null})));
            }
            2 => {
                assert!(prior.iter().any(|v| {
                    v["kind"] == "skill"
                        && v["value"]["instructions"]
                            .as_str()
                            .unwrap()
                            .contains("Missing prices remain unknown")
                }));
                let search = prior.iter().find(|v| v["kind"] == "search").unwrap();
                let hits = search["value"]["hits"].as_array().unwrap();
                assert_eq!(hits.len(), 2);
                for (index, hit) in hits.iter().enumerate() {
                    calls.push(tool_call(index,format!("read-{index}"),"knowledge_read_source",json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096})));
                }
            }
            3 => {
                let sources = prior
                    .iter()
                    .filter(|v| v["kind"] == "source")
                    .collect::<Vec<_>>();
                assert_eq!(sources.len(), 2);
                for (index, source) in sources.into_iter().enumerate() {
                    let text = source["value"]["text"].as_str().unwrap();
                    let words = text.split_whitespace().collect::<Vec<_>>();
                    calls.push(tool_call(
                        index,
                        format!("total-{}", words[1]),
                        "decimal_calculate",
                        json!({"operation":"sum","operands":[words[3],words[5]]}),
                    ));
                }
            }
            4 => {
                let mut totals = prior
                    .iter()
                    .filter(|v| v["kind"] == "decimal")
                    .map(|v| v["value"]["exact"].as_str().unwrap())
                    .collect::<Vec<_>>();
                totals.sort_unstable();
                assert_eq!(totals, vec!["14.25", "15"]);
                answer = Some(format!(
                    "저장된 합성 견적을 기준으로 배송비 포함 A는 {}, B는 {}입니다. 비용 기준 B를 추천합니다. 근거: 합성 견적 00, 합성 견적 01. 유효기간과 실시간 재고는 확인되지 않았으며 구매는 수행하지 않았습니다.",
                    totals[1], totals[0]
                ));
            }
            _ => panic!("unexpected comparison round"),
        }
    }
    let final_round = answer.is_some();
    let id = format!("recipe-round-{step}");
    let head = frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":answer.unwrap_or_default()},"finish_reason":null}]}),
    );
    let delta = if final_round {
        json!({})
    } else {
        json!({"tool_calls":calls})
    };
    let tail = frame(
        json!({"id":id,"model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":if final_round {"stop"} else {"tool_calls"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}),
    ) + "data: [DONE]\n\n";
    let held = final_round && matches!(state.mode, Mode::HoldFinal);
    let stream = futures_util::stream::unfold(
        (0, head, tail, state),
        move |(index, head, tail, state)| async move {
            let bytes = match index {
                0 => head.clone(),
                1 => {
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
                (index + 1, head, tail, state),
            ))
        },
    );
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from_stream(stream))
        .unwrap()
}
async fn ask(store: &Store, user: &UserContext, thread: &str, text: &str) -> ChatTurn {
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

#[tokio::test]
async fn delegated_comparison_delivers_evidence_and_exact_totals_without_external_effects() {
    let _guard = TEST_LOCK.lock().await;
    let upstream = server(Mode::Compare).await;
    let (_dir, store, user, thread) = tool_configured(&upstream.base).await;
    let ids = seed(
        &store,
        &user,
        "합성 견적",
        vec![
            "견적 A amount 12.50 shipping 2.50".into(),
            "견적 B amount 14.00 shipping 0.25".into(),
        ],
    )
    .await;
    let runtime = Runtime::start(store.clone());
    ask(
        &store,
        &user,
        &thread,
        "저장한 견적 두 개를 배송비 포함 총액으로 비교하고 추천해 줘.",
    )
    .await;
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    let text = done
        .assistant
        .iter()
        .map(|v| v.text.as_str())
        .collect::<String>();
    assert!(
        text.contains("A는 15, B는 14.25")
            && text.contains("B를 추천")
            && text.contains("구매는 수행하지 않았습니다")
    );
    assert_eq!(done.run.evidence.len(), 2);
    assert!(ids.iter().all(|id| {
        done.run
            .evidence
            .iter()
            .any(|e| e.id == *id && e.revision == 1)
    }));
    assert_eq!(upstream.state.calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        4
    );
    runtime.shutdown().await;
}

#[tokio::test]
async fn deleting_evidence_purges_a_derived_math_answer_during_final_stream() {
    let _guard = TEST_LOCK.lock().await;
    let upstream = server(Mode::HoldFinal).await;
    let (_dir, store, user, thread) = tool_configured(&upstream.base).await;
    let ids = seed(
        &store,
        &user,
        "합성 견적",
        vec![
            "견적 A amount 12.50 shipping 2.50".into(),
            "견적 B amount 14.00 shipping 0.25".into(),
        ],
    )
    .await;
    let runtime = Runtime::start(store.clone());
    ask(&store, &user, &thread, "저장한 견적을 비교해 줘.").await;
    runtime.wake();
    wait_for(&store, &user, &thread, |turn| {
        turn.assistant.iter().any(|v| v.text.contains("14.25"))
    })
    .await;
    store.delete_source(&user, ids[0].clone(), 1).await.unwrap();
    upstream.state.release.add_permits(1);
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert!(done.assistant.is_empty() && done.run.evidence.is_empty());
    assert_eq!(done.run.error.as_deref(), Some("knowledge_invalidated"));
    runtime.shutdown().await;
}

#[tokio::test]
async fn retry_after_a_recipe_read_cannot_reenter_memory_intent_planning() {
    let _guard = TEST_LOCK.lock().await;
    let upstream = server(Mode::RetryAfterSkill).await;
    let (_dir, store, user, thread) = tool_configured(&upstream.base).await;
    let runtime = Runtime::start(store.clone());
    ask(&store, &user, &thread, "기억을 바탕으로 견적을 비교해 줘.").await;
    runtime.wake();
    let failed = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(failed.run.state, RunState::Failed);
    store.retry_run(&user, failed.run.id, 1).await.unwrap();
    runtime.wake();
    let done = wait_for(&store, &user, &thread, |turn| {
        turn.run.attempt == 2 && !turn.run.state.active()
    })
    .await;
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(upstream.state.calls.load(Ordering::SeqCst), 4);
    assert_eq!(
        upstream
            .state
            .requests
            .lock()
            .await
            .iter()
            .filter(|input| input["tools"][0]["function"]["name"] == "memory_command_decision")
            .count(),
        1
    );
    assert!(store.memories(&user, None).await.unwrap().items.is_empty());
    runtime.shutdown().await;
}
