//! Actual schema47→current CLI acceptance; provision the real supported-floor executable.
#![cfg(feature = "test-fixtures")]
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
    time::Duration,
};
const PASSWORD: &str = "Synthetic-Schema-Upgrade-Password-42";
fn cli(binary: &Path, args: &[&str], fault: Option<&str>) -> Output {
    let mut c = Command::new(binary);
    c.args(args).env_remove("MOOSURI_INSTALLATION_FAULT");
    if let Some(point) = fault {
        c.env("MOOSURI_INSTALLATION_FAULT", point);
    }
    c.output().unwrap()
}
#[track_caller]
fn ok(out: Output) -> Vec<u8> {
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    out.stdout
}
fn private(parent: &Path, name: &str) -> PathBuf {
    let p = parent.join(name);
    fs::create_dir(&p).unwrap();
    fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
    p
}
fn owner_file(path: &Path, bytes: &[u8]) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn metadata(binary: &Path) -> Value {
    serde_json::from_slice(&ok(cli(binary, &["installation", "probe"], None))).unwrap()
}
fn signed(parent: &Path, name: &str, binary: &Path, seq: u64, key: &Ed25519KeyPair) -> PathBuf {
    let b = private(parent, name);
    let copy = b.join("moosuri");
    fs::copy(binary, &copy).unwrap();
    fs::set_permissions(&copy, fs::Permissions::from_mode(0o700)).unwrap();
    ok(cli(
        binary,
        &[
            "installation",
            "manifest",
            "--binary",
            copy.to_str().unwrap(),
            "--sequence",
            &seq.to_string(),
            "--output",
            b.join("manifest.json").to_str().unwrap(),
        ],
        None,
    ));
    owner_file(
        &b.join("manifest.ed25519"),
        key.sign(&fs::read(b.join("manifest.json")).unwrap())
            .as_ref(),
    );
    b
}
fn record(prefix: &Path) -> Value {
    serde_json::from_slice(&fs::read(prefix.join("current.json")).unwrap()).unwrap()
}
fn digest(p: &Path) -> Vec<u8> {
    Sha256::digest(fs::read(p).unwrap()).to_vec()
}
struct Server(std::process::Child);
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
async fn predecessor_content(manager: &Path, prefix: &Path) -> String {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    let origin = format!("http://{addr}");
    let mut server = Server(
        Command::new(manager)
            .args([
                "installation",
                "run",
                "--prefix",
                prefix.to_str().unwrap(),
                "--",
                "serve",
                "--origin",
                &origin,
                "--bind",
                &addr.to_string(),
                "--dev",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    let client = reqwest::Client::new();
    tokio::time::timeout(Duration::from_secs(12), async {
        loop {
            if let Ok(r) = client.get(format!("{origin}/health/ready")).send().await
                && r.status().is_success()
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await
    .unwrap();
    let login = client
        .post(format!("{origin}/api/v1/auth/login"))
        .header("Origin", &origin)
        .json(&json!({"email":"upgrade@example.test", "password":PASSWORD}))
        .send()
        .await
        .unwrap();
    assert!(login.status().is_success());
    let cookie = login.headers()[reqwest::header::SET_COOKIE]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_owned();
    let user: Value = login.json().await.unwrap();
    let created = client
        .post(format!("{origin}/api/v1/threads"))
        .header("Origin", &origin)
        .header("Cookie", &cookie)
        .header("X-Moosuri-Account", user["user"]["id"].as_str().unwrap())
        .json(&json!({"title":"Synthetic predecessor user content"}))
        .send()
        .await
        .unwrap();
    assert_eq!(created.status(), 201);
    let id = created.json::<Value>().await.unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        unsafe { libc::kill(server.0.id() as i32, libc::SIGTERM) },
        0
    );
    assert!(server.0.wait().unwrap().success());
    id
}
#[tokio::test]
#[ignore = "requires actual schema47 executable via MOOSURI_TEST_PREDECESSOR_BINARY; do not count unrun as acceptance"]
async fn signed_multiversion_update_interruption_retry_preserves_content_and_never_rolls_database_back()
 {
    let new = Path::new(env!("CARGO_BIN_EXE_moosuri"));
    let old = PathBuf::from(
        std::env::var_os("MOOSURI_TEST_PREDECESSOR_BINARY")
            .expect("real predecessor executable required"),
    );
    let old_probe = metadata(&old);
    let new_probe = metadata(new);
    let source_schema = old_probe["store_schema"].as_i64().unwrap();
    let target_schema = new_probe["store_schema"].as_i64().unwrap();
    // This is actual old-binary evidence, never a current DB relabeled by PRAGMA.
    assert_eq!(source_schema, 47);
    assert_eq!(
        source_schema,
        moosuri_store::backup::MIN_SUPPORTED_UPDATE_SCHEMA
    );
    assert!(
        target_schema >= 49,
        "target must span multiple real schema migrations"
    );
    assert!(target_schema - source_schema >= 2);
    assert!(moosuri_store::backup::supports_update_from(source_schema));
    assert_eq!(old_probe["target"], new_probe["target"]);
    for point in [
        "after_migration_started",
        "after_migration",
        "after_manager",
        "after_current",
    ] {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let key = Ed25519KeyPair::from_seed_unchecked(&[93; 32]).unwrap();
        let pin = root.path().join("pin");
        owner_file(
            &pin,
            key.public_key()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                .as_bytes(),
        );
        let first = signed(root.path(), "old-bundle", &old, 1, &key);
        let next = signed(root.path(), "new-bundle", new, 2, &key);
        // Cargo outputs can be hard-linked into deps; the operator runs the
        // owner-private single-link executable from the verified bundle.
        let updater = next.join("moosuri");
        let prefix = root.path().join("runtime");
        let data = root.path().join("data");
        ok(cli(
            &old,
            &[
                "installation",
                "install",
                "--bundle",
                first.to_str().unwrap(),
                "--trust-key",
                pin.to_str().unwrap(),
                "--prefix",
                prefix.to_str().unwrap(),
                "--data-dir",
                data.to_str().unwrap(),
                "--origin",
                "https://upgrade.example.test",
                "--network",
                "https",
            ],
            None,
        ));
        let manager = prefix.join("manager");
        let password = root.path().join("password");
        owner_file(&password, PASSWORD.as_bytes());
        ok(cli(
            &manager,
            &[
                "installation",
                "run",
                "--prefix",
                prefix.to_str().unwrap(),
                "--",
                "init",
                "--name",
                "Synthetic upgrade",
                "--email",
                "upgrade@example.test",
                "--password-file",
                password.to_str().unwrap(),
            ],
            None,
        ));
        let thread = predecessor_content(&manager, &prefix).await;
        let exports = private(root.path(), "exports");
        let identity = exports.join("identity");
        let recipient = String::from_utf8(ok(cli(
            &old,
            &["backup", "keygen", "--output", identity.to_str().unwrap()],
            None,
        )))
        .unwrap();
        ok(cli(
            &old,
            &[
                "--data-dir",
                data.to_str().unwrap(),
                "backup",
                "setup",
                "--recipient",
                recipient.trim(),
                "--identity-file",
                identity.to_str().unwrap(),
            ],
            None,
        ));
        let before = record(&prefix);
        let backup = exports.join("before.age");
        let args = [
            "installation",
            "update",
            "--bundle",
            next.to_str().unwrap(),
            "--trust-key",
            pin.to_str().unwrap(),
            "--prefix",
            prefix.to_str().unwrap(),
            "--backup-output",
            backup.to_str().unwrap(),
        ];
        if point == "after_migration_started" {
            let manifest_path = next.join("manifest.json");
            let original = fs::read(&manifest_path).unwrap();
            let source: Value = serde_json::from_slice(&original).unwrap();
            for (field, value, code) in [
                (
                    "target",
                    json!("wrong-target"),
                    "installation_target_mismatch",
                ),
                (
                    "store_schema",
                    json!(new_probe["store_schema"].as_i64().unwrap() + 1),
                    "installation_run_signed_target_binary",
                ),
            ] {
                let mut m = source.clone();
                m[field] = value;
                let bytes = serde_json::to_vec(&m).unwrap();
                owner_file(&manifest_path, &bytes);
                owner_file(&next.join("manifest.ed25519"), key.sign(&bytes).as_ref());
                let refused = cli(&updater, &args, None);
                assert!(!refused.status.success());
                assert!(String::from_utf8_lossy(&refused.stderr).contains(code));
                assert_eq!(record(&prefix), before);
                assert!(!backup.exists());
            }
            owner_file(&manifest_path, &original);
            let foreign_key = Ed25519KeyPair::from_seed_unchecked(&[94; 32]).unwrap();
            owner_file(
                &next.join("manifest.ed25519"),
                foreign_key.sign(&original).as_ref(),
            );
            let refused = cli(&updater, &args, None);
            assert!(!refused.status.success());
            assert!(String::from_utf8_lossy(&refused.stderr).contains("signature_invalid"));
            assert_eq!(record(&prefix), before);
            assert!(!backup.exists());
            owner_file(&next.join("manifest.ed25519"), key.sign(&original).as_ref());
        }
        // Data writer admission rejects before export or migration.
        let lock = fs::File::open(data.join("writer.lock")).unwrap();
        fs2::FileExt::try_lock_exclusive(&lock).unwrap();
        let busy = cli(&updater, &args, None);
        assert!(!busy.status.success());
        assert!(!backup.exists());
        drop(lock);
        let interrupted = cli(&updater, &args, Some(point));
        assert!(!interrupted.status.success(), "missing fault {point}");
        assert!(
            String::from_utf8_lossy(&interrupted.stderr).contains(point),
            "expected {point}, got {}",
            String::from_utf8_lossy(&interrupted.stderr)
        );
        let saved_export = digest(&backup);
        let pending: Value =
            serde_json::from_slice(&fs::read(prefix.join("pending.json")).unwrap()).unwrap();
        assert_eq!(pending["source_schema"], old_probe["store_schema"]);
        assert_eq!(pending["target_schema"], new_probe["store_schema"]);
        assert_eq!(pending["migration_started"], true);
        let run = cli(
            &updater,
            &[
                "installation",
                "run",
                "--prefix",
                prefix.to_str().unwrap(),
                "--",
                "doctor",
            ],
            None,
        );
        assert!(!run.status.success());
        let refused = cli(
            &updater,
            &[
                "installation",
                "recover",
                "--prefix",
                prefix.to_str().unwrap(),
                "--choose",
                "old",
            ],
            None,
        );
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("recover_new_only"));
        // A retry must preserve work committed by the newer Store after migration;
        // this is deliberately beyond managed startup, which remains fenced.
        let committed = if point == "after_migration_started" {
            None
        } else {
            let store = moosuri_store::Store::open(&data).unwrap();
            let user = store
                .login("upgrade@example.test".into(), PASSWORD.into())
                .await
                .unwrap()
                .user;
            assert!(
                store
                    .list_threads(&user)
                    .await
                    .unwrap()
                    .iter()
                    .any(|t| t.id == thread)
            );
            let id = store
                .create_thread(&user, "Synthetic work after migration".into())
                .await
                .unwrap()
                .id;
            drop(store);
            Some(id)
        };
        // The old installed manager cannot recover a new pending record before
        // replacement; the retained signed target bundle is the recovery entry.
        if matches!(point, "after_migration_started" | "after_migration") {
            let unavailable = cli(
                &manager,
                &[
                    "installation",
                    "recover",
                    "--prefix",
                    prefix.to_str().unwrap(),
                    "--choose",
                    "new",
                ],
                None,
            );
            assert!(
                !unavailable.status.success(),
                "legacy manager must not claim recovery of unknown schema state"
            );
        }
        let recovery_binary = if matches!(point, "after_manager" | "after_current") {
            &manager
        } else {
            &updater
        };
        ok(cli(
            recovery_binary,
            &[
                "installation",
                "recover",
                "--prefix",
                prefix.to_str().unwrap(),
                "--choose",
                "new",
            ],
            None,
        ));
        assert!(!prefix.join("pending.json").exists());
        assert_eq!(record(&prefix)["sequence"], 2);
        assert_eq!(digest(&backup), saved_export);
        assert_eq!(digest(&manager), digest(&next.join("moosuri")));
        let store = moosuri_store::Store::open(&data).unwrap();
        let user = store
            .login("upgrade@example.test".into(), PASSWORD.into())
            .await
            .unwrap()
            .user;
        let threads = store.list_threads(&user).await.unwrap();
        assert!(threads.iter().any(|t| t.id == thread));
        if let Some(committed) = committed {
            assert!(threads.iter().any(|t| t.id == committed));
        }
        drop(store);
        ok(cli(
            &manager,
            &[
                "installation",
                "run",
                "--prefix",
                prefix.to_str().unwrap(),
                "--",
                "doctor",
            ],
            None,
        ));
        assert_ne!(before, record(&prefix));
    }
}
