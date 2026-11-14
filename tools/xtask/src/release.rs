//! Offline packaging only: no builds, registry, signing key or binary execution.
use super::{Check, release_evidence as evidence, sbom};
use serde_json::{Value, json};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};
const BINARY_CAP: u64 = 256 * 1024 * 1024;
fn fail() -> String {
    "release_packaging_invalid".into()
}
fn read(path: &Path, cap: u64) -> Check<Vec<u8>> {
    let m = fs::symlink_metadata(path).map_err(|_| fail())?;
    if !m.is_file() || m.len() > cap || m.nlink() != 1 {
        return Err(fail());
    }
    let mut b = vec![];
    fs::File::open(path)
        .map_err(|_| fail())?
        .take(cap + 1)
        .read_to_end(&mut b)
        .map_err(|_| fail())?;
    if b.len() as u64 > cap {
        return Err(fail());
    }
    Ok(b)
}
fn create(path: &Path, bytes: &[u8]) -> Check<()> {
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| fail())?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(|_| fail())
}
fn encode(v: &Value) -> Check<Vec<u8>> {
    let mut b = serde_json::to_vec_pretty(v).map_err(|_| fail())?;
    b.push(b'\n');
    Ok(b)
}
fn manifest(bundle: &Path) -> Check<(Vec<u8>, Value)> {
    let raw = read(&bundle.join("manifest.json"), 16384)?;
    let m: Value = serde_json::from_slice(&raw).map_err(|_| fail())?;
    let required = [
        "format",
        "product",
        "version",
        "target",
        "sequence",
        "store_schema",
        "embedded_web",
        "binary",
        "size",
        "sha256",
    ];
    let o = m.as_object().ok_or_else(fail)?;
    if required.iter().any(|k| !o.contains_key(*k))
        || o.keys()
            .any(|k| !required.contains(&k.as_str()) && k != "provenance")
        || m["format"] != 1
        || m["product"] != "moosuri"
        || m["binary"] != "moosuri"
        || m["embedded_web"] != true
        || m["sequence"].as_u64().is_none_or(|n| n == 0)
        || m["store_schema"].as_u64().is_none_or(|n| n == 0)
        || ["target", "version"].iter().any(|k| {
            !m[*k].as_str().is_some_and(|s| {
                !s.is_empty()
                    && s.len() <= 128
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"-._+".contains(&b))
            })
        })
    {
        return Err(fail());
    }
    let binary = read(&bundle.join("moosuri"), BINARY_CAP)?;
    if binary.is_empty()
        || m["size"].as_u64() != Some(binary.len() as u64)
        || m["sha256"] != evidence::hash(&binary)
    {
        return Err("release_binary_hash_mismatch".into());
    }
    Ok((raw, m))
}
fn output(mut c: Command, cap: u64) -> Check<String> {
    let mut child = c
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| fail())?;
    let mut b = vec![];
    let result = child
        .stdout
        .take()
        .ok_or_else(fail)?
        .take(cap + 1)
        .read_to_end(&mut b);
    if result.is_err() || b.len() as u64 > cap {
        let _ = child.kill();
        let _ = child.wait();
        return Err(fail());
    }
    if !child.wait().map_err(|_| fail())?.success() {
        return Err(fail());
    }
    String::from_utf8(b).map_err(|_| fail())
}
fn git(root: &Path, args: &[&str], cap: u64) -> Check<String> {
    let mut c = Command::new("git");
    c.arg("-C").arg(root).args(args);
    output(c, cap)
}
fn metadata(root: &Path) -> Check<(Value, Value)> {
    let revision = git(root, &["rev-parse", "--verify", "HEAD"], 256)?
        .trim()
        .to_owned();
    let dirty = !git(
        root,
        &["status", "--porcelain=v1", "-z", "--untracked-files=normal"],
        1024 * 1024,
    )?
    .is_empty();
    let mut c = Command::new(std::env::var_os("RUSTC").unwrap_or_else(|| "rustc".into()));
    c.arg("-vV");
    let compiler = output(c, 4096)?;
    Ok((
        json!({"revision":revision,"dirty":dirty}),
        json!({"rustc_verbose":compiler}),
    ))
}
fn require_new(bundle: &Path, name: &str) -> Check<()> {
    match fs::symlink_metadata(bundle.join(name)) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("release_output_exists".into()),
    }
}
pub(super) fn run(root: &Path, args: &[String]) -> Check<()> {
    if args.len() != 3 || args[1] != "--bundle" {
        return Err(
            "usage: xtask <release-provenance|verify-provenance> --bundle DIRECTORY".into(),
        );
    }
    let bundle = PathBuf::from(&args[2]);
    let m = fs::symlink_metadata(&bundle).map_err(|_| fail())?;
    if !m.is_dir() || m.uid() != unsafe { geteuid() } || m.mode() & 0o077 != 0 {
        return Err("release_private_bundle_required".into());
    }
    let (original, mut manifest) = manifest(&bundle)?;
    if args[0] == "verify-provenance" {
        verify(&bundle, &manifest)?;
        println!(
            "Release artifact/provenance/inventory bytes agree. No signature or builder identity was authenticated; use the installer with an independent trust pin."
        );
        return Ok(());
    }
    if manifest.get("provenance").is_some() {
        return Err("release_output_exists".into());
    }
    for name in [
        "manifest.ed25519",
        evidence::PROVENANCE,
        evidence::INVENTORY,
    ] {
        require_new(&bundle, name)?;
    }
    let inventory = sbom::generate(root)?;
    let (source, compiler) = metadata(root)?;
    let p = json!({"format":1,"purpose":"moosuri-offline-binary-provenance-v1","claim":evidence::CLAIM,"artifact":evidence::artifact(&manifest),"source":source,"compiler":compiler,"inventory":evidence::reference(&inventory,evidence::INVENTORY)});
    let provenance = encode(&p)?;
    manifest["provenance"] = evidence::reference(&provenance, evidence::PROVENANCE);
    evidence::verify(&manifest["provenance"], &provenance, &inventory, &manifest)
        .map_err(str::to_owned)?;
    let updated = encode(&manifest)?;
    let temporary = bundle.join(format!(".manifest-provenance-{}", std::process::id()));
    create(&temporary, &updated)?;
    let result = (|| {
        create(&bundle.join(evidence::INVENTORY), &inventory)?;
        create(&bundle.join(evidence::PROVENANCE), &provenance)?;
        if read(&bundle.join("manifest.json"), 16384)? != original {
            return Err("release_manifest_changed".into());
        }
        require_new(&bundle, "manifest.ed25519")?;
        fs::rename(&temporary, bundle.join("manifest.json")).map_err(|_| fail())?;
        fs::File::open(&bundle)
            .and_then(|f| f.sync_all())
            .map_err(|_| fail())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    println!(
        "Unsigned manifest now binds provenance and exact source-lock inventory. Sign these final manifest bytes externally; no release key or builder authenticity was created."
    );
    Ok(())
}
unsafe extern "C" {
    fn geteuid() -> u32;
}
fn verify(bundle: &Path, manifest: &Value) -> Check<()> {
    let r = manifest
        .get("provenance")
        .ok_or("release_provenance_missing")?;
    evidence::validate_reference(r, evidence::PROVENANCE, evidence::PROVENANCE_CAP)
        .map_err(str::to_owned)?;
    evidence::verify(
        r,
        &read(&bundle.join(evidence::PROVENANCE), evidence::PROVENANCE_CAP)?,
        &read(&bundle.join(evidence::INVENTORY), evidence::INVENTORY_CAP)?,
        manifest,
    )
    .map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        os::unix::fs::DirBuilderExt,
        time::{SystemTime, UNIX_EPOCH},
    };
    struct Temporary(PathBuf);
    impl Drop for Temporary {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    fn fixture_git(root: &Path, args: &[&str]) {
        let mut c = Command::new("git");
        c.env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_AUTHOR_DATE", "2000-01-01T00:00:00Z")
            .env("GIT_COMMITTER_DATE", "2000-01-01T00:00:00Z")
            .arg("-C")
            .arg(root)
            .args(args);
        output(c, 4096).unwrap();
    }
    fn fixture_source(parent: &Path) -> PathBuf {
        let root = parent.join("source");
        fs::DirBuilder::new().mode(0o700).create(&root).unwrap();
        let version = env!("CARGO_PKG_VERSION");
        create(
            &root.join("Cargo.toml"),
            format!("[package]\nname = \"moosuri\"\nversion = \"{version}\"\n").as_bytes(),
        )
        .unwrap();
        create(
            &root.join("Cargo.lock"),
            format!("version = 4\n[[package]]\nname = \"moosuri\"\nversion = \"{version}\"\n[[package]]\nname = \"synthetic-dependency\"\nversion = \"1.2.3\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"{}\"\n", "ab".repeat(32)).as_bytes(),
        ).unwrap();
        for path in &evidence::LOCKS[1..] {
            let p = root.join(path);
            fs::create_dir_all(p.parent().unwrap()).unwrap();
            create(&p, br#"{"lockfileVersion":3,"packages":{"":{}}}"#).unwrap();
        }
        fixture_git(
            &root,
            &["-c", "init.defaultBranch=fixture", "init", "--quiet"],
        );
        for (name, value) in [
            ("user.name", "Synthetic fixture"),
            ("user.email", "fixture@example.test"),
            ("commit.gpgSign", "false"),
            ("core.hooksPath", "/dev/null"),
            ("core.fsmonitor", "false"),
        ] {
            fixture_git(&root, &["config", "--local", name, value]);
        }
        fixture_git(
            &root,
            &[
                "add",
                "--",
                "Cargo.toml",
                "Cargo.lock",
                "web/package-lock.json",
                "vault-web/package-lock.json",
                "tools/openspec/package-lock.json",
            ],
        );
        fixture_git(
            &root,
            &[
                "commit",
                "--quiet",
                "--no-verify",
                "-m",
                "Synthetic source-lock fixture",
            ],
        );
        root
    }
    #[test]
    fn release_provenance_actual_packaging_binds_fresh_inventory_and_rejects_mutation() {
        let dir = std::env::temp_dir().join(format!(
            "moosuri-provenance-synthetic-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        let temp = Temporary(dir);
        let root = fixture_source(&temp.0);
        let bundle = temp.0.join("bundle");
        fs::DirBuilder::new().mode(0o700).create(&bundle).unwrap();
        create(&bundle.join("moosuri"), b"synthetic artifact bytes").unwrap();
        let m = json!({"format":1,"product":"moosuri","version":env!("CARGO_PKG_VERSION"),"target":"synthetic-target","sequence":1,"store_schema":47,"embedded_web":true,"binary":"moosuri","size":24,"sha256":evidence::hash(b"synthetic artifact bytes")});
        // Derive the exact length rather than trusting the example declaration.
        let mut m = m;
        m["size"] = json!(b"synthetic artifact bytes".len());
        create(&bundle.join("manifest.json"), &encode(&m).unwrap()).unwrap();
        let path = bundle.to_str().unwrap().to_owned();
        run(
            &root,
            &["release-provenance".into(), "--bundle".into(), path.clone()],
        )
        .unwrap();
        let (_, m) = manifest(&bundle).unwrap();
        verify(&bundle, &m).unwrap();
        let p: Value = serde_json::from_slice(
            &read(&bundle.join(evidence::PROVENANCE), evidence::PROVENANCE_CAP).unwrap(),
        )
        .unwrap();
        assert_eq!(
            p["source"]["dirty"], false,
            "fixture artifacts must not dirty source"
        );
        assert_eq!(
            read(&bundle.join(evidence::INVENTORY), evidence::INVENTORY_CAP).unwrap(),
            sbom::generate(&root).unwrap()
        );
        let raw = read(&bundle.join("manifest.json"), 16384).unwrap();
        assert!(
            run(
                &root,
                &["release-provenance".into(), "--bundle".into(), path.clone()]
            )
            .is_err()
        );
        assert_eq!(raw, read(&bundle.join("manifest.json"), 16384).unwrap());
        fs::write(bundle.join(evidence::INVENTORY), b"changed").unwrap();
        assert!(
            run(
                &root,
                &["verify-provenance".into(), "--bundle".into(), path]
            )
            .is_err()
        );
        assert_eq!(
            read(&bundle.join("moosuri"), BINARY_CAP).unwrap(),
            b"synthetic artifact bytes"
        );
    }
}
