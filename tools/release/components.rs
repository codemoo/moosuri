//! Strict optional-component bundle contract; signed metadata is a packager claim.
use super::release_evidence as evidence;
use serde_json::{Value, json};
pub const PURPOSE: &str = "moosuri-component-bundle-v1";
pub const BINARY_CAP: u64 = 256 * 1024 * 1024;
pub const TARGETS: [&str; 4] = [
    "x86_64-unknown-linux-gnu",
    "aarch64-unknown-linux-gnu",
    "x86_64-apple-darwin",
    "aarch64-apple-darwin",
];
pub fn binary(component: &str) -> Result<&'static str, &'static str> {
    match component {
        "credential-broker" => Ok("moosuri-credential-broker"),
        "vault-service" => Ok("moosuri-vault-service"),
        _ => Err("component_bundle_component_refused"),
    }
}
pub fn validate(m: &Value, component: &str, target: &str) -> Result<(), &'static str> {
    let required = [
        "format",
        "purpose",
        "product",
        "component",
        "version",
        "target",
        "sequence",
        "binary",
        "size",
        "sha256",
    ];
    let object = m.as_object().ok_or("component_bundle_manifest_invalid")?;
    if required.iter().any(|k| !object.contains_key(*k))
        || object
            .keys()
            .any(|k| !required.contains(&k.as_str()) && k != "provenance")
        || m["format"] != 1
        || m["purpose"] != PURPOSE
        || m["product"] != "moosuri"
        || m["component"] != component
        || m["target"] != target
        || !TARGETS.contains(&target)
        || m["binary"] != binary(component)?
        || m["sequence"].as_u64().is_none_or(|n| n == 0)
        || !m["size"].as_u64().is_some_and(|n| n > 0 && n <= BINARY_CAP)
        || !m["sha256"].as_str().is_some_and(|s| {
            s.len() == 64
                && s.bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
        || !m["version"].as_str().is_some_and(version)
    {
        return Err("component_bundle_manifest_invalid");
    }
    if let Some(reference) = m.get("provenance") {
        evidence::validate_reference(reference, evidence::PROVENANCE, evidence::PROVENANCE_CAP)?;
    }
    Ok(())
}
// Dependency-free strict SemVer validation shared with the small xtask packager.
// Core numbers and numeric prerelease identifiers cannot contain leading zeros.
fn version(value: &str) -> bool {
    if value.is_empty() || value.len() > 64 || !value.is_ascii() {
        return false;
    }
    let (pre, build) = value
        .split_once('+')
        .map_or((value, None), |(a, b)| (a, Some(b)));
    let identifiers = |s: &str| {
        !s.is_empty()
            && s.split('.')
                .all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'))
    };
    if build.is_some_and(|s| !identifiers(s)) {
        return false;
    }
    let (core, pre) = pre
        .split_once('-')
        .map_or((pre, None), |(a, b)| (a, Some(b)));
    if pre.is_some_and(|s| {
        !identifiers(s)
            || s.split('.')
                .any(|p| p.bytes().all(|b| b.is_ascii_digit()) && p.len() > 1 && p.starts_with('0'))
    }) {
        return false;
    }
    let parts: Vec<_> = core.split('.').collect();
    parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && (p.len() == 1 || !p.starts_with('0'))
                && p.bytes().all(|b| b.is_ascii_digit())
                && p.parse::<u64>().is_ok()
        })
}
pub fn make(
    component: &str,
    target: &str,
    version: &str,
    sequence: u64,
    bytes: &[u8],
) -> Result<Value, &'static str> {
    let m = json!({"format":1,"purpose":PURPOSE,"product":"moosuri","component":component,"target":target,"version":version,"sequence":sequence,"binary":binary(component)?,"size":bytes.len(),"sha256":evidence::hash(bytes)});
    validate(&m, component, target)?;
    verify_binary(&m, bytes)?;
    Ok(m)
}
pub fn verify_binary(m: &Value, bytes: &[u8]) -> Result<(), &'static str> {
    if m["size"].as_u64() != Some(bytes.len() as u64) || m["sha256"] != evidence::hash(bytes) {
        return Err("component_bundle_binary_hash_mismatch");
    }
    // Check the executable's machine/container without executing it. The full
    // target ABI, component behavior and version remain signed packager claims.
    let target = m["target"]
        .as_str()
        .ok_or("component_bundle_target_refused")?;
    let arch = if target.starts_with("aarch64-") {
        183u16
    } else {
        62u16
    };
    let valid = if target.ends_with("-linux-gnu") {
        elf(bytes, arch)
    } else if target.ends_with("-apple-darwin") {
        macho(bytes, if arch == 183 { 0x0100000c } else { 0x01000007 })
    } else {
        false
    };
    if !valid {
        return Err("component_bundle_executable_target_mismatch");
    }
    Ok(())
}
fn u16le(b: &[u8], n: usize) -> u16 {
    u16::from_le_bytes([b[n], b[n + 1]])
}
fn u32le(b: &[u8], n: usize) -> u32 {
    u32::from_le_bytes(b[n..n + 4].try_into().unwrap())
}
fn u64le(b: &[u8], n: usize) -> u64 {
    u64::from_le_bytes(b[n..n + 8].try_into().unwrap())
}
fn range(offset: u64, size: u64, len: usize) -> bool {
    offset
        .checked_add(size)
        .is_some_and(|end| end <= len as u64)
}
fn elf(b: &[u8], arch: u16) -> bool {
    if b.len() < 64
        || &b[..7] != b"\x7fELF\x02\x01\x01"
        || !matches!(u16le(b, 16), 2 | 3)
        || u16le(b, 18) != arch
        || u32le(b, 20) != 1
        || u16le(b, 52) != 64
        || u16le(b, 54) != 56
    {
        return false;
    }
    let phoff = u64le(b, 32);
    let count = u16le(b, 56) as u64;
    if phoff < 64 || !(1..=128).contains(&count) || !range(phoff, count * 56, b.len()) {
        return false;
    }
    let shoff = u64le(b, 40);
    let shcount = u16le(b, 60) as u64;
    if shcount > 0 && (u16le(b, 58) != 64 || !range(shoff, shcount * 64, b.len())) {
        return false;
    }
    let entry = u64le(b, 24);
    let mut executable_entry = false;
    for index in 0..count {
        let n = (phoff + index * 56) as usize;
        let offset = u64le(b, n + 8);
        let address = u64le(b, n + 16);
        let filesz = u64le(b, n + 32);
        let memsz = u64le(b, n + 40);
        if !range(offset, filesz, b.len()) {
            return false;
        }
        if u32le(b, n) == 1 {
            if filesz > memsz {
                return false;
            }
            if u32le(b, n + 4) & 1 != 0
                && filesz > 0
                && entry >= address
                && address.checked_add(filesz).is_some_and(|end| entry < end)
            {
                executable_entry = true;
            }
        }
        if u32le(b, n) == 3 && (filesz < 2 || b[(offset + filesz - 1) as usize] != 0) {
            return false;
        }
    }
    executable_entry
}
fn macho(b: &[u8], arch: u32) -> bool {
    if b.len() < 32
        || b[..4] != [0xcf, 0xfa, 0xed, 0xfe]
        || u32le(b, 4) != arch
        || u32le(b, 12) != 2
    {
        return false;
    }
    let count = u32le(b, 16) as usize;
    let commands = u32le(b, 20) as usize;
    if !(1..=4096).contains(&count) || commands < count * 8 || !range(32, commands as u64, b.len())
    {
        return false;
    }
    let mut n = 32usize;
    let end = 32 + commands;
    let mut entry = None;
    let mut executable = vec![];
    for _ in 0..count {
        if n + 8 > end {
            return false;
        }
        let command = u32le(b, n);
        let size = u32le(b, n + 4) as usize;
        if size < 8 || size % 8 != 0 || size > end - n {
            return false;
        }
        if command == 0x19 {
            if size < 72 || (u32le(b, n + 64) as u64) * 80 + 72 != size as u64 {
                return false;
            }
            let offset = u64le(b, n + 40);
            let filesz = u64le(b, n + 48);
            if !range(offset, filesz, b.len()) || filesz > u64le(b, n + 32) {
                return false;
            }
            if u32le(b, n + 60) & 4 != 0 && filesz > 0 {
                executable.push((offset, filesz));
            }
        }
        if command == 0x80000028 {
            if size != 24 || entry.is_some() {
                return false;
            }
            entry = Some(u64le(b, n + 8));
        }
        n += size;
    }
    n == end
        && entry.is_some_and(|e| {
            executable
                .iter()
                .any(|(start, size)| e >= *start && e - start < *size)
        })
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn component_native_tables_bound_architecture_and_reject_truncation() {
        let mut bytes = vec![0; 136];
        bytes[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        bytes[4..8].copy_from_slice(&0x0100000cu32.to_le_bytes());
        bytes[12] = 2;
        bytes[16] = 2;
        bytes[20] = 96;
        bytes[32] = 0x19;
        bytes[36] = 72;
        bytes[64] = 136;
        bytes[80] = 136;
        bytes[92] = 4;
        bytes[104..108].copy_from_slice(&0x80000028u32.to_le_bytes());
        bytes[108] = 24;
        bytes[112] = 128;
        assert!(make("credential-broker", TARGETS[3], "1.0.0", 1, &bytes).is_ok());
        assert!(make("credential-broker", TARGETS[2], "1.0.0", 1, &bytes).is_err());
        assert!(make("vault-service", TARGETS[3], "1.0.0", 1, &bytes[..128]).is_err());
        bytes[108] = 32; // Load-command size exceeds the signed table boundary.
        assert!(make("vault-service", TARGETS[3], "1.0.0", 1, &bytes).is_err());
    }
    #[test]
    fn components_bind_identity_paths_target_version_and_bytes() {
        let mut bytes = vec![0; 128];
        bytes[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
        bytes[16] = 3;
        bytes[18] = 62;
        bytes[20] = 1;
        bytes[24] = 120;
        bytes[32] = 64;
        bytes[52] = 64;
        bytes[54] = 56;
        bytes[56] = 1;
        bytes[64] = 1;
        bytes[68] = 1;
        bytes[96] = 128;
        bytes[104] = 128;
        let m = make("credential-broker", TARGETS[0], "1.2.3-dev.1", 1, &bytes).unwrap();
        assert!(validate(&m, "vault-service", TARGETS[0]).is_err());
        assert!(validate(&m, "credential-broker", TARGETS[1]).is_err());
        let mut bad = m.clone();
        bad["binary"] = json!("../escape");
        assert!(validate(&bad, "credential-broker", TARGETS[0]).is_err());
        bad = m.clone();
        bad["provenance"] = Value::Null;
        assert!(validate(&bad, "credential-broker", TARGETS[0]).is_err());
        for v in ["01.2.3", "1.2", "1.2.3-01", "1.2.3+", "1.2.3+x+y"] {
            assert!(!version(v));
        }
        assert!(version("1.2.3-dev.1+build.9"));
        assert!(make("credential-broker", TARGETS[0], "1.2.3", 1, &bytes[..64]).is_err());
        let mut malformed = bytes.clone();
        malformed[96] = 255;
        assert!(make("credential-broker", TARGETS[0], "1.2.3", 1, &malformed).is_err());
        bytes[19] = 1;
        assert!(verify_binary(&m, &bytes).is_err());
        assert!(make("vault-service", TARGETS[1], "1.2.3", 1, &bytes).is_err());
    }
}
