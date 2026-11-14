//! Deterministic source-lock inventory; never resolves packages or reads credentials.
use super::Check;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, io::Write, path::Path};

const LOCKS: [&str; 4] = [
    "Cargo.lock",
    "web/package-lock.json",
    "vault-web/package-lock.json",
    "tools/openspec/package-lock.json",
];
const MAX_LOCK_BYTES: u64 = 8 * 1024 * 1024;

fn failure() -> String {
    "invalid or unsupported public dependency lock".into()
}
fn label(value: &str) -> Check<&str> {
    if value.is_empty()
        || value.len() > 200
        || !value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._+@/".contains(&b))
    {
        return Err(failure());
    }
    Ok(value)
}
fn encoded(value: &str) -> String {
    let mut result = String::new();
    for b in value.bytes() {
        if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
            result.push(b as char);
        } else {
            result.push_str(&format!("%{b:02X}"));
        }
    }
    result
}
fn property(name: &str, value: impl Into<String>) -> Value {
    json!({"name":name,"value":value.into()})
}
fn component(purl: &str, name: &str, version: &str, properties: Vec<Value>) -> Value {
    json!({"type":"library","bom-ref":purl,"name":name,"version":version,"purl":purl,"properties":properties})
}
fn cargo(input: &str, components: &mut BTreeMap<String, Value>) -> Check<String> {
    let lock: toml::Value = toml::from_str(input).map_err(|_| failure())?;
    if lock.get("version").and_then(toml::Value::as_integer) != Some(4) {
        return Err(failure());
    }
    let packages = lock
        .get("package")
        .and_then(toml::Value::as_array)
        .ok_or_else(failure)?;
    if packages.len() > 10000 {
        return Err(failure());
    }
    let mut product_version = None;
    for p in packages {
        let name = label(
            p.get("name")
                .and_then(toml::Value::as_str)
                .ok_or_else(failure)?,
        )?;
        let version = label(
            p.get("version")
                .and_then(toml::Value::as_str)
                .ok_or_else(failure)?,
        )?;
        let source = p.get("source").and_then(toml::Value::as_str);
        let workspace = source.is_none();
        if source.is_some_and(|s| s != "registry+https://github.com/rust-lang/crates.io-index") {
            // Do not put private registries, git credentials or local paths into an inventory.
            return Err(
                "SBOM requires reviewed handling for non-crates.io dependency sources".into(),
            );
        }
        let prefix = if workspace {
            "generic/moosuri"
        } else {
            "cargo"
        };
        let purl = format!("pkg:{prefix}/{}@{}", encoded(name), encoded(version));
        let mut item = component(
            &purl,
            name,
            version,
            vec![
                property("moosuri:lockfile", "Cargo.lock"),
                property(
                    "moosuri:source-kind",
                    if workspace {
                        "local-lock-entry"
                    } else {
                        "crates.io"
                    },
                ),
            ],
        );
        if !workspace {
            let hash = p
                .get("checksum")
                .and_then(toml::Value::as_str)
                .ok_or_else(failure)?;
            if hash.len() != 64 || !hash.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(failure());
            }
            item["hashes"] = json!([{"alg":"SHA-256","content":hash.to_ascii_lowercase()}]);
        }
        if workspace && name == "moosuri" && product_version.replace(version.to_owned()).is_some() {
            return Err(failure());
        }
        if components.insert(purl, item).is_some() {
            return Err(failure());
        }
    }
    product_version.ok_or_else(failure)
}
fn npm(input: &str, lockfile: &str, components: &mut BTreeMap<String, Value>) -> Check<()> {
    let lock: Value = serde_json::from_str(input).map_err(|_| failure())?;
    if lock.get("lockfileVersion").and_then(Value::as_u64) != Some(3) {
        return Err(failure());
    }
    let packages = lock
        .get("packages")
        .and_then(Value::as_object)
        .ok_or_else(failure)?;
    if packages.len() > 10000 {
        return Err(failure());
    }
    for (path, p) in packages {
        if path.is_empty() {
            continue;
        }
        if p.get("link").and_then(Value::as_bool) == Some(true) {
            return Err(failure());
        }
        // Keep installation locations only as source-relative public lock keys.
        if path.len() > 1024
            || !path.starts_with("node_modules/")
            || path
                .split('/')
                .any(|s| s.is_empty() || s == "." || s == "..")
        {
            return Err(failure());
        }
        let from_path = path.rsplit_once("node_modules/").ok_or_else(failure)?.1;
        let full_name = label(p.get("name").and_then(Value::as_str).unwrap_or(from_path))?;
        let version = label(
            p.get("version")
                .and_then(Value::as_str)
                .ok_or_else(failure)?,
        )?;
        let (group, name) = if full_name.starts_with('@') {
            let (group, name) = full_name.split_once('/').ok_or_else(failure)?;
            if name.contains('/') {
                return Err(failure());
            }
            (Some(group), name)
        } else {
            if full_name.contains('/') {
                return Err(failure());
            }
            (None, full_name)
        };
        let purl = format!(
            "pkg:npm/{}{}@{}",
            group
                .map(|s| format!("{}/", encoded(s)))
                .unwrap_or_default(),
            encoded(name),
            encoded(version)
        );
        let location = format!("{lockfile}#{path}");
        let mut properties = vec![
            property("moosuri:lock-location", location),
            property(
                "moosuri:lock-development",
                p.get("dev")
                    .and_then(Value::as_bool)
                    .unwrap_or(false)
                    .to_string(),
            ),
        ];
        if let Some(integrity) = p.get("integrity").and_then(Value::as_str) {
            if integrity.len() > 512
                || !integrity
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"-+/= ".contains(&b))
            {
                return Err(failure());
            }
            properties.push(property("moosuri:npm-integrity", integrity));
        }
        let mut item = component(&purl, name, version, properties);
        if let Some(group) = group {
            item["group"] = json!(group);
        }
        // Preserve reported license labels without asserting SPDX expression validity.
        if let Some(license) = p.get("license").and_then(Value::as_str) {
            if license.len() > 256 || license.chars().any(char::is_control) {
                return Err(failure());
            }
            item["licenses"] = json!([{"license":{"name":license}}]);
        }
        if let Some(prior) = components.get_mut(&purl) {
            if prior.get("licenses") != item.get("licenses") {
                return Err("conflicting npm license metadata in public locks".into());
            }
            let values = prior["properties"].as_array_mut().ok_or_else(failure)?;
            values.extend(
                item["properties"]
                    .as_array()
                    .ok_or_else(failure)?
                    .iter()
                    .cloned(),
            );
        } else {
            components.insert(purl, item);
        }
    }
    Ok(())
}
fn inventory(inputs: &[(&str, &str)]) -> Check<Value> {
    if inputs.iter().map(|p| p.0).collect::<Vec<_>>() != LOCKS {
        return Err(failure());
    }
    let mut components = BTreeMap::new();
    let version = cargo(inputs[0].1, &mut components)?;
    for (path, input) in &inputs[1..] {
        npm(input, path, &mut components)?;
    }
    for c in components.values_mut() {
        let properties = c["properties"].as_array_mut().ok_or_else(failure)?;
        properties.sort_by_key(Value::to_string);
        properties.dedup();
    }
    let mut properties = vec![
        property(
            "moosuri:inventory-scope",
            "Full source-lock inventory: all Rust target/dev entries, web/vault build dependencies and OpenSpec tooling; not a minimal or verified binary dependency graph.",
        ),
        property(
            "moosuri:license-scope",
            "npm lock labels only; Cargo.lock has no license evidence. This is not license clearance or a vulnerability audit.",
        ),
    ];
    for (path, input) in inputs {
        properties.push(property(
            &format!("moosuri:lock-sha256:{path}"),
            format!("{:x}", Sha256::digest(input.as_bytes())),
        ));
    }
    Ok(
        json!({"$schema":"https://cyclonedx.org/schema/bom-1.6.schema.json","bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"component":{"type":"application","bom-ref":"moosuri-source","name":"moosuri","version":version},"properties":properties},"components":components.into_values().collect::<Vec<_>>()}),
    )
}
pub(super) fn generate(root: &Path) -> Check<Vec<u8>> {
    let mut contents = Vec::new();
    for path in LOCKS {
        let p = root.join(path);
        let metadata = fs::symlink_metadata(&p).map_err(|_| failure())?;
        if !metadata.is_file() || metadata.len() > MAX_LOCK_BYTES {
            return Err(failure());
        }
        contents.push(fs::read_to_string(p).map_err(|_| failure())?);
    }
    let inputs: Vec<_> = LOCKS
        .into_iter()
        .zip(contents.iter().map(String::as_str))
        .collect();
    let value = inventory(&inputs)?;
    let mut bytes = serde_json::to_vec_pretty(&value).map_err(|_| failure())?;
    bytes.push(b'\n');
    Ok(bytes)
}
pub(super) fn print(root: &Path) -> Check<()> {
    std::io::stdout()
        .lock()
        .write_all(&generate(root)?)
        .map_err(|_| "cannot write SBOM".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn inventory_preserves_versions_nested_installations_and_public_evidence_only() {
        let rust = format!(
            "version = 4\n[[package]]\nname = \"moosuri\"\nversion = \"0.1.0\"\n[[package]]\nname = \"sample\"\nversion = \"1.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n[[package]]\nname = \"sample\"\nversion = \"2.0.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n",
            "ab".repeat(32),
            "cd".repeat(32)
        );
        let js = r#"{"lockfileVersion":3,"packages":{"":{},"node_modules/@fixture/example":{"version":"1.0.0","license":"MIT","dev":true,"resolved":"https://private.invalid/DO_NOT_EXPORT"},"node_modules/outer/node_modules/@fixture/example":{"version":"1.0.0","license":"MIT"}}}"#;
        let empty = r#"{"lockfileVersion":3,"packages":{"":{}}}"#;
        let inputs = [
            (LOCKS[0], rust.as_str()),
            (LOCKS[1], js),
            (LOCKS[2], empty),
            (LOCKS[3], empty),
        ];
        let result = inventory(&inputs).unwrap();
        let encoded = serde_json::to_string(&result).unwrap();
        assert!(!encoded.contains("DO_NOT_EXPORT"));
        let components = result["components"].as_array().unwrap();
        assert_eq!(components.len(), 4);
        assert!(
            components
                .iter()
                .any(|c| c["purl"] == "pkg:cargo/sample@1.0.0")
        );
        assert!(
            components
                .iter()
                .any(|c| c["purl"] == "pkg:cargo/sample@2.0.0")
        );
        let scoped = components
            .iter()
            .find(|c| c["purl"] == "pkg:npm/%40fixture/example@1.0.0")
            .unwrap();
        assert_eq!(scoped["group"], "@fixture");
        assert_eq!(
            scoped["properties"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|p| p["name"] == "moosuri:lock-location")
                .count(),
            2
        );
        assert_eq!(inventory(&inputs).unwrap(), result);
        let unsafe_source = rust.replace(
            "registry+https://github.com/rust-lang/crates.io-index",
            "git+https://name:DO_NOT_EXPORT@private.invalid/project",
        );
        let error = inventory(&[
            (LOCKS[0], &unsafe_source),
            (LOCKS[1], js),
            (LOCKS[2], empty),
            (LOCKS[3], empty),
        ])
        .unwrap_err();
        assert!(!error.contains("DO_NOT_EXPORT"));
        let traversal = js.replace("node_modules/@fixture/example", "node_modules/../example");
        assert!(
            inventory(&[
                (LOCKS[0], &rust),
                (LOCKS[1], &traversal),
                (LOCKS[2], empty),
                (LOCKS[3], empty)
            ])
            .is_err()
        );
    }
}
