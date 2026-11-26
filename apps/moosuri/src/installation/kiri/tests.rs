//! Failure-boundary tests with temporary roots and synthetic input only.
use super::*;
use std::{collections::BTreeMap, os::unix::fs::PermissionsExt};

fn dirs(root: &Path, list: &[&str]) {
    for d in list {
        let p = root.join(d.trim_start_matches('/'));
        fs::create_dir_all(&p).unwrap();
        let mut cur = p.as_path();
        while cur != root {
            fs::set_permissions(cur, fs::Permissions::from_mode(0o755)).unwrap();
            cur = cur.parent().unwrap();
        }
    }
    fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
}
fn temp() -> (tempfile::TempDir, Layout) {
    let tmp = tempfile::tempdir().unwrap();
    let root = fs::canonicalize(tmp.path()).unwrap();
    dirs(
        &root,
        &[
            "/etc",
            "/usr/local/bin",
            "/etc/systemd/system",
            "/usr/lib/systemd/system",
            "/var/lib",
            "/srv",
        ],
    );
    let layout = Layout::under(&root);
    (tmp, layout)
}
fn bundle(root: &Path) -> bundle::Bundle {
    let dir = root.join("bundle");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let mut files = BTreeMap::new();
    for (name, _) in bundle::FILES {
        fs::write(dir.join(name), name.as_bytes()).unwrap();
        fs::set_permissions(dir.join(name), fs::Permissions::from_mode(0o600)).unwrap();
        files.insert(name.to_owned(), host::sha256_bytes(name.as_bytes()));
    }
    bundle::Bundle {
        dir,
        manifest_sha256: "f".repeat(64),
        source_revision: "a".repeat(40),
        files,
        images: bundle::Images {
            member: format!("sha256:{}", "1".repeat(64)),
            browser: format!("sha256:{}", "2".repeat(64)),
            google_auth: format!("sha256:{}", "3".repeat(64)),
        },
    }
}
fn receipt() -> Receipt {
    let plan = |p: &str, n: u8| browser::NetPlan {
        subnet: format!("10.{n}.17.0/24"),
        proxy_ip: format!("10.{n}.17.2"),
        project: p.into(),
    };
    Receipt {
        format: 1,
        installation_id: Uuid::from_u128(0x1111_1111_1111_4111_8111_1111_1111_1111),
        request_id: Uuid::from_u128(0x2222_2222_2222_4222_8222_2222_2222_2222),
        admin_email: "Admin@Example.com".into(),
        admin_name: "Admin".into(),
        public_origin: "https://assistant.example.com".into(),
        vault_origin: "https://vault.example.com".into(),
        source_revision: "a".repeat(40),
        manifest_sha256: "f".repeat(64),
        member_capacity: 2,
        member_volume_bytes: 10 << 30,
        uid_pool: vec![20000, 20001],
        tls: "certbot".into(),
        proxy_gid: 33,
        browser: plan("moosuri-browser-x", 100),
        google: plan("moosuri-google-x", 101),
        complete: false,
    }
}

