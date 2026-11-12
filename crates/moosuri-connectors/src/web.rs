//! Unauthenticated public HTTP research. This does not run a browser or JavaScript.
//! URLs, response bodies and provider keys never appear in adapter errors.
use reqwest::{Client, Response, header};
use scraper::{ElementRef, Html, Node};
use serde::Serialize;
use std::{
    net::{IpAddr, SocketAddr},
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;
use url::{Host, Url};
use zeroize::Zeroizing;

const BRAVE_URL: &str = "https://api.search.brave.com/res/v1/web/search";
const MAX_URL: usize = 2048;
const MAX_BODY: usize = 1024 * 1024;
const MAX_TEXT: usize = 8192;
const MAX_RESULT: usize = 12 * 1024;
const TIMEOUT: Duration = Duration::from_secs(20);
// A cancelled blocking parse may finish in the background; bound that work too.
static PARSERS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);

/// Operator configuration, never model-selected. Deliberately not Debug/Serialize.
#[derive(Default)]
pub struct WebConfig {
    pub brave_api_key: Option<String>,
}

/// No credential/session sharing with model connections or personal browsers.
pub struct WebClient {
    brave_api_key: Option<Zeroizing<String>>,
    #[cfg(test)]
    test_endpoint: Option<Url>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum WebError {
    #[error("web search is not configured")]
    Unavailable,
    #[error("invalid web configuration")]
    InvalidConfiguration,
    #[error("invalid public HTTP URL")]
    InvalidUrl,
    #[error("destination is not a public address")]
    ForbiddenDestination,
    #[error("web DNS resolution failed")]
    Dns,
    #[error("web request failed")]
    Network,
    #[error("web request timed out")]
    Timeout,
    #[error("web request cancelled")]
    Cancelled,
    #[error("web response status is not successful")]
    Status,
    #[error("web redirect limit exceeded or invalid redirect")]
    Redirect,
    #[error("web response is too large")]
    TooLarge,
    #[error("unsupported web response type or encoding")]
    UnsupportedContent,
    #[error("search provider response is invalid")]
    InvalidResponse,
}

#[derive(Debug, Serialize)]
pub struct WebPage {
    pub url: String,
    pub title: String,
    pub text: String,
    pub content_type: String,
    pub retrieved_at_unix_ms: u64,
    pub truncated: bool,
    /// HTTP extraction never asserts that dynamically generated content was read.
    pub warning: &'static str,
}

#[derive(Debug, Serialize)]
pub struct SearchHit {
    pub title: String,
    pub url: String,
    pub description: String,
}

#[derive(Debug, Serialize)]
pub struct WebSearch {
    pub provider: &'static str,
    pub hits: Vec<SearchHit>,
    pub retrieved_at_unix_ms: u64,
    pub truncated: bool,
}

impl WebClient {
    pub fn has_search(&self) -> bool {
        self.brave_api_key.is_some()
    }
    pub fn new(config: WebConfig) -> Result<Self, WebError> {
        let brave_api_key = config.brave_api_key.map(Zeroizing::new);
        if brave_api_key.as_ref().is_some_and(|key| {
            key.is_empty()
                || key.len() > 8192
                || !key.is_ascii()
                || key
                    .bytes()
                    .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
        }) {
            return Err(WebError::InvalidConfiguration);
        }
        Ok(Self {
            brave_api_key,
            #[cfg(test)]
            test_endpoint: None,
        })
    }

    /// The caller persists this snapshot and may paginate its text locally. Re-fetching
    /// an URL is a new observation, never a continuation of an older document.
    pub async fn read(
        &self,
        raw_url: &str,
        cancel: &CancellationToken,
    ) -> Result<WebPage, WebError> {
        let url = parse_url(raw_url)?;
        self.bounded(cancel, self.read_inner(url)).await
    }

    pub async fn search(
        &self,
        query: &str,
        cancel: &CancellationToken,
    ) -> Result<WebSearch, WebError> {
        if query.trim().is_empty() || query.len() > 512 || query.chars().any(char::is_control) {
            return Err(WebError::InvalidUrl);
        }
        let key = self.brave_api_key.as_ref().ok_or(WebError::Unavailable)?;
        self.bounded(cancel, self.search_inner(query, key)).await
    }

