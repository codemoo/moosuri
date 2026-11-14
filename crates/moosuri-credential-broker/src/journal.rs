//! Private, fsynced, exclusively locked snapshot journal. Journal contains no secrets.
use crate::{Error, Result, auth::hash};
use serde::{Serialize, de::DeserializeOwned};
#[cfg(unix)]
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path},
};
const LIMIT: u64 = 16 * 1024 * 1024;
pub(crate) struct Journal {
    root: File,
    lock: File,
    file: File,
    head: File,
}
fn io<T>(r: std::io::Result<T>) -> Result<T> {
    r.map_err(|_| Error::PrivateState)
}
#[cfg(target_os = "macos")]
fn acl(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            id: libc::c_int,
            entry: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_free(ptr: *mut libc::c_void) -> libc::c_int;
    }
    // Darwin ACL_TYPE_EXTENDED, ACL_FIRST_ENTRY. Empty ACLs are accepted; any
    // explicit or inherited entry fails closed instead of guessing its semantics.
    unsafe {
        let a = acl_get_fd_np(file.as_raw_fd(), 0x100);
        if a.is_null() {
            return if matches!(
                std::io::Error::last_os_error().raw_os_error(),
                Some(libc::ENOENT | libc::ENOTSUP)
            ) {
                Ok(())
            } else {
                Err(Error::PrivateState)
            };
        }
        let mut entry = std::ptr::null_mut();
        let r = acl_get_entry(a, 0, &mut entry);
        let error = std::io::Error::last_os_error().raw_os_error();
        acl_free(a);
        if r != 0 && error == Some(libc::EINVAL) {
            Ok(())
        } else {
            Err(Error::PrivateState)
        }
    }
}
#[cfg(target_os = "linux")]
fn acl(file: &File) -> Result<()> {
    use std::os::fd::AsRawFd;
    for name in [
        c"system.posix_acl_access",
        c"system.posix_acl_default",
        c"system.nfs4_acl",
    ] {
        let r =
            unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
        if r >= 0 {
            return Err(Error::PrivateState);
        }
        let e = std::io::Error::last_os_error().raw_os_error();
        if e != Some(libc::ENODATA) && e != Some(libc::ENOTSUP) {
            return Err(Error::PrivateState);
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn acl(_: &File) -> Result<()> {
    Err(Error::PrivateState)
}
fn check(file: &File, dir: bool) -> Result<()> {
    let m = io(file.metadata())?;
    if m.is_dir() != dir || (!dir && !m.is_file()) {
        return Err(Error::PrivateState);
    }
    #[cfg(unix)]
    if m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != if dir { 0o700 } else { 0o600 }
        || (!dir && m.nlink() != 1)
    {
        return Err(Error::PrivateState);
    }
    acl(file)
}
fn open(root: &File, name: &str, create: bool) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::fd::{AsRawFd, FromRawFd};
        let name = std::ffi::CString::new(name).map_err(|_| Error::PrivateState)?;
        let flags = libc::O_RDWR
            | libc::O_NOFOLLOW
            | libc::O_CLOEXEC
            | libc::O_NONBLOCK
            | if create {
                libc::O_CREAT | libc::O_EXCL
            } else {
                0
            };
        let fd = unsafe { libc::openat(root.as_raw_fd(), name.as_ptr(), flags, 0o600) };
        if fd < 0 {
            return Err(Error::PrivateState);
        }
        let f = unsafe { File::from_raw_fd(fd) };
        check(&f, false)?;
        Ok(f)
    }
    #[cfg(not(unix))]
    {
        let _ = (root, name, create);
        Err(Error::PrivateState)
    }
}
fn root(path: &Path, create: bool) -> Result<File> {
    if !path.is_absolute() {
        return Err(Error::PrivateState);
    }
    let mut p = std::path::PathBuf::new();
    for c in path.components() {
        if matches!(c, Component::ParentDir | Component::CurDir) {
            return Err(Error::PrivateState);
        }
        p.push(c);
        if let Ok(m) = fs::symlink_metadata(&p) {
            if !m.is_dir() || m.file_type().is_symlink() {
                return Err(Error::PrivateState);
            }
            let mut o = OpenOptions::new();
            o.read(true);
            #[cfg(unix)]
            o.custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC);
            let f = io(o.open(&p))?;
            let m = io(f.metadata())?;
            #[cfg(unix)]
            if (m.uid() != unsafe { libc::geteuid() } && m.uid() != 0)
                || (m.mode() & 0o022 != 0 && m.mode() & 0o1000 == 0)
            {
                return Err(Error::PrivateState);
            }
            acl(&f)?;
        }
    }
    if create {
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            io(fs::DirBuilder::new().mode(0o700).create(path))?;
        }
        #[cfg(not(unix))]
        return Err(Error::PrivateState);
    }
    let mut o = OpenOptions::new();
    o.read(true);
    #[cfg(unix)]
    o.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_DIRECTORY);
    let f = io(o.open(path))?;
    check(&f, true)?;
    Ok(f)
}
impl Journal {
    pub(crate) fn create<T: Serialize>(path: &Path, key: &[u8], state: &T) -> Result<Self> {
        let root = root(path, true)?;
        let lock = open(&root, "owner.lock", true)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| Error::Locked)?;
        let mut secret = open(&root, "device.pkcs8", true)?;
        io(secret.write_all(key))?;
        io(secret.sync_all())?;
        let mut j = Self {
            file: open(&root, "journal", true)?,
            head: open(&root, "journal.head", true)?,
            root,
            lock,
        };
        j.append(state)?;
        io(j.root.sync_all())?;
        if let Some(p) = path.parent() {
            io(io(File::open(p))?.sync_all())?
        }
        Ok(j)
    }
    pub(crate) fn reopen<T: DeserializeOwned>(
        path: &Path,
    ) -> Result<(Self, zeroize::Zeroizing<Vec<u8>>, T)> {
        let root = root(path, false)?;
        let lock = open(&root, "owner.lock", false)?;
        fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| Error::Locked)?;
        let mut secret = open(&root, "device.pkcs8", false)?;
        if io(secret.metadata())?.len() > 256 {
            return Err(Error::PrivateState);
        }
        let mut key = zeroize::Zeroizing::new(Vec::new());
        io(secret.read_to_end(&mut key))?;
        let mut j = Self {
            file: open(&root, "journal", false)?,
            head: open(&root, "journal.head", false)?,
            root,
            lock,
        };
        let len = io(j.file.metadata())?.len();
        if len == 0 || len > LIMIT || io(j.head.metadata())?.len() != 40 {
            return Err(Error::PrivateState);
        }
        let mut anchor = [0; 40];
        io(j.head.read_exact(&mut anchor))?;
        let mut bytes = Vec::new();
        io(j.file.read_to_end(&mut bytes))?;
        if u64::from_be_bytes(anchor[..8].try_into().unwrap()) != len || hash(&bytes) != anchor[8..]
        {
            return Err(Error::PrivateState);
        }
        // Every complete line is a snapshot; empty/truncated/invalid entries fail closed.
        if bytes.last() != Some(&b'\n') {
            return Err(Error::PrivateState);
        }
        let mut latest = None;
        for line in bytes[..bytes.len() - 1].split(|b| *b == b'\n') {
            latest = Some(serde_json::from_slice(line).map_err(|_| Error::PrivateState)?)
        }
        Ok((j, key, latest.ok_or(Error::PrivateState)?))
    }
    pub(crate) fn append<T: Serialize>(&mut self, state: &T) -> Result<()> {
        use std::io::{Seek, SeekFrom};
        let mut bytes = serde_json::to_vec(state).map_err(|_| Error::PrivateState)?;
        bytes.push(b'\n');
        let len = io(self.file.metadata())?.len();
        if len + bytes.len() as u64 > LIMIT {
            return Err(Error::PrivateState);
        }
        io(self.file.seek(SeekFrom::End(0)))?;
        io(self.file.write_all(&bytes))?;
        io(self.file.sync_all())?;
        io(self.file.seek(SeekFrom::Start(0)))?;
        let mut all = Vec::new();
        io(self.file.read_to_end(&mut all))?;
        io(self.head.seek(SeekFrom::Start(0)))?;
        io(self.head.write_all(&(all.len() as u64).to_be_bytes()))?;
        io(self.head.write_all(&hash(&all)))?;
        io(self.head.set_len(40))?;
        io(self.head.sync_all())?;
        Ok(())
    }
}
impl Drop for Journal {
    fn drop(&mut self) {
        let _ = fs2::FileExt::unlock(&self.lock);
    }
}
