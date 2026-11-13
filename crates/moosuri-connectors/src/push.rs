//! Private Web Push crypto/transport. No arbitrary URL sender or Debug carriers.
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use jwt_compact::{AlgorithmExt, Claims, Header, TimeOptions, alg::Es256};
use serde::{Deserialize, Serialize};
use std::{
    net::{IpAddr, SocketAddr},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use url::Url;
use web_push_native::{
    Auth, WebPushBuilder,
    p256::{PublicKey, ecdsa::SigningKey, elliptic_curve::rand_core::OsRng},
};
use zeroize::Zeroizing;
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PushSubscription {
    pub endpoint: String,
    pub p256dh: String,
    pub auth: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PushOutcome {
    Accepted,
    Expired,
    Retry(u32),
    Rejected,
    Unknown,
}
#[derive(Clone, Copy, Debug, thiserror::Error)]
#[error("Web Push is unavailable")]
pub struct PushError;
pub fn public_vapid(secret: &[u8]) -> Result<String, PushError> {
    let key = SigningKey::from_slice(secret).map_err(|_| PushError)?;
    Ok(URL_SAFE_NO_PAD.encode(key.verifying_key().to_encoded_point(false).as_bytes()))
}
pub fn generate_vapid() -> Result<(Zeroizing<Vec<u8>>, String), PushError> {
    let key = SigningKey::random(&mut OsRng);
    let bytes = Zeroizing::new(key.to_bytes().to_vec());
    let public = public_vapid(&bytes)?;
    Ok((bytes, public))
}
fn endpoint(input: &str) -> Result<Url, PushError> {
    if input.len() > 4096 {
        return Err(PushError);
    }
    let u = Url::parse(input).map_err(|_| PushError)?;
    let host = u.host_str().ok_or(PushError)?;
    let approved = matches!(
        host,
        "fcm.googleapis.com" | "updates.push.services.mozilla.com" | "web.push.apple.com"
    ) || host
        .strip_suffix(".notify.windows.com")
        .is_some_and(|label| {
            !label.is_empty()
                && label.len() <= 63
                && label
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        });
    let windows = host.ends_with(".notify.windows.com");
    let path_ok = if host == "fcm.googleapis.com" {
        u.path()
            .strip_prefix("/fcm/send/")
            .or_else(|| u.path().strip_prefix("/wp/"))
            .is_some_and(|v| !v.is_empty())
    } else if host == "updates.push.services.mozilla.com" {
        u.path()
            .strip_prefix("/wpush/v2/")
            .or_else(|| u.path().strip_prefix("/wpush/v1/"))
            .is_some_and(|v| !v.is_empty())
    } else if windows {
        matches!(u.path(), "/w/" | "/w") && u.query().is_some_and(|q| q.starts_with("token="))
    } else {
        let token = u.path().trim_start_matches('/');
        token.len() >= 8
            && token
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
    };
    if !path_ok
        || (!windows && u.query().is_some())
        || !approved
        || u.scheme() != "https"
        || u.port_or_known_default() != Some(443)
        || !u.username().is_empty()
        || u.password().is_some()
        || u.fragment().is_some()
        || u.path() == "/"
    {
        return Err(PushError);
    }
    Ok(u)
}
pub fn validate_subscription(s: &PushSubscription) -> Result<(), PushError> {
    endpoint(&s.endpoint)?;
    if s.p256dh.len() != 87 || s.auth.len() != 22 {
        return Err(PushError);
    }
    let public = URL_SAFE_NO_PAD.decode(&s.p256dh).map_err(|_| PushError)?;
    let auth = URL_SAFE_NO_PAD.decode(&s.auth).map_err(|_| PushError)?;
    if public.len() != 65 || public.first() != Some(&4) || auth.len() != 16 {
        return Err(PushError);
    }
    PublicKey::from_sec1_bytes(&public).map_err(|_| PushError)?;
    Ok(())
}
fn public_address(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => {
            let [o, s, _, _] = ip.octets();
            !(o == 0
                || o == 10
                || o == 127
                || o >= 224
                || o == 169 && s == 254
                || o == 172 && (16..=31).contains(&s)
                || o == 192 && (s == 0 || s == 168)
                || o == 100 && (64..=127).contains(&s)
                || o == 198 && (s == 18 || s == 19 || s == 51)
                || o == 203 && s == 0)
        }
        IpAddr::V6(ip) => {
            if let Some(v4) = ip.to_ipv4_mapped() {
                return public_address(IpAddr::V4(v4));
            }
            let s = ip.segments();
            !(ip.is_loopback()
                || ip.is_unspecified()
                || ip.is_multicast()
                || s[0] & 0xfe00 == 0xfc00
                || s[0] & 0xffc0 == 0xfe80
                || s[0] & 0xffc0 == 0xfec0
                || s[0] == 0
                || s[0] == 0x2002
                || s[0] == 0x64 && s[1] == 0xff9b
                || s[0] == 0x2001 && s[1] == 0x0db8)
        }
    }
}
fn signed(
    s: &PushSubscription,
    secret: &[u8],
    subject: &str,
    payload: &[u8],
) -> Result<reqwest::Request, PushError> {
    validate_subscription(s)?;
    if payload.len() > 512 {
        return Err(PushError);
    }
    let subject_url = Url::parse(subject).map_err(|_| PushError)?;
    if subject_url.scheme() != "https" || subject_url.host_str().is_none() {
        return Err(PushError);
    }
    let key = SigningKey::from_slice(secret).map_err(|_| PushError)?;
    let public = URL_SAFE_NO_PAD.decode(&s.p256dh).map_err(|_| PushError)?;
    let auth = URL_SAFE_NO_PAD.decode(&s.auth).map_err(|_| PushError)?;
    let message = WebPushBuilder::new(
        s.endpoint.parse().map_err(|_| PushError)?,
        PublicKey::from_sec1_bytes(&public).map_err(|_| PushError)?,
        Auth::clone_from_slice(&auth),
    )
    .with_valid_duration(Duration::from_secs(3600))
    .build(payload.to_vec())
    .map_err(|_| PushError)?;
    let (mut parts, body) = message.into_parts();
    #[derive(Serialize)]
    struct VapidClaims<'a> {
        aud: String,
        sub: &'a str,
    }
    let destination = endpoint(&s.endpoint)?;
    let claims = Claims::new(VapidClaims {
        aud: destination.origin().ascii_serialization(),
        sub: subject,
    })
    .set_duration_and_issuance(&TimeOptions::default(), chrono::Duration::hours(1));
    let token = Es256
        .token(&Header::empty(), &claims, &key)
        .map_err(|_| PushError)?;
    let authorization = format!("vapid t={token}, k={}", public_vapid(secret)?);
    parts.headers.insert(
        reqwest::header::AUTHORIZATION,
        authorization.parse().map_err(|_| PushError)?,
    );
    let mut request = reqwest::Request::new(reqwest::Method::POST, endpoint(&s.endpoint)?);
    *request.headers_mut() = parts.headers;
    *request.body_mut() = Some(body.into());
    Ok(request)
}
pub struct PreparedPush {
    client: reqwest::Client,
    request: reqwest::Request,
}
impl PreparedPush {
    pub async fn prepare(
        s: &PushSubscription,
        secret: &[u8],
        subject: &str,
        payload: &[u8],
    ) -> Result<Self, PushError> {
        let url = endpoint(&s.endpoint)?;
        let host = url.host_str().ok_or(PushError)?;
        let addresses: Vec<SocketAddr> =
            tokio::time::timeout(Duration::from_secs(5), tokio::net::lookup_host((host, 443)))
                .await
                .map_err(|_| PushError)?
                .map_err(|_| PushError)?
                .take(33)
                .collect();
        if addresses.is_empty()
            || addresses.len() > 32
            || addresses.iter().any(|a| !public_address(a.ip()))
        {
            return Err(PushError);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .https_only(true)
            .redirect(reqwest::redirect::Policy::none())
            .retry(reqwest::retry::never())
            .connect_timeout(Duration::from_secs(5))
            .read_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(0)
            .resolve_to_addrs(host, &addresses)
            .build()
            .map_err(|_| PushError)?;
        Ok(Self {
            client,
            request: signed(s, secret, subject, payload)?,
        })
    }
    pub async fn send(self, cancel: &CancellationToken) -> PushOutcome {
        let result = tokio::select! {biased;_=cancel.cancelled()=>return PushOutcome::Unknown,result=self.client.execute(self.request)=>result};
        let Ok(response) = result else {
            return PushOutcome::Unknown;
        };
        let status = response.status().as_u16();
        // Only bounded headers are interpreted. Never collect/log a provider body;
        // dropping the unpooled response cancels further body reads.
        let retry = response
            .headers()
            .get("retry-after")
            .and_then(|v| v.to_str().ok())
            .filter(|v| v.len() <= 10)
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(60)
            .clamp(30, 3600);
        match status {
            200..=299 => PushOutcome::Accepted,
            404 | 410 => PushOutcome::Expired,
            429 | 500..=599 => PushOutcome::Retry(retry),
            _ => PushOutcome::Rejected,
        }
    }
}
#[cfg(test)]
mod tests;
