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
        provenance: None,
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
fn initial_publication_never_replaces_an_existing_directory_even_when_empty() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let staged = private(root.path(), "staged");
    create(&staged.join("marker"), b"complete installation", false).unwrap();
    let destination = private(root.path(), "unrelated");
    let inode = fs::metadata(&destination).unwrap().ino();
    assert!(publish_directory(&staged, &destination).is_err());
    assert_eq!(fs::metadata(&destination).unwrap().ino(), inode);
    assert!(!destination.join("marker").exists());
    let absent = root.path().join("runtime");
    publish_directory(&staged, &absent).unwrap();
    assert_eq!(
        fs::read(absent.join("marker")).unwrap(),
        b"complete installation"
    );
    assert!(!staged.exists());
}
fn provenance_fixture(root: &Path, m: &mut Manifest, key: &Ed25519KeyPair) {
    use release_evidence as e;
    let i=serde_json::to_vec(&serde_json::json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"component":{"name":"moosuri","version":m.version},"properties":e::LOCKS.iter().map(|name|serde_json::json!({"name":format!("moosuri:lock-sha256:{name}"),"value":"ab".repeat(32)})).collect::<Vec<_>>()},"components":[]})).unwrap();
    let p=serde_json::to_vec(&serde_json::json!({"format":1,"purpose":"moosuri-offline-binary-provenance-v1","claim":e::CLAIM,"artifact":e::artifact(&serde_json::to_value(&m).unwrap()),"source":{"revision":"cd".repeat(20),"dirty":true},"compiler":{"rustc_verbose":"rustc 1.89.0\nhost: synthetic-host\nrelease: 1.89.0\n"},"inventory":e::reference(&i,e::INVENTORY)})).unwrap();
    create(&root.join(e::INVENTORY), &i, false).unwrap();
    create(&root.join(e::PROVENANCE), &p, false).unwrap();
    m.provenance = Some(e::reference(&p, e::PROVENANCE));
    sign(root, m, key);
}
#[test]
fn release_provenance_signed_bytes_fail_before_install_and_legacy_remains_compatible() {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (root, pin, key) = fixture(t.path());
    let mut m = bundle(&root, &pin).unwrap().manifest;
    assert!(m.provenance.is_none());
    let prefix = t.path().join("runtime");
    let install = Command::Install {
        bundle: root.clone(),
        trust_key: pin.clone(),
        prefix: prefix.clone(),
        data_dir: t.path().join("data"),
        origin: "https://fixture.example.test".into(),
        network: Network::Https,
    };
    let mut explicit_null = serde_json::to_value(&m).unwrap();
    explicit_null["provenance"] = serde_json::Value::Null;
    let null_bytes = json(&explicit_null).unwrap();
    fs::write(root.join("manifest.json"), &null_bytes).unwrap();
    fs::write(
        root.join("manifest.ed25519"),
        key.sign(&null_bytes).as_ref(),
    )
    .unwrap();
    assert_eq!(
        bundle(&root, &pin).err().unwrap(),
        "installation_invalid_metadata"
    );
    assert!(run(&install).is_err());
    assert!(!prefix.exists());
    sign(&root, &m, &key);
    assert!(
        bundle(&root, &pin).is_ok(),
        "absent provenance remains legacy"
    );
    provenance_fixture(&root, &mut m, &key);
    assert!(bundle(&root, &pin).is_ok());
    let original = read(
        &root.join(release_evidence::INVENTORY),
        release_evidence::INVENTORY_CAP,
        true,
    )
    .unwrap();
    fs::write(root.join(release_evidence::INVENTORY), b"changed").unwrap();
    assert!(run(&install).is_err());
    assert!(!prefix.exists());
    fs::write(root.join(release_evidence::INVENTORY), original).unwrap();
    m.provenance.as_mut().unwrap()["file"] = serde_json::json!("../outside");
    sign(&root, &m, &key);
    assert!(bundle(&root, &pin).is_err());
    m.provenance = None;
    sign(&root, &m, &key);
    assert_eq!(
        bundle(&root, &pin).err().unwrap(),
        "installation_unbound_provenance"
    );
    fs::remove_file(root.join(release_evidence::INVENTORY)).unwrap();
    fs::remove_file(root.join(release_evidence::PROVENANCE)).unwrap();
    assert!(bundle(&root, &pin).is_ok());
}
#[test]
fn release_provenance_snapshots_reverify_refresh_manager_and_uninstall_retains_no_evidence() {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (root, pin, key) = fixture(t.path());
    let mut m = bundle(&root, &pin).unwrap().manifest;
    let p = Probe {
        embedded_web: true,
        ..own_probe()
    };
    let script = format!(
        "#!/bin/sh\ncat <<'MOOSURI_SYNTHETIC_PROBE'\n{}\nMOOSURI_SYNTHETIC_PROBE\n",
        serde_json::to_string(&p).unwrap()
    );
    fs::write(root.join("moosuri"), script.as_bytes()).unwrap();
    m.size = script.len() as u64;
    m.sha256 = hash(script.as_bytes());
    provenance_fixture(&root, &mut m, &key);
    let captured = bundle(&root, &pin).unwrap();
    let prefix = private(t.path(), "runtime");
    new_dir(&prefix.join("releases")).unwrap();
    // Original paths can change after admission; only the validated snapshot stages.
    fs::write(root.join("moosuri"), b"substitution").unwrap();
    fs::write(root.join(release_evidence::PROVENANCE), b"substitution").unwrap();
    let r = stage(&prefix, &captured).unwrap();
    let i = Installation {
        format: 1,
        id: uuid::Uuid::new_v4().to_string(),
        data_dir: t.path().join("data"),
        origin: "https://fixture.example.test".into(),
        network: Network::Https,
        trust_key: key_hex(key.public_key().as_ref()),
        managed: BTreeMap::new(),
    };
    create(&prefix.join("installation.json"), &json(&i).unwrap(), false).unwrap();
    create(&prefix.join("current.json"), &json(&r).unwrap(), false).unwrap();
    let a = Activation {
        from: r.clone(),
        to: r.clone(),
        backup_output: t.path().join("unused.age"),
        export_id: uuid::Uuid::new_v4().to_string(),
        source_schema: Some(m.store_schema),
        target_schema: Some(m.store_schema),
        migration_started: false,
    };
    create(&prefix.join("pending.json"), &json(&a).unwrap(), false).unwrap();
    finish_activation(&prefix, &a).unwrap();
    assert_eq!(
        read(&prefix.join("manager"), BINARY_CAP, true).unwrap(),
        script.as_bytes()
    );
    let slot = prefix.join("releases/1");
    assert!(verify_release(&prefix, &i, &r).is_ok());
    let original = read(
        &slot.join(release_evidence::INVENTORY),
        release_evidence::INVENTORY_CAP,
        true,
    )
    .unwrap();
    fs::write(slot.join(release_evidence::INVENTORY), b"changed").unwrap();
    assert!(verify_release(&prefix, &i, &r).is_err());
    fs::write(slot.join(release_evidence::INVENTORY), original).unwrap();
    let updated = installation(&prefix).unwrap();
    uninstall(&prefix, &updated).unwrap();
    assert!(!slot.exists());
}
#[test]
fn installation_bundle_rejects_signature_target_hash_paths_and_symlink_sources() {
    let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    let (root, pin, key) = fixture(t.path());
    let verified = bundle(&root, &pin).unwrap();
    fs::write(root.join("moosuri"), b"tampered fixture").unwrap();
    assert_eq!(
        bundle(&root, &pin).err().unwrap(),
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
        source_schema: None,
        target_schema: None,
        migration_started: false,
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
