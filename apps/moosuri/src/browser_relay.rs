//! Installation-side member Chromium broker. Each authenticated namespace gets its
//! own socket and workspace map. CapsuleReader owns the installation-wide process permits.
use crate::runtime_google::{
    Binding, SocketGuard, epoch, listener_named, private_bytes, private_dir,
};
use clap::Args;
use fs2::FileExt;
use moosuri_connectors::{
    browser::{
        BrowserError,
        capsule::{CapsuleConfig, CapsuleReader, CapsuleWorkspace, TabCommand},
    },
    model::CancellationToken,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    fs::{self, File},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
    sync::{Mutex, Semaphore},
    task::{JoinHandle, JoinSet},
    time::{interval, timeout},
};
use uuid::Uuid;

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    pub registry: PathBuf,
    #[arg(long)]
    pub socket_dir: PathBuf,
    #[arg(long)]
    pub lifecycle_config: PathBuf,
    /// Private JSON emitted for the verified deployment/browser capsule.
    #[arg(long)]
    pub capsule_config: PathBuf,
}

const REQUEST_BYTES: u64 = 16 * 1024;
const IO_DEADLINE: Duration = Duration::from_secs(5);
const OP_DEADLINE: Duration = Duration::from_secs(75);
const POLL: Duration = Duration::from_secs(1);

struct Request {
    workspace: Uuid,
    generation: i64,
    command: TabCommand,
}
impl Request {
    fn parse(bytes: &[u8]) -> Result<Self, &'static str> {
        let mut value: Value = serde_json::from_slice(bytes).map_err(|_| "invalid")?;
        let map = value.as_object_mut().ok_or("invalid")?;
        if map.remove("v") != Some(json!(1)) {
            return Err("invalid");
        }
        let generation = map
            .remove("generation")
            .and_then(|v| v.as_i64())
            .filter(|g| *g > 0)
            .ok_or("invalid")?;
        let workspace: Uuid = serde_json::from_value(map.remove("workspace_id").ok_or("invalid")?)
            .map_err(|_| "invalid")?;
        if workspace.is_nil() {
            return Err("invalid");
        }
        let command = serde_json::from_value(value).map_err(|_| "invalid")?;
        Ok(Self {
            workspace,
            generation,
            command,
        })
    }
}

struct Live {
    epoch: i64,
    worker: CapsuleWorkspace,
}
type Sessions = HashMap<Uuid, Live>;
#[derive(Clone)]
struct Host {
    registry: Arc<PathBuf>,
    reader: CapsuleReader,
    clients: Arc<Semaphore>,
    checks: Arc<Semaphore>,
}

impl Host {
    async fn current(&self, b: Binding) -> Option<i64> {
        let path = self.registry.clone();
        let guard = self.checks.clone().try_acquire_owned().ok()?;
        // A blocking SQLite read can outlive the request that initiated it.
        timeout(
            IO_DEADLINE,
            tokio::task::spawn_blocking(move || {
                let _guard = guard;
                epoch(&path, b).ok()
            }),
        )
        .await
        .ok()?
        .ok()?
    }
}

fn code(error: BrowserError) -> &'static str {
    match error {
        BrowserError::StalePage | BrowserError::InvalidHandle => "stale",
        BrowserError::Capacity => "capacity",
        BrowserError::ForbiddenDestination | BrowserError::InvalidUrl => "forbidden",
        BrowserError::Expired => "missing",
        _ => "unavailable",
    }
}

async fn dispatch(
    host: &Host,
    b: Binding,
    initial: i64,
    request: Request,
    sessions: &mut Sessions,
    stop: &CancellationToken,
) -> Result<Value, &'static str> {
    let id = request.workspace;
    sessions.retain(|_, s| s.epoch == initial && !s.worker.is_closed());
    if matches!(request.command, TabCommand::Stop {}) {
        if let Some(live) = sessions.remove(&id) {
            live.worker.shutdown().await.map_err(code)?;
        }
        return Ok(json!({"stopped":true}));
    }
    if matches!(request.command, TabCommand::Open { .. }) {
        if sessions.contains_key(&id) {
            return Err("stale");
        }
        // A fresh ephemeral profile per workspace. Chromium never mounts the member volume.
        let mut worker = host
            .reader
            .start_member_workspace(b.account_id, stop)
            .await
            .map_err(code)?;
        if host.current(b).await != Some(initial) {
            return Err("forbidden");
        }
        let result = worker.tab_command(request.command).await.map_err(code)?;
        sessions.insert(
            id,
            Live {
                epoch: initial,
                worker,
            },
        );
        return Ok(result);
    }
    let live = sessions.get_mut(&id).ok_or("missing")?;
    let result = live
        .worker
        .tab_command(request.command)
        .await
        .map_err(code)?;
    if result
        .get("tabs")
        .is_some_and(|v| v.as_array().is_some_and(Vec::is_empty))
        && let Some(live) = sessions.remove(&id)
    {
        live.worker.shutdown().await.map_err(code)?;
    }
    Ok(result)
}