    async fn bounded<T>(
        &self,
        cancel: &CancellationToken,
        task: impl std::future::Future<Output = Result<T, WebError>>,
    ) -> Result<T, WebError> {
        tokio::select! { biased;
            _ = cancel.cancelled() => Err(WebError::Cancelled),
            result = tokio::time::timeout(TIMEOUT, task) => result.map_err(|_| WebError::Timeout)?,
        }
    }

    async fn read_inner(&self, mut url: Url) -> Result<WebPage, WebError> {
        for hop in 0..=3 {
            let client = self.client(&url).await?;
            let response = client
                .get(url.clone())
                .send()
                .await
                .map_err(|_| WebError::Network)?;
            if response.status().is_redirection() {
                if hop == 3 {
                    return Err(WebError::Redirect);
                }
                let location = response
                    .headers()
                    .get(header::LOCATION)
                    .and_then(|h| h.to_str().ok())
                    .ok_or(WebError::Redirect)?;
                let next = url.join(location).map_err(|_| WebError::Redirect)?;
                url = parse_url(next.as_str())?;
                continue;
            }
            if !response.status().is_success() {
                return Err(WebError::Status);
            }
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|h| h.to_str().ok())
                .unwrap_or("")
                .split(';')
                .next()
                .unwrap_or("")
                .trim()
                .to_ascii_lowercase();
            if !matches!(
                content_type.as_str(),
                "text/html" | "application/xhtml+xml" | "text/plain" | "text/markdown"
            ) {
                return Err(WebError::UnsupportedContent);
            }
            let bytes = body(response).await?;
            let input = String::from_utf8_lossy(&bytes);
            let lossy = matches!(input, std::borrow::Cow::Owned(_));
            let input = input.into_owned();
            let is_html = matches!(content_type.as_str(), "text/html" | "application/xhtml+xml");
            let (title, text, truncated) = parse_blocking(move || {
                Ok(if is_html {
                    extract_html(&input)
                } else {
                    let mut text = Text::default();
                    text.push(&input);
                    (String::new(), text.value, text.truncated)
                })
            })
            .await?;
            let mut page = WebPage {
                url: url.into(),
                title,
                text,
                content_type,
                retrieved_at_unix_ms: now(),
                truncated,
                warning: if lossy {
                    "http_only; no_javascript; invalid_utf8_replaced"
                } else {
                    "http_only; no_javascript; dynamic_content_may_be_missing"
                },
            };
            while serde_json::to_vec(&page)
                .map_err(|_| WebError::InvalidResponse)?
                .len()
                > MAX_RESULT
            {
                page.truncated = true;
                shorten(&mut page.text, 256);
            }
            return Ok(page);
        }
        Err(WebError::Redirect)
    }

    async fn search_inner(&self, query: &str, key: &str) -> Result<WebSearch, WebError> {
        let endpoint = Url::parse(BRAVE_URL).map_err(|_| WebError::InvalidConfiguration)?;
        #[cfg(test)]
        let endpoint = self.test_endpoint.clone().unwrap_or(endpoint);
        let client = self.client(&endpoint).await?;
        let mut endpoint = endpoint;
        endpoint
            .query_pairs_mut()
            .append_pair("q", query)
            .append_pair("count", "5");
        let mut secret =
            header::HeaderValue::from_str(key).map_err(|_| WebError::InvalidConfiguration)?;
        secret.set_sensitive(true);
        let response = client
            .get(endpoint)
            .header("X-Subscription-Token", secret)
            .send()
            .await
            .map_err(|_| WebError::Network)?;
        if !response.status().is_success() {
            return Err(WebError::Status);
        }
        let bytes = body(response).await?;
        parse_blocking(move || parse_search(&bytes)).await
    }

