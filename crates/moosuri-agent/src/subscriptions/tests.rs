use super::*;
use axum::{
    Form, Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_SHA256, RsaKeyPair};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::atomic::{AtomicUsize, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};
const PASSWORD: &str = "Synthetic-personal-password-42";
const MODULUS: &str = "qz_j6Fsqd5usp23J5PcA5s5MYVXGUAO58aWgw8J4RRcEylrPcaR7xTet78qNrRlu5XBlgSGLRDMtoJKeuBz5yE6uALSivvwwyRkpdOHItrQbm84pB8IuPveGpUA_HG-6Tw_SHmJlAxKlRjIwm7EvbiRpvs7j6U07KxFK0R-BhPS-Q5-lUICqCaudBWNyJtOUUFGYoiBBUaWKeVnMNkVFTlpMJdfq4kC1WynkMi-CGm3rNybXHTh5-cybsSDb_Erwi5TDrhksFjU4SDGHbjoQt4mWQhlbdFFEjnTzCFvcK0aOv9fmcDys9GqgBVX6vG-1ITTS9BCNvVGoWGa5dAkLXQ";
struct Provider {
    tokens: AtomicUsize,
    refreshes: AtomicUsize,
    models: AtomicUsize,
    expires: AtomicUsize,
    delay: AtomicUsize,
    jwks_status: AtomicUsize,
    refresh_id: AtomicUsize,
    token_status: AtomicUsize,
    nonce: Mutex<String>,
    shared_documents: AtomicUsize,
}
struct Fixture {
    origin: String,
    state: Arc<Provider>,
    task: JoinHandle<()>,
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.task.abort()
    }
}
fn signed(nonce: &str) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let payload = json!({"iss":"https://auth.openai.com","sub":"synthetic-owner","aud":"issued-synthetic-client","iat":now,"exp":now+3600,"nonce":nonce});
    let h = URL_SAFE_NO_PAD.encode(br#"{"alg":"RS256","kid":"fixture"}"#);
    let p = URL_SAFE_NO_PAD.encode(payload.to_string());
    let input = format!("{h}.{p}");
    let k = RsaKeyPair::from_pkcs8(include_bytes!(
        "../../../moosuri-connectors/src/chatgpt_oauth/synthetic-test-key.pk8"
    ))
    .unwrap();
    let mut signature = vec![0; k.public().modulus_len()];
    k.sign(
        &RSA_PKCS1_SHA256,
        &ring::rand::SystemRandom::new(),
        input.as_bytes(),
        &mut signature,
    )
    .unwrap();
    format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature))
}
async fn fixture() -> Fixture {
    let state = Arc::new(Provider {
        tokens: AtomicUsize::new(0),
        refreshes: AtomicUsize::new(0),
        models: AtomicUsize::new(0),
        expires: AtomicUsize::new(3600),
        delay: AtomicUsize::new(0),
        jwks_status: AtomicUsize::new(200),
        refresh_id: AtomicUsize::new(0),
        token_status: AtomicUsize::new(200),
        nonce: Mutex::new(String::new()),
        shared_documents: AtomicUsize::new(0),
    });
    let app=Router::new()
 .route("/.well-known/openid-configuration",get(||async{Json(json!({"issuer":"https://auth.openai.com","authorization_endpoint":"https://auth.openai.com/api/accounts/authorize","token_endpoint":"https://auth.openai.com/api/accounts/oauth/token","revocation_endpoint":"https://auth.openai.com/api/accounts/oauth/revoke","jwks_uri":"https://auth.openai.com/.well-known/jwks.json","id_token_signing_alg_values_supported":["RS256"],"code_challenge_methods_supported":["S256"],"token_endpoint_auth_methods_supported":["none"]}))}))
 .route("/.well-known/jwks.json",get(|State(s):State<Arc<Provider>>|async move{let status=s.jwks_status.load(Ordering::SeqCst) as u16;if status!=200{return (axum::http::StatusCode::from_u16(status).unwrap(),Json(json!({"error":"temporary_fixture_failure"}))).into_response()}Json(json!({"keys":[{"kty":"RSA","kid":"fixture","alg":"RS256","use":"sig","key_ops":["verify"],"n":MODULUS,"e":"AQAB"}]})).into_response()}))
 .route("/api/accounts/oauth/token",post(|State(s):State<Arc<Provider>>,Form(f):Form<HashMap<String,String>>|async move{let n=s.tokens.fetch_add(1,Ordering::SeqCst);assert_eq!(f["client_id"],"issued-synthetic-client");assert_eq!(f["resource"],"https://api.openai.com/v1");if f["grant_type"]=="refresh_token"{s.refreshes.fetch_add(1,Ordering::SeqCst);tokio::time::sleep(Duration::from_millis(s.delay.load(Ordering::SeqCst) as u64)).await;let status=s.token_status.load(Ordering::SeqCst) as u16;if status!=200{return (axum::http::StatusCode::from_u16(status).unwrap(),Json(json!({"error":"temporary_fixture_failure"}))).into_response()}let mut body=json!({"access_token":format!("synthetic-access-{n}"),"refresh_token":format!("synthetic-refresh-{n}"),"token_type":"Bearer","expires_in":3600});if s.refresh_id.load(Ordering::SeqCst)==1{body["id_token"]=json!(signed(&s.nonce.lock().unwrap()));}Json(body).into_response()}else{*s.nonce.lock().unwrap()=f["code"].clone();Json(json!({"access_token":"synthetic-access-0","refresh_token":"synthetic-refresh-0","token_type":"Bearer","expires_in":s.expires.load(Ordering::SeqCst),"scope":"openid profile email offline_access resource.invoke chatgpt.tokens.use.direct","id_token":signed(&f["code"])})).into_response()}}))
 .route("/api/accounts/oauth/revoke",post(||async{axum::http::StatusCode::SERVICE_UNAVAILABLE}))
 .route("/v1/models",get(||async{Json(json!({"models":[{"slug":"fixture-model","display_name":"Fixture","visibility":"list"}]}))}))
 .route("/v1/chat/completions",post(|State(s):State<Arc<Provider>>,Json(body):Json<Value>|async move{
  assert!(body.to_string().contains("SYNTHETIC_PRIVATE_DOCUMENT_PSR2"));s.shared_documents.fetch_add(1,Ordering::SeqCst);
  let text=json!({"id":"shared","model":"fixture-model","choices":[{"index":0,"delta":{"role":"assistant","content":"Source read."},"finish_reason":null}]});
  let done=json!({"id":"shared","model":"fixture-model","choices":[{"index":0,"delta":{},"finish_reason":"stop"}],"usage":{"prompt_tokens":8,"completion_tokens":2}});
  ([(axum::http::header::CONTENT_TYPE,"text/event-stream")],format!("data: {text}\n\ndata: {done}\n\ndata: [DONE]\n\n")).into_response()
 }))
 .route("/v1/responses",post(|State(s):State<Arc<Provider>>,Json(body):Json<Value>|async move{
  s.models.fetch_add(1,Ordering::SeqCst);assert!(!body.to_string().contains("SYNTHETIC_PRIVATE_DOCUMENT_PSR2"));assert_eq!(body["model"],"fixture-model");assert_eq!(body["store"],false);assert_eq!(body["stream"],true);assert!(body.get("max_output_tokens").is_none());assert!(body.get("tools").is_none());for item in body["input"].as_array().unwrap(){assert_ne!(item["role"],"system")}
  let item=json!({"type":"message","id":"m","role":"assistant","status":"completed","content":[{"type":"output_text","text":"OK","annotations":[]}]});let events=[json!({"type":"response.created","response":{"id":"r","model":"fixture-model"}}),json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"m","role":"assistant","content":[]}}),json!({"type":"response.content_part.added","item_id":"m","output_index":0,"content_index":0,"part":{"type":"output_text","text":""}}),json!({"type":"response.output_text.delta","item_id":"m","output_index":0,"content_index":0,"delta":"OK"}),json!({"type":"response.output_text.done","item_id":"m","output_index":0,"content_index":0,"text":"OK"}),json!({"type":"response.content_part.done","item_id":"m","output_index":0,"content_index":0,"part":{"type":"output_text","text":"OK","annotations":[]}}),json!({"type":"response.output_item.done","output_index":0,"item":item}),json!({"type":"response.completed","response":{"id":"r","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":8,"output_tokens":2}}})];let text=events.iter().map(|e|format!("data: {e}\n\n")).collect::<String>();([(axum::http::header::CONTENT_TYPE,"text/event-stream")],text).into_response()
 })).with_state(state.clone());
    let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", l.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(l, app).await.unwrap() });
    Fixture {
        origin,
        state,
        task,
    }
}
fn service(store: Store, f: &Fixture) -> SubscriptionService {
    let (changes, _) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mut s = SubscriptionService::new(store, signals);
    s.fixture = Some((f.origin.clone(), f.origin.clone()));
    s
}
async fn login(s: &SubscriptionService, u: &UserContext) -> LocalLoginLaunch {
    let launch = s
        .local_login(u, None, "Personal fixture".into())
        .await
        .unwrap();
    let url = url::Url::parse(&launch.authorization_url).unwrap();
    let q = url.query_pairs().collect::<HashMap<_, _>>();
    let mut callback = url::Url::parse(&q["redirect_uri"]).unwrap();
    callback
        .query_pairs_mut()
        .append_pair("state", &q["state"])
        .append_pair("code", &q["nonce"])
        .append_pair("client_id", "issued-synthetic-client");
    let mut stream = tokio::net::TcpStream::connect(("127.0.0.1", callback.port().unwrap()))
        .await
        .unwrap();
    stream
        .write_all(
            format!(
                "GET {}?{} HTTP/1.1\r\nHost: 127.0.0.1:{}\r\nConnection: close\r\n\r\n",
                callback.path(),
                callback.query().unwrap(),
                callback.port().unwrap()
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes).await.unwrap();
    assert!(bytes.starts_with(b"HTTP/1.1 200"));
    assert_eq!(
        s.store
            .subscription_attempt(u, launch.attempt_id.clone())
            .await
            .unwrap(),
        "completed"
    );
    launch
}
async fn proof(
    s: &SubscriptionService,
    u: &UserContext,
    id: String,
    revision: i64,
) -> moosuri_core::ModelChoice {
    let b = s
        .store
        .subscription_ref(u, id, revision, "fixture-model".into(), None)
        .await
        .unwrap();
    let client = s
        .client(SubscriptionAuthority::User(u.clone()), b.clone())
        .await
        .unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let c = CancellationToken::new();
    client
        .stream_text(
            &moosuri_connectors::model::protocol::TextRequest {
                model: b.model.clone(),
                effort: None,
                max_output_tokens: 2048,
                messages: vec![moosuri_connectors::model::protocol::TextMessage {
                    role: moosuri_connectors::model::protocol::TextRole::User,
                    text: "Reply OK".into(),
                    phase: None,
                }],
            },
            &tx,
            &c,
        )
        .await
        .unwrap();
    let mut finished = false;
    while let Ok(e) = rx.try_recv() {
        if matches!(
            e,
            moosuri_connectors::model::protocol::ModelEvent::Finished {
                outcome: moosuri_connectors::model::protocol::FinishOutcome::Completed,
                ..
            }
        ) {
            finished = true
        }
    }
    assert!(finished);
    s.store.record_subscription_probe(u, b).await.unwrap()
}
#[tokio::test]
async fn signed_loopback_restart_real_plan_text_proof_selection_and_saved_chat() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let s = service(store.clone(), &f);
    let launch = login(&s, &owner.user).await;
    let token = owner.token.clone();
    drop(s);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let owner = store.authenticate(token).await.unwrap();
    let s = service(store.clone(), &f);
    let choice = proof(&s, &owner, launch.binding_id.clone(), launch.revision).await;
    assert_eq!(choice.source, moosuri_core::ModelSource::PersonalChatGpt);
    assert!(!choice.function_tools);
    let selection = moosuri_core::ModelSelection {
        source: choice.source,
        provider_id: choice.provider_id.clone(),
        provider_revision: choice.provider_revision,
        model: choice.model,
        effort: choice.effort,
    };
    assert!(
        store
            .set_default_model(&owner, Some(selection.clone()))
            .await
            .is_err()
    );
    store
        .set_model_preference(&owner, Some(selection))
        .await
        .unwrap();
    let thread = store
        .create_thread(&owner, "Private plan".into())
        .await
        .unwrap();
    store
        .submit_turn(
            &owner,
            thread.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Hello".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    assert!(claim.binding.subscription.is_some());
    crate::runner::execute(
        store.clone(),
        claim,
        s.signals.clone(),
        crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
        crate::web::Service::new(None),
        s.clone(),
    )
    .await;
    let page = store.conversation(&owner, thread.id, None).await.unwrap();
    assert_eq!(page.turns[0].run.state, RunState::Completed);
    assert_eq!(
        page.turns[0]
            .assistant
            .iter()
            .map(|v| v.text.as_str())
            .collect::<String>(),
        "OK"
    );
    assert_eq!(f.state.models.load(Ordering::SeqCst), 2);
    assert!(
        !s.disconnect(&owner, launch.binding_id, launch.revision)
            .await
            .unwrap()
    );
    let settings = store.model_settings(&owner).await.unwrap();
    assert!(settings.selection.is_some());
    assert!(settings.effective.is_none());
    assert!(store.resolve_model(&owner).await.is_err());
    for name in ["assistant.sqlite", "assistant.sqlite-wal"] {
        if let Ok(b) = std::fs::read(dir.path().join(name)) {
            assert!(!b.windows(16).any(|v| v == b"synthetic-access"));
            assert!(!b.windows(17).any(|v| v == b"synthetic-refresh"));
        }
    }
}
#[tokio::test]
async fn owners_fail_before_http_and_refresh_is_single_flight_and_revoke_wins() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    f.state.expires.store(1, Ordering::SeqCst);
    f.state.delay.store(150, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invited = store
        .create_user(&owner.user, "Other".into(), "other@example.test".into())
        .await
        .unwrap();
    let other = store
        .login("other@example.test".into(), invited.temporary_password)
        .await
        .unwrap();
    let other = store
        .change_password(&other.user, "Another-synthetic-password-99".into())
        .await
        .unwrap();
    let s = service(store.clone(), &f);
    let launch = login(&s, &owner.user).await;
    let before = f.state.tokens.load(Ordering::SeqCst);
    assert!(
        store
            .subscription_ref(
                &other.user,
                launch.binding_id.clone(),
                launch.revision,
                "fixture-model".into(),
                None
            )
            .await
            .is_err()
    );
    assert!(
        s.local_login(
            &other.user,
            Some((launch.binding_id.clone(), launch.revision)),
            "Guess".into()
        )
        .await
        .is_err()
    );
    assert_eq!(f.state.tokens.load(Ordering::SeqCst), before);
    let b = store
        .subscription_ref(
            &owner.user,
            launch.binding_id.clone(),
            launch.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    let (a, c) = tokio::join!(
        s.client(SubscriptionAuthority::User(owner.user.clone()), b.clone()),
        s.client(SubscriptionAuthority::User(owner.user.clone()), b.clone())
    );
    assert!(a.is_ok() && c.is_ok());
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
    let choice = proof(&s, &owner.user, launch.binding_id.clone(), launch.revision).await;
    assert!(!choice.function_tools);
    s.disconnect(&owner.user, launch.binding_id, launch.revision)
        .await
        .unwrap();
    assert!(
        s.client(SubscriptionAuthority::User(owner.user), b)
            .await
            .is_err()
    );
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn dispatched_rotation_loss_survives_restart_without_redeeming_old_refresh() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    f.state.expires.store(1, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let s = service(store.clone(), &f);
    let launch = login(&s, &owner.user).await;
    let b = store
        .subscription_ref(
            &owner.user,
            launch.binding_id.clone(),
            launch.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    let SubscriptionAccess::Refresh(lease, bundle) = store
        .subscription_access(SubscriptionAuthority::User(owner.user.clone()), b.clone())
        .await
        .unwrap()
    else {
        panic!("expected refresh")
    };
    store.dispatch_subscription_refresh(&lease).await.unwrap();
    let auth = ChatGptAuthClient::synthetic_fixture(&f.origin)
        .await
        .unwrap();
    let replacement = auth.refresh(&bundle).await.unwrap();
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
    // Provider rotated; process dies before local bundle CAS. The durable dispatched
    // marker must prevent even a valid-looking old renewable record being replayed.
    drop(replacement);
    drop(bundle);
    drop(lease);
    drop(auth);
    drop(s);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    let u = store.authenticate(owner.token).await.unwrap();
    let s = service(store.clone(), &f);
    assert!(
        s.client(SubscriptionAuthority::User(u.clone()), b)
            .await
            .is_err()
    );
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.subscriptions(&u).await.unwrap()[0].status,
        "needs_reauth"
    );
}

#[tokio::test]
async fn logout_and_explicit_disconnect_fence_late_refresh_commit() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for revoke in [false, true] {
        let f = fixture().await;
        f.state.expires.store(1, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Owner".into(),
                "owner@example.test".into(),
                PASSWORD.into(),
                false,
            )
            .await
            .unwrap();
        let owner = store
            .login("owner@example.test".into(), PASSWORD.into())
            .await
            .unwrap();
        let s = service(store.clone(), &f);
        let launch = login(&s, &owner.user).await;
        let b = store
            .subscription_ref(
                &owner.user,
                launch.binding_id.clone(),
                launch.revision,
                "fixture-model".into(),
                None,
            )
            .await
            .unwrap();
        let SubscriptionAccess::Refresh(lease, bundle) = store
            .subscription_access(SubscriptionAuthority::User(owner.user.clone()), b.clone())
            .await
            .unwrap()
        else {
            panic!("expected refresh")
        };
        store.dispatch_subscription_refresh(&lease).await.unwrap();
        let auth = ChatGptAuthClient::synthetic_fixture(&f.origin)
            .await
            .unwrap();
        let next = auth.refresh(&bundle).await.unwrap();
        if revoke {
            s.disconnect(&owner.user, launch.binding_id.clone(), launch.revision)
                .await
                .unwrap();
        } else {
            store.logout(&owner.user).await.unwrap();
        }
        assert!(
            store
                .complete_subscription_refresh(*lease, next)
                .await
                .is_err()
        );
        let current = store
            .login("owner@example.test".into(), PASSWORD.into())
            .await
            .unwrap();
        assert!(
            s.client(SubscriptionAuthority::User(current.user.clone()), b)
                .await
                .is_err()
        );
        assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.subscriptions(&current.user).await.unwrap()[0].status,
            if revoke {
                "disconnected"
            } else {
                "needs_reauth"
            }
        );
    }
}

#[tokio::test]
async fn successful_rotating_refresh_then_jwks_temporary_error_never_replays_old_token() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    for jwks_status in [429, 503] {
        let f = fixture().await;
        f.state.expires.store(1, Ordering::SeqCst);
        let dir = tempfile::tempdir().unwrap();
        let store = Store::open(dir.path()).unwrap();
        store
            .bootstrap(
                "Owner".into(),
                "owner@example.test".into(),
                PASSWORD.into(),
                false,
            )
            .await
            .unwrap();
        let owner = store
            .login("owner@example.test".into(), PASSWORD.into())
            .await
            .unwrap();
        let s = service(store.clone(), &f);
        let launch = login(&s, &owner.user).await;
        let b = store
            .subscription_ref(
                &owner.user,
                launch.binding_id,
                launch.revision,
                "fixture-model".into(),
                None,
            )
            .await
            .unwrap();
        f.state.refresh_id.store(1, Ordering::SeqCst);
        f.state.jwks_status.store(jwks_status, Ordering::SeqCst);
        assert!(
            s.client(SubscriptionAuthority::User(owner.user.clone()), b.clone())
                .await
                .is_err()
        );
        assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.subscriptions(&owner.user).await.unwrap()[0].status,
            "needs_reauth"
        );
        assert!(
            s.client(SubscriptionAuthority::User(owner.user.clone()), b.clone())
                .await
                .is_err()
        );
        assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
        drop(s);
        drop(store);
        let store = Store::open(dir.path()).unwrap();
        let u = store.authenticate(owner.token).await.unwrap();
        let s = service(store.clone(), &f);
        f.state.jwks_status.store(200, Ordering::SeqCst);
        assert!(
            s.client(SubscriptionAuthority::User(u.clone()), b)
                .await
                .is_err()
        );
        assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(
            store.subscriptions(&u).await.unwrap()[0].status,
            "needs_reauth"
        );
    }
}

