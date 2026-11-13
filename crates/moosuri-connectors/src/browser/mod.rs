//! Optional Linux capsule worker for unauthenticated public browser research.
//! Deployment MUST enforce the network/mount contract in `deployment/browser/`.
//! No arbitrary script, click, type, upload, credential or download API is exposed.
pub mod capsule;
mod cdp;
mod net;
pub mod proxy;
pub mod workspace;

use base64::Engine;
use cdp::Cdp;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    net::SocketAddr,
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{
    io::{AsyncBufReadExt, BufReader},
    process::{Child, Command},
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum BrowserError {
    #[error("browser requires the isolated Linux capsule")]
    IsolationRequired,
    #[error("browser executable is unavailable")]
    ExecutableUnavailable,
    #[error("browser setup failed")]
    Setup,
    #[error("browser session expired")]
    Expired,
    #[error("browser cleanup is unconfirmed; operator reconciliation required")]
    CleanupUnconfirmed,
    #[error("browser operation cancelled")]
    Cancelled,
    #[error("browser operation timed out")]
    Timeout,
    #[error("invalid browser page handle")]
    InvalidHandle,
    #[error("browser page changed; open it again")]
    StalePage,
    #[error("browser tab limit reached")]
    Capacity,
    #[error("invalid public HTTP URL")]
    InvalidUrl,
    #[error("destination is not a public address")]
    ForbiddenDestination,
    #[error("browser network connection failed")]
    Network,
    #[error("browser protocol failed")]
    Protocol,
    #[error("browser output exceeds its bound")]
    OutputTooLarge,
}

/// Operator-controlled configuration. A separate worker is required per tenant.
pub struct BrowserConfig {
    pub tenant: Uuid,
    pub chromium: PathBuf,
    pub profile_root: PathBuf,
    pub proxy: SocketAddr,
    /// Operator enables only after the Linux containment smoke test passes.
    pub javascript_enabled: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PageHandle {
    pub page: Uuid,
    pub worker_generation: Uuid,
    pub navigation_generation: u64,
}

#[derive(Debug, Serialize)]
pub struct BrowserPage {
    pub handle: PageHandle,
    pub url: String,
    pub title: String,
    pub text: String,
    pub links: Vec<BrowserLink>,
    pub truncated: bool,
    pub warning: &'static str,
}
#[derive(Debug, Serialize)]
pub struct BrowserLink {
    pub text: String,
    pub url: String,
}

pub struct Screenshot {
    pub png: Vec<u8>,
    pub handle: PageHandle,
}
struct Target {
    target: String,
    session: String,
    frame: String,
    loader: String,
}

pub struct BrowserSession {
    tenant: Uuid,
    generation: Uuid,
    child: Arc<Mutex<Option<Child>>>,
    watchdog: tokio::task::JoinHandle<()>,
    profile: PathBuf,
    cdp: Cdp,
    targets: HashMap<Uuid, Target>,
    deadline: Instant,
    poisoned: bool,
    cancellation: CancellationToken,
    javascript_enabled: bool,
}

impl BrowserSession {
    pub async fn launch(
        config: BrowserConfig,
        cancellation: CancellationToken,
    ) -> Result<Self, BrowserError> {
        Self::launch_bounded(config, cancellation, Duration::from_secs(120)).await
    }
    async fn launch_bounded(
        config: BrowserConfig,
        cancellation: CancellationToken,
        lifetime: Duration,
    ) -> Result<Self, BrowserError> {
        capsule_health(&config)?;
        let deadline = Instant::now() + lifetime;
        if cancellation.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        let generation = Uuid::new_v4();
        let profile = config
            .profile_root
            .join(format!("{}-{}", config.tenant, generation));
        tokio::fs::create_dir(&profile)
            .await
            .map_err(|_| BrowserError::Setup)?;
        tokio::fs::create_dir(profile.join("Default"))
            .await
            .map_err(|_| BrowserError::Setup)?;
        let preferences = json!({"profile":{"default_content_setting_values":{"javascript": if config.javascript_enabled {1} else {2},"automatic_downloads":2,"notifications":2,"geolocation":2,"media_stream_mic":2,"media_stream_camera":2,"popups":2}}});
        tokio::fs::write(profile.join("Default/Preferences"), preferences.to_string())
            .await
            .map_err(|_| BrowserError::Setup)?;
        let mut command = Command::new(&config.chromium);
        command
            .env_clear()
            .env("PATH", "/usr/bin:/bin")
            .env("LANG", "C.UTF-8")
            .env("HOME", &profile)
            .arg("--headless=new")
            .arg("--remote-debugging-address=127.0.0.1")
            .arg("--remote-debugging-port=0")
            .arg(format!("--user-data-dir={}", profile.display()))
            .arg(format!("--proxy-server=http://{}", config.proxy))
            .arg("--proxy-bypass-list=<-loopback>")
            .args([
                "--disable-quic",
                "--disable-background-networking",
                "--disable-component-update",
                "--disable-sync",
                "--no-first-run",
                "--no-default-browser-check",
                "--disable-extensions",
                "--disable-notifications",
                "--deny-permission-prompts",
                "--disable-breakpad",
                "--window-size=1280,900",
                "about:blank",
            ])
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::piped())
            .kill_on_drop(true);
        // No --no-sandbox or --disable-setuid-sandbox fallback is permitted.
        #[cfg(unix)]
        command.process_group(0);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = tokio::fs::remove_dir_all(&profile).await;
                return Err(BrowserError::ExecutableUnavailable);
            }
        };
        let stderr = child.stderr.take().ok_or(BrowserError::Setup)?;
        let connect = async {
            let mut reader = BufReader::new(stderr);
            let mut total = 0;
            loop {
                let mut line = String::new();
                let n = reader
                    .read_line(&mut line)
                    .await
                    .map_err(|_| BrowserError::Setup)?;
                total += n;
                if n == 0 || total > 64 * 1024 {
                    return Err(BrowserError::Setup);
                }
                if let Some(endpoint) = line.trim().strip_prefix("DevTools listening on ") {
                    let cdp = Cdp::connect(endpoint).await?;
                    // Drain bounded diagnostics without exposing Chromium/page data in logs.
                    tokio::spawn(async move {
                        let _ = tokio::io::copy(
                            &mut tokio::io::AsyncReadExt::take(reader, 1024 * 1024),
                            &mut tokio::io::sink(),
                        )
                        .await;
                    });
                    return Ok(cdp);
                }
            }
        };
        let result = tokio::select! { biased;
            _ = cancellation.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(15), connect) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        let mut cdp = match result {
            Ok(cdp) => cdp,
            Err(error) => {
                let _ = child.kill().await;
                let _ = tokio::fs::remove_dir_all(&profile).await;
                return Err(error);
            }
        };
        let setup = cdp.command(
            "Browser.setDownloadBehavior",
            json!({"behavior":"deny"}),
            None,
        );
        match tokio::time::timeout(Duration::from_secs(5), setup).await {
            Ok(Ok(_)) => {}
            _ => {
                let _ = child.kill().await;
                let _ = tokio::fs::remove_dir_all(&profile).await;
                return Err(BrowserError::Setup);
            }
        }
        let child = Arc::new(Mutex::new(Some(child)));
        let watched = child.clone();
        let stop = cancellation.clone();
        let watchdog = tokio::spawn(async move {
            tokio::select! { _ = stop.cancelled() => {}, _ = tokio::time::sleep_until(deadline.into()) => {} }
            if let Ok(mut child) = watched.lock()
                && let Some(child) = child.as_mut()
            {
                kill_child(child);
            }
        });
        Ok(Self {
            tenant: config.tenant,
            generation,
            child,
            watchdog,
            profile,
            cdp,
            targets: HashMap::new(),
            deadline,
            poisoned: false,
            cancellation,
            javascript_enabled: config.javascript_enabled,
        })
    }

    pub fn tenant(&self) -> Uuid {
        self.tenant
    }
    pub fn expires_in(&self) -> Duration {
        self.deadline.saturating_duration_since(Instant::now())
    }
    pub async fn open(&mut self, url: &str) -> Result<BrowserPage, BrowserError> {
        let url = net::public_url(url)?;
        let duration = self.remaining()?;
        let cancellation = self.cancellation.clone();
        let result = tokio::select! { biased;
            _ = cancellation.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(duration, self.open_inner(url.as_str())) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        self.after(&result);
        result
    }
    async fn open_inner(&mut self, url: &str) -> Result<BrowserPage, BrowserError> {
        let handle = self.create_page(url).await?;
        self.read_inner(&handle).await
    }
    async fn create_page(&mut self, url: &str) -> Result<PageHandle, BrowserError> {
        if self.targets.len() >= 4 {
            return Err(BrowserError::Capacity);
        }
        let result = self
            .cdp
            .command("Target.createTarget", json!({"url":"about:blank"}), None)
            .await?;
        let target = string(&result, "targetId")?;
        let result = self
            .cdp
            .command(
                "Target.attachToTarget",
                json!({"targetId":target,"flatten":true}),
                None,
            )
            .await?;
        let session = string(&result, "sessionId")?;
        self.cdp
            .command("Page.enable", json!({}), Some(&session))
            .await?;
        self.cdp
            .command(
                "Page.setLifecycleEventsEnabled",
                json!({"enabled":true}),
                Some(&session),
            )
            .await?;
        self.cdp
            .command("Network.enable", json!({}), Some(&session))
            .await?;
        self.cdp
            .command(
                "Emulation.setScriptExecutionDisabled",
                json!({"value":!self.javascript_enabled}),
                Some(&session),
            )
            .await?;
        self.cdp
            .command(
                "Network.setBypassServiceWorker",
                json!({"bypass":true}),
                Some(&session),
            )
            .await?;
        self.cdp
            .command(
                "Emulation.setDeviceMetricsOverride",
                json!({"width":1280,"height":900,"deviceScaleFactor":1,"mobile":false}),
                Some(&session),
            )
            .await?;
        self.cdp.before_navigation(&session);
        let navigation = self
            .cdp
            .command("Page.navigate", json!({"url":url}), Some(&session))
            .await?;
        if navigation.get("errorText").is_some() {
            return Err(BrowserError::Network);
        }
        self.cdp
            .wait_loaded(&session, &string(&navigation, "loaderId")?)
            .await?;
        let frame = self
            .cdp
            .command("Page.getFrameTree", json!({}), Some(&session))
            .await?;
        let frame = frame
            .pointer("/frameTree/frame")
            .ok_or(BrowserError::Protocol)?;
        let record = Target {
            target,
            session,
            frame: string(frame, "id")?,
            loader: string(frame, "loaderId")?,
        };
        let page = Uuid::new_v4();
        self.targets.insert(page, record);
        let handle = PageHandle {
            page,
            worker_generation: self.generation,
            navigation_generation: 1,
        };
        Ok(handle)
    }
    pub async fn read(&mut self, handle: &PageHandle) -> Result<BrowserPage, BrowserError> {
        let duration = self.remaining()?;
        let cancellation = self.cancellation.clone();
        let result = tokio::select! { biased;
            _ = cancellation.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(duration, self.read_inner(handle)) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        self.after(&result);
        result
    }
    async fn checked_session(&mut self, handle: &PageHandle) -> Result<String, BrowserError> {
        if handle.worker_generation != self.generation || handle.navigation_generation != 1 {
            return Err(BrowserError::InvalidHandle);
        }
        let target = self
            .targets
            .get(&handle.page)
            .ok_or(BrowserError::InvalidHandle)?;
        let session = target.session.clone();
        let actual = self
            .cdp
            .command("Page.getFrameTree", json!({}), Some(&session))
            .await?;
        let frame = actual
            .pointer("/frameTree/frame")
            .ok_or(BrowserError::Protocol)?;
        if string(frame, "id")? != target.frame || string(frame, "loaderId")? != target.loader {
            return Err(BrowserError::StalePage);
        }
        net::public_url(&string(frame, "url")?)?;
        Ok(session)
    }
    async fn read_inner(&mut self, handle: &PageHandle) -> Result<BrowserPage, BrowserError> {
        let session = self.checked_session(handle).await?;
        // Fixed implementation code only. No model/user supplied script is accepted.
        // innerText preserves visually present aria-hidden negation and respects layout.
        const READ: &str = "(()=>{const t=document.body?document.body.innerText:'';const links=[];for(let i=0;i<document.links.length&&i<4;i++){const a=document.links[i];links.push({text:(a.innerText||a.textContent||'').slice(0,128),url:a.href.slice(0,2049)})}return {url:location.href,title:document.title.slice(0,256),text:t.slice(0,8192),links,truncated:t.length>8192||document.links.length>4}})()";
        let result = self.cdp.command("Runtime.evaluate", json!({"expression":READ,"returnByValue":true,"awaitPromise":false,"userGesture":false,"timeout":2000}), Some(&session)).await?;
        if result.get("exceptionDetails").is_some() {
            return Err(BrowserError::Protocol);
        }
        let value = result
            .pointer("/result/value")
            .ok_or(BrowserError::Protocol)?;
        let mut text = string(value, "text")?;
        let mut title = string(value, "title")?;
        let truncated = value
            .get("truncated")
            .and_then(Value::as_bool)
            .unwrap_or(true)
            || text.len() > 8192;
        trim_bytes(&mut text, 8192);
        trim_bytes(&mut title, 512);
        let mut links = Vec::new();
        for link in value
            .get("links")
            .and_then(Value::as_array)
            .ok_or(BrowserError::Protocol)?
            .iter()
            .take(4)
        {
            let Ok(url) = net::public_url(&string(link, "url")?) else {
                continue;
            };
            let mut text = string(link, "text")?;
            trim_bytes(&mut text, 256);
            links.push(BrowserLink {
                text,
                url: url.to_string(),
            });
        }
        let url = net::public_url(&string(value, "url")?)?.to_string();
        self.checked_session(handle).await?;
        let mut page = BrowserPage {
            handle: handle.clone(),
            url,
            title,
            text,
            links,
            truncated,
            warning: if self.javascript_enabled {
                "chromium; javascript_enabled; public_unauthenticated_profile"
            } else {
                "chromium; javascript_disabled; dynamic_content_may_be_missing"
            },
        };
        while serde_json::to_vec(&page)
            .map_err(|_| BrowserError::Protocol)?
            .len()
            > 12 * 1024
        {
            page.truncated = true;
            if !page.text.is_empty() {
                let limit = page.text.len().saturating_sub(256);
                trim_bytes(&mut page.text, limit);
            } else if page.links.pop().is_none() {
                return Err(BrowserError::OutputTooLarge);
            }
        }
        Ok(page)
    }
    pub async fn screenshot(&mut self, handle: &PageHandle) -> Result<Screenshot, BrowserError> {
        let duration = self.remaining()?;
        let cancellation = self.cancellation.clone();
        let result = tokio::select! { biased;
            _ = cancellation.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(duration, self.screenshot_inner(handle)) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        self.after(&result);
        result
    }
    async fn screenshot_inner(&mut self, handle: &PageHandle) -> Result<Screenshot, BrowserError> {
        let session = self.checked_session(handle).await?;
        let result = self
            .cdp
            .command(
                "Page.captureScreenshot",
                json!({"format":"png","captureBeyondViewport":false,"fromSurface":true}),
                Some(&session),
            )
            .await?;
        let data = string(&result, "data")?;
        if data.len() > 3 * 1024 * 1024 {
            return Err(BrowserError::OutputTooLarge);
        }
        let png = base64::engine::general_purpose::STANDARD
            .decode(data)
            .map_err(|_| BrowserError::Protocol)?;
        if png.len() > 2 * 1024 * 1024 || !png.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(BrowserError::OutputTooLarge);
        }
        self.checked_session(handle).await?;
        Ok(Screenshot {
            png,
            handle: handle.clone(),
        })
    }
    pub async fn close(&mut self, handle: &PageHandle) -> Result<(), BrowserError> {
        let duration = self.remaining()?;
        if handle.worker_generation != self.generation || handle.navigation_generation != 1 {
            return Err(BrowserError::InvalidHandle);
        }
        let target = self
            .targets
            .remove(&handle.page)
            .ok_or(BrowserError::InvalidHandle)?;
        let result = tokio::time::timeout(
            duration,
            self.cdp.command(
                "Target.closeTarget",
                json!({"targetId":target.target}),
                None,
            ),
        )
        .await
        .map_err(|_| BrowserError::Timeout)
        .and_then(|v| v.map(|_| ()));
        self.after(&result);
        result
    }
    fn kill(&self) {
        if let Ok(mut child) = self.child.lock()
            && let Some(child) = child.as_mut()
        {
            kill_child(child);
        }
    }
    fn remaining(&mut self) -> Result<Duration, BrowserError> {
        if self.poisoned || self.cancellation.is_cancelled() {
            self.poisoned = true;
            self.kill();
            return Err(BrowserError::Cancelled);
        }
        let Some(duration) = self.deadline.checked_duration_since(Instant::now()) else {
            self.kill();
            self.poisoned = true;
            return Err(BrowserError::Expired);
        };
        Ok(duration.min(Duration::from_secs(15)))
    }
    fn after<T>(&mut self, result: &Result<T, BrowserError>) {
        if matches!(
            result,
            Err(BrowserError::Cancelled
                | BrowserError::Timeout
                | BrowserError::Protocol
                | BrowserError::Network
                | BrowserError::OutputTooLarge)
        ) {
            self.poisoned = true;
            self.kill();
        }
    }
    pub async fn shutdown(self) {
        self.watchdog.abort();
        self.kill();
        let child = self.child.lock().ok().and_then(|mut child| child.take());
        if let Some(mut child) = child {
            let _ = child.kill().await;
            let _ = child.wait().await;
        }
        let _ = tokio::fs::remove_dir_all(&self.profile).await;
    }
}

impl Drop for BrowserSession {
    fn drop(&mut self) {
        self.watchdog.abort();
        self.kill();
    }
}

fn kill_child(child: &mut Child) {
    #[cfg(unix)]
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
    let _ = child.start_kill();
}

fn trim_bytes(value: &mut String, limit: usize) {
    let mut end = limit.min(value.len());
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    value.truncate(end);
}
fn string(value: &Value, key: &str) -> Result<String, BrowserError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or(BrowserError::Protocol)
}

