//! Real binary + controlling PTY. All accounts, signing keys and files are synthetic.
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::{
        fd::FromRawFd,
        unix::{fs::PermissionsExt, process::CommandExt},
    },
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::{Duration, Instant},
};
const PASSWORD: &str = "Synthetic-Setup-Password-42";
struct Pty {
    child: Child,
    master: File,
    transcript: String,
    at: usize,
}
impl Pty {
    fn spawn(mut cmd: Command) -> Self {
        let (mut master, mut slave) = (0, 0);
        let mut ws = libc::winsize {
            ws_row: 45,
            ws_col: 120,
            ws_xpixel: 0,
            ws_ypixel: 0,
        };
        assert_eq!(
            unsafe {
                libc::openpty(
                    &mut master,
                    &mut slave,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::addr_of_mut!(ws),
                )
            },
            0
        );
        unsafe {
            cmd.pre_exec(move || {
                // Exercise the ordinary desktop/server default, even when the
                // test runner itself uses a more restrictive mask.
                libc::umask(0o022);
                if libc::setsid() < 0 || libc::ioctl(slave, libc::TIOCSCTTY as _, 0) < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                libc::dup2(slave, libc::STDIN_FILENO);
                libc::close(slave);
                Ok(())
            });
        }
        let child = cmd
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        unsafe {
            libc::close(slave);
            let flags = libc::fcntl(master, libc::F_GETFL);
            libc::fcntl(master, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        Self {
            child,
            master: unsafe { File::from_raw_fd(master) },
            transcript: String::new(),
            at: 0,
        }
    }
    fn until(&mut self, needle: &str) {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(i) = self.transcript[self.at..].find(needle) {
                self.at += i + needle.len();
                return;
            }
            if let Some(status) = self.child.try_wait().unwrap() {
                // A fast successful rerun may exit between the preceding read
                // and try_wait. Consume its final terminal output before failing.
                let mut bytes = [0; 8192];
                while let Ok(n) = self.master.read(&mut bytes) {
                    if n == 0 {
                        break;
                    }
                    self.transcript
                        .push_str(&String::from_utf8_lossy(&bytes[..n]));
                }
                if let Some(i) = self.transcript[self.at..].find(needle) {
                    self.at += i + needle.len();
                    return;
                }
                let mut err = String::new();
                self.child
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut err)
                    .unwrap();
                let mut out = String::new();
                self.child
                    .stdout
                    .take()
                    .unwrap()
                    .read_to_string(&mut out)
                    .unwrap();
                panic!("setup exited before prompt {needle}: {status}; out={out}; err={err}");
            }
            let mut bytes = [0; 8192];
            match self.master.read(&mut bytes) {
                Ok(n) if n > 0 => self
                    .transcript
                    .push_str(&String::from_utf8_lossy(&bytes[..n])),
                _ => std::thread::sleep(Duration::from_millis(10)),
            }
            assert!(
                Instant::now() < end,
                "prompt not received: {needle}; synthetic PTY: {}",
                self.transcript
            );
        }
    }
    fn line(&mut self, s: &str) {
        writeln!(self.master, "{s}").unwrap()
    }
    fn choose(&mut self, ko: bool, n: usize) {
        self.until(if ko {
            "번호 선택 (Enter = 기본값, q = 종료)"
        } else {
            "Choose a number (Enter = default, q = quit)"
        });
        self.line(&n.to_string())
    }
    fn finish(&mut self) -> (String, String) {
        self.finish_expected(true)
    }
    fn finish_expected(&mut self, success: bool) -> (String, String) {
        let end = Instant::now() + Duration::from_secs(15);
        let status = loop {
            if let Some(s) = self.child.try_wait().unwrap() {
                break s;
            }
            let mut drain = [0; 8192];
            if let Ok(n) = self.master.read(&mut drain) {
                self.transcript
                    .push_str(&String::from_utf8_lossy(&drain[..n]));
            }
            assert!(
                Instant::now() < end,
                "setup did not exit; synthetic PTY: {}",
                self.transcript
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        let mut out = String::new();
        self.child
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut out)
            .unwrap();
        let mut err = String::new();
        self.child
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut err)
            .unwrap();
        assert_eq!(status.success(), success, "stdout: {out}; stderr: {err}");
        (out, err)
    }
}
impl Drop for Pty {
    fn drop(&mut self) {
        let _ = self.child.kill();
        if let Ok(null) = File::open("/dev/null") {
            drop(std::mem::replace(&mut self.master, null));
        }
        for _ in 0..100 {
            if self.child.try_wait().ok().flatten().is_some() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
}
struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    bundle: PathBuf,
    pin: PathBuf,
    prefix: PathBuf,
    data: PathBuf,
    bin: PathBuf,
    port: u16,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let p = root.path();
        let home = p.join("home");
        let bundle = p.join("bundle");
        let bin = p.join("no-services");
        for d in [&home, &bundle, &bin] {
            fs::create_dir(d).unwrap();
            fs::set_permissions(d, fs::Permissions::from_mode(0o700)).unwrap();
        }
        for name in ["systemctl", "launchctl"] {
            fs::write(bin.join(name), "#!/bin/sh\nexit 1\n").unwrap();
            fs::set_permissions(bin.join(name), fs::Permissions::from_mode(0o700)).unwrap();
        }
        fs::copy(env!("CARGO_BIN_EXE_moosuri"), bundle.join("moosuri")).unwrap();
        fs::set_permissions(bundle.join("moosuri"), fs::Permissions::from_mode(0o700)).unwrap();
        let manifest = bundle.join("manifest.json");
        let output = Command::new(env!("CARGO_BIN_EXE_moosuri"))
            .args(["installation", "manifest", "--sequence", "1", "--binary"])
            .arg(bundle.join("moosuri"))
            .arg("--output")
            .arg(&manifest)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let key = Ed25519KeyPair::from_seed_unchecked(&[73; 32]).unwrap();
        fs::write(
            bundle.join("manifest.ed25519"),
            key.sign(&fs::read(&manifest).unwrap()).as_ref(),
        )
        .unwrap();
        let pin = p.join("pin");
        fs::write(
            &pin,
            key.public_key()
                .as_ref()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>(),
        )
        .unwrap();
        for f in [&pin, &manifest, &bundle.join("manifest.ed25519")] {
            fs::set_permissions(f, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        Self {
            prefix: p.join("runtime"),
            data: p.join("data"),
            home,
            bundle,
            pin,
            bin,
            port,
            _root: root,
        }
    }
    fn command(&self, action: &str) -> Command {
        let mut cmd = Command::new(self.prefix.join("manager"));
        cmd.env("HOME", &self.home)
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .arg(action)
            .arg("--prefix")
            .arg(&self.prefix);
        cmd
    }
    fn setup(&self, ko: bool) -> Command {
        let mut c = Command::new(env!("CARGO_BIN_EXE_moosuri"));
        c.env("HOME", &self.home)
            .env_remove("XDG_DATA_HOME")
            .env("SHELL", "/bin/bash")
            .env("PATH", format!("{}:/usr/bin:/bin", self.bin.display()))
            .args(["--data-dir"])
            .arg(&self.data)
            .args([
                "setup",
                "--plain",
                "--no-browser",
                "--language",
                if ko { "ko" } else { "en" },
                "--prefix",
            ])
            .arg(&self.prefix)
            .arg("--bundle")
            .arg(&self.bundle)
            .arg("--trust-key")
            .arg(&self.pin);
        c
    }
    fn account(&self, ko: bool, temporary: bool) -> Pty {
        let mut p = Pty::spawn(self.setup(ko));
        p.choose(ko, 1);
        p.until(if ko { "내부 포트" } else { "Local port" });
        p.line(&self.port.to_string());
        p.until(if ko { "이름:" } else { "Your name:" });
        p.line("Synthetic Owner");
        p.until(if ko { "이메일 (" } else { "Email (" });
        p.line("owner@example.test");
        p.until(if ko { "비밀번호 (" } else { "Password (" });
        p.line(if temporary { "" } else { PASSWORD });
        if !temporary {
            p.until(if ko {
                "비밀번호 확인"
            } else {
                "Confirm password"
            });
            p.line(PASSWORD)
        }
        p.choose(ko, 1);
        p
    }
    fn fresh(&self, ko: bool, temporary: bool) -> Pty {
        let mut p = self.account(ko, temporary);
        p.choose(ko, 1);
        p.choose(ko, 2);
        p
    }
}
#[test]
fn guided_setup_two_languages_resume_and_private_secret_channels() {
    for ko in [false, true] {
        let f = Fixture::new();
        let mut p = f.fresh(ko, false);
        let (out, err) = p.finish();
        assert!(
            !out.contains(PASSWORD) && !err.contains(PASSWORD) && !p.transcript.contains(PASSWORD)
        );
        let before = fs::read(f.prefix.join("installation.json")).unwrap();
        let profile = fs::read_to_string(f.home.join(".bashrc")).unwrap();
        assert_eq!(profile.matches("# >>> moosuri PATH >>>").count(), 1);
        let mut second = Pty::spawn(f.setup(ko));
        second.until(if ko {
            "관리자 계정이 이미 있습니다"
        } else {
            "Administrator already exists"
        });
        second.choose(ko, 1);
        second.choose(ko, 2);
        second.finish();
        assert_eq!(
            before,
            fs::read(f.prefix.join("installation.json")).unwrap()
        );
        assert_eq!(profile, fs::read_to_string(f.home.join(".bashrc")).unwrap());
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(async {
            let store = moosuri_store::Store::open(&f.data).unwrap();
            let login = store
                .login("owner@example.test".into(), PASSWORD.into())
                .await
                .unwrap();
            assert!(!login.user.account().must_change_password);
            let user = store.authenticate(login.token).await.unwrap();
            store.logout(&user).await.unwrap();
        });
        assert!(f.home.join(".local/bin/moosuri").is_file());
        let state: Value =
            serde_json::from_slice(&fs::read(f.prefix.join("setup.json")).unwrap()).unwrap();
        assert_eq!(state["port"], json!(f.port));
        assert_eq!(state["service"], "none");
    }
}
#[test]
fn unattended_setup_cannot_consume_piped_secrets_or_mutate_installation() {
    let f = Fixture::new();
    let mut cmd = f.setup(false);
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let o = cmd.stdin(Stdio::null()).output().unwrap();
    assert!(!o.status.success());
    assert!(!f.prefix.exists());
    assert!(!f.data.exists());
    assert!(String::from_utf8_lossy(&o.stderr).contains("setup_terminal_required"));
}

#[test]
fn setup_serializes_state_changes_and_preserves_a_running_foreground_server() {
    let f = Fixture::new();
    let mut wizard = Pty::spawn(f.setup(false));
    wizard.choose(false, 1);
    wizard.until("Local port");
    wizard.line(&f.port.to_string());
    wizard.until("Your name:");
    let original = fs::read(f.prefix.join("setup.json")).unwrap();
    for action in ["stop", "uninstall", "autostart"] {
        let mut cmd = f.command(action);
        if action == "autostart" {
            cmd.arg("off");
        }
        let out = cmd.output().unwrap();
        assert!(!out.status.success());
        assert!(String::from_utf8_lossy(&out.stderr).contains("setup_control_busy"));
    }
    assert_eq!(original, fs::read(f.prefix.join("setup.json")).unwrap());
    drop(wizard); // A hard interruption releases both guards; runtime is complete.
    let mut wizard = Pty::spawn(f.setup(false));
    wizard.until("Your name:");
    wizard.line("Synthetic Owner");
    wizard.until("Email (");
    wizard.line("owner@example.test");
    wizard.until("Password (");
    wizard.line(PASSWORD);
    wizard.until("Confirm password");
    wizard.line(PASSWORD);
    wizard.choose(false, 1);
    wizard.choose(false, 1);
    wizard.choose(false, 2);
    wizard.finish();

    // The opener records only the synthetic URL. It must not launch while stopped.
    let marker = f.home.join("browser-opened");
    for name in ["open", "xdg-open", "wslview"] {
        let path = f.bin.join(name);
        fs::write(
            &path,
            "#!/bin/sh\nprintf '%s' \"$1\" > \"$HOME/browser-opened\"\n",
        )
        .unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut browser = f
        .command("open")
        .arg("--wait-ready")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(350));
    assert!(
        !marker.exists(),
        "no dead browser page before server startup"
    );
    let mut server = f
        .command("start")
        .arg("--foreground")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        assert!(server.try_wait().unwrap().is_none());
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(browser.wait().unwrap().success());
    assert_eq!(
        fs::read_to_string(marker).unwrap(),
        format!("http://127.0.0.1:{}/app", f.port)
    );
    let before = fs::read(f.prefix.join("installation.json")).unwrap();
    let mut resumed = Pty::spawn(f.setup(false));
    resumed.until("Moosuri is already running");
    resumed.finish();
    assert_eq!(
        before,
        fs::read(f.prefix.join("installation.json")).unwrap()
    );
    let stop = f.command("stop").output().unwrap();
    assert!(!stop.status.success());
    assert!(String::from_utf8_lossy(&stop.stderr).contains("setup_foreground_stop_use_ctrl_c"));
    assert!(server.try_wait().unwrap().is_none());
    unsafe {
        libc::kill(server.id() as i32, libc::SIGTERM);
    }
    assert!(server.wait().unwrap().success());
}

#[cfg(feature = "test-fixtures")]
#[test]
fn interrupted_initial_install_retries_without_losing_remote_port() {
    for point in [
        "after_install_stage",
        "before_install_publish",
        "after_install_publish",
    ] {
        let f = Fixture::new();
        let mut cmd = f.setup(false);
        cmd.env("MOOSURI_INSTALLATION_FAULT", point);
        let mut wizard = Pty::spawn(cmd);
        wizard.choose(false, 3);
        wizard.until("Your domain");
        wizard.line("assistant.example.test");
        wizard.choose(false, 2); // Existing proxy; no certificate issuance in this scenario.
        wizard.until("Local port");
        wizard.line(&f.port.to_string());
        wizard.finish_expected(false);
        assert!(
            !f.data.exists(),
            "fault precedes any account initialization"
        );
        if point == "after_install_publish" {
            let state: Value =
                serde_json::from_slice(&fs::read(f.prefix.join("setup.json")).unwrap()).unwrap();
            assert_eq!(state["port"], json!(f.port));
            let mut resumed = Pty::spawn(f.setup(false));
            resumed.until("Your name:");
            assert!(!resumed.transcript.contains("Local port"));
        } else {
            assert!(
                !f.prefix.exists(),
                "unfinished initial runtime was never published"
            );
            let mut resumed = f.fresh(false, false);
            resumed.finish();
        }
    }
}

#[test]
fn conflicting_service_is_preserved_without_claiming_it_or_blocking_setup() {
    let f = Fixture::new();
    // Announce a service manager, but never call the host manager.
    for name in ["launchctl", "systemctl"] {
        fs::write(f.bin.join(name), "#!/bin/sh\nexit 0\n").unwrap();
    }
    let path = f.home.join(if cfg!(target_os = "macos") {
        "Library/LaunchAgents/io.moosuri.assistant.plist"
    } else {
        ".config/systemd/user/moosuri.service"
    });
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(&path, b"existing unrelated service").unwrap();
    let mut wizard = f.account(false, false);
    wizard.choose(false, 1); // Try the user service.
    wizard.until("Its files were preserved");
    wizard.choose(false, 1); // Add the CLI to PATH.
    wizard.choose(false, 2); // Finish without running.
    wizard.finish();
    let state: Value =
        serde_json::from_slice(&fs::read(f.prefix.join("setup.json")).unwrap()).unwrap();
    assert_eq!(state["service"], "none");
    assert!(state["service_file"].is_null());
    let mut removal = Pty::spawn(f.command("uninstall"));
    removal.until("Remove runtime; keep data");
    removal.line("2");
    removal.finish();
    assert_eq!(fs::read(&path).unwrap(), b"existing unrelated service");
    assert!(f.data.is_dir());
    assert!(!f.prefix.join("installation.json").exists());
}

fn browser_stub(f: &Fixture) -> PathBuf {
    for name in ["open", "xdg-open", "wslview"] {
        let p = f.bin.join(name);
        fs::write(
            &p,
            "#!/bin/sh\nprintf '%s' \"$1\" > \"$HOME/browser-opened\"\n",
        )
        .unwrap();
        fs::set_permissions(p, fs::Permissions::from_mode(0o700)).unwrap();
    }
    f.home.join("browser-opened")
}

#[test]
fn start_and_open_runs_in_terminal_and_rejects_another_installation_on_its_port() {
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let marker = browser_stub(&f);
    let mut open = f.command("open");
    open.arg("--start");
    let mut server = Pty::spawn(open);
    server.until("Keep it open; Ctrl+C stops the server");
    let deadline = Instant::now() + Duration::from_secs(20);
    while !marker.exists() {
        assert!(server.child.try_wait().unwrap().is_none());
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(50));
    }
    assert_eq!(
        fs::read_to_string(&marker).unwrap(),
        format!("http://127.0.0.1:{}/app", f.port)
    );
    // The manager cannot advertise this installation while opening a different data directory.
    let other_data = f._root.path().join("wrong-data");
    let out = Command::new(f.prefix.join("manager"))
        .arg("--data-dir")
        .arg(&other_data)
        .args([
            "serve",
            "--dev",
            "--origin",
            &format!("http://127.0.0.1:{}", f.port),
        ])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("installation_managed_server_data_mismatch")
    );
    assert!(!other_data.exists());
    // A second installation can be configured while stopped, then collide later.
    // Copy its durable runtime identity/state, using a different unused setup port first.
    let other = Fixture::new();
    other.fresh(false, false).finish();
    let mut state: Value =
        serde_json::from_slice(&fs::read(other.prefix.join("setup.json")).unwrap()).unwrap();
    state["port"] = json!(f.port);
    fs::write(
        other.prefix.join("setup.json"),
        serde_json::to_vec(&state).unwrap(),
    )
    .unwrap();
    let mut installation: Value =
        serde_json::from_slice(&fs::read(other.prefix.join("installation.json")).unwrap()).unwrap();
    installation["origin"] = json!(format!("http://127.0.0.1:{}", f.port));
    fs::write(
        other.prefix.join("installation.json"),
        serde_json::to_vec(&installation).unwrap(),
    )
    .unwrap();
    let other_marker = browser_stub(&other);
    let out = other.command("open").arg("--start").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("setup_port_occupied_by_another_server"));
    assert!(!other_marker.exists());
    let out = other.command("status").output().unwrap();
    assert!(out.status.success());
    assert!(String::from_utf8_lossy(&out.stdout).contains("Port used by another server"));
    unsafe {
        libc::kill(server.child.id() as i32, libc::SIGTERM);
    }
    server.finish();
}

