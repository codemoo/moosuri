//! Offline, checksum-bound Rust dependency notice evidence, not legal clearance.
use super::Check;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs, io::Read, io::Write, path::Path};

const FILE_CAP: usize = 2 * 1024 * 1024;
const MANIFEST_CAP: usize = 8 * 1024 * 1024;
const TOTAL_CAP: usize = 64 * 1024 * 1024;
const CLAIM: &str = "Source-lock Rust dependency evidence from explicitly supplied vendored sources. Cargo checksum metadata and notice bytes are compared; the local checksum manifest is not independent registry authentication. Includes all target/dev dependencies, not a minimal binary graph. Declared license expressions and candidate notice files are preserved without choosing a license or claiming completeness, legal clearance, npm coverage, or build authenticity.";

fn invalid() -> String {
    "invalid, missing, changed or unsupported Rust notice evidence".into()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn digest(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn package_label(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._+".contains(&b))
}
fn relative(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 1024
        && !value.contains('\\')
        && !value.contains(':')
        && !value.chars().any(char::is_control)
        && value.split('/').count() <= 16
        && value
            .split('/')
            .all(|p| !p.is_empty() && p != "." && p != "..")
}
fn directory(path: &Path) -> Check<()> {
    let meta = fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !meta.is_dir() || meta.file_type().is_symlink() {
        return Err(invalid());
    }
    Ok(())
}
fn read(root: &Path, name: &str, cap: usize) -> Check<Vec<u8>> {
    if !relative(name) {
        return Err(invalid());
    }
    let mut path = root.to_path_buf();
    let parts: Vec<_> = name.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        path.push(part);
        directory(&path)?;
    }
    path.push(parts[parts.len() - 1]);
    let meta = fs::symlink_metadata(&path).map_err(|_| invalid())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > cap as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    fs::File::open(&path)
        .map_err(|_| invalid())?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > cap {
        return Err(invalid());
    }
    Ok(bytes)
}
fn checked(
    root: &Path,
    name: &str,
    files: &serde_json::Map<String, Value>,
    cap: usize,
) -> Check<Vec<u8>> {
    let expected = files
        .get(name)
        .and_then(Value::as_str)
        .filter(|v| digest(v))
        .ok_or_else(invalid)?;
    let bytes = read(root, name, cap)?;
    if hash(&bytes) != expected.to_ascii_lowercase() {
        return Err(invalid());
    }
    Ok(bytes)
}
fn notice_name(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let parts: Vec<_> = lower.split('/').collect();
    if parts[..parts.len() - 1]
        .iter()
        .any(|part| matches!(*part, "licenses" | "licences" | "legal"))
    {
        return true;
    }
    let basename = parts[parts.len() - 1];
    ["license", "licence", "copying", "copyright", "notice"]
        .iter()
        .any(|prefix| {
            basename == *prefix
                || basename
                    .strip_prefix(prefix)
                    .is_some_and(|s| s.starts_with(['.', '-', '_']))
        })
}
fn text(bytes: &[u8]) -> Check<&str> {
    let value = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if value.is_empty() || value.contains('\0') {
        return Err(invalid());
    }
    Ok(value)
}
fn declaration(value: Option<&toml::Value>) -> Check<Option<String>> {
    match value {
        None => Ok(None),
        Some(value) => {
            let value = value.as_str().ok_or_else(invalid)?;
            if value.is_empty() || value.len() > 1024 || value.chars().any(char::is_control) {
                return Err(invalid());
            }
            Ok(Some(value.to_owned()))
        }
    }
}
fn generate(root: &Path, vendor: &Path) -> Check<Value> {
    directory(vendor)?;
    let lock_bytes = read(root, "Cargo.lock", MANIFEST_CAP)?;
    let lock: toml::Value = toml::from_str(text(&lock_bytes)?).map_err(|_| invalid())?;
    if lock.get("version").and_then(toml::Value::as_integer) != Some(4) {
        return Err(invalid());
    }
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(invalid)?;
    if packages.len() > 10000 {
        return Err(invalid());
    }
    let mut seen = BTreeSet::new();
    let mut components = Vec::new();
    let mut local = Vec::new();
    let mut total = 0usize;
    let mut incomplete = 0usize;
    for package in packages {
        let name = package
            .get("name")
            .and_then(toml::Value::as_str)
            .filter(|v| package_label(v))
            .ok_or_else(invalid)?;
        let version = package
            .get("version")
            .and_then(toml::Value::as_str)
            .filter(|v| package_label(v))
            .ok_or_else(invalid)?;
        if !seen.insert((name, version)) {
            return Err(invalid());
        }
        let Some(source) = package.get("source") else {
            local.push(
                json!({"name":name,"version":version,"status":"workspace_source_not_collected"}),
            );
            continue;
        };
        if source.as_str() != Some("registry+https://github.com/rust-lang/crates.io-index") {
            return Err("Rust notices require reviewed handling for non-crates.io sources".into());
        }
        let checksum = package
            .get("checksum")
            .and_then(toml::Value::as_str)
            .filter(|v| digest(v))
            .ok_or_else(invalid)?
            .to_ascii_lowercase();
        let dir = vendor.join(format!("{name}-{version}"));
        directory(&dir)?;
        let manifest = read(&dir, ".cargo-checksum.json", MANIFEST_CAP)?;
        let evidence: Value = serde_json::from_slice(&manifest).map_err(|_| invalid())?;
        if evidence.get("package").and_then(Value::as_str) != Some(checksum.as_str()) {
            return Err(invalid());
        }
        let files = evidence
            .get("files")
            .and_then(Value::as_object)
            .ok_or_else(invalid)?;
        if files.len() > 50000 || !files.keys().all(|p| relative(p)) {
            return Err(invalid());
        }
        let metadata = checked(&dir, "Cargo.toml", files, FILE_CAP)?;
        let cargo: toml::Value = toml::from_str(text(&metadata)?).map_err(|_| invalid())?;
        let p = cargo.get("package").ok_or_else(invalid)?;
        if p.get("name").and_then(toml::Value::as_str) != Some(name)
            || p.get("version").and_then(toml::Value::as_str) != Some(version)
        {
            return Err(invalid());
        }
        let license = declaration(p.get("license"))?;
        let explicit = declaration(p.get("license-file"))?;
        if explicit.as_ref().is_some_and(|p| !relative(p)) {
            return Err(invalid());
        }
        let mut selected: BTreeSet<String> =
            files.keys().filter(|p| notice_name(p)).cloned().collect();
        if let Some(path) = &explicit {
            selected.insert(path.clone());
        }
        if selected.len() > 512 {
            return Err(invalid());
        }
        let mut notices = Vec::new();
        for path in selected {
            let bytes = checked(&dir, &path, files, FILE_CAP)?;
            total = total.checked_add(bytes.len()).ok_or_else(invalid)?;
            if total > TOTAL_CAP {
                return Err(invalid());
            }
            notices.push(json!({"path":path,"sha256":hash(&bytes),"text":text(&bytes)?}));
        }
        let declaration_missing = license.is_none() && explicit.is_none();
        let notice_missing = notices.is_empty();
        if declaration_missing || notice_missing {
            incomplete += 1;
        }
        components.push(json!({"name":name,"version":version,"registry_package_sha256":checksum,
            "local_checksum_manifest_sha256":hash(&manifest),"cargo_toml_sha256":hash(&metadata),
            "declared_license":license,"declared_license_file":explicit,
            "declaration_missing":declaration_missing,"notice_missing":notice_missing,"notices":notices}));
    }
    components.sort_by(|a, b| {
        (a["name"].as_str(), a["version"].as_str())
            .cmp(&(b["name"].as_str(), b["version"].as_str()))
    });
    local.sort_by(|a, b| {
        (a["name"].as_str(), a["version"].as_str())
            .cmp(&(b["name"].as_str(), b["version"].as_str()))
    });
    Ok(
        json!({"format":1,"purpose":"moosuri-rust-notice-evidence","claim":CLAIM,
        "cargo_lock_sha256":hash(&lock_bytes),"registry_components":components.len(),
        "components_missing_evidence":incomplete,"collected_notice_bytes":total,"workspace_entries":local,"components":components}),
    )
}
pub(super) fn run(root: &Path, args: &[String]) -> Check<()> {
    if !matches!(args.len(), 3 | 4)
        || args[1] != "--vendor"
        || (args.len() == 4 && args[3] != "--require-complete")
    {
        return Err("Usage: rust-notices --vendor DIRECTORY [--require-complete]".into());
    }
    let result = generate(root, Path::new(&args[2]))?;
    let mut bytes = serde_json::to_vec_pretty(&result).map_err(|_| invalid())?;
    bytes.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|_| "cannot write Rust notice evidence".to_string())?;
    if args.len() == 4 && result["components_missing_evidence"] != 0 {
        return Err(
            "Rust dependency declaration or notice evidence is missing; see the emitted report"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, Ordering};
    static ID: AtomicU64 = AtomicU64::new(0);
    struct Fixture(std::path::PathBuf);
    impl Fixture {
        fn new() -> Self {
            let p = std::env::temp_dir().join(format!(
                "moosuri-notices-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&p).unwrap();
            fs::create_dir(p.join("vendor")).unwrap();
            Self(p)
        }
        fn add(&self, name: &str, version: &str, license: &str, notices: &[(&str, &str)]) {
            let p = self.0.join("vendor").join(format!("{name}-{version}"));
            fs::create_dir(&p).unwrap();
            let cargo =
                format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\n{license}\n");
            fs::write(p.join("Cargo.toml"), &cargo).unwrap();
            let mut files = serde_json::Map::new();
            files.insert("Cargo.toml".into(), json!(hash(cargo.as_bytes())));
            for (path, contents) in notices {
                let target = p.join(path);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(target, contents).unwrap();
                files.insert((*path).into(), json!(hash(contents.as_bytes())));
            }
            fs::write(
                p.join(".cargo-checksum.json"),
                serde_json::to_vec(&json!({"package":"ab".repeat(32),"files":files})).unwrap(),
            )
            .unwrap();
        }
        fn lock(&self, packages: &[(&str, &str)]) {
            let mut body = String::from("version = 4\n");
            for (name, version) in packages {
                body.push_str(&format!("\n[[package]]\nname = \"{name}\"\nversion = \"{version}\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n", "ab".repeat(32)));
            }
            fs::write(self.0.join("Cargo.lock"), body).unwrap();
        }
        fn report(&self) -> Check<Value> {
            generate(&self.0, &self.0.join("vendor"))
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn rust_notices_preserve_distinct_versions_and_exact_attribution_without_host_paths() {
        let f = Fixture::new();
        f.lock(&[("example", "2.0.0"), ("example", "1.0.0")]);
        f.add(
            "example",
            "1.0.0",
            "license = \"MIT OR Apache-2.0\"",
            &[
                (
                    "LICENSE-MIT",
                    "Synthetic copyright © 2027\nPermission example\n",
                ),
                ("vendor/NOTICE", "Synthetic nested attribution\n"),
            ],
        );
        f.add(
            "example",
            "2.0.0",
            "license-file = \"terms/custom.txt\"",
            &[("terms/custom.txt", "Synthetic explicit terms\n")],
        );
        let result = f.report().unwrap();
        assert_eq!(result["registry_components"], 2);
        assert_eq!(result["components_missing_evidence"], 0);
        assert_eq!(result["components"][0]["version"], "1.0.0");
        assert_eq!(
            result["components"][0]["notices"][0]["text"],
            "Synthetic copyright © 2027\nPermission example\n"
        );
        let bytes = serde_json::to_string(&result).unwrap();
        assert!(!bytes.contains(f.0.to_str().unwrap()));
        assert_eq!(result, f.report().unwrap());
        fs::write(
            f.0.join("vendor/example-1.0.0/LICENSE-MIT"),
            "replaced attribution",
        )
        .unwrap();
        assert!(f.report().is_err());
    }
    #[test]
    fn rust_notices_report_missing_evidence_and_reject_unsafe_or_forged_packages() {
        let f = Fixture::new();
        f.lock(&[("example", "1.0.0")]);
        f.add("example", "1.0.0", "", &[]);
        let result = f.report().unwrap();
        assert_eq!(result["components_missing_evidence"], 1);
        assert_eq!(result["components"][0]["declared_license"], Value::Null);
        let path = f.0.join("vendor/example-1.0.0/.cargo-checksum.json");
        let mut manifest: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        manifest["files"]["../LICENSE"] = json!("ab".repeat(32));
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(f.report().is_err());
        manifest["files"]
            .as_object_mut()
            .unwrap()
            .remove("../LICENSE");
        manifest["package"] = json!("cd".repeat(32));
        fs::write(&path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert!(f.report().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn rust_notices_never_follow_notice_or_package_symlinks() {
        let f = Fixture::new();
        f.lock(&[("example", "1.0.0")]);
        f.add(
            "example",
            "1.0.0",
            "license = \"MIT\"",
            &[("LICENSE", "Synthetic terms\n")],
        );
        let package = f.0.join("vendor/example-1.0.0");
        let original = package.join("LICENSE");
        fs::rename(&original, f.0.join("outside")).unwrap();
        std::os::unix::fs::symlink(f.0.join("outside"), &original).unwrap();
        assert!(f.report().is_err());
        fs::remove_file(&original).unwrap();
        fs::rename(f.0.join("outside"), &original).unwrap();
        assert!(f.report().is_ok());
        fs::rename(&package, f.0.join("external-package")).unwrap();
        std::os::unix::fs::symlink(f.0.join("external-package"), &package).unwrap();
        assert!(f.report().is_err());
    }
}
