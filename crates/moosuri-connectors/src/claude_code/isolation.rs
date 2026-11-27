//! Credential-free native admission of the exact production launch. It observes
//! confinement and an unchanged CLI version, never authenticates or infers.
use super::*;
use std::{fs, io, net::SocketAddr, os::unix::fs::symlink};

/// Public, credential-free evidence plus the exact profile admitted by it.
/// Callers persist this only after rechecking their current owner/request pin.
#[derive(Serialize)]
pub struct IsolationAdmission {
    pub profile_json: String,
    pub profile_digest: String,
    pub receipt: IsolationReceipt,
}

struct UnlaunchedEnvironment(Option<PathBuf>);
impl Drop for UnlaunchedEnvironment {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = fs::remove_dir_all(path);
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Challenge {
    nonce: String,
    inaccessible: Vec<PathBuf>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IsolationReceipt {
    version: u32,
    nonce: String,
    candidate_profile_digest: String,
    memory_bytes: u64,
    cpu_quota: u64,
    cpu_period: u64,
    pids: u64,
    auth_filesystem_bytes: u64,
    auth_filesystem_inodes: u64,
    rootless_uid_namespace: bool,
    no_new_privileges: bool,
    seccomp: bool,
    capabilities_dropped: bool,
    host_and_sibling_inaccessible: bool,
    immutable_mounts: bool,
    no_external_interface_or_route: bool,
    direct_network_denied: bool,
    proxy_destination_denied: bool,
    official_cli_version: String,
}
impl IsolationReceipt {
    fn accepted(&self, nonce: &str) -> bool {
        self.version == 1
            && self.nonce == nonce
            && self.memory_bytes == 805306368
            && self.cpu_quota == 100000
            && self.cpu_period == 100000
            && self.pids == 32
            && self.auth_filesystem_bytes > 0
            && self.auth_filesystem_bytes <= 128 * 1024 * 1024
            && self.auth_filesystem_inodes > 0
            && self.auth_filesystem_inodes <= 32768
            && self.rootless_uid_namespace
            && self.no_new_privileges
            && self.seccomp
            && self.capabilities_dropped
            && self.host_and_sibling_inaccessible
            && self.immutable_mounts
            && self.no_external_interface_or_route
            && self.direct_network_denied
            && self.proxy_destination_denied
            && self.official_cli_version == "2.1.322"
    }
}

impl Profile {
    /// The preparation input must explicitly have no prior acceptance. Neither
    /// login nor inference is exposed until the actual native probe succeeds.
    pub async fn admit_json(
        raw: &str,
        cancel: &CancellationToken,
    ) -> Result<IsolationAdmission, TransportError> {
        if raw.len() > 16384 || cancel.is_cancelled() {
            return Err(err());
        }
        let mut config: Config = serde_json::from_str(raw).map_err(|_| err())?;
        if !config.isolation_acceptance_sha256.is_empty() {
            return Err(err());
        }
        // Bind the probe to all prospective launch inputs without a circular
        // dependency on the receipt produced by that probe.
        config.isolation_acceptance_sha256 = hex(Sha256::digest(
            serde_json::to_vec(&config).map_err(|_| err())?,
        ));
        let candidate = Self::from_json(&serde_json::to_string(&config).map_err(|_| err())?)?;
        let receipt = candidate.probe_isolation(cancel).await?;
        if cancel.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        config.isolation_acceptance_sha256 = hex(Sha256::digest(
            serde_json::to_vec(&receipt).map_err(|_| err())?,
        ));
        let profile_json = serde_json::to_string(&config).map_err(|_| err())?;
        let admitted = Self::from_json(&profile_json)?;
        Ok(IsolationAdmission {
            profile_json,
            profile_digest: admitted.digest().to_owned(),
            receipt,
        })
    }
    /// Run only before storing credentials on the newly prepared bounded auth
    /// filesystem. The caller owns preparation serialization. No existing login
    /// environment is adopted: this method creates and removes its own UUID.
    async fn probe_isolation(
        &self,
        external: &CancellationToken,
    ) -> Result<IsolationReceipt, TransportError> {
        self.verify().await?;
        if external.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        let environment = probe_environment(&self.config.auth_root);
        let _lock = self.environment_lock(&environment).await?;
        // A stable reserved (version8) UUID permits recovery after process
        // death, while ordinary login uses a new UUID. Never inspect or sweep
        // an auth root that already contains a login environment or marker.
        for entry in fs::read_dir(&self.config.auth_root).map_err(|_| err())? {
            let entry = entry.map_err(|_| err())?;
            let name = entry.file_name();
            let name = name.to_str().ok_or_else(err)?;
            if name != environment
                && name != format!("{environment}.lock")
                && !name.starts_with("isolation-sibling-")
            {
                return Err(err());
            }
        }
        self.remove_environment_containers(&environment).await?;
        let previous = self.config.auth_root.join(&environment);
        if previous.try_exists().map_err(|_| err())? {
            directory(&previous)?;
            fs::remove_dir_all(&previous).map_err(|_| err())?;
        }
        for (root, prefix) in [
            (&self.config.auth_root, "isolation-sibling-"),
            (&self.config.scratch_root, "isolation-host-"),
            // A killed credential-free probe may leave its proxy directory.
            // The fresh-auth check above excludes every login environment.
            (&self.config.scratch_root, "claude-"),
        ] {
            for entry in fs::read_dir(root).map_err(|_| err())? {
                let entry = entry.map_err(|_| err())?;
                if entry.file_name().to_string_lossy().starts_with(prefix) {
                    directory(&entry.path())?;
                    fs::remove_dir_all(entry.path()).map_err(|_| err())?;
                }
            }
        }
        let (env, bridge, listener) = self.prepare(&environment).await?;
        // The only environment is fresh and credential-free. Its lifetime is
        // bounded even if a pre-launch filesystem check fails.
        let mut environment_dir = UnlaunchedEnvironment(Some(env.clone()));
        let outside = tempfile::Builder::new()
            .prefix("isolation-host-")
            .tempdir_in(&self.config.scratch_root)
            .map_err(|_| err())?;
        let sibling = tempfile::Builder::new()
            .prefix("isolation-sibling-")
            .tempdir_in(&self.config.auth_root)
            .map_err(|_| err())?;
        let nonce = uuid::Uuid::new_v4().to_string();
        let mut inaccessible = vec![self.config.engine_socket.clone()];
        for root in [outside.path(), sibling.path()] {
            let file = root.join("canary");
            fs::write(&file, nonce.as_bytes()).map_err(|_| err())?;
            inaccessible.push(file);
        }
        let challenge = Challenge {
            nonce: nonce.clone(),
            inaccessible,
        };
        fs::write(
            bridge.path().join("isolation.json"),
            serde_json::to_vec(&challenge).map_err(|_| err())?,
        )
        .map_err(|_| err())?;
        // Once launch may happen, retain the environment unless owned-container
        // removal is acknowledged. A failed cleanup cannot erase its evidence.
        environment_dir.0 = None;
        let name = format!("moosuri-claude-{}", uuid::Uuid::new_v4());
        let cancel = CancellationToken::new();
        let proxy = ProxyTask(tokio::spawn(proxy_server(listener, cancel.clone())));
        let operation = async {
            let mut child = self
                .launch_mode(&env, bridge.path(), &name, "isolation-probe", false, None)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|_| err())?;
            let mut output = Vec::new();
            child
                .stdout
                .take()
                .ok_or_else(err)?
                .take(8193)
                .read_to_end(&mut output)
                .await
                .map_err(|_| err())?;
            if output.len() > 8192 || !child.wait().await.map_err(|_| err())?.success() {
                return Err(err());
            }
            let mut receipt: IsolationReceipt =
                serde_json::from_slice(&output).map_err(|_| err())?;
            if !receipt.accepted(&nonce) || !receipt.candidate_profile_digest.is_empty() {
                return Err(err());
            }
            receipt.candidate_profile_digest = self.digest().to_owned();
            Ok(receipt)
        };
        let result = tokio::select! {
            biased;
            _ = external.cancelled() => Err(TransportError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(60), operation) => result.unwrap_or(Err(TransportError::Timeout)),
        };
        cancel.cancel();
        proxy.0.abort();
        // Removal ACK precedes filesystem deletion and any successful receipt.
        self.remove(&name).await?;
        fs::remove_dir_all(&env).map_err(|_| err())?;
        result
    }
}

fn probe_environment(root: &Path) -> String {
    let mut hash = Sha256::new();
    hash.update(b"moosuri:claude:credential-free-isolation:v1");
    hash.update(root.as_os_str().as_encoded_bytes());
    let mut bytes: [u8; 16] = hash.finalize()[..16].try_into().expect("sha256 prefix");
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes).to_string()
}

