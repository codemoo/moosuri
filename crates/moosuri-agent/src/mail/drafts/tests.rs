use super::*;
use axum::{
    Router,
    body::to_bytes,
    extract::{Request, State},
    http::{Method, StatusCode},
    response::IntoResponse,
    routing::any,
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_connectors::gmail::{COMPOSE_SCOPES, FrozenMail, MailContent, SecretString, TokenSet};
use moosuri_core::{MailApproval, MailDraftInput};
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
struct Fixture {
    raw: StdMutex<String>,
    writes: AtomicUsize,
    sends: AtomicUsize,
    gets: AtomicUsize,
    block: AtomicBool,
    waiting: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
async fn handler(State(s): State<Arc<Fixture>>, r: Request) -> axum::response::Response {
    let path = r.uri().path().to_string();
    let method = r.method().clone();
    if path.contains("send") {
        s.sends.fetch_add(1, Ordering::SeqCst);
        return StatusCode::BAD_REQUEST.into_response();
    }
    if method == Method::DELETE {
        s.writes.fetch_add(1, Ordering::SeqCst);
        return StatusCode::NO_CONTENT.into_response();
    }
    let value = if method == Method::GET {
        s.gets.fetch_add(1, Ordering::SeqCst);
        if s.block.load(Ordering::SeqCst) {
            s.waiting.notify_one();
            s.release.notified().await;
        }
        if path == "/drafts" {
            serde_json::json!({"drafts":[{"id":"draft1","message":{"id":"msg1","threadId":"thread1"}}]})
        } else {
            serde_json::json!({"id":"draft1","message":{"raw":s.raw.lock().unwrap().clone()}})
        }
    } else {
        assert!(method == Method::POST || method == Method::PUT);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(r.into_body(), 1000000).await.unwrap()).unwrap();
        *s.raw.lock().unwrap() = value["message"]["raw"].as_str().unwrap().into();
        s.writes.fetch_add(1, Ordering::SeqCst);
        serde_json::json!({"id":"draft1","message":{"id":"msg1","threadId":"thread1"}})
    };
    axum::Json(value).into_response()
}
fn approval(a: &moosuri_core::MailAction) -> MailApproval {
    MailApproval {
        revision: a.revision,
        digest: a.digest.clone(),
    }
}
async fn setup(
    compose: bool,
) -> (
    tempfile::TempDir,
    Store,
    UserContext,
    MailService,
    moosuri_core::MailAction,
    Arc<Fixture>,
    tokio::task::JoinHandle<()>,
) {
    let s = Arc::new(Fixture {
        raw: StdMutex::new(String::new()),
        writes: AtomicUsize::new(0),
        sends: AtomicUsize::new(0),
        gets: AtomicUsize::new(0),
        block: AtomicBool::new(false),
        waiting: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let router = Router::new().fallback(any(handler)).with_state(s.clone());
    let server = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let store = Store::open(dir.path()).unwrap();
    store
        .bootstrap(
            "Synthetic".into(),
            "owner@example.test".into(),
            "Synthetic-Account-Password-42".into(),
            false,
        )
        .await
        .unwrap();
    let user = store
        .login(
            "owner@example.test".into(),
            "Synthetic-Account-Password-42".into(),
        )
        .await
        .unwrap()
        .user;
    let app = Arc::new(OAuthClient {
        client_id: "synthetic-app".into(),
        client_secret: SecretString::new("synthetic-secret".into()).unwrap(),
        redirect_uri: "http://127.0.0.1/callback".into(),
    });
    let pin = GoogleAppPin {
        revision: None,
        client_id: app.client_id.clone(),
        redirect_uri: app.redirect_uri.clone(),
    };
    let start = if compose {
        store
            .start_mail_compose_oauth_with_app(&user, pin.clone())
            .await
            .unwrap()
    } else {
        store
            .start_mail_oauth_with_app(&user, pin.clone(), false)
            .await
            .unwrap()
    };
    let h = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let e = store
        .claim_mail_oauth_with_app(&user, h, pin)
        .await
        .unwrap();
    let connection = store
        .complete_mail_oauth(
            e,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: Some(COMPOSE_SCOPES.iter().map(|s| s.to_string()).collect()),
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap();
    let action = store
        .create_mail_draft(
            &user,
            MailDraftInput {
                connection_id: connection.id,
                content: MailDraftContent {
                    from: connection.email,
                    to: vec!["recipient@example.test".into()],
                    cc: vec![],
                    bcc: vec!["hidden@example.test".into()],
                    subject: "Synthetic provider draft".into(),
                    text: "Complete synthetic text".into(),
                    reply: None,
                },
                origin_thread_id: None,
            },
            None,
        )
        .await
        .unwrap();
    let runtime = crate::Runtime::start(store.clone());
    let mut mail = runtime.mail_service();
    mail.client = GmailClient::synthetic_loopback(addr).unwrap();
    mail.oauth = Some(app);
    runtime.shutdown().await;
    (dir, store, user, mail, action, s, server)
}
#[tokio::test]
async fn provider_crud_never_sends_or_rewrites_local_approval_and_detects_stale_provider_copy() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, store, user, mail, a, s, server) = setup(true).await;
    let cancel = CancellationToken::new();
    let saved = mail
        .save_gmail_draft(
            &user,
            a.id.clone(),
            GmailDraftSave {
                expected: approval(&a),
                draft_id: None,
                expected_provider_digest: None,
            },
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(saved.id, "draft1");
    assert_eq!(
        store.mail_action(&user, a.id.clone()).await.unwrap().state,
        "pending_approval"
    );
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let page = mail
        .gmail_drafts(&user, &a.connection_id, None, &cancel)
        .await
        .unwrap();
    assert_eq!(page.drafts.len(), 1);
    let first = mail
        .gmail_draft(&user, &a.connection_id, "draft1", &cancel)
        .await
        .unwrap();
    assert_eq!(first.content.as_ref().unwrap().bcc, ["hidden@example.test"]);
    mail.save_gmail_draft(
        &user,
        a.id.clone(),
        GmailDraftSave {
            expected: approval(&a),
            draft_id: Some(first.id.clone()),
            expected_provider_digest: Some(first.digest.clone()),
        },
        &cancel,
    )
    .await
    .unwrap();
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let changed = FrozenMail::prepare(
        &MailContent {
            from: "sender@example.test".into(),
            to: vec!["other@example.test".into()],
            cc: vec![],
            bcc: vec![],
            subject: "External edit".into(),
            text: "External body".into(),
            reply: None,
        },
        "external@example.test",
    )
    .unwrap();
    *s.raw.lock().unwrap() = URL_SAFE_NO_PAD.encode(changed.raw_mime());
    assert!(
        mail.save_gmail_draft(
            &user,
            a.id.clone(),
            GmailDraftSave {
                expected: approval(&a),
                draft_id: Some(first.id),
                expected_provider_digest: Some(first.digest.clone())
            },
            &cancel
        )
        .await
        .is_err()
    );
    assert_eq!(s.writes.load(Ordering::SeqCst), 2);
    let local = store.mail_action(&user, a.id.clone()).await.unwrap();
    assert_eq!(local.state, "approved");
    assert_eq!(local.digest, a.digest);
    assert_eq!(local.content.unwrap().subject, "Synthetic provider draft");
    assert!(
        mail.delete_gmail_draft(&user, &a.connection_id, "draft1", &first.digest, &cancel)
            .await
            .is_err()
    );
    let current = mail
        .gmail_draft(&user, &a.connection_id, "draft1", &cancel)
        .await
        .unwrap();
    mail.delete_gmail_draft(&user, &a.connection_id, "draft1", &current.digest, &cancel)
        .await
        .unwrap();
    assert_eq!(s.writes.load(Ordering::SeqCst), 3);
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    server.abort();
}
#[tokio::test]
async fn provider_drafts_require_opt_in_and_late_revoked_reads_cannot_publish() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, _store, user, mail, a, s, server) = setup(false).await;
    assert!(
        mail.gmail_drafts(&user, &a.connection_id, None, &CancellationToken::new())
            .await
            .is_err()
    );
    assert!(
        mail.save_gmail_draft(
            &user,
            a.id.clone(),
            GmailDraftSave {
                expected: approval(&a),
                draft_id: None,
                expected_provider_digest: None
            },
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(s.gets.load(Ordering::SeqCst), 0);
    assert_eq!(s.writes.load(Ordering::SeqCst), 0);
    server.abort();
    let (_dir, store, user, mail, a, s, server) = setup(true).await;
    s.block.store(true, Ordering::SeqCst);
    let m = mail.clone();
    let u = user.clone();
    let connection = a.connection_id.clone();
    let pending = tokio::spawn(async move {
        m.gmail_drafts(&u, &connection, None, &CancellationToken::new())
            .await
    });
    s.waiting.notified().await;
    store.disconnect_mail(&user, a.connection_id).await.unwrap();
    s.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn provider_update_held_read_cannot_write_a_now_rejected_local_snapshot() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, store, user, mail, a, s, server) = setup(true).await;
    let cancel = CancellationToken::new();
    mail.save_gmail_draft(
        &user,
        a.id.clone(),
        GmailDraftSave {
            expected: approval(&a),
            draft_id: None,
            expected_provider_digest: None,
        },
        &cancel,
    )
    .await
    .unwrap();
    let current = mail
        .gmail_draft(&user, &a.connection_id, "draft1", &cancel)
        .await
        .unwrap();
    s.block.store(true, Ordering::SeqCst);
    let m = mail.clone();
    let u = user.clone();
    let id = a.id.clone();
    let expected = approval(&a);
    let pending = tokio::spawn(async move {
        m.save_gmail_draft(
            &u,
            id,
            GmailDraftSave {
                expected,
                draft_id: Some("draft1".into()),
                expected_provider_digest: Some(current.digest),
            },
            &CancellationToken::new(),
        )
        .await
    });
    s.waiting.notified().await;
    store
        .decide_mail(&user, a.id.clone(), approval(&a), false)
        .await
        .unwrap();
    s.release.notify_one();
    assert!(pending.await.unwrap().is_err());
    assert_eq!(
        s.writes.load(Ordering::SeqCst),
        1,
        "the initial create is the only write; no late PUT"
    );
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    server.abort();
}
