//! Root-only connection records and the owner-wide lock. A record is the only
//! accepted evidence: it is authored by root from root-launched checks and lives
//! in the private control directory, never in a worker-writable path.
use crate::engine_login::Owner;
use crate::installation::claude_runtime::{CONTROL, PreparedRuntime};
use crate::installation::kiri::host::{self, Layout, Result};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    ffi::OsStr,
    fs::{self, File, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path, PathBuf},
};
use uuid::Uuid;

const CAP: u64 = 128 * 1024;
const STORAGE_ROOT: &str = "/var/lib/moosuri-claude-fs";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    /// Attempt recorded; evidence/config not complete.
    Reserved,
    /// Evidence and protected config exist; no login process was ever issued.
    Prepared,
    /// A login command was handed out; credentials may exist from here on.
    LoginIssued,
    Activating,
    /// Worker answered, but no runtime-model configuration was published yet.
    WorkerReady,
    /// The gateway confirmed publication under its installation registry lock.
    Active,
    /// Cleanup was requested and must finish before any other setup.
    Cleanup,
    /// Environment retired; deleting remaining root-owned inputs is replayable.
    RemovingFiles,
    /// Fresh failed attempt fully cleaned; runtime evidence stays reusable.
    Retired,
}
impl Phase {
    pub(super) fn credentials_possible(self) -> bool {
        !matches!(self, Self::Reserved | Self::Prepared)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Storage {
    pub auth_root: PathBuf,
    pub scratch_root: PathBuf,
    pub filesystem_unit: String,
    pub capacity_bytes: u64,
    pub maximum_inodes: u64,
}
impl Storage {
    pub(super) fn from_prepared(p: crate::kiri_storage::claude::Prepared) -> Result<Self> {
        let storage = Self {
            auth_root: p.auth_root,
            scratch_root: p.scratch_root,
            filesystem_unit: p.filesystem_unit,
            capacity_bytes: p.capacity_bytes,
            maximum_inodes: p.maximum_inodes,
        };
        storage.validate()?;
        Ok(storage)
    }
    pub(super) fn validate(&self) -> Result<()> {
        let plain = |p: &Path| {
            p.is_absolute()
                && p.starts_with(STORAGE_ROOT)
                && !p.as_os_str().as_encoded_bytes().contains(&b',')
                && p.components()
                    .all(|c| !matches!(c, Component::ParentDir | Component::CurDir))
        };
        if !plain(&self.auth_root)
            || !plain(&self.scratch_root)
            || self.auth_root.file_name() != Some(OsStr::new("auth"))
            || self.scratch_root.file_name() != Some(OsStr::new("scratch"))
            || self.auth_root.parent() != self.scratch_root.parent()
            || !(1..=128 * 1024 * 1024).contains(&self.capacity_bytes)
            || !(1..=32768).contains(&self.maximum_inodes)
            || !super::unit::safe_mount_unit(&self.filesystem_unit)
        {
            return Err("claude_connection_storage_invalid".into());
        }
        Ok(())
    }
}

/// Everything observed by root before any credential could exist.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Evidence {
    pub runtime: PreparedRuntime,
    pub engine_stack: Vec<super::super::prereq::PinnedFile>,
    pub storage: Storage,
    /// Exact credential-free candidate the native check was launched with.
    pub candidate_sha256: String,
    /// Canonical admitted profile JSON (byte-exact worker profile file).
    pub profile_json: String,
    pub profile_digest: String,
    pub receipt_json: String,
    pub receipt_sha256: String,
    pub kernel_release: String,
    pub boot_id: String,
}
impl Evidence {
    /// Same artifacts, identity, storage and kernel as observed again now.
    pub(super) fn matches(
        &self,
        runtime: &PreparedRuntime,
        storage: &Storage,
        kernel: &str,
    ) -> bool {
        self.runtime == *runtime && self.storage == *storage && self.kernel_release == kernel
    }
}

/// SHA-256 of each protected worker file written by root.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Hashes {
    pub profile: String,
    pub email: String,
    pub worker: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Attempt {
    pub request_id: Uuid,
    /// Official login environment (random UUIDv4, never reserved v8).
    pub environment_id: Uuid,
    pub email_sha256: String,
    pub model: String,
    pub effort: Option<String>,
    pub hosting_terms_accepted: bool,
    pub phase: Phase,
    pub files: Option<Hashes>,
    pub unit_sha256: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Record {
    pub version: u32,
    pub owner: Owner,
    /// Request id of the one prepared runtime (identity, service, storage).
    pub runtime_request_id: Uuid,
    pub attempt: Attempt,
    pub evidence: Option<Evidence>,
}
impl Record {
    pub(super) fn valid(&self) -> bool {
        let (a, o) = (&self.attempt, &self.owner);
        self.version == 1
            && !self.runtime_request_id.is_nil()
            && !a.request_id.is_nil()
            && a.environment_id.get_version_num() == 4
            && o.generation > 0
            && [o.account_id, o.tenant_id, o.volume_id]
                .iter()
                .all(|id| !id.is_nil())
            && host::is_hex(&a.email_sha256, 64)
            && (!matches!(
                a.phase,
                Phase::Prepared
                    | Phase::LoginIssued
                    | Phase::Activating
                    | Phase::WorkerReady
                    | Phase::Active
            ) || (self.evidence.is_some() && a.files.is_some()))
    }
}

pub(super) fn owner_key(o: &Owner) -> String {
    let mut h = Sha256::new();
    h.update(b"moosuri:claude:connection-owner:v1\n");
    h.update(format!(
        "{}\n{}\n{}\n{}",
        o.account_id.hyphenated(),
        o.tenant_id.hyphenated(),
        o.volume_id.hyphenated(),
        o.generation
    ));
    host::hex(&h.finalize())
}

pub(super) struct Store {
    layout: Layout,
    dir: PathBuf,
}
impl Store {
    pub(super) fn new(layout: &Layout) -> Self {
        Self {
            layout: layout.clone(),
            dir: layout.at(&format!("{CONTROL}/connections")),
        }
    }
    /// Trusted root-only directories (the control root is shared with receipts).
    pub(super) fn ensure(&self) -> Result<()> {
        self.layout.ensure_dir(&self.layout.at(CONTROL), 0o700)?;
        self.layout.ensure_dir(&self.dir, 0o700)
    }
    fn path(&self, owner: &Owner, ext: &str) -> PathBuf {
        self.dir.join(format!("{}.{ext}", owner_key(owner)))
    }
    /// Nonblocking whole-owner lease. It serializes prepare, admission, login
    /// and cleanup across processes; dropping the file releases it.
    pub(super) fn lock(&self, owner: &Owner) -> Result<File> {
        let path = self.path(owner, "lock");
        self.layout.safe_ancestors(&path)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_| "claude_connection_lock_unavailable")?;
        let m = lock
            .metadata()
            .map_err(|_| "claude_connection_lock_unavailable")?;
        if !m.is_file() || m.nlink() != 1 || m.uid() != host::euid() || m.mode() & 0o077 != 0 {
            return Err("claude_connection_lock_foreign".into());
        }
        lock.try_lock_exclusive()
            .map_err(|_| "claude_connection_busy")?;
        Ok(lock)
    }
    fn read(&self, path: &Path) -> Result<Vec<u8>> {
        let (mut file, _) = host::open_regular(path, CAP)?;
        let m = file
            .metadata()
            .map_err(|_| "claude_connection_record_unavailable")?;
        if m.mode() & 0o077 != 0 {
            return Err("claude_connection_record_conflict".into());
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(CAP + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "claude_connection_record_unavailable")?;
        if bytes.len() as u64 > CAP {
            return Err("claude_connection_record_conflict".into());
        }
        Ok(bytes)
    }
    /// `None` only when this owner never started. Anything foreign, for a
    /// different owner, or structurally invalid fails closed.
    pub(super) fn load(&self, owner: &Owner) -> Result<Option<Record>> {
        let path = self.path(owner, "json");
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("claude_connection_record_unavailable".into()),
        }
        let record: Record = serde_json::from_slice(&self.read(&path)?)
            .map_err(|_| "claude_connection_record_conflict")?;
        if record.owner != *owner || !record.valid() {
            return Err("claude_connection_record_conflict".into());
        }
        Ok(Some(record))
    }
    /// Overwrites only the exact bytes currently on disk (held under the lock).
    pub(super) fn save(&self, record: &Record) -> Result<()> {
        if !record.valid() {
            return Err("claude_connection_record_invalid".into());
        }
        let path = self.path(&record.owner, "json");
        let previous = match fs::symlink_metadata(&path) {
            Ok(_) => vec![host::sha256_bytes(&self.read(&path)?)],
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(_) => return Err("claude_connection_record_unavailable".into()),
        };
        let bytes = serde_json::to_vec_pretty(record)
            .map_err(|_| "claude_connection_record_unserializable")?;
        self.layout.write_file(&path, &bytes, 0o600, &previous)
    }
}
