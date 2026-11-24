//! No-progress pause of the private browser loop, with a fake read-only relay and a fake model.
use super::tests::{Member, listener, member, request, wait_state};
use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

type Page = (
    &'static str,
    &'static str,
    &'static str,
    &'static [&'static str],
);
#[derive(Clone, Default)]
struct Reads {
    follows: Arc<AtomicUsize>,
    selected: Arc<AtomicUsize>,
}
/// A tiny document site: open serves the first page, follow serves a listed page in a new tab,
/// and one accepted click filters whatever page is selected.
fn site(path: &std::path::Path, pages: &'static [Page]) -> Reads {
    let l = listener(path);
    let reads = Reads::default();
    let seen = reads.clone();
    tokio::spawn(async move {
        let mut tabs: Vec<(Uuid, &'static str)> = Vec::new();
        let mut filtered = false;
        loop {
            let (stream, _) = l.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            let r: Value = serde_json::from_str(&line).unwrap();
            let page = |url: &str, filtered: bool| {
                let (url, title, text, links) = pages.iter().find(|p| p.0 == url).unwrap();
                let text = if filtered {
                    format!("{text} filtered")
                } else {
                    text.to_string()
                };
                json!({"page":{"title":title,"url":url,"text":format!("Accessibility:\n{{\"name\":\"node-metadata\"}}\n\nVisible layout text:\n{text}"),"warning":"","retrieved_at_unix_ms":1,"links":links.iter().map(|u| json!({"text":format!("Open {u}"),"url":u})).collect::<Vec<_>>(),"truncated":false}})
            };
            let tabs_value = |tabs: &Vec<(Uuid, &str)>| json!({"active_tab":tabs.last().map(|t| t.0),"tabs":tabs.iter().map(|t| json!({"tab_id":t.0,"url":t.1})).collect::<Vec<_>>()});
            let value = match r["op"].as_str().unwrap() {
                "open" => {
                    tabs = vec![(Uuid::new_v4(), pages[0].0)];
                    json!({"tab_id":tabs[0].0,"observation":page(pages[0].0, false)})
                }
                "follow" => {
                    let url = pages
                        .iter()
                        .find(|p| p.0 == r["url"].as_str().unwrap())
                        .unwrap()
                        .0;
                    seen.follows.fetch_add(1, Ordering::SeqCst);
                    tabs.push((Uuid::new_v4(), url));
                    json!({"tab_id":tabs.last().unwrap().0,"observation":page(url, false)})
                }
                "selected" => {
                    seen.selected.fetch_add(1, Ordering::SeqCst);
                    page(tabs.last().unwrap().1, filtered)
                }
                "input" => {
                    filtered = true;
                    json!({"accepted":true})
                }
                "tabs" => tabs_value(&tabs),
                "stop" => json!({"stopped":true}),
                other => panic!("unexpected operation {other}"),
            };
            stream
                .write_all(format!("{}\n", json!({"ok":true,"result":value})).as_bytes())
                .await
                .unwrap();
        }
    });
    reads
}
/// Routing always picks the first offered document; answers are tagged with the page they read.
fn model(
    path: &std::path::Path,
) -> (
    moosuri_server::personal_text::Options,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
) {
    let l = listener(path);
    let routes = Arc::new(AtomicUsize::new(0));
    let answers = Arc::new(AtomicUsize::new(0));
    let (r_count, a_count) = (routes.clone(), answers.clone());
    let revision = Uuid::new_v4();
    tokio::spawn(async move {
        loop {
            let (stream, _) = l.accept().await.unwrap();
            let mut stream = BufReader::new(stream);
            let mut line = String::new();
            stream.read_line(&mut line).await.unwrap();
            let r: Value = serde_json::from_str(&line).unwrap();
            let value = if r["operation"] == "selection" {
                json!({"v":2,"state":"selection","connection_revision":revision,"selection_epoch":3,"model":"claude-fixture","effort":"medium","kind":"anthropic","reservation_tokens":4096})
            } else {
                let context = r["messages"][1]["text"].as_str().unwrap();
                assert!(!context.contains("node-metadata"));
                let data: Value = serde_json::from_str(context).unwrap();
                let output = if r["messages"][0]["text"]
                    .as_str()
                    .unwrap()
                    .contains("Choose the next browser observation")
                {
                    r_count.fetch_add(1, Ordering::SeqCst);
                    assert!(!data["available_documents"].as_array().unwrap().is_empty());
                    json!({"follow_index":0})
                } else {
                    a_count.fetch_add(1, Ordering::SeqCst);
                    json!({"answer":format!("Read {}.", data["observed_page"]["title"].as_str().unwrap()),"needs_user":false})
                };
                json!({"v":1,"state":"settled","call_id":r["call_id"],"connection_revision":revision,"model":"claude-fixture","effort":"medium","outcome":"completed","output":output.to_string(),"receipt":{"state":"settled","input_tokens":50,"output_tokens":8,"charged_tokens":58}})
            };
            stream
                .write_all(format!("{value}\n").as_bytes())
                .await
                .unwrap();
        }
    });
    (
        moosuri_server::personal_text::Options {
            socket: path.into(),
            broker_uid: unsafe { libc::geteuid() },
            max_output_tokens: 256,
        },
        routes,
        answers,
    )
}
fn router(
    m: &Member,
    socket: &std::path::Path,
    options: Option<moosuri_server::personal_text::Options>,
) -> Router {
    moosuri_server::personal_service_router(
        m.store.clone(),
        None,
        routes(
            m.store.clone(),
            m.owner,
            Some(socket.into()),
            unsafe { libc::geteuid() },
            options,
        ),
    )
}
async fn status(app: &Router, m: &Member, thread: &str) -> (StatusCode, Value) {
    request(
        app,
        "GET",
        &format!("/api/v1/threads/{thread}/browser"),
        m.owner.account,
        &m.token,
        Value::Null,
    )
    .await
}
async fn post(app: &Router, m: &Member, path: &str, body: Value) -> (StatusCode, Value) {
    request(app, "POST", path, m.owner.account, &m.token, body).await
}
fn version(w: &Value) -> Value {
    json!({"workspace_id":w["id"],"revision":w["revision"],"request_id":Uuid::new_v4()})
}

// The second page repeats the first page's content under another address and the same UI title.
static REPEATING: &[Page] = &[
    (
        "https://example.org/a",
        "Timetable",
        "Morning bus 08:15",
        &["https://example.org/b", "https://example.org/c"],
    ),
    (
        "https://example.org/b",
        "Timetable",
        "Morning  bus 08:15\n",
        &["https://example.org/c", "https://example.org/b"],
    ),
    ("https://example.org/c", "Platform", "Platform 7", &[]),
];
static CHANGING: &[Page] = &[
    (
        "https://example.org/a",
        "Timetable",
        "Morning bus 08:15",
        &["https://example.org/b"],
    ),
    (
        "https://example.org/b",
        "Timetable",
        "Afternoon bus 14:30",
        &[],
    ),
];
const TASK: &str = "Find the bus timetable details";

async fn paused_run(
    root: &std::path::Path,
    pages: &'static [Page],
) -> (
    Member,
    Router,
    Reads,
    Arc<AtomicUsize>,
    Arc<AtomicUsize>,
    std::path::PathBuf,
    String,
) {
    let a = member(&root.join("a")).await;
    let socket = root.join("browser.sock");
    let reads = site(&socket, pages);
    let (options, routes, answers) = model(&root.join("model.sock"));
    let app = router(&a, &socket, Some(options));
    let base = format!("/api/v1/threads/{}/browser", a.thread);
    let start = json!({"request_id":Uuid::new_v4(),"text":TASK,"url":"https://example.org/a"});
    assert_eq!(post(&app, &a, &base, start).await.0, StatusCode::ACCEPTED);
    (a, app, reads, routes, answers, socket, base)
}

#[tokio::test]
async fn repeated_identical_observation_pauses_with_resumable_reason_before_another_model_call() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (a, app, reads, routes, answers, socket, base) = paused_run(&root, REPEATING).await;
    let paused = wait_state(&app, &base, &a, "ready").await;
    let w = &paused["workspace"];
    assert_eq!(w["reason"], "browser_no_progress");
    // One planning call chose the link; the identical second page never reached the answer model.
    assert_eq!(routes.load(Ordering::SeqCst), 1);
    assert_eq!(answers.load(Ordering::SeqCst), 0);
    assert_eq!(reads.follows.load(Ordering::SeqCst), 1);
    // The pause stays a read-only continuation: public on-site links remain available to follow.
    assert_eq!(w["links"][0]["url"], "https://example.org/c");
    let user = a.store.authenticate(a.token.clone()).await.unwrap();
    let turns = a
        .store
        .conversation(&user, a.thread.clone(), None)
        .await
        .unwrap()
        .turns;
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].assistant.len(), 1);
    assert!(turns[0].assistant[0].text.contains("일시 중지"));
    // The reason is stored, not held in memory: a rebuilt service reads it back unchanged.
    let reloaded = router(&a, &socket, None);
    let again = status(&reloaded, &a, &a.thread).await.1;
    assert_eq!(again["workspace"]["reason"], "browser_no_progress");
    assert_eq!(again["workspace"]["id"], w["id"]);
    // Another thread of the same member and another member see nothing of this run.
    let other_thread = a
        .store
        .create_thread(&user, "Other".into())
        .await
        .unwrap()
        .id;
    assert!(status(&app, &a, &other_thread).await.1["workspace"].is_null());
    let b = member(&root.join("b")).await;
    let b_app = router(&b, &socket, None);
    assert_eq!(status(&b_app, &b, &a.thread).await.0, StatusCode::NOT_FOUND);
    assert!(status(&b_app, &b, &b.thread).await.1["workspace"].is_null());
}

