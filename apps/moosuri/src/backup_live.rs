//! Optional periodic recovery publication. SSH provides transport authentication;
//! actual protection is only the receiver's independently retained accepted head.
use moosuri_store::{
    Store,
    backup::live::{self, Acceptance, Captured, ExportBinding},
};
use serde::{Deserialize, Serialize};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt};
use std::{
    io::Read,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub fn token(value: &str) -> Result<(), String> {
    if value.is_empty()
        || value.len() > 64
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return Err("invalid_recovery_alias_or_target_id".into());
    }
    Ok(())
}
pub fn target_root(id: &str, create: bool) -> Result<PathBuf, String> {
    token(id)?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or("recovery_home_missing")?);
    if !home.is_absolute() {
        return Err("recovery_home_invalid".into());
    }
    // Refuse linked ancestors before creating any managed target directory.
    let mut checked = PathBuf::new();
    for part in home.components() {
        checked.push(part);
        let meta = std::fs::symlink_metadata(&checked).map_err(|_| "recovery_home_invalid")?;
        if meta.file_type().is_symlink() || !meta.is_dir() {
            return Err("recovery_home_invalid".into());
        }
    }
    let parent = home.join(".moosuri-recovery");
    for p in [&parent, &parent.join(id)] {
        if create && std::fs::symlink_metadata(p).is_err() {
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(p)
                .map_err(|_| "recovery_target_directory_failed")?;
        }
        let m = std::fs::symlink_metadata(p).map_err(|_| "recovery_target_missing")?;
        if !m.is_dir()
            || m.file_type().is_symlink()
            || m.uid() != unsafe { libc::geteuid() }
            || m.permissions().mode() & 0o077 != 0
        {
            return Err("recovery_target_directory_unsafe".into());
        }
    }
    Ok(parent.join(id))
}
#[derive(Serialize, Deserialize)]
pub struct Health {
    pub state: String,
    pub last_accepted: Option<Acceptance>,
    pub publication_interval_seconds: u64,
    pub rpo_is_unbounded_while_lagging: bool,
}
pub struct Worker {
    stop: Arc<AtomicBool>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Worker {
    pub fn start(store: Store, export: &Path, alias: String, id: String) -> Result<Self, String> {
        token(&alias)?;
        token(&id)?;
        let export = ExportBinding::open(export).map_err(|e| e.to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        eprintln!(
            "recovery_publication not_protected; publication interval30s; latestness comes from independently retained target ledger"
        );
        let task = tokio::spawn(async move {
            let mut last = None;
            while !stopping.load(Ordering::Relaxed) {
                let result = match store.capture_recovery_journals(export.clone()).await {
                    Ok(capture) => {
                        let alias = alias.clone();
                        let id = id.clone();
                        let cancel = stopping.clone();
                        tokio::task::spawn_blocking(move || transfer(capture, &alias, &id, cancel))
                            .await
                            .unwrap_or_else(|_| Err("recovery_worker_unavailable".into()))
                    }
                    Err(e) => Err(e.to_string()),
                };
                let state = if let Ok(a) = result {
                    last = Some(a);
                    "accepted_at_checkpoint"
                } else if last.is_some() {
                    "lagging"
                } else {
                    "not_protected"
                };
                // Fixed metadata only: no paths, SSH diagnostics or journal payloads.
                let h = Health {
                    state: state.into(),
                    last_accepted: last.clone(),
                    publication_interval_seconds: 30,
                    rpo_is_unbounded_while_lagging: true,
                };
                if let Ok(json) = serde_json::to_string(&h) {
                    eprintln!("recovery_publication {json}");
                }
                for _ in 0..300 {
                    if stopping.load(Ordering::Relaxed) {
                        break;
                    }
                    tokio::time::sleep(Duration::from_millis(100)).await;
                }
            }
        });
        Ok(Self {
            stop,
            task: Some(task),
        })
    }
    pub async fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn ssh_command(alias: &str, id: &str) -> Result<Command, String> {
    token(alias)?;
    token(id)?;
    let mut c = Command::new("ssh");
    c.args([
        "-T",
        "-o",
        "BatchMode=yes",
        "-o",
        "StrictHostKeyChecking=yes",
        "-o",
        "ConnectTimeout=10",
        "-o",
        "ServerAliveInterval=5",
        "-o",
        "ServerAliveCountMax=2",
        "--",
        alias,
        "moosuri",
        "backup",
        "receive-journal",
        "--target-id",
        id,
    ]);
    Ok(c)
}
fn transfer(
    capture: Captured,
    alias: &str,
    id: &str,
    cancel: Arc<AtomicBool>,
) -> Result<Acceptance, String> {
    let command = ssh_command(alias, id)?;
    transfer_command(capture, command, cancel)
}
fn transfer_command(
    capture: Captured,
    mut command: Command,
    cancel: Arc<AtomicBool>,
) -> Result<Acceptance, String> {
    let installation = capture.installation_id().to_owned();
    let fingerprint = capture
        .fingerprint()
        .map_err(|_| "recovery_capture_invalid")?;
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "recovery_transport_unavailable")?;
    let pid = child.id() as i32;
    let stdin = child.stdin.take().ok_or("recovery_transport_unavailable")?;
    let stdout = child
        .stdout
        .take()
        .ok_or("recovery_transport_unavailable")?;
    let writer = std::thread::spawn(move || capture.write_to(stdin));
    let reader = std::thread::spawn(move || {
        let mut b = vec![];
        stdout.take(32769).read_to_end(&mut b).map_err(|_| ())?;
        if b.len() > 32768 { Err(()) } else { Ok(b) }
    });
    let start = Instant::now();
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(60) {
            let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
                let _ = child.wait();
                break None;
            }
        }
    };
    // Kill inherited transport descendants too before joining pipe IO threads.
    let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
    let write = writer.join().map_err(|_| "recovery_transfer_failed")?;
    let read = reader.join().map_err(|_| "recovery_transfer_failed")?;
    if !status.is_some_and(|s| s.success()) || write.is_err() {
        return Err("recovery_transfer_failed".into());
    }
    let a: Acceptance = serde_json::from_slice(&read.map_err(|_| "recovery_ack_invalid")?)
        .map_err(|_| "recovery_ack_invalid")?;
    if a.format != 1
        || a.generation == 0
        || a.installation_id != installation
        || a.capture_sha256 != fingerprint
        || a.trusted_head_sha256.len() != 64
        || !a
            .trusted_head_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("recovery_ack_invalid".into());
    }
    Ok(a)
}
/// Receiver CLI input has a process deadline even when a peer stalls mid-frame.
/// Successful completion exits before the watchdog; no private key is required.
pub fn receive_command(root: &Path) -> Result<Acceptance, String> {
    let done = Arc::new(AtomicBool::new(false));
    let finished = done.clone();
    std::thread::spawn(move || {
        for _ in 0..650 {
            if finished.load(Ordering::Relaxed) {
                return;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        if !finished.load(Ordering::Relaxed) {
            std::process::exit(74)
        }
    });
    let result = live::receive(root, std::io::stdin().lock()).map_err(|e| e.to_string());
    done.store(true, Ordering::Relaxed);
    result
}
#[cfg(test)]
mod tests;
