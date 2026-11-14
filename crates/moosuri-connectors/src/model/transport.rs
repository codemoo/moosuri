use super::chatgpt::{PlanError, ResponsesDialect};
use super::protocol::tools::{
    ToolCapabilities, ToolRequest, ToolRound, ToolRoundDecoder, encode_tool_request,
};
use super::protocol::{
    ModelEvent, Protocol, ProtocolError, StreamDecoder, TextCapabilities, TextRequest,
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
    #[error("invalid model catalog response")]
    InvalidCatalog,
    #[error("model provider returned HTTP {status}")]
    Http {
        status: u16,
        retry_after_seconds: Option<u64>,
    },
    #[error("ChatGPT plan HTTP {status}: {code}")]
    ChatGptPlan {
        status: u16,
        code: PlanError,
        retry_after_seconds: Option<u64>,
    },
    #[error(transparent)]
    Protocol(#[from] ProtocolError),
}

pub struct ModelClient {
    dialect: ResponsesDialect,
    images: Vec<Vec<u8>>,
    protocol: Protocol,
    endpoint: Url,
    network: NetworkPolicy,
    credential: Option<Secret>,
    capabilities: TextCapabilities,
    router: Option<RouterPolicy>,
    limits: TransportLimits,
}

pub struct CatalogModel {
    pub id: String,
    pub name: String,
}
pub struct Catalog {
    pub models: Vec<CatalogModel>,
    pub truncated: bool,
}

impl ModelClient {
    /// Hosted credentials can only be sent to the corresponding fixed origin.
    /// Personal ChatGPT plan authorization uses the explicit separate constructor.
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
            dialect: ResponsesDialect::Api,
            images: vec![],
            protocol,
            endpoint: Url::parse(url).map_err(|_| TransportError::InvalidConfiguration)?,
            network: NetworkPolicy::PublicHttps,
            credential: Some(credential),
            capabilities,
            router,
            limits: TransportLimits::default(),
        })
    }

    /// Caller must resolve a personal, consented, valid SIWC access token. This is
    /// protocol construction only: it neither logs in nor registers a shared profile.
    pub fn chatgpt_plan(
        access_token: Secret,
        capabilities: TextCapabilities,
    ) -> Result<Self, TransportError> {
        let mut client = Self::hosted(Protocol::Responses, access_token, capabilities, None)?;
        client.dialect = ResponsesDialect::ChatGptPlan;
        Ok(client)
    }

    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub fn synthetic_chatgpt_plan(
        origin: &str,
        access_token: Secret,
        capabilities: TextCapabilities,
    ) -> Result<Self, TransportError> {
        let u = Url::parse(origin).map_err(|_| TransportError::DestinationDenied)?;
        if u.scheme() != "http"
            || u.host_str() != Some("127.0.0.1")
            || u.port().is_none_or(|p| p == 0 || p == 80)
            || !u.username().is_empty()
            || u.password().is_some()
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err(TransportError::DestinationDenied);
        }
        let mut client = Self::chatgpt_plan(access_token, capabilities)?;
        client.endpoint = u
            .join("v1/responses")
            .map_err(|_| TransportError::DestinationDenied)?;
        client.network = NetworkPolicy::LocalExact(vec![IpAddr::from([127, 0, 0, 1])]);
        Ok(client)
    }
    pub fn responses_dialect(&self) -> ResponsesDialect {
        self.dialect
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
            dialect: ResponsesDialect::Api,
            images: vec![],
            protocol: Protocol::ChatCompletions,
            endpoint,
            network,
            credential,
            capabilities,
            router: None,
            limits: TransportLimits::default(),
        })
    }

    /// Private bounded canonical images for this one client lifetime.
    pub fn with_images(mut self, images: Vec<Vec<u8>>) -> Result<Self, TransportError> {
        if self.dialect == ResponsesDialect::ChatGptPlan && !images.is_empty() {
            return Err(ProtocolError::UnsupportedCapability.into());
        }
        if images.len() > 4
            || images
                .iter()
                .any(|v| v.len() > 2 * 1024 * 1024 || !v.starts_with(&[255, 216, 255]))
        {
            return Err(TransportError::LimitExceeded);
        }
        self.images = images;
        Ok(self)
    }

    pub fn with_limits(mut self, limits: TransportLimits) -> Result<Self, TransportError> {
        limits.validate()?;
        self.limits = limits;
        Ok(self)
    }

    /// Discovery is not inference/capability verification. A single bounded page
    /// avoids unbounded installation-time work and never follows provider URLs.
    pub async fn list_models(
        &self,
        cancellation: &CancellationToken,
    ) -> Result<Catalog, TransportError> {
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(TransportError::Cancelled),
            result = tokio::time::timeout(self.limits.total, self.catalog_inner()) =>
                result.map_err(|_| TransportError::Timeout)?,
        }
    }

    async fn catalog_inner(&self) -> Result<Catalog, TransportError> {
        let mut endpoint = self.endpoint.clone();
        let path = match self.protocol {
            Protocol::Responses | Protocol::AnthropicMessages => "/v1/models".to_owned(),
            Protocol::OpenRouterChat => "/api/v1/models/user".to_owned(),
            Protocol::ChatCompletions => format!(
                "{}/models",
                endpoint
                    .path()
                    .strip_suffix("/chat/completions")
                    .ok_or(TransportError::InvalidConfiguration)?
            ),
        };
        endpoint.set_path(&path);
        match self.protocol {
            Protocol::AnthropicMessages => endpoint.set_query(Some("limit=512")),
            Protocol::OpenRouterChat => endpoint.set_query(Some("offset=0&limit=512")),
            _ => (),
        }
        let http = self.http_client(&endpoint).await?;
        let request = self.authorize(
            http.get(endpoint)
                .header(header::ACCEPT, "application/json"),
        )?;
        let mut response = request.send().await.map_err(map_network_error)?;
        self.check_status(&mut response).await?;
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|s| s.eq_ignore_ascii_case("application/json")) {
            return Err(TransportError::ContentType);
        }
        let limit = self.limits.response_bytes.min(1024 * 1024);
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(TransportError::LimitExceeded);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_network_error)? {
            if bytes.len().saturating_add(chunk.len()) > limit {
                return Err(TransportError::LimitExceeded);
            }
            bytes.extend_from_slice(&chunk);
        }
        if self.dialect == ResponsesDialect::ChatGptPlan {
            super::chatgpt::parse_catalog(&bytes)
        } else {
            parse_catalog(self.protocol, &bytes)
        }
    }

    async fn http_client(&self, endpoint: &Url) -> Result<Client, TransportError> {
        let host = endpoint
            .host_str()
            .ok_or(TransportError::InvalidConfiguration)?;
        let port = endpoint
            .port_or_known_default()
            .ok_or(TransportError::InvalidConfiguration)?;
        // Resolve once, inspect every answer, and pin the complete bounded set.
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
        Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(self.limits.connect)
            .read_timeout(self.limits.idle)
            .timeout(self.limits.total)
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| TransportError::Transport)
    }

    fn authorize(
        &self,
        mut call: reqwest::RequestBuilder,
    ) -> Result<reqwest::RequestBuilder, TransportError> {
        if let Some(secret) = &self.credential {
            let (name, value) = if matches!(self.protocol, Protocol::AnthropicMessages) {
                // Catalog and inference use the same explicit API version.
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
        Ok(call)
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
        let (body, decoder) = if self.dialect == ResponsesDialect::ChatGptPlan {
            (
                super::chatgpt::encode_text_request(request, &self.capabilities)?,
                StreamDecoder::chatgpt_plan(&request.model),
            )
        } else {
            (
                super::protocol::encode_image_request(
                    self.protocol,
                    request,
                    &self.capabilities,
                    &self.images,
                )?,
                StreamDecoder::new(self.protocol),
            )
        };
        self.stream_encoded(body, decoder, events).await
    }

    /// Complete calls are returned only after a successful validated round. Event
    /// previews remain partial observations and never authorize tool execution.
    pub async fn stream_tool_round(
        &self,
        request: &ToolRequest<'_>,
        capabilities: &ToolCapabilities,
        events: &mpsc::Sender<ModelEvent>,
        cancellation: &CancellationToken,
    ) -> Result<ToolRound, TransportError> {
        if cancellation.is_cancelled() || events.is_closed() {
            return Err(TransportError::Cancelled);
        }
        if !self.images.is_empty() {
            return Err(TransportError::InvalidConfiguration);
        }
        let (body, decoder) = if self.dialect == ResponsesDialect::ChatGptPlan {
            (
                super::protocol::tools::encode_chatgpt_plan_tool_request(
                    request,
                    &self.capabilities,
                    capabilities,
                )?,
                ToolRoundDecoder::chatgpt_plan(request, &self.capabilities, capabilities)?,
            )
        } else {
            (
                encode_tool_request(self.protocol, request, &self.capabilities, capabilities)?,
                ToolRoundDecoder::new(self.protocol, request, &self.capabilities, capabilities)?,
            )
        };
        tokio::select! {
            biased;
            _ = cancellation.cancelled() => Err(TransportError::Cancelled),
            _ = events.closed() => Err(TransportError::Cancelled),
            result = tokio::time::timeout(self.limits.total, self.stream_encoded(body, decoder, events)) =>
                result.map_err(|_| TransportError::Timeout)?,
        }
    }

    async fn check_status(&self, response: &mut reqwest::Response) -> Result<(), TransportError> {
        let error = match check_status(response) {
            Ok(()) => return Ok(()),
            Err(error) => error,
        };
        if self.dialect != ResponsesDialect::ChatGptPlan {
            return Err(error);
        }
        let status = response.status().as_u16();
        let retry_after_seconds = response
            .headers()
            .get(header::RETRY_AFTER)
            .and_then(|h| h.to_str().ok())
            .and_then(|v| v.parse::<u64>().ok())
            .filter(|n| *n <= 86_400);
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(map_network_error)? {
            if bytes.len().saturating_add(chunk.len()) > 16 * 1024 {
                return Err(error);
            }
            bytes.extend_from_slice(&chunk);
        }
        if let Ok(value) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(code) = PlanError::from_value(&value)
        {
            return Err(TransportError::ChatGptPlan {
                status,
                code,
                retry_after_seconds,
            });
        }
        Err(error)
    }

    async fn stream_encoded<D: WireDecoder>(
        &self,
        mut body: serde_json::Value,
        mut decoder: D,
        events: &mpsc::Sender<ModelEvent>,
    ) -> Result<D::Output, TransportError> {
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
        if body.len()
            > if self.images.is_empty() {
                1024 * 1024
            } else {
                12 * 1024 * 1024
            }
        {
            return Err(TransportError::LimitExceeded);
        }
        let client = self.http_client(&self.endpoint).await?;
        let call = self.authorize(
            client
                .post(self.endpoint.clone())
                .header(header::ACCEPT, "text/event-stream")
                .header(header::CONTENT_TYPE, "application/json")
                .body(body),
        )?;
        let mut response = call.send().await.map_err(map_network_error)?;
        self.check_status(&mut response).await?;
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|h| h.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|s| s.eq_ignore_ascii_case("text/event-stream")) {
            return Err(TransportError::ContentType);
        }
        let mut received = 0usize;
        let mut plan_terminal = None;
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
                    if self.dialect == ResponsesDialect::ChatGptPlan
                        && matches!(event, ModelEvent::Finished { .. })
                    {
                        plan_terminal = Some(event);
                        continue;
                    }
                    events
                        .send(event)
                        .await
                        .map_err(|_| TransportError::Cancelled)?;
                }
                if let Some(error) = batch.error {
                    return Err(TransportError::Protocol(error));
                }
            }
            // Text previews retain their existing terminal-marker behavior.
            // Executable tool rounds must also observe clean HTTP EOF: otherwise
            // an error/duplicate terminal in a later chunk could be missed.
            if decoder.is_terminal()
                && !D::REQUIRE_CLEAN_EOF
                && self.dialect != ResponsesDialect::ChatGptPlan
            {
                return decoder.finish().map_err(TransportError::Protocol);
            }
        }
        let result = decoder.finish().map_err(TransportError::Protocol)?;
        if let Some(event) = plan_terminal {
            events
                .send(event)
                .await
                .map_err(|_| TransportError::Cancelled)?;
        }
        Ok(result)
    }
}