#[cfg(feature = "test-fixtures")]
struct Tailnet {
    socket: PathBuf,
    _runtime: tokio::runtime::Runtime,
    config: std::sync::Arc<std::sync::Mutex<(Value, u64)>>,
    conflict: std::sync::Arc<std::sync::atomic::AtomicBool>,
    writes: std::sync::Arc<std::sync::atomic::AtomicUsize>,
    login_mode: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}
#[cfg(feature = "test-fixtures")]
impl Tailnet {
    fn new(f: &Fixture) -> Self {
        use axum::{
            Json, Router,
            http::{HeaderMap, StatusCode},
            routing::get,
        };
        use std::sync::{
            Arc, Mutex,
            atomic::{AtomicBool, AtomicUsize, Ordering},
        };
        let socket = f._root.path().join("tail.sock");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .unwrap();
        let config = Arc::new(Mutex::new((
            json!({"TCP":{"8443":{"HTTPS":true}},"Web":{"other.synthetic.ts.net:8443":{"Handlers":{"/":{"Text":"keep"}}}},"AllowFunnel":{"other.synthetic.ts.net:8443":true}}),
            1u64,
        )));
        let conflict = Arc::new(AtomicBool::new(false));
        let writes = Arc::new(AtomicUsize::new(0));
        let login_mode = Arc::new(AtomicUsize::new(0));
        let login_status = login_mode.clone();
        let read_config = config.clone();
        let set_config = config.clone();
        let collide = conflict.clone();
        let mutations = writes.clone();
        let app=Router::new().route("/localapi/v0/status",get(move||{let mode=login_status.clone();async move {
            let login=mode.compare_exchange(1,2,Ordering::Relaxed,Ordering::Relaxed).is_ok();
            Json(json!({"Version":"1.104.7","BackendState":if login {"NeedsLogin"} else {"Running"},"AuthURL":if login {"https://login.tailscale.com/a/synthetic-test-only"} else {""},"Self":{"ID":"synthetic-node","DNSName":"assistant.synthetic.ts.net."},"CertDomains":["assistant.synthetic.ts.net"]}))
        }}))
            .route("/localapi/v0/serve-config",get(move||{let config=read_config.clone();async move{let c=config.lock().unwrap();([("etag",format!("\"{}\"",c.1))],Json(c.0.clone()))}})
                .post(move|headers:HeaderMap,Json(next):Json<Value>|{let config=set_config.clone();let conflict=collide.clone();let writes=mutations.clone();async move{
                    assert_eq!(headers.get("host").unwrap(),"local-tailscaled.sock");
                    let mut c=config.lock().unwrap();
                    if conflict.swap(false,Ordering::Relaxed){c.0["TCP"]["443"]=json!({"HTTPS":true});c.0["Web"]["assistant.synthetic.ts.net:443"]=json!({"Handlers":{"/":{"Text":"another operator"}}});c.1+=1;}
                    if headers.get("if-match").and_then(|v|v.to_str().ok())!=Some(format!("\"{}\"",c.1).as_str()){return StatusCode::PRECONDITION_FAILED;}
                    c.0=next;c.1+=1;writes.fetch_add(1,Ordering::Relaxed);StatusCode::OK
                }}));
        let listener = runtime.block_on(async { tokio::net::UnixListener::bind(&socket).unwrap() });
        runtime.spawn(async {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            socket,
            _runtime: runtime,
            config,
            conflict,
            writes,
            login_mode,
        }
    }
    fn command(&self, f: &Fixture, action: &str) -> Command {
        let mut c = f.command("tailscale");
        c.arg("--socket").arg(&self.socket).arg(action);
        if action == "connect" {
            c.arg("--no-browser");
        }
        c
    }
}

#[cfg(feature = "test-fixtures")]
#[test]
fn tailscale_connect_resume_and_disconnect_preserve_accounts_and_unrelated_routes() {
    use std::sync::atomic::Ordering;
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let tail = Tailnet::new(&f);
    let original = fs::read(f.prefix.join("installation.json")).unwrap();
    let database = fs::read(f.data.join("assistant.sqlite")).unwrap();
    let unrelated = tail.config.lock().unwrap().0.clone();
    // Includes an interrupted external write and partial local example publication.
    for fault in [
        "tailscale_after_serve",
        "tailscale_during_templates",
        "tailscale_after_origin",
    ] {
        let out = tail
            .command(&f, "connect")
            .env("MOOSURI_INSTALLATION_FAULT", fault)
            .output()
            .unwrap();
        assert!(!out.status.success(), "fault must interrupt: {fault}");
        let blocked = f.command("start").arg("--foreground").output().unwrap();
        assert!(!blocked.status.success());
        assert!(String::from_utf8_lossy(&blocked.stderr).contains("tailscale_change_pending"));
        let out = tail.command(&f, "connect").output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let current: Value =
            serde_json::from_slice(&fs::read(f.prefix.join("installation.json")).unwrap()).unwrap();
        assert_eq!(current["origin"], "https://assistant.synthetic.ts.net");
        assert_eq!(current["network"], "tailscale");
        let before = tail.writes.load(Ordering::Relaxed);
        assert!(
            tail.command(&f, "connect")
                .output()
                .unwrap()
                .status
                .success()
        );
        assert_eq!(
            tail.writes.load(Ordering::Relaxed),
            before,
            "no duplicate external effect"
        );
        let out = tail.command(&f, "disconnect").output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        assert!(!f.prefix.join("tailscale.json").exists());
        assert_eq!(
            fs::read(f.prefix.join("installation.json")).unwrap(),
            original
        );
        assert_eq!(tail.config.lock().unwrap().0, unrelated);
        assert_eq!(fs::read(f.data.join("assistant.sqlite")).unwrap(), database);
    }
    // A managed tailnet origin still uses Moosuri credentials and secure cookies.
    assert!(
        tail.command(&f, "connect")
            .output()
            .unwrap()
            .status
            .success()
    );
    struct Running(Child);
    impl Drop for Running {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let mut server = Running(
        f.command("start")
            .arg("--foreground")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    tail._runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let base = format!("http://127.0.0.1:{}", f.port);
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if client
                .get(format!("{base}/health/ready"))
                .header("host", "assistant.synthetic.ts.net")
                .send()
                .await
                .is_ok_and(|r| r.status() == 204)
            {
                break;
            }
            assert!(Instant::now() < deadline && server.0.try_wait().unwrap().is_none());
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let response = client
            .post(format!("{base}/api/v1/auth/login"))
            .header("host", "assistant.synthetic.ts.net")
            .header("origin", "https://assistant.synthetic.ts.net")
            .header("x-forwarded-for", "ignored-untrusted-value")
            .json(&json!({"email":"owner@example.test","password":PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert!(
            response.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("; Secure")
        );
        let rejected = client
            .post(format!("{base}/api/v1/auth/login"))
            .header("host", "assistant.synthetic.ts.net")
            .header("origin", &base)
            .json(&json!({"email":"owner@example.test","password":PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(rejected.status(), 403);
    });
    unsafe {
        libc::kill(server.0.id() as i32, libc::SIGTERM);
    }
    assert!(server.0.wait().unwrap().success());
    assert!(
        tail.command(&f, "disconnect")
            .output()
            .unwrap()
            .status
            .success()
    );
}
#[cfg(feature = "test-fixtures")]
#[test]
fn tailscale_cas_race_and_unowned_routes_are_preserved() {
    use std::sync::atomic::Ordering;
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let tail = Tailnet::new(&f);
    let original = fs::read(f.prefix.join("installation.json")).unwrap();
    tail.conflict.store(true, Ordering::Relaxed);
    let out = tail.command(&f, "connect").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("tailscale_configuration_changed_retry"));
    assert_eq!(tail.writes.load(Ordering::Relaxed), 0);
    let concurrent = tail.config.lock().unwrap().0.clone();
    assert!(
        tail.command(&f, "disconnect")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(tail.config.lock().unwrap().0, concurrent);
    assert_eq!(
        fs::read(f.prefix.join("installation.json")).unwrap(),
        original
    );
    // A matching route configured outside Moosuri is still not owned.
    {
        let mut c = tail.config.lock().unwrap();
        c.0["Web"]["assistant.synthetic.ts.net:443"] =
            json!({"Handlers":{"/":{"Proxy":format!("http://127.0.0.1:{}",f.port)}}});
        c.1 += 1;
    }
    let out = tail.command(&f, "connect").output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("tailscale_unowned_route_preserved"));
    assert!(!f.prefix.join("tailscale.json").exists());
    assert_eq!(tail.writes.load(Ordering::Relaxed), 0);
}

#[cfg(feature = "test-fixtures")]
#[test]
fn tailscale_no_browser_and_owned_custom_endpoint_removal_survive_interruption() {
    use std::sync::atomic::Ordering;
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let tail = Tailnet::new(&f);
    let marker = browser_stub(&f);
    tail.login_mode.store(1, Ordering::Relaxed);
    let mut connection = Pty::spawn(tail.command(&f, "connect"));
    connection.until("Finish Tailscale sign-in");
    connection.finish();
    assert!(
        !marker.exists(),
        "no-browser also suppresses authentication browser dispatch"
    );
    let receipt = fs::read(f.prefix.join("tailscale.json")).unwrap();
    let mut alien: Value = serde_json::from_slice(&receipt).unwrap();
    alien["installation_id"] = json!("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
    fs::write(
        f.prefix.join("tailscale.json"),
        serde_json::to_vec(&alien).unwrap(),
    )
    .unwrap();
    let external = tail.config.lock().unwrap().0.clone();
    let writes = tail.writes.load(Ordering::Relaxed);
    let mut removal = Pty::spawn(f.command("uninstall"));
    removal.until("Remove runtime; keep data");
    removal.line("2");
    let (_, err) = removal.finish_expected(false);
    assert!(err.contains("tailscale_binding_invalid"));
    assert_eq!(tail.config.lock().unwrap().0, external);
    assert_eq!(tail.writes.load(Ordering::Relaxed), writes);
    assert_eq!(
        serde_json::from_slice::<Value>(&fs::read(f.prefix.join("tailscale.json")).unwrap())
            .unwrap(),
        alien
    );
    fs::write(f.prefix.join("tailscale.json"), receipt).unwrap();
    let mut cmd = f.command("uninstall");
    cmd.env("MOOSURI_INSTALLATION_FAULT", "tailscale_after_remove");
    Pty::spawn(cmd).finish_expected(false);
    assert!(f.prefix.join("tailscale.json").exists());
    let after_effect = tail.writes.load(Ordering::Relaxed);
    assert_eq!(after_effect, writes + 1);
    Pty::spawn(f.command("uninstall")).finish();
    assert_eq!(tail.writes.load(Ordering::Relaxed), after_effect);
    assert!(!f.prefix.join("installation.json").exists());
    assert!(f.data.join("assistant.sqlite").exists());
}

#[cfg(feature = "test-fixtures")]
struct PublicFixture {
    port: u16,
    ca: PathBuf,
    key: PathBuf,
}
#[cfg(feature = "test-fixtures")]
impl PublicFixture {
    fn new(f: &Fixture) -> Self {
        use sha2::{Digest, Sha256};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let domain = "assistant.example.test";
        let cert = rcgen::generate_simple_self_signed(vec![domain.into()]).unwrap();
        let directory = rustls_acme::acme::LETS_ENCRYPT_PRODUCTION_DIRECTORY;
        let digest = Sha256::digest(serde_json::to_vec(&(vec![domain], directory)).unwrap());
        let name = format!(
            "certificate-{}.pem",
            digest
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
        );
        let hash = Sha256::digest(f.data.as_os_str().as_encoded_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>();
        let dir = f
            .data
            .parent()
            .unwrap()
            .join(format!(".moosuri-https-{hash}"));
        fs::create_dir(&dir).unwrap();
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
        let key = dir.join(name);
        fs::write(
            &key,
            format!("{}{}", cert.key_pair.serialize_pem(), cert.cert.pem()),
        )
        .unwrap();
        fs::set_permissions(&key, fs::Permissions::from_mode(0o600)).unwrap();
        let ca = f.home.join("synthetic-ca.pem");
        fs::write(&ca, cert.cert.pem()).unwrap();
        fs::set_permissions(&ca, fs::Permissions::from_mode(0o600)).unwrap();
        Self { port, ca, key }
    }
    fn connect(&self, f: &Fixture) -> Command {
        let mut c = f.command("public");
        c.args([
            "connect",
            "--domain",
            "assistant.example.test",
            "--tls-port",
            &self.port.to_string(),
            "--no-browser",
        ])
        .env(
            "MOOSURI_PUBLIC_TEST_ADDRESS",
            format!("127.0.0.1:{}", self.port),
        )
        .env("MOOSURI_PUBLIC_TEST_CA", &self.ca);
        c
    }
}
#[cfg(feature = "test-fixtures")]
#[test]
fn public_registered_domain_verified_before_activation_and_recovers_to_local_access() {
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let tls = PublicFixture::new(&f);
    let original = fs::read(f.prefix.join("installation.json")).unwrap();
    let database = fs::read(f.data.join("assistant.sqlite")).unwrap();
    for fault in ["public_during_templates", "public_after_origin"] {
        let failed = tls
            .connect(&f)
            .env("MOOSURI_INSTALLATION_FAULT", fault)
            .output()
            .unwrap();
        assert!(!failed.status.success(), "{fault}");
        let denied = f.command("start").arg("--foreground").output().unwrap();
        assert!(!denied.status.success());
    }
    let connected = tls.connect(&f).output().unwrap();
    assert!(
        connected.status.success(),
        "{}",
        String::from_utf8_lossy(&connected.stderr)
    );
    assert_eq!(fs::read(f.data.join("assistant.sqlite")).unwrap(), database);
    let mut server = f
        .command("start")
        .arg("--foreground")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let runtime = tokio::runtime::Runtime::new().unwrap();
    runtime.block_on(async {
        let domain = "assistant.example.test";
        let addr = format!("127.0.0.1:{}", tls.port).parse().unwrap();
        let client = reqwest::Client::builder()
            .no_proxy()
            .resolve(domain, addr)
            .add_root_certificate(
                reqwest::Certificate::from_pem(&fs::read(&tls.ca).unwrap()).unwrap(),
            )
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let url = format!("https://{domain}:{}", tls.port);
        let end = tokio::time::Instant::now() + Duration::from_secs(12);
        loop {
            if let Ok(r) = client
                .get(format!("{url}/health/ready"))
                .header("host", domain)
                .send()
                .await
                && r.status() == 204
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < end,
                "managed HTTPS did not start"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        let login = client
            .post(format!("{url}/api/v1/auth/login"))
            .header("host", domain)
            .header("origin", format!("https://{domain}"))
            .header("x-real-ip", "8.8.4.4")
            .json(&json!({"email":"owner@example.test","password":PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(login.status(), 200);
        assert!(
            login.headers()["set-cookie"]
                .to_str()
                .unwrap()
                .contains("Secure")
        );
        let denied = client
            .post(format!("{url}/api/v1/auth/login"))
            .header("host", domain)
            .header("origin", "https://another.example.test")
            .json(&json!({"email":"owner@example.test","password":PASSWORD}))
            .send()
            .await
            .unwrap();
        assert_eq!(denied.status(), 403);
    });
    assert_eq!(unsafe { libc::kill(server.id() as i32, libc::SIGTERM) }, 0);
    assert!(server.wait().unwrap().success());
    let disconnected = f.command("public").arg("disconnect").output().unwrap();
    assert!(
        disconnected.status.success(),
        "{}",
        String::from_utf8_lossy(&disconnected.stderr)
    );
    assert_eq!(
        fs::read(f.prefix.join("installation.json")).unwrap(),
        original
    );
    assert!(tls.key.exists());
    let mut local = f
        .command("start")
        .arg("--foreground")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    runtime.block_on(async {
        let client = reqwest::Client::builder()
            .no_proxy()
            .timeout(Duration::from_secs(2))
            .build()
            .unwrap();
        let until = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if let Ok(r) = client
                .get(format!("http://127.0.0.1:{}/health/ready", f.port))
                .send()
                .await
                && r.status() == 204
            {
                break;
            }
            assert!(
                tokio::time::Instant::now() < until,
                "local runtime cannot reopen"
            );
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    });
    assert_eq!(unsafe { libc::kill(local.id() as i32, libc::SIGTERM) }, 0);
    assert!(local.wait().unwrap().success());
}
#[cfg(feature = "test-fixtures")]
#[test]
fn public_wrong_destination_and_foreign_receipt_never_activate_a_domain() {
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let tls = PublicFixture::new(&f);
    let before = fs::read(f.prefix.join("installation.json")).unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let wrong = reservation.local_addr().unwrap();
    drop(reservation);
    let failed = tls
        .connect(&f)
        .env("MOOSURI_PUBLIC_TEST_ADDRESS", wrong.to_string())
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert_eq!(
        fs::read(f.prefix.join("installation.json")).unwrap(),
        before
    );
    let receipt = f.prefix.join("public-access.json");
    let raw = fs::read(&receipt).unwrap();
    let mut foreign: Value = serde_json::from_slice(&raw).unwrap();
    foreign["installation_id"] = json!("foreign");
    fs::write(&receipt, serde_json::to_vec(&foreign).unwrap()).unwrap();
    assert!(
        !f.command("public")
            .arg("disconnect")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(receipt.exists());
    fs::write(receipt, raw).unwrap();
    assert!(
        f.command("public")
            .arg("disconnect")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert_eq!(
        fs::read(f.prefix.join("installation.json")).unwrap(),
        before
    );
}

#[cfg(feature = "test-fixtures")]
#[test]
fn public_existing_proxy_must_route_the_one_use_check_to_this_backend() {
    use rustls_acme::rustls;
    let f = Fixture::new();
    f.fresh(false, false).finish();
    let cert = rcgen::generate_simple_self_signed(vec!["assistant.example.test".into()]).unwrap();
    let ca = f.home.join("proxy-ca.pem");
    fs::write(&ca, cert.cert.pem()).unwrap();
    fs::set_permissions(&ca, fs::Permissions::from_mode(0o600)).unwrap();
    let tls = rustls::ServerConfig::builder_with_provider(std::sync::Arc::new(
        rustls::crypto::ring::default_provider(),
    ))
    .with_safe_default_protocol_versions()
    .unwrap()
    .with_no_client_auth()
    .with_single_cert(
        vec![cert.cert.der().clone()],
        rustls::pki_types::PrivatePkcs8KeyDer::from(cert.key_pair.serialize_der()).into(),
    )
    .unwrap();
    let config = axum_server::tls_rustls::RustlsConfig::from_config(std::sync::Arc::new(tls));
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    listener.set_nonblocking(true).unwrap();
    let backend = f.port;
    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .unwrap();
    let handle = axum_server::Handle::new();
    let server_handle = handle.clone();
    rt.spawn(async move {
        let app = axum::Router::new().fallback(move |uri: axum::http::Uri| async move {
            let client = reqwest::Client::builder()
                .no_proxy()
                .timeout(Duration::from_secs(3))
                .build()
                .unwrap();
            match client
                .get(format!("http://127.0.0.1:{backend}{}", uri.path()))
                .send()
                .await
            {
                Ok(r) => (r.status(), r.text().await.unwrap_or_default()),
                Err(_) => (reqwest::StatusCode::BAD_GATEWAY, String::new()),
            }
        });
        axum_server::from_tcp_rustls(listener, config)
            .unwrap()
            .handle(server_handle)
            .serve(app.into_make_service())
            .await
            .unwrap();
    });
    let connected = f
        .command("public")
        .args([
            "connect",
            "--domain",
            "assistant.example.test",
            "--proxy",
            "--no-browser",
        ])
        .env("MOOSURI_PUBLIC_TEST_ADDRESS", address.to_string())
        .env("MOOSURI_PUBLIC_TEST_CA", ca)
        .output()
        .unwrap();
    assert!(
        connected.status.success(),
        "{}",
        String::from_utf8_lossy(&connected.stderr)
    );
    let saved: Value =
        serde_json::from_slice(&fs::read(f.prefix.join("installation.json")).unwrap()).unwrap();
    assert_eq!(saved["network"], "https");
    assert_eq!(saved["origin"], "https://assistant.example.test");
    assert!(
        f.command("public")
            .arg("disconnect")
            .output()
            .unwrap()
            .status
            .success()
    );
    handle.shutdown();
}
