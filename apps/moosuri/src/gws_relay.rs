//! Member-scoped HTTPS CONNECT for isolated Kiri network namespaces.
//! The host sees a destination selector and forwards bytes without terminating TLS.

use crate::runtime_google::{Binding, epoch, listener, private_bytes, private_dir, same_owner};
use clap::Args;
use fs2::FileExt;
use moosuri_store::runtime_registry;
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File},
    net::{IpAddr, Ipv4Addr, SocketAddr},
    os::unix::fs::{FileTypeExt, MetadataExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWriteExt, BufReader, copy_bidirectional},
    net::{TcpListener, TcpStream, UnixListener, UnixStream, lookup_host},
    sync::{OwnedSemaphorePermit, Semaphore},
    task::{JoinHandle, JoinSet},
    time::{sleep, timeout},
};
use uuid::Uuid;
use zeroize::Zeroizing;

const REGISTRATION: [u8; 4] = *b"GR01";
const CLIENT_LIMIT: usize = 16384;
const CALLBACK_LIMIT: usize = 2048;
const REQUEST_LIMIT: u64 = 4096;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const TUNNEL_TIMEOUT: Duration = Duration::from_secs(120);
const FENCE_INTERVAL: Duration = Duration::from_secs(5);
const MEMBER_CONNECTIONS: usize = 12;
const TOTAL_CONNECTIONS: usize = 64;
const ESTABLISHED: &[u8] = b"HTTP/1.1 200 Connection Established\r\n\r\n";
const FORBIDDEN: &[u8] =
    b"HTTP/1.1 403 Forbidden\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";
const UNAVAILABLE: &[u8] =
    b"HTTP/1.1 502 Bad Gateway\r\nConnection: close\r\nContent-Length: 0\r\n\r\n";

#[derive(Args)]
pub struct Options {
    /// Existing private installation registry.
    #[arg(long)]
    pub registry: PathBuf,
    /// Existing dedicated host IPC root. Only one UUID child is mounted into each member.
    #[arg(long)]
    pub socket_dir: PathBuf,
    /// Private JSON file containing the registered member bindings.
    #[arg(
        long,
        required_unless_present = "lifecycle_config",
        conflicts_with = "lifecycle_config"
    )]
    pub config: Option<PathBuf>,
    /// Discover existing and newly enrolled members from the trusted Kiri profile.
    #[arg(long, required_unless_present = "config")]
    pub lifecycle_config: Option<PathBuf>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    bindings: Vec<Binding>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum Destination {
    Token = 0,
    Gmail = 1,
    OpenId = 2,
    Drive = 3,
    Docs = 4,
    Slides = 5,
    Sheets = 6,
}

impl Destination {
    const ALLOWED: [Self; 7] = [
        Self::Token,
        Self::Gmail,
        Self::OpenId,
        Self::Drive,
        Self::Docs,
        Self::Slides,
        Self::Sheets,
    ];

    fn authority(self) -> &'static str {
        match self {
            Self::Token => "oauth2.googleapis.com:443",
            Self::Gmail => "gmail.googleapis.com:443",
            Self::OpenId => "openidconnect.googleapis.com:443",
            // Drive v3's pinned discovery document uses this canonical HTTPS origin.
            Self::Drive => "www.googleapis.com:443",
            Self::Docs => "docs.googleapis.com:443",
            Self::Slides => "slides.googleapis.com:443",
            Self::Sheets => "sheets.googleapis.com:443",
        }
    }

    // The member transmits no HTTP headers over the Unix socket.
    fn frame(self) -> [u8; 4] {
        [b'G', b'W', b'1', self as u8]
    }

    fn from_frame(frame: [u8; 4]) -> Option<Self> {
        Self::ALLOWED
            .into_iter()
            .find(|entry| entry.frame() == frame)
    }
}

fn trusted_ancestors(path: &Path) -> Result<(), String> {
    for parent in path.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| "gws_relay_path_unavailable")?;
        if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("gws_relay_untrusted_ancestor".into());
        }
    }
    Ok(())
}