fn text(path: &str) -> Result<String, TransportError> {
    fs::read_to_string(path).map_err(|_| err())
}
fn number(path: &str) -> Result<u64, TransportError> {
    text(path)?.trim().parse().map_err(|_| err())
}
fn absent(path: &Path) -> bool {
    match fs::symlink_metadata(path) {
        Err(e) => matches!(
            e.kind(),
            io::ErrorKind::NotFound | io::ErrorKind::PermissionDenied
        ),
        Ok(_) => false,
    }
}
fn cannot_create(path: &Path) -> bool {
    match fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Err(e) => matches!(
            e.raw_os_error(),
            Some(libc::EROFS) | Some(libc::EACCES) | Some(libc::EPERM)
        ),
        Ok(_) => {
            let _ = fs::remove_file(path);
            false
        }
    }
}
fn filesystem() -> Result<(u64, u64), TransportError> {
    let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
    if unsafe { libc::statvfs(c"/home/claude".as_ptr(), stat.as_mut_ptr()) } != 0 {
        return Err(err());
    }
    let stat = unsafe { stat.assume_init() };
    Ok((
        (stat.f_blocks as u64)
            .checked_mul(stat.f_frsize as u64)
            .ok_or_else(err)?,
        stat.f_files as u64,
    ))
}
async fn proxy_denied(address: SocketAddr) -> Result<bool, TransportError> {
    let mut stream = TcpStream::connect(address).await.map_err(|_| err())?;
    stream
        .write_all(b"CONNECT example.invalid:443 HTTP/1.1\r\nHost: example.invalid:443\r\n\r\n")
        .await
        .map_err(|_| err())?;
    let mut response = [0; 512];
    match tokio::time::timeout(Duration::from_secs(3), stream.read(&mut response)).await {
        Ok(Ok(0)) | Ok(Err(_)) => Ok(true),
        Ok(Ok(n)) => Ok(response[..n].starts_with(b"HTTP/1.1 403")),
        Err(_) => Ok(false),
    }
}

