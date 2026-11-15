use super::*;
#[test]
fn model_selection_is_grant_and_expiry_bound_and_not_a_display_selection() {
    let device = Device {
        owner: moosuri_vault_protocol::Owner {
            tenant: Uuid::new_v4(),
            account: Uuid::new_v4(),
            installation: Uuid::new_v4(),
            epoch: 1,
        },
        id: Uuid::new_v4(),
        generation: 1,
    };
    let grant = policy::LocalGrant::select_google_model_read(
        device.clone(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        GoogleIdentity {
            subject: "synthetic-subject".into(),
            email: "owner@example.test".into(),
        },
        1,
        1,
        200,
        false,
    )
    .unwrap();
    let display = policy::LocalGrant::select_google_body(
        device,
        grant.item(),
        grant.vault().unwrap(),
        grant.google().unwrap().clone(),
        1,
        1,
        200,
    )
    .unwrap();
    let mut selections = Selections::default();
    let selection = selections
        .add(
            &grant,
            100,
            Selected::Message {
                id: "provider-id".into(),
                thread: "provider-thread".into(),
            },
        )
        .unwrap();
    assert!(selections.get(&grant, selection, 99).is_ok());
    assert!(selections.get(&grant, selection, 100).is_err());
    assert!(selections.get(&display, selection, 99).is_err());
    let request = Request {
        device: grant.device.clone(),
        invocation: Invocation {
            run: Uuid::new_v4(),
            step: 1,
            call: Uuid::new_v4(),
            nonce: Uuid::new_v4(),
        },
        binding: grant.binding.clone(),
        operation: Operation::GoogleMailModelRead {
            selection: Uuid::new_v4(),
        },
        repository: None,
        digest: [1; 32],
        expires_at: 100,
    };
    assert!(grant.check(&request, 99, 1, 1).is_ok());
    let mut old = request.clone();
    old.binding = display.binding.clone();
    assert!(display.check(&old, 99, 1, 1).is_err());
    assert!(!grant.google_model_background());
    assert_ne!(grant.binding.grant_hash, display.binding.grant_hash);
}
#[test]
fn pages_split_at_utf8_boundaries_and_secret_is_checked_before_truncation() {
    let text = "한".repeat(2000);
    let pages = text_pages(&text);
    assert_eq!(pages.concat(), text);
    assert!(pages.iter().all(|p| p.len() <= 4096));
    assert!(
        bounded("visible text plus synthetic-access-token", 7, &|s| s
            .contains("synthetic-access-token"))
        .is_err()
    );
}
const SECRET: &str = r#"{"version":1,"client_id":"synthetic-native-model-client","client_secret":"synthetic-native-model-secret","refresh_token":"synthetic-native-model-refresh"}"#;
const ACCESS: &str = "synthetic-native-model-access";
async fn provider() -> (
    GoogleAdapter,
    std::sync::Arc<std::sync::atomic::AtomicUsize>,
    std::sync::Arc<std::sync::Mutex<Vec<String>>>,
    tokio::task::JoinHandle<()>,
) {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let socket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = socket.local_addr().unwrap();
    let mode = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let modes = mode.clone();
    let paths = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
    let observed = paths.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut io, _) = socket.accept().await.unwrap();
            let mut input = Vec::new();
            loop {
                let mut buffer = [0; 4096];
                let n = io.read(&mut buffer).await.unwrap();
                if n == 0 {
                    break;
                }
                input.extend_from_slice(&buffer[..n]);
                if let Some(end) = input.windows(4).position(|w| w == b"\r\n\r\n") {
                    let header = String::from_utf8_lossy(&input[..end]).to_ascii_lowercase();
                    let content = header
                        .lines()
                        .find_map(|l| l.strip_prefix("content-length: "))
                        .map(|n| n.parse::<usize>().unwrap())
                        .unwrap_or(0);
                    if input.len() >= end + 4 + content {
                        break;
                    }
                }
            }
            let wire = String::from_utf8(input).unwrap();
            let line = wire.lines().next().unwrap();
            let path = line.split_whitespace().nth(1).unwrap();
            observed.lock().unwrap().push(path.to_owned());
            let mode = modes.load(std::sync::atomic::Ordering::SeqCst);
            if path != "/token" {
                assert!(
                    wire.to_ascii_lowercase()
                        .contains(&format!("authorization: bearer {ACCESS}"))
                );
            }
            assert!(!line.contains(ACCESS) && !line.contains("synthetic-native-model-refresh"));
            let body = if path == "/token" {
                serde_json::json!({"access_token":ACCESS,"token_type":"Bearer","expires_in":3600,"scope":if mode == 1 { "openid email" } else { "openid email https://www.googleapis.com/auth/gmail.readonly" }})
            } else if path == "/userinfo" {
                serde_json::json!({"sub":if mode == 2 {"wrong-subject"} else {"synthetic-subject"},"email":"owner@example.test","email_verified":true,"ignored":ACCESS})
            } else if path == "/profile" {
                serde_json::json!({"emailAddress":"owner@example.test"})
            } else if path.starts_with("/messages?") {
                serde_json::json!({"messages":[{"id":"provider-message","threadId":"provider-thread"}],"nextPageToken":"provider-cursor"})
            } else if path.starts_with("/threads/provider-thread?") {
                serde_json::json!({"id":"provider-thread","messages":[{"id":"provider-message"}]})
            } else if path == "/messages/provider-message/attachments/provider-attachment" {
                let text = if mode == 4 {
                    "changed file".into()
                } else {
                    "Synthetic attachment\n".repeat(260)
                };
                serde_json::json!({"size":text.len(),"data":URL_SAFE_NO_PAD.encode(text)})
            } else if path.starts_with("/messages/provider-message?") {
                let text = if mode == 3 {
                    ACCESS
                } else {
                    "Synthetic invoice total 42"
                };
                let file = if mode == 4 {
                    "changed file".into()
                } else {
                    "Synthetic attachment\n".repeat(260)
                };
                serde_json::json!({"id":"provider-message","threadId":"provider-thread","labelIds":["INBOX"],"payload":{"partId":"","mimeType":"multipart/mixed","headers":[{"name":"Subject","value":"Synthetic quote"},{"name":"From","value":"vendor@example.test"}],"parts":[{"partId":"0","mimeType":"text/plain","body":{"size":text.len(),"data":URL_SAFE_NO_PAD.encode(text)}},{"partId":"1","mimeType":if mode==5 {"application/pdf"} else {"text/plain"},"filename":if mode==5 {"quote.pdf"} else {"quote.txt"},"body":{"size":file.len(),"attachmentId":"provider-attachment"}}]}})
            } else {
                panic!("unexpected synthetic path {path}")
            };
            let body = body.to_string();
            let reply = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            io.write_all(reply.as_bytes()).await.unwrap();
        }
    });
    (
        GoogleAdapter::synthetic_loopback(address).unwrap(),
        mode,
        paths,
        task,
    )
}
fn grant(background: bool) -> policy::LocalGrant {
    let device = Device {
        owner: moosuri_vault_protocol::Owner {
            tenant: Uuid::new_v4(),
            account: Uuid::new_v4(),
            installation: Uuid::new_v4(),
            epoch: 1,
        },
        id: Uuid::new_v4(),
        generation: 1,
    };
    policy::LocalGrant::select_google_model_read(
        device,
        Uuid::new_v4(),
        Uuid::new_v4(),
        GoogleIdentity {
            subject: "synthetic-subject".into(),
            email: "owner@example.test".into(),
        },
        1,
        1,
        current_time().unwrap() + 300,
        background,
    )
    .unwrap()
}
fn request(read: MailModelRead) -> MailModelRequest {
    MailModelRequest {
        version: 1,
        background: false,
        read,
    }
}
#[tokio::test]
async fn synthetic_native_search_page_message_thread_and_selected_attachment_keep_provider_ids_local()
 {
    let (adapter, mode, paths, task) = provider().await;
    let g = grant(false);
    let expires = current_time().unwrap() + 120;
    let mut selected = Selections::default();
    let search = request(MailModelRead::Search {
        query: "newer_than:7d quote".into(),
        cursor: None,
    });
    let result = adapter
        .model_read(&g, &search, SECRET, expires, &mut selected, None)
        .await
        .unwrap();
    assert!(result.valid());
    assert_eq!(result.messages.len(), 1);
    assert_eq!(result.messages[0].text, "Synthetic invoice total 42");
    let output = String::from_utf8(result.bytes()).unwrap();
    for hidden in [
        ACCESS,
        "synthetic-native-model-refresh",
        "synthetic-native-model-secret",
        "provider-message",
        "provider-thread",
        "provider-cursor",
        "provider-attachment",
    ] {
        assert!(!output.contains(hidden), "leaked {hidden}");
    }
    let mail = result.messages[0].clone();
    let page = request(MailModelRead::Search {
        query: "newer_than:7d quote".into(),
        cursor: result.next_cursor,
    });
    adapter
        .model_read(&g, &page, SECRET, expires, &mut selected, None)
        .await
        .unwrap();
    assert!(
        paths
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("pageToken=provider-cursor"))
    );
    let wrong = request(MailModelRead::Search {
        query: "different".into(),
        cursor: result.next_cursor,
    });
    assert!(matches!(
        adapter
            .model_read(&g, &wrong, SECRET, expires, &mut selected, None)
            .await,
        Err(Error::Request)
    ));
    let body = adapter
        .model_read(
            &g,
            &request(MailModelRead::Message {
                selection: mail.selection,
            }),
            SECRET,
            expires,
            &mut selected,
            None,
        )
        .await
        .unwrap();
    assert_eq!(body.messages[0].selection, mail.selection);
    let thread = adapter
        .model_read(
            &g,
            &request(MailModelRead::Thread {
                selection: mail.thread,
            }),
            SECRET,
            expires,
            &mut selected,
            None,
        )
        .await
        .unwrap();
    assert_eq!(thread.messages[0].thread, mail.thread);
    let attachment = mail.attachments[0].selection;
    let first = adapter
        .model_read(
            &g,
            &request(MailModelRead::Attachment {
                selection: attachment,
                page: 0,
            }),
            SECRET,
            expires,
            &mut selected,
            None,
        )
        .await
        .unwrap()
        .attachment
        .unwrap();
    assert_eq!(first.total_pages, 2);
    assert!(first.partial);
    let second = adapter
        .model_read(
            &g,
            &request(MailModelRead::Attachment {
                selection: attachment,
                page: 1,
            }),
            SECRET,
            expires,
            &mut selected,
            None,
        )
        .await
        .unwrap()
        .attachment
        .unwrap();
    assert_eq!(first.content_digest, second.content_digest);
    assert!(second.text.len() < 4096);
    // A provider revision cannot silently splice a new file into the old page selection.
    mode.store(4, std::sync::atomic::Ordering::SeqCst);
    assert!(
        adapter
            .model_read(
                &g,
                &request(MailModelRead::Attachment {
                    selection: attachment,
                    page: 1
                }),
                SECRET,
                expires,
                &mut selected,
                None
            )
            .await
            .is_err()
    );
    task.abort();
}
#[tokio::test]
async fn synthetic_scope_account_echo_and_background_are_distinct_failures() {
    let (adapter, mode, paths, task) = provider().await;
    let g = grant(false);
    let expires = current_time().unwrap() + 120;
    let mut selections = Selections::default();
    let read = request(MailModelRead::Search {
        query: "quote".into(),
        cursor: None,
    });
    mode.store(1, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        adapter
            .model_read(&g, &read, SECRET, expires, &mut selections, None)
            .await,
        Err(Error::GoogleScope)
    ));
    mode.store(2, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        adapter
            .model_read(&g, &read, SECRET, expires, &mut selections, None)
            .await,
        Err(Error::GoogleAccount)
    ));
    mode.store(3, std::sync::atomic::Ordering::SeqCst);
    assert!(matches!(
        adapter
            .model_read(&g, &read, SECRET, expires, &mut selections, None)
            .await,
        Err(Error::ProviderContent)
    ));
    let calls = paths.lock().unwrap().len();
    let mut background = read.clone();
    background.background = true;
    assert!(matches!(
        adapter
            .model_read(&g, &background, SECRET, expires, &mut selections, None)
            .await,
        Err(Error::Policy)
    ));
    assert_eq!(calls, paths.lock().unwrap().len());
    task.abort();
}
#[test]
fn appended_model_read_preserves_all_legacy_operation_and_projection_wire_vectors() {
    // Independent Python struct.pack / SHA256 vectors, not derived by this encoder.
    let device = Device {
        owner: moosuri_vault_protocol::Owner {
            tenant: Uuid::from_u128(1),
            account: Uuid::from_u128(2),
            installation: Uuid::from_u128(3),
            epoch: 1,
        },
        id: Uuid::from_u128(4),
        generation: 1,
    };
    let base = Request {
        device,
        invocation: Invocation {
            run: Uuid::from_u128(5),
            step: 1,
            call: Uuid::from_u128(6),
            nonce: Uuid::from_u128(7),
        },
        binding: Binding {
            id: Uuid::from_u128(8),
            epoch: 1,
            grant_version: 1,
            grant_hash: [9; 32],
            item_epoch: 1,
            key_epoch: 2,
            expires_at: 300,
        },
        operation: Operation::FixedProbe,
        repository: None,
        digest: [10; 32],
        expires_at: 200,
    };
    let hex = |bytes: &[u8]| {
        auth::hash(bytes)
            .iter()
            .map(|v| format!("{v:02x}"))
            .collect::<String>()
    };
    let operations = [
        Operation::FixedProbe,
        Operation::GitHubRepositoryRead,
        Operation::GoogleInboxRead,
        Operation::GoogleInboxPreview,
        Operation::GoogleInboxBody {
            selection: Uuid::from_u128(11),
        },
        Operation::GoogleMailSend {
            effect: Uuid::from_u128(12),
            execution: [13; 32],
        },
        Operation::GoogleMailLookup {
            effect: Uuid::from_u128(12),
            execution: [13; 32],
        },
    ];
    let vectors = [
        "caff897552a8cf9ee3573a302d93d51482fbf3ea539f15564b5af7c62a58b424",
        "b28ee31362c3707dde96e8711bf1587ced7def13f05258d1bc0c8de530505e96",
        "d49288abac510a4fdecbf8b541e3a2a3394b340409d98564b629c8ff5d869a0f",
        "c66862f7f06d6e09f117b5eb32f9d3a5e04e88f1b665bc4495b11c00f2e5afb0",
        "3491348b8805b1315c0458309fd78ba677b10831ea73ea8faf9f01c8ddc2ee4b",
        "d573207e7d25b3b9f7ca97d7e57b55857ce8c63a5fb3b59be4fd015a9d687902",
        "ba771a077364001dbbe1712d92b112ba6b6b668e3d4146fd9a4a209b63e24863",
    ];
    for (operation, expected) in operations.into_iter().zip(vectors) {
        let mut r = base.clone();
        r.operation = operation;
        if operation == Operation::GitHubRepositoryRead {
            r.repository = Some(GitHubRepository {
                owner: "org".into(),
                repository: "repo".into(),
            });
        }
        assert_eq!(hex(&request_bytes(&r)), expected);
    }
    let preview = |selection| GoogleMessagePreview {
        selection,
        subject: "subject".into(),
        from: "from".into(),
        date: "date".into(),
        snippet: "snippet".into(),
        truncated: false,
    };
    let projections = [
        Projection::None,
        Projection::FixedProbeOk,
        Projection::GitHubRepository {
            id: 42,
            private: true,
            archived: false,
            stars: 3,
            open_issues: 4,
        },
        Projection::GoogleInbox {
            messages_total: 10,
            messages_unread: 2,
            threads_total: 5,
            threads_unread: 1,
        },
        Projection::GoogleInboxPreview {
            messages: vec![preview(None)],
            unavailable: 0,
            more_available: true,
        },
        Projection::GoogleInboxBody {
            text: "body".into(),
            truncated: false,
            unavailable: false,
        },
        Projection::GoogleInboxPreview {
            messages: vec![preview(Some(Uuid::from_u128(11)))],
            unavailable: 0,
            more_available: true,
        },
        Projection::GoogleMailSent {
            message_id: "m1".into(),
            thread_id: "t1".into(),
        },
        Projection::GoogleMailObserved {
            status: GoogleSentStatus::Matching,
            candidates: 1,
            message_id: Some("m1".into()),
            thread_id: Some("t1".into()),
        },
    ];
    let vectors = [
        "d3b4943575939bfa376a6df97cbd59faf9719e1771f0144f372a7841cc48234a",
        "5df7aabde509c028c7a18196a22473279f12ec134f7e5b4cf85f567b7cceb2f3",
        "1f4f79039e113b626689477af873f43ca07c2d7e09ec1871f08cec08b3ec89b7",
        "6d9d076063a7b88ae3def01131469faa9b66728177bb294c92466a854904096a",
        "cf649d8ea5ef114cae43dc56e8d7a9864fd210b09aac7ccf6e193bf07498b7dd",
        "2fa9c6f5b17ea9462374617d53afaef2232575c1e4d2e8cbd91f07d8f4fbeaf8",
        "20bd86214b13ea1ee87766619124cc94e27222d7ce5ffc2463c5f16495b45f22",
        "ac7c7acde33b5aa370690244206256f08fa905478e5c82de4305e52c51ba7f58",
        "b5931029e39a35e77da2af03d34d9fe8b0ba42855cc3b5a10506fd1768a7c26a",
    ];
    for (projection, expected) in projections.into_iter().zip(vectors) {
        assert_eq!(hex(&projection.bytes()), expected);
    }
}
#[tokio::test]
async fn selected_pdf_requires_explicit_device_local_parser_and_uses_bounded_child_projection() {
    use std::os::unix::fs::PermissionsExt;
    let (adapter, mode, paths, task) = provider().await;
    mode.store(5, std::sync::atomic::Ordering::SeqCst);
    let g = grant(false);
    let expires = current_time().unwrap() + 120;
    let mut selections = Selections::default();
    let result = adapter
        .model_read(
            &g,
            &request(MailModelRead::Search {
                query: "invoice".into(),
                cursor: None,
            }),
            SECRET,
            expires,
            &mut selections,
            None,
        )
        .await
        .unwrap();
    let id = result.messages[0].attachments[0].selection;
    assert_eq!(
        result.messages[0].attachments[0].media_type,
        "application/pdf"
    );
    let read = request(MailModelRead::Attachment {
        selection: id,
        page: 0,
    });
    let unavailable = adapter
        .model_read(&g, &read, SECRET, expires, &mut selections, None)
        .await
        .unwrap();
    assert!(unavailable.status == MailModelStatus::ParserUnavailable);
    assert!(
        !paths
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("/attachments/"))
    );
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("synthetic-pdf-child");
    std::fs::write(&path,b"#!/bin/sh\ncat >/dev/null\nprintf '{\"Ok\":{\"pages\":1,\"text\":\"Synthetic parsed PDF invoice 42\"}}'\n").unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let result = adapter
        .model_read(&g, &read, SECRET, expires, &mut selections, Some(&path))
        .await
        .unwrap();
    let page = result.attachment.unwrap();
    assert_eq!(page.text, "Synthetic parsed PDF invoice 42");
    assert!(
        paths
            .lock()
            .unwrap()
            .iter()
            .any(|p| p.contains("/attachments/"))
    );
    task.abort();
}
async fn engine_read(
    engine: &mut Engine,
    g: &policy::LocalGrant,
    adapter: &GoogleAdapter,
    payload: MailModelRequest,
) -> SignedReceipt {
    let now = current_time().unwrap();
    let r = Request {
        device: g.device.clone(),
        invocation: Invocation {
            run: Uuid::new_v4(),
            step: 1,
            call: Uuid::new_v4(),
            nonce: Uuid::new_v4(),
        },
        binding: g.binding.clone(),
        operation: Operation::GoogleMailModelRead {
            selection: Uuid::new_v4(),
        },
        repository: None,
        digest: auth::hash(&payload.bytes()),
        expires_at: now + 120,
    };
    engine
        .prepare(r.clone(), now, g.binding.item_epoch, g.binding.key_epoch)
        .unwrap();
    engine
        .attach_model_mail_payload(&r.invocation, payload)
        .unwrap();
    engine
        .accept_commitment(Commitment {
            request: r.clone(),
            nonce: Uuid::new_v4(),
        })
        .unwrap();
    let signed = engine
        .execute_model_mail_secret(
            &r.invocation,
            adapter,
            SECRET,
            g.binding.item_epoch,
            g.binding.key_epoch,
        )
        .await
        .unwrap();
    auth::verify(
        &engine.key.public_key(),
        &receipt_bytes(&signed.receipt),
        &signed.signature,
    )
    .unwrap();
    signed
}
#[tokio::test]
async fn signed_invalid_query_kind_or_stale_selector_keeps_grant_usable_but_scope_account_failure_requires_reconnect()
 {
    let (adapter, mode, paths, task) = provider().await;
    let g = grant(false);
    let tmp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let key = auth::SigningKey::generate_pkcs8().unwrap();
    let mut engine = Engine::enroll(
        &tmp.path().join("device"),
        g.device.clone(),
        "https://assistant.example.test".into(),
        &key,
    )
    .unwrap();
    engine.select(g.clone()).unwrap();
    let first = engine_read(
        &mut engine,
        &g,
        &adapter,
        request(MailModelRead::Search {
            query: "query A".into(),
            cursor: None,
        }),
    )
    .await;
    let Projection::GoogleMailModelRead { result } = first.receipt.projection else {
        panic!("search")
    };
    let cursor = result.next_cursor.unwrap();
    let message = result.messages[0].selection;
    let thread = result.messages[0].thread;
    for bad in [
        MailModelRead::Search {
            query: "query B".into(),
            cursor: Some(cursor),
        },
        MailModelRead::Message { selection: thread },
        MailModelRead::Message {
            selection: Uuid::new_v4(),
        },
    ] {
        let calls = paths.lock().unwrap().len();
        let signed = engine_read(&mut engine, &g, &adapter, request(bad)).await;
        assert!(signed.receipt.outcome == Outcome::NotDispatched);
        assert_eq!(paths.lock().unwrap().len(), calls);
    }
    let valid = engine_read(
        &mut engine,
        &g,
        &adapter,
        request(MailModelRead::Message { selection: message }),
    )
    .await;
    assert!(valid.receipt.outcome == Outcome::ProjectedSuccess);
    // A selector from the previous credential/grant version is stale source input.
    let mut rotated = g.clone();
    rotated.refresh_archive_google(2, 1).unwrap();
    rotated.confirm_refresh();
    engine.state.grants[0] = rotated.clone();
    engine.save().unwrap();
    let calls = paths.lock().unwrap().len();
    let stale = engine_read(
        &mut engine,
        &rotated,
        &adapter,
        request(MailModelRead::Message { selection: message }),
    )
    .await;
    assert!(stale.receipt.outcome == Outcome::NotDispatched);
    assert_eq!(paths.lock().unwrap().len(), calls);
    let fresh = engine_read(
        &mut engine,
        &rotated,
        &adapter,
        request(MailModelRead::Search {
            query: "fresh after credential rotation".into(),
            cursor: None,
        }),
    )
    .await;
    assert!(fresh.receipt.outcome == Outcome::ProjectedSuccess);
    for failure in [1, 2] {
        let mut failure_engine = Engine::enroll(
            &tmp.path().join(format!("credential-failure-{failure}")),
            g.device.clone(),
            "https://assistant.example.test".into(),
            &key,
        )
        .unwrap();
        failure_engine.select(g.clone()).unwrap();
        mode.store(failure, std::sync::atomic::Ordering::SeqCst);
        let failed = engine_read(
            &mut failure_engine,
            &g,
            &adapter,
            request(MailModelRead::Search {
                query: "fresh".into(),
                cursor: None,
            }),
        )
        .await;
        assert!(failed.receipt.outcome == Outcome::Rejected);
        let next = Request {
            device: g.device.clone(),
            invocation: Invocation {
                run: Uuid::new_v4(),
                step: 1,
                call: Uuid::new_v4(),
                nonce: Uuid::new_v4(),
            },
            binding: g.binding.clone(),
            operation: Operation::GoogleMailModelRead {
                selection: Uuid::new_v4(),
            },
            repository: None,
            digest: [1; 32],
            expires_at: current_time().unwrap() + 60,
        };
        assert!(matches!(
            failure_engine.prepare(next, current_time().unwrap(), 1, 1),
            Err(Error::Policy)
        ));
    }
    task.abort();
}