fn load_bindings(options: &Options, path: &Path) -> Result<Vec<Binding>, String> {
    let config: Config = serde_json::from_slice(&private_bytes(path, 64 * 1024)?)
        .map_err(|_| "gws_relay_config_invalid")?;
    if config.bindings.is_empty() || config.bindings.len() > 128 {
        return Err("gws_relay_config_invalid".into());
    }
    let mut accounts = BTreeSet::new();
    let mut uids = BTreeSet::new();
    for b in &config.bindings {
        if b.account_id.is_nil()
            || b.tenant_id.is_nil()
            || b.volume_id.is_nil()
            || b.generation <= 0
            || !(10000..=2_000_000_000).contains(&b.peer_uid)
            || !accounts.insert(b.account_id)
            || !uids.insert(b.peer_uid)
        {
            return Err("gws_relay_binding_invalid".into());
        }
        // Provisioning can mount the inert socket before the member is ready.
        let allocation = runtime_registry::lookup_allocated_id(&options.registry, b.account_id)
            .map_err(|_| "gws_relay_account_fenced")?;
        if !same_owner(&allocation, *b) {
            return Err("gws_relay_account_fenced".into());
        }
    }
    Ok(config.bindings)
}

pub async fn run(options: &Options) -> Result<(), String> {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err("gws_relay_linux_root_required".into());
    }
    private_dir(&options.registry)?;
    private_dir(&options.socket_dir)?;
    if options.registry.starts_with(&options.socket_dir)
        || options.socket_dir.starts_with(&options.registry)
    {
        return Err("gws_relay_dedicated_socket_root_required".into());
    }
    let config = options
        .config
        .as_ref()
        .or(options.lifecycle_config.as_ref())
        .ok_or("gws_relay_config_required")?;
    for path in [&options.registry, &options.socket_dir, config] {
        trusted_ancestors(path)?;
    }

    let lock = File::open(&options.socket_dir).map_err(|_| "gws_relay_socket_unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "gws_relay_already_running")?;
    let registry = Arc::new(options.registry.clone());
    let slots = Arc::new(Semaphore::new(TOTAL_CONNECTIONS));
    let mut members = BTreeMap::new();
    if let Some(path) = options.config.as_ref() {
        let bindings = load_bindings(options, path)?;
        reconcile_listeners(
            &options.socket_dir,
            &registry,
            &slots,
            &mut members,
            bindings,
            None,
        )
        .await?;
    }
    let signal = crate::shutdown_signal();
    tokio::pin!(signal);
    let mut poll = tokio::time::interval(Duration::from_secs(3));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = &mut signal => return Ok(()),
            _ = poll.tick() => {
                if let Some(config) = options.lifecycle_config.clone() {
                    let root = options.socket_dir.clone();
                    let registry_path = registry.clone();
                    let (bindings, callback) = tokio::task::spawn_blocking(move ||
                        crate::kiri_lifecycle::gws_bindings(&registry_path, &config, &root)
                    ).await.map_err(|_| "gws_relay_enrollment_unavailable")??;
                    reconcile_listeners(&options.socket_dir, &registry, &slots, &mut members, bindings, Some(callback)).await?;
                } else if members.values().any(|m| m.task.is_finished()) {
                    return Err("gws_relay_listener_stopped".into());
                }
            }
        }
    }
}

struct MemberListener {
    binding: Binding,
    callback: Option<String>,
    task: JoinHandle<()>,
    _guard: crate::runtime_google::SocketGuard,
}

