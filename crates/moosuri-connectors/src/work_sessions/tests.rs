use super::*;
use axum::{
    Json, Router,
    body::Bytes,
    extract::{Path, State as HttpState},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, FileTimes},
    os::unix::fs::{DirBuilderExt, PermissionsExt, symlink},
    sync::{Arc, Mutex},
    time::Duration,
};
const TIME: i64 = 1_800_000_000;
struct Fixture {
    home: PathBuf,
    options: Options,
}
impl Fixture {
    fn new(origin: String, connection_id: String) -> Self {
        let home =
            std::env::temp_dir().join(format!("moosuri-synthetic-work-feed-{}", Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&home).unwrap();
        let directory = home.join("chosen-input");
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&directory)
            .unwrap();
        let token_file = home.join("chosen-token");
        fs::write(&token_file, "a".repeat(64)).unwrap();
        fs::set_permissions(&token_file, fs::Permissions::from_mode(0o600)).unwrap();
        Self {
            options: Options {
                directory,
                origin,
                connection_id,
                token_file,
                state_file: home.join("cursor.json"),
            },
            home,
        }
    }
    fn write(&self, name: &str, bytes: &[u8]) {
        let p = self.options.directory.join(name);
        fs::write(&p, bytes).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).unwrap();
        File::open(p)
            .unwrap()
            .set_times(
                FileTimes::new()
                    .set_modified(UNIX_EPOCH + Duration::from_secs((TIME - 120) as u64)),
            )
            .unwrap();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.home);
    }
}
fn row(at: i64, kind: &str, payload: Value) -> String {
    json!({"timestamp":chrono::DateTime::from_timestamp(at,0).unwrap().to_rfc3339(),"type":kind,"payload":payload}).to_string()+"\n"
}
fn header(id: &str, source: &str, thread: &str) -> String {
    row(
        TIME - 5000,
        "session_meta",
        json!({"id":id,"session_id":id,"source":source,"thread_source":thread,"cwd":"/SYNTHETIC_PRIVATE_PATH","originator":"codex_cli_rs","cli_version":"synthetic-verified-shape","timestamp":"2027-01-15T06:36:40Z","base_instructions":"SYNTHETIC_CONFIG_CANARY"}),
    )
}
fn start(id: &str, trigger: &str) -> String {
    row(
        TIME - 5000,
        "event_msg",
        json!({"type":"task_started","turn_id":id,"root_turn_id":id,"turn_attribution":{"turn_id":id,"turn_trigger":trigger,"parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":id}}),
    )
}
fn user(text: &str) -> String {
    row(
        TIME - 4900,
        "event_msg",
        json!({"type":"user_message","message":text,"local_images":["SYNTHETIC_ATTACHMENT_CANARY"]}),
    )
}
fn complete(id: &str) -> String {
    row(
        TIME - 4800,
        "event_msg",
        json!({"type":"task_complete","turn_id":id,"last_agent_message":"SYNTHETIC_ASSISTANT_CANARY","error":null}),
    )
}
fn session(id: &str, text: &str, finished: bool) -> String {
    let mut raw = header(id, "cli", "user") + &start("human-turn", "user") + &user(text);
    if finished {
        raw.push_str(&complete("human-turn"));
    }
    raw
}

