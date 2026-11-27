//! Exact root-owned systemd system service for one owner's engine worker, and
//! the fixed commands run as the dedicated worker identity. No shell and no
//! caller-supplied command text: every value is derived from root evidence.
use crate::installation::kiri::host::{self, Exec, Input, Layout, Result, args};
use std::{fs, os::unix::fs::MetadataExt, path::Path, time::Duration};

pub(super) fn name(uid: u32) -> String {
    format!("moosuri-claude-worker-{uid}.service")
}
pub(super) fn socket_dir(uid: u32) -> String {
    format!("/run/moosuri-claude-worker-{uid}")
}
fn path(uid: u32) -> String {
    format!("/etc/systemd/system/{}", name(uid))
}

fn plain(s: &str) -> bool {
    !s.is_empty()
        && s.starts_with('/')
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"/._+-".contains(&b))
}

/// Mount unit names produced by the storage helper (`\xNN` escapes allowed).
pub(super) fn safe_mount_unit(s: &str) -> bool {
    s.len() <= 255
        && s.ends_with(".mount")
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-\\:".contains(&b))
}

/// The complete unit. Exact User/Group, no supplementary groups, private
/// 0700 runtime directory for the socket, and ordering after the rootless user
/// manager and this owner's authentication filesystem.
pub(super) fn render(
    uid: u32,
    gid: u32,
    filesystem_unit: &str,
    helper: &Path,
    config: &Path,
) -> Result<String> {
    let (helper, config) = (
        helper.to_str().ok_or("claude_connection_path_invalid")?,
        config.to_str().ok_or("claude_connection_path_invalid")?,
    );
    if uid == 0 || gid == 0 || !plain(helper) || !plain(config) || !safe_mount_unit(filesystem_unit)
    {
        return Err("claude_connection_unit_input_invalid".into());
    }
    let runtime = format!("moosuri-claude-worker-{uid}");
    Ok(format!(
        "[Unit]\n\
         Description=Moosuri Claude engine worker (identity {uid})\n\
         Requires=user@{uid}.service {filesystem_unit}\n\
         After=user@{uid}.service {filesystem_unit}\n\
         StartLimitIntervalSec=0\n\
         \n\
         [Service]\n\
         Type=simple\n\
         User={uid}\n\
         Group={gid}\n\
         SupplementaryGroups=\n\
         RuntimeDirectory={runtime}\n\
         RuntimeDirectoryMode=0700\n\
         RuntimeDirectoryPreserve=no\n\
         WorkingDirectory=/\n\
         NoNewPrivileges=yes\n\
         PrivateTmp=yes\n\
         UMask=0077\n\
         LimitCORE=0\n\
         MemoryMax=268435456\n\
         TasksMax=64\n\
         CPUQuota=100%\n\
         Environment=PATH=/usr/sbin:/usr/bin:/sbin:/bin\n\
         ExecStart={helper} engine-login-worker --config {config}\n\
         Restart=on-failure\n\
         RestartSec=10\n\
         TimeoutStopSec=30\n\
         \n\
         [Install]\n\
         WantedBy=multi-user.target\n"
    ))
}

/// `runuser -u <worker> -- env -i PATH=… <tail…>`: dedicated identity only.
pub(super) fn as_worker(username: &str, tail: &[String]) -> Vec<String> {
    let mut a = args(&[
        "-u",
        username,
        "--",
        "env",
        "-i",
        "PATH=/usr/sbin:/usr/bin:/sbin:/bin",
    ]);
    a.extend_from_slice(tail);
    a
}

fn ctl(exec: &dyn Exec, rest: &[&str], limit: u64) -> Result<host::Out> {
    exec.run(
        "systemctl",
        &args(rest),
        Input::None,
        Duration::from_secs(limit),
    )
}
fn ctl_ok(exec: &dyn Exec, rest: &[&str], limit: u64, reason: &str) -> Result<()> {
    ctl(exec, rest, limit)?.ok(reason).map(|_| ())
}

