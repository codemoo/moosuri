//! `moosuri setup-kiri`: clean Ubuntu 24.04 amd64 installation of the member Kiri
//! runtime, gateway, separate-origin vault and Google browser transport from a
//! private source-built artifact bundle chosen by root. It reuses the existing
//! registry, lifecycle, invitation, storage and installation-admin commands;
//! it adds no account or configuration store of its own. The receipt records
//! identities and hashes only, never a password or key.
//!
//! Not in this unit: creating a new Google OAuth application.
mod browser;
mod bundle;
mod host;
mod render;

use clap::Args;
use host::{Exec, Input, Layout, Result, System, args};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{BufRead, Read, Write},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
use zeroize::Zeroizing;

const RECEIPT: &str = "/etc/moosuri/kiri-install.json";
const KEY: &str = "/etc/moosuri/vault-adapter.key";
const BIN: &str = "/usr/local/bin/moosuri";
const VAULT_BIN: &str = "/usr/local/bin/moosuri-vault-service";
const MEMBERS: &str = "/etc/moosuri/kiri-members.json";
const QUICK: Duration = Duration::from_secs(60);
/// Everything first-member onboarding waits for. The model service starts first
/// (with no provider configured) so every member socket exists before any
/// member container is provisioned.
const START_BEFORE_ONBOARD: [&str; 4] = [
    "moosuri-kiri-model.service",
    "moosuri-kiri-gws.service",
    "moosuri-kiri-browser.service",
    "moosuri-google-browser.socket",
];

#[derive(Args)]
pub struct Options {
    /// Private root-owned directory containing manifest.json and the six artifacts.
    #[arg(long)]
    pub bundle: PathBuf,
    /// Assistant HTTPS origin, for example https://assistant.example.com
    #[arg(long)]
    pub public_origin: String,
    /// Different vault HTTPS origin, for example https://vault.example.com
    #[arg(long)]
    pub vault_origin: String,
    /// First administrator email (prompted when omitted and standard input is free).
    #[arg(long)]
    pub admin_email: Option<String>,
    #[arg(long)]
    pub admin_name: Option<String>,
    /// Dedicated member runtimes this installation may allocate.
    #[arg(long, default_value_t = 4, value_parser = clap::value_parser!(u32).range(1..=64))]
    pub member_capacity: u32,
    /// Maximum persistent volume for each member, in GiB.
    #[arg(long, default_value_t = 10, value_parser = clap::value_parser!(u64).range(1..=1024))]
    pub member_volume_gib: u64,
    /// Read one password line from standard input (synthetic tests); otherwise prompt hidden.
    #[arg(long)]
    pub password_stdin: bool,
    /// Operator-supplied certificate chain; requires --tls-key.
    #[arg(long, requires = "tls_key", conflicts_with = "certbot_email")]
    pub tls_certificate: Option<PathBuf>,
    #[arg(long, requires = "tls_certificate")]
    pub tls_key: Option<PathBuf>,
    /// Obtain one certificate for exactly the two selected names with certbot.
    #[arg(long, requires = "accept_certbot_terms")]
    pub certbot_email: Option<String>,
    /// Confirm that you accept the certificate authority subscriber terms.
    #[arg(long)]
    pub accept_certbot_terms: bool,
}

#[derive(Serialize, Deserialize, Clone, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Receipt {
    format: u32,
    installation_id: Uuid,
    request_id: Uuid,
    admin_email: String,
    admin_name: String,
    public_origin: String,
    vault_origin: String,
    source_revision: String,
    manifest_sha256: String,
    member_capacity: u32,
    member_volume_bytes: u64,
    uid_pool: Vec<u32>,
    tls: String,
    proxy_gid: u32,
    browser: browser::NetPlan,
    google: browser::NetPlan,
    complete: bool,
}
impl Receipt {
    /// A separate, stable operation identity for first-admin promotion. The
    /// registry deliberately refuses reusing a member-allocation request here.
    /// Deriving from the durable receipt also resumes older incomplete installs.
    fn admin_request(&self) -> Uuid {
        use sha2::{Digest, Sha256};
        let mut digest = Sha256::new();
        digest.update(b"moosuri:kiri-install:first-admin:v1\0");
        digest.update(self.installation_id.as_bytes());
        digest.update(self.request_id.as_bytes());
        let mut bytes = [0; 16];
        bytes.copy_from_slice(&digest.finalize()[..16]);
        uuid::Builder::from_random_bytes(bytes).into_uuid()
    }
    /// Operator inputs that must not change on a resumed installation.
    fn inputs(&self) -> (String, &str, &str, &str, &str, u32, u64, &str) {
        (
            self.admin_email.to_ascii_lowercase(),
            &self.admin_name,
            &self.public_origin,
            &self.vault_origin,
            &self.manifest_sha256,
            self.member_capacity,
            self.member_volume_bytes,
            &self.tls,
        )
    }
    fn tls(&self) -> render::Tls {
        match self
            .tls
            .strip_prefix("supplied:")
            .and_then(|p| p.split_once('|'))
        {
            Some((c, k)) => render::Tls::Supplied {
                certificate: c.into(),
                key: k.into(),
            },
            None => render::Tls::Certbot {
                name: format!(
                    "moosuri-kiri-{}",
                    &self.installation_id.simple().to_string()[..12]
                ),
            },
        }
    }
    fn profile(&self, image: &str) -> Vec<(&'static str, Vec<u8>)> {
        let p = render::Profile {
            public_origin: &self.public_origin,
            vault_origin: &self.vault_origin,
            member_image: image,
            volume_bytes: self.member_volume_bytes,
            uid_pool: &self.uid_pool,
            capacity: self.member_capacity,
        };
        vec![
            ("kiri-members.json", render::lifecycle(&p)),
            ("kiri-onboard-policy.json", render::policy(&p)),
            ("public-origin.env", render::origin_env(&self.public_origin)),
        ]
    }
}

