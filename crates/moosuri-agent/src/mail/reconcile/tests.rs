use super::*;
use moosuri_connectors::gmail::{SecretString, TokenSet};
use moosuri_core::{MailApproval, MailDraftContent, MailDraftInput};
use std::sync::{
    Mutex as StdMutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

struct Fixture {
    posts: AtomicUsize,
    gets: AtomicUsize,
    raw: StdMutex<String>,
    candidate_raw: StdMutex<Option<String>>,
    mode: AtomicUsize,
    waiting: tokio::sync::Notify,
    release: tokio::sync::Notify,
}
async fn fixture() -> (
    std::net::SocketAddr,
    Arc<Fixture>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let state = Arc::new(Fixture {
        posts: AtomicUsize::new(0),
        gets: AtomicUsize::new(0),
        raw: StdMutex::new(String::new()),
        candidate_raw: StdMutex::new(None),
        mode: AtomicUsize::new(0),
        waiting: tokio::sync::Notify::new(),
        release: tokio::sync::Notify::new(),
    });
    let s = state.clone();
    let task = tokio::spawn(async move {
        loop {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let s = s.clone();
            tokio::spawn(async move {
                let mut bytes = Vec::new();
                let head_end = loop {
                    let mut b = [0; 4096];
                    let n = stream.read(&mut b).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&b[..n]);
                    if let Some(p) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                    assert!(bytes.len() < 1000000)
                };
                let head = String::from_utf8(bytes[..head_end].to_vec()).unwrap();
                let length = head
                    .lines()
                    .find_map(|l| {
                        l.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap_or(0);
                while bytes.len() < head_end + length {
                    let mut b = [0; 4096];
                    let n = stream.read(&mut b).await.unwrap();
                    if n == 0 {
                        return;
                    }
                    bytes.extend_from_slice(&b[..n]);
                }
                if head.starts_with("POST / ") {
                    assert_eq!(s.mode.load(Ordering::SeqCst), 8);
                    s.waiting.notify_one();
                    s.release.notified().await;
                    let body = r#"{"access_token":"synthetic-rotated","expires_in":3600,"token_type":"Bearer"}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    stream.write_all(response.as_bytes()).await.unwrap();
                    return;
                }
                if head.starts_with("POST /messages/send ") {
                    s.posts.fetch_add(1, Ordering::SeqCst);
                    let body: serde_json::Value =
                        serde_json::from_slice(&bytes[head_end..head_end + length]).unwrap();
                    *s.raw.lock().unwrap() = body["raw"].as_str().unwrap().into();
                    // Provider accepted the exact frozen bytes, then response was lost.
                    return;
                }
                assert!(head.starts_with("GET "), "reconciliation must be GET only");
                s.gets.fetch_add(1, Ordering::SeqCst);
                let mode = s.mode.load(Ordering::SeqCst);
                let body=if head.starts_with("GET /messages?") {
            if mode==7 {s.waiting.notify_one();s.release.notified().await;}
            assert!(head.contains("in%3Asent+rfc822msgid%3A%3C"));
            match mode {
                1=>serde_json::json!({"messages":[]}),
                2=>serde_json::json!({"messages":[{"id":"observed","threadId":"thread"},{"id":"observed","threadId":"thread"}]}),
                3=>serde_json::json!({"messages":[{"id":"observed","threadId":"thread"}],"nextPageToken":"another-page"}),
                _=>serde_json::json!({"messages":[{"id":"observed","threadId":"thread"}]}),
            }
        } else {
            assert!(head.starts_with("GET /messages/observed?format=raw"));
            if mode==6 {s.waiting.notify_one();s.release.notified().await;}
            serde_json::json!({"id":if mode==4 {"wrong-id"}else{"observed"},"threadId":"thread","labelIds":if mode==5 {vec!["INBOX"]}else{vec!["SENT"]},"raw":s.candidate_raw.lock().unwrap().clone().unwrap_or_else(||s.raw.lock().unwrap().clone())})
        }.to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                stream.write_all(response.as_bytes()).await.unwrap();
            });
        }
    });
    (addr, state, task)
}
async fn connected(store: &Store, user: &UserContext) -> moosuri_core::MailConnection {
    let start = store.start_mail_oauth(user).await.unwrap();
    let h = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let e = store.claim_mail_oauth(user, h).await.unwrap();
    store
        .complete_mail_oauth(
            e,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap()
}
fn oauth() -> Arc<OAuthClient> {
    Arc::new(OAuthClient {
        client_id: "synthetic-app".into(),
        client_secret: SecretString::new("synthetic-secret".into()).unwrap(),
        redirect_uri: "http://127.0.0.1/callback".into(),
    })
}
#[tokio::test]
async fn accepted_post_response_loss_actual_service_only_reads_across_ticks_manual_and_restart() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (addr, state, server) = fixture().await;
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
    let c = connected(&store, &user).await;
    let a = store
        .create_mail_draft(
            &user,
            MailDraftInput {
                connection_id: c.id,
                content: MailDraftContent {
                    from: c.email,
                    to: vec!["recipient@example.test".into()],
                    cc: vec![],
                    bcc: vec!["hidden@example.test".into()],
                    subject: "Synthetic equality".into(),
                    text: "Synthetic complete body".into(),
                    reply: None,
                },
                origin_thread_id: None,
            },
            None,
        )
        .await
        .unwrap();
    store
        .decide_mail(
            &user,
            a.id.clone(),
            MailApproval {
                revision: a.revision,
                digest: a.digest,
            },
            true,
        )
        .await
        .unwrap();
    let runtime = crate::Runtime::start(store.clone());
    let mut mail = runtime.mail_service();
    mail.client = GmailClient::synthetic_loopback(addr).unwrap();
    mail.oauth = Some(oauth());
    let stop = CancellationToken::new();
    let worker = tokio::spawn(mail.clone().worker(stop.clone()));
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let row = store.mail_action(&user, a.id.clone()).await.unwrap();
            if row
                .reconciliation
                .as_ref()
                .is_some_and(|r| r.status == "matching_sent_observed")
            {
                assert_eq!(row.state, "outcome_unknown");
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let row = mail
        .check_unknown(&user, a.id.clone(), &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(row.state, "outcome_unknown");
    assert!(row.provider_message_id.is_none());
    assert!(matches!(
        mail.check_unknown(&user, a.id.clone(), &CancellationToken::new())
            .await,
        Err(StoreError::Busy)
    ));
    tokio::time::sleep(Duration::from_millis(650)).await;
    assert_eq!(state.posts.load(Ordering::SeqCst), 1);
    assert_eq!(state.gets.load(Ordering::SeqCst), 4);
    stop.cancel();
    worker.await.unwrap();
    drop(mail);
    runtime.shutdown().await;
    drop(runtime);
    drop(store);
    let store = Store::open(dir.path()).unwrap();
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let row = store.mail_action(&user, a.id).await.unwrap();
    assert_eq!(row.reconciliation.unwrap().status, "matching_sent_observed");
    assert_eq!(state.posts.load(Ordering::SeqCst), 1);
    server.abort();
}
#[tokio::test]
async fn actual_service_candidates_and_incomplete_search_never_become_sent() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    for (mode, expected) in [
        (1, "no_candidates"),
        (2, "multiple_candidates"),
        (3, "incomplete_search"),
        (4, "candidate_mismatch"),
        (5, "candidate_mismatch"),
        (9, "candidate_mismatch"),
        (10, "unsupported_candidate"),
        (11, "unsupported_candidate"),
        (12, "candidate_mismatch"),
        (13, "candidate_mismatch"),
    ] {
        let (addr, state, server) = fixture().await;
        state.mode.store(mode, Ordering::SeqCst);
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
        let c = connected(&store, &user).await;
        let a = store
            .create_mail_draft(
                &user,
                MailDraftInput {
                    connection_id: c.id,
                    content: MailDraftContent {
                        from: c.email,
                        to: vec!["recipient@example.test".into()],
                        cc: vec![],
                        bcc: vec!["hidden@example.test".into()],
                        subject: "Synthetic".into(),
                        text: "Synthetic complete body with enough text".into(),
                        reply: None,
                    },
                    origin_thread_id: None,
                },
                None,
            )
            .await
            .unwrap();
        store
            .decide_mail(
                &user,
                a.id.clone(),
                MailApproval {
                    revision: a.revision,
                    digest: a.digest,
                },
                true,
            )
            .await
            .unwrap();
        let runtime = crate::Runtime::start(store.clone());
        let mut mail = runtime.mail_service();
        mail.client = GmailClient::synthetic_loopback(addr).unwrap();
        mail.oauth = Some(oauth());
        let d = store.next_mail_dispatch().await.unwrap().unwrap();
        let original = String::from_utf8(d.frozen.raw_mime().to_vec()).unwrap();
        assert!(original.contains("Bcc:"));
        let altered = match mode {
            9 => Some(
                original
                    .split_inclusive('\n')
                    .filter(|l| !l.starts_with("Bcc:"))
                    .collect::<String>(),
            ),
            10 => Some(format!("X-Hidden: synthetic\r\n{original}")),
            11 => Some(original.replace("text/plain", "multipart/mixed; boundary=synthetic")),
            12 => Some(original[..original.len() - 8].into()),
            13 => Some(original.replace("recipient@example.test", "other@example.test")),
            _ => None,
        };
        *state.candidate_raw.lock().unwrap() =
            altered.map(|raw| encode_fixture_raw(raw.as_bytes()));

        mail.dispatch(d, &CancellationToken::new()).await;
        let row = mail
            .check_unknown(&user, a.id, &CancellationToken::new())
            .await
            .unwrap();
        assert_eq!(row.state, "outcome_unknown");
        assert_eq!(row.reconciliation.unwrap().status, expected);
        assert_eq!(state.posts.load(Ordering::SeqCst), 1);
        assert!(store.next_mail_dispatch().await.unwrap().is_none());
        runtime.shutdown().await;
        server.abort();
    }
}

#[tokio::test]
async fn actual_service_discards_a_full_candidate_that_arrives_after_disconnect() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    for mode in [6, 7] {
        let (addr, state, server) = fixture().await;
        state.mode.store(mode, Ordering::SeqCst);
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
        let c = connected(&store, &user).await;
        let a = store
            .create_mail_draft(
                &user,
                MailDraftInput {
                    connection_id: c.id.clone(),
                    content: MailDraftContent {
                        from: c.email,
                        to: vec!["recipient@example.test".into()],
                        cc: vec![],
                        bcc: vec![],
                        subject: "Synthetic".into(),
                        text: "Private late body".into(),
                        reply: None,
                    },
                    origin_thread_id: None,
                },
                None,
            )
            .await
            .unwrap();
        store
            .decide_mail(
                &user,
                a.id.clone(),
                MailApproval {
                    revision: a.revision,
                    digest: a.digest,
                },
                true,
            )
            .await
            .unwrap();
        let runtime = crate::Runtime::start(store.clone());
        let mut mail = runtime.mail_service();
        mail.client = GmailClient::synthetic_loopback(addr).unwrap();
        mail.oauth = Some(oauth());
        let d = store.next_mail_dispatch().await.unwrap().unwrap();
        mail.dispatch(d, &CancellationToken::new()).await;
        let check = tokio::spawn({
            let mail = mail.clone();
            let user = user.clone();
            let id = a.id.clone();
            async move {
                mail.check_unknown(&user, id, &CancellationToken::new())
                    .await
            }
        });
        tokio::time::timeout(Duration::from_secs(2), state.waiting.notified())
            .await
            .unwrap();
        store.disconnect_mail(&user, c.id).await.unwrap();
        state.release.notify_one();
        let row = check.await.unwrap().unwrap();
        assert_eq!(row.state, "outcome_unknown");
        assert!(row.content.is_none());
        assert_eq!(row.reconciliation.unwrap().status, "not_checked");
        assert_eq!(state.posts.load(Ordering::SeqCst), 1);
        assert_eq!(
            state.gets.load(Ordering::SeqCst),
            if mode == 7 { 1 } else { 2 }
        );
        runtime.shutdown().await;
        server.abort();
    }
}

#[tokio::test]
async fn actual_service_refresh_after_disconnect_cannot_resurrect_credentials() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (addr, state, server) = fixture().await;
    state.mode.store(8, Ordering::SeqCst);
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
    let start = store.start_mail_oauth(&user).await.unwrap();
    let h = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let e = store.claim_mail_oauth(&user, h).await.unwrap();
    let c = store
        .complete_mail_oauth(
            e,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 1,
                scopes: None,
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap();
    let a = store
        .create_mail_draft(
            &user,
            MailDraftInput {
                connection_id: c.id.clone(),
                content: MailDraftContent {
                    from: c.email,
                    to: vec!["recipient@example.test".into()],
                    cc: vec![],
                    bcc: vec![],
                    subject: "Synthetic".into(),
                    text: "Synthetic body".into(),
                    reply: None,
                },
                origin_thread_id: None,
            },
            None,
        )
        .await
        .unwrap();
    store
        .decide_mail(
            &user,
            a.id.clone(),
            MailApproval {
                revision: a.revision,
                digest: a.digest,
            },
            true,
        )
        .await
        .unwrap();
    let d = store.next_mail_dispatch().await.unwrap().unwrap();
    store.finish_mail_dispatch(&d, None, false).await.unwrap();
    let runtime = crate::Runtime::start(store.clone());
    let mut mail = runtime.mail_service();
    mail.client = GmailClient::synthetic_loopback(addr).unwrap();
    mail.oauth = Some(oauth());
    let check = tokio::spawn({
        let user = user.clone();
        let id = a.id.clone();
        async move {
            mail.check_unknown(&user, id, &CancellationToken::new())
                .await
        }
    });
    tokio::time::timeout(Duration::from_secs(2), state.waiting.notified())
        .await
        .unwrap();
    store.disconnect_mail(&user, c.id.clone()).await.unwrap();
    state.release.notify_one();
    let row = check.await.unwrap().unwrap();
    assert_eq!(row.reconciliation.unwrap().status, "not_checked");
    assert!(store.mail_binding(&user, c.id).await.is_err());
    assert_eq!(
        store.mail_connections(&user).await.unwrap()[0].status,
        "disconnected"
    );
    assert_eq!(state.posts.load(Ordering::SeqCst), 0);
    assert_eq!(state.gets.load(Ordering::SeqCst), 0);
    runtime.shutdown().await;
    server.abort();
}

// Tiny fixture-only URL-safe encoder keeps acceptance dependencies unchanged.
fn encode_fixture_raw(raw: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for c in raw.chunks(3) {
        out.push(ALPHABET[(c[0] >> 2) as usize] as char);
        out.push(
            ALPHABET[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char,
        );
        if c.len() > 1 {
            out.push(
                ALPHABET[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize]
                    as char,
            );
        }
        if c.len() > 2 {
            out.push(ALPHABET[(c[2] & 63) as usize] as char);
        }
    }
    out
}
