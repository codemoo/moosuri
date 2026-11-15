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
    extra_raw: StdMutex<std::collections::BTreeMap<String, String>>,
    fail_before_write: AtomicBool,
    writes: AtomicUsize,
    sends: AtomicUsize,
    gets: AtomicUsize,
    block: AtomicBool,
    lose_write_ack: AtomicBool,
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
            {
                let mut drafts = vec![
                    serde_json::json!({"id":"draft1","message":{"id":"msg1","threadId":"thread1"}}),
                ];
                drafts.extend(s.extra_raw.lock().unwrap().keys().map(|id| serde_json::json!({"id":id,"message":{"id":format!("msg-{id}"),"threadId":"thread1"}})));
                serde_json::json!({"drafts":drafts})
            }
        } else {
            {
                let id = path.trim_start_matches("/drafts/");
                let raw = if id == "draft1" {
                    s.raw.lock().unwrap().clone()
                } else {
                    s.extra_raw.lock().unwrap().get(id).unwrap().clone()
                };
                serde_json::json!({"id":id,"message":{"raw":raw}})
            }
        }
    } else {
        assert!(method == Method::POST || method == Method::PUT);
        let value: serde_json::Value =
            serde_json::from_slice(&to_bytes(r.into_body(), 1000000).await.unwrap()).unwrap();
        s.writes.fetch_add(1, Ordering::SeqCst);
        if s.fail_before_write.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        let id = if method == Method::POST {
            "draft1"
        } else {
            path.trim_start_matches("/drafts/")
        };
        let raw = value["message"]["raw"].as_str().unwrap().to_string();
        if id == "draft1" {
            *s.raw.lock().unwrap() = raw;
        } else {
            s.extra_raw.lock().unwrap().insert(id.into(), raw);
        }
        if s.lose_write_ack.load(Ordering::SeqCst) {
            return StatusCode::SERVICE_UNAVAILABLE.into_response();
        }
        serde_json::json!({"id":id,"message":{"id":if id == "draft1" { "msg1".to_string() } else { format!("msg-{id}") },"threadId":"thread1"}})
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
        extra_raw: StdMutex::new(Default::default()),
        fail_before_write: AtomicBool::new(false),
        writes: AtomicUsize::new(0),
        sends: AtomicUsize::new(0),
        gets: AtomicUsize::new(0),
        block: AtomicBool::new(false),
        lose_write_ack: AtomicBool::new(false),
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

#[tokio::test]
async fn provider_draft_same_intent_returns_receipt_without_second_post() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, store, user, mail, a, s, server) = setup(true).await;
    let intent = GmailDraftSave {
        expected: approval(&a),
        draft_id: None,
        expected_provider_digest: None,
    };
    let first = mail
        .save_gmail_draft(
            &user,
            a.id.clone(),
            intent.clone(),
            &CancellationToken::new(),
        )
        .await
        .unwrap();
    let replay = mail
        .save_gmail_draft(&user, a.id.clone(), intent, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(first.id, replay.id);
    assert_eq!(first.message_id, replay.message_id);
    assert_eq!(s.writes.load(Ordering::SeqCst), 1);
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    assert_eq!(
        store.mail_action(&user, a.id).await.unwrap().state,
        "pending_approval"
    );
    server.abort();
}

