use super::*;
use axum::{
    Router,
    body::to_bytes,
    extract::Request,
    http::{StatusCode, header},
    response::IntoResponse,
    routing::any,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

fn secret() -> SecretString {
    SecretString::new("synthetic-token-canary".into()).unwrap()
}
fn oauth() -> OAuthClient {
    OAuthClient {
        client_id: "fixture-client.apps.googleusercontent.com".into(),
        client_secret: secret(),
        redirect_uri: "https://example.test/oauth/callback".into(),
    }
}
fn content() -> MailContent {
    MailContent {
        from: "sender@example.test".into(),
        to: vec!["to@example.test".into()],
        cc: vec!["cc@example.test".into()],
        bcc: vec!["hidden@example.test".into()],
        subject: "회의 확인 ✓".into(),
        text: "안녕하세요\n내일 만나요. café".into(),
        reply: None,
    }
}
async fn server(router: Router) -> (GmailClient, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (
        GmailClient {
            transport: transport::Transport::fixture(base, 1024 * 1024),
        },
        task,
    )
}
fn json(value: serde_json::Value) -> impl IntoResponse {
    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "application/json")],
        value.to_string(),
    )
}

#[test]
fn oauth_url_is_fixed_s256_and_no_send_scope_expansion() {
    let verifier = "dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk";
    assert_eq!(
        pkce_challenge(verifier).unwrap(),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let result = GmailClient::authorization_url(
        "client",
        "http://127.0.0.1:8000/callback",
        &"s".repeat(43),
        verifier,
    )
    .unwrap();
    let url = url::Url::parse(&result).unwrap();
    assert_eq!(
        url.origin().ascii_serialization(),
        "https://accounts.google.com"
    );
    let query = url
        .query_pairs()
        .collect::<std::collections::HashMap<_, _>>();
    assert_eq!(query.get("code_challenge_method").unwrap(), "S256");
    assert_eq!(query.get("scope").unwrap(), &MINIMAL_SCOPES.join(" "));
    assert!(!result.contains(verifier));
    assert!(
        GmailClient::authorization_url(
            "client",
            "http://attacker.test/callback",
            &"s".repeat(43),
            verifier
        )
        .is_err()
    );
    assert!(
        GmailClient::authorization_url(
            "client",
            "https://example.test/cb#fragment",
            &"s".repeat(43),
            verifier
        )
        .is_err()
    );
    assert!(pkce_challenge("short").is_err());
}
#[test]
fn unicode_mime_preserves_all_recipients_and_snapshot_identity() {
    let mail = FrozenMail::prepare(&content(), "stable-id@example.test").unwrap();
    let parsed = parse_mail(mail.raw_mime()).unwrap();
    assert_eq!(parsed.subject, content().subject);
    assert_eq!(parsed.text.trim_end(), content().text);
    assert_eq!(parsed.to, vec!["to@example.test"]);
    assert_eq!(parsed.cc, vec!["cc@example.test"]);
    assert_eq!(parsed.bcc, vec!["hidden@example.test"]);
    assert_eq!(parsed.message_id.as_deref(), Some("stable-id@example.test"));
    let restored = FrozenMail::restore(mail.raw_mime().to_vec(), mail.digest(), None).unwrap();
    assert_eq!(restored.raw_mime(), mail.raw_mime());
    assert!(
        FrozenMail::restore(
            mail.raw_mime().to_vec(),
            mail.digest(),
            Some("different-thread".into())
        )
        .is_err()
    );
    let mut raw = mail.raw_mime().to_vec();
    raw.push(b'x');
    assert!(FrozenMail::restore(raw, mail.digest(), None).is_err());
}
#[test]
fn header_injection_unsupported_attachments_and_excess_input_are_rejected() {
    let mut c = content();
    c.to = vec!["to@example.test\r\nBcc: injected@example.test".into()];
    assert!(FrozenMail::prepare(&c, "id@example.test").is_err());
    let mut c = content();
    c.subject = "ok\r\nBcc: injected@example.test".into();
    assert!(FrozenMail::prepare(&c, "id@example.test").is_err());
    let mut c = content();
    c.text = "x".repeat(128 * 1024 + 1);
    assert!(FrozenMail::prepare(&c, "id@example.test").is_err());
    assert!(FrozenMail::prepare(&content(), "bad\r\n@example.test").is_err());
    let mut value = serde_json::to_value(content()).unwrap();
    value["attachments"] = serde_json::json!(["private-file"]);
    assert!(serde_json::from_value::<MailContent>(value).is_err());
    for address in [
        "Name <to@example.test>",
        "to@example.test,other@example.test",
        "a..b@example.test",
        "a@-bad.test",
        "a@example.test@else.test",
    ] {
        assert!(mime::validate_address(address).is_err());
    }
}
#[test]
fn receive_decodes_multipart_transfer_encodings_and_ignores_active_html() {
    let raw=b"From: Sender <sender@example.test>\r\nTo: to@example.test\r\nSubject: =?UTF-8?B?7ZqM7J2Y?=\r\nMIME-Version: 1.0\r\nContent-Type: multipart/alternative; boundary=fixture\r\n\r\n--fixture\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: base64\r\n\r\naGVsbG8gd29ybGQ=\r\n--fixture\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<p>hello</p><img src=\"https://example.test/tracker\"><script>bad()</script>\r\n--fixture--\r\n";
    let mail = parse_mail(raw).unwrap();
    assert_eq!(mail.subject, "회의");
    assert_eq!(mail.from, vec!["sender@example.test"]);
    assert_eq!(mail.text.trim(), "hello world");
    assert!(!mail.text.contains("tracker"));
}
#[tokio::test]
async fn token_exchange_and_refresh_keep_omitted_fields_distinct() {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let capture = seen.clone();
    let (client,task)=server(Router::new().fallback(any(move |request:Request|{let capture=capture.clone();async move{
        assert_eq!(request.method(),reqwest::Method::POST);
        assert!(request.headers().get(header::AUTHORIZATION).is_none());
        assert_eq!(request.headers().get(header::CONTENT_TYPE).unwrap(),"application/x-www-form-urlencoded");
        let body=to_bytes(request.into_body(),16*1024).await.unwrap();
        let fields=url::form_urlencoded::parse(&body).map(|(k,v)|(k.into_owned(),v.into_owned())).collect::<std::collections::HashMap<_,_>>();
        capture.lock().unwrap().push(fields.clone());
        if fields["grant_type"]=="authorization_code"{json(serde_json::json!({"access_token":"synthetic-access","refresh_token":"synthetic-refresh","expires_in":3600,"token_type":"Bearer","scope":MINIMAL_SCOPES.join(" ")}))}
        else {json(serde_json::json!({"access_token":"synthetic-new-access","expires_in":3600,"token_type":"Bearer"}))}
    }}))).await;
    let cancel = CancellationToken::new();
    let result = client
        .exchange_code(
            &oauth(),
            &secret(),
            &SecretString::new("v".repeat(43)).unwrap(),
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(
        result.refresh_token.as_ref().unwrap().expose(),
        "synthetic-refresh"
    );
    assert_eq!(result.scopes.as_ref().unwrap().len(), 2);
    let refreshed = client
        .refresh(&oauth(), result.refresh_token.as_ref().unwrap(), &cancel)
        .await
        .unwrap();
    assert!(refreshed.refresh_token.is_none());
    assert!(refreshed.scopes.is_none());
    let requests = seen.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0]["code_verifier"], "v".repeat(43));
    assert_eq!(requests[1]["refresh_token"], "synthetic-refresh");
    task.abort();
}
#[tokio::test]
async fn gmail_read_path_is_me_and_search_is_encoded_then_parsed() {
    let raw = FrozenMail::prepare(&content(), "source@example.test")
        .unwrap()
        .raw_mime()
        .to_vec();
    let encoded = URL_SAFE_NO_PAD.encode(raw);
    let observed = Arc::new(Mutex::new(Vec::new()));
    let capture = observed.clone();
    let (client,task)=server(Router::new().fallback(any(move |request:Request|{let encoded=encoded.clone();let capture=capture.clone();async move{
        assert_eq!(request.headers().get(header::AUTHORIZATION).unwrap(),"Bearer synthetic-token-canary");
        let path=request.uri().path().to_owned();capture.lock().unwrap().push(request.uri().to_string());
        if path=="/profile"{json(serde_json::json!({"emailAddress":"sender@example.test"}))}
        else if path=="/messages"{json(serde_json::json!({"messages":[{"id":"m1","threadId":"t1"}],"nextPageToken":"next","resultSizeEstimate":1}))}
        else if path=="/threads/t1"{json(serde_json::json!({"id":"t1","messages":[{"id":"m1"}]}))}
        else{json(serde_json::json!({"id":"m1","threadId":"t1","labelIds":["INBOX"],"raw":encoded}))}
    }}))).await;
    let cancel = CancellationToken::new();
    assert_eq!(
        client
            .profile(&secret(), &cancel)
            .await
            .unwrap()
            .email_address,
        "sender@example.test"
    );
    let page = client
        .list_messages(
            &secret(),
            "in:sent rfc822msgid:<source@example.test>",
            None,
            5,
            &cancel,
        )
        .await
        .unwrap();
    assert_eq!(page.messages[0].id, "m1");
    let thread = client.get_thread(&secret(), "t1", &cancel).await.unwrap();
    assert_eq!(thread.messages[0].mail.subject, "회의 확인 ✓");
    assert!(!thread.truncated);
    let requests = observed.lock().unwrap();
    assert!(requests[1].contains("q=in%3Asent+rfc822msgid%3A%3Csource%40example.test%3E"));
    assert!(
        client
            .transport
            .target(&Endpoint::Gmail, "../other-user")
            .is_err()
    );
    task.abort();
}
#[tokio::test]
async fn sending_uses_only_frozen_bytes_and_preserves_thread() {
    let mut c = content();
    c.reply = Some(ReplyContext {
        thread_id: "t1".into(),
        in_reply_to: "original@example.test".into(),
        references: vec![],
    });
    let mail = FrozenMail::prepare(&c, "outgoing@example.test").unwrap();
    let expected = mail.raw_mime().to_vec();
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let (client, task) = server(Router::new().fallback(any(move |request: Request| {
        let expected = expected.clone();
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            assert_eq!(request.uri().path(), "/messages/send");
            let bytes = to_bytes(request.into_body(), 1024 * 1024).await.unwrap();
            let body: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(
                URL_SAFE_NO_PAD
                    .decode(body["raw"].as_str().unwrap())
                    .unwrap(),
                expected
            );
            assert_eq!(body["threadId"], "t1");
            json(serde_json::json!({"id":"sent1","threadId":"t1"}))
        }
    })))
    .await;
    let receipt = client
        .send_frozen(&secret(), &mail, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(receipt.id, "sent1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    task.abort();
}
#[tokio::test]
async fn redirects_and_oversized_responses_never_leak_tokens() {
    let target_hits = Arc::new(AtomicUsize::new(0));
    let count = target_hits.clone();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let target = format!("http://{}/stolen", listener.local_addr().unwrap());
    let target_task = tokio::spawn(async move {
        axum::serve(
            listener,
            Router::new().fallback(any(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    "unexpected"
                }
            })),
        )
        .await
        .unwrap()
    });
    let (client, task) = server(Router::new().fallback(any(move || {
        let target = target.clone();
        async move { (StatusCode::FOUND, [(header::LOCATION, target)]) }
    })))
    .await;
    assert_eq!(
        client
            .profile(&secret(), &CancellationToken::new())
            .await
            .err(),
        Some(GmailError::Http(302))
    );
    assert_eq!(target_hits.load(Ordering::SeqCst), 0);
    task.abort();
    target_task.abort();
    let (mut client, task) = server(Router::new().fallback(any(|| async {
        (
            StatusCode::OK,
            [(header::CONTENT_TYPE, "application/json")],
            "x".repeat(1024),
        )
    })))
    .await;
    client.transport.test_limit = Some(32);
    assert_eq!(
        client
            .profile(&secret(), &CancellationToken::new())
            .await
            .err(),
        Some(GmailError::LimitExceeded)
    );
    task.abort();
}
#[tokio::test]
async fn http_errors_are_static_and_precancelled_call_never_dispatches() {
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let (client, task) = server(Router::new().fallback(any(move || {
        let count = count.clone();
        async move {
            count.fetch_add(1, Ordering::SeqCst);
            (StatusCode::UNAUTHORIZED, "token=synthetic-token-canary")
        }
    })))
    .await;
    let error = client
        .profile(&secret(), &CancellationToken::new())
        .await
        .err()
        .unwrap();
    assert_eq!(error, GmailError::Http(401));
    assert!(!format!("{error:?} {error}").contains("canary"));
    let cancel = CancellationToken::new();
    cancel.cancel();
    assert_eq!(
        client.profile(&secret(), &cancel).await.err(),
        Some(GmailError::Cancelled)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    task.abort();
}
#[tokio::test]
async fn lost_send_response_is_not_retried() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    let task = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        count.fetch_add(1, Ordering::SeqCst);
        let mut bytes = Vec::new();
        let mut buf = [0; 4096];
        let mut expected = None;
        loop {
            let n = stream.read(&mut buf).await.unwrap();
            if n == 0 {
                break;
            }
            bytes.extend_from_slice(&buf[..n]);
            if expected.is_none()
                && let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n")
            {
                let headers = String::from_utf8_lossy(&bytes[..end]);
                let length = headers
                    .lines()
                    .find_map(|line| {
                        line.to_ascii_lowercase()
                            .strip_prefix("content-length:")
                            .and_then(|s| s.trim().parse::<usize>().ok())
                    })
                    .unwrap();
                expected = Some(end + 4 + length);
            }
            if expected.is_some_and(|n| bytes.len() >= n) {
                break;
            }
        }
        stream.shutdown().await.unwrap();
        drop(stream);
        if tokio::time::timeout(std::time::Duration::from_millis(300), listener.accept())
            .await
            .is_ok()
        {
            count.fetch_add(1, Ordering::SeqCst);
        }
    });
    let client = GmailClient {
        transport: transport::Transport::fixture(base, 1024 * 1024),
    };
    let mail = FrozenMail::prepare(&content(), "lost@example.test").unwrap();
    assert!(
        client
            .send_frozen(&secret(), &mail, &CancellationToken::new())
            .await
            .is_err()
    );
    task.await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn chunked_body_limit_and_inflight_cancellation_are_enforced() {
    let (mut client, task) = server(Router::new().fallback(any(|| async {
        let chunks =
            futures_util::stream::iter((0..100).map(|_| Ok::<_, std::io::Error>("x".repeat(32))));
        (
            [(header::CONTENT_TYPE, "application/json")],
            axum::body::Body::from_stream(chunks),
        )
    })))
    .await;
    client.transport.test_limit = Some(100);
    assert_eq!(
        client
            .profile(&secret(), &CancellationToken::new())
            .await
            .err(),
        Some(GmailError::LimitExceeded)
    );
    task.abort();
    let started = Arc::new(tokio::sync::Notify::new());
    let signal = started.clone();
    let (client, task) = server(Router::new().fallback(any(move || {
        let signal = signal.clone();
        async move {
            signal.notify_one();
            std::future::pending::<()>().await;
            json(serde_json::json!({}))
        }
    })))
    .await;
    let cancel = CancellationToken::new();
    let token = secret();
    let operation = client.profile(&token, &cancel);
    tokio::pin!(operation);
    tokio::select! {_=&mut operation=>panic!("request unexpectedly finished"),_=started.notified()=>{}}
    cancel.cancel();
    assert_eq!(operation.await.err(), Some(GmailError::Cancelled));
    task.abort();
}

