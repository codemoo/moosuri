use super::*;
use crate::{Result as StoreResult, StoreError};
use fs2::FileExt;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};

pub(crate) struct RuntimeAdmission {
    pub root: PathBuf,
    pub lock: File,
}
fn store_error(error: Error) -> StoreError {
    match error {
        Error::Busy => StoreError::Busy,
        Error::Integrity | Error::UnsupportedInstallation => StoreError::Corrupt,
        Error::InvalidInput => StoreError::InvalidInput,
        _ => StoreError::Unavailable,
    }
}
pub(crate) fn controls_present(root: &Path) -> Result<bool> {
    for name in [MARKER, STAGING] {
        match fs::symlink_metadata(root.join(name)) {
            Ok(_) => return Ok(true),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(_) => return Err(Error::Unavailable),
        }
    }
    Ok(false)
}
/// Existing roots are inspected under their existing lock without chmod/create.
/// The lock is transferred directly into Store; it never opens a preflight gap.
pub(crate) fn runtime_admission(root: &Path) -> StoreResult<Option<RuntimeAdmission>> {
    runtime_inner(root, None).map_err(store_error)
}
pub(crate) fn personal_admission(
    root: &Path,
    owner: crate::personal_volume::VolumeOwner,
    initialize: bool,
) -> StoreResult<Option<RuntimeAdmission>> {
    runtime_inner(root, Some((owner, initialize))).map_err(store_error)
}
fn runtime_inner(
    root: &Path,
    personal: Option<(crate::personal_volume::VolumeOwner, bool)>,
) -> Result<Option<RuntimeAdmission>> {
    super::restore::reject_stage(root)?;
    if controls_present(root)? {
        return Err(Error::Sealed);
    }
    // Create missing directories without chmod of any existing root. Even an
    // initially absent/empty path needs a fresh admission under its writer lock.
    let root = crate::files::unprepared_dir(root).map_err(|_| Error::InvalidInput)?;
    let root = runtime_directory(&root, personal.is_some())?;
    let lock_path = root.join("writer.lock");
    let lock = match fs::symlink_metadata(&lock_path) {
        Ok(_) => inspection_file(&lock_path, 0)?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            // A sealed legacy root missing its lock must remain entirely unchanged.
            inspect_if_present(&root, personal)?;
            match OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&lock_path)
            {
                Ok(f) => f,
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                    inspection_file(&lock_path, 0)?
                }
                Err(_) => return Err(Error::Unavailable),
            }
        }
        Err(_) => return Err(Error::Unavailable),
    };
    lock.try_lock_exclusive().map_err(|e| {
        if e.kind() == io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            Error::Unavailable
        }
    })?;
    // This is the authoritative late check. No chmod, private_file, journal or
    // live SQLite opening is allowed before it, including callers holding None.
    inspect_if_present(&root, personal)?;
    sync_parent_before_admission(&root)?;
    crate::push::validate_optional_file(&root).map_err(|_| Error::Integrity)?;
    crate::files::prepare_dir(&root).map_err(|_| Error::InvalidInput)?;
    lock.set_permissions(fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::Unavailable)?;
    Ok(Some(RuntimeAdmission { root, lock }))
}
/// Caller retains the root's exclusive writer lock. A prior restore may have
/// renamed this complete root but failed before its parent was synced. Admit no
/// new authority or writes until the directory entry is durably published.
pub(crate) fn sync_parent_before_admission(root: &Path) -> Result<()> {
    fault(root, "admission_parent_sync")?;
    files::sync_dir(root.parent().ok_or(Error::InvalidInput)?)
}
fn inspect_if_present(
    root: &Path,
    personal: Option<(crate::personal_volume::VolumeOwner, bool)>,
) -> Result<()> {
    if controls_present(root)? {
        return Err(Error::Sealed);
    }
    match fs::symlink_metadata(root.join("assistant.sqlite")) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => allow_empty_store(root, personal),
        Ok(m) if m.is_file() && m.len() == 0 => allow_empty_store(root, personal),
        Ok(_) => {
            let (_temp, conn) = inspect(root)?;
            require_active(&conn, root)?;
            let bound: bool = conn.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='personal_runtime_identity' AND type='table')", [], |row|row.get(0))?;
            match personal {
                Some((owner, _)) if bound => owner
                    .check_identity(&conn, None)
                    .map_err(|_| Error::Integrity),
                // An interrupted first initialization can have completed schema creation
                // before the atomic owner/account transaction. An occupied DB is never adopted.
                Some((_, true)) if !bound => {
                    let empty:bool=conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM accounts) AND NOT EXISTS(SELECT 1 FROM tenants)",[],|row|row.get(0))?;
                    if empty { Ok(()) } else { Err(Error::Integrity) }
                }
                None if !bound => Ok(()),
                _ => Err(Error::Integrity),
            }
        }
        Err(_) => Err(Error::Unavailable),
    }
}

