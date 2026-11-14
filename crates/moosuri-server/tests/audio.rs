//! Structural/network acceptance uses generated nonspeech PCM, not speech-quality proof.
#![cfg(feature = "test-fixtures")]
use axum::{
    Json, Router,
    body::Body,
    extract::State,
    http::{Request, StatusCode},
    routing::post,
};
use http_body_util::BodyExt;
use moosuri_core::{ModelSelection, ProviderConfig, ProviderKind};
use moosuri_server::{ServerConfig, router};
use moosuri_store::{CredentialChange, LoginResult, ProviderChange, Store};
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tower::ServiceExt;
use uuid::Uuid;
struct Script {
    calls: AtomicUsize,
    mode: AtomicUsize,
    release: tokio::sync::Semaphore,
}
async fn provider(State(script): State<Arc<Script>>, request: Request<Body>) -> Json<Value> {
    assert_eq!(
        request.headers().get("authorization").unwrap(),
        "Bearer synthetic-audio-key"
    );
    assert!(
        request
            .headers()
            .get("content-type")
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("multipart/form-data; boundary=")
    );
    let bytes = request.into_body().collect().await.unwrap().to_bytes();
    assert!(bytes.windows(4).any(|x| x == b"RIFF"));
    let text = String::from_utf8_lossy(&bytes);
    assert!(text.contains("gpt-4o-mini-transcribe"));
    assert!(
        !text.contains("the blue bird"),
        "expected probe words must not be sent"
    );
    script.calls.fetch_add(1, Ordering::SeqCst);
    if script.mode.load(Ordering::SeqCst) == 1 {
        let p = script.release.acquire().await.unwrap();
        p.forget();
    }
    Json(json!({"text":"The blue bird sings beside the quiet river."}))
}
async fn text_provider() -> axum::response::Response {
    let first = json!({"id":"synthetic-text-audio","model":"text-fixture","choices":[{"index":0,"delta":{"role":"assistant","content":"Text remains available."},"finish_reason":null}]});
    let last = json!({"id":"synthetic-text-audio","model":"text-fixture","choices":[{"index":0,"delta":{},"finish_reason":"stop"}]});
    axum::response::Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(format!(
            "data: {first}\n\ndata: {last}\n\ndata: [DONE]\n\n"
        )))
        .unwrap()
}
fn wav() -> Vec<u8> {
    let samples = 1600;
    let mut b = Vec::new();
    b.extend(b"RIFF");
    b.extend((36u32 + samples * 2).to_le_bytes());
    b.extend(b"WAVEfmt ");
    b.extend(16u32.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(1u16.to_le_bytes());
    b.extend(16000u32.to_le_bytes());
    b.extend(32000u32.to_le_bytes());
    b.extend(2u16.to_le_bytes());
    b.extend(16u16.to_le_bytes());
    b.extend(b"data");
    b.extend((samples * 2).to_le_bytes());
    b.resize(44 + samples as usize * 2, 0);
    b
}
async fn send(
    app: &Router,
    login: &LoginResult,
    method: &str,
    path: &str,
    body: Vec<u8>,
    provider: Option<&str>,
) -> axum::response::Response {
    let mut r = Request::builder()
        .method(method)
        .uri(path)
        .header("host", "audio.example.test")
        .header("origin", "https://audio.example.test")
        .header("cookie", format!("moosuri_session={}", login.token))
        .header("X-Moosuri-Account", &login.user.account().id)
        .header(
            "content-type",
            if path.ends_with("/messages") {
                "application/json"
            } else {
                "application/octet-stream"
            },
        );
    if let Some(id) = provider {
        r = r
            .header("X-Audio-Provider", id)
            .header("X-Audio-Revision", "1");
    }
    app.clone()
        .oneshot(r.body(Body::from(body)).unwrap())
        .await
        .unwrap()
}
async fn value(r: axum::response::Response) -> Value {
    assert!(r.status().is_success(), "{}", r.status());
    serde_json::from_slice(&r.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn settled(app: &Router, login: &LoginResult, path: &str) -> Value {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let v = value(send(app, login, "GET", path, vec![], None).await).await;
            if v["state"] != "processing" {
                break v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap()
}
#[tokio::test]
async fn actual_http_probe_transcription_idempotency_invalid_predispatch_and_cancel() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let script = Arc::new(Script {
        calls: AtomicUsize::new(0),
        mode: AtomicUsize::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    let provider_app = Router::new()
        .route("/v1/audio/transcriptions", post(provider))
        .route("/v1/chat/completions", post(text_provider))
        .with_state(script.clone());
    let task = tokio::spawn(async move { axum::serve(listener, provider_app).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let password = "Synthetic-audio-password-42";
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login("owner@example.test".into(), password.into())
        .await
        .unwrap();
    let p = store
        .create_provider(
            &login.user,
            ProviderChange {
                name: "Synthetic Audio".into(),
                config: ProviderConfig {
                    kind: ProviderKind::OpenAi,
                    base_url: None,
                    allowed_ips: vec![],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Replace("synthetic-audio-key".to_owned().into()),
            },
        )
        .await
        .unwrap();
    let text_binding = store
        .create_provider(
            &login.user,
            ProviderChange {
                name: "Synthetic text".into(),
                config: ProviderConfig {
                    kind: ProviderKind::Compatible,
                    base_url: Some(format!("{}v1", base)),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &login.user,
            text_binding.id.clone(),
            1,
            "text-fixture".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &login.user,
            Some(ModelSelection {
                source: moosuri_core::ModelSource::Shared,
                provider_id: text_binding.id,
                provider_revision: 1,
                model: "text-fixture".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let app = router(
        store.clone(),
        ServerConfig::new("https://audio.example.test", false)
            .unwrap()
            .with_audio_fixture(base)
            .unwrap(),
    );
    let settings = value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/probe",
            wav(),
            Some(&p.id),
        )
        .await,
    )
    .await;
    assert_eq!(settings["available"], true);
    assert_eq!(script.calls.load(Ordering::SeqCst), 1);
    let id = Uuid::new_v4();
    let path = format!("/api/v1/audio/{id}");
    let mut bad = wav();
    bad[24] = 0;
    assert_eq!(
        send(&app, &login, "POST", &path, bad, None).await.status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(script.calls.load(Ordering::SeqCst), 1);
    let _ = value(send(&app, &login, "POST", &path, wav(), None).await).await;
    let ready = settled(&app, &login, &path).await;
    assert_eq!(ready["state"], "ready");
    assert_eq!(
        ready["transcript"],
        "The blue bird sings beside the quiet river."
    );
    let _ = value(send(&app, &login, "POST", &path, wav(), None).await).await;
    assert_eq!(script.calls.load(Ordering::SeqCst), 2);
    // Ordinary text-side storage remains independent while the provider is blocked.
    script.mode.store(1, Ordering::SeqCst);
    let path2 = format!("/api/v1/audio/{}", Uuid::new_v4());
    let _ = value(send(&app, &login, "POST", &path2, wav(), None).await).await;
    tokio::time::timeout(Duration::from_secs(2), async {
        while script.calls.load(Ordering::SeqCst) < 3 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let thread = store
        .create_thread(&login.user, "ordinary text".into())
        .await
        .unwrap();
    assert!(
        store
            .conversation(&login.user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns
            .is_empty()
    );
    // This explicit request is the first chat admission; a completed transcript alone created none.
    let prompt =
        json!({"request_id":Uuid::new_v4(),"text":"Ordinary text while speech is pending"});
    let submitted = send(
        &app,
        &login,
        "POST",
        &format!("/api/v1/threads/{}/messages", thread.id),
        prompt.to_string().into_bytes(),
        None,
    )
    .await;
    assert!(submitted.status().is_success());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let page = store
                .conversation(&login.user, thread.id.clone(), None)
                .await
                .unwrap();
            if let Some(turn) = page.turns.last()
                && !turn.run.state.active()
            {
                assert_eq!(turn.run.state, moosuri_core::RunState::Completed);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert_eq!(
        send(&app, &login, "DELETE", &path2, vec![], None)
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    script.release.add_permits(1);
    tokio::time::sleep(Duration::from_millis(300)).await;
    let cancelled = value(send(&app, &login, "GET", &path2, vec![], None).await).await;
    assert_eq!(cancelled["state"], "cancelled");
    assert!(cancelled["transcript"].is_null());
    assert_eq!(script.calls.load(Ordering::SeqCst), 3);
    task.abort();
}

#[tokio::test]
async fn settings_revoke_ready_processing_and_held_probes_irreversibly() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let script = Arc::new(Script {
        calls: AtomicUsize::new(0),
        mode: AtomicUsize::new(0),
        release: tokio::sync::Semaphore::new(0),
    });
    let fixture = Router::new()
        .route("/v1/audio/transcriptions", post(provider))
        .with_state(script.clone());
    let task = tokio::spawn(async move { axum::serve(listener, fixture).await.unwrap() });
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    let password = "Synthetic-audio-revocation-42";
    store
        .bootstrap(
            "owner".into(),
            "owner@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let login = store
        .login("owner@example.test".into(), password.into())
        .await
        .unwrap();
    let make = |name: &str| ProviderChange {
        name: name.into(),
        config: ProviderConfig {
            kind: ProviderKind::OpenAi,
            base_url: None,
            allowed_ips: vec![],
            router_providers: vec![],
            zero_data_retention: false,
        },
        enabled: true,
        credential: CredentialChange::Replace("synthetic-audio-key".to_owned().into()),
    };
    let p = store
        .create_provider(&login.user, make("first"))
        .await
        .unwrap();
    let other = store
        .create_provider(&login.user, make("replacement"))
        .await
        .unwrap();
    let app = router(
        store.clone(),
        ServerConfig::new("https://audio.example.test", false)
            .unwrap()
            .with_audio_fixture(base)
            .unwrap(),
    );
    value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/probe",
            wav(),
            Some(&p.id),
        )
        .await,
    )
    .await;
    let path = format!("/api/v1/audio/{}", Uuid::new_v4());
    value(send(&app, &login, "POST", &path, wav(), None).await).await;
    assert_eq!(settled(&app, &login, &path).await["state"], "ready");
    value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/settings",
            vec![],
            None,
        )
        .await,
    )
    .await;
    // Do not read the job while disabled; selecting the exact same binding must not revive it.
    value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/probe",
            wav(),
            Some(&p.id),
        )
        .await,
    )
    .await;
    let old = value(send(&app, &login, "GET", &path, vec![], None).await).await;
    assert!(
        old["transcript"].is_null(),
        "disabled job transcript revived after same-binding reverify"
    );
    assert_eq!(old["state"], "cancelled");
    // Hold one probe in the actual client/HTTP fixture, mutate authority, then release it.
    for replace in [false, true] {
        script.mode.store(1, Ordering::SeqCst);
        let before = script.calls.load(Ordering::SeqCst);
        let probe = send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/probe",
            wav(),
            Some(&p.id),
        );
        tokio::pin!(probe);
        tokio::select! {_=&mut probe=>panic!("probe must be held"),_ = tokio::time::timeout(Duration::from_secs(3),async{while script.calls.load(Ordering::SeqCst)==before{tokio::time::sleep(Duration::from_millis(5)).await;}})=>{}}
        script.mode.store(0, Ordering::SeqCst);
        if replace {
            value(
                send(
                    &app,
                    &login,
                    "POST",
                    "/api/v1/admin/audio/probe",
                    wav(),
                    Some(&other.id),
                )
                .await,
            )
            .await;
        } else {
            value(
                send(
                    &app,
                    &login,
                    "POST",
                    "/api/v1/admin/audio/settings",
                    vec![],
                    None,
                )
                .await,
            )
            .await;
        }
        script.release.add_permits(1);
        let response = probe.await;
        assert_eq!(
            response.status(),
            StatusCode::CONFLICT,
            "stale settings authority must not expire a still-valid administrator session"
        );
        let current =
            value(send(&app, &login, "GET", "/api/v1/audio/settings", vec![], None).await).await;
        if replace {
            assert_eq!(current["selection"]["provider_id"], other.id);
        } else {
            assert_eq!(current["available"], false);
        }
    }
    // A processing job retired by settings change cannot finish after same-binding reverify.
    value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/probe",
            wav(),
            Some(&p.id),
        )
        .await,
    )
    .await;
    script.mode.store(1, Ordering::SeqCst);
    let before = script.calls.load(Ordering::SeqCst);
    let processing = format!("/api/v1/audio/{}", Uuid::new_v4());
    value(send(&app, &login, "POST", &processing, wav(), None).await).await;
    tokio::time::timeout(Duration::from_secs(3), async {
        while script.calls.load(Ordering::SeqCst) == before {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/settings",
            vec![],
            None,
        )
        .await,
    )
    .await;
    script.mode.store(0, Ordering::SeqCst);
    value(
        send(
            &app,
            &login,
            "POST",
            "/api/v1/admin/audio/probe",
            wav(),
            Some(&p.id),
        )
        .await,
    )
    .await;
    script.release.add_permits(1);
    let receipt = settled(&app, &login, &processing).await;
    assert_eq!(receipt["state"], "cancelled");
    assert!(receipt["transcript"].is_null());
    task.abort();
}