trait WireDecoder {
    type Output;
    const REQUIRE_CLEAN_EOF: bool;
    fn push(&mut self, bytes: &[u8]) -> super::protocol::DecodeBatch;
    fn is_terminal(&self) -> bool;
    fn finish(self) -> Result<Self::Output, ProtocolError>;
}
impl WireDecoder for StreamDecoder {
    type Output = ();
    const REQUIRE_CLEAN_EOF: bool = false;
    fn push(&mut self, bytes: &[u8]) -> super::protocol::DecodeBatch {
        StreamDecoder::push(self, bytes)
    }
    fn is_terminal(&self) -> bool {
        StreamDecoder::is_terminal(self)
    }
    fn finish(mut self) -> Result<(), ProtocolError> {
        StreamDecoder::finish(&mut self)
    }
}
impl WireDecoder for ToolRoundDecoder {
    type Output = ToolRound;
    const REQUIRE_CLEAN_EOF: bool = true;
    fn push(&mut self, bytes: &[u8]) -> super::protocol::DecodeBatch {
        ToolRoundDecoder::push(self, bytes)
    }
    fn is_terminal(&self) -> bool {
        ToolRoundDecoder::is_terminal(self)
    }
    fn finish(self) -> Result<ToolRound, ProtocolError> {
        ToolRoundDecoder::finish(self)
    }
}