fn random_uuid() -> Uuid {
    let mut b = [0u8; 16];
    rand::rngs::OsRng.fill_bytes(&mut b);
    uuid::Builder::from_random_bytes(b).into_uuid()
}

pub fn parse_os_release(text: &str) -> Result<()> {
    let value = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().trim_matches('"').to_owned())
    };
    if value("ID=").as_deref() == Some("ubuntu") && value("VERSION_ID=").as_deref() == Some("24.04")
    {
        Ok(())
    } else {
        Err("kiri_install_requires_ubuntu_24_04".into())
    }
}
fn require_host() -> Result<()> {
    if !cfg!(target_os = "linux") || !cfg!(target_arch = "x86_64") {
        return Err("kiri_install_requires_ubuntu_24_04_amd64".into());
    }
    if host::euid() != 0 {
        return Err("kiri_install_root_required".into());
    }
    let release = fs::read_to_string("/etc/os-release").map_err(|_| "kiri_install_os_unknown")?;
    parse_os_release(&release)?;
    if !Path::new("/run/systemd/system").is_dir() {
        return Err("kiri_install_systemd_required".into());
    }
    Ok(())
}

/// Group named by nginx's `user` directive; never root.
pub fn nginx_group(conf: &str, groups: &str) -> Result<u32> {
    let line = conf
        .lines()
        .map(str::trim)
        .find(|l| l.starts_with("user ") || l.starts_with("user\t"))
        .ok_or("kiri_install_nginx_user_unknown")?;
    let words: Vec<_> = line.trim_end_matches(';').split_whitespace().collect();
    let group: &str = words
        .get(2)
        .or(words.get(1))
        .copied()
        .ok_or("kiri_install_nginx_user_unknown")?;
    let gid = groups
        .lines()
        .filter_map(|l| {
            let mut f = l.split(':');
            (f.next() == Some(group)).then(|| f.nth(1).and_then(|g| g.parse::<u32>().ok()))
        })
        .flatten()
        .next()
        .ok_or("kiri_install_nginx_group_unknown")?;
    if gid == 0 {
        return Err("kiri_install_nginx_group_unknown".into());
    }
    Ok(gid)
}
pub fn pick_uids(count: u32, free: impl Fn(u32) -> bool) -> Result<Vec<u32>> {
    let pool: Vec<u32> = (20000u32..=60000)
        .filter(|u| free(*u))
        .take(count as usize)
        .collect();
    if pool.len() != count as usize {
        return Err("kiri_install_no_free_member_uids".into());
    }
    Ok(pool)
}
fn valid_email(e: &str) -> bool {
    !e.starts_with('-')
        && e.len() <= 254
        && e.trim() == e
        && !e.chars().any(|c| c.is_control() || c.is_whitespace())
        && e.split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.contains('@'))
}
fn valid_name(n: &str) -> bool {
    !n.is_empty()
        && !n.starts_with('-')
        && n.chars().count() <= 100
        && n.trim() == n
        && !n.chars().any(char::is_control)
}
fn prompt(label: &str) -> Result<String> {
    print!("{label}");
    std::io::stdout()
        .flush()
        .map_err(|_| "kiri_install_prompt_failed")?;
    let mut line = String::new();
    std::io::stdin()
        .lock()
        .take(1024)
        .read_line(&mut line)
        .map_err(|_| "kiri_install_prompt_failed")?;
    Ok(line.trim().to_owned())
}
fn identity(o: &Options) -> Result<(String, String)> {
    let ask = |given: &Option<String>, label: &str| -> Result<String> {
        match given {
            Some(v) => Ok(v.trim().to_owned()),
            // Standard input carries the password in this mode.
            None if o.password_stdin => Err("kiri_install_admin_identity_required".into()),
            None => prompt(label),
        }
    };
    let email = ask(&o.admin_email, "Administrator email / 관리자 이메일: ")?;
    let name = ask(&o.admin_name, "Administrator name / 관리자 이름: ")?;
    if !valid_email(&email) || !valid_name(&name) {
        return Err("kiri_install_admin_identity_invalid".into());
    }
    Ok((email, name))
}
fn password(o: &Options) -> Result<Zeroizing<String>> {
    let secret = if o.password_stdin {
        let mut input = Zeroizing::new(String::new());
        std::io::stdin()
            .lock()
            .take(259)
            .read_to_string(&mut input)
            .map_err(|_| "kiri_install_password_unavailable")?;
        let line = input
            .strip_suffix('\n')
            .filter(|s| !s.contains(['\r', '\n']) && s.len() <= 256)
            .ok_or("kiri_install_expected_one_password_line")?;
        Zeroizing::new(line.to_owned())
    } else {
        let first = Zeroizing::new(
            rpassword::prompt_password("Administrator password / 관리자 비밀번호: ")
                .map_err(|_| "kiri_install_password_unavailable")?,
        );
        let again = Zeroizing::new(
            rpassword::prompt_password("Repeat password / 비밀번호 확인: ")
                .map_err(|_| "kiri_install_password_unavailable")?,
        );
        if *first != *again {
            return Err("kiri_install_passwords_differ".into());
        }
        first
    };
    if !moosuri_core::account_password_valid(&secret) {
        return Err("kiri_install_password_does_not_meet_account_policy".into());
    }
    Ok(secret)
}

