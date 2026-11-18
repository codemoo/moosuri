//! Opt-in text-only shared model broker for preallocated synthetic/new runtimes.
//! One configured Unix socket per owner. No subscription CLI, tools, provider
//! sessions, prompt/answer persistence, fallback, retry, or private Store opening.
use clap::Args;
use fs2::FileExt;
use moosuri_connectors::model::{
    CancellationToken, ModelClient, Secret, TransportLimits,
    protocol::{
        FinishOutcome, ModelEvent, TextCapabilities, TextMessage, TextRequest, TextRole, Usage,
    },
};
use moosuri_core::ProviderConfig;
use moosuri_store::{
    runtime_model_ledger::{Admission, Budget, CallKey, Ledger, Receipt},
    runtime_registry,
};
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::{Semaphore, mpsc},
    task::JoinSet,
};
use uuid::Uuid;
use zeroize::Zeroizing;

const MAX_FRAME: usize = 64 * 1024;
const MAX_OUTPUT: usize = 64 * 1024;
const MAX_REPLY: usize = 512 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 2048;
#[derive(Args)]
pub struct Options {
    /// Existing private installation account registry directory.
    #[arg(long)]
    pub registry: PathBuf,
    /// Dedicated private shared-ledger directory, never a personal volume.
    #[arg(long)]
    pub ledger: PathBuf,
    /// Private host directory; mount only each exact socket into its runtime.
    #[arg(long)]
    pub socket_dir: PathBuf,
    /// Owner-only JSON with one pinned provider/model/revision and exact bindings.
    #[arg(long)]
    pub config: PathBuf,
    /// Existing owner-only 32-byte random HMAC key; retained across broker restarts.
    #[arg(long)]
    pub fingerprint_key: PathBuf,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    connection_revision: Uuid,
    provider: ProviderConfig,
    model: String,
    /// API/local endpoint credential only. No personal subscription auth HOME.
    key_file: Option<PathBuf>,
    bindings: Vec<Binding>,
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    account_id: Uuid,
    tenant_id: Uuid,
    volume_id: Uuid,
    generation: i64,
    peer_uid: u32,
    budget: Budget,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Message {
    role: Role,
    text: String,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Role {
    System,
    User,
    Assistant,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u32,
    call_id: Uuid,
    run_id: Uuid,
    ordinal: u32,
    generation: i64,
    connection_revision: Uuid,
    model: String,
    max_output_tokens: u32,
    messages: Vec<Message>,
}
#[derive(Serialize, Deserialize)]
struct Reply {
    v: u32,
    state: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    call_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    output: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    outcome: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    receipt: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    connection_revision: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
}
impl Reply {
    fn error(code: &str) -> Self {
        Self {
            v: 1,
            state: code.into(),
            call_id: None,
            output: None,
            outcome: None,
            receipt: None,
            connection_revision: None,
            model: None,
        }
    }
    fn receipt(id: Uuid, r: Receipt) -> Self {
        Self {
            v: 1,
            state: match r.state {
                moosuri_store::runtime_model_ledger::State::Settled => {
                    "settled_response_unavailable"
                }
                moosuri_store::runtime_model_ledger::State::Unknown => "outcome_unknown",
                _ => "in_progress",
            }
            .into(),
            call_id: Some(id),
            output: None,
            outcome: None,
            receipt: serde_json::to_value(r).ok(),
            connection_revision: None,
            model: None,
        }
    }
}
fn private_dir(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|_| "private_directory_unavailable")?;
    if !path.is_absolute()
        || !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err("private_directory_invalid".into());
    }
    Ok(())
}
fn private_bytes(path: &Path, cap: usize) -> Result<Zeroizing<Vec<u8>>, String> {
    if !path.is_absolute() {
        return Err("private_file_invalid".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "private_file_unavailable")?;
    let m = file.metadata().map_err(|_| "private_file_unavailable")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o077 != 0
        || m.len() > cap as u64
    {
        return Err("private_file_invalid".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "private_file_unavailable")?;
    if bytes.len() > cap {
        return Err("private_file_invalid".into());
    }
    Ok(bytes)
}
// Read the installation-owned key for each admission. Reuse the transport
// (including its pinned HTTP pool) while the credential is unchanged.
struct ClientCache {
    tag: Option<hmac::Tag>,
    client: Arc<ModelClient>,
}
impl ClientCache {
    fn load(config: &Config, key: &hmac::Key, previous: Option<&Self>) -> Result<Self, String> {
        let bytes = config
            .key_file
            .as_deref()
            .map(|path| {
                let parent = path.parent().ok_or("model_key_invalid")?;
                let meta = fs::metadata(parent).map_err(|_| "model_key_invalid")?;
                if fs::canonicalize(parent).ok().as_deref() != Some(parent)
                    || meta.uid() != unsafe { libc::geteuid() }
                    || meta.mode() & 0o022 != 0
                {
                    return Err("model_key_invalid".into());
                }
                private_bytes(path, 8192)
            })
            .transpose()?;
        let tag = bytes.as_ref().map(|b| hmac::sign(key, b));
        if let Some(previous) = previous {
            if tag.as_ref().map(|t| t.as_ref()) == previous.tag.as_ref().map(|t| t.as_ref()) {
                return Ok(Self {
                    tag,
                    client: previous.client.clone(),
                });
            }
        }
        let credential = bytes
            .as_ref()
            .map(|b| {
                let value = std::str::from_utf8(b).map_err(|_| "model_key_invalid")?;
                Secret::new(value.trim_end_matches(['\n', '\r']).to_owned())
                    .map_err(|_| "model_key_invalid")
            })
            .transpose()?;
        let client =
            moosuri_agent::model_client(&config.provider, credential, TextCapabilities::default())
                .map_err(|_| "model_config_invalid")?
                .with_limits(TransportLimits {
                    connect: Duration::from_secs(5),
                    idle: Duration::from_secs(20),
                    total: Duration::from_secs(60),
                    response_bytes: 512 * 1024,
                })
                .map_err(|_| "model_config_invalid")?;
        Ok(Self {
            tag,
            client: Arc::new(client),
        })
    }
}
struct Broker {
    registry: PathBuf,
    config: Config,
    key: hmac::Key,
    client: Mutex<Option<ClientCache>>,
    ledger: Mutex<Ledger>,
    slots: Arc<Semaphore>,
    stop: CancellationToken,
}
fn binding_matches(a: &runtime_registry::Allocation, b: &Binding) -> bool {
    a.account_id == b.account_id.to_string()
        && a.tenant_id == b.tenant_id.to_string()
        && a.volume_id == b.volume_id.to_string()
        && a.generation == b.generation
}
fn authority(registry: &Path, binding: &Binding) -> Result<(), String> {
    let a =
        runtime_registry::lookup_id(registry, binding.account_id).map_err(|_| "account_fenced")?;
    if !binding_matches(&a, binding) {
        return Err("account_fenced".into());
    }
    Ok(())
}
impl Broker {
    fn new(options: &Options, stop: CancellationToken) -> Result<Arc<Self>, String> {
        private_dir(&options.registry)?;
        let config: Config = serde_json::from_slice(&private_bytes(&options.config, MAX_FRAME)?)
            .map_err(|_| "model_config_invalid")?;
        if config.version != 1
            || config.bindings.is_empty()
            || config.bindings.len() > 128
            || config.model.is_empty()
            || config.model.len() > 200
        {
            return Err("model_config_invalid".into());
        }
        let mut accounts = BTreeSet::new();
        let mut peer_uids = BTreeSet::new();
        for b in &config.bindings {
            b.budget.validate().map_err(|_| "model_budget_invalid")?;
            if b.generation <= 0 || !accounts.insert(b.account_id) || !peer_uids.insert(b.peer_uid)
            {
                return Err("model_binding_invalid".into());
            }
            // An inert listener may be created for reserved/fenced allocations.
            // Ready is required only at actual dispatch/result admission, avoiding
            // the member-ready -> model-socket -> routing-ready startup cycle.
            let allocated = runtime_registry::lookup_allocated_id(&options.registry, b.account_id)
                .map_err(|_| "model_binding_invalid")?;
            if !binding_matches(&allocated, b) {
                return Err("model_binding_invalid".into());
            }
        }
        let key = private_bytes(&options.fingerprint_key, 32)?;
        if key.len() != 32 {
            return Err("fingerprint_key_invalid".into());
        }
        let key = hmac::Key::new(hmac::HMAC_SHA256, &key);
        let client = Mutex::new(Some(ClientCache::load(&config, &key, None)?));
        let ledger = Ledger::open(&options.ledger).map_err(|_| "model_ledger_unavailable")?;
        Ok(Arc::new(Self {
            registry: options.registry.clone(),
            config,
            key,
            client,
            ledger: Mutex::new(ledger),
            slots: Arc::new(Semaphore::new(8)),
            stop,
        }))
    }
    fn request(&self, b: &Binding, r: &Request) -> Result<TextRequest, String> {
        if r.connection_revision != self.config.connection_revision || r.model != self.config.model
        {
            return Err("model_selection_changed".into());
        }
        if r.v != 1
            || r.generation != b.generation
            || r.ordinal > 1_000_000
            || r.messages.is_empty()
            || r.messages.len() > 32
            || r.max_output_tokens == 0
            || r.max_output_tokens > MAX_OUTPUT_TOKENS
            || r.max_output_tokens as u64 > b.budget.reservation_tokens
        {
            return Err("request_invalid".into());
        }
        let mut total = 0;
        let mut messages = Vec::new();
        for m in &r.messages {
            total += m.text.len();
            if m.text.is_empty() || total > MAX_FRAME {
                return Err("request_invalid".into());
            }
            messages.push(TextMessage {
                role: match m.role {
                    Role::System => TextRole::System,
                    Role::User => TextRole::User,
                    Role::Assistant => TextRole::Assistant,
                },
                text: m.text.clone(),
                phase: None,
            });
        }
        // Reserve a conservative byte-token envelope plus protocol overhead.
        // Actual provider accounting is still authoritative; any measured
        // overrun is charged in full before a further call can be admitted.
        let envelope =
            total as u64 + r.max_output_tokens as u64 + 1024 + 64 * r.messages.len() as u64;
        if envelope > b.budget.reservation_tokens {
            return Err("request_exceeds_reservation".into());
        }
        let request = TextRequest {
            model: self.config.model.clone(),
            messages,
            max_output_tokens: r.max_output_tokens,
            effort: None,
        };
        Ok(request)
    }
    /// Local acceptance point: account/generation validation, quota admission
    /// and the one durable dispatch intent serialize against registry fencing.
    /// The registry lock is released on return, before any provider IO.
    fn admit(
        &self,
        b: &Binding,
        key: &CallKey,
        provider_ready: bool,
    ) -> moosuri_store::Result<Admission> {
        runtime_registry::with_ready_allocation(&self.registry, b.account_id, |a| {
            if self.stop.is_cancelled() || !binding_matches(&a, b) {
                return Err(moosuri_store::StoreError::Forbidden);
            }
            let mut ledger = self
                .ledger
                .lock()
                .map_err(|_| moosuri_store::StoreError::Unavailable)?;
            let admission = ledger.admit_if_ready(key, b.budget, provider_ready)?;
            if let Admission::Fresh(permit) = &admission {
                ledger.dispatch(permit)?;
            }
            Ok(admission)
        })
    }
    async fn call(&self, b: &Binding, r: Request) -> Reply {
        let request = match self.request(b, &r) {
            Ok(r) => r,
            Err(e) => return Reply::error(&e),
        };
        // Keyed canonical fingerprint covers the complete request, immutable
        // owner tuple and admin-pinned connection revision/model/network policy.
        let canonical = match serde_json::to_vec(&(
            "moosuri/shared-model/v1",
            b.account_id,
            b.tenant_id,
            b.volume_id,
            b.generation,
            self.config.connection_revision,
            &self.config.provider,
            &self.config.model,
            &r,
        )) {
            Ok(v) => Zeroizing::new(v),
            Err(_) => return Reply::error("request_invalid"),
        };
        let tag = hmac::sign(&self.key, &canonical);
        let key = CallKey {
            call_id: r.call_id,
            account_id: b.account_id,
            tenant_id: b.tenant_id,
            generation: b.generation,
            run_id: r.run_id,
            ordinal: r.ordinal,
            connection_revision: self.config.connection_revision,
            fingerprint: tag.as_ref().try_into().unwrap(),
        };
        let client = match self.client.lock() {
            Ok(mut cached) => {
                *cached = ClientCache::load(&self.config, &self.key, cached.as_ref()).ok();
                cached.as_ref().map(|c| c.client.clone())
            }
            Err(_) => None,
        };
        if client
            .as_ref()
            .is_some_and(|c| c.validate_text_request(&request).is_err())
        {
            return Reply::error("request_invalid");
        }
        let permit = match self.admit(b, &key, client.is_some()) {
            Ok(Admission::Existing(receipt)) => return Reply::receipt(r.call_id, receipt),
            Ok(Admission::Fresh(p)) => p,
            Err(moosuri_store::StoreError::NotFound | moosuri_store::StoreError::Forbidden) => {
                return Reply::error("account_fenced");
            }
            Err(moosuri_store::StoreError::BudgetExhausted) => {
                return Reply::error("quota_exhausted");
            }
            Err(moosuri_store::StoreError::Conflict) => return Reply::error("call_conflict"),
            Err(moosuri_store::StoreError::ModelRequired) => {
                return Reply::error("model_credential_unavailable");
            }
            Err(_) => return Reply::error("ledger_unavailable"),
        };
        // Already admitted calls may reach the provider after fencing. They are
        // in-flight work, never a fresh post-fence acceptance. Late output is
        // checked again below and usage remains charged to this same receipt.
        let client = client.expect("a fresh admission requires a provider");
        let (tx, mut rx) = mpsc::channel(32);
        let cancel = self.stop.child_token();
        let (result, (text, usage, outcome)) = tokio::join!(
            async {
                let result = client.stream_text(&request, &tx, &cancel).await;
                drop(tx);
                result
            },
            async {
                let mut text = String::new();
                let mut usage = Usage::default();
                let mut outcome = None;
                loop {
                    tokio::select! {
                        _=cancel.cancelled()=>break,
                        event=rx.recv()=>match event {
                            Some(ModelEvent::TextDelta{text:delta,..}|ModelEvent::RefusalDelta{text:delta,..})=>{
                                if text.len()+delta.len()>MAX_OUTPUT {cancel.cancel();break;}text.push_str(&delta);
                            }
                            Some(ModelEvent::UsageSnapshot(next))=>usage=next,
                            Some(ModelEvent::Finished{outcome:next,..})=>{outcome=Some(next);}
                            None=>break,
                            _=>(),
                        }
                    }
                }
                // EOF is required even after Finished: malformed trailing frames
                // cannot turn a partial provider stream into a settled answer.
                drop(rx);
                (text, usage, outcome)
            }
        );
        if result.is_err() || outcome.is_none() {
            let receipt = self.ledger.lock().ok().and_then(|mut l| {
                l.uncertain(&permit, usage.input_tokens, usage.output_tokens)
                    .ok()
            });
            return receipt
                .map(|rct| Reply::receipt(r.call_id, rct))
                .unwrap_or_else(|| Reply::error("ledger_unavailable"));
        }
        let receipt = match self.ledger.lock().ok().and_then(|mut l| {
            l.settle(&permit, usage.input_tokens, usage.output_tokens)
                .ok()
        }) {
            Some(r) => r,
            None => return Reply::error("ledger_unavailable"),
        };
        if authority(&self.registry, b).is_err() {
            return Reply::receipt(r.call_id, receipt);
        }
        Reply {
            v: 1,
            state: "settled".into(),
            call_id: Some(r.call_id),
            output: Some(text),
            outcome: Some(
                match outcome.unwrap() {
                    FinishOutcome::Completed => "completed",
                    FinishOutcome::Incomplete => "incomplete",
                    FinishOutcome::Refused => "refused",
                    FinishOutcome::ContinuationRequired => "continuation_required",
                }
                .into(),
            ),
            receipt: serde_json::to_value(receipt).ok(),
            connection_revision: Some(self.config.connection_revision),
            model: Some(self.config.model.clone()),
        }
    }
}
async fn frame(stream: &mut UnixStream, cap: usize, wait: Duration) -> Result<Vec<u8>, String> {
    tokio::time::timeout(wait, async {
        let mut frame = Vec::new();
        let mut chunk = [0; 1024];
        loop {
            let n = stream
                .read(&mut chunk)
                .await
                .map_err(|_| "request_unavailable")?;
            if n == 0 {
                return Err("request_truncated".into());
            }
            if frame.len() + n > cap {
                return Err("request_too_large".into());
            }
            if let Some(end) = chunk[..n].iter().position(|b| *b == b'\n') {
                if chunk[end + 1..n].iter().any(|b| !b.is_ascii_whitespace()) {
                    return Err("request_invalid".into());
                }
                frame.extend_from_slice(&chunk[..end]);
                return Ok(frame);
            }
            frame.extend_from_slice(&chunk[..n]);
        }
    })
    .await
    .map_err(|_| "request_timeout".to_owned())?
}
async fn handle(mut stream: UnixStream, broker: Arc<Broker>, binding: Binding) {
    let reply = if stream.peer_cred().map(|c| c.uid()).ok() != Some(binding.peer_uid) {
        Reply::error("peer_denied")
    } else {
        match broker.slots.clone().try_acquire_owned() {
            Err(_) => Reply::error("broker_busy"),
            Ok(_slot) => match frame(&mut stream, MAX_FRAME, Duration::from_secs(10)).await {
                Err(e) => Reply::error(&e),
                Ok(bytes) => match serde_json::from_slice::<Request>(&bytes) {
                    Ok(r) => broker.call(&binding, r).await,
                    Err(_) => Reply::error("request_invalid"),
                },
            },
        }
    };
    if let Ok(mut bytes) = serde_json::to_vec(&reply) {
        bytes.push(b'\n');
        let _ = tokio::time::timeout(Duration::from_secs(5), stream.write_all(&bytes)).await;
    }
}
struct SocketGuard {
    path: PathBuf,
    dev: u64,
    ino: u64,
}
impl Drop for SocketGuard {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
async fn listener(root: &Path, b: &Binding) -> Result<(UnixListener, SocketGuard), String> {
    // Bind-mount this persistent directory read-only at /model. Replacing its
    // socket inode after restart leaves the container's directory mount valid.
    let leaf = root.join(b.account_id.to_string());
    match fs::create_dir(&leaf) {
        Ok(()) => {
            fs::set_permissions(&leaf, fs::Permissions::from_mode(0o711))
                .map_err(|_| "model_socket_directory_invalid")?;
            File::open(root)
                .and_then(|f| f.sync_all())
                .map_err(|_| "model_socket_directory_invalid")?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err("model_socket_directory_invalid".into()),
    }
    let leaf_meta = fs::symlink_metadata(&leaf).map_err(|_| "model_socket_directory_invalid")?;
    if !leaf_meta.is_dir()
        || leaf_meta.uid() != unsafe { libc::geteuid() }
        || ![0o711, 0o755].contains(&(leaf_meta.mode() & 0o777))
        || fs::canonicalize(&leaf).ok().as_deref() != Some(leaf.as_path())
    {
        return Err("model_socket_directory_invalid".into());
    }
    let path = leaf.join("model.sock");
    if path.as_os_str().len() > 100 {
        return Err("model_socket_path_too_long".into());
    }
    if let Ok(m) = fs::symlink_metadata(&path) {
        if !m.file_type().is_socket()
            || m.uid() != b.peer_uid
            || m.mode() & 0o077 != 0
            || m.nlink() != 1
        {
            return Err("model_socket_invalid".into());
        }
        match UnixStream::connect(&path).await {
            Ok(_) => return Err("model_socket_active".into()),
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                fs::remove_file(&path).map_err(|_| "model_socket_unavailable")?
            }
            Err(_) => return Err("model_socket_unavailable".into()),
        }
    }
    let listener = UnixListener::bind(&path).map_err(|_| "model_socket_unavailable")?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "model_socket_unavailable")?;
    if b.peer_uid != unsafe { libc::geteuid() } {
        use std::os::unix::ffi::OsStrExt;
        let c = std::ffi::CString::new(path.as_os_str().as_bytes())
            .map_err(|_| "model_socket_invalid")?;
        if unsafe { libc::chown(c.as_ptr(), b.peer_uid, !0) } != 0 {
            let _ = fs::remove_file(&path);
            return Err("model_socket_uid_unavailable".into());
        }
    }
    let m = fs::symlink_metadata(&path).map_err(|_| "model_socket_unavailable")?;
    Ok((
        listener,
        SocketGuard {
            path,
            dev: m.dev(),
            ino: m.ino(),
        },
    ))
}
/// Parent CLI can expose this through `RuntimeModel(Options)` without changing
/// ordinary serve/agent paths. This process never starts a personal assistant.
pub async fn run(options: &Options) -> Result<(), String> {
    let stop = CancellationToken::new();
    let signal = stop.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        signal.cancel();
    });
    serve(options, stop).await
}
async fn serve(options: &Options, stop: CancellationToken) -> Result<(), String> {
    private_dir(&options.socket_dir)?;
    let lock = OpenOptions::new()
        .read(true)
        .open(&options.socket_dir)
        .map_err(|_| "model_ipc_unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "model_broker_already_running")?;
    let broker = Broker::new(options, stop.clone())?;
    let mut listeners = JoinSet::new();
    let mut guards = Vec::new();
    for b in &broker.config.bindings {
        let (listener, guard) = listener(&options.socket_dir, b).await?;
        guards.push(guard);
        let broker = broker.clone();
        let binding = b.clone();
        let stop = stop.clone();
        listeners.spawn(async move {
            let mut requests=JoinSet::new();
            loop {tokio::select!{
                _=stop.cancelled()=>break,
                _=requests.join_next(),if !requests.is_empty()=>{},
                accepted=listener.accept()=>{let (stream,_)=accepted.map_err(|_|"model_listener_unavailable")?;requests.spawn(handle(stream,broker.clone(),binding.clone()));}
            }}
            while requests.join_next().await.is_some() {}
            Ok::<(),String>(())
        });
    }
    let result = tokio::select! {_ = stop.cancelled()=>Ok(()),r=listeners.join_next()=>r.ok_or("model_listener_stopped".to_owned()).and_then(|r|r.map_err(|_|"model_listener_stopped".to_owned())).and_then(|r|r)};
    stop.cancel();
    while listeners.join_next().await.is_some() {}
    drop(guards);
    drop(lock);
    result
}

#[derive(Args)]
pub struct ClientOptions {
    /// Fixed personal mount: /model/model.sock. Never a provider URL or key.
    #[arg(long)]
    pub socket: PathBuf,
    /// Persist this ID and the exact request locally before the first call.
    #[arg(long)]
    pub call_id: Uuid,
    #[arg(long)]
    pub run_id: Uuid,
    #[arg(long, default_value_t = 0)]
    pub ordinal: u32,
    #[arg(long)]
    pub generation: i64,
    #[arg(long)]
    pub connection_revision: Uuid,
    #[arg(long)]
    pub model: String,
    #[arg(long, default_value_t = 256)]
    pub max_output_tokens: u32,
}
/// Deliberate single text call from bounded plain stdin. No automatic retry.
/// Stdout is the requested private result/receipt, never a credential/config.
pub async fn run_client(options: &ClientOptions) -> Result<(), String> {
    let mut bytes = Zeroizing::new(Vec::new());
    std::io::stdin()
        .take(MAX_FRAME as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "client_input_unavailable")?;
    if bytes.is_empty() || bytes.len() > MAX_FRAME {
        return Err("client_input_invalid".into());
    }
    let text = std::str::from_utf8(&bytes)
        .map_err(|_| "client_input_invalid")?
        .to_owned();
    let r = Request {
        v: 1,
        call_id: options.call_id,
        run_id: options.run_id,
        ordinal: options.ordinal,
        generation: options.generation,
        connection_revision: options.connection_revision,
        model: options.model.clone(),
        max_output_tokens: options.max_output_tokens,
        messages: vec![Message {
            role: Role::User,
            text,
        }],
    };
    let request = Zeroizing::new(serde_json::to_vec(&r).map_err(|_| "client_input_invalid")?);
    let response = personal_call(&options.socket, &request).await?;
    println!(
        "{}",
        std::str::from_utf8(&response).map_err(|_| "client_response_invalid")?
    );
    Ok(())
}
/// IPC v1: one bounded JSON request followed by LF; one JSON receipt/result
/// followed by LF, then close. No account/provider fields are accepted. The
/// socket fixes owner/tenant/volume/UID; the request must name its generation.
/// A duplicate returns only a receipt: the personal caller must retain the
/// original response locally. Never make a replacement call on unknown state.
pub async fn personal_call(socket: &Path, request: &[u8]) -> Result<Vec<u8>, String> {
    if request.len() + 1 > MAX_FRAME
        || request.contains(&b'\n')
        || serde_json::from_slice::<Request>(request).is_err()
    {
        return Err("client_input_invalid".into());
    }
    let mut stream = tokio::time::timeout(Duration::from_secs(5), UnixStream::connect(socket))
        .await
        .map_err(|_| "client_connect_timeout")?
        .map_err(|_| "client_socket_unavailable")?;
    tokio::time::timeout(Duration::from_secs(5), async {
        stream.write_all(request).await?;
        stream.write_all(b"\n").await
    })
    .await
    .map_err(|_| "client_send_timeout")?
    .map_err(|_: std::io::Error| "client_send_unknown")?;
    let response = tokio::time::timeout(
        Duration::from_secs(70),
        frame(&mut stream, MAX_REPLY, Duration::from_secs(70)),
    )
    .await
    .map_err(|_| "client_result_unknown")??;
    serde_json::from_slice::<Reply>(&response).map_err(|_| "client_response_invalid")?;
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, extract::State, http::HeaderMap, response::IntoResponse, routing::post};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tokio::sync::Notify;
    struct Provider {
        count: AtomicUsize,
        mode: AtomicUsize,
        rotated_calls: AtomicUsize,
        entered: Notify,
        release: Notify,
    }
    async fn provider(
        State(p): State<Arc<Provider>>,
        headers: HeaderMap,
        axum::Json(body): axum::Json<serde_json::Value>,
    ) -> impl IntoResponse {
        p.count.fetch_add(1, Ordering::SeqCst);
        match headers.get("authorization").and_then(|v| v.to_str().ok()) {
            Some("Bearer SYNTHETIC-MODEL-KEY") => (),
            Some("Bearer SYNTHETIC-ROTATED-KEY") => {
                p.rotated_calls.fetch_add(1, Ordering::SeqCst);
            }
            _ => panic!("unexpected synthetic credential"),
        }
        assert_eq!(body["model"], "synthetic-model");
        if p.mode.load(Ordering::SeqCst) == 2 {
            p.entered.notify_one();
            p.release.notified().await;
        }
        let body = if p.mode.load(Ordering::SeqCst) == 1 {
            "data: {not-json}\n\n".to_owned()
        } else {
            concat!("data: {\"id\":\"fixture-id\",\"model\":\"synthetic-model\",\"choices\":[{\"index\":0,\"delta\":{\"role\":\"assistant\",\"content\":\"SYNTHETIC-ANSWER-CANARY\"},\"finish_reason\":null}]}\n\n",
            "data: {\"id\":\"fixture-id\",\"model\":\"synthetic-model\",\"choices\":[{\"index\":0,\"delta\":{},\"finish_reason\":\"stop\"}]}\n\n",
            "data: {\"id\":\"fixture-id\",\"model\":\"synthetic-model\",\"choices\":[],\"usage\":{\"prompt_tokens\":7,\"completion_tokens\":3}}\n\n",
            "data: [DONE]\n\n").to_owned()
        };
        ([("content-type", "text/event-stream")], body)
    }
    async fn upstream() -> (String, Arc<Provider>, tokio::task::JoinHandle<()>) {
        let p = Arc::new(Provider {
            count: AtomicUsize::new(0),
            mode: AtomicUsize::new(0),
            rotated_calls: AtomicUsize::new(0),
            entered: Notify::new(),
            release: Notify::new(),
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new()
            .route("/v1/chat/completions", post(provider))
            .with_state(p.clone());
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (format!("http://{address}/v1"), p, task)
    }
    fn dir(root: &Path, name: &str) -> PathBuf {
        let p = root.join(name);
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    fn file(path: &Path, value: &[u8]) {
        fs::write(path, value).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    fn fixture(root: &Path, url: String) -> (Options, Vec<Binding>) {
        let registry = dir(root, "r");
        let ledger = dir(root, "l");
        let socket_dir = dir(root, "s");
        let mut bindings = Vec::new();
        for email in ["a@example.invalid", "b@example.invalid"] {
            let a = runtime_registry::reserve(&registry, Uuid::new_v4(), email.into()).unwrap();
            runtime_registry::set_routing(&registry, &a, true).unwrap();
            bindings.push(Binding {
                account_id: Uuid::parse_str(&a.account_id).unwrap(),
                tenant_id: Uuid::parse_str(&a.tenant_id).unwrap(),
                volume_id: Uuid::parse_str(&a.volume_id).unwrap(),
                generation: a.generation,
                peer_uid: unsafe { libc::geteuid() },
                budget: Budget {
                    max_calls: 3,
                    max_tokens: 8192,
                    reservation_tokens: 2048,
                },
            });
        }
        let key_file = root.join("api.key");
        file(&key_file, b"SYNTHETIC-MODEL-KEY");
        let config = root.join("model.json");
        file(
            &config,
            &serde_json::to_vec(&Config {
                version: 1,
                connection_revision: Uuid::from_u128(0x123456781234423482341234567890ab),
                provider: ProviderConfig {
                    kind: moosuri_core::ProviderKind::Compatible,
                    base_url: Some(url),
                    allowed_ips: vec!["127.0.0.1".into()],
                    router_providers: vec![],
                    zero_data_retention: false,
                },
                model: "synthetic-model".into(),
                key_file: Some(key_file),
                // Native tests have one real OS uid. Never publish two sockets
                // for that uid simultaneously; switch sole configured binding
                // only after stopping the previous synthetic broker below.
                bindings: vec![bindings[0].clone()],
            })
            .unwrap(),
        );
        let fingerprint_key = root.join("fingerprint.key");
        file(&fingerprint_key, &[31; 32]);
        (
            Options {
                registry,
                ledger,
                socket_dir,
                config,
                fingerprint_key,
            },
            bindings,
        )
    }
    fn select_binding(options: &Options, binding: &Binding) {
        let mut config: Config =
            serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
        config.bindings = vec![binding.clone()];
        file(&options.config, &serde_json::to_vec(&config).unwrap());
    }
    fn request(binding: &Binding) -> Request {
        Request {
            v: 1,
            call_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            ordinal: 0,
            generation: binding.generation,
            connection_revision: Uuid::from_u128(0x123456781234423482341234567890ab),
            model: "synthetic-model".into(),
            max_output_tokens: 32,
            messages: vec![Message {
                role: Role::User,
                text: "SYNTHETIC-PROMPT-CANARY".into(),
            }],
        }
    }
    fn path(options: &Options, b: &Binding) -> PathBuf {
        options
            .socket_dir
            .join(b.account_id.to_string())
            .join("model.sock")
    }
    async fn call(path: &Path, r: &Request) -> Reply {
        let frame = serde_json::to_vec(r).unwrap();
        serde_json::from_slice(&personal_call(path, &frame).await.unwrap()).unwrap()
    }
    async fn listening(path: &Path) {
        for _ in 0..100 {
            if UnixStream::connect(path).await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        panic!("synthetic listener not ready");
    }
    #[tokio::test]
    async fn actual_unix_calls_are_owner_bound_private_once_only_and_restart_safe() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let (url, p, upstream) = upstream().await;
        let (options, b) = fixture(&root_path, url);
        let stop = CancellationToken::new();
        let o = options.clone_for_test();
        let s = stop.clone();
        let server = tokio::spawn(async move { serve(&o, s).await });
        let socket = path(&options, &b[0]);
        listening(&socket).await;
        let directory = fs::metadata(socket.parent().unwrap()).unwrap().ino();
        let r = request(&b[0]);
        let (a, c) = tokio::join!(call(&socket, &r), call(&socket, &r));
        assert!(a.state == "settled" || c.state == "settled");
        assert_eq!(p.count.load(Ordering::SeqCst), 1);
        let done = if a.state == "settled" { a } else { c };
        assert_eq!(done.output.as_deref(), Some("SYNTHETIC-ANSWER-CANARY"));
        assert_eq!(done.connection_revision, Some(r.connection_revision));
        assert_eq!(done.model.as_deref(), Some("synthetic-model"));
        assert_eq!(done.receipt.unwrap()["charged_tokens"], 10);
        let replay = call(&socket, &r).await;
        assert_eq!(replay.state, "settled_response_unavailable");
        assert!(replay.output.is_none());
        assert!(!path(&options, &b[1]).exists());
        let mut changed = request(&b[0]);
        changed.call_id = r.call_id;
        assert_eq!(call(&socket, &changed).await.state, "call_conflict");
        changed = request(&b[0]);
        changed.call_id = r.call_id;
        changed.run_id = r.run_id;
        changed.messages[0].text = "different private text".into();
        assert_eq!(call(&socket, &changed).await.state, "call_conflict");
        let mut v = serde_json::to_value(&r).unwrap();
        v["account_id"] = serde_json::json!(b[1].account_id);
        let response = personal_call(&socket, &serde_json::to_vec(&v).unwrap()).await;
        assert!(response.is_err());
        // The actual socket handler also rejects forged account/provider fields.
        let mut raw = UnixStream::connect(&socket).await.unwrap();
        raw.write_all(&serde_json::to_vec(&v).unwrap())
            .await
            .unwrap();
        raw.write_all(b"\n").await.unwrap();
        let denied: Reply = serde_json::from_slice(
            &frame(&mut raw, MAX_REPLY, Duration::from_secs(2))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(denied.state, "request_invalid");
        stop.cancel();
        server.await.unwrap().unwrap();
        assert!(socket.parent().unwrap().exists());
        // One OS uid in the native harness: switch the sole live socket rather
        // than weakening the production unique-peer-UID policy.
        select_binding(&options, &b[1]);
        let stop_b = CancellationToken::new();
        let o = options.clone_for_test();
        let s = stop_b.clone();
        let server_b = tokio::spawn(async move { serve(&o, s).await });
        let socket_b = path(&options, &b[1]);
        listening(&socket_b).await;
        assert!(!socket.exists());
        assert_eq!(call(&socket_b, &r).await.state, "call_conflict");
        stop_b.cancel();
        server_b.await.unwrap().unwrap();
        select_binding(&options, &b[0]);
        let stop = CancellationToken::new();
        let o = options.clone_for_test();
        let s = stop.clone();
        let server = tokio::spawn(async move { serve(&o, s).await });
        listening(&socket).await;
        assert_eq!(
            fs::metadata(socket.parent().unwrap()).unwrap().ino(),
            directory
        );
        assert_eq!(
            call(&socket, &r).await.state,
            "settled_response_unavailable"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 1);
        for _ in 0..2 {
            assert_eq!(call(&socket, &request(&b[0])).await.state, "settled");
        }
        assert_eq!(
            call(&socket, &request(&b[0])).await.state,
            "quota_exhausted"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 3);
        stop.cancel();
        server.await.unwrap().unwrap();
        upstream.abort();
        for f in fs::read_dir(&options.ledger).unwrap() {
            let bytes = fs::read(f.unwrap().path()).unwrap();
            for canary in [
                "SYNTHETIC-PROMPT-CANARY",
                "SYNTHETIC-ANSWER-CANARY",
                "SYNTHETIC-MODEL-KEY",
                "fixture-id",
            ] {
                assert!(!bytes.windows(canary.len()).any(|w| w == canary.as_bytes()));
            }
        }
    }
    #[tokio::test]
    async fn unknown_provider_result_stays_unknown_and_generation_fencing_hides_late_output() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let (url, p, upstream) = upstream().await;
        let (options, b) = fixture(&root_path, url);
        let broker = Broker::new(&options, CancellationToken::new()).unwrap();
        p.mode.store(1, Ordering::SeqCst);
        let r = request(&b[0]);
        let request_bytes = serde_json::to_vec(&r).unwrap();
        assert_eq!(broker.call(&b[0], r).await.state, "outcome_unknown");
        assert_eq!(
            broker
                .call(&b[0], serde_json::from_slice(&request_bytes).unwrap())
                .await
                .state,
            "outcome_unknown"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 1);
        drop(broker);
        let broker = Broker::new(&options, CancellationToken::new()).unwrap();
        assert_eq!(
            broker
                .call(&b[0], serde_json::from_slice(&request_bytes).unwrap())
                .await
                .state,
            "outcome_unknown"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 1);
        p.mode.store(2, Ordering::SeqCst);
        let r = request(&b[0]);
        let a = runtime_registry::lookup_id(&options.registry, b[0].account_id).unwrap();
        let (reply, _) = tokio::join!(broker.call(&b[0], r), async {
            p.entered.notified().await;
            runtime_registry::set_routing(&options.registry, &a, false).unwrap();
            p.release.notify_one();
        });
        assert_eq!(reply.state, "settled_response_unavailable");
        assert!(reply.output.is_none());
        assert_eq!(
            broker.call(&b[0], request(&b[0])).await.state,
            "account_fenced"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 2);
        upstream.abort();
    }
    #[tokio::test]
    async fn reserved_and_fenced_bindings_can_listen_but_cannot_dispatch() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let (url, p, upstream) = upstream().await;
        let (options, _) = fixture(&root_path, url);
        let a = runtime_registry::reserve(
            &options.registry,
            Uuid::new_v4(),
            "reserved@example.invalid".into(),
        )
        .unwrap();
        let binding = Binding {
            account_id: Uuid::parse_str(&a.account_id).unwrap(),
            tenant_id: Uuid::parse_str(&a.tenant_id).unwrap(),
            volume_id: Uuid::parse_str(&a.volume_id).unwrap(),
            generation: a.generation,
            peer_uid: unsafe { libc::geteuid() },
            budget: Budget {
                max_calls: 3,
                max_tokens: 8192,
                reservation_tokens: 2048,
            },
        };
        select_binding(&options, &binding);
        let stop = CancellationToken::new();
        let o = options.clone_for_test();
        let s = stop.clone();
        let server = tokio::spawn(async move { serve(&o, s).await });
        let socket = path(&options, &binding);
        listening(&socket).await;
        assert_eq!(
            call(&socket, &request(&binding)).await.state,
            "account_fenced"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 0);
        runtime_registry::set_routing(&options.registry, &a, true).unwrap();
        assert_eq!(call(&socket, &request(&binding)).await.state, "settled");
        assert_eq!(p.count.load(Ordering::SeqCst), 1);
        runtime_registry::set_routing(&options.registry, &a, false).unwrap();
        stop.cancel();
        server.await.unwrap().unwrap();
        let stop = CancellationToken::new();
        let o = options.clone_for_test();
        let s = stop.clone();
        let server = tokio::spawn(async move { serve(&o, s).await });
        listening(&socket).await;
        assert_eq!(
            call(&socket, &request(&binding)).await.state,
            "account_fenced"
        );
        assert_eq!(p.count.load(Ordering::SeqCst), 1);
        stop.cancel();
        server.await.unwrap().unwrap();
        upstream.abort();
    }
    #[tokio::test]
    async fn rotating_key_keeps_inflight_calls_and_replays_without_using_a_stale_key() {
        let root = tempfile::tempdir().unwrap();
        let root = root.path().canonicalize().unwrap();
        let (url, provider, upstream) = upstream().await;
        let (options, mut bindings) = fixture(&root, url);
        bindings[1].peer_uid = bindings[0].peer_uid + 1;
        let mut config: Config =
            serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
        config.bindings = bindings.clone();
        file(&options.config, &serde_json::to_vec(&config).unwrap());
        let broker = Broker::new(&options, CancellationToken::new()).unwrap();
        let a = request(&bindings[0]);
        let previous: Request = serde_json::from_value(serde_json::to_value(&a).unwrap()).unwrap();
        let (worker, first) = (broker.clone(), bindings[0].clone());
        provider.mode.store(2, Ordering::SeqCst);
        let pending = tokio::spawn(async move { worker.call(&first, a).await });
        tokio::time::timeout(Duration::from_secs(3), provider.entered.notified())
            .await
            .unwrap();

        let key = config.key_file.unwrap();
        let replacement = key.with_extension("new");
        file(&replacement, b"SYNTHETIC-ROTATED-KEY");
        fs::rename(replacement, &key).unwrap();
        provider.mode.store(0, Ordering::SeqCst);
        let b = &bindings[1];
        assert_eq!(broker.call(b, request(b)).await.state, "settled");
        assert_eq!(provider.rotated_calls.load(Ordering::SeqCst), 1);
        provider.release.notify_one();
        assert_eq!(pending.await.unwrap().state, "settled");

        // Invalid permissions block new paid calls even though a previous
        // client was usable. A completed call still has its original receipt.
        fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
        let a = &bindings[0];
        assert_eq!(
            broker.call(a, previous).await.state,
            "settled_response_unavailable"
        );
        assert_eq!(
            broker.call(b, request(b)).await.state,
            "model_credential_unavailable"
        );
        assert_eq!(provider.count.load(Ordering::SeqCst), 2);
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        // Two remaining calls are available under B's three-call budget.
        for _ in 0..2 {
            assert_eq!(broker.call(b, request(b)).await.state, "settled");
        }
        assert_eq!(broker.call(b, request(b)).await.state, "quota_exhausted");
        assert_eq!(provider.rotated_calls.load(Ordering::SeqCst), 3);
        upstream.abort();
    }
    #[test]
    fn fence_waits_for_atomic_dispatch_admission_and_blocks_following_calls() {
        use std::sync::mpsc as blocking;
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let (options, bindings) = fixture(&root_path, "http://127.0.0.1:65531/v1".into());
        let broker = Broker::new(&options, CancellationToken::new()).unwrap();
        let binding = bindings[0].clone();
        let allocation =
            runtime_registry::lookup_id(&options.registry, binding.account_id).unwrap();
        let key = CallKey {
            call_id: Uuid::new_v4(),
            account_id: binding.account_id,
            tenant_id: binding.tenant_id,
            generation: binding.generation,
            run_id: Uuid::new_v4(),
            ordinal: 0,
            connection_revision: broker.config.connection_revision,
            fingerprint: [9; 32],
        };
        let expected = CallKey {
            call_id: key.call_id,
            account_id: key.account_id,
            tenant_id: key.tenant_id,
            generation: key.generation,
            run_id: key.run_id,
            ordinal: key.ordinal,
            connection_revision: key.connection_revision,
            fingerprint: key.fingerprint,
        };
        // Hold the ledger so acceptance stops *inside* the registry fence lock.
        let held = broker.ledger.lock().unwrap();
        let admitted_broker = broker.clone();
        let admitted_binding = binding.clone();
        let accepted =
            std::thread::spawn(move || admitted_broker.admit(&admitted_binding, &key, true));
        let probe = File::open(&options.registry).unwrap();
        let mut locked = false;
        for _ in 0..100 {
            match probe.try_lock_exclusive() {
                Ok(()) => {
                    FileExt::unlock(&probe).unwrap();
                    std::thread::sleep(Duration::from_millis(2));
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                    locked = true;
                    break;
                }
                Err(e) => panic!("unexpected synthetic registry lock failure: {e}"),
            }
        }
        assert!(locked, "admission did not acquire registry lock");
        let registry = options.registry.clone();
        let (sent, received) = blocking::channel();
        let fenced = std::thread::spawn(move || {
            let result = runtime_registry::set_routing(&registry, &allocation, false);
            sent.send(()).unwrap();
            result
        });
        assert!(matches!(
            received.recv_timeout(Duration::from_millis(30)),
            Err(blocking::RecvTimeoutError::Timeout)
        ));
        drop(held);
        let permit = match accepted.join().unwrap().unwrap() {
            Admission::Fresh(p) => p,
            _ => panic!("expected one admission"),
        };
        fenced.join().unwrap().unwrap();
        received.recv_timeout(Duration::from_secs(1)).unwrap();
        // This call was accepted before the fence; the durable dispatch cannot
        // be mistaken for work newly admitted after fencing.
        let receipt = match broker
            .ledger
            .lock()
            .unwrap()
            .admit(&expected, binding.budget)
            .unwrap()
        {
            Admission::Existing(r) => r,
            _ => panic!("dispatch was not durably accepted"),
        };
        assert_eq!(
            receipt.state,
            moosuri_store::runtime_model_ledger::State::Dispatched
        );
        drop(permit);
        let later = CallKey {
            call_id: Uuid::new_v4(),
            account_id: binding.account_id,
            tenant_id: binding.tenant_id,
            generation: binding.generation,
            run_id: Uuid::new_v4(),
            ordinal: 1,
            connection_revision: broker.config.connection_revision,
            fingerprint: [10; 32],
        };
        assert!(matches!(
            broker.admit(&binding, &later, true),
            Err(moosuri_store::StoreError::NotFound)
        ));
    }
    #[tokio::test]
    async fn actual_peer_uid_stale_generation_and_private_key_boundaries_fail_closed() {
        let root = tempfile::tempdir().unwrap();
        let root_path = root.path().canonicalize().unwrap();
        let (url, p, upstream) = upstream().await;
        let (options, b) = fixture(&root_path, url);
        let broker = Broker::new(&options, CancellationToken::new()).unwrap();
        let (server, mut client) = UnixStream::pair().unwrap();
        let mut wrong = b[0].clone();
        wrong.peer_uid = wrong.peer_uid.wrapping_add(1);
        let h = tokio::spawn(handle(server, broker.clone(), wrong));
        let reply: Reply = serde_json::from_slice(
            &frame(&mut client, MAX_REPLY, Duration::from_secs(2))
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(reply.state, "peer_denied");
        h.await.unwrap();
        let mut rotated = request(&b[0]);
        rotated.connection_revision = Uuid::new_v4();
        assert_eq!(
            broker.call(&b[0], rotated).await.state,
            "model_selection_changed"
        );
        let mut replacement = request(&b[0]);
        replacement.model = "different-model".into();
        assert_eq!(
            broker.call(&b[0], replacement).await.state,
            "model_selection_changed"
        );
        let mut stale = request(&b[0]);
        stale.generation += 1;
        assert_eq!(broker.call(&b[0], stale).await.state, "request_invalid");
        assert_eq!(p.count.load(Ordering::SeqCst), 0);
        drop(broker);
        let mut duplicate_uid: Config =
            serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
        duplicate_uid.bindings = b.clone();
        file(
            &options.config,
            &serde_json::to_vec(&duplicate_uid).unwrap(),
        );
        assert!(Broker::new(&options, CancellationToken::new()).is_err());
        select_binding(&options, &b[0]);
        fs::set_permissions(&options.fingerprint_key, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Broker::new(&options, CancellationToken::new()).is_err());
        upstream.abort();
    }
    impl Options {
        fn clone_for_test(&self) -> Self {
            Self {
                registry: self.registry.clone(),
                ledger: self.ledger.clone(),
                socket_dir: self.socket_dir.clone(),
                config: self.config.clone(),
                fingerprint_key: self.fingerprint_key.clone(),
            }
        }
    }
}
