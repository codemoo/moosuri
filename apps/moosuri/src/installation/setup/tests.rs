use super::*;
#[test]
fn local_service_arguments_never_expose_listener_or_trust_forwarded_headers() {
    let i = Installation {
        format: 1,
        id: "synthetic".into(),
        data_dir: PathBuf::from("/private/synthetic-data"),
        origin: "http://127.0.0.1:3411".into(),
        network: Network::Local,
        trust_key: "ab".repeat(32),
        managed: BTreeMap::new(),
    };
    let s = State {
        format: 1,
        language: Language::Ko,
        port: 3411,
        service: service::Kind::None,
        launcher: None,
        service_file: None,
        next_page: "/app".into(),
        removing: false,
        autostart: true,
        desktop_files: Vec::new(),
        tailscale_requested: false,
    };
    let args = server_args(&i, &s);
    assert!(args.windows(2).any(|w| w == ["--bind", "127.0.0.1:3411"]));
    assert!(args.contains(&"--dev".into()));
    assert!(!args.contains(&"--trusted-proxy".into()));
}
#[test]
fn setup_state_rejects_external_navigation_and_unsafe_ports() {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let mut s = State {
        format: 1,
        language: Language::En,
        port: 3210,
        service: service::Kind::None,
        launcher: None,
        service_file: None,
        next_page: "/app".into(),
        removing: false,
        autostart: true,
        desktop_files: Vec::new(),
        tailscale_requested: false,
    };
    save(t.path(), &s).unwrap();
    assert!(load(t.path()).is_ok());
    s.next_page = "https://unrelated.example.test".into();
    save(t.path(), &s).unwrap();
    assert!(load(t.path()).is_err());
    s.next_page = "/app".into();
    s.port = 80;
    save(t.path(), &s).unwrap();
    assert!(load(t.path()).is_err());
}
#[test]
fn local_network_cannot_be_used_to_admit_public_http() {
    for origin in [
        "http://0.0.0.0:3210",
        "http://assistant.example.test",
        "http://127.0.0.1:3210/path",
        "http://user:pass@127.0.0.1:3210",
    ] {
        assert!(
            moosuri_server::ServerConfig::new(origin, true).is_err(),
            "{origin}"
        );
    }
    assert!(moosuri_server::ServerConfig::new("http://127.0.0.1:3210", true).is_ok());
}

#[test]
fn stop_confirmation_waits_for_runtime_shutdown_and_reports_a_bounded_timeout() {
    let t = tempfile::Builder::new()
        .permissions(std::os::unix::fs::PermissionsExt::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    create(&t.path().join("installation.lock"), b"", false).unwrap();
    let running = lock(t.path(), false).unwrap();
    assert_eq!(
        stopped_runtime(t.path(), Duration::from_millis(50))
            .err()
            .unwrap(),
        "setup_service_stop_pending_check_moosuri_status"
    );
    let shutdown = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        drop(running);
    });
    let stopped = stopped_runtime(t.path(), Duration::from_secs(1)).unwrap();
    assert!(
        lock(t.path(), false).is_err(),
        "removal retains exclusive runtime admission"
    );
    drop(stopped);
    shutdown.join().unwrap();
}