fn unit_paths(layout: &Layout) -> Vec<PathBuf> {
    render::UNITS
        .iter()
        .flat_map(|(n, _)| {
            [
                layout.at(&format!("{}/{n}", render::SYSTEMD)),
                layout.at(&format!("/usr/lib/systemd/system/{n}")),
            ]
        })
        .collect()
}
/// A new installation owns no path; an existing one is never adopted.
fn require_fresh(layout: &Layout) -> Result<()> {
    let etc = layout.at(render::ETC);
    if layout.exists(&etc)? {
        let empty = fs::symlink_metadata(&etc).is_ok_and(|m| m.is_dir())
            && fs::read_dir(&etc).is_ok_and(|mut d| d.next().is_none());
        if !empty {
            return Err(format!("kiri_install_conflicting_path:{}", render::ETC));
        }
    }
    let mut paths: Vec<PathBuf> = [
        render::CONTROL,
        render::STORAGE,
        render::ACME_ROOT,
        render::NGINX_CONF,
        BIN,
        VAULT_BIN,
        "/var/lib/moosuri-vault",
        "/var/lib/moosuri-kiri-browser",
        "/var/lib/moosuri-kiri-lifecycle",
        render::MODEL_SOCKETS,
        "/run/moosuri-kiri-google",
        "/run/moosuri-google-browser",
        "/run/moosuri-member-proxy",
    ]
    .iter()
    .map(|p| layout.at(p))
    .collect();
    paths.extend(unit_paths(layout));
    for p in paths {
        if layout.exists(&p)? {
            return Err(format!("kiri_install_conflicting_path:{}", p.display()));
        }
    }
    Ok(())
}
fn load_receipt(layout: &Layout) -> Result<Option<(Receipt, Vec<u8>)>> {
    let path = layout.at(RECEIPT);
    if !layout.exists(&path)? {
        return Ok(None);
    }
    let (mut file, _) = host::open_regular(&path, 64 * 1024)?;
    let mut bytes = Vec::new();
    (&mut file)
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|_| "kiri_install_receipt_unreadable")?;
    let receipt: Receipt =
        serde_json::from_slice(&bytes).map_err(|_| "kiri_install_receipt_invalid")?;
    if receipt.format != 1 || receipt.installation_id.is_nil() || receipt.request_id.is_nil() {
        return Err("kiri_install_receipt_invalid".into());
    }
    Ok(Some((receipt, bytes)))
}
fn save_receipt(layout: &Layout, receipt: &Receipt, previous: Option<&[u8]>) -> Result<()> {
    let mut bytes =
        serde_json::to_vec_pretty(receipt).map_err(|_| "kiri_install_receipt_invalid")?;
    bytes.push(b'\n');
    let old: Vec<String> = previous.map(host::sha256_bytes).into_iter().collect();
    layout.write_file(&layout.at(RECEIPT), &bytes, 0o600, &old)
}

/// Directories, binaries, configuration and units. Idempotent for the same receipt.
fn lay_down(
    layout: &Layout,
    bundle: &bundle::Bundle,
    r: &Receipt,
    capsules: Option<(&[u8], &[u8])>,
) -> Result<()> {
    for (path, mode) in [
        (render::ETC, 0o700),
        (render::CONTROL, 0o700),
        ("/var/lib/moosuri-control/registry", 0o700),
        ("/var/lib/moosuri-control/gateway", 0o700),
        ("/var/lib/moosuri-control/invitations", 0o700),
        ("/var/lib/moosuri-control/enrolled", 0o700),
        // Dedicated shared model ledger. The sockets directory is the model
        // unit's StateDirectory and is created by systemd at first start.
        (render::MODEL_LEDGER, 0o700),
        // Private model setup state (provider config and key), outside the
        // strict registry directory so registry operations never see it.
        (render::MODEL_SETUP_DIR, 0o700),
    ] {
        layout.ensure_dir(&layout.at(path), mode)?;
    }
    // The storage command later initializes and re-modes this root; only create it.
    let storage = layout.at(render::STORAGE);
    if !layout.exists(&storage)? {
        layout.ensure_dir(&storage, 0o700)?;
    }
    bundle.install_file(layout, "moosuri", &layout.at(BIN), 0o755)?;
    bundle.install_file(
        layout,
        "moosuri-vault-service",
        &layout.at(VAULT_BIN),
        0o755,
    )?;
    bundle.install_file(
        layout,
        "seccomp.json",
        &layout.at("/etc/moosuri/seccomp.json"),
        0o644,
    )?;
    ensure_adapter_key(layout)?;
    ensure_model_key(layout)?;
    for (name, bytes) in r.profile(&bundle.images.member) {
        layout.write_file(
            &layout.at(&format!("{}/{name}", render::ETC)),
            &bytes,
            0o600,
            &[],
        )?;
    }
    if let Some((browser, google)) = capsules {
        layout.write_file(&layout.at("/etc/moosuri/browser.json"), browser, 0o600, &[])?;
        layout.write_file(
            &layout.at("/etc/moosuri/google-browser.json"),
            google,
            0o600,
            &[],
        )?;
    }
    let id = r.installation_id.to_string();
    let values = render::Values {
        public_origin: &r.public_origin,
        vault_origin: &r.vault_origin,
        installation_id: &id,
        proxy_gid: r.proxy_gid,
    };
    for (name, template) in render::UNITS {
        let text = render::unit(template, &values)?;
        layout.write_file(
            &layout.at(&format!("{}/{name}", render::SYSTEMD)),
            text.as_bytes(),
            0o644,
            &[],
        )?;
    }
    Ok(())
}
/// Random 64-hex adapter authority key, mode 0400. An existing valid key is kept.
fn ensure_adapter_key(layout: &Layout) -> Result<()> {
    let path = layout.at(KEY);
    if layout.exists(&path)? {
        let (mut file, len) = host::open_regular(&path, 65)?;
        let m = file
            .metadata()
            .map_err(|_| "kiri_install_key_unavailable")?;
        let mut text = String::new();
        (&mut file)
            .read_to_string(&mut text)
            .map_err(|_| "kiri_install_key_unavailable")?;
        if m.mode() & 0o777 != 0o400 || !host::is_hex(text.trim_end(), 64) || len > 65 {
            return Err("kiri_install_adapter_key_invalid".into());
        }
        return Ok(());
    }
    let mut raw = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut raw[..]);
    let text = Zeroizing::new(format!("{}\n", host::hex(&raw[..])));
    layout.write_file(&path, text.as_bytes(), 0o400, &[])
}

