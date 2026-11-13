use super::*;
use std::{
    ffi::CString,
    os::{
        fd::AsRawFd,
        unix::{
            ffi::OsStrExt,
            fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        },
    },
};
pub(super) struct BoundedWriter<W> {
    inner: W,
    used: u64,
    limit: u64,
}
impl<W> BoundedWriter<W> {
    pub fn new(inner: W, limit: u64) -> Self {
        Self {
            inner,
            used: 0,
            limit,
        }
    }
    pub fn into_inner(self) -> W {
        self.inner
    }
}
impl<W: Write> Write for BoundedWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        if self
            .used
            .checked_add(buf.len() as u64)
            .is_none_or(|n| n > self.limit)
        {
            return Err(io::Error::other("checkpoint ciphertext limit"));
        }
        let n = self.inner.write(buf)?;
        self.used += n as u64;
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}
pub(super) fn target(root: &Path, path: &Path) -> Result<PathBuf> {
    let name = path.file_name().ok_or(Error::InvalidInput)?;
    let parent = files::directory(path.parent().ok_or(Error::InvalidInput)?)?;
    if parent.starts_with(root) || root.starts_with(&parent) {
        return Err(Error::InvalidInput);
    }
    for a in parent.ancestors() {
        if [
            "Cargo.toml",
            ".git",
            "package.json",
            "assistant.sqlite",
            "writer.lock",
            MARKER,
            STAGING,
        ]
        .iter()
        .any(|n| fs::symlink_metadata(a.join(n)).is_ok())
        {
            return Err(Error::InvalidInput);
        }
    }
    Ok(parent.join(name))
}
fn remove_owned(path: &Path, identity: (u64, u64)) -> Result<()> {
    let m = fs::symlink_metadata(path).map_err(|_| Error::Unavailable)?;
    if !m.is_file() || m.nlink() != 1 || (m.dev(), m.ino()) != identity {
        return Err(Error::Integrity);
    }
    fs::remove_file(path).map_err(|_| Error::Unavailable)
}
pub(super) fn stage(root: &Path, seal: &Seal, discard_unpublished: bool) -> Result<PathBuf> {
    let dir = root.join(STAGING);
    match fs::symlink_metadata(&dir) {
        Ok(_) => (),
        Err(e) if e.kind() == io::ErrorKind::NotFound && discard_unpublished => {
            fs::DirBuilder::new()
                .mode(0o700)
                .create(&dir)
                .map_err(|_| Error::Unavailable)?;
            files::sync_dir(root)?
        }
        Err(_) => return Err(Error::Integrity),
    }
    files::directory(&dir)?;
    let name = format!("{}.age", seal.checkpoint_id);
    let expected = dir.join(&name);
    for entry in fs::read_dir(&dir).map_err(|_| Error::Unavailable)? {
        let entry = entry.map_err(|_| Error::Unavailable)?;
        if entry.file_name() != std::ffi::OsStr::new(&name) {
            return Err(Error::Integrity);
        }
    }
    if discard_unpublished {
        match fs::symlink_metadata(&expected) {
            Ok(_) => {
                let f = files::read_file(&expected, CIPHER_LIMIT)?;
                let m = f.metadata().map_err(|_| Error::Unavailable)?;
                remove_owned(&expected, (m.dev(), m.ino()))?;
                files::sync_dir(&dir)?
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(_) => return Err(Error::Unavailable),
        }
    }
    Ok(expected)
}
pub(super) fn verify_file(path: &Path, digest: &str, size: u64, cap: u64) -> Result<()> {
    let e = backup::entry(path, "", cap)?;
    if e.size != size || e.sha256 != digest {
        return Err(Error::Integrity);
    }
    files::read_file(path, cap)?
        .sync_all()
        .map_err(|_| Error::Unavailable)
}
pub(super) fn ready_stage(
    root: &Path,
    seal: &Seal,
    output: &Path,
    head: &TrustedHead,
) -> Result<PathBuf> {
    use std::io::{Seek, SeekFrom};
    let dir = root.join(STAGING);
    if matches!(fs::symlink_metadata(&dir),Err(e) if e.kind()==io::ErrorKind::NotFound) {
        verify_file(
            output,
            &head.ciphertext_sha256,
            head.ciphertext_size,
            CIPHER_LIMIT,
        )?;
        fs::DirBuilder::new()
            .mode(0o700)
            .create(&dir)
            .map_err(|_| Error::Unavailable)?;
        files::sync_dir(root)?;
    }
    let path = stage(root, seal, false)?;
    let current = match fs::symlink_metadata(&path) {
        Ok(_) => Some(backup::entry(&path, "", CIPHER_LIMIT)?),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(_) => return Err(Error::Unavailable),
    };
    if current
        .as_ref()
        .is_some_and(|e| e.sha256 == head.ciphertext_sha256 && e.size == head.ciphertext_size)
    {
        verify_file(
            &path,
            &head.ciphertext_sha256,
            head.ciphertext_size,
            CIPHER_LIMIT,
        )?;
        files::sync_dir(&dir)?;
        return Ok(path);
    }
    // Repair only from exact previously published ciphertext. A failed repair
    // can leave a prefix; retry may append its missing bytes, never truncate or
    // replace a conflicting inode or re-encrypt under an adopted checkpoint ID.
    verify_file(
        output,
        &head.ciphertext_sha256,
        head.ciphertext_size,
        CIPHER_LIMIT,
    )?;
    let mut source = files::read_file(output, CIPHER_LIMIT)?;
    let mut append = if let Some(e) = current {
        if e.size > head.ciphertext_size {
            return Err(Error::Integrity);
        }
        let mut existing = files::read_file(&path, CIPHER_LIMIT)?;
        let original = existing.metadata().map_err(|_| Error::Unavailable)?;
        let mut left = e.size;
        let mut a = [0; 65536];
        let mut b = [0; 65536];
        while left > 0 {
            let n = (left as usize).min(a.len());
            existing
                .read_exact(&mut a[..n])
                .map_err(|_| Error::Unavailable)?;
            source
                .read_exact(&mut b[..n])
                .map_err(|_| Error::Unavailable)?;
            if a[..n] != b[..n] {
                return Err(Error::Integrity);
            }
            left -= n as u64;
        }
        let f = OpenOptions::new()
            .append(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(&path)
            .map_err(|_| Error::Unavailable)?;
        let m = f.metadata().map_err(|_| Error::Unavailable)?;
        if (m.dev(), m.ino(), m.len()) != (original.dev(), original.ino(), e.size) {
            return Err(Error::Integrity);
        }
        source
            .seek(SeekFrom::Start(e.size))
            .map_err(|_| Error::Unavailable)?;
        f
    } else {
        files::create_file(&path)?
    };
    let mut buf = [0; 65536];
    let mut remaining = head
        .ciphertext_size
        .checked_sub(append.metadata().map_err(|_| Error::Unavailable)?.len())
        .ok_or(Error::Integrity)?;
    while remaining > 0 {
        let limit = (remaining as usize).min(buf.len());
        let n = source
            .read(&mut buf[..limit])
            .map_err(|_| Error::Unavailable)?;
        if n == 0 {
            return Err(Error::Integrity);
        }
        append
            .write_all(&buf[..n])
            .map_err(|_| Error::Unavailable)?;
        remaining -= n as u64;
    }
    if source.read(&mut buf[..1]).map_err(|_| Error::Unavailable)? != 0 {
        return Err(Error::Integrity);
    }
    fault(root, "stage_repair_sync")?;
    append.sync_all().map_err(|_| Error::Unavailable)?;
    files::sync_dir(&dir)?;
    verify_file(
        &path,
        &head.ciphertext_sha256,
        head.ciphertext_size,
        CIPHER_LIMIT,
    )?;
    Ok(path)
}

struct OwnedTemp {
    path: PathBuf,
    identity: (u64, u64),
    published: bool,
}
impl Drop for OwnedTemp {
    fn drop(&mut self) {
        if !self.published {
            let _ = remove_owned(&self.path, self.identity);
        }
    }
}
pub(super) fn no_replace(temp: &Path, output: &Path) -> Result<()> {
    let parent = output.parent().ok_or(Error::InvalidInput)?;
    let fd = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(parent)
        .map_err(|_| Error::Unavailable)?;
    let from = CString::new(temp.file_name().ok_or(Error::InvalidInput)?.as_bytes())
        .map_err(|_| Error::InvalidInput)?;
    let to = CString::new(output.file_name().ok_or(Error::InvalidInput)?.as_bytes())
        .map_err(|_| Error::InvalidInput)?;
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            fd.as_raw_fd(),
            from.as_ptr(),
            fd.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            fd.as_raw_fd(),
            from.as_ptr(),
            fd.as_raw_fd(),
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else if io::Error::last_os_error().kind() == io::ErrorKind::AlreadyExists {
        Err(Error::OutputExists)
    } else {
        Err(Error::Unavailable)
    }
}
fn publish(
    root: &Path,
    mut input: impl Read,
    output: &Path,
    id: &str,
    expected: (&str, u64),
    role: &str,
    start: Instant,
) -> Result<()> {
    let (digest, size) = expected;
    deadline(start)?;
    let parent = output.parent().ok_or(Error::InvalidInput)?;
    match fs::symlink_metadata(output) {
        Ok(_) => {
            fault(root, &format!("{role}_sync"))?;
            verify_file(output, digest, size, CIPHER_LIMIT)?;
            fault(root, &format!("{role}_dir_sync"))?;
            return files::sync_dir(parent);
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
        Err(_) => return Err(Error::Unavailable),
    }
    // Each attempt owns one new inode. Interrupted stale temps are never adopted or
    // deleted by a later process; native no-replace rename avoids hard-link residue.
    let path = parent.join(format!(
        ".moosuri-checkpoint-{id}-{role}-{}.partial",
        Uuid::new_v4()
    ));
    let mut f = files::create_file(&path)?;
    let m = f.metadata().map_err(|_| Error::Unavailable)?;
    let mut temp = OwnedTemp {
        path: path.clone(),
        identity: (m.dev(), m.ino()),
        published: false,
    };
    let mut hash = Sha256::new();
    let mut count = 0u64;
    let mut buf = [0; 65536];
    loop {
        deadline(start)?;
        let n = input.read(&mut buf).map_err(|_| Error::Unavailable)?;
        if n == 0 {
            break;
        }
        count = count.checked_add(n as u64).ok_or(Error::LimitExceeded)?;
        if count > size {
            return Err(Error::Integrity);
        }
        f.write_all(&buf[..n]).map_err(|_| Error::Unavailable)?;
        hash.update(&buf[..n]);
    }
    if count != size || backup::hex(&hash.finalize()) != digest {
        return Err(Error::Integrity);
    }
    fault(root, &format!("{role}_sync"))?;
    f.sync_all().map_err(|_| Error::Unavailable)?;
    match no_replace(&path, output) {
        Ok(()) => temp.published = true,
        Err(Error::OutputExists) => verify_file(output, digest, size, CIPHER_LIMIT)?,
        Err(e) => return Err(e),
    }
    fault(root, &format!("{role}_after_rename"))?;
    fault(root, &format!("{role}_dir_sync"))?;
    files::sync_dir(parent)
}
pub(super) fn publish_file(
    root: &Path,
    source: &Path,
    output: &Path,
    id: &str,
    expected: (&str, u64),
    role: &str,
    start: Instant,
) -> Result<()> {
    publish(
        root,
        files::read_file(source, CIPHER_LIMIT)?,
        output,
        id,
        expected,
        role,
        start,
    )
}
pub(super) fn publish_bytes(
    root: &Path,
    bytes: &[u8],
    output: &Path,
    id: &str,
    role: &str,
    start: Instant,
) -> Result<()> {
    publish(
        root,
        bytes,
        output,
        id,
        (&hash(bytes), bytes.len() as u64),
        role,
        start,
    )
}
