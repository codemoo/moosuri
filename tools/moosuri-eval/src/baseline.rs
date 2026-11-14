//! Content preflight for the private first-release reference implementation.
//! Never imports, executes, publishes or reads operational data from that tree.
use crate::digest;
use serde::Serialize;
use std::{collections::BTreeSet, fs, io::Read, path::Path};

const MANIFEST: &str = include_str!("../../../audit/source-manifest.tsv");
const MANIFEST_SHA256: &str = "d1bb74b49e4669e7b88fdaa75f30352576a24efd5df33ab0343931ae0bf8d469";
const RUNTIME_ROOTS: [&str; 2] = ["agent/src/musuri_agent", "agent/migrations"];
const DEPENDENCIES: [&str; 3] = [
    "agent/pyproject.toml",
    "agent/uv.lock",
    "agent/requirements.lock",
];

#[derive(Clone)]
struct Entry {
    name: String,
    sha256: String,
    size: u64,
}

#[derive(Serialize)]
pub struct PinReport {
    pub format_version: u32,
    pub evidence_class: &'static str,
    pub source_manifest_sha256: &'static str,
    pub baseline_source_sha256: Option<String>,
    pub expected_files: usize,
    pub verified_files: usize,
    pub verified_bytes: u64,
    pub checks: Vec<PinCheck>,
    pub runtime_status: &'static str,
    pub fixture_adapter_status: &'static str,
    pub model_profile_status: &'static str,
    pub non_inferiority_status: &'static str,
    pub success: bool,
}

#[derive(Serialize)]
pub struct PinCheck {
    /// Fixed codes only. Never echo source bytes, paths or operating configuration.
    pub code: &'static str,
    pub passed: bool,
}

fn selected(name: &str) -> bool {
    DEPENDENCIES.contains(&name)
        || RUNTIME_ROOTS.iter().any(|root| {
            name.strip_prefix(root)
                .is_some_and(|rest| rest.starts_with('/'))
        })
}

fn entries(manifest: &str) -> Result<Vec<Entry>, &'static str> {
    let mut lines = manifest.lines();
    if lines.next() != Some("source\tsha256\tbytes") {
        return Err("manifest_header");
    }
    let mut entries = vec![];
    let mut names = BTreeSet::new();
    for line in lines {
        let fields = line.split('\t').collect::<Vec<_>>();
        if fields.len() != 3 {
            return Err("manifest_row");
        }
        if !selected(fields[0]) {
            continue;
        }
        if fields[0]
            .split('/')
            .any(|s| s.is_empty() || s == "." || s == "..")
            || fields[0].contains('\\')
            || !names.insert(fields[0])
            || fields[1].len() != 64
            || !fields[1]
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err("manifest_entry");
        }
        let size = fields[2].parse::<u64>().map_err(|_| "manifest_size")?;
        if size > 4 * 1024 * 1024 {
            return Err("manifest_size");
        }
        entries.push(Entry {
            name: fields[0].into(),
            sha256: fields[1].into(),
            size,
        });
    }
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(entries)
}

fn regular_path(root: &Path, name: &str) -> bool {
    let mut path = root.to_path_buf();
    let mut parts = name.split('/').peekable();
    while let Some(part) = parts.next() {
        path.push(part);
        let Ok(metadata) = fs::symlink_metadata(&path) else {
            return false;
        };
        if metadata.file_type().is_symlink()
            || if parts.peek().is_some() {
                !metadata.is_dir()
            } else {
                !metadata.is_file()
            }
        {
            return false;
        }
    }
    true
}

fn inventory(root: &Path, relative: &str, found: &mut BTreeSet<String>) -> bool {
    if relative.split('/').count() > 16 || found.len() > 149 {
        return false;
    }
    let Ok(children) = fs::read_dir(root.join(relative)) else {
        return false;
    };
    for child in children {
        let Ok(child) = child else { return false };
        let Some(name) = child.file_name().to_str().map(str::to_owned) else {
            return false;
        };
        // Python bytecode is not part of the reference. A runtime adapter must
        // start from a fresh allowlisted copy, never import these cached files.
        if name == "__pycache__" {
            continue;
        }
        let Ok(kind) = child.file_type() else {
            return false;
        };
        let relative = format!("{relative}/{name}");
        if kind.is_dir() {
            if !inventory(root, &relative, found) {
                return false;
            }
        } else if kind.is_file() {
            found.insert(relative);
        } else {
            return false;
        }
    }
    true
}

