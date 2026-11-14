//! Synthetic engine/protocol fixture only. It never invokes an OCI engine or Claude.
use super::Profile;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{os::unix::fs::PermissionsExt, path::Path};
pub struct Fixture {
    pub directory: tempfile::TempDir,
    pub profile: Profile,
    _socket: std::os::unix::net::UnixListener,
}
fn digest(path: &Path) -> String {
    Sha256::digest(std::fs::read(path).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
impl Fixture {
    pub fn new() -> Self {
        let directory = tempfile::Builder::new()
            .prefix("moosuri-claude-fixture-")
            .tempdir_in(std::env::temp_dir().canonicalize().unwrap())
            .unwrap();
        let root = directory.path();
        for name in ["auth", "scratch"] {
            let p = root.join(name);
            std::fs::create_dir(&p).unwrap();
            std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700)).unwrap();
        }
        let engine = root.join("engine");
        let script = include_str!("fixture_engine.py")
            .replace("FIXTURE_ROOT", &format!("{:?}", root.to_str().unwrap()));
        std::fs::write(&engine, script).unwrap();
        std::fs::set_permissions(&engine, std::fs::Permissions::from_mode(0o700)).unwrap();
        for name in ["official", "helper"] {
            std::fs::write(root.join(name), b"synthetic executable placeholder").unwrap();
            std::fs::set_permissions(root.join(name), std::fs::Permissions::from_mode(0o500))
                .unwrap();
        }
        let socket = root.join("engine.sock");
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let json=json!({"engine":engine,"engine_sha256":digest(&engine),"engine_socket":socket,"image":format!("example.invalid/synthetic@sha256:{}","a".repeat(64)),"official_cli":root.join("official"),"cli_sha256":digest(&root.join("official")),"cli_version":"2.1.322","helper":root.join("helper"),"helper_sha256":digest(&root.join("helper")),"auth_root":root.join("auth"),"scratch_root":root.join("scratch"),"isolation_acceptance_sha256":"b".repeat(64),"hosting_terms_accepted":true}).to_string();
        let profile = Profile::synthetic_from_json(&json).unwrap();
        Self {
            directory,
            profile,
            _socket: listener,
        }
    }
    pub fn behavior(&self, value: &str) {
        std::fs::write(self.directory.path().join("behavior"), value).unwrap();
    }
    pub fn launches(&self) -> usize {
        std::fs::read_to_string(self.directory.path().join("launches"))
            .unwrap_or_default()
            .lines()
            .count()
    }
}
impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
