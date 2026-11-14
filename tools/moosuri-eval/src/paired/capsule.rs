//! A prepared local copy is a prerequisite, not interchangeable with a source pin.
//! Markers protect operator mistakes; they do not authenticate a hostile local UID.
use crate::{EvalResult, baseline, digest};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path},
};
const COPY: &str = ".paired-copy-v1.json";
const READY: &str = ".paired-prepared-v1.json";
const PYTHON: &str = "/opt/homebrew/opt/python@3.13/bin/python3.13";
#[derive(Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct CopyMarker {
    version: u8,
    manifest: String,
    inventory: String,
    files: usize,
    bytes: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Prepared {
    version: u8,
    copy_sha256: String,
    runtime_sha256: String,
}
fn uid() -> EvalResult<u32> {
    tempfile::tempfile_in("/private/tmp")
        .and_then(|f| f.metadata())
        .map(|m| m.uid())
        .map_err(|_| "capsule_owner_probe")
}
pub(super) fn root(root: &Path) -> EvalResult<()> {
    if !root.is_absolute()
        || root.parent() != Some(Path::new("/private/tmp"))
        || !root
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with("moosuri-paired-subject-") && s.len() > 23)
    {
        return Err("execution_capsule_root");
    }
    let mut path = std::path::PathBuf::new();
    for part in root.components() {
        if matches!(part, Component::ParentDir | Component::CurDir) {
            return Err("execution_capsule_root");
        }
        path.push(part.as_os_str());
        if fs::symlink_metadata(&path)
            .map_err(|_| "execution_capsule_root")?
            .file_type()
            .is_symlink()
        {
            return Err("execution_capsule_symlink");
        }
    }
    let m = fs::symlink_metadata(root).map_err(|_| "execution_capsule_root")?;
    if !m.is_dir()
        || m.uid() != uid()?
        || m.mode() & 0o777 != 0o700
        || root
            .canonicalize()
            .map_err(|_| "execution_capsule_root")?
            .as_os_str()
            != root.as_os_str()
    {
        return Err("execution_capsule_owner_mode");
    }
    Ok(())
}
fn bytes(root: &Path, name: &str) -> EvalResult<Vec<u8>> {
    let p = root.join(name);
    let m = fs::symlink_metadata(&p).map_err(|_| "execution_capsule_marker")?;
    if !m.is_file()
        || m.uid() != uid()?
        || m.nlink() != 1
        || m.mode() & 0o077 != 0
        || m.len() > 4096
    {
        return Err("execution_capsule_marker");
    }
    fs::read(p).map_err(|_| "execution_capsule_marker")
}
fn write(root: &Path, name: &str, value: &impl Serialize) -> EvalResult<()> {
    let raw = serde_json::to_vec(value).map_err(|_| "capsule_marker_encode")?;
    let path = root.join(name);
    let mut f = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(&path)
        .map_err(|_| "capsule_marker_create")?;
    f.write_all(&raw)
        .and_then(|_| f.sync_all())
        .map_err(|_| "capsule_marker_write")?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o400))
        .map_err(|_| "capsule_marker_mode")?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| "capsule_marker_sync")
}
fn expected(pin: &baseline::PinReport) -> EvalResult<CopyMarker> {
    if !pin.success {
        return Err("execution_capsule_pin");
    }
    Ok(CopyMarker {
        version: 1,
        manifest: pin.source_manifest_sha256.into(),
        inventory: pin
            .baseline_source_sha256
            .clone()
            .ok_or("execution_capsule_pin")?,
        files: pin.verified_files,
        bytes: pin.verified_bytes,
    })
}
pub(super) fn record_copy(root_path: &Path) -> EvalResult<()> {
    root(root_path)?;
    write(root_path, COPY, &expected(&baseline::preflight(root_path))?)
}
pub(super) fn validate_copy(root_path: &Path) -> EvalResult<baseline::PinReport> {
    root(root_path)?;
    let pin = baseline::preflight(root_path);
    let marker: CopyMarker =
        serde_json::from_slice(&bytes(root_path, COPY)?).map_err(|_| "execution_capsule_marker")?;
    if marker != expected(&pin)? {
        return Err("execution_capsule_copy_mismatch");
    }
    Ok(pin)
}
fn runtime(root: &Path) -> EvalResult<String> {
    let venv = root.join("agent/.venv");
    let python = venv.join("bin/python");
    // uv's interpreter leaf symlinks are expected; no directory may redirect
    // into an original checkout, user environment or unrelated runtime.
    for d in [root.join("agent"), venv.clone(), venv.join("bin")] {
        let m = fs::symlink_metadata(d).map_err(|_| "capsule_runtime")?;
        if !m.is_dir() || m.file_type().is_symlink() {
            return Err("capsule_runtime");
        }
    }
    let expected = Path::new(PYTHON)
        .canonicalize()
        .map_err(|_| "capsule_runtime")?;
    if python.canonicalize().map_err(|_| "capsule_runtime")? != expected
        || !venv.join("pyvenv.cfg").is_file()
    {
        return Err("capsule_runtime");
    }
    let mut h = Sha256::new();
    let mut count = 0usize;
    let mut size = 0u64;
    fn walk(
        base: &Path,
        path: &Path,
        interpreter: &Path,
        h: &mut Sha256,
        count: &mut usize,
        size: &mut u64,
    ) -> EvalResult<()> {
        let m = fs::symlink_metadata(path).map_err(|_| "capsule_runtime")?;
        let name = path
            .strip_prefix(base)
            .map_err(|_| "capsule_runtime")?
            .to_str()
            .ok_or("capsule_runtime")?;
        if m.is_dir() {
            let mut children = fs::read_dir(path)
                .map_err(|_| "capsule_runtime")?
                .map(|e| e.map(|e| e.path()))
                .collect::<Result<Vec<_>, _>>()
                .map_err(|_| "capsule_runtime")?;
            children.sort();
            for p in children {
                walk(base, &p, interpreter, h, count, size)?
            }
            return Ok(());
        }
        *count += 1;
        if *count > 100000 {
            return Err("capsule_runtime_bound");
        }
        h.update((name.len() as u64).to_le_bytes());
        h.update(name.as_bytes());
        if m.file_type().is_symlink() {
            if path.parent().is_none_or(|p| p != base.join("bin"))
                || !path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| matches!(n, "python" | "python3" | "python3.13"))
                || path.canonicalize().map_err(|_| "capsule_runtime")? != interpreter
            {
                return Err("capsule_runtime_symlink");
            }
            h.update(b"fixed-interpreter");
            return Ok(());
        }
        if !m.is_file() {
            return Err("capsule_runtime");
        }
        *size = size.checked_add(m.len()).ok_or("capsule_runtime_bound")?;
        if *size > 512 * 1024 * 1024 {
            return Err("capsule_runtime_bound");
        }
        h.update(m.len().to_le_bytes());
        let mut f = fs::File::open(path).map_err(|_| "capsule_runtime")?;
        let mut b = [0; 65536];
        loop {
            let n = f.read(&mut b).map_err(|_| "capsule_runtime")?;
            if n == 0 {
                break;
            }
            h.update(&b[..n]);
        }
        Ok(())
    }
    walk(&venv, &venv, &expected, &mut h, &mut count, &mut size)?;
    let mut f = fs::File::open(expected).map_err(|_| "capsule_runtime")?;
    let mut b = [0; 65536];
    loop {
        let n = f.read(&mut b).map_err(|_| "capsule_runtime")?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    Ok(format!("{:x}", h.finalize()))
}
pub(super) fn record_prepared(root_path: &Path) -> EvalResult<()> {
    validate_copy(root_path)?;
    write(
        root_path,
        READY,
        &Prepared {
            version: 1,
            copy_sha256: digest(&bytes(root_path, COPY)?),
            runtime_sha256: runtime(root_path)?,
        },
    )
}
pub(super) fn validate(root_path: &Path) -> EvalResult<baseline::PinReport> {
    validate_with(root_path, baseline::preflight, runtime)
}
fn validate_with(
    root_path: &Path,
    pin: impl FnOnce(&Path) -> baseline::PinReport,
    runtime: impl FnOnce(&Path) -> EvalResult<String>,
) -> EvalResult<baseline::PinReport> {
    // Root checks precede even source pinning: a matching original is never an
    // execution candidate and never gains a blanket profile read exception.
    root(root_path)?;
    let pin = pin(root_path);
    let copy = bytes(root_path, COPY)?;
    let marker: CopyMarker =
        serde_json::from_slice(&copy).map_err(|_| "execution_capsule_marker")?;
    if marker != expected(&pin)? {
        return Err("execution_capsule_copy_mismatch");
    }
    let prepared: Prepared = serde_json::from_slice(&bytes(root_path, READY)?)
        .map_err(|_| "execution_capsule_unprepared")?;
    if prepared.version != 1
        || prepared.copy_sha256 != digest(&copy)
        || prepared.runtime_sha256 != runtime(root_path)?
    {
        return Err("execution_capsule_runtime_mismatch");
    }
    Ok(pin)
}
#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    fn pin(root: &Path) -> baseline::PinReport {
        let mut p = baseline::preflight(Path::new("/nonexistent-synthetic-root"));
        let raw = fs::read(root.join("synthetic-source")).unwrap();
        p.success = raw == b"synthetic pinned runtime";
        p.baseline_source_sha256 = Some(digest(&raw));
        p.verified_files = 1;
        p.verified_bytes = raw.len() as u64;
        p
    }
    #[test]
    fn paired_capsule_matching_pin_never_admits_original_or_unprepared_root() {
        let original = tempfile::Builder::new()
            .prefix("original-synthetic-")
            .tempdir_in("/private/tmp")
            .unwrap();
        fs::write(
            original.path().join("synthetic-source"),
            b"synthetic pinned runtime",
        )
        .unwrap();
        assert!(pin(original.path()).success);
        assert!(validate_with(original.path(), pin, |_| Ok("runtime".into())).is_err());
        let prepared = tempfile::Builder::new()
            .prefix("moosuri-paired-subject-")
            .tempdir_in("/private/tmp")
            .unwrap();
        fs::set_permissions(prepared.path(), fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(
            prepared.path().join("synthetic-source"),
            b"synthetic pinned runtime",
        )
        .unwrap();
        assert!(pin(prepared.path()).success);
        write(
            prepared.path(),
            COPY,
            &expected(&pin(prepared.path())).unwrap(),
        )
        .unwrap();
        assert!(validate_with(prepared.path(), pin, |_| Ok("runtime".into())).is_err());
        write(
            prepared.path(),
            READY,
            &Prepared {
                version: 1,
                copy_sha256: digest(&bytes(prepared.path(), COPY).unwrap()),
                runtime_sha256: "runtime".into(),
            },
        )
        .unwrap();
        assert!(validate_with(prepared.path(), pin, |_| Ok("runtime".into())).is_ok());
        assert!(validate_with(prepared.path(), pin, |_| Ok("changed-runtime".into())).is_err());
        fs::set_permissions(prepared.path(), fs::Permissions::from_mode(0o755)).unwrap();
        assert!(validate_with(prepared.path(), pin, |_| Ok("runtime".into())).is_err());
    }
}
