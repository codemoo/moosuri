//! Runtime-only signing authority. Stopped export/inventory never calls ensure.
use super::*;
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    path::PathBuf,
};
pub const KEY_FILE: &str = "web-push-vapid.key";
// Remove a partial key on ordinary I/O failure. A process crash can leave a
// private orphan; strict root inventory must reject that unexpected file.
struct PendingKey(PathBuf);
impl Drop for PendingKey {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
fn path(conn: &Connection) -> Result<PathBuf> {
    Ok(Path::new(conn.path().ok_or(StoreError::Unavailable)?)
        .parent()
        .ok_or(StoreError::Unavailable)?
        .join(KEY_FILE))
}
fn read(path: &Path) -> Result<Zeroizing<Vec<u8>>> {
    let metadata = fs::symlink_metadata(path).map_err(|_| StoreError::Unavailable)?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() != 32 {
        return Err(StoreError::Unavailable);
    }
    #[cfg(unix)]
    if metadata.uid() != unsafe { libc::geteuid() } || metadata.permissions().mode() & 0o077 != 0 {
        return Err(StoreError::Unavailable);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(path).map_err(|_| StoreError::Unavailable)?;
    let opened = file.metadata().map_err(|_| StoreError::Unavailable)?;
    if !opened.is_file() || opened.len() != 32 {
        return Err(StoreError::Unavailable);
    }
    #[cfg(unix)]
    if opened.uid() != unsafe { libc::geteuid() }
        || opened.permissions().mode() & 0o077 != 0
        || opened.nlink() != 1
        || opened.dev() != metadata.dev()
        || opened.ino() != metadata.ino()
    {
        return Err(StoreError::Unavailable);
    }
    let mut bytes = Zeroizing::new(vec![]);
    file.take(33)
        .read_to_end(&mut bytes)
        .map_err(|_| StoreError::Unavailable)?;
    if bytes.len() != 32 {
        return Err(StoreError::Unavailable);
    }
    moosuri_connectors::push::public_vapid(&bytes).map_err(|_| StoreError::Unavailable)?;
    Ok(bytes)
}
/// Optional strict-inventory member only: do not archive/copy this authority.
pub fn validate_optional_file(root: &Path) -> Result<()> {
    let path = root.join(KEY_FILE);
    match fs::symlink_metadata(&path) {
        Ok(_) => {
            read(&path)?;
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(StoreError::Unavailable),
    }
    Ok(())
}
pub fn ensure(conn: &Connection, contact: &str) -> Result<String> {
    let u = url::Url::parse(contact).map_err(|_| StoreError::InvalidInput)?;
    if u.scheme() != "https" || u.host_str().is_none() {
        return Err(StoreError::InvalidInput);
    }
    let old: Option<(String, bool)> = conn
        .query_row(
            "SELECT public_key,retired FROM push_vapid_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let path = path(conn)?;
    if let Some((expected, false)) = &old {
        let bytes = read(&path)?;
        let found =
            moosuri_connectors::push::public_vapid(&bytes).map_err(|_| StoreError::Unavailable)?;
        if &found != expected {
            return Err(StoreError::Unavailable);
        }
        return Ok(found);
    }
    let bytes = if old.is_none() && path.exists() {
        read(&path)?
    } else {
        if fs::symlink_metadata(&path).is_ok() {
            read(&path)?;
        }
        let (bytes, _) =
            moosuri_connectors::push::generate_vapid().map_err(|_| StoreError::Unavailable)?;
        let temporary_path = path.with_file_name(format!(".web-push-vapid-{}.tmp", Uuid::new_v4()));
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600).custom_flags(libc::O_NOFOLLOW);
        let mut file = options
            .open(&temporary_path)
            .map_err(|_| StoreError::Unavailable)?;
        let temporary = PendingKey(temporary_path);
        file.write_all(&bytes)
            .and_then(|_| file.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        drop(file);
        fs::rename(&temporary.0, &path).map_err(|_| StoreError::Unavailable)?;
        File::open(path.parent().ok_or(StoreError::Unavailable)?)
            .and_then(|f| f.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        bytes
    };
    let public =
        moosuri_connectors::push::public_vapid(&bytes).map_err(|_| StoreError::Unavailable)?;
    conn.execute("INSERT INTO push_vapid_meta VALUES(1,?1,?2,0) ON CONFLICT(singleton) DO UPDATE SET public_key=?1,contact=?2,retired=0",params![public,contact])?;
    Ok(public)
}
pub(super) fn private(conn: &Connection) -> Result<(Zeroizing<Vec<u8>>, String)> {
    let (public, contact): (String, String) = conn
        .query_row(
            "SELECT public_key,contact FROM push_vapid_meta WHERE singleton=1 AND retired=0",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::Unavailable)?;
    let bytes = read(&path(conn)?)?;
    if moosuri_connectors::push::public_vapid(&bytes).map_err(|_| StoreError::Unavailable)?
        != public
    {
        return Err(StoreError::Unavailable);
    }
    Ok((bytes, contact))
}
