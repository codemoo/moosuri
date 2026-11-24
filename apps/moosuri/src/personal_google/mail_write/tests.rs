//! Synthetic scenarios only: no network, no real account, no credentials.
use super::*;
use axum::{
    body::{Body, to_bytes},
    http::Request,
};
use moosuri_connectors::gmail::TokenSet;
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt};
use tower::ServiceExt;
use uuid::Uuid;

const REDIRECT: &str = "https://assistant.example.test/api/v1/mail/oauth/callback";

struct Env {
    _temp: tempfile::TempDir,
    store: Store,
    owner: VolumeOwner,
    token: String,
    app: Router,
}
fn oauth_client() -> OAuthClient {
    OAuthClient {
        client_id: "synthetic-app".into(),
        client_secret: SecretString::new("synthetic-secret".into()).unwrap(),
        redirect_uri: REDIRECT.into(),
    }
}
fn relay() -> GmailClient {
    GmailClient::google_gmail_proxy("127.0.0.1:9".parse().unwrap()).unwrap()
}
async fn env(app: App) -> Env {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("member");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    let owner = VolumeOwner {
        account: Uuid::new_v4(),
        tenant: Uuid::new_v4(),
        volume: Uuid::new_v4(),
        generation: 1,
    };
    owner
        .initialize(
            &root,
            "Synthetic member".into(),
            "a@example.test".into(),
            "Synthetic-member-password-42".into(),
        )
        .await
        .unwrap();
    let store = owner.open_verified(&root).await.unwrap();
    let login = store
        .login(
            "a@example.test".into(),
            "Synthetic-member-password-42".into(),
        )
        .await
        .unwrap();
    let token = store
        .change_password(&login.user, "Changed-member-password-42".into())
        .await
        .unwrap()
        .token;
    Env {
        _temp: temp,
        app: router(store.clone(), owner, app, relay()),
        store,
        owner,
        token,
    }
}
async fn send(
    e: &Env,
    token: &str,
    owner: VolumeOwner,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    send_to(&e.app, token, owner, method, path, body).await
}
async fn send_to(
    app: &Router,
    token: &str,
    owner: VolumeOwner,
    method: &str,
    path: &str,
    body: Value,
) -> (StatusCode, Value) {
    let reply = app
        .clone()
        .oneshot(
            Request::builder()
                .method(method)
                .uri(path)
                .header("host", "personal.internal")
                .header("origin", "https://personal.internal")
                .header("x-moosuri-account", owner.account.to_string())
                .header("cookie", format!("moosuri_session={token}"))
                .header("content-type", "application/json")
                .body(if method == "GET" {
                    Body::empty()
                } else {
                    Body::from(body.to_string())
                })
                .unwrap(),
        )
        .await
        .unwrap();
    let status = reply.status();
    let bytes = to_bytes(reply.into_body(), 1 << 20).await.unwrap();
    (
        status,
        serde_json::from_slice(&bytes).unwrap_or(Value::Null),
    )
}
async fn call(e: &Env, method: &str, action: &str, body: Value) -> (StatusCode, Value) {
    send(
        e,
        &e.token,
        e.owner,
        method,
        &format!("{BASE}/{action}"),
        body,
    )
    .await
}
/// What a completed standard compose consent leaves in the member store.
async fn grant(e: &Env, mailbox: &str) -> MailConnection {
    let user = e.store.authenticate(e.token.clone()).await.unwrap();
    let pin = GoogleAppPin {
        revision: None,
        client_id: "synthetic-app".into(),
        redirect_uri: REDIRECT.into(),
    };
    let start = e
        .store
        .start_mail_compose_oauth_with_app(&user, pin.clone())
        .await
        .unwrap();
    let handle = e
        .store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = e
        .store
        .claim_mail_oauth_with_app(&user, handle, pin)
        .await
        .unwrap();
    e.store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("SYNTHETIC-ACCESS".into()).unwrap(),
                refresh_token: Some(SecretString::new("SYNTHETIC-REFRESH".into()).unwrap()),
                expires_in: 3600,
                scopes: Some(COMPOSE_SCOPES.iter().map(|s| s.to_string()).collect()),
            },
            mailbox.into(),
        )
        .await
        .unwrap()
}
fn conn(email: &str, status: &str, compose: bool, native: bool) -> MailConnection {
    MailConnection {
        native_send: native,
        compose_enabled: compose,
        id: format!("c-{email}-{status}"),
        email: email.into(),
        epoch: 1,
        status: status.into(),
    }
}
fn reason(r: Reply) -> Reason {
    match r {
        Reply::NeedsConnection { reason, .. } => reason,
        other => panic!("expected needs_connection, got {other:?}"),
    }
}

