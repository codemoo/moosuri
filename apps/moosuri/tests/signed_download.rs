//! Real CLI + ordinary TLS synthetic mirror. Python/OpenSSL are fixture tools only.
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::{Duration, Instant},
};
#[path = "../../../tools/release/provenance.rs"]
mod evidence;

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
            base: format!("https://127.0.0.1:{port}/v{version}/"),
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
            "download",
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

#[test]
fn signed_download_tls_cli_failures_then_verify_install_without_implicit_execution() {
    let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&[72; 32]).unwrap();
    let pin = root.path().join("pin");
    private_file(
        &pin,
        key.public_key()
            .as_ref()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    );
    let bundle = root.path().join("bundle");
    fs::create_dir(&bundle).unwrap();
    fs::set_permissions(&bundle, fs::Permissions::from_mode(0o700)).unwrap();
    let binary = bundle.join("moosuri");
    fs::copy(env!("CARGO_BIN_EXE_moosuri"), &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o700)).unwrap();
    ok(command(&[
        "installation",
        "manifest",
        "--binary",
        binary.to_str().unwrap(),
        "--sequence",
        "1",
        "--output",
        bundle.join("manifest.json").to_str().unwrap(),
    ]));
    let mut manifest: Value =
        serde_json::from_slice(&fs::read(bundle.join("manifest.json")).unwrap()).unwrap();
    let version = manifest["version"].as_str().unwrap().to_owned();
    let inventory = serde_json::to_vec(&json!({"bomFormat":"CycloneDX","specVersion":"1.6","version":1,"metadata":{"component":{"name":"moosuri","version":version},"properties":evidence::LOCKS.iter().map(|p|json!({"name":format!("moosuri:lock-sha256:{p}"),"value":"ab".repeat(32)})).collect::<Vec<_>>()},"components":[]})).unwrap();
    let provenance = serde_json::to_vec(&json!({"format":1,"purpose":"moosuri-offline-binary-provenance-v1","claim":evidence::CLAIM,"artifact":evidence::artifact(&manifest),"source":{"revision":"cd".repeat(20),"dirty":false},"compiler":{"rustc_verbose":"rustc 1.89.0\nhost: synthetic\nrelease: 1.89.0\n"},"inventory":evidence::reference(&inventory,evidence::INVENTORY)})).unwrap();
    manifest["provenance"] = evidence::reference(&provenance, evidence::PROVENANCE);
    let bytes = serde_json::to_vec(&manifest).unwrap();
    private_file(&bundle.join("manifest.json"), &bytes);
    private_file(&bundle.join("manifest.ed25519"), key.sign(&bytes).as_ref());
    private_file(&bundle.join(evidence::PROVENANCE), provenance);
    private_file(&bundle.join(evidence::INVENTORY), inventory);
    let mirror = Mirror::start(root.path(), &version);
    let destination = root.path().join("download");
    for (file, mode, code, binary_requested) in [
        (
            "manifest.ed25519",
            "tampered",
            "installation_signature_invalid",
            false,
        ),
        (
            "manifest.ed25519",
            "missing",
            "installation_download_http_status_refused",
            false,
        ),
        (
            "manifest.json",
            "oversized",
            "installation_download_size_mismatch",
            false,
        ),
        (
            evidence::PROVENANCE,
            "tampered",
            "release_provenance_hash_mismatch",
            false,
        ),
        (
            evidence::INVENTORY,
            "missing",
            "installation_download_http_status_refused",
            false,
        ),
        (
            evidence::INVENTORY,
            "tampered",
            "release_provenance_hash_mismatch",
            false,
        ),
        (
            "moosuri",
            "tampered",
            "installation_binary_hash_mismatch",
            true,
        ),
        (
            "moosuri",
            "oversized",
            "installation_download_size_mismatch",
            true,
        ),
        (
            "moosuri",
            "partial",
            "installation_download_transport_failed",
            true,
        ),
        (
            "moosuri",
            "redirect",
            "installation_download_http_status_refused",
            true,
        ),
    ] {
        mirror.control(file, mode);
        let output = mirror.download(&mirror.base, &pin, &destination, true);
        assert!(!output.status.success(), "accepted {file}/{mode}");
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{file}/{mode}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(
            mirror.requests().lines().any(|s| s == "moosuri"),
            binary_requested
        );
        assert!(
            mirror
                .requests()
                .lines()
                .filter(|s| *s == "moosuri")
                .count()
                <= 1,
            "redirect must not produce a second request"
        );
        assert!(!destination.exists());
        assert_no_staging(root.path());
    }
    mirror.control("", "");
    // Normal TLS rejects an untrusted test CA; no insecure test switch.
    assert!(
        !mirror
            .download(&mirror.base, &pin, &destination, false)
            .status
            .success()
    );
    assert_no_staging(root.path());
    for invalid in [
        mirror.base.replacen("https:", "http:", 1),
        mirror
            .base
            .replace("127.0.0.1", "operator:secret@127.0.0.1"),
        format!("{}?token=secret", mirror.base),
        format!("{}../", mirror.base),
    ] {
        let output = mirror.download(&invalid, &pin, &destination, true);
        assert!(!output.status.success());
        assert!(!String::from_utf8_lossy(&output.stderr).contains("secret"));
    }
    assert!(mirror.requests().is_empty());
    let mismatch = mirror.base.replace(&format!("v{version}"), "v9.9.9");
    assert!(
        !mirror
            .download(&mismatch, &pin, &destination, true)
            .status
            .success()
    );
    assert!(!mirror.requests().lines().any(|s| s == "moosuri"));
    assert_no_staging(root.path());
    let wrong_pin = root.path().join("wrong-pin");
    private_file(&wrong_pin, "ff".repeat(32));
    mirror.control("", "");
    let wrong = mirror.download(&mirror.base, &wrong_pin, &destination, true);
    assert!(!wrong.status.success());
    assert!(String::from_utf8_lossy(&wrong.stderr).contains("installation_signature_invalid"));
    assert!(!mirror.requests().lines().any(|s| s == "moosuri"));
    assert_no_staging(root.path());
    for (member, value, code) in [
        (
            "binary",
            json!("../escape"),
            "installation_manifest_refused",
        ),
        (
            "target",
            json!("unrelated-native-target"),
            "installation_target_mismatch",
        ),
    ] {
        let mut rejected = manifest.clone();
        rejected[member] = value;
        let rejected = serde_json::to_vec(&rejected).unwrap();
        private_file(&bundle.join("manifest.json"), &rejected);
        private_file(
            &bundle.join("manifest.ed25519"),
            key.sign(&rejected).as_ref(),
        );
        mirror.control("", "");
        let output = mirror.download(&mirror.base, &pin, &destination, true);
        assert!(!output.status.success());
        assert!(String::from_utf8_lossy(&output.stderr).contains(code));
        assert_eq!(mirror.requests(), "manifest.json\nmanifest.ed25519\n");
        assert_no_staging(root.path());
    }
    private_file(&bundle.join("manifest.json"), &bytes);
    private_file(&bundle.join("manifest.ed25519"), key.sign(&bytes).as_ref());

    // A signed executable with a visible side effect must remain unexecuted by
    // both download and offline verify, even when its metadata claims this build.
    let actual_binary = root.path().join("saved-native-binary");
    fs::rename(&binary, &actual_binary).unwrap();
    let witness = root.path().join("execution-witness");
    let script = format!(
        "#!/bin/sh\nprintf executed > '{}'\nexit 97\n",
        witness.display()
    );
    private_file(&binary, script.as_bytes());
    let mut inert_manifest = manifest.clone();
    inert_manifest.as_object_mut().unwrap().remove("provenance");
    inert_manifest["size"] = json!(script.len());
    inert_manifest["sha256"] = json!(format!("{:x}", Sha256::digest(script.as_bytes())));
    let inert_bytes = serde_json::to_vec(&inert_manifest).unwrap();
    private_file(&bundle.join("manifest.json"), &inert_bytes);
    private_file(
        &bundle.join("manifest.ed25519"),
        key.sign(&inert_bytes).as_ref(),
    );
    let inert_destination = root.path().join("unexecuted-download");
    mirror.control("", "");
    ok(mirror.download(&mirror.base, &pin, &inert_destination, true));
    ok(command(&[
        "installation",
        "verify",
        "--bundle",
        inert_destination.to_str().unwrap(),
        "--trust-key",
        pin.to_str().unwrap(),
    ]));
    assert!(
        !witness.exists(),
        "download and verify must never execute bundle bytes"
    );
    fs::remove_file(&binary).unwrap();
    fs::rename(&actual_binary, &binary).unwrap();
    private_file(&bundle.join("manifest.json"), &bytes);
    private_file(&bundle.join("manifest.ed25519"), key.sign(&bytes).as_ref());
    mirror.control("", "");
    ok(mirror.download(&mirror.base, &pin, &destination, true));
    assert_eq!(fs::read_dir(&destination).unwrap().count(), 5);
    assert_eq!(
        fs::metadata(&destination).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        Sha256::digest(fs::read(destination.join("moosuri")).unwrap()),
        Sha256::digest(fs::read(&binary).unwrap())
    );
    ok(command(&[
        "installation",
        "verify",
        "--bundle",
        destination.to_str().unwrap(),
        "--trust-key",
        pin.to_str().unwrap(),
    ]));
    mirror.control("", "");
    assert!(
        !mirror
            .download(&mirror.base, &pin, &destination, true)
            .status
            .success()
    );
    assert!(
        mirror.requests().is_empty(),
        "existing destination rejects before network"
    );
    let unrelated = root.path().join("unrelated");
    fs::create_dir(&unrelated).unwrap();
    private_file(&unrelated.join("keep"), b"retained");
    assert!(
        !mirror
            .download(&mirror.base, &pin, &unrelated, true)
            .status
            .success()
    );
    assert_eq!(fs::read(unrelated.join("keep")).unwrap(), b"retained");
    let prefix = root.path().join("runtime");
    let data = root.path().join("data");
    assert!(
        !prefix.exists() && !data.exists(),
        "download/verify do not install or initialize"
    );
    ok(command(&[
        "installation",
        "install",
        "--bundle",
        destination.to_str().unwrap(),
        "--trust-key",
        pin.to_str().unwrap(),
        "--prefix",
        prefix.to_str().unwrap(),
        "--data-dir",
        data.to_str().unwrap(),
        "--origin",
        "https://assistant.example.test",
        "--network",
        "https",
    ]));
    assert!(prefix.join("manager").is_file());
    assert!(
        !data.exists(),
        "explicit install still leaves init separate"
    );
    // Legacy three-file bundles remain downloadable; no unsigned adjacent evidence fetched.
    manifest.as_object_mut().unwrap().remove("provenance");
    let bytes = serde_json::to_vec(&manifest).unwrap();
    private_file(&bundle.join("manifest.json"), &bytes);
    private_file(&bundle.join("manifest.ed25519"), key.sign(&bytes).as_ref());
    mirror.control("", "");
    let legacy = root.path().join("legacy-download");
    ok(mirror.download(&mirror.base, &pin, &legacy, true));
    assert_eq!(fs::read_dir(legacy).unwrap().count(), 3);
    assert_eq!(
        mirror.requests(),
        "manifest.json\nmanifest.ed25519\nmoosuri\n"
    );
    assert_no_staging(root.path());
}
