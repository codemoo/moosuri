use moosuri_store::{
    Store,
    backup::{
        self,
        live::{self, Acceptance, ExportBinding},
    },
};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
const BIN: &str = env!("CARGO_BIN_EXE_moosuri");
fn dir(p: &Path, name: &str) -> PathBuf {
    let p = p.join(name);
    fs::DirBuilder::new().mode(0o700).create(&p).unwrap();
    p
}
#[tokio::test]
async fn actual_receiver_child_accepts_bounded_pipe_and_retry_returns_same_latest() {
    let temp = tempfile::Builder::new()
        .permissions(fs::Permissions::from_mode(0o700))
        .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
        .unwrap();
    let home = dir(temp.path(), "home");
    let kit = dir(temp.path(), "kit");
    let out = dir(temp.path(), "out");
    let source = temp.path().join("source");
    let artifacts = dir(temp.path(), "artifacts");
    let ledger = dir(temp.path(), "ledger");
    let store = Store::open(&source).unwrap();
    store
        .bootstrap(
            "Synthetic".into(),
            "child@example.test".into(),
            "Synthetic-child-password".into(),
            false,
        )
        .await
        .unwrap();
    drop(store);
    let key = kit.join("key");
    let recipient = backup::keygen(&key).unwrap();
    let cfg = backup::setup(&source, &recipient, &key).unwrap();
    let export = out.join("baseline.age");
    backup::create(&source, &export).unwrap();
    let init = Command::new(BIN)
        .env("HOME", &home)
        .args([
            "backup",
            "target-init",
            "--target-id",
            "synthetic",
            "--installation-id",
            &cfg.installation_id,
            "--artifacts",
            artifacts.to_str().unwrap(),
            "--ledger",
            ledger.to_str().unwrap(),
            "--export",
            export.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        init.status.success(),
        "{}",
        String::from_utf8_lossy(&init.stderr)
    );
    let store = Store::open(&source).unwrap();
    let captured = store
        .capture_recovery_journals(ExportBinding::open(&export).unwrap())
        .await
        .unwrap();
    let mut frame = vec![];
    captured.write_to(&mut frame).unwrap();
    let mut prior = None;
    for _ in 0..2 {
        let mut c = Command::new(BIN)
            .env("HOME", &home)
            .args(["backup", "receive-journal", "--target-id", "synthetic"])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let data = frame.clone();
        let mut input = c.stdin.take().unwrap();
        let writer = std::thread::spawn(move || {
            use std::io::Write;
            input.write_all(&data).unwrap();
        });
        let result = c.wait_with_output().unwrap();
        writer.join().unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        let a: Acceptance = serde_json::from_slice(&result.stdout).unwrap();
        assert_eq!(a.generation, 1);
        if let Some(prior) = &prior {
            assert_eq!(&a.trusted_head_sha256, prior)
        }
        prior = Some(a.trusted_head_sha256);
    }
    let status = Command::new(BIN)
        .env("HOME", &home)
        .args(["backup", "target-status", "--target-id", "synthetic"])
        .output()
        .unwrap();
    assert!(status.status.success());
    let value: serde_json::Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(value["current"]["generation"], 1);
    let mut corrupt = Command::new(BIN)
        .env("HOME", &home)
        .args(["backup", "receive-journal", "--target-id", "synthetic"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    {
        use std::io::Write;
        corrupt.stdin.take().unwrap().write_all(b"invalid").unwrap();
    }
    assert!(!corrupt.wait().unwrap().success());
    drop(store);
    let tools = dir(temp.path(), "tools");
    let shim = tools.join("ssh");
    let quote = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    fs::write(
        &shim,
        format!(
            "#!/bin/sh\nexec {} backup receive-journal --target-id synthetic\n",
            quote(BIN)
        ),
    )
    .unwrap();
    fs::set_permissions(&shim, fs::Permissions::from_mode(0o700)).unwrap();
    let old_path = std::env::var_os("PATH").unwrap_or_default();
    let path = std::env::join_paths(
        std::iter::once(tools.clone()).chain(std::env::split_paths(&old_path)),
    )
    .unwrap();
    for (name, accept) in [("worker-ok", true), ("worker-outage", false)] {
        if !accept {
            fs::write(&shim, "#!/bin/sh\nexit 71\n").unwrap();
        }
        let log = temp.path().join(format!("{name}.log"));
        let log_file = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&log)
            .unwrap();
        fs::set_permissions(&log, fs::Permissions::from_mode(0o600)).unwrap();
        let child = Command::new(BIN)
            .env("HOME", &home)
            .env("PATH", &path)
            .args([
                "--data-dir",
                source.to_str().unwrap(),
                "serve",
                "--dev",
                "--origin",
                "http://127.0.0.1:3210",
                "--bind",
                "127.0.0.1:0",
                "--recovery-export",
                export.to_str().unwrap(),
                "--recovery-ssh-target",
                "synthetic-host",
                "--recovery-target-id",
                "synthetic",
            ])
            .stdout(Stdio::null())
            .stderr(log_file)
            .spawn()
            .unwrap();
        let mut child = Guard(child);
        let start = std::time::Instant::now();
        let mut observed = false;
        while start.elapsed() < std::time::Duration::from_secs(15) {
            let text = fs::read_to_string(&log).unwrap_or_default();
            if text.contains(if accept {
                "accepted_at_checkpoint"
            } else {
                "\"state\":\"not_protected\""
            }) {
                observed = true;
                break;
            }
            if child.0.try_wait().unwrap().is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            observed,
            "worker did not report expected bounded state: {}",
            fs::read_to_string(&log).unwrap_or_default()
        );
        assert_eq!(
            live::target_status(&home.join(".moosuri-recovery/synthetic"))
                .unwrap()
                .current
                .unwrap()
                .generation,
            1
        );
        unsafe {
            libc::kill(child.0.id() as i32, libc::SIGTERM);
        }
        let started = std::time::Instant::now();
        while child.0.try_wait().unwrap().is_none()
            && started.elapsed() < std::time::Duration::from_secs(8)
        {
            std::thread::sleep(std::time::Duration::from_millis(25));
        }
        assert!(
            child.0.try_wait().unwrap().is_some(),
            "worker did not shut down"
        );
    }
    // These are real subprocess/pipe/file checks, not evidence of off-host SSH deployment.
    assert_eq!(
        live::target_status(&home.join(".moosuri-recovery/synthetic"))
            .unwrap()
            .current
            .unwrap()
            .generation,
        1
    );
}

struct Guard(std::process::Child);
impl Drop for Guard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