impl Drop for MemberListener {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn reconcile_listeners(
    root: &Path,
    registry: &Arc<PathBuf>,
    slots: &Arc<Semaphore>,
    members: &mut BTreeMap<Uuid, MemberListener>,
    desired: Vec<Binding>,
    callback: Option<String>,
) -> Result<(), String> {
    members.retain(|_, m| {
        desired.contains(&m.binding) && callback == m.callback && !m.task.is_finished()
    });
    for binding in desired {
        if members.contains_key(&binding.account_id) {
            continue;
        }
        let bound = timeout(REQUEST_TIMEOUT, listener(root, binding))
            .await
            .map_err(|_| "gws_relay_socket_unavailable".to_owned())
            .and_then(|result| result);
        let (socket, guard) = match bound {
            Ok(bound) => bound,
            // One unavailable mount must not interrupt other members. The
            // lifecycle worker reports that member waiting and both retry.
            Err(_) if callback.is_some() => continue,
            Err(error) => return Err(error),
        };
        let task = tokio::spawn(serve_binding(
            socket,
            registry.clone(),
            binding,
            slots.clone(),
            callback.clone().map(Arc::new),
        ));
        members.insert(
            binding.account_id,
            MemberListener {
                binding,
                callback: callback.clone(),
                task,
                _guard: guard,
            },
        );
    }
    Ok(())
}

async fn serve_binding(
    listener: UnixListener,
    registry: Arc<PathBuf>,
    binding: Binding,
    slots: Arc<Semaphore>,
    callback: Option<Arc<String>>,
) {
    let mut clients = JoinSet::new();
    loop {
        tokio::select! {
            Some(_) = clients.join_next(), if !clients.is_empty() => {}
            accepted = listener.accept(), if clients.len() < MEMBER_CONNECTIONS => {
                let Ok((stream, _)) = accepted else { break };
                let Ok(slot) = slots.clone().try_acquire_owned() else { continue };
                let admission = Admission { registry: registry.clone(), binding, slot: Arc::new(slot), callback: callback.clone() };
                clients.spawn(relay_connection(stream, admission, dial_google));
            }
        }
    }
}

#[derive(Clone)]
struct Admission {
    registry: Arc<PathBuf>,
    binding: Binding,
    slot: Arc<OwnedSemaphorePermit>,
    callback: Option<Arc<String>>,
}

impl Admission {
    async fn epoch(&self) -> Option<i64> {
        let Self {
            registry,
            binding,
            slot,
            ..
        } = self.clone();
        // A registry lock may outlive the async timeout. Keep a global permit
        // until the blocking check returns, including after task cancellation.
        tokio::task::spawn_blocking(move || {
            let _slot = slot;
            epoch(&registry, binding).ok()
        })
        .await
        .ok()
        .flatten()
    }