    async fn client(&self, url: &Url) -> Result<Client, WebError> {
        let host = url.host_str().ok_or(WebError::InvalidUrl)?;
        let port = url.port_or_known_default().ok_or(WebError::InvalidUrl)?;
        let addresses: Vec<SocketAddr> = match url.host().ok_or(WebError::InvalidUrl)? {
            Host::Ipv4(ip) => vec![SocketAddr::new(ip.into(), port)],
            Host::Ipv6(ip) => vec![SocketAddr::new(ip.into(), port)],
            Host::Domain(_) => tokio::net::lookup_host((host, port))
                .await
                .map_err(|_| WebError::Dns)?
                .collect(),
        };
        if addresses.is_empty() || addresses.len() > 64 {
            return Err(WebError::Dns);
        }
        #[cfg(test)]
        let local_test = self.test_endpoint.as_ref().is_some_and(|u| {
            u.host_str() == url.host_str() && u.port_or_known_default() == Some(port)
        });
        #[cfg(not(test))]
        let local_test = false;
        validate_addresses(&addresses, local_test)?;
        Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .referer(false)
            .no_gzip()
            .no_brotli()
            .no_deflate()
            .no_zstd()
            .connect_timeout(Duration::from_secs(5))
            .timeout(TIMEOUT)
            .pool_max_idle_per_host(0)
            .http1_only()
            .user_agent("moosuri-public-research/0.1")
            .default_headers({
                let mut h = header::HeaderMap::new();
                h.insert(
                    header::ACCEPT_ENCODING,
                    header::HeaderValue::from_static("identity"),
                );
                h
            })
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| WebError::Network)
    }
}

fn parse_search(bytes: &[u8]) -> Result<WebSearch, WebError> {
    let json: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| WebError::InvalidResponse)?;
    let entries = json
        .get("web")
        .and_then(|w| w.get("results"))
        .and_then(|r| r.as_array());
    // A valid empty Brave response can omit `web`; provider error objects cannot.
    if json.get("error").is_some()
        || json.get("type").and_then(|v| v.as_str()) != Some("search")
        || (json.get("web").is_some() && entries.is_none())
    {
        return Err(WebError::InvalidResponse);
    }
    let mut result = WebSearch {
        provider: "brave",
        hits: Vec::new(),
        retrieved_at_unix_ms: now(),
        truncated: false,
    };
    if let Some(entries) = entries {
        result.truncated = entries.len() > 5;
        for entry in entries.iter().take(5) {
            let raw = entry
                .get("url")
                .and_then(|v| v.as_str())
                .ok_or(WebError::InvalidResponse)?;
            let Ok(url) = parse_url(raw) else {
                result.truncated = true;
                continue;
            };
            // URLs are reported as provider observations, not fetched or trusted as safe destinations.
            let mut title = Text::with_cap(256);
            title.push(entry.get("title").and_then(|v| v.as_str()).unwrap_or(""));
            let (_, description, description_truncated) = extract_html_with_cap(
                entry
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or(""),
                768,
            );
            result.truncated |= title.truncated || description_truncated;
            result.hits.push(SearchHit {
                title: title.value,
                url: url.into(),
                description,
            });
        }
    }
    while serde_json::to_vec(&result)
        .map_err(|_| WebError::InvalidResponse)?
        .len()
        > MAX_RESULT
    {
        result.truncated = true;
        result.hits.pop();
    }
    Ok(result)
}

async fn parse_blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, WebError> + Send + 'static,
) -> Result<T, WebError> {
    let permit = PARSERS.acquire().await.map_err(|_| WebError::Network)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        task()
    })
    .await
    .map_err(|_| WebError::InvalidResponse)?
}

fn parse_url(raw: &str) -> Result<Url, WebError> {
    if raw.len() > MAX_URL || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return Err(WebError::InvalidUrl);
    }
    if raw.split_once("://").is_some_and(|(_, rest)| {
        rest.split(['/', '?', '#'])
            .next()
            .is_some_and(|authority| authority.contains('@'))
    }) {
        return Err(WebError::InvalidUrl);
    }
    let mut url = Url::parse(raw).map_err(|_| WebError::InvalidUrl)?;
    if !matches!(url.scheme(), "http" | "https")
        || !url.username().is_empty()
        || url.password().is_some()
        || url.host().is_none()
        || url
            .port()
            .is_some_and(|p| p != if url.scheme() == "http" { 80 } else { 443 })
    {
        return Err(WebError::InvalidUrl);
    }
    if let Some(Host::Domain(host)) = url.host() {
        let h = host.trim_end_matches('.');
        if !h.contains('.')
            || [
                "localhost",
                "local",
                "internal",
                "home.arpa",
                "test",
                "invalid",
                "onion",
            ]
            .iter()
            .any(|suffix| h == *suffix || h.ends_with(&format!(".{suffix}")))
        {
            return Err(WebError::ForbiddenDestination);
        }
    }
    url.set_fragment(None);
    Ok(url)
}