async fn serve_client(
    stream: UnixStream,
    host: Host,
    b: Binding,
    sessions: Arc<Mutex<Sessions>>,
    stop: CancellationToken,
) {
    if stream.peer_cred().ok().map(|cred| cred.uid()) != Some(b.peer_uid) {
        return;
    }
    let (read, mut write) = stream.into_split();
    let mut reader = BufReader::new(read);
    let mut bytes = Vec::new();
    if !matches!(
        timeout(
            IO_DEADLINE,
            (&mut reader)
                .take(REQUEST_BYTES + 1)
                .read_until(b'\n', &mut bytes)
        )
        .await,
        Ok(Ok(1..))
    ) || bytes.len() as u64 > REQUEST_BYTES
        || !bytes.ends_with(b"\n")
        || !reader.buffer().is_empty()
    {
        return;
    }
    let result = async {
        let request = Request::parse(&bytes)?;
        if request.generation != b.generation {
            return Err("forbidden");
        }
        // Fail fast instead of queueing old frame captures, clicks, or controller requests.
        let mut sessions = sessions.try_lock().map_err(|_| "capacity")?;
        let initial = host.current(b).await.ok_or("forbidden")?;
        if stop.is_cancelled() {
            return Err("forbidden");
        }
        let id = request.workspace;
        let mut extra = [0; 1];
        let fenced = async {
            loop {
                tokio::time::sleep(POLL).await;
                if host.current(b).await != Some(initial) {
                    return;
                }
            }
        };
        let operation = dispatch(&host, b, initial, request, &mut sessions, &stop);
        let result = tokio::select! {
            biased;
            _ = stop.cancelled() => None,
            _ = reader.read(&mut extra) => None,
            _ = fenced => None,
            result = timeout(OP_DEADLINE, operation) => result.ok(),
        };
        let Some(result) = result else {
            sessions.remove(&id);
            return Err("unavailable");
        };
        if stop.is_cancelled() || host.current(b).await != Some(initial) {
            sessions.remove(&id);
            return Err("forbidden");
        }
        result
    }
    .await;
    let reply = match result {
        Ok(value) => json!({"ok":true,"result":value}),
        Err(code) => json!({"ok":false,"code":code}),
    };
    if let Ok(mut line) = serde_json::to_vec(&reply) {
        line.push(b'\n');
        let _ = timeout(IO_DEADLINE, write.write_all(&line)).await;
    }
}

async fn serve_member(
    listener: UnixListener,
    _guard: SocketGuard,
    b: Binding,
    host: Host,
    stop: CancellationToken,
) {
    let sessions = Arc::new(Mutex::new(Sessions::new()));
    let mut clients = JoinSet::new();
    let mut audit = interval(POLL);
    loop {
        tokio::select! {
            biased;
            _ = stop.cancelled() => break,
            Some(_) = clients.join_next(), if !clients.is_empty() => {},
            _ = audit.tick() => {
                if sessions.try_lock().is_ok_and(|map| !map.is_empty()) {
                    let current = host.current(b).await;
                    if let Ok(mut map) = sessions.try_lock() {
                        map.retain(|_, s| Some(s.epoch) == current && !s.worker.is_closed());
                    }
                }
            }
            next = listener.accept(), if clients.len() < 6 => {
                let Ok((stream, _)) = next else { break };
                let Ok(slot) = host.clients.clone().try_acquire_owned() else { continue };
                let task = serve_client(stream, host.clone(), b, sessions.clone(), stop.clone());
                clients.spawn(async move {
                    let _slot = slot;
                    task.await;
                });
            }
        }
    }
    stop.cancel();
    while clients.join_next().await.is_some() {}
    for (_, live) in sessions.lock().await.drain() {
        let _ = live.worker.shutdown().await;
    }
}