    async fn wait_until_fenced(&self, initial: i64) {
        loop {
            sleep(FENCE_INTERVAL).await;
            if self.epoch().await != Some(initial) {
                return;
            }
        }
    }
}

async fn relay_connection<F>(
    mut stream: UnixStream,
    admission: Admission,
    dial: impl FnOnce(Destination) -> F,
) where
    F: Future<Output = Option<TcpStream>>,
{
    if stream.peer_cred().map(|cred| cred.uid()).ok() != Some(admission.binding.peer_uid) {
        return;
    }
    let ready = timeout(CONNECT_TIMEOUT, async {
        let mut frame = [0; 4];
        stream.read_exact(&mut frame).await.ok()?;
        if frame == REGISTRATION {
            send_registration(&mut stream, &admission).await?;
            return None;
        }
        let destination = Destination::from_frame(frame)?;
        let initial = admission.epoch().await?;
        let upstream = dial(destination).await?;
        if admission.epoch().await != Some(initial) {
            return None;
        }
        stream.write_u8(1).await.ok()?;
        Some((upstream, initial))
    })
    .await;
    if let Ok(Some((mut upstream, initial))) = ready {
        tokio::select! {
            _ = timeout(TUNNEL_TIMEOUT, copy_bidirectional(&mut stream, &mut upstream)) => {}
            _ = admission.wait_until_fenced(initial) => {}
        }
    }
}

// This request has no target/path field. Its listener fixes the member and
// installation. The shared app is the only secret the host can return.
async fn send_registration(stream: &mut UnixStream, admission: &Admission) -> Option<()> {
    let callback = admission.callback.as_deref()?;
    let epoch = admission.epoch().await?;
    let path = admission.registry.join("google-web-client.json");
    let destination = callback.clone();
    let slot = admission.slot.clone();
    let client = tokio::task::spawn_blocking(move || {
        let _slot = slot;
        let client =
            crate::runtime_google::private_bytes_with_mode(&path, CLIENT_LIMIT, Some(0o600))
                .ok()?;
        moosuri_connectors::gmail::OAuthClient::from_web_client_json(
            std::str::from_utf8(&client).ok()?,
            &destination,
        )
        .ok()?;
        Some(client)
    })
    .await
    .ok()??;
    if admission.epoch().await != Some(epoch) {
        return None;
    }
    stream
        .write_u16(callback.len().try_into().ok()?)
        .await
        .ok()?;
    stream.write_u16(client.len().try_into().ok()?).await.ok()?;
    stream.write_all(callback.as_bytes()).await.ok()?;
    stream.write_all(&client).await.ok()?;
    Some(())
}

/// Read a bounded app snapshot exclusively from the authenticated host relay.
/// Member-local configuration verifies the JSON and callback again before applying.
pub(crate) async fn registration(
    socket: &Path,
    broker_uid: u32,
) -> Option<(String, Zeroizing<Vec<u8>>)> {
    timeout(REQUEST_TIMEOUT, async {
        let mut stream = broker_stream(socket, broker_uid).await?;
        stream.write_all(&REGISTRATION).await.ok()?;
        let callback_len = stream.read_u16().await.ok()? as usize;
        let client_len = stream.read_u16().await.ok()? as usize;
        if !(1..=CALLBACK_LIMIT).contains(&callback_len)
            || !(1..=CLIENT_LIMIT).contains(&client_len)
        {
            return None;
        }
        let mut callback = vec![0; callback_len];
        let mut client = Zeroizing::new(vec![0; client_len]);
        stream.read_exact(&mut callback).await.ok()?;
        stream.read_exact(&mut client).await.ok()?;
        Some((String::from_utf8(callback).ok()?, client))
    })
    .await
    .ok()
    .flatten()
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => !matches!(
            ip.octets(),
            [0 | 10 | 127 | 224..=255, ..]
                | [100, 64..=127, ..]
                | [169, 254, ..]
                | [172, 16..=31, ..]
                | [192, 0, 0 | 2, _]
                | [192, 88, 99, _]
                | [192, 168, ..]
                | [198, 18 | 19, ..]
                | [198, 51, 100, _]
                | [203, 0, 113, _]
        ),
        IpAddr::V6(ip) => {
            matches!(ip.segments()[0], 0x2000..=0x3fff)
                && !matches!(
                    ip.segments(),
                    [0x2001, 0..=0x1ff | 0xdb8, ..] | [0x2002 | 0x3fff, ..]
                )
        }
    }
}

async fn dial_google(destination: Destination) -> Option<TcpStream> {
    let addresses: Vec<_> = lookup_host(destination.authority())
        .await
        .ok()?
        .filter(|address| is_public(address.ip()))
        .take(16)
        .collect();
    // Connect to the vetted numeric answers without resolving the name again.
    TcpStream::connect(&addresses[..]).await.ok()
}

/// Owns the ephemeral loopback listener and its active tunnels.
pub struct MemberProxy {
    address: SocketAddr,
    task: JoinHandle<()>,
}

impl MemberProxy {
    pub fn address(&self) -> SocketAddr {
        self.address
    }
}

impl Drop for MemberProxy {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub async fn start_member(socket: PathBuf, broker_uid: u32) -> Result<MemberProxy, &'static str> {
    if !socket.is_absolute() {
        return Err("gws_relay_socket_invalid");
    }
    let listener = TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| "gws_relay_member_listener_unavailable")?;
    let address = listener
        .local_addr()
        .map_err(|_| "gws_relay_member_listener_unavailable")?;
    let task = tokio::spawn(async move {
        let socket = Arc::new(socket);
        let mut clients = JoinSet::new();
        loop {
            tokio::select! {
                Some(_) = clients.join_next(), if !clients.is_empty() => {}
                accepted = listener.accept(), if clients.len() < MEMBER_CONNECTIONS => {
                    let Ok((stream, _)) = accepted else { break };
                    clients.spawn(member_connection(stream, socket.clone(), broker_uid));
                }
            }
        }
    });
    Ok(MemberProxy { address, task })
}

