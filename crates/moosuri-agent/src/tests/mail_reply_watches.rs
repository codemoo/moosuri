//! Synthetic provider + real Store/agent/chat admission. No live mailbox or model.
use super::*;
use axum::response::IntoResponse;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_connectors::gmail::{OAuthClient, SecretString, TokenSet};
use moosuri_core::{ExternalToolRequest, MailToolRequest};
use moosuri_core::{MailApproval, MailDraftContent, MailDraftInput, MailReplyWatchCreate};
use moosuri_store::GoogleAppPin;
struct Wire {
    mode: AtomicU8,
    probes: AtomicUsize,
    bodies: AtomicUsize,
    models: AtomicUsize,
    entered: Semaphore,
    release: Semaphore,
}
struct Fixture {
    mail: crate::mail::MailService,
    store: Store,
    user: UserContext,
    thread: String,
    connection: String,
    wire: Arc<Wire>,
    server: JoinHandle<()>,
    _dir: tempfile::TempDir,
}
async fn fixture(mode: u8) -> Fixture {
    let wire = Arc::new(Wire {
        mode: AtomicU8::new(mode),
        probes: AtomicUsize::new(0),
        bodies: AtomicUsize::new(0),
        models: AtomicUsize::new(0),
        entered: Semaphore::new(0),
        release: Semaphore::new(0),
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let handler = wire.clone();
    let app=Router::new().fallback(axum::routing::any(move|request:axum::extract::Request|{let w=handler.clone();async move {
        if request.uri().path().starts_with("/v1/"){w.models.fetch_add(1,Ordering::SeqCst);return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response()}
        assert_eq!(request.method(),"GET");let mode=w.mode.load(Ordering::SeqCst);
        let value=if request.uri().path()=="/threads/t1" {
            w.probes.fetch_add(1,Ordering::SeqCst);let q=request.uri().query().unwrap();assert!(q.contains("format=metadata"));assert!(!q.contains("format=full"));
            if mode==6{return axum::http::StatusCode::SERVICE_UNAVAILABLE.into_response()}
            let item=|id:&str,date:&str,labels:Vec<&str>|json!({"id":id,"threadId":"t1","labelIds":labels,"internalDate":date});
            let mut items=vec![item("m0","1000",vec!["SENT"])];
            match mode {
                1|8|9=>items.push(item("m1","2000",vec!["INBOX"])),
                2=>{items.push(item("m2","2000",vec!["SENT"]));items.push(item("m3","3000",vec!["DRAFT"]));},
                3=>items.push(item("old","900",vec!["INBOX"])),
                4=>items[0]["labelIds"]=json!(["INBOX","UNREAD"]),
                5=>items.push(items[0].clone()),
                7=>{for n in 1..=5 {items.push(item(&format!("m{n}"),"2000",vec!["INBOX"]));}},
                10=>items[0].as_object_mut().unwrap().remove("internalDate").map(|_|()).unwrap(),
                _=>{},
            }
            json!({"id":"t1","messages":items})
        }else {
            assert!(matches!(request.uri().path(),"/messages/m1"|"/messages/m0"));assert!(request.uri().query().unwrap().contains("format=full"));w.bodies.fetch_add(1,Ordering::SeqCst);
            if mode==8 {w.entered.add_permits(1);w.release.acquire().await.unwrap().forget();}
            let id=request.uri().path().trim_start_matches("/messages/");
            let body="Synthetic vendor reply: stock arrives Friday. <script>untrusted</script>";
            json!({"id":id,"threadId":if mode==9{"other-thread"}else{"t1"},"labelIds":["INBOX"],"payload":{"partId":"","mimeType":"text/plain","headers":[{"name":"Subject","value":"Synthetic vendor reply"},{"name":"From","value":"vendor@example.test"}],"body":{"size":body.len(),"data":URL_SAFE_NO_PAD.encode(body)}}})
        };
        axum::Json(value).into_response()
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, thread) =
        super::knowledge::tool_configured(&format!("http://{address}/v1")).await;
    let oauth = Arc::new(OAuthClient {
        client_id: "synthetic.apps.googleusercontent.com".into(),
        client_secret: SecretString::new("synthetic-secret".into()).unwrap(),
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
    let c = store
        .complete_mail_oauth(
            exchange,
            TokenSet {
                access_token: SecretString::new("synthetic-access".into()).unwrap(),
                refresh_token: Some(SecretString::new("synthetic-refresh".into()).unwrap()),
                expires_in: 3600,
                scopes: None,
            },
            "owner@example.test".into(),
        )
        .await
        .unwrap();
    let action = store
        .create_mail_draft(
            &user,
            MailDraftInput {
                connection_id: c.id.clone(),
                content: MailDraftContent {
                    from: c.email,
                    to: vec!["vendor@example.test".into()],
                    cc: vec![],
                    bcc: vec![],
                    subject: "Synthetic stock question".into(),
                    text: "Is it available?".into(),
                    reply: None,
                },
                origin_thread_id: Some(thread.clone()),
            },
            None,
        )
        .await
        .unwrap();
    store
        .decide_mail(
            &user,
            action.id,
            MailApproval {
                revision: action.revision,
                digest: action.digest,
            },
            true,
        )
        .await
        .unwrap();
    let dispatch = store.next_mail_dispatch().await.unwrap().unwrap();
    store
        .finish_mail_dispatch(&dispatch, Some(("m0".into(), "t1".into())), false)
        .await
        .unwrap();
    let (changes, _) = watch::channel(0);
    let signals = Signals {
        stop: CancellationToken::new(),
        wake: Arc::new(Notify::new()),
        changes,
        delivery: Arc::new(tokio::sync::RwLock::new(())),
    };
    let mail = crate::mail::MailService::new(store.clone(), Some(oauth), signals)
        .with_synthetic_google(address);
    Fixture {
        mail,
        store,
        user,
        thread,
        connection: c.id,
        wire,
        server,
        _dir: dir,
    }
}
async fn start(f: &Fixture) -> moosuri_core::MailReplyWatch {
    let inventory = f
        .store
        .mail_reply_watches(&f.user, Some(f.thread.clone()))
        .await
        .unwrap();
    assert_eq!(inventory.targets.len(), 1);
    f.store
        .create_mail_reply_watch(
            &f.user,
            MailReplyWatchCreate {
                request_id: uuid::Uuid::new_v4().to_string(),
                thread_id: f.thread.clone(),
                target_id: inventory.targets[0].id.clone(),
                cadence_seconds: 1800,
                duration_days: 7,
            },
        )
        .await
        .unwrap()
}
#[tokio::test]
async fn reply_watch_unchanged_label_only_old_mail_and_outbound_use_one_probe_no_body_no_model() {
    let _g = TEST_LOCK.lock().await;
    for mode in [0, 2, 3, 4] {
        let f = fixture(mode).await;
        start(&f).await;
        let lease = f.store.next_mail_reply_watch().await.unwrap().unwrap();
        f.mail
            .poll_reply_watch(&lease, &CancellationToken::new())
            .await;
        let w = f
            .store
            .mail_reply_watches(&f.user, Some(f.thread.clone()))
            .await
            .unwrap()
            .watches
            .remove(0);
        assert_eq!(w.state, "active");
        assert!(w.last_checked_at.is_some());
        assert!(w.reason.is_none());
        assert_eq!(f.wire.probes.load(Ordering::SeqCst), 1);
        assert_eq!(f.wire.bodies.load(Ordering::SeqCst), 0);
        assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
        assert!(f.store.claim_agent_run().await.unwrap().is_none());
        assert!(f.store.next_mail_reply_watch().await.unwrap().is_none());
        f.server.abort();
    }
}
#[tokio::test]
async fn reply_watch_chat_registration_finds_reply_before_first_poll_once_without_model() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture(1).await;
    let turn = f
        .store
        .submit_turn(
            &f.user,
            f.thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "답장 오면 알려줘".into(),
        )
        .await
        .unwrap();
    assert_eq!(turn.run.state, RunState::Completed);
    assert!(
        turn.assistant
            .iter()
            .any(|a| a.text.contains("등록되어 있어요"))
    );
    let lease = f.store.next_mail_reply_watch().await.unwrap().unwrap();
    f.mail
        .poll_reply_watch(&lease, &CancellationToken::new())
        .await;
    f.mail
        .poll_reply_watch(&lease, &CancellationToken::new())
        .await;
    let list = f
        .store
        .mail_reply_watches(&f.user, Some(f.thread.clone()))
        .await
        .unwrap();
    assert_eq!(list.watches[0].state, "replied");
    assert!(
        list.watches[0].replies[0]
            .text
            .contains("stock arrives Friday")
    );
    assert_eq!(
        f.store
            .notifications(&f.user, None, false)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(f.wire.probes.load(Ordering::SeqCst), 1);
    assert_eq!(f.wire.bodies.load(Ordering::SeqCst), 1);
    assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
    assert!(f.store.claim_agent_run().await.unwrap().is_none());
    f.server.abort();
}
#[tokio::test]
async fn reply_watch_failed_partial_and_mismatched_provider_reads_never_mean_unchanged() {
    let _g = TEST_LOCK.lock().await;
    for mode in [5, 6, 7, 9, 10] {
        let f = fixture(mode).await;
        start(&f).await;
        let lease = f.store.next_mail_reply_watch().await.unwrap().unwrap();
        f.mail
            .poll_reply_watch(&lease, &CancellationToken::new())
            .await;
        let w = f
            .store
            .mail_reply_watches(&f.user, Some(f.thread.clone()))
            .await
            .unwrap()
            .watches
            .remove(0);
        assert_eq!(
            w.state,
            if mode == 6 { "active" } else { "blocked" },
            "mode {mode}"
        );
        assert!(w.last_checked_at.is_none());
        assert!(w.replies.is_empty());
        assert!(w.reason.is_some());
        assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
        f.server.abort();
    }
}
#[tokio::test]
async fn reply_watch_stop_or_disconnect_during_body_read_fences_preview_and_notification() {
    let _g = TEST_LOCK.lock().await;
    for disconnect in [false, true] {
        let f = fixture(8).await;
        let w = start(&f).await;
        let lease = f.store.next_mail_reply_watch().await.unwrap().unwrap();
        let mail = f.mail.clone();
        let worker = tokio::spawn(async move {
            mail.poll_reply_watch(&lease, &CancellationToken::new())
                .await
        });
        tokio::time::timeout(Duration::from_secs(3), f.wire.entered.acquire())
            .await
            .unwrap()
            .unwrap()
            .forget();
        if disconnect {
            f.store
                .disconnect_mail(&f.user, f.connection.clone())
                .await
                .unwrap()
        } else {
            f.store.stop_mail_reply_watch(&f.user, w.id).await.unwrap();
        }
        f.wire.release.add_permits(1);
        worker.await.unwrap();
        assert!(
            f.store
                .notifications(&f.user, None, false)
                .await
                .unwrap()
                .items
                .is_empty()
        );
        let inventory = f
            .store
            .mail_reply_watches(&f.user, Some(f.thread.clone()))
            .await
            .unwrap();
        assert!(inventory.watches[0].replies.is_empty());
        assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
        f.server.abort();
    }
}
#[tokio::test]
async fn reply_watch_target_requires_completed_actual_read_and_chat_stop_is_local() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture(0).await;
    read_message(&f, "m0").await;
    let targets = f
        .store
        .mail_reply_watches(&f.user, Some(f.thread.clone()))
        .await
        .unwrap();
    assert_eq!(targets.targets[0].title, "Synthetic vendor reply");
    start(&f).await;
    let turn = f
        .store
        .submit_turn(
            &f.user,
            f.thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "메일 감시 중지".into(),
        )
        .await
        .unwrap();
    assert_eq!(turn.run.state, RunState::Completed);
    assert!(f.store.next_mail_reply_watch().await.unwrap().is_none());
    assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
    f.server.abort();
}

async fn read_message(f: &Fixture, id: &str) {
    use moosuri_store::{
        ExternalToolStart, KnowledgeToolInput, PendingKnowledgeCall, PrivateCheckpoint,
    };
    f.store
        .submit_turn(
            &f.user,
            f.thread.clone(),
            uuid::Uuid::new_v4().to_string(),
            "Read the known message".into(),
        )
        .await
        .unwrap();
    let claim = f.store.claim_agent_run().await.unwrap().unwrap();
    let step = f
        .store
        .begin_agent_step(&claim.lease, claim.context.checkpoint_seq)
        .await
        .unwrap();
    f.store
        .commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![PendingKnowledgeCall {
                call_id: "read".into(),
                input: KnowledgeToolInput::External {
                    request: ExternalToolRequest::Mail(MailToolRequest::Read {
                        connection_id: f.connection.clone(),
                        message_id: id.into(),
                    }),
                },
            }],
        )
        .await
        .unwrap();
    let ExternalToolStart::Dispatch(permit) = f
        .store
        .prepare_external_tool(&step, "read".into())
        .await
        .unwrap()
    else {
        panic!("read receipt")
    };
    let result = f.mail.external(&permit, &CancellationToken::new()).await;
    f.store
        .complete_external_tool(&permit, result)
        .await
        .unwrap();
    let context = f.store.agent_context(&claim.lease).await.unwrap();
    let end = f
        .store
        .begin_agent_step(&claim.lease, context.checkpoint_seq)
        .await
        .unwrap();
    f.store
        .commit_agent_step(
            &end,
            PrivateCheckpoint {
                bytes: b"{}".to_vec(),
            },
            vec![],
        )
        .await
        .unwrap();
    f.store
        .finish_agent_run(&end, RunState::Completed, None)
        .await
        .unwrap();
}

#[tokio::test]
async fn reply_watch_rearm_preserves_observations_without_replaying_a_completed_reply() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture(1).await;
    start(&f).await;
    let first = f.store.next_mail_reply_watch().await.unwrap().unwrap();
    f.mail
        .poll_reply_watch(&first, &CancellationToken::new())
        .await;
    start(&f).await;
    let second = f.store.next_mail_reply_watch().await.unwrap().unwrap();
    f.mail
        .poll_reply_watch(&second, &CancellationToken::new())
        .await;
    assert_eq!(f.wire.probes.load(Ordering::SeqCst), 2);
    assert_eq!(f.wire.bodies.load(Ordering::SeqCst), 1);
    assert_eq!(
        f.store
            .notifications(&f.user, None, false)
            .await
            .unwrap()
            .items
            .len(),
        1
    );
    assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
    f.server.abort();
}
#[tokio::test]
async fn reply_watch_historical_read_does_not_forget_the_later_message_already_observed() {
    let _g = TEST_LOCK.lock().await;
    let f = fixture(1).await;
    read_message(&f, "m1").await;
    read_message(&f, "m0").await;
    assert_eq!(f.wire.bodies.load(Ordering::SeqCst), 2);
    start(&f).await;
    let lease = f.store.next_mail_reply_watch().await.unwrap().unwrap();
    f.mail
        .poll_reply_watch(&lease, &CancellationToken::new())
        .await;
    assert_eq!(f.wire.probes.load(Ordering::SeqCst), 1);
    assert_eq!(f.wire.bodies.load(Ordering::SeqCst), 2);
    assert!(
        f.store
            .notifications(&f.user, None, false)
            .await
            .unwrap()
            .items
            .is_empty()
    );
    assert_eq!(f.wire.models.load(Ordering::SeqCst), 0);
    f.server.abort();
}
