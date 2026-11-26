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
static MEMBER_SETUP: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

async fn member(root: &std::path::Path, email: &str) -> (Store, VolumeOwner, String, String) {
    // Production deliberately bounds concurrent password KDFs. Serialize only
    // synthetic account preparation; the scenarios themselves remain concurrent.
    let _setup = MEMBER_SETUP.lock().await;
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
    post_to(app, owner, token, thread, "work-interpret", body).await
}
async fn post_to(
    app: &Router,
    owner: VolumeOwner,
    token: &str,
    thread: &str,
    route: &str,
    body: Value,
) -> (StatusCode, Value) {
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/v1/threads/{thread}/{route}"))
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

#[tokio::test]
async fn recall_is_bound_to_the_original_text_and_an_explicit_opt_out_wins() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let broker = Broker::start(&root);
    let (store, owner, token, thread) = member(&root.join("a"), "a@example.test").await;
    let app = personal_text_preview_router(store.clone(), owner, broker.options.clone());
    let user = store.authenticate(token.clone()).await.unwrap();
    let saved = "회의는 오전을 선호합니다";
    let memory = store
        .create_memory(
            &user,
            uuid::Uuid::new_v4().to_string(),
            moosuri_core::MemoryDraft {
                subject: "회의".into(),
                text: saved.into(),
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            },
        )
        .await
        .unwrap();
    for (question, expected) in [
        ("회의 시간 하나 추천해줘", true),
        (
            "오늘 한 번만 오후로 할게. 오늘 회의 시간대만 답해 줘.",
            false,
        ),
        ("이번에는 회의 시간을 언제로 잡으면 좋을까?", true),
        ("내 취향 잊지 말고 회의 시간대를 추천해줘", true),
        ("화상 말고 내 평소 회의 시간대 취향으로 추천해줘.", true),
        ("내 회의 취향을 무시하지 말고 평소 시간대를 알려줘.", true),
        ("내 회의 취향은 말고 팀에 좋은 시간을 알려줘.", false),
        ("내 취향을 따르지 마. 회의 시간을 알려줘.", false),
        ("내 취향을 고려하지 말아줘. 회의 시간 추천해줘.", false),
        ("오늘은 오후 3.30으로 하자~", false),
        ("오늘은 오후로 할게요 ㅎㅎ", false),
        ("오늘은 오후로 할게, 이번 회의 시간대만 답해줘.", false),
        (
            "이번 회의는 오후로 할게. 평소 내 회의 시간대는 뭐였지?",
            true,
        ),
        ("오늘은 오후로 할게. 평소 내 선호도 알려줘.", true),
        ("내 선호는 쓰지 말고 일반적인 회의 시간을 알려줘", false),
    ] {
        broker.say(json!({"intent":"chat","recall":["회의"]}));
        let mut input = body(question);
        assert_eq!(
            post(&app, owner, &token, &thread, input.clone()).await.1,
            json!({"kind":"chat"})
        );
        input["capability"] = json!("text_only_preview");
        if expected {
            let mut changed = input.clone();
            changed["text"] = json!("다른 내용");
            assert_eq!(
                post_to(&app, owner, &token, &thread, "text-preview", changed)
                    .await
                    .0,
                StatusCode::CONFLICT
            );
        }
        broker.say(json!(if expected { saved } else { "답변" }));
        assert_eq!(
            post_to(&app, owner, &token, &thread, "text-preview", input.clone())
                .await
                .0,
            StatusCode::CREATED
        );
        let prompt: Value =
            serde_json::from_str(broker.prompts.lock().unwrap().last().unwrap()).unwrap();
        let current = prompt.as_array().unwrap().last().unwrap()["text"]
            .as_str()
            .unwrap();
        assert_eq!(current.contains(saved), expected);
        if !expected {
            assert!(
                !prompt
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|message| message["text"].as_str().unwrap().contains(saved))
            );
        }
        assert!(!current.contains(&memory.id));
        let calls = broker.calls();
        assert_eq!(
            post_to(&app, owner, &token, &thread, "text-preview", input)
                .await
                .0,
            StatusCode::OK
        );
        assert_eq!(broker.calls(), calls);
    }
    assert!(groups(&store, owner, &token, &thread).await.is_empty());
    broker.task.abort();
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

fn inbox_reply(thread: &str) -> Value {
    json!({"kind":"inbox_read","thread_id":thread,"scope":"one_inbox_page_metadata_only","limit":5})
}

