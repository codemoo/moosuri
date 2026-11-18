use super::knowledge::tool_configured;
use super::*;
use moosuri_connectors::gws_inbox::GwsInboxRegistry;
use std::os::unix::fs::PermissionsExt;

fn private(path: &std::path::Path, text: &str) {
    std::fs::write(path, text).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600)).unwrap();
}
fn registry(
    dir: &std::path::Path,
    owner: &str,
    endpoint: &str,
) -> (Arc<GwsInboxRegistry>, std::path::PathBuf) {
    let key = dir.join("gws.key");
    private(&key, &"a".repeat(64));
    let config = dir.join("gws.json");
    private(&config,&json!([{"account_id":owner,"mailbox":"owner@example.test","endpoint":endpoint,"key_file":key}]).to_string());
    (
        Arc::new(GwsInboxRegistry::from_file(config.clone()).unwrap()),
        config,
    )
}
fn service(store: &Store, registry: Arc<GwsInboxRegistry>) -> crate::mail::MailService {
    let (changes, _) = watch::channel(0);
    crate::mail::MailService::new(
        store.clone(),
        None,
        Signals {
            stop: CancellationToken::new(),
            wake: Arc::new(Notify::new()),
            changes,
            delivery: Arc::new(tokio::sync::RwLock::new(())),
        },
    )
    .with_gws_inbox(Some(registry))
}
async fn claim(
    store: &Store,
    user: &UserContext,
    thread: &str,
    text: &str,
) -> moosuri_store::AgentClaim {
    store
        .submit_turn(
            user,
            thread.into(),
            uuid::Uuid::new_v4().to_string(),
            text.into(),
        )
        .await
        .unwrap();
    store.claim_agent_run().await.unwrap().unwrap()
}
#[tokio::test]
async fn gws_latest_metadata_is_owner_bound_literal_and_general_context_has_no_mail() {
    let _g = TEST_LOCK.lock().await;
    let calls = Arc::new(AtomicUsize::new(0));
    let counted = calls.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/inbox/latest", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/inbox/latest",post(move |headers:axum::http::HeaderMap,axum::Json(body):axum::Json<Value>|{let counted=counted.clone();async move {
        assert_eq!(headers["authorization"],format!("Bearer {}","a".repeat(64)));
        assert!(!headers.contains_key("cookie"));assert!(!headers.contains_key("origin"));assert_eq!(body,json!({}));
        counted.fetch_add(1,Ordering::SeqCst);
        tokio::time::sleep(Duration::from_secs(9)).await;
        axum::Json(json!({"mailbox":"owner@example.test","messages":[{"id":"older","from":"old@example.test","subject":"SYNTHETIC_OLDER","date":"2026-11-17","internal_date":1000},{"id":"newer","from":"new@example.test","subject":"SYNTHETIC_NEWEST_METADATA","date":"2026-11-18","internal_date":2000}]}))
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, thread) = tool_configured("http://127.0.0.1:1/v1").await;
    let (r, _) = registry(dir.path(), &user.account().id, &endpoint);
    let binding = r.binding(&user.account().id).unwrap().unwrap();
    let confirmation = store
        .gws_inbox_challenge(&user, binding.fingerprint())
        .await
        .unwrap()
        .unwrap();
    store
        .confirm_gws_inbox(&user, binding.fingerprint(), confirmation)
        .await
        .unwrap();
    let mail = service(&store, r.clone());
    let semantic = claim(
        &store,
        &user,
        &thread,
        "새로 도착한 항목부터 정리해 줄래? 받은편지함 얘기야.",
    )
    .await;
    let context = mail
        .model_connection_context(&semantic.lease)
        .await
        .unwrap();
    assert!(context.contains("\"run_read_route\":\"none\""));
    assert!(context.contains("\"recovery\":\"admin_setup\""));
    assert!(!context.contains("SYNTHETIC_NEWEST_METADATA"));
    assert!(
        mail.connection_help_available(&semantic.lease)
            .await
            .unwrap()
    );
    store
        .finish_agent_execution(&semantic.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    let ordinary = claim(&store, &user, &thread, "최근 메일 뭐야?").await;
    assert!(mail.latest_inbox_chat(&ordinary.lease).await.unwrap());
    let ordinary_turn = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .pop()
        .unwrap();
    assert!(matches!(
        ordinary_turn.run.mail_connection_recovery,
        Some(moosuri_core::MailConnectionRecovery::AdminSetup)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let latest = claim(&store, &user, &thread, "GWS 내 최신 메일 보여줘").await;
    assert!(mail.latest_inbox_chat(&latest.lease).await.unwrap());
    let turn = store
        .conversation(&user, thread.clone(), None)
        .await
        .unwrap()
        .turns
        .pop()
        .unwrap();
    assert_eq!(turn.run.state, RunState::Completed);
    assert!(turn.run.mail_connection_recovery.is_none());
    let text = turn
        .assistant
        .iter()
        .map(|m| m.text.as_str())
        .collect::<String>();
    assert!(text.contains("GWS 연결"));
    assert!(text.contains("SYNTHETIC_NEWEST_METADATA"));
    assert!(!text.contains("SYNTHETIC_OLDER"));
    assert!(text.contains("최근 최대 20통 중"));
    let next = claim(&store, &user, &thread, "그림 그리는 방법을 설명해 줘").await;
    assert!(
        !next
            .messages
            .iter()
            .any(|m| m.text.contains("SYNTHETIC_NEWEST_METADATA"))
    );
    store
        .finish_agent_execution(&next.lease, RunState::Cancelled, None)
        .await
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    assert!(store.mail_actions(&user).await.unwrap().is_empty());
    server.abort();
}

#[tokio::test]
async fn gws_offline_wrong_mailbox_and_revoked_binding_never_deliver_metadata_or_fall_back() {
    let _g = TEST_LOCK.lock().await;
    let mode = Arc::new(AtomicU8::new(0));
    let shared = mode.clone();
    let entered = Arc::new(Semaphore::new(0));
    let enter = entered.clone();
    let release = Arc::new(Semaphore::new(0));
    let released = release.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/inbox/latest", listener.local_addr().unwrap());
    let app=Router::new().route("/v1/inbox/latest",post(move ||{let shared=shared.clone();let enter=enter.clone();let released=released.clone();async move {
        let m=shared.load(Ordering::SeqCst);
        if m==0{return (axum::http::StatusCode::SERVICE_UNAVAILABLE,axum::Json(json!({})));}
        if m>=2 {enter.add_permits(1);released.acquire().await.unwrap().forget();}
        (axum::http::StatusCode::OK,axum::Json(json!({"mailbox":if m==1{"foreign@example.test"}else{"owner@example.test"},"messages":[{"id":"one","from":"sender@example.test","subject":"MUST_NOT_DELIVER","date":"today","internal_date":1000}]})))
    }}));
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, thread) = tool_configured("http://127.0.0.1:1/v1").await;
    let (r, path) = registry(dir.path(), &user.account().id, &endpoint);
    let binding = r.binding(&user.account().id).unwrap().unwrap();
    let confirmation = store
        .gws_inbox_challenge(&user, binding.fingerprint())
        .await
        .unwrap()
        .unwrap();
    store
        .confirm_gws_inbox(&user, binding.fingerprint(), confirmation)
        .await
        .unwrap();
    let mail = service(&store, r);
    let saved = std::fs::read_to_string(&path).unwrap();
    for m in 0..4 {
        if m == 3 {
            private(&path, &saved);
        }
        mode.store(m, Ordering::SeqCst);
        let run = claim(&store, &user, &thread, "GWS 내 최신 메일 보여줘").await;
        if m >= 2 {
            let service = mail.clone();
            let lease = run.lease.clone();
            let work = tokio::spawn(async move { service.latest_inbox_chat(&lease).await });
            entered.acquire().await.unwrap().forget();
            if m == 2 {
                private(&path, "[]");
            } else {
                store
                    .cancel_run(&user, run.lease.run_id().into(), run.lease.attempt())
                    .await
                    .unwrap();
            }
            release.add_permits(1);
            let result = work.await.unwrap();
            if m == 2 {
                assert!(result.unwrap());
            } else {
                assert!(result.is_err());
            }
        } else {
            assert!(mail.latest_inbox_chat(&run.lease).await.unwrap());
        }
        let turn = store
            .conversation(&user, thread.clone(), None)
            .await
            .unwrap()
            .turns
            .pop()
            .unwrap();
        if m == 3 {
            assert_eq!(turn.run.state, RunState::Cancelled);
            assert!(turn.run.mail_connection_recovery.is_none());
            assert!(turn.assistant.is_empty());
            continue;
        }
        assert_eq!(turn.run.state, RunState::Completed);
        assert!(matches!(
            turn.run.mail_connection_recovery,
            Some(moosuri_core::MailConnectionRecovery::GwsSetup)
        ));
        assert!(
            turn.assistant
                .iter()
                .all(|m| !m.text.contains("MUST_NOT_DELIVER"))
        );
        assert!(
            turn.assistant
                .iter()
                .any(|m| m.text.contains("확인하지 못했어요"))
        );
    }
    assert_eq!(
        store.usage_summary(&user).await.unwrap().dispatched_calls,
        0
    );
    server.abort();
}

#[tokio::test]
async fn gws_first_link_checks_only_signed_profile_and_requires_exact_owner_confirmation() {
    let _guard = TEST_LOCK.lock().await;
    let lists = Arc::new(AtomicUsize::new(0));
    let count = lists.clone();
    let mode = Arc::new(AtomicU8::new(0));
    let shared = mode.clone();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/inbox/latest", listener.local_addr().unwrap());
    let app = Router::new()
        .route(
            "/v1/inbox/latest",
            post(move || {
                let count = count.clone();
                async move {
                    count.fetch_add(1, Ordering::SeqCst);
                    axum::Json(json!({"mailbox":"owner@example.test","messages":[]}))
                }
            }),
        )
        .route(
            "/v1/profile/check",
            post(move |axum::Json(body): axum::Json<Value>| {
                let shared = shared.clone();
                async move {
                    let state = if shared.load(Ordering::SeqCst) == 0 {
                        "ready"
                    } else {
                        "wrong_account"
                    };
                    let mailbox = if state == "ready" {
                        "owner@example.test"
                    } else {
                        ""
                    };
                    let signed = serde_json::to_vec(&(
                        "moosuri/gws-profile/v1",
                        body["nonce"].as_str().unwrap(),
                        state,
                        mailbox,
                    ))
                    .unwrap();
                    let digest =
                        ring::digest::digest(&ring::digest::SHA256, "a".repeat(64).as_bytes());
                    let signature: String = ring::hmac::sign(
                        &ring::hmac::Key::new(ring::hmac::HMAC_SHA256, digest.as_ref()),
                        &signed,
                    )
                    .as_ref()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                    axum::Json(json!({"state":state,"mailbox":mailbox,"signature":signature}))
                }
            }),
        );
    let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let (dir, store, user, thread) = tool_configured("http://127.0.0.1:1/v1").await;
    let (registry, path) = registry(dir.path(), &user.account().id, &endpoint);
    let mail = service(&store, registry.clone());
    assert_eq!(
        mail.gws_inbox_status(&user).await.unwrap()["confirmed"],
        false
    );
    let before = claim(&store, &user, &thread, "GWS show my latest email").await;
    assert!(mail.latest_inbox_chat(&before.lease).await.unwrap());
    assert_eq!(lists.load(Ordering::SeqCst), 0);
    let check = mail.gws_inbox_readiness(&user).await.unwrap();
    assert_eq!(check["state"], "needs_confirmation");
    assert_eq!(lists.load(Ordering::SeqCst), 0);
    let confirmation = check["confirmation"].as_str().unwrap().to_owned();
    let ready = mail
        .gws_inbox_confirm(&user, confirmation.clone())
        .await
        .unwrap();
    assert_eq!(ready["state"], "ready");
    assert!(mail.gws_inbox_confirm(&user, confirmation).await.is_err());
    assert_eq!(
        mail.gws_inbox_readiness(&user).await.unwrap()["state"],
        "ready"
    );
    assert_eq!(lists.load(Ordering::SeqCst), 0);
    let after = claim(&store, &user, &thread, "GWS show my latest email").await;
    assert!(mail.latest_inbox_chat(&after.lease).await.unwrap());
    assert_eq!(lists.load(Ordering::SeqCst), 1);
    mode.store(1, Ordering::SeqCst);
    assert_eq!(
        mail.gws_inbox_readiness(&user).await.unwrap()["state"],
        "wrong_account"
    );
    assert_eq!(
        mail.gws_inbox_status(&user).await.unwrap()["confirmed"],
        false
    );
    mode.store(0, Ordering::SeqCst);
    let check = mail.gws_inbox_readiness(&user).await.unwrap();
    assert_eq!(check["state"], "needs_confirmation");
    let original = std::fs::read_to_string(&path).unwrap();
    private(
        &path,
        &original.replace("owner@example.test", "new-owner@example.test"),
    );
    assert_eq!(
        mail.gws_inbox_status(&user).await.unwrap()["confirmed"],
        false
    );
    let changed = mail
        .gws_inbox_confirm(&user, check["confirmation"].as_str().unwrap().into())
        .await
        .unwrap();
    assert_eq!(changed["state"], "offline"); // signed profile identity cannot match newly selected binding
    assert_eq!(lists.load(Ordering::SeqCst), 1);
    server.abort();
}