/// Operator deployment configuration: exactly one RFC1918 IPv4 proxy, fixed port.
/// No hostname/DNS or arbitrary browser network destination is accepted here.
pub fn private_proxy(raw: &str) -> Result<SocketAddr, BrowserError> {
    let ip = raw
        .parse::<std::net::Ipv4Addr>()
        .map_err(|_| BrowserError::IsolationRequired)?;
    if !ip.is_private() || ip.to_string() != raw {
        return Err(BrowserError::IsolationRequired);
    }
    Ok(SocketAddr::new(ip.into(), 3128))
}
fn marker_matches(marker: &str, proxy: SocketAddr) -> bool {
    private_proxy(&proxy.ip().to_string()).is_ok_and(|expected| expected == proxy)
        && marker == format!("moosuri-browser-isolation-v2\nproxy={proxy}\n")
}

fn capsule_status_confined(status: &str) -> bool {
    [
        "NoNewPrivs:\t1",
        "Seccomp:\t2",
        "CapEff:\t0000000000000000",
        "CapPrm:\t0000000000000000",
        "CapInh:\t0000000000000000",
        "CapAmb:\t0000000000000000",
        "CapBnd:\t0000000000000000",
    ]
    .iter()
    .all(|expected| status.lines().any(|line| line == *expected))
}