#[tokio::test]
async fn genuinely_changed_page_continues_even_when_the_title_is_the_same() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (a, app, reads, routes, answers, _socket, base) = paused_run(&root, CHANGING).await;
    let done = wait_state(&app, &base, &a, "completed").await;
    assert!(done["workspace"]["reason"].is_null());
    assert_eq!(routes.load(Ordering::SeqCst), 1);
    assert_eq!(answers.load(Ordering::SeqCst), 1);
    assert_eq!(reads.follows.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn guidance_then_follow_proceeds_once_and_a_replayed_follow_has_no_second_effect() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let (a, app, reads, routes, answers, _socket, base) = paused_run(&root, REPEATING).await;
    let paused = wait_state(&app, &base, &a, "ready").await;
    let id = paused["workspace"]["id"].clone();
    let guide =
        json!({"request_id":Uuid::new_v4(),"workspace_id":id,"text":"Only the platform number"});
    assert_eq!(post(&app, &a, &base, guide.clone()).await.0, StatusCode::OK);
    assert_eq!(post(&app, &a, &base, guide).await.0, StatusCode::OK);
    let guided = status(&app, &a, &a.thread).await.1;
    assert_eq!(guided["workspace"]["state"], "ready");
    assert_eq!(guided["workspace"]["reason"], "new_guidance");
    assert_eq!(reads.follows.load(Ordering::SeqCst), 1);
    let mut follow = version(&guided["workspace"]);
    follow["action"] = json!("follow");
    follow["url"] = json!("https://example.org/c");
    let (code, done) = post(&app, &a, &format!("{base}/tabs"), follow.clone()).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(done["workspace"]["state"], "completed");
    assert!(done["workspace"]["reason"].is_null());
    // The replay changes neither the page nor the model's work.
    assert_eq!(
        post(&app, &a, &format!("{base}/tabs"), follow).await.0,
        StatusCode::CONFLICT
    );
    assert_eq!(reads.follows.load(Ordering::SeqCst), 2);
    assert_eq!(routes.load(Ordering::SeqCst), 1);
    assert_eq!(answers.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn unchanged_selection_pauses_and_a_real_change_after_takeover_continues() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let a = member(&root.join("a")).await;
    let socket = root.join("browser.sock");
    let reads = site(&socket, CHANGING);
    let (options, routes, answers) = model(&root.join("model.sock"));
    let app = router(&a, &socket, Some(options));
    let base = format!("/api/v1/threads/{}/browser", a.thread);
    let start = json!({"request_id":Uuid::new_v4(),"text":TASK,"url":"https://example.org/a","wait_for_user":true});
    assert_eq!(post(&app, &a, &base, start).await.0, StatusCode::ACCEPTED);
    let waiting = wait_state(&app, &base, &a, "ready").await;
    assert_eq!(waiting["workspace"]["reason"], "waiting_for_user");
    let frame = "f".repeat(64);
    // Hand back without changing anything: the selected content is identical to what was read.
    let first = post(
        &app,
        &a,
        &format!("{base}/takeover"),
        version(&waiting["workspace"]),
    )
    .await
    .1;
    let mut resume = version(&first["workspace"]);
    resume["control_token"] = first["control_token"].clone();
    resume["frame_token"] = json!(frame);
    let (code, paused) = post(&app, &a, &format!("{base}/resume"), resume.clone()).await;
    assert_eq!(code, StatusCode::OK);
    assert_eq!(paused["workspace"]["state"], "ready");
    assert_eq!(paused["workspace"]["reason"], "browser_no_progress");
    assert_eq!(reads.selected.load(Ordering::SeqCst), 1);
    assert_eq!(answers.load(Ordering::SeqCst), 0);
    // Replaying the hand-back is a receipt, not a second selected-content read.
    assert_eq!(
        post(&app, &a, &format!("{base}/resume"), resume).await.0,
        StatusCode::OK
    );
    assert_eq!(reads.selected.load(Ordering::SeqCst), 1);
    // Control resolves the pause; a person's filter then makes the next read genuinely new.
    let second = post(
        &app,
        &a,
        &format!("{base}/takeover"),
        version(&paused["workspace"]),
    )
    .await
    .1;
    assert!(second["workspace"]["reason"].is_null());
    let mut click = version(&second["workspace"]);
    click["control_token"] = second["control_token"].clone();
    click["frame_token"] = json!(frame);
    click["input"] = json!({"kind":"click","x":50,"y":60});
    let clicked = post(&app, &a, &format!("{base}/input"), click).await.1;
    let mut resume = version(&clicked["workspace"]);
    resume["control_token"] = second["control_token"].clone();
    resume["frame_token"] = json!(frame);
    let done = post(&app, &a, &format!("{base}/resume"), resume).await.1;
    assert_eq!(done["workspace"]["state"], "completed");
    assert_eq!(reads.selected.load(Ordering::SeqCst), 2);
    assert_eq!(answers.load(Ordering::SeqCst), 1);
    assert_eq!(routes.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn old_caption_with_a_new_document_choice_still_makes_progress() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    static NEW_CHOICE: &[Page] = &[
        (
            "https://example.org/a",
            "Documents",
            "Choose a timetable",
            &["https://example.org/b"],
        ),
        (
            "https://example.org/b",
            "Documents",
            "Choose a timetable",
            &["https://example.org/c"],
        ),
        ("https://example.org/c", "Gate", "Platform four", &[]),
    ];
    let (member, app, observed, _, answers, _, base) = paused_run(&root, NEW_CHOICE).await;
    let done = wait_state(&app, &base, &member, "completed").await;
    assert!(done["workspace"]["reason"].is_null());
    assert_eq!(observed.follows.load(Ordering::SeqCst), 2);
    assert_eq!(answers.load(Ordering::SeqCst), 1);
}
