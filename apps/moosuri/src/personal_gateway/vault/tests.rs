//! Synthetic accounts, sockets and tokens only.
use super::*;
use moosuri_vault_protocol::Owner;
use std::sync::atomic::{AtomicUsize, Ordering};
use tower::ServiceExt;

const ORIGIN: &str = "https://vault.example.test";
const SESSION_TOKEN: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

#[derive(Default)]
struct Behaviour {
    handoff_status: u16,
    tenant: Option<Uuid>,
    digest: Option<String>,
    used: bool,
    handoff: Option<String>,
    grant: Option<String>,
    lifecycle: Option<Uuid>,
    ack_digest: Option<String>,
    ack_failure: bool,
}
struct Member {
    allocation: Allocation,
    count: Arc<AtomicUsize>,
    behaviour: Arc<Mutex<Behaviour>>,
    task: tokio::task::JoinHandle<()>,
}
fn directory(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir(&p).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    p
}
async fn member(options: &Options, email: &str) -> Member {
    let a = runtime_registry::reserve(&options.registry, Uuid::new_v4(), email.into()).unwrap();
    runtime_registry::set_routing(&options.registry, &a, true).unwrap();
    let root = directory(&options.ipc_root, &a.account_id);
    let path = root.join("personal.sock");
    let listener = tokio::net::UnixListener::bind(&path).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let count = Arc::new(AtomicUsize::new(0));
    let behaviour = Arc::new(Mutex::new(Behaviour {
        handoff_status: 200,
        ..Default::default()
    }));
    let (c, b, alloc) = (count.clone(), behaviour.clone(), a.clone());
    let app = Router::new().fallback(axum::routing::any(
        move |uri: Uri, h: HeaderMap, body: axum::body::Bytes| {
            let (c, b, a) = (c.clone(), b.clone(), alloc.clone());
            async move {
                assert_eq!(one(&h, "host"), Some("personal.internal"));
                assert_eq!(one(&h, "x-moosuri-account"), Some(a.account_id.as_str()));
                c.fetch_add(1, Ordering::SeqCst);
                match uri.path() {
                    "/api/v1/auth/session" => Json(serde_json::json!({"user":{"id":a.account_id,"email":"m@example.test","name":"M","role":"member","must_change_password":false},"password_change_required":false,"chat_draft_scope":"a".repeat(64)})).into_response(),
                    STATUS_PATH => Json(serde_json::json!({"vault_origin":ORIGIN,"account":a.account_id})).into_response(),
                    HANDOFF_PATH => {
                        let status = b.lock().unwrap().handoff_status;
                        if status != 200 {
                            return (StatusCode::from_u16(status).unwrap(), Json(serde_json::json!({"code":"forbidden"}))).into_response();
                        }
                        let exp = now().unwrap() as i64 + 30;
                        let token = b.lock().unwrap().handoff.clone().unwrap_or_else(|| "c".repeat(64));
                        Json(serde_json::json!({"token":token,"vault_origin":ORIGIN,"expires_at":exp})).into_response()
                    }
                    AUTHORITY_PATH => {
                        assert!(!h.contains_key(header::COOKIE));
                        let request: Request = serde_json::from_slice(&body).unwrap();
                        let mut s = b.lock().unwrap();
                        if matches!(request, Request::Exchange { .. }) {
                            if s.used {
                                return (StatusCode::UNAUTHORIZED, Json(serde_json::json!({"code":"unauthorized"}))).into_response();
                            }
                            s.used = true;
                        }
                        let tenant = s.tenant.unwrap_or_else(|| a.tenant_id.parse().unwrap());
                        let (grant, operation, digest) = match &request {
                            Request::Exchange { .. } => (Some(s.grant.clone().unwrap_or_else(|| "d".repeat(64))), None, None),
                            Request::Commit { operation, digest, .. } => (None, Some(*operation), Some(s.digest.clone().unwrap_or(digest.clone()))),
                            Request::LifecycleNext => {
                                if s.lifecycle.is_none() { return StatusCode::NOT_FOUND.into_response(); }
                                (None, s.lifecycle, Some("logout".into()))
                            },
                            Request::LifecycleAck { event } => {
                                if s.ack_failure { return StatusCode::SERVICE_UNAVAILABLE.into_response(); }
                                if s.lifecycle != Some(*event) { return StatusCode::CONFLICT.into_response(); }
                                let digest = s.ack_digest.clone().unwrap_or_else(|| "logout".into());
                                if digest == "logout" { s.lifecycle = None; }
                                (None, Some(*event), Some(digest))
                            },
                            _ => (None, None, None),
                        };
                        Json(Reply { owner: Owner { account: a.account_id.parse().unwrap(), tenant, installation: a.account_id.parse().unwrap(), epoch: 1 }, grant, operation, digest }).into_response()
                    }
                    _ => StatusCode::NOT_FOUND.into_response(),
                }
            }
        },
    ));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    Member {
        allocation: a,
        count,
        behaviour,
        task,
    }
}
fn enrolled(parent: &Path, account: Option<&str>) -> Options {
    let mut options = super::super::tests::options(parent);
    options.vault_site_origin = Some(ORIGIN.into());
    options.vault_member = account.map(|a| a.parse().unwrap());
    options.vault_adapter_listen = Some("127.0.0.1:41999".parse().unwrap());
    // Operator-provisioned, read-only; the gateway never creates this file.
    let key = directory(parent, "vault").join("adapter.key");
    fs::write(&key, format!("{}\n", "1f".repeat(32))).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o400)).unwrap();
    options.vault_adapter_key_file = Some(key);
    options
}
/// Genuine handoff through the public route, registering the member's handoff mapping.
async fn handoff_token(g: &Gateway, m: &Member) -> StatusCode {
    let post = browser_headers(g, &m.allocation, &Method::POST);
    call(router(g.clone()), Method::POST, HANDOFF_PATH, post, "{}")
        .await
        .0
}
fn exchange_body() -> String {
    format!(
        r#"{{"kind":"exchange","handoff":"{}","vault_origin":"{ORIGIN}"}}"#,
        "c".repeat(64)
    )
}
fn browser_headers(g: &Gateway, a: &Allocation, method: &Method) -> HeaderMap {
    let (alloc, epoch) = runtime_registry::session_route(
        &g.registry,
        runtime_registry::SessionDestination::Account(a.account_id.parse().unwrap()),
    )
    .unwrap();
    let route = Route {
        epoch,
        account: alloc.account_id.parse().unwrap(),
        tenant: alloc.tenant_id.parse().unwrap(),
        volume: alloc.volume_id.parse().unwrap(),
        generation: alloc.generation,
        expires: now().unwrap() + 3600,
        session_hash: hash(SESSION_TOKEN),
    };
    let mut h = HeaderMap::new();
    h.insert("host", "assistant.example.test".parse().unwrap());
    h.insert("x-real-ip", "192.0.2.9".parse().unwrap());
    h.insert("x-moosuri-account", a.account_id.parse().unwrap());
    h.insert(
        header::COOKIE,
        format!(
            "{SESSION}={SESSION_TOKEN}; {ROUTER}={}",
            g.sign(&route).unwrap()
        )
        .parse()
        .unwrap(),
    );
    if method == Method::POST {
        h.insert("origin", "https://assistant.example.test".parse().unwrap());
        h.insert("content-type", "application/json".parse().unwrap());
    } else {
        h.insert("sec-fetch-site", "same-origin".parse().unwrap());
    }
    h
}
async fn call(
    router: Router,
    method: Method,
    path: &str,
    h: HeaderMap,
    body: &str,
) -> (StatusCode, serde_json::Value) {
    let mut request = axum::http::Request::builder()
        .method(method)
        .uri(path)
        .body(Body::from(body.to_string()))
        .unwrap();
    *request.headers_mut() = h;
    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let bytes = axum::body::to_bytes(response.into_body(), 1 << 20)
        .await
        .unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(serde_json::Value::Null),
    )
}
fn adapter_headers(key: &str) -> HeaderMap {
    let mut h = HeaderMap::new();
    h.insert("content-type", "application/json".parse().unwrap());
    h.insert(KEY_HEADER, key.parse().unwrap());
    h
}
fn key_of(options: &Options) -> String {
    fs::read_to_string(options.vault_adapter_key_file.as_ref().unwrap())
        .unwrap()
        .trim_end()
        .to_owned()
}