#[test]
fn first_admin_has_a_distinct_replayable_registry_operation() {
    use moosuri_core::{AccountSummary, Role, SessionView};
    use moosuri_store::runtime_registry::{self, admin};
    let tmp = tempfile::tempdir().unwrap();
    fs::set_permissions(tmp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let r = receipt();
    let restored: Receipt = serde_json::from_slice(&serde_json::to_vec(&r).unwrap()).unwrap();
    assert_eq!(r.admin_request(), restored.admin_request());
    assert_ne!(r.admin_request(), r.request_id);
    assert_eq!(
        r.admin_request().to_string(),
        "d7c6651a-ee72-48ac-b436-2ec8871c7fc1"
    );
    let a = runtime_registry::reserve(tmp.path(), r.request_id, r.admin_email.to_ascii_lowercase())
        .unwrap();
    runtime_registry::set_routing(tmp.path(), &a, true).unwrap();
    let view = SessionView {
        user: AccountSummary {
            id: a.account_id.clone(),
            name: r.admin_name.clone(),
            email: r.admin_email.to_ascii_lowercase(),
            role: Role::Member,
            must_change_password: false,
        },
        password_change_required: false,
        chat_draft_scope: None,
    };
    assert!(matches!(
        admin::bootstrap(tmp.path(), r.request_id, &a, &view),
        Err(moosuri_store::StoreError::Conflict)
    ));
    let first = admin::bootstrap(tmp.path(), r.admin_request(), &a, &view).unwrap();
    assert_eq!(first.role, "admin");
    let again = admin::bootstrap(tmp.path(), restored.admin_request(), &a, &view).unwrap();
    assert_eq!(first.role_revision, again.role_revision);
    assert_eq!(
        runtime_registry::reserve(tmp.path(), r.request_id, r.admin_email.to_ascii_lowercase())
            .unwrap(),
        a
    );
}

#[test]
fn unsupported_hosts_and_uid_or_group_inputs_are_clear() {
    assert!(parse_os_release("ID=ubuntu\nVERSION_ID=\"24.04\"\n").is_ok());
    for bad in [
        "ID=ubuntu\nVERSION_ID=\"22.04\"\n",
        "ID=debian\nVERSION_ID=\"12\"\n",
        "",
    ] {
        assert_eq!(
            parse_os_release(bad).unwrap_err(),
            "kiri_install_requires_ubuntu_24_04"
        );
    }
    let groups = "root:x:0:\nwww-data:x:33:\nnginx:x:0:\n";
    assert_eq!(
        nginx_group("user www-data;\nevents {}", groups).unwrap(),
        33
    );
    assert!(nginx_group("user nginx;", groups).is_err());
    assert!(nginx_group("events {}", groups).is_err());
    assert_eq!(
        pick_uids(3, |u| u != 20001).unwrap(),
        vec![20000, 20002, 20003]
    );
    assert!(pick_uids(2, |_| false).is_err());
}

#[test]
fn fresh_install_refuses_every_foreign_or_existing_path() {
    let (_tmp, layout) = temp();
    require_fresh(&layout).unwrap();
    // An empty directory is not foreign data; a populated one is.
    fs::create_dir(layout.at("/etc/moosuri")).unwrap();
    require_fresh(&layout).unwrap();
    fs::write(layout.at("/etc/moosuri/other"), b"x").unwrap();
    assert!(
        require_fresh(&layout)
            .unwrap_err()
            .starts_with("kiri_install_conflicting_path:")
    );
    fs::remove_file(layout.at("/etc/moosuri/other")).unwrap();
    for path in [
        BIN,
        "/etc/nginx-placeholder",
        render::NGINX_CONF,
        "/var/lib/moosuri-vault",
    ] {
        if path == "/etc/nginx-placeholder" {
            continue;
        }
        let target = layout.at(path);
        fs::create_dir_all(target.parent().unwrap()).unwrap();
        fs::write(&target, b"foreign").unwrap();
        assert!(require_fresh(&layout).is_err(), "{path}");
        fs::remove_file(&target).unwrap();
    }
    let unit = layout.at("/etc/systemd/system/moosuri-kiri-gateway.service");
    std::os::unix::fs::symlink("/dev/null", &unit).unwrap();
    assert!(require_fresh(&layout).is_err());
}

#[test]
fn lay_down_is_resumable_and_never_replaces_changed_files_or_secrets() {
    let (tmp, layout) = temp();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let b = bundle(&root);
    let r = receipt();
    lay_down(&layout, &b, &r, Some((b"{}\n", b"{}\n"))).unwrap();
    let key = fs::read(layout.at(KEY)).unwrap();
    assert_eq!(key.len(), 65);
    assert_eq!(fs::metadata(layout.at(KEY)).unwrap().mode() & 0o777, 0o400);
    let gateway =
        fs::read_to_string(layout.at("/etc/systemd/system/moosuri-kiri-gateway.service")).unwrap();
    assert!(gateway.contains("--vault-all-members") && gateway.contains("--proxy-gid 33"));
    assert!(!gateway.contains(std::str::from_utf8(&key[..64]).unwrap()));
    let lifecycle = fs::read_to_string(layout.at(MEMBERS)).unwrap();
    for field in [
        "vault_origin",
        "public_origin",
        "google_sockets_root",
        "browser_sockets_root",
    ] {
        assert!(lifecycle.contains(field), "{field}");
    }
    // Same invocation: identical result, same random key, no write.
    lay_down(&layout, &b, &r, Some((b"{}\n", b"{}\n"))).unwrap();
    assert_eq!(fs::read(layout.at(KEY)).unwrap(), key);
    // A modified managed file or binary is foreign and is not overwritten.
    let members = layout.at(MEMBERS);
    fs::write(&members, b"changed").unwrap();
    assert_eq!(
        lay_down(&layout, &b, &r, Some((b"{}\n", b"{}\n"))).unwrap_err(),
        "kiri_install_foreign_or_changed_file"
    );
    assert_eq!(fs::read(&members).unwrap(), b"changed");
    // A conflicting existing key or tampered bundle file stops before copying.
    fs::set_permissions(layout.at(KEY), fs::Permissions::from_mode(0o600)).unwrap();
    assert!(ensure_adapter_key(&layout).is_err());
}

#[test]
fn changed_bundle_file_is_not_installed() {
    let (tmp, layout) = temp();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let b = bundle(&root);
    fs::write(b.path("moosuri"), b"swapped").unwrap();
    assert_eq!(
        lay_down(&layout, &b, &receipt(), None).unwrap_err(),
        "kiri_install_source_hash_mismatch"
    );
    assert!(!layout.at(BIN).exists());
}

#[test]
fn receipt_identity_blocks_changed_origins_bundle_or_capacity() {
    let (_tmp, layout) = temp();
    layout
        .ensure_dir(&layout.at("/etc/moosuri"), 0o700)
        .unwrap();
    let r = receipt();
    save_receipt(&layout, &r, None).unwrap();
    let (loaded, bytes) = load_receipt(&layout).unwrap().unwrap();
    assert!(loaded == r);
    assert!(
        !String::from_utf8_lossy(&bytes)
            .to_lowercase()
            .contains("password")
    );
    // Case differences in the same email are the same invocation.
    let mut same = r.clone();
    same.admin_email = "admin@example.com".into();
    assert!(same.inputs() == r.inputs());
    for change in [
        |x: &mut Receipt| x.vault_origin = "https://other.example.com".into(),
        |x: &mut Receipt| x.manifest_sha256 = "e".repeat(64),
        |x: &mut Receipt| x.member_capacity = 3,
        |x: &mut Receipt| x.tls = "supplied:/a|/b".into(),
    ] {
        let mut other = r.clone();
        change(&mut other);
        assert!(other.inputs() != r.inputs());
    }
    // Completion may update only the same receipt; a foreign replacement is refused.
    let mut done = r.clone();
    done.complete = true;
    save_receipt(&layout, &done, Some(&bytes)).unwrap();
    assert!(save_receipt(&layout, &r, Some(b"not the old receipt")).is_err());
}

#[test]
fn supplied_tls_must_be_a_private_canonical_pair() {
    let (tmp, layout) = temp();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let cert = root.join("cert.pem");
    let key = root.join("key.pem");
    fs::write(&cert, b"-----BEGIN CERTIFICATE-----\nAAAA\n").unwrap();
    fs::write(&key, b"-----BEGIN PRIVATE KEY-----\nAAAA\n").unwrap();
    fs::set_permissions(&cert, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o644)).unwrap();
    assert_eq!(
        validate_supplied(&layout, &cert, &key).unwrap_err(),
        "kiri_install_tls_file_invalid"
    );
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    validate_supplied(&layout, &cert, &key).unwrap();
    assert!(validate_supplied(&layout, &key, &cert).is_err());
    let link = root.join("link.pem");
    std::os::unix::fs::symlink(&cert, &link).unwrap();
    assert!(validate_supplied(&layout, &link, &key).is_err());
}

