//! Real Store/router ingestion with synthetic audio transport only.
use axum::{
    Router,
    body::{Body, Bytes},
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Request, StatusCode},
    middleware::{self, Next},
    response::IntoResponse,
    routing::post,
};
use moosuri_connectors::{
    audio::AudioClient,
    calls::{Options, run_loopback_fixture},
};
use moosuri_core::{CreateSourceConnection, SourceConnectionKind, UNATTRIBUTED_CALL_SPEAKER};
use moosuri_store::Store;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, FileTimes},
    os::unix::fs::PermissionsExt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, UNIX_EPOCH},
};
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn write(path: &std::path::Path, bytes: &[u8], settled: bool) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    if settled {
        File::open(path)
            .unwrap()
            .set_times(FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1000)))
            .unwrap();
    }
}
fn wav() -> Vec<u8> {
    let n = 121 * 16000 * 2;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend(((36 + n) as u32).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(32000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((n as u32).to_le_bytes());
    b.resize(44 + n, 0);
    b
}
#[tokio::test]
async fn calls_actual_ingest_defers_changes_recovers_receipts_and_never_resends_unknown_chunks() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let home = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(home.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let input = home.path().join("explicit-export");
    fs::create_dir(&input).unwrap();
    fs::set_permissions(&input, fs::Permissions::from_mode(0o700)).unwrap();
    let store = Store::open(&home.path().join("store")).unwrap();
    store
        .bootstrap(
            "Synthetic call owner".into(),
            "calls@example.test".into(),
            "Synthetic-calls-passphrase".into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login(
            "calls@example.test".into(),
            "Synthetic-calls-passphrase".into(),
        )
        .await
        .unwrap();
    let connection = store
        .create_source_connection(
            &login.user,
            CreateSourceConnection {
                name: "Synthetic selected calls".into(),
                kind: SourceConnectionKind::CustomHttp,
                scope: "synthetic-normalized-calls".into(),
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
    let token_file = home.path().join("ingest-token");
    write(&token_file, token.as_bytes(), true);
    let key = home.path().join("transcription-key");
    write(&key, b"SYNTHETIC_ONLY_KEY", true);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let config = moosuri_server::ServerConfig::new(&origin, true).unwrap();
    let runtime = config.start_runtime(store.clone());
    let batches = Arc::new(AtomicUsize::new(0));
    let lost = Arc::new(AtomicBool::new(false));
    let c = batches.clone();
    let lost_ack = lost.clone();
    let app = moosuri_server::router_with_runtime(store.clone(), config, runtime.clone()).layer(
        middleware::from_fn(move |request: Request<Body>, next: Next| {
            let c = c.clone();
            let lost = lost_ack.clone();
            async move {
                let batch = request.uri().path() == "/api/v1/source-ingest/batches";
                if batch {
                    c.fetch_add(1, Ordering::SeqCst);
                }
                let response = next.run(request).await;
                if batch && response.status().is_success() && lost.swap(false, Ordering::SeqCst) {
                    StatusCode::BAD_GATEWAY.into_response()
                } else {
                    response
                }
            }
        }),
    );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let audio_requests = Arc::new(AtomicUsize::new(0));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let audio_origin = format!("http://{}/", listener.local_addr().unwrap());
    let audio_app = Router::new()
        .route(
            "/v1/audio/transcriptions",
            post(
                |State(count): State<Arc<AtomicUsize>>, headers:HeaderMap, body: Bytes| async move {
                    assert!(body.len() > 1000);
                    let n = count.fetch_add(1, Ordering::SeqCst) + 1;
                    if headers.get("authorization").is_some_and(|h|h=="Bearer SYNTHETIC_REJECTED_KEY") {
                        StatusCode::UNAUTHORIZED.into_response()
                    } else if n == 2 {
                        StatusCode::BAD_GATEWAY.into_response()
                    } else {
                        axum::Json(json!({"text":format!("Synthetic call chunk {n}.")}))
                            .into_response()
                    }
                },
            ),
        )
        .layer(DefaultBodyLimit::max(5 * 1024 * 1024))
        .with_state(audio_requests.clone());
    let audio_server = tokio::spawn(async move { axum::serve(listener, audio_app).await.unwrap() });
    let mut options = Options {
        source: moosuri_connectors::work_sessions::Options {
            directory: input.clone(),
            origin,
            connection_id: connection.id.clone(),
            token_file,
            state_file: home.path().join("call-cursor.json"),
        },
        transcription_key_file: Some(key),
    };
    let selected_key = options.transcription_key_file.clone();
    options.transcription_key_file = Some(options.source.token_file.clone());
    assert!(matches!(
        run_loopback_fixture(
            &options,
            AudioClient::fixture(audio_origin.clone()).unwrap()
        )
        .await,
        Err("transcription_key_must_be_separate_outside_source")
    ));
    options.transcription_key_file = selected_key;
    let audio = wav();
    write(&input.join("unknown.wav"), &audio, true);
    let unknown = json!({"format_version":1,"kind":"moosuri_call_export","call_id":uuid::Uuid::new_v4(),"ended_at":100,"settled":true,"title":"Synthetic unknown chunks","audio":{"file":"unknown.wav","sha256":hash(&audio)},"transcript":null});
    write(
        &input.join("unknown.call.json"),
        unknown.to_string().as_bytes(),
        true,
    );
    let receipt = run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(receipt.transcription.as_ref().unwrap().attempted, 2);
    assert_eq!(receipt.transcription.unwrap().unknown, 1);
    assert!(
        store
            .sources(&login.user, connection.collection_id.clone(), None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        audio_requests.load(Ordering::SeqCst),
        2,
        "known prefix and unknown chunk never redispatch"
    );
    let path = input.join("chosen.call.json");
    let mut transcript = json!({"format_version":1,"kind":"moosuri_call_export","call_id":uuid::Uuid::new_v4(),"ended_at":100,"settled":true,"title":"Synthetic explicit transcript","audio":null,"transcript":{"segments":[{"speaker":"Synthetic Mina","text":"I prefer quiet meetings."},{"speaker":null,"text":"Unknown speaker reply."}]}});
    write(&path, transcript.to_string().as_bytes(), false);
    run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert!(
        store
            .sources(&login.user, connection.collection_id.clone(), None)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    write(&path, transcript.to_string().as_bytes(), true);
    lost.store(true, Ordering::SeqCst);
    let committed = run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(committed.accepted, 1);
    let sources = store
        .sources(&login.user, connection.collection_id.clone(), None)
        .await
        .unwrap();
    assert_eq!(sources.items.len(), 1);
    let id = sources.items[0].id.clone();
    let source = store.source(&login.user, id.clone(), None).await.unwrap();
    assert_eq!(
        source.source.speaker.as_deref(),
        Some(UNATTRIBUTED_CALL_SPEAKER)
    );
    assert!(
        source
            .text
            .contains(&hash(transcript.to_string().as_bytes()))
    );
    assert_eq!(
        source.provenance.as_ref().unwrap().connection_id,
        connection.id
    );
    let before = batches.load(Ordering::SeqCst);
    run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(batches.load(Ordering::SeqCst), before);
    assert_eq!(audio_requests.load(Ordering::SeqCst), 2);
    transcript["transcript"]["segments"][0]["text"] = json!("I now prefer morning meetings.");
    write(&path, transcript.to_string().as_bytes(), false);
    run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .source(&login.user, id.clone(), None)
            .await
            .unwrap()
            .source
            .revision,
        1
    );
    write(&path, transcript.to_string().as_bytes(), true);
    run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        store
            .source(&login.user, id, None)
            .await
            .unwrap()
            .source
            .revision,
        2
    );
    let mut complete_audio = audio;
    *complete_audio.last_mut().unwrap() = 1;
    write(&input.join("complete.wav"), &complete_audio, true);
    let complete = json!({"format_version":1,"kind":"moosuri_call_export","call_id":uuid::Uuid::new_v4(),"ended_at":100,"settled":true,"title":"Synthetic complete long audio","audio":{"file":"complete.wav","sha256":hash(&complete_audio)},"transcript":null});
    write(
        &input.join("complete.call.json"),
        complete.to_string().as_bytes(),
        true,
    );
    let receipt = run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(receipt.accepted, 1);
    assert_eq!(audio_requests.load(Ordering::SeqCst), 4);
    let all = store
        .sources(&login.user, connection.collection_id.clone(), None)
        .await
        .unwrap();
    assert_eq!(all.items.len(), 2);
    let complete = all
        .items
        .iter()
        .find(|s| s.title == "Synthetic complete long audio")
        .unwrap();
    let doc = store
        .source(&login.user, complete.id.clone(), None)
        .await
        .unwrap();
    assert!(
        doc.text.contains("Synthetic call chunk 3.")
            && doc.text.contains("Synthetic call chunk 4.")
    );
    assert!(doc.text.contains(&hash(&complete_audio)));
    assert!(doc.text.contains("\"chunk\":0") && doc.text.contains("\"chunk\":1"));
    assert_eq!(
        doc.source.speaker.as_deref(),
        Some(UNATTRIBUTED_CALL_SPEAKER)
    );
    run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(audio_requests.load(Ordering::SeqCst), 4);
    let state: Value =
        serde_json::from_slice(&fs::read(&options.source.state_file).unwrap()).unwrap();
    assert_eq!(state["producer"], "normalized-call-v1");
    assert_eq!(state["transcriptions"].as_object().unwrap().len(), 4);
    assert!(
        !state.to_string().contains("SYNTHETIC_ONLY_KEY") && !state.to_string().contains(&token)
    );
    // Keep the scan within its existing four-descriptor bound, while proving
    // that local absence does not remove the already ingested complete source.
    fs::remove_file(input.join("complete.call.json")).unwrap();
    let key_path = options.transcription_key_file.as_ref().unwrap();
    write(key_path, b"SYNTHETIC_REJECTED_KEY", true);
    let mut retry_audio = complete_audio;
    *retry_audio.last_mut().unwrap() = 2;
    write(&input.join("retry.wav"), &retry_audio, true);
    let retry = json!({"format_version":1,"kind":"moosuri_call_export","call_id":uuid::Uuid::new_v4(),"ended_at":100,"settled":true,"title":"Synthetic rejected then fixed","audio":{"file":"retry.wav","sha256":hash(&retry_audio)},"transcript":null});
    for name in ["retry-a.call.json", "retry-b.call.json"] {
        write(&input.join(name), retry.to_string().as_bytes(), true);
    }
    let before = audio_requests.load(Ordering::SeqCst);
    let rejected = run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        audio_requests.load(Ordering::SeqCst),
        before + 1,
        "two descriptors must not retry a newly rejected chunk in the same invocation"
    );
    assert_eq!(rejected.transcription.as_ref().unwrap().attempted, 1);
    assert!(rejected.transcription.as_ref().unwrap().retryable >= 1);
    assert_eq!(
        store
            .sources(&login.user, connection.collection_id.clone(), None)
            .await
            .unwrap()
            .items
            .len(),
        2,
        "a rejected partial call is not staged"
    );
    let state: Value =
        serde_json::from_slice(&fs::read(&options.source.state_file).unwrap()).unwrap();
    let known_rejection = state["transcriptions"]
        .as_object()
        .unwrap()
        .values()
        .find(|v| v["state"] == "retryable")
        .unwrap();
    let rejected_id = known_rejection["rejections"]["last"]["request_id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(known_rejection["rejections"]["count"], 1);
    assert_eq!(
        known_rejection["rejections"]["last"]["reason"],
        "credentials"
    );
    write(key_path, b"SYNTHETIC_ONLY_KEY", true);
    let recovered = run_loopback_fixture(
        &options,
        AudioClient::fixture(audio_origin.clone()).unwrap(),
    )
    .await
    .unwrap();
    assert_eq!(
        audio_requests.load(Ordering::SeqCst),
        before + 3,
        "later explicit invocation retries only the rejected chunk, then completes the remaining chunk"
    );
    assert_eq!(recovered.accepted, 1);
    assert_eq!(
        store
            .sources(&login.user, connection.collection_id.clone(), None)
            .await
            .unwrap()
            .items
            .len(),
        3
    );
    let state: Value =
        serde_json::from_slice(&fs::read(&options.source.state_file).unwrap()).unwrap();
    let complete = state["transcriptions"]
        .as_object()
        .unwrap()
        .values()
        .find(|v| v["state"] == "complete" && v["rejections"]["first"]["request_id"] == rejected_id)
        .unwrap();
    assert_eq!(complete["rejections"]["count"], 1);
    assert_eq!(complete["rejections"]["last"]["request_id"], rejected_id);
    assert_ne!(
        complete["request_id"], rejected_id,
        "rejection receipt remains attached to a new successful intent"
    );
    assert_eq!(
        state["transcriptions"]
            .as_object()
            .unwrap()
            .values()
            .filter(|v| v["state"] == "intent")
            .count(),
        1,
        "genuine unknown attempt stays permanently fenced after key repair"
    );
    let before = audio_requests.load(Ordering::SeqCst);
    run_loopback_fixture(&options, AudioClient::fixture(audio_origin).unwrap())
        .await
        .unwrap();
    assert_eq!(audio_requests.load(Ordering::SeqCst), before);
    runtime.shutdown().await;
    server.abort();
    audio_server.abort();
}