pub(super) async fn child(proxy: SocketAddr) -> Result<(), TransportError> {
    let raw = fs::read("/bridge/isolation.json").map_err(|_| err())?;
    if raw.len() > 8192 {
        return Err(err());
    }
    let challenge: Challenge = serde_json::from_slice(&raw).map_err(|_| err())?;
    if uuid::Uuid::parse_str(&challenge.nonce).is_err() || challenge.inaccessible.len() != 3 {
        return Err(err());
    }
    let status = text("/proc/self/status")?;
    let field = |name: &str| {
        status
            .lines()
            .find_map(|line| line.strip_prefix(name))
            .map(str::trim)
    };
    let uidmap = text("/proc/self/uid_map")?;
    let rootless = uidmap.lines().next().is_some_and(|line| {
        let fields: Vec<_> = line.split_whitespace().collect();
        fields.len() == 3 && fields[0] == "0" && fields[1].parse::<u32>().is_ok_and(|id| id != 0)
    });
    let mut hidden = true;
    for (i, path) in challenge.inaccessible.iter().enumerate() {
        if !path.is_absolute() {
            return Err(err());
        }
        hidden &= absent(path);
        hidden &=
            absent(&Path::new("/proc/1/root").join(path.strip_prefix("/").map_err(|_| err())?));
        let link = Path::new("/home/claude").join(format!("isolation-link-{i}"));
        symlink(path, &link).map_err(|_| err())?;
        hidden &= fs::read(&link).is_err();
        fs::remove_file(link).map_err(|_| err())?;
    }
    let immutable = [
        "/isolation-write",
        "/runtime/isolation-write",
        "/bridge/isolation-write",
    ]
    .iter()
    .all(|path| cannot_create(Path::new(path)));
    let interfaces = fs::read_dir("/sys/class/net")
        .map_err(|_| err())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| err())?;
    let no_interface = interfaces.len() == 1 && interfaces[0].file_name() == "lo";
    let no_route = text("/proc/net/route")?
        .lines()
        .skip(1)
        .all(|line| line.trim().is_empty());
    let mut direct_denied = true;
    for address in [
        "1.1.1.1:443",
        "169.254.169.254:80",
        "172.17.0.1:80",
        "8.8.8.8:53",
    ] {
        // Timeout is an unproven outcome, not evidence of network denial.
        direct_denied &= matches!(
            tokio::time::timeout(Duration::from_secs(1), TcpStream::connect(address)).await,
            Ok(Err(_))
        );
    }
    let cpu = text("/sys/fs/cgroup/cpu.max")?;
    let cpu: Vec<_> = cpu.split_whitespace().collect();
    if cpu.len() != 2 {
        return Err(err());
    }
    let (bytes, inodes) = filesystem()?;
    let output = tokio::time::timeout(
        Duration::from_secs(15),
        Command::new("/runtime/claude")
            .arg("--version")
            .env_clear()
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| err())?
    .map_err(|_| err())?;
    if !output.status.success() || output.stdout.len() > 128 {
        return Err(err());
    }
    let version = std::str::from_utf8(&output.stdout)
        .map_err(|_| err())?
        .split_whitespace()
        .next()
        .ok_or_else(err)?;
    let receipt = IsolationReceipt {
        version: 1,
        nonce: challenge.nonce.clone(),
        candidate_profile_digest: String::new(),
        memory_bytes: number("/sys/fs/cgroup/memory.max")?,
        cpu_quota: cpu[0].parse().map_err(|_| err())?,
        cpu_period: cpu[1].parse().map_err(|_| err())?,
        pids: number("/sys/fs/cgroup/pids.max")?,
        auth_filesystem_bytes: bytes,
        auth_filesystem_inodes: inodes,
        rootless_uid_namespace: rootless,
        no_new_privileges: field("NoNewPrivs:") == Some("1"),
        seccomp: field("Seccomp:") == Some("2"),
        capabilities_dropped: field("CapEff:") == Some("0000000000000000"),
        host_and_sibling_inaccessible: hidden,
        immutable_mounts: immutable,
        no_external_interface_or_route: no_interface && no_route,
        direct_network_denied: direct_denied,
        proxy_destination_denied: proxy_denied(proxy).await?,
        official_cli_version: version.to_owned(),
    };
    if !receipt.accepted(&challenge.nonce) {
        return Err(err());
    }
    println!("{}", serde_json::to_string(&receipt).map_err(|_| err())?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn evidence() -> serde_json::Value {
        json!({"version":1,"nonce":"probe-nonce","candidate_profile_digest":"",
            "memory_bytes":805306368u64,"cpu_quota":100000,"cpu_period":100000,"pids":32,
            "auth_filesystem_bytes":128*1024*1024,"auth_filesystem_inodes":32768,
            "rootless_uid_namespace":true,"no_new_privileges":true,"seccomp":true,
            "capabilities_dropped":true,"host_and_sibling_inaccessible":true,
            "immutable_mounts":true,"no_external_interface_or_route":true,
            "direct_network_denied":true,"proxy_destination_denied":true,
            "official_cli_version":"2.1.322"})
    }
    #[test]
    fn missing_or_unbounded_kernel_controls_cannot_admit_a_profile() {
        let value = evidence();
        assert!(
            serde_json::from_value::<IsolationReceipt>(value.clone())
                .unwrap()
                .accepted("probe-nonce")
        );
        for field in [
            "rootless_uid_namespace",
            "no_new_privileges",
            "seccomp",
            "capabilities_dropped",
            "host_and_sibling_inaccessible",
            "immutable_mounts",
            "no_external_interface_or_route",
            "direct_network_denied",
            "proxy_destination_denied",
        ] {
            let mut changed = value.clone();
            changed[field] = json!(false);
            assert!(
                !serde_json::from_value::<IsolationReceipt>(changed)
                    .unwrap()
                    .accepted("probe-nonce"),
                "{field}"
            );
        }
        for (field, number) in [
            ("memory_bytes", 805306369),
            ("cpu_quota", 200000),
            ("pids", 33),
            ("auth_filesystem_bytes", 134217729),
            ("auth_filesystem_inodes", 32769),
            ("auth_filesystem_inodes", 0),
        ] {
            let mut changed = value.clone();
            changed[field] = json!(number);
            assert!(
                !serde_json::from_value::<IsolationReceipt>(changed)
                    .unwrap()
                    .accepted("probe-nonce"),
                "{field}"
            );
        }
        assert!(
            !serde_json::from_value::<IsolationReceipt>(value.clone())
                .unwrap()
                .accepted("foreign-probe")
        );
        let mut unknown = value;
        unknown["ready"] = json!(true);
        assert!(serde_json::from_value::<IsolationReceipt>(unknown).is_err());
    }
    #[tokio::test]
    async fn fresh_admission_refuses_existing_acceptance_and_cancel_before_launch() {
        let f = super::super::super::fixture::Fixture::new();
        let raw = serde_json::to_string(&*f.profile.config).unwrap();
        assert!(
            Profile::admit_json(&raw, &CancellationToken::new())
                .await
                .is_err()
        );
        let cancelled = CancellationToken::new();
        cancelled.cancel();
        assert!(Profile::admit_json(&raw, &cancelled).await.is_err());
        assert_eq!(f.launches(), 0);
    }
    #[test]
    fn existing_host_canary_is_never_reported_inaccessible() {
        let dir = tempfile::tempdir().unwrap();
        let canary = dir.path().join("host");
        fs::write(&canary, b"public-fixture").unwrap();
        assert!(!absent(&canary));
        assert!(absent(&dir.path().join("missing")));
    }

    #[tokio::test]
    async fn admission_never_sweeps_a_root_with_an_existing_login() {
        let f = super::super::super::fixture::Fixture::new();
        let existing = f
            .profile
            .config
            .auth_root
            .join(uuid::Uuid::new_v4().to_string());
        fs::create_dir(&existing).unwrap();
        fs::write(existing.join("sentinel"), b"synthetic-owned-data").unwrap();
        assert!(
            f.profile
                .probe_isolation(&CancellationToken::new())
                .await
                .is_err()
        );
        assert_eq!(
            fs::read(existing.join("sentinel")).unwrap(),
            b"synthetic-owned-data"
        );
        assert_eq!(f.launches(), 0);
    }

    #[test]
    fn retry_uses_one_reserved_probe_identity_per_auth_filesystem() {
        let root = Path::new("/var/lib/synthetic-owner/auth");
        let a = probe_environment(root);
        assert_eq!(a, probe_environment(root));
        assert_eq!(uuid::Uuid::parse_str(&a).unwrap().get_version_num(), 8);
        assert_ne!(a, probe_environment(Path::new("/var/lib/other-owner/auth")));
    }

    #[tokio::test]
    async fn reserved_probe_identity_cannot_login_bind_or_retire() {
        let f = super::super::super::fixture::Fixture::new();
        let environment = probe_environment(&f.profile.config.auth_root);
        let path = f.profile.config.auth_root.join(&environment);
        fs::create_dir(&path).unwrap();
        fs::write(path.join("sentinel"), b"probe-only").unwrap();
        assert!(f.profile.login_allowed(&environment).is_err());
        assert!(f.profile.login(&environment).await.is_err());
        let (events, _receiver) = mpsc::channel(1);
        let (_sender, mut codes) = mpsc::channel(1);
        let cancel = CancellationToken::new();
        assert!(
            f.profile
                .login_web(&environment, &events, &mut codes, &cancel)
                .await
                .is_err()
        );
        assert!(
            f.profile
                .account_models(&environment, &cancel)
                .await
                .is_err()
        );
        assert!(f.profile.retire_environment(&environment).await.is_err());
        assert_eq!(fs::read(path.join("sentinel")).unwrap(), b"probe-only");
        assert!(
            !f.profile
                .config
                .auth_root
                .join(format!("{environment}.retired"))
                .exists()
        );
        assert_eq!(f.launches(), 0);
    }
}
