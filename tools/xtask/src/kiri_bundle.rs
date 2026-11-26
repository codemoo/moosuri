//! Build a source-selected Linux installation bundle. No host service changes.
use super::{Check, ensure};
use serde_json::json;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn command(root: &Path, binary: &str) -> Check<Command> {
    let mut command = Command::new(binary);
    if binary == "git" {
        for (key, _) in
            std::env::vars_os().filter(|(key, _)| key.to_string_lossy().starts_with("GIT_"))
        {
            command.env_remove(key);
        }
        command
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_SYSTEM", "/dev/null")
            .env("GIT_ATTR_NOSYSTEM", "1")
            .args(["-c", "core.attributesFile=/dev/null"])
            .arg("-c")
            .arg(format!("safe.directory={}", root.display()));
    }
    if binary == "docker" {
        command
            .args(["--host=unix:///var/run/docker.sock", "--config"])
            .arg(
                root.parent()
                    .ok_or("kiri_bundle_staging_failed")?
                    .join("docker-config"),
            )
            .env_remove("DOCKER_CONTEXT")
            .env_remove("DOCKER_HOST")
            .env_remove("DOCKER_TLS_VERIFY")
            .env_remove("DOCKER_CERT_PATH")
            .env_remove("BUILDX_BUILDER");
    }
    if binary == "tar" {
        command.env_remove("TAR_OPTIONS");
    }
    command.current_dir(root).stdin(Stdio::null());
    Ok(command)
}
fn capture(root: &Path, binary: &str, args: &[&str]) -> Check<String> {
    let result = command(root, binary)?
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| format!("kiri_bundle_{binary}_unavailable"))?;
    ensure(
        result.status.success(),
        format!("kiri_bundle_{binary}_failed"),
    )?;
    ensure(
        result.stdout.len() <= 1024 * 1024,
        "kiri_bundle_output_limit",
    )?;
    String::from_utf8(result.stdout).map_err(|_| "kiri_bundle_output_invalid".into())
}
fn run_command(root: &Path, binary: &str, args: &[&str]) -> Check<()> {
    let status = command(root, binary)?
        .args(args)
        .current_dir(root)
        .stdin(Stdio::null())
        .status()
        .map_err(|_| format!("kiri_bundle_{binary}_unavailable"))?;
    ensure(status.success(), format!("kiri_bundle_{binary}_failed"))
}
fn path(path: &Path) -> Check<&str> {
    path.to_str()
        .filter(|s| !s.contains([',', '\n', '\r']))
        .ok_or_else(|| "kiri_bundle_path_invalid".into())
}
fn digest(path: &Path, cap: u64) -> Check<String> {
    let meta = fs::symlink_metadata(path).map_err(|_| "kiri_bundle_artifact_missing")?;
    ensure(
        meta.is_file() && meta.nlink() == 1 && meta.len() > 0 && meta.len() <= cap,
        "kiri_bundle_artifact_invalid",
    )?;
    let mut file = File::open(path).map_err(|_| "kiri_bundle_artifact_unavailable")?;
    let mut sha = Sha256::new();
    let mut buffer = [0u8; 65536];
    let mut total = 0u64;
    loop {
        let n = file
            .read(&mut buffer)
            .map_err(|_| "kiri_bundle_artifact_unavailable")?;
        if n == 0 {
            break;
        }
        total += n as u64;
        ensure(total <= cap, "kiri_bundle_artifact_limit")?;
        sha.update(&buffer[..n]);
    }
    ensure(total == meta.len(), "kiri_bundle_artifact_changed")?;
    Ok(format!("{:x}", sha.finalize()))
}

/// Set an explicit mode (independent of umask) and verify the artifact is a
/// regular, single-link, expected-owner file that group/other cannot write.
fn secure_artifact(path: &Path, mode: u32, cap: u64, owner: u32) -> Check<()> {
    let before = fs::symlink_metadata(path).map_err(|_| "kiri_bundle_artifact_missing")?;
    ensure(
        before.is_file() && before.nlink() == 1,
        "kiri_bundle_artifact_invalid",
    )?;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .map_err(|_| "kiri_bundle_artifact_mode_failed")?;
    let meta = fs::symlink_metadata(path).map_err(|_| "kiri_bundle_artifact_missing")?;
    ensure(
        meta.is_file()
            && meta.nlink() == 1
            && meta.uid() == owner
            && meta.mode() & 0o022 == 0
            && meta.mode() & 0o7777 == mode
            && meta.len() > 0
            && meta.len() <= cap,
        "kiri_bundle_artifact_invalid",
    )
}

