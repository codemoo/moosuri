//! Protected worker inputs. Directory: root-owned, group = the worker's gid,
//! mode 0710 (traverse only). Files: root-owned, same group, mode 0440. The worker
//! can read them but can never create, replace, rename or truncate any of them,
//! so nothing here can be turned into accepted evidence by the worker.
use super::record::{Attempt, Hashes};
use crate::engine_login::Owner;
use crate::installation::kiri::host::{self, Layout, Result};
use serde::Serialize;
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};
use uuid::Uuid;

pub(super) const BASE: &str = "/var/lib/moosuri-claude-config";
pub(super) const PROFILE: &str = "profile.json";
pub(super) const EMAIL: &str = "owner-email";
pub(super) const WORKER: &str = "worker.json";
pub(super) const CANDIDATE: &str = "profile-candidate.json";
const FILE_CAP: u64 = 16384;

pub(super) fn dir(environment: &Uuid) -> PathBuf {
    Path::new(BASE).join(environment.hyphenated().to_string())
}
pub(super) fn path(environment: &Uuid, name: &str) -> PathBuf {
    dir(environment).join(name)
}

fn chown_group(file: &File, gid: u32) -> Result<()> {
    if unsafe { libc::fchown(file.as_raw_fd(), 0, gid) } == 0 {
        Ok(())
    } else {
        Err("claude_connection_config_chown_failed".into())
    }
}

/// Create (or accept the identical) `BASE/<env>` as root:gid 0710.
pub(super) fn ensure_dir(layout: &Layout, environment: &Uuid, gid: u32) -> Result<()> {
    layout.ensure_dir(&layout.at(BASE), 0o755)?;
    let actual = layout.at(dir(environment)
        .to_str()
        .ok_or("claude_connection_path_invalid")?);
    layout.ensure_dir(&actual, 0o710)?;
    let handle = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&actual)
        .map_err(|_| "claude_connection_config_untrusted")?;
    chown_group(&handle, gid)?;
    verify_dir(layout, environment, gid)
}

pub(super) fn verify_dir(layout: &Layout, environment: &Uuid, gid: u32) -> Result<()> {
    let actual = layout.at(dir(environment)
        .to_str()
        .ok_or("claude_connection_path_invalid")?);
    let m = fs::symlink_metadata(&actual).map_err(|_| "claude_connection_config_untrusted")?;
    let base =
        fs::symlink_metadata(layout.at(BASE)).map_err(|_| "claude_connection_config_untrusted")?;
    if !m.is_dir()
        || m.uid() != 0
        || m.gid() != gid
        || m.mode() & 0o7777 != 0o710
        || !base.is_dir()
        || base.uid() != 0
        || base.mode() & 0o022 != 0
        || fs::canonicalize(&actual).ok().as_deref() != Some(actual.as_path())
    {
        return Err("claude_connection_config_untrusted".into());
    }
    Ok(())
}

/// Write one file exactly once (an identical replay is accepted, different
/// bytes are not) and return its SHA-256.
pub(super) fn write(
    layout: &Layout,
    environment: &Uuid,
    gid: u32,
    name: &str,
    bytes: &[u8],
) -> Result<String> {
    if bytes.is_empty() || bytes.len() as u64 > FILE_CAP {
        return Err("claude_connection_config_invalid".into());
    }
    let actual = layout.at(path(environment, name)
        .to_str()
        .ok_or("claude_connection_path_invalid")?);
    layout
        .write_file(&actual, bytes, 0o440, &[])
        .map_err(|_| "claude_connection_config_conflict")?;
    // The directory is root-only for writes, so no one else can have swapped it;
    // the descriptor still refuses a final symlink.
    let handle = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&actual)
        .map_err(|_| "claude_connection_config_untrusted")?;
    chown_group(&handle, gid)?;
    let digest = host::sha256_bytes(bytes);
    verify_file(layout, environment, gid, name, &digest)?;
    Ok(digest)
}

