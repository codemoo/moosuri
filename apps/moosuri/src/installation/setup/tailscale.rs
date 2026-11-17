//! Host-owned Tailscale Serve integration; application account authentication stays separate.
mod api;
mod config;
mod state;
#[cfg(test)]
mod tests;
use super::*;
use api::Api;
use config::Route;
use serde_json::{Value, json};
pub(in crate::installation) use state::check_start;
use state::{Binding, Phase, load_binding, remove_binding};

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    pub prefix: Option<PathBuf>,
    /// Advanced: an existing local tailscaled Unix socket.
    #[arg(long)]
    pub socket: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Action,
}
#[derive(clap::Subcommand)]
pub enum Action {
    /// Connect private HTTPS access; sign in through Tailscale if needed.
    Connect {
        #[arg(long)]
        no_browser: bool,
    },
    /// Inspect connection, owned route and HTTPS readiness without changing them.
    Status,
    /// Remove only this installation's unchanged route and restore its prior origin.
    Disconnect,
}
pub async fn run(options: &Options) -> Result<()> {
    let p = prefix(&options.prefix)?;
    let s = load(&p)?;
    let result = match options.command {
        Action::Connect { no_browser } => {
            connect(&p, options.socket.as_deref(), !no_browser, true).await
        }
        Action::Status => status(&p, options.socket.as_deref()).await,
        Action::Disconnect => disconnect(&p, options.socket.as_deref()).await,
    };
    if let Err(code) = &result {
        eprintln!("{}", hint(code, s.language));
    }
    result
}
pub(super) fn hint(code: &str, lang: Language) -> &'static str {
    let (en, ko) = match code {
        "tailscale_not_available"
        | "tailscale_socket_invalid"
        | "tailscale_transport_unavailable" => (
            "Open Tailscale and connect this computer. Install it from https://tailscale.com/download if needed. WSL: use the Windows-host/manual setup guide.",
            "Tailscale 앱을 열고 이 컴퓨터를 연결하세요. 미설치 시 https://tailscale.com/download 에서 설치하세요. WSL은 Windows 호스트 수동 연결 안내를 따릅니다.",
        ),
        "tailscale_login_required" | "tailscale_login_timeout" => (
            "Finish Tailscale sign-in, then retry moosuri tailscale connect.",
            "Tailscale 로그인을 마친 뒤 moosuri tailscale connect를 다시 실행하세요.",
        ),
        "tailscale_https_required" => (
            "Enable HTTPS certificates for this tailnet in https://login.tailscale.com/admin/dns, then retry. Tailscale's certificate disclosure notice applies.",
            "https://login.tailscale.com/admin/dns 에서 이 네트워크의 HTTPS 인증서를 켠 뒤 다시 실행하세요. Tailscale의 인증서 이름 공개 안내를 확인하세요.",
        ),
        "tailscale_machine_approval_required" => (
            "Approve this device in your Tailscale admin console, then retry.",
            "Tailscale 관리자 화면에서 이 기기를 승인한 뒤 다시 실행하세요.",
        ),
        "tailscale_operator_required" => (
            "This OS account cannot manage Tailscale. Use Tailscale's operator/admin configuration; Moosuri never invokes sudo.",
            "이 OS 계정에 Tailscale 관리 권한이 없습니다. Tailscale의 operator/관리자 설정을 확인하세요.",
        ),
        "tailscale_existing_route_preserved"
        | "tailscale_funnel_conflict_preserved"
        | "tailscale_unowned_route_preserved" => (
            "HTTPS port 443 already has another or unowned route. It was preserved. Choose a dedicated Tailscale device or resolve that route before retrying.",
            "HTTPS 443 포트에 기존 연결이 있어 보존했습니다. 전용 Tailscale 기기를 쓰거나 기존 연결을 정리한 뒤 다시 실행하세요.",
        ),
        "tailscale_foreground_stop_required" => (
            "Stop Moosuri with Ctrl+C in its terminal, reconnect, then run moosuri open --start.",
            "무수리를 실행한 터미널에서 Ctrl+C로 중지하고 연결한 뒤 moosuri open --start로 실행하세요.",
        ),
        "tailscale_change_pending_retry_connect_or_disconnect"
        | "tailscale_configuration_changed_retry" => (
            "Connection setup is incomplete or configuration changed. Retry moosuri tailscale connect, or use disconnect to restore the previous access mode.",
            "연결 설정이 중단됐거나 다른 설정 변경이 있었습니다. moosuri tailscale connect로 재개하거나 disconnect로 이전 접속 방식을 복구하세요.",
        ),
        "tailscale_hostname_changed" => (
            "The Tailscale device/address changed. Switch back to the original Tailscale profile to disconnect, then connect the desired profile.",
            "Tailscale 기기나 주소가 바뀌었습니다. 원래 Tailscale 프로필에서 disconnect한 뒤 원하는 프로필로 다시 연결하세요.",
        ),
        _ => (
            "Tailscale integration could not finish. Check moosuri tailscale status and the installation guide; existing data is retained.",
            "Tailscale 연동을 완료하지 못했습니다. moosuri tailscale status와 설치 안내를 확인하세요. 기존 데이터는 유지됩니다.",
        ),
    };
    lang.text(en, ko)
}
async fn connected(api: &Api, mut tty: Option<&mut Terminal>, open_browser: bool) -> Result<Value> {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    let mut initialized = false;
    let mut woke = false;
    let mut requested = false;
    let mut opened = zeroize::Zeroizing::new(String::new());
    loop {
        let status = api.status().await?;
        match status["BackendState"].as_str() {
            Some("Running") => {
                config::node_origin(&status)?;
                return Ok(status);
            }
            Some("NeedsMachineAuth") => return Err("tailscale_machine_approval_required".into()),
            Some("Stopped") if !woke => {
                api.wake().await?;
                woke = true;
            }
            Some("NoState") if !initialized => {
                api.initialize().await?;
                initialized = true;
            }
            Some("NeedsLogin") => {
                let tty = tty.as_deref_mut().ok_or("tailscale_login_required")?;
                let auth = status["AuthURL"].as_str().unwrap_or("");
                if auth.is_empty() && !requested {
                    api.login().await?;
                    requested = true;
                } else if !auth.is_empty() && opened.as_str() != auth {
                    let u = url::Url::parse(auth).map_err(|_| "tailscale_login_required")?;
                    if u.scheme() != "https"
                        || u.host_str() != Some("login.tailscale.com")
                        || !u.username().is_empty()
                        || u.password().is_some()
                        || u.port().is_some()
                    {
                        return Err("tailscale_login_required".into());
                    }
                    tty.say(
                        "Finish Tailscale sign-in in your browser. Waiting up to two minutes…",
                        "브라우저에서 Tailscale 로그인을 마쳐주세요. 최대 2분 동안 기다립니다…",
                    )?;
                    tty.line(auth)?; // Controlling terminal only, never redirected logs.
                    if open_browser {
                        let _ = browser_impl(auth, false);
                    }
                    *opened = auth.into();
                }
            }
            Some("Starting") | Some("Stopped") | Some("NoState") => (),
            _ => return Err("tailscale_not_connected".into()),
        }
        if tokio::time::Instant::now() >= deadline {
            return Err("tailscale_login_timeout".into());
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
/// First setup can continue locally if Tailscale is not installed/approved yet.
pub(super) enum Prepared {
    Automatic,
    Local,
    Manual(String),
}
pub(super) async fn prepare(tty: &mut Terminal, open_browser: bool) -> Result<Prepared> {
    loop {
        let result = match Api::discover(None) {
            Ok(api) => connected(&api, Some(tty), open_browser).await.map(|_| ()),
            Err(e) => Err(e),
        };
        match result {
            Ok(()) => {
                tty.say("Tailscale is ready. Private HTTPS access will be connected after account setup.","Tailscale을 확인했습니다. 계정 설정 후 전용 HTTPS 접속을 연결합니다.")?;
                return Ok(Prepared::Automatic);
            }
            Err(e) => tty.line(hint(&e, tty.lang))?,
        }
        let action = tty.select(
            "Continue Tailscale setup",
            "Tailscale 연결 계속하기",
            &[
                (
                    "Check again after configuring Tailscale",
                    "Tailscale 설정 후 다시 확인",
                ),
                (
                    "Use this computer for now; connect later",
                    "지금은 이 컴퓨터에서 사용, 나중에 연결",
                ),
                (
                    "Enter Tailscale HTTPS address manually (WSL/advanced)",
                    "Tailscale HTTPS 주소 직접 입력 (WSL/고급)",
                ),
            ],
            1,
        )?;
        if action == 1 {
            return Ok(Prepared::Local);
        }
        if action == 2 {
            loop {
                let origin = tty.input("Tailscale HTTPS address", "Tailscale HTTPS 주소", "")?;
                if config::host(&origin).is_ok() {
                    return Ok(Prepared::Manual(origin));
                }
                tty.say(
                    "Use https://device.tailnet.ts.net without a path or port.",
                    "경로나 포트 없이 https://device.tailnet.ts.net 형식으로 입력하세요.",
                )?;
            }
        }
    }
}
fn admission(p: &Path) -> Result<()> {
    if load(p)?.removing {
        return Err("setup_uninstall_pending".into());
    }
    no_pending(p)?;
    if current(p)?.version != own_probe().version {
        return Err("setup_existing_version_requires_signed_update".into());
    }
    Ok(())
}
fn stop_for_change(p: &Path, s: &State) -> Result<File> {
    if matches!(s.service, service::Kind::None) {
        return lock(p, true).map_err(|e| {
            if e == "installation_runtime_busy" {
                "tailscale_foreground_stop_required".into()
            } else {
                e
            }
        });
    }
    service::stop(p, s)?;
    stopped_runtime(p, Duration::from_secs(20))
}
pub(super) async fn connect(
    p: &Path,
    socket: Option<&Path>,
    open_browser: bool,
    start_after: bool,
) -> Result<()> {
    let _control = desktop::control_lock(p)?;
    let mut preparation = Some(lock(p, false)?);
    admission(p)?;
    let expected_release = current(p)?;
    let mut i = installation(p)?;
    let mut s = load(p)?;
    let existing = load_binding(p)?;
    if let Some(binding) = &existing {
        binding.validate(p, &i, &s)?;
    }
    let endpoint = socket
        .or_else(|| existing.as_ref().and_then(|b| b.socket.as_deref()))
        .map(Path::to_path_buf);
    let api = Api::discover(endpoint.as_deref())?;
    let mut tty = Terminal::open(Some(s.language), true).ok();
    let node = connected(&api, tty.as_mut(), open_browser).await?;
    let origin = config::node_origin(&node)?;
    let (config, etag) = api.config().await?;
    let found = config::route(&config, &origin, s.port)?;
    let mut binding = match existing {
        Some(b) => {
            b.validate(p, &i, &s)?;
            if b.origin != origin || b.node_id != config::node_id(&node)? {
                return Err("tailscale_hostname_changed".into());
            }
            b
        }
        None => {
            if found != Route::Empty {
                return Err("tailscale_unowned_route_preserved".into());
            }
            let resume = !matches!(s.service, service::Kind::None)
                && (start_after || service::running(p, &s));
            Binding::prepare(
                p,
                &i,
                &s,
                origin.clone(),
                config::node_id(&node)?,
                endpoint.clone(),
                resume,
            )?
        }
    };
    binding.socket = endpoint;
    if binding.phase == Phase::Disconnecting {
        return Err("tailscale_change_pending_retry_connect_or_disconnect".into());
    }
    if binding.phase != Phase::Connected || found != Route::Matching {
        // Validate before stopping a working server or claiming the external route.
        binding.validate(p, &i, &s)?;
        let guard = if matches!(s.service, service::Kind::None) {
            drop(preparation.take());
            let guard = stop_for_change(p, &s)?;
            if current(p)? != expected_release {
                return Err("installation_runtime_changed_retry".into());
            }
            binding.phase = Phase::Connecting;
            binding.save(p)?;
            guard
        } else {
            binding.phase = Phase::Connecting;
            binding.save(p)?;
            drop(preparation.take());
            let guard = stop_for_change(p, &s)?;
            if current(p)? != expected_release {
                return Err("installation_runtime_changed_retry".into());
            }
            guard
        };
        if found == Route::Empty {
            api.set_config(&config::changed(&config, &origin, s.port, true)?, &etag)
                .await?;
        }
        let (actual, _) = api.config().await?;
        if config::route(&actual, &origin, s.port)? != Route::Matching {
            return Err("tailscale_configuration_changed_retry".into());
        }
        super::super::upgrade_fault("tailscale_after_serve")?;
        binding.apply_origin(p, &mut i, &s, true)?;
        super::super::upgrade_fault("tailscale_after_origin")?;
        binding.phase = Phase::Connected;
        binding.save(p)?;
        drop(guard);
    }
    binding.save(p)?;
    if s.tailscale_requested {
        s.tailscale_requested = false;
        save(p, &s)?;
    }
    if !matches!(s.service, service::Kind::None) && (start_after || binding.resume_service) {
        start_service(p, &i, &s).await?;
    }
    println!(
        "{}: {origin}/app",
        s.language.text(
            "Tailscale private HTTPS configured",
            "Tailscale 전용 HTTPS 연결 설정 완료"
        )
    );
    let accessible =
        Readiness::new(&i, &s)?.observe().await == Observation::Ready && public_ready(&i).await;
    println!(
        "{}",
        if accessible {
            s.language.text(
                "HTTPS is ready. Sign in with your Moosuri account.",
                "HTTPS 접속을 확인했습니다. 무수리 계정으로 로그인하세요.",
            )
        } else {
            s.language.text("HTTPS is not reachable yet. Start Moosuri and check Tailscale on both devices; accounts remain separate from Tailscale.","아직 HTTPS 접속을 확인하지 못했습니다. 무수리를 시작하고 두 기기의 Tailscale 연결을 확인하세요. 무수리 계정 로그인은 별도입니다.")
        }
    );
    if open_browser && accessible {
        browser(&format!("{origin}/app"))?;
    }
    Ok(())
}
async fn disconnect(p: &Path, socket: Option<&Path>) -> Result<()> {
    let _control = desktop::control_lock(p)?;
    let mut preparation = Some(lock(p, false)?);
    admission(p)?;
    let expected_release = current(p)?;
    let mut i = installation(p)?;
    let mut s = load(p)?;
    let Some(mut b) = load_binding(p)? else {
        println!(
            "{}",
            s.language.text(
                "No Moosuri-managed Tailscale connection.",
                "무수리가 관리하는 Tailscale 연결이 없습니다."
            )
        );
        return Ok(());
    };
    b.validate(p, &i, &s)?;
    let api = Api::discover(socket.or(b.socket.as_deref()))?;
    if config::node_id(&api.status().await?)? != b.node_id {
        return Err("tailscale_hostname_changed".into());
    }
    let (config, etag) = api.config().await?;
    let own = matches!(
        config::route(&config, &b.origin, b.port),
        Ok(Route::Matching)
    );
    if b.phase != Phase::Disconnecting {
        b.resume_service =
            service::running(p, &s) || (b.phase == Phase::Connecting && b.resume_service);
        b.phase = Phase::Disconnecting;
        b.save(p)?;
    }
    drop(preparation.take());
    let guard = stop_for_change(p, &s)?;
    if current(p)? != expected_release {
        return Err("installation_runtime_changed_retry".into());
    }
    if own {
        api.set_config(&config::changed(&config, &b.origin, b.port, false)?, &etag)
            .await?;
    }
    // Changed/unowned routes remain intact. Restore only our origin and examples.
    b.apply_origin(p, &mut i, &s, false)?;
    s.tailscale_requested = false;
    save(p, &s)?;
    remove_binding(p)?;
    drop(guard);
    if !matches!(s.service, service::Kind::None) && b.resume_service {
        start_service(p, &i, &s).await?;
    }
    println!(
        "{}: {}/app",
        s.language.text(
            "Previous Moosuri access restored",
            "이전 무수리 접속 방식으로 복구했습니다"
        ),
        i.origin
    );
    if !own {
        println!(
            "{}",
            s.language.text(
                "An absent or changed external route was left untouched.",
                "없거나 변경된 외부 연결은 보존했습니다."
            )
        );
    }
    Ok(())
}
async fn status(p: &Path, socket: Option<&Path>) -> Result<()> {
    let i = installation(p)?;
    let s = load(p)?;
    let binding = load_binding(p)?;
    if let Some(b) = &binding {
        b.validate(p, &i, &s)?;
    }
    let api = Api::discover(socket.or_else(|| binding.as_ref().and_then(|b| b.socket.as_deref())))?;
    let node = api.status().await?;
    let origin = config::node_origin(&node)?;
    let (config, _) = api.config().await?;
    let managed = binding.as_ref().is_some_and(|b| {
        b.origin == origin
            && config::node_id(&node).is_ok_and(|id| id == b.node_id)
            && b.phase == Phase::Connected
            && b.validate(p, &i, &s).is_ok()
    });
    let matching = config::route(&config, &origin, s.port)? == Route::Matching;
    println!(
        "Tailscale: {}\n{origin}/app",
        s.language.text("connected", "연결됨")
    );
    println!(
        "{}: {}",
        s.language
            .text("Moosuri-managed HTTPS route", "무수리 관리 HTTPS 연결"),
        managed && matching
    );
    if binding
        .as_ref()
        .is_some_and(|b| b.phase != Phase::Connected)
    {
        println!(
            "{}",
            hint(
                "tailscale_change_pending_retry_connect_or_disconnect",
                s.language
            )
        );
    }
    println!(
        "HTTPS: {}",
        managed
            && matching
            && Readiness::new(&i, &s)?.observe().await == Observation::Ready
            && public_ready(&i).await
    );
    Ok(())
}
pub(super) async fn public_ready(i: &Installation) -> bool {
    let Ok(client) = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(4))
        .build()
    else {
        return false;
    };
    client
        .get(format!("{}/health/ready", i.origin))
        .send()
        .await
        .is_ok_and(|r| {
            r.status() == reqwest::StatusCode::NO_CONTENT
                && r.headers()
                    .get("x-moosuri-runtime")
                    .and_then(|v| v.to_str().ok())
                    == Some(i.id.as_str())
        })
}

pub(super) async fn detach_for_removal(p: &Path) -> Result<()> {
    let Some(b) = load_binding(p)? else {
        return Ok(());
    };
    b.validate(p, &installation(p)?, &load(p)?)?;
    let api = Api::discover(b.socket.as_deref())?;
    if config::node_id(&api.status().await?)? != b.node_id {
        return Err("tailscale_hostname_changed".into());
    }
    let (current, etag) = api.config().await?;
    if matches!(
        config::route(&current, &b.origin, b.port),
        Ok(Route::Matching)
    ) {
        api.set_config(&config::changed(&current, &b.origin, b.port, false)?, &etag)
            .await?;
    }
    super::super::upgrade_fault("tailscale_after_remove")?;
    remove_binding(p)
}
