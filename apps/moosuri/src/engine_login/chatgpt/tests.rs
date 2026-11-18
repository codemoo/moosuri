use super::*;
use axum::{
    Form, Json, Router,
    extract::State,
    http::HeaderMap,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_connectors::{
    chatgpt_oauth::ChatGptAuthClient,
    model::protocol::{FinishOutcome, TextMessage, TextRole, Usage},
};
use ring::{
    rand::SystemRandom,
    signature::{RSA_PKCS1_SHA256, RsaKeyPair},
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::OpenOptions,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, sync::Notify, task::JoinHandle, time::sleep};

const EMAIL: &str = "engine-synthetic@example.com";
const PASSWORD: &str = "Synthetic-engine-password-590";
const CLIENT: &str = "synthetic-engine-client";
const MODEL: &str = "synthetic-engine-model";
const MODULUS: &str = "qz_j6Fsqd5usp23J5PcA5s5MYVXGUAO58aWgw8J4RRcEylrPcaR7xTet78qNrRlu5XBlgSGLRDMtoJKeuBz5yE6uALSivvwwyRkpdOHItrQbm84pB8IuPveGpUA_HG-6Tw_SHmJlAxKlRjIwm7EvbiRpvs7j6U07KxFK0R-BhPS-Q5-lUICqCaudBWNyJtOUUFGYoiBBUaWKeVnMNkVFTlpMJdfq4kC1WynkMi-CGm3rNybXHTh5-cybsSDb_Erwi5TDrhksFjU4SDGHbjoQt4mWQhlbdFFEjnTzCFvcK0aOv9fmcDys9GqgBVX6vG-1ITTS9BCNvVGoWGa5dAkLXQ";

#[derive(Default)]
struct Counters {
    refreshes: AtomicUsize,
    responses: AtomicUsize,
    refresh_entered: Notify,
}
struct Fixture {
    origin: String,
    counts: Arc<Counters>,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

fn signed(nonce: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let header = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"engine-synthetic-key"}"#);
    let claims = URL_SAFE_NO_PAD.encode(
        json!({
            "iss": "https://auth.openai.com", "sub": "synthetic-subject", "aud": CLIENT,
            "iat": now, "exp": now + 600, "nonce": nonce
        })
        .to_string(),
    );
    let input = format!("{header}.{claims}");
    let key = RsaKeyPair::from_pkcs8(include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../crates/moosuri-connectors/src/chatgpt_oauth/synthetic-test-key.pk8"
    )))
    .unwrap();
    let mut signature = vec![0; key.public().modulus_len()];
    key.sign(
        &RSA_PKCS1_SHA256,
        &SystemRandom::new(),
        input.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}

async fn token(
    State(counts): State<Arc<Counters>>,
    Form(form): Form<HashMap<String, String>>,
) -> Json<Value> {
    assert_eq!(form["client_id"], CLIENT);
    assert_eq!(form["resource"], "https://api.openai.com/v1");
    if form["grant_type"] == "refresh_token" {
        assert_eq!(form["refresh_token"], "synthetic-first-refresh");
        assert!(!form.contains_key("scope"));
        counts.refreshes.fetch_add(1, Ordering::SeqCst);
        counts.refresh_entered.notify_one();
        sleep(Duration::from_millis(150)).await;
        return Json(json!({
            "access_token": "synthetic-rotated-access",
            "refresh_token": "synthetic-rotated-refresh",
            "expires_in": 3600, "token_type": "Bearer"
        }));
    }
    assert_eq!(form["grant_type"], "authorization_code");
    Json(json!({
        "access_token": "synthetic-first-access",
        "refresh_token": "synthetic-first-refresh",
        "scope": "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct",
        "expires_in": 3600, "token_type": "Bearer", "id_token": signed(&form["code"])
    }))
}

