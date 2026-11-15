//! Shared offline release evidence checks. Metadata is a packager claim, not builder authentication.
use serde_json::Value;
use sha2::{Digest, Sha256};

pub const PROVENANCE: &str = "provenance.json";
pub const INVENTORY: &str = "source.cdx.json";
pub const PROVENANCE_CAP: u64 = 16384;
pub const INVENTORY_CAP: u64 = 8 * 1024 * 1024;
pub const CLAIM: &str = "Packager-reported source checkout and compiler; not authenticated build execution, a minimal binary dependency graph, SLSA provenance, license clearance or a vulnerability audit.";
pub const LOCKS: [&str; 4] = [
    "Cargo.lock",
    "web/package-lock.json",
    "vault-web/package-lock.json",
    "tools/openspec/package-lock.json",
];
pub type Result<T> = std::result::Result<T, &'static str>;
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn invalid() -> &'static str {
    "release_provenance_invalid"
}
fn keys(v: &Value, expected: &[&str]) -> Result<()> {
    let o = v.as_object().ok_or_else(invalid)?;
    if o.len() != expected.len() || expected.iter().any(|k| !o.contains_key(*k)) {
        return Err(invalid());
    }
    Ok(())
}
fn hex(v: &Value, len: usize) -> bool {
    v.as_str().is_some_and(|s| {
        s.len() == len
            && s.bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    })
}
pub fn reference(bytes: &[u8], name: &str) -> Value {
    serde_json::json!({"file":name,"size":bytes.len(),"sha256":hash(bytes)})
}
pub fn validate_reference(v: &Value, name: &str, cap: u64) -> Result<()> {
    keys(v, &["file", "size", "sha256"])?;
    if v["file"] != name
        || !v["size"].as_u64().is_some_and(|n| n > 0 && n <= cap)
        || !hex(&v["sha256"], 64)
    {
        return Err(invalid());
    }
    Ok(())
}
pub fn check_reference(v: &Value, bytes: &[u8], name: &str, cap: u64) -> Result<()> {
    validate_reference(v, name, cap)?;
    if v != &reference(bytes, name) {
        return Err("release_provenance_hash_mismatch");
    }
    Ok(())
}
/// Exact public artifact fields; signed sequence is intentionally outside this reusable evidence.
pub fn artifact(manifest: &Value) -> Value {
    let mut o = serde_json::Map::new();
    if manifest["purpose"] == "moosuri-component-bundle-v1" {
        for key in [
            "purpose",
            "product",
            "component",
            "version",
            "target",
            "binary",
            "size",
            "sha256",
        ] {
            o.insert(key.into(), manifest[key].clone());
        }
        return Value::Object(o);
    }
    for key in [
        "product",
        "version",
        "target",
        "store_schema",
        "embedded_web",
        "binary",
        "size",
        "sha256",
    ] {
        o.insert(key.into(), manifest[key].clone());
    }
    Value::Object(o)
}
pub fn verify(
    reference: &Value,
    provenance: &[u8],
    inventory: &[u8],
    manifest: &Value,
) -> Result<()> {
    check_reference(reference, provenance, PROVENANCE, PROVENANCE_CAP)?;
    let p: Value = serde_json::from_slice(provenance).map_err(|_| invalid())?;
    keys(
        &p,
        &[
            "format",
            "purpose",
            "claim",
            "artifact",
            "source",
            "compiler",
            "inventory",
        ],
    )?;
    if p["format"] != 1
        || p["purpose"] != "moosuri-offline-binary-provenance-v1"
        || p["claim"] != CLAIM
        || p["artifact"] != artifact(manifest)
    {
        return Err(invalid());
    }
    keys(&p["source"], &["revision", "dirty"])?;
    if !(hex(&p["source"]["revision"], 40) || hex(&p["source"]["revision"], 64))
        || !p["source"]["dirty"].is_boolean()
    {
        return Err(invalid());
    }
    keys(&p["compiler"], &["rustc_verbose"])?;
    let compiler = p["compiler"]["rustc_verbose"]
        .as_str()
        .ok_or_else(invalid)?;
    if compiler.len() > 4096
        || !compiler.starts_with("rustc ")
        || !compiler.lines().any(|l| l.starts_with("host: "))
        || !compiler.lines().any(|l| l.starts_with("release: "))
        || compiler.chars().any(|c| c.is_control() && c != '\n')
    {
        return Err(invalid());
    }
    check_reference(&p["inventory"], inventory, INVENTORY, INVENTORY_CAP)?;
    let i: Value = serde_json::from_slice(inventory).map_err(|_| invalid())?;
    if i["bomFormat"] != "CycloneDX"
        || i["specVersion"] != "1.6"
        || i["version"] != 1
        || i.pointer("/metadata/component/name") != Some(&Value::String("moosuri".into()))
        || i.pointer("/metadata/component/version") != manifest.get("version")
        || !i["components"].is_array()
    {
        return Err(invalid());
    }
    let properties = i
        .pointer("/metadata/properties")
        .and_then(Value::as_array)
        .ok_or_else(invalid)?;
    for lock in LOCKS {
        let name = format!("moosuri:lock-sha256:{lock}");
        let found = properties
            .iter()
            .filter(|v| v["name"] == name)
            .collect::<Vec<_>>();
        if found.len() != 1 || !hex(&found[0]["value"], 64) {
            return Err(invalid());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn release_provenance_rejects_paths_bytes_target_and_unbounded_claims() {
        let m = json!({"product":"moosuri","version":"1.0.0","target":"synthetic-target","store_schema":47,"embedded_web":true,"binary":"moosuri","size":4,"sha256":"ab".repeat(32)});
        let i=serde_json::to_vec(&json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"component":{"name":"moosuri","version":"1.0.0"},"properties":LOCKS.iter().map(|p|json!({"name":format!("moosuri:lock-sha256:{p}"),"value":"cd".repeat(32)})).collect::<Vec<_>>()},"components":[]})).unwrap();
        let p = json!({"format":1,"purpose":"moosuri-offline-binary-provenance-v1","claim":CLAIM,"artifact":artifact(&m),"source":{"revision":"ab".repeat(20),"dirty":false},"compiler":{"rustc_verbose":"rustc 1.89.0\nhost: synthetic-host\nrelease: 1.89.0\n"},"inventory":reference(&i,INVENTORY)});
        let b = serde_json::to_vec(&p).unwrap();
        let r = reference(&b, PROVENANCE);
        assert!(verify(&r, &b, &i, &m).is_ok());
        let mut bad = r.clone();
        bad["file"] = json!("../provenance.json");
        assert!(verify(&bad, &b, &i, &m).is_err());
        assert!(verify(&r, &b, b"changed", &m).is_err());
        let mut bad = m.clone();
        bad["target"] = json!("other");
        assert!(verify(&r, &b, &i, &bad).is_err());
        let mut bad = p.clone();
        bad["source"]["revision"] = json!("private/path");
        let bb = serde_json::to_vec(&bad).unwrap();
        assert!(verify(&reference(&bb, PROVENANCE), &bb, &i, &m).is_err());
        let mut bad = p;
        bad["inventory"]["file"] = json!("../source.cdx.json");
        let bb = serde_json::to_vec(&bad).unwrap();
        assert!(verify(&reference(&bb, PROVENANCE), &bb, &i, &m).is_err());
    }
}
