use super::*;
use crate::tests::{TEST_LOCK, admin, directory};
use moosuri_credential_broker::{
    Engine,
    auth::SigningKey,
    github::GitHubAdapter,
    google::{GoogleAdapter, GoogleIdentity},
    transport::Transport,
};
use moosuri_vault_core::{
    Continue, ItemFields, OsEntropy, PendingVault, RecoveryKey, SecretText, StorageReceipt,
    UnlockedArchive, inventory_digest,
};
use moosuri_vault_protocol::broker::Command as BrokerCommand;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
const REFRESH: &str = "SYNTHETIC-NATIVE-SEND-REFRESH";
const ACCESS: &str = "SYNTHETIC-NATIVE-SEND-ACCESS";
const SECRET: &str = r#"{"version":1,"client_id":"synthetic-send.apps.googleusercontent.com","refresh_token":"SYNTHETIC-NATIVE-SEND-REFRESH"}"#;
fn identity() -> GoogleIdentity {
    GoogleIdentity {
        subject: "synthetic-send-subject".into(),
        email: "sender@example.test".into(),
    }
}
fn archive(device: &Device) -> (UnlockedArchive, Uuid) {
    let mut entropy = OsEntropy;
    let password = SecretText::new("Synthetic native mail archive password42".into()).unwrap();
    let mut pending = PendingVault::prepare(
        moosuri_vault_core::Binding {
            owner_id: device.owner.account,
            tenant_id: device.owner.tenant,
            vault_id: Uuid::new_v4(),
        },
        &password,
        &mut entropy,
    )
    .unwrap();
    let recovery = RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap();
    pending.verify_recovery(&recovery).unwrap();
    let (id, digest) = pending.candidate().unwrap();
    pending
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    let mut active = pending.activate().unwrap();
    let fields:ItemFields=serde_json::from_value(serde_json::json!({"kind":"api_key","title":"Synthetic send","username":"","url":"https://oauth2.googleapis.com","secret":SECRET,"notes":"","favorite":false,"expires_at":"","totp":""})).unwrap();
    let item = active
        .seal_item(Uuid::new_v4(), 1, &fields, &mut entropy)
        .unwrap();
    let id = item.id;
    let digest = inventory_digest(std::slice::from_ref(&item)).unwrap();
    let bytes = active.export_archive(&[item], digest, &Continue).unwrap();
    (UnlockedArchive::unlock(&bytes, &password).unwrap(), id)
}
async fn request(mut socket: tokio::net::TcpStream) -> (tokio::net::TcpStream, String, Vec<u8>) {
    use tokio::io::AsyncReadExt;
    let mut bytes = Vec::new();
    loop {
        let mut chunk = [0; 8192];
        let n = socket.read(&mut chunk).await.unwrap();
        assert!(n > 0);
        bytes.extend_from_slice(&chunk[..n]);
        assert!(bytes.len() < 1024 * 1024);
        if let Some(end) = bytes.windows(4).position(|b| b == b"\r\n\r\n") {
            let header = String::from_utf8(bytes[..end].to_vec()).unwrap();
            let size = header
                .lines()
                .find_map(|l| {
                    l.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(str::to_owned)
                })
                .map(|s| s.parse::<usize>().unwrap())
                .unwrap_or(0);
            if bytes.len() >= end + 4 + size {
                return (socket, header, bytes[end + 4..end + 4 + size].to_vec());
            }
        }
    }
}
async fn respond(mut socket: tokio::net::TcpStream, status: u16, body: Vec<u8>) {
    use tokio::io::AsyncWriteExt;
    let header = format!(
        "HTTP/1.1 {status} Synthetic\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    socket.write_all(header.as_bytes()).await.unwrap();
    socket.write_all(&body).await.unwrap();
}
async fn commands(store: Store) -> (Transport, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let lost = Arc::new(AtomicUsize::new(0));
    let flag = lost.clone();
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let (socket, headers, body) = request(socket).await;
            let path = headers
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            let result = if path.ends_with("/challenge") {
                let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
                store
                    .broker_auth_challenge(v["device"].as_str().unwrap().parse().unwrap())
                    .await
                    .map(|v| serde_json::to_vec(&v).unwrap())
            } else {
                let command: SignedCommand = serde_json::from_slice(&body).unwrap();
                let commit = matches!(command.command, BrokerCommand::CommitCurrent { .. });
                let settle = matches!(command.command, BrokerCommand::Settle { .. });
                let result = store
                    .broker_command(command)
                    .await
                    .map(|v| serde_json::to_vec(&v).unwrap());
                if ((commit
                    && flag
                        .compare_exchange(1, 0, Ordering::SeqCst, Ordering::SeqCst)
                        .is_ok())
                    || (settle
                        && flag
                            .compare_exchange(2, 0, Ordering::SeqCst, Ordering::SeqCst)
                            .is_ok()))
                    && result.is_ok()
                {
                    drop(socket);
                    continue;
                }
                result
            };
            match result {
                Ok(body) => respond(socket, 200, body).await,
                Err(_) => respond(socket, 403, b"{}".to_vec()).await,
            }
        }
    });
    (
        Transport::synthetic_loopback(
            "https://assistant.example.test",
            &format!("http://{address}"),
        )
        .unwrap(),
        lost,
        task,
    )
}
async fn provider() -> (
    GoogleAdapter,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<Vec<u8>>>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mode = Arc::new(AtomicUsize::new(0));
    let current = mode.clone();
    let sent = Arc::new(Mutex::new(Vec::new()));
    let output = sent.clone();
    let task = tokio::spawn(async move {
        loop {
            let (socket, _) = listener.accept().await.unwrap();
            let (socket, headers, body) = request(socket).await;
            let first = headers.lines().next().unwrap();
            let path = first.split_whitespace().nth(1).unwrap();
            let mode = current.load(Ordering::SeqCst);
            let value = match path {
                "/token" => {
                    assert!(first.starts_with("POST "));
                    assert!(String::from_utf8_lossy(&body).contains(REFRESH));
                    serde_json::json!({"access_token":ACCESS,"expires_in":3600,"token_type":"Bearer","scope":if mode==1 {"openid email https://www.googleapis.com/auth/gmail.modify"}else{"openid email https://www.googleapis.com/auth/gmail.readonly https://www.googleapis.com/auth/gmail.send"}})
                }
                "/userinfo" => {
                    serde_json::json!({"sub":"synthetic-send-subject","email":"sender@example.test","email_verified":true})
                }
                "/profile" => {
                    serde_json::json!({"emailAddress":if mode==3 {"wrong@example.test"}else{"sender@example.test"}})
                }
                "/messages/send" => {
                    assert!(first.starts_with("POST "));
                    let value: serde_json::Value = serde_json::from_slice(&body).unwrap();
                    assert_eq!(value.as_object().unwrap().len(), 1);
                    let raw = URL_SAFE_NO_PAD
                        .decode(value["raw"].as_str().unwrap())
                        .unwrap();
                    output.lock().unwrap().push(raw);
                    while current.load(Ordering::SeqCst) == 7 {
                        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                    }
                    if mode == 2 {
                        drop(socket);
                        continue;
                    }
                    serde_json::json!({"id":"sent-id","threadId":"sent-thread","ignored":REFRESH})
                }
                _ if path.starts_with("/messages?") => {
                    assert!(first.starts_with("GET "));
                    assert!(path.contains("in%3Asent") || path.contains("in:sent"));
                    assert!(!path.contains(ACCESS));
                    serde_json::json!({"messages":[{"id":"sent-id","threadId":"sent-thread"}]})
                }
                "/messages/sent-id?format=raw&fields=id,threadId,labelIds,raw" => {
                    let raw = output.lock().unwrap().last().unwrap().clone();
                    serde_json::json!({"id":"sent-id","threadId":"sent-thread","labelIds":["SENT"],"raw":URL_SAFE_NO_PAD.encode(raw)})
                }
                _ => panic!("unexpected synthetic route"),
            };
            let status = if mode == 4 && path == "/messages/send" {
                403
            } else {
                200
            };
            respond(socket, status, serde_json::to_vec(&value).unwrap()).await;
        }
    });
    (
        GoogleAdapter::synthetic_loopback(address).unwrap(),
        mode,
        sent,
        task,
    )
}
async fn enroll(store: &Store, user: &UserContext, root: &std::path::Path) -> Engine {
    let key = SigningKey::generate_pkcs8().unwrap();
    let signer = SigningKey::from_pkcs8(&key).unwrap();
    let pair = store
        .broker_pair_challenge(
            user,
            "https://assistant.example.test".into(),
            signer.public_key(),
        )
        .await
        .unwrap();
    store
        .broker_pair(PairProof {
            challenge: pair.nonce,
            signature: signer.sign(&pairing_bytes(&pair).unwrap()),
        })
        .await
        .unwrap();
    Engine::enroll(root, pair.device, pair.origin, &key).unwrap()
}
async fn draft(store: &Store, user: &UserContext, connection: String) -> MailAction {
    store
        .create_mail_draft(
            user,
            MailDraftInput {
                connection_id: connection,
                content: MailDraftContent {
                    from: "sender@example.test".into(),
                    to: vec!["recipient@example.test".into()],
                    cc: vec!["copy@example.test".into()],
                    bcc: vec!["hidden@example.test".into()],
                    subject: "Synthetic full approved subject".into(),
                    text: "Synthetic complete body\n<markup> is literal".into(),
                    reply: None,
                },
                origin_thread_id: None,
            },
            None,
        )
        .await
        .unwrap()
}
fn approval(a: &MailAction) -> MailApproval {
    MailApproval {
        revision: a.revision,
        digest: a.digest.clone(),
    }
}
fn github() -> GitHubAdapter {
    GitHubAdapter::synthetic_loopback("127.0.0.1:9".parse().unwrap()).unwrap()
}
#[tokio::test]
async fn native_exact_approved_mail_actual_device_send_schedule_unknown_and_sent_reconciliation() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let local = directory();
    let root = local.path().join("device");
    let mut engine = enroll(&store, &user, &root).await;
    let (mut vault, item) = archive(engine.device());
    let binding = engine
        .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
        .unwrap();
    let (transport, _, command_task) = commands(store.clone()).await;
    transport
        .register_google(&mut engine, &binding)
        .await
        .unwrap();
    let (adapter, mode, sent, provider_task) = provider().await;
    let a = draft(&store, &user, binding.id.to_string()).await;
    assert!(a.native_send);
    assert_eq!(
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .unwrap(),
        0
    );
    assert!(sent.lock().unwrap().is_empty());
    let id = a.id.clone();
    let raw: Vec<u8> = store
        .call(move |db| {
            Ok(db
                .conn
                .query_row("SELECT raw FROM mail_actions WHERE id=?1", [id], |r| {
                    r.get(0)
                })?)
        })
        .await
        .unwrap();
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    assert!(store.next_mail_dispatch().await.unwrap().is_none());
    assert_eq!(
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .unwrap(),
        1
    );
    assert_eq!(sent.lock().unwrap()[0], raw);
    assert_eq!(store.mail_action(&user, a.id).await.unwrap().state, "sent");
    assert_eq!(sent.lock().unwrap().len(), 1);
    let a = draft(&store, &user, binding.id.to_string()).await;
    let old = approval(&a);
    let a = store
        .schedule_mail(
            &user,
            a.id.clone(),
            MailScheduleInput {
                expected: approval(&a),
                schedule: Some(MailSchedule {
                    due_at: now().unwrap() + 2,
                    timezone: "UTC".into(),
                }),
            },
        )
        .await
        .unwrap();
    assert!(
        store
            .decide_mail(&user, a.id.clone(), old, true)
            .await
            .is_err()
    );
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    assert_eq!(
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .unwrap(),
        0
    );
    tokio::time::sleep(std::time::Duration::from_millis(2100)).await;
    mode.store(2, Ordering::SeqCst);
    transport
        .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
        .await
        .unwrap();
    let unknown = store.mail_action(&user, a.id.clone()).await.unwrap();
    assert_eq!(unknown.state, "outcome_unknown");
    assert_eq!(sent.lock().unwrap().len(), 2);
    assert!(unknown.content.is_none());
    mode.store(0, Ordering::SeqCst);
    drop(engine);
    let mut engine = Engine::open_local(&root).unwrap();
    transport
        .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
        .await
        .unwrap();
    let observed = store.mail_action(&user, a.id.clone()).await.unwrap();
    assert_eq!(observed.state, "outcome_unknown");
    assert_eq!(
        observed.reconciliation.unwrap().status,
        "matching_sent_observed"
    );
    assert_eq!(sent.lock().unwrap().len(), 2);
    assert!(
        store
            .queue_native_mail_reconciliation(&user, a.id.clone())
            .await
            .unwrap()
    );
    transport
        .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
        .await
        .unwrap();
    assert_eq!(sent.lock().unwrap().len(), 2);
    let connections = store.mail_connections(&user).await.unwrap();
    assert!(connections[0].native_send);
    let tenant = user.tenant.0.clone();
    store
        .call(move |db| {
            let stored: Option<Vec<u8>> = db.conn.query_row(
                "SELECT credential FROM mail_connections WHERE tenant_id=?1",
                [tenant],
                |r| r.get(0),
            )?;
            assert!(stored.is_none());
            Ok(())
        })
        .await
        .unwrap();
    command_task.abort();
    provider_task.abort();
}
#[tokio::test]
async fn native_send_lost_commit_ack_dual_device_and_precommit_revoke_do_not_send_or_failover() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let local = directory();
    let mut engine = enroll(&store, &user, &local.path().join("first")).await;
    let (mut vault, item) = archive(engine.device());
    let binding = engine
        .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
        .unwrap();
    let (transport, lost, commands) = commands(store.clone()).await;
    transport
        .register_google(&mut engine, &binding)
        .await
        .unwrap();
    let (adapter, _, sent, provider) = provider().await;
    let mut second = enroll(&store, &user, &local.path().join("second")).await;
    let second_binding = second
        .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
        .unwrap();
    transport
        .register_google(&mut second, &second_binding)
        .await
        .unwrap();
    let a = draft(&store, &user, binding.id.to_string()).await;
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    assert_eq!(
        transport
            .poll_with_google(&mut second, &mut vault, &github(), &adapter)
            .await
            .unwrap(),
        0
    );
    assert!(sent.lock().unwrap().is_empty());
    lost.store(1, Ordering::SeqCst);
    assert!(
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .is_err()
    );
    assert!(sent.lock().unwrap().is_empty());
    transport
        .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
        .await
        .unwrap();
    assert_eq!(
        store.mail_action(&user, a.id).await.unwrap().state,
        "failed_definite"
    );
    assert!(sent.lock().unwrap().is_empty());
    let a = draft(&store, &user, binding.id.to_string()).await;
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    let Reply::Waiting { requests, .. } = transport
        .command(&engine, BrokerCommand::PollMail)
        .await
        .unwrap()
    else {
        panic!("waiting")
    };
    assert_eq!(requests.len(), 1);
    engine
        .prepare_google(&vault, requests[0].clone(), now().unwrap())
        .unwrap();
    let mut changed = a.content.clone().unwrap();
    changed.text = "Changed after adoption; new final approval required".into();
    let edited = store
        .create_mail_draft(
            &user,
            MailDraftInput {
                connection_id: binding.id.to_string(),
                content: changed,
                origin_thread_id: None,
            },
            Some((a.id.clone(), a.revision)),
        )
        .await
        .unwrap();
    assert!(
        store
            .decide_mail(&user, edited.id.clone(), approval(&a), true)
            .await
            .is_err()
    );
    store
        .decide_mail(&user, edited.id.clone(), approval(&edited), true)
        .await
        .unwrap();
    let Reply::Waiting {
        requests: revised, ..
    } = transport
        .command(&engine, BrokerCommand::PollMail)
        .await
        .unwrap()
    else {
        panic!("waiting")
    };
    assert_eq!(revised.len(), 1);
    assert_ne!(requests[0].invocation.run, revised[0].invocation.run);
    transport
        .command(
            &engine,
            BrokerCommand::Abandon {
                invocation: requests[0].invocation.clone(),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        store.mail_action(&user, edited.id).await.unwrap().state,
        "approved"
    );
    engine.delivered(requests[0].invocation.clone()).unwrap();
    store
        .broker_revoke(&user, engine.device().id)
        .await
        .unwrap();
    let signed = transport
        .command(&engine, BrokerCommand::PollMail)
        .await
        .unwrap();
    assert!(matches!(
        signed,
        Reply::Waiting {
            revocation_pending: true,
            ..
        }
    ));
    assert_eq!(
        store.mail_action(&user, a.id).await.unwrap().state,
        "cancelled"
    );
    assert_eq!(
        transport
            .poll_with_google(&mut second, &mut vault, &github(), &adapter)
            .await
            .unwrap(),
        0
    );
    assert!(sent.lock().unwrap().is_empty());
    commands.abort();
    provider.abort();
}

