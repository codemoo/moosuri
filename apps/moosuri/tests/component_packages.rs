//! Optional packages exercise the real CLI and normal TLS without executing artifacts.
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[path = "../../../tools/release/components.rs"]
mod contract;
#[path = "../../../tools/release/provenance.rs"]
mod release_evidence;
fn command(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_moosuri"))
        .args(args)
        .output()
        .unwrap()
}
fn ok(output: std::process::Output) -> Vec<u8> {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    output.stdout
}
fn private_file(path: &Path, bytes: impl AsRef<[u8]>) {
    fs::write(path, bytes).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
struct Mirror {
    child: std::process::Child,
    root: PathBuf,
    base: String,
    ca: PathBuf,
}
impl Drop for Mirror {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
impl Mirror {
    fn start(root: &Path, version: &str) -> Self {
        private_file(&root.join("cert.conf"), b"[req]\ndistinguished_name=dn\nx509_extensions=ext\nprompt=no\n[dn]\nCN=synthetic-mirror-ca\n[ext]\nsubjectAltName=IP:127.0.0.1,DNS:localhost\nbasicConstraints=critical,CA:TRUE\nkeyUsage=critical,keyCertSign,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n");
        let result = Command::new("openssl")
            .args([
                "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-days", "1", "-config",
            ])
            .arg(root.join("cert.conf"))
            .arg("-keyout")
            .arg(root.join("tls.key"))
            .arg("-out")
            .arg(root.join("tls.pem"))
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "fixture certificate generation failed"
        );
        private_file(&root.join("server.conf"), b"[req]\ndistinguished_name=dn\nprompt=no\n[dn]\nCN=localhost\n[ext]\nsubjectAltName=IP:127.0.0.1,DNS:localhost\nbasicConstraints=critical,CA:FALSE\nkeyUsage=critical,digitalSignature,keyEncipherment\nextendedKeyUsage=serverAuth\n");
        for args in [
            vec![
                "req",
                "-new",
                "-newkey",
                "rsa:2048",
                "-nodes",
                "-config",
                "server.conf",
                "-keyout",
                "server.key",
                "-out",
                "server.csr",
            ],
            vec![
                "x509",
                "-req",
                "-in",
                "server.csr",
                "-CA",
                "tls.pem",
                "-CAkey",
                "tls.key",
                "-CAcreateserial",
                "-days",
                "1",
                "-extfile",
                "server.conf",
                "-extensions",
                "ext",
                "-out",
                "server.pem",
            ],
        ] {
            assert!(
                Command::new("openssl")
                    .args(args)
                    .current_dir(root)
                    .output()
                    .unwrap()
                    .status
                    .success(),
                "fixture server certificate failed"
            );
        }
        let ca = root.join("tls.pem");
        fs::set_permissions(&ca, fs::Permissions::from_mode(0o600)).unwrap();
        fs::set_permissions(root.join("tls.key"), fs::Permissions::from_mode(0o600)).unwrap();
        private_file(&root.join("control.json"), b"{}");
        let script = r#"
import http.server, ssl, json, pathlib, sys
root=pathlib.Path(sys.argv[1])
class Handler(http.server.BaseHTTPRequestHandler):
 def log_message(self, *args): pass
 def do_GET(self):
  name=self.path.rsplit('/',1)[-1]
  with (root/'requests').open('a') as log: log.write(name+'\n')
  control=json.loads((root/'control.json').read_text())
  mode=control.get('mode') if name==control.get('file') else None
  path=root/'bundle'/name
  if mode=='missing' or not path.is_file():
   self.send_response(404); self.end_headers(); return
  if mode=='redirect':
   self.send_response(302); self.send_header('Location','/escaped/moosuri'); self.end_headers(); return
  data=path.read_bytes()
  if mode=='tampered': data=bytes([data[0]^1])+data[1:]
  self.send_response(200)
  self.send_header('Content-Length',str((16385 if name=='manifest.json' else len(data)+1) if mode=='oversized' else len(data)))
  self.end_headers()
  try:
   self.wfile.write(data[:len(data)//2] if mode=='partial' else data)
  except (BrokenPipeError, ConnectionResetError): pass
server=http.server.HTTPServer(('127.0.0.1',0),Handler)
context=ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
context.load_cert_chain(root/'server.pem',root/'server.key')
server.socket=context.wrap_socket(server.socket,server_side=True)
(root/'port').write_text(str(server.server_port))
server.serve_forever()
"#;
        let child = Command::new("python3")
            .args(["-c", script])
            .arg(root)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let start = Instant::now();
        let port = loop {
            if let Ok(port) = fs::read_to_string(root.join("port")) {
                break port;
            }
            assert!(
                start.elapsed() < Duration::from_secs(10),
                "TLS fixture did not start"
            );
            std::thread::sleep(Duration::from_millis(20));
        };
        Self {
            child,
            root: root.to_owned(),
            base: format!(
                "https://127.0.0.1:{port}/v{version}/credential-broker/x86_64-unknown-linux-gnu/"
            ),
            ca,
        }
    }
    fn control(&self, file: &str, mode: &str) {
        private_file(
            &self.root.join("control.json"),
            serde_json::to_vec(&json!({"file":file,"mode":mode})).unwrap(),
        );
        private_file(&self.root.join("requests"), b"");
    }
    fn requests(&self) -> String {
        fs::read_to_string(self.root.join("requests")).unwrap_or_default()
    }
    fn download(
        &self,
        base: &str,
        pin: &Path,
        destination: &Path,
        ca: bool,
    ) -> std::process::Output {
        let mut args = vec![
            "installation",
            "component",
            "download",
            "--component",
            "credential-broker",
            "--target",
            "x86_64-unknown-linux-gnu",
            "--base-url",
            base,
            "--trust-key",
            pin.to_str().unwrap(),
            "--destination",
            destination.to_str().unwrap(),
        ];
        if ca {
            args.extend(["--tls-ca-file", self.ca.to_str().unwrap()]);
        }
        command(&args)
    }
}
fn assert_no_staging(root: &Path) {
    assert!(!fs::read_dir(root).unwrap().any(|e| {
        e.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".moosuri-download-")
    }));
}

fn package(root: &Path, kind: &str, target: &str, bytes: &[u8], key: &Ed25519KeyPair) -> PathBuf {
    let bundle = root.join("bundle");
    fs::create_dir(&bundle).unwrap();
    fs::set_permissions(&bundle, fs::Permissions::from_mode(0o700)).unwrap();
    let binary = bundle.join(contract::binary(kind).unwrap());
    private_file(&binary, bytes);
    ok(command(&[
        "installation",
        "component",
        "manifest",
        "--component",
        kind,
        "--target",
        target,
        "--version",
        "1.2.3",
        "--sequence",
        "1",
        "--binary",
        binary.to_str().unwrap(),
        "--output",
        bundle.join("manifest.json").to_str().unwrap(),
    ]));
    sign(&bundle, key);
    bundle
}
fn sign(bundle: &Path, key: &Ed25519KeyPair) {
    private_file(
        &bundle.join("manifest.ed25519"),
        key.sign(&fs::read(bundle.join("manifest.json")).unwrap())
            .as_ref(),
    );
}
fn attach_evidence(bundle: &Path, key: &Ed25519KeyPair) {
    let mut m: Value =
        serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
    let inventory=serde_json::to_vec(&json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"component":{"name":"moosuri","version":"1.2.3"},"properties":release_evidence::LOCKS.iter().map(|p|json!({"name":format!("moosuri:lock-sha256:{p}"),"value":"ab".repeat(32)})).collect::<Vec<_>>()},"components":[]})).unwrap();
    let p=serde_json::to_vec(&json!({"format":1,"purpose":"moosuri-offline-binary-provenance-v1","claim":release_evidence::CLAIM,"artifact":release_evidence::artifact(&m),"source":{"revision":"cd".repeat(20),"dirty":false},"compiler":{"rustc_verbose":"rustc 1.89.0\nhost: synthetic\nrelease: 1.89.0\n"},"inventory":release_evidence::reference(&inventory,release_evidence::INVENTORY)})).unwrap();
    m["provenance"] = release_evidence::reference(&p, release_evidence::PROVENANCE);
    private_file(
        &bundle.join("manifest.json"),
        serde_json::to_vec(&m).unwrap(),
    );
    private_file(&bundle.join(release_evidence::PROVENANCE), p);
    private_file(&bundle.join(release_evidence::INVENTORY), inventory);
    sign(bundle, key);
}
// A tiny synthetic x86_64 Linux executable that would create a witness file if
// invoked. Delivery must never run it; no test intentionally executes this code.
fn witness_elf(witness: &Path) -> Vec<u8> {
    let mut code = vec![
        0x48, 0x8d, 0x3d, 24, 0, 0, 0, 0xbe, 0x80, 1, 0, 0, 0xb8, 85, 0, 0, 0, 0x0f, 0x05, 0xb8,
        60, 0, 0, 0, 0xbf, 97, 0, 0, 0, 0x0f, 0x05,
    ];
    code.extend_from_slice(witness.as_os_str().as_encoded_bytes());
    code.push(0);
    let len = 120 + code.len();
    let mut b = vec![0; len];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    b[16] = 2;
    b[18] = 62;
    b[20] = 1;
    b[24..32].copy_from_slice(&0x400078u64.to_le_bytes());
    b[32] = 64;
    b[52] = 64;
    b[54] = 56;
    b[56] = 1;
    b[64] = 1;
    b[68] = 5;
    b[80..88].copy_from_slice(&0x400000u64.to_le_bytes());
    b[96..104].copy_from_slice(&(len as u64).to_le_bytes());
    b[104..112].copy_from_slice(&(len as u64).to_le_bytes());
    b[112..120].copy_from_slice(&4096u64.to_le_bytes());
    b[120..].copy_from_slice(&code);
    b
}
fn synthetic_elf() -> Vec<u8> {
    let mut b = vec![0; 128];
    b[..7].copy_from_slice(b"\x7fELF\x02\x01\x01");
    for (n, v) in [
        (16, 3),
        (18, 62),
        (20, 1),
        (24, 120),
        (32, 64),
        (52, 64),
        (54, 56),
        (56, 1),
        (64, 1),
        (68, 1),
        (96, 128),
        (104, 128),
    ] {
        b[n] = v;
    }
    b
}
fn verify(bundle: &Path, pin: &Path, kind: &str, target: &str) -> std::process::Output {
    command(&[
        "installation",
        "component",
        "verify",
        "--component",
        kind,
        "--target",
        target,
        "--bundle",
        bundle.to_str().unwrap(),
        "--trust-key",
        pin.to_str().unwrap(),
    ])
}
#[test]
fn component_packages_signed_https_identity_tamper_and_no_implicit_deployment() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[81; 32]).unwrap();
    let pin = root.path().join("pin");
    private_file(
        &pin,
        key.public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    let witness = root.path().join("must-never-execute");
    let bundle = package(
        root.path(),
        "credential-broker",
        "x86_64-unknown-linux-gnu",
        &witness_elf(&witness),
        &key,
    );
    attach_evidence(&bundle, &key);
    let original = fs::read(bundle.join("manifest.json")).unwrap();
    let mirror = Mirror::start(root.path(), "1.2.3");
    let destination = root.path().join("download");
    for (file, mode, code) in [
        (
            "manifest.ed25519",
            "tampered",
            "installation_signature_invalid",
        ),
        (
            release_evidence::PROVENANCE,
            "tampered",
            "release_provenance_hash_mismatch",
        ),
        (
            release_evidence::INVENTORY,
            "missing",
            "installation_download_http_status_refused",
        ),
        (
            "moosuri-credential-broker",
            "missing",
            "installation_download_http_status_refused",
        ),
        (
            "moosuri-credential-broker",
            "tampered",
            "component_bundle_binary_hash_mismatch",
        ),
        (
            "moosuri-credential-broker",
            "partial",
            "installation_download_transport_failed",
        ),
        (
            "moosuri-credential-broker",
            "oversized",
            "installation_download_size_mismatch",
        ),
        (
            "moosuri-credential-broker",
            "redirect",
            "installation_download_http_status_refused",
        ),
    ] {
        mirror.control(file, mode);
        let output = mirror.download(&mirror.base, &pin, &destination, true);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(!destination.exists());
        assert_no_staging(root.path());
        assert!(
            mirror
                .requests()
                .lines()
                .filter(|s| *s == "moosuri-credential-broker")
                .count()
                <= 1
        );
        if file == "manifest.ed25519"
            || file == release_evidence::PROVENANCE
            || file == release_evidence::INVENTORY
        {
            assert!(!mirror.requests().contains("moosuri-credential-broker"));
        }
    }
    for (field, value) in [
        ("component", json!("vault-service")),
        ("target", json!("aarch64-unknown-linux-gnu")),
        ("binary", json!("../escape")),
        ("purpose", json!("moosuri-assistant")),
        ("version", json!("9.9.9")),
    ] {
        let mut m: Value = serde_json::from_slice(&original).unwrap();
        m[field] = value;
        private_file(
            &bundle.join("manifest.json"),
            serde_json::to_vec(&m).unwrap(),
        );
        sign(&bundle, &key);
        mirror.control("", "");
        assert!(
            !mirror
                .download(&mirror.base, &pin, &destination, true)
                .status
                .success()
        );
        assert_eq!(mirror.requests(), "manifest.json\nmanifest.ed25519\n");
        assert_no_staging(root.path());
    }
    private_file(&bundle.join("manifest.json"), &original);
    sign(&bundle, &key);
    mirror.control("", "");
    assert!(
        !mirror
            .download(&mirror.base, &pin, &destination, false)
            .status
            .success()
    );
    assert_no_staging(root.path());
    ok(mirror.download(&mirror.base, &pin, &destination, true));
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 5);
    assert!(
        !witness.exists(),
        "component download/verify must never execute bytes"
    );
    ok(verify(
        &destination,
        &pin,
        "credential-broker",
        "x86_64-unknown-linux-gnu",
    ));
    assert!(
        !verify(
            &destination,
            &pin,
            "vault-service",
            "x86_64-unknown-linux-gnu"
        )
        .status
        .success()
    );
    assert!(
        !verify(
            &destination,
            &pin,
            "credential-broker",
            "aarch64-unknown-linux-gnu"
        )
        .status
        .success()
    );
    // This purpose-tagged component must not enter the assistant install path.
    assert!(
        !command(&[
            "installation",
            "verify",
            "--bundle",
            destination.to_str().unwrap(),
            "--trust-key",
            pin.to_str().unwrap()
        ])
        .status
        .success()
    );
    mirror.control("", "");
    assert!(
        !mirror
            .download(&mirror.base, &pin, &destination, true)
            .status
            .success()
    );
    assert!(mirror.requests().is_empty());
    private_file(&destination.join("unsigned-config"), b"unrelated");
    assert!(
        !verify(
            &destination,
            &pin,
            "credential-broker",
            "x86_64-unknown-linux-gnu"
        )
        .status
        .success()
    );
    assert_eq!(
        fs::read(destination.join("unsigned-config")).unwrap(),
        b"unrelated"
    );
    // Even fully signed/hash-consistent metadata cannot pass truncated headers.
    let mut malformed: Value = serde_json::from_slice(&original).unwrap();
    malformed.as_object_mut().unwrap().remove("provenance");
    let truncated = &synthetic_elf()[..64];
    malformed["size"] = json!(truncated.len());
    malformed["sha256"] = json!(release_evidence::hash(truncated));
    private_file(&bundle.join("moosuri-credential-broker"), truncated);
    private_file(
        &bundle.join("manifest.json"),
        serde_json::to_vec(&malformed).unwrap(),
    );
    sign(&bundle, &key);
    let rejected = root.path().join("truncated-download");
    mirror.control("", "");
    let output = mirror.download(&mirror.base, &pin, &rejected, true);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr)
            .contains("component_bundle_executable_target_mismatch")
    );
    assert!(!rejected.exists());
    assert_no_staging(root.path());
    private_file(&bundle.join("moosuri-credential-broker"), synthetic_elf());

    // Replace the source with a separately signed vault package; caller intent/base
    // must both name vault-service, and no assistant/runtime/config is created.
    fs::remove_file(bundle.join("manifest.ed25519")).unwrap();
    fs::remove_file(bundle.join("manifest.json")).unwrap();
    fs::rename(
        bundle.join("moosuri-credential-broker"),
        bundle.join("moosuri-vault-service"),
    )
    .unwrap();
    let m = contract::make(
        "vault-service",
        "x86_64-unknown-linux-gnu",
        "1.2.3",
        2,
        &synthetic_elf(),
    )
    .unwrap();
    private_file(
        &bundle.join("manifest.json"),
        serde_json::to_vec(&m).unwrap(),
    );
    sign(&bundle, &key);
    let vault = root.path().join("vault-download");
    let base = mirror.base.replace("credential-broker", "vault-service");
    ok(command(&[
        "installation",
        "component",
        "download",
        "--component",
        "vault-service",
        "--target",
        "x86_64-unknown-linux-gnu",
        "--base-url",
        &base,
        "--trust-key",
        pin.to_str().unwrap(),
        "--destination",
        vault.to_str().unwrap(),
        "--tls-ca-file",
        mirror.ca.to_str().unwrap(),
    ]));
    ok(verify(
        &vault,
        &pin,
        "vault-service",
        "x86_64-unknown-linux-gnu",
    ));
    assert_eq!(fs::read_dir(&vault).unwrap().count(), 3);
    assert!(!root.path().join("runtime").exists());
    assert_no_staging(root.path());
    assert!(
        !witness.exists(),
        "no package/create/download/verify operation executes a component"
    );
}
#[test]
#[ignore = "requires separately built real broker/vault executables in explicit environment paths"]
fn component_packages_real_prebuilt_broker_and_vault_signed_https_delivery() {
    for (kind, variable) in [
        ("credential-broker", "MOOSURI_COMPONENT_TEST_BROKER"),
        ("vault-service", "MOOSURI_COMPONENT_TEST_VAULT"),
    ] {
        let target = std::env::var("MOOSURI_COMPONENT_TEST_TARGET")
            .expect("set explicit compiled native target");
        let path = std::env::var(variable).expect("set explicit prebuilt component path");
        let bytes = fs::read(path).unwrap();
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
        let key = Ed25519KeyPair::from_seed_unchecked(&[82; 32]).unwrap();
        let pin = root.path().join("pin");
        private_file(
            &pin,
            key.public_key()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        );
        let bundle = package(root.path(), kind, &target, &bytes, &key);
        // Real executable bytes take exactly the same authenticated HTTPS route.
        let mut manifest: Value =
            serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
        manifest["version"] = json!(env!("CARGO_PKG_VERSION"));
        private_file(
            &bundle.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        );
        sign(&bundle, &key);
        ok(verify(&bundle, &pin, kind, &target));
        let mirror = Mirror::start(root.path(), env!("CARGO_PKG_VERSION"));
        let base = mirror
            .base
            .replace("credential-broker", kind)
            .replace("x86_64-unknown-linux-gnu", &target);
        let destination = root.path().join("authenticated-real-component");
        ok(command(&[
            "installation",
            "component",
            "download",
            "--component",
            kind,
            "--target",
            &target,
            "--base-url",
            &base,
            "--trust-key",
            pin.to_str().unwrap(),
            "--destination",
            destination.to_str().unwrap(),
            "--tls-ca-file",
            mirror.ca.to_str().unwrap(),
        ]));
        ok(verify(&destination, &pin, kind, &target));
        assert_eq!(
            fs::read(destination.join(contract::binary(kind).unwrap())).unwrap(),
            bytes
        );
        assert_eq!(
            fs::read(bundle.join(contract::binary(kind).unwrap())).unwrap(),
            bytes
        );
    }
}
