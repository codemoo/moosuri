//! Fixed-argv OpenSSH transport with bounded pipes, deadline and process-group reap.
use super::*;
use std::{
    process::{Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::watch;
fn token(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 64
        || !value.as_bytes()[0].is_ascii_alphanumeric()
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
    {
        return Err(Error::Invalid);
    }
    Ok(())
}
/// Target commands use a managed private root, never a source-provided remote path.
pub fn target_root(id: &str, create: bool) -> Result<PathBuf> {
    token(id)?;
    let home = PathBuf::from(std::env::var_os("HOME").ok_or(Error::Invalid)?);
    check_components(&home)?;
    let parent = home.join(".moosuri-vault-recovery");
    let root = parent.join(id);
    for path in [&parent, &root] {
        if create && !path.try_exists()? {
            fs::DirBuilder::new().mode(0o700).create(path)?;
            sync_dir(path.parent().ok_or(Error::Invalid)?)?;
        }
        private_dir(path)?;
    }
    Ok(root)
}
#[derive(Clone)]
pub struct WorkerConfig {
    pub directory: PathBuf,
    pub recipient: String,
    pub snapshot_head: PathBuf,
    pub ssh_target: String,
    pub target_id: String,
    pub interval_seconds: u64,
}
pub struct Worker {
    stop: Arc<AtomicBool>,
    wake: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Worker {
    pub fn start(db: Arc<Mutex<Database>>, c: WorkerConfig) -> Result<Self> {
        token(&c.ssh_target)?;
        token(&c.target_id)?;
        if !(30..=604800).contains(&c.interval_seconds) {
            return Err(Error::Invalid);
        }
        let _: age::x25519::Recipient = c.recipient.parse().map_err(|_| Error::Invalid)?;
        private_dir(&c.directory)?;
        let baseline: SnapshotTrustedHead = read_json(&c.snapshot_head)?;
        snapshot_binding(&baseline)?;
        {
            let guard = db.lock().map_err(|_| Error::Unavailable)?;
            if installation(&guard)? != baseline.snapshot.source {
                return Err(Error::Conflict);
            }
            // Reuse capture path rules without starting an immediate capture.
            let root = Path::new(guard.conn.path().ok_or(Error::Unavailable)?)
                .parent()
                .ok_or(Error::Invalid)?;
            disjoint(root, &c.directory)?;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let stopping = stop.clone();
        let (wake, mut waiting) = watch::channel(false);
        eprintln!("vault_journal_publication not_protected periodic_gap");
        let task = tokio::spawn(async move {
            let mut accepted = false;
            loop {
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                let db = db.clone();
                let cfg = c.clone();
                let cancel = stopping.clone();
                let result = tokio::task::spawn_blocking(move || {
                    if cancel.load(Ordering::Relaxed) {
                        return Err(Error::Unavailable);
                    }
                    let prepared = prepare(&db, &cfg, &cancel)?;
                    transfer(prepared, &cfg.ssh_target, &cfg.target_id, cancel)
                })
                .await;
                match result {
                    Ok(Ok(a)) => {
                        accepted = true;
                        eprintln!(
                            "vault_journal_publication accepted_at_checkpoint generation={} sequence={} trusted_head_sha256={} periodic_gap",
                            a.generation, a.journal.journal.sequence, a.trusted_head_sha256
                        );
                    }
                    _ if accepted => eprintln!("vault_journal_publication lagging periodic_gap"),
                    _ => eprintln!("vault_journal_publication not_protected periodic_gap"),
                }
                if stopping.load(Ordering::Relaxed) {
                    break;
                }
                tokio::select! {_=waiting.changed()=>break,_=tokio::time::sleep(Duration::from_secs(c.interval_seconds))=>()}
            }
        });
        Ok(Self {
            stop,
            wake,
            task: Some(task),
        })
    }
    pub async fn shutdown(mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = self.wake.send(true);
    }
}
fn prepare(db: &Arc<Mutex<Database>>, c: &WorkerConfig, cancel: &AtomicBool) -> Result<Prepared> {
    // Same exclusive mutex as HTTP. Temporary guard ends here; semantic replay,
    // age and SSH run only on the frozen stage.
    let capture = db
        .lock()
        .map_err(|_| Error::Unavailable)?
        .capture_snapshot(&c.directory)?;
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Unavailable);
    }
    capture.prepare_publication(&c.recipient, &c.snapshot_head)
}
fn ssh_command(alias: &str, id: &str) -> Result<Command> {
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
        "-o",
        "ClearAllForwardings=yes",
        "--",
        alias,
        "moosuri-vault-service",
        "recovery",
        "receive-journal",
        "--target-id",
        id,
    ]);
    Ok(c)
}
fn transfer(
    capture: Prepared,
    alias: &str,
    id: &str,
    cancel: Arc<AtomicBool>,
) -> Result<Acceptance> {
    transfer_command(
        capture,
        ssh_command(alias, id)?,
        cancel,
        Duration::from_secs(60),
    )
}
#[allow(unsafe_code)]
fn kill_group(pid: i32) {
    // SAFETY: positive child PID is converted to its dedicated process group.
    let _ = unsafe { libc::kill(-pid, libc::SIGKILL) };
}
fn transfer_command(
    capture: Prepared,
    mut command: Command,
    cancel: Arc<AtomicBool>,
    timeout: Duration,
) -> Result<Acceptance> {
    if cancel.load(Ordering::Relaxed) {
        return Err(Error::Unavailable);
    }
    let expected = capture.request.clone();
    let fingerprint = expected.fingerprint()?;
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?;
    let pid = i32::try_from(child.id()).map_err(|_| Error::Unavailable)?;
    let stdin = child.stdin.take().ok_or(Error::Unavailable)?;
    let stdout = child.stdout.take().ok_or(Error::Unavailable)?;
    let writer = std::thread::spawn(move || capture.write_to(stdin));
    let reader = std::thread::spawn(move || {
        let mut b = vec![];
        stdout.take(JSON_LIMIT + 1).read_to_end(&mut b)?;
        if b.len() as u64 > JSON_LIMIT {
            Err(Error::Limit)
        } else {
            Ok(b)
        }
    });
    let start = Instant::now();
    // Pipe threads are included in the transport lifetime. Child exit by itself
    // is insufficient: inherited descendant pipes must be killed before joining.
    let status = loop {
        if cancel.load(Ordering::Relaxed) || start.elapsed() > timeout {
            kill_group(pid);
            let _ = child.wait();
            break None;
        }
        match child.try_wait() {
            Ok(Some(s)) => {
                kill_group(pid);
                break Some(s);
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(_) => {
                kill_group(pid);
                let _ = child.wait();
                break None;
            }
        }
    };
    let written = writer.join().map_err(|_| Error::Unavailable)?;
    let read = reader.join().map_err(|_| Error::Unavailable)??;
    if !status.is_some_and(|s| s.success()) || written.is_err() {
        return Err(Error::Unavailable);
    }
    let a: Acceptance = serde_json::from_slice(&read).map_err(|_| Error::Invalid)?;
    if a.format != FORMAT
        || a.version != 1
        || a.generation == 0
        || a.generation > GENERATIONS
        || a.source != expected.binding.source
        || a.capture_sha256 != fingerprint
        || request_from(&a).fingerprint()? != fingerprint
        || a.journal.journal.source != a.source
        || a.journal.ciphertext_size > historical::LIMIT
        || a.journal.pin()? != a.trusted_head_sha256
    {
        return Err(Error::Unavailable);
    }
    unhex(&a.previous_receipt_sha256)?;
    unhex(&a.journal.ciphertext_sha256)?;
    Ok(a)
}
/// Dedicated CLI watchdog bounds stalled stdin and checked receiver processing.
/// Process exit leaves only encrypted partial output; no raw journal is staged.
pub fn receive_command(root: &Path) -> Result<Acceptance> {
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
            std::process::exit(74);
        }
    });
    let result = receive(root, std::io::stdin().lock());
    done.store(true, Ordering::Relaxed);
    result
}
#[cfg(test)]
mod tests;