fn check_status(response: &reqwest::Response) -> Result<(), TransportError> {
    if response.status().as_u16() == 200 {
        return Ok(());
    }
    let retry_after_seconds = response
        .headers()
        .get(header::RETRY_AFTER)
        .and_then(|h| h.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|value| *value <= 86_400);
    Err(TransportError::Http {
        status: response.status().as_u16(),
        retry_after_seconds,
    })
}

fn parse_catalog(protocol: Protocol, bytes: &[u8]) -> Result<Catalog, TransportError> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| TransportError::InvalidCatalog)?;
    let entries = value
        .get("data")
        .and_then(|data| data.as_array())
        .ok_or(TransportError::InvalidCatalog)?;
    let mut seen = std::collections::BTreeSet::new();
    let mut models = Vec::new();
    // A provider's display names are untrusted, bounded plain text in React.
    for entry in entries.iter().take(512) {
        let id = entry
            .get("id")
            .and_then(|v| v.as_str())
            .filter(|s| {
                !s.is_empty()
                    && s.len() <= 256
                    && !s.chars().any(|c| c.is_control() || c.is_whitespace())
            })
            .ok_or(TransportError::InvalidCatalog)?;
        let name = entry
            .get(if protocol == Protocol::AnthropicMessages {
                "display_name"
            } else {
                "name"
            })
            .and_then(|v| v.as_str())
            .unwrap_or(id);
        if name.is_empty() || name.len() > 512 || name.chars().any(char::is_control) {
            return Err(TransportError::InvalidCatalog);
        }
        if seen.insert(id.to_owned()) {
            models.push(CatalogModel {
                id: id.to_owned(),
                name: name.to_owned(),
            });
        }
    }
    let has_more = match value.get("has_more") {
        None => false,
        Some(value) => value.as_bool().ok_or(TransportError::InvalidCatalog)?,
    };
    let truncated = entries.len() > 512
        || has_more
        || (protocol == Protocol::OpenRouterChat && entries.len() == 512);
    Ok(Catalog { models, truncated })
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
