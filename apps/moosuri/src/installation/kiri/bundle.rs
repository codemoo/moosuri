//! Private source-built artifact bundle for the clean Kiri installer.
//! The hashes are local integrity checks chosen by root, not a signature or
//! independent attestation. The manifest cannot name a command, path or hook.
use super::host::{self, Layout, Result};
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
};

const MANIFEST_CAP: u64 = 64 * 1024;
const BINARY_CAP: u64 = 256 << 20;
const IMAGE_CAP: u64 = 4 << 30;
/// Current Kiri-capable derived profile from `deployment/browser/seccomp.json`.
pub const SECCOMP_SHA256: &str = "25a51bedbc152a36eb3ec4557231c18928f640ea61c9131499b3bc5491e03dc2";
pub const FILES: [(&str, u64); 6] = [
    ("moosuri", BINARY_CAP),
    ("moosuri-vault-service", BINARY_CAP),
    ("member.tar", IMAGE_CAP),
    ("browser.tar", IMAGE_CAP),
    ("google-auth.tar", IMAGE_CAP),
    ("seccomp.json", 64 * 1024),
];

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
pub struct Images {
    pub member: String,
    pub browser: String,
    pub google_auth: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    source_revision: String,
    files: BTreeMap<String, String>,
    images: Images,
}
#[derive(Clone, Debug)]
pub struct Bundle {
    pub dir: PathBuf,
    pub manifest_sha256: String,
    pub source_revision: String,
    pub files: BTreeMap<String, String>,
    pub images: Images,
}

fn image_id(s: &str) -> bool {
    s.strip_prefix("sha256:")
        .is_some_and(|h| host::is_hex(h, 64))
}

/// Validate every artifact before anything is executed or copied.
pub fn validate(layout: &Layout, dir: &Path) -> Result<Bundle> {
    if !dir.is_absolute()
        || dir.to_str().is_none()
        || dir.components().any(|c| {
            matches!(
                c,
                std::path::Component::ParentDir | std::path::Component::CurDir
            )
        })
        || fs::canonicalize(dir).ok().as_deref() != Some(dir)
    {
        return Err("kiri_install_bundle_path_invalid".into());
    }
    let m = fs::symlink_metadata(dir).map_err(|_| "kiri_install_bundle_unavailable")?;
    if !m.is_dir() || m.uid() != host::euid() || m.mode() & 0o022 != 0 {
        return Err("kiri_install_bundle_directory_untrusted".into());
    }
    layout.safe_ancestors(dir)?;
    if layout.exists(&dir.join(".incomplete"))? {
        return Err("kiri_install_bundle_incomplete".into());
    }
    let manifest_path = dir.join("manifest.json");
    let (mut file, _) = host::open_regular(&manifest_path, MANIFEST_CAP)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(MANIFEST_CAP + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "kiri_install_manifest_unreadable")?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| "kiri_install_manifest_invalid")?;
    if manifest.format != 1
        || !host::is_hex(&manifest.source_revision, 40)
        || manifest.files.len() != FILES.len()
        || !image_id(&manifest.images.member)
        || !image_id(&manifest.images.browser)
        || !image_id(&manifest.images.google_auth)
        // The browser and official Chrome authentication images are different roles.
        || manifest.images.member == manifest.images.browser
        || manifest.images.member == manifest.images.google_auth
        || manifest.images.browser == manifest.images.google_auth
    {
        return Err("kiri_install_manifest_invalid".into());
    }
    for (name, cap) in FILES {
        let expected = manifest
            .files
            .get(name)
            .filter(|h| host::is_hex(h, 64))
            .ok_or("kiri_install_manifest_invalid")?;
        // Hash streaming never loads an image archive into memory.
        let actual = host::sha256_file(&dir.join(name), cap)?;
        if &actual != expected {
            return Err(format!(
                "kiri_install_artifact_hash_mismatch_{}",
                name.replace(['.', '-'], "_")
            ));
        }
    }
    if manifest.files["seccomp.json"] != SECCOMP_SHA256 {
        return Err("kiri_install_seccomp_profile_unsupported".into());
    }
    Ok(Bundle {
        dir: dir.into(),
        manifest_sha256: host::sha256_bytes(&bytes),
        source_revision: manifest.source_revision,
        files: manifest.files,
        images: manifest.images,
    })
}

