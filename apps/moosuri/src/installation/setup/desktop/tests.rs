use super::*;
#[test]
fn loaded_or_autostart_enabled_never_proves_server_readiness() {
    assert_eq!(
        classify(false, Observation::Unavailable, true),
        Phase::Stopped
    );
    assert_eq!(classify(true, Observation::Unavailable, true), Phase::Error);
    assert_eq!(classify(true, Observation::Ready, true), Phase::Ready);
    assert_eq!(
        classify(false, Observation::Unavailable, false),
        Phase::Error
    );
}
#[test]
fn desktop_and_control_locks_are_independent_and_single_instance() {
    let temp = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(
        temp.path(),
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )
    .unwrap();
    let desktop = named_lock(temp.path(), "desktop.lock").unwrap();
    let control = control_lock(temp.path()).unwrap();
    assert!(named_lock(temp.path(), "desktop.lock").is_err());
    assert!(control_lock(temp.path()).is_err());
    drop(desktop);
    // Concurrent tests spawn processes. Between fork and exec a child can hold
    // a transient copy of this CLOEXEC descriptor even after its parent drops it.
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match named_lock(temp.path(), "desktop.lock") {
            Ok(_) => break,
            Err(e) if e == "setup_control_busy" && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => panic!("released desktop lock remained unavailable: {e}"),
        }
    }
    drop(control);
}

#[test]
fn browser_dispatch_returns_while_the_viewer_is_still_running() {
    let start = Instant::now();
    assert!(dispatch_detached(
        Process::new("/bin/sh").args(["-c", "sleep 2"])
    ));
    assert!(start.elapsed() < Duration::from_secs(1));
}

#[test]
fn embedded_product_icon_decodes_to_the_tray_dimensions() {
    assert_eq!(icon_rgba().len(), 24 * 24 * 4);
}

#[test]
fn quit_cannot_be_lost_behind_a_queued_action_and_web_actions_require_readiness() {
    let (sender, receiver) = mpsc::sync_channel(1);
    let c = Controller {
        sender,
        state: Arc::new(Mutex::new(Snapshot::default())),
        quitting: Arc::new(AtomicBool::new(false)),
        language: Language::En,
    };
    c.send(Action::Chat);
    assert!(receiver.try_recv().is_err());
    set_state(&c.state, |s| {
        s.phase = Phase::Stopped;
        s.controllable = true;
    });
    c.send(Action::Chat);
    assert!(matches!(receiver.try_recv(), Ok(Action::Chat)));
    c.send(Action::Admin);
    assert!(receiver.try_recv().is_err());
    set_state(&c.state, |s| {
        s.phase = Phase::Error;
        s.detail = "port_conflict";
    });
    c.send(Action::Chat);
    assert!(receiver.try_recv().is_err());
    assert_eq!(
        classify(false, Observation::OtherServer, true),
        Phase::Error
    );
    set_state(&c.state, |s| s.phase = Phase::Ready);
    c.send(Action::Admin);
    c.send(Action::Quit);
    assert!(c.quitting.load(Ordering::Acquire));
    assert!(c.snapshot().exit);
    assert!(matches!(receiver.try_recv(), Ok(Action::Admin)));
    assert!(
        !diagnostics::hint(
            diagnostics::classify("private-token=synthetic"),
            Language::En
        )
        .contains("synthetic")
    );
}
