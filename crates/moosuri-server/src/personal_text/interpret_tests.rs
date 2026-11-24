//! Work interpretation over the real private-socket protocol with a synthetic broker.
//! These check stored effects, replay and member isolation, not just reply wording.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use serde_json::{Value, json};
use std::{
    collections::VecDeque,
    fs,
    os::unix::fs::PermissionsExt,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tower::ServiceExt;

const PASSWORD: &str = "Synthetic-interpret-member-42";
const TRAVEL: &str = "여행 얘기랑 메모리에 있는 짐 싸기 자료 찾아서 정리해 줘";

async fn member(root: &std::path::Path, email: &str) -> (Store, VolumeOwner, String, String) {
    fs::create_dir(root).unwrap();
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
    let owner = VolumeOwner {
        account: uuid::Uuid::new_v4(),
        tenant: uuid::Uuid::new_v4(),
        volume: uuid::Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            root,
            "Synthetic member".into(),
            email.into(),
            PASSWORD.into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(root).await.unwrap();
    let initial = store.login(email.into(), PASSWORD.into()).await.unwrap();
    let login = store
        .change_password(&initial.user, "Changed-interpret-member-42".into())
        .await
        .unwrap();
    let thread = store
        .create_thread(&login.user, "Work".into())
        .await
        .unwrap()
        .id;
    (store, owner, login.token, thread)
}
async fn post(
    app: &Router,
    owner: VolumeOwner,
    token: &str,
    thread: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/threads/{thread}/work-interpret"))
                .header("host", "personal.internal")
                .header("origin", "https://personal.internal")
                .header("x-moosuri-account", owner.account.to_string())
                .header(SESSION_EPOCH_HEADER, "3")
                .header("cookie", format!("moosuri_session={token}"))
                .header("content-type", "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let bytes = to_bytes(response.into_body(), 1024 * 1024).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
fn body(text: &str) -> Value {
    json!({"request_id": uuid::Uuid::new_v4(), "text": text})
}

struct Broker {
    options: Options,
    outputs: Arc<Mutex<VecDeque<String>>>,
    calls: Arc<AtomicUsize>,
    prompts: Arc<Mutex<Vec<String>>>,
    task: tokio::task::JoinHandle<()>,
}
impl Broker {
    fn start(root: &std::path::Path) -> Self {
        let socket = root.join("model.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let outputs: Arc<Mutex<VecDeque<String>>> = Default::default();
        let calls = Arc::new(AtomicUsize::new(0));
        let prompts: Arc<Mutex<Vec<String>>> = Default::default();
        let (queue, counted, seen) = (outputs.clone(), calls.clone(), prompts.clone());
        let task = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let byte = stream.read_u8().await.unwrap();
                    if byte == b'\n' {
                        break;
                    }
                    bytes.push(byte);
                }
                let r: Value = serde_json::from_slice(&bytes).unwrap();
                let reply = if r["operation"] == "selection" {
                    json!({"v":2,"state":"selection","connection_revision":uuid::Uuid::from_u128(7),
                        "selection_epoch":3,"model":"synthetic-text","kind":"compatible","reservation_tokens":1_000_000})
                } else {
                    assert!(r.get("tools").is_none());
                    counted.fetch_add(1, Ordering::SeqCst);
                    seen.lock().unwrap().push(r["messages"].to_string());
                    let output = queue.lock().unwrap().pop_front().unwrap_or_default();
                    json!({"v":1,"state":"settled","call_id":r["call_id"],"connection_revision":r["connection_revision"],
                        "model":r["model"],"effort":r["effort"],"output":output,"outcome":"completed",
                        "receipt":{"state":"settled","input_tokens":7,"output_tokens":3,"charged_tokens":10}})
                };
                let mut line = serde_json::to_vec(&reply).unwrap();
                line.push(b'\n');
                stream.write_all(&line).await.unwrap();
            }
        });
        Self {
            options: Options {
                socket,
                broker_uid: unsafe { libc::geteuid() },
                max_output_tokens: 128,
            },
            outputs,
            calls,
            prompts,
            task,
        }
    }
    fn say(&self, decision: Value) {
        self.outputs.lock().unwrap().push_back(decision.to_string());
    }
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}
async fn groups(store: &Store, owner: VolumeOwner, token: &str, thread: &str) -> Vec<Value> {
    let user = store.authenticate(token.into()).await.unwrap();
    let page = store
        .conversation_work_page(owner, user, thread.into(), None)
        .await
        .unwrap();
    serde_json::to_value(page).unwrap()["groups"]
        .as_array()
        .unwrap()
        .clone()
}

#[tokio::test]
async fn ordinary_text_is_free_and_unavailable_broker_never_fakes_work() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (store, owner, token, thread) = member(&root.join("a"), "a@example.test").await;
    // No broker exists at all.
    let options = Options {
        socket: root.join("absent.sock"),
        broker_uid: unsafe { libc::geteuid() },
        max_output_tokens: 128,
    };
    let app = personal_text_preview_router(store.clone(), owner, options);
    for ordinary in ["안녕하세요", "고마워요", "hi"] {
        let (status, reply) = post(&app, owner, &token, &thread, body(ordinary)).await;
        assert_eq!((status, reply), (StatusCode::OK, json!({"kind":"chat"})));
    }
    let (status, reply) = post(&app, owner, &token, &thread, body(TRAVEL)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply["code"], "personal_text_model_unavailable");
    assert!(groups(&store, owner, &token, &thread).await.is_empty());
    for crafted in [
        json!({"request_id":"x","text":"hi"}),
        json!({"request_id":uuid::Uuid::new_v4(),"text":"hi","model":"m"}),
        json!({"request_id":uuid::Uuid::new_v4(),"text":"  "}),
    ] {
        let (status, _) = post(&app, owner, &token, &thread, crafted).await;
        assert!(status.is_client_error(), "{status}");
    }
}

