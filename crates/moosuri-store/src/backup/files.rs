use super::*;
use fs2::FileExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::Component;

pub(super) struct Temporary(pub PathBuf);
impl Temporary {
    pub fn new(source_root: &Path) -> Result<Self> {
        let parent = std::env::temp_dir()
            .canonicalize()
            .map_err(|_| Error::Unavailable)?;
        if parent.starts_with(source_root) {
            return Err(Error::InvalidInput);
        }
        let path = parent.join(format!("moosuri-export-{}", Uuid::new_v4()));
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .map_err(|_| Error::Unavailable)?;
        Ok(Self(path))
    }
}
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

pub(super) fn directory(path: &Path) -> Result<PathBuf> {
    if !path.is_absolute() {
        return Err(Error::InvalidInput);
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        match part {
            Component::ParentDir | Component::CurDir => return Err(Error::InvalidInput),
            _ => current.push(part),
        }
        let info = fs::symlink_metadata(&current).map_err(|_| Error::InvalidInput)?;
        if info.file_type().is_symlink() || !info.is_dir() {
            return Err(Error::InvalidInput);
        }
    }
    let info = fs::metadata(&current).map_err(|_| Error::InvalidInput)?;
    if current.parent().is_none()
        || info.uid() != unsafe { libc::geteuid() }
        || info.permissions().mode() & 0o077 != 0
    {
        return Err(Error::InvalidInput);
    }
    Ok(current)
}
pub(super) fn read_file(path: &Path, cap: u64) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Error::InvalidInput)?;
    let info = file.metadata().map_err(|_| Error::Unavailable)?;
    if !info.is_file()
        || info.nlink() != 1
        || info.uid() != unsafe { libc::geteuid() }
        || info.permissions().mode() & 0o077 != 0
    {
        return Err(Error::InvalidInput);
    }
    if info.len() > cap {
        return Err(Error::LimitExceeded);
    }
    Ok(file)
}
pub(super) fn create_file(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(|_| Error::Unavailable)
}
pub(super) fn copy_file(source: &Path, dest: &Path, cap: u64) -> Result<()> {
    let input = read_file(source, cap)?;
    let expected = input.metadata().map_err(|_| Error::Unavailable)?.len();
    let mut input = input.take(cap + 1);
    let mut output = create_file(dest)?;
    let copied = io::copy(&mut input, &mut output).map_err(|_| Error::Unavailable)?;
    if copied != expected || copied > cap {
        return Err(Error::LimitExceeded);
    }
    output.sync_all().map_err(|_| Error::Unavailable)
}
pub(super) fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::Unavailable)
}
pub(super) fn stopped_lock(root: &Path) -> Result<(PathBuf, File)> {
    let root = directory(root)?;
    // Reject unrelated application data instead of silently omitting unknown files.
    const NAMES: &[&str] = &[
        "writer.lock",
        "assistant.sqlite",
        "assistant.sqlite-wal",
        "assistant.sqlite-shm",
        "auth-epochs.journal",
        "knowledge-invalidations.journal",
        "destructive-intents.journal",
        "provider-secrets.key",
    ];
    for entry in fs::read_dir(&root).map_err(|_| Error::Unavailable)? {
        let entry = entry.map_err(|_| Error::Unavailable)?;
        if !NAMES.iter().any(|name| entry.file_name() == *name) {
            return Err(Error::UnsupportedInstallation);
        }
    }
    let lock = read_file(&root.join("writer.lock"), 0)?;
    lock.try_lock_exclusive().map_err(|e| {
        if e.kind() == io::ErrorKind::WouldBlock {
            Error::Busy
        } else {
            Error::Unavailable
        }
    })?;
    // Provider key is deliberately not required or opened: export never decrypts
    // server credentials and future recovery must require re-registration.
    Ok((root, lock))
}
pub(super) fn output_path(root: &Path, output: &Path) -> Result<PathBuf> {
    let output = private_output_path(output)?;
    let parent = output.parent().ok_or(Error::InvalidInput)?;
    if parent.starts_with(root) || root.starts_with(parent) {
        return Err(Error::InvalidInput);
    }
    Ok(output)
}
pub(super) fn private_output_path(output: &Path) -> Result<PathBuf> {
    let name = output.file_name().ok_or(Error::InvalidInput)?;
    let parent = directory(output.parent().ok_or(Error::InvalidInput)?)?;
    for ancestor in parent.ancestors() {
        if ancestor.join("Cargo.toml").is_file()
            || ancestor.join(".git").exists()
            || ancestor.join("package.json").is_file()
            || ancestor.join("assistant.sqlite").exists()
            || ancestor.join("writer.lock").exists()
        {
            return Err(Error::InvalidInput);
        }
    }
    let output = parent.join(name);
    match fs::symlink_metadata(&output) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(output),
        _ => Err(Error::OutputExists),
    }
}

pub(super) fn publish(partial: &Path, output: &Path) -> Result<()> {
    // Same-directory hard-link publication is atomic and cannot overwrite.
    fs::hard_link(partial, output).map_err(|e| {
        if e.kind() == io::ErrorKind::AlreadyExists {
            Error::OutputExists
        } else {
            Error::Unavailable
        }
    })?;
    fs::remove_file(partial).map_err(|_| Error::Unavailable)?;
    sync_dir(output.parent().ok_or(Error::InvalidInput)?)
}