fn inspect(root: &Path, expected: &[Entry]) -> (usize, u64, Vec<PinCheck>) {
    let mut verified = 0;
    let mut bytes = 0;
    let mut regular = true;
    let mut contents = true;
    for entry in expected {
        if !regular_path(root, &entry.name) {
            regular = false;
            contents = false;
            continue;
        }
        let path = root.join(&entry.name);
        let matched = (|| -> std::io::Result<bool> {
            let file = fs::File::open(path)?;
            if file.metadata()?.len() != entry.size {
                return Ok(false);
            }
            let mut raw = vec![];
            file.take(entry.size + 1).read_to_end(&mut raw)?;
            Ok(raw.len() as u64 == entry.size && digest(&raw) == entry.sha256)
        })()
        .unwrap_or(false);
        if matched {
            verified += 1;
            bytes += entry.size;
        } else {
            contents = false;
        }
    }
    let mut found = BTreeSet::new();
    let complete = RUNTIME_ROOTS
        .iter()
        .all(|relative| inventory(root, relative, &mut found));
    let runtime: BTreeSet<_> = expected
        .iter()
        .filter(|entry| !DEPENDENCIES.contains(&entry.name.as_str()))
        .map(|entry| entry.name.clone())
        .collect();
    (
        verified,
        bytes,
        vec![
            PinCheck {
                code: "regular_allowlisted_files",
                passed: regular,
            },
            PinCheck {
                code: "source_and_lock_bytes_match",
                passed: contents,
            },
            PinCheck {
                code: "no_unpinned_runtime_files",
                passed: complete && found == runtime,
            },
        ],
    )
}

/// Verifies only source bytes against the historical audit. It intentionally
/// makes no Git-cleanliness, runtime, model-profile or benchmark claim.
pub fn preflight(root: &Path) -> PinReport {
    let expected = entries(MANIFEST).unwrap_or_default();
    let pinned = digest(MANIFEST.as_bytes()) == MANIFEST_SHA256
        && expected.len() == 149
        && expected.iter().map(|e| e.size).sum::<u64>() == 1_750_513;
    let (verified_files, verified_bytes, mut checks) = if pinned {
        inspect(root, &expected)
    } else {
        (0, 0, vec![])
    };
    checks.push(PinCheck {
        code: "historical_manifest_pin",
        passed: pinned,
    });
    let success = checks.iter().all(|check| check.passed);
    // The canonical selected manifest binds names, sizes and each source hash.
    let identity = expected
        .iter()
        .map(|e| format!("{}\t{}\t{}\n", e.name, e.sha256, e.size))
        .collect::<String>();
    PinReport {
        format_version: 1,
        evidence_class: "private_legacy_content_preflight",
        source_manifest_sha256: MANIFEST_SHA256,
        baseline_source_sha256: success.then(|| digest(identity.as_bytes())),
        expected_files: expected.len(),
        verified_files,
        verified_bytes,
        checks,
        runtime_status: "not_run",
        fixture_adapter_status: "not_run",
        model_profile_status: "not_run",
        non_inferiority_status: "not_run",
        success,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> (tempfile::TempDir, Vec<Entry>) {
        let dir = tempfile::tempdir().unwrap();
        let names = [
            "agent/src/musuri_agent/runtime.py",
            "agent/migrations/001_schema.sql",
            "agent/uv.lock",
        ];
        let mut manifest = "source\tsha256\tbytes\n".to_owned();
        for name in names {
            let path = dir.path().join(name);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(&path, b"synthetic-reference").unwrap();
            manifest += &format!("{name}\t{}\t19\n", digest(b"synthetic-reference"));
        }
        (dir, entries(&manifest).unwrap())
    }

    #[test]
    fn rejects_changed_missing_and_unpinned_source_without_executing_it() {
        let (dir, expected) = fixture();
        assert!(inspect(dir.path(), &expected).2.iter().all(|c| c.passed));
        let source = dir.path().join("agent/src/musuri_agent/runtime.py");
        fs::write(&source, b"changed--reference!").unwrap();
        let (verified, _, checks) = inspect(dir.path(), &expected);
        assert_eq!(verified, 2);
        assert!(!checks[1].passed);
        fs::write(&source, b"synthetic-reference").unwrap();
        fs::write(source.with_file_name("injected.py"), b"do not execute me").unwrap();
        assert!(!inspect(dir.path(), &expected).2[2].passed);
        fs::remove_file(&source).unwrap();
        assert!(!inspect(dir.path(), &expected).2[0].passed);
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlinked_files_and_directories() {
        use std::os::unix::fs::symlink;
        let (dir, expected) = fixture();
        let path = dir.path().join("agent/uv.lock");
        let outside = tempfile::tempdir().unwrap();
        fs::write(outside.path().join("canary"), b"synthetic-reference").unwrap();
        fs::remove_file(&path).unwrap();
        symlink(outside.path().join("canary"), &path).unwrap();
        assert!(!inspect(dir.path(), &expected).2[0].passed);
        fs::remove_file(&path).unwrap();
        fs::write(path, b"synthetic-reference").unwrap();
        fs::rename(
            dir.path().join("agent/migrations"),
            dir.path().join("original-migrations"),
        )
        .unwrap();
        symlink(
            dir.path().join("original-migrations"),
            dir.path().join("agent/migrations"),
        )
        .unwrap();
        assert!(!inspect(dir.path(), &expected).2[0].passed);
    }

    #[test]
    fn public_report_never_claims_runtime_or_emits_private_path() {
        let dir = tempfile::tempdir().unwrap();
        let report = preflight(dir.path());
        assert!(!report.success);
        assert_eq!(report.expected_files, 149);
        assert_eq!(report.baseline_source_sha256, None);
        assert_eq!(report.runtime_status, "not_run");
        let output = serde_json::to_string(&report).unwrap();
        assert!(!output.contains(dir.path().to_str().unwrap()));
        assert!(!output.contains("runtime.py"));
    }
}