fn capsule_health(config: &BrowserConfig) -> Result<(), BrowserError> {
    if !cfg!(target_os = "linux") {
        return Err(BrowserError::IsolationRequired);
    }
    #[cfg(unix)]
    if unsafe { libc::geteuid() } == 0 {
        return Err(BrowserError::IsolationRequired);
    }
    let status = std::fs::read_to_string("/proc/self/status")
        .map_err(|_| BrowserError::IsolationRequired)?;
    if !capsule_status_confined(&status) {
        return Err(BrowserError::IsolationRequired);
    }
    // Entrypoint writes this root-owned marker only after installing namespace firewall.
    // It is a deployment prerequisite, not remote attestation of the host.
    let marker = std::fs::read_to_string("/run/browser-isolation")
        .map_err(|_| BrowserError::IsolationRequired)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::symlink_metadata("/run/browser-isolation")
            .map_err(|_| BrowserError::IsolationRequired)?;
        if !metadata.is_file() || metadata.uid() != 0 || metadata.mode() & 0o022 != 0 {
            return Err(BrowserError::IsolationRequired);
        }
    }
    if !marker_matches(&marker, config.proxy)
        || config.profile_root != std::path::Path::new("/profiles")
        || config.chromium != std::path::Path::new("/usr/bin/chromium")
    {
        return Err(BrowserError::IsolationRequired);
    }
    if !config.chromium.is_file() {
        return Err(BrowserError::ExecutableUnavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn public_navigation_cannot_name_local_files_credentials_or_private_destinations() {
        for url in [
            "file:///etc/passwd",
            "https://u:p@example.com",
            "http://127.0.0.1",
            "http://2130706433",
            "http://169.254.169.254",
            "http://[::ffff:127.0.0.1]",
            "https://example.com:444/",
            "http://a.local/",
        ] {
            assert!(net::public_url(url).is_err());
        }
        assert!(net::public_url("https://example.com/").is_ok());
    }
    #[tokio::test]
    async fn unavailable_capsule_has_no_host_chrome_fallback() {
        let result = BrowserSession::launch(
            BrowserConfig {
                tenant: Uuid::new_v4(),
                chromium: PathBuf::from("/missing"),
                profile_root: PathBuf::from("/profiles"),
                proxy: "172.30.0.2:3128".parse().unwrap(),
                javascript_enabled: false,
            },
            CancellationToken::new(),
        )
        .await;
        assert!(matches!(
            result,
            Err(BrowserError::IsolationRequired | BrowserError::ExecutableUnavailable)
        ));
    }
    #[test]
    fn deployment_proxy_and_firewall_marker_must_match_exactly() {
        let proxy = private_proxy("10.42.17.2").unwrap();
        assert!(marker_matches(
            "moosuri-browser-isolation-v2\nproxy=10.42.17.2:3128\n",
            proxy
        ));
        for marker in [
            "moosuri-browser-isolation-v1\n",
            "moosuri-browser-isolation-v2\nproxy=10.42.17.3:3128\n",
        ] {
            assert!(!marker_matches(marker, proxy));
        }
        for raw in [
            "127.0.0.1",
            "168.63.129.16",
            "1.1.1.1",
            "::ffff:10.42.17.2",
            "10.42.17.2:3128",
            "010.42.17.2",
            "proxy.local",
        ] {
            assert!(private_proxy(raw).is_err());
        }
    }
}
