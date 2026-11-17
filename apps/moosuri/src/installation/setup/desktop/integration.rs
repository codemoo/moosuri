use super::*;

fn wsl() -> bool {
    cfg!(target_os = "linux")
        && fs::read_to_string("/proc/sys/kernel/osrelease")
            .is_ok_and(|v| v.to_ascii_lowercase().contains("microsoft"))
}
fn available() -> bool {
    if std::env::var_os("SSH_CONNECTION").is_some() || std::env::var_os("SSH_TTY").is_some() {
        return false;
    }
    if cfg!(target_os = "macos") {
        matches!(service::available(), service::Kind::Launchd)
    } else {
        wsl()
            || std::env::var_os("DISPLAY").is_some()
            || std::env::var_os("WAYLAND_DISPLAY").is_some()
    }
}
fn desktop_exec(s: &str) -> String {
    // Desktop Entry string escaping, then Exec quoting; '%' expands field codes.
    format!(
        "\"{}\"",
        s.replace('\\', "\\\\\\\\")
            .replace('"', "\\\\\"")
            .replace('$', "\\\\$")
            .replace('`', "\\\\`")
            .replace('%', "%%")
    )
}
fn files(prefix: &Path) -> Result<Vec<(PathBuf, String, bool)>> {
    render_files(prefix, &home()?, wsl(), false)
}
fn known_files(prefix: &Path) -> Result<Vec<(PathBuf, String, bool)>> {
    let mut result = files(prefix)?;
    if wsl() {
        result.extend(render_files(prefix, &home()?, true, true)?);
    }
    Ok(result)
}
fn render_files(
    prefix: &Path,
    h: &Path,
    is_wsl: bool,
    legacy: bool,
) -> Result<Vec<(PathBuf, String, bool)>> {
    let manager = prefix.join("manager").to_string_lossy().into_owned();
    let p = prefix.to_string_lossy();
    if cfg!(target_os = "macos") && !is_wsl {
        let app = h.join("Applications/Moosuri.app/Contents");
        Ok(vec![
            (app.join("Info.plist"), "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>CFBundleIdentifier</key><string>io.moosuri.desktop</string><key>CFBundleName</key><string>Moosuri</string><key>CFBundleExecutable</key><string>moosuri</string><key>CFBundlePackageType</key><string>APPL</string><key>LSUIElement</key><true/></dict></plist>\n".into(), false),
            (app.join("MacOS/moosuri"), format!("#!/bin/sh\nexec {} desktop --prefix {}\n", shell(&manager), shell(&p)), true),
            (h.join("Library/LaunchAgents/io.moosuri.desktop.plist"), format!("<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict><key>Label</key><string>io.moosuri.desktop</string><key>ProgramArguments</key><array><string>{}</string><string>desktop</string><string>--prefix</string><string>{}</string></array><key>RunAtLoad</key><true/><key>Umask</key><integer>63</integer></dict></plist>\n", service::xml(&manager), service::xml(&p)), false),
        ])
    } else {
        let command = format!(
            "{} {} --prefix {}",
            desktop_exec(&manager),
            if is_wsl {
                if legacy { "open" } else { "open --start" }
            } else {
                "desktop"
            },
            desktop_exec(&p)
        );
        let terminal = is_wsl && !legacy;
        let text = format!(
            "[Desktop Entry]\nType=Application\nName=Moosuri\nName[ko]=무수리\nComment=Assistant server controls and web dashboard\nComment[ko]=비서 서버 실행 관리와 웹 대시보드\nExec={command}\nIcon=applications-internet\nTerminal={terminal}\nCategories=Utility;\n"
        );
        let mut result = vec![(
            h.join(".local/share/applications/moosuri.desktop"),
            text.clone(),
            false,
        )];
        if !is_wsl {
            result.push((h.join(".config/autostart/moosuri.desktop"), text, false));
        }
        Ok(result)
    }
}
pub(in crate::installation::setup) fn offer(
    prefix: &Path,
    state: &mut State,
    tty: &mut Terminal,
) -> Result<()> {
    if !state.desktop_files.is_empty() {
        let planned = files(prefix)?;
        let known = known_files(prefix)?;
        // Accept only exact generated templates for this prefix. Either version
        // may be on disk if an earlier refresh stopped before saving its hashes.
        for f in &state.desktop_files {
            recognized_file(f, &known, true)?;
        }
        for (path, _, _) in &planned {
            if !state.desktop_files.iter().any(|f| f.path == *path) {
                return Err("setup_desktop_file_changed".into());
            }
            if fs::symlink_metadata(path).is_ok() {
                let bytes = read(path, 65536, true)?;
                if !known
                    .iter()
                    .any(|(p, t, _)| p == path && t.as_bytes() == bytes)
                {
                    return Err("setup_desktop_file_changed".into());
                }
            }
        }
        for (path, text, executable) in &planned {
            if path.exists() {
                atomic(path, text.as_bytes())?;
                if *executable {
                    fs::set_permissions(path, std::os::unix::fs::PermissionsExt::from_mode(0o700))
                        .map_err(error)?;
                }
            } else {
                service::managed(path, text.as_bytes(), *executable)?;
            }
        }
        state.desktop_files = planned
            .iter()
            .map(|(path, text, _)| ManagedFile {
                path: path.clone(),
                sha256: hash(text.as_bytes()),
            })
            .collect();
        save(prefix, state)?;
        return Ok(());
    }
    if !available() {
        return Ok(());
    }
    if !wsl() && matches!(state.service, service::Kind::None) {
        return Ok(());
    }
    if tty.select(
        "Add desktop access?",
        "데스크톱에서도 바로 사용할까요?",
        &[
            (
                if wsl() {
                    "Add a web shortcut (recommended)"
                } else {
                    "Add menu icon and app shortcut (recommended)"
                },
                if wsl() {
                    "웹 바로가기 추가 (추천)"
                } else {
                    "메뉴 아이콘과 앱 바로가기 추가 (추천)"
                },
            ),
            ("Use terminal commands only", "터미널 명령어만 사용"),
        ],
        0,
    )? != 0
    {
        return Ok(());
    }
    let planned = files(prefix)?;
    // Preflight the whole set before changing an existing desktop entry.
    for (path, text, _) in &planned {
        if fs::symlink_metadata(path).is_ok() && read(path, 65536, true)? != text.as_bytes() {
            return Err("setup_existing_desktop_file_preserved".into());
        }
    }
    state.desktop_files = planned
        .iter()
        .map(|(path, text, _)| ManagedFile {
            path: path.clone(),
            sha256: hash(text.as_bytes()),
        })
        .collect();
    save(prefix, state)?;
    for (path, text, executable) in planned {
        service::managed(&path, text.as_bytes(), executable)?;
    }
    tty.say("The menu controls this server for everyone. Quitting the menu leaves the server running. Account and model settings stay on the web.", "메뉴에서 이 서버 전체를 켜고 끕니다. 메뉴 앱을 닫아도 서버는 계속 실행됩니다. 계정과 모델 설정은 웹에서 관리합니다.")?;
    Ok(())
}
pub(in crate::installation::setup) fn launch_if_installed(
    prefix: &Path,
    state: &State,
) -> Result<()> {
    if state.desktop_files.is_empty() || wsl() || !available() {
        return Ok(());
    }
    // Launching a second instance is harmless: the GUI owns a distinct lock.
    if !dispatch_detached(
        Process::new(prefix.join("manager"))
            .args(["desktop", "--prefix"])
            .arg(prefix),
    ) {
        return Err("setup_desktop_launch_failed".into());
    }
    Ok(())
}
// Validate the saved ownership claim and the actual bytes independently, so a
// refresh interrupted between file replacement and state save remains removable.
fn recognized_file(
    file: &ManagedFile,
    known: &[(PathBuf, String, bool)],
    missing_ok: bool,
) -> Result<Option<String>> {
    let expected: Vec<_> = known
        .iter()
        .filter(|(p, _, _)| *p == file.path)
        .map(|(_, text, _)| hash(text.as_bytes()))
        .collect();
    if !expected.contains(&file.sha256) {
        return Err("setup_desktop_file_changed".into());
    }
    match fs::symlink_metadata(&file.path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound && missing_ok => Ok(None),
        Err(e) => Err(error(e)),
        Ok(_) => {
            let actual = hash(&read(&file.path, 65536, true)?);
            if !expected.contains(&actual) {
                return Err("setup_desktop_file_changed".into());
            }
            Ok(Some(actual))
        }
    }
}
pub(in crate::installation::setup) fn preflight_remove(prefix: &Path, state: &State) -> Result<()> {
    let expected = known_files(prefix)?;
    for file in &state.desktop_files {
        recognized_file(file, &expected, state.removing)?;
    }
    Ok(())
}
pub(in crate::installation::setup) fn remove(prefix: &Path, state: &mut State) -> Result<()> {
    preflight_remove(prefix, state)?;
    if cfg!(target_os = "macos") && !state.desktop_files.is_empty() {
        let label = format!("gui/{}/io.moosuri.desktop", unsafe { libc::geteuid() });
        if service::success("launchctl", &["print", &label])
            && !service::success("launchctl", &["bootout", &label])
        {
            return Err("setup_desktop_stop_failed".into());
        }
    }
    // A manually launched menu observes the persisted removal intent and exits.
    let end = Instant::now() + Duration::from_secs(20);
    let _tray = loop {
        match named_lock(prefix, "desktop.lock") {
            Ok(f) => break f,
            Err(e) if e == "setup_control_busy" && Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Err(e) => return Err(e),
        }
    };
    let known = known_files(prefix)?;
    while let Some(f) = state.desktop_files.first() {
        if let Some(digest) = recognized_file(f, &known, true)?
            && !remove_exact(&f.path, &digest)?
        {
            return Err("setup_desktop_file_changed".into());
        }
        state.desktop_files.remove(0);
        save(prefix, state)?;
    }
    if cfg!(target_os = "macos") {
        let app = home()?.join("Applications/Moosuri.app");
        for p in [app.join("Contents/MacOS"), app.join("Contents"), app] {
            let _ = fs::remove_dir(p);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wsl_shortcut_refresh_recognizes_old_and_interrupted_templates_but_preserves_edits() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        fs::set_permissions(
            dir.path(),
            std::os::unix::fs::PermissionsExt::from_mode(0o700),
        )
        .unwrap();
        let prefix = dir.path().join("runtime");
        let old = render_files(&prefix, dir.path(), true, true).unwrap();
        let new = render_files(&prefix, dir.path(), true, false).unwrap();
        assert!(old[0].1.contains("Terminal=false"));
        assert!(new[0].1.contains("open --start --prefix"));
        assert!(new[0].1.contains("Terminal=true"));
        let known = [old.clone(), new.clone()].concat();
        let f = ManagedFile {
            path: old[0].0.clone(),
            sha256: hash(old[0].1.as_bytes()),
        };
        service::managed(&f.path, old[0].1.as_bytes(), false).unwrap();
        assert_eq!(
            recognized_file(&f, &known, false).unwrap(),
            Some(f.sha256.clone())
        );
        atomic(&f.path, new[0].1.as_bytes()).unwrap();
        assert_eq!(
            recognized_file(&f, &known, false).unwrap(),
            Some(hash(new[0].1.as_bytes()))
        );
        atomic(&f.path, b"user shortcut").unwrap();
        assert!(recognized_file(&f, &known, false).is_err());
        assert_eq!(fs::read(&f.path).unwrap(), b"user shortcut");
    }
}
