use super::protocol;
#[path = "discovery.rs"]
mod discovery;
pub use discovery::{AccountModels, ModelEffortMetadata};
#[path = "usage.rs"]
mod usage;
pub use usage::{AccountUsage, UsageLimit};
#[path = "login.rs"]
mod login;
use crate::model::{
    CancellationToken, TransportError,
    protocol::{ModelEvent, TextRequest},
};
pub use login::{LoginCode, LoginEvent};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    os::unix::{
        fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
        net::UnixListener as StdUnixListener,
    },
    path::{Path, PathBuf},
    process::Stdio,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::{TcpListener, TcpStream, UnixListener, UnixStream},
    process::Command,
    sync::mpsc,
};
const HOSTS: &[&str] = &[
    "api.anthropic.com",
    "claude.ai",
    "claude.com",
    "auth.claude.com",
    "console.anthropic.com",
    "platform.claude.com",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    engine: PathBuf,
    engine_sha256: String,
    engine_socket: PathBuf,
    image: String,
    official_cli: PathBuf,
    cli_sha256: String,
    cli_version: String,
    helper: PathBuf,
    helper_sha256: String,
    auth_root: PathBuf,
    scratch_root: PathBuf,
    /// Deployment acceptance digest, obtained by the documented credential-free Linux isolation gate.
    isolation_acceptance_sha256: String,
    hosting_terms_accepted: bool,
}
#[derive(Clone)]
pub struct Profile {
    config: Arc<Config>,
    digest: String,
}
fn err() -> TransportError {
    TransportError::InvalidConfiguration
}
fn hex(b: impl AsRef<[u8]>) -> String {
    b.as_ref().iter().map(|b| format!("{b:02x}")).collect()
}
fn hashfile(p: &Path, expected: &str) -> Result<(), TransportError> {
    if !p.is_absolute() || p.canonicalize().map_err(|_| err())? != p {
        return Err(err());
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(p)
        .map_err(|_| err())?;
    let m = f.metadata().map_err(|_| err())?;
    if !m.is_file()
        || m.mode() & 0o022 != 0
        || p.as_os_str().as_encoded_bytes().contains(&b',')
        || m.len() > 512 * 1024 * 1024
    {
        return Err(err());
    }
    let mut h = Sha256::new();
    let mut b = [0; 65536];
    loop {
        let n = std::io::Read::read(&mut f, &mut b).map_err(|_| err())?;
        if n == 0 {
            break;
        }
        h.update(&b[..n]);
    }
    if hex(h.finalize()) != expected {
        return Err(err());
    }
    Ok(())
}
fn directory(p: &Path) -> Result<(), TransportError> {
    let m = std::fs::symlink_metadata(p).map_err(|_| err())?;
    if !p.is_absolute()
        || p.canonicalize().map_err(|_| err())? != p
        || !m.is_dir()
        || p.as_os_str().as_encoded_bytes().contains(&b',')
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o777 != 0o700
    {
        return Err(err());
    }
    Ok(())
}
fn private_child(root: &Path, id: &str) -> Result<PathBuf, TransportError> {
    if uuid::Uuid::parse_str(id).map_err(|_| err())?.to_string() != id {
        return Err(err());
    }
    directory(root)?;
    let p = root.join(id);
    match std::fs::create_dir(&p) {
        Ok(()) => std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o700))
            .map_err(|_| err())?,
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(err()),
    };
    directory(&p)?;
    Ok(p)
}
impl Profile {
    pub fn from_json(s: &str) -> Result<Self, TransportError> {
        Self::parse(s, false)
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn synthetic_from_json(s: &str) -> Result<Self, TransportError> {
        Self::parse(s, true)
    }
    fn parse(s: &str, synthetic: bool) -> Result<Self, TransportError> {
        if s.len() > 16384 {
            return Err(err());
        }
        let c: Config = serde_json::from_str(s).map_err(|_| err())?;
        if !synthetic && (!cfg!(target_os = "linux") || unsafe { libc::geteuid() } == 0)
            || !c.hosting_terms_accepted
            || c.cli_version != "2.1.322"
            || c.image.len() > 256
            || c.image.starts_with('-')
            || c.image.matches("@sha256:").count() != 1
            || !c
                .image
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/._:@-".contains(&b))
            || !c.image.contains("@sha256:")
            || c.image
                .rsplit("@sha256:")
                .next()
                .is_none_or(|x| x.len() != 64 || !x.bytes().all(|b| b.is_ascii_hexdigit()))
            || c.isolation_acceptance_sha256.len() != 64
            || !c
                .isolation_acceptance_sha256
                .bytes()
                .all(|b| b.is_ascii_hexdigit())
        {
            return Err(err());
        }
        directory(&c.auth_root)?;
        directory(&c.scratch_root)?;
        let p = Self {
            digest: hex(Sha256::digest(serde_json::to_vec(&c).map_err(|_| err())?)),
            config: Arc::new(c),
        };
        p.files()?;
        let empty = p.config.scratch_root.join("engine-empty");
        if !empty.exists() {
            std::fs::create_dir(&empty).map_err(|_| err())?;
            std::fs::set_permissions(&empty, std::fs::Permissions::from_mode(0o700))
                .map_err(|_| err())?;
        }
        directory(&empty)?;
        if std::fs::read_dir(&empty)
            .map_err(|_| err())?
            .next()
            .is_some()
        {
            return Err(err());
        }
        Ok(p)
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn root_id(&self) -> String {
        hex(Sha256::digest(
            self.config.auth_root.as_os_str().as_encoded_bytes(),
        ))
    }
    fn files(&self) -> Result<(), TransportError> {
        let c = &self.config;
        hashfile(&c.engine, &c.engine_sha256)?;
        hashfile(&c.official_cli, &c.cli_sha256)?;
        hashfile(&c.helper, &c.helper_sha256)?;
        let m = std::fs::symlink_metadata(&c.engine_socket).map_err(|_| err())?;
        use std::os::unix::fs::FileTypeExt;
        if !c.engine_socket.is_absolute()
            || c.engine_socket.canonicalize().map_err(|_| err())? != c.engine_socket
            || !m.file_type().is_socket()
            || m.uid() != unsafe { libc::geteuid() }
        {
            return Err(err());
        }
        Ok(())
    }
    // Host-owned metadata outside the container-writable HOME. Every login
    // checks it under the same OS lock used by text inference.
    fn login_allowed(&self, environment: &str) -> Result<(), TransportError> {
        self.allowed_environment(environment)?;
        match std::fs::symlink_metadata(
            self.config
                .auth_root
                .join(format!("{environment}.text-verified")),
        ) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(err()),
        }
    }
    fn retain_text_identity(&self, environment: &str) -> Result<(), TransportError> {
        self.allowed_environment(environment)?;
        let path = self
            .config
            .auth_root
            .join(format!("{environment}.text-verified"));
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| err())?;
        let m = f.metadata().map_err(|_| err())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || m.mode() & 0o777 != 0o600
        {
            return Err(err());
        }
        f.sync_all().map_err(|_| err())?;
        std::fs::File::open(&self.config.auth_root)
            .map_err(|_| err())?
            .sync_all()
            .map_err(|_| err())
    }
    pub fn block_environment(&self, environment: &str) -> Result<(), TransportError> {
        if uuid::Uuid::parse_str(environment)
            .map_err(|_| err())?
            .to_string()
            != environment
        {
            return Err(err());
        }
        directory(&self.config.auth_root)?;
        let path = self.config.auth_root.join(format!("{environment}.retired"));
        let f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|_| err())?;
        let m = f.metadata().map_err(|_| err())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || m.mode() & 0o777 != 0o600
        {
            return Err(err());
        }
        f.sync_all().map_err(|_| err())?;
        std::fs::File::open(&self.config.auth_root)
            .map_err(|_| err())?
            .sync_all()
            .map_err(|_| err())?;
        Ok(())
    }
    pub async fn retire_environment(&self, environment: &str) -> Result<(), TransportError> {
        self.block_environment(environment)?;
        let _os = self.os_lock(environment).await?;
        // A credential directory is not retired while any owned container may still use it.
        self.verify().await?;
        let root_label = format!("label=moosuri.claude.installation={}", self.root_id());
        let environment_label = format!("label=moosuri.claude.environment={environment}");
        let containers = self
            .engine_output(&[
                "ps",
                "-aq",
                "--no-trunc",
                "--filter",
                &root_label,
                "--filter",
                &environment_label,
            ])
            .await?;
        for id in std::str::from_utf8(&containers).map_err(|_| err())?.lines() {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err());
            }
            self.engine_output(&["rm", "-f", id]).await?;
        }
        let p = self.config.auth_root.join(environment);
        match std::fs::symlink_metadata(&p) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(_) => return Err(err()),
            Ok(m) if m.file_type().is_symlink() => return Err(err()),
            Ok(_) => {}
        }
        directory(&p)?;
        std::fs::remove_dir_all(p).map_err(|_| err())?;
        std::fs::File::open(&self.config.auth_root)
            .map_err(|_| err())?
            .sync_all()
            .map_err(|_| err())?;
        Ok(())
    }
    async fn os_lock(&self, environment: &str) -> Result<std::fs::File, TransportError> {
        use std::os::fd::AsRawFd;
        if uuid::Uuid::parse_str(environment)
            .map_err(|_| err())?
            .to_string()
            != environment
        {
            return Err(err());
        }
        let f = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(self.config.auth_root.join(format!("{environment}.lock")))
            .map_err(|_| err())?;
        let m = f.metadata().map_err(|_| err())?;
        if !m.is_file()
            || m.uid() != unsafe { libc::geteuid() }
            || m.nlink() != 1
            || m.mode() & 0o777 != 0o600
        {
            return Err(err());
        }
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if unsafe { libc::flock(f.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0 {
                return Ok(f);
            }
            if tokio::time::Instant::now() >= deadline {
                return Err(TransportError::Timeout);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }
    fn allowed_environment(&self, environment: &str) -> Result<(), TransportError> {
        if self
            .config
            .auth_root
            .join(format!("{environment}.retired"))
            .try_exists()
            .map_err(|_| err())?
        {
            return Err(err());
        }
        Ok(())
    }

    fn engine(&self) -> Command {
        let mut c = Command::new(&self.config.engine);
        c.env_clear()
            .arg("--host")
            .arg(format!("unix://{}", self.config.engine_socket.display()))
            .env("PATH", "/usr/bin:/bin")
            .env(
                "DOCKER_CONFIG",
                self.config.scratch_root.join("engine-empty"),
            )
            .kill_on_drop(true);
        c
    }
    async fn engine_output(&self, args: &[&str]) -> Result<Vec<u8>, TransportError> {
        let mut c = self.engine();
        c.args(args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let mut child = c.spawn().map_err(|_| err())?;
        let mut out = Vec::new();
        let stdout = child.stdout.take().ok_or_else(err)?;
        let status = tokio::time::timeout(Duration::from_secs(10), async {
            stdout
                .take(65537)
                .read_to_end(&mut out)
                .await
                .map_err(|_| err())?;
            child.wait().await.map_err(|_| err())
        })
        .await
        .map_err(|_| TransportError::Timeout)??;
        if !status.success() || out.len() > 65536 {
            return Err(err());
        }
        Ok(out)
    }
    pub async fn verify(&self) -> Result<(), TransportError> {
        let p = self.clone();
        tokio::task::spawn_blocking(move || p.files())
            .await
            .map_err(|_| err())??;
        let info = self
            .engine_output(&["info", "--format", "{{json .SecurityOptions}}"])
            .await?;
        let values: Vec<String> = serde_json::from_slice(&info).map_err(|_| err())?;
        if !values.iter().any(|s| s == "name=rootless") {
            return Err(err());
        }
        Ok(())
    }
    pub async fn cleanup_stale(&self) -> Result<(), TransportError> {
        self.verify().await?;
        let label = format!(
            "label=moosuri.claude.installation={}",
            hex(Sha256::digest(
                self.config.auth_root.as_os_str().as_encoded_bytes()
            ))
        );
        let out = self
            .engine_output(&["ps", "-aq", "--no-trunc", "--filter", &label])
            .await?;
        for id in std::str::from_utf8(&out).map_err(|_| err())?.lines() {
            if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err(err());
            }
            self.engine_output(&["rm", "-f", id]).await?;
        }
        Ok(())
    }
    async fn remove(&self, name: &str) -> Result<(), TransportError> {
        if self.engine_output(&["rm", "-f", name]).await.is_ok() {
            return Ok(());
        }
        // --rm can win the race. Only a successful empty engine query proves absence.
        let filter = format!("name=^/{name}$");
        let found = self
            .engine_output(&["ps", "-aq", "--no-trunc", "--filter", &filter])
            .await?;
        if !found.is_empty() {
            return Err(TransportError::Transport);
        }
        Ok(())
    }
    fn launch(
        &self,
        env: &Path,
        bridge: &Path,
        name: &str,
        login: bool,
        terminal: bool,
    ) -> Command {
        let c = &self.config;
        let mut cmd = self.engine();
        cmd.args([
            "run",
            "--rm",
            "--pull=never",
            "--network=none",
            "--log-driver=none",
            "--read-only",
            "--cap-drop=ALL",
            "--security-opt=no-new-privileges",
            "--pids-limit=32",
            "--memory=768m",
            "--cpus=1",
            "--ipc=none",
            "--user=0:0",
            "--workdir=/work",
            "--tmpfs=/tmp:rw,noexec,nosuid,size=16777216",
            "--tmpfs=/work:rw,noexec,nosuid,size=16777216",
            "--name",
            name,
            "--label",
            &format!("moosuri.claude.profile={}", self.digest),
            "--label",
            &format!(
                "moosuri.claude.installation={}",
                hex(Sha256::digest(c.auth_root.as_os_str().as_encoded_bytes()))
            ),
            "--entrypoint=/runtime/moosuri",
        ]);
        cmd.arg("--label").arg(format!(
            "moosuri.claude.environment={}",
            env.file_name()
                .expect("owned environment UUID")
                .to_string_lossy()
        ));
        for (src, dst, ro) in [
            (c.official_cli.as_path(), "/runtime/claude", true),
            (c.helper.as_path(), "/runtime/moosuri", true),
            (env, "/home/claude", false),
            (bridge, "/bridge", true),
        ] {
            cmd.arg("--mount").arg(format!(
                "type=bind,src={},dst={}{}",
                src.display(),
                dst,
                if ro { ",readonly" } else { "" }
            ));
        }
        if terminal {
            cmd.arg("-it");
        } else {
            cmd.arg("-i");
        }
        cmd.arg(&c.image)
            .arg("internal-claude-run")
            .arg(if login { "login" } else { "text" });
        cmd
    }
    async fn prepare(
        &self,
        environment: &str,
    ) -> Result<(PathBuf, tempfile::TempDir, UnixListener), TransportError> {
        self.verify().await?;
        self.allowed_environment(environment)?;
        let env = private_child(&self.config.auth_root, environment)?;
        let temp = tempfile::Builder::new()
            .prefix("claude-")
            .tempdir_in(&self.config.scratch_root)
            .map_err(|_| err())?;
        let listener = StdUnixListener::bind(temp.path().join("proxy.sock")).map_err(|_| err())?;
        listener.set_nonblocking(true).map_err(|_| err())?;
        let listener = UnixListener::from_std(listener).map_err(|_| err())?;
        Ok((env, temp, listener))
    }
    pub async fn login(&self, environment: &str) -> Result<(), TransportError> {
        self.login_with_cancel(environment, &CancellationToken::new())
            .await
    }
    pub async fn login_with_cancel(
        &self,
        environment: &str,
        external: &CancellationToken,
    ) -> Result<(), TransportError> {
        if external.is_cancelled() {
            return Err(TransportError::Cancelled);
        }
        let _os = tokio::select! {_=external.cancelled()=>return Err(TransportError::Cancelled),r=self.os_lock(environment)=>r?};
        self.login_allowed(environment)?;
        let (env, temp, listener) = tokio::select! {_=external.cancelled()=>return Err(TransportError::Cancelled),r=self.prepare(environment)=>r?};
        let name = format!("moosuri-claude-{}", uuid::Uuid::new_v4());
        let cancel = CancellationToken::new();
        let proxy = ProxyTask(tokio::spawn(proxy_server(listener, cancel.clone())));
        let mut cmd = self.launch(&env, temp.path(), &name, true, true);
        cmd.stdin(Stdio::inherit())
            .stdout(Stdio::inherit())
            .stderr(Stdio::inherit());
        let result = tokio::select! {biased;
            _=external.cancelled()=>Err(TransportError::Cancelled),
            r=tokio::time::timeout(Duration::from_secs(300),async{self.allowed_environment(environment)?;cmd.spawn().map_err(|_|err())?.wait().await.map_err(|_|err())})=>r.unwrap_or(Err(TransportError::Timeout))
        };
        cancel.cancel();
        proxy.0.abort();
        // Keep the OS auth lock through the bounded container-removal acknowledgement.
        if self.remove(&name).await.is_err() {
            let _ = self.block_environment(environment);
            return Err(TransportError::Transport);
        }
        match result {
            Ok(s) if s.success() => Ok(()),
            Err(e) => Err(e),
            _ => Err(TransportError::Transport),
        }
    }
    pub async fn stream_text(
        &self,
        environment: &str,
        request: &TextRequest,
        tx: &mpsc::Sender<ModelEvent>,
        cancel: &CancellationToken,
    ) -> Result<(), TransportError> {
        let _os = tokio::select! {_=cancel.cancelled()=>return Err(TransportError::Cancelled),r=self.os_lock(environment)=>r?};
        let (session, user) = protocol::ids();
        let (system, input) = protocol::input(request, &session, &user)?;
        let (env, temp, listener) = tokio::select! {_=cancel.cancelled()=>return Err(TransportError::Cancelled),r=self.prepare(environment)=>r?};
        std::fs::write(temp.path().join("system.txt"), system).map_err(|_| err())?;
        std::fs::write(temp.path().join("mcp.json"), b"{\"mcpServers\":{}}").map_err(|_| err())?;
        let mut settings = serde_json::json!({"disableAllHooks":true,"autoMemoryEnabled":false});
        if let Some(effort) = &request.effort {
            settings["effortLevel"] = serde_json::json!(effort);
        }
        std::fs::write(
            temp.path().join("settings.json"),
            serde_json::to_vec(&settings).map_err(|_| err())?,
        )
        .map_err(|_| err())?;
        let name = format!("moosuri-claude-{}", session);
        let local = CancellationToken::new();
        let proxy = ProxyTask(tokio::spawn(proxy_server(listener, local.clone())));
        let mut cmd = self.launch(&env, temp.path(), &name, false, false);
        cmd.arg(&request.model).arg(&session);
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        let operation = async {
            self.allowed_environment(environment)?;
            let mut child = cmd.spawn().map_err(|_| TransportError::Transport)?;
            let mut stdin = child.stdin.take().ok_or_else(err)?;
            stdin
                .write_all(&input)
                .await
                .map_err(|_| TransportError::Transport)?;
            stdin
                .shutdown()
                .await
                .map_err(|_| TransportError::Transport)?;
            drop(stdin);
            let mut stdout = BufReader::new(child.stdout.take().ok_or_else(err)?);
            let mut decoder =
                protocol::Decoder::new(&session, &user, &request.model, &self.config.cli_version);
            let mut total = 0usize;
            loop {
                let mut line = Vec::new();
                let n = tokio::time::timeout(
                    Duration::from_secs(30),
                    (&mut stdout)
                        .take(256 * 1024 + 1)
                        .read_until(b'\n', &mut line),
                )
                .await
                .map_err(|_| TransportError::Timeout)?
                .map_err(|_| TransportError::Transport)?;
                if n == 0 {
                    break;
                }
                total += n;
                if n > 256 * 1024 || total > 8 * 1024 * 1024 || line.last() != Some(&b'\n') {
                    return Err(TransportError::LimitExceeded);
                }
                for e in decoder.line(&line)? {
                    tx.send(e).await.map_err(|_| TransportError::Cancelled)?;
                }
            }
            let status = child.wait().await.map_err(|_| TransportError::Transport)?;
            for e in decoder.finish(status.success())? {
                tx.send(e).await.map_err(|_| TransportError::Cancelled)?;
            }
            Ok(())
        };
        let result = tokio::select! {biased;_=cancel.cancelled()=>Err(TransportError::Cancelled),_=tx.closed()=>Err(TransportError::Cancelled),r=tokio::time::timeout(Duration::from_secs(120),operation)=>r.unwrap_or(Err(TransportError::Timeout))};
        local.cancel();
        proxy.0.abort();
        if self.remove(&name).await.is_err() {
            let _ = self.block_environment(environment);
            return Err(TransportError::Transport);
        }
        if result.is_ok() {
            self.retain_text_identity(environment)?;
        }
        result
    }
}
struct ProxyTask(tokio::task::JoinHandle<()>);
impl Drop for ProxyTask {
    fn drop(&mut self) {
        self.0.abort();
    }
}
async fn proxy_server(listener: UnixListener, cancel: CancellationToken) {
    let mut jobs = tokio::task::JoinSet::new();
    loop {
        tokio::select! {
            biased;
            _=cancel.cancelled()=>break,
            Some(_)=jobs.join_next(), if !jobs.is_empty()=>{},
            r=listener.accept()=>match r {
                Ok((s,_)) if jobs.len()<32=>{jobs.spawn(async move{let _=proxy_connection(s).await;});},
                Ok((s,_))=>drop(s), // Refuse excess connections without closing the parent lease.
                Err(_)=>break,
            }
        }
    }
    jobs.abort_all();
    while jobs.join_next().await.is_some() {}
}
async fn proxy_connection(mut client: UnixStream) -> Result<(), TransportError> {
    let mut header = Vec::new();
    while !header.ends_with(b"\r\n\r\n") {
        let b = tokio::time::timeout(Duration::from_secs(10), client.read_u8())
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(|_| TransportError::Transport)?;
        header.push(b);
        if header.len() > 4096 {
            return Err(TransportError::LimitExceeded);
        }
    }
    let h = std::str::from_utf8(&header).map_err(|_| err())?;
    let first = h.lines().next().ok_or_else(err)?;
    if first == "MOOSURI_LEASE / HTTP/1.1" {
        let mut b = [0];
        let _ = client.read(&mut b).await;
        return Ok(());
    }
    let mut parts = first.split(' ');
    if parts.next() != Some("CONNECT") {
        return Err(TransportError::DestinationDenied);
    }
    let target = parts.next().ok_or_else(err)?;
    let (host, port) = target.rsplit_once(':').ok_or_else(err)?;
    if port != "443"
        || !HOSTS.contains(&host)
        || parts.next() != Some("HTTP/1.1")
        || parts.next().is_some()
    {
        return Err(TransportError::DestinationDenied);
    }
    let addresses = tokio::net::lookup_host((host, 443))
        .await
        .map_err(|_| TransportError::Transport)?
        .collect::<Vec<_>>();
    if addresses.is_empty()
        || addresses
            .iter()
            .any(|a| !crate::model::public_audio_address(a.ip()))
    {
        return Err(TransportError::DestinationDenied);
    }
    let mut upstream =
        tokio::time::timeout(Duration::from_secs(10), TcpStream::connect(addresses[0]))
            .await
            .map_err(|_| TransportError::Timeout)?
            .map_err(|_| TransportError::Transport)?;
    client
        .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
        .await
        .map_err(|_| TransportError::Transport)?;
    let (cr, mut cw) = client.split();
    let (sr, mut sw) = upstream.split();
    let transfer = async {
        let out = async {
            tokio::io::copy(&mut cr.take(1024 * 1024), &mut sw).await?;
            sw.shutdown().await
        };
        let input = async {
            tokio::io::copy(&mut sr.take(16 * 1024 * 1024), &mut cw).await?;
            cw.shutdown().await
        };
        tokio::try_join!(out, input)
    };
    let _ = tokio::time::timeout(Duration::from_secs(125), transfer).await;
    Ok(())
}
/// Executed only inside the no-network OCI container. It has no credential parser.
pub async fn helper_main(
    mode: &str,
    model: Option<&str>,
    session: Option<&str>,
) -> Result<(), TransportError> {
    if !["text", "login"].contains(&mode) {
        return Err(err());
    }
    let mut lease = UnixStream::connect("/bridge/proxy.sock")
        .await
        .map_err(|_| err())?;
    lease
        .write_all(b"MOOSURI_LEASE / HTTP/1.1\r\n\r\n")
        .await
        .map_err(|_| err())?;
    let listener = TcpListener::bind("127.0.0.1:0").await.map_err(|_| err())?;
    let address = listener.local_addr().map_err(|_| err())?;
    let relay = tokio::spawn(async move {
        let mut jobs = tokio::task::JoinSet::new();
        loop {
            tokio::select! {
                biased;
                Some(_)=jobs.join_next(), if !jobs.is_empty()=>{},
                next=listener.accept()=>match next {
                    Ok((mut tcp,_)) if jobs.len()<32=>{jobs.spawn(async move {
                        if let Ok(mut unix) = UnixStream::connect("/bridge/proxy.sock").await {
                            let _ = tokio::time::timeout(Duration::from_secs(310), tokio::io::copy_bidirectional(&mut tcp, &mut unix)).await;
                        }
                    });},
                    Ok((tcp,_))=>drop(tcp),
                    Err(_)=>break,
                }
            }
        }
    });
    let mut cmd = Command::new("/runtime/claude");
    cmd.env_clear()
        .env("PATH", "/runtime")
        .env("HOME", "/home/claude")
        .env("CLAUDE_CONFIG_DIR", "/home/claude/.claude")
        .env("XDG_CONFIG_HOME", "/home/claude/.config")
        .env("XDG_CACHE_HOME", "/home/claude/.cache")
        .env("TMPDIR", "/tmp")
        .env("LANG", "C.UTF-8")
        .env("HTTPS_PROXY", format!("http://{address}"))
        .env("HTTP_PROXY", format!("http://{address}"));
    for key in [
        "CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC",
        "CLAUDE_CODE_DISABLE_BACKGROUND_TASKS",
        "CLAUDE_CODE_DISABLE_AGENT_VIEW",
        "CLAUDE_CODE_DISABLE_AUTO_MEMORY",
        "CLAUDE_CODE_DISABLE_OFFICIAL_MARKETPLACE_AUTOINSTALL",
    ] {
        cmd.env(key, "1");
    }
    cmd.env("CLAUDE_CODE_MAX_RETRIES", "0")
        .current_dir("/work")
        .kill_on_drop(true);
    if mode == "login" {
        cmd.args(["auth", "login", "--claudeai"]);
    } else {
        let model = model.ok_or_else(err)?;
        let session = session.ok_or_else(err)?;
        cmd.args([
            "-p",
            "--safe-mode",
            "--restricted",
            "--tools",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            "/bridge/mcp.json",
            "--setting-sources",
            "",
            "--settings",
            "/bridge/settings.json",
            "--permission-prompts",
            "none",
            "--no-session-persistence",
            "--max-turns",
            "1",
            "--model",
            model,
            "--session-id",
            session,
            "--input-format",
            "stream-json",
            "--output-format",
            "stream-json",
            "--verbose",
            "--include-partial-messages",
            "--system-prompt-file",
            "/bridge/system.txt",
        ]);
    }
    let mut child = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .map_err(|_| TransportError::Transport)?;
    let mut probe = [0];
    let result = tokio::select! {r=child.wait()=>r.map_err(|_|TransportError::Transport),_=lease.read(&mut probe)=>{let _=child.kill().await;Err(TransportError::Cancelled)}};
    relay.abort();
    if result?.success() {
        Ok(())
    } else {
        Err(TransportError::Transport)
    }
}
