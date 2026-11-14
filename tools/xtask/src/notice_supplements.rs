//! Explicit offline operator-reviewed notice text; never authenticated package evidence.
use super::Check;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::Read,
    path::{Path, PathBuf},
};
const CAP: usize = 8 * 1024 * 1024;
fn invalid() -> String {
    "invalid, changed or unmatched offline notice supplement".into()
}
fn hash(b: &[u8]) -> String {
    format!("{:x}", Sha256::digest(b))
}
fn hex(s: &str, n: usize) -> bool {
    s.len() == n
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
fn relative(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 1024
        && !s.contains(['\\', ':'])
        && !s.chars().any(char::is_control)
        && s.split('/').count() <= 32
        && s.split('/')
            .all(|p| !p.is_empty() && !matches!(p, "." | ".."))
}
fn table<'a>(v: &'a toml::Value, keys: &[&str]) -> Check<&'a toml::value::Table> {
    let t = v.as_table().ok_or_else(invalid)?;
    if t.len() != keys.len() || !keys.iter().all(|k| t.contains_key(*k)) {
        return Err(invalid());
    }
    Ok(t)
}
fn string<'a>(t: &'a toml::value::Table, key: &str, max: usize) -> Check<&'a str> {
    let s = t
        .get(key)
        .and_then(toml::Value::as_str)
        .ok_or_else(invalid)?;
    if s.len() > max || s.contains('\0') {
        return Err(invalid());
    }
    Ok(s)
}
fn repository(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("https://") else {
        return false;
    };
    let Some((host, path)) = rest.split_once('/') else {
        return false;
    };
    !host.is_empty()
        && host.contains('.')
        && host
            .split('.')
            .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
        && relative(path)
        && path
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._/".contains(&b))
}
fn read_manifest(path: &Path) -> Check<Vec<u8>> {
    let raw = path.to_str().ok_or_else(invalid)?;
    if !path.is_absolute()
        || raw.len() > 4096
        || raw.contains('\\')
        || raw.chars().any(char::is_control)
        || raw
            .split('/')
            .skip(1)
            .any(|p| p.is_empty() || matches!(p, "." | ".."))
    {
        return Err(invalid());
    }
    let parts = raw.split('/').skip(1).collect::<Vec<_>>();
    let mut current = PathBuf::from("/");
    for (i, part) in parts.iter().enumerate() {
        current.push(part);
        let meta = fs::symlink_metadata(&current).map_err(|_| invalid())?;
        if meta.file_type().is_symlink()
            || (i + 1 == parts.len() && (!meta.is_file() || meta.len() > CAP as u64))
            || (i + 1 < parts.len() && !meta.is_dir())
        {
            return Err(invalid());
        }
    }
    let mut b = Vec::new();
    fs::File::open(path)
        .map_err(|_| invalid())?
        .take(CAP as u64 + 1)
        .read_to_end(&mut b)
        .map_err(|_| invalid())?;
    if b.len() > CAP {
        return Err(invalid());
    }
    Ok(b)
}
pub(super) fn options(args: &[String]) -> Check<(Option<PathBuf>, bool)> {
    let mut manifest = None;
    let mut complete = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--supplement" if manifest.is_none() => {
                i += 1;
                manifest = Some(PathBuf::from(args.get(i).ok_or_else(invalid)?));
            }
            "--require-complete" if !complete => complete = true,
            _ => return Err(invalid()),
        }
        i += 1;
    }
    Ok((manifest, complete))
}
pub(super) fn attach(report: &mut Value, path: &Path, ecosystem: &str) -> Check<()> {
    let bytes = read_manifest(path)?;
    // TOML rejects duplicate keys/tables, unlike an ordinary JSON Value object.
    let source: toml::Value = toml::from_str(std::str::from_utf8(&bytes).map_err(|_| invalid())?)
        .map_err(|_| invalid())?;
    let top = table(&source, &["format", "entries"])?;
    if top["format"].as_integer() != Some(1) {
        return Err(invalid());
    }
    let entries = top["entries"].as_array().ok_or_else(invalid)?;
    if entries.is_empty() || entries.len() > 512 {
        return Err(invalid());
    }
    let components = report["components"].as_array().ok_or_else(invalid)?;
    let mut seen = BTreeSet::new();
    let mut planned = Vec::new();
    let mut text_bytes = 0usize;
    let mut notice_count = 0;
    let mut expanded_bytes = 0usize;
    for entry in entries {
        let t = table(
            entry,
            &[
                "ecosystem",
                "name",
                "version",
                "lock_digest",
                "repository",
                "commit",
                "package_path",
                "provenance",
                "notices",
            ],
        )?;
        let e = string(t, "ecosystem", 16)?;
        let name = string(t, "name", 214)?;
        let version = string(t, "version", 200)?;
        let lock = string(t, "lock_digest", 200)?;
        if !matches!(e, "cargo" | "npm") || e != ecosystem || !seen.insert((e, name, version, lock))
        {
            return Err(invalid());
        }
        let repo = string(t, "repository", 2048)?;
        let commit = string(t, "commit", 40)?;
        let package = string(t, "package_path", 1024)?;
        let provenance = string(t, "provenance", 64)?;
        if !repository(repo)
            || !hex(commit, 40)
            || (!package.is_empty() && !relative(package))
            || !matches!(
                provenance,
                "registry_vcs_view_unverified"
                    | "registry_git_head_unverified"
                    | "attestation_payload_unverified"
                    | "reviewed_commit_only"
            )
        {
            return Err(invalid());
        }
        let mut matches = Vec::new();
        let digest_key = if e == "cargo" {
            "registry_package_sha256"
        } else {
            "lock_integrity"
        };
        for (i, c) in components.iter().enumerate() {
            if c["name"].as_str() == Some(name)
                && c["version"].as_str() == Some(version)
                && c[digest_key].as_str() == Some(lock)
            {
                matches.push(i);
            }
        }
        if matches.is_empty() {
            return Err(invalid());
        }
        let values = t["notices"].as_array().ok_or_else(invalid)?;
        if values.is_empty() || values.len() > 512 {
            return Err(invalid());
        }
        let mut paths = BTreeSet::new();
        let mut notices = Vec::new();
        for n in values {
            let n = n.as_table().ok_or_else(invalid)?;
            if !(n.len() == 3 || (n.len() == 4 && n.contains_key("lines")))
                || !["path", "sha256", "text"]
                    .iter()
                    .all(|k| n.contains_key(*k))
            {
                return Err(invalid());
            }
            let lines = if let Some(v) = n.get("lines") {
                let a = v.as_array().ok_or_else(invalid)?;
                if a.len() != 2 {
                    return Err(invalid());
                }
                let start = a[0].as_integer().ok_or_else(invalid)?;
                let end = a[1].as_integer().ok_or_else(invalid)?;
                if start < 1 || end < start || end > 1_000_000 {
                    return Err(invalid());
                }
                Some([start, end])
            } else {
                None
            };
            let path = string(n, "path", 1024)?;
            let digest = string(n, "sha256", 64)?;
            let text = string(n, "text", 2 * 1024 * 1024)?;
            text_bytes = text_bytes.checked_add(text.len()).ok_or_else(invalid)?;
            notice_count += 1;
            if !relative(path)
                || !paths.insert(path)
                || !hex(digest, 64)
                || text.is_empty()
                || hash(text.as_bytes()) != digest
                || text_bytes > CAP
                || notice_count > 4096
            {
                return Err(invalid());
            }
            let mut notice = json!({"path":path,"sha256":digest,"text":text});
            if let Some(lines) = lines {
                notice["lines"] = json!(lines);
            }
            notices.push(notice);
        }
        let entry_bytes = notices
            .iter()
            .map(|n| n["text"].as_str().unwrap().len())
            .sum::<usize>();
        expanded_bytes = expanded_bytes
            .checked_add(entry_bytes.checked_mul(matches.len()).ok_or_else(invalid)?)
            .ok_or_else(invalid)?;
        if expanded_bytes > 64 * 1024 * 1024 {
            return Err(invalid());
        }
        notices.sort_by(|a, b| a["path"].as_str().cmp(&b["path"].as_str()));
        planned.push((matches,json!({"repository":repo,"commit":commit,"package_path":package,"provenance":provenance,"authenticity_verified":false,"notices":notices})));
    }
    let mut covered = 0;
    let mut missing_covered = 0;
    let components = report["components"].as_array_mut().ok_or_else(invalid)?;
    for (positions, evidence) in planned {
        for i in positions {
            let c = &mut components[i];
            covered += 1;
            missing_covered += usize::from(if ecosystem == "cargo" {
                c["declaration_missing"] == true || c["notice_missing"] == true
            } else {
                c["evidence_complete"] == false
            });
            c["supplemental_evidence"] = evidence.clone();
        }
    }
    report["supplement"] = json!({"format":1,"manifest_sha256":hash(&bytes),"entries":entries.len(),"notice_files":notice_count,"notice_bytes":text_bytes,"component_notice_bytes":expanded_bytes,"components_with_supplement":covered,"incomplete_components_with_supplement":missing_covered,"affects_strict_completeness":false,"claim":"Operator-supplied external notice text matched to exact lock identity and content SHA256 only. Repository, commit and provenance are assertions, not authenticated source/archive/attestation evidence. Local missing/conflict flags and strict completeness are unchanged; no legal clearance or source obligations are established."});
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "moosuri-notice-supplements-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            Self(p)
        }
        fn manifest(&self, ecosystem: &str, lock: &str) -> PathBuf {
            let text = "Synthetic attribution © Example\nExact reviewed terms.\n";
            let body = format!(
                "format=1\n[[entries]]\necosystem={ecosystem:?}\nname=\"demo\"\nversion=\"1.0.0\"\nlock_digest={lock:?}\nrepository=\"https://example.test/reviewed/demo\"\ncommit=\"{}\"\npackage_path=\"crate\"\nprovenance=\"reviewed_commit_only\"\n[[entries.notices]]\npath=\"NOTICE\"\nsha256={:?}\ntext={}\n[[entries.notices]]\npath=\"LICENSE\"\nsha256={:?}\ntext={}\n",
                "a".repeat(40),
                hash(text.as_bytes()),
                serde_json::to_string(text).unwrap(),
                hash(text.as_bytes()),
                serde_json::to_string(text).unwrap()
            );
            let p = self.0.join("supplement.toml");
            fs::write(&p, body).unwrap();
            p
        }
        fn reports(&self) -> (Value, Value) {
            fs::create_dir_all(self.0.join("vendor/demo-1.0.0")).unwrap();
            let cargo = "[package]\nname='demo'\nversion='1.0.0'\nlicense='MIT'\n";
            fs::write(self.0.join("Cargo.lock"),format!("version=4\n[[package]]\nname='demo'\nversion='1.0.0'\nsource='registry+https://github.com/rust-lang/crates.io-index'\nchecksum='{}'\n","ab".repeat(32))).unwrap();
            fs::write(self.0.join("vendor/demo-1.0.0/Cargo.toml"), cargo).unwrap();
            fs::write(self.0.join("vendor/demo-1.0.0/.cargo-checksum.json"),serde_json::to_vec(&json!({"package":"ab".repeat(32),"files":{"Cargo.toml":hash(cargo.as_bytes())}})).unwrap()).unwrap();
            for (app, name) in [("web", "moosuri-web"), ("vault-web", "moosuri-vault-web")] {
                fs::create_dir(self.0.join(app)).unwrap();
                fs::create_dir(self.0.join(format!("{app}_modules"))).unwrap();
                fs::write(self.0.join(format!("{app}/package-lock.json")),serde_json::to_vec(&json!({"name":name,"version":"1.0.0","lockfileVersion":3,"packages":{"":{"name":name,"version":"1.0.0"},"node_modules/demo":{"version":"1.0.0","resolved":"https://registry.npmjs.org/demo/-/demo-1.0.0.tgz","integrity":"sha512-YWJj","license":"MIT"}}})).unwrap()).unwrap();
            }
            fs::create_dir(self.0.join("web_modules/demo")).unwrap();
            fs::write(
                self.0.join("web_modules/demo/package.json"),
                r#"{"name":"demo","version":"1.0.0","license":"Apache-2.0"}"#,
            )
            .unwrap();
            (
                super::super::notices::generate(&self.0, &self.0.join("vendor")).unwrap(),
                super::super::web_notices::generate(
                    &self.0,
                    &self.0.join("web_modules"),
                    &self.0.join("vault-web_modules"),
                )
                .unwrap(),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn supplementary_attribution_is_deterministic_and_never_repairs_original_missing_or_conflicting_evidence()
     {
        let f = Fixture::new();
        let (rust, web) = f.reports();
        assert_eq!(rust["components_missing_evidence"], 1);
        assert_eq!(web["components_missing_evidence"], 2);
        assert!(super::super::web_notices::complete(&web).is_err());
        for (baseline, ecosystem, lock, coverage) in [
            (rust, "cargo", "ab".repeat(32), 1),
            (web, "npm", "sha512-YWJj".into(), 2),
        ] {
            let manifest = f.manifest(ecosystem, &lock);
            let mut report = baseline.clone();
            attach(&mut report, &manifest, ecosystem).unwrap();
            let mut again = baseline.clone();
            attach(&mut again, &manifest, ecosystem).unwrap();
            assert_eq!(report, again);
            assert_eq!(report["supplement"]["components_with_supplement"], coverage);
            assert_eq!(report["supplement"]["affects_strict_completeness"], false);
            assert_eq!(
                report["components"][0]["supplemental_evidence"]["notices"][0]["path"],
                "LICENSE"
            );
            assert_eq!(
                report["components"][0]["supplemental_evidence"]["notices"][0]["text"],
                "Synthetic attribution © Example\nExact reviewed terms.\n"
            );
            if ecosystem == "npm" {
                assert!(super::super::web_notices::complete(&report).is_err());
            }
            report.as_object_mut().unwrap().remove("supplement");
            for c in report["components"].as_array_mut().unwrap() {
                c.as_object_mut().unwrap().remove("supplemental_evidence");
            }
            assert_eq!(report, baseline);
        }
    }
    #[test]
    fn supplement_rejects_unknown_identity_lock_hash_duplicate_or_unsupported_provenance_without_partial_output()
     {
        let f = Fixture::new();
        let (baseline, _) = f.reports();
        let path = f.manifest("cargo", &"ab".repeat(32));
        let original: toml::Value = toml::from_str(&fs::read_to_string(&path).unwrap()).unwrap();
        for (field, value) in [
            ("name", "other"),
            ("version", "2.0.0"),
            ("ecosystem", "npm"),
            ("lock_digest", "cd"),
            ("commit", "main"),
            ("package_path", "../secret"),
            ("repository", "https://token@example.test/repo"),
            ("provenance", "authenticated"),
            ("unknown", "ignored"),
        ] {
            let mut v = original.clone();
            v["entries"][0]
                .as_table_mut()
                .unwrap()
                .insert(field.into(), toml::Value::String(value.into()));
            fs::write(&path, toml::to_string(&v).unwrap()).unwrap();
            let mut report = baseline.clone();
            assert!(attach(&mut report, &path, "cargo").is_err(), "{field}");
            assert_eq!(report, baseline);
        }
        for kind in 0..6 {
            let mut v = original.clone();
            match kind {
                0 => {
                    v["entries"][0]["notices"][0]["sha256"] = toml::Value::String("0".repeat(64));
                }
                1 => {
                    v["entries"][0]["notices"][0]["path"] =
                        toml::Value::String("../LICENSE".into());
                }
                2 => {
                    let entry = v["entries"][0].clone();
                    v["entries"].as_array_mut().unwrap().push(entry);
                }
                3 => {
                    let notice = v["entries"][0]["notices"][0].clone();
                    v["entries"][0]["notices"]
                        .as_array_mut()
                        .unwrap()
                        .push(notice);
                }
                4 => {
                    v["entries"][0]["notices"][0]
                        .as_table_mut()
                        .unwrap()
                        .insert(
                            "lines".into(),
                            toml::Value::Array(vec![
                                toml::Value::Integer(0),
                                toml::Value::Integer(3),
                            ]),
                        );
                }
                _ => {
                    v["entries"][0]["notices"][0]["text"] = toml::Value::String("nul\0text".into());
                }
            }
            fs::write(&path, toml::to_string(&v).unwrap()).unwrap();
            assert!(attach(&mut baseline.clone(), &path, "cargo").is_err());
        }
        fs::write(&path, "format=1\nformat=1\nentries=[]").unwrap();
        assert!(attach(&mut baseline.clone(), &path, "cargo").is_err());
    }
    #[cfg(unix)]
    #[test]
    fn supplement_manifest_paths_are_bounded_explicit_and_never_follow_observed_symlinks() {
        let f = Fixture::new();
        let (baseline, _) = f.reports();
        let path = f.manifest("cargo", &"ab".repeat(32));
        let alias = f.0.join("alias");
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        assert!(attach(&mut baseline.clone(), &alias, "cargo").is_err());
        let parent = f.0.join("linked");
        std::os::unix::fs::symlink(&f.0, &parent).unwrap();
        assert!(
            attach(
                &mut baseline.clone(),
                &parent.join("supplement.toml"),
                "cargo"
            )
            .is_err()
        );
        assert!(
            attach(
                &mut baseline.clone(),
                &f.0.join("../supplement.toml"),
                "cargo"
            )
            .is_err()
        );
        assert!(attach(&mut baseline.clone(), Path::new("supplement.toml"), "cargo").is_err());
        fs::write(&path, vec![b' '; CAP + 1]).unwrap();
        assert!(attach(&mut baseline.clone(), &path, "cargo").is_err());
        assert!(options(&["--supplement".into()]).is_err());
        assert!(options(&["--require-complete".into(), "--require-complete".into()]).is_err());
    }
}
