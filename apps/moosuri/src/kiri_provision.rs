//! Linux Kiri 0.2.1 provisioning preview. Experimental operator command; it does not enable any host service.
//! Identity is authoritative in runtime_registry; these allocations only reserve host
//! UID/path ownership. No image pulling, shell commands, volume initialization,
//! mount creation, container replacement, or production installation occurs here.
//! RestartPolicy:no is intentional: Kiri restarts even never-started containers
//! with unless-stopped after daemon recovery; a supervisor must reconcile/start.
use clap::Args;
use fs2::FileExt;
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
        net::UnixStream,
    },
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;

const ENTRYPOINT: &str = "/usr/local/bin/moosuri";
const API: &str = "/v1.47";
const BODY_LIMIT: usize = 1 << 20;
const LABEL: &str = "org.moosuri.runtime";
const PROFILE: &str = "kiri-0.2.1-none-v1";

#[derive(Args, Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Options {
    /// Explicitly opt in to the experimental profile; adoption is not established.
    #[arg(long)]
    pub experimental: bool,
    /// Existing private installation registry directory; identity is looked up here.
    #[arg(long)]
    pub registry_directory: PathBuf,
    #[arg(long)]
    pub email: String,
    /// Existing root-owned directory containing administrator-mounted <volume-id> filesystems.
    #[arg(long)]
    pub mounts_root: PathBuf,
    /// Existing root-owned private directory; each <account-id> is a private bind of the account filesystem IPC directory.
    #[arg(long)]
    pub ipc_root: PathBuf,
    /// Existing root-owned mode 0700 host UID reservation directory.
    #[arg(long)]
    pub allocations_root: PathBuf,
    /// Optional root-owned private broker parent; only the registry account child is mounted.
    #[arg(long)]
    pub model_sockets_root: Option<PathBuf>,
    /// Mount this owner's socket directory from the dedicated Google broker or relay root.
    #[arg(long)]
    pub google_sockets_root: Option<PathBuf>,
    /// Root-owned directory of member-specific browser relay sockets.
    #[arg(long)]
    pub browser_sockets_root: Option<PathBuf>,
    /// Use GWS inside the member runtime and the mounted socket for Google egress.
    #[arg(long, requires = "google_sockets_root")]
    pub enable_local_gws: bool,
    /// Enable the image-pinned member text preview on its read-only broker mount.
    #[arg(long, requires = "model_sockets_root")]
    pub enable_text_preview: bool,
    /// Administrator-assigned dedicated non-root host UID/GID, at least 10000.
    #[arg(long)]
    pub uid: u32,
    /// Already-local, exact sha256 image config ID. Tags and pulling are unsupported.
    #[arg(long)]
    pub image: String,
    #[arg(long, default_value = "/run/kiri/docker.sock")]
    pub engine_socket: PathBuf,
    #[arg(long, default_value_t = 536870912)]
    pub memory_bytes: u64,
    #[arg(long, default_value_t = 1000000000)]
    pub nano_cpus: u64,
    #[arg(long, default_value_t = 128)]
    pub pids: u32,
    /// Maximum total bytes of the dedicated backing filesystem, not free space.
    #[arg(long, default_value_t = 10737418240)]
    pub max_volume_bytes: u64,
    /// Start only after creation/reconciliation and an exact inspected-profile match.
    #[arg(long)]
    pub start: bool,
}

#[derive(Clone)]
struct Expected {
    name: String,
    image: String,
    account: String,
    tenant: String,
    volume: String,
    generation: i64,
    uid: u32,
    data: PathBuf,
    ipc: PathBuf,
    scratch: PathBuf,
    model: Option<PathBuf>,
    google: Option<PathBuf>,
    browser: Option<PathBuf>,
    local_gws: bool,
    text_preview: bool,
    memory: u64,
    nano_cpus: u64,
    pids: u32,
    env: Value,
}
impl Expected {
    fn labels(&self) -> Value {
        json!({LABEL: PROFILE, "org.moosuri.account": self.account,
            "org.moosuri.tenant": self.tenant, "org.moosuri.volume": self.volume,
            "org.moosuri.generation": self.generation.to_string()})
    }
    fn command(&self) -> Value {
        let mut command = vec![
            "personal-volume".to_owned(),
            "--root".to_owned(),
            "/data".to_owned(),
            "--account".to_owned(),
            self.account.clone(),
            "--tenant".to_owned(),
            self.tenant.clone(),
            "--volume".to_owned(),
            self.volume.clone(),
            "--generation".to_owned(),
            self.generation.to_string(),
            "serve".to_owned(),
            "--socket".to_owned(),
            "/ipc/personal.sock".to_owned(),
        ];
        if self.text_preview {
            command.extend([
                "--text-preview-config".to_owned(),
                "/etc/moosuri/text-preview.json".to_owned(),
            ]);
        }
        if self.local_gws {
            command.push("--local-gws".into());
        } else if self.google.is_some() {
            command.push("--google-readonly".into());
        }
        if self.browser.is_some() {
            command.push("--browser".into());
        }
        json!(command)
    }
    fn create(&self) -> Value {
        let mut binds = vec![
            format!("{}:/data:rw", self.data.display()),
            format!("{}:/ipc:rw", self.ipc.display()),
            format!("{}:/tmp:rw", self.scratch.display()),
        ];
        if let Some(model) = &self.model {
            binds.push(format!("{}:/model:ro", model.display()));
        }
        if let Some(google) = &self.google {
            binds.push(format!("{}:/google:ro", google.display()));
        }
        if let Some(browser) = &self.browser {
            binds.push(format!("{}:/browser:ro", browser.display()));
        }
        json!({"Image": self.image, "Entrypoint": [ENTRYPOINT], "Cmd": self.command(),
            "User": format!("{}:{}",self.uid,self.uid), "WorkingDir": "/", "Env": self.env,
            "Labels": self.labels(), "Healthcheck": {"Test": ["NONE"]},
            "OpenStdin": false, "Tty": false,
            "HostConfig": {"NetworkMode":"none", "ReadonlyRootfs":true,
                "Privileged":false, "CapDrop":["ALL"], "CapAdd":[],
                "SecurityOpt":["no-new-privileges:true"],
                "Memory":self.memory,"NanoCpus":self.nano_cpus,"PidsLimit":self.pids,
                "RestartPolicy":{"Name":"no","MaximumRetryCount":0},
                "LogConfig":{"Type":"none","Config":{}},
                "Binds":binds}})
    }
    fn reservation(&self) -> Value {
        let mut row = json!({"profile":PROFILE,"account":self.account,"tenant":self.tenant,
            "volume":self.volume,"uid":self.uid,"data":self.data,"ipc":self.ipc,"scratch":self.scratch});
        if let Some(model) = &self.model {
            row["model"] = json!(model);
        }
        if let Some(google) = &self.google {
            row["google"] = json!(google);
        }
        row
    }
}

