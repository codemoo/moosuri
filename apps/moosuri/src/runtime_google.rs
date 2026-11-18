//! Opt-in, per-owner Google egress. Fixed OAuth/Gmail read operations only;
//! tokens and mailbox content are transient private IPC, never broker storage.
//! This component does not capture callbacks or grant a browser session.
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::Args;
use fs2::FileExt;
use moosuri_connectors::{
    gmail::{GmailClient, GmailError, OAuthClient, READ_SCOPE, SecretString, TokenSet},
    model::CancellationToken,
};
use moosuri_store::runtime_registry;
use ring::hmac;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::Semaphore,
    task::JoinSet,
};
use uuid::Uuid;
use zeroize::Zeroizing;
const MAX_REQUEST: usize = 64 * 1024;
const MAX_REPLY: usize = 128 * 1024;
const CALLBACK: &str = "/api/v1/mail/oauth/callback";
const REFRESH_LIFETIME: u64 = 400 * 24 * 3600;
pub const PERSONAL_SOCKET: &str = "/google/google.sock";
#[derive(Args)]
pub struct Options {
    /// Existing private installation account registry.
    #[arg(long)]
    pub registry: PathBuf,
    /// Existing private host IPC root; mount only one UUID child at /google:ro.
    #[arg(long)]
    pub socket_dir: PathBuf,
    /// Owner-only JSON: app revision, downloaded Google client file, bindings.
    #[arg(long)]
    pub config: PathBuf,
    /// Persistent owner-only 32-byte random key for owner-bound token receipts.
    #[arg(long)]
    pub capability_key: PathBuf,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    app_revision: Uuid,
    client_file: PathBuf,
    redirect_uri: String,
    bindings: Vec<Binding>,
}
#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Binding {
    pub(super) account_id: Uuid,
    pub(super) tenant_id: Uuid,
    pub(super) volume_id: Uuid,
    pub(super) generation: i64,
    pub(super) peer_uid: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Owner {
    account_id: Uuid,
    tenant_id: Uuid,
    volume_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    v: u32,
    owner: Owner,
    request_id: Uuid,
    generation: i64,
    app_revision: Option<Uuid>,
    operation: Operation,
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Operation {
    Describe,
    AuthorizationUrl {
        state: String,
        verifier: SecretString,
    },
    Exchange {
        code: SecretString,
        verifier: SecretString,
    },
    Refresh {
        refresh_token: SecretString,
        refresh_binding: String,
    },
    InboxPage {
        access_token: SecretString,
        access_binding: String,
    },
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenBinding {
    v: u32,
    owner: Binding,
    app_revision: Uuid,
    purpose: String,
    mailbox: String,
    token_tag: String,
    expires_at: u64,
}
struct Broker {
    registry: PathBuf,
    revision: Uuid,
    client: OAuthClient,
    gmail: GmailClient,
    key: hmac::Key,
    slots: Arc<Semaphore>,
    stop: CancellationToken,
}
// Explicit serialization is confined to protected IPC. This carrier has no Debug.
struct SecretWire<'a>(&'a SecretString);
impl Serialize for SecretWire<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0.expose())
    }
}
#[derive(Serialize)]
struct Tokens<'a> {
    access_token: SecretWire<'a>,
    refresh_token: Option<SecretWire<'a>>,
    expires_in: u64,
    scopes: [&'static str; 1],
    mailbox: String,
    access_binding: String,
    refresh_binding: String,
}
#[derive(Serialize)]
struct Metadata {
    id: String,
    received_at_ms: i64,
    subject: String,
    from: String,
    date: String,
}
#[derive(Serialize)]
struct Inbox {
    mailbox: String,
    messages: Vec<Metadata>,
    has_more: bool,
    scope: &'static str,
}
#[derive(Serialize)]
struct Reply<T: Serialize> {
    v: u32,
    request_id: Uuid,
    owner: Binding,
    app_revision: Uuid,
    state: &'static str,
    result: T,
}
// Ignore result while checking the carrier, avoiding an ordinary heap copy of
// access/refresh secrets inside a general serde_json::Value.
#[derive(Deserialize)]
struct Correlation<'a> {
    v: u32,
    #[serde(borrow)]
    state: &'a str,
    request_id: Option<Uuid>,
    owner: Option<Binding>,
    app_revision: Option<Uuid>,
}
pub(super) fn private_dir(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|_| "google_directory_unavailable")?;
    if !path.is_absolute()
        || !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || fs::canonicalize(path).ok().as_deref() != Some(path)
    {
        return Err("google_directory_invalid".into());
    }
    Ok(())
}
pub(super) fn private_bytes(path: &Path, cap: usize) -> Result<Zeroizing<Vec<u8>>, String> {
    if !path.is_absolute() || fs::canonicalize(path).ok().as_deref() != Some(path) {
        return Err("google_file_invalid".into());
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "google_file_unavailable")?;
    let m = file.metadata().map_err(|_| "google_file_unavailable")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
        || m.len() > cap as u64
    {
        return Err("google_file_invalid".into());
    }
    let mut bytes = Zeroizing::new(Vec::new());
    file.take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "google_file_unavailable")?;
    if bytes.len() > cap {
        return Err("google_file_invalid".into());
    }
    Ok(bytes)
}
fn now() -> Result<u64, &'static str> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|n| n.as_secs())
        .map_err(|_| "clock_unavailable")
}
fn error(code: &'static str) -> Zeroizing<Vec<u8>> {
    Zeroizing::new(serde_json::to_vec(&serde_json::json!({"v":1,"state":code})).unwrap())
}
fn provider(error: GmailError) -> &'static str {
    match error {
        GmailError::Http(401 | 403) => "google_reauthentication_required",
        GmailError::Http(429) => "google_rate_limited",
        GmailError::InvalidInput => "google_request_invalid",
        _ => "google_result_unavailable",
    }
}
pub(super) fn same_owner(a: &runtime_registry::Allocation, b: Binding) -> bool {
    a.account_id == b.account_id.to_string()
        && a.tenant_id == b.tenant_id.to_string()
        && a.volume_id == b.volume_id.to_string()
        && a.generation == b.generation
}
pub(super) fn epoch(registry: &Path, b: Binding) -> Result<i64, &'static str> {
    let (a, epoch) = runtime_registry::session_route(
        registry,
        runtime_registry::SessionDestination::Account(b.account_id),
    )
    .map_err(|_| "account_fenced")?;
    if !same_owner(&a, b) {
        return Err("account_fenced");
    }
    Ok(epoch)
}
fn uninterrupted(registry: &Path, b: Binding, expected: i64) -> Result<(), &'static str> {
    if epoch(registry, b)? != expected {
        return Err("account_fenced");
    }
    Ok(())
}
impl Broker {
    fn new(
        options: &Options,
        stop: CancellationToken,
    ) -> Result<(Arc<Self>, Vec<Binding>), String> {
        private_dir(&options.registry)?;
        let config: Config = serde_json::from_slice(&private_bytes(&options.config, 64 * 1024)?)
            .map_err(|_| "google_config_invalid")?;
        let redirect =
            url::Url::parse(&config.redirect_uri).map_err(|_| "google_redirect_invalid")?;
        if config.app_revision.is_nil()
            || config.bindings.is_empty()
            || config.bindings.len() > 128
            || redirect.scheme() != "https"
            || redirect.path() != CALLBACK
            || redirect.query().is_some()
            || redirect.fragment().is_some()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
        {
            return Err("google_config_invalid".into());
        }
        let mut ids = BTreeSet::new();
        let mut uids = BTreeSet::new();
        for b in &config.bindings {
            if b.account_id.is_nil()
                || b.tenant_id.is_nil()
                || b.volume_id.is_nil()
                || b.generation <= 0
                || !ids.insert(b.account_id)
                || !uids.insert(b.peer_uid)
            {
                return Err("google_binding_invalid".into());
            }
            // Provision inert sockets before the member is ready. Dispatch
            // always requires a currently ready registry allocation.
            let a = runtime_registry::lookup_allocated_id(&options.registry, b.account_id)
                .map_err(|_| "account_fenced")?;
            if !same_owner(&a, *b) {
                return Err("account_fenced".into());
            }
        }
        let client = OAuthClient::from_web_client_json(
            std::str::from_utf8(&private_bytes(&config.client_file, 16384)?)
                .map_err(|_| "google_client_invalid")?,
            &config.redirect_uri,
        )
        .map_err(|_| "google_client_invalid")?;
        let key = private_bytes(&options.capability_key, 32)?;
        if key.len() != 32 {
            return Err("google_capability_key_invalid".into());
        }
        Ok((
            Arc::new(Self {
                registry: options.registry.clone(),
                revision: config.app_revision,
                client,
                gmail: GmailClient::google(),
                key: hmac::Key::new(hmac::HMAC_SHA256, &key),
                slots: Arc::new(Semaphore::new(8)),
                stop,
            }),
            config.bindings,
        ))
    }
    fn tag(&self, token: &SecretString) -> String {
        URL_SAFE_NO_PAD.encode(hmac::sign(&self.key, token.expose().as_bytes()).as_ref())
    }
    fn issue(
        &self,
        b: Binding,
        purpose: &str,
        mailbox: &str,
        token: &SecretString,
        lifetime: u64,
    ) -> Result<String, &'static str> {
        let claims = TokenBinding {
            v: 1,
            owner: b,
            app_revision: self.revision,
            purpose: purpose.into(),
            mailbox: mailbox.into(),
            token_tag: self.tag(token),
            expires_at: now()?.checked_add(lifetime).ok_or("clock_unavailable")?,
        };
        let payload = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(&claims).map_err(|_| "google_receipt_unavailable")?);
        let signature = URL_SAFE_NO_PAD.encode(hmac::sign(&self.key, payload.as_bytes()).as_ref());
        Ok(format!("{payload}.{signature}"))
    }
    fn check(
        &self,
        b: Binding,
        purpose: &str,
        receipt: &str,
        token: &SecretString,
    ) -> Result<String, &'static str> {
        if receipt.len() > 4096 {
            return Err("google_binding_denied");
        }
        let (payload, signature) = receipt.split_once('.').ok_or("google_binding_denied")?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| "google_binding_denied")?;
        hmac::verify(&self.key, payload.as_bytes(), &signature)
            .map_err(|_| "google_binding_denied")?;
        let c: TokenBinding = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(payload)
                .map_err(|_| "google_binding_denied")?,
        )
        .map_err(|_| "google_binding_denied")?;
        if c.v != 1
            || c.owner.account_id != b.account_id
            || c.owner.tenant_id != b.tenant_id
            || c.owner.volume_id != b.volume_id
            || c.owner.generation != b.generation
            || c.owner.peer_uid != b.peer_uid
            || c.app_revision != self.revision
            || c.purpose != purpose
            || c.expires_at <= now()?
            || c.token_tag != self.tag(token)
        {
            return Err("google_binding_denied");
        }
        Ok(c.mailbox)
    }
    fn success<T: Serialize>(
        &self,
        b: Binding,
        id: Uuid,
        result: T,
    ) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        let reply = Reply {
            v: 1,
            request_id: id,
            owner: b,
            app_revision: self.revision,
            state: "ok",
            result,
        };
        let bytes =
            Zeroizing::new(serde_json::to_vec(&reply).map_err(|_| "google_response_invalid")?);
        if bytes.len() + 1 > MAX_REPLY {
            return Err("google_response_invalid");
        }
        Ok(bytes)
    }
    async fn tokens(
        &self,
        b: Binding,
        id: Uuid,
        tokens: TokenSet,
        grant_epoch: i64,
        previous: Option<&SecretString>,
        expected: Option<&str>,
    ) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        // Scope omission is allowed only for refresh of our own authenticated
        // read-only grant; OAuth defines that omission as unchanged scope.
        if tokens
            .scopes
            .as_ref()
            .is_some_and(|s| s.len() != 1 || s[0] != READ_SCOPE)
            || tokens.scopes.is_none() && previous.is_none()
        {
            return Err("google_scope_denied");
        }
        let refresh = tokens
            .refresh_token
            .as_ref()
            .or(previous)
            .ok_or("google_offline_grant_missing")?;
        uninterrupted(&self.registry, b, grant_epoch)?;
        let mailbox = self
            .gmail
            .profile(&tokens.access_token, &self.stop)
            .await
            .map_err(provider)?
            .email_address;
        if expected.is_some_and(|e| !e.eq_ignore_ascii_case(&mailbox)) {
            return Err("google_account_mismatch");
        }
        uninterrupted(&self.registry, b, grant_epoch)?;
        let access_binding = self.issue(
            b,
            "access",
            &mailbox,
            &tokens.access_token,
            tokens.expires_in,
        )?;
        let refresh_binding = self.issue(b, "refresh", &mailbox, refresh, REFRESH_LIFETIME)?;
        self.success(
            b,
            id,
            Tokens {
                access_token: SecretWire(&tokens.access_token),
                refresh_token: tokens.refresh_token.as_ref().map(SecretWire),
                expires_in: tokens.expires_in,
                scopes: [READ_SCOPE],
                mailbox,
                access_binding,
                refresh_binding,
            },
        )
    }
    async fn dispatch(&self, b: Binding, r: Request) -> Result<Zeroizing<Vec<u8>>, &'static str> {
        if r.v != 1
            || r.request_id.is_nil()
            || r.generation != b.generation
            || r.owner.account_id != b.account_id
            || r.owner.tenant_id != b.tenant_id
            || r.owner.volume_id != b.volume_id
        {
            return Err("google_request_invalid");
        }
        let grant_epoch = epoch(&self.registry, b)?;
        if !matches!(r.operation, Operation::Describe) && r.app_revision != Some(self.revision) {
            return Err("google_app_changed");
        }
        let response = match r.operation {
            Operation::Describe => self.success(b,r.request_id,serde_json::json!({"client_id":self.client.client_id,"redirect_uri":self.client.redirect_uri,"scope":READ_SCOPE,"operations":["authorization_url","exchange","refresh","inbox_page"]})),
            Operation::AuthorizationUrl { state, verifier } => {
                let url = GmailClient::authorization_url_scoped(
                    &self.client.client_id, &self.client.redirect_uri, &state,
                    verifier.expose(), &[READ_SCOPE],
                ).map_err(provider)?;
                self.success(b,r.request_id,serde_json::json!({"authorization_url":url}))
            }
            Operation::Exchange { code, verifier } => {
                let tokens = self.gmail.exchange_code(&self.client,&code,&verifier,&self.stop).await.map_err(provider)?;
                self.tokens(b,r.request_id,tokens,grant_epoch,None,None).await
            }
            Operation::Refresh { refresh_token, refresh_binding } => {
                let email = self.check(b,"refresh",&refresh_binding,&refresh_token)?;
                let tokens = self.gmail.refresh(&self.client,&refresh_token,&self.stop).await.map_err(provider)?;
                self.tokens(b,r.request_id,tokens,grant_epoch,Some(&refresh_token),Some(&email)).await
            }
            Operation::InboxPage { access_token, access_binding } => {
                let mailbox = self.check(b,"access",&access_binding,&access_token)?;
                // One explicit page, five metadata projections. Never label this
                // bounded observation as the entire mailbox's latest message.
                let page = self.gmail.list_messages(&access_token,"in:inbox",None,5,&self.stop).await.map_err(provider)?;
                let mut messages = Vec::new();
                let mut seen = BTreeSet::new();
                for item in page.messages {
                    uninterrupted(&self.registry,b,grant_epoch)?;
                    if !seen.insert(item.id.clone()) { return Err("google_response_invalid"); }
                    let m = self.gmail.inbox_metadata(&access_token,&item.id,&self.stop).await.map_err(provider)?;
                    messages.push(Metadata {id:m.id,received_at_ms:m.received_at_ms,subject:m.subject,from:m.from,date:m.date});
                }
                messages.sort_by(|a,b|b.received_at_ms.cmp(&a.received_at_ms).then_with(||b.id.cmp(&a.id)));
                self.success(b,r.request_id,Inbox {mailbox,messages,has_more:page.next_page_token.is_some(),scope:"one_inbox_page_metadata_only"})
            }
        }?;
        uninterrupted(&self.registry, b, grant_epoch)?;
        Ok(response)
    }
}
async fn frame(stream: &mut UnixStream, cap: usize) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    let mut bytes = Zeroizing::new(Vec::new());
    let mut chunk = Zeroizing::new([0u8; 1024]);
    loop {
        let n = stream
            .read(&mut chunk[..])
            .await
            .map_err(|_| "google_ipc_unavailable")?;
        if n == 0 {
            return Err("google_ipc_truncated");
        }
        if bytes.len() + n > cap {
            return Err("google_ipc_limit");
        }
        if let Some(end) = chunk[..n].iter().position(|b| *b == b'\n') {
            if end + 1 != n {
                return Err("google_ipc_invalid");
            }
            bytes.extend_from_slice(&chunk[..end]);
            return Ok(bytes);
        }
        bytes.extend_from_slice(&chunk[..n]);
    }
}
async fn handle(
    mut stream: UnixStream,
    broker: Arc<Broker>,
    binding: Binding,
    owner_slots: Arc<Semaphore>,
) {
    let response = if stream.peer_cred().map(|p| p.uid()).ok() != Some(binding.peer_uid) {
        error("peer_denied")
    } else {
        match (
            broker.slots.clone().try_acquire_owned(),
            owner_slots.try_acquire_owned(),
        ) {
            (Ok(_global), Ok(_owner)) => {
                match tokio::time::timeout(Duration::from_secs(90), async {
                    let bytes = frame(&mut stream, MAX_REQUEST).await?;
                    let request: Request =
                        serde_json::from_slice(&bytes).map_err(|_| "google_request_invalid")?;
                    broker.dispatch(binding, request).await
                })
                .await
                {
                    Ok(Ok(value)) => value,
                    Ok(Err(code)) => error(code),
                    Err(_) => error("google_result_unknown"),
                }
            }
            _ => error("google_broker_busy"),
        }
    };
    let _ = tokio::time::timeout(Duration::from_secs(5), async {
        stream.write_all(&response).await?;
        stream.write_all(b"\n").await
    })
    .await;
}
pub(super) struct SocketGuard {
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
pub(super) async fn listener(
    root: &Path,
    b: Binding,
) -> Result<(UnixListener, SocketGuard), String> {
    let leaf = root.join(b.account_id.to_string());
    match fs::create_dir(&leaf) {
        Ok(()) => {
            fs::set_permissions(&leaf, fs::Permissions::from_mode(0o711))
                .map_err(|_| "google_socket_directory_invalid")?;
            File::open(root)
                .and_then(|f| f.sync_all())
                .map_err(|_| "google_socket_directory_invalid")?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => (),
        Err(_) => return Err("google_socket_directory_invalid".into()),
    }
    let meta = fs::symlink_metadata(&leaf).map_err(|_| "google_socket_directory_invalid")?;
    if !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o777 != 0o711
        || fs::canonicalize(&leaf).ok().as_deref() != Some(leaf.as_path())
    {
        return Err("google_socket_directory_invalid".into());
    }
    let path = leaf.join("google.sock");
    if path.as_os_str().len() > 100 {
        return Err("google_socket_path_too_long".into());
    }
    if let Ok(m) = fs::symlink_metadata(&path) {
        if !m.file_type().is_socket()
            || m.uid() != b.peer_uid
            || m.nlink() != 1
            || m.mode() & 0o777 != 0o600
        {
            return Err("google_socket_invalid".into());
        }
        match UnixStream::connect(&path).await {
            Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                fs::remove_file(&path).map_err(|_| "google_socket_unavailable")?
            }
            _ => return Err("google_socket_active".into()),
        }
    }
    let listener = UnixListener::bind(&path).map_err(|_| "google_socket_unavailable")?;
    let m = fs::symlink_metadata(&path).map_err(|_| "google_socket_unavailable")?;
    let guard = SocketGuard {
        path: path.clone(),
        dev: m.dev(),
        ino: m.ino(),
    };
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .map_err(|_| "google_socket_unavailable")?;
    if b.peer_uid != unsafe { libc::geteuid() } {
        std::os::unix::fs::chown(&path, Some(b.peer_uid), None)
            .map_err(|_| "google_socket_uid_unavailable")?;
    }
    Ok((listener, guard))
}
pub async fn run(options: &Options) -> Result<(), String> {
    let stop = CancellationToken::new();
    let signal = stop.clone();
    tokio::spawn(async move {
        crate::shutdown_signal().await;
        signal.cancel();
    });
    serve(options, stop, None).await
}
async fn serve(
    options: &Options,
    stop: CancellationToken,
    started: Option<tokio::sync::oneshot::Sender<()>>,
) -> Result<(), String> {
    let (broker, bindings) = Broker::new(options, stop.clone())?;
    serve_broker(&options.socket_dir, broker, bindings, stop, started).await
}
async fn serve_broker(
    root: &Path,
    broker: Arc<Broker>,
    bindings: Vec<Binding>,
    stop: CancellationToken,
    started: Option<tokio::sync::oneshot::Sender<()>>,
) -> Result<(), String> {
    private_dir(root)?;
    let lock = File::open(root).map_err(|_| "google_socket_unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "google_broker_already_running")?;
    let mut tasks = JoinSet::new();
    let mut guards = Vec::new();
    for b in bindings {
        let (listener, guard) = listener(root, b).await?;
        guards.push(guard);
        let broker = broker.clone();
        let stop = stop.clone();
        let slots = Arc::new(Semaphore::new(1));
        tasks.spawn(async move {
            let mut clients = JoinSet::new();
            loop {
                tokio::select! {
                    _ = stop.cancelled() => break,
                    accepted = listener.accept() => {
                        let (stream, _) = accepted.map_err(|_| "google_listener_unavailable".to_string())?;
                        clients.spawn(handle(stream, broker.clone(), b, slots.clone()));
                    }
                    Some(_) = clients.join_next(), if !clients.is_empty() => {}
                }
            }
            Ok::<(), String>(())
        });
    }
    if let Some(started) = started {
        let _ = started.send(());
    }
    let result = tokio::select! {
        _ = stop.cancelled() => Ok(()),
        ended = tasks.join_next() => match ended {
            Some(Ok(result)) => result,
            _ => Err("google_listener_stopped".into()),
        }
    };
    stop.cancel();
    while tasks.join_next().await.is_some() {}
    drop(guards);
    result
}
#[derive(Args)]
pub struct ClientOptions {
    #[arg(long)]
    pub broker_uid: u32,
}
/// Public configuration/consent diagnostics only; token operations use the
/// authenticated in-process API and must go directly to private member storage.
pub async fn run_client(options: &ClientOptions) -> Result<(), String> {
    let mut request = Zeroizing::new(Vec::new());
    std::io::stdin()
        .take(MAX_REQUEST as u64 + 1)
        .read_to_end(&mut request)
        .map_err(|_| "google_input_unavailable")?;
    let parsed: Request = serde_json::from_slice(&request).map_err(|_| "google_request_invalid")?;
    if !matches!(
        parsed.operation,
        Operation::Describe | Operation::AuthorizationUrl { .. }
    ) {
        return Err("google_private_api_required".into());
    }
    let reply = personal_call(Path::new(PERSONAL_SOCKET), options.broker_uid, &request)
        .await
        .map_err(str::to_owned)?;
    println!(
        "{}",
        std::str::from_utf8(&reply).map_err(|_| "google_response_invalid")?
    );
    Ok(())
}
pub async fn personal_call(
    socket: &Path,
    broker_uid: u32,
    request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if socket != Path::new(PERSONAL_SOCKET) {
        return Err("google_socket_invalid");
    }
    authenticated_call(socket, broker_uid, request).await
}
async fn authenticated_call(
    socket: &Path,
    broker_uid: u32,
    request: &[u8],
) -> Result<Zeroizing<Vec<u8>>, &'static str> {
    if !socket.is_absolute() || request.len() + 1 > MAX_REQUEST || request.contains(&b'\n') {
        return Err("google_request_invalid");
    }
    let expected: Request =
        serde_json::from_slice(request).map_err(|_| "google_request_invalid")?;
    let m = fs::symlink_metadata(socket).map_err(|_| "google_socket_unavailable")?;
    if !m.file_type().is_socket()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o077 != 0
    {
        return Err("google_socket_invalid");
    }
    tokio::time::timeout(Duration::from_secs(100), async {
        let mut stream = UnixStream::connect(socket)
            .await
            .map_err(|_| "google_socket_unavailable")?;
        if stream.peer_cred().map(|p| p.uid()).ok() != Some(broker_uid) {
            return Err("broker_identity_denied");
        }
        stream
            .write_all(request)
            .await
            .map_err(|_| "google_result_unknown")?;
        stream
            .write_all(b"\n")
            .await
            .map_err(|_| "google_result_unknown")?;
        let mut response = Zeroizing::new(Vec::new());
        let mut chunk = Zeroizing::new([0u8; 4096]);
        loop {
            let n = stream
                .read(&mut chunk[..])
                .await
                .map_err(|_| "google_result_unknown")?;
            if n == 0 {
                break;
            }
            if response.len() + n > MAX_REPLY {
                return Err("google_response_invalid");
            }
            response.extend_from_slice(&chunk[..n]);
        }
        if response.last() != Some(&b'\n') || response[..response.len() - 1].contains(&b'\n') {
            return Err("google_response_invalid");
        }
        response.pop();
        let v: Correlation<'_> =
            serde_json::from_slice(&response).map_err(|_| "google_response_invalid")?;
        if v.v != 1 || v.state.is_empty() || v.state.len() > 64 {
            return Err("google_response_invalid");
        }
        if v.state == "ok" {
            let owner = v.owner.ok_or("google_response_invalid")?;
            if v.request_id != Some(expected.request_id)
                || owner.account_id != expected.owner.account_id
                || owner.tenant_id != expected.owner.tenant_id
                || owner.volume_id != expected.owner.volume_id
                || owner.generation != expected.generation
                || owner.peer_uid != unsafe { libc::geteuid() }
                || v.app_revision.is_none_or(|revision| revision.is_nil())
                || expected
                    .app_revision
                    .is_some_and(|revision| v.app_revision != Some(revision))
            {
                return Err("google_response_invalid");
            }
        }
        Ok(response)
    })
    .await
    .map_err(|_| "google_result_unknown")?
}

#[cfg(all(test, feature = "test-fixtures"))]
pub(crate) mod tests {
    use super::*;
    use axum::{Router, body::to_bytes, extract::State, http::Request, routing::any};
    use serde_json::{Value, json};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use tokio::sync::Notify;

