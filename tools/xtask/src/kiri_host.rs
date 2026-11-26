//! Host packages only. Product state and administrator enrollment stay in setup-kiri.
use super::{Check, ensure};
use sha2::{Digest, Sha256};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::Path,
    process::{Command, Stdio},
};

const KIRI_URL: &str =
    "https://github.com/libracoder/kiri/releases/download/v0.2.2/kiri-lite_0.2.2_amd64.deb";
const KIRI_SHA256: &str = "24798125ee50bb24db86594a9ee1469b1032d9b2ffd7da616b06e40763c723b9";

fn run(binary: &str, args: &[&str]) -> Check<()> {
    let result = Command::new(binary)
        .args(args)
        .env("DEBIAN_FRONTEND", "noninteractive")
        .stdin(Stdio::null())
        .status()
        .map_err(|_| format!("kiri_host_{binary}_unavailable"))?;
    ensure(result.success(), format!("kiri_host_{binary}_failed"))
}
fn capture(binary: &str, args: &[&str]) -> Check<String> {
    let result = Command::new(binary)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|_| format!("kiri_host_{binary}_unavailable"))?;
    ensure(
        result.status.success(),
        format!("kiri_host_{binary}_failed"),
    )?;
    String::from_utf8(result.stdout).map_err(|_| "kiri_host_output_invalid".into())
}

/// Kiri's supported domain API persists its loopback-only router ports. The
/// default 80/443 race nginx on reboot. Changing ports requires an empty engine;
/// reapplying the same ports is idempotent and never restarts a container.
fn configure_domain_ports() -> Check<()> {
    let api = |write: bool| -> Check<serde_json::Value> {
        let mut args = vec![
            "--fail",
            "--silent",
            "--show-error",
            "--max-time",
            "10",
            "--unix-socket",
            "/run/kiri/docker.sock",
        ];
        if !write {
            // systemd may report active before the engine socket is accepting.
            args.extend([
                "--retry",
                "20",
                // curl's connrefused retry excludes an absent Unix socket.
                "--retry-all-errors",
                "--retry-delay",
                "1",
                "--retry-max-time",
                "30",
            ]);
        }
        if write {
            args.extend([
                "--header",
                "Content-Type: application/json",
                "--data",
                r#"{"http":18780,"https":18743}"#,
            ]);
        }
        args.push("http://localhost/kiri/domains");
        serde_json::from_str(&capture("curl", &args)?)
            .map_err(|_| "kiri_host_domain_reply_invalid".into())
    };
    let matches = |v: &serde_json::Value| v["http"]["port"] == 18780 && v["https"]["port"] == 18743;
    if !matches(&api(false)?) {
        ensure(
            capture(
                "docker",
                &[
                    "--host=unix:///run/kiri/docker.sock",
                    "container",
                    "ls",
                    "--all",
                    "--quiet",
                ],
            )?
            .trim()
            .is_empty(),
            "kiri_host_existing_containers_require_operator_setup",
        )?;
    }
    // Even after a lost response, replaying the same ports finishes persistence.
    ensure(matches(&api(true)?), "kiri_host_domain_ports_not_applied")
}