impl Bundle {
    pub fn path(&self, name: &str) -> PathBuf {
        self.dir.join(name)
    }
    pub fn hash(&self, name: &str) -> &str {
        self.files.get(name).map_or("", String::as_str)
    }
    pub fn cap(name: &str) -> u64 {
        FILES.iter().find(|f| f.0 == name).map_or(0, |f| f.1)
    }
    /// The executing installer must be the exact bundled binary.
    pub fn require_running(&self, executable: &Path) -> Result<()> {
        let actual = host::sha256_file(executable, BINARY_CAP)?;
        if actual != self.hash("moosuri") {
            return Err("kiri_install_running_binary_not_bundled".into());
        }
        Ok(())
    }
    /// Copy one validated file to a new root-owned destination, re-verifying the stream.
    pub fn install_file(
        &self,
        layout: &Layout,
        name: &str,
        destination: &Path,
        mode: u32,
    ) -> Result<()> {
        let (mut file, _) = host::open_regular(&self.path(name), Self::cap(name))?;
        layout.write_stream(
            destination,
            &mut file,
            Self::cap(name),
            self.hash(name),
            mode,
            &[],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::os::unix::fs::PermissionsExt;

    pub fn fixture(dir: &Path) -> serde_json::Value {
        let mut files = serde_json::Map::new();
        for (name, _) in FILES {
            let body: &[u8] = if name == "seccomp.json" {
                // A different byte string is tested separately; this is the digest fixture.
                b"{}"
            } else {
                name.as_bytes()
            };
            fs::write(dir.join(name), body).unwrap();
            fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o600)).unwrap();
            files.insert(name.into(), json!(host::sha256_bytes(body)));
        }
        json!({
            "format": 1,
            "source_revision": "a".repeat(40),
            "files": files,
            "images": {
                "member": format!("sha256:{}", "1".repeat(64)),
                "browser": format!("sha256:{}", "2".repeat(64)),
                "google_auth": format!("sha256:{}", "3".repeat(64)),
            }
        })
    }
    fn write(dir: &Path, manifest: &serde_json::Value) {
        fs::write(dir.join("manifest.json"), manifest.to_string()).unwrap();
        fs::set_permissions(dir.join("manifest.json"), fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(dir, fs::Permissions::from_mode(0o700)).unwrap();
    }
    #[test]
    fn rejects_unsupported_profile_and_every_tamper() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(tmp.path()).unwrap();
        let layout = Layout::under(&dir);
        let manifest = fixture(&dir);
        write(&dir, &manifest);
        // Synthetic seccomp bytes are not the accepted Kiri profile.
        assert_eq!(
            validate(&layout, &dir).unwrap_err(),
            "kiri_install_seccomp_profile_unsupported"
        );
        let mut bad = manifest.clone();
        bad["images"]["browser"] = bad["images"]["google_auth"].clone();
        write(&dir, &bad);
        assert_eq!(
            validate(&layout, &dir).unwrap_err(),
            "kiri_install_manifest_invalid"
        );
        let mut extra = manifest.clone();
        extra["files"]["hook.sh"] = json!("0".repeat(64));
        write(&dir, &extra);
        assert_eq!(
            validate(&layout, &dir).unwrap_err(),
            "kiri_install_manifest_invalid"
        );
        write(&dir, &manifest);
        fs::write(dir.join("member.tar"), b"changed").unwrap();
        assert!(
            validate(&layout, &dir)
                .unwrap_err()
                .contains("hash_mismatch_member_tar")
        );
    }
    #[test]
    fn rejects_symlink_hardlink_and_shared_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = fs::canonicalize(tmp.path()).unwrap();
        let layout = Layout::under(&dir);
        let manifest = fixture(&dir);
        write(&dir, &manifest);
        fs::remove_file(dir.join("browser.tar")).unwrap();
        fs::write(dir.join("elsewhere"), b"browser.tar").unwrap();
        std::os::unix::fs::symlink(dir.join("elsewhere"), dir.join("browser.tar")).unwrap();
        assert!(validate(&layout, &dir).is_err());
        fs::remove_file(dir.join("browser.tar")).unwrap();
        fs::hard_link(dir.join("elsewhere"), dir.join("browser.tar")).unwrap();
        assert!(validate(&layout, &dir).is_err());
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777)).unwrap();
        assert_eq!(
            validate(&layout, &dir).unwrap_err(),
            "kiri_install_bundle_directory_untrusted"
        );
    }
}