#[test]
fn a_readonly_grant_alone_or_the_wrong_mailbox_never_yields_a_write_connection() {
    // Readonly-only member: no compose connection exists at all.
    assert_eq!(
        reason(choose(&[], Some("a@example.test"))),
        Reason::WriteConsentRequired
    );
    // A connection without compose (plain/send-only) is not a write grant.
    let plain = conn("a@example.test", "active", false, false);
    assert_eq!(
        reason(choose(&[plain], Some("a@example.test"))),
        Reason::WriteConsentRequired
    );
    // Same-looking grant on a different mailbox than the one that was read.
    let other = conn("b@example.test", "active", true, false);
    assert_eq!(
        reason(choose(&[other.clone()], Some("a@example.test"))),
        Reason::MailboxMismatch
    );
    // Revoked/needs-reconnect grants are inactive, never usable.
    let revoked = conn("a@example.test", "disconnected", true, false);
    assert_eq!(
        reason(choose(&[revoked.clone()], Some("a@example.test"))),
        Reason::ConnectionInactive
    );
    // native_send connections are dispatched elsewhere and are ignored here.
    let native = conn("a@example.test", "active", true, true);
    assert_eq!(
        reason(choose(&[native], Some("a@example.test"))),
        Reason::WriteConsentRequired
    );
    // The right grant is picked even behind unusable ones, case-insensitively.
    let good = conn("A@Example.test", "active", true, false);
    match choose(&[other, revoked, good.clone()], Some("a@example.test")) {
        Reply::Connected {
            connection_id,
            mailbox,
            ..
        } => {
            assert_eq!((connection_id, mailbox), (good.id, good.email));
        }
        other => panic!("{other:?}"),
    }
}

