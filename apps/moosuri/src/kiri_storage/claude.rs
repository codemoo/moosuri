//! Bounded persistent storage for one prepared Claude worker. Reuses the same
//! owned-image/format/mount recovery as member storage, in a separate fixed root.
//! No credentials are read and storage readiness grants no model/login binding.
use super::*;
use crate::engine_login::Owner;

const ROOT: &str = "/var/lib/moosuri-claude-fs";
const CAPACITY: u64 = 128 * 1024 * 1024;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Claim {
    version: u32,
    request_id: Uuid,
    owner: Owner,
    uid: u32,
    gid: u32,
}
impl Claim {
    fn valid(&self) -> bool {
        self.version == 1
            && !self.request_id.is_nil()
            && self.owner.generation > 0
            && [
                self.owner.account_id,
                self.owner.tenant_id,
                self.owner.volume_id,
            ]
            .iter()
            .all(|id| !id.is_nil())
            && (1..20000).contains(&self.uid)
            && (1..20000).contains(&self.gid)
    }
    fn root(&self) -> PathBuf {
        PathBuf::from(ROOT)
    }
    fn plan(&self) -> Plan {
        Plan {
            v: 1,
            root: self.root(),
            request: self.request_id,
            account: self.owner.account_id.to_string(),
            tenant: self.owner.tenant_id.to_string(),
            // This is the authentication filesystem's UUID. The member's
            // separate volume stays pinned in the private Claim above.
            volume: self.request_id.to_string(),
            generation: self.owner.generation,
            uid: self.uid,
            cap: CAPACITY,
        }
    }
}

#[derive(Clone, Serialize)]
pub(crate) struct Prepared {
    pub(crate) auth_root: PathBuf,
    pub(crate) scratch_root: PathBuf,
    pub(crate) filesystem_unit: String,
    pub(crate) capacity_bytes: u64,
    pub(crate) maximum_inodes: u64,
}

fn claim(root: &Path, wanted: &Claim) -> Result<(), String> {
    let path = root
        .join("assignments")
        .join(format!("{}.json", wanted.request_id));
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            let mut bytes = Vec::new();
            private_file(&path)?
                .take(4097)
                .read_to_end(&mut bytes)
                .map_err(|_| "claude_storage_claim_unavailable")?;
            if bytes.len() > 4096
                || serde_json::from_slice::<Claim>(&bytes).ok().as_ref() != Some(wanted)
            {
                return Err("claude_storage_claim_changed".into());
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // Never attach a fresh claim to a filesystem already prepared by
            // another path through the storage helper.
            if wanted
                .plan()
                .record()
                .try_exists()
                .map_err(|_| "claude_storage_claim_unavailable")?
            {
                return Err("claude_storage_unclaimed_filesystem".into());
            }
            write_new(
                &path,
                &serde_json::to_vec(wanted).map_err(|_| "claude_storage_claim_invalid")?,
            )?;
        }
        Err(_) => return Err("claude_storage_claim_unavailable".into()),
    }
    Ok(())
}

