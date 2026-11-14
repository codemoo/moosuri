//! Unix owner-private, descriptor-relative files; no path-based read/write after open.
use fs2::FileExt;
use moosuri_connectors::chatgpt_oauth::handoff::HandoffKey;
use std::{
    ffi::CString,
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::Path,
};
use zeroize::Zeroizing;
pub struct PrivateState {
    dir: File,
    _lock: File,
    pub key: HandoffKey,
}
fn err() -> String {
    "private_helper_state_unavailable".into()
}
fn check(f: &File) -> Result<(), String> {
    let m = f.metadata().map_err(|_| err())?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o777 != 0o600
    {
        return Err(err());
    }
    private_acl(f)
}
#[cfg(target_os = "macos")]
fn private_acl(dir: &File) -> Result<(), String> {
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            entry: libc::c_int,
            result: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    struct Acl(*mut libc::c_void);
    impl Drop for Acl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }
    let acl = unsafe { acl_get_fd_np(dir.as_raw_fd(), 0x100) }; // ACL_TYPE_EXTENDED
    if acl.is_null() {
        // Darwin reports ENOENT for a valid pinned directory with no extended
        // ACL, including ordinary root ancestry. Other inspection failures fail.
        if matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOENT | libc::ENOTSUP)
        ) {
            return Ok(());
        }
        return Err("private_helper_acl_unavailable".into());
    }
    let acl = Acl(acl);
    // Darwin returns -1/EINVAL at end-of-list. The descriptor yields a native
    // valid ACL; cap entries rather than consuming an unbounded list.
    for index in 0..=128 {
        let mut entry = std::ptr::null_mut();
        if unsafe { acl_get_entry(acl.0, if index == 0 { 0 } else { -1 }, &mut entry) } != 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                Ok(())
            } else {
                Err("private_helper_acl_unavailable".into())
            };
        }
        let mut tag = 0;
        if index == 128 || unsafe { acl_get_tag_type(entry, &mut tag) } != 0 || tag != 2 {
            return Err("private_helper_acl_grant_denied".into());
        }
    }
    Err("private_helper_acl_unavailable".into())
}
// Reject Linux extended/default/NFSv4 ACLs rather than claiming that mode bits
// describe every filesystem ACL policy. No attribute contents/principals leak.
#[cfg(target_os = "linux")]
fn private_acl(dir: &File) -> Result<(), String> {
    for attribute in [
        c"system.posix_acl_access",
        c"system.posix_acl_default",
        c"system.nfs4_acl",
    ] {
        if unsafe { libc::fgetxattr(dir.as_raw_fd(), attribute.as_ptr(), std::ptr::null_mut(), 0) }
            >= 0
        {
            return Err("private_helper_acl_grant_denied".into());
        }
        let error = std::io::Error::last_os_error().raw_os_error();
        if error != Some(libc::ENODATA) && error != Some(libc::ENOTSUP) {
            return Err("private_helper_acl_unavailable".into());
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "macos", target_os = "linux")))]
fn private_acl(_file: &File) -> Result<(), String> {
    Err("private_helper_acl_unavailable".into())
}
impl PrivateState {
    pub fn open(path: &Path) -> Result<Self, String> {
        // Reject symlink parents. The private final directory is pinned by fd.
        let mut parent = std::path::PathBuf::new();
        for component in path.components() {
            parent.push(component);
            match std::fs::symlink_metadata(&parent) {
                Ok(m) if m.file_type().is_symlink() => return Err(err()),
                Ok(m) if !m.is_dir() => return Err(err()),
                Ok(_) => (),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    std::fs::DirBuilder::new()
                        .mode(0o700)
                        .recursive(false)
                        .create(&parent)
                        .map_err(|_| err())?;
                }
                Err(_) => return Err(err()),
            }
            // Pin/check every ancestor too: grant/delete-child ACLs cannot make
            // a mode-private final directory reachable or replaceable by others.
            let ancestor = OpenOptions::new()
                .read(true)
                .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&parent)
                .map_err(|_| err())?;
            let metadata = ancestor.metadata().map_err(|_| err())?;
            if !metadata.is_dir()
                || (metadata.uid() != unsafe { libc::geteuid() } && metadata.uid() != 0)
                || (metadata.mode() & 0o022 != 0 && metadata.mode() & 0o1000 == 0)
            {
                return Err(err());
            }
            private_acl(&ancestor)?;
        }
        let dir = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| err())?;
        let m = dir.metadata().map_err(|_| err())?;
        if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o777 != 0o700 {
            return Err(err());
        }
        private_acl(&dir)?;
        let open = |name: &str, flags: i32| -> Result<File, String> {
            let name = CString::new(name).map_err(|_| err())?;
            let fd = unsafe {
                libc::openat(
                    dir.as_raw_fd(),
                    name.as_ptr(),
                    flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
                    0o600,
                )
            };
            if fd < 0 {
                return Err(err());
            }
            let f = unsafe { File::from_raw_fd(fd) };
            check(&f)?;
            Ok(f)
        };
        let lock = open("lock", libc::O_CREAT | libc::O_RDWR)?;
        lock.try_lock_exclusive()
            .map_err(|_| "helper_already_running".to_string())?;
        let key = match open("journal.key", libc::O_RDONLY) {
            Ok(f) => {
                let mut bytes = Zeroizing::new(Vec::new());
                f.take(33).read_to_end(&mut bytes).map_err(|_| err())?;
                HandoffKey::from_private_bytes(&bytes).map_err(|_| err())?
            }
            Err(_) => {
                // O_EXCL makes a malformed/existing key fail closed.
                let key = HandoffKey::generate().map_err(|_| err())?;
                let mut f = open("journal.key", libc::O_CREAT | libc::O_EXCL | libc::O_WRONLY)?;
                key.with_private_bytes(|b| f.write_all(b))
                    .map_err(|_| err())?;
                f.sync_all().map_err(|_| err())?;
                dir.sync_all().map_err(|_| err())?;
                key
            }
        };
        Ok(Self {
            dir,
            _lock: lock,
            key,
        })
    }
    fn name(name: &str) -> Result<CString, String> {
        if name.is_empty()
            || !name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
            || name == "."
            || name == ".."
        {
            return Err(err());
        }
        CString::new(name).map_err(|_| err())
    }
    pub fn read(&self, name: &str, limit: usize) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
        let name = Self::name(name)?;
        let fd = unsafe {
            libc::openat(
                self.dir.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDONLY | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            )
        };
        if fd < 0 {
            return if std::io::Error::last_os_error().kind() == std::io::ErrorKind::NotFound {
                Ok(None)
            } else {
                Err(err())
            };
        }
        let f = unsafe { File::from_raw_fd(fd) };
        check(&f)?;
        let mut data = Zeroizing::new(Vec::new());
        f.take((limit + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|_| err())?;
        if data.len() > limit {
            return Err(err());
        }
        Ok(Some(data))
    }
    pub fn write(&self, name: &str, data: &[u8]) -> Result<(), String> {
        let dest = Self::name(name)?;
        let temp = Self::name(&format!("pending-{:032x}", rand::random::<u128>()))?;
        let fd = unsafe {
            libc::openat(
                self.dir.as_raw_fd(),
                temp.as_ptr(),
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            )
        };
        if fd < 0 {
            return Err(err());
        }
        let mut f = unsafe { File::from_raw_fd(fd) };
        check(&f)?;
        f.write_all(data).map_err(|_| err())?;
        f.sync_all().map_err(|_| err())?;
        // Destination must be an owned single-link regular file if present.
        self.read(name, 256 * 1024)?;
        if unsafe {
            libc::renameat(
                self.dir.as_raw_fd(),
                temp.as_ptr(),
                self.dir.as_raw_fd(),
                dest.as_ptr(),
            )
        } != 0
        {
            return Err(err());
        }
        self.dir.sync_all().map_err(|_| err())
    }
    pub fn remove(&self, name: &str) -> Result<(), String> {
        self.read(name, 256 * 1024)?;
        let name = Self::name(name)?;
        let rc = unsafe { libc::unlinkat(self.dir.as_raw_fd(), name.as_ptr(), 0) };
        if rc != 0 && std::io::Error::last_os_error().kind() != std::io::ErrorKind::NotFound {
            return Err(err());
        }
        self.dir.sync_all().map_err(|_| err())
    }
    pub fn local_host(&self) -> Result<String, String> {
        if let Some(bytes) = self.read("host", 128)? {
            return String::from_utf8(bytes.to_vec()).map_err(|_| err());
        }
        let host = format!("urn:uuid:{}", uuid::Uuid::new_v4());
        self.write("host", host.as_bytes())?;
        Ok(host)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[cfg(target_os = "macos")]
    #[test]
    fn inherited_allow_acl_rejects_existing_directory_and_key_before_secret_read() {
        let root = tempfile::tempdir().unwrap();
        let parent = root.path().join("acl-parent");
        std::fs::create_dir(&parent).unwrap();
        assert!(std::process::Command::new("/bin/chmod").args(["+a","everyone allow list,search,readattr,readextattr,readsecurity,file_inherit,directory_inherit"]).arg(&parent).status().unwrap().success());
        let path = parent.join("state");
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&path)
            .unwrap();
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(path.join("journal.key"))
            .unwrap();
        file.write_all(&[42; 32]).unwrap();
        file.sync_all().unwrap();
        assert_eq!(file.metadata().unwrap().mode() & 0o777, 0o600);
        let listing = std::process::Command::new("/bin/ls")
            .arg("-lde")
            .arg(path.join("journal.key"))
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&listing.stdout).contains("inherited"),
            "fixture must have a real inherited ACL"
        );
        assert!(
            check(&file).is_err(),
            "pinned inherited key descriptor fails before reads"
        );
        drop(file);
        assert!(
            PrivateState::open(&path).is_err(),
            "inherited allow ACL must never be accepted"
        );
        assert!(
            std::process::Command::new("/bin/chmod")
                .arg("-RN")
                .arg(&parent)
                .status()
                .unwrap()
                .success()
        );
        let state = PrivateState::open(&path).unwrap();
        drop(state);
        assert!(
            std::process::Command::new("/bin/chmod")
                .args(["+a", "everyone allow read"])
                .arg(path.join("journal.key"))
                .status()
                .unwrap()
                .success()
        );
        assert!(
            PrivateState::open(&path).is_err(),
            "key allow ACL must fail before reading"
        );
        assert!(
            std::process::Command::new("/bin/chmod")
                .arg("-N")
                .arg(path.join("journal.key"))
                .status()
                .unwrap()
                .success()
        );
        assert!(
            std::process::Command::new("/bin/chmod")
                .args(["+a", "everyone deny delete"])
                .arg(&path)
                .status()
                .unwrap()
                .success()
        );
        let state = PrivateState::open(&path).unwrap();
        state.write("synthetic-deny", b"synthetic").unwrap();
        assert!(state.read("synthetic-deny", 32).unwrap().is_some());
        drop(state);
        assert!(
            std::process::Command::new("/bin/chmod")
                .arg("-RN")
                .arg(&parent)
                .status()
                .unwrap()
                .success()
        );
    }
    #[test]
    fn private_state_rejects_symlinks_hardlinks_modes_and_concurrent_owner() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("state");
        let state = PrivateState::open(&path).unwrap();
        state.write("synthetic", b"synthetic").unwrap();
        assert_eq!(
            &**state.read("synthetic", 20).unwrap().as_ref().unwrap(),
            b"synthetic"
        );
        assert!(PrivateState::open(&path).is_err());
        assert!(state.write("../escape", b"x").is_err());
        std::os::unix::fs::symlink("synthetic", path.join("symlink")).unwrap();
        assert!(state.read("symlink", 20).is_err());
        std::fs::hard_link(path.join("synthetic"), path.join("hardlink")).unwrap();
        assert!(state.read("synthetic", 20).is_err());
        std::fs::remove_file(path.join("hardlink")).unwrap();
        std::fs::set_permissions(
            path.join("synthetic"),
            std::fs::Permissions::from_mode(0o644),
        )
        .unwrap();
        assert!(state.read("synthetic", 20).is_err());
    }
}
