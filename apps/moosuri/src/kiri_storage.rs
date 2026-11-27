//! Explicit Linux installation storage preparation. Only new files inside an
//! installer-owned dedicated root are formatted. No source/adoption/resize API.
use clap::Args;
use fs2::FileExt;
use moosuri_store::runtime_registry::{self, Allocation, InviteState};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Command,
};
use uuid::Uuid;
pub(crate) mod claude;
const MARKER: &str = "moosuri-kiri-storage-v1\n";
#[derive(Args, Clone)]
pub struct Options {
    #[arg(long)]
    pub experimental: bool,
    #[arg(long)]
    pub registry_directory: PathBuf,
    #[arg(long)]
    pub request_id: Uuid,
    #[arg(long)]
    pub email: String,
    /// Dedicated existing empty root-owned0700 directory under trusted ancestors.
    #[arg(long)]
    pub root: PathBuf,
    /// Dedicated non-root UID/GID; must not be a host user/group or another stored owner.
    #[arg(long)]
    pub uid: u32,
    /// Fixed preallocated backing bytes, including filesystem overhead. No resize.
    #[arg(long, default_value_t = 10737418240)]
    pub capacity_bytes: u64,
}
#[derive(Debug, Serialize)]
pub struct Prepared {
    pub account_id: String,
    pub tenant_id: String,
    pub volume_id: String,
    pub generation: i64,
    pub uid: u32,
    pub capacity_bytes: u64,
    pub mounts_root: PathBuf,
    pub ipc_root: PathBuf,
    pub allocations_root: PathBuf,
    pub member_data: PathBuf,
    pub scratch: PathBuf,
    pub ipc: PathBuf,
    pub filesystem_unit: String,
    pub ipc_unit: String,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Plan {
    v: u32,
    root: PathBuf,
    request: Uuid,
    account: String,
    tenant: String,
    volume: String,
    generation: i64,
    uid: u32,
    cap: u64,
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Planned,
    ImageCreated,
    Allocated,
    Formatting,
    Formatted,
    Ready,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    plan: Plan,
    phase: Phase,
    image_identity: Option<(u64, u64)>,
}
impl Plan {
    fn image(&self) -> PathBuf {
        self.root
            .join("images")
            .join(format!("{}.img", self.volume))
    }
    fn mount(&self) -> PathBuf {
        self.root.join("mounts").join(&self.volume)
    }
    fn ipc(&self) -> PathBuf {
        self.root.join("ipc").join(&self.account)
    }
    fn record(&self) -> PathBuf {
        self.root
            .join("records")
            .join(format!("{}.json", self.volume))
    }
    fn prepared(&self) -> Prepared {
        Prepared {
            account_id: self.account.clone(),
            tenant_id: self.tenant.clone(),
            volume_id: self.volume.clone(),
            generation: self.generation,
            uid: self.uid,
            capacity_bytes: self.cap,
            mounts_root: self.root.join("mounts"),
            ipc_root: self.root.join("ipc"),
            allocations_root: self.root.join("assignments"),
            member_data: self.mount().join("member-data"),
            scratch: self.mount().join("scratch"),
            ipc: self.ipc(),
            filesystem_unit: unit_name(&self.mount()),
            ipc_unit: unit_name(&self.ipc()),
        }
    }
}
pub async fn run(o: &Options) -> Result<(), String> {
    // Reject unsupported hosts before even opening installation metadata.
    if !cfg!(target_os = "linux") || !o.experimental || unsafe { libc::geteuid() } != 0 {
        return Err("storage_explicit_experimental_linux_root_only".into());
    }
    let o = o.clone();
    let result = tokio::task::spawn_blocking(move || {
        let a = runtime_registry::lookup(&o.registry_directory, o.email.clone())
            .map_err(|_| "storage_allocation_unavailable")?;
        prepare(&o, &a, o.uid)
    })
    .await
    .map_err(|_| "storage_worker_failed")??;
    println!(
        "{}",
        serde_json::to_string(&result).map_err(|_| "storage_result_invalid")?
    );
    Ok(())
}
/// Blocking host-only operation. Authority is rechecked here, not delegated to
/// CLI parsing. Caller must not hold a registry lock while invoking this method.
pub(crate) fn prepare(o: &Options, a: &Allocation, uid: u32) -> Result<Prepared, String> {
    if !cfg!(target_os = "linux") || !o.experimental || unsafe { libc::geteuid() } != 0 {
        return Err("storage_explicit_experimental_linux_root_only".into());
    }
    if uid != o.uid
        || !(10000..=60000).contains(&uid)
        || !(64 * 1024 * 1024..=1024u64 * 1024 * 1024 * 1024).contains(&o.capacity_bytes)
        || o.capacity_bytes % 4096 != 0
        || o.request_id.is_nil()
    {
        return Err("storage_invalid_limits".into());
    }
    trusted(&o.registry_directory, 0, 0o077)?;
    let allocated = runtime_registry::lookup(&o.registry_directory, o.email.clone())
        .map_err(|_| "storage_allocation_unavailable")?;
    if &allocated != a {
        return Err("storage_allocation_changed".into());
    }
    let state = runtime_registry::invitation_state(&o.registry_directory, o.request_id, a)
        .map_err(|_| "storage_allocation_not_authorized")?;
    if state == InviteState::Fenced {
        return Err("storage_allocation_fenced".into());
    }
    for id in [&a.account_id, &a.tenant_id, &a.volume_id] {
        let v = Uuid::parse_str(id).map_err(|_| "storage_invalid_identity")?;
        if v.is_nil() || v.to_string() != *id {
            return Err("storage_invalid_identity".into());
        }
    }
    if a.generation <= 0 {
        return Err("storage_invalid_identity".into());
    }
    unused_uid(uid)?;
    trusted(&o.root, 0, 0o022)?;
    let root_mode = fs::metadata(&o.root)
        .map_err(|_| "storage_root_unavailable")?
        .mode()
        & 0o777;
    if root_mode != 0o700 && !(root_mode == 0o711 && o.root.join("owner").exists()) {
        return Err("storage_root_mode_invalid".into());
    }
    if o.root.components().count() < 3 {
        return Err("storage_root_not_dedicated".into());
    }
    let lock = File::open(&o.root).map_err(|_| "storage_root_unavailable")?;
    lock.lock_exclusive().map_err(|_| "storage_root_busy")?;
    initialize_root(&o.root)?;
    check_traversal(uid, &o.root.join("mounts"))?;
    let plan = Plan {
        v: 1,
        root: o.root.clone(),
        request: o.request_id,
        account: a.account_id.clone(),
        tenant: a.tenant_id.clone(),
        volume: a.volume_id.clone(),
        generation: a.generation,
        uid,
        cap: o.capacity_bytes,
    };
    if unit_name(&plan.mount()).len() > 240 || unit_name(&plan.ipc()).len() > 240 {
        return Err("storage_unit_path_too_long".into());
    }
    let mut record = load_or_plan(&plan, state == InviteState::Ready)?;
    // Fence/suspension observed after acquiring our storage lock, before effects.
    if runtime_registry::invitation_state(&o.registry_directory, o.request_id, a)
        .map_err(|_| "storage_allocation_not_authorized")?
        == InviteState::Fenced
    {
        return Err("storage_allocation_fenced".into());
    }
    prepare_image(&mut record)?;
    let mount = plan.mount();
    let ipc = plan.ipc();
    empty_target(&mount)?;
    let fsunit = unit_name(&mount);
    let ipcunit = unit_name(&ipc);
    let (fsbody, ipcbody) = units(&plan);
    install_unit(&fsunit, &fsbody)?;
    command("/usr/bin/systemctl", &["daemon-reload"])?;
    if mounted(&mount)?.is_none() {
        command("/usr/bin/systemctl", &["start", &fsunit])?;
    }
    verify_mount(&plan)?;
    for name in ["member-data", "scratch", "ipc"] {
        layout(&mount.join(name), uid)?;
    }
    check_traversal(uid, &mount.join("member-data"))?;
    empty_target(&ipc)?;
    install_unit(&ipcunit, &ipcbody)?;
    command("/usr/bin/systemctl", &["daemon-reload"])?;
    if mounted(&ipc)?.is_none() {
        command("/usr/bin/systemctl", &["start", &ipcunit])?;
    }
    verify_ipc(&plan)?;
    // Both units are enabled only after source layout is persistent and verified.
    enable_unit(&fsunit)?;
    enable_unit(&ipcunit)?;
    command("/usr/bin/systemctl", &["daemon-reload"])?;
    record.phase = Phase::Ready;
    save(&record)?;
    // A concurrent control-plane fence never authorizes routing from this result.
    if runtime_registry::invitation_state(&o.registry_directory, o.request_id, a)
        .map_err(|_| "storage_allocation_not_authorized")?
        == InviteState::Fenced
    {
        return Err("storage_allocation_fenced".into());
    }
    Ok(plan.prepared())
}
fn plain(p: &Path) -> Result<(), String> {
    if !p.is_absolute()
        || p.to_str().is_none()
        || p.as_os_str()
            .as_encoded_bytes()
            .iter()
            .any(|b| !b.is_ascii_alphanumeric() && !b"/._-".contains(b))
    {
        return Err("storage_plain_absolute_path_required".into());
    }
    Ok(())
}
fn trusted(p: &Path, uid: u32, mask: u32) -> Result<(), String> {
    plain(p)?;
    if fs::canonicalize(p).ok().as_deref() != Some(p) {
        return Err("storage_noncanonical_path".into());
    }
    let m = fs::symlink_metadata(p).map_err(|_| "storage_path_unavailable")?;
    if !m.is_dir() || m.uid() != uid || m.mode() & mask != 0 {
        return Err("storage_untrusted_directory".into());
    }
    for parent in p.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| "storage_ancestor_unavailable")?;
        if m.file_type().is_symlink() || m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("storage_untrusted_ancestor".into());
        }
    }
    Ok(())
}
fn private_file(p: &Path) -> Result<File, String> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(p)
        .map_err(|_| "storage_file_unavailable")?;
    let m = f.metadata().map_err(|_| "storage_file_unavailable")?;
    if !m.is_file() || m.uid() != 0 || m.nlink() != 1 || m.mode() & 0o077 != 0 {
        return Err("storage_untrusted_file".into());
    }
    Ok(f)
}
fn write_new(p: &Path, bytes: &[u8]) -> Result<(), String> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(|_| "storage_new_file_conflict")?;
    f.write_all(bytes)
        .and_then(|_| f.sync_all())
        .map_err(|_| "storage_write_failed")?;
    sync(p.parent().ok_or("storage_parent_missing")?)
}
fn sync(p: &Path) -> Result<(), String> {
    File::open(p)
        .and_then(|f| f.sync_all())
        .map_err(|_| "storage_sync_failed".into())
}
fn owned_dir(p: &Path, mode: u32) -> Result<(), String> {
    match fs::symlink_metadata(p) {
        Ok(_) => trusted(p, 0, if mode == 0o700 { 0o077 } else { 0o022 }),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            fs::DirBuilder::new()
                .mode(mode)
                .create(p)
                .map_err(|_| "storage_directory_create_failed")?;
            fs::set_permissions(p, fs::Permissions::from_mode(mode))
                .map_err(|_| "storage_directory_mode_failed")?;
            sync(p.parent().ok_or("storage_parent_missing")?)
        }
        Err(_) => Err("storage_path_unavailable".into()),
    }
}
use std::os::unix::fs::DirBuilderExt;
fn initialize_root(root: &Path) -> Result<(), String> {
    let marker = root.join("owner");
    if !marker.exists() {
        if fs::read_dir(root)
            .map_err(|_| "storage_root_unavailable")?
            .next()
            .is_some()
        {
            return Err("storage_root_not_empty_or_owned".into());
        }
        write_new(&marker, MARKER.as_bytes())?;
    }
    let mut value = String::new();
    private_file(&marker)?
        .take(128)
        .read_to_string(&mut value)
        .map_err(|_| "storage_owner_invalid")?;
    if value != MARKER {
        return Err("storage_owner_invalid".into());
    }
    for item in fs::read_dir(root).map_err(|_| "storage_root_unavailable")? {
        let entry = item.map_err(|_| "storage_root_unavailable")?;
        if ![
            "owner",
            "images",
            "records",
            "staging",
            "mounts",
            "ipc",
            "assignments",
        ]
        .iter()
        .any(|n| entry.file_name() == *n)
        {
            return Err("storage_foreign_root_entry".into());
        }
    }
    for (name, mode) in [
        ("images", 0o700),
        ("records", 0o700),
        ("staging", 0o700),
        ("mounts", 0o755),
        ("ipc", 0o700),
        ("assignments", 0o700),
    ] {
        owned_dir(&root.join(name), mode)?;
        if name == "mounts" {
            fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o755))
                .map_err(|_| "storage_directory_mode_failed")?;
            sync(&root.join(name))?;
        }
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o711))
        .map_err(|_| "storage_root_traversal_failed")?;
    sync(root)?;
    Ok(())
}
fn read_record(p: &Path) -> Result<Record, String> {
    let mut raw = Vec::new();
    private_file(p)?
        .take(8193)
        .read_to_end(&mut raw)
        .map_err(|_| "storage_record_unavailable")?;
    if raw.len() > 8192 {
        return Err("storage_record_too_large".into());
    }
    serde_json::from_slice(&raw).map_err(|_| "storage_record_invalid".into())
}
/// Read the durable UID claims, including interrupted operations that predate enrollment.
pub(crate) fn reserved_uids(root: &Path) -> Result<Vec<(Uuid, u32)>, String> {
    trusted(root, 0, 0o022)?;
    let directory = File::open(root).map_err(|_| "storage_root_unavailable")?;
    FileExt::lock_shared(&directory).map_err(|_| "storage_root_busy")?;
    let records = root.join("records");
    if !records
        .try_exists()
        .map_err(|_| "storage_records_unavailable")?
    {
        return Ok(Vec::new());
    }
    trusted(&records, 0, 0o077)?;
    let mut result = Vec::new();
    for entry in fs::read_dir(records).map_err(|_| "storage_records_unavailable")? {
        let path = entry.map_err(|_| "storage_records_unavailable")?.path();
        // A legacy temporary cannot create a claim: its already committed
        // record remains the reservation until exclusive recovery below.
        if path.extension() == Some("next".as_ref()) {
            private_file(&path)?;
            let committed = read_record(&path.with_extension("json"))?;
            if committed.plan.root != root || committed.plan.record() != path.with_extension("json")
            {
                return Err("storage_untrusted_reservation".into());
            }
            continue;
        }
        let plan = read_record(&path)?.plan;
        if plan.v != 1 || plan.root != root || plan.record() != path {
            return Err("storage_untrusted_reservation".into());
        }
        result.push((plan.request, plan.uid));
    }
    Ok(result)
}
fn load_or_plan(p: &Plan, ready: bool) -> Result<Record, String> {
    recover_pending_records(&p.root)?;
    for entry in fs::read_dir(p.root.join("records")).map_err(|_| "storage_records_unavailable")? {
        let path = entry.map_err(|_| "storage_records_unavailable")?.path();
        if path.extension() != Some("json".as_ref()) {
            return Err("storage_pending_record_write_requires_inspection".into());
        }
        let r = read_record(&path)?;
        if r.plan.root != p.root
            || r.plan.v != 1
            || r.plan.volume != p.volume && (r.plan.uid == p.uid || r.plan.account == p.account)
        {
            return Err("storage_uid_or_account_already_reserved".into());
        }
    }
    match fs::symlink_metadata(p.record()) {
        Ok(_) => {
            let r = read_record(&p.record())?;
            if r.plan != *p {
                return Err("storage_plan_changed".into());
            }
            if ready && r.phase != Phase::Ready {
                return Err("storage_ready_allocation_without_ready_storage".into());
            }
            Ok(r)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            if ready {
                return Err("storage_ready_allocation_cannot_adopt_storage".into());
            }
            for path in [p.image(), p.mount(), p.ipc()] {
                if fs::symlink_metadata(path).is_ok() {
                    return Err("storage_unclaimed_resource".into());
                }
            }
            let r = Record {
                plan: p.clone(),
                phase: Phase::Planned,
                image_identity: None,
            };
            save(&r)?;
            Ok(r)
        }
        Err(_) => Err("storage_record_unavailable".into()),
    }
}
fn save(r: &Record) -> Result<(), String> {
    let p = r.plan.record();
    // Uncommitted bytes never appear in the reservation directory. A killed
    // writer can leave this exact private temporary without blocking owners.
    let staging = r.plan.root.join("staging");
    owned_dir(&staging, 0o700)?;
    let pending = staging.join(format!("{}.pending", r.plan.volume));
    if fs::symlink_metadata(&pending).is_ok() {
        private_file(&pending)?;
        fs::remove_file(&pending).map_err(|_| "storage_record_stage_failed")?;
    }
    write_new(
        &pending,
        &serde_json::to_vec(r).map_err(|_| "storage_record_invalid")?,
    )?;
    fs::rename(&pending, &p).map_err(|_| "storage_record_commit_failed")?;
    sync(p.parent().ok_or("storage_parent_missing")?)
}

