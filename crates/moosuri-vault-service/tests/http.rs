//! Actual Rust assistant adapter plus ciphertext HTTP routes, entirely synthetic.
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use moosuri_store::{Store, UserContext};
use moosuri_vault_service::{
    database::Database,
    http::{Adapter, Config, router},
};
use serde_json::{Value, json};
use tower::ServiceExt;
const ASSISTANT: &str = "https://assistant.example.test";
const VAULT: &str = "https://vault.example.test";
const KEY: &str = "0000000000000000000000000000000000000000000000000000000000000000"; // synthetic adapter fixture
fn request(
    path: &str,
    method: &str,
    origin: Option<&str>,
    cookie: Option<&str>,
    account: Option<&str>,
    body: String,
    content_type: &str,
) -> Request<Body> {
    let mut r = Request::builder()
        .uri(path)
        .method(method)
        .header("host", "vault.example.test");
    if let Some(o) = origin {
        r = r.header("origin", o)
    }
    if let Some(c) = cookie {
        r = r.header("cookie", c)
    }
    if let Some(a) = account {
        r = r.header("x-moosuri-vault-account", a)
    }
    r.header("content-type", content_type)
        .body(Body::from(body))
        .unwrap()
}
async fn bytes(r: axum::response::Response) -> Vec<u8> {
    to_bytes(r.into_body(), 4096).await.unwrap().to_vec()
}
async fn exchange(app: &Router, s: &Store, u: &UserContext) -> String {
    let handoff = s.vault_handoff(u, VAULT.into()).await.unwrap();
    let denied = app
        .clone()
        .oneshot(request(
            "/session",
            "POST",
            Some(VAULT),
            None,
            None,
            format!("handoff={}", handoff.token),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(denied.status(), StatusCode::UNAUTHORIZED);
    let r = app
        .clone()
        .oneshot(request(
            "/session",
            "POST",
            Some(ASSISTANT),
            None,
            None,
            format!("handoff={}", handoff.token),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SEE_OTHER);
    let cookie = r.headers()["set-cookie"].to_str().unwrap();
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("Secure")
            && cookie.contains("SameSite=Strict")
            && !cookie.contains("Domain=")
    );
    assert!(cookie.starts_with("__Host-moosuri_vault="));
    let cookie = cookie.split(';').next().unwrap().into();
    let replay = app
        .clone()
        .oneshot(request(
            "/session",
            "POST",
            Some(ASSISTANT),
            None,
            None,
            format!("handoff={}", handoff.token),
            "application/x-www-form-urlencoded",
        ))
        .await
        .unwrap();
    assert_eq!(replay.status(), StatusCode::UNAUTHORIZED);
    cookie
}
#[tokio::test]
async fn online_actual_session_cookie_origin_owner_and_unavailable_authority_fail_closed() {
    let tmp = tempfile::tempdir().unwrap();
    let store = Store::open(&tmp.path().join("assistant")).unwrap();
    let password = "Synthetic assistant fixture password";
    store
        .bootstrap(
            "Synthetic A".into(),
            "vault-a@example.test".into(),
            password.into(),
            false,
        )
        .await
        .unwrap();
    let a = store
        .login("vault-a@example.test".into(), password.into())
        .await
        .unwrap();
    let invite = store
        .create_user(&a.user, "Synthetic B".into(), "vault-b@example.test".into())
        .await
        .unwrap();
    let b = store
        .login("vault-b@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let b = store
        .change_password(&b.user, "Synthetic member fixture password".into())
        .await
        .unwrap();
    let internal = moosuri_server::vault::adapter_router(store.clone(), KEY).unwrap();
    let refused = internal
        .clone()
        .oneshot(
            Request::builder()
                .uri("/internal/v1/vault")
                .method("POST")
                .header("content-type", "application/json")
                .body(Body::from(
                    json!({"kind":"validate","grant":"a".repeat(64)}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(refused.status(), StatusCode::UNAUTHORIZED);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, internal).await.unwrap() });
    let adapter = Adapter::new(&format!("http://{address}/internal/v1/vault"), KEY.into()).unwrap();
    let app = router(
        Database::open(&tmp.path().join("vault")).unwrap(),
        adapter,
        Config::new(VAULT, ASSISTANT, false).unwrap(),
    );
    let shell = app
        .clone()
        .oneshot(request(
            "/",
            "GET",
            None,
            None,
            None,
            String::new(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(shell.headers()["cache-control"], "no-store");
    let policy = shell.headers()["content-security-policy"].to_str().unwrap();
    assert!(
        policy.contains("worker-src 'self'")
            && policy.contains("'wasm-unsafe-eval'")
            && !policy.contains("'unsafe-inline'")
    );
    let ac = exchange(&app, &store, &a.user).await;
    let bc = exchange(&app, &store, &b.user).await;
    let r = app
        .clone()
        .oneshot(request(
            "/session/current",
            "GET",
            None,
            Some(&ac),
            None,
            String::new(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    let current: Value = serde_json::from_slice(&bytes(r).await).unwrap();
    assert_eq!(current["owner"]["account"], a.user.account().id);
    assert_eq!(
        current["head"],
        json!({"header_sha256":"","revision":0,"envelope_epoch":0,"generation":0})
    );
    assert!(current.get("header").is_none());

    for expected in [None, Some(b.user.account().id.as_str())] {
        let r = app
            .clone()
            .oneshot(request(
                "/api/state",
                "GET",
                None,
                Some(&ac),
                expected,
                String::new(),
                "application/json",
            ))
            .await
            .unwrap();
        assert_eq!(r.status(), StatusCode::UNAUTHORIZED)
    }
    let r = app
        .clone()
        .oneshot(request(
            "/api/state",
            "GET",
            None,
            Some(&ac),
            Some(&a.user.account().id),
            String::new(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    assert_eq!(r.headers()["cache-control"], "no-store");
    let value: Value = serde_json::from_slice(&bytes(r).await).unwrap();
    assert_eq!(value["owner"]["account"], a.user.account().id);
    let canary = "SYNTHETIC MALFORMED PAYLOAD MUST NOT ECHO";
    let r = app
        .clone()
        .oneshot(request(
            "/api/operations",
            "POST",
            Some(VAULT),
            Some(&ac),
            Some(&a.user.account().id),
            json!({"kind":canary}).to_string(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::BAD_REQUEST);
    assert!(!String::from_utf8(bytes(r).await).unwrap().contains(canary));
    let r = app
        .clone()
        .oneshot(request(
            "/api/operations",
            "POST",
            Some(ASSISTANT),
            Some(&ac),
            Some(&a.user.account().id),
            "{}".into(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    store.logout(&a.user).await.unwrap();
    let r = app
        .clone()
        .oneshot(request(
            "/api/state",
            "GET",
            None,
            Some(&ac),
            Some(&a.user.account().id),
            String::new(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::UNAUTHORIZED);
    let r = app
        .clone()
        .oneshot(request(
            "/api/state",
            "GET",
            None,
            Some(&bc),
            Some(&b.user.account().id),
            String::new(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::OK);
    server.abort();
    let _ = server.await;
    let r = app
        .clone()
        .oneshot(request(
            "/api/state",
            "GET",
            None,
            Some(&bc),
            Some(&b.user.account().id),
            String::new(),
            "application/json",
        ))
        .await
        .unwrap();
    assert_eq!(r.status(), StatusCode::SERVICE_UNAVAILABLE);
}