#[derive(Clone, Copy, PartialEq)]
enum Fault {
    None,
    RejectBeforeCommit,
    LostAfterCommit,
    LostFinishAck,
}
struct Mock {
    connection: String,
    sequence: i64,
    checkpoint: Option<String>,
    active: Option<Run>,
    last_run: Option<Run>,
    receipts: BTreeMap<String, SourceBatchReceipt>,
    bodies: Vec<String>,
    committed: BTreeMap<String, String>,
    fault: Fault,
    hidden_receipts: u32,
    starts: u32,
    forbid_status: bool,
    redirect: bool,
    trap: u32,
}
type Shared = Arc<Mutex<Mock>>;
fn authenticated(headers: &HeaderMap) -> bool {
    headers
        .get("authorization")
        .is_some_and(|v| v.as_bytes() == format!("Bearer {}", "a".repeat(64)).as_bytes())
        && headers.get("cookie").is_none()
        && headers.get("origin").is_none()
}
async fn status(HttpState(s): HttpState<Shared>, headers: HeaderMap) -> Response {
    let s = s.lock().unwrap();
    assert!(authenticated(&headers));
    if s.forbid_status {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if s.redirect {
        return (StatusCode::FOUND, [("location", "/trap")]).into_response();
    }
    Json(json!({"start_binding":"b".repeat(64),"run_generation":s.starts,"connection_id":s.connection,"config_revision":1,"local_sequence":s.sequence,"checkpoint":s.checkpoint,"active_run":s.active.as_ref().map(|r|json!({"id":r.run_id,"generation":r.generation}))})).into_response()
}
async fn begin(
    HttpState(s): HttpState<Shared>,
    headers: HeaderMap,
    Json(input): Json<moosuri_core::SourceRunStart>,
) -> Response {
    assert!(authenticated(&headers));
    let mut s = s.lock().unwrap();
    if s.active.is_some() {
        return StatusCode::CONFLICT.into_response();
    }
    s.starts += 1;
    let run = Run {
        run_id: Uuid::new_v4().to_string(),
        generation: i64::from(s.starts),
    };
    s.active = Some(run.clone());
    s.last_run = Some(run.clone());
    if let Some(intent) = input.intent {
        return (
            StatusCode::CREATED,
            Json(SourceRunStartReceipt {
                request_id: intent.request_id,
                expected_run_generation: intent.expected_run_generation,
                run_id: run.run_id,
                generation: run.generation,
                state: "running".into(),
            }),
        )
            .into_response();
    }
    (StatusCode::CREATED, Json(run)).into_response()
}
async fn receipt(
    HttpState(s): HttpState<Shared>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    assert!(authenticated(&headers));
    let mut s = s.lock().unwrap();
    if s.hidden_receipts > 0 && s.receipts.contains_key(&id) {
        s.hidden_receipts -= 1;
        return StatusCode::NOT_FOUND.into_response();
    }
    s.receipts
        .get(&id)
        .map(|r| Json(r.clone()).into_response())
        .unwrap_or_else(|| StatusCode::NOT_FOUND.into_response())
}
async fn batch(HttpState(s): HttpState<Shared>, headers: HeaderMap, body: Bytes) -> Response {
    assert!(authenticated(&headers));
    let body = String::from_utf8(body.to_vec()).unwrap();
    let value: SourceBatch = serde_json::from_str(&body).unwrap();
    let mut s = s.lock().unwrap();
    s.bodies.push(body.clone());
    assert_eq!(value.protocol_version, 1);
    assert!(!value.complete_snapshot);
    assert!(value.tombstones.is_empty());
    assert_eq!(value.config_revision, 1);
    assert_eq!(value.expected_sequence, s.sequence);
    assert_eq!(value.checkpoint_before, s.checkpoint);
    let active = s.active.as_ref().unwrap();
    assert_eq!(value.run_id, active.run_id);
    assert_eq!(value.generation, active.generation);
    if s.fault == Fault::RejectBeforeCommit {
        s.fault = Fault::None;
        return StatusCode::SERVICE_UNAVAILABLE.into_response();
    }
    assert!(value.entries.len() <= 16);
    let mut accepted = 0;
    let mut unchanged = 0;
    for e in &value.entries {
        if s.committed.get(&e.external_id) == Some(&e.version) {
            unchanged += 1;
        } else {
            s.committed.insert(e.external_id.clone(), e.version.clone());
            accepted += 1;
        }
    }
    s.sequence += 1;
    s.checkpoint = value.checkpoint_after;
    let r = SourceBatchReceipt {
        request_id: value.request_id.clone(),
        run_id: value.run_id,
        sequence: s.sequence,
        committed_at: TIME,
        accepted,
        unchanged,
        source_ids: vec![],
    };
    s.receipts.insert(value.request_id, r.clone());
    if s.fault == Fault::LostAfterCommit {
        s.fault = Fault::None;
        s.hidden_receipts = 1;
        return StatusCode::BAD_GATEWAY.into_response();
    }
    (StatusCode::CREATED, Json(r)).into_response()
}
async fn finish(
    HttpState(s): HttpState<Shared>,
    headers: HeaderMap,
    Json(value): Json<Value>,
) -> Response {
    assert!(authenticated(&headers));
    let mut s = s.lock().unwrap();
    let run = s.last_run.as_ref().unwrap();
    assert_eq!(value["run_id"], run.run_id);
    assert_eq!(value["generation"], run.generation);
    assert_eq!(value["outcome"], "partial");
    s.active = None;
    if s.fault == Fault::LostFinishAck {
        s.fault = Fault::None;
        s.forbid_status = true;
        return StatusCode::BAD_GATEWAY.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}
async fn server(fault: Fault) -> (String, Shared, tokio::task::JoinHandle<()>) {
    let mock = Arc::new(Mutex::new(Mock {
        connection: Uuid::new_v4().to_string(),
        sequence: 0,
        checkpoint: None,
        active: None,
        last_run: None,
        receipts: BTreeMap::new(),
        bodies: vec![],
        committed: BTreeMap::new(),
        fault,
        hidden_receipts: 0,
        starts: 0,
        forbid_status: false,
        redirect: false,
        trap: 0,
    }));
    let app = Router::new()
        .route("/api/v1/source-ingest/status", get(status))
        .route("/api/v1/source-ingest/runs", post(begin))
        .route("/api/v1/source-ingest/batches", post(batch))
        .route("/api/v1/source-ingest/receipts/{id}", get(receipt))
        .route("/api/v1/source-ingest/finish", post(finish))
        .route(
            "/trap",
            get(|HttpState(s): HttpState<Shared>| async move {
                s.lock().unwrap().trap += 1;
                StatusCode::OK
            }),
        )
        .with_state(mock.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (origin, mock, task)
}
#[tokio::test]
async fn local_file_to_custom_ingest_recovers_unknown_batch_and_finish_without_cursor_or_body_drift()
 {
    for fault in [
        Fault::RejectBeforeCommit,
        Fault::LostAfterCommit,
        Fault::LostFinishAck,
    ] {
        let (origin, mock, task) = server(fault).await;
        let connection = mock.lock().unwrap().connection.clone();
        let f = Fixture::new(origin, connection);
        let id = Uuid::new_v4().to_string();
        let raw = session(&id, "Please repair the synthetic report layout.", true)
            + &row(
                TIME - 4700,
                "response_item",
                json!({"type":"function_call_output","output":"SYNTHETIC_TOOL_TRACE_CANARY"}),
            );
        f.write("selected.jsonl", raw.as_bytes());
        assert!(run_at(&f.options, TIME).await.is_err());
        mock.lock().unwrap().forbid_status = false;
        let bytes = fs::read(&f.options.state_file).unwrap();
        let saved: State = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved.sequence, 0);
        assert!(saved.objects.is_empty());
        let original = saved.pending.as_ref().unwrap().envelope.clone().unwrap();
        let request = saved.pending.as_ref().unwrap().request_id.clone();
        if fault == Fault::LostAfterCommit {
            mock.lock().unwrap().sequence = 2;
            assert!(run_at(&f.options, TIME).await.is_err());
            mock.lock().unwrap().sequence = 1;
            let checkpoint = mock.lock().unwrap().checkpoint.clone();
            mock.lock().unwrap().checkpoint = Some("different-commit".into());
            assert!(run_at(&f.options, TIME).await.is_err());
            mock.lock().unwrap().checkpoint = checkpoint;
            let active = mock.lock().unwrap().active.clone();
            mock.lock().unwrap().active = Some(Run {
                run_id: Uuid::new_v4().to_string(),
                generation: 999,
            });
            assert!(run_at(&f.options, TIME).await.is_err());
            mock.lock().unwrap().active = active;
            let retained: State =
                serde_json::from_slice(&fs::read(&f.options.state_file).unwrap()).unwrap();
            assert_eq!(retained.sequence, 0);
            assert!(retained.objects.is_empty());
            assert!(retained.pending.is_some());
        }
        for canary in [
            "SYNTHETIC_TOOL_TRACE_CANARY",
            "SYNTHETIC_ASSISTANT_CANARY",
            "SYNTHETIC_CONFIG_CANARY",
            "SYNTHETIC_PRIVATE_PATH",
            "SYNTHETIC_ATTACHMENT_CANARY",
        ] {
            assert!(!original.contains(canary));
            assert!(!String::from_utf8_lossy(&bytes).contains(canary));
        }
        assert!(!String::from_utf8_lossy(&bytes).contains(&"a".repeat(64)));
        // Mutate the source after uncertainty: recovery must still use the first
        // staged request, not re-extract new file text under that UUID.
        f.write(
            "selected.jsonl",
            session(&id, "This later edit belongs to another invocation.", true).as_bytes(),
        );
        let receipt = run_at(&f.options, TIME).await.unwrap();
        assert_eq!(receipt.request_id.as_deref(), Some(request.as_str()));
        assert_eq!(receipt.accepted, 1);
        let state: State =
            serde_json::from_slice(&fs::read(&f.options.state_file).unwrap()).unwrap();
        assert_eq!(state.sequence, 1);
        assert!(state.pending.is_none());
        assert_eq!(state.objects.len(), 1);
        let s = mock.lock().unwrap();
        assert_eq!(s.starts, 1);
        assert_eq!(s.committed.len(), 1);
        assert!(s.bodies.iter().all(|b| b == &original));
        assert_eq!(s.sequence, 1);
        drop(s);
        task.abort();
    }
}
#[tokio::test]
async fn only_proven_human_prose_is_evidence_finished_partial_append_and_inode_replacement_deduplicate()
 {
    let (origin, mock, task) = server(Fault::None).await;
    let f = Fixture::new(origin, mock.lock().unwrap().connection.clone());
    let id = Uuid::new_v4().to_string();
    let raw = header(&id, "cli", "user")
        + &start("human-turn", "user")
        + &user("Please review the synthetic release notes.")
        + &user("api_key = SYNTHETIC_CREDENTIAL_CANARY")
        + &row(
            TIME - 4800,
            "response_item",
            json!({"type":"message","role":"user","content":[{"type":"input_text","text":"SYNTHETIC_CONTEXT_CANARY"}]}),
        )
        + &row(
            TIME - 4790,
            "response_item",
            json!({"type":"message","role":"assistant","content":[{"type":"output_text","text":"SYNTHETIC_ASSISTANT_CANARY"}]}),
        );
    f.write("human.jsonl", raw.as_bytes());
    f.write(
        "exec.jsonl",
        (header(&Uuid::new_v4().to_string(), "exec", "user")
            + &start("automation", "user")
            + &user("AUTOMATION_CANARY")
            + &complete("automation"))
            .as_bytes(),
    );
    f.write(
        "subagent.jsonl",
        (header(&Uuid::new_v4().to_string(), "cli", "subagent")
            + &start("subagent", "user")
            + &user("SUBAGENT_CANARY"))
            .as_bytes(),
    );
    symlink(
        f.options.directory.join("human.jsonl"),
        f.options.directory.join("linked.jsonl"),
    )
    .unwrap();
    let r = run_at(&f.options, TIME).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert_eq!(r.unsupported_files, 2);
    assert_eq!(mock.lock().unwrap().committed.len(), 1);
    let initial = mock.lock().unwrap().bodies.last().cloned().unwrap();
    assert!(initial.contains("Please review"));
    assert!(initial.contains("Turn state: partial"));
    for text in [
        "SYNTHETIC_CREDENTIAL_CANARY",
        "SYNTHETIC_CONTEXT_CANARY",
        "SYNTHETIC_ASSISTANT_CANARY",
        "AUTOMATION_CANARY",
        "SUBAGENT_CANARY",
    ] {
        assert!(!initial.contains(text));
    }
    let r = run_at(&f.options, TIME).await.unwrap();
    assert_eq!(r.accepted, 0);
    assert_eq!(mock.lock().unwrap().committed.len(), 1);
    // A completed line changes the same source object's semantic version.
    f.write(
        "human.jsonl",
        (raw.clone() + &complete("human-turn")).as_bytes(),
    );
    let r = run_at(&f.options, TIME).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert_eq!(mock.lock().unwrap().committed.len(), 1);
    assert!(
        mock.lock()
            .unwrap()
            .bodies
            .last()
            .unwrap()
            .contains("finished_marker")
    );
    // Atomic replacement with identical contents changes inode/fingerprint, but
    // does not duplicate the semantic evidence or turn it into another object.
    let p = f.options.directory.join("human.jsonl");
    fs::rename(&p, f.home.join("old-inode")).unwrap();
    f.write("human.jsonl", (raw + &complete("human-turn")).as_bytes());
    let r = run_at(&f.options, TIME).await.unwrap();
    assert_eq!(r.accepted, 0);
    assert_eq!(mock.lock().unwrap().committed.len(), 1);
    task.abort();
}
#[tokio::test]
async fn incomplete_lines_and_recent_active_turns_defer_then_import_only_complete_user_records() {
    let (origin, mock, task) = server(Fault::None).await;
    let f = Fixture::new(origin, mock.lock().unwrap().connection.clone());
    let id = Uuid::new_v4().to_string();
    let raw = header(&id, "cli", "user")
        + &start("human-turn", "user")
        + &row(
            TIME - 100,
            "event_msg",
            json!({"type":"user_message","message":"Please inspect the synthetic graph."}),
        );
    f.write("partial.jsonl",(raw.clone()+"{\"type\":\"event_msg\",\"payload\":{\"type\":\"user_message\",\"message\":\"INCOMPLETE_CANARY").as_bytes());
    let r = run_at(&f.options, TIME).await.unwrap();
    assert_eq!(r.accepted, 0);
    assert_eq!(r.deferred_files, 1);
    assert!(mock.lock().unwrap().committed.is_empty());
    let r = run_at(&f.options, TIME + 4000).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert_eq!(r.deferred_files, 1);
    assert!(
        !mock
            .lock()
            .unwrap()
            .bodies
            .last()
            .unwrap()
            .contains("INCOMPLETE_CANARY")
    );
    f.write("partial.jsonl", (raw + &complete("human-turn")).as_bytes());
    let r = run_at(&f.options, TIME + 4000).await.unwrap();
    assert_eq!(r.accepted, 1);
    assert_eq!(mock.lock().unwrap().committed.len(), 1);
    task.abort();
}
#[tokio::test]
async fn destination_auth_redirect_and_unsafe_private_scope_fail_without_batch_or_cross_origin_send()
 {
    let (origin, mock, task) = server(Fault::None).await;
    let mut f = Fixture::new(origin, mock.lock().unwrap().connection.clone());
    f.write(
        "human.jsonl",
        session(
            &Uuid::new_v4().to_string(),
            "Please inspect the synthetic chart.",
            true,
        )
        .as_bytes(),
    );
    let original = f.options.connection_id.clone();
    f.options.connection_id = Uuid::new_v4().to_string();
    assert!(run_at(&f.options, TIME).await.is_err());
    f.options.connection_id = original;
    mock.lock().unwrap().forbid_status = true;
    assert!(run_at(&f.options, TIME).await.is_err());
    mock.lock().unwrap().forbid_status = false;
    mock.lock().unwrap().redirect = true;
    assert!(run_at(&f.options, TIME).await.is_err());
    assert_eq!(mock.lock().unwrap().trap, 0);
    mock.lock().unwrap().redirect = false;
    let state = f.options.state_file.clone();
    f.options.state_file = f.options.directory.join("inside-root.json");
    assert!(run_at(&f.options, TIME).await.is_err());
    f.options.state_file = state;
    fs::create_dir(f.home.join(".git")).unwrap();
    assert!(run_at(&f.options, TIME).await.is_err());
    fs::remove_dir(f.home.join(".git")).unwrap();
    let original = f.options.token_file.clone();
    symlink(&original, f.home.join("token-link")).unwrap();
    f.options.token_file = f.home.join("token-link");
    assert!(run_at(&f.options, TIME).await.is_err());
    f.options.token_file = original;
    let fifo = f.home.join("token-fifo");
    let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
    // SAFETY: name is a live NUL-terminated path in this owned synthetic fixture.
    assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
    f.options.token_file = fifo;
    assert!(run_at(&f.options, TIME).await.is_err());
    assert!(mock.lock().unwrap().bodies.is_empty());
    assert_eq!(mock.lock().unwrap().starts, 0);
    task.abort();
}

#[test]
fn mutable_nonsticky_ancestor_is_refused_for_source_token_and_state() {
    let f = Fixture::new(
        "https://synthetic.example.test".into(),
        Uuid::new_v4().to_string(),
    );
    let ancestor = f.home.join("mutable");
    fs::create_dir(&ancestor).unwrap();
    let private = ancestor.join("private");
    fs::create_dir(&private).unwrap();
    fs::set_permissions(&private, fs::Permissions::from_mode(0o700)).unwrap();
    let token = private.join("token");
    fs::write(&token, "a".repeat(64)).unwrap();
    fs::set_permissions(&token, fs::Permissions::from_mode(0o600)).unwrap();
    // Every final directory/file is private, but the subtree is renameable.
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o777)).unwrap();
    assert!(files::Root::open(&private).is_err());
    assert!(transport::token_path(&token).is_err());
    assert!(transport::state_path(&private.join("state.json")).is_err());
    fs::set_permissions(&ancestor, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(files::Root::open(&private).is_ok());
    assert!(transport::token_path(&token).is_ok());
    assert!(transport::state_path(&private.join("state.json")).is_ok());
}
