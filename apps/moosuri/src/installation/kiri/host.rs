//! Root-controlled filesystem and command helpers for the clean Kiri installer.
//! Nothing here reads credentials; every managed file is created without
//! replacing an unrelated path, and a repeated run may only observe identical bytes.
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
        process::ExitStatusExt,
    },
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
use zeroize::Zeroizing;

pub type Result<T> = std::result::Result<T, String>;

pub fn euid() -> u32 {
    unsafe { libc::geteuid() }
}

/// Filesystem prefix. Production is always `/`; tests use a temporary directory
/// while generated contents still name the real absolute locations.
#[derive(Clone)]
pub struct Layout {
    root: PathBuf,
}
impl Layout {
    pub fn system() -> Self {
        Self { root: "/".into() }
    }
    #[cfg(test)]
    pub fn under(root: &Path) -> Self {
        Self { root: root.into() }
    }
    pub fn at(&self, absolute: &str) -> PathBuf {
        if self.root == Path::new("/") {
            absolute.into()
        } else {
            self.root.join(absolute.trim_start_matches('/'))
        }
    }
    /// Every existing ancestor at or below the layout root must be an unshared,
    /// non-symlink directory owned by the installer.
    pub fn safe_ancestors(&self, path: &Path) -> Result<()> {
        for a in path.ancestors().skip(1) {
            if !a.starts_with(&self.root) {
                break;
            }
            let m = fs::symlink_metadata(a).map_err(|_| "kiri_install_ancestor_unavailable")?;
            if m.file_type().is_symlink()
                || !m.is_dir()
                || m.mode() & 0o022 != 0
                || (m.uid() != euid() && m.uid() != 0)
            {
                return Err("kiri_install_unsafe_ancestor".into());
            }
        }
        Ok(())
    }
    pub fn exists(&self, path: &Path) -> Result<bool> {
        match fs::symlink_metadata(path) {
            Ok(_) => Ok(true),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(_) => Err("kiri_install_path_unavailable".into()),
        }
    }
    /// Create one private directory, or accept an existing equally private one.
    pub fn ensure_dir(&self, path: &Path, mode: u32) -> Result<()> {
        self.safe_ancestors(path)?;
        match fs::symlink_metadata(path) {
            Ok(m) => {
                if m.file_type().is_symlink()
                    || !m.is_dir()
                    || m.uid() != euid()
                    || m.mode() & 0o022 != 0
                    || (m.mode() & 0o777) & !mode != 0
                {
                    return Err("kiri_install_foreign_directory".into());
                }
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                fs::DirBuilder::new()
                    .mode(mode)
                    .create(path)
                    .map_err(|_| "kiri_install_directory_create_failed")?;
                fs::set_permissions(path, fs::Permissions::from_mode(mode))
                    .map_err(|_| "kiri_install_directory_mode_failed")?;
                Ok(())
            }
            Err(_) => Err("kiri_install_path_unavailable".into()),
        }
    }
    /// Write a managed file. Existing bytes may only equal `bytes` or one of the
    /// exact previous digests this installer itself produced for the same step.
    pub fn write_file(
        &self,
        path: &Path,
        bytes: &[u8],
        mode: u32,
        previous: &[String],
    ) -> Result<()> {
        let digest = hex(&Sha256::digest(bytes));
        self.write_stream(path, &mut &bytes[..], bytes.len() as u64, &digest, mode, previous)
    }
    /// Stream `source` into a same-directory temporary file, verify length and
    /// SHA-256, then atomically publish without clobbering an unrelated file.
    pub fn write_stream(
        &self,
        path: &Path,
        source: &mut dyn Read,
        cap: u64,
        expected: &str,
        mode: u32,
        previous: &[String],
    ) -> Result<()> {
        let parent = path.parent().ok_or("kiri_install_path_invalid")?;
        self.safe_ancestors(path)?;
        let existing = match fs::symlink_metadata(path) {
            Ok(m) => {
                if !m.is_file() || m.nlink() != 1 || m.uid() != euid() || m.mode() & 0o022 != 0 {
                    return Err("kiri_install_foreign_file".into());
                }
                let current = sha256_file(path, cap.max(m.len()))?;
                if current == expected {
                    return Ok(());
                }
                if !previous.iter().any(|p| *p == current) {
                    return Err("kiri_install_foreign_or_changed_file".into());
                }
                true
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => false,
            Err(_) => return Err("kiri_install_path_unavailable".into()),
        };
        let mut temp = tempfile::Builder::new()
            .prefix(".moosuri-kiri-")
            .tempfile_in(parent)
            .map_err(|_| "kiri_install_write_failed")?;
        let mut hasher = Sha256::new();
        let mut total = 0u64;
        let mut buffer = vec![0u8; 1 << 20];
        loop {
            let n = source
                .read(&mut buffer)
                .map_err(|_| "kiri_install_source_unreadable")?;
            if n == 0 {
                break;
            }
            total += n as u64;
            if total > cap {
                return Err("kiri_install_source_too_large".into());
            }
            hasher.update(&buffer[..n]);
            temp.write_all(&buffer[..n])
                .map_err(|_| "kiri_install_write_failed")?;
        }
        if hex(&hasher.finalize()) != expected {
            return Err("kiri_install_source_hash_mismatch".into());
        }
        temp.as_file()
            .set_permissions(fs::Permissions::from_mode(mode))
            .map_err(|_| "kiri_install_write_failed")?;
        temp.as_file()
            .sync_all()
            .map_err(|_| "kiri_install_write_failed")?;
        if existing {
            temp.persist(path).map_err(|_| "kiri_install_write_failed")?;
        } else {
            temp.persist_noclobber(path)
                .map_err(|_| "kiri_install_path_appeared")?;
        }
        File::open(parent)
            .and_then(|d| d.sync_all())
            .map_err(|_| "kiri_install_write_failed".into())
    }
}

pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
pub fn is_hex(s: &str, len: usize) -> bool {
    s.len() == len && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
pub fn sha256_bytes(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

/// Open a regular, single-link, installer-owned, non-world/group-writable file
/// without following a final symlink or blocking on a special file.
pub fn open_regular(path: &Path, cap: u64) -> Result<(File, u64)> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "kiri_install_file_unavailable")?;
    let m = file.metadata().map_err(|_| "kiri_install_file_unavailable")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != euid()
        || m.mode() & 0o022 != 0
        || m.len() == 0
        || m.len() > cap
    {
        return Err("kiri_install_file_untrusted_or_unbounded".into());
    }
    Ok((file, m.len()))
}
pub fn sha256_file(path: &Path, cap: u64) -> Result<String> {
    let (mut file, _) = open_regular(path, cap)?;
    sha256_reader(&mut file, cap)
}
pub fn sha256_reader(reader: &mut dyn Read, cap: u64) -> Result<String> {
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut total = 0u64;
    loop {
        let n = reader
            .read(&mut buffer)
            .map_err(|_| "kiri_install_file_unreadable")?;
        if n == 0 {
            return Ok(hex(&hasher.finalize()));
        }
        total += n as u64;
        if total > cap {
            return Err("kiri_install_file_too_large".into());
        }
        hasher.update(&buffer[..n]);
    }
}

pub enum Input {
    None,
    Secret(Zeroizing<Vec<u8>>),
    File(PathBuf),
}
pub struct Out {
    pub code: i32,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
}
impl Out {
    pub fn ok(self, reason: &str) -> Result<Vec<u8>> {
        if self.code == 0 {
            Ok(self.stdout)
        } else {
            Err(reason.into())
        }
    }
    /// A safe snake_case worker reason token, never raw process output.
    pub fn reason(&self) -> String {
        let text = String::from_utf8_lossy(&self.stderr);
        let line = text
            .lines()
            .rev()
            .find_map(|l| l.strip_prefix("moosuri: "))
            .unwrap_or("");
        line.chars()
            .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
            .take(120)
            .collect()
    }
}
/// Command seam: production resolves fixed system directories; tests substitute a fake.
pub trait Exec {
    fn run(&self, program: &str, args: &[String], input: Input, limit: Duration) -> Result<Out>;
}
pub struct System {
    pub docker_config: PathBuf,
}
const TOOL_DIRS: [&str; 4] = ["/usr/local/sbin", "/usr/sbin", "/usr/bin", "/bin"];
pub fn find_tool(name: &str) -> Result<PathBuf> {
    if name.contains('/') {
        let p = PathBuf::from(name);
        return trusted_tool(&p).map(|_| p);
    }
    for dir in TOOL_DIRS {
        let p = Path::new(dir).join(name);
        if trusted_tool(&p).is_ok() {
            return Ok(p);
        }
    }
    Err(format!("kiri_install_missing_{}", name.replace('-', "_")))
}
fn trusted_tool(p: &Path) -> Result<()> {
    let m = fs::metadata(p).map_err(|_| "kiri_install_tool_unavailable")?;
    if !m.is_file() || m.uid() != 0 || m.mode() & 0o111 == 0 || m.mode() & 0o022 != 0 {
        return Err("kiri_install_tool_untrusted".into());
    }
    Ok(())
}
impl Exec for System {
    fn run(&self, program: &str, args: &[String], input: Input, limit: Duration) -> Result<Out> {
        let tool = find_tool(program)?;
        let mut command = Command::new(tool);
        command
            .args(args)
            .env_clear()
            .env("PATH", "/usr/sbin:/usr/bin:/sbin:/bin")
            .env("LC_ALL", "C")
            .env("DOCKER_CONFIG", &self.docker_config)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        match &input {
            Input::None => {
                command.stdin(Stdio::null());
            }
            Input::Secret(_) => {
                command.stdin(Stdio::piped());
            }
            Input::File(path) => {
                let (file, _) = open_regular(path, 4 << 30)?;
                command.stdin(Stdio::from(file));
            }
        }
        let mut child = command.spawn().map_err(|_| "kiri_install_spawn_failed")?;
        if let (Input::Secret(secret), Some(mut stdin)) = (&input, child.stdin.take()) {
            // A child that does not need the secret may exit first.
            let _ = stdin.write_all(secret);
        }
        let mut out = child.stdout.take().ok_or("kiri_install_spawn_failed")?;
        let mut err = child.stderr.take().ok_or("kiri_install_spawn_failed")?;
        let stdout = std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = (&mut out).take(1 << 20).read_to_end(&mut v);
            v
        });
        let stderr = std::thread::spawn(move || {
            let mut v = Vec::new();
            let _ = (&mut err).take(64 << 10).read_to_end(&mut v);
            v
        });
        let deadline = Instant::now() + limit;
        let status = loop {
            match child.try_wait().map_err(|_| "kiri_install_wait_failed")? {
                Some(s) => break s,
                None if Instant::now() >= deadline => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("kiri_install_command_timeout".into());
                }
                None => std::thread::sleep(Duration::from_millis(50)),
            }
        };
        Ok(Out {
            code: status.code().unwrap_or_else(|| 128 + status.signal().unwrap_or(0)),
            stdout: stdout.join().unwrap_or_default(),
            stderr: stderr.join().unwrap_or_default(),
        })
    }
}

pub fn args(items: &[&str]) -> Vec<String> {
    items.iter().map(|s| (*s).to_owned()).collect()
}

#[cfg(test)]
pub mod fake {
    use super::*;
    use std::{cell::RefCell, collections::VecDeque};
    /// Records command lines (never secret input) and returns scripted replies.
    #[derive(Default)]
    pub struct Fake {
        pub calls: RefCell<Vec<String>>,
        pub replies: RefCell<VecDeque<(i32, String)>>,
    }
    impl Fake {
        pub fn reply(&self, code: i32, stdout: &str) {
            self.replies.borrow_mut().push_back((code, stdout.into()));
        }
    }
    impl Exec for Fake {
        fn run(&self, program: &str, a: &[String], _: Input, _: Duration) -> Result<Out> {
            self.calls.borrow_mut().push(format!("{program} {}", a.join(" ")));
            let (code, stdout) = self
                .replies
                .borrow_mut()
                .pop_front()
                .unwrap_or((0, String::new()));
            Ok(Out {
                code,
                stdout: stdout.into_bytes(),
                stderr: Vec::new(),
            })
        }
    }
}