#[cfg(target_os = "linux")]
#[test]
fn https_certificate_must_cover_both_origins_and_not_be_expired() {
    let dir = tempfile::tempdir().unwrap();
    let exec = System {
        docker_config: dir.path().into(),
    };
    let cert = dir.path().join("cert.pem");
    let key = dir.path().join("key.pem");
    exec.run(
        "openssl",
        &args(&[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=assistant.example.com",
            "-addext",
            "subjectAltName=DNS:assistant.example.com,DNS:vault.example.com",
            "-keyout",
            key.to_str().unwrap(),
            "-out",
            cert.to_str().unwrap(),
        ]),
        Input::None,
        Duration::from_secs(15),
    )
    .unwrap()
    .ok("fixture_certificate")
    .unwrap();
    let public = "https://assistant.example.com";
    let vault = "https://vault.example.com";
    validate_certificate(&exec, &cert, public, vault).unwrap();
    assert!(validate_certificate(&exec, &cert, "https://wrong.example.com", vault).is_err());
    assert!(validate_certificate(&exec, &cert, public, "https://wrong.example.com").is_err());
    let expired = dir.path().join("expired.pem");
    exec.run(
        "openssl",
        &args(&[
            "x509",
            "-in",
            cert.to_str().unwrap(),
            "-signkey",
            key.to_str().unwrap(),
            "-days",
            "-1",
            "-out",
            expired.to_str().unwrap(),
        ]),
        Input::None,
        Duration::from_secs(10),
    )
    .unwrap()
    .ok("fixture_expired_certificate")
    .unwrap();
    assert!(validate_certificate(&exec, &expired, public, vault).is_err());
}

