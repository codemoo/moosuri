use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicUsize, Ordering},
};
const REFRESH: &str = "SYNTHETIC-REFRESH-CANARY";
const ACCESS: &str = "SYNTHETIC-ACCESS-CANARY";
const SECRET: &str = r#"{"version":1,"client_id":"synthetic.apps.googleusercontent.com","client_secret":"SYNTHETIC-CLIENT-CANARY","refresh_token":"SYNTHETIC-REFRESH-CANARY"}"#;
fn identity() -> GoogleIdentity {
    GoogleIdentity {
        subject: "synthetic-subject-17".into(),
        email: "synthetic@example.test".into(),
    }
}
async fn provider() -> (
    GoogleAdapter,
    Arc<AtomicUsize>,
    Arc<Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let mode = Arc::new(AtomicUsize::new(0));
    let paths = Arc::new(Mutex::new(Vec::new()));
    let m = mode.clone();
    let p = paths.clone();
    let server = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut data = Vec::new();
            loop {
                let mut buf = [0; 4096];
                let n = socket.read(&mut buf).await.unwrap();
                assert!(n > 0);
                data.extend_from_slice(&buf[..n]);
                assert!(data.len() < 32768);
                if let Some(end) = data.windows(4).position(|v| v == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&data[..end]).to_lowercase();
                    let size = headers
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .map(|v| v.parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if data.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            let wire = String::from_utf8(data).unwrap();
            let first = wire.lines().next().unwrap();
            let path = first.split_whitespace().nth(1).unwrap();
            p.lock().unwrap().push(path.into());
            assert!(!first.contains(ACCESS) && !first.contains(REFRESH));
            let mode = m.load(Ordering::SeqCst);
            let body = match path {
                "/token" => {
                    assert!(first.starts_with("POST "));
                    assert!(wire.contains("grant_type=refresh_token"));
                    assert!(wire.contains(REFRESH));
                    assert!(!wire.to_lowercase().contains("authorization:"));
                    let scope = if mode == 1 {
                        "openid email https://www.googleapis.com/auth/gmail.modify"
                    } else {
                        "openid https://www.googleapis.com/auth/userinfo.email https://www.googleapis.com/auth/gmail.readonly"
                    };
                    let rotated = if mode == 4 {
                        ",\"refresh_token\":\"SYNTHETIC-ROTATED-CANARY\""
                    } else {
                        ""
                    };
                    format!(
                        "{{\"access_token\":\"{ACCESS}\",\"expires_in\":3600,\"token_type\":\"Bearer\",\"scope\":\"{scope}\"{rotated},\"unrecognized\":\"{REFRESH}\"}}"
                    )
                }
                "/userinfo" => {
                    assert!(
                        wire.to_lowercase()
                            .contains(&format!("authorization: bearer {}", ACCESS.to_lowercase()))
                    );
                    let sub = if mode == 2 {
                        "other-subject"
                    } else {
                        "synthetic-subject-17"
                    };
                    format!(
                        "{{\"sub\":\"{sub}\",\"email\":\"synthetic@example.test\",\"email_verified\":true,\"name\":\"{REFRESH}\"}}"
                    )
                }
                "/profile" => {
                    let email = if mode == 3 {
                        "other@example.test"
                    } else {
                        "synthetic@example.test"
                    };
                    format!("{{\"emailAddress\":\"{email}\",\"historyId\":\"{REFRESH}\"}}")
                }
                "/inbox" => {
                    assert!(first.starts_with("GET "));
                    assert!(
                        wire.to_lowercase()
                            .contains(&format!("authorization: bearer {}", ACCESS.to_lowercase()))
                    );
                    if mode == 5 {
                        r#"{"id":"INBOX","messagesTotal":1,"messagesUnread":2,"threadsTotal":1,"threadsUnread":0}"#.into()
                    } else if mode == 7 {
                        format!(
                            "{{\"id\":\"INBOX\",\"messagesTotal\":\"{ACCESS}\",\"messagesUnread\":2,\"threadsTotal\":5,\"threadsUnread\":1}}"
                        )
                    } else {
                        format!(
                            "{{\"id\":\"INBOX\",\"messagesTotal\":10,\"messagesUnread\":2,\"threadsTotal\":5,\"threadsUnread\":1,\"name\":\"{REFRESH}\"}}"
                        )
                    }
                }
                _ => panic!("unexpected fixed route"),
            };
            let status = if mode == 6 {
                "302 Found\r\nLocation: https://redirect.invalid/"
            } else {
                "200 OK"
            };
            let reply = format!(
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (
        GoogleAdapter::synthetic_loopback(address).unwrap(),
        mode,
        paths,
        server,
    )
}
#[tokio::test]
async fn google_actual_scope_subject_mailbox_rotation_redirect_and_numeric_projection_fail_closed()
{
    let (adapter, mode, paths, server) = provider().await;
    let p = adapter.read(&identity(), SECRET).await.unwrap();
    assert!(matches!(
        p,
        Projection::GoogleInbox {
            messages_total: 10,
            messages_unread: 2,
            threads_total: 5,
            threads_unread: 1
        }
    ));
    assert!(!serde_json::to_string(&p).unwrap().contains("CANARY"));
    for (bad, expected) in [(1, 1), (2, 2), (3, 3), (4, 1), (5, 4), (6, 1), (7, 4)] {
        mode.store(bad, Ordering::SeqCst);
        paths.lock().unwrap().clear();
        assert!(adapter.read(&identity(), SECRET).await.is_err());
        assert_eq!(paths.lock().unwrap().len(), expected);
    }
    assert!(
        Credential::parse(&SECRET.replace("\"version\":1", "\"version\":1,\"unknown\":1")).is_err()
    );
    server.abort();
}
fn archives(device: &Device, versions: bool) -> (Vec<UnlockedArchive>, uuid::Uuid) {
    use moosuri_vault_core::{
        Continue, OsEntropy, PendingVault, RecoveryKey, StorageReceipt, inventory_digest,
    };
    let password = SecretText::new("SYNTHETIC Google archive password".into()).unwrap();
    let mut entropy = OsEntropy;
    let mut pending = PendingVault::prepare(
        moosuri_vault_core::Binding {
            owner_id: device.owner.account,
            tenant_id: device.owner.tenant,
            vault_id: uuid::Uuid::new_v4(),
        },
        &password,
        &mut entropy,
    )
    .unwrap();
    pending
        .verify_recovery(&RecoveryKey::parse(&pending.recovery().expose_encoded()).unwrap())
        .unwrap();
    let (id, digest) = pending.candidate().unwrap();
    pending
        .confirm_commit(StorageReceipt::from_verified_storage(id, digest))
        .unwrap();
    let mut active = pending.activate().unwrap();
    let fields:ItemFields=serde_json::from_value(serde_json::json!({"kind":"api_key","title":"Synthetic Google credential","username":"","url":"https://oauth2.googleapis.com","secret":SECRET,"notes":"","favorite":false,"expires_at":"","totp":""})).unwrap();
    let item = active
        .seal_item(uuid::Uuid::new_v4(), 1, &fields, &mut entropy)
        .unwrap();
    let id = item.id;
    let inventory = inventory_digest(std::slice::from_ref(&item)).unwrap();
    let bytes = active
        .export_archive(&[item], inventory, &Continue)
        .unwrap();
    let mut snapshots = vec![UnlockedArchive::unlock(&bytes, &password).unwrap()];
    if versions {
        let mut revised_fields = fields;
        revised_fields.secret =
            SecretText::new(SECRET.replace("REFRESH-CANARY", "REFRESH-CANARY-V2")).unwrap();
        let revised = active
            .seal_item(id, 2, &revised_fields, &mut entropy)
            .unwrap();
        let inventory = inventory_digest(std::slice::from_ref(&revised)).unwrap();
        let bytes = active
            .export_archive(std::slice::from_ref(&revised), inventory, &Continue)
            .unwrap();
        snapshots.push(UnlockedArchive::unlock(&bytes, &password).unwrap());
        let mut rotation = active
            .prepare_root_rotation(
                &password,
                std::slice::from_ref(&revised),
                inventory,
                &mut entropy,
            )
            .unwrap();
        rotation
            .verify_recovery(
                &RecoveryKey::parse(&rotation.recovery().unwrap().expose_encoded()).unwrap(),
            )
            .unwrap();
        let (candidate, digest) = rotation.candidate().unwrap();
        rotation
            .confirm_commit(StorageReceipt::from_verified_storage(candidate, digest))
            .unwrap();
        let rotated = rotation.activate().unwrap();
        let inventory = inventory_digest(&rotated).unwrap();
        let bytes = active
            .export_archive(&rotated, inventory, &Continue)
            .unwrap();
        snapshots.push(UnlockedArchive::unlock(&bytes, &password).unwrap());
    }
    (snapshots, id)
}
fn archive(device: &Device) -> (UnlockedArchive, uuid::Uuid) {
    let (mut snapshots, id) = archives(device, false);
    (snapshots.remove(0), id)
}
#[tokio::test]
async fn google_local_grant_receipt_retains_exact_read_and_secrets_never_reach_journal() {
    let (adapter, mode, paths, server) = provider().await;
    let tmp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = tmp.path().join("device");
    let device = Device {
        owner: moosuri_vault_protocol::Owner {
            tenant: uuid::Uuid::new_v4(),
            account: uuid::Uuid::new_v4(),
            installation: uuid::Uuid::new_v4(),
            epoch: 1,
        },
        id: uuid::Uuid::new_v4(),
        generation: 1,
    };
    let pkcs8 = auth::SigningKey::generate_pkcs8().unwrap();
    let mut e = Engine::enroll(
        &root,
        device.clone(),
        "https://assistant.example.test".into(),
        &pkcs8,
    )
    .unwrap();
    let key = auth::SigningKey::from_pkcs8(&pkcs8).unwrap().public_key();
    let (mut archive, item) = archive(&device);
    let binding = e
        .archive_google_grant(
            &mut archive,
            item,
            identity(),
            current_time().unwrap() + 600,
        )
        .unwrap();
    let request = Request {
        device,
        invocation: Invocation {
            run: uuid::Uuid::new_v4(),
            step: 1,
            call: uuid::Uuid::new_v4(),
            nonce: uuid::Uuid::new_v4(),
        },
        binding,
        operation: Operation::GoogleInboxRead,
        repository: None,
        digest: resource_digest(),
        expires_at: current_time().unwrap() + 300,
    };
    let mut forged = request.clone();
    forged.operation = Operation::GitHubRepositoryRead;
    assert!(e.prepare(forged, current_time().unwrap(), 1, 1).is_err());
    let mut forged = request.clone();
    forged.repository = Some(GitHubRepository {
        owner: "owner".into(),
        repository: "repo".into(),
    });
    assert!(e.prepare(forged, current_time().unwrap(), 1, 1).is_err());
    assert!(
        e.prepare(request.clone(), current_time().unwrap(), 2, 1)
            .is_err()
    );
    e.prepare_google(&archive, request.clone(), current_time().unwrap())
        .unwrap();
    e.accept_commitment(Commitment {
        request: request.clone(),
        nonce: uuid::Uuid::new_v4(),
    })
    .unwrap();
    let receipt = e
        .execute_google(&mut archive, &request.invocation, &adapter)
        .await
        .unwrap();
    auth::verify(&key, &receipt_bytes(&receipt.receipt), &receipt.signature).unwrap();
    let exact = serde_json::to_vec(&receipt).unwrap();
    assert!(!String::from_utf8_lossy(&exact).contains("CANARY"));
    drop(e);
    let mut e = Engine::open_local(&root).unwrap();
    let retained = e.receipt(&request.invocation).unwrap().unwrap();
    assert_eq!(serde_json::to_vec(&retained).unwrap(), exact);
    assert!(
        e.execute_google(&mut archive, &request.invocation, &adapter)
            .await
            .is_err()
    );
    assert_eq!(paths.lock().unwrap().len(), 4);
    // A new original request uses the same grant; failed actual scope then suspends it.
    let mut next = request.clone();
    next.invocation.run = uuid::Uuid::new_v4();
    next.invocation.call = uuid::Uuid::new_v4();
    next.invocation.nonce = uuid::Uuid::new_v4();
    e.prepare_google(&archive, next.clone(), current_time().unwrap())
        .unwrap();
    e.accept_commitment(Commitment {
        request: next.clone(),
        nonce: uuid::Uuid::new_v4(),
    })
    .unwrap();
    mode.store(1, Ordering::SeqCst);
    let rejected = e
        .execute_google(&mut archive, &next.invocation, &adapter)
        .await
        .unwrap();
    assert!(rejected.receipt.outcome == Outcome::Rejected);
    next.invocation.run = uuid::Uuid::new_v4();
    next.invocation.call = uuid::Uuid::new_v4();
    assert!(
        e.prepare_google(&archive, next, current_time().unwrap())
            .is_err()
    );
    assert_eq!(paths.lock().unwrap().len(), 5);

    for entry in std::fs::read_dir(&root).unwrap() {
        let p = entry.unwrap().path();
        if p.is_file() {
            let bytes = std::fs::read(p).unwrap();
            assert!(!String::from_utf8_lossy(&bytes).contains("CANARY"));
        }
    }
    server.abort();
}

#[test]
fn google_protocol_extension_preserves_all_existing_signed_variants() {
    use uuid::Uuid;
    let id = Uuid::from_u128;
    let device = Device {
        owner: moosuri_vault_protocol::Owner {
            tenant: id(1),
            account: id(2),
            installation: id(3),
            epoch: 4,
        },
        id: id(5),
        generation: 6,
    };
    let invocation = Invocation {
        run: id(7),
        step: 8,
        call: id(9),
        nonce: id(10),
    };
    let binding = Binding {
        id: id(11),
        epoch: 12,
        grant_version: 13,
        grant_hash: [14; 32],
        item_epoch: 15,
        key_epoch: 16,
        expires_at: 17,
    };
    let repository = GitHubRepository {
        owner: "owner".into(),
        repository: "repo".into(),
    };
    let request = Request {
        device: device.clone(),
        invocation: invocation.clone(),
        binding: binding.clone(),
        operation: Operation::GitHubRepositoryRead,
        repository: Some(repository.clone()),
        digest: [18; 32],
        expires_at: 19,
    };
    let receipt = SignedReceipt {
        receipt: Receipt {
            commitment: Commitment {
                request: request.clone(),
                nonce: id(20),
            },
            outcome: Outcome::ProjectedSuccess,
            projection: Projection::GitHubRepository {
                id: 21,
                private: true,
                archived: false,
                stars: 22,
                open_issues: 23,
            },
            projection_digest: [24; 32],
        },
        signature: vec![27; 64],
    };
    let challenge = AuthChallenge {
        device,
        origin: "https://assistant.example.test".into(),
        nonce: id(25),
        expires_at: 26,
    };
    let hex = |bytes: &[u8]| {
        auth::hash(bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
    };
    // Independent Python struct encoder of the pre-extension byte contract.
    assert_eq!(
        hex(&request_bytes(&request)),
        "9d313d587cf0fee77f5d146fdcfa12682a9759a792dc26f4dd711a4d7eac840f"
    );
    assert_eq!(
        hex(&receipt_bytes(&receipt.receipt)),
        "904fdde32f900d919d36ef1271128d1509bd06a2ed554cdd76ad13663473ec3c"
    );
    let commands = [
        Command::Register {
            binding: binding.clone(),
        },
        Command::Commit {
            invocation: invocation.clone(),
        },
        Command::Recover {
            invocation: invocation.clone(),
        },
        Command::Settle {
            receipt: Box::new(receipt),
        },
        Command::AckRevoke,
        Command::RegisterRepository {
            binding: binding.clone(),
            repository: repository.clone(),
        },
        Command::Poll,
        Command::Abandon {
            invocation: invocation.clone(),
        },
        Command::RefreshRepository {
            expected: binding.clone(),
            binding,
            repository,
        },
        Command::CommitCurrent {
            invocation,
            request_digest: [28; 32],
        },
    ];
    let expected = [
        "f6b7a375a4e5037a0bf89643a0b1129bc8b29fdd41fee919f60d34c3e063991c",
        "d793e42a241aac7c251a2c4d27e3c535aece14991d20dcf1a24e9c8b9acd46e9",
        "cc577375947a1033a6a54351fd7f027fcf6f6c75ea2c4f4eedc91494f49b858e",
        "66d683cc3c3897ab86c77dcfb36c23411d101b2296ba29ba3d7b4ed4792e8796",
        "bd602f942257770673ebc66045acc1c11b43392aa636c86d5ad328ca1117ded1",
        "2b9c0943392d4ea4da41df6fe5769c17118ecc66faee2fd34dab93089bc3b31c",
        "9cf3486967e07be75e1c51d4c0cc32401d834528df99c8ff5776f2b26f399a9b",
        "4a1f915faceb0093e8783f7cb1983c751143c3b5f325b1d5e27781ca8c2c373c",
        "aad4bef0cea0a20ebaade092197c61b66811b299264c7d7dd7d609ba93ed5317",
        "09bdeb5dd68ae07da19891a80353600e546435f236a1a8697c5d66718ea6e475",
    ];
    for (command, expected) in commands.iter().zip(expected) {
        assert_eq!(
            hex(&command_bytes(&challenge, "POST", DEVICE_PATH, command).unwrap()),
            expected
        )
    }
    let old = r#"{"owner":"owner","repository":"repo"}"#;
    let resource: Resource = serde_json::from_str(old).unwrap();
    assert_eq!(serde_json::to_string(&resource).unwrap(), old);
    assert!(
        serde_json::from_str::<Resource>(r#"{"google_inbox":"other","owner":"owner"}"#).is_err()
    );
}

#[tokio::test]
async fn google_archive_reconnect_versions_preserve_expiry_reconcile_and_lost_ack() {
    let (adapter, _, paths, server) = provider().await;
    let tmp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = tmp.path().join("device");
    let device = Device {
        owner: moosuri_vault_protocol::Owner {
            tenant: uuid::Uuid::new_v4(),
            account: uuid::Uuid::new_v4(),
            installation: uuid::Uuid::new_v4(),
            epoch: 1,
        },
        id: uuid::Uuid::new_v4(),
        generation: 1,
    };
    let key = auth::SigningKey::generate_pkcs8().unwrap();
    let mut engine = Engine::enroll(
        &root,
        device.clone(),
        "https://assistant.example.test".into(),
        &key,
    )
    .unwrap();
    let (mut versions, item) = archives(&device, true);
    let mut rotated = versions.pop().unwrap();
    let mut revised = versions.pop().unwrap();
    let mut original = versions.pop().unwrap();
    let expiry = current_time().unwrap() + 600;
    let first = engine
        .archive_google_grant(&mut original, item, identity(), expiry)
        .unwrap();
    let request = |binding: Binding| Request {
        device: device.clone(),
        invocation: Invocation {
            run: uuid::Uuid::new_v4(),
            step: 1,
            call: uuid::Uuid::new_v4(),
            nonce: uuid::Uuid::new_v4(),
        },
        binding,
        operation: Operation::GoogleInboxRead,
        repository: None,
        digest: resource_digest(),
        expires_at: expiry - 10,
    };
    let old = request(first.clone());
    engine
        .prepare_google(&original, old.clone(), current_time().unwrap())
        .unwrap();
    engine
        .accept_commitment(Commitment {
            request: old.clone(),
            nonce: uuid::Uuid::new_v4(),
        })
        .unwrap();
    let receipt = engine
        .execute_google(&mut original, &old.invocation, &adapter)
        .await
        .unwrap();
    let exact = serde_json::to_vec(&receipt).unwrap();
    assert!(
        engine
            .archive_google_grant(&mut revised, item, identity(), expiry + 86400)
            .is_err()
    );
    assert_eq!(
        serde_json::to_vec(&engine.settle_without_retry(&old.invocation).unwrap()).unwrap(),
        exact
    );
    // The authenticated original receipt is reconciled/delivered before version change.
    engine.delivered(old.invocation.clone()).unwrap();
    let second = engine
        .archive_google_grant(&mut revised, item, identity(), expiry + 86400)
        .unwrap();
    assert_eq!(second.id, first.id);
    assert_eq!(second.expires_at, expiry);
    assert_eq!(second.item_epoch, 2);
    assert_eq!(second.key_epoch, 1);
    assert_eq!(second.grant_version, first.grant_version + 1);
    assert_eq!(second.epoch, first.epoch + 1);
    assert!(
        engine
            .prepare_google(&revised, request(second.clone()), current_time().unwrap())
            .is_err()
    );
    // A lost registration ACK keeps the exact old/new CAS after restart; no extra version.
    drop(engine);
    let mut engine = Engine::open_local(&root).unwrap();
    let retried = engine
        .archive_google_grant(&mut revised, item, identity(), expiry + 172800)
        .unwrap();
    assert!(retried == second);
    assert!(engine.grants()[0].pending_refresh() == Some(&first));
    engine.confirm_live_refresh(&second).unwrap();
    assert!(
        engine
            .execute_google(&mut revised, &old.invocation, &adapter)
            .await
            .is_err()
    );
    assert_eq!(paths.lock().unwrap().len(), 4);
    let current = request(second.clone());
    engine
        .prepare_google(&revised, current.clone(), current_time().unwrap())
        .unwrap();
    engine
        .accept_commitment(Commitment {
            request: current.clone(),
            nonce: uuid::Uuid::new_v4(),
        })
        .unwrap();
    // Retained entered-without-receipt work must stay unknown, never re-enter.
    engine
        .begin_dispatch(&current.invocation, current_time().unwrap(), 2, 1)
        .unwrap();
    drop(engine);
    let mut engine = Engine::open_local(&root).unwrap();
    assert!(
        engine
            .archive_google_grant(&mut rotated, item, identity(), expiry + 86400)
            .is_err()
    );
    let unknown = engine.settle_without_retry(&current.invocation).unwrap();
    assert!(unknown.receipt.outcome == Outcome::Unknown);
    assert!(
        engine
            .execute_google(&mut revised, &current.invocation, &adapter)
            .await
            .is_err()
    );
    assert_eq!(paths.lock().unwrap().len(), 4);
    engine.delivered(current.invocation.clone()).unwrap();
    let third = engine
        .archive_google_grant(&mut rotated, item, identity(), expiry + 86400)
        .unwrap();
    assert_eq!(third.id, first.id);
    assert_eq!(third.expires_at, expiry);
    assert_eq!(third.item_epoch, 3);
    assert_eq!(third.key_epoch, 2);
    assert_eq!(third.grant_version, second.grant_version + 1);
    engine.confirm_live_refresh(&third).unwrap();
    assert!(
        engine
            .archive_google_grant(&mut original, item, identity(), expiry)
            .is_err()
    );
    let new = request(third);
    engine
        .prepare_google(&rotated, new.clone(), current_time().unwrap())
        .unwrap();
    engine
        .accept_commitment(Commitment {
            request: new.clone(),
            nonce: uuid::Uuid::new_v4(),
        })
        .unwrap();
    let success = engine
        .execute_google(&mut rotated, &new.invocation, &adapter)
        .await
        .unwrap();
    assert!(success.receipt.outcome == Outcome::ProjectedSuccess);
    assert_eq!(paths.lock().unwrap().len(), 8);
    server.abort();
}
