use super::*;
use axum::{
    Router,
    body::{Body, Bytes},
    extract::State,
    response::Response,
    routing::post,
};
use moosuri_core::{ChatTurn, ModelSelection, ProviderConfig, ProviderKind};
use moosuri_store::{CredentialChange, ProviderChange, UserContext};
use serde_json::{Value, json};
use std::sync::atomic::{AtomicU8, AtomicUsize, Ordering};
use tokio::sync::Semaphore;

const PASSWORD: &str = "Synthetic-runtime-password-42";
struct Upstream {
    calls: AtomicUsize,
    mode: AtomicU8,
    release: Arc<Semaphore>,
}
impl Default for Upstream {
    fn default() -> Self {
        Self {
            calls: AtomicUsize::new(0),
            mode: AtomicU8::new(0),
            release: Arc::new(Semaphore::new(0)),
        }
    }
}
static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
struct Fixture {
    state: Arc<Upstream>,
    base: String,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}
fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}
async fn fixture() -> Fixture {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/v1", listener.local_addr().unwrap());
    let state = Arc::new(Upstream::default());
    let app=Router::new().route("/v1/chat/completions",post(|State(state):State<Arc<Upstream>>,axum::Json(body):axum::Json<Value>|async move{
        state.calls.fetch_add(1,Ordering::SeqCst);
        assert_eq!(body["model"],"fixture-model");assert_eq!(body["max_tokens"],4096);
        assert!(body["messages"][0]["content"].as_str().unwrap().contains("Tools, external actions"));
        let mode=state.mode.load(Ordering::SeqCst);
        let stream=futures_util::stream::unfold((0,state),move |(step,state)|async move{
            let value=match step {
                0=>{
                    let model=if mode==3 {"substituted-model"} else {"fixture-model"};
                    let text=if mode==5 {"🌱".repeat(6000)} else {"안녕 🌱".into()};
                    frame(json!({"id":"fixture-response","model":model,"choices":[{"index":0,"delta":{"role":"assistant","content":text},"finish_reason":null}],"usage":{"prompt_tokens":7,"completion_tokens":2}}))
                }
                1=>{
                    let permit=state.release.clone().acquire_owned().await.unwrap();permit.forget();
                    match mode {
                        6=>return None, // EOF before a terminal frame.
                        1=>frame(json!({"error":{"message":"synthetic-private-provider-marker"}})),
                        2=>frame(json!({"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"length"}]}))+"data: [DONE]\n\n",
                        4=>frame(json!({"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":{"refusal":"Cannot answer"},"finish_reason":"content_filter"}]}))+"data: [DONE]\n\n",
                        _=>frame(json!({"id":"fixture-response","model":"fixture-model","choices":[{"index":0,"delta":{"content":" done"},"finish_reason":"stop"}],"usage":{"prompt_tokens":7,"completion_tokens":4}}))+"data: [DONE]\n\n",
                    }
                }
                _=>return None,
            };
            Some((Ok::<_,std::io::Error>(Bytes::from(value)),(step+1,state)))
        });
        Response::builder().header("content-type","text/event-stream").body(Body::from_stream(stream)).unwrap()
    })).with_state(state.clone());
    let task = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    Fixture { state, base, task }
}
async fn configured(base: &str) -> (tempfile::TempDir, Store, UserContext, String) {
    let dir = tempfile::Builder::new()
        .prefix("moosuri-runtime-")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Synthetic Administrator".into(),
            "admin@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let user = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let provider = store
        .create_provider(
            &user,
            ProviderChange {
                name: "Local fixture".into(),
                enabled: true,
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(base.into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&user, provider.id.clone(), 1, "fixture-model".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                provider_id: provider.id,
                provider_revision: 1,
                model: "fixture-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Synthetic conversation".into())
        .await
        .unwrap()
        .id;
    (dir, store, user, thread)
}
async fn submit(store: &Store, user: &UserContext, thread: &str) -> ChatTurn {
    store
        .submit_turn(
            user,
            thread.into(),
            uuid::Uuid::new_v4().to_string(),
            "Synthetic question".into(),
        )
        .await
        .unwrap()
}
async fn wait_for(
    store: &Store,
    user: &UserContext,
    thread: &str,
    predicate: impl Fn(&ChatTurn) -> bool,
) -> ChatTurn {
    tokio::time::timeout(Duration::from_secs(4), async {
        loop {
            let page = store.conversation(user, thread.into(), None).await.unwrap();
            if let Some(turn) = page.turns.last()
                && predicate(turn)
            {
                return turn.clone();
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
fn text(turn: &ChatTurn) -> String {
    turn.assistant.iter().map(|m| m.text.as_str()).collect()
}

#[tokio::test]
async fn durable_partial_survives_subscriber_disconnect_and_duplicate_ack_calls_once() {
    let _guard = TEST_LOCK.lock().await;
    let f = fixture().await;
    let (_dir, store, user, thread) = configured(&f.base).await;
    let runtime = Runtime::start(store.clone());
    let browser_subscription = runtime.subscribe();
    let first = submit(&store, &user, &thread).await;
    runtime.wake();
    let duplicate = store
        .submit_turn(
            &user,
            thread.clone(),
            first.run.request_id.clone(),
            "Synthetic question".into(),
        )
        .await
        .unwrap();
    assert_eq!(duplicate.run.id, first.run.id);
    let partial = wait_for(&store, &user, &thread, |turn| !turn.assistant.is_empty()).await;
    assert_eq!(partial.run.state, RunState::Running);
    assert_eq!(text(&partial), "안녕 🌱");
    drop(browser_subscription);
    f.state.release.add_permits(1);
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::Completed);
    assert_eq!(text(&done), "안녕 🌱 done");
    assert_eq!(done.run.usage.input_tokens, Some(7));
    assert_eq!(done.run.usage.output_tokens, Some(4));
    assert_eq!(f.state.calls.load(Ordering::SeqCst), 1);
    runtime.shutdown().await;
}
#[tokio::test]
async fn streamed_failure_keeps_partial_and_incomplete_refusal_identity_are_distinct() {
    let _guard = TEST_LOCK.lock().await;
    for (mode, state, error) in [
        (1, RunState::Failed, Some("protocol_failed")),
        (2, RunState::Incomplete, Some("output_limit")),
        (3, RunState::Failed, Some("identity_unverified")),
        (4, RunState::Refused, None),
        (6, RunState::OutcomeUnknown, Some("protocol_failed")),
    ] {
        let f = fixture().await;
        f.state.mode.store(mode, Ordering::SeqCst);
        f.state.release.add_permits(1);
        let (_dir, store, user, thread) = configured(&f.base).await;
        let runtime = Runtime::start(store.clone());
        submit(&store, &user, &thread).await;
        runtime.wake();
        let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
        assert_eq!(done.run.state, state);
        assert_eq!(done.run.error.as_deref(), error);
        if mode == 3 {
            assert!(done.assistant.is_empty());
        } else {
            assert!(text(&done).starts_with("안녕 🌱"));
        }
        assert!(!text(&done).contains("private-provider-marker"));
        if mode == 4 {
            assert!(done.assistant.iter().any(|message| message.refusal));
        }
        runtime.shutdown().await;
    }
}
#[tokio::test]
async fn cancellation_and_deletion_fence_held_model_output() {
    let _guard = TEST_LOCK.lock().await;
    for delete in [false, true] {
        let f = fixture().await;
        let (_dir, store, user, thread) = configured(&f.base).await;
        let runtime = Runtime::start(store.clone());
        let admitted = submit(&store, &user, &thread).await;
        runtime.wake();
        let partial = wait_for(&store, &user, &thread, |turn| !turn.assistant.is_empty()).await;
        if delete {
            store.delete_thread(&user, thread.clone()).await.unwrap();
        } else {
            store.cancel_run(&user, admitted.run.id, 1).await.unwrap();
        }
        runtime.wake();
        tokio::time::sleep(Duration::from_millis(350)).await;
        f.state.release.add_permits(1);
        runtime.shutdown().await;
        if delete {
            assert!(matches!(
                store.conversation(&user, thread, None).await,
                Err(moosuri_store::StoreError::NotFound)
            ));
        } else {
            let done = store
                .conversation(&user, thread, None)
                .await
                .unwrap()
                .turns
                .remove(0);
            assert_eq!(done.run.state, RunState::Cancelled);
            assert_eq!(text(&done), text(&partial));
        }
        assert_eq!(f.state.calls.load(Ordering::SeqCst), 1);
    }
}
#[tokio::test]
async fn logout_revokes_worker_and_shutdown_records_unknown_without_replay() {
    let _guard = TEST_LOCK.lock().await;
    for revoke in [false, true] {
        let f = fixture().await;
        let (dir, store, user, thread) = configured(&f.base).await;
        let runtime = Runtime::start(store.clone());
        submit(&store, &user, &thread).await;
        runtime.wake();
        let partial = wait_for(&store, &user, &thread, |turn| !turn.assistant.is_empty()).await;
        if revoke {
            store.logout(&user).await.unwrap();
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
        let mut notifications = runtime.subscribe();
        let before = *notifications.borrow_and_update();
        tokio::time::timeout(Duration::from_secs(5), runtime.shutdown())
            .await
            .unwrap();
        assert!(runtime.is_stopping());
        assert!(*notifications.borrow() > before);
        drop(runtime);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let user = store
            .login("admin@example.test".into(), PASSWORD.into())
            .await
            .unwrap()
            .user;
        let done = store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .remove(0);
        assert_eq!(text(&done), text(&partial));
        assert_eq!(
            done.run.state,
            if revoke {
                RunState::Cancelled
            } else {
                RunState::OutcomeUnknown
            }
        );
        let runtime = Runtime::start(store.clone());
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(f.state.calls.load(Ordering::SeqCst), 1);
        runtime.shutdown().await;
    }
}
#[tokio::test]
async fn dropping_last_runtime_handle_cancels_owned_tasks() {
    let _guard = TEST_LOCK.lock().await;
    let f = fixture().await;
    let (_dir, store, user, thread) = configured(&f.base).await;
    let runtime = Runtime::start(store.clone());
    let copy = runtime.clone();
    submit(&store, &user, &thread).await;
    runtime.wake();
    wait_for(&store, &user, &thread, |turn| !turn.assistant.is_empty()).await;
    drop(runtime);
    assert!(!copy.is_stopping());
    drop(copy);
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::OutcomeUnknown);
    assert_eq!(done.run.error.as_deref(), Some("interrupted"));
}

#[tokio::test]
async fn large_unicode_deltas_checkpoint_as_bounded_utf8_patches() {
    let _guard = TEST_LOCK.lock().await;
    let f = fixture().await;
    f.state.mode.store(5, Ordering::SeqCst);
    let (_dir, store, user, thread) = configured(&f.base).await;
    let runtime = Runtime::start(store.clone());
    submit(&store, &user, &thread).await;
    runtime.wake();
    let expected = "🌱".repeat(6000);
    let partial = wait_for(&store, &user, &thread, |turn| text(turn).len() == 24000).await;
    assert_eq!(text(&partial), expected);
    let events = store.chat_events(&user, thread.clone(), 0).await.unwrap();
    let mut appends = 0;
    for event in events.events {
        if let moosuri_core::ChatChange::Delta { text, .. } = event.change {
            assert!(text.len() <= 8192);
            appends += 1;
        }
    }
    assert!(appends >= 3);
    f.state.release.add_permits(1);
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(text(&done), expected + " done");
    assert_eq!(done.run.state, RunState::Completed);
    runtime.shutdown().await;
}

#[tokio::test]
async fn transport_timeout_releases_dispatch_capacity_without_replaying_the_call() {
    let _guard = TEST_LOCK.lock().await;
    let f = fixture().await;
    let (_dir, store, user, thread) = configured(&f.base).await;
    let runtime = Runtime::start(store.clone());
    submit(&store, &user, &thread).await;
    runtime.wake();
    wait_for(&store, &user, &thread, |turn| !turn.assistant.is_empty()).await;
    tokio::time::pause();
    tokio::time::advance(Duration::from_secs(181)).await;
    tokio::time::resume();
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::OutcomeUnknown);
    assert_eq!(done.run.error.as_deref(), Some("timed_out"));
    assert!(done.run.can_retry);
    assert_eq!(f.state.calls.load(Ordering::SeqCst), 1);
    // A different explicit question can dispatch; timeout itself is never replayed.
    f.state.release.add_permits(2);
    submit(&store, &user, &thread).await;
    runtime.wake();
    let next = wait_for(&store, &user, &thread, |turn| {
        turn.run.state == RunState::Completed
    })
    .await;
    assert_ne!(done.run.id, next.run.id);
    assert_eq!(f.state.calls.load(Ordering::SeqCst), 2);
    runtime.shutdown().await;
}

#[tokio::test]
async fn unavailable_upstream_does_not_leave_a_running_lease() {
    let _guard = TEST_LOCK.lock().await;
    let f = fixture().await;
    let (_dir, store, user, thread) = configured(&f.base).await;
    f.task.abort();
    tokio::task::yield_now().await;
    let runtime = Runtime::start(store.clone());
    submit(&store, &user, &thread).await;
    let done = wait_for(&store, &user, &thread, |turn| !turn.run.state.active()).await;
    assert_eq!(done.run.state, RunState::OutcomeUnknown);
    assert_eq!(done.run.error.as_deref(), Some("connection_failed"));
    assert!(done.run.can_retry);
    runtime.shutdown().await;
}

#[tokio::test]
async fn exited_worker_without_terminal_checkpoint_is_settled_without_replay() {
    let _guard = TEST_LOCK.lock().await;
    let f = fixture().await;
    let (_dir, store, user, thread) = configured(&f.base).await;
    submit(&store, &user, &thread).await;
    let claimed = store.claim_agent_run().await.unwrap().unwrap();
    let (changes, _) = watch::channel(0);
    let signals = Signals {
        delivery: Arc::new(tokio::sync::RwLock::new(())),
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
    };
    let mut jobs = JoinSet::new();
    let id = jobs.spawn(async {}).id();
    let mut leases = HashMap::from([(id, claimed.lease)]);
    let mut retired = Vec::new();
    reap(
        jobs.join_next_with_id().await.unwrap(),
        &mut leases,
        &mut retired,
    );
    assert!(leases.is_empty());
    settle_retired(&mut retired, &store, &signals).await;
    assert!(retired.is_empty());
    let done = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .remove(0);
    assert_eq!(done.run.state, RunState::OutcomeUnknown);
    assert_eq!(done.run.error.as_deref(), Some("interrupted"));
    submit(&store, &user, &thread).await;
    assert!(store.claim_agent_run().await.unwrap().is_some());
    assert_eq!(f.state.calls.load(Ordering::SeqCst), 0);
}

mod knowledge;
mod memory;

mod live_memory;