#[tokio::test]
async fn native_send_actual_receipt_lost_ack_restart_and_revoke_after_entry_preserve_single_effect()
{
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let local = directory();
    let root = local.path().join("device");
    let mut engine = enroll(&store, &user, &root).await;
    let (mut vault, item) = archive(engine.device());
    let binding = engine
        .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
        .unwrap();
    let (transport, lost, command_task) = commands(store.clone()).await;
    transport
        .register_google(&mut engine, &binding)
        .await
        .unwrap();
    let (adapter, mode, sent, provider_task) = provider().await;
    let a = draft(&store, &user, binding.id.to_string()).await;
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    lost.store(2, Ordering::SeqCst);
    assert!(
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .is_err()
    );
    assert_eq!(sent.lock().unwrap().len(), 1);
    assert_eq!(
        store.mail_action(&user, a.id.clone()).await.unwrap().state,
        "sent"
    );
    drop(engine);
    let mut engine = Engine::open_local(&root).unwrap();
    transport
        .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
        .await
        .unwrap();
    assert_eq!(sent.lock().unwrap().len(), 1);
    assert_eq!(store.mail_action(&user, a.id).await.unwrap().state, "sent");
    let a = draft(&store, &user, binding.id.to_string()).await;
    store
        .decide_mail(&user, a.id.clone(), approval(&a), true)
        .await
        .unwrap();
    mode.store(7, Ordering::SeqCst);
    let device = engine.device().id;
    let request = tokio::spawn(async move {
        let result = transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await;
        (result, engine, vault)
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        while sent.lock().unwrap().len() < 2 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await
        }
    })
    .await
    .unwrap();
    store.broker_revoke(&user, device).await.unwrap();
    mode.store(0, Ordering::SeqCst);
    let (result, _engine, _vault) = request.await.unwrap();
    result.unwrap();
    let a = store.mail_action(&user, a.id).await.unwrap();
    assert_eq!(a.state, "sent");
    assert_eq!(a.provider_message_id.as_deref(), Some("sent-id"));
    assert_eq!(sent.lock().unwrap().len(), 2);
    command_task.abort();
    provider_task.abort();
}
#[tokio::test]
async fn native_send_actual_scope_and_mailbox_mismatch_never_posts() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let local = directory();
    let mut engine = enroll(&store, &user, &local.path().join("device")).await;
    let (mut vault, item) = archive(engine.device());
    let binding = engine
        .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
        .unwrap();
    let (transport, _, commands) = commands(store.clone()).await;
    transport
        .register_google(&mut engine, &binding)
        .await
        .unwrap();
    let (adapter, mode, sent, provider) = provider().await;
    for bad in [1, 3] {
        mode.store(bad, Ordering::SeqCst);
        let a = draft(&store, &user, binding.id.to_string()).await;
        store
            .decide_mail(&user, a.id.clone(), approval(&a), true)
            .await
            .unwrap();
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .unwrap();
        assert_eq!(
            store.mail_action(&user, a.id).await.unwrap().state,
            "failed_definite"
        );
        assert!(sent.lock().unwrap().is_empty());
        let current = engine
            .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
            .unwrap();
        transport
            .register_google(&mut engine, &current)
            .await
            .unwrap();
    }
    commands.abort();
    provider.abort();
}

