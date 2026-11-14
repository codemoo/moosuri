use super::super::tests::Fixture;
use super::*;
#[test]
fn publisher_fixed_argv_rejects_shell_and_option_injection() {
    for value in [
        "",
        "-oProxyCommand=x",
        "a;b",
        "a b",
        "a/b",
        "a@host",
        "a\nnext",
        "$(x)",
    ] {
        assert!(ssh_command(value, "target").is_err());
        assert!(ssh_command("host", value).is_err());
    }
    let c = ssh_command("backup-host", "target-1").unwrap();
    let args = c
        .get_args()
        .map(|s| s.to_str().unwrap())
        .collect::<Vec<_>>();
    assert!(args.contains(&"StrictHostKeyChecking=yes"));
    assert!(args.contains(&"ClearAllForwardings=yes"));
    assert!(args.ends_with(&[
        "--",
        "backup-host",
        "moosuri-vault-service",
        "recovery",
        "receive-journal",
        "--target-id",
        "target-1"
    ]));
}
#[test]
fn publisher_transport_requires_exact_ack_and_cancels_blocked_pipe_group() {
    let mut f = Fixture::new();
    f.db.close_session("next").unwrap();
    let mut frame = vec![];
    f.prepared().write_to(&mut frame).unwrap();
    let ack = receive(&f.target, frame.as_slice()).unwrap();
    let ack_path = f.scratch.join("synthetic-ack.json");
    write_new(&ack_path, &encode(&ack).unwrap()).unwrap();
    let mut cmd = Command::new("/bin/sh");
    // Fixed fixture script; only the separately supplied argv path is variable.
    cmd.args(["-c", "cat >/dev/null; cat \"$1\"", "fixture"])
        .arg(&ack_path);
    let result = transfer_command(
        f.prepared(),
        cmd,
        Arc::new(AtomicBool::new(false)),
        Duration::from_secs(5),
    )
    .unwrap();
    assert_eq!(result, ack);
    let mut wrong = ack.clone();
    wrong.journal_size += 1;
    fs::write(&ack_path, encode(&wrong).unwrap()).unwrap();
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "cat >/dev/null; cat \"$1\"", "fixture"])
        .arg(&ack_path);
    assert!(
        transfer_command(
            f.prepared(),
            cmd,
            Arc::new(AtomicBool::new(false)),
            Duration::from_secs(5)
        )
        .is_err()
    );
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 10"]);
    let started = Instant::now();
    assert!(
        transfer_command(
            f.prepared(),
            cmd,
            Arc::new(AtomicBool::new(false)),
            Duration::from_millis(100)
        )
        .is_err()
    );
    assert!(started.elapsed() < Duration::from_secs(3));
    let cancel = Arc::new(AtomicBool::new(false));
    let cancelled = cancel.clone();
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 10"]);
    let prepared = f.prepared();
    let handle = std::thread::spawn(move || {
        transfer_command(prepared, cmd, cancelled, Duration::from_secs(10))
    });
    std::thread::sleep(Duration::from_millis(100));
    cancel.store(true, Ordering::Relaxed);
    assert!(handle.join().unwrap().is_err());
    assert!(!f.source.join("vault.sealed").exists());
    f.db.close_session("still-live").unwrap();
}
#[tokio::test]
async fn publisher_worker_rejects_bad_config_and_stops_before_admitting_capture() {
    let f = Fixture::new();
    let config = WorkerConfig {
        directory: f.scratch.clone(),
        recipient: f.recipient.clone(),
        snapshot_head: f.snapshot_head.clone(),
        ssh_target: "synthetic-host".into(),
        target_id: "synthetic".into(),
        interval_seconds: 30,
    };
    let db = Arc::new(Mutex::new(f.db));
    let mut bad = config.clone();
    bad.ssh_target = "bad;command".into();
    assert!(Worker::start(db.clone(), bad).is_err());
    let mut bad = config.clone();
    bad.directory = f.source.clone();
    assert!(Worker::start(db.clone(), bad).is_err());
    let mut bad = config.clone();
    bad.interval_seconds = 1;
    assert!(Worker::start(db.clone(), bad).is_err());
    // Stop before the spawned future first polls. This must not invoke SSH or
    // initialize/capture new source state, and must drain without a timer delay.
    let worker = Worker::start(db.clone(), config).unwrap();
    tokio::time::timeout(Duration::from_secs(2), worker.shutdown())
        .await
        .unwrap();
    db.lock().unwrap().close_session("after-shutdown").unwrap();
}

#[test]
fn publisher_capture_releases_exact_writer_before_stalled_transport_and_preserves_append_offset() {
    let f = Fixture::new();
    let source = f.source.clone();
    let config = WorkerConfig {
        directory: f.scratch.clone(),
        recipient: f.recipient.clone(),
        snapshot_head: f.snapshot_head.clone(),
        ssh_target: "synthetic-host".into(),
        target_id: "synthetic".into(),
        interval_seconds: 30,
    };
    let db = Arc::new(Mutex::new(f.db));
    let cancel = Arc::new(AtomicBool::new(false));
    let prepared = prepare(&db, &config, &cancel).unwrap();
    let captured_sequence = prepared.request.binding.sequence;
    let mut cmd = Command::new("/bin/sh");
    cmd.args(["-c", "sleep 10"]);
    let cancelled = cancel.clone();
    let handle = std::thread::spawn(move || {
        transfer_command(prepared, cmd, cancelled, Duration::from_secs(5))
    });
    {
        let mut writer = db
            .try_lock()
            .expect("transport must not retain HTTP Database mutex");
        writer.close_session("during-transport").unwrap();
        assert!(writer.sequence > captured_sequence);
    }
    cancel.store(true, Ordering::Relaxed);
    assert!(handle.join().unwrap().is_err());
    drop(db);
    let reopened = Database::open(&source).unwrap();
    assert_eq!(reopened.sequence, captured_sequence + 1);
    assert!(!source.join("vault.sealed").exists());
}