/// Random raw 32-byte fingerprint key for the model broker, mode 0400. It keys
/// the durable ledger's request fingerprints, so an existing valid key is always
/// kept on resume or reboot (rotation would orphan receipts); anything else at
/// the path is a conflict. No provider credential is created here.
fn ensure_model_key(layout: &Layout) -> Result<()> {
    let path = layout.at(render::MODEL_KEY);
    if layout.exists(&path)? {
        fn invalid<E>(_: E) -> String {
            "kiri_install_model_key_invalid".into()
        }
        let (mut file, len) = host::open_regular(&path, 33).map_err(invalid)?;
        let m = file.metadata().map_err(invalid)?;
        let mut raw = Zeroizing::new(Vec::new());
        (&mut file)
            .take(33)
            .read_to_end(&mut raw)
            .map_err(invalid)?;
        if m.mode() & 0o777 != 0o400 || len != 32 || raw.len() != 32 {
            return Err("kiri_install_model_key_invalid".into());
        }
        return Ok(());
    }
    let mut raw = Zeroizing::new([0u8; 32]);
    rand::rngs::OsRng.fill_bytes(&mut raw[..]);
    layout.write_file(&path, &raw[..], 0o400, &[])
}

fn systemctl(exec: &dyn Exec, items: &[&str], reason: &str) -> Result<()> {
    exec.run("systemctl", &args(items), Input::None, QUICK)?
        .ok(reason)
        .map(|_| ())
}
fn start(exec: &dyn Exec, unit: &str) -> Result<()> {
    let reason = format!(
        "kiri_install_unit_failed_{}",
        unit.replace(['.', '-', '@'], "_")
    );
    systemctl(exec, &["enable", "--now", unit], &reason)?;
    systemctl(exec, &["is-active", "--quiet", unit], &reason)
}

/// A transient systemd active state does not prove the gateway can serve HTTP.
/// Check both public routes through the installed TLS proxy before completion.
fn web_ready(exec: &dyn Exec, r: &Receipt) -> Result<()> {
    let (cert, _) = r.tls().paths();
    for origin in [&r.public_origin, &r.vault_origin] {
        let host = render::host_of(origin)?;
        let resolve = format!("{host}:443:127.0.0.1");
        let url = format!("{origin}/");
        exec.run(
            "curl",
            &args(&[
                "--fail",
                "--silent",
                "--show-error",
                "--output",
                "/dev/null",
                "--noproxy",
                "*",
                "--resolve",
                &resolve,
                "--cacert",
                &cert,
                "--connect-timeout",
                "3",
                "--max-time",
                "5",
                "--retry",
                "5",
                "--retry-delay",
                "1",
                "--retry-max-time",
                "25",
                "--retry-all-errors",
                &url,
            ]),
            Input::None,
            Duration::from_secs(35),
        )?
        .ok("kiri_install_web_not_ready")?;
    }
    Ok(())
}

