//! Host-only login-backed inference. The model broker authenticates the actual
//! Unix peer before sending even the owner pin. Prompts follow ledger admission.
pub(crate) mod chatgpt;
pub(crate) use chatgpt::{InspectOptions as ChatGptInspectOptions, inspect as inspect_chatgpt};
pub(crate) mod claude;
mod wire;
use clap::Args;
use fs2::FileExt;
use moosuri_connectors::model::{
    CancellationToken,
    protocol::{ModelEvent, TextRequest},
};
use moosuri_store::runtime_registry::engine::ModelChoice;
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::AsyncReadExt,
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use uuid::Uuid;
use wire::{Event, Hello, Prompt};

pub(crate) fn model_catalog(
    default_model: &str,
    default_effort: Option<&str>,
    additional: &[ModelChoice],
) -> Option<Vec<ModelChoice>> {
    if additional.len() >= 128 {
        return None;
    }
    let mut choices = vec![ModelChoice {
        model: default_model.to_owned(),
        effort: default_effort.map(str::to_owned),
    }];
    choices.extend_from_slice(additional);
    let mut seen = std::collections::BTreeSet::new();
    if choices.iter().any(|choice| {
        choice.model.is_empty()
            || choice.model.len() > 200
            || !choice.model.bytes().all(|b| b.is_ascii_graphic())
            || choice.effort.as_ref().is_some_and(|effort| {
                effort.is_empty()
                    || effort.len() > 32
                    || !effort
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
            })
            || !seen.insert((&choice.model, &choice.effort))
    }) {
        return None;
    }
    Some(choices)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Route {
    ChatgptPlan,
    ClaudeCode,
}
#[derive(Clone, Copy, Debug, Eq, PartialEq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Owner {
    pub account_id: Uuid,
    pub tenant_id: Uuid,
    pub volume_id: Uuid,
    pub generation: i64,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Endpoint {
    pub socket: PathBuf,
    pub worker_uid: u32,
}
impl Endpoint {
    pub(crate) fn validate(&self) -> Result<(), String> {
        if !self.socket.is_absolute()
            || self.socket.as_os_str().len() > 100
            || self.socket.file_name() != Some("engine.sock".as_ref())
            || self.socket.components().any(|c| {
                matches!(
                    c,
                    std::path::Component::ParentDir | std::path::Component::CurDir
                )
            })
            || self.worker_uid == 0
        {
            return Err("engine_worker_endpoint_invalid".into());
        }
        Ok(())
    }
}

pub(crate) struct Connection {
    stream: UnixStream,
    pin: Option<Hello>,
}
impl Connection {
    pub(crate) async fn connect(endpoint: &Endpoint) -> Result<Self, String> {
        endpoint.validate()?;
        let meta =
            fs::symlink_metadata(&endpoint.socket).map_err(|_| "engine_worker_unavailable")?;
        if !meta.file_type().is_socket()
            || meta.uid() != endpoint.worker_uid
            || meta.mode() & 0o077 != 0
            || meta.nlink() != 1
            || !trusted_parents(&endpoint.socket, endpoint.worker_uid)
            || fs::canonicalize(&endpoint.socket).ok().as_ref() != Some(&endpoint.socket)
        {
            return Err("engine_worker_socket_invalid".into());
        }
        let stream = tokio::time::timeout(
            Duration::from_secs(2),
            UnixStream::connect(&endpoint.socket),
        )
        .await
        .map_err(|_| "engine_worker_timeout")?
        .map_err(|_| "engine_worker_unavailable")?;
        if stream
            .peer_cred()
            .map_err(|_| "engine_worker_peer_invalid")?
            .uid()
            != endpoint.worker_uid
        {
            return Err("engine_worker_peer_invalid".into());
        }
        Ok(Self { stream, pin: None })
    }
    pub(crate) async fn authorize(
        mut self,
        route: Route,
        owner: Owner,
        revision: Uuid,
        model: &str,
        effort: Option<&str>,
    ) -> Result<Self, String> {
        let pin = Hello {
            version: 1,
            route,
            owner,
            revision,
            model: model.to_owned(),
            effort: effort.map(str::to_owned),
        };
        tokio::time::timeout(Duration::from_secs(3), async {
            wire::send(&mut self.stream, &pin, wire::HEADER_LIMIT).await?;
            if !wire::receive::<bool>(&mut self.stream, wire::HEADER_LIMIT).await? {
                return Err("engine_worker_pin_rejected".to_owned());
            }
            Ok(())
        })
        .await
        .map_err(|_| "engine_worker_timeout")??;
        self.pin = Some(pin);
        Ok(self)
    }
    pub(crate) async fn stream(
        mut self,
        route: Route,
        owner: Owner,
        revision: Uuid,
        request: &TextRequest,
        tx: &mpsc::Sender<ModelEvent>,
        cancel: &CancellationToken,
    ) -> Result<(), String> {
        if !self.pin.as_ref().is_some_and(|h| {
            h.route == route
                && h.owner == owner
                && h.revision == revision
                && h.model == request.model
                && h.effort == request.effort
        }) {
            return Err("engine_worker_pin_rejected".into());
        }
        if cancel.is_cancelled() || tx.is_closed() {
            return Err("engine_worker_cancelled".into());
        }
        let call = async {
            wire::send(
                &mut self.stream,
                &Prompt::from_request(request)?,
                wire::PROMPT_LIMIT,
            )
            .await?;
            let mut bytes = 0usize;
            let mut frames = 0;
            loop {
                let frame: Event = wire::receive(&mut self.stream, wire::EVENT_LIMIT).await?;
                frames += 1;
                bytes += frame.text_len();
                if frames > 16384 || bytes > 65536 {
                    return Err("engine_worker_output_limit".into());
                }
                let complete = frame.finished();
                let event = frame.into_model()?;
                if complete {
                    // A terminal record is authoritative only after clean transport EOF.
                    let mut trailing = [0];
                    if self
                        .stream
                        .read(&mut trailing)
                        .await
                        .map_err(|_| "engine_worker_read")?
                        != 0
                    {
                        return Err("engine_worker_trailing_data".into());
                    }
                }
                tx.send(event)
                    .await
                    .map_err(|_| "engine_worker_cancelled")?;
                if complete {
                    return Ok(());
                }
            }
        };
        tokio::select! {
            biased;
            _ = cancel.cancelled() => Err("engine_worker_cancelled".into()),
            _ = tx.closed() => Err("engine_worker_cancelled".into()),
            result = tokio::time::timeout(Duration::from_secs(60), call) => {
                result.map_err(|_| "engine_worker_timeout".to_owned())?
            }
        }
    }
}

#[derive(Args)]
pub(crate) struct Options {
    /// Private host-worker configuration (mode 0600).
    #[arg(long)]
    pub config: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    socket_directory: PathBuf,
    owner: Owner,
    connection_revision: Uuid,
    model: String,
    #[serde(default)]
    effort: Option<String>,
    #[serde(default)]
    models: Vec<ModelChoice>,
    adapter: Settings,
}
#[derive(Deserialize)]
#[serde(
    rename_all = "snake_case",
    tag = "type",
    content = "settings",
    deny_unknown_fields
)]
enum Settings {
    ChatgptPlan(chatgpt::Config),
    ClaudeCode(claude::Config),
}
enum Adapter {
    Chatgpt(Box<chatgpt::Adapter>),
    Claude(claude::Adapter),
    #[cfg(test)]
    Fixture,
}
impl Adapter {
    async fn stream(
        &self,
        req: &TextRequest,
        tx: &mpsc::Sender<ModelEvent>,
        stop: &CancellationToken,
    ) -> Result<(), String> {
        match self {
            Self::Chatgpt(a) => a.stream(req, tx, stop).await,
            Self::Claude(a) => a.stream(req, tx, stop).await,
            #[cfg(test)]
            Self::Fixture => tests::fixture(req, tx, stop).await,
        }
    }
}
struct Worker {
    pin: Hello,
    models: Vec<ModelChoice>,
    adapter: Adapter,
}
async fn handle(
    mut stream: UnixStream,
    broker_uid: u32,
    worker: Arc<Worker>,
    stop: CancellationToken,
) -> Result<(), String> {
    if stream
        .peer_cred()
        .map_err(|_| "engine_broker_peer_invalid")?
        .uid()
        != broker_uid
    {
        return Err("engine_broker_peer_invalid".into());
    }
    let hello = tokio::time::timeout(
        Duration::from_secs(3),
        wire::receive::<Hello>(&mut stream, wire::HEADER_LIMIT),
    )
    .await
    .map_err(|_| "engine_broker_timeout")??;
    let admitted = hello.version == worker.pin.version
        && hello.route == worker.pin.route
        && hello.owner == worker.pin.owner
        && hello.revision == worker.pin.revision
        && worker
            .models
            .iter()
            .any(|choice| choice.model == hello.model && choice.effort == hello.effort);
    wire::send(&mut stream, &admitted, wire::HEADER_LIMIT).await?;
    if !admitted {
        return Ok(());
    }
    let prompt = tokio::time::timeout(
        Duration::from_secs(10),
        wire::receive::<Prompt>(&mut stream, wire::PROMPT_LIMIT),
    )
    .await
    .map_err(|_| "engine_broker_timeout")??;
    let req = prompt.into_request(hello.model, hello.effort)?;
    let (mut inbound, mut outbound) = stream.into_split();
    let cancel = stop.child_token();
    let (tx, mut rx) = mpsc::channel(16);
    let provider = async {
        let result = worker.adapter.stream(&req, &tx, &cancel).await;
        drop(tx);
        result
    };
    let writer = &mut outbound;
    let output = async move {
        let mut end = None;
        let mut total = 0;
        let mut frames = 0;
        while let Some(event) = rx.recv().await {
            if end.is_some() {
                return Err("engine_worker_invalid_terminal".to_owned());
            }
            let Some(frame) = Event::from_model(event) else {
                continue;
            };
            total += frame.text_len();
            frames += 1;
            if frames > 16384 || total > 65536 {
                return Err("engine_worker_output_limit".into());
            }
            if frame.finished() {
                end = Some(frame);
            } else {
                wire::send(writer, &frame, wire::EVENT_LIMIT).await?;
            }
        }
        end.ok_or_else(|| "engine_worker_incomplete".into())
    };
    let stream_output = async {
        let mut extra = [0];
        let result = tokio::select! {
            _=inbound.read(&mut extra)=>Err("engine_broker_disconnected".into()),
            _=stop.cancelled()=>Err("engine_worker_cancelled".into()),
            r=tokio::time::timeout(Duration::from_secs(55),output)=>r.map_err(|_|"engine_worker_timeout".to_owned())?
        };
        result
    };
    // Let the official runner finish cancellation/child cleanup. Never publish a
    // terminal event until the provider has accepted its whole stream.
    let (provider_result, terminal) = tokio::join!(provider, async {
        let result = stream_output.await;
        if result.is_err() {
            cancel.cancel();
        }
        result
    });
    provider_result?;
    tokio::time::timeout(
        Duration::from_secs(2),
        wire::send(&mut outbound, &terminal?, wire::EVENT_LIMIT),
    )
    .await
    .map_err(|_| "engine_worker_timeout")?
}
// Service state must live below directories controlled by this service or root.
// A sticky root-owned temporary directory is also safe for a private child.
fn trusted_parents(path: &Path, uid: u32) -> bool {
    path.ancestors().skip(1).all(|parent| {
        fs::symlink_metadata(parent).is_ok_and(|m| {
            m.is_dir()
                && (m.uid() == 0 || m.uid() == uid)
                && (m.mode() & 0o022 == 0 || m.mode() & 0o1000 != 0)
        })
    })
}