#[test]
fn additional_mixed_inline_sections_are_explicitly_incomplete() {
    let raw=b"From: sender@example.test\r\nTo: to@example.test\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=mixed\r\n\r\n--mixed\r\nContent-Type: text/plain\r\n\r\nRefundable.\r\n--mixed\r\nContent-Type: text/plain\r\n\r\nException: not refundable after Friday.\r\n--mixed--\r\n";
    let parsed = parse_mail(raw).unwrap();
    assert!(parsed.text.contains("Refundable"));
    assert!(
        parsed.text_truncated,
        "the first section must not masquerade as the complete email"
    );
}

#[test]
fn downloaded_web_client_requires_exact_callback_and_never_selects_endpoints() {
    let redirect = "https://assistant.example.test/api/v1/mail/oauth/callback";
    let file = serde_json::json!({"web":{
        "client_id":"synthetic.apps.googleusercontent.com","client_secret":"synthetic-setup-secret",
        "redirect_uris":[redirect],"auth_uri":"https://attacker.example/authorize",
        "token_uri":"https://attacker.example/token","project_id":"synthetic-project"
    }});
    let client = OAuthClient::from_web_client_json(&file.to_string(), redirect).unwrap();
    let auth = GmailClient::authorization_url(
        &client.client_id,
        &client.redirect_uri,
        &"s".repeat(64),
        &"v".repeat(64),
    )
    .unwrap();
    let auth = url::Url::parse(&auth).unwrap();
    assert_eq!(
        auth.origin().ascii_serialization(),
        "https://accounts.google.com"
    );
    assert_eq!(
        auth.query_pairs()
            .find(|(key, _)| key == "redirect_uri")
            .unwrap()
            .1,
        redirect
    );
    assert!(!auth.as_str().contains("synthetic-setup-secret"));
    assert!(matches!(
        OAuthClient::from_web_client_json(&file.to_string(), &format!("{redirect}/")),
        Err(GoogleClientFileError::RedirectMismatch)
    ));
    assert!(matches!(
        OAuthClient::from_web_client_json(
            &file.to_string(),
            "https://other.example.test/api/v1/mail/oauth/callback"
        ),
        Err(GoogleClientFileError::RedirectMismatch)
    ));
}

#[test]
fn downloaded_client_rejects_other_credential_types_and_bounded_bad_inputs() {
    let redirect = "http://127.0.0.1:3210/api/v1/mail/oauth/callback";
    for file in ["{}".to_owned(), "not-json secret-canary".into(), " ".repeat(16385),
        serde_json::json!({"installed":{"client_id":"synthetic","client_secret":"secret-canary","redirect_uris":[redirect]}}).to_string(),
        serde_json::json!({"type":"service_account","private_key":"secret-canary"}).to_string(),
        serde_json::json!({"web":{"client_id":"synthetic","client_secret":"secret\ncanary","redirect_uris":[redirect]}}).to_string(),
        serde_json::json!({"web":{"client_id":"synthetic","client_secret":"secret-canary","redirect_uris":[redirect]},"installed":{}}).to_string(),
    ] {
        assert!(matches!(OAuthClient::from_web_client_json(&file, redirect),Err(GoogleClientFileError::Invalid)));
    }
    let good=serde_json::json!({"web":{"client_id":"synthetic","client_secret":"secret-canary","redirect_uris":[redirect]}}).to_string();
    assert!(OAuthClient::from_web_client_json(&good, redirect).is_ok());
}