pub fn run_host(args: &[String]) -> Check<()> {
    ensure(args.is_empty(), "usage: kiri-host")?;
    ensure(
        cfg!(all(target_os = "linux", target_arch = "x86_64")) && unsafe { libc::geteuid() } == 0,
        "kiri_host_root_linux_amd64_required",
    )?;
    let os = fs::read_to_string("/etc/os-release").map_err(|_| "kiri_host_os_unknown")?;
    let get = |key: &str| {
        os.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|s| s.trim_matches('"'))
    };
    ensure(
        get("ID=") == Some("ubuntu")
            && get("VERSION_ID=") == Some("24.04")
            && Path::new("/run/systemd/system").is_dir(),
        "kiri_host_ubuntu_24_04_systemd_required",
    )?;
    // Refuse to replace a different engine installation. An absent engine is the normal path.
    if Path::new("/usr/bin/kiri").exists() || Path::new("/usr/local/bin/kiri").exists() {
        ensure(
            capture("dpkg-query", &["-W", "-f=${Version}", "kiri-lite"])?.trim() == "0.2.2",
            "kiri_host_existing_engine_version_conflict",
        )?;
    }
    run("apt-get", &["update"])?;
    run(
        "apt-get",
        &[
            "install",
            "-y",
            "--no-install-recommends",
            "ca-certificates",
            "curl",
            "docker.io",
            "docker-buildx",
            "docker-compose-v2",
            "certbot",
            "e2fsprogs",
            "iproute2",
            "iptables",
            "apparmor",
            "apparmor-utils",
            "util-linux",
            "openssl",
        ],
    )?;
    let dir = Path::new("/var/cache/moosuri-kiri-package");
    if dir.exists() {
        let m = fs::symlink_metadata(dir).map_err(|_| "kiri_host_package_path_invalid")?;
        ensure(
            m.is_dir() && m.uid() == 0 && m.mode() & 0o777 == 0o700,
            "kiri_host_package_path_invalid",
        )?;
    } else {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .map_err(|_| "kiri_host_package_path_invalid")?;
    }
    let package = dir.join("kiri-lite_0.2.2_amd64.deb");
    let valid = |p: &Path| -> Check<bool> {
        let m = fs::symlink_metadata(p).map_err(|_| "kiri_host_package_missing")?;
        ensure(
            m.is_file()
                && m.uid() == 0
                && m.nlink() == 1
                && m.mode() & 0o022 == 0
                && m.len() <= 256 << 20,
            "kiri_host_package_invalid",
        )?;
        let bytes = fs::read(p).map_err(|_| "kiri_host_package_unreadable")?;
        Ok(format!("{:x}", Sha256::digest(bytes)) == KIRI_SHA256)
    };
    let exists = fs::symlink_metadata(&package).is_ok();
    if exists && !valid(&package)? {
        fs::remove_file(&package).map_err(|_| "kiri_host_package_recovery_failed")?;
    }
    if !package
        .try_exists()
        .map_err(|_| "kiri_host_package_path_invalid")?
    {
        let pending = dir.join(format!("download-{}.part", std::process::id()));
        let _guard = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&pending)
            .map_err(|_| "kiri_host_package_download_conflict")?;
        run(
            "curl",
            &[
                "--fail",
                "--location",
                "--silent",
                "--show-error",
                "--retry",
                "2",
                "--proto",
                "=https",
                "--proto-redir",
                "=https",
                "--max-filesize",
                "268435456",
                "--output",
                pending.to_str().unwrap(),
                KIRI_URL,
            ],
        )?;
        ensure(valid(&pending)?, "kiri_host_package_checksum_failed")?;
        fs::rename(pending, &package).map_err(|_| "kiri_host_package_download_failed")?;
    }
    ensure(valid(&package)?, "kiri_host_package_checksum_failed")?;
    run("apt-get", &["install", "-y", package.to_str().unwrap()])?;
    ensure(
        capture("dpkg-query", &["-W", "-f=${Version}", "kiri-lite"])?.trim() == "0.2.2",
        "kiri_host_engine_version_mismatch",
    )?;
    // Ubuntu ships the Chrome executable's user-namespace admission profile.
    // Use it instead of installing a second attachment for the same path.
    ensure(
        Path::new("/etc/apparmor.d/chrome").is_file(),
        "kiri_host_chrome_apparmor_profile_missing",
    )?;
    run("apparmor_parser", &["-r", "/etc/apparmor.d/chrome"])?;
    run(
        "systemctl",
        &["enable", "--now", "docker.service", "kiri.service"],
    )?;
    configure_domain_ports()?;
    // apt may start nginx immediately; install it after Kiri has released 80/443.
    run(
        "apt-get",
        &["install", "-y", "--no-install-recommends", "nginx"],
    )?;
    run(
        "systemctl",
        &[
            "enable",
            "--now",
            "docker.service",
            "kiri.service",
            "nginx.service",
        ],
    )?;
    for unit in ["docker.service", "kiri.service", "nginx.service"] {
        run("systemctl", &["is-active", "--quiet", unit])?;
    }
    println!(
        "Host dependencies ready: Ubuntu 24.04, Kiri 0.2.2, Docker, nginx and certificate tooling."
    );
    Ok(())
}