/// Kiri identifies images by the config JSON digest. With Docker's containerd
/// image store, even image inspect .Id may instead be an OCI index digest.
/// Read the saved archive's config without extracting any path to the host.
fn image_config_id(root: &Path, archive: &Path) -> Check<String> {
    let member = |name: &str| {
        capture(
            root,
            "tar",
            &[
                "--extract",
                "--to-stdout",
                "--occurrence=1",
                "--file",
                path(archive)?,
                "--",
                name,
            ],
        )
    };
    let manifest: serde_json::Value = serde_json::from_str(&member("manifest.json")?)
        .map_err(|_| "kiri_bundle_image_manifest_invalid")?;
    let items = manifest
        .as_array()
        .filter(|a| a.len() == 1)
        .ok_or("kiri_bundle_single_image_required")?;
    let config = items[0]["Config"]
        .as_str()
        .ok_or("kiri_bundle_image_config_invalid")?;
    let expected = config
        .strip_prefix("blobs/sha256/")
        .or_else(|| config.strip_suffix(".json"))
        .filter(|h| {
            h.len() == 64
                && h.bytes()
                    .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
        })
        .ok_or("kiri_bundle_image_config_invalid")?;
    let bytes = member(config)?;
    let digest = format!("{:x}", Sha256::digest(bytes.as_bytes()));
    ensure(
        digest == expected,
        "kiri_bundle_image_config_digest_mismatch",
    )?;
    let config: serde_json::Value =
        serde_json::from_str(&bytes).map_err(|_| "kiri_bundle_image_config_invalid")?;
    ensure(
        config["os"] == "linux" && config["architecture"] == "amd64",
        "kiri_bundle_image_platform_invalid",
    )?;
    Ok(format!("sha256:{digest}"))
}

