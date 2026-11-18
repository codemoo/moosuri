//! Opt-in restart recovery for preconfigured, already-active personal runtimes.
//! This worker never reserves identity, initializes data/passwords, activates a
//! route, replaces a conflicting image or changes administrator-managed mounts.
pub(crate) mod onboard;
use crate::kiri_provision;
use clap::Args;
use fs2::FileExt;
use moosuri_connectors::model::CancellationToken;
use moosuri_store::runtime_registry::{self, Allocation};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use uuid::Uuid;
const LIMIT: u64 = 1 << 20;

#[derive(Args)]
pub struct Options {
    /// Explicitly opt into configured Linux member restart recovery.
    #[arg(long)]
    pub experimental: bool,
    /// Existing root-owned private JSON deployment allowlist, never credentials.
    #[arg(long)]
    pub config: PathBuf,
    /// Existing root-owned0700 dedicated lock/status directory.
    #[arg(long)]
    pub state_directory: PathBuf,
    /// Run one recovery pass and fail if a configured member is not ready.
    #[arg(long)]
    pub once: bool,
    #[arg(long, default_value_t = 5, value_parser=clap::value_parser!(u64).range(1..=300))]
    pub interval_seconds: u64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    version: u32,
    registry_directory: PathBuf,
    mounts_root: PathBuf,
    ipc_root: PathBuf,
    allocations_root: PathBuf,
    model_sockets_root: Option<PathBuf>,
    google_sockets_root: Option<PathBuf>,
    image: String,
    #[serde(default = "engine_socket")]
    engine_socket: PathBuf,
    #[serde(default = "memory")]
    memory_bytes: u64,
    #[serde(default = "cpu")]
    nano_cpus: u64,
    #[serde(default = "pids")]
    pids: u32,
    #[serde(default = "disk")]
    max_volume_bytes: u64,
    enrollment_directory: Option<PathBuf>,
    members: Vec<Member>,
}
#[derive(Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Member {
    account_id: Uuid,
    generation: i64,
    email: String,
    uid: u32,
    #[serde(default)]
    enable_text_preview: bool,
    #[serde(default)]
    enable_google_readonly: bool,
    #[serde(default)]
    enable_local_gws: bool,
}
fn engine_socket() -> PathBuf {
    "/run/kiri/docker.sock".into()
}
fn memory() -> u64 {
    512 << 20
}
fn cpu() -> u64 {
    1_000_000_000
}
fn pids() -> u32 {
    128
}
fn disk() -> u64 {
    10 << 30
}
impl Config {
    fn provision(&self, m: &Member) -> kiri_provision::Options {
        kiri_provision::Options {
            experimental: true,
            registry_directory: self.registry_directory.clone(),
            email: m.email.clone(),
            mounts_root: self.mounts_root.clone(),
            ipc_root: self.ipc_root.clone(),
            allocations_root: self.allocations_root.clone(),
            model_sockets_root: self.model_sockets_root.clone(),
            google_sockets_root: if m.enable_google_readonly || m.enable_local_gws {
                self.google_sockets_root.clone()
            } else {
                None
            },
            enable_local_gws: m.enable_local_gws,
            enable_text_preview: m.enable_text_preview,
            uid: m.uid,
            image: self.image.clone(),
            engine_socket: self.engine_socket.clone(),
            memory_bytes: self.memory_bytes,
            nano_cpus: self.nano_cpus,
            pids: self.pids,
            max_volume_bytes: self.max_volume_bytes,
            start: true,
        }
    }
    fn validate(&self) -> Result<(), String> {
        if self.version != 1 || self.members.len() > 128 {
            return Err("kiri_lifecycle_invalid_config".into());
        }
        for p in [
            &self.registry_directory,
            &self.mounts_root,
            &self.ipc_root,
            &self.allocations_root,
            &self.engine_socket,
        ]
        .into_iter()
        .chain(self.model_sockets_root.iter())
        .chain(self.google_sockets_root.iter())
        {
            plain_absolute(p)?;
        }
        let mut ids = BTreeSet::new();
        let mut uids = BTreeSet::new();
        let mut emails = BTreeSet::new();
        for m in &self.members {
            if m.account_id.is_nil()
                || m.generation <= 0
                || m.email.is_empty()
                || m.email.len() > 254
                || m.email.chars().any(char::is_control)
                || m.email.trim() != m.email
                || !ids.insert(m.account_id)
                || !uids.insert(m.uid)
                || !emails.insert(m.email.to_ascii_lowercase())
                || (m.enable_text_preview && self.model_sockets_root.is_none())
                || (m.enable_google_readonly && self.google_sockets_root.is_none())
                || (m.enable_google_readonly && m.enable_local_gws)
            {
                return Err("kiri_lifecycle_invalid_or_duplicate_member".into());
            }
            kiri_provision::validate_limits(&self.provision(m))?;
        }
        Ok(())
    }
}
#[derive(Debug, Serialize, PartialEq, Eq)]
struct MemberState {
    account_id: Uuid,
    generation: i64,
    state: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<String>,
}
impl MemberState {
    fn new(m: &Member, state: &'static str, reason: Option<String>) -> Self {
        Self {
            account_id: m.account_id,
            generation: m.generation,
            state,
            reason,
        }
    }
}
#[derive(Serialize)]
struct Snapshot<'a> {
    version: u32,
    checked_unix_seconds: u64,
    members: &'a [MemberState],
}
fn plain_absolute(p: &Path) -> Result<(), String> {
    if !p.is_absolute()
        || p.to_str().is_none()
        || p.to_string_lossy().contains([':', '\r', '\n', '\0'])
        || p.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
    {
        return Err("kiri_lifecycle_plain_absolute_path_required".into());
    }
    Ok(())
}
fn trusted_path(p: &Path, directory: bool) -> Result<fs::Metadata, String> {
    plain_absolute(p)?;
    if fs::canonicalize(p).ok().as_deref() != Some(p) {
        return Err("kiri_lifecycle_noncanonical_path".into());
    }
    let m = fs::symlink_metadata(p).map_err(|_| "kiri_lifecycle_path_unavailable")?;
    if m.uid() != 0
        || m.mode() & 0o077 != 0
        || (directory && !m.is_dir())
        || (!directory && (!m.is_file() || m.nlink() != 1 || m.len() > LIMIT))
    {
        return Err("kiri_lifecycle_private_root_owned_path_required".into());
    }
    for parent in p.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| "kiri_lifecycle_ancestor_unavailable")?;
        if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("kiri_lifecycle_untrusted_ancestor".into());
        }
    }
    Ok(m)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    trusted_path(path, false)?;
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| "kiri_lifecycle_config_unavailable")?;
    let m = file
        .metadata()
        .map_err(|_| "kiri_lifecycle_config_unavailable")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o077 != 0 || m.nlink() != 1 || m.len() > LIMIT {
        return Err("kiri_lifecycle_config_replaced".into());
    }
    let mut bytes = Vec::new();
    file.take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "kiri_lifecycle_config_unavailable")?;
    if bytes.len() as u64 > LIMIT {
        return Err("kiri_lifecycle_config_too_large".into());
    }
    serde_json::from_slice(&bytes).map_err(|_| "kiri_lifecycle_invalid_config".into())
}
fn load(path: &Path, private_directory: &Path) -> Result<Config, String> {
    let mut config: Config = read_json(path)?;
    onboard::include(&mut config, path, private_directory)?;
    Ok(config)
}
fn lock_state(root: &Path) -> Result<File, String> {
    trusted_path(root, true)?;
    for entry in fs::read_dir(root).map_err(|_| "kiri_lifecycle_state_unavailable")? {
        let entry = entry.map_err(|_| "kiri_lifecycle_state_unavailable")?;
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("kiri_lifecycle_unrelated_state_directory")?;
        if name != "status.json" && !name.starts_with(".status-") {
            return Err("kiri_lifecycle_unrelated_state_directory".into());
        }
        trusted_path(&entry.path(), false)?;
    }
    let directory = File::open(root).map_err(|_| "kiri_lifecycle_state_unavailable")?;
    directory
        .try_lock_exclusive()
        .map_err(|_| "kiri_lifecycle_already_running")?;
    Ok(directory)
}
fn persist(root: &Path, directory: &File, states: &[MemberState]) -> Result<(), String> {
    let snapshot = Snapshot {
        version: 1,
        checked_unix_seconds: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "kiri_lifecycle_clock_invalid")?
            .as_secs(),
        members: states,
    };
    let bytes = serde_json::to_vec(&snapshot).map_err(|_| "kiri_lifecycle_state_encode_failed")?;
    let mut file = tempfile::Builder::new()
        .prefix(".status-")
        .tempfile_in(root)
        .map_err(|_| "kiri_lifecycle_state_write_failed")?;
    file.write_all(&bytes)
        .and_then(|_| file.as_file().sync_all())
        .map_err(|_| "kiri_lifecycle_state_write_failed")?;
    file.persist(root.join("status.json"))
        .map_err(|_| "kiri_lifecycle_state_publish_failed")?;
    directory
        .sync_all()
        .map_err(|_| "kiri_lifecycle_state_publish_failed".to_owned())
}
fn matches(m: &Member, a: &Allocation) -> bool {
    a.account_id == m.account_id.to_string() && a.generation == m.generation
}
fn admitted(config: &Config, m: &Member) -> Result<Allocation, &'static str> {
    let a = runtime_registry::lookup_id(&config.registry_directory, m.account_id)
        .map_err(|_| "allocation_not_ready_or_registry_unavailable")?;
    let email = runtime_registry::lookup(&config.registry_directory, m.email.clone())
        .map_err(|_| "configured_identity_changed")?;
    if !matches(m, &a) || a != email {
        return Err("configured_identity_changed");
    }
    Ok(a)
}
pub(crate) async fn health(socket: &Path, a: &Allocation, uid: u32) -> Result<(), &'static str> {
    // Authenticate the connected process before sending even the fixed GET. An
    // account-writable socket name must never redirect this privileged worker.
    if fs::canonicalize(socket).ok().as_deref() != Some(socket) {
        return Err("member_not_ready");
    }
    let meta = fs::symlink_metadata(socket).map_err(|_| "member_not_ready")?;
    if !meta.file_type().is_socket()
        || meta.uid() != uid
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
    {
        return Err("member_identity_mismatch");
    }
    tokio::time::timeout(Duration::from_secs(2),async {
        let mut stream=tokio::net::UnixStream::connect(socket).await.map_err(|_|"member_not_ready")?;
        if stream.peer_cred().map_err(|_|"member_identity_mismatch")?.uid()!=uid {return Err("member_identity_mismatch");}
        stream.write_all(b"GET /health/ready HTTP/1.1\r\nHost: personal.internal\r\nConnection: close\r\n\r\n").await.map_err(|_|"member_not_ready")?;
        let mut head=Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut byte=[0];stream.read_exact(&mut byte).await.map_err(|_|"member_not_ready")?;head.push(byte[0]);
            if head.len()>8192{return Err("member_invalid_health_response");}
        }
        let head=std::str::from_utf8(&head).map_err(|_|"member_invalid_health_response")?;
        let mut lines=head.lines();
        let first=lines.next().ok_or("member_invalid_health_response")?;
        let mut words=first.split_whitespace();
        if words.next()!=Some("HTTP/1.1") || words.next()!=Some("204") {return Err("member_not_ready");}
        let mut headers=BTreeMap::new();
        for line in lines.filter(|l|!l.is_empty()) {
            let (key,value)=line.split_once(':').ok_or("member_invalid_health_response")?;
            if headers.insert(key.to_ascii_lowercase(),value.trim()).is_some(){return Err("member_invalid_health_response");}
        }
        for (name,value) in [("x-moosuri-account",a.account_id.as_str()),("x-moosuri-tenant",a.tenant_id.as_str()),("x-moosuri-volume",a.volume_id.as_str())] {
            if headers.get(name).copied()!=Some(value){return Err("member_identity_mismatch");}
        }
        if headers.get("x-moosuri-generation").and_then(|s|s.parse::<i64>().ok())!=Some(a.generation){return Err("member_identity_mismatch");}
        Ok(())
    }).await.map_err(|_|"member_not_ready")?
}
async fn member(config: &Config, m: &Member) -> MemberState {
    let a = match admitted(config, m) {
        Ok(a) => a,
        Err(reason) => return MemberState::new(m, "not_active", Some(reason.into())),
    };
    if let Err(reason) = kiri_provision::reconcile_member(&config.provision(m)).await {
        return MemberState::new(m, "waiting", Some(reason));
    }
    // A fence never becomes a route activation, including after a slow engine call.
    if admitted(config, m).ok().as_ref() != Some(&a) {
        return MemberState::new(
            m,
            "not_active",
            Some("allocation_changed_during_reconcile".into()),
        );
    }
    let socket = config.ipc_root.join(&a.account_id).join("personal.sock");
    let deadline = tokio::time::Instant::now() + Duration::from_secs(3);
    let result = loop {
        match health(&socket, &a, m.uid).await {
            Ok(()) => break Ok(()),
            Err("member_identity_mismatch") => break Err("member_identity_mismatch"),
            Err(reason) => {
                if tokio::time::Instant::now() >= deadline {
                    break Err(reason);
                }
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };
    if admitted(config, m).ok().as_ref() != Some(&a) {
        return MemberState::new(
            m,
            "not_active",
            Some("allocation_changed_during_health".into()),
        );
    }
    match result {
        Ok(()) => MemberState::new(m, "ready", None),
        Err(reason) => MemberState::new(m, "waiting", Some(reason.into())),
    }
}
async fn cycle(config: &Config, stop: &CancellationToken) -> Vec<MemberState> {
    let mut states = Vec::with_capacity(config.members.len());
    for m in &config.members {
        if stop.is_cancelled() {
            break;
        }
        states.push(member(config, m).await);
    }
    states
}
pub async fn run(options: &Options) -> Result<(), String> {
    if !cfg!(target_os = "linux") || !options.experimental || unsafe { libc::geteuid() } != 0 {
        return Err("kiri_lifecycle_explicit_root_linux_only".into());
    }
    let directory = lock_state(&options.state_directory)?;
    let mut previous = None;
    let mut termination = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "kiri_lifecycle_signal_unavailable")?;
    let stop = CancellationToken::new();
    let signal = stop.clone();
    tokio::spawn(async move {
        tokio::select! {_=tokio::signal::ctrl_c()=>{},_=termination.recv()=>{}}
        signal.cancel();
    });
    loop {
        let config = load(&options.config, &options.state_directory)?;
        let states = cycle(&config, &stop).await;
        if stop.is_cancelled() {
            return Ok(());
        }
        persist(&options.state_directory, &directory, &states)?;
        if previous.as_ref() != Some(&states) {
            println!(
                "{}",
                serde_json::json!({"event":"member_lifecycle_state","members":states})
            );
        }
        if options.once {
            return if states.iter().all(|s| s.state == "ready") {
                Ok(())
            } else {
                Err("kiri_lifecycle_members_not_ready".into())
            };
        }
        previous = Some(states);
        tokio::select! {
            _=tokio::time::sleep(Duration::from_secs(options.interval_seconds))=>{},
            _=stop.cancelled()=>return Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::{fs::PermissionsExt, net::UnixListener};
    fn configuration(registry: &Path) -> Config {
        serde_json::from_value(json!({"version":1,"registry_directory":registry,"mounts_root":"/srv/members","ipc_root":"/srv/ipc","allocations_root":"/srv/assignments","image":format!("sha256:{}","a".repeat(64)),"members":[]})).unwrap()
    }
    #[test]
    fn unsafe_deployment_values_and_duplicate_owners_are_rejected_as_one_configuration() {
        let mut c = configuration(Path::new("/srv/registry"));
        let id = Uuid::new_v4();
        c.members = vec![
            Member {
                account_id: id,
                generation: 1,
                email: "a@example.invalid".into(),
                uid: 10001,
                enable_text_preview: false,
                enable_google_readonly: false,
                enable_local_gws: false,
            },
            Member {
                account_id: Uuid::new_v4(),
                generation: 1,
                email: "b@example.invalid".into(),
                uid: 10001,
                enable_text_preview: false,
                enable_google_readonly: false,
                enable_local_gws: false,
            },
        ];
        assert!(c.validate().is_err());
        c.members[1].uid = 10002;
        assert!(c.validate().is_ok());
        c.members[1].generation = 0;
        assert!(c.validate().is_err());
        c.members[1].generation = 1;
        c.members[1].account_id = id;
        assert!(c.validate().is_err());
        c.members[1].account_id = Uuid::new_v4();
        c.members[0].enable_text_preview = true;
        assert!(c.validate().is_err());
        assert!(
            serde_json::from_value::<Config>(json!({"version":1,"password":"never accepted"}))
                .is_err()
        );
    }
    #[test]
    fn local_gws_is_member_scoped_and_requires_an_egress_root() {
        let mut c = configuration(Path::new("/srv/registry"));
        c.members = serde_json::from_value(json!([
            {"account_id":Uuid::new_v4(),"generation":1,"email":"a@example.invalid","uid":10001,"enable_local_gws":true},
            {"account_id":Uuid::new_v4(),"generation":1,"email":"b@example.invalid","uid":10002}
        ]))
        .unwrap();
        assert!(c.validate().is_err());
        let relay = PathBuf::from("/srv/google-relay");
        c.google_sockets_root = Some(relay.clone());
        assert!(c.validate().is_ok());
        let enabled = c.provision(&c.members[0]);
        assert!(enabled.enable_local_gws);
        assert_eq!(enabled.google_sockets_root, Some(relay));
        let disabled = c.provision(&c.members[1]);
        assert!(!disabled.enable_local_gws);
        assert_eq!(disabled.google_sockets_root, None);
        c.members[0].enable_google_readonly = true;
        assert!(c.validate().is_err());
    }
    #[tokio::test]
    async fn reserved_fenced_or_stale_generation_cannot_be_autoactivated() {
        let temp = tempfile::tempdir().unwrap();
        let registry = temp.path().canonicalize().unwrap();
        fs::set_permissions(&registry, fs::Permissions::from_mode(0o700)).unwrap();
        let a = runtime_registry::reserve(&registry, Uuid::new_v4(), "a@example.invalid".into())
            .unwrap();
        let mut c = configuration(&registry);
        c.members.push(Member {
            account_id: Uuid::parse_str(&a.account_id).unwrap(),
            generation: 1,
            email: "a@example.invalid".into(),
            uid: 10001,
            enable_text_preview: false,
            enable_google_readonly: false,
            enable_local_gws: false,
        });
        let states = cycle(&c, &CancellationToken::new()).await;
        assert_eq!(states[0].state, "not_active");
        assert!(runtime_registry::lookup_id(&registry, c.members[0].account_id).is_err());
        runtime_registry::set_routing(&registry, &a, true).unwrap();
        c.members[0].generation = 2;
        let states = cycle(&c, &CancellationToken::new()).await;
        assert_eq!(
            states[0].reason.as_deref(),
            Some("configured_identity_changed")
        );
        c.members[0].generation = 1;
        runtime_registry::set_routing(&registry, &a, false).unwrap();
        let states = cycle(&c, &CancellationToken::new()).await;
        assert_eq!(states[0].state, "not_active");
        assert!(runtime_registry::lookup_id(&registry, c.members[0].account_id).is_err());
    }
    fn fake_health(
        a: &Allocation,
        wrong: bool,
    ) -> (tempfile::TempDir, PathBuf, std::thread::JoinHandle<()>) {
        let root = tempfile::tempdir().unwrap();
        let socket = root.path().join("member.sock");
        let listener = UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let account = if wrong {
            Uuid::new_v4().to_string()
        } else {
            a.account_id.clone()
        };
        let tenant = a.tenant_id.clone();
        let volume = a.volume_id.clone();
        let generation = a.generation;
        let thread = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            s.set_read_timeout(Some(Duration::from_secs(2))).unwrap();
            let mut buf = [0; 4096];
            if s.read(&mut buf).unwrap() == 0 {
                return;
            }
            write!(s,"HTTP/1.1 204 No Content\r\nx-moosuri-account: {account}\r\nx-moosuri-tenant: {tenant}\r\nx-moosuri-volume: {volume}\r\nx-moosuri-generation: {generation}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").unwrap();
        });
        (root, socket, thread)
    }
    #[tokio::test]
    async fn a_ready_http_status_from_another_owner_is_not_readiness() {
        let a = Allocation {
            account_id: Uuid::new_v4().to_string(),
            tenant_id: Uuid::new_v4().to_string(),
            volume_id: Uuid::new_v4().to_string(),
            generation: 1,
        };
        for wrong in [true, false] {
            let (_root, path, thread) = fake_health(&a, wrong);
            let uid = unsafe { libc::geteuid() };
            assert_eq!(health(&path, &a, uid).await.is_ok(), !wrong);
            thread.join().unwrap();
        }
    }
}
