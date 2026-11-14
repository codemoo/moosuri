//! One production-boundary regression: real Store + router, only finish ACK is faulted.
use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
};
use moosuri_connectors::work_sessions::{Options, run_loopback_fixture};
use moosuri_core::{CreateSourceConnection, SourceConnectionKind};
use moosuri_store::Store;
use serde_json::{Value, json};
use std::{
    fs::{self, File, FileTimes},
    net::TcpListener,
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
struct Counts {
    block_finish: AtomicBool,
    batches: AtomicUsize,
    finishes: AtomicUsize,
}
fn serve(
    listener: &TcpListener,
    store: Store,
    origin: &str,
    counts: Arc<Counts>,
) -> (moosuri_agent::Runtime, tokio::task::JoinHandle<()>) {
    let config = moosuri_server::ServerConfig::new(origin, true).unwrap();
    let runtime = config.start_runtime(store.clone());
    let app: Router = moosuri_server::router_with_runtime(store, config, runtime.clone()).layer(
        middleware::from_fn(move |request: Request<Body>, next: Next| {
            let c = counts.clone();
            async move {
                match request.uri().path() {
                    "/api/v1/source-ingest/batches" => {
                        c.batches.fetch_add(1, Ordering::SeqCst);
                    }
                    "/api/v1/source-ingest/finish" => {
                        c.finishes.fetch_add(1, Ordering::SeqCst);
                        if c.block_finish.load(Ordering::SeqCst) {
                            return StatusCode::BAD_GATEWAY.into_response();
                        }
                    }
                    _ => {}
                }
                next.run(request).await
            }
        }),
    );
    let listener = tokio::net::TcpListener::from_std(listener.try_clone().unwrap()).unwrap();
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (runtime, task)
}
#[tokio::test]
async fn actual_store_router_restart_reconciles_committed_receipt_without_duplicate_or_unknown_rewrite()
 {
    let home = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let data = home.path().join("store");
    let store = Store::open(&data).unwrap();
    let password = "Synthetic-F41-recovery-password";
    store
        .bootstrap(
            "Synthetic owner".into(),
            "f41-owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login("f41-owner@example.test".into(), password.into())
        .await
        .unwrap();
    let connection = store
        .create_source_connection(
            &login.user,
            CreateSourceConnection {
                name: "Synthetic local requests".into(),
                kind: SourceConnectionKind::CustomHttp,
                scope: "synthetic-selected-work-requests".into(),
                remote_account: "external_producer".into(),
                mail_connection_id: None,
                sync_enabled: false,
                poll_interval_seconds: 60,
            },
        )
        .await
        .unwrap();
    let token = store
        .issue_source_token(&login.user, connection.id.clone(), 3600)
        .await
        .unwrap()
        .token;
    let input = home.path().join("chosen-input");
    fs::create_dir(&input).unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o700)).unwrap();
    let session = uuid::Uuid::new_v4().to_string();
    let rows = [
        json!({"timestamp":"2024-01-01T00:00:00Z","type":"session_meta","payload":{"id":session,"session_id":session,"source":"cli","thread_source":"user","cwd":"/synthetic-unused","originator":"codex_cli_rs","cli_version":"synthetic-verified-shape","timestamp":"2024-01-01T00:00:00Z"}}),
        json!({"timestamp":"2024-01-01T00:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"fixture-turn","root_turn_id":"fixture-turn","turn_attribution":{"turn_id":"fixture-turn","turn_trigger":"user","parent_turn_id":null,"initiating_agent_path":null,"root_turn_id":"fixture-turn"}}}),
        json!({"timestamp":"2024-01-01T00:00:02Z","type":"event_msg","payload":{"type":"user_message","message":"Please inspect the synthetic recovery graph."}}),
        json!({"timestamp":"2024-01-01T00:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"fixture-turn","last_agent_message":"SYNTHETIC_ASSISTANT_NOT_EVIDENCE","error":null}}),
    ];
    let path = input.join("selected.jsonl");
    fs::write(
        &path,
        rows.into_iter()
            .map(|r| r.to_string() + "\n")
            .collect::<String>(),
    )
    .unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs()
        - 120;
    File::open(&path)
        .unwrap()
        .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(time)))
        .unwrap();
    let token_file = home.path().join("token");
    fs::write(&token_file, &token).unwrap();
    fs::set_permissions(&token_file, fs::Permissions::from_mode(0o600)).unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let counts = Arc::new(Counts {
        block_finish: AtomicBool::new(true),
        batches: AtomicUsize::new(0),
        finishes: AtomicUsize::new(0),
    });
    let (runtime, task) = serve(&listener, store.clone(), &origin, counts.clone());
    let options = Options {
        directory: input,
        origin: origin.clone(),
        connection_id: connection.id.clone(),
        token_file,
        state_file: home.path().join("cursor.json"),
    };
    assert!(run_loopback_fixture(&options).await.is_err());
    let staged: Value = serde_json::from_slice(&fs::read(&options.state_file).unwrap()).unwrap();
    assert_eq!(staged["sequence"], 0);
    assert!(staged["objects"].as_object().unwrap().is_empty());
    assert_eq!(staged["pending"]["committed"], true);
    let request = staged["pending"]["request_id"].as_str().unwrap().to_owned();
    let actual = store
        .source_batch_receipt(&login.user, connection.id.clone(), request.clone())
        .await
        .unwrap();
    assert_eq!(actual.sequence, 1);
    assert_eq!(actual.accepted, 1);
    assert_eq!(actual.source_ids.len(), 1);
    task.abort();
    let _ = task.await;
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    // Reopen is the real startup hook that turns unfinished source runs unknown.
    let store = Store::open(&data).unwrap();
    let before = store
        .source_sync_runs(&login.user, connection.id.clone())
        .await
        .unwrap();
    assert_eq!(before.len(), 1);
    assert_eq!(before[0].state, "unknown");
    counts.block_finish.store(false, Ordering::SeqCst);
    let (runtime, task) = serve(&listener, store.clone(), &origin, counts.clone());
    // Establish the production conflict that the old all-accepting mock missed.
    let refused=reqwest::Client::new().post(format!("{origin}/api/v1/source-ingest/finish")).bearer_auth(&token).json(&json!({"run_id":staged["pending"]["run"]["run_id"],"generation":staged["pending"]["run"]["generation"],"outcome":"partial","error":null,"retry_after_seconds":null})).send().await.unwrap();
    assert_eq!(refused.status().as_u16(), 409);
    let finishes = counts.finishes.load(Ordering::SeqCst);
    let recovered = run_loopback_fixture(&options).await.unwrap();
    assert_eq!(recovered.request_id.as_deref(), Some(request.as_str()));
    assert_eq!(recovered.accepted, 1);
    let committed: Value = serde_json::from_slice(&fs::read(&options.state_file).unwrap()).unwrap();
    assert_eq!(committed["sequence"], 1);
    assert!(committed["pending"].is_null());
    assert_eq!(committed["objects"].as_object().unwrap().len(), 1);
    let cap = store.authenticate_source_token(token).await.unwrap();
    let status = store.source_ingest_status(&cap).await.unwrap();
    assert_eq!(status.local_sequence, 1);
    assert_eq!(
        status.checkpoint.as_deref(),
        committed["checkpoint"].as_str()
    );
    assert!(status.active_run.is_none());
    assert_eq!(counts.batches.load(Ordering::SeqCst), 1);
    assert_eq!(counts.finishes.load(Ordering::SeqCst), finishes);
    let after = store
        .source_sync_runs(&login.user, connection.id)
        .await
        .unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, before[0].id);
    assert_eq!(after[0].state, "unknown");
    assert_eq!(after[0].error, before[0].error);
    task.abort();
    let _ = task.await;
    runtime.shutdown().await;
}
