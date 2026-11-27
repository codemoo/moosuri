//! Descriptor-based access to the worker-owned home. The worker can later
//! modify anything it owns, so root never follows a path inside it: each
//! directory is opened with O_NOFOLLOW, its owner re-checked on the open
//! descriptor, and files are created exclusively through that descriptor.
use crate::installation::kiri::host::Result;
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::{ffi::OsStrExt, fs::MetadataExt},
    },
    path::Path,
};
use uuid::Uuid;

// Unit tests cannot chown to a real service uid; there the "worker" is the test
// process and chown is a no-op. Production always uses the exact uid/gid.
#[cfg(test)]
pub(super) fn owner_ids(_uid: u32, _gid: u32) -> (u32, u32) {
    unsafe { (libc::geteuid(), libc::getegid()) }
}
#[cfg(not(test))]
pub(super) fn owner_ids(uid: u32, gid: u32) -> (u32, u32) {
    (uid, gid)
}
#[cfg(test)]
fn chown(_file: &File, _uid: u32, _gid: u32) -> Result<()> {
    Ok(())
}
#[cfg(not(test))]
fn chown(file: &File, uid: u32, gid: u32) -> Result<()> {
    if unsafe { libc::fchown(file.as_raw_fd(), uid, gid) } == 0 {
        Ok(())
    } else {
        Err("claude_runtime_home_chown_failed".into())
    }
}

fn errno() -> Option<i32> {
    std::io::Error::last_os_error().raw_os_error()
}
fn component(name: &str) -> Result<CString> {
    if name.is_empty() || name.contains('/') {
        return Err("claude_runtime_path_invalid".into());
    }
    CString::new(name).map_err(|_| "claude_runtime_path_invalid".into())
}
const DIR_FLAGS: libc::c_int =
    libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC;

pub(super) struct Dir {
    file: File,
    uid: u32,
    gid: u32,
}

impl Dir {
    /// Open the exact managed home (not through a final symlink), require it to
    /// be the service's own directory, then make it private (0700).
    pub(super) fn open_home(path: &Path, uid: u32, gid: u32) -> Result<Self> {
        let c =
            CString::new(path.as_os_str().as_bytes()).map_err(|_| "claude_runtime_path_invalid")?;
        let fd = unsafe { libc::open(c.as_ptr(), DIR_FLAGS) };
        if fd < 0 {
            return Err("claude_runtime_home_unavailable".into());
        }
        let dir = Self {
            file: unsafe { File::from_raw_fd(fd) },
            uid,
            gid,
        };
        dir.check()?;
        if unsafe { libc::fchmod(dir.file.as_raw_fd(), 0o700) } != 0 {
            return Err("claude_runtime_home_mode_failed".into());
        }
        Ok(dir)
    }

    fn check(&self) -> Result<()> {
        let m = self
            .file
            .metadata()
            .map_err(|_| "claude_runtime_home_unavailable")?;
        let (uid, gid) = owner_ids(self.uid, self.gid);
        if !m.is_dir() || m.uid() != uid || m.gid() != gid || m.mode() & 0o022 != 0 {
            return Err("claude_runtime_home_foreign".into());
        }
        Ok(())
    }

    /// Open or create (0700, chowned through the descriptor) a child directory.
    pub(super) fn child(&self, name: &str) -> Result<Self> {
        let c = component(name)?;
        let parent = self.file.as_raw_fd();
        let mut fd = unsafe { libc::openat(parent, c.as_ptr(), DIR_FLAGS) };
        let mut created = false;
        if fd < 0 {
            if errno() != Some(libc::ENOENT)
                || unsafe { libc::mkdirat(parent, c.as_ptr(), 0o700) } != 0
            {
                return Err("claude_runtime_home_foreign".into());
            }
            created = true;
            fd = unsafe { libc::openat(parent, c.as_ptr(), DIR_FLAGS) };
        }
        if fd < 0 {
            return Err("claude_runtime_home_foreign".into());
        }
        let child = Self {
            file: unsafe { File::from_raw_fd(fd) },
            uid: self.uid,
            gid: self.gid,
        };
        if created {
            chown(&child.file, self.uid, self.gid)?;
        }
        child.check()?;
        Ok(child)
    }

    /// A regular, single-link, worker-owned, non-group/world-writable file.
    pub(super) fn read_file(&self, name: &str, cap: u64) -> Result<Option<Vec<u8>>> {
        let c = component(name)?;
        let flags = libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK;
        let fd = unsafe { libc::openat(self.file.as_raw_fd(), c.as_ptr(), flags) };
        if fd < 0 {
            return if errno() == Some(libc::ENOENT) {
                Ok(None)
            } else {
                Err("claude_runtime_user_unit_conflict".into())
            };
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let m = file
            .metadata()
            .map_err(|_| "claude_runtime_user_unit_conflict")?;
        let (uid, _) = owner_ids(self.uid, self.gid);
        if !m.is_file()
            || m.nlink() != 1
            || m.uid() != uid
            || m.mode() & 0o022 != 0
            || m.len() > cap
        {
            return Err("claude_runtime_user_unit_conflict".into());
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(cap + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "claude_runtime_user_unit_conflict")?;
        Ok(Some(bytes))
    }

    /// Complete bytes are written to a private temporary file, then published
    /// with a no-clobber hard link. Returns false if the name already exists.
    pub(super) fn create_file(&self, name: &str, bytes: &[u8]) -> Result<bool> {
        let target = component(name)?;
        let temporary = component(&format!(".{name}.{}", Uuid::new_v4().simple()))?;
        let dir = self.file.as_raw_fd();
        let flags =
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC;
        let fd = unsafe { libc::openat(dir, temporary.as_ptr(), flags, 0o600 as libc::c_uint) };
        if fd < 0 {
            return Err("claude_runtime_user_unit_write_failed".into());
        }
        let mut file = unsafe { File::from_raw_fd(fd) };
        let written = chown(&file, self.uid, self.gid).and_then(|()| {
            file.write_all(bytes)
                .and_then(|()| file.sync_all())
                .map_err(|_| "claude_runtime_user_unit_write_failed".to_owned())
        });
        let published = match written {
            Err(e) => Err(e),
            Ok(()) => {
                if unsafe { libc::linkat(dir, temporary.as_ptr(), dir, target.as_ptr(), 0) } == 0 {
                    Ok(true)
                } else if errno() == Some(libc::EEXIST) {
                    Ok(false)
                } else {
                    Err("claude_runtime_user_unit_write_failed".to_owned())
                }
            }
        };
        unsafe { libc::unlinkat(dir, temporary.as_ptr(), 0) };
        let _ = self.file.sync_all();
        published
    }
}