#[tokio::test]
async fn native_thirtytwo_stale_adoptions_cannot_starve_fresh_actual_send() {
    let _serial = TEST_LOCK.lock().await;
    let dir = directory();
    let store = Store::open(dir.path()).unwrap();
    let user = admin(&store).await.user;
    let local = directory();
    let mut engine = enroll(&store, &user, &local.path().join("device")).await;
    let (mut vault, item) = archive(engine.device());
    let binding = engine
        .archive_google_send_grant(&mut vault, item, identity(), now().unwrap() + 600)
        .unwrap();
    let (transport, _, commands) = commands(store.clone()).await;
    transport
        .register_google(&mut engine, &binding)
        .await
        .unwrap();
    let (adapter, _, sent, provider) = provider().await;
    let mut stale = Vec::new();
    for _ in 0..32 {
        let a = draft(&store, &user, binding.id.to_string()).await;
        store
            .decide_mail(&user, a.id.clone(), approval(&a), true)
            .await
            .unwrap();
        stale.push(a);
    }
    for expected in [8, 16, 24, 32] {
        let Reply::Waiting { requests, .. } = transport
            .command(&engine, BrokerCommand::PollMail)
            .await
            .unwrap()
        else {
            panic!("waiting")
        };
        assert_eq!(requests.len(), expected);
    }
    for a in stale {
        store
            .decide_mail(&user, a.id.clone(), approval(&a), false)
            .await
            .unwrap();
    }
    let fresh = draft(&store, &user, binding.id.to_string()).await;
    store
        .decide_mail(&user, fresh.id.clone(), approval(&fresh), true)
        .await
        .unwrap();
    assert_eq!(
        transport
            .poll_with_google(&mut engine, &mut vault, &github(), &adapter)
            .await
            .unwrap(),
        1
    );
    assert_eq!(sent.lock().unwrap().len(), 1);
    assert_eq!(
        store.mail_action(&user, fresh.id).await.unwrap().state,
        "sent"
    );
    store.call(move|db|{let retired:i64=db.conn.query_row("SELECT count(*) FROM credential_broker_invocations i JOIN native_mail_effects e ON e.tenant_id=i.tenant_id AND e.effect_id=i.run_id WHERE i.cancelled=1",[],|r|r.get(0))?;assert_eq!(retired,32);Ok(())}).await.unwrap();
    commands.abort();
    provider.abort();
}
async fn issue(store: &Store, engine: &Engine, command: BrokerCommand) -> crate::Result<Reply> {
    let challenge = store.broker_auth_challenge(engine.device().id).await?;
    let signed = engine
        .sign_command(&challenge, "POST", DEVICE_PATH, command, now().unwrap())
        .unwrap();
    store.broker_command(signed).await
}
#[tokio::test]
async fn native_current_sealed_restore_fences_precommit_and_preserves_committed_recovery() {
    let _serial = TEST_LOCK.lock().await;
    use std::os::unix::fs::PermissionsExt;
    let private_directory = || {
        tempfile::Builder::new()
            .permissions(std::fs::Permissions::from_mode(0o700))
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap()
    };
    let root = private_directory();
    let data = root.path().join("source");
    let store = Store::open(&data).unwrap();
    let user = admin(&store).await.user;
    let mut engine = enroll(&store, &user, &root.path().join("device")).await;
    let grant = moosuri_credential_broker::policy::LocalGrant::select_google_send(
        engine.device().clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        identity(),
        1,
        1,
        now().unwrap() + 600,
    )
    .unwrap();
    let binding = engine.select(grant).unwrap();
    issue(
        &store,
        &engine,
        BrokerCommand::RegisterGoogleSend {
            binding: binding.clone(),
            email: identity().email,
        },
    )
    .await
    .unwrap();
    for _ in 0..2 {
        let a = draft(&store, &user, binding.id.to_string()).await;
        store
            .decide_mail(&user, a.id.clone(), approval(&a), true)
            .await
            .unwrap();
    }
    let Reply::Waiting { requests, .. } = issue(&store, &engine, BrokerCommand::PollMail)
        .await
        .unwrap()
    else {
        panic!("waiting")
    };
    assert_eq!(requests.len(), 2);
    for request in &requests {
        engine
            .prepare(request.clone(), now().unwrap(), 1, 1)
            .unwrap();
    }
    let Reply::Commitment { commitment } = issue(
        &store,
        &engine,
        BrokerCommand::CommitCurrent {
            invocation: requests[0].invocation.clone(),
            request_digest: auth::hash(&request_bytes(&requests[0])),
        },
    )
    .await
    .unwrap() else {
        panic!("commit")
    };
    engine.accept_commitment(*commitment.clone()).unwrap();
    drop(store);
    let kit = private_directory();
    let heads = private_directory();
    let identity_file = kit.path().join("identity");
    let recipient = crate::backup::keygen(&identity_file).unwrap();
    crate::backup::setup(&data, &recipient, &identity_file).unwrap();
    let cipher = kit.path().join("current.age");
    let head = heads.path().join("current.json");
    crate::cutover::checkpoint(&data, &cipher, &head).unwrap();
    let destination = root.path().join("restored");
    crate::cutover::restore(&cipher, &head, &identity_file, &destination).unwrap();
    let restored = Store::open(&destination).unwrap();
    restored.call(|db|{assert_eq!(db.conn.query_row("PRAGMA user_version",[],|r|r.get::<_,i64>(0))?,56);assert_eq!(db.conn.query_row("SELECT count(*) FROM mail_actions WHERE state='outcome_unknown'",[],|r|r.get::<_,i64>(0))?,1);assert_eq!(db.conn.query_row("SELECT count(*) FROM mail_actions WHERE state='cancelled'",[],|r|r.get::<_,i64>(0))?,1);assert_eq!(db.conn.query_row("SELECT count(*) FROM native_mail_connections n JOIN mail_connections c ON c.tenant_id=n.tenant_id AND c.id=n.connection_id WHERE c.status='active'",[],|r|r.get::<_,i64>(0))?,0);assert_eq!(db.conn.query_row("SELECT count(*) FROM credential_broker_invocations WHERE cancelled=1",[],|r|r.get::<_,i64>(0))?,1);assert_eq!(db.conn.query_row("SELECT count(*) FROM mail_reconciliations WHERE quarantined=1 AND lease_nonce IS NULL AND next_due IS NULL",[],|r|r.get::<_,i64>(0))?,1);Ok(())}).await.unwrap();
    assert!(
        matches!(issue(&restored,&engine,BrokerCommand::PollMail).await.unwrap(),Reply::Waiting{requests,revocation_pending:true}if requests.is_empty())
    );
    let Reply::Commitment {
        commitment: recovered,
    } = issue(
        &restored,
        &engine,
        BrokerCommand::Recover {
            invocation: requests[0].invocation.clone(),
        },
    )
    .await
    .unwrap()
    else {
        panic!("recover")
    };
    assert!(*recovered == *commitment);
    assert!(
        issue(
            &restored,
            &engine,
            BrokerCommand::CommitCurrent {
                invocation: requests[1].invocation.clone(),
                request_digest: auth::hash(&request_bytes(&requests[1]))
            }
        )
        .await
        .is_err()
    );
}