fn reload_nginx(exec: &dyn Exec) -> Result<()> {
    exec.run("nginx", &args(&["-t"]), Input::None, QUICK)?
        .ok("kiri_install_nginx_config_rejected")?;
    systemctl(
        exec,
        &["reload-or-restart", "nginx"],
        "kiri_install_nginx_reload_failed",
    )
}
/// Refuse a name already served by another nginx file.
fn nginx_conflicts(layout: &Layout, hosts: &[&str]) -> Result<()> {
    let ours = layout.at(render::NGINX_CONF);
    let mut files = vec![layout.at("/etc/nginx/nginx.conf")];
    for dir in ["/etc/nginx/conf.d", "/etc/nginx/sites-enabled"] {
        if let Ok(read) = fs::read_dir(layout.at(dir)) {
            files.extend(read.flatten().map(|e| e.path()));
        }
    }
    for file in files.into_iter().filter(|f| *f != ours) {
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        // Conservative text scan, including one-line and commented declarations.
        for declaration in text.split("server_name").skip(1) {
            let names = declaration.split(';').next().unwrap_or("");
            if names.split_whitespace().any(|name| hosts.contains(&name)) {
                return Err("kiri_install_nginx_server_name_conflict".into());
            }
        }
    }
    Ok(())
}
fn remove_owned(layout: &Layout, path: &Path, hashes: &[String]) {
    let _ = layout.safe_ancestors(path);
    if host::sha256_file(path, 1 << 20).is_ok_and(|h| hashes.contains(&h)) {
        let _ = fs::remove_file(path);
    }
}
fn validate_supplied(layout: &Layout, cert: &Path, key: &Path) -> Result<()> {
    for (path, cap, secret) in [(cert, 256 * 1024u64, false), (key, 64 * 1024, true)] {
        // Strict allowlist: the path is later written into nginx configuration
        // and the receipt, so whitespace, '#', quotes and separators are refused.
        if !path.is_absolute()
            || fs::canonicalize(path).ok().as_deref() != Some(path)
            || path.to_str().is_none_or(|s| {
                s.len() > 512
                    || !s
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"/._-".contains(&b))
            })
        {
            return Err("kiri_install_tls_path_invalid".into());
        }
        layout.safe_ancestors(path)?;
        let (mut file, _) = host::open_regular(path, cap)?;
        let m = file
            .metadata()
            .map_err(|_| "kiri_install_tls_unavailable")?;
        let mut head = Zeroizing::new(String::new());
        (&mut file)
            .take(cap)
            .read_to_string(&mut head)
            .map_err(|_| "kiri_install_tls_unavailable")?;
        if (secret && m.mode() & 0o077 != 0)
            || !head.starts_with("-----BEGIN ")
            || (!secret && !head.starts_with("-----BEGIN CERTIFICATE-----"))
            // Passphrase-protected keys cannot be used by unattended nginx.
            || (secret && head.contains("ENCRYPTED"))
        {
            return Err("kiri_install_tls_file_invalid".into());
        }
    }
    Ok(())
}
/// A valid PEM and matching private key alone do not make HTTPS usable. Check
/// the actual leaf certificate's lifetime and both selected DNS names before
/// installing the nginx configuration. Pairing is checked separately; this does
/// not assert trust by any external CA.
fn validate_certificate(exec: &dyn Exec, cert: &Path, public: &str, vault: &str) -> Result<()> {
    let (a, v) = render::origins(public, vault)?;
    let cert = cert.to_str().ok_or("kiri_install_tls_path_invalid")?;
    for host in [&a, &v] {
        exec.run(
            "openssl",
            // x509 -checkhost prints a mismatch but exits zero in OpenSSL 3.
            // verify has a failure exit status and also checks validity dates.
            // Trust only the supplied leaf for this local consistency check.
            &args(&[
                "verify",
                "-trusted",
                cert,
                "-partial_chain",
                "-no-CAfile",
                "-no-CApath",
                "-no-CAstore",
                "-purpose",
                "sslserver",
                "-verify_hostname",
                host,
                cert,
            ]),
            Input::None,
            Duration::from_secs(10),
        )?
        .ok("kiri_install_certificate_expired_or_wrong_host")?;
    }
    Ok(())
}
/// Hosts, dates and certificate/private-key pairing for an operator-supplied
/// pair. Fixed openssl arguments, null stdin (no prompt); only public key text
/// is captured and the private key is never printed. An encrypted key fails
/// because no passphrase is available.
fn validate_tls_material(
    exec: &dyn Exec,
    cert: &Path,
    key: &Path,
    public: &str,
    vault: &str,
) -> Result<()> {
    validate_certificate(exec, cert, public, vault)?;
    let cert = cert.to_str().ok_or("kiri_install_tls_path_invalid")?;
    let key = key.to_str().ok_or("kiri_install_tls_path_invalid")?;
    let limit = Duration::from_secs(10);
    let from_cert = exec
        .run(
            "openssl",
            &args(&["x509", "-in", cert, "-noout", "-pubkey"]),
            Input::None,
            limit,
        )?
        .ok("kiri_install_tls_certificate_invalid")?;
    let from_key = exec
        .run(
            "openssl",
            &args(&["pkey", "-in", key, "-pubout", "-passin", "pass:"]),
            Input::None,
            limit,
        )?
        .ok("kiri_install_tls_key_invalid_or_encrypted")?;
    if from_cert.trim_ascii().is_empty() || from_cert.trim_ascii() != from_key.trim_ascii() {
        return Err("kiri_install_tls_key_mismatch".into());
    }
    Ok(())
}
fn configure_tls(
    layout: &Layout,
    exec: &dyn Exec,
    r: &Receipt,
    certbot_email: Option<&str>,
) -> Result<()> {
    let (a, v) = render::origins(&r.public_origin, &r.vault_origin)?;
    nginx_conflicts(layout, &[&a, &v])?;
    let tls = r.tls();
    let conf = layout.at(render::NGINX_CONF);
    let final_text = render::nginx_final(&r.public_origin, &r.vault_origin, &tls)?;
    let final_hash = host::sha256_bytes(final_text.as_bytes());
    let challenge = render::nginx_challenge(&r.public_origin, &r.vault_origin)?;
    let challenge_hash = host::sha256_bytes(challenge.as_bytes());
    let both = [final_hash.clone(), challenge_hash.clone()];
    let apply = |text: &str, previous: &[String]| -> Result<()> {
        layout.write_file(&conf, text.as_bytes(), 0o644, previous)?;
        reload_nginx(exec).inspect_err(|_| remove_owned(layout, &conf, &both))
    };
    let issued = match &tls {
        render::Tls::Certbot { name } => {
            layout.exists(&layout.at(&format!("/etc/letsencrypt/live/{name}/fullchain.pem")))?
        }
        render::Tls::Supplied { .. } => true,
    };
    if let (render::Tls::Certbot { name }, false) = (&tls, issued) {
        let email = certbot_email.ok_or("kiri_install_certbot_email_required")?;
        layout.ensure_dir(&layout.at(render::ACME_ROOT), 0o755)?;
        apply(&challenge, std::slice::from_ref(&final_hash))?;
        exec.run(
            "certbot",
            &args(&[
                "certonly",
                "--webroot",
                "-w",
                render::ACME_ROOT,
                "--cert-name",
                name,
                "-d",
                &a,
                "-d",
                &v,
                "--non-interactive",
                "--agree-tos",
                "-m",
                email,
                "--keep-until-expiring",
                "--deploy-hook",
                "systemctl reload nginx",
            ]),
            Input::None,
            Duration::from_secs(15 * 60),
        )?
        .ok("kiri_install_certbot_failed")?;
    }
    // Revalidate immediately before nginx sees the files (also covers certbot output).
    let (cert_path, key_path) = tls.paths();
    validate_tls_material(
        exec,
        Path::new(&cert_path),
        Path::new(&key_path),
        &r.public_origin,
        &r.vault_origin,
    )?;
    apply(&final_text, std::slice::from_ref(&challenge_hash))
}