/// Install the exact unit (an existing different file is a conflict) and start it.
pub(super) fn install_and_start(
    layout: &Layout,
    exec: &dyn Exec,
    uid: u32,
    text: &str,
) -> Result<()> {
    layout
        .write_file(&layout.at(&path(uid)), text.as_bytes(), 0o644, &[])
        .map_err(|_| "claude_connection_unit_conflict")?;
    ctl_ok(
        exec,
        &["daemon-reload"],
        60,
        "claude_connection_unit_reload_failed",
    )?;
    let unit = name(uid);
    ctl_ok(
        exec,
        &["enable", &unit],
        30,
        "claude_connection_unit_enable_failed",
    )?;
    ctl_ok(
        exec,
        &["start", &unit],
        120,
        "claude_connection_worker_start_failed",
    )
}

pub(super) fn active_state(exec: &dyn Exec, uid: u32) -> Result<String> {
    let out = ctl(exec, &["show", &name(uid), "--property=ActiveState"], 30)?
        .ok("claude_connection_unit_state_unavailable")?;
    String::from_utf8_lossy(&out)
        .lines()
        .find_map(|l| l.strip_prefix("ActiveState="))
        .map(str::to_owned)
        .ok_or_else(|| "claude_connection_unit_state_unavailable".into())
}

pub(super) fn mount_active(exec: &dyn Exec, unit: &str) -> Result<()> {
    if !safe_mount_unit(unit) {
        return Err("claude_connection_storage_invalid".into());
    }
    ctl_ok(
        exec,
        &["is-active", "--quiet", unit],
        30,
        "claude_connection_storage_not_mounted",
    )
}

fn stopped(exec: &dyn Exec, unit: &str) -> Result<()> {
    let bytes = ctl(
        exec,
        &["show", unit, "--property=ActiveState,MainPID,ControlPID"],
        30,
    )?
    .ok("claude_connection_worker_stop_unconfirmed")?;
    let text = String::from_utf8_lossy(&bytes);
    let value = |key: &str| text.lines().find_map(|line| line.strip_prefix(key));
    if !matches!(value("ActiveState="), Some("inactive" | "failed"))
        || value("MainPID=") != Some("0")
        || value("ControlPID=") != Some("0")
    {
        return Err("claude_connection_worker_stop_unconfirmed".into());
    }
    Ok(())
}

/// Stop and remove only this uid's unit, and only if it is byte-exact ours.
/// A failed stop keeps the unit (and the caller keeps its cleanup marker).
pub(super) fn remove(
    layout: &Layout,
    exec: &dyn Exec,
    uid: u32,
    expected_sha256: &str,
) -> Result<()> {
    let actual = layout.at(&path(uid));
    let unit = name(uid);
    match fs::symlink_metadata(&actual) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            // A loaded copy may outlive its removed file. A failed stop is not
            // evidence of absence; prove both inactive state and zero processes.
            let _ = ctl(exec, &["stop", &unit], 60);
            return stopped(exec, &unit);
        }
        Err(_) => return Err("claude_connection_unit_conflict".into()),
        Ok(m) => {
            if !m.is_file()
                || m.nlink() != 1
                || m.uid() != host::euid()
                || host::sha256_file(&actual, 4096)? != expected_sha256
            {
                return Err("claude_connection_unit_conflict".into());
            }
        }
    }
    ctl_ok(
        exec,
        &["stop", &unit],
        60,
        "claude_connection_worker_stop_failed",
    )?;
    stopped(exec, &unit)?;
    let _ = ctl(exec, &["disable", &unit], 30);
    fs::remove_file(&actual).map_err(|_| "claude_connection_unit_remove_failed")?;
    ctl_ok(
        exec,
        &["daemon-reload"],
        60,
        "claude_connection_unit_reload_failed",
    )?;
    let _ = ctl(exec, &["reset-failed", &unit], 30);
    Ok(())
}