    #[derive(Default)]
    struct Provider {
        calls: AtomicUsize,
        metadata: AtomicUsize,
        hold: AtomicBool,
        entered: Notify,
        release: Notify,
    }
    async fn provider(
        State(p): State<Arc<Provider>>,
        r: Request<axum::body::Body>,
    ) -> axum::Json<Value> {
        p.calls.fetch_add(1, Ordering::SeqCst);
        let path = r.uri().path().to_owned();
        let query = r.uri().query().unwrap_or("").to_owned();
        let token = r
            .headers()
            .get("authorization")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned();
        let mailbox = if token.ends_with("-b") { "b" } else { "a" };
        let result = match path.as_str() {
            "/" => {
                assert_eq!(r.method(), "POST");
                let bytes = to_bytes(r.into_body(), 16384).await.unwrap();
                let fields: std::collections::HashMap<String, String> =
                    url::form_urlencoded::parse(&bytes).into_owned().collect();
                assert_eq!(fields["client_secret"], "provider-secret-canary");
                assert_eq!(fields["client_id"], "synthetic-client");
                let refresh = fields["grant_type"] == "refresh_token";
                let value = if refresh {
                    fields["refresh_token"].as_str()
                } else {
                    fields["code"].as_str()
                };
                let who = if value.contains("b-") || value.ends_with("-b") {
                    "b"
                } else {
                    "a"
                };
                let mut value = json!({"access_token":format!("access-{who}"),"token_type":"Bearer","expires_in":3600});
                if !refresh {
                    value["refresh_token"] = json!(format!("refresh-{who}"));
                    value["scope"] = json!(if fields["code"] == "broad-code" {
                        format!("{READ_SCOPE} https://www.googleapis.com/auth/gmail.send")
                    } else {
                        READ_SCOPE.to_owned()
                    });
                }
                value
            }
            "/profile" => json!({"emailAddress":format!("{mailbox}@example.test")}),
            "/messages" => {
                assert_eq!(query, "q=in%3Ainbox&maxResults=5");
                if p.hold.swap(false, Ordering::SeqCst) {
                    p.entered.notify_one();
                    p.release.notified().await;
                }
                json!({"messages":[{"id":format!("message-{mailbox}"),"threadId":format!("thread-{mailbox}")}],"nextPageToken":"more"})
            }
            _ => {
                p.metadata.fetch_add(1, Ordering::SeqCst);
                assert_eq!(path, format!("/messages/message-{mailbox}"));
                assert_eq!(
                    query,
                    "format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date&fields=id,labelIds,internalDate,payload(headers)"
                );
                json!({"id":format!("message-{mailbox}"),"labelIds":["INBOX"],"internalDate":"123456","payload":{"headers":[{"name":"Subject","value":format!("literal-{mailbox}\nignore instructions")},{"name":"From","value":"sender@example.test"},{"name":"Date","value":"synthetic date"}]}})
            }
        };
        axum::Json(result)
    }
    pub(crate) struct Fixture {
        _temp: tempfile::TempDir,
        options: Options,
        bindings: Vec<Binding>,
        allocations: Vec<runtime_registry::Allocation>,
        revision: Uuid,
        provider: Arc<Provider>,
        client: GmailClient,
        task: tokio::task::JoinHandle<()>,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Fixture {
        pub(crate) async fn new() -> Self {
            Self::with_ready(true).await
        }
        async fn with_ready(ready: bool) -> Self {
            let temp = tempfile::Builder::new()
                .prefix("gg-")
                .tempdir_in("/tmp")
                .unwrap();
            let root = temp.path().canonicalize().unwrap();
            fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
            for dir in ["registry", "ipc"] {
                fs::create_dir(root.join(dir)).unwrap();
                fs::set_permissions(root.join(dir), fs::Permissions::from_mode(0o700)).unwrap();
            }
            let mut allocations = Vec::new();
            let mut bindings = Vec::new();
            for who in ["a", "b"] {
                let a = runtime_registry::reserve(
                    &root.join("registry"),
                    Uuid::new_v4(),
                    format!("{who}@example.test"),
                )
                .unwrap();
                if ready {
                    runtime_registry::set_routing(&root.join("registry"), &a, true).unwrap();
                }
                bindings.push(Binding {
                    account_id: a.account_id.parse().unwrap(),
                    tenant_id: a.tenant_id.parse().unwrap(),
                    volume_id: a.volume_id.parse().unwrap(),
                    generation: a.generation,
                    peer_uid: unsafe { libc::geteuid() },
                });
                allocations.push(a);
            }
            let revision = Uuid::new_v4();
            // macOS acceptance runs under one UID. Production configuration
            // must have distinct UIDs; only the synthetic test listener below
            // collapses them to exercise two owner tuples in one process.
            let configured: Vec<_> = bindings
                .iter()
                .enumerate()
                .map(|(i, b)| Binding {
                    peer_uid: b.peer_uid + i as u32,
                    ..*b
                })
                .collect();
            let redirect = "https://assistant.example.test/api/v1/mail/oauth/callback";
            let client_file = root.join("client.json");
            let config = root.join("config.json");
            let key = root.join("key");
            fs::write(&client_file,serde_json::to_vec(&json!({"web":{"client_id":"synthetic-client","client_secret":"provider-secret-canary","redirect_uris":[redirect]}})).unwrap()).unwrap();
            fs::write(&config,serde_json::to_vec(&json!({"app_revision":revision,"client_file":client_file,"redirect_uri":redirect,"bindings":configured})).unwrap()).unwrap();
            fs::write(&key, [37u8; 32]).unwrap();
            for path in [&client_file, &config, &key] {
                fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
            }
            let provider = Arc::new(Provider::default());
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let client = GmailClient::synthetic_loopback(listener.local_addr().unwrap()).unwrap();
            let state = provider.clone();
            let task = tokio::spawn(async move {
                axum::serve(
                    listener,
                    Router::new()
                        .fallback(any(self::provider))
                        .with_state(state),
                )
                .await
                .unwrap();
            });
            Self {
                options: Options {
                    registry: root.join("registry"),
                    socket_dir: root.join("ipc"),
                    config,
                    capability_key: key,
                },
                _temp: temp,
                bindings,
                allocations,
                revision,
                provider,
                client,
                task,
            }
        }
        pub(crate) async fn start(
            &self,
        ) -> (
            CancellationToken,
            tokio::task::JoinHandle<Result<(), String>>,
        ) {
            let stop = CancellationToken::new();
            let (mut broker, mut bindings) = Broker::new(&self.options, stop.clone()).unwrap();
            for b in &mut bindings {
                b.peer_uid = unsafe { libc::geteuid() };
            }
            Arc::get_mut(&mut broker).unwrap().gmail = self.client.clone();
            let root = self.options.socket_dir.clone();
            let signal = stop.clone();
            let (tx, rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move {
                serve_broker(&root, broker, bindings, signal, Some(tx)).await
            });
            tokio::time::timeout(Duration::from_secs(3), rx)
                .await
                .unwrap()
                .unwrap();
            (stop, task)
        }
        fn request(&self, index: usize, operation: Value) -> Value {
            let b = self.bindings[index];
            json!({"v":1,"request_id":Uuid::new_v4(),"owner":{"account_id":b.account_id,"tenant_id":b.tenant_id,"volume_id":b.volume_id},"generation":b.generation,"app_revision":self.revision,"operation":operation})
        }
        pub(crate) fn registry(&self) -> PathBuf {
            self.options.registry.clone()
        }
        pub(crate) fn transport(&self, index: usize) -> crate::personal_google::TestTransport {
            let socket = self.socket(index);
            Arc::new(move |request| {
                let path = socket.clone();
                Box::pin(async move {
                    authenticated_call(&path, unsafe { libc::geteuid() }, &request).await
                })
            })
        }
        fn socket(&self, index: usize) -> PathBuf {
            self.options
                .socket_dir
                .join(self.bindings[index].account_id.to_string())
                .join("google.sock")
        }
        async fn call(&self, index: usize, operation: Value) -> Value {
            call(&self.socket(index), self.request(index, operation)).await
        }
        async fn exchange(&self, index: usize) -> Value {
            let v = self.call(index,json!({"kind":"exchange","code":if index==0 {"a-code"} else {"b-code"},"verifier":"v".repeat(43)})).await;
            assert_eq!(v["state"], "ok");
            v["result"].clone()
        }
    }
    async fn call(path: &Path, request: Value) -> Value {
        serde_json::from_slice(
            &authenticated_call(
                path,
                unsafe { libc::geteuid() },
                &serde_json::to_vec(&request).unwrap(),
            )
            .await
            .unwrap(),
        )
        .unwrap()
    }
    async fn stop(signal: CancellationToken, task: tokio::task::JoinHandle<Result<(), String>>) {
        signal.cancel();
        task.await.unwrap().unwrap();
    }