#[tokio::test]
async fn provider_draft_lost_ack_fences_retry_and_edited_intent() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, store, user, mail, a, s, server) = setup(true).await;
    s.lose_write_ack.store(true, Ordering::SeqCst);
    let intent = GmailDraftSave {
        expected: approval(&a),
        draft_id: None,
        expected_provider_digest: None,
    };
    assert!(
        mail.save_gmail_draft(
            &user,
            a.id.clone(),
            intent.clone(),
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    s.lose_write_ack.store(false, Ordering::SeqCst);
    *s.raw.lock().unwrap() = URL_SAFE_NO_PAD.encode(b"From: sender@example.test\r\nTo: recipient@example.test\r\nSubject: Externally changed\r\nContent-Type: text/plain\r\n\r\nChanged body\r\n");
    assert!(
        mail.save_gmail_draft(&user, a.id.clone(), intent, &CancellationToken::new())
            .await
            .is_err()
    );
    let mut content = a.content.unwrap();
    content.text = "A changed synthetic draft".into();
    let changed = store
        .create_mail_draft(
            &user,
            MailDraftInput {
                connection_id: a.connection_id,
                content,
                origin_thread_id: None,
            },
            Some((a.id, a.revision)),
        )
        .await
        .unwrap();
    assert!(
        mail.save_gmail_draft(
            &user,
            changed.id.clone(),
            GmailDraftSave {
                expected: approval(&changed),
                draft_id: None,
                expected_provider_digest: None
            },
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    assert_eq!(s.writes.load(Ordering::SeqCst), 1);
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    server.abort();
}

#[tokio::test]
async fn provider_draft_lost_ack_reads_back_exact_original_without_posting() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, _store, user, mail, a, s, server) = setup(true).await;
    let intent = GmailDraftSave {
        expected: approval(&a),
        draft_id: None,
        expected_provider_digest: None,
    };
    s.lose_write_ack.store(true, Ordering::SeqCst);
    assert!(
        mail.save_gmail_draft(
            &user,
            a.id.clone(),
            intent.clone(),
            &CancellationToken::new()
        )
        .await
        .is_err()
    );
    s.lose_write_ack.store(false, Ordering::SeqCst);
    let receipt = mail
        .save_gmail_draft(&user, a.id.clone(), intent, &CancellationToken::new())
        .await
        .unwrap();
    assert_eq!(receipt.id, "draft1");
    assert_eq!(s.writes.load(Ordering::SeqCst), 1);
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    server.abort();
}

/// The same local approval may prepare several different provider operations.
/// A receipt for one must never settle another operation's unknown outcome.
#[tokio::test]
async fn provider_draft_earlier_put_cannot_settle_unknown_other_put_or_post() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    for update in [true, false] {
        let (_dir, _store, user, mail, a, s, server) = setup(true).await;
        let original = URL_SAFE_NO_PAD.encode(b"From: sender@example.test\r\nTo: recipient@example.test\r\nSubject: Original\r\nContent-Type: text/plain\r\n\r\nOriginal body\r\n");
        *s.raw.lock().unwrap() = original.clone();
        s.extra_raw
            .lock()
            .unwrap()
            .insert("draft2".into(), original.clone());
        let cancel = CancellationToken::new();
        let first = mail
            .gmail_draft(&user, &a.connection_id, "draft1", &cancel)
            .await
            .unwrap();
        mail.save_gmail_draft(
            &user,
            a.id.clone(),
            GmailDraftSave {
                expected: approval(&a),
                draft_id: Some(first.id),
                expected_provider_digest: Some(first.digest),
            },
            &cancel,
        )
        .await
        .unwrap();
        let prior_raw = s.raw.lock().unwrap().clone();
        let second = mail
            .gmail_draft(&user, &a.connection_id, "draft2", &cancel)
            .await
            .unwrap();
        let intent = GmailDraftSave {
            expected: approval(&a),
            draft_id: update.then_some(second.id),
            expected_provider_digest: update.then_some(second.digest),
        };
        s.fail_before_write.store(true, Ordering::SeqCst);
        assert!(
            mail.save_gmail_draft(&user, a.id.clone(), intent.clone(), &cancel)
                .await
                .is_err()
        );
        s.fail_before_write.store(false, Ordering::SeqCst);
        assert!(
            mail.save_gmail_draft(&user, a.id.clone(), intent, &cancel)
                .await
                .is_err(),
            "an earlier PUT's raw MIME cannot prove a different PUT or POST completed"
        );
        assert_eq!(*s.raw.lock().unwrap(), prior_raw);
        assert_eq!(s.extra_raw.lock().unwrap()["draft2"], original);
        assert_eq!(
            s.writes.load(Ordering::SeqCst),
            2,
            "no repeat external write"
        );
        assert_eq!(s.sends.load(Ordering::SeqCst), 0);
        server.abort();
    }
}

