use super::ReadError;
use crate::gmail::SecretString;
use reqwest::{Client, Method, header};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use zeroize::Zeroizing;

pub(super) enum Endpoint {
    Gmail,
    Slack,
}
#[derive(Clone)]
pub(super) struct Transport {
    #[cfg(test)]
    test_base: Option<String>,
    #[cfg(test)]
    pub(super) test_limit: Option<usize>,
}
impl Transport {
    pub(super) fn fixed() -> Self {
        Self {
            #[cfg(test)]
            test_base: None,
            #[cfg(test)]
            test_limit: None,
        }
    }
    #[cfg(test)]
    pub(super) fn fixture(base: String, limit: usize) -> Self {
        Self {
            test_base: Some(base),
            test_limit: Some(limit),
        }
    }
    pub(super) fn target(&self, endpoint: &Endpoint, path: &str) -> Result<Url, ReadError> {
        // `path` is assembled exclusively by typed methods, never passed by a
        // public caller; reject an accidental absolute or parent-relative path.
        if path.starts_with('/')
            || path
                .split('?')
                .next()
                .is_some_and(|p| p.contains("://") || p.contains(".."))
        {
            return Err(ReadError::InvalidInput);
        }
        #[cfg(test)]
        if let Some(base) = &self.test_base {
            return Url::parse(&format!("{base}/{path}")).map_err(|_| ReadError::InvalidInput);
        }
        match endpoint {
            Endpoint::Slack => Url::parse(&format!("https://slack.com/api/{path}")),
            Endpoint::Gmail => Url::parse(&format!(
                "https://gmail.googleapis.com/gmail/v1/users/me/{path}"
            )),
        }
        .map_err(|_| ReadError::InvalidInput)
    }
    async fn client(&self, url: &Url) -> Result<Client, ReadError> {
        let host = url.host_str().ok_or(ReadError::DestinationDenied)?;
        let port = url
            .port_or_known_default()
            .ok_or(ReadError::DestinationDenied)?;
        #[cfg(test)]
        let fixture = self.test_base.is_some();
        #[cfg(not(test))]
        let fixture = false;
        if !fixture
            && (url.scheme() != "https"
                || port != 443
                || !matches!(host, "slack.com" | "gmail.googleapis.com"))
        {
            return Err(ReadError::DestinationDenied);
        }
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| ReadError::Transport)?
            .take(33)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses
                .iter()
                .any(|a| !(public_address(a.ip()) || fixture && a.ip().is_loopback()))
        {
            return Err(ReadError::DestinationDenied);
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
            .map_err(|_| ReadError::Transport)
    }
    pub(super) async fn request(
        &self,
        endpoint: Endpoint,
        path: &str,
        token: Option<&SecretString>,
        cancel: &CancellationToken,
    ) -> Result<Zeroizing<Vec<u8>>, ReadError> {
        if cancel.is_cancelled() {
            return Err(ReadError::Cancelled);
        }
        // Total timeout includes DNS resolution, not only HTTP socket time.
        tokio::select! {
            biased;
            _=cancel.cancelled()=>Err(ReadError::Cancelled),
            result=tokio::time::timeout(Duration::from_secs(30),self.inner(endpoint,path,token))=>result.map_err(|_|ReadError::Timeout)?,
        }
    }
    async fn inner(
        &self,
        endpoint: Endpoint,
        path: &str,
        token: Option<&SecretString>,
    ) -> Result<Zeroizing<Vec<u8>>, ReadError> {
        let url = self.target(&endpoint, path)?;
        let client = self.client(&url).await?;
        let mut request = client
            .request(
                if matches!(endpoint, Endpoint::Slack)
                    && path.split('?').next() == Some("auth.test")
                {
                    Method::POST
                } else {
                    Method::GET
                },
                url,
            )
            .header(header::ACCEPT, "application/json");
        if let Some(secret) = token {
            let mut value = header::HeaderValue::from_str(&format!("Bearer {}", secret.expose()))
                .map_err(|_| ReadError::InvalidInput)?;
            value.set_sensitive(true);
            request = request.header(header::AUTHORIZATION, value);
        }
        let mut response = request.send().await.map_err(network)?;
        if !response.status().is_success() {
            let code = response.status().as_u16();
            // Gmail also reports quota exhaustion as HTTP 403. Inspect only a
            // bounded error code envelope; never return raw provider messages.
            if code == 403 && matches!(endpoint, Endpoint::Gmail) {
                let mut error_bytes = Zeroizing::new(Vec::new());
                while let Some(chunk) = response.chunk().await.map_err(network)? {
                    if error_bytes.len().saturating_add(chunk.len()) > 64 * 1024 {
                        return Err(ReadError::ScopeDenied);
                    }
                    error_bytes.extend_from_slice(&chunk);
                }
                let quota = serde_json::from_slice::<serde_json::Value>(&error_bytes)
                    .ok()
                    .and_then(|v| v.get("error")?.get("errors")?.as_array().cloned())
                    .is_some_and(|errors| {
                        errors.iter().any(|error| {
                            matches!(
                                error.get("reason").and_then(|v| v.as_str()),
                                Some(
                                    "rateLimitExceeded"
                                        | "userRateLimitExceeded"
                                        | "dailyLimitExceeded"
                                )
                            )
                        })
                    });
                return Err(if quota {
                    ReadError::RateLimited {
                        retry_after_seconds: 60,
                    }
                } else {
                    ReadError::ScopeDenied
                });
            }
            return Err(match code {
                401 => ReadError::AuthenticationRequired,
                403 => ReadError::ScopeDenied,
                429 => ReadError::RateLimited {
                    retry_after_seconds: response
                        .headers()
                        .get(header::RETRY_AFTER)
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .unwrap_or(60)
                        .clamp(1, 86_400),
                },
                code => ReadError::Http(code),
            });
        }
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(';').next())
            .map(str::trim);
        if !content_type.is_some_and(|s| s.eq_ignore_ascii_case("application/json")) {
            return Err(ReadError::InvalidResponse);
        }
        let limit = 1024 * 1024;
        #[cfg(test)]
        let limit = self.test_limit.unwrap_or(limit).min(limit);
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(ReadError::LimitExceeded);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if bytes
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > limit)
            {
                return Err(ReadError::LimitExceeded);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }
}
fn network(error: reqwest::Error) -> ReadError {
    if error.is_timeout() {
        ReadError::Timeout
    } else {
        ReadError::Transport
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