/// Host preparer only. Owner/worker provenance must already have been checked
/// against its root-owned preparation receipt; no web path or UID is accepted.
pub(crate) fn prepare(
    request_id: Uuid,
    owner: Owner,
    uid: u32,
    gid: u32,
) -> Result<Prepared, String> {
    if !cfg!(target_os = "linux") || unsafe { libc::geteuid() } != 0 {
        return Err("claude_storage_linux_root_required".into());
    }
    let wanted = Claim {
        version: 1,
        request_id,
        owner,
        uid,
        gid,
    };
    if !wanted.valid() {
        return Err("claude_storage_claim_invalid".into());
    }
    let root = wanted.root();
    // The directory becomes traversable after initialize_root writes its
    // ownership marker. Preserve that established mode on replay.
    if fs::symlink_metadata(&root).is_err() {
        owned_dir(&root, 0o700)?;
    }
    trusted(&root, 0, 0o022)?;
    let lock = File::open(&root).map_err(|_| "claude_storage_root_unavailable")?;
    lock.lock_exclusive().map_err(|_| "claude_storage_busy")?;
    initialize_root(&root)?;
    claim(&root, &wanted)?;
    let plan = wanted.plan();
    let mut record = load_or_plan(&plan, false)?;
    // Pin the count at format time; host mke2fs.conf must not choose an
    // unbounded inode allocation that makes every later replay fail.
    prepare_image_with_inodes(&mut record, Some(32768))?;
    let target = plan.mount();
    empty_target(&target)?;
    let filesystem_unit = unit_name(&target);
    let (body, _) = units(&plan);
    let body = body.replace(
        "Moosuri member filesystem",
        "Moosuri Claude authentication filesystem",
    );
    install_unit(&filesystem_unit, &body)?;
    command("/usr/bin/systemctl", &["daemon-reload"])?;
    if mounted(&target)?.is_none() {
        command("/usr/bin/systemctl", &["start", &filesystem_unit])?;
    }
    verify_mount(&plan)?;
    let maximum_inodes = bounded_inodes(&target)?;
    for name in ["auth", "scratch"] {
        layout_for_ids(&target.join(name), uid, gid)?;
        check_traversal(uid, &target.join(name))?;
    }
    enable_unit(&filesystem_unit)?;
    command("/usr/bin/systemctl", &["daemon-reload"])?;
    record.phase = Phase::Ready;
    save(&record)?;
    Ok(Prepared {
        auth_root: target.join("auth"),
        scratch_root: target.join("scratch"),
        filesystem_unit,
        capacity_bytes: CAPACITY,
        maximum_inodes,
    })
}

#[cfg(target_os = "linux")]
fn bounded_inodes(path: &Path) -> Result<u64, String> {
    let path = std::ffi::CString::new(path.as_os_str().as_encoded_bytes())
        .map_err(|_| "claude_storage_path_invalid")?;
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err("claude_storage_inode_probe_failed".into());
    }
    let count = unsafe { stat.assume_init() }.f_files as u64;
    if count == 0 || count > 32768 {
        return Err("claude_storage_inode_bound_invalid".into());
    }
    Ok(count)
}
#[cfg(not(target_os = "linux"))]
fn bounded_inodes(_: &Path) -> Result<u64, String> {
    Err("claude_storage_linux_root_required".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn sample() -> Claim {
        Claim {
            version: 1,
            request_id: Uuid::new_v4(),
            uid: 801,
            gid: 802,
            owner: Owner {
                account_id: Uuid::new_v4(),
                tenant_id: Uuid::new_v4(),
                volume_id: Uuid::new_v4(),
                generation: 1,
            },
        }
    }
    #[test]
    fn worker_storage_never_accepts_root_or_member_pool_identities() {
        let mut c = sample();
        assert!(c.valid());
        c.uid = 0;
        assert!(!c.valid());
        c.uid = 20000;
        assert!(!c.valid());
        c.uid = 801;
        c.gid = 20000;
        assert!(!c.valid());
        c.gid = 802;
        c.owner.volume_id = Uuid::nil();
        assert!(!c.valid());
        c.owner.volume_id = Uuid::new_v4();
        c.owner.generation = 0;
        assert!(!c.valid());
    }
    #[test]
    fn independent_requests_have_independent_filesystems_with_the_full_owner_pin() {
        let a = sample();
        let mut b = a.clone();
        b.request_id = Uuid::new_v4();
        assert_ne!(a.plan().mount(), b.plan().mount());
        assert!(a.plan() != b.plan());
        assert_eq!(a.plan().volume, a.request_id.to_string());
        // The real bridge must fit Linux sockaddr_un, including its random
        // tempfile suffix; a duplicated UUID formerly made this142 bytes.
        let socket = a.plan().mount().join("scratch/claude-123456/proxy.sock");
        assert!(socket.as_os_str().len() <= 107);
        assert_eq!(a.plan().cap, CAPACITY);
        let (unit, _) = units(&a.plan());
        assert!(unit.contains("Options=loop,rw,nosuid,nodev,noexec"));
    }
    #[test]
    fn gid_and_owner_changes_are_not_exact_claim_replays() {
        let a = sample();
        let mut b = a.clone();
        b.gid += 1;
        assert!(a != b);
        b = a.clone();
        b.owner.tenant_id = Uuid::new_v4();
        assert!(a != b);
        let mut value = serde_json::to_value(a).unwrap();
        value["path"] = serde_json::json!("/arbitrary");
        assert!(serde_json::from_value::<Claim>(value).is_err());
    }
}