fn allow_empty_store(
    root: &Path,
    personal: Option<(crate::personal_volume::VolumeOwner, bool)>,
) -> Result<()> {
    let Some((_, initializing)) = personal else {
        return Ok(());
    };
    if !initializing {
        return Err(Error::Integrity);
    }
    // A new initialization can resume before its first DB commit. Preserved
    // authority, keys or a nonempty WAL alongside a lost DB require recovery.
    for entry in fs::read_dir(root).map_err(|_| Error::Unavailable)? {
        let entry = entry.map_err(|_| Error::Unavailable)?;
        let m = fs::symlink_metadata(entry.path()).map_err(|_| Error::Unavailable)?;
        if !matches!(
            entry.file_name().to_str(),
            Some(
                "writer.lock"
                    | "assistant.sqlite"
                    | "assistant.sqlite-wal"
                    | "assistant.sqlite-shm"
            )
        ) || !m.is_file()
            || m.len() != 0
        {
            return Err(Error::Integrity);
        }
    }
    Ok(())
}

fn runtime_directory(path: &Path, personal: bool) -> Result<PathBuf> {
    use std::path::Component;
    let path = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| Error::Unavailable)?
            .join(path)
    };
    let mut current = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir => return Err(Error::InvalidInput),
            Component::CurDir => continue,
            _ => current.push(part),
        }
        let m = fs::symlink_metadata(&current).map_err(|_| Error::InvalidInput)?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err(Error::InvalidInput);
        }
    }
    const NAMES: &[&str] = &[
        "provider-secrets.key",
        crate::push::KEY_FILE,
        "knowledge-invalidations.journal",
        "destructive-intents.journal",
        "writer.lock",
        "assistant.sqlite",
        "auth-epochs.journal",
        "assistant.sqlite-wal",
        "assistant.sqlite-shm",
        MARKER,
        STAGING,
    ];
    for entry in fs::read_dir(&current).map_err(|_| Error::Unavailable)? {
        let entry = entry.map_err(|_| Error::Unavailable)?;
        if personal && entry.file_name() == crate::personal_volume::GWS_DIRECTORY {
            let meta = fs::symlink_metadata(entry.path()).map_err(|_| Error::InvalidInput)?;
            if !meta.is_dir()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o077 != 0
            {
                return Err(Error::InvalidInput);
            }
            continue;
        }
        if !NAMES.iter().any(|n| entry.file_name() == *n) {
            return Err(Error::InvalidInput);
        }
    }
    let m = fs::metadata(&current).map_err(|_| Error::Unavailable)?;
    if current.parent().is_none() || m.uid() != unsafe { libc::geteuid() } {
        return Err(Error::InvalidInput);
    }
    Ok(current)
}
fn inspection_file(path: &Path, cap: u64) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Error::Unavailable)?;
    let m = f.metadata().map_err(|_| Error::Unavailable)?;
    if !m.is_file() || m.uid() != unsafe { libc::geteuid() } || m.nlink() != 1 || m.len() > cap {
        return Err(Error::Integrity);
    }
    Ok(f)
}
fn inspection_copy(source: &Path, dest: &Path, cap: u64) -> Result<()> {
    let f = inspection_file(source, cap)?;
    let size = f.metadata().map_err(|_| Error::Unavailable)?.len();
    let mut input = f.take(cap + 1);
    let mut output = files::create_file(dest)?;
    let copied = io::copy(&mut input, &mut output).map_err(|_| Error::Unavailable)?;
    if copied != size || copied > cap {
        return Err(Error::LimitExceeded);
    }
    output.sync_all().map_err(|_| Error::Unavailable)
}
pub(crate) fn inspect(root: &Path) -> Result<(files::Temporary, Connection)> {
    let temp = files::Temporary::new(root)?;
    inspection_copy(
        &root.join("assistant.sqlite"),
        &temp.0.join("assistant.sqlite"),
        backup::DB_LIMIT,
    )?;
    match fs::symlink_metadata(root.join("assistant.sqlite-wal")) {
        Ok(_) => inspection_copy(
            &root.join("assistant.sqlite-wal"),
            &temp.0.join("assistant.sqlite-wal"),
            backup::DB_LIMIT,
        )?,
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(_) => return Err(Error::Unavailable),
    }
    let conn = Connection::open_with_flags(
        temp.0.join("assistant.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch("PRAGMA trusted_schema=OFF;")?;
    Ok((temp, conn))
}
pub(crate) fn require_active(conn: &Connection, root: &Path) -> Result<()> {
    if controls_present(root)? {
        return Err(Error::Sealed);
    }
    let schema: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if schema > crate::CURRENT_SCHEMA_VERSION {
        return Err(Error::UnsupportedInstallation);
    }
    let has_cutover: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_schema WHERE name='installation_cutover')",
        [],
        |r| r.get(0),
    )?;
    if (schema >= SCHEMA || has_cutover) && !matches!(state(conn)?, State::Active) {
        return Err(Error::Sealed);
    }
    Ok(())
}