#[tokio::test]
async fn compound_mission_explicit_provider_save_reconciles_original_post_without_send_authority() {
    let _serial = crate::tests::TEST_LOCK.lock().await;
    let (_dir, store, user, mail, a, s, server) = setup(true).await;
    let provider = store
        .create_provider(
            &user,
            moosuri_store::ProviderChange {
                name: "Synthetic compound model".into(),
                config: moosuri_core::ProviderConfig {
                    kind: moosuri_core::ProviderKind::Compatible,
                    base_url: Some("http://127.0.0.1:11434/v1".into()),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                enabled: true,
                credential: moosuri_store::CredentialChange::Clear,
            },
        )
        .await
        .unwrap();
    store
        .record_model_probe(&user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .record_tool_probe(&user, provider.id.clone(), 1, "synthetic".into(), None)
        .await
        .unwrap();
    store
        .set_default_model(
            &user,
            Some(moosuri_core::ModelSelection {
                source: Default::default(),
                provider_id: provider.id,
                provider_revision: 1,
                model: "synthetic".into(),
                effort: None,
            }),
        )
        .await
        .unwrap();
    let task =
        "Read Gmail and prepare an email draft to recipient@example.test and save Gmail draft";
    store
        .create_mission(
            &user,
            moosuri_core::MissionCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                task: task.into(),
                constraints: String::new(),
                checks: vec![moosuri_core::MissionCheck::UserReview {
                    description: "Review the draft".into(),
                }],
                execution_mode: moosuri_core::MissionMode::Durable,
            },
        )
        .await
        .unwrap();
    let claim = store.claim_agent_run().await.unwrap().unwrap();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let st = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    let mut content = a.content.unwrap();
    content.bcc.clear();
    store
        .commit_agent_step(
            &st,
            moosuri_store::PrivateCheckpoint {
                bytes: br#"{"kind":"mission_receipt_checkpoint"}"#.to_vec(),
            },
            vec![moosuri_store::PendingKnowledgeCall {
                call_id: "prepare".into(),
                input: moosuri_store::KnowledgeToolInput::MissionAction {
                    request: moosuri_core::MissionActionRequest::PrepareMail {
                        intent_quote: task.into(),
                        input: MailDraftInput {
                            connection_id: a.connection_id,
                            origin_thread_id: None,
                            content,
                        }
                        .into(),
                        evidence: vec![],
                    },
                },
            }],
        )
        .await
        .unwrap();
    let moosuri_store::KnowledgeToolResult::External(prepared) = store
        .execute_knowledge_tool(&st, "prepare".into())
        .await
        .unwrap()
    else {
        panic!("actual local draft required")
    };
    let id = prepared["action_id"].as_str().unwrap().to_owned();
    let original = store.mail_action(&user, id.clone()).await.unwrap();
    let context = store.agent_context(&claim.lease).await.unwrap();
    let st = store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    store
        .commit_agent_step(
            &st,
            moosuri_store::PrivateCheckpoint {
                bytes: br#"{"kind":"mission_receipt_checkpoint"}"#.to_vec(),
            },
            vec![moosuri_store::PendingKnowledgeCall {
                call_id: "save-provider".into(),
                input: moosuri_store::KnowledgeToolInput::MissionAction {
                    request: moosuri_core::MissionActionRequest::SaveProviderDraft {
                        intent_quote: task.into(),
                        action_id: id.clone(),
                        expected: approval(&original),
                    },
                },
            }],
        )
        .await
        .unwrap();
    let permit = match store
        .prepare_mission_action(&st, "save-provider".into())
        .await
        .unwrap()
    {
        moosuri_store::MissionActionStart::Dispatch(p) => p,
        _ => panic!("mission provider permit required"),
    };
    let cancel = CancellationToken::new();
    s.lose_write_ack.store(true, Ordering::SeqCst);
    assert!(
        mail.save_mission_provider_draft(&permit, &cancel)
            .await
            .is_err()
    );
    s.lose_write_ack.store(false, Ordering::SeqCst);
    let receipt = mail
        .save_mission_provider_draft(&permit, &cancel)
        .await
        .unwrap();
    store
        .complete_mission_action(&permit, receipt)
        .await
        .unwrap();
    assert_eq!(s.writes.load(Ordering::SeqCst), 1);
    assert_eq!(s.sends.load(Ordering::SeqCst), 0);
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    let current = store.mail_action(&user, id).await.unwrap();
    assert_eq!(current.state, "pending_approval");
    assert_eq!(current.digest, original.digest);
    assert_eq!(current.revision, original.revision);
    assert!(
        store.missions(&user).await.unwrap()[0].actions[0]
            .provider_receipt
            .is_some()
    );
    server.abort();
}
