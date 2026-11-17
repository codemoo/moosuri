//! Guided setup reuses signed installation and ordinary account/model boundaries.
mod connection;
pub mod desktop;
mod diagnostics;
mod readiness;
mod service;
mod terminal;
#[cfg(test)]
mod tests;
use super::*;
use clap::Args;
use moosuri_store::Store;
use readiness::{Readiness, ready};
use std::{net::TcpListener, time::Duration};
use terminal::{Language, Terminal};

#[derive(Args)]
pub struct Options {
    /// Signed local bundle supplied by the one-line bootstrap (new installation only).
    #[arg(long, requires = "trust_key")]
    pub bundle: Option<PathBuf>,
    /// Independently pinned public signing key supplied by the bootstrap.
    #[arg(long, requires = "bundle")]
    pub trust_key: Option<PathBuf>,
    #[arg(long)]
    pub prefix: Option<PathBuf>,
    /// Choose English or 한국어 without the initial language question.
    #[arg(long, value_enum)]
    pub language: Option<Language>,
    /// Numbered menus for simple terminals; otherwise use arrows and Enter.
    #[arg(long)]
    pub plain: bool,
    /// Finish with a link rather than opening a browser.
    #[arg(long)]
    pub no_browser: bool,
}
#[derive(Args)]
pub struct Location {
    #[arg(long)]
    pub prefix: Option<PathBuf>,
}
#[derive(Args)]
pub struct Open {
    #[arg(long)]
    pub prefix: Option<PathBuf>,
    /// Used by setup's bounded browser helper across the foreground exec.
    #[arg(long, hide = true)]
    pub wait_ready: bool,
    #[arg(long, hide = true, default_value = "/app", value_parser = ["/app", "/settings/model", "/admin/providers", "/admin/google"])]
    pub page: String,
}
#[derive(Args)]
pub struct Start {
    #[arg(long)]
    pub prefix: Option<PathBuf>,
    #[arg(long)]
    pub foreground: bool,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManagedFile {
    path: PathBuf,
    sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct State {
    format: u32,
    language: Language,
    port: u16,
    service: service::Kind,
    launcher: Option<ManagedFile>,
    service_file: Option<ManagedFile>,
    next_page: String,
    #[serde(default)]
    removing: bool,
    #[serde(default = "default_autostart")]
    autostart: bool,
    #[serde(default)]
    desktop_files: Vec<ManagedFile>,
}
fn default_autostart() -> bool {
    true
}
fn home() -> Result<PathBuf> {
    let path = PathBuf::from(std::env::var_os("HOME").ok_or("setup_home_required")?);
    let path = path.canonicalize().map_err(error)?;
    path_checked(&path)?;
    let m = fs::metadata(&path).map_err(error)?;
    if m.uid() != unsafe { libc::geteuid() } {
        return Err("setup_owned_home_required".into());
    }
    Ok(path)
}
fn default_prefix() -> Result<PathBuf> {
    Ok(home()?.join(if cfg!(target_os = "macos") {
        "Library/Application Support/moosuri-runtime"
    } else {
        ".local/share/moosuri-runtime"
    }))
}
fn prefix(path: &Option<PathBuf>) -> Result<PathBuf> {
    path.clone().map(Ok).unwrap_or_else(default_prefix)
}
fn parent(path: &Path) -> Result<&Path> {
    path.parent().ok_or_else(|| "setup_parent_required".into())
}
fn directories(path: &Path) -> Result<()> {
    path_checked(path)?;
    if path.exists() {
        return Ok(());
    }
    directories(parent(path)?)?;
    new_dir(path)
}
fn load(prefix: &Path) -> Result<State> {
    let s: State = parse(&read(&prefix.join("setup.json"), 16384, true)?)?;
    if s.format != 1
        || s.port < 1024
        || !matches!(
            s.next_page.as_str(),
            "/app" | "/settings/model" | "/admin/providers" | "/admin/google"
        )
    {
        return Err("setup_state_invalid".into());
    }
    Ok(s)
}
fn save(prefix: &Path, state: &State) -> Result<()> {
    atomic(&prefix.join("setup.json"), &json(state)?)
}
fn server_args(i: &Installation, s: &State) -> Vec<String> {
    let mut a = vec![
        "serve".into(),
        "--bind".into(),
        format!("127.0.0.1:{}", s.port),
        "--origin".into(),
        i.origin.clone(),
    ];
    match i.network {
        Network::Local => a.push("--dev".into()),
        Network::Https => a.extend(["--trusted-proxy".into(), "127.0.0.1".into()]),
        Network::Tailscale => (),
    };
    a
}
fn validate_port(port: u16) -> Result<()> {
    if port < 1024 {
        return Err("setup_port_invalid".into());
    }
    TcpListener::bind(("127.0.0.1", port))
        .map(|_| ())
        .map_err(|_| "setup_port_busy".into())
}
pub async fn run(options: &Options, data: Option<&Path>) -> Result<()> {
    if unsafe { libc::geteuid() } == 0 {
        return Err("setup_run_without_sudo".into());
    }
    let mut tty = Terminal::open(options.language, options.plain)?;
    if options.language.is_none() {
        let n = tty.select(
            "Language / 언어",
            "Language / 언어",
            &[("English", "English"), ("한국어", "한국어")],
            if matches!(tty.lang, Language::Ko) {
                1
            } else {
                0
            },
        )?;
        tty.lang = if n == 0 { Language::En } else { Language::Ko };
    }
    let result = wizard(options, data, &mut tty).await;
    if let Err(code) = &result {
        tty.say(
            "Setup paused. Your existing data has not been removed.",
            "설치를 멈췄습니다. 기존 데이터는 삭제하지 않았습니다.",
        )?;
        tty.line(diagnostics::hint(diagnostics::classify(code), tty.lang))?;
        tty.line("https://github.com/codemoo/moosuri/blob/main/docs/install.md#troubleshooting--문제-해결")?;
        tty.line(&format!(
            "{}: {code}",
            tty.lang.text("Details", "상세 코드")
        ))?;
    }
    result
}
async fn wizard(options: &Options, data: Option<&Path>, tty: &mut Terminal) -> Result<()> {
    tty.say("\nMoosuri · Personal assistant · Development preview\nUse arrows or numbers, then Enter. Optional connections can wait.","\n무수리 · 개인 비서 · 개발 미리보기\n방향키나 번호로 고르고 Enter를 누르세요. 외부 연결은 나중에 해도 됩니다.")?;
    let prefix = prefix(&options.prefix)?;
    path_checked(&prefix)?;
    let fresh = !prefix.exists();
    let control = if fresh {
        let bundle = options
            .bundle
            .as_ref()
            .ok_or("setup_bundle_required_use_one_line_installer")?;
        let key = options
            .trust_key
            .as_ref()
            .ok_or("setup_trust_key_required")?;
        // Verify before collecting any password or modifying installation state.
        super::bundle(bundle, key)?;
        tty.say("\n1/5 · Access", "\n1/5 · 접속 방식")?;
        let mode = tty.select(
            "Where will you use Moosuri?",
            "어디에서 사용할까요?",
            &[
                (
                    "This computer (recommended)",
                    "이 컴퓨터에서 바로 사용 (추천)",
                ),
                (
                    "Tailscale · private network",
                    "Tailscale · 내 기기끼리 접속",
                ),
                ("My HTTPS domain · advanced", "내 HTTPS 도메인 · 고급 설정"),
            ],
            0,
        )?;
        if mode == 1 {
            tty.say("Use an existing Tailscale HTTPS hostname. Moosuri will show the Serve command; it never enables public Funnel.","이미 연결한 Tailscale의 HTTPS 주소를 사용합니다. Serve 명령을 안내하며 공개 Funnel은 켜지 않습니다.")?;
        }
        if mode == 2 {
            tty.say("Use an existing HTTPS reverse proxy. DNS, certificates and other services are left under your control.","준비된 HTTPS 리버스 프록시를 사용합니다. DNS·인증서·기존 서비스는 직접 설정합니다.")?;
        }
        let port = loop {
            let value = tty.input("Local port", "내부 포트", "3210")?;
            match value
                .parse::<u16>()
                .ok()
                .filter(|p| validate_port(*p).is_ok())
            {
                Some(p) => break p,
                None => tty.say(
                    "That port is unavailable. Choose a free port above 1023.",
                    "사용할 수 없는 포트입니다. 1024 이상의 빈 포트를 선택해주세요.",
                )?,
            }
        };
        let (network, origin) = if mode == 0 {
            (Network::Local, format!("http://127.0.0.1:{port}"))
        } else {
            let origin = loop {
                let value = tty.input(
                    "HTTPS address (e.g. https://assistant.example.com)",
                    "HTTPS 주소 (예: https://assistant.example.com)",
                    "",
                )?;
                if value.starts_with("https://")
                    && moosuri_server::ServerConfig::new(&value, false).is_ok()
                {
                    break url::Url::parse(&value)
                        .map_err(error)?
                        .origin()
                        .ascii_serialization();
                }
                tty.say(
                    "Enter one HTTPS origin without a path, password or query.",
                    "경로·비밀번호·쿼리 없는 HTTPS 주소를 입력해주세요.",
                )?
            };
            (
                if mode == 1 {
                    Network::Tailscale
                } else {
                    Network::Https
                },
                origin,
            )
        };
        let data = data
            .map(Path::to_path_buf)
            .or_else(crate::default_data_dir)
            .ok_or("setup_data_directory_required")?;
        if data.exists() {
            return Err("setup_existing_data_refused_choose_new_directory".into());
        }
        directories(parent(&prefix)?)?;
        directories(parent(&data)?)?;
        tty.line(&format!(
            "{}: {}",
            tty.lang.text("Private data", "개인 데이터"),
            data.display()
        ))?;
        let initial = State {
            format: 1,
            language: tty.lang,
            port,
            service: service::Kind::None,
            launcher: None,
            service_file: None,
            next_page: "/app".into(),
            removing: false,
            autostart: true,
            desktop_files: Vec::new(),
        };
        super::install(
            bundle,
            key,
            &prefix,
            &data,
            &origin,
            &network,
            Some(&json(&initial)?),
        )?
    } else {
        // Do not add a lock file to an unrelated or legacy partial prefix.
        installation(&prefix)?;
        // Acquire before reading state: a paused wizard cannot overwrite a later
        // autostart change or race uninstall. The same guard covers all prompts.
        Some(desktop::control_lock(&prefix)?)
    };
    let (runtime, active) = match lock(&prefix, true) {
        Ok(guard) => (guard, false),
        Err(e) if e == "installation_runtime_busy" => (lock(&prefix, false)?, true),
        Err(e) => return Err(e),
    };
    no_pending(&prefix)?;
    if current(&prefix)?.version != own_probe().version {
        return Err("setup_existing_version_requires_signed_update".into());
    }
    if !fresh {
        tty.say(
            "Resuming your installed Moosuri. Existing accounts and settings are preserved.",
            "기존 무수리 설치를 이어갑니다. 계정과 설정은 유지합니다.",
        )?;
    }
    let i = installation(&prefix)?;
    let mut state = match load(&prefix) {
        Ok(s) => s,
        Err(e) => {
            if prefix.join("setup.json").exists() {
                return Err(e);
            }
            // Recover a first install interrupted immediately after signed activation.
            let port = if matches!(i.network, Network::Local) {
                url::Url::parse(&i.origin)
                    .map_err(error)?
                    .port()
                    .ok_or("setup_port_required")?
            } else {
                // Older low-level installs did not record the bind port. Never
                // invent one for an existing HTTPS proxy or Tailscale mapping.
                loop {
                    let value = tty.input(
                        "Internal port configured in your proxy/Serve",
                        "프록시/Serve에 설정한 내부 포트",
                        "3210",
                    )?;
                    if let Ok(port) = value.parse::<u16>()
                        && port >= 1024
                    {
                        break port;
                    }
                }
            };
            State {
                format: 1,
                language: tty.lang,
                port,
                service: service::Kind::None,
                launcher: None,
                service_file: None,
                next_page: "/app".into(),
                removing: false,
                autostart: true,
                desktop_files: Vec::new(),
            }
        }
    };
    if state.removing {
        return Err("setup_pending_uninstall_resume_with_manager_uninstall".into());
    }
    state.language = tty.lang;
    save(&prefix, &state)?;
    if active {
        if !Readiness::new(&i, &state)?
            .wait(Duration::from_secs(20))
            .await
        {
            return Err("setup_service_not_ready_check_moosuri_status".into());
        }
        tty.say(
            "Moosuri is already running. Open settings to change your model or invite people.",
            "무수리가 이미 실행 중입니다. 설정에서 모델을 바꾸거나 사용자를 초대하세요.",
        )?;
        service::launcher(&prefix, &mut state)?;
        save(&prefix, &state)?;
        desktop::offer(&prefix, &mut state, tty)?;
        launch_menu(&prefix, &state, tty)?;
        if !options.no_browser {
            let _ = browser(&format!("{}/app", i.origin));
        }
        return Ok(());
    }
    tty.say("\n2/5 · Administrator", "\n2/5 · 관리자 계정")?;
    let store = Store::open(&i.data_dir).map_err(|e| e.to_string())?;
    if !store.initialized().await.map_err(|e| e.to_string())? {
        let name = loop {
            let s = tty.input("Your name", "이름", "")?;
            if !s.is_empty() && s.len() <= 200 {
                break s;
            }
        };
        let email = loop {
            let s = tty.input(
                "Email (used to sign in; no email is sent)",
                "이메일 (로그인용이며 메일은 보내지 않습니다)",
                "",
            )?;
            if valid_email(&s) {
                break s;
            }
            tty.say(
                "Enter a valid email address.",
                "올바른 이메일 주소를 입력해주세요.",
            )?
        };
        let password = loop {
            let p = tty.secret(
                "Password (12+ characters; Enter creates a temporary one)",
                "비밀번호 (12자 이상, Enter로 임시 비밀번호 생성)",
            )?;
            if p.is_empty() {
                break p;
            }
            if p.len() < 12 || p.len() > 256 {
                tty.say(
                    "Use 12–256 bytes.",
                    "12자 이상으로 입력해주세요 (최대 256바이트).",
                )?;
                continue;
            }
            if p == tty.secret("Confirm password", "비밀번호 확인")? {
                break p;
            }
            tty.say(
                "Passwords did not match. Try again.",
                "비밀번호가 다릅니다. 다시 입력해주세요.",
            )?;
        };
        let temporary = password.is_empty();
        let password = if temporary {
            use rand::{Rng, distributions::Alphanumeric};
            zeroize::Zeroizing::new(
                rand::rngs::OsRng
                    .sample_iter(Alphanumeric)
                    .take(32)
                    .map(char::from)
                    .collect::<String>(),
            )
        } else {
            password
        };
        store
            .bootstrap(name, email.clone(), password.to_string(), temporary)
            .await
            .map_err(|e| e.to_string())?;
        if temporary {
            tty.say(
                "Temporary password (shown here once). Save it; sign in and change it first:",
                "임시 비밀번호를 한 번 표시합니다. 보관한 뒤 로그인하여 먼저 변경하세요:",
            )?;
            tty.line(&password)?;
        }
        tty.say(
            "\n3/5 · Model connection (optional)",
            "\n3/5 · 모델 연결 (선택)",
        )?;
        state.next_page = connection::configure(&store, tty, &email, &password, temporary).await?;
        save(&prefix, &state)?;
    } else {
        tty.say(
            "Administrator already exists. No password or model settings were changed.",
            "관리자 계정이 이미 있습니다. 비밀번호와 모델 설정은 바꾸지 않았습니다.",
        )?;
    }
    drop(store);
    tty.say("\n4/5 · Command and startup", "\n4/5 · 명령어와 자동 시작")?;
    service::launcher(&prefix, &mut state)?;
    save(&prefix, &state)?;
    if !matches!(state.service, service::Kind::None) {
        let kind = state.service;
        service::install(&prefix, &mut state, kind)?;
        save(&prefix, &state)?;
    } else {
        let available = service::available();
        if !matches!(available, service::Kind::None)
            && tty.select(
                "When should Moosuri run?",
                "언제 실행할까요?",
                &[
                    (
                        "Start now and at login (recommended)",
                        "지금 시작하고 로그인할 때 자동 시작 (추천)",
                    ),
                    (
                        "Only when I run moosuri start --foreground",
                        "moosuri start --foreground로 직접 실행",
                    ),
                ],
                0,
            )? == 0
        {
            service::install(&prefix, &mut state, available)?;
            save(&prefix, &state)?;
        } else if matches!(available, service::Kind::None) {
            tty.say("No user service manager is available. Use foreground mode. For WSL2, see the systemd steps in the installation guide.","사용자 서비스 관리자를 사용할 수 없습니다. 터미널 실행을 사용하세요. WSL2의 systemd 설정은 설치 안내에 있습니다.")?;
        }
    }
    desktop::offer(&prefix, &mut state, tty)?;
    tty.say("\n5/5 · Open Moosuri", "\n5/5 · 무수리 열기")?;
    tty.line(&format!("{}{}", i.origin, state.next_page))?;
    tty.say("Commands: moosuri start · moosuri stop · moosuri status · moosuri open\nModel, Google and invitation settings are available after sign-in.","명령어: moosuri start · moosuri stop · moosuri status · moosuri open\n로그인한 뒤 모델·Google 연결·사용자 초대를 설정할 수 있습니다.")?;
    service::path_hint(tty)?;
    match i.network {
        Network::Tailscale => tty.line(&format!(
            "tailscale serve --bg http://127.0.0.1:{}",
            state.port
        ))?,
        Network::Https => tty.line(&format!(
            "{}: {}",
            tty.lang.text(
                "Proxy example (adjust port if changed)",
                "프록시 예시 (포트를 바꿨으면 수정)"
            ),
            prefix.join("deployment/proxy.nginx.conf").display()
        ))?,
        Network::Local => (),
    };
    drop(runtime);
    drop(control);
    if !matches!(state.service, service::Kind::None) {
        start(&Start {
            prefix: Some(prefix.clone()),
            foreground: false,
        })
        .await?;
        launch_menu(&prefix, &state, tty)?;
        if !options.no_browser {
            let _ = browser(&format!("{}{}", i.origin, state.next_page));
        }
    } else if tty.select(
        "Run in this terminal now?",
        "이 터미널에서 지금 실행할까요?",
        &[
            ("Run now (Ctrl+C to stop)", "지금 실행 (Ctrl+C로 종료)"),
            ("Finish setup; start later", "설치 완료, 나중에 실행"),
        ],
        0,
    )? == 0
    {
        if !options.no_browser {
            // A separate bounded helper survives the exec into the signed server.
            // It opens only after readiness; the URL above remains the fallback.
            let _ = dispatch_detached(
                Process::new(prefix.join("manager"))
                    .args(["open", "--wait-ready", "--prefix"])
                    .arg(&prefix)
                    .arg("--page")
                    .arg(&state.next_page),
            );
        }
        start(&Start {
            prefix: Some(prefix),
            foreground: true,
        })
        .await?;
    }
    Ok(())
}
fn valid_email(s: &str) -> bool {
    s.len() <= 254
        && s.is_ascii()
        && !s.chars().any(|c| c.is_whitespace() || c.is_control())
        && s.split_once('@').is_some_and(|(a, b)| {
            !a.is_empty()
                && a.len() <= 64
                && b.contains('.')
                && !b.starts_with('.')
                && !b.ends_with('.')
                && !b.contains('@')
        })
}
fn launch_menu(prefix: &Path, state: &State, tty: &mut Terminal) -> Result<()> {
    if desktop::launch_if_installed(prefix, state).is_err() {
        tty.say("The menu could not start. Continue in the browser; retry with moosuri desktop, or check moosuri status.", "메뉴를 시작하지 못했습니다. 웹에서 계속 사용할 수 있습니다. moosuri desktop으로 다시 열거나 moosuri status로 확인하세요.")?;
    }
    Ok(())
}
pub(super) fn check_start_allowed(prefix: &Path) -> Result<()> {
    if prefix.join("setup.json").exists() && load(prefix)?.removing {
        return Err("setup_uninstall_pending".into());
    }
    Ok(())
}
pub async fn start(options: &Start) -> Result<()> {
    let p = prefix(&options.prefix)?;
    let i = installation(&p)?;
    let s = load(&p)?;
    check_start_allowed(&p)?;
    no_pending(&p)?;
    if options.foreground {
        return super::run(&Command::Run {
            prefix: p,
            args: server_args(&i, &s),
        });
    }
    let _control = desktop::control_lock(&p)?;
    let s = load(&p)?;
    check_start_allowed(&p)?;
    if matches!(s.service, service::Kind::None) {
        return Err("setup_no_user_service_run_moosuri_start_foreground".into());
    }
    let probe = Readiness::new(&i, &s)?;
    if !service::running(&p, &s) || !probe.check().await {
        service::start(&p, &s)?;
        if !probe.wait(Duration::from_secs(20)).await {
            return Err("setup_service_not_ready_check_moosuri_status".into());
        }
    }
    println!(
        "{}: {}/app",
        s.language.text("Moosuri is ready", "무수리 실행 완료"),
        i.origin
    );
    Ok(())
}
pub fn stop(location: &Location) -> Result<()> {
    let p = prefix(&location.prefix)?;
    let _control = desktop::control_lock(&p)?;
    let s = load(&p)?;
    if matches!(s.service, service::Kind::None) {
        return Err("setup_foreground_stop_use_ctrl_c".into());
    }
    service::stop(&p, &s)?;
    let _stopped = stopped_runtime(&p, Duration::from_secs(20))?;
    println!(
        "{}",
        s.language.text(
            "Moosuri service stopped. Data retained.",
            "무수리 서비스를 중지했습니다. 데이터는 유지합니다."
        )
    );
    Ok(())
}
// launchctl can acknowledge bootout before the runtime finishes shutdown.
// Confirm writer release before claiming a stop or starting runtime removal.
fn stopped_runtime(prefix: &Path, budget: Duration) -> Result<File> {
    let end = std::time::Instant::now() + budget;
    loop {
        match lock(prefix, true) {
            Ok(guard) => return Ok(guard),
            Err(e) if e == "installation_runtime_busy" => {
                if std::time::Instant::now() >= end {
                    return Err("setup_service_stop_pending_check_moosuri_status".into());
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(e) => return Err(e),
        }
    }
}
pub async fn status(location: &Location) -> Result<()> {
    let p = prefix(&location.prefix)?;
    let i = installation(&p)?;
    let s = load(&p)?;
    super::run(&Command::Status { prefix: p.clone() })?;
    println!(
        "{}: {}\n{}/app",
        s.language.text("Service ready", "서비스 준비 상태"),
        ready(&i, &s).await,
        i.origin
    );
    println!("{}", service::log_hint(&p, &s));
    Ok(())
}
pub async fn open(options: &Open) -> Result<()> {
    let p = prefix(&options.prefix)?;
    let i = installation(&p)?;
    let s = load(&p)?;
    check_start_allowed(&p)?;
    no_pending(&p)?;
    let probe = Readiness::new(&i, &s)?;
    let healthy = if options.wait_ready {
        probe.wait(Duration::from_secs(20)).await
    } else {
        probe.check().await
    };
    if !healthy {
        return Err("setup_service_not_ready_check_moosuri_status".into());
    }
    browser(&format!("{}{}", i.origin, options.page))
}
fn browser(url: &str) -> Result<()> {
    let parsed = url::Url::parse(url).map_err(error)?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("setup_browser_url_invalid".into());
    }
    let candidates = if cfg!(target_os = "macos") {
        vec!["open"]
    } else {
        vec!["wslview", "xdg-open", "explorer.exe"]
    };
    for c in candidates {
        if dispatch_detached(Process::new(c).arg(url)) {
            return Ok(());
        }
    }
    println!("{}", url);
    Err("setup_browser_unavailable_open_printed_url".into())
}
// Some desktop openers stay attached to the viewer; the menu must not wait for it.
fn dispatch_detached(command: &mut Process) -> bool {
    let Ok(mut child) = command
        // A menu/opener must survive the install terminal closing (SIGHUP) and
        // must not share the foreground server's Ctrl+C process group.
        .process_group(0)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    let end = std::time::Instant::now() + Duration::from_millis(250);
    while std::time::Instant::now() < end {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Err(_) => return false,
            _ => std::thread::sleep(Duration::from_millis(10)),
        }
    }
    std::thread::spawn(move || {
        let _ = child.wait();
    });
    true
}
pub fn uninstall(location: &Location) -> Result<()> {
    let p = prefix(&location.prefix)?;
    let _control = desktop::control_lock(&p)?;
    let mut s = load(&p)?;
    let mut tty = Terminal::open(Some(s.language), false)?;
    if !s.removing
        && tty.select(
            "Remove Moosuri's runtime and user service? Private data will remain.",
            "무수리 실행 파일과 사용자 서비스를 제거할까요? 개인 데이터는 유지합니다.",
            &[
                ("Keep installed", "설치 유지"),
                ("Remove runtime; keep data", "실행 파일 제거, 데이터 유지"),
            ],
            0,
        )? == 0
    {
        return Ok(());
    }
    service::preflight_remove(&p, &s)?;
    desktop::preflight_remove(&p, &s)?;
    service::stop(&p, &s)?;
    {
        let _guard = stopped_runtime(&p, Duration::from_secs(20))?;
        let i = installation(&p)?;
        let _data = maintenance(&i.data_dir)?;
        no_pending(&p)?;
        s.removing = true;
        save(&p, &s)?;
        desktop::remove(&p, &mut s)?;
        service::remove(&p, &mut s)?;
    }
    super::run(&Command::Uninstall { prefix: p.clone() })?;
    fs::remove_file(p.join("setup.json")).map_err(error)?;
    for name in ["desktop.lock", "control.lock"] {
        let _ = remove_exact(&p.join(name), &hash(b""));
    }
    let _ = fs::remove_dir(&p);
    Ok(())
}