async fn read_connect<R: AsyncRead + Unpin>(reader: &mut BufReader<R>) -> Option<Destination> {
    let mut reader = reader.take(REQUEST_LIMIT);
    let mut bytes = Vec::new();
    loop {
        let start = bytes.len();
        reader.read_until(b'\n', &mut bytes).await.ok()?;
        let line = &bytes[start..];
        if line == b"\r\n" {
            break;
        }
        if !line.ends_with(b"\r\n") {
            return None;
        }
    }

    let mut lines = std::str::from_utf8(&bytes).ok()?.split_terminator("\r\n");
    let mut request = lines.next()?.split(' ');
    if request.next()? != "CONNECT" {
        return None;
    }
    let authority = request.next()?;
    let destination = Destination::ALLOWED
        .into_iter()
        .find(|entry| entry.authority() == authority)?;
    if !matches!(request.next()?, "HTTP/1.1" | "HTTP/1.0") || request.next().is_some() {
        return None;
    }

    let mut host = None;
    for line in lines.take_while(|line| !line.is_empty()) {
        let (name, value) = line.split_once(':')?;
        // A CONNECT request has no body or proxy credentials. Do not
        // interpret folded lines or pass through any client headers.
        if name.is_empty()
            || !name.bytes().all(|n| n.is_ascii_alphanumeric() || n == b'-')
            || !value.bytes().all(|b| matches!(b, b'\t' | b' '..=b'~'))
            || [
                "Content-Length",
                "Transfer-Encoding",
                "Proxy-Authorization",
                "Authorization",
            ]
            .iter()
            .any(|header| header.eq_ignore_ascii_case(name))
        {
            return None;
        }
        if name.eq_ignore_ascii_case("Host") && host.replace(value.trim()).is_some() {
            return None;
        }
    }
    if host.is_some_and(|host| host != authority) {
        return None;
    }
    Some(destination)
}

async fn member_connection(stream: TcpStream, socket: Arc<PathBuf>, broker_uid: u32) {
    let mut stream = BufReader::new(stream);
    let destination = match timeout(REQUEST_TIMEOUT, read_connect(&mut stream)).await {
        Ok(Some(destination)) => destination,
        Ok(None) => {
            let _ = timeout(REQUEST_TIMEOUT, stream.write_all(FORBIDDEN)).await;
            return;
        }
        Err(_) => return,
    };
    let broker = timeout(
        CONNECT_TIMEOUT,
        connect_relay(&socket, broker_uid, destination),
    )
    .await;
    let Ok(Some(mut broker)) = broker else {
        let _ = timeout(REQUEST_TIMEOUT, stream.write_all(UNAVAILABLE)).await;
        return;
    };
    let _ = timeout(TUNNEL_TIMEOUT, async {
        stream.write_all(ESTABLISHED).await?;
        copy_bidirectional(&mut stream, &mut broker).await
    })
    .await;
}

async fn connect_relay(
    socket: &Path,
    broker_uid: u32,
    destination: Destination,
) -> Option<UnixStream> {
    let mut stream = broker_stream(socket, broker_uid).await?;
    stream.write_all(&destination.frame()).await.ok()?;
    (stream.read_u8().await.ok()? == 1).then_some(stream)
}

async fn broker_stream(socket: &Path, broker_uid: u32) -> Option<UnixStream> {
    let m = fs::symlink_metadata(socket).ok()?;
    if !m.file_type().is_socket()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != 0o600
        || m.nlink() != 1
    {
        return None;
    }
    let stream = UnixStream::connect(socket).await.ok()?;
    if stream.peer_cred().map(|cred| cred.uid()).ok() != Some(broker_uid) {
        return None;
    }
    Some(stream)
}

#[cfg(test)]
mod tests;
