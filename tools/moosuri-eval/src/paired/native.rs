//! Local subject supervisor. macOS is explicit; other platforms fail closed.
use super::{fixture::Provider, oracle::Observation};
use crate::{EvalResult, digest};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs,
    io::{Read, Write},
    net::{TcpListener, TcpStream},
    os::unix::{
        fs::{DirBuilderExt, PermissionsExt},
        process::CommandExt,
    },
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
const PG: &str = "/opt/homebrew/opt/postgresql@16/bin";
const PYTHON: &str = "/opt/homebrew/opt/python@3.13/bin/python3.13";
const TOKEN: &str = "Synthetic-paired-local-token-no-account";
type Env = BTreeMap<String, String>;
fn directory(path: &Path) -> EvalResult<()> {
    fs::DirBuilder::new()
        .mode(0o700)
        .create(path)
        .map_err(|_| "paired_directory")
}
fn env(root: &Path) -> Env {
    BTreeMap::from([
        ("HOME".into(), root.join("home").display().to_string()),
        ("TMPDIR".into(), root.join("temp").display().to_string()),
        ("PATH".into(), format!("{PG}:/usr/bin:/bin")),
        ("LANG".into(), "en_US.UTF-8".into()),
        ("LC_ALL".into(), "en_US.UTF-8".into()),
        ("TZ".into(), "UTC".into()),
        ("PYTHONDONTWRITEBYTECODE".into(), "1".into()),
        ("PYTHONNOUSERSITE".into(), "1".into()),
        ("LANGSMITH_TRACING".into(), "false".into()),
        ("LANGCHAIN_TRACING_V2".into(), "false".into()),
    ])
}
fn command(program: &Path, args: &[String], env: &Env, cwd: &Path) -> Command {
    let mut c = Command::new(program);
    c.args(args)
        .env_clear()
        .envs(env)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .process_group(0);
    c
}
struct Process {
    child: Child,
    finished: bool,
}
impl Process {
    fn start(c: &mut Command) -> EvalResult<Self> {
        Ok(Self {
            child: c.spawn().map_err(|_| "paired_process_start")?,
            finished: false,
        })
    }
    fn signal(&self, signal: &str) {
        let _ = Command::new("/bin/kill")
            .args([signal, "--", &format!("-{}", self.child.id())])
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    fn wait(&mut self, seconds: u64) -> EvalResult<()> {
        let start = Instant::now();
        loop {
            if let Some(status) = self.child.try_wait().map_err(|_| "paired_process_wait")? {
                // Descendant-held pipes must retire before a reader is joined.
                self.signal("-KILL");
                self.finished = true;
                return if status.success() {
                    Ok(())
                } else {
                    Err("paired_process_failed")
                };
            }
            if start.elapsed() > Duration::from_secs(seconds) {
                return Err("paired_process_timeout");
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn stop(&mut self) {
        if self.finished {
            return;
        }
        self.signal("-TERM");
        if self.wait(15).is_err() {
            self.signal("-KILL");
            let _ = self.child.wait();
            self.finished = true;
        }
    }
}
impl Drop for Process {
    fn drop(&mut self) {
        self.stop();
    }
}
async fn bounded(mut command: Command, seconds: u64) -> EvalResult<()> {
    tokio::task::spawn_blocking(move || {
        let mut p = Process::start(&mut command)?;
        p.wait(seconds)
    })
    .await
    .map_err(|_| "paired_join")?
}
pub(super) async fn prepare(capsule: &Path, uv: &Path) -> EvalResult<()> {
    super::capsule::validate_copy(capsule)?;
    if !cfg!(target_os = "macos") || !uv.is_absolute() || !Path::new(PYTHON).exists() {
        return Err("paired_prepare_platform");
    }
    let root = capsule.join("package-preparation");
    directory(&root)?;
    directory(&root.join("home"))?;
    directory(&root.join("temp"))?;
    let mut e = env(&root);
    e.insert(
        "UV_CACHE_DIR".into(),
        root.join("cache").display().to_string(),
    );
    e.insert("UV_NO_CONFIG".into(), "1".into());
    e.insert("UV_PYTHON_DOWNLOADS".into(), "never".into());
    // Only the exact hashed dependency lock chooses packages. No old package is
    // installed/imported and no subject runs during network-enabled preparation.
    let args = [
        "sync",
        "--frozen",
        "--no-dev",
        "--no-install-project",
        "--python",
        PYTHON,
    ]
    .map(str::to_owned);
    bounded(command(uv, &args, &e, &capsule.join("agent")), 240).await
}
fn quote(path: &Path) -> EvalResult<String> {
    let s = path.to_str().ok_or("paired_path")?;
    if s.contains(['"', '\\', '\n', '\r']) {
        return Err("paired_path");
    }
    Ok(format!("\"{s}\""))
}
fn profile(
    capsule: &Path,
    state: &Path,
    model: u16,
    api: u16,
    socket: &Path,
) -> EvalResult<String> {
    Ok(format!(
        r#"(version 1)
(allow default)
(deny file-read* (subpath "/Users") (subpath "/private/tmp") (subpath "/private/var/folders") (subpath "/Volumes"))
(allow file-read-metadata (literal "/private/tmp") (literal {}))
(allow file-read* (subpath {}) (subpath {}))
(deny file-write*)
(allow file-write* (subpath {}) (subpath "/dev"))
(deny mach-lookup)
(allow mach-lookup (global-name "com.apple.system.opendirectoryd.libinfo"))
(deny network*)
(allow network-outbound (remote ip "localhost:{model}"))
(allow network-bind (local ip "localhost:{api}"))
(allow network-inbound (local ip "localhost:{api}"))
(allow network-outbound (remote unix-socket (literal {})))
(allow network-bind (local unix-socket (literal {})))
(allow network-inbound (local unix-socket (literal {})))
"#,
        // initdb stat()s its existing ancestors before creating pgdata. Only
        // metadata is exposed; unrelated temporary contents remain denied.
        quote(state.parent().ok_or("paired_path")?)?,
        quote(capsule)?,
        quote(state)?,
        quote(state)?,
        quote(socket)?,
        quote(socket)?,
        quote(socket)?
    ))
}
fn sandbox(profile: &Path, program: &Path, args: &[String], env: &Env, state: &Path) -> Command {
    let mut a = vec![
        "-f".into(),
        profile.display().to_string(),
        program.display().to_string(),
    ];
    a.extend_from_slice(args);
    command(Path::new("/usr/bin/sandbox-exec"), &a, env, state)
}
async fn captured(mut c: Command) -> EvalResult<Vec<u8>> {
    c.stdout(Stdio::piped());
    tokio::task::spawn_blocking(move || {
        let mut p = Process::start(&mut c)?;
        let stdout = p.child.stdout.take().ok_or("paired_pipe")?;
        let reader = std::thread::spawn(move || {
            let mut b = vec![];
            stdout
                .take(65537)
                .read_to_end(&mut b)
                .map_err(|_| "paired_pipe")?;
            if b.len() > 65536 {
                return Err("paired_output_bound");
            }
            Ok(b)
        });
        let result = p.wait(20);
        if result.is_err() {
            p.stop();
        }
        let out = reader.join().map_err(|_| "paired_pipe")??;
        result?;
        Ok(out)
    })
    .await
    .map_err(|_| "paired_join")?
}
async fn http(
    port: u16,
    method: &str,
    path: &str,
    body: Value,
    cookie: &str,
) -> EvalResult<(Value, String)> {
    let method = method.to_owned();
    let path = path.to_owned();
    let cookie = cookie.to_owned();
    tokio::task::spawn_blocking(move||{
        let mut stream=TcpStream::connect_timeout(&format!("127.0.0.1:{port}").parse().unwrap(),Duration::from_secs(2)).map_err(|_|"paired_http_connect")?;
        stream.set_read_timeout(Some(Duration::from_secs(3))).map_err(|_|"paired_http_timeout")?;stream.set_write_timeout(Some(Duration::from_secs(3))).map_err(|_|"paired_http_timeout")?;
        let body=if body.is_null(){String::new()}else{body.to_string()};
        write!(stream,"{method} {path} HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\nOrigin: http://127.0.0.1:{port}\r\nContent-Type: application/json\r\nCookie: {cookie}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).map_err(|_|"paired_http_write")?;
        let mut b=vec![];stream.take(262145).read_to_end(&mut b).map_err(|_|"paired_http_read")?;if b.len()>262144{return Err("paired_http_bound")}
        let end=b.windows(4).position(|w|w==b"\r\n\r\n").ok_or("paired_http_header")?;let headers=std::str::from_utf8(&b[..end]).map_err(|_|"paired_http_header")?;
        let status=headers.split_whitespace().nth(1).ok_or("paired_http_status")?;if !matches!(status,"200"|"201"|"204"){return Err("paired_http_status")}
        let cookie=headers.lines().find_map(|line|line.to_lowercase().starts_with("set-cookie:").then(||line.split_once(':').unwrap().1.trim().split(';').next().unwrap_or("").to_owned())).unwrap_or_default();
        let body=&b[end+4..];Ok((if body.is_empty(){Value::Null}else{serde_json::from_slice(body).map_err(|_|"paired_http_json")?},cookie))
    }).await.map_err(|_|"paired_join")?
}
pub(super) struct Legacy {
    admitted: std::sync::atomic::AtomicBool,
    _root: tempfile::TempDir,
    state: PathBuf,
    capsule: PathBuf,
    profile: PathBuf,
    env: Env,
    provider: Provider,
    api_port: u16,
    cookie: String,
    postgres: Option<Process>,
    api: Option<Process>,
    runner: Option<Process>,
}
impl Legacy {
    pub async fn start(capsule: &Path) -> EvalResult<Self> {
        super::capsule::validate(capsule)?;
        if !cfg!(target_os = "macos") {
            return Err("paired_macos_isolation_required");
        }
        let capsule = capsule.to_owned();
        let root = tempfile::Builder::new()
            .prefix("moosuri-paired-run-")
            .tempdir_in("/private/tmp")
            .map_err(|_| "paired_directory")?;
        fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))
            .map_err(|_| "paired_directory")?;
        let state = root.path().join("state");
        directory(&state)?;
        directory(&state.join("home"))?;
        directory(&state.join("temp"))?;
        directory(&state.join("socket"))?;
        let api_port = TcpListener::bind("127.0.0.1:0")
            .map_err(|_| "paired_api_port")?
            .local_addr()
            .map_err(|_| "paired_api_port")?
            .port();
        let provider = Provider::start().await?;
        let socket = state.join("socket/.s.PGSQL.15432");
        let file = state.join("isolation.sb");
        fs::write(
            &file,
            profile(&capsule, &state, provider.port, api_port, &socket)?,
        )
        .map_err(|_| "paired_profile")?;
        let mut e = env(&state);
        e.insert(
            "PYTHONPATH".into(),
            capsule.join("agent/src").display().to_string(),
        );
        e.insert(
            "MUSURI_DATABASE_URL".into(),
            format!(
                "host={} port=15432 dbname=postgres user=paired",
                state.join("socket").display()
            ),
        );
        e.insert(
            "MUSURI_ORIGIN".into(),
            format!("http://127.0.0.1:{api_port}"),
        );
        e.insert("MUSURI_OWNER_TOKEN_SHA256".into(), digest(TOKEN.as_bytes()));
        e.insert("MUSURI_COOKIE_SECURE".into(), "0".into());
        e.insert(
            "MUSURI_RUNNER_LOCK".into(),
            state.join("runner.lock").display().to_string(),
        );
        e.insert("MUSURI_MODEL".into(), "gpt-6.1-terra".into());
        e.insert("MUSURI_REASONING_EFFORT".into(), "none".into());
        e.insert(
            "OPENAI_API_KEY".into(),
            "SYNTHETIC-NOT-A-PROVIDER-KEY".into(),
        );
        e.insert(
            "OPENAI_BASE_URL".into(),
            format!("http://127.0.0.1:{}/v1", provider.port),
        );
        let mut s = Self {
            admitted: std::sync::atomic::AtomicBool::new(false),
            _root: root,
            state,
            capsule,
            profile: file,
            env: e,
            provider,
            api_port,
            cookie: String::new(),
            postgres: None,
            api: None,
            runner: None,
        };
        s.probe().await?;
        for tool in ["postgres", "initdb", "pg_ctl", "psql"] {
            let version = captured(s.cmd(&Path::new(PG).join(tool), &["--version".into()])).await?;
            if !std::str::from_utf8(&version)
                .unwrap_or("")
                .split_whitespace()
                .any(|v| v.starts_with("16."))
            {
                return Err("paired_postgres_version");
            }
        }
        bounded(
            s.cmd(
                Path::new(PG).join("initdb").as_path(),
                &[
                    "-D".into(),
                    s.state.join("pgdata").display().to_string(),
                    "-U".into(),
                    "paired".into(),
                    "-A".into(),
                    "trust".into(),
                    "--no-locale".into(),
                    "--encoding=UTF8".into(),
                ],
            ),
            30,
        )
        .await
        .map_err(|_| "paired_initdb")?;
        s.postgres = Some(Process::start(&mut s.cmd(
            &Path::new(PG).join("postgres"),
            &[
                "-D".into(),
                s.state.join("pgdata").display().to_string(),
                "-k".into(),
                s.state.join("socket").display().to_string(),
                "-h".into(),
                String::new(),
                "-p".into(),
                "15432".into(),
            ],
        ))?);
        let started = Instant::now();
        let mut ready = false;
        while started.elapsed() < Duration::from_secs(10) {
            if s.sql("SELECT '{}'::json").await.is_ok() {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if !ready {
            return Err("paired_postgres_start");
        }
        bounded(s.python(&["migrate"]), 40)
            .await
            .map_err(|_| "paired_migration")?;
        s.launch().await?;
        Ok(s)
    }
    fn cmd(&self, program: &Path, args: &[String]) -> Command {
        sandbox(&self.profile, program, args, &self.env, &self.state)
    }
    fn python(&self, args: &[&str]) -> Command {
        let mut a = vec!["-B".into(), "-m".into(), "musuri_agent.cli".into()];
        a.extend(args.iter().map(|s| s.to_string()));
        self.cmd(&self.capsule.join("agent/.venv/bin/python"), &a)
    }
    async fn probe(&self) -> EvalResult<()> {
        // Positive controls prevent a broken launcher being mistaken for isolation.
        bounded(self.cmd(Path::new("/usr/bin/true"), &[]), 3)
            .await
            .map_err(|_| "paired_sandbox_unavailable")?;
        let canary = self._root.path().join("excluded-canary");
        fs::write(&canary, b"SYNTHETIC-EXCLUDED-CANARY").map_err(|_| "paired_probe")?;
        if captured(self.cmd(Path::new("/bin/cat"), &[canary.display().to_string()]))
            .await
            .is_ok()
        {
            return Err("paired_file_isolation");
        }
        let decoy = TcpListener::bind("127.0.0.1:0").map_err(|_| "paired_probe")?;
        decoy.set_nonblocking(true).map_err(|_| "paired_probe")?;
        let port = decoy.local_addr().map_err(|_| "paired_probe")?.port();
        let c = self.cmd(
            Path::new("/usr/bin/curl"),
            &[
                "--noproxy".into(),
                "*".into(),
                "--max-time".into(),
                "1".into(),
                format!("http://127.0.0.1:{port}/"),
            ],
        );
        if captured(c).await.is_ok() || decoy.accept().is_ok() {
            return Err("paired_egress_isolation");
        }
        let positive = self.cmd(
            Path::new("/usr/bin/curl"),
            &[
                "--noproxy".into(),
                "*".into(),
                "--max-time".into(),
                "2".into(),
                "--output".into(),
                "/dev/null".into(),
                "--write-out".into(),
                "%{http_code}".into(),
                format!("http://127.0.0.1:{}/", self.provider.port),
            ],
        );
        if captured(positive).await? != b"404" {
            return Err("paired_network_control");
        }
        if captured(self.cmd(Path::new("/usr/bin/touch"), &[canary.display().to_string()]))
            .await
            .is_ok()
        {
            return Err("paired_write_isolation");
        }
        // This same executable can read an allowed file; denial isn't absence.
        let allowed = self.state.join("allowed-canary");
        fs::write(&allowed, b"SYNTHETIC-ALLOWED").map_err(|_| "paired_probe")?;
        if captured(self.cmd(Path::new("/bin/cat"), &[allowed.display().to_string()])).await?
            != b"SYNTHETIC-ALLOWED"
        {
            return Err("paired_file_control");
        }
        Ok(())
    }
    async fn launch(&mut self) -> EvalResult<()> {
        let port = self.api_port.to_string();
        self.api = Some(Process::start(&mut self.python(&[
            "api",
            "--host",
            "127.0.0.1",
            "--port",
            &port,
        ]))?);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(15) {
            if let Ok((_, cookie)) = http(
                self.api_port,
                "POST",
                "/api/session",
                json!({"token":TOKEN}),
                "",
            )
            .await
                && !cookie.is_empty()
            {
                self.cookie = cookie;
                break;
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        if self.cookie.is_empty() {
            return Err("paired_api_start");
        }
        self.runner = Some(Process::start(&mut self.python(&["runner"]))?);
        Ok(())
    }
    async fn sql(&self, sql: &str) -> EvalResult<Value> {
        let raw = captured(self.cmd(
            &Path::new(PG).join("psql"),
            &[
                "-X".into(),
                "-A".into(),
                "-t".into(),
                "--no-psqlrc".into(),
                "-d".into(),
                self.env["MUSURI_DATABASE_URL"].clone(),
                "-c".into(),
                sql.into(),
            ],
        ))
        .await?;
        serde_json::from_slice(&raw).map_err(|_| "paired_observer_json")
    }
    async fn turn(&self, text: &str) -> EvalResult<Value> {
        let (thread, _) = http(
            self.api_port,
            "POST",
            "/api/threads",
            json!({"client_request_id":uuid::Uuid::new_v4()}),
            &self.cookie,
        )
        .await?;
        let id = thread["id"].as_str().ok_or("paired_thread_id")?;
        let (run, _) = http(
            self.api_port,
            "POST",
            &format!("/api/threads/{id}/messages"),
            json!({"client_request_id":uuid::Uuid::new_v4(),"text":text}),
            &self.cookie,
        )
        .await?;
        let run = run["run_id"]
            .as_str()
            .ok_or("paired_run_id")?
            .parse::<uuid::Uuid>()
            .map_err(|_| "paired_run_id")?;
        self.admitted
            .store(true, std::sync::atomic::Ordering::SeqCst);
        let started = Instant::now();
        while started.elapsed() < Duration::from_secs(20) {
            let value=self.sql(&format!("SELECT json_build_object('status',status,'answer',coalesce((SELECT string_agg(text,'' ORDER BY created_at,id) FROM ma_messages WHERE run_id='{run}' AND role='assistant' AND status='complete'),''),'reads',coalesce((SELECT json_agg(json_build_object('id',item_id,'revision',revision)) FROM ma_memory_reads WHERE run_id='{run}'),'[]'::json)) FROM ma_runs WHERE id='{run}'")).await?;
            if !matches!(value["status"].as_str(), Some("queued" | "running")) {
                return if value["status"] == "succeeded" {
                    Ok(value)
                } else {
                    Err("paired_legacy_terminal")
                };
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        Err("paired_legacy_timeout")
    }
    pub async fn memory(&mut self) -> EvalResult<Observation> {
        let id = uuid::Uuid::new_v4();
        http(self.api_port,"PUT",&format!("/api/items/memories/{id}"),json!({"expected_revision":0,"data":{"title":"Synthetic archive preference","text":super::MEMORY,"source":"Synthetic evaluator direct input"}}),&self.cookie).await?;
        let start = self.provider.calls();
        let mut out = Observation {
            terminal: true,
            ..Default::default()
        };
        for index in 0..2 {
            if index == 1 {
                self.stop_frontend().await;
                self.cookie.clear();
                self.launch().await?;
            }
            let observed = self.turn(super::MEMORY_QUERY).await?;
            out.answers
                .push(observed["answer"].as_str().ok_or("paired_answer")?.into());
            out.read_revisions.push(
                observed["reads"]
                    .as_array()
                    .filter(|v| v.len() == 1 && v[0]["id"] == id.to_string())
                    .and_then(|v| v[0]["revision"].as_i64())
                    .unwrap_or(0),
            );
        }
        let state=self.sql(&format!("SELECT json_build_object('text',data->>'text','revision',revision,'count',(SELECT count(*) FROM ma_items WHERE kind='memories' AND NOT deleted)) FROM ma_items WHERE kind='memories' AND id='{id}'")).await?;
        out.memory_text = state["text"].as_str().ok_or("paired_memory_state")?.into();
        out.memory_revision = state["revision"].as_i64().ok_or("paired_memory_state")?;
        out.memory_count = state["count"].as_i64().ok_or("paired_memory_state")?;
        out.model_calls = self.provider.calls() - start;
        Ok(out)
    }
    pub async fn calculation(&mut self) -> EvalResult<Observation> {
        let start = self.provider.calls();
        let observed = self.turn(super::CALCULATION).await?;
        let state=self.sql("SELECT json_build_object('count',count(*)) FROM ma_items WHERE kind='memories' AND NOT deleted").await?;
        Ok(Observation {
            terminal: true,
            answers: vec![observed["answer"].as_str().ok_or("paired_answer")?.into()],
            memory_count: state["count"].as_i64().ok_or("paired_memory_state")?,
            model_calls: self.provider.calls() - start,
            ..Default::default()
        })
    }
    pub fn has_executed(&self) -> bool {
        self.admitted.load(std::sync::atomic::Ordering::SeqCst)
    }
    async fn stop_frontend(&mut self) {
        let runner = self.runner.take();
        let api = self.api.take();
        let _ = tokio::task::spawn_blocking(move || {
            drop(runner);
            drop(api);
        })
        .await;
    }
    pub async fn shutdown(&mut self) {
        self.stop_frontend().await;
        let pg = self.postgres.take();
        let _ = tokio::task::spawn_blocking(move || drop(pg)).await;
    }
}
impl Drop for Legacy {
    fn drop(&mut self) {
        drop(self.runner.take());
        drop(self.api.take());
        drop(self.postgres.take());
    }
}