#[tokio::test]
async fn inbox_read_is_one_fixed_instruction_with_a_final_receipt_that_replays_without_effects() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let broker = Broker::start(&root);
    let (store, owner, token, thread) = member(&root.join("a"), "a@example.test").await;
    let (other, b, btoken, _bthread) = member(&root.join("b"), "b@example.test").await;
    let app = personal_text_preview_router(store.clone(), owner, broker.options.clone());
    let bapp = personal_text_preview_router(other, b, broker.options.clone());

    // A fresh read-only lookup never creates an interpretation or calls the model.
    let request = body("최근 받은 메일 5개 확인해줘");
    let mut peek = request.clone();
    peek["read_only"] = json!(true);
    let (status, _) = post(&app, owner, &token, &thread, peek.clone()).await;
    assert!(!status.is_success());
    assert_eq!(broker.calls(), 0);

    // The proposal is only a proposal; the reply carries no account, connection or query.
    broker.say(json!({"intent":"inbox_read"}));
    let (status, first) = post(&app, owner, &token, &thread, request.clone()).await;
    assert_eq!((status, &first), (StatusCode::OK, &inbox_reply(&thread)));
    assert_eq!(broker.calls(), 1);
    assert!(groups(&store, owner, &token, &thread).await.is_empty());

    // The record is final (nothing claimed or planned is left to block the same id), and every
    // replay, including after a restart, answers without another paid call.
    drop(app);
    drop(store);
    let store = owner.open_verified(&root.join("a")).await.unwrap();
    let app = personal_text_preview_router(store.clone(), owner, broker.options.clone());
    for again in [request.clone(), peek.clone()] {
        let (status, reply) = post(&app, owner, &token, &thread, again).await;
        assert_eq!((status, &reply), (StatusCode::OK, &inbox_reply(&thread)));
    }
    assert_eq!(broker.calls(), 1);
    let mut changed = request.clone();
    changed["text"] = json!("받은 메일 5개 확인해줘 다시");
    assert_eq!(
        post(&app, owner, &token, &thread, changed).await.0,
        StatusCode::CONFLICT
    );

    // Another member cannot replay this member's receipt, in this or any other thread.
    let (status, reply) = post(&bapp, b, &btoken, &thread, peek.clone()).await;
    assert!(!status.is_success() && reply["kind"].is_null(), "{reply}");
    assert!(
        post(&app, owner, &btoken, &thread, peek)
            .await
            .0
            .is_client_error()
    );
    broker.task.abort();
}

#[tokio::test]
async fn inbox_read_needs_the_members_own_direct_supported_request() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let broker = Broker::start(&root);
    let (store, owner, token, thread) = member(&root.join("a"), "a@example.test").await;
    let app = personal_text_preview_router(store.clone(), owner, broker.options.clone());
    // The model says inbox_read for all of these; only the member's own words may authorize it.
    for (text, expected) in [
        ("최근 받은 메일 5개 확인해줘", "inbox_read"),
        ("받은 편지함 보여 주세요", "inbox_read"),
        ("받은 메일 5개 확인하지 마", "clarify"),
        ("받은 메일 확인해 줘라고 말했어", "chat"),
        ("\"받은 메일 확인해줘\"", "chat"),
        ("받은 메일은 어떻게 확인하는 거야", "chat"),
        ("만약 받은 메일 확인해줘 하면 어떻게 돼", "chat"),
        ("받은 메일 10개 확인해줘", "clarify"),
        ("받은 메일에서 김 대리 메일 찾아줘", "clarify"),
        ("받은 메일 전체 본문 요약해줘", "clarify"),
        ("오늘 받은 메일 확인해줘", "clarify"),
        ("다른 계정 받은 메일 확인해줘", "clarify"),
        ("other@example.test 받은 메일 확인해줘", "clarify"),
        ("이 부분 확인해줘", "clarify"),
        ("받은메일 확인해줘", "inbox_read"),
        ("내 받은편지함 5건 보여줘", "inbox_read"),
        ("받은 메일 좀 확인해 주세요.", "inbox_read"),
        ("메일함 확인해줘", "inbox_read"),
        ("받은 메일 확인 안 해줘", "clarify"),
        ("받은 메일 안 보여줘", "clarify"),
        ("받은 메일 확인 안 할래", "clarify"),
        ("받은 메일 확인하고 일정 등록해줘", "clarify"),
        ("받은 메일 확인하고 캘린더에 넣어줘", "clarify"),
        ("내일 받은 메일 확인해줘", "clarify"),
        ("나중에 받은 메일 확인해줘", "clarify"),
        ("새 메일 오면 받은 메일 확인해줘", "clarify"),
        ("중요 메일함 확인해줘", "clarify"),
        ("네이버 메일함 확인해줘", "clarify"),
        ("회사 메일함 보여줘", "clarify"),
    ] {
        broker.say(json!({"intent":"inbox_read"}));
        let (status, reply) = post(&app, owner, &token, &thread, body(text)).await;
        assert_eq!(status, StatusCode::OK, "{text}: {reply}");
        assert_eq!(reply["kind"], expected, "{text}: {reply}");
        if expected == "clarify" && !text.contains("하지 마") {
            // (negations the keyword guard misses still fall to the scope clarification)
            // Reports the supported scope instead of claiming search, all mail or body summaries.
            assert!(
                reply["message"].as_str().unwrap().contains("최대 5건"),
                "{text}"
            );
        }
    }
    // A model that tries to name a target is rejected; no connection or account field exists.
    for forged in [
        json!({"intent":"inbox_read","connection_id":"c"}),
        json!({"intent":"inbox_read","account":"other@example.test"}),
    ] {
        broker.say(forged);
        let (status, reply) = post(&app, owner, &token, &thread, body("받은 메일 확인해줘")).await;
        assert_eq!(status, StatusCode::UNPROCESSABLE_ENTITY, "{reply}");
        assert_eq!(reply["code"], "personal_work_interpret_invalid");
    }
    assert!(groups(&store, owner, &token, &thread).await.is_empty());
    broker.task.abort();
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