fn onboard(exec: &dyn Exec, r: &Receipt, secret: &Zeroizing<String>) -> Result<()> {
    let uid = r
        .uid_pool
        .first()
        .ok_or("kiri_install_receipt_invalid")?
        .to_string();
    let request = r.request_id.to_string();
    for attempt in 0..6 {
        let mut input = Zeroizing::new(secret.as_bytes().to_vec());
        input.push(b'\n');
        let out = exec.run(
            BIN,
            &args(&[
                "kiri-onboard",
                "--experimental",
                "--config",
                MEMBERS,
                "--request-id",
                &request,
                "--first-name",
                &r.admin_name,
                "--first-email",
                &r.admin_email,
                "--uid",
                &uid,
                "--operations-root",
                "/var/lib/moosuri-control/invitations",
                "--enable-local-gws",
                "--enable-text-preview",
                "--password-stdin",
            ]),
            Input::Secret(input),
            Duration::from_secs(30 * 60),
        )?;
        if out.code == 0 {
            let stage = out
                .stdout
                .split(|b| *b == b'\n')
                .rev()
                .find_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
                .ok_or("kiri_install_first_member_reply_invalid")?;
            if stage["ready"] == true || stage["stage"] == "ready" {
                return Ok(());
            }
            match stage["stage"].as_str() {
                Some("service_waiting" | "initialization_waiting") if attempt < 5 => {
                    std::thread::sleep(Duration::from_secs(5));
                    continue;
                }
                Some("preparation_required") => {
                    return Err("kiri_install_first_member_preparation_required".into());
                }
                Some("password_required") => {
                    return Err("kiri_install_first_member_password_required".into());
                }
                _ => return Err("kiri_install_first_member_not_ready".into()),
            }
        }
        let reason = out.reason();
        if reason.contains("retry_same_invitation") && attempt < 5 {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        return Err(if reason.is_empty() {
            "kiri_install_first_member_failed".into()
        } else {
            format!("kiri_install_first_member_{reason}")
        });
    }
    Err("kiri_install_first_member_failed".into())
}
async fn complete_admin(r: &Receipt, secret: &Zeroizing<String>) -> Result<()> {
    use moosuri_store::runtime_registry::{self, admin};
    let registry = Path::new("/var/lib/moosuri-control/registry");
    if let Ok(a) = runtime_registry::lookup(registry, r.admin_email.clone())
        && let Ok(id) = Uuid::parse_str(&a.account_id)
        && admin::status(registry, id).is_ok_and(|m| m.role == "admin")
    {
        return Ok(());
    }
    let uid = *r.uid_pool.first().ok_or("kiri_install_receipt_invalid")?;
    let mut last = String::new();
    for _ in 0..10 {
        match crate::installation_admin::bootstrap_adopting_password(
            registry,
            r.admin_request(),
            &r.admin_email,
            Path::new("/srv/moosuri-storage/ipc"),
            uid,
            Zeroizing::new(secret.to_string()),
        )
        .await
        {
            Ok(_) => return Ok(()),
            // Only service startup is retriable. Authorization, malformed
            // replies and request conflicts must retain their original cause.
            Err(e)
                if matches!(
                    e.as_str(),
                    "allocation_not_ready"
                        | "invalid_member_socket"
                        | "member_unavailable"
                        | "member_login_unavailable"
                ) =>
            {
                last = e
            }
            Err(e) => return Err(format!("kiri_install_admin_{e}")),
        }
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    Err(format!("kiri_install_admin_not_ready_{last}"))
}

fn plan_fresh(
    layout: &Layout,
    exec: &dyn Exec,
    docker: &browser::Docker,
    o: &Options,
    bundle: &bundle::Bundle,
    who: (String, String),
) -> Result<Receipt> {
    require_fresh(layout)?;
    // Operator TLS input is fully validated before any inventory, receipt or resource.
    let tls = match (&o.tls_certificate, &o.tls_key) {
        (Some(c), Some(k)) => {
            validate_supplied(layout, c, k)?;
            validate_tls_material(exec, c, k, &o.public_origin, &o.vault_origin)?;
            format!("supplied:{}|{}", c.display(), k.display())
        }
        _ => {
            let email = o
                .certbot_email
                .as_deref()
                .ok_or("kiri_install_tls_choice_required")?;
            if !valid_email(email) {
                return Err("kiri_install_certbot_email_invalid".into());
            }
            "certbot".into()
        }
    };
    let id = random_uuid();
    let mut used = browser::host_networks(&String::from_utf8_lossy(
        &exec
            .run("ip", &args(&["-4", "route", "show"]), Input::None, QUICK)?
            .ok("kiri_install_network_inventory_failed")?,
    ));
    used.extend(browser::host_networks(&String::from_utf8_lossy(
        &exec
            .run(
                "ip",
                &args(&["-4", "-o", "addr", "show"]),
                Input::None,
                QUICK,
            )?
            .ok("kiri_install_network_inventory_failed")?,
    )));
    used.extend(docker.used_subnets()?);
    let browser_plan = browser::plan(&used, &[], format!("moosuri-browser-{id}"))?;
    let google_plan = browser::plan(
        &used,
        std::slice::from_ref(&browser_plan),
        format!("moosuri-google-{id}"),
    )?;
    let conf = fs::read_to_string(layout.at("/etc/nginx/nginx.conf"))
        .map_err(|_| "kiri_install_nginx_missing")?;
    let groups = fs::read_to_string(layout.at("/etc/group"))
        .map_err(|_| "kiri_install_nginx_group_unknown")?;
    Ok(Receipt {
        format: 1,
        installation_id: id,
        request_id: random_uuid(),
        admin_email: who.0,
        admin_name: who.1,
        public_origin: o.public_origin.clone(),
        vault_origin: o.vault_origin.clone(),
        source_revision: bundle.source_revision.clone(),
        manifest_sha256: bundle.manifest_sha256.clone(),
        member_capacity: o.member_capacity,
        member_volume_bytes: o.member_volume_gib << 30,
        uid_pool: pick_uids(o.member_capacity, |u| {
            crate::kiri_storage::unused_uid(u).is_ok()
        })?,
        tls,
        proxy_gid: nginx_group(&conf, &groups)?,
        browser: browser_plan,
        google: google_plan,
        complete: false,
    })
}

pub async fn run(o: &Options) -> std::result::Result<(), String> {
    require_host()?;
    let layout = Layout::system();
    render::origins(&o.public_origin, &o.vault_origin)?;
    if o.tls_certificate.is_none() && o.certbot_email.is_none() {
        return Err("kiri_install_tls_choice_required".into());
    }
    let bundle = bundle::validate(&layout, &o.bundle)?;
    bundle.require_running(
        &std::env::current_exe().map_err(|_| "kiri_install_executable_unknown")?,
    )?;
    let who = identity(o)?;
    let secret = password(o)?;
    let docker_config = tempfile::Builder::new()
        .prefix("moosuri-kiri-docker-")
        .tempdir_in("/run")
        .map_err(|_| "kiri_install_runtime_directory_unavailable")?;
    let exec = System {
        docker_config: docker_config.path().into(),
    };
    for tool in [
        "docker",
        "ip",
        "nginx",
        "systemctl",
        "curl",
        "mkfs.ext4",
        "blkid",
        "openssl",
    ] {
        host::find_tool(tool)?;
    }
    if o.certbot_email.is_some() {
        host::find_tool("certbot")?;
    }
    let docker = browser::Docker { exec: &exec };
    docker.require_kiri()?;

    let (receipt, previous) = match load_receipt(&layout)? {
        Some((existing, bytes)) => {
            let probe = plan_inputs(&existing, o, &bundle, &who);
            if probe.inputs() != existing.inputs()
                || existing.source_revision != bundle.source_revision
            {
                return Err("kiri_install_conflicting_existing_installation".into());
            }
            if existing.complete {
                web_ready(&exec, &existing)?;
                println!("This Kiri installation is already complete; nothing was changed.");
                return Ok(());
            }
            (existing, Some(bytes))
        }
        None => {
            let fresh = plan_fresh(&layout, &exec, &docker, o, &bundle, who)?;
            layout.ensure_dir(&layout.at(render::ETC), 0o700)?;
            save_receipt(&layout, &fresh, None)?;
            (fresh, None)
        }
    };
    let owner = receipt.installation_id.to_string();
    if let render::Tls::Supplied { certificate, key } = receipt.tls() {
        validate_supplied(&layout, &certificate, &key)?;
        validate_tls_material(
            &exec,
            &certificate,
            &key,
            &receipt.public_origin,
            &receipt.vault_origin,
        )?;
    }

    // Nothing below executes the bundle's own payloads except through fixed commands.
    for (file, id) in [
        ("member.tar", &bundle.images.member),
        ("browser.tar", &bundle.images.browser),
        ("google-auth.tar", &bundle.images.google_auth),
    ] {
        docker.ensure_image(&bundle.path(file), id)?;
        if host::sha256_file(&bundle.path(file), bundle::Bundle::cap(file))? != bundle.hash(file) {
            return Err("kiri_install_artifact_changed_during_install".into());
        }
    }
    let (browser_network, browser_proxy) =
        docker.capsule(&bundle.images.browser, &receipt.browser, &owner)?;
    let (google_network, google_proxy) =
        docker.capsule(&bundle.images.google_auth, &receipt.google, &owner)?;
    let capsule = |image: &str, plan: &browser::NetPlan, network: &str, proxy: &str| {
        browser::capsule_json(&browser::Capsule {
            image_id: image,
            network_id: network,
            proxy_container_id: proxy,
            plan,
            owner: &owner,
        })
    };
    let browser_json = capsule(
        &bundle.images.browser,
        &receipt.browser,
        &browser_network,
        &browser_proxy,
    );
    let google_json = capsule(
        &bundle.images.google_auth,
        &receipt.google,
        &google_network,
        &google_proxy,
    );
    // Install the trusted seccomp profile before the real browser startup probes.
    // Capsule configs are published only when both engines and owned cleanup pass.
    lay_down(&layout, &bundle, &receipt, None)?;
    let probe = |bytes: Vec<u8>, google: bool| async move {
        use moosuri_connectors::browser::capsule::{CapsuleConfig, CapsuleReader};
        let config: CapsuleConfig = serde_json::from_slice(&bytes)
            .map_err(|_| "kiri_install_capsule_invalid".to_owned())?;
        CapsuleReader::verify_installation(
            config,
            google,
            &moosuri_connectors::model::CancellationToken::new(),
        )
        .await
        .map_err(|_| {
            if google {
                "kiri_install_google_browser_probe_failed".to_owned()
            } else {
                "kiri_install_browser_probe_failed".to_owned()
            }
        })
    };
    let (browser_config, browser_evidence) = probe(browser_json, false).await?;
    let (google_config, google_evidence) = probe(google_json, true).await?;
    let browser_json =
        serde_json::to_vec_pretty(&browser_config).map_err(|_| "kiri_install_capsule_invalid")?;
    let google_json =
        serde_json::to_vec_pretty(&google_config).map_err(|_| "kiri_install_capsule_invalid")?;
    for (name, evidence) in [
        ("browser-smoke.json", browser_evidence),
        ("google-browser-smoke.json", google_evidence),
    ] {
        layout.write_file(
            &layout.at(&format!("/etc/moosuri/{name}")),
            &evidence,
            0o600,
            &[],
        )?;
    }
    lay_down(
        &layout,
        &bundle,
        &receipt,
        Some((&browser_json, &google_json)),
    )?;

    systemctl(
        &exec,
        &["daemon-reload"],
        "kiri_install_daemon_reload_failed",
    )?;
    for unit in START_BEFORE_ONBOARD {
        start(&exec, unit)?;
    }
    onboard(&exec, &receipt, &secret)?;
    start(&exec, "moosuri-kiri-lifecycle.service")?;
    complete_admin(&receipt, &secret).await?;
    for unit in ["moosuri-kiri-gateway.service", "moosuri-kiri-vault.service"] {
        start(&exec, unit)?;
    }
    configure_tls(&layout, &exec, &receipt, o.certbot_email.as_deref())?;
    start(&exec, "moosuri-kiri-onboard.timer")?;
    web_ready(&exec, &receipt)?;

    let mut done = receipt.clone();
    done.complete = true;
    let current = match previous {
        Some(bytes) => bytes,
        None => load_receipt(&layout)?.map(|(_, b)| b).unwrap_or_default(),
    };
    save_receipt(&layout, &done, Some(&current))?;
    println!("Moosuri Kiri is installed.");
    println!("Assistant: {}", receipt.public_origin);
    println!("Vault: {}", receipt.vault_origin);
    println!(
        "Sign in as {} with the password you entered; it was not stored.",
        receipt.admin_email
    );
    println!("Google OAuth application setup is a separate next step.");
    Ok(())
}
/// Re-derive the operator inputs of an invocation for comparison with the receipt.
fn plan_inputs(
    existing: &Receipt,
    o: &Options,
    bundle: &bundle::Bundle,
    who: &(String, String),
) -> Receipt {
    let mut probe = existing.clone();
    probe.admin_email.clone_from(&who.0);
    probe.admin_name.clone_from(&who.1);
    probe.public_origin.clone_from(&o.public_origin);
    probe.vault_origin.clone_from(&o.vault_origin);
    probe.manifest_sha256.clone_from(&bundle.manifest_sha256);
    probe.member_capacity = o.member_capacity;
    probe.member_volume_bytes = o.member_volume_gib << 30;
    probe.tls = match (&o.tls_certificate, &o.tls_key) {
        (Some(c), Some(k)) => format!("supplied:{}|{}", c.display(), k.display()),
        _ => "certbot".into(),
    };
    probe
}

#[cfg(test)]
mod tests;
