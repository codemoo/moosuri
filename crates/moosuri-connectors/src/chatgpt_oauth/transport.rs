use super::{AuthError, ReauthReason};
use reqwest::{Client, header};
use serde::Deserialize;
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use zeroize::Zeroizing;

#[derive(Clone, Copy)]
pub(super) enum Endpoint {
    Discovery,
    Jwks,
    Token,
    Revoke,
}
impl Endpoint {
    fn path(self) -> &'static str {
        match self {
            Self::Discovery => "/.well-known/openid-configuration",
            Self::Jwks => "/.well-known/jwks.json",
            Self::Token => "/api/accounts/oauth/token",
            Self::Revoke => "/api/accounts/oauth/revoke",
        }
    }
    fn limit(self) -> usize {
        if matches!(self, Self::Jwks) {
            256 * 1024
        } else {
            64 * 1024
        }
    }
}
pub(super) struct Transport {
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(super) fixture: Option<String>,
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(super) deadline: Duration,
    #[cfg(any(test, feature = "test-fixtures"))]
    pub(super) body_limit: Option<usize>,
}
impl Transport {
    pub(super) fn official() -> Self {
        Self {
            #[cfg(any(test, feature = "test-fixtures"))]
            fixture: None,
            #[cfg(any(test, feature = "test-fixtures"))]
            deadline: Duration::from_secs(30),
            #[cfg(any(test, feature = "test-fixtures"))]
            body_limit: None,
        }
    }
    pub(super) async fn get(&self, endpoint: Endpoint) -> Result<Zeroizing<Vec<u8>>, AuthError> {
        self.request(endpoint, None).await
    }
    pub(super) async fn form(
        &self,
        endpoint: Endpoint,
        fields: &[(&str, &str)],
    ) -> Result<Zeroizing<Vec<u8>>, AuthError> {
        self.request(endpoint, Some(fields)).await
    }
    async fn request(
        &self,
        endpoint: Endpoint,
        fields: Option<&[(&str, &str)]>,
    ) -> Result<Zeroizing<Vec<u8>>, AuthError> {
        #[cfg(not(any(test, feature = "test-fixtures")))]
        let deadline = Duration::from_secs(30);
        #[cfg(any(test, feature = "test-fixtures"))]
        let deadline = self.deadline;
        // Includes DNS and complete bounded body, including error responses.
        tokio::time::timeout(deadline, self.inner(endpoint, fields))
            .await
            .map_err(|_| AuthError::Timeout)?
    }
    async fn inner(
        &self,
        endpoint: Endpoint,
        fields: Option<&[(&str, &str)]>,
    ) -> Result<Zeroizing<Vec<u8>>, AuthError> {
        #[cfg(any(test, feature = "test-fixtures"))]
        let (base, fixture) = (
            self.fixture.as_deref().unwrap_or(super::ISSUER),
            self.fixture.is_some(),
        );
        #[cfg(not(any(test, feature = "test-fixtures")))]
        let (base, fixture) = (super::ISSUER, false);
        let url = url::Url::parse(&format!("{base}{}", endpoint.path()))
            .map_err(|_| AuthError::DestinationDenied)?;
        let host = url.host_str().ok_or(AuthError::DestinationDenied)?;
        let port = url
            .port_or_known_default()
            .ok_or(AuthError::DestinationDenied)?;
        if !fixture && (url.scheme() != "https" || host != "auth.openai.com" || port != 443) {
            return Err(AuthError::DestinationDenied);
        }
        if fixture && (url.scheme() != "http" || host != "127.0.0.1") {
            return Err(AuthError::DestinationDenied);
        }
        let addresses: Vec<SocketAddr> = tokio::net::lookup_host((host, port))
            .await
            .map_err(|_| AuthError::Transport)?
            .take(33)
            .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses
                .iter()
                .any(|a| !(public_address(a.ip()) || fixture && a.ip().is_loopback()))
        {
            return Err(AuthError::DestinationDenied);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(10))
            .timeout(Duration::from_secs(25))
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| AuthError::Transport)?;
        let request = if let Some(fields) = fields {
            let mut encoded = url::form_urlencoded::Serializer::new(String::new());
            for (name, value) in fields {
                encoded.append_pair(name, value);
            }
            client
                .post(url)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(encoded.finish())
        } else {
            client.get(url)
        };
        let mut response = request
            .header(header::ACCEPT, "application/json")
            .send()
            .await
            .map_err(network)?;
        let status = response.status().as_u16();
        let is_json = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.split(';').next())
            .is_some_and(|s| s.trim().eq_ignore_ascii_case("application/json"));
        let limit = endpoint.limit();
        #[cfg(any(test, feature = "test-fixtures"))]
        let limit = self.body_limit.unwrap_or(limit).min(limit);
        if response.content_length().is_some_and(|n| n > limit as u64) {
            return Err(AuthError::LimitExceeded);
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(network)? {
            if bytes
                .len()
                .checked_add(chunk.len())
                .is_none_or(|n| n > limit)
            {
                return Err(AuthError::LimitExceeded);
            }
            bytes.extend_from_slice(&chunk);
        }
        if status != 200 {
            if matches!(endpoint, Endpoint::Token)
                && matches!(status, 400 | 401)
                && is_json
                && let Ok(error) = serde_json::from_slice::<OAuthError>(&bytes)
                && let Some(reason) = reauth_reason(&error.error)
            {
                return Err(AuthError::ReauthorizationRequired(reason));
            }
            return Err(
                if status == 408 || status == 425 || status == 429 || status >= 500 {
                    AuthError::TemporaryHttp(status)
                } else {
                    AuthError::HttpRejected(status)
                },
            );
        }
        if !matches!(endpoint, Endpoint::Revoke) && !is_json {
            return Err(AuthError::InvalidResponse);
        }
        Ok(bytes)
    }
}
#[derive(Deserialize)]
struct OAuthError {
    error: String,
}
fn reauth_reason(code: &str) -> Option<ReauthReason> {
    Some(match code {
        "invalid_grant" => ReauthReason::InvalidGrant,
        "invalid_refresh_token" => ReauthReason::InvalidRefreshToken,
        "token_expired" => ReauthReason::TokenExpired,
        "refresh_token_expired" => ReauthReason::RefreshTokenExpired,
        "refresh_token_invalidated" => ReauthReason::RefreshTokenInvalidated,
        "refresh_token_reused" => ReauthReason::RefreshTokenReused,
        _ => return None,
    })
}
fn network(error: reqwest::Error) -> AuthError {
    if error.is_timeout() {
        AuthError::Timeout
    } else {
        AuthError::Transport
    }
}
pub(super) fn public_address(ip: IpAddr) -> bool {
    let ip = match ip {
        IpAddr::V6(v) => v.to_ipv4_mapped().map(IpAddr::V4).unwrap_or(IpAddr::V6(v)),
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
                && !(s[0] == 0x2001 && (s[1] < 0x0200 || s[1] == 0x0db8))
                && !(s[0] == 0x3fff && s[1] < 0x1000)
        }
    }
}
