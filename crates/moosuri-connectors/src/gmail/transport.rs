use super::{GmailError, SecretString};
use reqwest::{Client, Method, header};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

pub(crate) enum Endpoint {
    Token,
    Gmail,
    Calendar,
}
pub(crate) enum Body<'a> {
    Empty,
    Form(Vec<(&'a str, &'a str)>),
    Json(serde_json::Value),
}
#[derive(Clone)]
pub(crate) struct Transport {
    #[cfg(any(test, feature = "test-fixtures"))]
    test_base: Option<String>,
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) test_limit: Option<usize>,
}
impl Transport {
    pub(crate) fn google() -> Self {
        Self {
            #[cfg(any(test, feature = "test-fixtures"))]
            test_base: None,
            #[cfg(any(test, feature = "test-fixtures"))]
            test_limit: None,
        }
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(crate) fn fixture(base: String, limit: usize) -> Self {
        Self {
            test_base: Some(base),
            test_limit: Some(limit),
        }
    }
    pub(crate) fn target(&self, endpoint: &Endpoint, path: &str) -> Result<Url, GmailError> {
        // `path` is assembled exclusively by typed methods, never passed by a
        // public caller; reject an accidental absolute or parent-relative path.
        if path.starts_with('/')
            || path
                .split('?')
                .next()
                .is_some_and(|p| p.contains("://") || p.contains(".."))
        {
            return Err(GmailError::InvalidInput);
        }
        #[cfg(any(test, feature = "test-fixtures"))]
        if let Some(base) = &self.test_base {
            return Url::parse(&format!("{base}/{path}")).map_err(|_| GmailError::InvalidInput);
        }
        match endpoint {
            Endpoint::Token if path.is_empty() => Url::parse("https://oauth2.googleapis.com/token"),
            Endpoint::Calendar => {
                Url::parse(&format!("https://www.googleapis.com/calendar/v3/{path}"))
            }
            Endpoint::Gmail => Url::parse(&format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/{path}"
            )),
            _ => return Err(GmailError::InvalidInput),
        }
        .map_err(|_| GmailError::InvalidInput)
    }
    async fn client(&self, url: &Url) -> Result<Client, GmailError> {
        let host = url.host_str().ok_or(GmailError::DestinationDenied)?;
        let port = url
            .port_or_known_default()
            .ok_or(GmailError::DestinationDenied)?;
        #[cfg(any(test, feature = "test-fixtures"))]
        let fixture = self.test_base.is_some();
        #[cfg(not(any(test, feature = "test-fixtures")))]
        let fixture = false;
        if !fixture
            && (url.scheme() != "https"
                || port != 443
                || !matches!(
                    host,
                    "oauth2.googleapis.com" | "gmail.googleapis.com" | "www.googleapis.com"
                ))
        {
            return Err(GmailError::DestinationDenied);
        }
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| GmailError::Transport)?
            .take(33)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses
                .iter()
                .any(|a| !(public_address(a.ip()) || fixture && a.ip().is_loopback()))
        {
            return Err(GmailError::DestinationDenied);
        }
        Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| GmailError::Transport)
    }
    pub(crate) async fn request(
        &self,
        endpoint: Endpoint,
        method: Method,
        path: &str,
        token: Option<&SecretString>,
        body: Body<'_>,
        cancel: &CancellationToken,
    ) -> Result<Zeroizing<Vec<u8>>, GmailError> {
        if cancel.is_cancelled() {
            return Err(GmailError::Cancelled);
        }
        // Total timeout includes DNS resolution, not only HTTP socket time.
        tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(GmailError::Cancelled),
            result=tokio::time::timeout(Duration::from_secs(30),self.inner(endpoint,method,path,token,body,None))=>result.map_err(|_|GmailError::Timeout)?.map(|v|v.2),
        }
    }
    pub(crate) async fn calendar(
        &self,
        method: Method,
        path: &str,
        token: &SecretString,
        body: Body<'_>,
        etag: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(u16, Option<String>, Zeroizing<Vec<u8>>), GmailError> {
        tokio::select! {biased;_=cancel.cancelled()=>Err(GmailError::Cancelled),result=tokio::time::timeout(Duration::from_secs(30),self.inner(Endpoint::Calendar,method,path,Some(token),body,etag))=>result.map_err(|_|GmailError::Timeout)?,}
    }
    async fn inner(
        &self,
        endpoint: Endpoint,
        method: Method,
        path: &str,
        token: Option<&SecretString>,
        body: Body<'_>,
        etag: Option<&str>,
    ) -> Result<(u16, Option<String>, Zeroizing<Vec<u8>>), GmailError> {
        let url = self.target(&endpoint, path)?;
        let client = self.client(&url).await?;
        let mut request = client
            .request(method, url)
            .header(header::ACCEPT, "application/json");
        if let Some(secret) = token {
            let mut value = header::HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
                .map_err(|_| GmailError::InvalidInput)?;
            value.set_sensitive(true);
            request = request.header(header::AUTHORIZATION, value);
        }
        request = match body {
            Body::Empty => request,
            Body::Form(fields) => {
                let mut form = url::form_urlencoded::Serializer::new(String::new());
                for (name, value) in fields {
                    form.append_pair(name, value);
                }
                request
                    .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                    .body(form.finish())
            }
            Body::Json(value) => request.json(&value),
        };
        if let Some(etag) = etag {
            if etag.len() > 512 || etag.chars().any(char::is_control) {
                return Err(GmailError::InvalidInput);
            }
            request = request.header(header::IF_MATCH, etag);
        }
        let mut response = request.send().await.map_err(network)?;
        if !response.status().is_success() {
            return Err(GmailError::Http(response.status().as_u16()));
        }
        let status = response.status().as_u16();
        let etag = response
            .headers()
            .get(header::ETAG)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned);
        if status == 204 && matches!(endpoint, Endpoint::Calendar) {
            return Ok((status, etag, Zeroizing::new(Vec::new())));
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|s| s.eq_ignore_ascii_case("application/json")) {
            return Err(GmailError::InvalidResponse);
        }
        let limit = match endpoint {
            Endpoint::Token => 64 * 1024,
            Endpoint::Gmail | Endpoint::Calendar => 1024 * 1024,
        };
        #[cfg(any(test, feature = "test-fixtures"))]
        let limit = self.test_limit.unwrap_or(limit).min(limit);
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(GmailError::LimitExceeded);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if bytes
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > limit)
            {
                return Err(GmailError::LimitExceeded);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok((status, etag, bytes))
    }
}
fn network(error: reqwest::Error) -> GmailError {
    if error.is_timeout() {
        GmailError::Timeout
    } else {
        GmailError::Transport
    }
}
fn public_address(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(ip) => ip
            .to_ipv4_mapped()
            .map(IpAddr::V4)
            .unwrap_or(IpAddr::V6(ip)),
        v => v,
    };
    match ip {
        IpAddr::V4(ip) => {
            let o = ip.octets();
            !(ip.is_private()
                || ip.is_loopback()
                || ip.is_link_local()
                || ip.is_unspecified()
                || ip.is_multicast()
                || ip.is_broadcast()
                || ip.is_documentation()
                || o[0] == 0
                || o[0] >= 224
                || o == [168, 63, 129, 16]
                || (o[0] == 100 && (64..=127).contains(&o[1]))
                || (o[0] == 192 && o[1] == 0)
                || (o[0] == 198 && (18..=19).contains(&o[1])))
        }
        IpAddr::V6(ip) => {
            let s = ip.segments();
            s[0] & 0xe000 == 0x2000
                && s[0] != 0x2002
                && !(s[0] == 0x2001 && (s[1] == 0xdb8 || s[1] == 0))
        }
    }
}
