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
    pub system_subids: bool,
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
    // Ubuntu22 assigns subordinate ranges for regular users. A dedicated
    // nologin account below the member pool can use that native allocator.
    let help = exec.run(
        "useradd",
        &args(&["--help"]),
        Input::None,
        Duration::from_secs(10),
    )?;
    let help_text = String::from_utf8_lossy(&help.stdout);
    let system_subids = help_text.contains("--add-subids-for-system");
    if help.code != 0
        || (!system_subids
            && !["--uid", "--no-create-home", "--user-group"]
                .iter()
                .all(|flag| help_text.contains(flag)))
    {
        return Err("claude_runtime_unsupported_useradd_subids".into());
    }
    if !system_subids {
        let path = layout.at("/etc/login.defs");
        let (mut file, _) = host::open_regular(&path, 128 * 1024)
            .map_err(|_| "claude_runtime_subid_allocator_unconfigured")?;
        let mut text = String::new();
        std::io::Read::read_to_string(&mut file, &mut text)
            .map_err(|_| "claude_runtime_subid_allocator_unconfigured")?;
        for prefix in ["SUB_UID", "SUB_GID"] {
            let value = |suffix: &str| -> Option<u64> {
                let key = format!("{prefix}_{suffix}");
                let values: Vec<_> = text
                    .lines()
                    .filter_map(|line| {
                        let mut fields = line.split('#').next()?.split_whitespace();
                        (fields.next()? == key)
                            .then(|| fields.next()?.parse::<u64>().ok())
                            .flatten()
                    })
                    .collect();
                (values.len() == 1).then(|| values[0])
            };
            if !matches!((value("MIN"), value("MAX"), value("COUNT")),
                (Some(min), Some(max), Some(count)) if min > 0 && count >= 65536 && max >= min && max-min >= count)
            {
                return Err("claude_runtime_subid_allocator_unconfigured".into());
            }
        }
    }
    Ok(Tools {
        systemctl,
        engine,
        system_subids,
    })
}
