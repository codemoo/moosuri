//! Root-authored isolation evidence. Root builds the credential-free candidate
//! profile from root-verified artifacts, launches the existing native admission
//! as the dedicated worker identity BEFORE any credential can exist, and accepts
//! its output only if every byte reproduces from root's own inputs. A profile
//! string, digest or receipt supplied by anyone else is never evidence.
use super::record::{Evidence, Storage};
use super::unit::as_worker;
use crate::installation::claude_runtime::PreparedRuntime;
use crate::installation::kiri::host::{self, Exec, Input, Layout, Result};
use moosuri_connectors::claude_code::IsolationReceipt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{fs, path::Path, path::PathBuf, time::Duration};
use uuid::Uuid;

/// Native output bound (the receipt is ~1 KiB, the profile <2 KiB).
pub(super) const OUTPUT_CAP: usize = 32 * 1024;

/// Field-for-field and order-for-order copy of the connector profile. Key order
/// is part of the digest, so this struct IS the canonical serialization.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProfileConfig {
    pub engine: PathBuf,
    pub engine_sha256: String,
    pub engine_socket: PathBuf,
    pub image: String,
    pub official_cli: PathBuf,
    pub cli_sha256: String,
    pub cli_version: String,
    pub helper: PathBuf,
    pub helper_sha256: String,
    pub auth_root: PathBuf,
    pub scratch_root: PathBuf,
    pub isolation_acceptance_sha256: String,
    pub hosting_terms_accepted: bool,
}

fn sha(bytes: &[u8]) -> String {
    host::sha256_bytes(bytes)
}
fn json<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| "claude_connection_evidence_invalid".into())
}
fn invalid<E>(_: E) -> String {
    "claude_connection_evidence_invalid".to_owned()
}

/// The acceptance-free candidate, derived only from root-verified facts.
pub(super) fn candidate(runtime: &PreparedRuntime, storage: &Storage) -> Result<ProfileConfig> {
    storage.validate()?;
    let comma = |p: &Path| p.as_os_str().as_encoded_bytes().contains(&b',');
    if runtime.image.is_empty()
        || runtime.service.uid == 0
        || [
            &runtime.engine.path,
            &runtime.official_cli.path,
            &runtime.helper.path,
            &runtime.rootless_socket,
        ]
        .into_iter()
        .any(|p| !p.is_absolute() || comma(p))
    {
        return Err("claude_connection_evidence_invalid".into());
    }
    Ok(ProfileConfig {
        engine: runtime.engine.path.clone(),
        engine_sha256: runtime.engine.sha256.clone(),
        engine_socket: runtime.rootless_socket.clone(),
        image: runtime.image.clone(),
        official_cli: runtime.official_cli.path.clone(),
        cli_sha256: runtime.official_cli.sha256.clone(),
        cli_version: runtime.official_cli.version.clone(),
        helper: runtime.helper.path.clone(),
        helper_sha256: runtime.helper.sha256.clone(),
        auth_root: storage.auth_root.clone(),
        scratch_root: storage.scratch_root.clone(),
        isolation_acceptance_sha256: String::new(),
        hosting_terms_accepted: true,
    })
}

