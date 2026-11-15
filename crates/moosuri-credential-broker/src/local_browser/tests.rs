use super::*;
use moosuri_vault_protocol::{Owner, broker::Device};

fn engine() -> (tempfile::TempDir, PathBuf, Engine) {
    let tmp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let root = tmp.path().join("device");
    let key = crate::auth::SigningKey::generate_pkcs8().unwrap();
    let device = Device {
        owner: Owner {
            tenant: Uuid::new_v4(),
            account: Uuid::new_v4(),
            installation: Uuid::new_v4(),
            epoch: 1,
        },
        id: Uuid::new_v4(),
        generation: 1,
    };
    let engine =
        Engine::enroll(&root, device, "https://assistant.example.test".into(), &key).unwrap();
    (tmp, root, engine)
}

fn attach_fixture(engine: &mut Engine, now: i64) -> UnixStream {
    let mut record = Record::new(now).unwrap();
    record.phase = Phase::AwaitingHuman;
    let (pipe, peer) = UnixStream::pair().unwrap();
    engine.local_browser = Some(Browser {
        process: Arc::new(Mutex::new(None)),
        watchdog: tokio::spawn(std::future::pending()),
        pipe,
        next: 0,
        deadline: Arc::new(Mutex::new(Instant::now() + Duration::from_secs(300))),
    });
    engine.state.local_browser = Some(record);
    engine.save().unwrap();
    peer
}

async fn reply(peer: &mut UnixStream, number: u64) {
    let mut request = Vec::new();
    loop {
        let byte = peer.read_u8().await.unwrap();
        if byte == 0 {
            break;
        }
        request.push(byte);
    }
    let value: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(
        value,
        json!({"id":number,"method":"Target.getTargets","params":{}})
    );
    let result = json!({"id":number,"result":{"targetInfos":[{
        "type":"page", "title":"secret-password-canary", "url":"https://example.test/callback?token=secret-cookie-canary",
        "targetId":"private-cdp-canary"
    }]}});
    peer.write_all(format!("{result}\0").as_bytes())
        .await
        .unwrap();
}

#[tokio::test]
async fn local_human_confirmation_observes_without_echo_or_polling_renewal() {
    let (_tmp, root, mut engine) = engine();
    let mut peer = attach_fixture(&mut engine, 1000);
    let server = tokio::spawn(async move {
        reply(&mut peer, 1).await;
        reply(&mut peer, 2).await;
    });
    let first = engine.browser_observe(true, 1001).await.unwrap();
    assert_eq!(first.phase, Phase::Ready);
    assert_eq!(first.page_count, Some(1));
    assert!(!first.site_adapter_available);
    let next = engine.browser_observe(false, 1100).await.unwrap();
    assert_eq!(next.idle_until, first.idle_until);
    assert_eq!(next.expires_at, first.expires_at);
    assert!(engine.browser_observe(true, 1101).await.is_err());
    for bytes in [
        serde_json::to_string(&next).unwrap(),
        fs::read_to_string(root.join("journal")).unwrap(),
    ] {
        for secret in [
            "secret-password-canary",
            "secret-cookie-canary",
            "private-cdp-canary",
            "example.test/callback",
        ] {
            assert!(!bytes.contains(secret));
        }
    }
    assert!(
        engine
            .browser_observe(false, first.idle_until)
            .await
            .is_err()
    );
    assert_eq!(
        engine.state.local_browser.as_ref().unwrap().phase,
        Phase::Expired
    );
    assert!(engine.local_browser.is_none());
    server.await.unwrap();
}

#[tokio::test]
async fn local_reopen_preserves_deadlines_and_never_replays_or_assumes_process_exit() {
    let (_tmp, root, mut engine) = engine();
    let _peer = attach_fixture(&mut engine, 1000);
    let original = engine.state.local_browser.as_ref().unwrap().clone();
    drop(engine);
    let mut engine = Engine::open_local(&root).unwrap();
    assert!(engine.local_browser.is_none());
    let retained = engine.state.local_browser.as_ref().unwrap();
    assert_eq!(retained.id, original.id);
    assert_eq!(retained.expires_at, original.expires_at);
    assert_eq!(retained.idle_until, original.idle_until);
    assert_eq!(retained.phase, Phase::RecoveryRequired);
    assert!(engine.browser_observe(true, 1002).await.is_err());
    // A live retained process group is never attached to or killed by old PID.
    engine.state.local_browser.as_mut().unwrap().process_group =
        Some(unsafe { libc::getpgrp() } as u32);
    assert_eq!(
        engine.browser_close(1003).await.unwrap_err(),
        Error::Unknown
    );
    engine.state.local_browser.as_mut().unwrap().process_group = None;
    engine.state.local_browser.as_mut().unwrap().phase = Phase::Starting;
    assert_eq!(
        engine.browser_close(1003).await.unwrap_err(),
        Error::Unknown
    );
    // Missing/corrupt journal cannot become a fresh enrollment/recovery.
    drop(engine);
    fs::write(root.join("journal.head"), b"broken").unwrap();
    assert!(Engine::open_local(&root).is_err());
}

#[tokio::test]
async fn local_handoff_timeout_clock_rollback_and_revocation_fence_observation() {
    for mode in 0..3 {
        let (_tmp, _root, mut engine) = engine();
        let _peer = attach_fixture(&mut engine, 1000);
        let now = match mode {
            0 => 1300,
            1 => 999,
            _ => {
                engine.revoke().unwrap();
                1001
            }
        };
        assert!(engine.browser_observe(true, now).await.is_err());
        assert!(engine.local_browser.is_none());
    }
}

/// Real headed Chromium acceptance is explicitly local/opt-in. It navigates only
/// the new private about:blank window and never connects to a real account/site.
#[tokio::test]
#[ignore = "set MOOSURI_NATIVE_BROWSER_CHROMIUM to an explicitly trusted local executable"]
async fn actual_local_chromium_pipe_blank_window_and_cleanup() {
    let executable = PathBuf::from(std::env::var_os("MOOSURI_NATIVE_BROWSER_CHROMIUM").unwrap());
    let (_tmp, _root, mut engine) = engine();
    let started = engine
        .browser_start(&executable, false, 1000)
        .await
        .unwrap();
    assert_eq!(started.phase, Phase::AwaitingHuman);
    let observed = engine.browser_observe(true, 1001).await.unwrap();
    assert_eq!(observed.page_count, Some(1));
    assert!(!observed.site_adapter_available);
    let profile = engine
        .state
        .local_browser
        .as_ref()
        .unwrap()
        .profile(&engine.local_root);
    engine.browser_close(1002).await.unwrap();
    assert!(!profile.exists());
}
