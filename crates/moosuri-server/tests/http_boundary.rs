use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use moosuri_server::{ServerConfig, router};
use moosuri_store::Store;
use serde_json::{Value, json};
use tower::ServiceExt;

static TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());
const PASSWORD: &str = "Synthetic-Account-Password-42";
const HOST: &str = "assistant.example.test";
const ORIGIN: &str = "https://assistant.example.test";

#[tokio::test]
async fn claude_settings_require_admin_and_keep_unconfigured_runtime_unavailable() {
    let _g = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let admin = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invited = store
        .create_user(
            &admin.user,
            "Member".into(),
            "claude-member@example.test".into(),
        )
        .await
        .unwrap();
    let member = store
        .login(
            "claude-member@example.test".into(),
            invited.temporary_password,
        )
        .await
        .unwrap();
    store
        .change_password(&member.user, "Synthetic-Claude-member-password".into())
        .await
        .unwrap();
    let binding = store
        .create_claude_binding(
            &admin.user,
            "Private Claude".into(),
            "a".repeat(64),
            "b".repeat(64),
        )
        .await
        .unwrap();
    let a = login(&app, "admin@example.test", PASSWORD).await;
    let b = login(
        &app,
        "claude-member@example.test",
        "Synthetic-Claude-member-password",
    )
    .await;
    let base = "/api/v1/settings/claude";
    assert_eq!(
        send(&app, "GET", base, None, json!({})).await.status(),
        StatusCode::UNAUTHORIZED
    );
    let listed = value(send(&app, "GET", base, Some(&a), json!({})).await).await;
    assert_eq!(listed["runtime_configured"], false);
    assert_eq!(listed["bindings"][0]["id"], binding.id);
    assert_eq!(
        send(&app, "GET", base, Some(&b), json!({})).await.status(),
        StatusCode::FORBIDDEN
    );
    for (method, suffix) in [
        ("POST", "/native"),
        ("POST", "/probe"),
        ("POST", "/vision-probe"),
        ("DELETE", ""),
    ] {
        let path = format!("{base}/{}{suffix}", binding.id);
        assert_eq!(
            send(
                &app,
                method,
                &path,
                Some(&b),
                json!({"revision":1,"model":"claude-fixture-v1"})
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    assert_eq!(
        send(
            &app,
            "POST",
            &format!("{base}/{}/probe", binding.id),
            Some(&a),
            json!({"revision":1,"model":"claude-fixture-v1"})
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("{base}/{}", binding.id),
            Some(&a),
            json!({"revision":1})
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert!(
        store
            .claude_launch_ref(&admin.user, binding.id, 1)
            .await
            .is_err()
    );
}

#[tokio::test]
async fn member_model_management_is_forbidden_but_chat_uses_the_installation_default() {
    let _g = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let admin = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(&admin.user, "Member".into(), "member@example.test".into())
        .await
        .unwrap();
    let member = store
        .login("member@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let member = store
        .change_password(&member.user, PASSWORD.into())
        .await
        .unwrap();
    let a = login(&app, "admin@example.test", PASSWORD).await;
    let b = login(&app, "member@example.test", PASSWORD).await;
    let provider = store
        .create_provider(
            &admin.user,
            moosuri_store::ProviderChange {
                name: "Synthetic shared provider".into(),
                config: moosuri_core::ProviderConfig {
                    kind: moosuri_core::ProviderKind::OpenAi,
                    base_url: None,
                    allowed_ips: vec![],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: moosuri_store::CredentialChange::Replace(
                    "synthetic-api-key".to_owned().into(),
                ),
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(
            &admin.user,
            provider.id.clone(),
            1,
            "fixture-model".into(),
            Some("high".into()),
        )
        .await
        .unwrap();
    let legacy_route = format!("/api/v1/admin/accounts/{}/model", member.user.account().id);
    let default_route = "/api/v1/admin/providers/default";
    let selection = json!({"provider_id":provider.id,"provider_revision":1,"model":"fixture-model","effort":"high"});
    for (method, path, body) in [
        ("POST", default_route, json!({"selection":selection})),
        (
            "PUT",
            "/api/v1/settings/model",
            json!({"selection":selection}),
        ),
        (
            "POST",
            "/api/v1/settings/chatgpt/local-login",
            json!({"label":"Synthetic"}),
        ),
        (
            "POST",
            "/api/v1/settings/chatgpt/handoffs",
            json!({"label":"Synthetic"}),
        ),
        ("GET", "/api/v1/settings/chatgpt", Value::Null),
        (
            "GET",
            "/api/v1/settings/compatible-providers/discover",
            Value::Null,
        ),
        (
            "POST",
            "/api/v1/settings/compatible-providers",
            json!({"name":"Synthetic","base_url":"https://model.invalid/v1","enabled":true,"credential":{"action":"clear"}}),
        ),
        (
            "POST",
            "/api/v1/settings/claude",
            json!({"label":"Synthetic"}),
        ),
    ] {
        assert_eq!(
            send(&app, method, path, Some(&b), body).await.status(),
            StatusCode::FORBIDDEN,
            "{method} {path}"
        );
    }
    for session in [&a, &b] {
        assert_eq!(
            send(
                &app,
                "PUT",
                &legacy_route,
                Some(session),
                json!({"selection":selection})
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    let saved = send(
        &app,
        "POST",
        default_route,
        Some(&a),
        json!({"selection":selection}),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(value(saved).await["default_selection"], selection);

    let model = send(&app, "GET", "/api/v1/settings/model", Some(&b), Value::Null).await;
    assert_eq!(model.status(), StatusCode::OK);
    let model = value(model).await;
    assert_eq!(model["effective"]["model"], "fixture-model");
    assert_eq!(model["effective"]["effort"], "high");
    assert_eq!(model["selection"], Value::Null);
    assert_eq!(model["choices"], json!([]));
    assert!(model.get("installation_default").is_none());
    assert!(!model.to_string().contains("synthetic-api-key"));

    let cleared = send(
        &app,
        "POST",
        default_route,
        Some(&a),
        json!({"selection":null}),
    )
    .await;
    assert_eq!(cleared.status(), StatusCode::OK);
    assert_eq!(value(cleared).await["default_selection"], Value::Null);
    let model = send(&app, "GET", "/api/v1/settings/model", Some(&b), Value::Null).await;
    assert_eq!(value(model).await["effective"], Value::Null);
}

async fn fixture(temporary: bool) -> (tempfile::TempDir, Store, Router) {
    let tmp = tempfile::Builder::new()
        .prefix("moosuri-http-")
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let path = tmp.path().canonicalize().unwrap().join("data");
    let store = Store::open(&path).unwrap();
    store
        .bootstrap(
            "Administrator".into(),
            "admin@example.test".into(),
            PASSWORD.into(),
            temporary,
        )
        .await
        .unwrap();
    let app = router(store.clone(), ServerConfig::new(ORIGIN, false).unwrap());
    (tmp, store, app)
}
async fn send(
    app: &Router,
    method: &str,
    path: &str,
    cookie: Option<&str>,
    body: Value,
) -> axum::response::Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json");
    if let Some(cookie) = cookie {
        builder = builder.header(header::COOKIE, cookie);
        // Ordinary fixture calls use the currently authenticated identity. The stale-tab
        // regression below deliberately sends a different expected identity directly.
        let probe = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/v1/auth/session")
                    .header(header::HOST, HOST)
                    .header(header::COOKIE, cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        if probe.status() == StatusCode::OK {
            let account = value(probe).await;
            builder = builder.header("x-moosuri-account", account["user"]["id"].as_str().unwrap());
        }
    }
    app.clone()
        .oneshot(builder.body(Body::from(body.to_string())).unwrap())
        .await
        .unwrap()
}
fn session_cookie(response: &axum::response::Response) -> String {
    response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned()
}
async fn value(response: axum::response::Response) -> Value {
    serde_json::from_slice(&response.into_body().collect().await.unwrap().to_bytes()).unwrap()
}
async fn login(app: &Router, email: &str, password: &str) -> String {
    let response = send(
        app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":email,"password":password}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(
        cookie.contains("HttpOnly")
            && cookie.contains("Secure")
            && cookie.contains("SameSite=Strict")
    );
    session_cookie(&response)
}

#[tokio::test]
async fn restricted_admin_cannot_use_personal_or_admin_routes_and_old_cookie_dies() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(true).await;
    let old = login(&app, "admin@example.test", PASSWORD).await;
    for path in ["/api/v1/threads", "/api/v1/admin/users"] {
        assert_eq!(
            send(&app, "GET", path, Some(&old), Value::Null)
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    let changed = send(
        &app,
        "POST",
        "/api/v1/auth/password",
        Some(&old),
        json!({"password":"Different-Synthetic-Password-55"}),
    )
    .await;
    assert_eq!(changed.status(), StatusCode::OK);
    let new = session_cookie(&changed);
    assert_ne!(old, new);
    assert_eq!(
        send(&app, "GET", "/api/v1/admin/users", Some(&new), Value::Null)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        send(&app, "GET", "/api/v1/auth/session", Some(&old), Value::Null)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/auth/login",
            None,
            json!({"email":"admin@example.test","password":PASSWORD})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn member_thread_is_not_visible_or_deletable_by_admin() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let admin = login(&app, "admin@example.test", PASSWORD).await;
    let created = send(
        &app,
        "POST",
        "/api/v1/admin/users",
        Some(&admin),
        json!({"name":"Member","email":"member@example.test"}),
    )
    .await;
    assert_eq!(created.status(), StatusCode::CREATED);
    let invite = value(created).await;
    let temporary = login(
        &app,
        "member@example.test",
        invite["temporary_password"].as_str().unwrap(),
    )
    .await;
    let changed = send(
        &app,
        "POST",
        "/api/v1/auth/password",
        Some(&temporary),
        json!({"password":PASSWORD}),
    )
    .await;
    let member = session_cookie(&changed);
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/admin/users",
            Some(&member),
            Value::Null
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let thread = value(
        send(
            &app,
            "POST",
            "/api/v1/threads",
            Some(&member),
            json!({"title":"Member private conversation"}),
        )
        .await,
    )
    .await;
    let id = thread["id"].as_str().unwrap();
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/threads/{id}"),
            Some(&admin),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert!(value(send(&app,"GET","/api/v1/threads",Some(&admin),Value::Null).await).await["threads"].as_array().unwrap().is_empty());
    assert_eq!(value(send(&app,"GET","/api/v1/threads",Some(&member),Value::Null).await).await["threads"].as_array().unwrap().len(),1);
    assert_eq!(
        send(
            &app,
            "DELETE",
            &format!("/api/v1/threads/{id}"),
            Some(&member),
            Value::Null
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
}

#[tokio::test]
async fn csrf_host_spoof_and_duplicate_session_cookie_are_rejected() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    for origin in [None, Some("https://attacker.example.test")] {
        let mut request = Request::builder()
            .method("POST")
            .uri("/api/v1/auth/logout")
            .header(header::HOST, HOST)
            .header(header::COOKIE, &cookie);
        if let Some(origin) = origin {
            request = request.header(header::ORIGIN, origin);
        }
        assert_eq!(
            app.clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
    }
    let spoof = Request::builder()
        .uri("/api/v1/auth/session")
        .header(header::HOST, "attacker.example.test")
        .header(header::COOKIE, &cookie)
        .body(Body::empty())
        .unwrap();
    assert_eq!(
        app.clone().oneshot(spoof).await.unwrap().status(),
        StatusCode::MISDIRECTED_REQUEST
    );
    let duplicate = format!("{cookie}; {cookie}");
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&duplicate),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let session = send(
        &app,
        "GET",
        "/api/v1/auth/session",
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert_eq!(session.status(), StatusCode::OK);
    assert!(!session.headers().contains_key(header::SET_COOKIE));
    assert_eq!(session.headers()[header::CACHE_CONTROL], "no-store");
    let logout = send(
        &app,
        "POST",
        "/api/v1/auth/logout",
        Some(&cookie),
        Value::Null,
    )
    .await;
    assert_eq!(logout.status(), StatusCode::NO_CONTENT);
    assert!(!logout.headers().contains_key(header::SET_COOKIE));
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn login_attempts_are_bounded_before_expensive_hashing() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    for _ in 0..20 {
        let _ = send(
            &app,
            "POST",
            "/api/v1/auth/login",
            None,
            json!({"email":"admin@example.test","password":"x"}),
        )
        .await;
    }
    let response = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":"admin@example.test","password":PASSWORD}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(response.headers()[header::RETRY_AFTER], "300");
    let other = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":"other@example.test","password":PASSWORD}),
    )
    .await;
    assert_eq!(other.status(), StatusCode::UNAUTHORIZED);
    let spoof = Request::builder()
        .method("POST")
        .uri("/api/v1/auth/login")
        .header(header::HOST, HOST)
        .header(header::ORIGIN, ORIGIN)
        .header(header::CONTENT_TYPE, "application/json")
        .header("x-real-ip", "203.0.113.2")
        .body(Body::from(
            json!({"email":"admin@example.test","password":PASSWORD}).to_string(),
        ))
        .unwrap();
    assert_eq!(
        app.clone().oneshot(spoof).await.unwrap().status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

#[test]
fn public_http_requires_https_and_local_http_requires_explicit_opt_in() {
    assert!(ServerConfig::new("http://example.test", true).is_err());
    assert!(ServerConfig::new("http://127.0.0.1:3210", false).is_err());
    assert!(ServerConfig::new("http://127.0.0.1:3210", true).is_ok());
    for origin in [
        "https://user:password@example.test",
        "https://example.test/path",
        "https://example.test?x=y",
        "file:///private/tmp",
    ] {
        assert!(ServerConfig::new(origin, false).is_err());
    }
}

#[tokio::test]
async fn only_configured_proxy_can_supply_a_single_client_ip() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, _app) = fixture(false).await;
    let config = ServerConfig::new(ORIGIN, false)
        .unwrap()
        .with_trusted_proxy("127.0.0.1".parse().unwrap(), "x-real-ip")
        .unwrap();
    let app = router(store, config);
    let request = |ip: &str, password: &str| {
        Request::builder()
            .method("POST")
            .uri("/api/v1/auth/login")
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .header("x-real-ip", ip)
            .extension(axum::extract::ConnectInfo(
                "127.0.0.1:5555".parse::<std::net::SocketAddr>().unwrap(),
            ))
            .body(Body::from(
                json!({"email":"admin@example.test","password":password}).to_string(),
            ))
            .unwrap()
    };
    for _ in 0..20 {
        assert_eq!(
            app.clone()
                .oneshot(request("203.0.113.1", "wrong-password"))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        app.clone()
            .oneshot(request("203.0.113.1", PASSWORD))
            .await
            .unwrap()
            .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        app.clone()
            .oneshot(request("203.0.113.2", PASSWORD))
            .await
            .unwrap()
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        app.clone()
            .oneshot(request("203.0.113.2, 203.0.113.3", PASSWORD))
            .await
            .unwrap()
            .status(),
        StatusCode::BAD_REQUEST
    );
    let mut untrusted = request("203.0.113.3", PASSWORD);
    untrusted
        .extensions_mut()
        .remove::<axum::extract::ConnectInfo<std::net::SocketAddr>>();
    assert_eq!(
        app.clone().oneshot(untrusted).await.unwrap().status(),
        StatusCode::BAD_REQUEST
    );
}

#[tokio::test]
async fn stale_tab_account_assertion_cannot_act_with_new_shared_cookie() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let admin = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(&admin.user, "Member".into(), "member@example.test".into())
        .await
        .unwrap();
    let member = store
        .login("member@example.test".into(), invite.temporary_password)
        .await
        .unwrap();
    let member = store
        .change_password(&member.user, PASSWORD.into())
        .await
        .unwrap();
    for (path, body) in [
        ("/api/v1/threads", json!({"title":"Wrong tab"})),
        ("/api/v1/auth/password", json!({"password":PASSWORD})),
        ("/api/v1/auth/logout", json!({})),
    ] {
        let request = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::HOST, HOST)
            .header(header::ORIGIN, ORIGIN)
            .header(header::CONTENT_TYPE, "application/json")
            .header(header::COOKIE, format!("moosuri_session={}", member.token))
            .header("x-moosuri-account", &admin.user.account().id)
            .body(Body::from(body.to_string()))
            .unwrap();
        let response = app.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
    }
    assert!(store.list_threads(&member.user).await.unwrap().is_empty());
    assert!(store.authenticate(member.token).await.is_ok());
}

#[path = "cases/subscriptions.rs"]
mod subscriptions;

#[path = "cases/provider_settings.rs"]
mod provider_settings;

#[path = "cases/chat.rs"]
mod chat;

#[path = "cases/knowledge.rs"]
mod knowledge;

#[path = "cases/knowledge_agent.rs"]
mod knowledge_agent;

#[path = "cases/memory_command.rs"]
mod memory_command;

#[path = "cases/mail.rs"]
mod mail;

#[path = "cases/source_connections.rs"]
mod source_connections;

#[path = "cases/google_setup.rs"]
mod google_setup_tests;

#[tokio::test]
async fn community_catalog_lifecycle_requires_current_account_and_does_not_admit_execution() {
    let _lock = TEST_LOCK.lock().await;
    let (_dir, store, app) = fixture(false).await;
    assert_eq!(
        send(&app, "GET", "/api/v1/community-skills", None, json!(null))
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let cookie = login(&app, "admin@example.test", PASSWORD).await;
    let cards = value(
        send(
            &app,
            "GET",
            "/api/v1/community-skills",
            Some(&cookie),
            json!(null),
        )
        .await,
    )
    .await;
    let digest = cards[0]["digest"].as_str().unwrap();
    let install = send(
        &app,
        "POST",
        "/api/v1/community-skills/source-briefing",
        Some(&cookie),
        json!({"action":"install","expected_revision":0,"expected_digest":digest}),
    )
    .await;
    assert_eq!(install.status(), StatusCode::OK);
    assert_eq!(value(install).await[0]["enabled"], true);
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/community-skills/source-briefing",
            Some(&cookie),
            json!({"action":"remove","expected_revision":0,"expected_digest":null})
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let stale = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/community-skills/source-briefing")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, &cookie)
                .header("x-moosuri-account", "different-account")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"action":"remove","expected_revision":1,"expected_digest":null})
                        .to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(stale.status(), StatusCode::UNAUTHORIZED);
    assert!(store.claim_agent_run().await.unwrap().is_none());
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/community-skills/source-briefing",
            Some(&cookie),
            json!({"action":"remove","expected_revision":1,"expected_digest":null})
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[path = "cases/github_skills.rs"]
mod github_skills_tests;

#[path = "cases/accounts.rs"]
mod account_management_tests;

#[tokio::test]
async fn chat_draft_scope_is_public_non_authenticating_and_bound_to_exact_server_session() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, store, app) = fixture(false).await;
    let signed_in = send(
        &app,
        "POST",
        "/api/v1/auth/login",
        None,
        json!({"email":"admin@example.test","password":PASSWORD}),
    )
    .await;
    assert_eq!(signed_in.status(), StatusCode::OK);
    let cookie = session_cookie(&signed_in);
    let initial = value(signed_in).await;
    let scope = initial["chat_draft_scope"].as_str().unwrap().to_owned();
    assert_eq!(scope.len(), 64);
    assert!(scope.bytes().all(|c| c.is_ascii_hexdigit()));
    assert!(!cookie.contains(&scope));
    use sha2::Digest;
    let bearer = cookie.split_once('=').unwrap().1;
    let bearer_digest = format!("{:x}", sha2::Sha256::digest(bearer.as_bytes()));
    assert_ne!(scope, bearer_digest);
    assert!(!initial.to_string().contains(bearer));
    assert!(!initial.to_string().contains(&bearer_digest));
    let first = value(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(first["chat_draft_scope"], scope);
    let second_cookie = login(&app, "admin@example.test", PASSWORD).await;
    let second = value(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&second_cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_ne!(second["chat_draft_scope"], scope);
    assert_eq!(second["user"]["id"], initial["user"]["id"]);
    // A public namespace is not a bearer, nor accepted through session headers.
    let forged = format!("moosuri_session={scope}");
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&forged),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let owner = store
        .login("admin@example.test".into(), PASSWORD.into())
        .await
        .unwrap();
    let invite = store
        .create_user(
            &owner.user,
            "Synthetic member".into(),
            "draft-member@example.test".into(),
        )
        .await
        .unwrap();
    let restricted_cookie = login(
        &app,
        "draft-member@example.test",
        &invite.temporary_password,
    )
    .await;
    let restricted = value(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&restricted_cookie),
            Value::Null,
        )
        .await,
    )
    .await;
    assert_eq!(restricted["password_change_required"], true);
    assert!(restricted.get("chat_draft_scope").is_none());
    let changed = send(
        &app,
        "POST",
        "/api/v1/auth/password",
        Some(&cookie),
        json!({"password":"Synthetic-new-draft-password-42"}),
    )
    .await;
    assert_eq!(changed.status(), StatusCode::OK);
    let changed_cookie = session_cookie(&changed);
    let rotated = value(changed).await;
    assert_ne!(rotated["chat_draft_scope"], scope);
    assert_ne!(rotated["chat_draft_scope"], second["chat_draft_scope"]);
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/auth/logout",
            Some(&changed_cookie),
            json!({})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&changed_cookie),
            Value::Null
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[tokio::test]
async fn incoming_share_origin_is_authenticated_bounded_private_and_revocation_aware() {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let old = login(&app, "admin@example.test", PASSWORD).await;
    let original =
        value(send(&app, "GET", "/api/v1/auth/session", Some(&old), json!({})).await).await;
    let scope = original["chat_draft_scope"].as_str().unwrap();
    let current = login(&app, "admin@example.test", PASSWORD).await;
    let current_view = value(
        send(
            &app,
            "GET",
            "/api/v1/auth/session",
            Some(&current),
            json!({}),
        )
        .await,
    )
    .await;
    let expected = current_view["chat_draft_scope"].as_str().unwrap();
    let mismatch = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/v1/shares/origin")
                .header(header::HOST, HOST)
                .header(header::ORIGIN, ORIGIN)
                .header(header::COOKIE, &current)
                .header("x-moosuri-account", "synthetic-other-account")
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(
                    json!({"origin_scope":scope,"expected_current_scope":expected}).to_string(),
                ))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(mismatch.status(), StatusCode::UNAUTHORIZED);
    let check = send(
        &app,
        "POST",
        "/api/v1/shares/origin",
        Some(&current),
        json!({"origin_scope":scope,"expected_current_scope":expected}),
    )
    .await;
    assert_eq!(check.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        check.headers().get(header::CACHE_CONTROL).unwrap(),
        "no-store"
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            None,
            json!({"origin_scope":scope,"expected_current_scope":expected})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    for input in [
        json!({"origin_scope":"x","expected_current_scope":expected}),
        json!({"origin_scope":scope,"expected_current_scope":expected,"account":"forged"}),
        json!({"origin_scope":"x".repeat(400),"expected_current_scope":expected}),
    ] {
        assert_eq!(
            send(&app, "POST", "/api/v1/shares/origin", Some(&current), input)
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    assert_eq!(
        send(
            &app,
            "GET",
            "/api/v1/shares/origin",
            Some(&current),
            json!({})
        )
        .await
        .status(),
        StatusCode::METHOD_NOT_ALLOWED
    );
    assert_eq!(
        send(&app, "POST", "/api/v1/auth/logout", Some(&old), json!({}))
            .await
            .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            Some(&current),
            json!({"origin_scope":scope,"expected_current_scope":expected})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let changed = send(
        &app,
        "POST",
        "/api/v1/auth/password",
        Some(&current),
        json!({"password":"Synthetic-new-password-42"}),
    )
    .await;
    assert_eq!(changed.status(), StatusCode::OK);
    let next = session_cookie(&changed);
    let next_view =
        value(send(&app, "GET", "/api/v1/auth/session", Some(&next), json!({})).await).await;
    let expected = next_view["chat_draft_scope"].as_str().unwrap();
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            Some(&next),
            json!({"origin_scope":scope,"expected_current_scope":expected})
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[tokio::test]
async fn incoming_share_origin_pins_actual_current_session_but_allows_unclaimed_same_owner_continuity()
 {
    let _guard = TEST_LOCK.lock().await;
    let (_tmp, _store, app) = fixture(false).await;
    let a = login(&app, "admin@example.test", PASSWORD).await;
    let a_view = value(send(&app, "GET", "/api/v1/auth/session", Some(&a), json!({})).await).await;
    let a_scope = a_view["chat_draft_scope"].as_str().unwrap();
    let b = login(&app, "admin@example.test", PASSWORD).await;
    let b_view = value(send(&app, "GET", "/api/v1/auth/session", Some(&b), json!({})).await).await;
    let b_scope = b_view["chat_draft_scope"].as_str().unwrap();
    assert_ne!(a_scope, b_scope);
    // A was already claimed. Cookie B arrives at final proof despite the last
    // GET returning A: account assertion alone must never approve this request.
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            Some(&b),
            json!({"origin_scope":a_scope,"expected_current_scope":a_scope})
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    // An unclaimed origin A explicitly adopted under current B remains valid.
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            Some(&b),
            json!({"origin_scope":a_scope,"expected_current_scope":b_scope})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            Some(&a),
            json!({"origin_scope":a_scope,"expected_current_scope":a_scope})
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        send(
            &app,
            "POST",
            "/api/v1/shares/origin",
            Some(&b),
            json!({"origin_scope":a_scope})
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    for expected in ["".to_string(), "x".repeat(64), "a".repeat(63)] {
        assert_eq!(
            send(
                &app,
                "POST",
                "/api/v1/shares/origin",
                Some(&b),
                json!({"origin_scope":a_scope,"expected_current_scope":expected})
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
}

#[path = "cases/catalog_runtime.rs"]
mod catalog_runtime_tests;

#[path = "cases/credential_broker.rs"]
mod credential_broker_tests;

#[path = "cases/thread_creation.rs"]
mod thread_creation;

#[path = "cases/provider_quota.rs"]
mod provider_quota;

#[path = "cases/gws_inbox.rs"]
mod gws_inbox;
