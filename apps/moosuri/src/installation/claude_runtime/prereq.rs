//! Read-only prerequisite and trusted-file checks. Everything here runs before
//! the first write; a missing or unsupported prerequisite is a fixed token.
//! Nothing is installed from the network and no Docker version is changed.
use crate::installation::kiri::host::{self, Exec, Input, Layout, Result, args};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};

const TOOL_DIRS: [&str; 4] = ["/usr/local/sbin", "/usr/sbin", "/usr/bin", "/bin"];
const ENGINE_CAP: u64 = 512 << 20;
pub(super) const HELPER_CAP: u64 = 512 << 20;
const REQUIRED_TOOLS: [&str; 14] = [
    "newuidmap",
    "newgidmap",
    "rootlesskit",
    "slirp4netns",
    "fuse-overlayfs",
    "loginctl",
    "runuser",
    "getent",
    "useradd",
    "curl",
    "dockerd",
    "containerd",
    "runc",
    "env",
];
const CONTROLLERS: [&str; 5] = ["cpu", "cpuset", "io", "memory", "pids"];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PinnedFile {
    pub path: PathBuf,
    pub sha256: String,
}

pub(super) struct Tools {
    pub systemctl: PathBuf,
    pub engine: PinnedFile,
}

/// A root-owned, executable, non-group/world-writable regular tool.
fn find_in(layout: &Layout, name: &str) -> Result<PathBuf> {
    for dir in TOOL_DIRS {
        let logical = format!("{dir}/{name}");
        let Ok(m) = fs::metadata(layout.at(&logical)) else {
            continue;
        };
        if m.is_file() && m.uid() == host::euid() && m.mode() & 0o111 != 0 && m.mode() & 0o022 == 0
        {
            return Ok(PathBuf::from(logical));
        }
    }
    Err(format!(
        "claude_runtime_missing_{}",
        name.replace(['-', '.'], "_")
    ))
}

/// Canonical absolute path below the layout root.
pub(super) fn logical_canonical(layout: &Layout, actual: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(actual).map_err(|_| "claude_runtime_path_unavailable")?;
    let root = fs::canonicalize(layout.at("/")).map_err(|_| "claude_runtime_path_unavailable")?;
    let relative = canonical
        .strip_prefix(&root)
        .map_err(|_| "claude_runtime_path_untrusted")?;
    Ok(Path::new("/").join(relative))
}

/// Canonical, root-owned, single-link, executable file with safe ancestors,
/// hashed within `cap`. Returns the canonical path and its SHA-256.
pub(super) fn trusted_file(layout: &Layout, path: &Path, cap: u64) -> Result<PinnedFile> {
    let text = path.to_str().ok_or("claude_runtime_path_invalid")?;
    if !path.is_absolute() {
        return Err("claude_runtime_path_invalid".into());
    }
    let logical = logical_canonical(layout, &layout.at(text))?;
    let canonical = layout.at(logical.to_str().ok_or("claude_runtime_path_invalid")?);
    layout.safe_ancestors(&canonical)?;
    let (mut file, _) = host::open_regular(&canonical, cap)?;
    let mode = file
        .metadata()
        .map_err(|_| "claude_runtime_file_unavailable")?
        .mode();
    if mode & 0o111 == 0 {
        return Err("claude_runtime_file_not_executable".into());
    }
    Ok(PinnedFile {
        path: logical,
        sha256: host::sha256_reader(&mut file, cap)?,
    })
}

pub(super) fn check(layout: &Layout, exec: &dyn Exec) -> Result<Tools> {
    for tool in REQUIRED_TOOLS {
        find_in(layout, tool)?;
    }
    let systemctl = find_in(layout, "systemctl")?;
    let docker = find_in(layout, "docker")?;
    if !fs::metadata(layout.at(super::SHELL)).is_ok_and(|m| m.is_file()) {
        return Err("claude_runtime_missing_nologin".into());
    }
    let controllers = fs::read_to_string(layout.at("/sys/fs/cgroup/cgroup.controllers"))
        .map_err(|_| "claude_runtime_cgroup2_required")?;
    let available: Vec<&str> = controllers.split_whitespace().collect();
    if CONTROLLERS.iter().any(|c| !available.contains(c)) {
        return Err("claude_runtime_cgroup_controllers_unavailable".into());
    }
    // The user manager needs a session bus socket unit (dbus-user-session).
    if ![
        "/usr/lib/systemd/user/dbus.socket",
        "/lib/systemd/user/dbus.socket",
    ]
    .iter()
    .any(|p| layout.at(p).exists())
    {
        return Err("claude_runtime_missing_dbus_user_session".into());
    }
    let engine = trusted_file(layout, &docker, ENGINE_CAP)?;
    // Without --add-subids-for-system (e.g. Ubuntu 22.04) no safe automatic
    // subordinate range exists for a system user. Fail before any write.
    let help = exec.run(
        "useradd",
        &args(&["--help"]),
        Input::None,
        Duration::from_secs(10),
    )?;
    if help.code != 0 || !String::from_utf8_lossy(&help.stdout).contains("--add-subids-for-system")
    {
        return Err("claude_runtime_unsupported_useradd_subids".into());
    }
    Ok(Tools { systemctl, engine })
}