#[tokio::test]
async fn model_proposal_creates_one_local_group_replays_and_controls_by_ordinal() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let broker = Broker::start(&root);
    let (store, owner, token, thread) = member(&root.join("a"), "a@example.test").await;
    let (other, b, btoken, bthread) = member(&root.join("b"), "b@example.test").await;
    let app = personal_text_preview_router(store.clone(), owner, broker.options.clone());
    let bapp = personal_text_preview_router(other.clone(), b, broker.options.clone());

    // Invalid JSON is not a success and creates nothing.
    broker
        .outputs
        .lock()
        .unwrap()
        .push_back("여행 검색을 시작할게요".into());
    let (status, reply) = post(&app, owner, &token, &thread, body(TRAVEL)).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(reply["code"], "personal_work_interpret_invalid");
    assert!(groups(&store, owner, &token, &thread).await.is_empty());

    // A valid proposal becomes exactly one stored group containing the member's own words.
    broker.say(json!({"intent":"create","report":"none","items":[
        {"title":"여행 대화","scope":"conversation","query":"여행"},
        {"title":"짐 싸기 메모리","scope":"memory","query":"짐"}]}));
    let request = body(TRAVEL);
    let (status, first) = post(&app, owner, &token, &thread, request.clone()).await;
    assert_eq!(status, StatusCode::OK, "{first}");
    assert_eq!(first["kind"], "work");
    assert_eq!(first["focus"], "tasks");
    assert!(first["task_id"].is_null());
    let tasks = first["group"]["tasks"].as_array().unwrap();
    assert_eq!(tasks.len(), 2);
    assert_eq!(first["group"]["message"], TRAVEL);
    let stored = groups(&store, owner, &token, &thread).await;
    assert_eq!(stored.len(), 1);
    let calls = broker.calls();

    // The same request replays the stored group without another paid call or group.
    drop(app);
    drop(store);
    let store = owner.open_verified(&root.join("a")).await.unwrap();
    let app = personal_text_preview_router(store.clone(), owner, broker.options.clone());
    let (status, again) = post(&app, owner, &token, &thread, request.clone()).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(again["group"]["id"], first["group"]["id"]);
    assert_eq!(broker.calls(), calls);
    assert_eq!(groups(&store, owner, &token, &thread).await.len(), 1);
    // The same id with other words is a conflict, not a reinterpretation.
    let mut changed = request.clone();
    changed["text"] = json!("다른 요청 찾아서 정리해 줘 메모리");
    assert_eq!(
        post(&app, owner, &token, &thread, changed).await.0,
        StatusCode::CONFLICT
    );

    // A negated sentence never reaches the store even when the proposal says cancel.
    broker.say(json!({"intent":"cancel","target":2}));
    let (status, reply) = post(&app, owner, &token, &thread, body("2번 작업은 취소하지 마")).await;
    assert_eq!(
        (status, reply["kind"].as_str()),
        (StatusCode::OK, Some("clarify"))
    );
    let stored = groups(&store, owner, &token, &thread).await;
    assert!(
        stored[0]["tasks"]
            .as_array()
            .unwrap()
            .iter()
            .all(|t| t["state"] != "cancelled")
    );

    // An ordinal maps to the server's current task; its sibling is untouched.
    broker.say(json!({"intent":"cancel","target":2}));
    let (status, reply) = post(&app, owner, &token, &thread, body("2번 작업 취소해 줘")).await;
    assert_eq!(status, StatusCode::OK, "{reply}");
    assert_eq!(reply["focus"], "status");
    assert_eq!(reply["task_id"], tasks[1]["id"]);
    let stored = groups(&store, owner, &token, &thread).await;
    assert_eq!(stored[0]["tasks"][1]["state"], "cancelled");
    assert_ne!(stored[0]["tasks"][0]["state"], "cancelled");
    // Candidate prompts carry names, never member identifiers.
    let prompts = broker.prompts.lock().unwrap().join("\n");
    assert!(!prompts.contains(tasks[0]["id"].as_str().unwrap()));
    assert!(!prompts.contains(first["group"]["id"].as_str().unwrap()));

    // A model that tries to name an identifier is rejected as invalid and changes nothing.
    broker.say(json!({"intent":"cancel","task_id":tasks[0]["id"]}));
    let (status, reply) = post(&app, owner, &token, &thread, body("1번 작업 취소해 줘")).await;
    assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
    assert_ne!(
        groups(&store, owner, &token, &thread).await[0]["tasks"][0]["state"],
        "cancelled"
    );

    // Another member cannot reach this thread or its groups, and has none of their own.
    let before = broker.calls();
    broker.say(json!({"intent":"status","target":1}));
    let (status, reply) = post(
        &bapp,
        b,
        &btoken,
        &thread,
        body("1번 작업 진행 상태 알려 줘"),
    )
    .await;
    assert!(!status.is_success(), "{status} {reply}");
    assert!(reply.get("group").is_none());
    let (status, _) = post(&bapp, b, &token, &bthread, body("안녕")).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let _ = before;
    assert!(groups(&other, b, &btoken, &bthread).await.is_empty());
    broker.task.abort();
}