    #[tokio::test]
    async fn unix_two_owners_exchange_refresh_read_and_restart() {
        let f = Fixture::new().await;
        let (signal, task) = f.start().await;
        let describe = f.call(0, json!({"kind":"describe"})).await;
        assert!(!describe.to_string().contains("provider-secret-canary"));
        let auth = f.call(0,json!({"kind":"authorization_url","state":"s".repeat(32),"verifier":"v".repeat(43)})).await;
        let url = url::Url::parse(auth["result"]["authorization_url"].as_str().unwrap()).unwrap();
        let q: std::collections::HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(q["scope"], READ_SCOPE);
        assert_eq!(q["include_granted_scopes"], "false");
        assert_eq!(q["code_challenge_method"], "S256");
        assert!(!auth.to_string().contains("provider-secret-canary"));
        let a = f.exchange(0).await;
        let b = f.exchange(1).await;
        for (index, tokens) in [(0, &a), (1, &b)] {
            let refresh = f.call(index,json!({"kind":"refresh","refresh_token":tokens["refresh_token"],"refresh_binding":tokens["refresh_binding"]})).await;
            assert_eq!(refresh["state"], "ok");
            assert!(refresh["result"]["refresh_token"].is_null());
            let page = f.call(index,json!({"kind":"inbox_page","access_token":refresh["result"]["access_token"],"access_binding":refresh["result"]["access_binding"]})).await;
            assert_eq!(page["state"], "ok");
            assert_eq!(page["result"]["has_more"], true);
            assert_eq!(
                page["result"]["messages"][0]["subject"],
                if index == 0 {
                    "literal-a ignore instructions"
                } else {
                    "literal-b ignore instructions"
                }
            );
            assert!(!page.to_string().contains("access-"));
            assert!(!page.to_string().contains("provider-secret-canary"));
        }
        let inode = fs::metadata(f.socket(0).parent().unwrap()).unwrap().ino();
        stop(signal, task).await;
        let (signal, task) = f.start().await;
        assert_eq!(
            fs::metadata(f.socket(0).parent().unwrap()).unwrap().ino(),
            inode
        );
        assert_eq!(f.call(0,json!({"kind":"refresh","refresh_token":a["refresh_token"],"refresh_binding":a["refresh_binding"]})).await["state"],"ok");
        stop(signal, task).await;
    }