#[tokio::test]
async fn the_member_opts_in_with_an_exact_compose_url_and_only_their_own_session_can_start_it() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    // No Google app configured in this container: actionable, not an error page.
    let bare = env(App::Missing).await;
    let (status, v) = call(&bare, "GET", "status", json!({})).await;
    assert_eq!(
        (status, &v["state"], &v["reason"], &v["next"]),
        (
            StatusCode::OK,
            &json!("needs_connection"),
            &json!("google_setup_required"),
            &json!("configure_google_app")
        )
    );
    let (_, v) = call(&bare, "POST", "start", json!({})).await;
    assert_eq!(v["reason"], "google_setup_required");

    let e = env(App::Ready(Arc::new(oauth_client()))).await;
    let (_, v) = call(&e, "GET", "status", json!({})).await;
    assert_eq!(
        (&v["reason"], &v["next"]),
        (
            &json!("write_consent_required"),
            &json!("start_write_consent")
        )
    );
    // Another login, or the same cookie against another member's volume, gets nothing.
    let (status, _) = send(
        &e,
        "other-session",
        e.owner,
        "POST",
        &format!("{BASE}/start"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let foreign = VolumeOwner {
        account: Uuid::new_v4(),
        ..e.owner
    };
    let (status, _) = send(
        &e,
        &e.token,
        foreign,
        "POST",
        &format!("{BASE}/start"),
        json!({}),
    )
    .await;
    assert!(!status.is_success());
    // A callback code for an unknown state cannot become a handle.
    let (status, _) = send(
        &e,
        &e.token,
        e.owner,
        "POST",
        CAPTURE,
        json!({"state":"0".repeat(64),"code":"synthetic-code"}),
    )
    .await;
    assert!(!status.is_success());
    // A handle nobody captured cannot be completed, even by the right session.
    let (status, _) = call(&e, "POST", "complete", json!({"handle":"1".repeat(64)})).await;
    assert!(!status.is_success());

    let (_, before_read) = call(&e, "POST", "start", json!({})).await;
    assert_eq!(before_read["state"], "needs_connection");
    let gmail = super::super::routes_with_test_transport(
        e.store.clone(),
        e.owner,
        super::super::tests::transport_for(
            Arc::new(super::super::tests::Fake::default()),
            Uuid::new_v4(),
            "a@example.test".into(),
            REDIRECT.into(),
        ),
    );
    let (_, read_start) = send_to(
        &gmail,
        &e.token,
        e.owner,
        "POST",
        "/api/v1/personal/google/start",
        json!({}),
    )
    .await;
    let read_url = url::Url::parse(read_start["authorization_url"].as_str().unwrap()).unwrap();
    let state = read_url
        .query_pairs()
        .find(|(key, _)| key == "state")
        .unwrap()
        .1
        .into_owned();
    let (_, captured) = send_to(
        &gmail,
        &e.token,
        e.owner,
        "POST",
        "/internal/google/capture",
        json!({"state":state,"code":"synthetic-read"}),
    )
    .await;
    let (status, _) = send_to(
        &gmail,
        &e.token,
        e.owner,
        "POST",
        "/api/v1/personal/google/complete",
        json!({"handle":captured["handle"]}),
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    let (status, started) = call(&e, "POST", "start", json!({})).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(started["state"], "authorization");
    let url = url::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
    let scope = url
        .query_pairs()
        .find(|(k, _)| k == "scope")
        .unwrap()
        .1
        .into_owned();
    assert_eq!(scope, COMPOSE_SCOPES.join(" "));
    assert!(
        url.query_pairs()
            .any(|(k, v)| k == "include_granted_scopes" && v == "false")
    );
    let path = format!("{BASE}/start");
    let bytes = serde_json::to_vec(&started).unwrap();
    assert!(
        public_response(&path, &Method::POST, StatusCode::OK, &bytes)
            .unwrap()
            .is_ok()
    );
    // A gateway must refuse a widened scope, a foreign host, or a start that claims "connected".
    let mut widened = started.clone();
    let wide = started["authorization_url"]
        .as_str()
        .unwrap()
        .replace("gmail.compose", "gmail.modify");
    widened["authorization_url"] = json!(wide);
    assert!(
        public_response(
            &path,
            &Method::POST,
            StatusCode::OK,
            &serde_json::to_vec(&widened).unwrap()
        )
        .unwrap()
        .is_err()
    );
    let evil = started["authorization_url"]
        .as_str()
        .unwrap()
        .replace("accounts.google.com", "accounts.example.test");
    let mut moved = started.clone();
    moved["authorization_url"] = json!(evil);
    assert!(
        public_response(
            &path,
            &Method::POST,
            StatusCode::OK,
            &serde_json::to_vec(&moved).unwrap()
        )
        .unwrap()
        .is_err()
    );
    let fake =
        json!({"state":"connected","connection_id":"c1","epoch":1,"mailbox":"a@example.test"});
    assert!(
        public_response(
            &path,
            &Method::POST,
            StatusCode::OK,
            &serde_json::to_vec(&fake).unwrap()
        )
        .unwrap()
        .is_err()
    );
    // Browser DTOs can never carry credential fields.
    let leak = json!({"state":"connected","connection_id":"c1","epoch":1,"mailbox":"a@example.test","access_token":"X"});
    assert!(
        public_response(
            &format!("{BASE}/status"),
            &Method::GET,
            StatusCode::OK,
            &serde_json::to_vec(&leak).unwrap()
        )
        .unwrap()
        .is_err()
    );
    assert!(
        public_response(
            "/api/v1/personal/google/inbox",
            &Method::POST,
            StatusCode::OK,
            &bytes
        )
        .is_none()
    );
}

#[tokio::test]
async fn consent_is_visible_as_connected_and_revoking_it_returns_to_needs_connection() {
    let _fixture = crate::ACCOUNT_FIXTURE_LOCK.lock().await;
    let e = env(App::Ready(Arc::new(oauth_client()))).await;
    let connection = grant(&e, "a@example.test").await;
    let (_, v) = call(&e, "GET", "status", json!({})).await;
    assert_eq!(v["state"], "connected");
    assert_eq!(v["connection_id"], connection.id);
    assert!(v.get("access_token").is_none() && v.get("refresh_token").is_none());
    // Stale epoch or someone else's id cannot revoke by guessing.
    let (status, _) = call(
        &e,
        "POST",
        "disconnect",
        json!({"connection_id":connection.id,"epoch":connection.epoch + 1}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &e,
        "POST",
        "disconnect",
        json!({"connection_id":"nope","epoch":connection.epoch}),
    )
    .await;
    assert_eq!(status, StatusCode::CONFLICT);
    let (status, _) = call(
        &e,
        "POST",
        "disconnect",
        json!({"connection_id":connection.id,"epoch":connection.epoch}),
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT);
    let (_, v) = call(&e, "GET", "status", json!({})).await;
    assert_eq!(v["state"], "needs_connection");
    assert!(matches!(
        v["reason"].as_str(),
        Some("connection_inactive" | "write_consent_required")
    ));
    // A login that was revoked after consent cannot read the state either.
    let (status, _) = send(
        &e,
        "revoked-session",
        e.owner,
        "GET",
        &format!("{BASE}/status"),
        json!({}),
    )
    .await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
}

#[test]
fn the_send_fence_fails_closed_for_wrong_member_source_app_or_missing_approval() {
    let ok = || Facts {
        owner_account: "member-a",
        dispatch_account: "member-a",
        has_compose: true,
        app_matches: true,
        read_mailbox: Some("a@example.test"),
        binding_mailbox: "A@example.test",
        approved: true,
    };
    assert!(ok().allow());
    // A queued write from another member's account is never sent from this container.
    assert!(
        !Facts {
            dispatch_account: "member-b",
            ..ok()
        }
        .allow()
    );
    // Readonly-grade binding (no compose scope) or a different Google app pin.
    assert!(
        !Facts {
            has_compose: false,
            ..ok()
        }
        .allow()
    );
    assert!(
        !Facts {
            app_matches: false,
            ..ok()
        }
        .allow()
    );
    // Source withdrawn (read grant gone) or the mailbox changed since the read.
    assert!(
        !Facts {
            read_mailbox: None,
            ..ok()
        }
        .allow()
    );
    assert!(
        !Facts {
            binding_mailbox: "b@example.test",
            ..ok()
        }
        .allow()
    );
    // The work module no longer vouches for this exact final digest/session/connection.
    assert!(
        !Facts {
            approved: false,
            ..ok()
        }
        .allow()
    );
}