pub fn run(root: &Path, args: &[String]) -> Check<()> {
    let root = root
        .canonicalize()
        .map_err(|_| "kiri_bundle_checkout_missing")?;
    let root = root.as_path();
    ensure(
        cfg!(all(target_os = "linux", target_arch = "x86_64")),
        "kiri_bundle_linux_amd64_required",
    )?;
    ensure(
        (args.len() == 2 || (args.len() == 4 && args[2] == "--revision")) && args[0] == "--output",
        "usage: kiri-bundle --output NEW_ABSOLUTE_DIRECTORY [--revision COMMIT]",
    )?;
    let output = PathBuf::from(&args[1]);
    ensure(
        output.is_absolute()
            && !output.starts_with(root)
            && output.components().all(|c| {
                matches!(
                    c,
                    std::path::Component::RootDir | std::path::Component::Normal(_)
                )
            }),
        "kiri_bundle_output_outside_checkout_required",
    )?;
    let parent = output.parent().ok_or("kiri_bundle_output_invalid")?;
    ensure(
        parent.canonicalize().ok().as_deref() == Some(parent),
        "kiri_bundle_canonical_parent_required",
    )?;
    path(&output)?;
    let uid = unsafe { libc::geteuid() };
    let owner = std::env::var("SUDO_UID")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(uid);
    for dir in [root.to_owned(), root.join(".git")] {
        let m = fs::symlink_metadata(&dir).map_err(|_| "kiri_bundle_checkout_untrusted")?;
        ensure(
            m.is_dir() && m.mode() & 0o022 == 0 && (m.uid() == uid || m.uid() == owner),
            "kiri_bundle_checkout_untrusted",
        )?;
    }
    ensure(
        !root
            .join(".git/info/attributes")
            .try_exists()
            .map_err(|_| "kiri_bundle_checkout_untrusted")?,
        "kiri_bundle_archive_attributes_require_review",
    )?;
    let selected = args.get(3).map(String::as_str).unwrap_or("HEAD");
    ensure(
        selected == "HEAD"
            || (selected.len() == 40 && selected.bytes().all(|b| b.is_ascii_hexdigit())),
        "kiri_bundle_revision_invalid",
    )?;
    let revision = capture(
        root,
        "git",
        &["rev-parse", "--verify", &format!("{selected}^{{commit}}")],
    )?;
    let revision = revision.trim();
    ensure(
        revision.len() == 40 && revision.bytes().all(|c| c.is_ascii_hexdigit()),
        "kiri_bundle_revision_invalid",
    )?;
    // A Git archive excludes ignored build assets, credentials and editor state.
    // Reject symlinks before extraction instead of following them as build inputs.
    let tree = capture(root, "git", &["ls-tree", "-r", revision])?;
    ensure(
        !tree
            .lines()
            .any(|s| s.starts_with("120000 ") || s.starts_with("160000 ")),
        "kiri_bundle_source_links_refused",
    )?;
    // Avoid archive rules silently changing the source selected by the revision.
    ensure(
        !tree.lines().any(|s| {
            s.split_once('\t')
                .is_some_and(|(_, p)| p == ".gitattributes" || p.ends_with("/.gitattributes"))
        }),
        "kiri_bundle_archive_attributes_require_review",
    )?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&output)
        .map_err(|_| "kiri_bundle_new_output_required")?;
    fs::write(output.join(".incomplete"), revision).map_err(|_| "kiri_bundle_staging_failed")?;
    let scratch = output.join(".build");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&scratch)
        .map_err(|_| "kiri_bundle_staging_failed")?;
    let source = scratch.join("source");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(scratch.join("docker-config"))
        .map_err(|_| "kiri_bundle_staging_failed")?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&source)
        .map_err(|_| "kiri_bundle_staging_failed")?;
    let archive = scratch.join("source.tar");
    println!("Building public source revision {revision}. Uncommitted files are excluded.");
    run_command(
        root,
        "git",
        &[
            "archive",
            "--format=tar",
            "--output",
            path(&archive)?,
            revision,
        ],
    )?;
    run_command(root, "tar", &["-xf", path(&archive)?, "-C", path(&source)?])?;
    let native = scratch.join("native");
    let export = format!("type=local,dest={}", path(&native)?);
    run_command(
        &source,
        "docker",
        &[
            "build",
            "--file",
            "deployment/kiri/build-installation.Dockerfile",
            "--target",
            "artifacts",
            "--output",
            &export,
            ".",
        ],
    )?;
    for name in ["moosuri", "moosuri-vault-service"] {
        digest(&native.join(name), 256 << 20)?;
        fs::copy(native.join(name), output.join(name)).map_err(|_| "kiri_bundle_copy_failed")?;
        fs::set_permissions(output.join(name), fs::Permissions::from_mode(0o700))
            .map_err(|_| "kiri_bundle_copy_failed")?;
    }
    fs::copy(
        source.join("deployment/browser/seccomp.json"),
        output.join("seccomp.json"),
    )
    .map_err(|_| "kiri_bundle_copy_failed")?;
    // Git archives may retain group-write bits from the source checkout. Bundle
    // admission correctly refuses those, so normalize our newly copied payload.
    fs::set_permissions(
        output.join("seccomp.json"),
        fs::Permissions::from_mode(0o600),
    )
    .map_err(|_| "kiri_bundle_copy_failed")?;
    let mut images = BTreeMap::new();
    for (name, file, target) in [
        ("member", "deployment/kiri/Dockerfile", None),
        ("browser", "deployment/browser/Dockerfile", Some("runtime")),
        (
            "google-auth",
            "deployment/browser/Dockerfile",
            Some("google-auth"),
        ),
    ] {
        let iid = scratch.join(format!("{name}.iid"));
        let mut command = vec!["build", "--file", file, "--iidfile", path(&iid)?];
        if let Some(target) = target {
            command.extend(["--target", target]);
        }
        command.push(".");
        run_command(&source, "docker", &command)?;
        let id = fs::read_to_string(&iid).map_err(|_| "kiri_bundle_image_missing")?;
        let id = capture(
            &source,
            "docker",
            &["image", "inspect", "--format", "{{.Id}}", id.trim()],
        )?;
        let id = id.trim();
        ensure(
            id.strip_prefix("sha256:")
                .is_some_and(|s| s.len() == 64 && s.bytes().all(|c| c.is_ascii_hexdigit())),
            "kiri_bundle_image_invalid",
        )?;
        run_command(
            &source,
            "docker",
            &[
                "save",
                "--output",
                path(&output.join(format!("{name}.tar")))?,
                id,
            ],
        )?;
        let config_id = image_config_id(&source, &output.join(format!("{name}.tar")))?;
        images.insert(name.replace('-', "_"), config_id);
    }
    for name in ["member.tar", "browser.tar", "google-auth.tar"] {
        secure_artifact(&output.join(name), 0o600, 4 << 30, uid)?;
    }
    for name in ["moosuri", "moosuri-vault-service"] {
        secure_artifact(&output.join(name), 0o700, 256 << 20, uid)?;
    }
    secure_artifact(&output.join("seccomp.json"), 0o600, 64 * 1024, uid)?;
    let mut files = BTreeMap::new();
    for name in [
        "moosuri",
        "moosuri-vault-service",
        "member.tar",
        "browser.tar",
        "google-auth.tar",
        "seccomp.json",
    ] {
        files.insert(
            name,
            digest(
                &output.join(name),
                if name.ends_with(".tar") {
                    4 << 30
                } else {
                    256 << 20
                },
            )?,
        );
    }
    let manifest = json!({"format":1,"source_revision":revision,"files":files,"images":images});
    for name in ["LICENSE-APACHE", "LICENSE-MIT"] {
        fs::copy(source.join(name), output.join(name)).map_err(|_| "kiri_bundle_copy_failed")?;
        fs::set_permissions(output.join(name), fs::Permissions::from_mode(0o644))
            .map_err(|_| "kiri_bundle_copy_failed")?;
    }
    // Only our newly created archive/build tree is removed; image cache is retained.
    fs::remove_dir_all(&scratch).map_err(|_| "kiri_bundle_staging_cleanup_failed")?;
    let pending = output.join("manifest.pending");
    let mut file = File::create(&pending).map_err(|_| "kiri_bundle_manifest_write_failed")?;
    file.write_all(
        &serde_json::to_vec_pretty(&manifest).map_err(|_| "kiri_bundle_manifest_invalid")?,
    )
    .and_then(|_| file.sync_all())
    .map_err(|_| "kiri_bundle_manifest_write_failed")?;
    drop(file);
    secure_artifact(&pending, 0o600, 64 * 1024, uid)?;
    fs::rename(&pending, output.join("manifest.json"))
        .map_err(|_| "kiri_bundle_manifest_write_failed")?;
    secure_artifact(&output.join("manifest.json"), 0o600, 64 * 1024, uid)?;
    fs::remove_file(output.join(".incomplete"))
        .map_err(|_| "kiri_bundle_staging_cleanup_failed")?;
    File::open(&output)
        .and_then(|f| f.sync_all())
        .map_err(|_| "kiri_bundle_manifest_write_failed")?;
    println!(
        "Source-built bundle ready: {}. Hashes are not a release signature or host acceptance.",
        output.display()
    );
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;

    #[test]
    fn secure_artifact_sets_modes_and_rejects_unsafe_files() {
        let d = tempfile::tempdir().unwrap();
        let uid = unsafe { libc::geteuid() };
        let f = d.path().join("a.tar");
        fs::write(&f, b"x").unwrap();
        fs::set_permissions(&f, fs::Permissions::from_mode(0o666)).unwrap();
        secure_artifact(&f, 0o600, 10, uid).unwrap();
        assert_eq!(fs::metadata(&f).unwrap().mode() & 0o7777, 0o600);
        assert!(secure_artifact(&f, 0o600, 0, uid).is_err()); // size cap
        assert!(secure_artifact(&f, 0o600, 10, uid.wrapping_add(1)).is_err()); // owner
        let link = d.path().join("l.tar");
        fs::hard_link(&f, &link).unwrap();
        assert!(secure_artifact(&f, 0o600, 10, uid).is_err()); // nlink
        fs::remove_file(&link).unwrap();
        std::os::unix::fs::symlink(&f, &link).unwrap();
        assert!(secure_artifact(&link, 0o600, 10, uid).is_err()); // symlink
        let empty = d.path().join("e");
        fs::write(&empty, b"").unwrap();
        assert!(secure_artifact(&empty, 0o600, 10, uid).is_err());
    }

    #[test]
    fn docker_legacy_and_containerd_archives_use_the_config_digest() {
        let d = tempfile::tempdir().unwrap();
        let root = d.path();
        let content =
            br#"{"os":"linux","architecture":"amd64","rootfs":{"type":"layers","diff_ids":[]}}"#;
        let hash = format!("{:x}", Sha256::digest(content));
        let archive = root.join("image.tar");
        fs::create_dir_all(root.join("blobs/sha256")).unwrap();
        for config in [format!("{hash}.json"), format!("blobs/sha256/{hash}")] {
            fs::write(root.join(&config), content).unwrap();
            fs::write(
                root.join("manifest.json"),
                json!([{"Config":config,"Layers":[]}]).to_string(),
            )
            .unwrap();
            run_command(
                root,
                "tar",
                &["-cf", path(&archive).unwrap(), "manifest.json", &config],
            )
            .unwrap();
            assert_eq!(
                image_config_id(root, &archive).unwrap(),
                format!("sha256:{hash}")
            );
            // The name must identify these exact config bytes, not a manifest/index digest.
            fs::write(root.join(&config), b"{}").unwrap();
            run_command(
                root,
                "tar",
                &["-cf", path(&archive).unwrap(), "manifest.json", &config],
            )
            .unwrap();
            assert!(image_config_id(root, &archive).is_err());
        }
        for invalid in [
            json!([{"Config":"../../host.json"}]),
            json!([{"Config":format!("{hash}.json")},{"Config":format!("{hash}.json")}]),
        ] {
            fs::write(root.join("manifest.json"), invalid.to_string()).unwrap();
            run_command(
                root,
                "tar",
                &["-cf", path(&archive).unwrap(), "manifest.json"],
            )
            .unwrap();
            assert!(image_config_id(root, &archive).is_err());
        }
    }
}