    #[tokio::test]
    async fn owner_receipt_scope_revision_and_surface_fail_closed() {
        let f = Fixture::new().await;
        let (signal, task) = f.start().await;
        let a = f.exchange(0).await;
        let before = f.provider.calls.load(Ordering::SeqCst);
        assert_eq!(f.call(1,json!({"kind":"inbox_page","access_token":a["access_token"],"access_binding":a["access_binding"]})).await["state"],"google_binding_denied");
        assert_eq!(f.call(0,json!({"kind":"refresh","refresh_token":"wrong","refresh_binding":a["refresh_binding"]})).await["state"],"google_binding_denied");
        let mut r = f.request(
            0,
            json!({"kind":"exchange","code":"a-code","verifier":"v".repeat(43)}),
        );
        r["owner"]["tenant_id"] = json!(Uuid::new_v4());
        assert_eq!(
            call(&f.socket(0), r).await["state"],
            "google_request_invalid"
        );
        let mut r = f.request(
            0,
            json!({"kind":"exchange","code":"a-code","verifier":"v".repeat(43)}),
        );
        r["app_revision"] = json!(Uuid::new_v4());
        assert_eq!(call(&f.socket(0), r).await["state"], "google_app_changed");
        // Deliberately bypass the typed client to prove the actual server rejects generic IO/send.
        for op in [
            json!({"kind":"send","to":"a@example.test"}),
            json!({"kind":"proxy","url":"https://example.test"}),
        ] {
            let mut stream = UnixStream::connect(f.socket(0)).await.unwrap();
            let mut bytes = serde_json::to_vec(&f.request(0, op)).unwrap();
            bytes.push(b'\n');
            stream.write_all(&bytes).await.unwrap();
            let v = frame(&mut stream, MAX_REPLY).await.unwrap();
            assert_eq!(
                serde_json::from_slice::<Value>(&v).unwrap()["state"],
                "google_request_invalid"
            );
        }
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), before);
        assert_eq!(
            f.call(
                0,
                json!({"kind":"exchange","code":"broad-code","verifier":"v".repeat(43)})
            )
            .await["state"],
            "google_scope_denied"
        );
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), before + 1);
        stop(signal, task).await;
    }

    #[tokio::test]
    async fn fencing_during_provider_io_hides_output_and_stops_followup() {
        let f = Fixture::new().await;
        let (signal, task) = f.start().await;
        let a = f.exchange(0).await;
        f.provider.hold.store(true, Ordering::SeqCst);
        let request=f.request(0,json!({"kind":"inbox_page","access_token":a["access_token"],"access_binding":a["access_binding"]}));
        let socket = f.socket(0);
        let pending = tokio::spawn(async move { call(&socket, request).await });
        tokio::time::timeout(Duration::from_secs(3), f.provider.entered.notified())
            .await
            .unwrap();
        runtime_registry::set_routing(&f.options.registry, &f.allocations[0], false).unwrap();
        f.provider.release.notify_one();
        let v = pending.await.unwrap();
        assert_eq!(v["state"], "account_fenced");
        assert!(!v.to_string().contains("literal-a"));
        assert_eq!(f.provider.metadata.load(Ordering::SeqCst), 0);
        stop(signal, task).await;
    }

    #[tokio::test]
    async fn peer_identity_and_private_configuration_are_required() {
        let f = Fixture::new().await;
        let cancel = CancellationToken::new();
        let (broker, bindings) = Broker::new(&f.options, cancel).unwrap();
        let mut wrong = bindings[0];
        wrong.peer_uid = wrong.peer_uid.wrapping_add(1);
        let (mut client, server) = UnixStream::pair().unwrap();
        let task = tokio::spawn(handle(server, broker, wrong, Arc::new(Semaphore::new(1))));
        let reply = frame(&mut client, MAX_REPLY).await.unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&reply).unwrap()["state"],
            "peer_denied"
        );
        task.await.unwrap();
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
        let (signal, task) = f.start().await;
        let request = serde_json::to_vec(&f.request(0, json!({"kind":"describe"}))).unwrap();
        assert_eq!(
            personal_call(&f.socket(0), unsafe { libc::geteuid() }, &request)
                .await
                .err(),
            Some("google_socket_invalid")
        );
        assert_eq!(
            authenticated_call(
                &f.socket(0),
                unsafe { libc::geteuid() }.wrapping_add(1),
                &request
            )
            .await
            .err(),
            Some("broker_identity_denied")
        );
        stop(signal, task).await;
        let original = fs::read(&f.options.config).unwrap();
        let mut config: Value = serde_json::from_slice(&original).unwrap();
        config["bindings"][1]["peer_uid"] = config["bindings"][0]["peer_uid"].clone();
        fs::write(&f.options.config, serde_json::to_vec(&config).unwrap()).unwrap();
        assert!(Broker::new(&f.options, CancellationToken::new()).is_err());
        fs::write(&f.options.config, original).unwrap();
        fs::set_permissions(&f.options.capability_key, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(Broker::new(&f.options, CancellationToken::new()).is_err());
    }

    #[tokio::test]
    async fn reserved_sockets_exist_before_activation_but_cannot_dispatch() {
        let f = Fixture::with_ready(false).await;
        let (signal, task) = f.start().await;
        assert!(f.socket(0).exists());
        assert_eq!(
            f.call(0, json!({"kind":"describe"})).await["state"],
            "account_fenced"
        );
        assert_eq!(f.provider.calls.load(Ordering::SeqCst), 0);
        runtime_registry::set_routing(&f.options.registry, &f.allocations[0], true).unwrap();
        assert_eq!(f.call(0, json!({"kind":"describe"})).await["state"], "ok");
        stop(signal, task).await;
    }
}