async fn responses(
    State(counts): State<Arc<Counters>>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> ([(axum::http::header::HeaderName, &'static str); 1], String) {
    counts.responses.fetch_add(1, Ordering::SeqCst);
    assert_eq!(headers["authorization"], "Bearer synthetic-rotated-access");
    assert_eq!(body["model"], MODEL);
    assert_eq!(body["store"], false);
    assert_eq!(body["stream"], true);
    assert_eq!(body["input"][0]["role"], "developer");
    for omitted in ["tools", "max_output_tokens", "previous_response_id"] {
        assert!(body.get(omitted).is_none());
    }
    let item = json!({
        "type": "message", "id": "message", "role": "assistant", "status": "completed",
        "content": [{"type": "output_text", "text": "OK", "annotations": []}]
    });
    let events = [
        json!({"type":"response.created","response":{"id":"fixture","model":MODEL}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"message","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","item_id":"message","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","item_id":"message","output_index":0,"content_index":0,"delta":"OK"}),
        json!({"type":"response.output_text.done","item_id":"message","output_index":0,"content_index":0,"text":"OK"}),
        json!({"type":"response.content_part.done","item_id":"message","output_index":0,"content_index":0,"part":{"type":"output_text","text":"OK","annotations":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"fixture","model":MODEL,"status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}}),
    ];
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        events.iter().map(|e| format!("data: {e}\n\n")).collect(),
    )
}

async fn fixture() -> Fixture {
    let counts = Arc::new(Counters::default());
    let app = Router::new()
        .route(
            "/.well-known/openid-configuration",
            get(|| async {
                Json(json!({
                    "issuer": "https://auth.openai.com",
                    "authorization_endpoint": "https://auth.openai.com/api/accounts/authorize",
                    "token_endpoint": "https://auth.openai.com/api/accounts/oauth/token",
                    "revocation_endpoint": "https://auth.openai.com/api/accounts/oauth/revoke",
                    "jwks_uri": "https://auth.openai.com/.well-known/jwks.json",
                    "id_token_signing_alg_values_supported": ["RS256"],
                    "code_challenge_methods_supported": ["S256"],
                    "token_endpoint_auth_methods_supported": ["none"],
                }))
            }),
        )
        .route(
            "/.well-known/jwks.json",
            get(|| async {
                Json(json!({"keys": [{
                    "kty": "RSA", "kid": "engine-synthetic-key", "alg": "RS256",
                    "use": "sig", "key_ops": ["verify"], "n": MODULUS, "e": "AQAB"
                }]}))
            }),
        )
        .route("/api/accounts/oauth/token", post(token))
        .route("/v1/responses", post(responses))
        .with_state(counts.clone());
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Fixture {
        origin,
        counts,
        task,
    }
}

async fn setup(dir: &Path) -> (Enrollment, moosuri_core::AccountSummary) {
    let settings = Enrollment {
        store_dir: dir.join("store"),
        enrollment_email: EMAIL.into(),
        password_file: dir.join("password"),
    };
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&settings.password_file)
        .unwrap();
    writeln!(file, "{PASSWORD}").unwrap();
    let store = Store::open(&settings.store_dir).unwrap();
    let account = store
        .bootstrap("Engine owner".into(), EMAIL.into(), PASSWORD.into(), false)
        .await
        .unwrap();
    (settings, account)
}

async fn enroll(settings: &Enrollment, fixture: &Fixture) -> SubscriptionRef {
    let store = settings.open_store().unwrap();
    let session = settings.login(&store).await.unwrap();
    let ticket = store
        .begin_subscription_login(&session.user, None, "Synthetic engine".into())
        .await
        .unwrap();
    let auth = ChatGptAuthClient::synthetic_fixture(&fixture.origin)
        .await
        .unwrap();
    let (attempt, url) = auth.begin_registration(&ticket.host_id, 32101).unwrap();
    let url = url::Url::parse(url.for_system_browser()).unwrap();
    let query = url.query_pairs().collect::<HashMap<_, _>>();
    let mut callback = url::Url::parse(&query["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &query["state"])
        .append_pair("code", &query["nonce"])
        .append_pair("client_id", CLIENT);
    let pending = attempt.accept_callback(callback.as_str()).unwrap();
    store
        .save_subscription_registration(&ticket, pending.registration())
        .await
        .unwrap();
    let bundle = auth.exchange(pending).await.unwrap();
    store
        .complete_subscription_login(&ticket, bundle)
        .await
        .unwrap();
    let binding = store
        .subscription_ref(
            &session.user,
            ticket.binding.id,
            ticket.binding.revision,
            MODEL.into(),
            None,
        )
        .await
        .unwrap();
    store
        .record_subscription_probe(&session.user, binding.clone())
        .await
        .unwrap();
    store
        .expire_subscription_fixture(&session.user, binding.id.clone(), binding.revision)
        .await
        .unwrap();
    binding
}

fn request(model: &str) -> TextRequest {
    TextRequest {
        model: model.into(),
        effort: None,
        max_output_tokens: 32,
        messages: [
            (TextRole::System, "Reply briefly."),
            (TextRole::User, "Say OK."),
        ]
        .into_iter()
        .map(|(role, text)| TextMessage {
            role,
            text: text.into(),
            phase: None,
        })
        .collect(),
    }
}

async fn infer(adapter: &Adapter) {
    let (tx, mut rx) = mpsc::channel(32);
    adapter
        .stream(&request(MODEL), &tx, &CancellationToken::new())
        .await
        .unwrap();
    drop(tx);
    let (mut output, mut usage, mut finished) = (String::new(), None, false);
    while let Some(event) = rx.recv().await {
        match event {
            ModelEvent::TextDelta { text, .. } => output.push_str(&text),
            ModelEvent::UsageSnapshot(snapshot) => usage = Some(snapshot),
            ModelEvent::Finished {
                outcome: FinishOutcome::Completed,
                ..
            } => finished = true,
            _ => (),
        }
    }
    assert!(finished);
    assert_eq!(output, "OK");
    assert_eq!(
        usage,
        Some(Usage {
            input_tokens: Some(8),
            output_tokens: Some(2),
            ..Usage::default()
        })
    );
}

#[tokio::test]
async fn exact_owner_and_model_fence_and_concurrent_streams_use_one_rotating_refresh() {
    let work = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let paths = work.path().canonicalize().unwrap();
    let other = paths.join("other");
    fs::create_dir(&other).unwrap();
    let (settings, owner) = setup(&paths).await;
    let (other_settings, other_owner) = setup(&other).await;
    let fixture = fixture().await;
    let enrolled = enroll(&settings, &fixture).await;
    let config = |enrollment: &Enrollment, account: &str| Config {
        enrollment: enrollment.clone(),
        enrollment_account: Uuid::parse_str(account).unwrap(),
        binding: Uuid::parse_str(&enrolled.id).unwrap(),
        revision: enrolled.revision,
    };
    assert_eq!(
        Adapter::open(config(&other_settings, &owner.id), MODEL)
            .await
            .err()
            .as_deref(),
        Some("chatgpt_engine_owner_mismatch")
    );
    assert_eq!(
        Adapter::open(config(&other_settings, &other_owner.id), MODEL)
            .await
            .err()
            .as_deref(),
        Some("chatgpt_engine_model_unverified")
    );
    assert_eq!(
        Adapter::open(config(&settings, &owner.id), "unverified")
            .await
            .err()
            .as_deref(),
        Some("chatgpt_engine_model_unverified")
    );

    let mut adapter = Adapter::open(config(&settings, &owner.id), MODEL)
        .await
        .unwrap();
    adapter.fixture = Some(fixture.origin.clone());
    assert!(
        Adapter::open(config(&settings, &owner.id), MODEL)
            .await
            .is_err()
    );
    let (tx, _rx) = mpsc::channel(8);
    assert_eq!(
        adapter
            .stream(&request("unverified"), &tx, &CancellationToken::new())
            .await,
        Err("chatgpt_engine_request_invalid".into())
    );
    assert_eq!(fixture.counts.refreshes.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.counts.responses.load(Ordering::SeqCst), 0);

    tokio::join!(infer(&adapter), infer(&adapter));
    assert_eq!(fixture.counts.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.counts.responses.load(Ordering::SeqCst), 2);
    drop(adapter);

    let mut restarted = Adapter::open(config(&settings, &owner.id), MODEL)
        .await
        .unwrap();
    restarted.fixture = Some(fixture.origin.clone());
    infer(&restarted).await;
    assert_eq!(fixture.counts.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.counts.responses.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn cancellation_during_provider_refresh_persists_rotation_and_fences_inference() {
    let work = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (enrollment, owner) = setup(work.path()).await;
    let fixture = fixture().await;
    let enrolled = enroll(&enrollment, &fixture).await;
    let mut adapter = Adapter::open(
        Config {
            enrollment,
            enrollment_account: Uuid::parse_str(&owner.id).unwrap(),
            binding: Uuid::parse_str(&enrolled.id).unwrap(),
            revision: enrolled.revision,
        },
        MODEL,
    )
    .await
    .unwrap();
    adapter.fixture = Some(fixture.origin.clone());

    let cancel = CancellationToken::new();
    let (tx, _rx) = mpsc::channel(16);
    let (result, ()) = tokio::join!(
        async { adapter.stream(&request(MODEL), &tx, &cancel).await },
        async {
            tokio::time::timeout(
                Duration::from_secs(5),
                fixture.counts.refresh_entered.notified(),
            )
            .await
            .unwrap();
            cancel.cancel();
        }
    );
    assert_eq!(result, Err("chatgpt_engine_cancelled".into()));
    assert_eq!(fixture.counts.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.counts.responses.load(Ordering::SeqCst), 0);
    infer(&adapter).await;
    assert_eq!(fixture.counts.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(fixture.counts.responses.load(Ordering::SeqCst), 1);
}
