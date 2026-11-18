use super::*;
use moosuri_connectors::model::protocol::{FinishOutcome, TextMessage, TextRole, Usage};
use std::os::unix::process::CommandExt;
use std::process::{Command, Stdio};

fn pin() -> Hello {
    Hello {
        version: 1,
        route: Route::ClaudeCode,
        owner: Owner {
            account_id: Uuid::from_u128(1),
            tenant_id: Uuid::from_u128(2),
            volume_id: Uuid::from_u128(3),
            generation: 1,
        },
        revision: Uuid::from_u128(4),
        model: "synthetic-test".into(),
        effort: None,
    }
}
fn prompt(text: &str) -> TextRequest {
    TextRequest {
        model: pin().model,
        messages: vec![TextMessage {
            role: TextRole::User,
            text: text.into(),
            phase: None,
        }],
        max_output_tokens: 128,
        effort: None,
    }
}
pub(super) async fn fixture(
    req: &TextRequest,
    tx: &mpsc::Sender<ModelEvent>,
    _: &CancellationToken,
) -> Result<(), String> {
    let prefix = &req.messages[0].text;
    tx.send(ModelEvent::TextDelta {
        text: format!("answer {prefix}"),
        item_id: None,
        phase: None,
    })
    .await
    .map_err(|_| "closed")?;
    tx.send(ModelEvent::UsageSnapshot(Usage {
        input_tokens: Some(7),
        output_tokens: Some(3),
        ..Default::default()
    }))
    .await
    .map_err(|_| "closed")?;
    tx.send(ModelEvent::Finished {
        outcome: FinishOutcome::Completed,
        native_reason: "synthetic".into(),
    })
    .await
    .map_err(|_| "closed")?;
    if prefix == "after-terminal" {
        // A provider blocked on output must be released when the relay rejects
        // the stream, even if the provider only observes channel closure.
        for _ in 0..64 {
            tx.send(ModelEvent::TextDelta {
                text: "late".into(),
                item_id: None,
                phase: None,
            })
            .await
            .map_err(|_| "closed")?;
        }
    }
    if prefix == "broken" {
        return Err("synthetic_bad_suffix".into());
    }
    Ok(())
}

#[tokio::test]
async fn validated_channel_preserves_usage_and_rejects_wrong_owner_or_broken_terminal() {
    let fixture = Arc::new(Worker {
        pin: pin(),
        models: model_catalog(&pin().model, None, &[]).unwrap(),
        adapter: Adapter::Fixture,
    });
    let stop = CancellationToken::new();
    let uid = unsafe { libc::geteuid() };
    for scenario in ["different_member", "ok", "broken", "after-terminal"] {
        let (stream, server) = UnixStream::pair().unwrap();
        let worker = tokio::spawn(handle(server, uid, fixture.clone(), stop.clone()));
        let peer = Connection { stream, pin: None };
        let h = pin();
        let mut caller = h.owner;
        if scenario == "different_member" {
            caller.account_id = Uuid::from_u128(9);
        }
        let connected = peer
            .authorize(h.route, caller, h.revision, &h.model, h.effort.as_deref())
            .await;
        if scenario == "different_member" {
            assert!(connected.is_err());
            worker.await.unwrap().unwrap();
            continue;
        }
        let (tx, mut rx) = mpsc::channel(8);
        let answer = connected
            .unwrap()
            .stream(h.route, caller, h.revision, &prompt(scenario), &tx, &stop)
            .await;
        drop(tx);
        let mut text = None;
        let mut usage = None;
        let mut terminal = false;
        while let Some(e) = rx.recv().await {
            match e {
                ModelEvent::TextDelta { text: delta, .. } => text = Some(delta),
                ModelEvent::UsageSnapshot(u) => usage = Some(u),
                ModelEvent::Finished { .. } => terminal = true,
                _ => panic!("unexpected worker event"),
            }
        }
        assert_eq!(text, Some(format!("answer {scenario}")));
        assert_eq!(usage.unwrap().input_tokens, Some(7));
        assert_eq!(answer.is_ok(), scenario == "ok");
        assert_eq!(terminal, scenario == "ok");
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(2), worker)
                .await
                .unwrap()
                .unwrap()
                .is_ok(),
            scenario == "ok"
        );
    }
}

/// Launched by the Linux root test below under an isolated non-root UID.
#[tokio::test]
async fn dedicated_service_identity_child() {
    let Some(root) = std::env::var_os("MOOSURI_TEST_ENGINE_SERVICE_DIRECTORY") else {
        return;
    };
    let uid = unsafe { libc::geteuid() };
    assert_ne!(uid, 0);
    let (listener, _lock) = bind(Path::new(&root), uid).await.unwrap();
    let (stream, _) = listener.accept().await.unwrap();
    handle(
        stream,
        0,
        Arc::new(Worker {
            pin: pin(),
            models: model_catalog(&pin().model, None, &[]).unwrap(),
            adapter: Adapter::Fixture,
        }),
        CancellationToken::new(),
    )
    .await
    .unwrap();
}
#[test]
fn linux_root_engine_and_separate_service_uid_use_one_verified_socket() {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return;
    }
    let tmp = tempfile::tempdir_in("/tmp").unwrap();
    let root = tmp.path().canonicalize().unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o711)).unwrap();
    let dir = root.join("service");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let uid = 10071;
    std::os::unix::fs::chown(&dir, Some(uid), Some(uid)).unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .uid(uid)
        .gid(uid)
        .env_clear()
        .env("MOOSURI_TEST_ENGINE_SERVICE_DIRECTORY", &dir)
        .args([
            "--exact",
            "engine_login::tests::dedicated_service_identity_child",
        ])
        .stdin(Stdio::null())
        .spawn()
        .unwrap();
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let result = rt.block_on(async {
        tokio::time::timeout(Duration::from_secs(15), async {
            let socket = dir.join("engine.sock");
            while !fs::symlink_metadata(&socket).is_ok_and(|m| m.file_type().is_socket()) {
                if child.try_wait().unwrap().is_some() {
                    return Err("worker exited".into());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
            let wrong = Endpoint {
                socket: socket.clone(),
                worker_uid: uid + 1,
            };
            assert!(Connection::connect(&wrong).await.is_err());
            let endpoint = Endpoint {
                socket,
                worker_uid: uid,
            };
            let h = pin();
            let (tx, mut rx) = mpsc::channel(8);
            Connection::connect(&endpoint)
                .await?
                .authorize(h.route, h.owner, h.revision, &h.model, h.effort.as_deref())
                .await?
                .stream(
                    h.route,
                    h.owner,
                    h.revision,
                    &prompt("linux"),
                    &tx,
                    &CancellationToken::new(),
                )
                .await?;
            drop(tx);
            let mut text = None;
            while let Some(e) = rx.recv().await {
                if let ModelEvent::TextDelta { text: s, .. } = e {
                    text = Some(s);
                }
            }
            assert_eq!(text.as_deref(), Some("answer linux"));
            Ok::<(), String>(())
        })
        .await
    });
    if result.is_err() || result.as_ref().is_ok_and(|v| v.is_err()) {
        let _ = child.kill();
    }
    let status = child.wait().unwrap();
    result.unwrap().unwrap();
    assert!(status.success());
}