/// Descriptor-based recheck: owner, group, exact mode, link count, content.
pub(super) fn verify_file(
    layout: &Layout,
    environment: &Uuid,
    gid: u32,
    name: &str,
    expected: &str,
) -> Result<()> {
    let actual = layout.at(path(environment, name)
        .to_str()
        .ok_or("claude_connection_path_invalid")?);
    let handle = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(&actual)
        .map_err(|_| "claude_connection_config_untrusted")?;
    let m = handle
        .metadata()
        .map_err(|_| "claude_connection_config_untrusted")?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != 0
        || m.gid() != gid
        || m.mode() & 0o7777 != 0o440
        || m.len() == 0
        || m.len() > FILE_CAP
    {
        return Err("claude_connection_config_untrusted".into());
    }
    let mut handle = handle;
    if host::sha256_reader(&mut handle, FILE_CAP)? != expected {
        return Err("claude_connection_config_changed".into());
    }
    Ok(())
}

pub(super) fn verify_all(
    layout: &Layout,
    environment: &Uuid,
    gid: u32,
    hashes: &Hashes,
) -> Result<()> {
    verify_dir(layout, environment, gid)?;
    verify_file(layout, environment, gid, PROFILE, &hashes.profile)?;
    verify_file(layout, environment, gid, EMAIL, &hashes.email)?;
    verify_file(layout, environment, gid, WORKER, &hashes.worker)
}

/// Remove this attempt's root-owned directory. Absent is fine; anything that
/// is not a root-owned regular file is left alone and reported.
pub(super) fn remove_all(layout: &Layout, environment: &Uuid) -> Result<()> {
    let actual = layout.at(dir(environment)
        .to_str()
        .ok_or("claude_connection_path_invalid")?);
    let entries = match fs::read_dir(&actual) {
        Ok(e) => e,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("claude_connection_config_untrusted".into()),
    };
    let top = fs::symlink_metadata(&actual).map_err(|_| "claude_connection_config_untrusted")?;
    if !top.is_dir() || top.uid() != 0 {
        return Err("claude_connection_config_untrusted".into());
    }
    for entry in entries {
        let entry = entry.map_err(|_| "claude_connection_config_untrusted")?;
        let m =
            fs::symlink_metadata(entry.path()).map_err(|_| "claude_connection_config_untrusted")?;
        if !m.is_file() || m.uid() != 0 || m.nlink() != 1 {
            return Err("claude_connection_config_untrusted".into());
        }
        fs::remove_file(entry.path()).map_err(|_| "claude_connection_config_remove_failed")?;
    }
    fs::remove_dir(&actual).map_err(|_| "claude_connection_config_remove_failed".into())
}

pub(super) fn email_bytes(email: &str) -> Vec<u8> {
    format!("{email}\n").into_bytes()
}

#[derive(Serialize)]
struct Settings<'a> {
    profile_file: &'a Path,
    profile_digest: &'a str,
    environment_id: Uuid,
    expected_email_file: &'a Path,
}
#[derive(Serialize)]
struct Adapter<'a> {
    #[serde(rename = "type")]
    kind: &'static str,
    settings: Settings<'a>,
}
#[derive(Serialize)]
struct WorkerConfig<'a> {
    version: u32,
    socket_directory: String,
    owner: Owner,
    connection_revision: Uuid,
    model: &'a str,
    effort: Option<&'a str>,
    models: [(); 0],
    adapter: Adapter<'a>,
}

/// The engine worker's `Config`. Every path is derived from the attempt's
/// own protected directory and uid; no caller path or authority is copied.
pub(super) fn render_worker(
    attempt: &Attempt,
    owner: &Owner,
    uid: u32,
    profile_digest: &str,
) -> Result<Vec<u8>> {
    if uid == 0
        || !host::is_hex(profile_digest, 64)
        || attempt.environment_id.get_version_num() != 4
    {
        return Err("claude_connection_config_invalid".into());
    }
    let (profile, email) = (
        path(&attempt.environment_id, PROFILE),
        path(&attempt.environment_id, EMAIL),
    );
    serde_json::to_vec(&WorkerConfig {
        version: 1,
        socket_directory: super::unit::socket_dir(uid),
        owner: *owner,
        connection_revision: attempt.request_id,
        model: &attempt.model,
        effort: attempt.effort.as_deref(),
        models: [],
        adapter: Adapter {
            kind: "claude_code",
            settings: Settings {
                profile_file: &profile,
                profile_digest,
                environment_id: attempt.environment_id,
                expected_email_file: &email,
            },
        },
    })
    .map_err(|_| "claude_connection_config_invalid".into())
}