fn trusted_ancestors(path: &Path) -> Result<(), String> {
    if !path.is_absolute() || fs::canonicalize(path).ok().as_deref() != Some(path) {
        return Err("browser_relay_path_invalid".into());
    }
    for parent in path.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| "browser_relay_path_invalid")?;
        if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("browser_relay_path_untrusted".into());
        }
    }
    Ok(())
}

struct Member {
    binding: Binding,
    stop: CancellationToken,
    task: JoinHandle<()>,
}
fn same(a: Binding, b: Binding) -> bool {
    a.account_id == b.account_id
        && a.tenant_id == b.tenant_id
        && a.volume_id == b.volume_id
        && a.generation == b.generation
        && a.peer_uid == b.peer_uid
}
async fn reconcile(
    host: &Host,
    root: &Path,
    members: &mut HashMap<Uuid, Member>,
    desired: Vec<Binding>,
) -> Result<(), String> {
    let old: Vec<_> = members
        .iter()
        .filter(|(_, m)| m.task.is_finished() || !desired.iter().any(|b| same(*b, m.binding)))
        .map(|(id, _)| *id)
        .collect();
    for id in &old {
        members[id].stop.cancel();
    }
    for id in old {
        if let Some(m) = members.remove(&id) {
            let _ = m.task.await;
        }
    }
    for b in desired {
        if members.contains_key(&b.account_id) {
            continue;
        }
        let (listener, guard) = timeout(IO_DEADLINE, listener_named(root, b, "browser.sock"))
            .await
            .map_err(|_| "browser_relay_bind_timeout")??;
        let stop = CancellationToken::new();
        let task = tokio::spawn(serve_member(listener, guard, b, host.clone(), stop.clone()));
        members.insert(
            b.account_id,
            Member {
                binding: b,
                stop,
                task,
            },
        );
    }
    Ok(())
}

pub async fn run(options: &Options) -> Result<(), String> {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err("browser_relay_linux_root_required".into());
    }
    private_dir(&options.registry)?;
    private_dir(&options.socket_dir)?;
    for path in [
        &options.registry,
        &options.socket_dir,
        &options.lifecycle_config,
        &options.capsule_config,
    ] {
        trusted_ancestors(path)?;
    }
    if options.socket_dir.starts_with(&options.registry)
        || options.registry.starts_with(&options.socket_dir)
    {
        return Err("browser_relay_private_socket_root_required".into());
    }
    let lock = File::open(&options.socket_dir).map_err(|_| "browser_relay_socket_unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "browser_relay_already_running")?;
    let bytes = private_bytes(&options.capsule_config, 16384)?;
    let config = CapsuleConfig::from_json(
        std::str::from_utf8(&bytes).map_err(|_| "browser_capsule_config_invalid")?,
    )
    .map_err(|_| "browser_capsule_config_invalid")?;
    trusted_ancestors(&config.seccomp_file)?;
    let host = Host {
        registry: Arc::new(options.registry.clone()),
        reader: CapsuleReader::new(config).map_err(|_| "browser_capsule_config_invalid")?,
        clients: Arc::new(Semaphore::new(32)),
        checks: Arc::new(Semaphore::new(64)),
    };
    let mut members = HashMap::new();
    let mut poll = interval(POLL);
    let signal = crate::shutdown_signal();
    tokio::pin!(signal);
    let result = loop {
        tokio::select! {
            _ = &mut signal => break Ok(()),
            _ = poll.tick() => {
                let registry = options.registry.clone();
                let config = options.lifecycle_config.clone();
                let root = options.socket_dir.clone();
                let desired = tokio::task::spawn_blocking(move ||
                    crate::kiri_lifecycle::browser_bindings(&registry, &config, &root)
                ).await;
                let Ok(Ok(bindings)) = desired else {
                    break Err("browser_relay_enrollment_invalid".into());
                };
                if let Err(error) = reconcile(&host, &options.socket_dir, &mut members, bindings).await {
                    break Err(error);
                }
            }
        }
    };
    for m in members.values() {
        m.stop.cancel();
    }
    for (_, m) in members {
        let _ = m.task.await;
    }
    result
}

#[cfg(test)]
mod tests;
