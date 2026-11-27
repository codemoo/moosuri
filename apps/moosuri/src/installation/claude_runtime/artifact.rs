//! Pinned artifact acquisition. Bytes arrive in a private scratch directory
//! from a fixed HTTPS URL (curl through the Exec seam), and only a file whose
//! exact size and SHA-256 match the pin is atomically published as a 0755
//! root-owned artifact. An existing artifact is never replaced: it must already
//! match exactly (hash, size, owner, mode) on every run.
use super::{CONTROL, Pin, prereq};
use crate::installation::kiri::host::{self, Exec, Input, Layout, Result};
use std::{fs, os::unix::fs::MetadataExt, path::Path, time::Duration};
use uuid::Uuid;

pub(super) fn ensure(
    layout: &Layout,
    exec: &dyn Exec,
    pin: &Pin,
    dest: &str,
    limit: Duration,
) -> Result<prereq::PinnedFile> {
    if !layout.exists(&layout.at(dest))? {
        download(layout, exec, pin, dest, limit)?;
    }
    verify(layout, pin, dest)
}

fn verify(layout: &Layout, pin: &Pin, dest: &str) -> Result<prereq::PinnedFile> {
    fn conflict<E>(_: E) -> String {
        "claude_runtime_artifact_conflict".to_owned()
    }
    let file = prereq::trusted_file(layout, Path::new(dest), pin.max).map_err(conflict)?;
    let metadata = fs::metadata(layout.at(dest)).map_err(conflict)?;
    if file.path != Path::new(dest)
        || file.sha256 != pin.sha256
        || metadata.mode() & 0o7777 != 0o755
        || pin.size.is_some_and(|size| metadata.len() != size)
    {
        return Err("claude_runtime_artifact_conflict".into());
    }
    Ok(file)
}

fn download(
    layout: &Layout,
    exec: &dyn Exec,
    pin: &Pin,
    dest: &str,
    limit: Duration,
) -> Result<()> {
    let scratch = layout.at(&format!("{CONTROL}/downloads/{}", Uuid::new_v4().simple()));
    layout.ensure_dir(&scratch, 0o700)?;
    let result = fetch(layout, exec, pin, &scratch, &layout.at(dest), limit);
    let _ = fs::remove_file(scratch.join("payload"));
    let _ = fs::remove_dir(&scratch);
    result
}

fn fetch(
    layout: &Layout,
    exec: &dyn Exec,
    pin: &Pin,
    scratch: &Path,
    dest: &Path,
    limit: Duration,
) -> Result<()> {
    let payload = scratch.join("payload");
    let output = payload.to_str().ok_or("claude_runtime_path_invalid")?;
    let secs = limit.as_secs().to_string();
    let max = pin.max.to_string();
    // HTTPS only, no redirect, no curlrc/proxy environment, bounded time/size.
    let args = host::args(&[
        "--disable",
        "--fail",
        "--silent",
        "--proto",
        "=https",
        "--proto-redir",
        "=https",
        "--tlsv1.2",
        "--max-redirs",
        "0",
        "--connect-timeout",
        "30",
        "--max-time",
        secs.as_str(),
        "--max-filesize",
        max.as_str(),
        "--output",
        output,
        "--url",
        pin.url,
    ]);
    exec.run("curl", &args, Input::None, limit)?
        .ok("claude_runtime_download_failed")?;
    let (mut file, length) =
        host::open_regular(&payload, pin.max).map_err(|_| "claude_runtime_download_invalid")?;
    if pin.size.is_some_and(|size| size != length) {
        return Err("claude_runtime_download_invalid".into());
    }
    layout.write_stream(dest, &mut file, pin.max, pin.sha256, 0o755, &[])
}

/// Remove abandoned scratch directories left by a crash (best effort).
pub(super) fn sweep(layout: &Layout) {
    let Ok(entries) = fs::read_dir(layout.at(&format!("{CONTROL}/downloads"))) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if fs::symlink_metadata(&path).is_ok_and(|m| m.is_dir() && m.uid() == host::euid()) {
            let _ = fs::remove_file(path.join("payload"));
            let _ = fs::remove_dir(&path);
        }
    }
}
