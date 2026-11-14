use super::*;
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::os::unix::fs::PermissionsExt;
fn private(root: &Path, name: &str) -> PathBuf {
    let p = root.join(name);
    new_dir(&p).unwrap();
    p
}
fn fixture(root: &Path) -> (PathBuf, PathBuf, Ed25519KeyPair) {
    let b = private(root, "bundle");
    let k = Ed25519KeyPair::from_seed_unchecked(&[17; 32]).unwrap();
    let pin = root.join("pin");
    create(&pin, key_hex(k.public_key().as_ref()).as_bytes(), false).unwrap();
    let bytes = b"synthetic executable fixture";
    create(&b.join("moosuri"), bytes, true).unwrap();
    let p = own_probe();
    let m = Manifest {
        format: 1,
        product: "moosuri".into(),
        version: p.version,
        target: p.target,
        sequence: 1,
        store_schema: p.store_schema,
        embedded_web: true,
        binary: "moosuri".into(),
        size: bytes.len() as u64,
        sha256: hash(bytes),
    };
    sign(&b, &m, &k);
    (b, pin, k)
}
fn sign(root: &Path, m: &Manifest, key: &Ed25519KeyPair) {
    let bytes = json(m).unwrap();
    for name in ["manifest.json", "manifest.ed25519"] {
        let _ = fs::remove_file(root.join(name));
    }
    create(&root.join("manifest.json"), &bytes, false).unwrap();
    create(
        &root.join("manifest.ed25519"),
        key.sign(&bytes).as_ref(),
        false,
    )
    .unwrap();
}
#[test]
fn installation_bundle_rejects_signature_target_hash_paths_and_symlink_sources() {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (root, pin, key) = fixture(t.path());
    let verified = bundle(&root, &pin).unwrap();
    let prefix = private(t.path(), "prefix");
    new_dir(&prefix.join("releases")).unwrap();
    fs::write(root.join("moosuri"), b"tampered fixture").unwrap();
    assert_eq!(
        stage(&prefix, &verified).unwrap_err(),
        "installation_binary_hash_mismatch"
    );
    let mut m = verified.manifest.clone();
    m.binary = "../outside".into();
    sign(&root, &m, &key);
    assert!(bundle(&root, &pin).is_err());
    m.binary = "moosuri".into();
    m.target = "wrong-target".into();
    sign(&root, &m, &key);
    assert_eq!(
        bundle(&root, &pin).err().unwrap(),
        "installation_target_mismatch"
    );
    let bad = Ed25519KeyPair::from_seed_unchecked(&[18; 32]).unwrap();
    m.target = own_probe().target;
    sign(&root, &m, &bad);
    assert_eq!(
        bundle(&root, &pin).err().unwrap(),
        "installation_signature_invalid"
    );
    let link = t.path().join("linked-bundle");
    std::os::unix::fs::symlink(&root, &link).unwrap();
    assert!(bundle(&link, &pin).is_err());
    assert!(path_checked(&t.path().join("line\nbreak")).is_err());
}
#[test]
fn installation_activation_journal_records_old_new_before_atomic_publish_and_refuses_ordinary_start()
 {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let old = Release {
        sequence: 1,
        version: "0.1.0-dev.1".into(),
        manifest_sha256: "a".repeat(64),
    };
    let new = Release {
        sequence: 2,
        ..old.clone()
    };
    create(&t.path().join("current.json"), &json(&old).unwrap(), false).unwrap();
    let p = Activation {
        from: old.clone(),
        to: new.clone(),
        backup_output: t.path().join("backup.age"),
        export_id: uuid::Uuid::new_v4().to_string(),
    };
    create(&t.path().join("pending.json"), &json(&p).unwrap(), false).unwrap();
    sync(t.path()).unwrap();
    assert!(no_pending(t.path()).is_err());
    assert!(current(t.path()).unwrap() == old);
    atomic(&t.path().join("current.json"), &json(&new).unwrap()).unwrap();
    assert!(no_pending(t.path()).is_err());
    assert!(current(t.path()).unwrap() == new);
    assert!(pending(t.path()).unwrap().unwrap().from == old);
}

#[cfg(target_os = "macos")]
#[test]
fn installation_explicit_and_inherited_acl_grants_cannot_change_private_trust_or_runtime() {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let dir = private(t.path(), "acl-prefix");
    let grant = "everyone allow read,write,search,add_file,add_subdirectory,delete_child,file_inherit,directory_inherit";
    assert!(
        Process::new("/bin/chmod")
            .args(["+a", grant])
            .arg(&dir)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::metadata(&dir).unwrap().mode() & 0o777, 0o700);
    assert!(private_dir(&dir).is_err());
    assert!(new_dir(&dir.join("runtime")).is_err());
    assert!(!dir.join("runtime").exists());
    let pin = t.path().join("public-pin");
    create(&pin, b"synthetic-public-pin", false).unwrap();
    assert!(
        Process::new("/bin/chmod")
            .args(["+a", "everyone allow read,write"])
            .arg(&pin)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(fs::metadata(&pin).unwrap().mode() & 0o777, 0o600);
    assert!(read(&pin, 128, true).is_err());
}
#[test]
fn installation_network_templates_match_single_ip_parser_and_direct_tailnet_topology() {
    let p = Path::new("/home/operator/private/runtime");
    let mut i = Installation {
        format: 1,
        id: uuid::Uuid::new_v4().to_string(),
        data_dir: "/home/operator/private/data".into(),
        origin: "https://assistant.example.test".into(),
        network: Network::Https,
        trust_key: "00".repeat(32),
        managed: BTreeMap::new(),
    };
    let https = templates(p, &i).unwrap();
    assert!(https["deployment/moosuri.service"].contains("--trusted-proxy 127.0.0.1"));
    assert!(
        https["deployment/proxy.nginx.conf"].contains("proxy_set_header X-Real-IP $remote_addr;")
    );
    i.network = Network::Tailscale;
    let tail = templates(p, &i).unwrap();
    assert!(!tail["deployment/moosuri.service"].contains("--trusted-proxy"));
    assert!(!tail["deployment/moosuri.service"].contains("--client-ip-header"));
    assert!(
        tail["deployment/tailscale-serve.txt"]
            .contains("tailscale serve --bg http://127.0.0.1:3210")
    );
    assert!(tail["deployment/tailscale-serve.txt"].contains("possibly chained X-Forwarded-For"));
}