/// Reconciles one already-initialized member; never creates identity or private DBs.
/// The caller must perform runtime health/auth checks before declaring readiness.
pub async fn run(args: &Options) -> Result<(), String> {
    println!("{}", reconcile_member(args).await?);
    Ok(())
}
/// Same guarded operation without emitting per-poll output for the lifecycle worker.
pub(crate) async fn reconcile_member(args: &Options) -> Result<Value, String> {
    let args = args.clone();
    tokio::task::spawn_blocking(move || run_blocking(&args))
        .await
        .map_err(|_| "kiri_provision_worker_failed".to_owned())?
}
fn run_blocking(args: &Options) -> Result<Value, String> {
    let (expected, engine) = expected_member(args, true)?;
    let _lock = reserve_expected(args, &expected)?;
    let state = reconcile(&engine, &expected, args.start)?;
    Ok(
        json!({"profile":PROFILE,"container":expected.name,"account":expected.account,"generation":expected.generation,"state":state,
        "readiness":"unverified-private-auth-only","production":false}),
    )
}
fn expected_member(args: &Options, initialized: bool) -> Result<(Expected, Engine), String> {
    if !cfg!(target_os = "linux") || !args.experimental {
        return Err("kiri_provision_explicit_experimental_linux_only".into());
    }
    if unsafe { libc::geteuid() } != 0 {
        return Err("kiri_provision_requires_operator_root".into());
    }
    validate_limits(args)?;
    require_unused_host_uid(args.uid)?;
    trusted_dir(&args.registry_directory, 0, true)?;
    trusted_dir(&args.mounts_root, 0, false)?;
    trusted_dir(&args.ipc_root, 0, true)?;
    trusted_dir(&args.allocations_root, 0, true)?;
    let socket = checked_metadata(&args.engine_socket)?;
    if !socket.file_type().is_socket() || socket.uid() != 0 || socket.mode() & 0o007 != 0 {
        return Err("kiri_provision_untrusted_engine_socket".into());
    }
    let allocation =
        moosuri_store::runtime_registry::lookup(&args.registry_directory, args.email.clone())
            .map_err(|_| "kiri_provision_registry_lookup_failed".to_owned())?;
    let account = canonical_uuid(&allocation.account_id)?;
    let tenant = canonical_uuid(&allocation.tenant_id)?;
    let volume = canonical_uuid(&allocation.volume_id)?;
    if allocation.generation <= 0 {
        return Err("kiri_provision_invalid_generation".into());
    }
    let mount = args.mounts_root.join(&volume);
    validate_mount(&mount, args.max_volume_bytes)?;
    let data = mount.join("member-data");
    trusted_dir(&data, args.uid, true)?;
    validate_member_data(
        &data,
        &format!("{volume}-{account}-{tenant}-g{}", allocation.generation),
        args.uid,
        initialized,
    )?;
    let scratch = mount.join("scratch");
    trusted_dir(&scratch, args.uid, true)?;
    let ipc = args.ipc_root.join(&account);
    trusted_dir(&ipc, args.uid, true)?;
    if fs::metadata(&ipc)
        .map_err(|_| "kiri_provision_ipc_metadata_failed")?
        .dev()
        != fs::metadata(&data)
            .map_err(|_| "kiri_provision_data_metadata_failed")?
            .dev()
    {
        return Err("kiri_provision_ipc_must_share_bounded_account_filesystem".into());
    }

    for entry in fs::read_dir(&ipc).map_err(|_| "kiri_provision_ipc_read_failed")? {
        let entry = entry.map_err(|_| "kiri_provision_ipc_read_failed")?;
        let m = fs::symlink_metadata(entry.path()).map_err(|_| "kiri_provision_ipc_read_failed")?;
        if entry.file_name() != "personal.sock"
            || !m.file_type().is_socket()
            || m.uid() != args.uid
            || m.mode() & 0o077 != 0
        {
            return Err("kiri_provision_foreign_ipc_entry".into());
        }
    }
    let model = args
        .model_sockets_root
        .as_deref()
        .map(|root| broker_directory(root, &account, args.uid, "model.sock"))
        .transpose()?;
    let engine = Engine(args.engine_socket.clone());
    let version = engine.required("GET", "/version", None)?;
    if version["KiriVersion"] != "0.2.1"
        || version["ApiVersion"] != "1.47"
        || version["Os"] != "linux"
    {
        return Err("kiri_provision_requires_kiri_0_2_1_linux_api_1_47".into());
    }
    let image = engine.required("GET", &format!("{API}/images/{}/json", args.image), None)?;
    if image["Id"] != args.image
        || image["Os"] != "linux"
        || image["Config"]["Entrypoint"] != json!([ENTRYPOINT])
    {
        return Err("kiri_provision_unvalidated_image".into());
    }
    // Kiri 0.2.1 image inspect omits VOLUME declarations. Exact mount reconciliation
    // below rejects any inherited anonymous mount before the new container can start.
    let env = image["Config"]["Env"].clone();
    let vars = env
        .as_array()
        .ok_or("kiri_provision_image_env_unsupported")?;
    if vars.len() > 1
        || vars.iter().any(|v| {
            v.as_str() != Some("PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin")
        })
    {
        return Err("kiri_provision_image_environment_not_minimal".into());
    }
    let google = args
        .google_sockets_root
        .as_deref()
        .map(|root| broker_directory(root, &account, args.uid, "google.sock"))
        .transpose()?;
    let browser = args
        .browser_sockets_root
        .as_deref()
        .map(|root| broker_directory(root, &account, args.uid, "browser.sock"))
        .transpose()?;
    let expected = Expected {
        name: format!("moosuri-member-{account}"),
        image: args.image.clone(),
        account,
        tenant,
        volume,
        generation: allocation.generation,
        uid: args.uid,
        data,
        ipc,
        scratch,
        google,
        browser,
        model,
        local_gws: args.enable_local_gws,
        text_preview: args.enable_text_preview,
        memory: args.memory_bytes,
        nano_cpus: args.nano_cpus,
        pids: args.pids,
        env,
    };
    Ok((expected, engine))
}