fn validate_addresses(addresses: &[SocketAddr], local_test: bool) -> Result<(), WebError> {
    if addresses
        .iter()
        .all(|a| public_address(a.ip()) || (local_test && a.ip().is_loopback()))
    {
        Ok(())
    } else {
        Err(WebError::ForbiddenDestination)
    }
}

fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [a, b, c, _] = ip.octets();
            !(ip.octets() == [168, 63, 129, 16]
                || a == 0
                || a == 10
                || a == 127
                || a >= 224
                || (a == 100 && (64..=127).contains(&b))
                || (a == 169 && b == 254)
                || (a == 172 && (16..=31).contains(&b))
                || (a == 192 && (b == 0 || b == 168 || (b == 88 && c == 99)))
                || (a == 198 && ((18..=19).contains(&b) || (b == 51 && c == 100)))
                || (a == 203 && b == 0 && c == 113))
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_address(v4.into());
            }
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] < 0x200 || s[1] == 0xdb8))
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}

async fn body(mut response: Response) -> Result<Vec<u8>, WebError> {
    if response
        .headers()
        .get(header::CONTENT_ENCODING)
        .is_some_and(|v| v != "identity")
    {
        return Err(WebError::UnsupportedContent);
    }
    if response
        .content_length()
        .is_some_and(|n| n > MAX_BODY as u64)
    {
        return Err(WebError::TooLarge);
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| WebError::Network)? {
        if chunk.len() > MAX_BODY.saturating_sub(bytes.len()) {
            return Err(WebError::TooLarge);
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}
fn shorten(text: &mut String, amount: usize) {
    let mut end = text.len().saturating_sub(amount);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    text.truncate(end);
}

struct Text {
    value: String,
    cap: usize,
    truncated: bool,
}
impl Default for Text {
    fn default() -> Self {
        Self::with_cap(MAX_TEXT)
    }
}
impl Text {
    fn with_cap(cap: usize) -> Self {
        Self {
            value: String::new(),
            cap,
            truncated: false,
        }
    }
    fn push(&mut self, raw: &str) {
        for ch in raw.chars() {
            let ch = if ch.is_whitespace() {
                ' '
            } else if ch.is_control() {
                continue;
            } else {
                ch
            };
            if ch == ' ' && (self.value.is_empty() || self.value.ends_with(' ')) {
                continue;
            }
            if self.value.len() + ch.len_utf8() > self.cap {
                self.truncated = true;
                break;
            }
            self.value.push(ch);
        }
    }
}
fn extract_html(input: &str) -> (String, String, bool) {
    extract_html_with_cap(input, MAX_TEXT)
}
fn extract_html_with_cap(input: &str, cap: usize) -> (String, String, bool) {
    let html = Html::parse_document(input);
    let mut title = Text::with_cap(256);
    if let Ok(selector) = scraper::Selector::parse("title")
        && let Some(element) = html.select(&selector).next()
    {
        for text in element.text() {
            title.push(text);
        }
    }
    let mut text = Text::with_cap(cap);
    let mut visited = 0;
    walk(html.root_element(), &mut text, 0, &mut visited);
    (
        title.value.trim().to_owned(),
        text.value.trim().to_owned(),
        text.truncated,
    )
}
fn walk(element: ElementRef<'_>, text: &mut Text, depth: usize, visited: &mut usize) {
    *visited += 1;
    if depth > 128 || *visited > 100_000 {
        text.truncated = true;
        return;
    }
    let value = element.value();
    if matches!(
        value.name(),
        "script" | "style" | "template" | "head" | "svg" | "noscript" | "iframe" | "object"
    ) || value.attr("hidden").is_some()
        || value.attr("style").is_some_and(|s| {
            let s = s.replace(char::is_whitespace, "").to_ascii_lowercase();
            s.contains("display:none") || s.contains("visibility:hidden")
        })
    {
        return;
    }
    let block = matches!(
        value.name(),
        "p" | "div"
            | "section"
            | "article"
            | "br"
            | "li"
            | "h1"
            | "h2"
            | "h3"
            | "h4"
            | "td"
            | "tr"
            | "header"
            | "footer"
    );
    if block {
        text.push(" ");
    }
    for child in element.children() {
        if text.truncated {
            break;
        }
        if let Node::Text(raw) = child.value() {
            text.push(raw);
        } else if let Some(element) = ElementRef::wrap(child) {
            walk(element, text, depth + 1, visited);
        }
    }
    if block {
        text.push(" ");
    }
}

#[cfg(test)]
mod tests;