fn openssl(exec: &System, words: &[&str]) {
    exec.run(
        "openssl",
        &args(words),
        Input::None,
        Duration::from_secs(30),
    )
    .unwrap()
    .ok("fixture_openssl")
    .unwrap();
}
/// Self-signed pair for the given SAN names, private key mode 0600.
fn pair(exec: &System, dir: &Path, name: &str, sans: &str) -> (PathBuf, PathBuf) {
    let (cert, key) = (
        dir.join(format!("{name}.crt")),
        dir.join(format!("{name}.key")),
    );
    openssl(
        exec,
        &[
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-nodes",
            "-days",
            "2",
            "-subj",
            "/CN=fixture.example.com",
            "-addext",
            sans,
            "-keyout",
            key.to_str().unwrap(),
            "-out",
            cert.to_str().unwrap(),
        ],
    );
    for path in [&cert, &key] {
        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
    (cert, key)
}

#[cfg(target_os = "linux")]
#[test]
fn supplied_tls_material_is_fully_checked_with_real_openssl() {
    let (tmp, layout) = temp();
    let dir = fs::canonicalize(tmp.path()).unwrap();
    let exec = System {
        docker_config: dir.clone(),
    };
    let (public, vault) = ("https://assistant.example.com", "https://vault.example.com");
    let names = "subjectAltName=DNS:assistant.example.com,DNS:vault.example.com";
    let (cert, key) = pair(&exec, &dir, "good", names);
    validate_supplied(&layout, &cert, &key).unwrap();
    validate_tls_material(&exec, &cert, &key, public, vault).unwrap();

    // Certificate for other names.
    let (wrong, wrong_key) = pair(&exec, &dir, "wrong", "subjectAltName=DNS:other.example.com");
    assert!(validate_tls_material(&exec, &wrong, &wrong_key, public, vault).is_err());
    // Valid certificate with somebody else's private key.
    assert_eq!(
        validate_tls_material(&exec, &cert, &wrong_key, public, vault).unwrap_err(),
        "kiri_install_tls_key_mismatch"
    );
    // Expired.
    let expired = dir.join("expired.crt");
    openssl(
        &exec,
        &[
            "x509",
            "-in",
            cert.to_str().unwrap(),
            "-signkey",
            key.to_str().unwrap(),
            "-days",
            "-1",
            "-out",
            expired.to_str().unwrap(),
        ],
    );
    assert!(validate_tls_material(&exec, &expired, &key, public, vault).is_err());
    // Passphrase-protected key: refused textually and by openssl without a prompt.
    let encrypted = dir.join("encrypted.key");
    openssl(
        &exec,
        &[
            "pkey",
            "-in",
            key.to_str().unwrap(),
            "-aes256",
            "-passout",
            "pass:synthetic",
            "-out",
            encrypted.to_str().unwrap(),
        ],
    );
    fs::set_permissions(&encrypted, fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        validate_supplied(&layout, &cert, &encrypted).unwrap_err(),
        "kiri_install_tls_file_invalid"
    );
    assert!(validate_tls_material(&exec, &cert, &encrypted, public, vault).is_err());
}

#[test]
fn tls_paths_cannot_smuggle_nginx_tokens() {
    let (tmp, layout) = temp();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let key = root.join("key.pem");
    fs::write(&key, b"-----BEGIN PRIVATE KEY-----\nAAAA\n").unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    for bad in [
        "a#b.pem", "a b.pem", "a\tb.pem", "a\nb.pem", "a,b.pem", "a`b.pem", "a&b.pem",
    ] {
        let cert = root.join(bad);
        fs::write(&cert, b"-----BEGIN CERTIFICATE-----\nAAAA\n").unwrap();
        assert_eq!(
            validate_supplied(&layout, &cert, &key).unwrap_err(),
            "kiri_install_tls_path_invalid",
            "{bad:?}"
        );
    }
}

#[test]
fn bad_supplied_tls_fails_before_inventory_receipt_or_resources() {
    let (tmp, layout) = temp();
    let root = fs::canonicalize(tmp.path()).unwrap();
    let (cert, key) = (root.join("c.pem"), root.join("k.pem"));
    fs::write(&cert, b"-----BEGIN CERTIFICATE-----\nAAAA\n").unwrap();
    fs::write(&key, b"-----BEGIN PRIVATE KEY-----\nAAAA\n").unwrap();
    fs::set_permissions(&cert, fs::Permissions::from_mode(0o600)).unwrap();
    fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
    let options = Options {
        bundle: root.join("bundle"),
        public_origin: "https://assistant.example.com".into(),
        vault_origin: "https://vault.example.com".into(),
        admin_email: None,
        admin_name: None,
        member_capacity: 1,
        member_volume_gib: 1,
        password_stdin: false,
        tls_certificate: Some(cert),
        tls_key: Some(key),
        certbot_email: None,
        accept_certbot_terms: false,
    };
    let b = bundle(&root);
    let fake = host::fake::Fake::default();
    fake.reply(1, ""); // openssl verify rejects the certificate
    let docker = browser::Docker { exec: &fake };
    let err = plan_fresh(
        &layout,
        &fake,
        &docker,
        &options,
        &b,
        ("a@example.com".into(), "Admin".into()),
    )
    .err()
    .unwrap();
    assert_eq!(err, "kiri_install_certificate_expired_or_wrong_host");
    // Only the certificate check ran: no network/docker inventory, no receipt.
    assert!(
        fake.calls
            .borrow()
            .iter()
            .all(|c| c.starts_with("openssl "))
    );
    assert!(!layout.exists(&layout.at(RECEIPT)).unwrap());
    assert!(!layout.exists(&layout.at(render::ETC)).unwrap());
}

#[test]
fn nginx_never_adopts_a_name_served_elsewhere() {
    let (_tmp, layout) = temp();
    fs::create_dir_all(layout.at("/etc/nginx/conf.d")).unwrap();
    fs::write(layout.at("/etc/nginx/nginx.conf"), "user www-data;\n").unwrap();
    nginx_conflicts(&layout, &["assistant.example.com"]).unwrap();
    fs::write(
        layout.at("/etc/nginx/conf.d/other.conf"),
        "server { server_name www.example.com assistant.example.com; }\n",
    )
    .unwrap();
    assert_eq!(
        nginx_conflicts(&layout, &["assistant.example.com"]).unwrap_err(),
        "kiri_install_nginx_server_name_conflict"
    );
}

#[test]
fn identity_validation_bounds_prompts_and_arguments() {
    assert!(valid_email("a@example.com"));
    for bad in [
        "a",
        "@example.com",
        "a@b",
        "a b@example.com",
        "a@@example.com",
        " a@example.com",
    ] {
        assert!(!valid_email(bad), "{bad}");
    }
    assert!(valid_name("Kim Admin"));
    assert!(!valid_name("") && !valid_name("a\nb") && !valid_name(&"x".repeat(101)));
}
