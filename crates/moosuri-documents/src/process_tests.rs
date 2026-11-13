//! Synthetic executable fixtures exercise the parent's actual process boundary.
use super::{PdfError, extract_pdf};
use std::{
    fs::{self, OpenOptions},
    io::Read,
    os::unix::{ffi::OsStrExt, fs::OpenOptionsExt, fs::PermissionsExt},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};

fn executable(root: &Path, body: &str) -> PathBuf {
    let path = root.join("synthetic-extractor");
    fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700)).unwrap();
    path
}

#[tokio::test]
async fn child_inherits_actual_cpu_file_descriptor_and_linux_memory_limits() {
    let root = tempfile::tempdir().unwrap();
    let path = executable(
        root.path(),
        r#"printf '{"Ok":{"pages":1,"text":"cpu=%s;file=%s;core=%s;fds=%s;as=%s"}}' "$(ulimit -t)" "$(ulimit -f)" "$(ulimit -c)" "$(ulimit -n)" "$(ulimit -v)""#,
    );
    let result = extract_pdf(&path, b"synthetic input").await.unwrap();
    assert!(result.text.starts_with("cpu=5;file=0;core=0;fds=32;as="));
    #[cfg(target_os = "linux")]
    assert!(result.text.ends_with("as=524288"));
}

#[allow(unsafe_code)]
fn fifo(path: &Path) {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: a valid, owned, absent path in the test's private temporary root.
    assert_eq!(unsafe { libc::mkfifo(path.as_ptr(), 0o600) }, 0);
}

#[allow(unsafe_code)]
fn process_exists(pid: i32) -> bool {
    assert!(pid > 1);
    // SAFETY: signal zero performs only an existence check; it sends no signal.
    unsafe { libc::kill(pid, 0) == 0 }
}

async fn stalled_child(
    root: &Path,
) -> (
    i32,
    tokio::task::JoinHandle<Result<super::PdfText, PdfError>>,
) {
    let output = root.join("pid.fifo");
    let hold = root.join("hold.fifo");
    fifo(&output);
    fifo(&hold);
    // A FIFO stays readable without consuming the child's zero-size file budget.
    // Nonblocking reads avoid leaking a blocked OS thread if child startup fails.
    let mut reader = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&output)
        .unwrap();
    let quote = |path: &Path| format!("'{}'", path.to_str().unwrap().replace('\'', "'\\''"));
    let script = format!(
        "printf '%s' \"$$\" > {}\nexec 3<> {}\nread -r waiting <&3",
        quote(&output),
        quote(&hold),
    );
    let path = executable(root, &script);
    let task = tokio::spawn(async move { extract_pdf(&path, b"synthetic input").await });
    let start = Instant::now();
    let mut bytes = Vec::new();
    loop {
        match reader.read_to_end(&mut bytes) {
            Ok(_) if !bytes.is_empty() => break,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("synthetic pid pipe: {e}"),
        }
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "child did not announce its PID"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    let pid = std::str::from_utf8(&bytes).unwrap().parse::<i32>().unwrap();
    assert!(process_exists(pid));
    (pid, task)
}

async fn assert_reaped(pid: i32) {
    let start = Instant::now();
    while process_exists(pid) && start.elapsed() < Duration::from_secs(3) {
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        !process_exists(pid),
        "owned extraction child was not reaped"
    );
}

#[tokio::test]
async fn cancelled_extraction_kills_and_reaps_its_child() {
    let root = tempfile::tempdir().unwrap();
    let (pid, task) = stalled_child(root.path()).await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    assert_reaped(pid).await;
}

#[tokio::test]
async fn wall_timeout_kills_and_reaps_a_child_without_cpu_activity() {
    let root = tempfile::tempdir().unwrap();
    let (pid, task) = stalled_child(root.path()).await;
    let result = tokio::time::timeout(Duration::from_secs(12), task)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.unwrap_err(), PdfError::TimedOut);
    assert_reaped(pid).await;
}
