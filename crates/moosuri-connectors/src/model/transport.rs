use super::protocol::{
    ModelEvent, Protocol, ProtocolError, StreamDecoder, TextCapabilities, TextRequest,
    encode_request,
};
use reqwest::{Client, header};
use std::{net::IpAddr, time::Duration};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

/// No Debug/Serialize implementation: values only enter the configured HTTP header.
pub struct Secret(Zeroizing<String>);

impl Secret {
    pub fn new(value: String) -> Result<Self, TransportError> {
        let value = Zeroizing::new(value);
        if value.is_empty()
            || value.len() > 8192
            || value
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        {
            return Err(TransportError::InvalidConfiguration);
        }
        Ok(Self(value))
    }
}

/// Only an installation administrator may supply these policies. A user/model
/// selects a previously registered connection ID, never a URL or IP allowlist.
pub enum NetworkPolicy {
    PublicHttps,
    /// Exact addresses intentionally registered for a local/LAN/Tailscale server.
    /// Metadata, link-local, multicast and unspecified destinations stay forbidden.
    LocalExact(Vec<IpAddr>),
}

pub struct RouterPolicy {
    /// Approved OpenRouter provider/endpoint slugs. Empty means unavailable.
    pub providers: Vec<String>,
    pub zero_data_retention: bool,
}

#[derive(Clone, Copy)]
pub struct TransportLimits {
    pub connect: Duration,
    pub idle: Duration,
    pub total: Duration,
    pub response_bytes: usize,
}

impl Default for TransportLimits {
    fn default() -> Self {
        Self {
            connect: Duration::from_secs(10),
            idle: Duration::from_secs(30),
            total: Duration::from_secs(180),
            response_bytes: 8 * 1024 * 1024,
        }
    }
}