fn reserve_expected(args: &Options, expected: &Expected) -> Result<File, String> {
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(args.allocations_root.join(".lock"))
        .map_err(|_| "kiri_provision_allocation_lock_failed")?;
    let metadata = lock
        .metadata()
        .map_err(|_| "kiri_provision_allocation_lock_failed")?;
    if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o077 != 0 {
        return Err("kiri_provision_untrusted_lock".into());
    }
    lock.lock_exclusive()
        .map_err(|_| "kiri_provision_allocation_lock_failed")?;
    reserve_host(args, expected)?;
    Ok(lock)
}

/// Metadata-only first-use preflight. Never opens the private SQLite database.
pub(crate) struct PreparedMember {
    pub account_id: String,
    pub tenant_id: String,
    pub volume_id: String,
    pub generation: i64,
    pub data_directory: PathBuf,
    pub scratch_directory: PathBuf,
    pub ipc_directory: PathBuf,
    pub runtime_running: bool,
}
pub(crate) async fn prepare_member(args: &Options) -> Result<PreparedMember, String> {
    let args = args.clone();
    tokio::task::spawn_blocking(move || {
        let (e, engine) = expected_member(&args, false)?;
        let _lock = reserve_expected(&args, &e)?;
        let (status, v) =
            engine.request("GET", &format!("{API}/containers/{}/json", e.name), None)?;
        let running = match status {
            404 => false,
            200 => {
                verify_inspect(&v, &e)?;
                v["State"]["Running"] == true
            }
            n => return Err(format!("kiri_provision_engine_http_{n}")),
        };
        Ok(PreparedMember {
            account_id: e.account,
            tenant_id: e.tenant,
            volume_id: e.volume,
            generation: e.generation,
            data_directory: e.data,
            scratch_directory: e.scratch,
            ipc_directory: e.ipc,
            runtime_running: running,
        })
    })
    .await
    .map_err(|_| "kiri_provision_worker_failed".to_owned())?
}
fn validate_member_data(
    root: &Path,
    name: &str,
    uid: u32,
    initialized: bool,
) -> Result<(), String> {
    if initialized {
        return validate_initialized_data(root, name, uid);
    }
    let entries = fs::read_dir(root)
        .map_err(|_| "kiri_provision_volume_read_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "kiri_provision_volume_read_failed")?;
    if entries.is_empty() {
        return Ok(());
    }
    if entries.len() != 1 || entries[0].file_name() != name {
        return Err("kiri_provision_exact_owner_directory_required".into());
    }
    let m = checked_metadata(&root.join(name))?;
    if !m.is_dir() || m.uid() != uid || m.mode() & 0o077 != 0 {
        return Err("kiri_provision_private_owner_subdirectory_required".into());
    }
    Ok(())
}

pub(crate) fn validate_limits(a: &Options) -> Result<(), String> {
    if a.enable_local_gws && a.google_sockets_root.is_none() {
        return Err("kiri_provision_local_gws_requires_google_sockets_root".into());
    }
    if a.uid < 10000
        || a.uid > 2_000_000_000
        || !config_id(&a.image)
        || !(32 * 1024 * 1024..=64 * 1024 * 1024 * 1024u64).contains(&a.memory_bytes)
        || !(10_000_000..=64_000_000_000).contains(&a.nano_cpus)
        || a.nano_cpus % 10_000 != 0
        || !(16..=4096).contains(&a.pids)
        || !(16 * 1024 * 1024..=1024 * 1024 * 1024 * 1024u64).contains(&a.max_volume_bytes)
    {
        return Err("kiri_provision_invalid_limits_or_image".into());
    }
    Ok(())
}
fn require_unused_host_uid(uid: u32) -> Result<(), String> {
    let mut buffer = vec![0u8; 65536];
    let mut user = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut found = std::ptr::null_mut();
    let code = unsafe {
        libc::getpwuid_r(
            uid,
            user.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if code != 0 || !found.is_null() {
        return Err("kiri_provision_uid_is_existing_host_user".into());
    }
    let mut group = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut found = std::ptr::null_mut();
    let code = unsafe {
        libc::getgrgid_r(
            uid,
            group.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut found,
        )
    };
    if code != 0 || !found.is_null() {
        return Err("kiri_provision_gid_is_existing_host_group".into());
    }
    Ok(())
}
fn config_id(s: &str) -> bool {
    s.strip_prefix("sha256:").is_some_and(|h| {
        h.len() == 64
            && h.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
fn canonical_uuid(s: &str) -> Result<String, String> {
    let id = Uuid::parse_str(s).map_err(|_| "kiri_provision_invalid_registry_uuid")?;
    if id.is_nil() || id.to_string() != s {
        return Err("kiri_provision_noncanonical_registry_uuid".into());
    }
    Ok(s.into())
}
fn checked_metadata(p: &Path) -> Result<fs::Metadata, String> {
    if !p.is_absolute()
        || p.to_str().is_none()
        || p.to_string_lossy().contains([':', '\n', '\r', '\0'])
    {
        return Err("kiri_provision_requires_plain_absolute_path".into());
    }
    let canonical = fs::canonicalize(p).map_err(|_| "kiri_provision_path_missing")?;
    if canonical != p {
        return Err("kiri_provision_symlink_or_noncanonical_path".into());
    }
    fs::symlink_metadata(p).map_err(|_| "kiri_provision_path_metadata_failed".into())
}
fn trusted_dir(p: &Path, uid: u32, private: bool) -> Result<(), String> {
    let m = checked_metadata(p)?;
    if !m.is_dir() || m.uid() != uid || m.mode() & (if private { 0o077 } else { 0o022 }) != 0 {
        return Err("kiri_provision_untrusted_directory".into());
    }
    // A writable ancestor would permit swapping a validated directory for a symlink.
    for parent in p.ancestors().skip(1) {
        let m =
            fs::symlink_metadata(parent).map_err(|_| "kiri_provision_ancestor_metadata_failed")?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("kiri_provision_untrusted_ancestor".into());
        }
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn validate_mount(p: &Path, cap: u64) -> Result<(), String> {
    trusted_dir(p, 0, false)?;
    let mounts = fs::read_to_string("/proc/self/mountinfo")
        .map_err(|_| "kiri_provision_mountinfo_unavailable")?;
    let line = mounts
        .lines()
        .find(|line| line.split_whitespace().nth(4) == p.to_str())
        .ok_or("kiri_provision_dedicated_mount_required")?;
    let (left, right) = line
        .split_once(" - ")
        .ok_or("kiri_provision_mountinfo_invalid")?;
    let opts = left
        .split_whitespace()
        .nth(5)
        .ok_or("kiri_provision_mountinfo_invalid")?;
    let mut tail = right.split_whitespace();
    let kind = tail.next().unwrap_or("");
    let source = tail.next().unwrap_or("");
    if !matches!(kind, "ext4" | "xfs")
        || !source.starts_with("/dev/")
        || !["rw", "nosuid", "nodev", "noexec"]
            .iter()
            .all(|o| opts.split(',').any(|s| s == *o))
    {
        return Err("kiri_provision_bounded_private_filesystem_required".into());
    }
    if fs::metadata(p)
        .map_err(|_| "kiri_provision_mount_metadata_failed")?
        .dev()
        == fs::metadata(p.parent().ok_or("kiri_provision_mount_parent_missing")?)
            .map_err(|_| "kiri_provision_mount_metadata_failed")?
            .dev()
    {
        return Err("kiri_provision_separate_device_required".into());
    }
    let c = std::ffi::CString::new(p.as_os_str().as_encoded_bytes())
        .map_err(|_| "kiri_provision_invalid_mount_path")?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err("kiri_provision_statvfs_failed".into());
    }
    let stat = unsafe { stat.assume_init() };
    let bytes = u64::try_from(u128::from(stat.f_blocks) * u128::from(stat.f_frsize))
        .map_err(|_| "kiri_provision_filesystem_size_overflow")?;
    if bytes == 0 || bytes > cap {
        return Err("kiri_provision_filesystem_over_capacity".into());
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn validate_mount(_: &Path, _: u64) -> Result<(), String> {
    Err("kiri_provision_linux_only".into())
}
// Bind the directory, not a socket inode: broker replacement remains visible.
fn broker_directory(
    root: &Path,
    account: &str,
    uid: u32,
    filename: &str,
) -> Result<PathBuf, String> {
    trusted_dir(root, 0, true)?;
    let child = root.join(account);
    trusted_dir(&child, 0, false)?;
    let meta = checked_metadata(&child)?;
    if meta.mode() & 0o7777 != 0o711 {
        return Err("kiri_provision_model_directory_mode_required".into());
    }
    let entries = fs::read_dir(&child)
        .map_err(|_| "kiri_provision_model_directory_unavailable")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "kiri_provision_model_directory_unavailable")?;
    if entries.len() != 1 || entries[0].file_name() != filename {
        return Err("kiri_provision_exact_model_socket_required".into());
    }
    let socket = checked_metadata(&child.join(filename))?;
    if !socket.file_type().is_socket()
        || socket.uid() != uid
        || socket.nlink() != 1
        || socket.mode() & 0o7777 != 0o600
    {
        return Err("kiri_provision_private_owner_model_socket_required".into());
    }
    Ok(child)
}
fn validate_initialized_data(root: &Path, expected: &str, uid: u32) -> Result<(), String> {
    let entries = fs::read_dir(root)
        .map_err(|_| "kiri_provision_volume_read_failed")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "kiri_provision_volume_read_failed")?;
    if entries.len() != 1 || entries[0].file_name() != expected {
        return Err("kiri_provision_initialized_exact_owner_volume_required".into());
    }
    let dir = root.join(expected);
    let meta = checked_metadata(&dir)?;
    if !meta.is_dir() || meta.uid() != uid || meta.mode() & 0o077 != 0 {
        return Err("kiri_provision_private_owner_subdirectory_required".into());
    }
    let db = checked_metadata(&dir.join("assistant.sqlite"))?;
    if !db.is_file()
        || db.nlink() != 1
        || db.uid() != uid
        || db.mode() & 0o077 != 0
        || db.len() == 0
    {
        return Err("kiri_provision_initialized_private_db_required".into());
    }
    // The runtime verifies DB owner rows under its writer lock before socket binding.
    Ok(())
}
// Older allocations may predate the optional Google mount. The host record
// remains unchanged; every live mount is still matched by container inspection.
fn reservation_matches(existing: &Value, desired: &Value, local_gws: bool) -> bool {
    if existing == desired {
        return true;
    }
    if !local_gws || existing.get("google").is_some() {
        return false;
    }
    let mut previous = desired.clone();
    previous
        .as_object_mut()
        .and_then(|fields| fields.remove("google"))
        .is_some()
        && previous == *existing
}

fn reserve_host(args: &Options, e: &Expected) -> Result<(), String> {
    let want = e.reservation();
    let mut found = false;
    let path = args.allocations_root.join(format!("uid-{}.json", e.uid));
    for entry in fs::read_dir(&args.allocations_root)
        .map_err(|_| "kiri_provision_allocations_unavailable")?
    {
        let entry = entry.map_err(|_| "kiri_provision_allocations_unavailable")?;
        if entry.file_name() == ".lock" {
            continue;
        }
        let name = entry.file_name();
        let name = name
            .to_str()
            .ok_or("kiri_provision_allocation_name_invalid")?;
        if !name.starts_with("uid-") || !name.ends_with(".json") {
            return Err("kiri_provision_foreign_allocation_entry".into());
        }
        let m = checked_metadata(&entry.path())?;
        if !m.is_file() || m.uid() != 0 || m.mode() & 0o077 != 0 || m.len() > 4096 {
            return Err("kiri_provision_untrusted_allocation".into());
        }
        let row: Value = serde_json::from_slice(
            &fs::read(entry.path()).map_err(|_| "kiri_provision_allocation_read_failed")?,
        )
        .map_err(|_| "kiri_provision_allocation_corrupt")?;
        if entry.path() == path {
            if !reservation_matches(&row, &want, e.local_gws) {
                return Err("kiri_provision_uid_assignment_conflict".into());
            }
            found = true;
            continue;
        }
        if ["account", "volume", "data", "ipc", "scratch"]
            .iter()
            .any(|k| row[*k] == want[*k])
        {
            return Err("kiri_provision_host_assignment_reuse".into());
        }
    }
    if found {
        return Ok(());
    }
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "kiri_provision_uid_reservation_failed")?;
    file.write_all(
        &serde_json::to_vec(&want).map_err(|_| "kiri_provision_allocation_encode_failed")?,
    )
    .and_then(|_| file.sync_all())
    .map_err(|_| "kiri_provision_uid_reservation_write_failed")?;
    File::open(&args.allocations_root)
        .and_then(|f| f.sync_all())
        .map_err(|_| "kiri_provision_allocation_directory_sync_failed")?;
    Ok(())
}

struct Engine(PathBuf);
impl Engine {
    fn request(
        &self,
        method: &str,
        path: &str,
        body: Option<Value>,
    ) -> Result<(u16, Value), String> {
        let mut s =
            UnixStream::connect(&self.0).map_err(|_| "kiri_provision_engine_unavailable")?;
        s.set_read_timeout(Some(Duration::from_secs(10)))
            .and_then(|_| s.set_write_timeout(Some(Duration::from_secs(10))))
            .map_err(|_| "kiri_provision_engine_timeout_setup_failed")?;
        let body = body
            .map(|b| serde_json::to_vec(&b))
            .transpose()
            .map_err(|_| "kiri_provision_request_encode_failed")?
            .unwrap_or_default();
        write!(s,"{method} {path} HTTP/1.1\r\nHost: localhost\r\nContent-Type: application/json\r\nConnection: close\r\nContent-Length: {}\r\n\r\n",body.len()).and_then(|_|s.write_all(&body)).map_err(|_|"kiri_provision_engine_write_failed")?;
        let mut head = Vec::new();
        while !head.ends_with(b"\r\n\r\n") {
            let mut b = [0];
            s.read_exact(&mut b)
                .map_err(|_| "kiri_provision_engine_header_failed")?;
            head.push(b[0]);
            if head.len() > 16384 {
                return Err("kiri_provision_engine_header_too_large".into());
            }
        }
        let text =
            std::str::from_utf8(&head).map_err(|_| "kiri_provision_engine_header_invalid")?;
        let status = text
            .lines()
            .next()
            .and_then(|l| l.split_whitespace().nth(1))
            .and_then(|n| n.parse::<u16>().ok())
            .ok_or("kiri_provision_engine_status_invalid")?;
        let mut length = None;
        for line in text.lines().skip(1) {
            if let Some((k, v)) = line.split_once(':') {
                if k.eq_ignore_ascii_case("transfer-encoding") {
                    return Err("kiri_provision_engine_transfer_encoding_unsupported".into());
                }
                if k.eq_ignore_ascii_case("content-length") {
                    if length.is_some() {
                        return Err("kiri_provision_engine_duplicate_length".into());
                    }
                    length = Some(
                        v.trim()
                            .parse::<usize>()
                            .map_err(|_| "kiri_provision_engine_length_invalid")?,
                    );
                }
            }
        }
        let length = if status == 204 {
            0
        } else {
            length.ok_or("kiri_provision_engine_length_missing")?
        };
        if length > BODY_LIMIT {
            return Err("kiri_provision_engine_body_too_large".into());
        }
        let mut body = vec![0; length];
        s.read_exact(&mut body)
            .map_err(|_| "kiri_provision_engine_body_failed")?;
        let value = if body.is_empty() {
            Value::Null
        } else {
            serde_json::from_slice(&body).map_err(|_| "kiri_provision_engine_json_invalid")?
        };
        Ok((status, value))
    }
    fn required(&self, m: &str, p: &str, b: Option<Value>) -> Result<Value, String> {
        let (status, v) = self.request(m, p, b)?;
        if !(200..300).contains(&status) {
            return Err(format!("kiri_provision_engine_http_{status}"));
        }
        Ok(v)
    }
}
fn reconcile(engine: &Engine, e: &Expected, start: bool) -> Result<String, String> {
    let path = format!("{API}/containers/{}/json", e.name);
    let (status, mut inspect) = engine.request("GET", &path, None)?;
    if status == 404 {
        let created = engine.required(
            "POST",
            &format!("{API}/containers/create?name={}", e.name),
            Some(e.create()),
        )?;
        if !created["Warnings"].as_array().is_some_and(|a| a.is_empty()) {
            return Err("kiri_provision_create_warning".into());
        }
        inspect = engine.required("GET", &path, None)?;
    } else if status != 200 {
        return Err(format!("kiri_provision_engine_http_{status}"));
    }
    verify_inspect(&inspect, e)?;
    if start && inspect["State"]["Running"] == false {
        engine.required("POST", &format!("{API}/containers/{}/start", e.name), None)?;
        inspect = engine.required("GET", &path, None)?;
        verify_inspect(&inspect, e)?;
        if inspect["State"]["Running"] != true {
            return Err("kiri_provision_start_not_running".into());
        }
    }
    Ok(if inspect["State"]["Running"] == true {
        "running"
    } else {
        "stopped"
    }
    .into())
}
fn verify_inspect(v: &Value, e: &Expected) -> Result<(), String> {
    let mut checks = vec![
        (&v["Name"], json!(format!("/{}", e.name))),
        (&v["Image"], json!(e.image)),
        (&v["Path"], json!(ENTRYPOINT)),
        (&v["Args"], e.command()),
        (&v["Config"]["Entrypoint"], json!([ENTRYPOINT])),
        (&v["Config"]["Cmd"], e.command()),
        (&v["Config"]["User"], json!(format!("{}:{}", e.uid, e.uid))),
        (&v["Config"]["WorkingDir"], json!("/")),
        (&v["Config"]["Env"], e.env.clone()),
        (&v["Config"]["Labels"], e.labels()),
        (&v["Config"]["Healthcheck"]["Test"], json!(["NONE"])),
        (&v["Config"]["Tty"], json!(false)),
        (&v["Config"]["OpenStdin"], json!(false)),
    ];
    let h = &v["HostConfig"];
    for (key, want) in [
        ("NetworkMode", json!("none")),
        ("ReadonlyRootfs", json!(true)),
        ("Privileged", json!(false)),
        ("AutoRemove", json!(false)),
        ("CapAdd", json!([])),
        ("CapDrop", json!(["ALL"])),
        ("SecurityOpt", json!(["no-new-privileges:true"])),
        ("Memory", json!(e.memory)),
        ("NanoCpus", json!(e.nano_cpus)),
        ("CpuQuota", json!(e.nano_cpus / 10000)),
        ("CpuPeriod", json!(100000)),
        ("PidsLimit", json!(e.pids)),
        ("RestartPolicy", json!({"Name":"no","MaximumRetryCount":0})),
        ("LogConfig", json!({"Type":"none","Config":{}})),
        ("Devices", json!([])),
        ("Dns", json!([])),
        ("DnsSearch", json!([])),
        ("DnsOptions", json!([])),
    ] {
        checks.push((&h[key], want));
    }
    if checks.iter().any(|(got, want)| *got != want) {
        return Err("kiri_provision_inspected_profile_conflict".into());
    }
    for key in [
        "GroupAdd",
        "Ulimits",
        "Sysctls",
        "DeviceCgroupRules",
        "Links",
    ] {
        if !h[key].is_null() && h[key] != json!([]) && h[key] != json!({}) {
            return Err("kiri_provision_inspected_host_option_conflict".into());
        }
    }
    let networks = v["NetworkSettings"]["Networks"]
        .as_object()
        .ok_or("kiri_provision_network_inspect_missing")?;
    if networks.len() != 1
        || !networks.contains_key("none")
        || v["NetworkSettings"]["Ports"] != json!({})
        || networks["none"]["IPAddress"] != ""
        || networks["none"]["GlobalIPv6Address"] != ""
    {
        return Err("kiri_provision_inspected_network_conflict".into());
    }
    let mounts = v["Mounts"]
        .as_array()
        .ok_or("kiri_provision_mount_inspect_missing")?;
    if mounts.len()
        != 3 + usize::from(e.model.is_some())
            + usize::from(e.google.is_some())
            + usize::from(e.browser.is_some())
    {
        return Err("kiri_provision_inspected_mount_conflict".into());
    }
    let mut required = vec![
        ("/data", &e.data, true),
        ("/ipc", &e.ipc, true),
        ("/tmp", &e.scratch, true),
    ];
    if let Some(google) = &e.google {
        required.push(("/google", google, false));
    }
    if let Some(model) = &e.model {
        required.push(("/model", model, false));
    }
    if let Some(browser) = &e.browser {
        required.push(("/browser", browser, false));
    }
    for (dest, source, writable) in required {
        let m = mounts
            .iter()
            .find(|m| m["Destination"] == dest)
            .ok_or("kiri_provision_inspected_mount_conflict")?;
        if m["Type"] != "bind"
            || m["Source"] != json!(source)
            || m["RW"] != writable
            || m["Propagation"] != "rprivate"
        {
            return Err("kiri_provision_inspected_mount_conflict".into());
        }
    }
    if !v["State"]["Running"].is_boolean()
        || v["State"]["Paused"] != false
        || v["State"]["Dead"] != false
        || v["State"]["Restarting"] != false
    {
        return Err("kiri_provision_inspected_state_conflict".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{os::unix::net::UnixListener, thread};
    fn expected() -> Expected {
        Expected {
            name: "moosuri-member-00000000-0000-0000-0000-000000000001".into(),
            image: format!("sha256:{}", "a".repeat(64)),
            account: "00000000-0000-0000-0000-000000000001".into(),
            tenant: "00000000-0000-0000-0000-000000000002".into(),
            volume: "00000000-0000-0000-0000-000000000003".into(),
            generation: 1,
            uid: 10001,
            data: "/srv/synthetic/data".into(),
            ipc: "/srv/synthetic/ipc".into(),
            scratch: "/srv/synthetic/scratch".into(),
            model: None,
            google: None,
            browser: None,
            local_gws: false,
            text_preview: false,
            memory: 67108864,
            nano_cpus: 500000000,
            pids: 32,
            env: json!([]),
        }
    }
    // Fields and null/empty shapes are from Kiri 0.2.1's real Linux inspect.
    fn inspect(e: &Expected, running: bool) -> Value {
        let mut v = json!({
        "Name":format!("/{}",e.name),"Image":e.image,"Path":ENTRYPOINT,"Args":e.command(),
        "Config":{"Entrypoint":[ENTRYPOINT],"Cmd":e.command(),"User":"10001:10001","WorkingDir":"/","Env":[],"Labels":e.labels(),"Healthcheck":{"Test":["NONE"]},"Tty":false,"OpenStdin":false},
        "HostConfig":{"AutoRemove":false,"Privileged":false,"CapAdd":[],"CapDrop":["ALL"],"SecurityOpt":["no-new-privileges:true"],"ReadonlyRootfs":true,"Memory":67108864,"NanoCpus":500000000,"CpuQuota":50000,"CpuPeriod":100000,"PidsLimit":32,"NetworkMode":"none","Dns":[],"DnsSearch":[],"DnsOptions":[],"Devices":[],"GroupAdd":null,"Ulimits":[],"Sysctls":null,"DeviceCgroupRules":null,"Links":null,"RestartPolicy":{"Name":"no","MaximumRetryCount":0},"LogConfig":{"Type":"none","Config":{}}},
        "NetworkSettings":{"Networks":{"none":{"IPAddress":"","GlobalIPv6Address":""}},"Ports":{}},
        "Mounts":[{"Destination":"/data","Type":"bind","Source":e.data,"RW":true,"Propagation":"rprivate"},{"Destination":"/ipc","Type":"bind","Source":e.ipc,"RW":true,"Propagation":"rprivate"},{"Destination":"/tmp","Type":"bind","Source":e.scratch,"RW":true,"Propagation":"rprivate"}],
        "State":{"Running":running,"Paused":false,"Dead":false,"Restarting":false}});
        if let Some(model) = &e.model {
            v["Mounts"].as_array_mut().unwrap().push(json!({"Destination":"/model","Type":"bind","Source":model,"RW":false,"Propagation":"rprivate"}));
        }
        if let Some(google) = &e.google {
            v["Mounts"].as_array_mut().unwrap().push(json!({"Destination":"/google","Type":"bind","Source":google,"RW":false,"Propagation":"rprivate"}));
        }
        if let Some(browser) = &e.browser {
            v["Mounts"].as_array_mut().unwrap().push(json!({"Destination":"/browser","Type":"bind","Source":browser,"RW":false,"Propagation":"rprivate"}));
        }
        v
    }
    #[test]
    fn browser_socket_is_readonly_and_bound_to_the_expected_member() {
        let mut e = expected();
        let old_reservation = e.reservation();
        e.browser = Some("/srv/browser/member".into());
        assert_eq!(e.reservation(), old_reservation);
        assert_eq!(e.command().as_array().unwrap().last().unwrap(), "--browser");
        let mut actual = inspect(&e, true);
        assert!(verify_inspect(&actual, &e).is_ok());
        let last = actual["Mounts"].as_array_mut().unwrap().last_mut().unwrap();
        last["Source"] = json!("/srv/browser/other");
        assert!(verify_inspect(&actual, &e).is_err());
        let mut actual = inspect(&e, true);
        actual["Mounts"].as_array_mut().unwrap().last_mut().unwrap()["RW"] = json!(true);
        assert!(verify_inspect(&actual, &e).is_err());
    }
    #[test]
    fn google_readonly_socket_mount_must_match_profile() {
        let mut e = expected();
        e.google = Some(PathBuf::from("/srv/google/member"));
        let v = inspect(&e, true);
        let (dir, engine, worker) = fake(vec![(200, v.clone())]);
        assert_eq!(reconcile(&engine, &e, true).unwrap(), "running");
        worker.join().unwrap();
        drop(dir);
        let mut writable = v;
        writable["Mounts"]
            .as_array_mut()
            .unwrap()
            .last_mut()
            .unwrap()["RW"] = json!(true);
        let (_dir, engine, worker) = fake(vec![(200, writable)]);
        assert!(reconcile(&engine, &e, true).is_err());
        worker.join().unwrap();
    }
    #[test]
    fn old_reservation_can_gain_only_the_verified_google_mount() {
        let mut e = expected();
        let original = e.reservation();
        e.google = Some("/srv/google/member".into());
        let desired = e.reservation();
        assert!(reservation_matches(&original, &desired, true));
        assert!(!reservation_matches(&original, &desired, false));
        let mut changed = original.clone();
        changed["volume"] = json!("another-volume");
        assert!(!reservation_matches(&changed, &desired, true));
        let mut another = desired.clone();
        another["google"] = json!("/srv/another/member");
        assert!(!reservation_matches(&another, &desired, true));
    }

    #[test]
    fn local_gws_uses_only_the_member_relay_mount_and_its_own_startup_mode() {
        let mut e = expected();
        e.google = Some("/srv/google/member".into());
        let legacy = inspect(&e, false);
        e.local_gws = true;
        assert!(verify_inspect(&legacy, &e).is_err());
        let create = e.create();
        assert_eq!(
            create["Cmd"].as_array().unwrap().last().unwrap(),
            "--local-gws"
        );
        assert_eq!(create["HostConfig"]["NetworkMode"], "none");
        assert_eq!(
            create["HostConfig"]["Binds"],
            json!([
                "/srv/synthetic/data:/data:rw",
                "/srv/synthetic/ipc:/ipc:rw",
                "/srv/synthetic/scratch:/tmp:rw",
                "/srv/google/member:/google:ro"
            ])
        );
        assert!(verify_inspect(&inspect(&e, true), &e).is_ok());
    }
    fn fake(
        responses: Vec<(u16, Value)>,
    ) -> (
        tempfile::TempDir,
        Engine,
        thread::JoinHandle<Vec<(String, Value)>>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("engine.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let worker = thread::spawn(move || {
            responses
                .into_iter()
                .map(|(status, response)| {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(Duration::from_secs(2)))
                        .unwrap();
                    let mut head = Vec::new();
                    while !head.ends_with(b"\r\n\r\n") {
                        let mut b = [0];
                        stream.read_exact(&mut b).unwrap();
                        head.push(b[0]);
                    }
                    let head = String::from_utf8(head).unwrap();
                    let n = head
                        .lines()
                        .find_map(|l| l.strip_prefix("Content-Length: "))
                        .unwrap()
                        .parse::<usize>()
                        .unwrap();
                    let mut body = vec![0; n];
                    stream.read_exact(&mut body).unwrap();
                    let body = if n == 0 {
                        Value::Null
                    } else {
                        serde_json::from_slice(&body).unwrap()
                    };
                    let bytes = if status == 204 {
                        vec![]
                    } else {
                        serde_json::to_vec(&response).unwrap()
                    };
                    write!(
                        stream,
                        "HTTP/1.1 {status} synthetic\r\nContent-Length: {}\r\n\r\n",
                        bytes.len()
                    )
                    .unwrap();
                    stream.write_all(&bytes).unwrap();
                    (head.lines().next().unwrap().to_owned(), body)
                })
                .collect()
        });
        (dir, Engine(path), worker)
    }
    #[test]
    fn matched_running_member_is_read_only_reconciliation() {
        let e = expected();
        let (_dir, engine, worker) = fake(vec![(200, inspect(&e, true))]);
        assert_eq!(reconcile(&engine, &e, true).unwrap(), "running");
        let requests = worker.join().unwrap();
        assert_eq!(requests.len(), 1);
        assert!(requests[0].0.starts_with("GET "));
    }
    #[test]
    fn missing_member_is_inspected_before_explicit_start() {
        let e = expected();
        let (_dir, engine, worker) = fake(vec![
            (404, json!({})),
            (201, json!({"Id":"synthetic","Warnings":[]})),
            (200, inspect(&e, false)),
            (204, Value::Null),
            (200, inspect(&e, true)),
        ]);
        assert_eq!(reconcile(&engine, &e, true).unwrap(), "running");
        let requests = worker.join().unwrap();
        assert!(requests[2].0.starts_with("GET "));
        assert!(requests[3].0.contains("/start "));
        assert_eq!(requests[1].1["HostConfig"]["RestartPolicy"]["Name"], "no");
        assert_eq!(requests[1].1["Entrypoint"], json!([ENTRYPOINT]));
        assert_eq!(requests[1].1["Cmd"], e.command());
        assert_eq!(requests[1].1["HostConfig"]["NetworkMode"], "none");
    }
    #[test]
    fn foreign_generation_mount_network_or_security_never_starts() {
        let e = expected();
        let mut fixtures = Vec::new();
        let mut v = inspect(&e, false);
        v["Config"]["Labels"]["org.moosuri.generation"] = json!("2");
        fixtures.push(v);
        let mut v = inspect(&e, false);
        v["Mounts"][0]["Source"] = json!("/srv/another-member");
        fixtures.push(v);
        let mut v = inspect(&e, false);
        v["Mounts"]
            .as_array_mut()
            .unwrap()
            .push(json!({"Destination":"/secret"}));
        fixtures.push(v);
        let mut v = inspect(&e, false);
        v["NetworkSettings"]["Networks"]["bridge"] = json!({});
        fixtures.push(v);
        let mut v = inspect(&e, false);
        v["HostConfig"]["SecurityOpt"] = json!(["seccomp=unconfined"]);
        fixtures.push(v);
        let mut v = inspect(&e, false);
        v["Config"]["Cmd"] = json!(["shell"]);
        fixtures.push(v);
        for v in fixtures {
            let (_dir, engine, worker) = fake(vec![(200, v)]);
            assert!(reconcile(&engine, &e, true).is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
    }
    #[test]
    fn silently_ignored_create_settings_leave_container_unstarted() {
        let e = expected();
        let mut v = inspect(&e, false);
        v["HostConfig"]["ReadonlyRootfs"] = json!(false);
        let (_dir, engine, worker) = fake(vec![
            (404, json!({})),
            (201, json!({"Warnings":[]})),
            (200, v),
        ]);
        assert!(reconcile(&engine, &e, true).is_err());
        assert_eq!(worker.join().unwrap().len(), 3);
    }
    #[test]
    fn optional_model_mount_is_exact_owner_directory_and_read_only() {
        let mut e = expected();
        e.model = Some(PathBuf::from("/srv/model").join(&e.account));
        let body = e.create();
        assert!(
            body["HostConfig"]["Binds"]
                .as_array()
                .unwrap()
                .iter()
                .any(|v| v.as_str()
                    == Some(&format!(
                        "{}:/model:ro",
                        e.model.as_ref().unwrap().display()
                    )))
        );
        let (_dir, engine, worker) = fake(vec![(200, inspect(&e, true))]);
        assert_eq!(reconcile(&engine, &e, true).unwrap(), "running");
        assert_eq!(worker.join().unwrap().len(), 1);
        let mut writable = inspect(&e, false);
        writable["Mounts"][3]["RW"] = json!(true);
        let mut peer = inspect(&e, false);
        peer["Mounts"][3]["Source"] = json!("/srv/model/00000000-0000-0000-0000-000000000099");
        for v in [writable, peer] {
            let (_dir, engine, worker) = fake(vec![(200, v)]);
            assert!(reconcile(&engine, &e, true).is_err());
            assert_eq!(worker.join().unwrap().len(), 1);
        }
        let without_option = expected();
        assert!(verify_inspect(&inspect(&e, true), &without_option).is_err());
    }
}

#[cfg(test)]
mod first_use_tests {
    use super::*;
    #[test]
    fn first_use_preflight_accepts_only_empty_or_exact_owner_layout_without_writes() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let name = "synthetic-owner";
        let uid = unsafe { libc::geteuid() };
        validate_member_data(&root, name, uid, false).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 0);
        assert!(validate_member_data(&root, name, uid, true).is_err());
        fs::create_dir(root.join("other-owner")).unwrap();
        assert!(validate_member_data(&root, name, uid, false).is_err());
    }
}
