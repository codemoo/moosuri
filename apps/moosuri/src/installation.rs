//! Signed bundles and stopped forward-schema activation. No service mutation.
mod component;
mod download;
pub mod setup;
use clap::{Subcommand, ValueEnum};
use fs2::FileExt;
use ring::signature;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::CommandExt,
    },
    path::{Component, Path, PathBuf},
    process::Command as Process,
};
#[path = "../../../tools/release/provenance.rs"]
mod release_evidence;
type Result<T> = std::result::Result<T, String>;
const BINARY_CAP: u64 = 256 * 1024 * 1024;
#[derive(Clone, ValueEnum, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Network {
    Local,
    Https,
    Tailscale,
}
#[derive(Subcommand)]
pub enum Command {
    /// Public build metadata only; never opens installation data.
    #[command(hide = true)]
    Probe,
    /// Produce unsigned exact manifest metadata for external Ed25519 signing.
    Manifest {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        sequence: u64,
        #[arg(long)]
        output: PathBuf,
    },
    /// Authenticate a local bundle and its bound evidence without installing or executing it.
    Verify {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        trust_key: PathBuf,
    },
    /// Package, authenticate or download a separate optional native component; never deploy it.
    Component {
        #[command(subcommand)]
        command: component::Command,
    },
    /// Download a pinned signed bundle from an explicit versioned HTTPS mirror; never execute it.
    Download {
        #[arg(long)]
        base_url: String,
        #[arg(long)]
        trust_key: PathBuf,
        #[arg(long)]
        destination: PathBuf,
        /// Optional private PEM CA certificate; normal TLS hostname validation remains required.
        #[arg(long)]
        tls_ca_file: Option<PathBuf>,
    },
    /// Install a local signed bundle into a new private prefix; initialize separately.
    Install {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        trust_key: PathBuf,
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long)]
        data_dir: PathBuf,
        #[arg(long)]
        origin: String,
        #[arg(long, value_enum)]
        network: Network,
    },
    /// Export under the stopped writer lock, migrate forward if needed, then activate.
    Update {
        #[arg(long)]
        bundle: PathBuf,
        #[arg(long)]
        trust_key: PathBuf,
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long)]
        backup_output: PathBuf,
    },
    /// Resolve a durable interrupted activation before starting any managed runtime.
    Recover {
        #[arg(long)]
        prefix: PathBuf,
        #[arg(long, value_enum)]
        choose: Choice,
    },
    /// Remove exact unchanged managed runtime files, retaining data/unrelated files.
    Uninstall {
        #[arg(long)]
        prefix: PathBuf,
    },
    /// Show release/activation state without reading private account data.
    Status {
        #[arg(long)]
        prefix: PathBuf,
    },
    /// Run init/serve/doctor/backup/recover-account using the selected signed binary and fixed data root.
    Run {
        #[arg(long)]
        prefix: PathBuf,
        #[arg(last = true, required = true)]
        args: Vec<String>,
    },
}
#[derive(Clone, ValueEnum)]
pub enum Choice {
    New,
    Old,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct Probe {
    product: String,
    version: String,
    target: String,
    store_schema: i64,
    embedded_web: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: u32,
    product: String,
    version: String,
    target: String,
    sequence: u64,
    store_schema: i64,
    embedded_web: bool,
    binary: String,
    size: u64,
    sha256: String,
    #[serde(
        default,
        deserialize_with = "present_provenance",
        skip_serializing_if = "Option::is_none"
    )]
    provenance: Option<serde_json::Value>,
}
// Missing is the legacy format. Explicit null is a new signed member which
// older strict managers cannot read, and must not silently acquire legacy semantics.
fn present_provenance<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> std::result::Result<Option<serde_json::Value>, D::Error> {
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(serde::de::Error::custom(
            "installation_provenance_null_refused",
        ));
    }
    Ok(Some(value))
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq, Debug)]
#[serde(deny_unknown_fields)]
struct Release {
    sequence: u64,
    version: String,
    manifest_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Installation {
    format: u32,
    id: String,
    data_dir: PathBuf,
    origin: String,
    network: Network,
    trust_key: String,
    managed: BTreeMap<String, String>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Activation {
    from: Release,
    to: Release,
    backup_output: PathBuf,
    export_id: String,
    // Defaults preserve recovery of pre-upgrade same-schema pending records.
    #[serde(default)]
    source_schema: Option<i64>,
    #[serde(default)]
    target_schema: Option<i64>,
    #[serde(default)]
    migration_started: bool,
}
struct Bundle {
    manifest: Manifest,
    bytes: Vec<u8>,
    signature: Vec<u8>,
    key: Vec<u8>,
    binary: Vec<u8>,
    evidence: Vec<(&'static str, Vec<u8>)>,
}
fn error(_: impl std::fmt::Display) -> String {
    "installation_io_failed".into()
}
fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect()
}
fn decode_hex(text: &str) -> Result<Vec<u8>> {
    if text.len() % 2 != 0 || !text.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("installation_invalid_hex".into());
    }
    (0..text.len())
        .step_by(2)
        .map(|n| u8::from_str_radix(&text[n..n + 2], 16).map_err(error))
        .collect()
}
fn key_hex(key: &[u8]) -> String {
    key.iter().map(|v| format!("{v:02x}")).collect()
}
fn json<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    serde_json::to_vec_pretty(v).map_err(error)
}
fn parse<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T> {
    serde_json::from_slice(bytes).map_err(|_| "installation_invalid_metadata".into())
}
// Reuse the broker's descriptor-based fail-closed ACL boundary. Ancestry
// permits only deny entries (the existing skills boundary supports macOS homes);
// private runtime/trust objects reject every explicit or inherited ACL entry.
#[cfg(target_os = "macos")]
fn acl_supported(file: &File, deny_only_ancestry: bool) -> Result<()> {
    use std::os::fd::AsRawFd;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: libc::c_int, kind: libc::c_int) -> *mut libc::c_void;
        fn acl_get_entry(
            acl: *mut libc::c_void,
            id: libc::c_int,
            result: *mut *mut libc::c_void,
        ) -> libc::c_int;
        fn acl_get_tag_type(entry: *mut libc::c_void, tag: *mut libc::c_int) -> libc::c_int;
        fn acl_free(acl: *mut libc::c_void) -> libc::c_int;
    }
    struct Acl(*mut libc::c_void);
    impl Drop for Acl {
        fn drop(&mut self) {
            unsafe {
                acl_free(self.0);
            }
        }
    }
    let raw = unsafe { acl_get_fd_np(file.as_raw_fd(), 0x100) };
    if raw.is_null() {
        return if matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENOENT | libc::ENOTSUP)
        ) {
            Ok(())
        } else {
            Err("installation_acl_inspection_failed".into())
        };
    }
    let acl = Acl(raw);
    for index in 0..=128 {
        let mut entry = std::ptr::null_mut();
        if unsafe { acl_get_entry(acl.0, if index == 0 { 0 } else { -1 }, &mut entry) } != 0 {
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::EINVAL) {
                Ok(())
            } else {
                Err("installation_acl_inspection_failed".into())
            };
        }
        let mut tag = 0;
        if index == 128
            || !deny_only_ancestry
            || unsafe { acl_get_tag_type(entry, &mut tag) } != 0
            || tag != 2
        {
            return Err("installation_acl_refused".into());
        }
    }
    Err("installation_acl_refused".into())
}
#[cfg(target_os = "linux")]
fn acl_supported(file: &File, _deny_only_ancestry: bool) -> Result<()> {
    use std::os::fd::AsRawFd;
    for attribute in [
        c"system.posix_acl_access",
        c"system.posix_acl_default",
        c"system.nfs4_acl",
    ] {
        if unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                attribute.as_ptr(),
                std::ptr::null_mut(),
                0,
            )
        } >= 0
        {
            return Err("installation_acl_refused".into());
        }
        if !matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::ENODATA | libc::ENOTSUP)
        ) {
            return Err("installation_acl_inspection_failed".into());
        }
    }
    Ok(())
}
#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn acl_supported(_file: &File, _deny_only_ancestry: bool) -> Result<()> {
    Err("installation_acl_platform_unsupported".into())
}
fn directory_handle(path: &Path) -> Result<File> {
    OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(error)
}
fn path_checked(path: &Path) -> Result<()> {
    if path
        .as_os_str()
        .as_encoded_bytes()
        .iter()
        .any(|b| b.is_ascii_control())
    {
        return Err("installation_control_character_refused".into());
    }
    if !path.is_absolute() || path.parent().is_none() {
        return Err("installation_absolute_path_required".into());
    }
    let mut cur = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::CurDir | Component::ParentDir) {
            return Err("installation_path_component_refused".into());
        }
        cur.push(part);
        match fs::symlink_metadata(&cur) {
            Ok(info) if info.file_type().is_symlink() => {
                return Err("installation_symlink_refused".into());
            }
            Ok(info) if info.is_dir() => {
                let dir = directory_handle(&cur)?;
                let info = dir.metadata().map_err(error)?;
                if (info.uid() != 0 && info.uid() != unsafe { libc::geteuid() })
                    || (info.mode() & 0o022 != 0 && info.mode() & 0o1000 == 0)
                {
                    return Err("installation_untrusted_ancestor".into());
                }
                acl_supported(&dir, true)?;
            }
            Ok(_) => (),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(e) => return Err(error(e)),
        }
    }
    Ok(())
}
fn private_dir(path: &Path) -> Result<()> {
    path_checked(path)?;
    let dir = directory_handle(path)?;
    let m = dir.metadata().map_err(error)?;
    if !m.is_dir() || m.uid() != unsafe { libc::geteuid() } || m.mode() & 0o077 != 0 {
        return Err("installation_private_directory_required".into());
    }
    acl_supported(&dir, false)
}
fn new_dir(path: &Path) -> Result<()> {
    path_checked(path)?;
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(error)?;
    private_dir(path)?;
    sync(path.parent().ok_or("installation_parent_required")?)
}
fn sync(path: &Path) -> Result<()> {
    File::open(path).and_then(|f| f.sync_all()).map_err(error)
}
fn read(path: &Path, cap: u64, private: bool) -> Result<Vec<u8>> {
    path_checked(path)?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(error)?;
    let m = f.metadata().map_err(error)?;
    if !m.is_file()
        || m.nlink() != 1
        || m.uid() != unsafe { libc::geteuid() }
        || m.len() > cap
        || private && m.mode() & 0o077 != 0
    {
        return Err("installation_regular_bounded_owned_file_required".into());
    }
    acl_supported(&f, false)?;
    let mut bytes = vec![];
    f.take(cap + 1).read_to_end(&mut bytes).map_err(error)?;
    if bytes.len() as u64 > cap {
        return Err("installation_file_limit".into());
    }
    Ok(bytes)
}
fn create(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    path_checked(path)?;
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(if executable { 0o700 } else { 0o600 })
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(error)?;
    acl_supported(&f, false)?;
    f.write_all(bytes)
        .and_then(|()| f.sync_all())
        .map_err(error)
}
fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    atomic_mode(path, bytes, false)
}
fn atomic_mode(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    let parent = path.parent().ok_or("installation_parent_required")?;
    let tmp = parent.join(format!(".activation-{}", uuid::Uuid::new_v4()));
    #[cfg(feature = "test-fixtures")]
    if path.file_name().is_some_and(|name| name == "pending.json")
        && !path.exists()
        && std::env::var("MOOSURI_INSTALLATION_FAULT").ok().as_deref()
            == Some("during_initial_pending_write")
    {
        // Model an interrupted private temporary-file write. The public pending
        // marker must remain absent until its complete durable bytes can rename.
        create(&tmp, &bytes[..bytes.len() / 2], executable)?;
        return Err("installation_test_interrupted_during_initial_pending_write".into());
    }
    create(&tmp, bytes, executable)?;
    fs::rename(&tmp, path).map_err(error)?;
    sync(parent)
}
fn lock(prefix: &Path, exclusive: bool) -> Result<File> {
    private_dir(prefix)?;
    let path = prefix.join("installation.lock");
    let _ = read(&path, 0, true)?;
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)
        .map_err(error)?;
    acl_supported(&f, false)?;
    if exclusive {
        f.try_lock_exclusive()
    } else {
        FileExt::try_lock_shared(&f)
    }
    .map_err(|_| "installation_runtime_busy".to_owned())?;
    Ok(f)
}
fn own_probe() -> Probe {
    Probe {
        product: "moosuri".into(),
        version: env!("CARGO_PKG_VERSION").into(),
        target: env!("MOOSURI_BUILD_TARGET").into(),
        store_schema: moosuri_store::schema_version(),
        embedded_web: super::ASSETS.iter().any(|(name, _)| *name == "/index.html"),
    }
}
fn probe(binary: &Path) -> Result<Probe> {
    let mut child = Process::new(binary)
        .args(["installation", "probe"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(error)?;
    let start = std::time::Instant::now();
    loop {
        if child.try_wait().map_err(error)?.is_some() {
            break;
        }
        if start.elapsed() > std::time::Duration::from_secs(5) {
            let _ = child.kill();
            let _ = child.wait();
            return Err("installation_probe_timeout".into());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    let output = child.wait_with_output().map_err(error)?;
    if !output.status.success() || output.stdout.len() > 4096 {
        return Err("installation_probe_failed".into());
    }
    parse(&output.stdout)
}
fn validate_manifest(m: &Manifest) -> Result<()> {
    let own = own_probe();
    if m.format != 1
        || m.product != "moosuri"
        || m.binary != "moosuri"
        || m.sequence == 0
        || m.size == 0
        || m.size > BINARY_CAP
        || m.sha256.len() != 64
        || decode_hex(&m.sha256)?.len() != 32
        || !m.embedded_web
    {
        return Err("installation_manifest_refused".into());
    }
    if m.target != own.target {
        return Err("installation_target_mismatch".into());
    }
    if m.store_schema < 1 {
        return Err("installation_schema_invalid".into());
    }
    if m.version.len() > 64 || semver::Version::parse(&m.version).is_err() {
        return Err("installation_version_invalid".into());
    }
    if let Some(r) = &m.provenance {
        release_evidence::validate_reference(
            r,
            release_evidence::PROVENANCE,
            release_evidence::PROVENANCE_CAP,
        )
        .map_err(str::to_owned)?;
    }
    Ok(())
}
fn evidence(root: &Path, m: &Manifest, private: bool) -> Result<Vec<(&'static str, Vec<u8>)>> {
    use release_evidence::{INVENTORY, INVENTORY_CAP, PROVENANCE, PROVENANCE_CAP};
    let Some(reference) = &m.provenance else {
        // An unsigned adjacent claim is not release evidence. Reserved sidecars
        // require a signed reference; old ordinary three-file bundles still work.
        for name in [PROVENANCE, INVENTORY] {
            match fs::symlink_metadata(root.join(name)) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
                _ => return Err("installation_unbound_provenance".into()),
            }
        }
        return Ok(vec![]);
    };
    let p = read(&root.join(PROVENANCE), PROVENANCE_CAP, private)?;
    let i = read(&root.join(INVENTORY), INVENTORY_CAP, private)?;
    release_evidence::verify(reference, &p, &i, &serde_json::to_value(m).map_err(error)?)
        .map_err(str::to_owned)?;
    Ok(vec![(PROVENANCE, p), (INVENTORY, i)])
}
fn bundle(root: &Path, key_path: &Path) -> Result<Bundle> {
    private_dir(root)?;
    let keytext = read(key_path, 128, true)?;
    let key = decode_hex(std::str::from_utf8(&keytext).map_err(error)?.trim())?;
    if key.len() != 32 {
        return Err("installation_public_key_length".into());
    }
    let bytes = read(&root.join("manifest.json"), 16384, false)?;
    let sig = read(&root.join("manifest.ed25519"), 64, false)?;
    signature::UnparsedPublicKey::new(&signature::ED25519, &key)
        .verify(&bytes, &sig)
        .map_err(|_| "installation_signature_invalid".to_owned())?;
    let m: Manifest = parse(&bytes)?;
    validate_manifest(&m)?;
    // Capture verified bytes before any installation/update state mutation.
    // Staging consumes this snapshot, not replaceable source paths.
    let binary = read(&root.join("moosuri"), BINARY_CAP, false)?;
    if binary.len() as u64 != m.size || hash(&binary) != m.sha256 {
        return Err("installation_binary_hash_mismatch".into());
    }
    let evidence = evidence(root, &m, false)?;
    Ok(Bundle {
        binary,
        evidence,
        manifest: m,
        bytes,
        signature: sig,
        key,
    })
}
fn stage(prefix: &Path, b: &Bundle) -> Result<Release> {
    let releases = prefix.join("releases");
    private_dir(&releases)?;
    let dir = releases.join(b.manifest.sequence.to_string());
    let binary = &b.binary;
    if binary.len() as u64 != b.manifest.size || hash(binary) != b.manifest.sha256 {
        return Err("installation_binary_hash_mismatch".into());
    }
    if dir.exists() {
        private_dir(&dir)?;
        for entry in fs::read_dir(&dir).map_err(error)? {
            let name = entry.map_err(error)?.file_name();
            if !["moosuri", "manifest.json", "manifest.ed25519"]
                .iter()
                .any(|expected| name == *expected)
                && !b.evidence.iter().any(|(expected, _)| name == *expected)
            {
                return Err("installation_release_unrelated_entry".into());
            }
        }
    } else {
        new_dir(&dir)?;
    }
    // An exact same signed slot can finish staging after a pre-activation crash.
    // Changed files are never overwritten or recursively removed.
    let mut files = vec![
        ("moosuri", binary.as_slice(), true),
        ("manifest.json", b.bytes.as_slice(), false),
        ("manifest.ed25519", b.signature.as_slice(), false),
    ];
    files.extend(
        b.evidence
            .iter()
            .map(|(name, bytes)| (*name, bytes.as_slice(), false)),
    );
    for (name, bytes, executable) in files {
        let path = dir.join(name);
        match fs::symlink_metadata(&path) {
            Ok(_) => {
                if read(&path, BINARY_CAP, true)? != bytes {
                    return Err("installation_staged_file_changed".into());
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => create(&path, bytes, executable)?,
            Err(e) => return Err(error(e)),
        }
    }
    let p = probe(&dir.join("moosuri"))?;
    if p != (Probe {
        product: b.manifest.product.clone(),
        version: b.manifest.version.clone(),
        target: b.manifest.target.clone(),
        store_schema: b.manifest.store_schema,
        embedded_web: true,
    }) {
        return Err("installation_binary_metadata_mismatch".into());
    }
    sync(&dir)?;
    sync(&releases)?;
    Ok(Release {
        sequence: b.manifest.sequence,
        version: b.manifest.version.clone(),
        manifest_sha256: hash(&b.bytes),
    })
}
fn installation(prefix: &Path) -> Result<Installation> {
    private_dir(prefix)?;
    let i: Installation = parse(&read(&prefix.join("installation.json"), 16384, true)?)?;
    if i.format != 1
        || uuid::Uuid::parse_str(&i.id).is_err()
        || decode_hex(&i.trust_key)?.len() != 32
    {
        return Err("installation_marker_invalid".into());
    }
    path_checked(&i.data_dir)?;
    if i.data_dir.starts_with(prefix) || prefix.starts_with(&i.data_dir) {
        return Err("installation_data_runtime_overlap".into());
    }
    Ok(i)
}
fn current(prefix: &Path) -> Result<Release> {
    parse(&read(&prefix.join("current.json"), 4096, true)?)
}
fn verify_release(prefix: &Path, i: &Installation, r: &Release) -> Result<PathBuf> {
    let dir = prefix.join("releases").join(r.sequence.to_string());
    private_dir(&dir)?;
    let bytes = read(&dir.join("manifest.json"), 16384, true)?;
    let sig = read(&dir.join("manifest.ed25519"), 64, true)?;
    let key = decode_hex(&i.trust_key)?;
    signature::UnparsedPublicKey::new(&signature::ED25519, &key)
        .verify(&bytes, &sig)
        .map_err(|_| "installation_signature_invalid".to_owned())?;
    let m: Manifest = parse(&bytes)?;
    validate_manifest(&m)?;
    if r.sequence != m.sequence || r.version != m.version || r.manifest_sha256 != hash(&bytes) {
        return Err("installation_release_record_mismatch".into());
    }
    let binary = dir.join("moosuri");
    let bytes = read(&binary, BINARY_CAP, true)?;
    if bytes.len() as u64 != m.size || hash(&bytes) != m.sha256 {
        return Err("installation_binary_hash_mismatch".into());
    }
    evidence(&dir, &m, true)?;
    Ok(binary)
}
fn pending(prefix: &Path) -> Result<Option<Activation>> {
    match fs::symlink_metadata(prefix.join("pending.json")) {
        Ok(_) => parse(&read(&prefix.join("pending.json"), 16384, true)?).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(error(e)),
    }
}
fn no_pending(prefix: &Path) -> Result<()> {
    if pending(prefix)?.is_some() {
        Err("installation_activation_pending_run_recover".into())
    } else {
        Ok(())
    }
}
fn finish_activation(prefix: &Path, p: &Activation) -> Result<()> {
    let target: Manifest = parse(&read(
        &prefix
            .join("releases")
            .join(p.to.sequence.to_string())
            .join("manifest.json"),
        16384,
        true,
    )?)?;
    if p.source_schema.is_some() && p.source_schema != p.target_schema
        || target.provenance.is_some()
    {
        // Legacy managers reject new signed fields as well as new schemas.
        // Refresh under the pending fence before selecting an evidence-bearing slot.
        let mut i = installation(prefix)?;
        let bytes = read(&verify_release(prefix, &i, &p.to)?, BINARY_CAP, true)?;
        atomic_mode(&prefix.join("manager"), &bytes, true)?;
        upgrade_fault("after_manager")?;
        i.managed.insert("manager".into(), hash(&bytes));
        atomic(&prefix.join("installation.json"), &json(&i)?)?;
    }
    atomic(&prefix.join("current.json"), &json(&p.to)?)?;
    upgrade_fault("after_current")?;
    fs::remove_file(prefix.join("pending.json")).map_err(error)?;
    sync(prefix)
}
fn release_schema(prefix: &Path, r: &Release) -> Result<i64> {
    let m: Manifest = parse(&read(
        &prefix
            .join("releases")
            .join(r.sequence.to_string())
            .join("manifest.json"),
        16384,
        true,
    )?)?;
    Ok(m.store_schema)
}
fn require_target_updater(prefix: &Path, p: &Activation) -> Result<()> {
    let own = own_probe();
    let binary = prefix
        .join("releases")
        .join(p.to.sequence.to_string())
        .join("moosuri");
    if p.target_schema != Some(own.store_schema)
        || hash(&read(
            &std::env::current_exe().map_err(error)?,
            BINARY_CAP,
            false,
        )?) != hash(&read(&binary, BINARY_CAP, true)?)
    {
        return Err("installation_run_signed_target_binary_for_upgrade_or_recover".into());
    }
    Ok(())
}
fn resume_upgrade(
    prefix: &Path,
    p: &mut Activation,
    guard: &moosuri_store::backup::MaintenanceGuard,
) -> Result<()> {
    let source = p
        .source_schema
        .ok_or("installation_upgrade_record_invalid")?;
    let target = p
        .target_schema
        .ok_or("installation_upgrade_record_invalid")?;
    if source >= target
        || guard.schema_version() < source
        || guard.schema_version() > target
        || !moosuri_store::backup::supports_update_from(source)
    {
        return Err("installation_upgrade_schema_mismatch".into());
    }
    require_target_updater(prefix, p)?;
    // Once this durable marker exists, selecting the old executable is refused
    // even if a failed migration happened to leave its first transaction unchanged.
    p.migration_started = true;
    atomic(&prefix.join("pending.json"), &json(p)?)?;
    upgrade_fault("after_migration_started")?;
    drop(
        guard
            .migrate()
            .map_err(|e| format!("installation_upgrade_failed_recover_new: {e}"))?,
    );
    upgrade_fault("after_migration")?;
    finish_activation(prefix, p)
}
fn upgrade_fault(_point: &str) -> Result<()> {
    #[cfg(feature = "test-fixtures")]
    if std::env::var("MOOSURI_INSTALLATION_FAULT").ok().as_deref() == Some(_point) {
        return Err(format!("installation_test_interrupted_{_point}"));
    }
    Ok(())
}
fn maintenance(data: &Path) -> Result<Option<moosuri_store::backup::MaintenanceGuard>> {
    if data.join("assistant.sqlite").exists() {
        moosuri_store::backup::MaintenanceGuard::open(data)
            .map(Some)
            .map_err(|e| e.to_string())
    } else {
        if data.join("writer.lock").exists() {
            return Err("installation_data_incomplete".into());
        }
        Ok(None)
    }
}
fn shell(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}
fn unit(s: &str) -> String {
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('%', "%%")
            .replace('$', "$$")
    )
}
fn templates(prefix: &Path, i: &Installation) -> Result<BTreeMap<String, String>> {
    let manager = prefix.join("manager");
    let prefix = prefix.to_str().ok_or("installation_utf8_path_required")?;
    let manager = manager.to_str().ok_or("installation_utf8_path_required")?;
    let proxy_arguments = match i.network {
        Network::Local => " --dev",
        Network::Https => " --trusted-proxy 127.0.0.1",
        Network::Tailscale => "",
    };
    let bind_arguments = if matches!(i.network, Network::Local) {
        let port = url::Url::parse(&i.origin)
            .map_err(error)?
            .port_or_known_default()
            .ok_or("installation_local_port_required")?;
        format!(" --bind 127.0.0.1:{port}")
    } else {
        String::new()
    };
    let service = format!(
        "[Unit]\nDescription=moosuri private assistant preview\nAfter=network.target\n\n[Service]\nType=simple\nExecStart={} installation run --prefix {} -- serve --origin {}{}{}\nRestart=on-failure\nUMask=0077\nNoNewPrivileges=true\nPrivateTmp=true\nTimeoutStopSec=15\n\n[Install]\nWantedBy=default.target\n",
        unit(manager),
        unit(prefix),
        unit(&i.origin),
        proxy_arguments,
        bind_arguments
    );
    let host = url::Url::parse(&i.origin)
        .map_err(error)?
        .host_str()
        .ok_or("installation_origin_host_required")?
        .to_owned();
    let proxy = format!(
        "# Review and place ONLY in your chosen TLS virtual host for {host}.\n# Configure certificates separately. Do not overwrite existing vhosts.\nlocation / {{\n proxy_pass http://127.0.0.1:3210;\n proxy_set_header Host $http_host;\n proxy_set_header X-Real-IP $remote_addr;\n proxy_buffering off;\n proxy_read_timeout 300s;\n client_max_body_size 32m;\n}}\n"
    );
    let tail = format!(
        "# Review for your own tailnet host matching {}. This file was not executed.\n# Direct Serve mode: choose --network tailscale. The app deliberately ignores\n# forwarded headers; login buckets use loopback peer + normalized account email.\n# Do not add --trusted-proxy or parse a possibly chained X-Forwarded-For value.\n# Serve does not create app accounts; Funnel is never enabled.\ntailscale serve --bg http://127.0.0.1:3210\n# App: {}/app\n# Init first: {} installation run --prefix {} -- init --name Administrator --email admin@example.test\n",
        i.origin,
        i.origin,
        shell(manager),
        shell(prefix)
    );
    Ok(BTreeMap::from([
        ("deployment/moosuri.service".into(), service),
        ("deployment/proxy.nginx.conf".into(), proxy),
        ("deployment/tailscale-serve.txt".into(), tail),
    ]))
}
pub fn run(command: &Command) -> Result<()> {
    match command {
        Command::Component { command } => component::run(command)?,
        Command::Probe => println!("{}", serde_json::to_string(&own_probe()).map_err(error)?),
        Command::Manifest {
            binary,
            sequence,
            output,
        } => {
            path_checked(binary)?;
            let p = probe(binary)?;
            if p.product != "moosuri" || !p.embedded_web || *sequence == 0 {
                return Err("installation_embedded_web_required".into());
            }
            let bytes = read(binary, BINARY_CAP, false)?;
            let m = Manifest {
                format: 1,
                product: p.product,
                version: p.version,
                target: p.target,
                sequence: *sequence,
                store_schema: p.store_schema,
                embedded_web: p.embedded_web,
                binary: "moosuri".into(),
                size: bytes.len() as u64,
                sha256: hash(&bytes),
                provenance: None,
            };
            create(output, &json(&m)?, false)?;
            sync(output.parent().ok_or("installation_parent_required")?)?;
            println!(
                "Unsigned manifest created. Sign its exact bytes externally with your explicitly pinned Ed25519 key; no official release trust was created."
            );
        }
        Command::Download {
            base_url,
            trust_key,
            destination,
            tls_ca_file,
        } => {
            download::run(base_url, trust_key, destination, tls_ca_file.as_deref())?;
            println!("Authenticated bundle downloaded. No bundle code was executed or installed.");
        }
        Command::Verify {
            bundle: root,
            trust_key,
        } => {
            let verified = bundle(root, trust_key)?;
            println!(
                "Pinned signature and binary verified. Bound provenance: {}. No bundle code was executed or installation data opened. Provenance metadata remains a packager claim.",
                if verified.manifest.provenance.is_some() {
                    "verified"
                } else {
                    "absent (legacy format1)"
                }
            );
        }
        Command::Install {
            bundle: root,
            trust_key,
            prefix,
            data_dir,
            origin,
            network,
        } => {
            let b = bundle(root, trust_key)?;
            if b.manifest.store_schema != own_probe().store_schema {
                return Err("installation_run_target_binary_required".into());
            }
            path_checked(prefix)?;
            path_checked(data_dir)?;
            if prefix.starts_with(data_dir) || data_dir.starts_with(prefix) {
                return Err("installation_data_runtime_overlap".into());
            }
            if data_dir.exists() {
                private_dir(data_dir)?
            }
            if origin.bytes().any(|b| b.is_ascii_control()) {
                return Err("installation_control_character_refused".into());
            }
            let u = url::Url::parse(origin).map_err(error)?;
            let local = matches!(network, Network::Local);
            if moosuri_server::ServerConfig::new(origin, local).is_err()
                || (local
                    && (u.scheme() != "http" || u.port_or_known_default().is_none_or(|p| p < 1024)))
                || (!local && u.scheme() != "https")
                || u.username() != ""
                || u.password().is_some()
                || u.query().is_some()
                || u.fragment().is_some()
                || u.path() != "/"
                || u.host_str().is_none()
            {
                return Err("installation_origin_network_mismatch".into());
            }
            new_dir(prefix)?;
            create(&prefix.join("installation.lock"), b"", false)?;
            let _lock = lock(prefix, true)?;
            new_dir(&prefix.join("releases"))?;
            new_dir(&prefix.join("deployment"))?;
            let r = stage(prefix, &b)?;
            let binary = prefix
                .join("releases")
                .join(r.sequence.to_string())
                .join("moosuri");
            create(
                &prefix.join("manager"),
                &read(&binary, BINARY_CAP, true)?,
                true,
            )?;
            let mut i = Installation {
                format: 1,
                id: uuid::Uuid::new_v4().to_string(),
                data_dir: data_dir.clone(),
                origin: u.origin().ascii_serialization(),
                network: network.clone(),
                trust_key: key_hex(&b.key),
                managed: BTreeMap::new(),
            };
            i.managed.insert(
                "manager".into(),
                hash(&read(&prefix.join("manager"), BINARY_CAP, true)?),
            );
            for (path, text) in templates(prefix, &i)? {
                create(&prefix.join(&path), text.as_bytes(), false)?;
                i.managed.insert(path, hash(text.as_bytes()));
            }
            create(&prefix.join("current.json"), &json(&r)?, false)?;
            create(&prefix.join("installation.json"), &json(&i)?, false)?;
            sync(&prefix.join("deployment"))?;
            sync(prefix)?;
            println!(
                "Verified native preview installed. Data is separate and unchanged. Run manager installation run --prefix <this-prefix> -- init, then review deployment files. No service/network setting was changed."
            );
        }
        Command::Update {
            bundle: root,
            trust_key,
            prefix,
            backup_output,
        } => {
            let _lock = lock(prefix, true)?;
            let i = installation(prefix)?;
            no_pending(prefix)?;
            let from = current(prefix)?;
            verify_release(prefix, &i, &from)?;
            let b = bundle(root, trust_key)?;
            if key_hex(&b.key) != i.trust_key {
                return Err("installation_trust_pin_changed".into());
            }
            if b.manifest.sequence <= from.sequence
                || semver::Version::parse(&b.manifest.version).map_err(error)?
                    < semver::Version::parse(&from.version).map_err(error)?
            {
                return Err("installation_release_not_newer".into());
            }
            if b.manifest.store_schema != own_probe().store_schema {
                return Err("installation_run_signed_target_binary_for_upgrade_or_recover".into());
            }
            let guard = moosuri_store::backup::MaintenanceGuard::open_for_upgrade(&i.data_dir)
                .map_err(|e| e.to_string())?;
            let source_schema = guard.schema_version();
            if source_schema != release_schema(prefix, &from)?
                || source_schema > b.manifest.store_schema
            {
                return Err("installation_upgrade_schema_mismatch".into());
            }
            let to = stage(prefix, &b)?;
            let mut p = Activation {
                from,
                to,
                backup_output: backup_output.clone(),
                export_id: String::new(),
                source_schema: Some(source_schema),
                target_schema: Some(b.manifest.store_schema),
                migration_started: false,
            };
            let upgrading = source_schema != b.manifest.store_schema;
            if upgrading {
                require_target_updater(prefix, &p)?;
            }
            let export = guard
                .encrypted_export(backup_output)
                .map_err(|e| e.to_string())?;
            p.export_id = export.export_id;
            // Exclusive prefix admission makes this absent-check authoritative;
            // never expose a partial initial pending record or overwrite one.
            no_pending(prefix)?;
            atomic(&prefix.join("pending.json"), &json(&p)?)?;
            if upgrading {
                resume_upgrade(prefix, &mut p, &guard)?;
            } else {
                finish_activation(prefix, &p)?;
            }
            println!(
                "Verified binary activated under the stopped writer lock (schema {source_schema} -> {}). Encrypted source export retained; no database rollback, service start or traffic change was performed.",
                b.manifest.store_schema
            );
        }
        Command::Recover { prefix, choose } => {
            let _lock = lock(prefix, true)?;
            let i = installation(prefix)?;
            let mut p = pending(prefix)?.ok_or("installation_no_pending_activation")?;
            let guard = moosuri_store::backup::MaintenanceGuard::open_for_upgrade(&i.data_dir)
                .map_err(|e| e.to_string())?;
            let active = current(prefix)?;
            if active != p.from && active != p.to {
                return Err("installation_activation_record_conflict".into());
            }
            verify_release(prefix, &i, &p.from)?;
            verify_release(prefix, &i, &p.to)?;
            let from_schema = release_schema(prefix, &p.from)?;
            let to_schema = release_schema(prefix, &p.to)?;
            let upgrading = from_schema != to_schema;
            if upgrading {
                if p.source_schema != Some(from_schema) || p.target_schema != Some(to_schema) {
                    return Err("installation_upgrade_record_invalid".into());
                }
                match choose {
                    Choice::Old => {
                        if p.migration_started || guard.schema_version() != from_schema {
                            return Err("installation_upgrade_started_recover_new_only_no_database_rollback".into());
                        }
                        atomic(&prefix.join("current.json"), &json(&p.from)?)?;
                        fs::remove_file(prefix.join("pending.json")).map_err(error)?;
                        sync(prefix)?;
                    }
                    Choice::New => resume_upgrade(prefix, &mut p, &guard)?,
                }
            } else {
                if p.migration_started || guard.schema_version() != from_schema {
                    return Err("installation_upgrade_schema_mismatch".into());
                }
                match choose {
                    Choice::New => finish_activation(prefix, &p)?,
                    Choice::Old => {
                        atomic(&prefix.join("current.json"), &json(&p.from)?)?;
                        fs::remove_file(prefix.join("pending.json")).map_err(error)?;
                        sync(prefix)?;
                    }
                }
            }
            println!(
                "Pending activation resolved under stopped admission. SQLite was never rolled back. Review status before explicitly starting the service."
            );
        }
        Command::Uninstall { prefix } => {
            let _lock = lock(prefix, true)?;
            let i = installation(prefix)?;
            no_pending(prefix)?;
            let _data = maintenance(&i.data_dir)?;
            uninstall(prefix, &i)?;
        }
        Command::Status { prefix } => {
            let _lock = lock(prefix, false)?;
            let i = installation(prefix)?;
            let r = current(prefix)?;
            verify_release(prefix, &i, &r)?;
            println!(
                "Version {} (release {}). Pending activation: {}. Data is retained at {}. No service/network state was inspected.",
                r.version,
                r.sequence,
                pending(prefix)?.is_some(),
                i.data_dir.display()
            );
        }
        Command::Run { prefix, args } => {
            setup::check_start_allowed(prefix)?;
            let _lock = lock(prefix, false)?;
            let i = installation(prefix)?;
            no_pending(prefix)?;
            if !matches!(
                args.first().map(String::as_str),
                Some(
                    "init" | "serve" | "doctor" | "backup" | "recover-account" | "recovery-budget"
                )
            ) || args
                .iter()
                .any(|s| s == "--data-dir" || s.starts_with("--data-dir="))
            {
                return Err("installation_supported_command_required".into());
            }
            let r = current(prefix)?;
            let binary = verify_release(prefix, &i, &r)?;
            // Keep the runtime-prefix shared lock across exec and the entire server lifetime.
            let fd = std::os::fd::AsRawFd::as_raw_fd(&_lock);
            let flags = unsafe { libc::fcntl(fd, libc::F_GETFD) };
            if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFD, flags & !libc::FD_CLOEXEC) } < 0
            {
                return Err("installation_lock_inheritance_failed".into());
            }
            return Err(error(
                Process::new(binary)
                    .arg("--data-dir")
                    .arg(&i.data_dir)
                    .args(args)
                    .exec(),
            ));
        }
    }
    Ok(())
}
fn remove_exact(path: &Path, digest: &str) -> Result<bool> {
    match read(path, BINARY_CAP, true) {
        Ok(bytes) if hash(&bytes) == digest => {
            fs::remove_file(path).map_err(error)?;
            Ok(true)
        }
        _ => Ok(false),
    }
}
fn uninstall(prefix: &Path, i: &Installation) -> Result<()> {
    let mut kept = 0;
    for (path, digest) in &i.managed {
        if !matches!(
            path.as_str(),
            "manager"
                | "deployment/moosuri.service"
                | "deployment/proxy.nginx.conf"
                | "deployment/tailscale-serve.txt"
        ) {
            return Err("installation_managed_path_refused".into());
        }
        if !remove_exact(&prefix.join(path), digest)? {
            kept += 1
        }
    }
    let releases = prefix.join("releases");
    private_dir(&releases)?;
    for entry in fs::read_dir(&releases).map_err(error)? {
        let entry = entry.map_err(error)?;
        let name = entry.file_name();
        let Some(n) = name.to_str().and_then(|s| s.parse::<u64>().ok()) else {
            kept += 1;
            continue;
        };
        let path = entry.path();
        let bytes = match read(&path.join("manifest.json"), 16384, true) {
            Ok(v) => v,
            Err(_) => {
                kept += 1;
                continue;
            }
        };
        let m: Manifest = match parse(&bytes) {
            Ok(v) => v,
            Err(_) => {
                kept += 1;
                continue;
            }
        };
        let r = Release {
            sequence: n,
            version: m.version.clone(),
            manifest_sha256: hash(&bytes),
        };
        if verify_release(prefix, i, &r).is_err() {
            kept += 1;
            continue;
        }
        let mut files = vec![
            ("moosuri", BINARY_CAP),
            ("manifest.json", 16384),
            ("manifest.ed25519", 64),
        ];
        if m.provenance.is_some() {
            files.extend([
                (
                    release_evidence::PROVENANCE,
                    release_evidence::PROVENANCE_CAP,
                ),
                (release_evidence::INVENTORY, release_evidence::INVENTORY_CAP),
            ]);
        }
        for (file, cap) in files {
            let digest = hash(&read(&path.join(file), cap, true)?);
            remove_exact(&path.join(file), &digest)?;
        }
        if fs::remove_dir(path).is_err() {
            kept += 1
        }
    }
    for file in ["current.json", "installation.json"] {
        let bytes = read(&prefix.join(file), 16384, true)?;
        remove_exact(&prefix.join(file), &hash(&bytes))?;
    }
    // Never recursively delete a prefix, data directory or unrelated entry.
    for dir in ["deployment", "releases"] {
        let _ = fs::remove_dir(prefix.join(dir));
    }
    let _ = fs::remove_file(prefix.join("installation.lock"));
    sync(prefix)?;
    println!(
        "Managed runtime removed; private data retained at {}. Unrelated/changed entries retained: {kept}. Service/Tailscale/DNS configuration was not changed.",
        i.data_dir.display()
    );
    Ok(())
}
/// Direct execution from a managed release also respects pending activation and
/// holds shared admission, so it cannot race the updater before Store opens.
pub fn guard_managed_executable() -> Result<Option<File>> {
    let exe = std::env::current_exe().map_err(error)?;
    let parent = exe.parent().ok_or("installation_executable_parent")?;
    let prefix = if parent.join("installation.json").exists() {
        Some(parent.to_path_buf())
    } else {
        parent
            .parent()
            .filter(|p| p.file_name().is_some_and(|n| n == "releases"))
            .and_then(Path::parent)
            .map(Path::to_path_buf)
            .filter(|p| p.join("installation.json").exists())
    };
    let Some(prefix) = prefix else {
        return Ok(None);
    };
    let l = lock(&prefix, false)?;
    no_pending(&prefix)?;
    setup::check_start_allowed(&prefix)?;
    let _ = installation(&prefix)?;
    Ok(Some(l))
}
#[cfg(test)]
mod tests;
