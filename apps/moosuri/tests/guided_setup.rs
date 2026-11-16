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
                    &mut ws,
                )
            },
            0
        );
        unsafe {
            cmd.pre_exec(move || {
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
        assert!(status.success(), "stdout: {out}; stderr: {err}");
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
    fn fresh(&self, ko: bool, temporary: bool) -> Pty {
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