#[tokio::test]
async fn token_endpoint_temporary_rejection_keeps_a_retryable_renewable_session() {
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    f.state.expires.store(1, Ordering::SeqCst);
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let s = service(store.clone(), &f);
    let launch = login(&s, &owner.user).await;
    let b = store
        .subscription_ref(
            &owner.user,
            launch.binding_id,
            launch.revision,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    f.state.token_status.store(503, Ordering::SeqCst);
    assert!(
        s.client(SubscriptionAuthority::User(owner.user.clone()), b.clone())
            .await
            .is_err()
    );
    assert_eq!(
        store.subscriptions(&owner.user).await.unwrap()[0].status,
        "active"
    );
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 1);
    f.state.token_status.store(200, Ordering::SeqCst);
    assert!(
        s.client(SubscriptionAuthority::User(owner.user), b)
            .await
            .is_ok()
    );
    assert_eq!(f.state.refreshes.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn queued_shared_document_history_blocks_personal_text_without_provider_or_raw_document_leak()
{
    let _guard = crate::tests::TEST_LOCK.lock().await;
    let f = fixture().await;
    let dir = tempfile::tempdir().unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Owner".into(),
            "owner@example.test".into(),
            PASSWORD.into(),
            false,
        )
        .await
        .unwrap();
    let owner = store
        .login("owner@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let shared = store
        .create_provider(
            &owner.user,
            moosuri_store::ProviderChange {
                name: "Shared fixture".into(),
                config: moosuri_core::ProviderConfig {
                    kind: moosuri_core::ProviderKind::Compatible,
                    base_url: Some(format!("{}/v1", f.origin)),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: moosuri_store::CredentialChange::Replace(
                    "synthetic-shared-key".to_owned().into(),
                ),
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &owner.user,
            shared.id.clone(),
            1,
            "fixture-model".into(),
            None,
        )
        .await
        .unwrap();
    store
        .set_default_model(
            &owner.user,
            Some(moosuri_core::ModelSelection {
                source: moosuri_core::ModelSource::Shared,
                provider_id: shared.id,
                provider_revision: 1,
                model: "fixture-model".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let marker = b"SYNTHETIC_PRIVATE_DOCUMENT_PSR2";
    let digest: [u8; 32] = ring::digest::digest(&ring::digest::SHA256, marker)
        .as_ref()
        .try_into()
        .unwrap();
    let file_id = uuid::Uuid::new_v4().to_string();
    let moosuri_store::media::UploadAdmission::Process(upload) = store
        .file_begin(&owner.user, file_id, digest, "private.txt".into())
        .await
        .unwrap()
    else {
        panic!("new upload")
    };
    let file = store
        .file_finish(
            upload,
            Some(moosuri_store::private_files::extract(marker.to_vec(), "text/plain").unwrap()),
        )
        .await
        .unwrap();
    let thread = store
        .create_thread(&owner.user, "Document conversation".into())
        .await
        .unwrap();
    let ancestor = store
        .submit_turn_with_media(
            &owner.user,
            thread.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Read this source".into(),
            None,
            vec![moosuri_core::MediaRef {
                id: file.id,
                revision: file.revision,
            }],
        )
        .await
        .unwrap();
    assert_eq!(ancestor.run.state, RunState::Queued);
    let s = service(store.clone(), &f);
    let launch = login(&s, &owner.user).await;
    let choice = proof(&s, &owner.user, launch.binding_id, launch.revision).await;
    let selection = moosuri_core::ModelSelection {
        source: choice.source,
        provider_id: choice.provider_id,
        provider_revision: choice.provider_revision,
        model: choice.model,
        effort: choice.effort,
    };
    store
        .set_model_preference(&owner.user, Some(selection.clone()))
        .await
        .unwrap();
    let requests = f.state.models.load(Ordering::SeqCst);
    // Queued ancestors are excluded, without disturbing their pinned shared source.
    assert!(matches!(
        store
            .submit_turn(
                &owner.user,
                thread.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Follow-up while queued".into()
            )
            .await,
        Err(StoreError::Conflict)
    ));
    let first = store.claim_agent_run().await.unwrap().unwrap();
    assert!(
        first
            .documents
            .iter()
            .any(|v| v.contains(std::str::from_utf8(marker).unwrap()))
    );
    crate::runner::execute(
        store.clone(),
        first,
        s.signals.clone(),
        crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
        crate::web::Service::new(None),
        s.clone(),
    )
    .await;
    assert_eq!(
        store
            .conversation(&owner.user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns[0]
            .run
            .state,
        RunState::Completed
    );
    // A now-completed prior document must also fail personal admission, before
    // provider IO or source-body materialization. This is the original PSR-2 leak.
    assert!(matches!(
        store
            .submit_turn(
                &owner.user,
                thread.id.clone(),
                uuid::Uuid::new_v4().to_string(),
                "Plain follow-up".into()
            )
            .await,
        Err(StoreError::Conflict)
    ));
    assert_eq!(f.state.models.load(Ordering::SeqCst), requests);
    assert_eq!(
        store
            .conversation(&owner.user, thread.id.clone(), None)
            .await
            .unwrap()
            .turns
            .len(),
        1
    );
    let settings = store.model_settings(&owner.user).await.unwrap();
    assert_eq!(
        settings.selection.unwrap().source,
        moosuri_core::ModelSource::PersonalChatGpt
    );
    assert_eq!(
        settings.effective.unwrap().source,
        moosuri_core::ModelSource::PersonalChatGpt
    );
    // Explicit shared selection continues to inherit and send supported documents.
    store.set_model_preference(&owner.user, None).await.unwrap();
    let following = store
        .submit_turn(
            &owner.user,
            thread.id.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Shared follow-up".into(),
        )
        .await
        .unwrap();
    let second = store.claim_agent_run().await.unwrap().unwrap();
    assert_eq!(second.lease.run_id(), following.run.id);
    assert!(
        second
            .documents
            .iter()
            .any(|v| v.contains(std::str::from_utf8(marker).unwrap()))
    );
    crate::runner::execute(
        store.clone(),
        second,
        s.signals.clone(),
        crate::mail::MailService::new(store.clone(), None, s.signals.clone()),
        crate::web::Service::new(None),
        s.clone(),
    )
    .await;
    assert_eq!(
        store
            .conversation(&owner.user, thread.id, None)
            .await
            .unwrap()
            .turns[1]
            .run
            .state,
        RunState::Completed
    );
    assert_eq!(f.state.shared_documents.load(Ordering::SeqCst), 2);
    assert_eq!(f.state.models.load(Ordering::SeqCst), requests);
}