impl TransportLimits {
    fn validate(&self) -> Result<(), TransportError> {
        if self.connect.is_zero()
            || self.idle.is_zero()
            || self.total.is_zero()
            || self.connect > self.total
            || self.idle > self.total
            || self.total > Duration::from_secs(300)
            || self.response_bytes == 0
            || self.response_bytes > 8 * 1024 * 1024
        {
            return Err(TransportError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Safe categories only. Provider bodies, request text, URLs and credentials are
/// deliberately absent from errors. A retry hint never starts an automatic retry.
#[derive(Debug, thiserror::Error)]
pub enum TransportError {
    #[error("invalid model connection configuration")]
    InvalidConfiguration,
    #[error("model destination is not allowed")]
    DestinationDenied,
    #[error("model transport failed")]
    Transport,
    #[error("model request timed out")]
    Timeout,
    #[error("model request cancelled; remote processing may continue")]
    Cancelled,
    #[error("model response exceeded its size limit")]
    LimitExceeded,
    #[error("unexpected model response content type")]
    ContentType,
    #[error("model provider returned HTTP {status}")]
    Http {
        status: u16,
        retry_after_seconds: Option<u64>,
    },
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

pub struct ModelClient {
    protocol: Protocol,
    endpoint: Url,
    network: NetworkPolicy,
    credential: Option<Secret>,
    capabilities: TextCapabilities,
    router: Option<RouterPolicy>,
    limits: TransportLimits,
}

impl ModelClient {
    /// Hosted credentials can only be sent to the corresponding fixed origin.
    /// ChatGPT OAuth and Claude CLI authorization are separate, future adapters.
    pub fn hosted(
        protocol: Protocol,
        credential: Secret,
        capabilities: TextCapabilities,
        router: Option<RouterPolicy>,
    ) -> Result<Self, TransportError> {
        let url = match protocol {
            Protocol::Responses => "https://api.openai.com/v1/responses",
            Protocol::AnthropicMessages => "https://api.anthropic.com/v1/messages",
            Protocol::OpenRouterChat => "https://openrouter.ai/api/v1/chat/completions",
            Protocol::ChatCompletions => return Err(TransportError::InvalidConfiguration),
        };
        if matches!(protocol, Protocol::OpenRouterChat) != router.is_some() {
            return Err(TransportError::InvalidConfiguration);
        }
        if let Some(policy) = &router
            && (policy.providers.is_empty()
                || policy.providers.len() > 16
                || policy.providers.iter().any(|name| {
                    name.is_empty()
                        || name.len() > 256
                        || !name
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b"-_./:".contains(&b))
                }))
        {
            return Err(TransportError::InvalidConfiguration);
        }
        Ok(Self {
            protocol,
            endpoint: Url::parse(url).map_err(|_| TransportError::InvalidConfiguration)?,
            network: NetworkPolicy::PublicHttps,
            credential: Some(credential),
            capabilities,
            router,
            limits: TransportLimits::default(),
        })
    }

    /// Custom/local connections use only the verified Chat Completions subset.
    /// The key is newly supplied for this endpoint; hosted clients cannot retarget.
    pub fn compatible(
        base_url: &str,
        network: NetworkPolicy,
        credential: Option<Secret>,
        capabilities: TextCapabilities,
    ) -> Result<Self, TransportError> {
        let mut endpoint =
            Url::parse(base_url).map_err(|_| TransportError::InvalidConfiguration)?;
        if endpoint.host_str().is_none()
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
            || !matches!(endpoint.scheme(), "http" | "https")
            || !endpoint
                .path()
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/_-".contains(&b))
        {
            return Err(TransportError::InvalidConfiguration);
        }
        match &network {
            NetworkPolicy::PublicHttps if endpoint.scheme() != "https" => {
                return Err(TransportError::DestinationDenied);
            }
            NetworkPolicy::LocalExact(addresses)
                if addresses.is_empty()
                    || addresses.len() > 32
                    || addresses.iter().any(|ip| forbidden(*ip)) =>
            {
                return Err(TransportError::DestinationDenied);
            }
            _ => {}
        }
        if let Some(host) = endpoint.host() {
            let ip = match host {
                url::Host::Ipv4(ip) => Some(IpAddr::V4(ip)),
                url::Host::Ipv6(ip) => Some(IpAddr::V6(ip)),
                url::Host::Domain(_) => None,
            };
            if ip.is_some_and(|ip| !allowed(&network, ip)) {
                return Err(TransportError::DestinationDenied);
            }
        }
        let path = format!("{}/chat/completions", endpoint.path().trim_end_matches('/'));
        endpoint.set_path(&path);
        Ok(Self {
            protocol: Protocol::ChatCompletions,
            endpoint,
            network,
            credential,
            capabilities,
            router: None,
            limits: TransportLimits::default(),
        })
    }

    pub fn with_limits(mut self, limits: TransportLimits) -> Result<Self, TransportError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }

    /// The caller owns this future and a bounded event channel. Dropping/cancelling
    /// it drops local HTTP work; it cannot promise remote cancellation or a refund.
    /// Already delivered deltas/usage remain valid partial observations on failure.
    pub async fn stream_text(
        &self,
        request: &TextRequest,
        events: &mpsc::Sender<ModelEvent>,
        cancellation: &CancellationToken,
    ) -> Result<(), TransportError> {
        if cancellation.is_cancelled() || events.is_closed() {
            return Err(TransportError::Cancelled);
        }
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(TransportError::Cancelled),
            _ = events.closed() => Err(TransportError::Cancelled),
            result = tokio::time::timeout(self.limits.total, self.stream_inner(request, events)) => {
                result.map_err(|_| TransportError::Timeout)?
            }
        }
    }

    async fn stream_inner(
        &self,
        request: &TextRequest,
        events: &mpsc::Sender<ModelEvent>,
    ) -> Result<(), TransportError> {
        let mut body = encode_request(self.protocol, request, &self.capabilities)?;
        if let Some(policy) = &self.router {
            body["provider"] = serde_json::json!({
                "only": policy.providers,
                "allow_fallbacks": false,
                "require_parameters": true,
                "data_collection": "deny",
                "zdr": policy.zero_data_retention,
            });
        }
        let body = serde_json::to_vec(&body).map_err(|_| TransportError::InvalidConfiguration)?;
        if body.len() > 1024 * 1024 {
            return Err(TransportError::LimitExceeded);
        }
        let host = self
            .endpoint
            .host_str()
            .ok_or(TransportError::InvalidConfiguration)?;
        let port = self
            .endpoint
            .port_or_known_default()
            .ok_or(TransportError::InvalidConfiguration)?;
        // Resolve once for this invocation, inspect every answer, and pin that
        // exact set in reqwest. No second DNS lookup or ambient proxy can bypass it.
        let addresses: Vec<_> = tokio::net::lookup_host((host.trim_matches(['[', ']']), port))
            .await
            .map_err(|_| TransportError::Transport)?
            .take(33)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses
                .iter()
                .any(|addr| !allowed(&self.network, addr.ip()))
        {
            return Err(TransportError::DestinationDenied);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(self.limits.connect)
            .read_timeout(self.limits.idle)
            .timeout(self.limits.total)
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| TransportError::Transport)?;
        let mut call = client
            .post(self.endpoint.clone())
            .header(header::ACCEPT, "text/event-stream")
            .header(header::CONTENT_TYPE, "application/json")
            .body(body);
        if let Some(secret) = &self.credential {
            let (name, value) = if matches!(self.protocol, Protocol::AnthropicMessages) {
                call = call.header("anthropic-version", "2023-06-01");
                ("x-api-key", header::HeaderValue::from_str(&secret.0))
            } else {
                (
                    "authorization",
                    header::HeaderValue::from_str(&format!("Bearer {}", secret.0.as_str())),
                )
            };
            let mut value = value.map_err(|_| TransportError::InvalidConfiguration)?;
            value.set_sensitive(true);
            call = call.header(name, value);
        }
        let mut response = call.send().await.map_err(map_network_error)?;
        if response.status().as_u16() != 200 {
            let retry_after_seconds = response
                .headers()
                .get(header::RETRY_AFTER)
                .and_then(|h| h.to_str().ok())
                .and_then(|v| v.parse::<u64>().ok())
                .filter(|value| *value <= 86_400);
            return Err(TransportError::Http {
                status: response.status().as_u16(),
                retry_after_seconds,
            });
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|s| s.eq_ignore_ascii_case("text/event-stream")) {
            return Err(TransportError::ContentType);
        }
        let mut decoder = StreamDecoder::new(self.protocol);
        let mut received = 0usize;
        while let Some(chunk) = response.chunk().await.map_err(map_network_error)? {
            received = received
                .checked_add(chunk.len())
                .ok_or(TransportError::LimitExceeded)?;
            if received > self.limits.response_bytes {
                return Err(TransportError::LimitExceeded);
            }
            // Forward each SSE frame before parsing a later frame in the same
            // HTTP chunk. A late error must not discard already observed deltas.
            for fragment in chunk.split_inclusive(|b| *b == b'\r' || *b == b'\n') {
                let batch = decoder.push(fragment);
                for event in batch.events {
                    events
                        .send(event)
                        .await
                        .map_err(|_| TransportError::Cancelled)?;
                }
                if let Some(error) = batch.error {
                    return Err(TransportError::Protocol(error));
                }
            }
            if decoder.is_terminal() {
                decoder.finish()?;
                return Ok(());
            }
        }
        decoder.finish()?;
        Ok(())
    }
}

fn map_network_error(error: reqwest::Error) -> TransportError {
    if error.is_timeout() {
        TransportError::Timeout
    } else {
        TransportError::Transport
    }
}

fn normalized(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V6(v6) => v6.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(ip),
        _ => ip,
    }
}

fn forbidden(ip: IpAddr) -> bool {
    match normalized(ip) {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            o[0] == 0
                || o[0] >= 224
                || (o[0] == 169 && o[1] == 254)
                || o == [168, 63, 129, 16]
                || o == [100, 100, 100, 200]
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            ip.is_unspecified()
                || ip.is_multicast()
                || s[0] & 0xffc0 == 0xfe80
                || s == [0xfd00, 0xec2, 0, 0, 0, 0, 0, 0x254]
        }
    }
}

fn public_address(ip: IpAddr) -> bool {
    if forbidden(ip) {
        return false;
    }
    match normalized(ip) {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0)
                || (o[0] == 198 && (18..=19).contains(&o[1]))
                || (o[0..3] == [198, 51, 100])
                || (o[0..3] == [203, 0, 113]))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] == 0))
        }
    }
}

fn allowed(policy: &NetworkPolicy, ip: IpAddr) -> bool {
    if forbidden(ip) {
        return false;
    }
    match policy {
        NetworkPolicy::PublicHttps => public_address(ip),
        NetworkPolicy::LocalExact(ips) => ips.iter().any(|i| normalized(*i) == normalized(ip)),
    }
}

#[cfg(test)]
mod tests;