#[tokio::test]
async fn enrolled_member_gets_ready_status_and_a_bound_short_lived_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut options = enrolled(&root, None);
    let m = member(&options, "a@example.test").await;
    options.vault_member = Some(m.allocation.account_id.parse().unwrap());
    let g = Gateway::new(&options).unwrap();
    let h = browser_headers(&g, &m.allocation, &Method::GET);
    let (status, body) = call(router(g.clone()), Method::GET, STATUS_PATH, h, "").await;
    assert_eq!(
        (status, body["state"].as_str(), body["site_origin"].as_str()),
        (StatusCode::OK, Some("ready"), Some(ORIGIN))
    );
    let post = browser_headers(&g, &m.allocation, &Method::POST);
    let (status, body) = call(
        router(g.clone()),
        Method::POST,
        HANDOFF_PATH,
        post.clone(),
        "{}",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(body["vault_origin"], ORIGIN);
    assert_eq!(body["token"].as_str().unwrap().len(), 64);
    // Missing session, foreign Origin and client-supplied identity fields are refused.
    let mut foreign = post.clone();
    foreign.insert("origin", "https://evil.example.test".parse().unwrap());
    assert_eq!(
        call(router(g.clone()), Method::POST, HANDOFF_PATH, foreign, "{}")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut bare = post.clone();
    bare.remove(header::COOKIE);
    assert_eq!(
        call(router(g.clone()), Method::POST, HANDOFF_PATH, bare, "{}")
            .await
            .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            router(g.clone()),
            Method::POST,
            HANDOFF_PATH,
            post,
            r#"{"origin":"https://evil.example.test"}"#
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    m.task.abort();
}

#[tokio::test]
async fn foreign_suspended_and_locked_members_receive_no_handoff() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut options = enrolled(&root, None);
    let enrolled_member = member(&options, "a@example.test").await;
    let foreign = member(&options, "b@example.test").await;
    options.vault_member = Some(enrolled_member.allocation.account_id.parse().unwrap());
    let g = Gateway::new(&options).unwrap();
    let h = browser_headers(&g, &foreign.allocation, &Method::GET);
    let (status, body) = call(router(g.clone()), Method::GET, STATUS_PATH, h, "").await;
    assert_eq!(
        (status, body["state"].as_str(), body.get("site_origin")),
        (StatusCode::OK, Some("setup_required"), None)
    );
    let before = foreign.count.load(Ordering::SeqCst);
    let post = browser_headers(&g, &foreign.allocation, &Method::POST);
    assert_eq!(
        call(router(g.clone()), Method::POST, HANDOFF_PATH, post, "{}")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(foreign.count.load(Ordering::SeqCst), before);
    // Member runtime lock/denial is surfaced, never replaced by a minted token.
    let post = browser_headers(&g, &enrolled_member.allocation, &Method::POST);
    enrolled_member.behaviour.lock().unwrap().handoff_status = 403;
    assert_eq!(
        call(
            router(g.clone()),
            Method::POST,
            HANDOFF_PATH,
            post.clone(),
            "{}"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    // Online fence: the same valid browser session stops immediately.
    runtime_registry::set_routing(&g.registry, &enrolled_member.allocation, false).unwrap();
    let (status, body) = call(router(g.clone()), Method::POST, HANDOFF_PATH, post, "{}").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert!(body.get("token").is_none());
    let (status, _) = call(
        adapter_router(g.clone()),
        Method::POST,
        "/internal/v1/vault",
        adapter_headers(&key_of(&options)),
        r#"{"kind":"lifecycle_next"}"#,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    enrolled_member.task.abort();
    foreign.task.abort();
}

#[tokio::test]
async fn adapter_is_not_on_the_public_router_and_needs_the_owner_only_key() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut options = enrolled(&root, None);
    let m = member(&options, "a@example.test").await;
    options.vault_member = Some(m.allocation.account_id.parse().unwrap());
    let g = Gateway::new(&options).unwrap();
    let key = key_of(&options);
    assert_eq!(key.len(), 64);
    assert_eq!(
        fs::metadata(options.vault_adapter_key_file.as_ref().unwrap())
            .unwrap()
            .mode()
            & 0o777,
        0o400
    );
    let body = serde_json::to_string(&Request::Validate {
        grant: "d".repeat(64),
    })
    .unwrap();
    let (status, _) = call(
        router(g.clone()),
        Method::POST,
        "/internal/v1/vault",
        adapter_headers(&key),
        &body,
    )
    .await;
    assert!(!status.is_success());
    for headers in [
        adapter_headers(&"0".repeat(64)),
        HeaderMap::new(),
        {
            let mut h = adapter_headers(&key);
            h.insert("origin", "https://assistant.example.test".parse().unwrap());
            h
        },
        {
            let mut h = adapter_headers(&key);
            h.insert(header::COOKIE, "moosuri_session=x".parse().unwrap());
            h
        },
    ] {
        let (status, _) = call(
            adapter_router(g.clone()),
            Method::POST,
            "/internal/v1/vault",
            headers,
            &body,
        )
        .await;
        assert_eq!(status, StatusCode::UNAUTHORIZED);
    }
    assert_eq!(m.count.load(Ordering::SeqCst), 0);
    // No registered mapping: denied before any member-runtime call.
    let (status, _) = call(
        adapter_router(g.clone()),
        Method::POST,
        "/internal/v1/vault",
        adapter_headers(&key),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    assert_eq!(m.count.load(Ordering::SeqCst), 0);
    assert_eq!(handoff_token(&g, &m).await, StatusCode::OK);
    let (status, _) = call(
        adapter_router(g.clone()),
        Method::POST,
        "/internal/v1/vault",
        adapter_headers(&key),
        &exchange_body(),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, _) = call(
        adapter_router(g.clone()),
        Method::POST,
        "/internal/v1/vault",
        adapter_headers(&key),
        &body,
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    // Foreign origin never reaches the member runtime.
    let foreign = r#"{"kind":"exchange","handoff":"cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc","vault_origin":"https://evil.example.test"}"#;
    let count = m.count.load(Ordering::SeqCst);
    assert_eq!(
        call(
            adapter_router(g),
            Method::POST,
            "/internal/v1/vault",
            adapter_headers(&key),
            foreign
        )
        .await
        .0,
        StatusCode::BAD_REQUEST
    );
    assert_eq!(m.count.load(Ordering::SeqCst), count);
    m.task.abort();
}

#[tokio::test]
async fn exchange_replay_and_receipt_mismatches_are_not_relayed() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut options = enrolled(&root, None);
    let m = member(&options, "a@example.test").await;
    options.vault_member = Some(m.allocation.account_id.parse().unwrap());
    let g = Gateway::new(&options).unwrap();
    let key = key_of(&options);
    let exchange = exchange_body();
    assert_eq!(handoff_token(&g, &m).await, StatusCode::OK);
    let send = |body: String| {
        call(
            adapter_router(g.clone()),
            Method::POST,
            "/internal/v1/vault",
            adapter_headers(&key),
            Box::leak(body.into_boxed_str()),
        )
    };
    let (status, body) = send(exchange.clone()).await;
    assert_eq!(
        (status, body["grant"].as_str().map(str::len)),
        (StatusCode::OK, Some(64))
    );
    assert_eq!(body["owner"]["account"], m.allocation.account_id);
    // The one-time handoff cannot be replayed.
    assert_eq!(send(exchange).await.0, StatusCode::UNAUTHORIZED);
    // A commit receipt must echo the exact operation and payload digest.
    let operation = Uuid::new_v4();
    let commit = format!(
        r#"{{"kind":"commit","grant":"{}","operation":"{operation}","digest":"{}"}}"#,
        "d".repeat(64),
        "a".repeat(64)
    );
    let (status, body) = send(commit.clone()).await;
    assert_eq!(
        (status, body["operation"].as_str().map(str::to_owned)),
        (StatusCode::OK, Some(operation.to_string()))
    );
    m.behaviour.lock().unwrap().digest = Some("b".repeat(64));
    assert_eq!(
        send(commit.clone()).await.0,
        StatusCode::SERVICE_UNAVAILABLE
    );
    // A reply for another tenant is rejected before it reaches the vault.
    {
        let mut s = m.behaviour.lock().unwrap();
        s.digest = None;
        s.tenant = Some(Uuid::new_v4());
    }
    assert_eq!(send(commit).await.0, StatusCode::SERVICE_UNAVAILABLE);
    m.task.abort();
}

#[tokio::test]
async fn suspension_then_resume_denies_old_grants_and_restart_denies_old_sessions() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut options = enrolled(&root, None);
    let m = member(&options, "a@example.test").await;
    options.vault_member = Some(m.allocation.account_id.parse().unwrap());
    let g = Gateway::new(&options).unwrap();
    let key = key_of(&options);
    let validate = format!(r#"{{"kind":"validate","grant":"{}"}}"#, "d".repeat(64));
    let send = |g: &Gateway, body: String| {
        call(
            adapter_router(g.clone()),
            Method::POST,
            "/internal/v1/vault",
            adapter_headers(&key),
            Box::leak(body.into_boxed_str()),
        )
    };
    assert_eq!(handoff_token(&g, &m).await, StatusCode::OK);
    assert_eq!(send(&g, exchange_body()).await.0, StatusCode::OK);
    assert_eq!(send(&g, validate.clone()).await.0, StatusCode::OK);
    // Suspension then reactivation: same volume/generation, new registry epoch.
    use moosuri_store::runtime_registry::admin as registry_admin;
    let admin = runtime_registry::reserve(&g.registry, Uuid::new_v4(), "admin@example.test".into())
        .unwrap();
    runtime_registry::set_routing(&g.registry, &admin, true).unwrap();
    let view = |a: &Allocation, email: &str| moosuri_core::SessionView {
        user: moosuri_core::AccountSummary {
            id: a.account_id.clone(),
            name: "Member".into(),
            email: email.into(),
            role: moosuri_core::Role::Member,
            must_change_password: false,
        },
        password_change_required: false,
        chat_draft_scope: None,
    };
    let admin_view = view(&admin, "admin@example.test");
    registry_admin::bootstrap(&g.registry, Uuid::new_v4(), &admin, &admin_view).unwrap();
    let member = registry_admin::observe_session(
        &g.registry,
        &m.allocation,
        &view(&m.allocation, "a@example.test"),
    )
    .unwrap();
    let admin_session = registry_admin::checked_session(&g.registry, &admin, &admin_view).unwrap();
    let id = m.allocation.account_id.parse().unwrap();
    let suspended = registry_admin::change_member(
        &g.registry,
        &admin_session,
        Uuid::new_v4(),
        id,
        1,
        member.role_revision,
        registry_admin::Change::Suspend,
    )
    .unwrap();
    assert_eq!(send(&g, validate.clone()).await.0, StatusCode::UNAUTHORIZED);
    registry_admin::change_member(
        &g.registry,
        &admin_session,
        Uuid::new_v4(),
        id,
        1,
        suspended.role_revision,
        registry_admin::Change::Resume,
    )
    .unwrap();
    let before = m.count.load(Ordering::SeqCst);
    assert_eq!(send(&g, validate.clone()).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(m.count.load(Ordering::SeqCst), before);
    // A fresh handoff after resume works; a restarted gateway forgets it.
    m.behaviour.lock().unwrap().used = false;
    assert_eq!(handoff_token(&g, &m).await, StatusCode::OK);
    assert_eq!(send(&g, exchange_body()).await.0, StatusCode::OK);
    assert_eq!(send(&g, validate.clone()).await.0, StatusCode::OK);
    let restarted = Gateway::new(&options).unwrap();
    let before = m.count.load(Ordering::SeqCst);
    assert_eq!(send(&restarted, validate).await.0, StatusCode::UNAUTHORIZED);
    assert_eq!(
        send(&restarted, exchange_body()).await.0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(m.count.load(Ordering::SeqCst), before);
    m.task.abort();
}

#[test]
fn provisioned_key_must_be_read_only_exact_hex_with_one_optional_newline() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let dir = directory(&root, "keys");
    let path = dir.join("k");
    let hex = "ab".repeat(32);
    for (text, mode, ok) in [
        (hex.clone(), 0o400, true),
        (format!("{hex}\n"), 0o400, true),
        (format!("{hex}\n\n"), 0o400, false),
        (format!("{hex}\r\n"), 0o400, false),
        (hex.clone(), 0o600, false),
        (hex.clone(), 0o440, false),
        ("zz".repeat(32), 0o400, false),
    ] {
        let _ = fs::remove_file(&path);
        fs::write(&path, &text).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(load_key(&path).is_ok(), ok);
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
    }
    // Never generated or created.
    let _ = fs::remove_file(&path);
    assert!(load_key(&path).is_err());
    assert!(!path.exists());
}

#[test]
fn enrollment_is_all_or_nothing_and_legacy_link_only_is_unchanged() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    let mut options = enrolled(&root, Some("0b7f0d57-5c3b-4a3c-a9b0-3d5a9d4f7b11"));
    assert!(Gateway::new(&options).unwrap().vault.is_some());
    options.vault_adapter_listen = Some("0.0.0.0:41999".parse().unwrap());
    assert_eq!(
        Gateway::new(&options).err().unwrap(),
        "vault_enrollment_invalid"
    );
    options.vault_adapter_listen = None;
    assert_eq!(
        Gateway::new(&options).err().unwrap(),
        "vault_enrollment_incomplete"
    );
    options.vault_member = None;
    options.vault_adapter_key_file = None;
    let g = Gateway::new(&options).unwrap();
    assert!(g.vault.is_none());
    assert_eq!(g.vault_site_origin.as_deref(), Some(ORIGIN));
}

#[path = "members_tests.rs"]
mod members_tests;