/// Previous releases placed their atomic-write temporary next to claims.
/// Recover only a named, private temporary anchored by an exact durable claim.
/// Partial bytes had not committed any later phase; the old record is retained.
fn recover_pending_records(root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(root.join("records")).map_err(|_| "storage_records_unavailable")? {
        let path = entry.map_err(|_| "storage_records_unavailable")?.path();
        if path.extension() != Some("next".as_ref()) {
            continue;
        }
        let committed = path.with_extension("json");
        let old = read_record(&committed)?;
        if old.plan.root != root || old.plan.record() != committed || old.plan.v != 1 {
            return Err("storage_untrusted_reservation".into());
        }
        let mut pending_bytes = Vec::new();
        private_file(&path)?
            .take(8193)
            .read_to_end(&mut pending_bytes)
            .map_err(|_| "storage_record_unavailable")?;
        if pending_bytes.len() > 8192 {
            return Err("storage_record_too_large".into());
        }
        if let Ok(next) = serde_json::from_slice::<Record>(&pending_bytes) {
            if !record_follows(&old, &next) {
                return Err("storage_pending_record_conflict".into());
            }
            if next.image_identity.is_some() {
                image_identity(&next)?;
            }
            fs::rename(&path, &committed).map_err(|_| "storage_record_commit_failed")?;
        } else {
            // Unknown but complete JSON is a schema/conflict, not a partial
            // write. Keep it for inspection rather than erasing its claim.
            if serde_json::from_slice::<serde_json::Value>(&pending_bytes).is_ok() {
                return Err("storage_pending_record_conflict".into());
            }
            fs::remove_file(&path).map_err(|_| "storage_record_stage_failed")?;
        }
        sync(committed.parent().ok_or("storage_parent_missing")?)?;
    }
    Ok(())
}
fn record_follows(old: &Record, next: &Record) -> bool {
    old.plan == next.plan
        && match (old.phase, next.phase) {
            (Phase::Planned, Phase::ImageCreated) => {
                old.image_identity.is_none() && next.image_identity.is_some()
            }
            (Phase::ImageCreated, Phase::Allocated)
            | (Phase::Allocated, Phase::Formatting)
            | (Phase::Formatting, Phase::Formatted)
            | (Phase::Formatted, Phase::Ready) => {
                old.image_identity.is_some() && old.image_identity == next.image_identity
            }
            (a, b) if a == b => old.image_identity == next.image_identity,
            _ => false,
        }
}
fn image_identity(r: &Record) -> Result<(), String> {
    let f = private_file(&r.plan.image())?;
    let m = f.metadata().map_err(|_| "storage_image_unavailable")?;
    if r.image_identity != Some((m.dev(), m.ino())) || m.len() > r.plan.cap {
        return Err("storage_image_identity_changed".into());
    }
    Ok(())
}
fn prepare_image(r: &mut Record) -> Result<(), String> {
    prepare_image_with_inodes(r, None)
}
fn prepare_image_with_inodes(r: &mut Record, inodes: Option<u32>) -> Result<(), String> {
    if r.phase == Phase::Planned {
        let f = match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(r.plan.image())
        {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                // The committed plan owns this exact image namespace.
                // Only an empty private image can precede ImageCreated;
                // never adopt formatted or otherwise populated data.
                let f = private_file(&r.plan.image())?;
                if f.metadata().map_err(|_| "storage_image_unavailable")?.len() != 0 {
                    return Err("storage_unclaimed_image_requires_inspection".into());
                }
                f
            }
            Err(_) => return Err("storage_unclaimed_image_requires_inspection".into()),
        };
        f.sync_all().map_err(|_| "storage_image_sync_failed")?;
        let m = f.metadata().map_err(|_| "storage_image_unavailable")?;
        sync(r.plan.image().parent().ok_or("storage_parent_missing")?)?;
        r.image_identity = Some((m.dev(), m.ino()));
        r.phase = Phase::ImageCreated;
        save(r)?;
    }
    image_identity(r)?;
    if r.phase == Phase::ImageCreated {
        allocate(&r.plan.image(), r.plan.cap)?;
        r.phase = Phase::Allocated;
        save(r)?;
    }
    if fs::metadata(r.plan.image())
        .map_err(|_| "storage_image_unavailable")?
        .len()
        != r.plan.cap
    {
        return Err("storage_image_size_changed".into());
    }
    if r.phase == Phase::Allocated {
        if filesystem(&r.plan.image())?.is_some() {
            return Err("storage_existing_filesystem_will_not_be_formatted".into());
        }
        r.phase = Phase::Formatting;
        save(r)?;
        let inode_count = inodes.map(|n| n.to_string());
        let image_path = r.plan.image();
        let mut arguments = vec![
            "-q",
            "-t",
            "ext4",
            "-E",
            "nodiscard,lazy_itable_init=0,lazy_journal_init=0",
            "-F",
            "-m",
            "0",
            "-U",
            &r.plan.volume,
        ];
        if let Some(count) = inode_count.as_deref() {
            arguments.extend(["-N", count]);
        }
        arguments.push(image_path.to_str().ok_or("storage_path_invalid")?);
        command("/usr/sbin/mkfs.ext4", &arguments)?;
    }
    private_file(&r.plan.image())?
        .sync_all()
        .map_err(|_| "storage_image_sync_failed")?;
    // On an interrupted mkfs, inspect only. Never repeat destructive formatting.
    if filesystem(&r.plan.image())? != Some(("ext4".into(), r.plan.volume.clone())) {
        return Err("storage_format_uncertain_requires_inspection".into());
    }
    if r.phase == Phase::Formatting {
        // mkfs may release unused blocks in a regular image; reserve them again
        // without modifying any existing logical byte or filesystem metadata.
        allocate(&r.plan.image(), r.plan.cap)?;
        r.phase = Phase::Formatted;
        save(r)?;
    }
    if fs::metadata(r.plan.image())
        .map_err(|_| "storage_image_unavailable")?
        .blocks()
        .checked_mul(512)
        .is_none_or(|n| n < r.plan.cap)
    {
        return Err("storage_physical_reservation_lost".into());
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn allocate(p: &Path, cap: u64) -> Result<(), String> {
    use std::os::fd::AsRawFd;
    let f = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(|_| "storage_image_unavailable")?;
    if unsafe { libc::posix_fallocate(f.as_raw_fd(), 0, cap as i64) } != 0 {
        return Err("storage_physical_capacity_unavailable".into());
    }
    f.sync_all().map_err(|_| "storage_image_sync_failed".into())
}
#[cfg(not(target_os = "linux"))]
fn allocate(_: &Path, _: u64) -> Result<(), String> {
    Err("storage_linux_only".into())
}
fn filesystem(p: &Path) -> Result<Option<(String, String)>, String> {
    let output = run_command(
        "/usr/sbin/blkid",
        &[
            "-p",
            "-o",
            "export",
            p.to_str().ok_or("storage_path_invalid")?,
        ],
    )?;
    if output.status.code() == Some(2) {
        return Ok(None);
    }
    if !output.status.success() {
        return Err("storage_filesystem_probe_failed".into());
    }
    let text =
        std::str::from_utf8(&output.stdout).map_err(|_| "storage_filesystem_probe_invalid")?;
    let get = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(str::to_owned)
    };
    Ok(Some((
        get("TYPE=").ok_or("storage_filesystem_type_missing")?,
        get("UUID=").ok_or("storage_filesystem_uuid_missing")?,
    )))
}
fn run_command(path: &str, args: &[&str]) -> Result<std::process::Output, String> {
    let p = Path::new(path);
    let canonical = fs::canonicalize(p).map_err(|_| "storage_required_tool_missing")?;
    let m = fs::metadata(&canonical).map_err(|_| "storage_required_tool_missing")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o022 != 0 {
        return Err("storage_untrusted_tool".into());
    }
    for parent in canonical.ancestors().skip(1) {
        let m = fs::symlink_metadata(parent).map_err(|_| "storage_untrusted_tool")?;
        if m.uid() != 0 || m.mode() & 0o022 != 0 {
            return Err("storage_untrusted_tool".into());
        }
    }
    Command::new(canonical)
        .args(args)
        .env_clear()
        .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
        .env("LANG", "C")
        .stdin(std::process::Stdio::null())
        .output()
        .map_err(|_| "storage_command_unavailable".into())
}
fn command(path: &str, args: &[&str]) -> Result<(), String> {
    if !run_command(path, args)?.status.success() {
        return Err("storage_host_command_failed".into());
    }
    Ok(())
}
fn unit_name(p: &Path) -> String {
    let mut out = String::new();
    for (i, b) in p.to_str().unwrap().trim_matches('/').bytes().enumerate() {
        match b {
            b'/' => out.push('-'),
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'_' | b'.' if !(i == 0 && b == b'.') => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("\\x{b:02x}")),
        }
    }
    out.push_str(".mount");
    out
}
fn units(p: &Plan) -> (String, String) {
    let fsunit = unit_name(&p.mount());
    let fsbody = format!(
        "# Owned by moosuri kiri-storage v1; do not repoint.\n[Unit]\nDescription=Moosuri member filesystem {}\nRequiresMountsFor={}\n\n[Mount]\nWhat={}\nWhere={}\nType=ext4\nOptions=loop,rw,nosuid,nodev,noexec\nTimeoutSec=30\n\n[Install]\nWantedBy=local-fs.target\n",
        p.volume,
        p.root.join("images").display(),
        p.image().display(),
        p.mount().display()
    );
    let ipcbody = format!(
        "# Owned by moosuri kiri-storage v1; do not repoint.\n[Unit]\nDescription=Moosuri member IPC {}\nRequires={fsunit}\nAfter={fsunit}\n\n[Mount]\nWhat={}\nWhere={}\nType=none\nOptions=bind,rw,nosuid,nodev,noexec\nTimeoutSec=30\n\n[Install]\nWantedBy=local-fs.target\n",
        p.account,
        p.mount().join("ipc").display(),
        p.ipc().display()
    );
    (fsbody, ipcbody)
}
fn install_unit(name: &str, body: &str) -> Result<(), String> {
    let root = Path::new("/etc/systemd/system");
    trusted(root, 0, 0o022)?;
    let p = root.join(name);
    if fs::symlink_metadata(&p).is_ok() {
        let mut existing = String::new();
        private_file(&p)?
            .take(8193)
            .read_to_string(&mut existing)
            .map_err(|_| "storage_unit_unavailable")?;
        if existing != body {
            return Err("storage_foreign_unit_conflict".into());
        }
    } else {
        write_new(&p, body.as_bytes())?;
    }
    Ok(())
}
fn enable_unit(name: &str) -> Result<(), String> {
    let root = Path::new("/etc/systemd/system/local-fs.target.wants");
    owned_dir(root, 0o755)?;
    let link = root.join(name);
    let target = Path::new("/etc/systemd/system").join(name);
    match fs::symlink_metadata(&link) {
        Ok(m)
            if m.file_type().is_symlink() && fs::read_link(&link).ok() == Some(target.clone()) => {}
        Ok(_) => return Err("storage_foreign_enable_link".into()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            std::os::unix::fs::symlink(target, &link).map_err(|_| "storage_enable_failed")?;
            sync(root)?;
        }
        Err(_) => return Err("storage_enable_failed".into()),
    }
    Ok(())
}
#[derive(Clone)]
struct Mount {
    device: String,
    root: String,
    options: String,
    kind: String,
}
fn mounted(p: &Path) -> Result<Option<Mount>, String> {
    let content =
        fs::read_to_string("/proc/self/mountinfo").map_err(|_| "storage_mountinfo_unavailable")?;
    parse_mount(&content, p)
}
fn parse_mount(raw: &str, p: &Path) -> Result<Option<Mount>, String> {
    let mut found = None;
    for l in raw.lines() {
        let Some((left, right)) = l.split_once(" - ") else {
            return Err("storage_mountinfo_invalid".into());
        };
        let a: Vec<_> = left.split_whitespace().collect();
        let b: Vec<_> = right.split_whitespace().collect();
        if a.len() < 6 || b.len() < 3 {
            return Err("storage_mountinfo_invalid".into());
        }
        if Some(a[4]) == p.to_str() {
            if found.is_some() {
                return Err("storage_stacked_mounts_refused".into());
            }
            found = Some(Mount {
                device: a[2].into(),
                root: a[3].into(),
                options: a[5].into(),
                kind: b[0].into(),
            });
        }
    }
    Ok(found)
}
fn flags(m: &Mount) -> bool {
    m.kind == "ext4"
        && ["rw", "nosuid", "nodev", "noexec"]
            .iter()
            .all(|v| m.options.split(',').any(|x| x == *v))
}
fn empty_target(p: &Path) -> Result<(), String> {
    if mounted(p)?.is_some() {
        return Ok(());
    }
    owned_dir(p, 0o755)?;
    if fs::read_dir(p)
        .map_err(|_| "storage_mountpoint_unavailable")?
        .next()
        .is_some()
    {
        return Err("storage_mountpoint_not_empty".into());
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn verify_mount(p: &Plan) -> Result<(), String> {
    verify_mount_mode(p, false)
}
#[cfg(target_os = "linux")]
fn verify_mount_mode(p: &Plan, executable: bool) -> Result<(), String> {
    let m = mounted(&p.mount())?.ok_or("storage_mount_missing")?;
    let correct_flags = if executable {
        m.kind == "ext4"
            && ["rw", "nosuid", "nodev"]
                .iter()
                .all(|v| m.options.split(',').any(|x| x == *v))
            && !m.options.split(',').any(|x| x == "noexec")
    } else {
        flags(&m)
    };
    if !correct_flags || m.root != "/" {
        return Err("storage_unexpected_mount".into());
    }
    let backing = fs::read_to_string(format!("/sys/dev/block/{}/loop/backing_file", m.device))
        .map_err(|_| "storage_not_owned_loop_device")?;
    let meta = fs::metadata(backing.trim()).map_err(|_| "storage_backing_file_missing")?;
    let expected = fs::metadata(p.image()).map_err(|_| "storage_image_unavailable")?;
    if (meta.dev(), meta.ino()) != (expected.dev(), expected.ino()) {
        return Err("storage_foreign_backing_device".into());
    }
    trusted(&p.mount(), 0, 0o022)?;
    let path = std::ffi::CString::new(p.mount().as_os_str().as_encoded_bytes())
        .map_err(|_| "storage_path_invalid")?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err("storage_capacity_probe_failed".into());
    }
    let stat = unsafe { stat.assume_init() };
    let total = u64::try_from(u128::from(stat.f_blocks) * u128::from(stat.f_frsize))
        .map_err(|_| "storage_capacity_overflow")?;
    if total == 0 || total > p.cap {
        return Err("storage_capacity_mismatch".into());
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn verify_mount_mode(_: &Plan, _: bool) -> Result<(), String> {
    Err("storage_linux_only".into())
}
#[cfg(not(target_os = "linux"))]
fn verify_mount(_: &Plan) -> Result<(), String> {
    Err("storage_linux_only".into())
}
fn layout(p: &Path, uid: u32) -> Result<(), String> {
    layout_for_ids(p, uid, uid)
}
fn layout_for_ids(p: &Path, uid: u32, gid: u32) -> Result<(), String> {
    if fs::symlink_metadata(p).is_err() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(p)
            .map_err(|_| "storage_layout_create_failed")?;
        std::os::unix::fs::chown(p, Some(uid), Some(gid))
            .map_err(|_| "storage_layout_owner_failed")?;
        sync(p.parent().ok_or("storage_parent_missing")?)?;
    }
    let m = fs::symlink_metadata(p).map_err(|_| "storage_layout_unavailable")?;
    if !m.is_dir()
        || m.uid() != uid
        || m.gid() != gid
        || m.mode() & 0o777 != 0o700
        || fs::canonicalize(p).ok().as_deref() != Some(p)
        || m.dev()
            != fs::metadata(p.parent().ok_or("storage_parent_missing")?)
                .map_err(|_| "storage_parent_unavailable")?
                .dev()
        || mounted(p)?.is_some()
    {
        return Err("storage_foreign_layout".into());
    }
    sync(p)?;
    Ok(())
}
fn verify_ipc(p: &Plan) -> Result<(), String> {
    let m = mounted(&p.ipc())?.ok_or("storage_ipc_mount_missing")?;
    let volume = mounted(&p.mount())?.ok_or("storage_mount_missing")?;
    let source =
        fs::metadata(p.mount().join("ipc")).map_err(|_| "storage_ipc_source_unavailable")?;
    let target = fs::metadata(p.ipc()).map_err(|_| "storage_ipc_target_unavailable")?;
    if !flags(&m)
        || m.root != "/ipc"
        || m.device != volume.device
        || (source.dev(), source.ino()) != (target.dev(), target.ino())
        || target.uid() != p.uid
        || target.gid() != p.uid
        || target.mode() & 0o777 != 0o700
    {
        return Err("storage_ipc_binding_mismatch".into());
    }
    Ok(())
}
#[cfg(target_os = "linux")]
fn check_traversal(uid: u32, p: &Path) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let mut child = Command::new("/usr/bin/test");
    child
        .arg("-x")
        .arg(p)
        .current_dir("/")
        .env_clear()
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    // Only credential syscalls in the post-fork child, then a fixed system tool.
    unsafe {
        child.pre_exec(move || {
            if libc::setgroups(0, std::ptr::null()) != 0
                || libc::setresgid(uid, uid, uid) != 0
                || libc::setresuid(uid, uid, uid) != 0
                || libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) != 0
            {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    if !child
        .status()
        .map_err(|_| "storage_member_traversal_check_failed")?
        .success()
    {
        return Err("storage_member_cannot_traverse_mount_hierarchy".into());
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn check_traversal(_: u32, _: &Path) -> Result<(), String> {
    Err("storage_linux_only".into())
}
pub(crate) fn unused_uid(uid: u32) -> Result<(), String> {
    let mut buf = vec![0u8; 65536];
    let mut user = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut ptr = std::ptr::null_mut();
    if unsafe {
        libc::getpwuid_r(
            uid,
            user.as_mut_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut ptr,
        )
    } != 0
        || !ptr.is_null()
    {
        return Err("storage_uid_is_host_user".into());
    }
    let mut group = std::mem::MaybeUninit::<libc::group>::uninit();
    let mut ptr = std::ptr::null_mut();
    if unsafe {
        libc::getgrgid_r(
            uid,
            group.as_mut_ptr(),
            buf.as_mut_ptr().cast(),
            buf.len(),
            &mut ptr,
        )
    } != 0
        || !ptr.is_null()
    {
        return Err("storage_gid_is_host_group".into());
    }
    Ok(())
}
#[cfg(test)]
mod tests;
