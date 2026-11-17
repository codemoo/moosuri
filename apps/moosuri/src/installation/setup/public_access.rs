//! Verify a user-configured domain before activating public self-hosted HTTPS.
mod record;
#[cfg(test)]
mod tests;
pub mod tls;
mod verification;
use super::*;
use record::{Phase, Record};
use std::net::{IpAddr, SocketAddr};
const NAME: &str = "public-access.json";
#[derive(Args)]
pub struct Options {
    #[arg(long)]
    pub prefix: Option<PathBuf>,
    #[command(subcommand)]
    pub command: Action,
}
#[derive(clap::Subcommand)]
pub enum Action {
    /// Configure a public domain; no arguments opens the bilingual guide.
    Connect {
        #[arg(long)]
        domain: Option<String>,
        /// Native TLS port. External TCP443 must reach this listener.
        #[arg(long, default_value_t = 8443)]
        tls_port: u16,
        /// Use an existing TLS proxy instead of issuing certificates here.
        #[arg(long)]
        proxy: bool,
        #[arg(long)]
        no_browser: bool,
    },
    /// Check DNS, local runtime and public HTTPS separately.
    Status,
    /// Restore prior application access. DNS records and certificate keys are retained.
    Disconnect,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(super) struct Request {
    host: String,
    /// None means an operator-managed existing reverse proxy.
    tls_port: Option<u16>,
}
pub async fn run(options: &Options) -> Result<()> {
    let p = prefix(&options.prefix)?;
    let lang = load(&p)?.language;
    let result = match &options.command {
        Action::Connect {
            domain,
            tls_port,
            proxy,
            no_browser,
        } => {
            let request = domain
                .as_deref()
                .map(|domain| -> Result<Request> {
                    Ok(Request {
                        host: host(domain)?,
                        tls_port: (!proxy).then_some(*tls_port),
                    })
                })
                .transpose()?;
            connect(&p, request, !no_browser, true).await
        }
        Action::Status => status(&p).await,
        Action::Disconnect => disconnect(&p).await,
    };
    if result.is_err() {
        eprintln!("{}", lang.text(
            "Public access is incomplete. Retry connect or disconnect to restore prior access. Register DNS to this server first; native HTTPS needs external TCP443 forwarded to its TLS port. Existing data is retained. Guide: https://github.com/codemoo/moosuri/blob/main/docs/public-access.md",
            "공개 접속을 마치지 못했습니다. connect로 이어가거나 disconnect로 이전 접속을 복원하세요. DNS는 미리 이 서버로 등록해야 하고, 자동 HTTPS에는 외부 TCP443→TLS 포트 연결이 필요합니다. 데이터는 유지됩니다. 안내: https://github.com/codemoo/moosuri/blob/main/docs/public-access.md"
        ));
    }
    result
}
fn host(value: &str) -> Result<String> {
    let value = if value.contains("://") {
        value.to_owned()
    } else {
        format!("https://{value}")
    };
    let u = url::Url::parse(&value).map_err(|_| "public_domain_invalid")?;
    let h = u.host_str().ok_or("public_domain_invalid")?;
    if u.scheme() != "https"
        || u.origin().ascii_serialization() != value
        || u.port().is_some()
        || h.parse::<IpAddr>().is_ok()
        || !h.contains('.')
        || h.len() > 253
        || [".localhost", ".local", ".ts.net", ".onion"]
            .iter()
            .any(|s| h.ends_with(s))
        || h.split('.').any(|p| {
            p.is_empty()
                || p.len() > 63
                || p.starts_with('-')
                || p.ends_with('-')
                || !p
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
        })
    {
        return Err("public_domain_invalid".into());
    }
    Ok(h.into())
}
impl Request {
    fn validate(&self) -> Result<()> {
        if host(&self.host)? != self.host || self.tls_port == Some(0) {
            return Err("public_access_configuration_invalid".into());
        }
        Ok(())
    }
    fn origin(&self) -> String {
        format!("https://{}", self.host)
    }
    fn network(&self) -> Network {
        if self.tls_port.is_some() {
            Network::Public
        } else {
            Network::Https
        }
    }
}
pub(super) fn prepare(tty: &mut Terminal) -> Result<Request> {
    tty.say("Public self-hosting needs a domain you own and a reachable server. Native HTTPS automatically renews Let's Encrypt certificates; its subscriber terms apply and certificate names are public. https://letsencrypt.org/repository/",
        "공개 셀프호스팅에는 소유 도메인과 외부 접속 가능한 서버가 필요합니다. 자동 HTTPS는 Let's Encrypt 인증서를 발급·갱신하며 해당 약관이 적용되고 인증서 이름은 공개됩니다. https://letsencrypt.org/repository/")?;
    let host = loop {
        match host(&tty.input(
            "Your domain (assistant.example.com)",
            "내 도메인 (assistant.example.com)",
            "",
        )?) {
            Ok(h) => break h,
            Err(_) => tty.say(
                "Enter one domain, without a path or port.",
                "경로나 포트 없는 도메인을 입력하세요.",
            )?,
        }
    };
    let mode = tty.select(
        "HTTPS",
        "HTTPS 설정",
        &[
            (
                "Automatic certificates in Moosuri",
                "무수리에서 인증서 자동 발급·갱신",
            ),
            (
                "Use my existing Nginx/Caddy/TLS proxy",
                "기존 Nginx/Caddy/HTTPS 프록시 사용",
            ),
        ],
        0,
    )?;
    let tls_port = if mode == 0 {
        tty.say("Forward external TCP443 to this server's TLS port. 8443 needs no root; DNS alone cannot bypass a router or CGNAT. Use Tailscale if inbound access is unavailable.",
            "외부 TCP443을 이 서버의 TLS 포트로 연결하세요. 8443은 root 없이 사용합니다. DNS만으로 공유기나 CGNAT을 통과할 수는 없습니다. 외부 연결이 어려우면 Tailscale을 사용하세요.")?;
        Some(loop {
            if let Ok(port) = tty
                .input(
                    "TLS port (external443 → this port)",
                    "TLS 포트 (외부443 → 이 포트)",
                    "8443",
                )?
                .parse::<u16>()
                && port != 0
            {
                break port;
            }
        })
    } else {
        None
    };
    tty.say("Register DNS for this domain to your server before continuing. Moosuri only checks that connection; it does not change DNS records.", "먼저 이 도메인의 DNS를 서버로 등록해주세요. 무수리는 연결만 확인하며 DNS 기록을 변경하지 않습니다.")?;
    let request = Request { host, tls_port };
    request.validate()?;
    Ok(request)
}
fn admission(p: &Path) -> Result<()> {
    if load(p)?.removing {
        return Err("setup_uninstall_pending".into());
    }
    no_pending(p)?;
    if current(p)?.version != own_probe().version {
        return Err("setup_existing_version_requires_signed_update".into());
    }
    if p.join("tailscale.json").exists() {
        return Err("public_disconnect_tailscale_first".into());
    }
    Ok(())
}
pub(in crate::installation) fn check_start(p: &Path) -> Result<()> {
    if Record::load(p)?.is_some_and(|r| r.phase != Phase::Connected) {
        return Err("public_access_pending_connect_or_disconnect".into());
    }
    Ok(())
}
pub(super) async fn connect(
    p: &Path,
    request: Option<Request>,
    open_browser: bool,
    start_after: bool,
) -> Result<()> {
    let _control = desktop::control_lock(p)?;
    let preparation = lock(p, false)?;
    admission(p)?;
    let release = current(p)?;
    let mut i = installation(p)?;
    let mut s = load(p)?;
    let mut tty = Terminal::open(Some(s.language), true).ok();
    let mut r = match Record::load(p)? {
        Some(r) => {
            r.validate(p, &i, &s)?;
            if request
                .as_ref()
                .is_some_and(|q| q.host != r.request.host || q.tls_port != r.request.tls_port)
            {
                return Err("public_disconnect_before_changing_domain".into());
            }
            if r.phase == Phase::Disconnecting {
                return Err("public_access_pending_connect_or_disconnect".into());
            }
            r
        }
        None => {
            let q = match request.or(s.public_requested.clone()) {
                Some(q) => q,
                None => prepare(tty.as_mut().ok_or("setup_terminal_required")?)?,
            };
            q.validate()?;
            verification::addresses(&q.host).await?;
            if let Some(port) = q.tls_port {
                if port == s.port {
                    return Err("public_tls_port_must_differ_from_local_port".into());
                }
                tls::preflight(port)?;
            }
            let resume = !matches!(s.service, service::Kind::None)
                && (start_after || service::running(p, &s));
            Record::prepare(p, &i, &s, q, resume)?
        }
    };
    if r.phase != Phase::Connected {
        drop(preparation);
        if !matches!(s.service, service::Kind::None) {
            service::stop(p, &s)?;
        }
        let guard = stopped_runtime(p, Duration::from_secs(20))?;
        if current(p)? != release {
            return Err("installation_runtime_changed_retry".into());
        }
        r.save(p)?;
        verification::verify(&r.request, &i.data_dir, s.port).await?;
        r.apply(p, &mut i, &s, true)?;
        super::super::upgrade_fault("public_after_origin")?;
        r.phase = Phase::Connected;
        r.save(p)?;
        drop(guard);
    } else {
        drop(preparation);
    }
    s.public_requested = None;
    save(p, &s)?;
    if !matches!(s.service, service::Kind::None) && (start_after || r.resume_service) {
        start_service(p, &i, &s).await?;
    }
    println!(
        "{}: {}/app",
        s.language.text(
            "Domain connection verified and public access configured",
            "도메인 연결을 확인하고 공개 접속을 설정했습니다"
        ),
        i.origin
    );
    let ready = if start_after {
        inspect(&i, &s).await?
    } else {
        false
    };
    if ready && open_browser {
        browser(&format!("{}/app", i.origin))?;
    }
    Ok(())
}
async fn disconnect(p: &Path) -> Result<()> {
    let _control = desktop::control_lock(p)?;
    let preparation = lock(p, false)?;
    admission(p)?;
    let release = current(p)?;
    let mut i = installation(p)?;
    let mut s = load(p)?;
    let Some(mut r) = Record::load(p)? else {
        println!("No managed public connection.");
        return Ok(());
    };
    r.validate(p, &i, &s)?;
    if r.phase != Phase::Disconnecting {
        r.resume_service =
            service::running(p, &s) || (r.phase == Phase::Connecting && r.resume_service);
        r.phase = Phase::Disconnecting;
        r.save(p)?;
    }
    drop(preparation);
    if !matches!(s.service, service::Kind::None) {
        service::stop(p, &s)?;
    }
    let guard = stopped_runtime(p, Duration::from_secs(20))?;
    if current(p)? != release {
        return Err("installation_runtime_changed_retry".into());
    }
    r.apply(p, &mut i, &s, false)?;
    s.public_requested = None;
    save(p, &s)?;
    r.remove(p)?;
    drop(guard);
    if !matches!(s.service, service::Kind::None) && r.resume_service {
        start_service(p, &i, &s).await?;
    }
    println!(
        "{}: {}/app",
        s.language.text(
            "Prior access restored. DNS records and certificate keys retained.",
            "이전 접속을 복원했습니다. DNS 기록과 인증서 키는 유지합니다."
        ),
        i.origin
    );
    Ok(())
}
pub(super) fn detach_for_removal(p: &Path) -> Result<()> {
    if let Some(r) = Record::load(p)? {
        r.validate(p, &installation(p)?, &load(p)?)?;
        r.remove(p)?;
        println!("Public DNS records and certificate keys retained; only this runtime is removed.");
    }
    Ok(())
}
async fn status(p: &Path) -> Result<()> {
    let i = installation(p)?;
    let s = load(p)?;
    let Some(r) = Record::load(p)? else {
        println!(
            "{}",
            s.language.text(
                "No managed public connection. Use moosuri public connect.",
                "공개 접속이 설정되지 않았습니다. moosuri public connect를 실행하세요."
            )
        );
        return Ok(());
    };
    r.validate(p, &i, &s)?;
    if r.phase != Phase::Connected {
        println!(
            "{}",
            s.language.text(
                "Setup is pending. Retry connect or disconnect to restore prior access.",
                "설정이 중단되었습니다. connect로 재개하거나 disconnect로 이전 접속을 복원하세요."
            )
        );
        return Ok(());
    }
    inspect(&i, &s).await.map(|_| ())
}
pub(super) async fn browser_ready(p: &Path, i: &Installation, s: &State) -> Result<bool> {
    if let Some(r) = Record::load(p)? {
        r.validate(p, i, s)?;
        inspect(i, s).await
    } else {
        Ok(true)
    }
}
async fn inspect(i: &Installation, s: &State) -> Result<bool> {
    let h = host(&i.origin)?;
    let addresses = tokio::time::timeout(
        Duration::from_secs(4),
        tokio::net::lookup_host((h.as_str(), 443)),
    )
    .await;
    let addresses: Vec<SocketAddr> = match addresses {
        Ok(Ok(a)) => a.collect(),
        _ => Vec::new(),
    };
    let dns = !addresses.is_empty()
        && addresses
            .iter()
            .all(|a| moosuri_connectors::web::public_address(a.ip()));
    let local = Readiness::new(i, s)?.observe().await == Observation::Ready;
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(6))
        .resolve_to_addrs(&h, &addresses)
        .build()
        .map_err(|_| "public_probe_unavailable")?;
    let https = if dns && local {
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
    } else {
        false
    };
    println!(
        "DNS: {dns}\n{}: {local}\n{}: {https}",
        s.language.text("Local runtime", "서버 실행"),
        s.language
            .text("HTTPS from this host", "이 호스트에서 HTTPS 접속")
    );
    if !https {
        println!("{}", s.language.text("Check DNS, TCP443 forwarding, TLS port permissions and certificate issuance. Try the address on another network before inviting others.", "DNS·외부443 포트 전달·TLS 포트 권한·인증서 발급 상태를 확인하세요. 초대 전에 다른 네트워크에서도 주소를 열어보세요."));
    }
    Ok(https)
}
