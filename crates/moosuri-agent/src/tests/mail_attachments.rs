//! Synthetic Gmail IO + actual Store receipts; the opt-in PDF case uses the real
//! deployment executable's fixed parser child, never a fixture extraction result.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_connectors::gmail::{OAuthClient, SecretString, TokenSet};
use moosuri_core::{ExternalToolRequest, MailToolRequest};
use moosuri_store::{
    ExternalToolStart, GoogleAppPin, KnowledgeToolInput, KnowledgeToolResult, PendingKnowledgeCall,
    PrivateCheckpoint,
};
use std::sync::atomic::AtomicBool;

struct Wire {
    bytes: tokio::sync::Mutex<Vec<u8>>,
    hold: AtomicBool,
    entered: Semaphore,
    release: Semaphore,
    requests: AtomicUsize,
    address: std::sync::OnceLock<std::net::SocketAddr>,
    mime: &'static str,
    dense: AtomicBool,
}
async fn fixture(
    bytes: Vec<u8>,
    mime: &'static str,
) -> (
    crate::mail::MailService,
    Store,
    UserContext,
    String,
    String,
    Arc<Wire>,
    JoinHandle<()>,
    tempfile::TempDir,
) {
    let (dir, store, user, thread) =
        super::knowledge::tool_configured("http://127.0.0.1:9/v1").await;
    let oauth = Arc::new(OAuthClient {
        client_id: "synthetic.apps.googleusercontent.com".into(),
        client_secret: SecretString::new("synthetic-client-secret".into()).unwrap(),
        redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
    });
    let pin = GoogleAppPin {
        revision: None,
        client_id: oauth.client_id.clone(),
        redirect_uri: oauth.redirect_uri.clone(),
    };
    let start = store
        .start_mail_oauth_with_app(&user, pin.clone(), false)
        .await
        .unwrap();
    let handle = store
        .capture_mail_oauth(
            start.state,
            SecretString::new("synthetic-code".into()).unwrap(),
        )
        .await
        .unwrap();
    let exchange = store
        .claim_mail_oauth_with_app(&user, handle, pin)
        .await
        .unwrap();
    let connection = store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-read-token".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "sender@example.test".into(),
        )
        .await
        .unwrap();
    let wire = Arc::new(Wire {
        bytes: tokio::sync::Mutex::new(bytes),
        hold: AtomicBool::new(false),
        entered: Semaphore::new(0),
        release: Semaphore::new(0),
        requests: AtomicUsize::new(0),
        address: std::sync::OnceLock::new(),
        mime,
        dense: AtomicBool::new(false),
    });
    let handler = wire.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    wire.address.set(address).unwrap();
    let app=Router::new().fallback(axum::routing::any(move|request:axum::extract::Request|{let wire=handler.clone();async move{
        wire.requests.fetch_add(1,Ordering::SeqCst);assert_eq!(request.method(),"GET");assert_eq!(request.headers()["authorization"],"Bearer synthetic-read-token");
        let bytes=wire.bytes.lock().await.clone();
        if wire.dense.load(Ordering::SeqCst) {
            if request.uri().path()=="/threads/t1" {return axum::Json(json!({"id":"t1","messages":[{"id":"m1"},{"id":"m2"},{"id":"m3"}]}));}
            let id=request.uri().path().strip_prefix("/messages/").unwrap();
            assert!(matches!(id,"m1"|"m2"|"m3"));
            let name="\"\\".repeat(128);
            let attachment_id="A".repeat(2048);
            let addresses=(0..100).map(|i|format!("{}{}@example.test","a".repeat(50),i)).collect::<Vec<_>>().join(", ");
            let mut parts=vec![json!({"partId":"0","mimeType":"text/plain","body":{"size":bytes.len(),"data":URL_SAFE_NO_PAD.encode(&bytes)}})];
            parts.extend((0..62).map(|i|json!({"partId":format!("{i}.{}","1".repeat(120)),"mimeType":"application/pdf","filename":name,"body":{"size":10,"attachmentId":attachment_id}})));
            return axum::Json(json!({"id":id,"threadId":"t1","labelIds":(0..128).map(|_|"L".repeat(256)).collect::<Vec<_>>(),"payload":{"partId":"","mimeType":"multipart/mixed","headers":[{"name":"Subject","value":"\"\\".repeat(1024)},{"name":"Message-ID","value":format!("<{}@example.test>","m".repeat(980))},{"name":"From","value":addresses},{"name":"To","value":addresses},{"name":"Cc","value":addresses},{"name":"Bcc","value":addresses}],"parts":parts}}));
        }

        let value=if request.uri().path()=="/messages/m1/attachments/a1" {
            if wire.hold.load(Ordering::SeqCst) {wire.entered.add_permits(1);let permit=wire.release.acquire().await.unwrap();permit.forget();}
            json!({"size":bytes.len(),"data":URL_SAFE_NO_PAD.encode(&bytes)})
        }else {assert_eq!(request.uri().path(),"/messages/m1");assert!(request.uri().query().unwrap().contains("format=full"));json!({"id":"m1","threadId":"t1","labelIds":["INBOX"],"payload":{"partId":"","mimeType":"multipart/mixed","headers":[{"name":"Subject","value":"Synthetic attachment"}],"parts":[{"partId":"0","mimeType":"text/plain","body":{"size":4,"data":"Ym9keQ"}},{"partId":"1","mimeType":wire.mime,"filename":"synthetic-file","body":{"size":bytes.len(),"attachmentId":"a1"}}]}})};
        axum::Json(value)
    }}));
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (changes, _) = tokio::sync::watch::channel(0);
    let signals = crate::Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(tokio::sync::Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mail = crate::mail::MailService::new(store.clone(), Some(oauth), signals)
        .with_synthetic_google(address);
    (mail, store, user, thread, connection.id, wire, task, dir)
}
fn request(connection: &str, start: u32, digest: Option<String>) -> MailToolRequest {
    MailToolRequest::Attachment {
        connection_id: connection.into(),
        message_id: "m1".into(),
        part_id: "1".into(),
        attachment_id: Some("a1".into()),
        start_byte: start,
        max_bytes: 4096,
        expected_digest: digest,
    }
}

