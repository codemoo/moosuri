//! Native LocalAPI transport. Credentials and raw daemon replies never leave this module.
use super::*;
use reqwest::{Method, StatusCode, header};
use std::os::unix::fs::FileTypeExt;
use zeroize::Zeroizing;

pub(super) struct Api {
    client: reqwest::Client,
    base: String,
    token: Option<Zeroizing<String>>,
}
fn client() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(Duration::from_secs(8))
}
impl Api {
    pub(super) fn discover(socket: Option<&Path>) -> Result<Self> {
        if let Some(socket) = socket {
            return Self::unix(socket);
        }
        #[cfg(target_os = "macos")]
        if let Some((port, token)) = mac_credentials() {
            return Ok(Self {
                client: client()
                    .build()
                    .map_err(|_| "tailscale_transport_unavailable")?,
                base: format!("http://127.0.0.1:{port}"),
                token: Some(token),
            });
        }
        Self::unix(Path::new(if cfg!(target_os = "macos") {
            "/var/run/tailscaled.socket"
        } else {
            "/var/run/tailscale/tailscaled.sock"
        }))
    }
    fn unix(path: &Path) -> Result<Self> {
        let meta = fs::symlink_metadata(path).map_err(|_| "tailscale_not_available")?;
        if !path.is_absolute()
            || !meta.file_type().is_socket()
            || ![0, unsafe { libc::geteuid() }].contains(&meta.uid())
        {
            return Err("tailscale_socket_invalid".into());
        }
        Ok(Self {
            client: client()
                .unix_socket(path)
                .build()
                .map_err(|_| "tailscale_transport_unavailable")?,
            base: "http://local-tailscaled.sock".into(),
            token: None,
        })
    }
    async fn request(
        &self,
        method: Method,
        path: &str,
        body: Option<&Value>,
        etag: Option<&str>,
    ) -> Result<reqwest::Response> {
        let mut request = self
            .client
            .request(method, format!("{}/localapi/v0/{path}", self.base))
            .header(header::HOST, "local-tailscaled.sock");
        if let Some(token) = &self.token {
            request = request.basic_auth("", Some(token.as_str()));
        }
        if let Some(body) = body {
            request = request.json(body);
        }
        if let Some(tag) = etag {
            request = request.header(header::IF_MATCH, tag);
        }
        let response = request
            .send()
            .await
            .map_err(|_| "tailscale_not_available")?;
        match response.status() {
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
                Err("tailscale_operator_required".into())
            }
            StatusCode::PRECONDITION_FAILED => Err("tailscale_configuration_changed_retry".into()),
            status if status.is_success() => Ok(response),
            _ => Err("tailscale_api_unavailable".into()),
        }
    }
    pub(super) async fn status(&self) -> Result<Value> {
        bounded_json(
            self.request(Method::GET, "status?peers=false", None, None)
                .await?,
        )
        .await
    }
    pub(super) async fn config(&self) -> Result<(Value, String)> {
        let response = self
            .request(Method::GET, "serve-config", None, None)
            .await?;
        let tag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .filter(|v| !v.is_empty() && v.len() <= 256)
            .ok_or("tailscale_etag_required")?
            .to_owned();
        let value = bounded_json(response).await?;
        // The official daemon represents an absent Serve configuration as null.
        let value = if value.is_null() { json!({}) } else { value };
        if !value.is_object() {
            return Err("tailscale_unsupported_configuration".into());
        }
        Ok((value, tag))
    }
    pub(super) async fn set_config(&self, value: &Value, etag: &str) -> Result<()> {
        self.request(Method::POST, "serve-config", Some(value), Some(etag))
            .await?;
        Ok(())
    }
    pub(super) async fn wake(&self) -> Result<()> {
        // Discard the full preferences response: it can contain private state.
        self.request(
            Method::PATCH,
            "prefs",
            Some(&json!({"WantRunning":true,"WantRunningSet":true})),
            None,
        )
        .await?;
        Ok(())
    }
    pub(super) async fn initialize(&self) -> Result<()> {
        self.request(Method::POST, "start", Some(&json!({})), None)
            .await?;
        Ok(())
    }
    pub(super) async fn login(&self) -> Result<()> {
        self.request(Method::POST, "login-interactive", None, None)
            .await?;
        Ok(())
    }
}
async fn bounded_json(mut response: reqwest::Response) -> Result<Value> {
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "tailscale_response_unavailable")?
    {
        if bytes.len() + chunk.len() > 1024 * 1024 {
            return Err("tailscale_response_too_large".into());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| "tailscale_response_invalid".into())
}
#[cfg(target_os = "macos")]
fn credentials(port: &str, token: &str) -> Option<(u16, Zeroizing<String>)> {
    let port = port.parse::<u16>().ok().filter(|v| *v >= 1024)?;
    if token.len() != 20
        || !token
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return None;
    }
    Some((port, Zeroizing::new(token.into())))
}
#[cfg(target_os = "macos")]
fn mac_credentials() -> Option<(u16, Zeroizing<String>)> {
    let user = format!("-u{}", unsafe { libc::geteuid() });
    if let Some((true, bytes)) = service::command(
        "/usr/sbin/lsof",
        &["-n", "-a", &user, "-c", "IPNExtension", "-F"],
        true,
        Duration::from_secs(3),
    ) {
        let bytes = Zeroizing::new(bytes);
        let text = String::from_utf8_lossy(&bytes);
        for line in text.lines() {
            let Some(path) = line.strip_prefix('n') else {
                continue;
            };
            let Some((_, suffix)) = path.split_once(".tailscale.ipn.macos/sameuserproof-") else {
                continue;
            };
            let Ok(meta) = fs::symlink_metadata(path) else {
                continue;
            };
            if !meta.is_file()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o022 != 0
            {
                continue;
            }
            if let Some((port, token)) = suffix.split_once('-')
                && let Some(value) = credentials(port, token)
            {
                return Some(value);
            }
        }
    }
    let pointer = Path::new("/Library/Tailscale/ipnport");
    let meta = fs::symlink_metadata(pointer).ok()?;
    if !meta.file_type().is_symlink() || meta.uid() != 0 {
        return None;
    }
    let target = fs::read_link(pointer).ok()?;
    let port = target.to_str()?;
    if !port.bytes().all(|v| v.is_ascii_digit()) || port.len() > 5 {
        return None;
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(format!("/Library/Tailscale/sameuserproof-{port}"))
        .ok()?;
    let meta = file.metadata().ok()?;
    if !meta.is_file()
        || meta.uid() != 0
        || meta.gid() != 80
        || meta.mode() & 0o027 != 0
        || meta.nlink() != 1
        || meta.len() > 128
    {
        return None;
    }
    let mut token = Zeroizing::new(String::new());
    file.take(129).read_to_string(&mut token).ok()?;
    credentials(port, token.trim())
}
