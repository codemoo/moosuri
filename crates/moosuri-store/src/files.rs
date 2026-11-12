use crate::{Result, StoreError};
#[cfg(unix)]
use std::os::unix::{
    fs::DirBuilderExt,
    fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
};
use std::{
    fs::{self, File, OpenOptions},
    path::{Component, Path, PathBuf},
};

pub(crate) fn prepare_dir(path: &Path) -> Result<PathBuf> {
    if path.as_os_str().is_empty() {
        return Err(StoreError::InvalidInput);
    }
    let absolute = if path.is_absolute() {
        path.to_owned()
    } else {
        std::env::current_dir()
            .map_err(|_| StoreError::Unavailable)?
            .join(path)
    };
    let mut current = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => return Err(StoreError::InvalidInput),
            Component::CurDir => continue,
            _ => current.push(component),
        }
        match fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() || !meta.is_dir() => {
                return Err(StoreError::InvalidInput);
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                let mut builder = fs::DirBuilder::new();
                #[cfg(unix)]
                builder.mode(0o700);
                builder
                    .create(&current)
                    .map_err(|_| StoreError::Unavailable)?;
                if let Some(parent) = current.parent() {
                    File::open(parent)
                        .and_then(|f| f.sync_all())
                        .map_err(|_| StoreError::Unavailable)?;
                }
            }
            Err(_) => return Err(StoreError::Unavailable),
        }
    }
    // Never turn an unrelated home, source tree or system directory into private app data.
    let unfinished_names = [
        "provider-secrets.key",
        "knowledge-invalidations.journal",
        "writer.lock",
        "assistant.sqlite",
        "auth-epochs.journal",
        "assistant.sqlite-wal",
        "assistant.sqlite-shm",
    ];
    let entries = fs::read_dir(&absolute).map_err(|_| StoreError::Unavailable)?;
    let unrelated = entries.into_iter().any(|entry| match entry {
        Ok(entry) => !unfinished_names
            .iter()
            .any(|name| entry.file_name() == *name),
        Err(_) => true,
    });
    if absolute.parent().is_none() || unrelated {
        return Err(StoreError::InvalidInput);
    }
    #[cfg(unix)]
    {
        let metadata = fs::metadata(&absolute).map_err(|_| StoreError::Unavailable)?;
        // The private root must belong to the process owner; never chmod someone else's directory.
        if metadata.uid() != unsafe { libc::geteuid() } {
            return Err(StoreError::InvalidInput);
        }
        fs::set_permissions(&absolute, fs::Permissions::from_mode(0o700))
            .map_err(|_| StoreError::Unavailable)?;
    }
    Ok(absolute)
}

pub(crate) fn private_file(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).read(true).write(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(path).map_err(|_| StoreError::Unavailable)?;
    let metadata = file.metadata().map_err(|_| StoreError::Unavailable)?;
    if !metadata.is_file() {
        return Err(StoreError::InvalidInput);
    }
    #[cfg(unix)]
    {
        if metadata.nlink() != 1 || metadata.uid() != unsafe { libc::geteuid() } {
            return Err(StoreError::InvalidInput);
        }
        file.set_permissions(fs::Permissions::from_mode(0o600))
            .map_err(|_| StoreError::Unavailable)?;
    }
    Ok(file)
}