#[derive(Debug)]
pub(super) struct Admitted {
    pub candidate_sha256: String,
    pub profile_json: String,
    pub profile_digest: String,
    pub receipt_json: String,
    pub receipt_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Output {
    profile_json: String,
    profile_digest: String,
    receipt: IsolationReceipt,
}
#[derive(Deserialize)]
struct RawOutput {
    receipt: Value,
}

/// Root's independent copy of the receipt's acceptance rule (the child applied
/// it too). Unknown or missing facts are refusals.
fn acceptable(v: &Value) -> bool {
    let number = |k: &str| v[k].as_u64();
    let yes = |k: &str| v[k] == true;
    v["version"] == 1
        && v["nonce"]
            .as_str()
            .is_some_and(|n| Uuid::parse_str(n).is_ok())
        && number("memory_bytes") == Some(805_306_368)
        && number("cpu_quota") == Some(100_000)
        && number("cpu_period") == Some(100_000)
        && number("pids") == Some(32)
        && number("auth_filesystem_bytes").is_some_and(|n| (1..=128 * 1024 * 1024).contains(&n))
        && number("auth_filesystem_inodes").is_some_and(|n| (1..=32768).contains(&n))
        && [
            "rootless_uid_namespace",
            "no_new_privileges",
            "seccomp",
            "capabilities_dropped",
            "host_and_sibling_inaccessible",
            "immutable_mounts",
            "no_external_interface_or_route",
            "direct_network_denied",
            "proxy_destination_denied",
        ]
        .iter()
        .copied()
        .all(|k: &str| yes(k))
        && v["official_cli_version"] == "2.1.322"
}

/// Rebuild the whole admission from `expected` and compare byte for byte. The
/// candidate must carry no prior acceptance.
pub(super) fn verify_output(expected: &ProfileConfig, out: &[u8]) -> Result<Admitted> {
    if out.len() > OUTPUT_CAP || !expected.isolation_acceptance_sha256.is_empty() {
        return Err("claude_connection_evidence_invalid".into());
    }
    let parsed: Output = serde_json::from_slice(out).map_err(invalid)?;
    let raw: RawOutput = serde_json::from_slice(out).map_err(invalid)?;
    if !acceptable(&raw.receipt) {
        return Err("claude_connection_isolation_rejected".into());
    }
    // The probe binds itself to the acceptance-free candidate…
    let candidate_bytes = json(expected)?;
    let mut bound = expected.clone();
    bound.isolation_acceptance_sha256 = sha(&candidate_bytes);
    let candidate_digest = sha(&json(&bound)?);
    if raw.receipt["candidate_profile_digest"] != candidate_digest.as_str() {
        return Err("claude_connection_evidence_mismatch".into());
    }
    // …and the final profile's acceptance is the hash of that exact receipt.
    let receipt_bytes = json(&parsed.receipt)?;
    let mut accepted = expected.clone();
    accepted.isolation_acceptance_sha256 = sha(&receipt_bytes);
    let profile_bytes = json(&accepted)?;
    if parsed.profile_json.as_bytes() != profile_bytes.as_slice()
        || parsed.profile_digest != sha(&profile_bytes)
    {
        return Err("claude_connection_evidence_mismatch".into());
    }
    Ok(Admitted {
        candidate_sha256: sha(&candidate_bytes),
        profile_json: parsed.profile_json,
        profile_digest: parsed.profile_digest,
        receipt_json: String::from_utf8(receipt_bytes).map_err(invalid)?,
        receipt_sha256: sha(&json(&parsed.receipt)?),
    })
}

/// Exact candidate bytes for the protected input file.
pub(super) fn candidate_bytes(expected: &ProfileConfig) -> Result<Vec<u8>> {
    json(expected)
}

/// Launch the existing credential-free native admission as the worker (root
/// runs the exact pinned helper; the worker only reads the protected file).
pub(super) fn run_admission(
    exec: &dyn Exec,
    runtime: &PreparedRuntime,
    candidate_file: &Path,
    expected: &ProfileConfig,
) -> Result<Admitted> {
    let tail = vec![
        runtime
            .helper
            .path
            .to_str()
            .ok_or("claude_connection_path_invalid")?
            .to_owned(),
        "claude-engine-isolation".to_owned(),
        "--profile-file".to_owned(),
        candidate_file
            .to_str()
            .ok_or("claude_connection_path_invalid")?
            .to_owned(),
    ];
    let out = exec.run(
        "runuser",
        &as_worker(&runtime.service.username, &tail),
        Input::None,
        Duration::from_secs(150),
    )?;
    if out.code != 0 {
        return Err("claude_connection_isolation_rejected".into());
    }
    verify_output(expected, &out.stdout)
}

/// Re-derive everything stored in a record. A hand-edited or mixed record
/// (other helper, other acceptance, other receipt) is refused.
pub(super) fn validate_stored(ev: &Evidence) -> Result<()> {
    let expected = candidate(&ev.runtime, &ev.storage)?;
    let mut stored: ProfileConfig = serde_json::from_str(&ev.profile_json).map_err(invalid)?;
    let acceptance = std::mem::take(&mut stored.isolation_acceptance_sha256);
    let canonical = json(&{
        let mut c = stored.clone();
        c.isolation_acceptance_sha256 = acceptance.clone();
        c
    })?;
    if stored != expected
        || canonical != ev.profile_json.as_bytes()
        || acceptance != ev.receipt_sha256
        || acceptance != sha(ev.receipt_json.as_bytes())
        || ev.profile_digest != sha(ev.profile_json.as_bytes())
        || ev.candidate_sha256 != sha(&json(&expected)?)
        || !host::is_hex(&acceptance, 64)
    {
        return Err("claude_connection_evidence_mismatch".into());
    }
    Ok(())
}

/// Only an auth root with no login state may ever be probed: empty, or holding
/// only the reserved credential-free probe's own leftovers.
pub(super) fn auth_root_fresh(layout: &Layout, auth_root: &Path) -> Result<bool> {
    let actual = layout.at(auth_root.to_str().ok_or("claude_connection_path_invalid")?);
    let reserved = |name: &str| {
        let stem = name.strip_suffix(".lock").unwrap_or(name);
        Uuid::parse_str(stem).is_ok_and(|id| id.get_version_num() == 8)
    };
    for entry in fs::read_dir(&actual).map_err(|_| "claude_connection_storage_invalid")? {
        let entry = entry.map_err(|_| "claude_connection_storage_invalid")?;
        let name = entry.file_name();
        let name = name.to_str().ok_or("claude_connection_storage_invalid")?;
        if !(name.starts_with("isolation-sibling-") || reserved(name)) {
            return Ok(false);
        }
    }
    Ok(true)
}