fn read_config(path: &Path) -> Result<Config, String> {
    if !path.is_absolute()
        || !trusted_parents(path, unsafe { libc::geteuid() })
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err("engine_config_invalid".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "engine_config_unavailable")?;
    let meta = file.metadata().map_err(|_| "engine_config_unavailable")?;
    if !meta.is_file()
        || meta.nlink() != 1
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o177 != 0
        || meta.len() > 16384
    {
        return Err("engine_config_invalid".into());
    }
    let mut bytes = Vec::new();
    file.take(16385)
        .read_to_end(&mut bytes)
        .map_err(|_| "engine_config_unavailable")?;
    if bytes.len() > 16384 {
        return Err("engine_config_invalid".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "engine_config_invalid".into())
}
struct Bound {
    path: PathBuf,
    dev: u64,
    ino: u64,
    _lock: File,
}
impl Drop for Bound {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
async fn bind(dir: &Path, uid: u32) -> Result<(UnixListener, Bound), String> {
    let meta = fs::symlink_metadata(dir).map_err(|_| "engine_socket_directory_invalid")?;
    if !dir.is_absolute()
        || !trusted_parents(dir, uid)
        || fs::canonicalize(dir).ok().as_deref() != Some(dir)
        || !meta.is_dir()
        || meta.uid() != uid
        || meta.mode() & 0o077 != 0
    {
        return Err("engine_socket_directory_invalid".into());
    }
    let lock = File::open(dir).map_err(|_| "engine_socket_directory_invalid")?;
    lock.try_lock_exclusive()
        .map_err(|_| "engine_worker_already_running")?;
    let path = dir.join("engine.sock");
    Endpoint {
        socket: path.clone(),
        worker_uid: uid,
    }
    .validate()?;
    match fs::symlink_metadata(&path) {
        Ok(m) if m.file_type().is_socket() && m.uid() == uid && m.nlink() == 1 => {
            match UnixStream::connect(&path).await {
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    fs::remove_file(&path).map_err(|_| "engine_worker_socket_invalid")?
                }
                _ => return Err("engine_worker_socket_busy".into()),
            }
        }
        Ok(_) => return Err("engine_worker_socket_invalid".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err("engine_worker_socket_invalid".into()),
    }
    let listener = UnixListener::bind(&path).map_err(|_| "engine_worker_socket_bind")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "engine_worker_socket_invalid")?;
    let m = fs::symlink_metadata(&path).map_err(|_| "engine_worker_socket_invalid")?;
    Ok((
        listener,
        Bound {
            path,
            dev: m.dev(),
            ino: m.ino(),
            _lock: lock,
        },
    ))
}
pub(crate) async fn run(o: &Options) -> Result<(), String> {
    let uid = unsafe { libc::geteuid() };
    if !cfg!(target_os = "linux") || uid == 0 {
        return Err("engine_worker_requires_unprivileged_linux_user".into());
    }
    let config = read_config(&o.config)?;
    if config.version != 1
        || config.owner.generation <= 0
        || [
            config.owner.account_id,
            config.owner.tenant_id,
            config.owner.volume_id,
            config.connection_revision,
        ]
        .iter()
        .any(Uuid::is_nil)
    {
        return Err("engine_config_invalid".into());
    }
    let models = model_catalog(&config.model, config.effort.as_deref(), &config.models)
        .ok_or("engine_config_invalid")?;
    let (listener, _socket) = bind(&config.socket_directory, uid).await?;
    let (route, adapter) = match config.adapter {
        Settings::ChatgptPlan(c) => (
            Route::ChatgptPlan,
            Adapter::Chatgpt(Box::new(chatgpt::Adapter::open(c, &models).await?)),
        ),
        Settings::ClaudeCode(c) => (
            Route::ClaudeCode,
            Adapter::Claude(claude::Adapter::open(c, &models).await?),
        ),
    };
    let worker = Arc::new(Worker {
        pin: Hello {
            version: 1,
            route,
            owner: config.owner,
            revision: config.connection_revision,
            model: config.model,
            effort: config.effort,
        },
        models,
        adapter,
    });
    let stop = CancellationToken::new();
    let slots = Arc::new(Semaphore::new(4));
    let mut clients = JoinSet::new();
    let signal = crate::shutdown_signal();
    tokio::pin!(signal);
    let result = loop {
        tokio::select! {
            _=&mut signal=>break Ok(()),
            Some(_)=clients.join_next(), if !clients.is_empty()=>{},
            next=listener.accept()=>{
                let Ok((stream,_))=next else {break Err("engine_worker_listener_failed".into());};
                if let Ok(permit)=slots.clone().try_acquire_owned(){
                    let worker=worker.clone();let stop=stop.clone();
                    clients.spawn(async move {let _permit=permit;handle(stream,0,worker,stop).await});
                }
            }
        }
    };
    stop.cancel();
    while clients.join_next().await.is_some() {}
    result
}
#[cfg(test)]
mod tests;
