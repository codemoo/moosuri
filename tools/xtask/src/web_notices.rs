//! Offline npm lock/installed-tree notice evidence, never license clearance.
use super::Check;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    fs,
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};
const LOCK_CAP: usize = 8 * 1024 * 1024;
const FILE_CAP: usize = 2 * 1024 * 1024;
const TOTAL_CAP: usize = 64 * 1024 * 1024;
const ENTRY_CAP: usize = 200_000;
const CLAIM: &str = "Full web and vault-web npm source-lock inventory, including development and other-platform optional entries. Exact installed name/version and selected package.json/license/notice bytes are local evidence only. npm tarball integrity is recorded from the lock, not verified against unpacked files; neither package authenticity nor a minimal embedded-bundle graph is established. Missing evidence is explicit. Declared labels and candidate texts do not establish legal clearance, complete attribution, license choices, source obligations or asset coverage.";
fn invalid() -> String {
    "invalid, changed or unsupported web notice evidence".into()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn segment(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 214
        && !s.starts_with(['.', '_'])
        && !matches!(s, "." | "..")
        && s.bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"-._".contains(&b))
}
fn name(s: &str) -> bool {
    if let Some(s) = s.strip_prefix('@') {
        let Some((scope, package)) = s.split_once('/') else {
            return false;
        };
        segment(scope) && segment(package)
    } else {
        segment(s)
    }
}
fn version(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
}
fn relative(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 1024
        && !s.contains(['\\', ':'])
        && !s.chars().any(char::is_control)
        && s.split('/').count() <= 32
        && s.split('/')
            .all(|s| !s.is_empty() && !matches!(s, "." | ".."))
}
fn directory(path: &Path) -> Check<bool> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() && !m.file_type().is_symlink() => Ok(true),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(false),
        _ => Err(invalid()),
    }
}
/// Explicit roots must already be absolute real paths, without symlink ancestors.
/// This checks observed entries in a stable trusted tree, not concurrent replacement.
fn root(path: &Path) -> Check<()> {
    let value = path.to_str().ok_or_else(invalid)?;
    if !path.is_absolute()
        || value.len() > 4096
        || value.contains('\\')
        || value.chars().any(char::is_control)
        || value
            .split('/')
            .skip(1)
            .any(|s| s.is_empty() || matches!(s, "." | ".."))
    {
        return Err(invalid());
    }
    let mut current = PathBuf::new();
    for part in path.components() {
        if !matches!(part, Component::RootDir | Component::Normal(_)) {
            return Err(invalid());
        }
        current.push(part);
        if !directory(&current)? {
            return Err(invalid());
        }
    }
    Ok(())
}
fn package_dir(base: &Path, relative_path: &str) -> Check<Option<PathBuf>> {
    if !relative(relative_path) {
        return Err(invalid());
    }
    let mut path = base.to_path_buf();
    for part in relative_path.split('/') {
        path.push(part);
        if !directory(&path)? {
            return Ok(None);
        }
    }
    Ok(Some(path))
}
fn read(base: &Path, path: &str, cap: usize) -> Check<Vec<u8>> {
    if !relative(path) {
        return Err(invalid());
    }
    let mut target = base.to_path_buf();
    let parts: Vec<_> = path.split('/').collect();
    for part in &parts[..parts.len() - 1] {
        target.push(part);
        if !directory(&target)? {
            return Err(invalid());
        }
    }
    target.push(parts[parts.len() - 1]);
    let meta = fs::symlink_metadata(&target).map_err(|_| invalid())?;
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > cap as u64 {
        return Err(invalid());
    }
    let mut bytes = Vec::new();
    fs::File::open(target)
        .map_err(|_| invalid())?
        .take(cap as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| invalid())?;
    if bytes.len() > cap {
        return Err(invalid());
    }
    Ok(bytes)
}
fn text(bytes: &[u8]) -> Check<&str> {
    let s = std::str::from_utf8(bytes).map_err(|_| invalid())?;
    if s.is_empty() || s.contains('\0') {
        return Err(invalid());
    }
    Ok(s)
}
fn declaration(value: Option<&Value>) -> Check<Option<String>> {
    match value {
        None => Ok(None),
        Some(value) => {
            let s = value.as_str().ok_or_else(invalid)?;
            if s.is_empty() || s.len() > 1024 || s.chars().any(char::is_control) {
                return Err(invalid());
            }
            Ok(Some(s.into()))
        }
    }
}
fn boolean(value: &Value, key: &str) -> Check<bool> {
    match value.get(key) {
        None => Ok(false),
        Some(v) => v.as_bool().ok_or_else(invalid),
    }
}
/// Parse npm installation positions, preserving nested/scoped packages and aliases.
fn position(path: &str) -> Check<(&str, &str)> {
    if !relative(path) {
        return Err(invalid());
    }
    let parts: Vec<_> = path.split('/').collect();
    let mut at = 0;
    let mut start = 0;
    while at < parts.len() {
        if parts[at] != "node_modules" {
            return Err(invalid());
        }
        at += 1;
        start = at;
        let part = *parts.get(at).ok_or_else(invalid)?;
        if let Some(scope) = part.strip_prefix('@') {
            if !segment(scope) || !parts.get(at + 1).is_some_and(|p| segment(p)) {
                return Err(invalid());
            }
            at += 2;
        } else {
            if !segment(part) {
                return Err(invalid());
            }
            at += 1;
        }
    }
    let prefix = parts[..start].join("/").len() + 1;
    Ok((&path["node_modules/".len()..], &path[prefix..]))
}
fn integrity(value: &Value) -> Check<String> {
    let value = value.as_str().ok_or_else(invalid)?;
    let Some((algorithm, data)) = value.split_once('-') else {
        return Err(invalid());
    };
    if !matches!(algorithm, "sha1" | "sha256" | "sha512")
        || data.is_empty()
        || data.len() > 128
        || !data
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"+/=".contains(&b))
    {
        return Err(invalid());
    }
    Ok(value.into())
}
fn notice_name(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let parts: Vec<_> = lower.split('/').collect();
    parts[..parts.len() - 1]
        .iter()
        .any(|p| matches!(*p, "licenses" | "licences" | "legal"))
        || [
            "license",
            "licence",
            "copying",
            "copyright",
            "notice",
            "thirdpartynoticetext",
            "thirdpartynotices",
            "third-party-notices",
            "third_party_notices",
        ]
        .iter()
        .any(|prefix| {
            parts[parts.len() - 1] == *prefix
                || parts[parts.len() - 1]
                    .strip_prefix(prefix)
                    .is_some_and(|s| s.starts_with(['.', '-', '_']))
        })
}
fn selected_files(
    base: &Path,
    path: &str,
    selected: &mut BTreeSet<String>,
    entries: &mut usize,
) -> Check<()> {
    if path.split('/').count() > 16 {
        return Err(invalid());
    }
    for entry in fs::read_dir(base.join(path)).map_err(|_| invalid())? {
        *entries = entries.checked_add(1).ok_or_else(invalid)?;
        if *entries > ENTRY_CAP {
            return Err(invalid());
        }
        let entry = entry.map_err(|_| invalid())?;
        let file = entry.file_name().into_string().map_err(|_| invalid())?;
        // Nested dependency trees are inventoried by their own lock positions.
        if file == "node_modules" {
            continue;
        }
        let relative_path = if path.is_empty() {
            file
        } else {
            format!("{path}/{file}")
        };
        if !relative(&relative_path) {
            return Err(invalid());
        }
        let meta = fs::symlink_metadata(entry.path()).map_err(|_| invalid())?;
        if meta.file_type().is_symlink() {
            return Err(invalid());
        }
        if meta.is_dir() {
            selected_files(base, &relative_path, selected, entries)?;
        } else if notice_name(&relative_path) {
            if !meta.is_file() {
                return Err(invalid());
            }
            selected.insert(relative_path);
            if selected.len() > 512 {
                return Err(invalid());
            }
        }
    }
    Ok(())
}
fn add_bytes(total: &mut usize, size: usize) -> Check<()> {
    *total = total.checked_add(size).ok_or_else(invalid)?;
    if *total > TOTAL_CAP {
        return Err(invalid());
    }
    Ok(())
}
fn complete(report: &Value) -> Check<()> {
    if report["components_missing_evidence"] != 0 {
        return Err("web dependency package metadata, declaration or notice evidence is missing; see the emitted report".into());
    }
    Ok(())
}
fn generate(project: &Path, web: &Path, vault: &Path) -> Check<Value> {
    root(project)?;
    root(web)?;
    root(vault)?;
    let mut components = Vec::new();
    let mut apps = Vec::new();
    let mut total = 0;
    let mut notice_bytes = 0;
    let mut entries = 0;
    let mut incomplete = 0;
    let mut missing_packages = 0;
    for (app, expected_app, installed) in [
        ("web", "moosuri-web", web),
        ("vault-web", "moosuri-vault-web", vault),
    ] {
        let lock_path = format!("{app}/package-lock.json");
        let bytes = read(project, &lock_path, LOCK_CAP)?;
        let lock: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if lock["lockfileVersion"].as_u64() != Some(3)
            || lock["name"].as_str() != Some(expected_app)
        {
            return Err(invalid());
        }
        let packages = lock["packages"].as_object().ok_or_else(invalid)?;
        if packages.len() > 10000
            || packages.get("").and_then(|v| v["name"].as_str()) != Some(expected_app)
        {
            return Err(invalid());
        }
        let app_version = lock["version"]
            .as_str()
            .filter(|s| version(s))
            .ok_or_else(invalid)?;
        if packages[""]["version"].as_str() != Some(app_version) {
            return Err(invalid());
        }
        apps.push(json!({"app":app,"name":expected_app,"version":app_version,"lockfile":lock_path,"lock_sha256":hash(&bytes),"lock_components":packages.len()-1}));
        for (path, package) in packages {
            if path.is_empty() {
                continue;
            }
            let (relative_path, path_name) = position(path)?;
            if !package.is_object() || boolean(package, "link")? {
                return Err(invalid());
            }
            let expected_name = package
                .get("name")
                .map(|v| v.as_str().ok_or_else(invalid))
                .transpose()?
                .unwrap_or(path_name);
            if !name(expected_name) {
                return Err(invalid());
            }
            let expected_version = package["version"]
                .as_str()
                .filter(|s| version(s))
                .ok_or_else(invalid)?;
            let source = package["resolved"].as_str().ok_or_else(invalid)?;
            if !source.starts_with("https://registry.npmjs.org/")
                || source.len() > 2048
                || !source.bytes().all(|b| b.is_ascii_graphic())
                || source.contains(['?', '#', '\\'])
            {
                return Err(
                    "web notices require reviewed handling for unsupported npm source metadata"
                        .into(),
                );
            }
            let sri = integrity(&package["integrity"])?;
            let locked_license = declaration(package.get("license"))?;
            let development = boolean(package, "dev")?;
            let optional = boolean(package, "optional")?;
            let mut notices = Vec::new();
            let mut metadata_hash = None;
            let mut installed_license = None;
            let mut explicit = BTreeSet::new();
            let mut explicit_missing = false;
            if let Some(path) = locked_license
                .as_ref()
                .and_then(|s| s.strip_prefix("SEE LICENSE IN "))
            {
                if !relative(path) || !notice_name(path) {
                    return Err(invalid());
                }
                explicit.insert(path.to_owned());
            }
            let package_root = package_dir(installed, relative_path)?;
            if let Some(package_root) = &package_root {
                let metadata = read(package_root, "package.json", FILE_CAP)?;
                add_bytes(&mut total, metadata.len())?;
                let local: Value = serde_json::from_slice(&metadata).map_err(|_| invalid())?;
                if local["name"].as_str() != Some(expected_name)
                    || local["version"].as_str() != Some(expected_version)
                {
                    return Err(invalid());
                }
                metadata_hash = Some(hash(&metadata));
                installed_license = declaration(local.get("license"))?;
                let mut selected = BTreeSet::new();
                selected_files(package_root, "", &mut selected, &mut entries)?;
                if let Some(path) = installed_license
                    .as_ref()
                    .and_then(|s| s.strip_prefix("SEE LICENSE IN "))
                {
                    if !relative(path) || !notice_name(path) {
                        return Err(invalid());
                    }
                    explicit.insert(path.to_owned());
                }
                explicit_missing = explicit.iter().any(|path| !selected.contains(path));
                for path in selected {
                    let bytes = read(package_root, &path, FILE_CAP)?;
                    add_bytes(&mut total, bytes.len())?;
                    notice_bytes += bytes.len();
                    notices.push(json!({"path":path,"sha256":hash(&bytes),"text":text(&bytes)?}));
                }
            }
            let metadata_missing = package_root.is_none();
            let declaration_missing = locked_license.is_none() && installed_license.is_none();
            let notice_missing = notices.is_empty();
            let conflict = locked_license
                .as_ref()
                .zip(installed_license.as_ref())
                .is_some_and(|(a, b)| a != b);
            let missing = metadata_missing
                || declaration_missing
                || notice_missing
                || explicit_missing
                || conflict;
            incomplete += usize::from(missing);
            missing_packages += usize::from(metadata_missing);
            components.push(json!({"app":app,"lock_path":path,"name":expected_name,"version":expected_version,"lock_integrity":sri,"development":development,"optional":optional,"package_json_sha256":metadata_hash,"lock_declared_license":locked_license,"installed_declared_license":installed_license,"declared_license_files":explicit,"metadata_missing":metadata_missing,"declaration_missing":declaration_missing,"notice_missing":notice_missing,"declared_notice_missing":explicit_missing,"declaration_conflict":conflict,"evidence_complete":!missing,"notices":notices}));
        }
    }
    components.sort_by(|a, b| {
        (a["app"].as_str(), a["lock_path"].as_str())
            .cmp(&(b["app"].as_str(), b["lock_path"].as_str()))
    });
    Ok(
        json!({"format":1,"purpose":"moosuri-web-notice-evidence","claim":CLAIM,"apps":apps,"lock_components":components.len(),"components_missing_evidence":incomplete,"packages_missing":missing_packages,"collected_notice_bytes":notice_bytes,"collected_selected_bytes":total,"components":components}),
    )
}
pub(super) fn run(project: &Path, args: &[String]) -> Check<()> {
    if !matches!(args.len(), 5 | 6)
        || args[1] != "--web-node-modules"
        || args[3] != "--vault-node-modules"
        || (args.len() == 6 && args[5] != "--require-complete")
    {
        return Err("Usage: web-notices --web-node-modules DIRECTORY --vault-node-modules DIRECTORY [--require-complete]".into());
    }
    let report = generate(project, Path::new(&args[2]), Path::new(&args[4]))?;
    let mut bytes = serde_json::to_vec_pretty(&report).map_err(|_| invalid())?;
    bytes.push(b'\n');
    std::io::stdout()
        .lock()
        .write_all(&bytes)
        .map_err(|_| "cannot write web notice evidence".to_owned())?;
    if args.len() == 6 {
        complete(&report)?
    }
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
            let root = std::env::temp_dir().canonicalize().unwrap().join(format!(
                "moosuri-web-notices-{}-{}",
                std::process::id(),
                ID.fetch_add(1, Ordering::Relaxed)
            ));
            fs::create_dir(&root).unwrap();
            for dir in ["web", "vault-web", "web_modules", "vault_modules"] {
                fs::create_dir(root.join(dir)).unwrap();
            }
            let f = Self(root);
            f.lock("web", json!({}));
            f.lock("vault-web", json!({}));
            f
        }
        fn lock(&self, app: &str, entries: Value) {
            let mut entries = entries.as_object().unwrap().clone();
            let name = if app == "web" {
                "moosuri-web"
            } else {
                "moosuri-vault-web"
            };
            entries.insert("".into(), json!({"name":name,"version":"1.0.0"}));
            fs::write(
                self.0.join(app).join("package-lock.json"),
                serde_json::to_vec(
                    &json!({"name":name,"version":"1.0.0","lockfileVersion":3,"packages":entries}),
                )
                .unwrap(),
            )
            .unwrap();
        }
        fn installed(&self, app: &str) -> PathBuf {
            self.0.join(if app == "web" {
                "web_modules"
            } else {
                "vault_modules"
            })
        }
        fn package(
            &self,
            app: &str,
            path: &str,
            name: &str,
            version: &str,
            license: Option<&str>,
            notices: &[(&str, &str)],
        ) {
            let p = self
                .installed(app)
                .join(path.strip_prefix("node_modules/").unwrap());
            fs::create_dir_all(&p).unwrap();
            let mut metadata = json!({"name":name,"version":version,"private_configuration":"DO_NOT_EXPORT_METADATA","scripts":{"install":"DO_NOT_EXECUTE"}});
            if let Some(license) = license {
                metadata["license"] = json!(license)
            }
            fs::write(
                p.join("package.json"),
                serde_json::to_vec(&metadata).unwrap(),
            )
            .unwrap();
            for (path, text) in notices {
                let target = p.join(path);
                fs::create_dir_all(target.parent().unwrap()).unwrap();
                fs::write(target, text).unwrap();
            }
        }
        fn report(&self) -> Check<Value> {
            generate(
                &self.0,
                &self.installed("web"),
                &self.installed("vault-web"),
            )
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn locked(version: &str, license: Option<&str>) -> Value {
        let mut entry = json!({"version":version,"resolved":"https://registry.npmjs.org/example/-/example-1.0.0.tgz","integrity":format!("sha512-{}==","A".repeat(86)),"dev":true});
        if let Some(s) = license {
            entry["license"] = json!(s)
        }
        entry
    }
    #[test]
    fn web_notices_bind_nested_scoped_alias_versions_and_preserve_exact_notice_text() {
        let f = Fixture::new();
        let mut alias = locked("3.0.0", Some("MIT OR Apache-2.0"));
        alias["name"] = json!("actual-name");
        f.lock("web",json!({"node_modules/@fixture/example":locked("1.0.0",Some("MIT")),"node_modules/outer":locked("1.0.0",Some("MIT")),"node_modules/outer/node_modules/@fixture/example":locked("2.0.0",Some("SEE LICENSE IN legal/custom.txt")),"node_modules/alias":alias}));
        f.lock(
            "vault-web",
            json!({"node_modules/@fixture/example":locked("1.0.0",Some("MIT"))}),
        );
        for (app, path, name, version, license, notice) in [
            (
                "web",
                "node_modules/@fixture/example",
                "@fixture/example",
                "1.0.0",
                "MIT",
                "LICENSE",
            ),
            (
                "web",
                "node_modules/outer",
                "outer",
                "1.0.0",
                "MIT",
                "NOTICE.txt",
            ),
            (
                "web",
                "node_modules/outer/node_modules/@fixture/example",
                "@fixture/example",
                "2.0.0",
                "SEE LICENSE IN legal/custom.txt",
                "legal/custom.txt",
            ),
            (
                "web",
                "node_modules/alias",
                "actual-name",
                "3.0.0",
                "MIT OR Apache-2.0",
                "dist/LICENSE-MIT",
            ),
            (
                "vault-web",
                "node_modules/@fixture/example",
                "@fixture/example",
                "1.0.0",
                "MIT",
                "LICENSE",
            ),
        ] {
            f.package(
                app,
                path,
                name,
                version,
                Some(license),
                &[(
                    notice,
                    "Synthetic copyright © 2027\nExact attributed text.\n",
                )],
            );
        }
        fs::write(
            f.installed("web").join("alias/ThirdPartyNoticeText.txt"),
            "Synthetic bundled third-party attribution\n",
        )
        .unwrap();
        let report = f.report().unwrap();
        assert!(report["components"].as_array().unwrap().iter().any(|p| {
            p["name"] == "actual-name"
                && p["notices"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|n| n["path"] == "ThirdPartyNoticeText.txt")
        }));
        assert_eq!(report["lock_components"], 5);
        assert_eq!(report["components_missing_evidence"], 0);
        complete(&report).unwrap();
        let parts = report["components"].as_array().unwrap();
        let outer = parts.iter().find(|p| p["name"] == "outer").unwrap();
        assert_eq!(outer["notices"].as_array().unwrap().len(), 1);
        assert_eq!(
            parts
                .iter()
                .filter(|p| p["name"] == "@fixture/example")
                .count(),
            3
        );
        assert!(
            parts
                .iter()
                .any(|p| p["name"] == "actual-name" && p["lock_path"] == "node_modules/alias")
        );
        assert_eq!(
            parts[0]["notices"][0]["text"],
            "Synthetic copyright © 2027\nExact attributed text.\n"
        );
        let bytes = serde_json::to_string(&report).unwrap();
        assert!(
            !bytes.contains("DO_NOT_EXPORT")
                && !bytes.contains("DO_NOT_EXECUTE")
                && !bytes.contains("https://registry.npmjs.org/")
                && !bytes.contains(f.0.to_str().unwrap())
        );
        assert_eq!(report, f.report().unwrap());
        let path = f.installed("vault-web").join("@fixture/example/LICENSE");
        fs::write(path, "Updated local terms\n").unwrap();
        let changed = f.report().unwrap();
        assert_ne!(
            changed["components"][0]["notices"][0]["sha256"],
            report["components"][0]["notices"][0]["sha256"]
        );
        // Local notices have content hashes, not a false tarball-integrity check.
        assert_eq!(
            changed["components"][0]["lock_integrity"],
            report["components"][0]["lock_integrity"]
        );
    }
    #[test]
    fn web_notices_report_missing_optional_packages_declarations_notices_and_conflicts() {
        let f = Fixture::new();
        let mut missing = locked("1.0.0", Some("MIT"));
        missing["optional"] = json!(true);
        f.lock("web",json!({"node_modules/platform":missing,"node_modules/empty":locked("1.0.0",None),"node_modules/conflict":locked("1.0.0",Some("MIT")),"node_modules/explicit":locked("1.0.0",Some("SEE LICENSE IN LICENSE.other"))}));
        f.package("web", "node_modules/empty", "empty", "1.0.0", None, &[]);
        f.package(
            "web",
            "node_modules/conflict",
            "conflict",
            "1.0.0",
            Some("Apache-2.0"),
            &[("LICENSE", "Synthetic terms\n")],
        );
        f.package(
            "web",
            "node_modules/explicit",
            "explicit",
            "1.0.0",
            None,
            &[("LICENSE", "Unrelated candidate\n")],
        );
        let report = f.report().unwrap();
        assert_eq!(report["packages_missing"], 1);
        assert_eq!(report["components_missing_evidence"], 4);
        assert!(complete(&report).is_err());
        let parts = report["components"].as_array().unwrap();
        assert!(
            parts
                .iter()
                .any(|p| p["optional"] == true && p["metadata_missing"] == true)
        );
        assert!(parts.iter().any(|p| p["declaration_conflict"] == true));
        assert!(parts.iter().any(|p| p["declared_notice_missing"] == true));
        assert!(run(&f.0, &["web-notices".into()]).is_err());
    }
    #[test]
    fn web_notices_reject_forged_identity_unsafe_paths_sources_and_binary_notices() {
        let f = Fixture::new();
        f.lock(
            "web",
            json!({"node_modules/example":locked("1.0.0",Some("MIT"))}),
        );
        f.package(
            "web",
            "node_modules/example",
            "forged",
            "1.0.0",
            Some("MIT"),
            &[("LICENSE", "Synthetic terms\n")],
        );
        assert!(f.report().is_err());
        f.package(
            "web",
            "node_modules/example",
            "example",
            "2.0.0",
            Some("MIT"),
            &[],
        );
        assert!(f.report().is_err());
        f.package(
            "web",
            "node_modules/example",
            "example",
            "1.0.0",
            Some("MIT"),
            &[],
        );
        assert!(f.report().is_ok());
        for path in [
            "node_modules/../outside",
            "node_modules/example/other/escape",
            "node_modules/@scope",
            "node_modules/.git",
            "node_modules/example\\outside",
        ] {
            f.lock("web", json!({(path):locked("1.0.0",Some("MIT"))}));
            assert!(f.report().is_err());
        }
        for license in [
            "SEE LICENSE IN ../outside",
            "SEE LICENSE IN .npmrc",
            "SEE LICENSE IN legal/../../outside",
        ] {
            f.lock(
                "web",
                json!({"node_modules/example":locked("1.0.0",Some(license))}),
            );
            assert!(f.report().is_err());
        }
        let mut entry = locked("1.0.0", Some("MIT"));
        entry["resolved"] = json!("file:/DO_NOT_READ_OR_EXPORT");
        f.lock("web", json!({"node_modules/example":entry}));
        assert!(!f.report().unwrap_err().contains("DO_NOT_READ"));
        f.lock(
            "web",
            json!({"node_modules/example":locked("1.0.0",Some("MIT"))}),
        );
        fs::write(f.installed("web").join("example/LICENSE"), [0xff, 0x00]).unwrap();
        assert!(f.report().is_err());
    }
    #[cfg(unix)]
    #[test]
    fn web_notices_reject_roots_packages_parent_directories_and_notice_symlinks() {
        use std::os::unix::fs::symlink;
        let f = Fixture::new();
        f.lock(
            "web",
            json!({"node_modules/@fixture/example":locked("1.0.0",Some("MIT"))}),
        );
        f.package(
            "web",
            "node_modules/@fixture/example",
            "@fixture/example",
            "1.0.0",
            Some("MIT"),
            &[("LICENSE", "Synthetic terms\n")],
        );
        let p = f.installed("web").join("@fixture/example");
        fs::rename(p.join("LICENSE"), f.0.join("outside")).unwrap();
        symlink(f.0.join("outside"), p.join("LICENSE")).unwrap();
        assert!(f.report().is_err());
        fs::remove_file(p.join("LICENSE")).unwrap();
        fs::rename(f.0.join("outside"), p.join("LICENSE")).unwrap();
        fs::rename(&p, f.0.join("external-package")).unwrap();
        symlink(f.0.join("external-package"), &p).unwrap();
        assert!(f.report().is_err());
        fs::remove_file(&p).unwrap();
        fs::rename(f.0.join("external-package"), &p).unwrap();
        fs::rename(
            f.installed("web").join("@fixture"),
            f.0.join("external-scope"),
        )
        .unwrap();
        symlink(
            f.0.join("external-scope"),
            f.installed("web").join("@fixture"),
        )
        .unwrap();
        assert!(f.report().is_err());
        fs::remove_file(f.installed("web").join("@fixture")).unwrap();
        fs::rename(
            f.0.join("external-scope"),
            f.installed("web").join("@fixture"),
        )
        .unwrap();
        symlink(f.installed("web"), f.0.join("linked-root")).unwrap();
        assert!(generate(&f.0, &f.0.join("linked-root"), &f.installed("vault-web")).is_err());
        assert!(
            generate(
                &f.0,
                &f.0.join("linked-root/../web_modules"),
                &f.installed("vault-web")
            )
            .is_err()
        );
    }
}