#[tokio::test]
async fn gmail_attachment_real_receipt_pages_digest_and_disconnect_revoke_private_output() {
    let _guard = TEST_LOCK.lock().await;
    let text = "\"\\\n".repeat(3500);
    let (mail, store, user, thread, connection, wire, task, _dir) =
        fixture(text.into_bytes(), "text/plain").await;
    let first = mail
        .read(
            &user,
            &request(&connection, 0, None),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(first["status"], "ok");
    assert!(first["partial"].as_bool().unwrap());
    assert!(
        serde_json::to_vec(&KnowledgeToolResult::External(first.clone()))
            .unwrap()
            .len()
            <= 12 * 1024
    );
    let next = first["next_start_byte"].as_u64().unwrap() as u32;
    let digest = first["page_digest"].as_str().unwrap().to_owned();
    let page = mail
        .read(
            &user,
            &request(&connection, next, Some(digest.clone())),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(page["start_byte"], next);
    *wire.bytes.lock().await = b"different document".to_vec();
    assert_eq!(
        mail.read(
            &user,
            &request(&connection, next, Some(digest)),
            &CancellationToken::new()
        )
        .await
        .unwrap()["status"],
        "changed_restart_read"
    );
    store
        .submit_turn(
            &user,
            thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Read synthetic attached text".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"synthetic":true}"#.to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "attachment".into(),
                input: KnowledgeToolInput::External {
                    request: ExternalToolRequest::Mail(request(&connection, 0, None)),
                },
            }],
        )
        .await
        .unwrap();
    let ExternalToolStart::Dispatch(permit) = store
        .prepare_external_tool(&step, "attachment".into())
        .await
        .unwrap()
    else {
        panic!("expected IO permit")
    };
    let output = mail.external(&permit, &CancellationToken::new()).await;
    let saved = store
        .complete_external_tool(&permit, output.clone())
        .await
        .unwrap();
    let KnowledgeToolResult::External(value) = &saved else {
        panic!("expected successful receipt")
    };
    assert_eq!(value["status"], "ok");
    assert_eq!(value["text"], "different document");
    assert_eq!(value["message_id"], "m1");
    assert_eq!(value["attachment"]["part_id"], "1");
    assert!(value["content_digest"].is_string() && value["text_digest"].is_string());
    let calls = wire.requests.load(Ordering::SeqCst);
    store.complete_external_tool(&permit, output).await.unwrap();
    assert_eq!(wire.requests.load(Ordering::SeqCst), calls);
    let context = store.agent_context(&claim.lease).await.unwrap();
    let terminal = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &terminal,
            PrivateCheckpoint {
                bytes: br#"{"synthetic_final":true}"#.to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    store
        .append_agent_output(
            &terminal,
            vec![moosuri_store::OutputPatch::Append {
                item: "answer".into(),
                text: "different document".into(),
                phase: None,
                refusal: false,
            }],
        )
        .await
        .unwrap();
    store
        .finish_agent_run(&terminal, RunState::Completed, None)
        .await
        .unwrap();
    store.disconnect_mail(&user, connection).await.unwrap();
    let history = store.conversation(&user, thread, None).await.unwrap();
    assert!(history.turns[0].assistant.is_empty());
    assert!(store.agent_context(&claim.lease).await.is_err());
    task.abort();
}
#[tokio::test]
async fn gmail_attachment_disconnect_during_download_drops_late_private_text_and_parser_rejects_forged_fields()
 {
    let _guard = TEST_LOCK.lock().await;
    let (mail, store, user, _thread, connection, wire, task, _dir) =
        fixture(b"late secret".to_vec(), "text/plain").await;
    wire.hold.store(true, Ordering::SeqCst);
    let mail_user = user.clone();
    let id = connection.clone();
    let read = tokio::spawn(async move {
        mail.read(
            &mail_user,
            &request(&id, 0, None),
            &CancellationToken::new(),
        )
        .await
    });
    let entered = wire.entered.acquire().await.unwrap();
    entered.forget();
    store.disconnect_mail(&user, connection).await.unwrap();
    wire.release.add_permits(1);
    assert!(read.await.unwrap().is_err());
    assert!(crate::mail_tools::parse("mail_attachment",r#"{"connection_id":"c","message_id":"m","part_id":"1","attachment_id":"a","start_byte":0,"max_bytes":4096,"expected_digest":null,"url":"https://evil.test"}"#).is_none());
    task.abort();
}
fn synthetic_pdf(text: &str) -> Vec<u8> {
    let content = format!("BT /F1 12 Tf 20 200 Td ({text}) Tj ET");
    let objects=["<< /Type /Catalog /Pages 2 0 R >>".to_owned(),"<< /Type /Pages /Kids [3 0 R] /Count 1 /MediaBox [0 0 300 300] /Resources << /Font << /F1 4 0 R >> >> >>".into(),"<< /Type /Page /Parent 2 0 R /Contents 5 0 R >>".into(),"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".into(),format!("<< /Length {} >>\nstream\n{content}\nendstream",content.len())];
    let mut pdf = b"%PDF-1.5\n".to_vec();
    let mut offsets = vec![0];
    for (i, object) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{object}\nendobj\n", i + 1).as_bytes());
    }
    pdf.extend_from_slice(format!("%{}\n", "x".repeat(600 * 1024)).as_bytes());
    let xref = pdf.len();
    pdf.extend_from_slice(b"xref\n0 6\n0000000000 65535 f \n");
    for offset in offsets.iter().skip(1) {
        pdf.extend_from_slice(format!("{offset:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(
        format!("trailer\n<< /Size 6 /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n").as_bytes(),
    );
    pdf
}
#[tokio::test]
#[ignore = "requires built deployment PDF child in MOOSURI_TEST_PDF_EXECUTABLE; synthetic IO only"]
async fn gmail_attachment_large_pdf_uses_real_fixed_child_and_reports_no_text_invalid_and_unavailable()
 {
    let _guard = TEST_LOCK.lock().await;
    let path = std::path::PathBuf::from(
        std::env::var_os("MOOSURI_TEST_PDF_EXECUTABLE").expect("configured deployment executable"),
    );
    let bytes = synthetic_pdf("Synthetic receipt amount 37");
    assert!(bytes.len() > 512 * 1024);
    let (mail, store, user, _thread, connection, wire, task, _dir) =
        fixture(bytes, "application/pdf").await;
    let body = mail
        .read(
            &user,
            &MailToolRequest::Read {
                connection_id: connection.clone(),
                message_id: "m1".into(),
            },
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(body["mail"]["text"], "body");
    let unavailable = mail
        .read(
            &user,
            &request(&connection, 0, None),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(unavailable["status"], "extractor_unavailable");
    // Exercise the same deployment-owned runtime configuration used by
    // Config::start_runtime, rather than injecting an extraction result.
    let oauth = Arc::new(OAuthClient {
        client_id: "synthetic.apps.googleusercontent.com".into(),
        client_secret: SecretString::new("synthetic-client-secret".into()).unwrap(),
        redirect_uri: "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
    });
    let runtime = Runtime::start_with_google_setup_claude_pdf(
        store.clone(),
        Some(oauth),
        None,
        None,
        "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
        None,
        Some(path),
    );
    let mail = runtime
        .mail_service()
        .with_synthetic_google(*wire.address.get().unwrap());
    let value = mail
        .read(
            &user,
            &request(&connection, 0, None),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(value["status"], "ok");
    assert_eq!(value["pages"], 1);
    assert!(
        value["text"]
            .as_str()
            .unwrap()
            .contains("Synthetic receipt amount 37")
    );
    *wire.bytes.lock().await = synthetic_pdf("");
    assert_eq!(
        mail.read(
            &user,
            &request(&connection, 0, None),
            &CancellationToken::new()
        )
        .await
        .unwrap()["status"],
        "no_text"
    );
    *wire.bytes.lock().await = b"%PDF-invalid".to_vec();
    assert_eq!(
        mail.read(
            &user,
            &request(&connection, 0, None),
            &CancellationToken::new()
        )
        .await
        .unwrap()["status"],
        "invalid_document"
    );
    assert_eq!(
        store.mail_connections(&user).await.unwrap()[0].status,
        "active"
    );
    runtime.shutdown().await;
    task.abort();
}

#[tokio::test]
async fn gmail_attachment_unsupported_metadata_never_downloads_and_continuation_requires_digest() {
    let _guard = TEST_LOCK.lock().await;
    let (mail, _store, user, _thread, connection, wire, task, _dir) =
        fixture(b"not downloaded".to_vec(), "application/zip").await;
    let value = mail
        .read(
            &user,
            &request(&connection, 0, None),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    assert_eq!(value["status"], "unsupported_type");
    assert!(value.get("text").is_none());
    assert_eq!(wire.requests.load(Ordering::SeqCst), 1);
    assert!(
        mail.read(
            &user,
            &request(&connection, 1, None),
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(wire.requests.load(Ordering::SeqCst), 1);
    task.abort();
}

#[tokio::test]
async fn gmail_attachment_dense_mail_and_three_message_thread_complete_real_store_receipts() {
    let _guard = TEST_LOCK.lock().await;
    let (mail, store, user, thread, connection, wire, task, _dir) =
        fixture("\"\\\n".repeat(5000).into_bytes(), "text/plain").await;
    wire.dense.store(true, Ordering::SeqCst);
    let read = MailToolRequest::Read {
        connection_id: connection.clone(),
        message_id: "m1".into(),
    };
    let full = mail
        .read(&user, &read, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(full["attachments"].as_array().unwrap().len(), 62);
    assert!(
        serde_json::to_vec(&full).unwrap().len() > 16 * 1024,
        "fixture must exceed the Store cap before model projection"
    );
    store
        .submit_turn(
            &user,
            thread,
            uuid::Uuid::new_v4().to_string(),
            "Synthetic dense mail read".into(),
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let step = store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    let requests = [
        ("read", read),
        (
            "thread",
            MailToolRequest::Thread {
                connection_id: connection.clone(),
                thread_id: "t1".into(),
            },
        ),
        (
            "list",
            MailToolRequest::Attachments {
                connection_id: connection,
                message_id: "m1".into(),
                start_index: 0,
            },
        ),
    ];
    store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: br#"{"synthetic_dense":true}"#.to_vec(),
            },
            requests
                .into_iter()
                .map(|(id, request)| PendingKnowledgeCall {
                    call_id: id.into(),
                    input: KnowledgeToolInput::External {
                        request: ExternalToolRequest::Mail(request),
                    },
                })
                .collect(),
        )
        .await
        .unwrap();
    for id in ["read", "thread", "list"] {
        let ExternalToolStart::Dispatch(permit) =
            store.prepare_external_tool(&step, id.into()).await.unwrap()
        else {
            panic!("read permit")
        };
        let outcome = mail.external(&permit, &CancellationToken::new()).await;
        assert!(
            serde_json::to_vec(&outcome).unwrap().len() <= 12 * 1024,
            "whole escaped {id} outcome"
        );
        let saved = store
            .complete_external_tool(&permit, outcome)
            .await
            .unwrap();
        let KnowledgeToolResult::External(value) = saved else {
            panic!("{id} must persist a real external result, not TooLargeNarrowRead")
        };
        if id == "list" {
            let attachments = value["attachments"].as_array().unwrap();
            assert!(!attachments.is_empty());
            assert_eq!(
                value["next_start_index"].as_u64().unwrap(),
                attachments.len() as u64
            );
            assert_eq!(
                attachments[0]["attachment_id"].as_str().unwrap().len(),
                2048
            );
        } else {
            let messages = if id == "thread" {
                let messages = value["messages"].as_array().unwrap();
                assert_eq!(messages.len(), 3);
                messages.clone()
            } else {
                vec![value]
            };
            for (i, message) in messages.iter().enumerate() {
                assert_eq!(message["id"], format!("m{}", i + 1));
                assert_eq!(message["thread_id"], "t1");
                assert_eq!(message["attachment_total"], 62);
                assert_eq!(message["partial"], true);
                assert_eq!(message["mail"]["text_truncated"], true);
                assert_eq!(message["mail"]["headers_truncated"], true);
                let attachments = message["attachments"].as_array().unwrap();
                assert_eq!(
                    message["attachments_next_start_index"].as_u64().unwrap(),
                    attachments.len() as u64
                );
                let names = message["mail"]["attachment_names"].as_array().unwrap();
                assert_eq!(names.len(), attachments.len());
                for (attachment, name) in attachments.iter().zip(names) {
                    assert_eq!(attachment["filename"], *name);
                    assert_eq!(attachment["attachment_id"].as_str().unwrap().len(), 2048);
                    assert_eq!(attachment["part_id"].as_str().unwrap().len(), 122);
                }
            }
        }
    }
    let context = store.agent_context(&claim.lease).await.unwrap();
    assert!(context.calls.iter().all(|slot| slot.result.is_some()));
    task.abort();
}
