//! Actual historical recovery, stopped CLI allocation, HTTP probes/STT and Runtime.
#![cfg(feature = "test-fixtures")]
use axum::{Router, body::Body, extract::State, response::Response, routing::post};
use moosuri_core::*;
use moosuri_store::{CredentialChange, ProviderChange, Store};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::Path,
    process::Command,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
const EMAIL: &str = "forward@example.test";
const PASSWORD: &str = "Synthetic-forward-current-password";
#[derive(Default)]
struct Upstream {
    calls: AtomicUsize,
    audio: AtomicUsize,
    hold: AtomicUsize,
    entered: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
fn stream(text: Value, tool: bool) -> Response {
    let delta = if tool {
        json!({"role":"assistant","tool_calls":[{"index":0,"id":"probe-call","type":"function","function":{"name":text["name"],"arguments":text["args"]}}]})
    } else {
        json!({"role":"assistant","content":text})
    };
    let a = json!({"id":"synthetic-forward","model":"synthetic-model","choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let b = json!({"id":"synthetic-forward","model":"synthetic-model","choices":[{"index":0,"delta":{},"finish_reason":if tool{"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":9,"completion_tokens":3}});
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(format!(
            "data: {a}\n\ndata: {b}\n\ndata: [DONE]\n\n"
        )))
        .unwrap()
}
async fn model(State(s): State<Arc<Upstream>>, axum::Json(b): axum::Json<Value>) -> Response {
    s.calls.fetch_add(1, Ordering::SeqCst);
    s.entered.notify_one();
    if s.hold.load(Ordering::SeqCst) == 1 {
        s.release.notified().await;
    }
    if let Some(tools) = b["tools"].as_array() {
        let name = tools[0]["function"]["name"].as_str().unwrap();
        if name == "source_memory_selection" {
            return stream(json!({"name":name,"args":"{\"selected\":[0]}"}), true);
        }
        if name == "moosuri_verification_ping" {
            if let Some(result) = b["messages"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["role"] == "tool")
            {
                return stream(result["content"].clone(), false);
            }
            return stream(json!({"name":name,"args":"{}"}), true);
        }
    }
    stream(json!("OK"), false)
}
fn cli(data: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .arg("--data-dir")
        .arg(data)
        .args(args)
        .output()
        .unwrap()
}
fn success(o: std::process::Output) -> Vec<u8> {
    assert!(o.status.success(), "{}", String::from_utf8_lossy(&o.stderr));
    o.stdout
}
fn status(data: &Path) -> Value {
    let bytes = success(cli(data, &["recovery-budget", "status"]));
    serde_json::Deserializer::from_slice(&bytes)
        .into_iter::<Value>()
        .next()
        .unwrap()
        .unwrap()
}
fn allocation(
    data: &Path,
    restore: &str,
    id: &str,
    calls: &str,
    audio: &str,
) -> std::process::Output {
    cli(
        data,
        &[
            "recovery-budget",
            "allocate",
            "--restore-id",
            restore,
            "--request-id",
            id,
            "--calls",
            calls,
            "--audio-seconds",
            audio,
        ],
    )
}
fn private(parent: &Path, name: &str) -> std::path::PathBuf {
    let p = parent.join(name);
    fs::create_dir(&p).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    p
}
async fn config(store: &Store, user: &moosuri_store::UserContext, base: &str) -> ProviderSummary {
    store
        .create_provider(
            user,
            ProviderChange {
                name: "Synthetic forward fixture".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(format!("{base}/v1")),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap()
}
struct App {
    server: tokio::task::JoinHandle<()>,
    runtime: moosuri_agent::Runtime,
    origin: String,
    cookie: String,
    client: reqwest::Client,
    account: String,
}
impl App {
    async fn start(store: Store, audio: &str, account: String) -> Self {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let config = moosuri_server::ServerConfig::new(&origin, true)
            .unwrap()
            .with_audio_fixture(format!("{audio}/"))
            .unwrap();
        let runtime = config.start_runtime(store.clone());
        let router = moosuri_server::router_with_runtime(store, config, runtime.clone());
        let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::new();
        let login = client
            .post(format!("{origin}/api/v1/auth/login"))
            .header("Origin", &origin)
            .json(&json!({"email":EMAIL,"password":PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(login.status(), 200);
        let cookie = login.headers()[reqwest::header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        Self {
            server,
            runtime,
            origin,
            cookie,
            client,
            account,
        }
    }
    async fn request(&self, path: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("{}/api/v1{path}", self.origin))
            .header("Origin", &self.origin)
            .header("Cookie", &self.cookie)
            .header("X-Moosuri-Account", &self.account)
            .json(&body)
            .send()
            .await
            .unwrap()
    }
    async fn stop(self) {
        self.runtime.shutdown().await;
        self.server.abort();
        drop(self.runtime);
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}
fn wav() -> Vec<u8> {
    let size = 32000u32;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((size + 36).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(32000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend(size.to_le_bytes());
    b.resize(32044, 0);
    b
}
async fn audio_request(
    app: &App,
    path: &str,
    provider: Option<&ProviderSummary>,
) -> reqwest::Response {
    let mut q = app
        .client
        .post(format!("{}/api/v1{path}", app.origin))
        .header("Origin", &app.origin)
        .header("Cookie", &app.cookie)
        .header("X-Moosuri-Account", &app.account)
        .header("Content-Type", "audio/wav");
    if let Some(p) = provider {
        q = q
            .header("X-Audio-Provider", &p.id)
            .header("X-Audio-Revision", p.revision);
    }
    q.body(wav()).send().await.unwrap()
}
#[tokio::test]
async fn historical_restore_explicit_forward_requests_and_stt_are_bounded_without_refunds_or_accounting_reset()
 {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let source = root.path().join("source");
    let dest = root.path().join("restored");
    let kit = private(root.path(), "kit");
    let out = private(root.path(), "out");
    let upstream = Arc::new(Upstream::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let s = upstream.clone();
    let fixture = Router::new()
        .route("/v1/chat/completions", post(model))
        .route(
            "/v1/audio/transcriptions",
            post(move || {
                let s = s.clone();
                async move {
                    s.audio.fetch_add(1, Ordering::SeqCst);
                    axum::Json(json!({"text":"the blue bird sings beside the quiet river"}))
                }
            }),
        )
        .with_state(upstream.clone());
    let mock = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let store = Store::open(&source).unwrap();
    store
        .bootstrap("Synthetic".into(), EMAIL.into(), PASSWORD.into(), false)
        .await
        .unwrap();
    let user = store
        .login(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let owner = user.account().id.clone();
    let p = config(&store, &user, &base).await;
    store
        .record_model_probe(
            &user,
            p.id.clone(),
            p.revision,
            "synthetic-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: p.id,
                provider_revision: p.revision,
                model: "synthetic-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    // Create a real surviving lower-bound receipt before backup; no fake provenance.
    let thread = store
        .create_thread(&user, "Before restore".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &user,
            thread.id,
            uuid::Uuid::new_v4().to_string(),
            "Synthetic earlier work".into(),
        )
        .await
        .unwrap();
    let rt = moosuri_agent::Runtime::start(store.clone());
    tokio::time::timeout(Duration::from_secs(8), async {
        while store
            .usage_summary(&user)
            .await
            .unwrap()
            .reported_output_tokens
            == 0
        {
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    rt.shutdown().await;
    drop(rt);
    let lower = store.usage_summary(&user).await.unwrap();
    assert_eq!(lower.dispatched_calls, 1);
    drop(store);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let identity = kit.join("identity");
    let recipient = moosuri_store::backup::keygen(&identity).unwrap();
    moosuri_store::backup::setup(&source, &recipient, &identity).unwrap();
    let export = out.join("source.age");
    moosuri_store::backup::create(&source, &export).unwrap();
    let journal = out.join("journal.age");
    let head = kit.join("head.json");
    let publication =
        moosuri_store::backup::publish_journal(&source, &export, &journal, &head).unwrap();
    let restored = moosuri_store::backup::restore_historical(
        &export,
        &journal,
        &head,
        &publication.trusted_head_sha256,
        &identity,
        &identity,
        &dest,
    )
    .unwrap();
    moosuri_store::recover_account(
        &dest,
        owner.clone(),
        Role::Admin,
        "Synthetic-temporary-forward-password".into(),
    )
    .await
    .unwrap();
    let store = Store::open(&dest).unwrap();
    let temporary = store
        .login(EMAIL.into(), "Synthetic-temporary-forward-password".into())
        .await
        .unwrap();
    let user = store
        .change_password(&temporary.user, PASSWORD.into())
        .await
        .unwrap()
        .user;
    let fresh = config(&store, &user, &base).await;
    assert!(store.usage_summary(&user).await.is_err());
    let app = App::start(store.clone(), &base, owner.clone()).await;
    let probe_path = format!("/admin/providers/{}/probe", fresh.id);
    let input = json!({"revision":fresh.revision,"model":"synthetic-model","effort":null});
    let calls = upstream.calls.load(Ordering::SeqCst);
    let blocked = app.request(&probe_path, input.clone()).await;
    assert_eq!(blocked.status(), 409);
    assert_eq!(
        blocked.json::<Value>().await.unwrap()["code"],
        "recovery_forward_allocation_required"
    );
    assert_eq!(upstream.calls.load(Ordering::SeqCst), calls);
    app.stop().await;
    drop(store);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let initial = status(&dest);
    let restore = initial["restore_id"].as_str().unwrap();
    let request = uuid::Uuid::new_v4().to_string();
    success(allocation(&dest, restore, &request, "8", "2"));
    success(allocation(&dest, restore, &request, "8", "2"));
    assert!(
        !allocation(&dest, restore, &request, "9", "2")
            .status
            .success()
    );
    assert!(
        !allocation(
            &dest,
            &uuid::Uuid::new_v4().to_string(),
            &uuid::Uuid::new_v4().to_string(),
            "1",
            "0"
        )
        .status
        .success()
    );
    assert_eq!(status(&dest)["allocation_count"], 1);
    let store = Store::open(&dest).unwrap();
    let user = store
        .login(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    let app = App::start(store.clone(), &base, owner.clone()).await;
    let stopped_only = allocation(&dest, restore, &uuid::Uuid::new_v4().to_string(), "1", "0");
    assert!(!stopped_only.status.success());
    assert_eq!(app.request(&probe_path, input.clone()).await.status(), 200); //1
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: fresh.id.clone(),
                provider_revision: fresh.revision,
                model: "synthetic-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&user, "Useful forward work".into())
        .await
        .unwrap();
    let turn = store
        .submit_turn(
            &user,
            thread.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Reply OK".into(),
        )
        .await
        .unwrap();
    app.runtime.wake();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let state = store
                .conversation(&user, thread.id.clone(), None)
                .await
                .unwrap();
            if state
                .turns
                .iter()
                .any(|t| t.run.id == turn.run.id && t.run.state == RunState::Completed)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap(); //2
    assert_eq!(
        app.request(
            &format!("/admin/providers/{}/tool-probe", fresh.id),
            input.clone()
        )
        .await
        .status(),
        200
    ); //3,4
    let collection = store
        .create_collection(
            &user,
            "Forward literal extraction".into(),
            uuid::Uuid::new_v4().to_string(),
        )
        .await
        .unwrap();
    store
        .set_memory_extraction(
            &user,
            collection.id.clone(),
            MemoryExtractionChange {
                expected_revision: 0,
                enabled: true,
                attribution_mode: "reported".into(),
                reported_speaker: String::new(),
                daily_calls: 3,
                daily_input_bytes: 24000,
                daily_output_tokens: 3072,
            },
        )
        .await
        .unwrap();
    store
        .ingest_text(
            &user,
            collection.id.clone(),
            IngestBatch {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_cursor: 0,
                next_cursor: 1,
                complete_snapshot: false,
                entries: vec![SourceInput {
                    external_id: "forward".into(),
                    version: "1".into(),
                    expected_revision: None,
                    title: "Synthetic source".into(),
                    text: "I prefer synthetic evidence.".into(),
                    speaker: Some("Synthetic reporter".into()),
                    observed_at: 100,
                    valid_from: None,
                    valid_until: None,
                }],
            },
        )
        .await
        .unwrap();
    app.runtime.wake();
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            if store
                .memory_extraction_status(&user, collection.id.clone())
                .await
                .unwrap()
                .state
                == "completed"
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap(); //5
    let audio = store
        .create_provider(
            &user,
            ProviderChange {
                name: "Synthetic STT".into(),
                config: ProviderConfig {
                    kind: ProviderKind::OpenAi,
                    base_url: None,
                    allowed_ips: vec![],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Replace(zeroize::Zeroizing::new(
                    "Synthetic-STT-credential".into(),
                )),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        audio_request(&app, "/admin/audio/probe", Some(&audio))
            .await
            .status(),
        200
    ); //6
    let audio_id = uuid::Uuid::new_v4().to_string();
    assert!(
        audio_request(&app, &format!("/audio/{audio_id}"), None)
            .await
            .status()
            .is_success()
    ); //7
    tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let receipt = app
                .client
                .get(format!("{}/api/v1/audio/{audio_id}", app.origin))
                .header("Cookie", &app.cookie)
                .header("X-Moosuri-Account", &app.account)
                .send()
                .await
                .unwrap();
            assert!(receipt.status().is_success());
            if receipt.json::<Value>().await.unwrap()["state"] == "ready" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    let a = app.request(&probe_path, input.clone());
    let b = app.request(&probe_path, input.clone());
    let (a, b) = tokio::join!(a, b);
    assert_eq!(
        [a.status().as_u16(), b.status().as_u16()]
            .iter()
            .filter(|s| **s == 200)
            .count(),
        1
    ); //8 only one wins last slot
    assert_eq!(
        store
            .recovery_budget_status(&user)
            .await
            .unwrap()
            .committed_calls,
        8
    );
    assert_eq!(
        store
            .recovery_budget_status(&user)
            .await
            .unwrap()
            .committed_stt_duration_ms,
        2000
    );
    assert!(store.usage_summary(&user).await.is_err());
    let exhausted = app.request(&probe_path, input.clone()).await;
    assert_eq!(exhausted.status(), 409);
    assert_eq!(
        exhausted.json::<Value>().await.unwrap()["code"],
        "recovery_forward_allowance_exhausted"
    );
    app.stop().await;
    drop(store);
    tokio::time::sleep(Duration::from_millis(100)).await;
    let spent = status(&dest);
    assert_eq!(spent["committed_calls"], 8);
    // A new stopped allocation resumes useful work without clearing old unknown.
    success(allocation(
        &dest,
        restore,
        &uuid::Uuid::new_v4().to_string(),
        "1",
        "0",
    ));
    let store = Store::open(&dest).unwrap();
    let user = store
        .login(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    // Select text-only again to isolate a single held dispatch from tool classifiers.
    let plain = config(&store, &user, &base).await;
    store
        .record_model_probe(
            &user,
            plain.id.clone(),
            plain.revision,
            "synthetic-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(ModelSelection {
                source: Default::default(),
                provider_id: plain.id,
                provider_revision: plain.revision,
                model: "synthetic-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    upstream.hold.store(1, Ordering::SeqCst);
    let app = App::start(store.clone(), &base, owner).await;
    let baseline = upstream.calls.load(Ordering::SeqCst);
    let thread = store
        .create_thread(&user, "Unknown outcome".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &user,
            thread.id,
            uuid::Uuid::new_v4().to_string(),
            "Synthetic held call".into(),
        )
        .await
        .unwrap();
    app.runtime.wake();
    tokio::time::timeout(Duration::from_secs(8), async {
        while upstream.calls.load(Ordering::SeqCst) == baseline {
            tokio::time::sleep(Duration::from_millis(20)).await
        }
    })
    .await
    .unwrap();
    app.stop().await;
    upstream.release.notify_one();
    drop(store);
    tokio::time::sleep(Duration::from_millis(100)).await;
    moosuri_store::backup::setup(&dest, &recipient, &identity).unwrap();
    let before_delete_export = out.join("before-delete.age");
    moosuri_store::backup::create(&dest, &before_delete_export).unwrap();
    let store = Store::open(&dest).unwrap();
    let user = store
        .login(EMAIL.into(), PASSWORD.into())
        .await
        .unwrap()
        .user;
    // Immutable opaque admission evidence must survive actual account deletion
    // without blocking private tenant purge, restart or matching restore retry.
    let invite = store
        .create_user(
            &user,
            "Synthetic replacement admin".into(),
            "replacement@example.test".into(),
        )
        .await
        .unwrap();
    let restricted = store
        .login("replacement@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let replacement = store
        .change_password(&restricted.user, "Synthetic-replacement-password".into())
        .await
        .unwrap()
        .user;
    let target = store
        .managed_accounts(&user)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == replacement.account().id)
        .unwrap();
    let prepared = store
        .prepare_account_management(
            &user,
            target.user.id,
            AccountManagementInput {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: target.revision,
                action: AccountManagementAction::ChangeRole { role: Role::Admin },
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap();
    let replacement = store
        .login(
            "replacement@example.test".into(),
            "Synthetic-replacement-password".into(),
        )
        .await
        .unwrap()
        .user;
    let target = store
        .managed_accounts(&replacement)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == user.account().id)
        .unwrap();
    let prepared = store
        .prepare_account_management(
            &replacement,
            target.user.id,
            AccountManagementInput {
                request_id: uuid::Uuid::new_v4().to_string(),
                expected_revision: target.revision,
                action: AccountManagementAction::Delete,
            },
        )
        .await
        .unwrap();
    store.apply_account_management(prepared).await.unwrap();
    let deleted = store
        .managed_accounts(&replacement)
        .await
        .unwrap()
        .into_iter()
        .find(|a| a.user.id == user.account().id)
        .unwrap();
    assert!(deleted.deletion.unwrap().local_complete);
    drop(store);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert_eq!(status(&dest)["committed_calls"], 9);
    let retry = moosuri_store::backup::restore_historical(
        &export,
        &journal,
        &head,
        &publication.trusted_head_sha256,
        &identity,
        &identity,
        &dest,
    )
    .unwrap();
    assert!(retry.already_published);
    assert_eq!(retry.installation_id, restored.installation_id);
    assert_eq!(status(&dest)["committed_calls"], 9);
    assert_eq!(status(&dest)["accounting_state"], "accounting_unknown");
    let latest = out.join("after-delete-journal.age");
    let latest_head = kit.join("after-delete-head.json");
    let after_delete =
        moosuri_store::backup::publish_journal(&dest, &before_delete_export, &latest, &latest_head)
            .unwrap();
    let again = root.path().join("restored-again");
    moosuri_store::backup::restore_historical(
        &before_delete_export,
        &latest,
        &latest_head,
        &after_delete.trusted_head_sha256,
        &identity,
        &identity,
        &again,
    )
    .unwrap();
    let next = status(&again);
    assert_eq!(next["accounting_state"], "accounting_unknown");
    assert_eq!(next["allocation_count"], 0);
    assert_eq!(next["committed_calls"], 0);
    assert_ne!(next["restore_id"], json!(restore));
    assert!(
        !allocation(&again, restore, &request, "8", "2")
            .status
            .success()
    );
    // Original source remains usable and was never assigned a recovery allowance.
    assert!(status(&source)["restore_id"].is_null());
    mock.abort();
}
