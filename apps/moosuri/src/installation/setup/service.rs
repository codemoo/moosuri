//! Exact owner-created user service files. Never invoke sudo or overwrite a unit.
use super::*;
use std::process::Stdio;
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Kind {
    None,
    Systemd,
    Launchd,
}
const UNIT: &str = "moosuri.service";
const LABEL: &str = "io.moosuri.assistant";
// Service-manager calls are local but may hang when the user session is unavailable.
// Bound both execution and captured status text; never forward raw service output.
pub(super) fn command(
    program: &str,
    args: &[&str],
    capture: bool,
    timeout: Duration,
) -> Option<(bool, Vec<u8>)> {
    use std::os::fd::AsRawFd;
    let mut child = Process::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(if capture {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut pipe = child.stdout.take();
    if let Some(pipe) = &pipe {
        let fd = pipe.as_raw_fd();
        let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
        if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
    }
    let mut bytes = Vec::new();
    let mut overflow = false;
    let end = std::time::Instant::now() + timeout;
    let status = loop {
        drain_available(&mut pipe, &mut bytes, &mut overflow);
        match child.try_wait() {
            Ok(Some(status)) => break Some(status.success()),
            Err(_) => break None,
            _ if std::time::Instant::now() >= end => break None,
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    };
    if status.is_none() {
        let _ = child.kill();
    }
    let _ = child.wait();
    drain_available(&mut pipe, &mut bytes, &mut overflow);
    // Do not wait for EOF: a descendant may inherit stdout after the direct child exits.
    if overflow {
        None
    } else {
        Some((status?, bytes))
    }
}
fn drain_available(
    pipe: &mut Option<std::process::ChildStdout>,
    bytes: &mut Vec<u8>,
    overflow: &mut bool,
) {
    if let Some(pipe) = pipe {
        // Limit each drain: continuous output cannot prevent deadline checks.
        for _ in 0..17 {
            let mut chunk = [0; 4096];
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(n) if bytes.len() + n <= 65536 => bytes.extend_from_slice(&chunk[..n]),
                Ok(_) => *overflow = true,
                Err(_) => break,
            }
        }
    }
}
fn output(program: &str, args: &[&str]) -> Option<(bool, Vec<u8>)> {
    command(program, args, true, Duration::from_secs(8))
}
pub(super) fn success(program: &str, args: &[&str]) -> bool {
    command(program, args, false, Duration::from_secs(8)).is_some_and(|(ok, _)| ok)
}
pub(super) fn available() -> Kind {
    if cfg!(target_os = "macos") {
        if success(
            "launchctl",
            &["print", &format!("gui/{}", unsafe { libc::geteuid() })],
        ) {
            Kind::Launchd
        } else {
            Kind::None
        }
    } else if success(
        "systemctl",
        &["--user", "--no-ask-password", "show-environment"],
    ) {
        Kind::Systemd
    } else {
        Kind::None
    }
}
fn expected_path(kind: Kind) -> Result<PathBuf> {
    Ok(match kind {
        Kind::Systemd => home()?.join(".config/systemd/user/moosuri.service"),
        Kind::Launchd => home()?.join("Library/LaunchAgents/io.moosuri.assistant.plist"),
        Kind::None => return Err("setup_no_user_service".into()),
    })
}
pub(super) fn xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}
fn render(prefix: &Path, kind: Kind) -> Result<String> {
    let p = prefix.to_str().ok_or("installation_utf8_path_required")?;
    let manager = prefix.join("manager");
    let manager = manager.to_str().ok_or("installation_utf8_path_required")?;
    Ok(match kind {
        Kind::Systemd => format!(
            "[Unit]\nDescription=Moosuri personal assistant\nAfter=network.target\n\n[Service]\nType=simple\nExecStart={} start --prefix {} --foreground\nRestart=on-failure\nRestartSec=3\nUMask=0077\nNoNewPrivileges=true\nTimeoutStopSec=20\n\n[Install]\nWantedBy=default.target\n",
            unit(manager),
            unit(p)
        ),
        Kind::Launchd => format!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>Label</key><string>{LABEL}</string><key>ProgramArguments</key><array><string>{}</string><string>start</string><string>--prefix</string><string>{}</string><string>--foreground</string></array><key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>ThrottleInterval</key><integer>3</integer><key>Umask</key><integer>63</integer><key>StandardOutPath</key><string>{}</string><key>StandardErrorPath</key><string>{}</string></dict></plist>\n",
            xml(manager),
            xml(p),
            xml(&prefix.join("service.log").to_string_lossy()),
            xml(&prefix.join("service.log").to_string_lossy())
        ),
        Kind::None => return Err("setup_no_user_service".into()),
    })
}
pub(super) fn managed(path: &Path, bytes: &[u8], executable: bool) -> Result<ManagedFile> {
    path_checked(path)?;
    directories(parent(path)?)?;
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => create(path, bytes, executable)?,
        Ok(_) => {
            if read(path, 65536, true)? != bytes {
                return Err("setup_existing_file_preserved".into());
            }
        }
        Err(e) => return Err(error(e)),
    }
    sync(parent(path)?)?;
    Ok(ManagedFile {
        path: path.to_owned(),
        sha256: hash(bytes),
    })
}
pub(super) fn launcher(prefix: &Path, state: &mut State) -> Result<()> {
    let target = home()?.join(".local/bin/moosuri");
    // Prefix injection remains shell-quoted; all ordinary CLI commands are unchanged.
    // High-level commands carry their prefix explicitly through a small dispatch.
    let manager = prefix.join("manager");
    let text = format!(
        "#!/bin/sh\ncase \"${{1-}}\" in\n  setup|start|restart|stop|status|open|uninstall|desktop|autostart|tailscale) command=$1; shift; exec {} \"$command\" --prefix {} \"$@\" ;;\n  *) exec {} \"$@\" ;;\nesac\n",
        shell(&manager.to_string_lossy()),
        shell(&prefix.to_string_lossy()),
        shell(&manager.to_string_lossy())
    );
    if let Some(previous) = &state.launcher
        && previous.path == target
        && read(&target, 65536, true).is_ok_and(|bytes| hash(&bytes) == previous.sha256)
        && previous.sha256 != hash(text.as_bytes())
    {
        atomic(&target, text.as_bytes())?;
        fs::set_permissions(&target, std::os::unix::fs::PermissionsExt::from_mode(0o700))
            .map_err(error)?;
    }
    state.launcher = Some(managed(&target, text.as_bytes(), true)?);
    Ok(())
}
fn preflight_install_file(path: &Path, expected: &[u8]) -> Result<()> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Ok(_) if read(path, 65536, true).is_ok_and(|bytes| bytes == expected) => Ok(()),
        _ => Err("setup_existing_service_preserved".into()),
    }
}
pub(super) fn install(prefix: &Path, state: &mut State, kind: Kind) -> Result<()> {
    let path = expected_path(kind)?;
    let content = render(prefix, kind)?;
    // Check before claiming ownership in setup.json. A collision must not trap
    // future setup/uninstall behind an external file we never created.
    preflight_install_file(&path, content.as_bytes())?;
    if !path.exists() {
        let collision = match kind {
            Kind::Systemd => success("systemctl", &["--user", "--no-ask-password", "cat", UNIT]),
            Kind::Launchd => success(
                "launchctl",
                &[
                    "print",
                    &format!("gui/{}/{LABEL}", unsafe { libc::geteuid() }),
                ],
            ),
            Kind::None => false,
        };
        if collision {
            return Err("setup_existing_service_preserved".into());
        }
    }
    // Persist intended bytes before creating external files; a rerun can safely adopt
    // only exact matching files after interruption.
    state.service = kind;
    state.service_file = Some(ManagedFile {
        path: path.clone(),
        sha256: hash(content.as_bytes()),
    });
    save(prefix, state)?;
    managed(&path, content.as_bytes(), false)?;
    if matches!(kind, Kind::Launchd) && !prefix.join("service.log").exists() {
        create(&prefix.join("service.log"), b"", false)?;
    }
    set_autostart(prefix, state, state.autostart)?;
    Ok(())
}
pub(super) fn verify(prefix: &Path, state: &State) -> Result<PathBuf> {
    let file = state.service_file.as_ref().ok_or("setup_no_user_service")?;
    let expected = expected_path(state.service)?;
    let content = render(prefix, state.service)?;
    if file.path != expected
        || file.sha256 != hash(content.as_bytes())
        || hash(&read(&file.path, 65536, true)?) != file.sha256
    {
        return Err("setup_service_file_changed".into());
    }
    Ok(expected)
}
pub(super) fn start(prefix: &Path, state: &State) -> Result<()> {
    let path = verify(prefix, state)?;
    let policy = state.autostart;
    if matches!(state.service, Kind::Launchd) {
        set_autostart(prefix, state, true)?;
    }
    let ok = match state.service {
        Kind::Systemd => {
            success(
                "systemctl",
                &["--user", "--no-ask-password", "daemon-reload"],
            ) && success("systemctl", &["--user", "--no-ask-password", "start", UNIT])
        }
        Kind::Launchd => {
            let domain = format!("gui/{}", unsafe { libc::geteuid() });
            let label = format!("{domain}/{LABEL}");
            if !success("launchctl", &["print", &label])
                && !success(
                    "launchctl",
                    &[
                        "bootstrap",
                        &domain,
                        path.to_str().ok_or("installation_utf8_path_required")?,
                    ],
                )
            {
                false
            } else {
                success("launchctl", &["kickstart", &label])
            }
        }
        Kind::None => false,
    };
    if matches!(state.service, Kind::Launchd) {
        set_autostart(prefix, state, policy)?;
    }
    if ok {
        Ok(())
    } else {
        Err("setup_service_start_failed_run_foreground".into())
    }
}
pub(super) fn running(prefix: &Path, state: &State) -> bool {
    if verify(prefix, state).is_err() {
        return false;
    }
    match state.service {
        Kind::Systemd => success(
            "systemctl",
            &["--user", "--no-ask-password", "is-active", "--quiet", UNIT],
        ),
        Kind::Launchd => output(
            "launchctl",
            &[
                "print",
                &format!("gui/{}/{LABEL}", unsafe { libc::geteuid() }),
            ],
        )
        .is_some_and(|(ok, bytes)| {
            ok && String::from_utf8_lossy(&bytes)
                .lines()
                .any(|line| line.trim() == "state = running")
        }),
        Kind::None => false,
    }
}
pub(super) fn set_autostart(prefix: &Path, state: &State, enabled: bool) -> Result<()> {
    verify(prefix, state)?;
    let ok = match state.service {
        Kind::Systemd => {
            success(
                "systemctl",
                &["--user", "--no-ask-password", "daemon-reload"],
            ) && success(
                "systemctl",
                &[
                    "--user",
                    "--no-ask-password",
                    if enabled { "enable" } else { "disable" },
                    UNIT,
                ],
            )
        }
        Kind::Launchd => success(
            "launchctl",
            &[
                if enabled { "enable" } else { "disable" },
                &format!("gui/{}/{LABEL}", unsafe { libc::geteuid() }),
            ],
        ),
        Kind::None => false,
    };
    if ok {
        Ok(())
    } else {
        Err("setup_autostart_failed".into())
    }
}
pub(super) fn stop(prefix: &Path, state: &State) -> Result<()> {
    if matches!(state.service, Kind::None) {
        return Ok(());
    }
    if state.removing
        && state
            .service_file
            .as_ref()
            .is_some_and(|f| !f.path.exists())
    {
        return Ok(());
    }
    let _path = verify(prefix, state)?;
    let ok = match state.service {
        Kind::Systemd => success("systemctl", &["--user", "--no-ask-password", "stop", UNIT]),
        Kind::Launchd => {
            let label = format!("gui/{}/{LABEL}", unsafe { libc::geteuid() });
            !success("launchctl", &["print", &label]) || success("launchctl", &["bootout", &label])
        }
        Kind::None => true,
    };
    if ok {
        Ok(())
    } else {
        Err("setup_service_stop_failed".into())
    }
}
pub(super) fn file_present(file: &ManagedFile, removing: bool) -> Result<bool> {
    match fs::symlink_metadata(&file.path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && removing => Ok(false),
        Ok(_) => {
            if hash(&read(&file.path, 65536, true)?) != file.sha256 {
                return Err("setup_managed_file_changed_preserved".into());
            }
            Ok(true)
        }
        Err(e) => Err(error(e)),
    }
}
fn log_metadata(path: &Path) -> Result<Option<File>> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(error(e)),
        _ => (),
    };
    path_checked(path)?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(error)?;
    let m = f.metadata().map_err(error)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err("setup_log_preserved".into());
    }
    acl_supported(&f, false)?;
    Ok(Some(f))
}
pub(super) fn preflight_remove(prefix: &Path, state: &State) -> Result<()> {
    if let Some(f) = &state.service_file {
        if f.path != expected_path(state.service)?
            || f.sha256 != hash(render(prefix, state.service)?.as_bytes())
        {
            return Err("setup_service_file_changed".into());
        }
        file_present(f, state.removing)?;
    }
    if let Some(f) = &state.launcher {
        if f.path != home()?.join(".local/bin/moosuri") {
            return Err("setup_launcher_changed_preserved".into());
        }
        file_present(f, state.removing)?;
    }
    log_metadata(&prefix.join("service.log"))?;
    Ok(())
}
pub(super) fn remove(prefix: &Path, state: &mut State) -> Result<()> {
    preflight_remove(prefix, state)?;
    if let Some(file) = &state.service_file {
        if file_present(file, state.removing)? {
            if matches!(state.service, Kind::Systemd)
                && !success(
                    "systemctl",
                    &["--user", "--no-ask-password", "disable", UNIT],
                )
            {
                return Err("setup_service_disable_failed".into());
            }
            if !remove_exact(&file.path, &file.sha256)? {
                return Err("setup_service_file_changed".into());
            }
        }
        if matches!(state.service, Kind::Systemd) {
            let _ = success(
                "systemctl",
                &["--user", "--no-ask-password", "daemon-reload"],
            );
        }
        state.service_file = None;
        state.service = Kind::None;
        save(prefix, state)?;
    }
    if let Some(file) = &state.launcher {
        if file_present(file, state.removing)? && !remove_exact(&file.path, &file.sha256)? {
            return Err("setup_launcher_changed_preserved".into());
        }
        state.launcher = None;
        save(prefix, state)?;
    }
    let log = prefix.join("service.log");
    if let Some(file) = log_metadata(&log)? {
        let before = file.metadata().map_err(error)?;
        let now = fs::symlink_metadata(&log).map_err(error)?;
        if before.dev() != now.dev() || before.ino() != now.ino() {
            return Err("setup_log_preserved".into());
        }
        let dest = prefix.with_extension(format!("service-log-{}", uuid::Uuid::new_v4()));
        download::publish(&log, &dest)?;
        sync(parent(prefix)?)?;
    }
    Ok(())
}
pub(super) fn log_hint(prefix: &Path, state: &State) -> String {
    match state.service {
        Kind::Systemd => "journalctl --user -u moosuri.service -n 50".into(),
        Kind::Launchd => format!("Log: {}", prefix.join("service.log").display()),
        Kind::None => "moosuri start --foreground".into(),
    }
}
pub(super) fn path_hint(tty: &mut Terminal) -> Result<()> {
    let bin = home()?.join(".local/bin");
    if std::env::var_os("PATH").is_some_and(|p| std::env::split_paths(&p).any(|p| p == bin)) {
        return Ok(());
    }
    let shell_name = std::env::var("SHELL")
        .ok()
        .and_then(|s| {
            PathBuf::from(s)
                .file_name()
                .map(|v| v.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "bash".into());
    let profile = match shell_name.as_str() {
        "zsh" => Some(".zshrc"),
        "bash" => Some(".bashrc"),
        _ => None,
    };
    if let Some(profile) = profile
        && tty.select(
            "Make the moosuri command available in new terminals?",
            "새 터미널에서 moosuri 명령을 바로 사용할까요?",
            &[
                (
                    "Add a small PATH block (recommended)",
                    "PATH 설정 한 블록 추가 (추천)",
                ),
                ("I will configure my shell myself", "직접 셸 설정"),
            ],
            0,
        )? == 0
    {
        if append_path(&home()?.join(profile)).is_ok() {
            tty.say(
                "Command enabled for new terminals. In this already-open terminal, run:",
                "새 터미널에서 명령어를 바로 쓸 수 있습니다. 현재 터미널에서는 다음을 실행하세요:",
            )?;
            tty.line("export PATH=\"$HOME/.local/bin:$PATH\"")?;
            return Ok(());
        }
        tty.say(
            "Your existing shell file needs manual configuration; it was preserved.",
            "기존 셸 파일은 보존했습니다. 직접 PATH를 설정해주세요.",
        )?;
    }
    tty.line("export PATH=\"$HOME/.local/bin:$PATH\"")?;
    tty.line(&format!("{}", bin.join("moosuri").display()))?;
    Ok(())
}
fn append_path(path: &Path) -> Result<()> {
    const BLOCK: &str = "\n# >>> moosuri PATH >>>\ncase \":$PATH:\" in *\":$HOME/.local/bin:\"*) ;; *) export PATH=\"$HOME/.local/bin:$PATH\" ;; esac\n# <<< moosuri PATH <<<\n";
    path_checked(path)?;
    let mut f = OpenOptions::new()
        .read(true)
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(error)?;
    let m = f.metadata().map_err(error)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.nlink() != 1
        || m.mode() & 0o022 != 0
        || m.len() > 1024 * 1024
    {
        return Err("setup_shell_profile_preserved".into());
    }
    let mut contents = String::new();
    std::io::Read::by_ref(&mut f)
        .take(1024 * 1024 + 1)
        .read_to_string(&mut contents)
        .map_err(error)?;
    if contents.contains(BLOCK) {
        return Ok(());
    }
    if contents.contains("# >>> moosuri PATH >>>") {
        return Err("setup_shell_profile_preserved".into());
    }
    // Advisory lock avoids two simultaneous installers appending duplicate blocks.
    f.try_lock_exclusive().map_err(error)?;
    if f.metadata().map_err(error)?.len() != m.len() {
        return Err("setup_shell_profile_changed".into());
    }
    f.write_all(BLOCK.as_bytes())
        .and_then(|()| f.sync_all())
        .map_err(error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn manager_completion_never_waits_for_an_inherited_pipe() {
        let began = std::time::Instant::now();
        assert!(
            command(
                "/bin/sh",
                &["-c", "sleep 2 & exit 0"],
                true,
                Duration::from_millis(150)
            )
            .unwrap()
            .0
        );
        assert!(began.elapsed() < Duration::from_secs(1));
        let began = std::time::Instant::now();
        assert!(
            command(
                "/bin/sh",
                &["-c", "sleep 2 & wait"],
                true,
                Duration::from_millis(150)
            )
            .is_none()
        );
        assert!(began.elapsed() < Duration::from_secs(1));
    }
    #[test]
    fn status_only_manager_success_ignores_large_stdout() {
        assert!(success("/bin/sh", &["-c", "head -c 70000 /dev/zero"]));
    }
    #[test]
    fn teardown_preflight_preserves_changed_files_and_accepts_owned_missing_checkpoint() {
        let t = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let p = t.path().join("managed");
        create(&p, b"original", true).unwrap();
        let f = ManagedFile {
            path: p.clone(),
            sha256: hash(b"original"),
        };
        assert!(file_present(&f, false).unwrap());
        fs::write(&p, b"operator changed").unwrap();
        assert!(file_present(&f, true).is_err());
        assert_eq!(fs::read(&p).unwrap(), b"operator changed");
        fs::remove_file(&p).unwrap();
        assert!(file_present(&f, false).is_err());
        assert!(!file_present(&f, true).unwrap());
        let log = t.path().join("service.log");
        create(&log, b"", false).unwrap();
        File::options()
            .write(true)
            .open(&log)
            .unwrap()
            .set_len(20 * 1024 * 1024)
            .unwrap();
        assert_eq!(
            log_metadata(&log)
                .unwrap()
                .unwrap()
                .metadata()
                .unwrap()
                .len(),
            20 * 1024 * 1024
        );
    }
}
