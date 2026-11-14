//! Bounded official OSS Sign in with ChatGPT protocol, deliberately unwired.
//!
//! The caller owns authenticated-user authorization, durable attempt ownership,
//! issued-client persistence **before** exchange, encrypted atomic bundle storage,
//! cross-process refresh leases/generation CAS, and logout dispatch cancellation.
//! Consuming Rust values prevent local reuse; they are not durable authorization.
mod jwt;
pub mod sealed;
#[cfg(test)]
mod tests;
mod transport;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::rand::{SecureRandom, SystemRandom};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeSet,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use url::Url;
use zeroize::{Zeroize, Zeroizing};

pub const ISSUER: &str = "https://auth.openai.com";
const RESOURCE: &str = "https://api.openai.com/v1";
const SCOPES: &str =
    "openid profile email offline_access resource.invoke chatgpt.tokens.use.direct";
const DYNAMIC_CLIENT: &str = "dynamic_agent_client";
const ATTEMPT_TTL: Duration = Duration::from_secs(300);

/// Closed, sanitized errors: provider descriptions, URLs and tokens never enter errors.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AuthError {
    #[error("invalid authentication input")]
    InvalidInput,
    #[error("authentication attempt expired")]
    AttemptExpired,
    #[error("authentication callback mismatch")]
    CallbackMismatch,
    #[error("authentication denied")]
    AuthorizationDenied,
    #[error("registration incomplete")]
    RegistrationIncomplete,
    #[error("returning registration mismatch")]
    RegistrationMismatch,
    #[error("invalid authentication response")]
    InvalidResponse,
    #[error("identity verification failed")]
    IdentityInvalid,
    #[error("ChatGPT plan permission is absent")]
    PlanDisabled,
    #[error("access token expired; refresh required")]
    AccessExpired,
    #[error("authentication destination denied")]
    DestinationDenied,
    #[error("authentication response exceeds limit")]
    LimitExceeded,
    /// The token endpoint accepted a rotating refresh, but replacement validation
    /// did not finish. The old refresh token must never be redeemed again.
    #[error("refresh replacement could not be verified; reauthentication required")]
    RefreshOutcomeUnknown,
    #[error("temporary authentication transport failure")]
    Transport,
    #[error("authentication deadline elapsed")]
    Timeout,
    #[error("temporary authentication HTTP failure ({0})")]
    TemporaryHttp(u16),
    #[error("authentication HTTP request rejected ({0})")]
    HttpRejected(u16),
    #[error("reauthentication required ({0:?})")]
    ReauthorizationRequired(ReauthReason),
    #[error("secure randomness unavailable")]
    RandomUnavailable,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReauthReason {
    InvalidGrant,
    InvalidRefreshToken,
    TokenExpired,
    RefreshTokenExpired,
    RefreshTokenInvalidated,
    RefreshTokenReused,
}

/// Verified registration identity; no email or opaque workspace metadata is authority.
/// Persistence must also bind this tuple to the authenticated local owner.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedIdentity {
    client_id: String,
    subject: String,
    host_id: String,
}
impl VerifiedIdentity {
    pub fn issuer(&self) -> &'static str {
        ISSUER
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    pub fn subject(&self) -> &str {
        &self.subject
    }
    pub fn host_id(&self) -> &str {
        &self.host_id
    }
}

/// An issued client mapping is not a verified identity. Persist before redemption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IssuedRegistration {
    client_id: String,
    host_id: String,
}
impl IssuedRegistration {
    /// Non-authoritative metadata only. Caller must authorize its saved owner mapping.
    pub fn from_saved_metadata(client_id: String, host_id: String) -> Result<Self, AuthError> {
        if !safe_atom(&client_id, 512) || client_id == DYNAMIC_CLIENT || !safe_atom(&host_id, 256) {
            return Err(AuthError::InvalidInput);
        }
        Ok(Self { client_id, host_id })
    }
    pub fn client_id(&self) -> &str {
        &self.client_id
    }
    pub fn host_id(&self) -> &str {
        &self.host_id
    }
}

/// No Debug/Serialize: the complete URL is private, including state/nonce.
pub struct AuthorizationUrl(Zeroizing<String>);
impl AuthorizationUrl {
    pub fn for_system_browser(&self) -> &str {
        &self.0
    }
}

/// Neither cloneable nor serializable. Start a loopback listener before creating it.
/// A failed callback consumes the attempt as well as a successful callback.
pub struct AuthorizationAttempt {
    state: Zeroizing<String>,
    nonce: Zeroizing<String>,
    verifier: Zeroizing<String>,
    redirect: String,
    host_id: String,
    issued_client: Option<String>,
    returning: Option<VerifiedIdentity>,
    started: Instant,
}
/// Single-use pending redemption. The caller must persist `registration()` first.
pub struct PendingCode {
    code: Zeroizing<String>,
    nonce: Zeroizing<String>,
    verifier: Zeroizing<String>,
    redirect: String,
    registration: IssuedRegistration,
    returning: Option<VerifiedIdentity>,
    started: Instant,
}
impl PendingCode {
    pub fn registration(&self) -> &IssuedRegistration {
        &self.registration
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanGrant {
    Enabled,
    SignedInPlanDisabled,
}

/// Private rotating bundle. No Debug/Serialize/Clone. A successful refresh returns
/// a whole replacement; the caller must commit it with a durable lease and CAS.
pub struct TokenBundle {
    identity: VerifiedIdentity,
    access: Zeroizing<String>,
    refresh: Zeroizing<String>,
    id_token: Zeroizing<String>,
    nonce: Zeroizing<String>,
    scopes: BTreeSet<String>,
    received_at: u64,
    expires_at: u64,
    earliest_refresh_at: Option<serde_json::Value>,
}
/// Explicit sensitive persistence boundary. Never send this view to logs or models.
pub struct SealingView<'a> {
    pub access_token: &'a str,
    pub refresh_token: &'a str,
    pub id_token: &'a str,
    pub nonce: &'a str,
    pub earliest_refresh_at: Option<&'a serde_json::Value>,
}
impl TokenBundle {
    pub fn identity(&self) -> &VerifiedIdentity {
        &self.identity
    }
    pub fn scopes(&self) -> &BTreeSet<String> {
        &self.scopes
    }
    pub fn received_at_unix(&self) -> u64 {
        self.received_at
    }
    pub fn expires_at_unix(&self) -> u64 {
        self.expires_at
    }
    pub fn plan_grant(&self) -> PlanGrant {
        if self.scopes.contains("chatgpt.tokens.use.direct")
            && self.scopes.contains("resource.invoke")
        {
            PlanGrant::Enabled
        } else {
            PlanGrant::SignedInPlanDisabled
        }
    }
    pub fn plan_access_token(&self) -> Result<&str, AuthError> {
        if self.plan_grant() == PlanGrant::Enabled {
            if unix_now()? >= self.expires_at {
                return Err(AuthError::AccessExpired);
            }
            Ok(&self.access)
        } else {
            Err(AuthError::PlanDisabled)
        }
    }
    pub fn with_secrets_for_sealing<T>(&self, seal: impl FnOnce(SealingView<'_>) -> T) -> T {
        seal(SealingView {
            access_token: &self.access,
            refresh_token: &self.refresh,
            id_token: &self.id_token,
            nonce: &self.nonce,
            earliest_refresh_at: self.earliest_refresh_at.as_ref(),
        })
    }
}

/// Only runtime-validated fixed-origin discovery can construct this client.
/// No public custom origin, proxy, TLS bypass, raw bearer or unchecked JWT path.
pub struct ChatGptAuthClient {
    transport: transport::Transport,
}
impl ChatGptAuthClient {
    /// Synthetic loopback acceptance only; absent from default production builds.
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub async fn synthetic_fixture(origin: &str) -> Result<Self, AuthError> {
        let u = Url::parse(origin).map_err(|_| AuthError::DestinationDenied)?;
        if u.scheme() != "http"
            || u.host_str() != Some("127.0.0.1")
            || u.port().is_none_or(|p| p == 0 || p == 80)
            || !u.username().is_empty()
            || u.password().is_some()
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
        {
            return Err(AuthError::DestinationDenied);
        }
        Self::discover_using(transport::Transport {
            fixture: Some(origin.trim_end_matches('/').to_owned()),
            deadline: Duration::from_secs(30),
            body_limit: None,
        })
        .await
    }
    pub async fn discover() -> Result<Self, AuthError> {
        let transport = transport::Transport::official();
        Self::discover_using(transport).await
    }
    async fn discover_using(transport: transport::Transport) -> Result<Self, AuthError> {
        let bytes = transport.get(transport::Endpoint::Discovery).await?;
        let discovery: Discovery =
            serde_json::from_slice(&bytes).map_err(|_| AuthError::InvalidResponse)?;
        discovery.validate()?;
        Ok(Self { transport })
    }
    /// New installation/host registration. Host IDs are persistent random opaque
    /// values created by the caller, never copied between laptop and VM.
    pub fn begin_registration(
        &self,
        host_id: &str,
        loopback_port: u16,
    ) -> Result<(AuthorizationAttempt, AuthorizationUrl), AuthError> {
        self.begin(host_id, loopback_port, None, None)
    }
    pub fn begin_returning(
        &self,
        identity: &VerifiedIdentity,
        loopback_port: u16,
    ) -> Result<(AuthorizationAttempt, AuthorizationUrl), AuthError> {
        self.begin(
            &identity.host_id,
            loopback_port,
            Some(identity.client_id.clone()),
            Some(identity.clone()),
        )
    }
    /// Recover an incomplete registration after rejected/expired code exchange,
    /// consuming its retained issued mapping instead of registering another app.
    /// This carrier proves no subject. Once identity is verified, use
    /// `begin_returning` to retain the known subject pin. Durable owner selection
    /// and registration persistence remain caller duties.
    /// ```compile_fail
    /// # use moosuri_connectors::chatgpt_oauth::{ChatGptAuthClient, IssuedRegistration};
    /// fn reuse(client: &ChatGptAuthClient, issued: IssuedRegistration) {
    ///     let _ = client.begin_reauthorization(issued, 12345);
    ///     let _ = client.begin_reauthorization(issued, 12346);
    /// }
    /// ```
    pub fn begin_reauthorization(
        &self,
        registration: IssuedRegistration,
        loopback_port: u16,
    ) -> Result<(AuthorizationAttempt, AuthorizationUrl), AuthError> {
        self.begin(
            &registration.host_id,
            loopback_port,
            Some(registration.client_id),
            None,
        )
    }
    fn begin(
        &self,
        host_id: &str,
        port: u16,
        issued_client: Option<String>,
        returning: Option<VerifiedIdentity>,
    ) -> Result<(AuthorizationAttempt, AuthorizationUrl), AuthError> {
        if !safe_atom(host_id, 256) || port == 0 {
            return Err(AuthError::InvalidInput);
        }
        let attempt = AuthorizationAttempt {
            state: random_secret()?,
            nonce: random_secret()?,
            verifier: random_secret()?,
            redirect: format!("http://127.0.0.1:{port}/auth/callback"),
            host_id: host_id.to_owned(),
            issued_client,
            returning,
            started: Instant::now(),
        };
        let mut url = Url::parse("https://auth.openai.com/api/accounts/authorize")
            .map_err(|_| AuthError::InvalidInput)?;
        {
            let mut query = url.query_pairs_mut();
            query.append_pair(
                "client_id",
                attempt.issued_client.as_deref().unwrap_or(DYNAMIC_CLIENT),
            );
            if attempt.issued_client.is_none() {
                query.append_pair("agent_name_hint", "Moosuri");
            }
            query
                .append_pair("ext_agent_host_id", host_id)
                .append_pair("response_type", "code")
                .append_pair("redirect_uri", &attempt.redirect)
                .append_pair("scope", SCOPES)
                .append_pair("resource", RESOURCE)
                .append_pair("state", &attempt.state)
                .append_pair("nonce", &attempt.nonce)
                .append_pair("code_challenge_method", "S256")
                .append_pair(
                    "code_challenge",
                    &URL_SAFE_NO_PAD.encode(Sha256::digest(attempt.verifier.as_bytes())),
                );
        }
        Ok((attempt, AuthorizationUrl(Zeroizing::new(url.into()))))
    }
    /// Pending redemption cannot be replayed through the safe API.
    /// ```compile_fail
    /// # use moosuri_connectors::chatgpt_oauth::{ChatGptAuthClient, PendingCode};
    /// async fn replay(client: &ChatGptAuthClient, pending: PendingCode) {
    ///     let _ = client.exchange(pending).await;
    ///     let _ = client.exchange(pending).await;
    /// }
    /// ```
    pub async fn exchange(&self, pending: PendingCode) -> Result<TokenBundle, AuthError> {
        if pending.started.elapsed() >= ATTEMPT_TTL {
            return Err(AuthError::AttemptExpired);
        }
        let bytes = self
            .transport
            .form(
                transport::Endpoint::Token,
                &[
                    ("grant_type", "authorization_code"),
                    ("client_id", &pending.registration.client_id),
                    ("code", &pending.code),
                    ("code_verifier", &pending.verifier),
                    ("redirect_uri", &pending.redirect),
                    ("resource", RESOURCE),
                ],
            )
            .await?;
        let mut token = parse_token(&bytes)?;
        let raw_id = token
            .id_token
            .as_deref()
            .ok_or(AuthError::IdentityInvalid)?;
        let now = unix_now()?;
        let jwks = self.transport.get(transport::Endpoint::Jwks).await?;
        let subject = jwt::verify(
            raw_id,
            &jwks,
            &pending.registration.client_id,
            &pending.nonce,
            true,
            unix_now()?,
        )?;
        if pending
            .returning
            .as_ref()
            .is_some_and(|identity| identity.subject != subject)
        {
            return Err(AuthError::RegistrationMismatch);
        }
        let scopes = parse_scopes(token.scope.as_deref().ok_or(AuthError::InvalidResponse)?)?;
        let identity = VerifiedIdentity {
            client_id: pending.registration.client_id,
            subject,
            host_id: pending.registration.host_id,
        };
        let expires_at = now
            .checked_add(token.expires_in)
            .ok_or(AuthError::InvalidResponse)?;
        Ok(TokenBundle {
            identity,
            access: Zeroizing::new(std::mem::take(&mut token.access_token)),
            refresh: Zeroizing::new(std::mem::take(&mut token.refresh_token)),
            id_token: Zeroizing::new(token.id_token.take().ok_or(AuthError::IdentityInvalid)?),
            nonce: pending.nonce,
            scopes,
            received_at: now,
            expires_at,
            earliest_refresh_at: token.earliest_refresh_at.take(),
        })
    }
    /// Caller must hold the durable per-session refresh lease. No automatic retry:
    /// response loss after rotation is an unknown outcome, not safe replay.
    pub async fn refresh(&self, current: &TokenBundle) -> Result<TokenBundle, AuthError> {
        let bytes = self
            .transport
            .form(
                transport::Endpoint::Token,
                &[
                    ("grant_type", "refresh_token"),
                    ("client_id", &current.identity.client_id),
                    ("refresh_token", &current.refresh),
                    ("resource", RESOURCE),
                ],
            )
            .await?;
        // Only errors from the token request above can be a safe temporary
        // rejection. Once HTTP200 returned, the provider may have rotated:
        // parse/JWKS/signature/claim failures cannot re-enable the old token.
        async {
            let mut token = parse_token(&bytes)?;
            let now = unix_now()?;
            if let Some(raw_id) = &token.id_token {
                let jwks = self.transport.get(transport::Endpoint::Jwks).await?;
                let subject = jwt::verify(
                    raw_id,
                    &jwks,
                    &current.identity.client_id,
                    &current.nonce,
                    false,
                    unix_now()?,
                )?;
                if subject != current.identity.subject {
                    return Err(AuthError::RegistrationMismatch);
                }
            }
            let scopes = token
                .scope
                .as_deref()
                .map(parse_scopes)
                .transpose()?
                .unwrap_or_else(|| current.scopes.clone());
            let expires_at = now
                .checked_add(token.expires_in)
                .ok_or(AuthError::InvalidResponse)?;
            Ok::<TokenBundle, AuthError>(TokenBundle {
                identity: current.identity.clone(),
                access: Zeroizing::new(std::mem::take(&mut token.access_token)),
                refresh: Zeroizing::new(std::mem::take(&mut token.refresh_token)),
                id_token: Zeroizing::new(
                    token
                        .id_token
                        .take()
                        .unwrap_or_else(|| current.id_token.to_string()),
                ),
                nonce: Zeroizing::new(current.nonce.to_string()),
                scopes,
                received_at: now,
                expires_at,
                earliest_refresh_at: token.earliest_refresh_at.take(),
            })
        }
        .await
        .map_err(|_| AuthError::RefreshOutcomeUnknown)
    }
    /// Remote receipt only. Caller stops dispatch first and clears local tokens
    /// even when remote revocation cannot be confirmed; do not retain hidden tokens.
    pub async fn revoke(&self, current: &TokenBundle) -> Result<(), AuthError> {
        self.transport
            .form(
                transport::Endpoint::Revoke,
                &[
                    ("token", &current.refresh),
                    ("token_type_hint", "refresh_token"),
                    ("client_id", &current.identity.client_id),
                ],
            )
            .await?;
        Ok(())
    }
}
impl AuthorizationAttempt {
    /// A replay also fails at compile time after a rejected callback.
    /// ```compile_fail
    /// # use moosuri_connectors::chatgpt_oauth::AuthorizationAttempt;
    /// fn replay(attempt: AuthorizationAttempt) {
    ///     let _ = attempt.accept_callback("http://127.0.0.1:12345/auth/callback?state=wrong");
    ///     let _ = attempt.accept_callback("http://127.0.0.1:12345/auth/callback?state=wrong");
    /// }
    /// ```
    pub fn accept_callback(self, callback_url: &str) -> Result<PendingCode, AuthError> {
        if self.started.elapsed() >= ATTEMPT_TTL {
            return Err(AuthError::AttemptExpired);
        }
        if callback_url.len() > 16 * 1024 {
            return Err(AuthError::LimitExceeded);
        }
        let raw_query = callback_url
            .strip_prefix(&format!("{}?", self.redirect))
            .ok_or(AuthError::CallbackMismatch)?;
        let url = Url::parse(callback_url).map_err(|_| AuthError::CallbackMismatch)?;
        let expected = Url::parse(&self.redirect).map_err(|_| AuthError::CallbackMismatch)?;
        if url.scheme() != expected.scheme()
            || url.host_str() != expected.host_str()
            || url.port() != expected.port()
            || url.path() != expected.path()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(AuthError::CallbackMismatch);
        }
        let params = callback_parameters(raw_query)?;
        let state = params.get("state").ok_or(AuthError::CallbackMismatch)?;
        if !constant_eq(state.as_bytes(), self.state.as_bytes()) {
            return Err(AuthError::CallbackMismatch);
        }
        if params.contains_key("error") {
            if params.contains_key("code") {
                return Err(AuthError::CallbackMismatch);
            }
            return Err(AuthError::AuthorizationDenied);
        }
        if params.contains_key("error_description") || params.contains_key("error_uri") {
            return Err(AuthError::CallbackMismatch);
        }
        let code = params
            .get("code")
            .filter(|c| safe_atom(c, 8192))
            .ok_or(AuthError::CallbackMismatch)?;
        let client_id = match (self.issued_client, params.get("client_id")) {
            (Some(expected), Some(actual)) if actual.as_str() != expected => {
                return Err(AuthError::RegistrationMismatch);
            }
            (Some(expected), _) => expected,
            (None, Some(actual)) if safe_atom(actual, 512) && actual.as_str() != DYNAMIC_CLIENT => {
                actual.to_string()
            }
            _ => return Err(AuthError::RegistrationIncomplete),
        };
        Ok(PendingCode {
            code: Zeroizing::new(code.to_string()),
            nonce: self.nonce,
            verifier: self.verifier,
            redirect: self.redirect,
            registration: IssuedRegistration {
                client_id,
                host_id: self.host_id,
            },
            returning: self.returning,
            started: self.started,
        })
    }
}
fn callback_parameters(
    query: &str,
) -> Result<std::collections::BTreeMap<String, Zeroizing<String>>, AuthError> {
    let mut result = std::collections::BTreeMap::new();
    for pair in query.split('&') {
        let (key, value) = pair.split_once('=').ok_or(AuthError::CallbackMismatch)?;
        let key = decode_form(key)?;
        if !matches!(
            key.as_str(),
            "code" | "state" | "client_id" | "scope" | "error" | "error_description" | "error_uri"
        ) {
            return Err(AuthError::CallbackMismatch);
        }
        if result
            .insert(key, Zeroizing::new(decode_form(value)?))
            .is_some()
        {
            return Err(AuthError::CallbackMismatch);
        }
    }
    Ok(result)
}
fn decode_form(value: &str) -> Result<String, AuthError> {
    let mut decoded = Zeroizing::new(Vec::new());
    let mut chars = value.as_bytes().iter().copied();
    while let Some(c) = chars.next() {
        decoded.push(match c {
            b'+' => b' ',
            b'%' => {
                let hi = chars
                    .next()
                    .and_then(|v| (v as char).to_digit(16))
                    .ok_or(AuthError::CallbackMismatch)?;
                let lo = chars
                    .next()
                    .and_then(|v| (v as char).to_digit(16))
                    .ok_or(AuthError::CallbackMismatch)?;
                (hi * 16 + lo) as u8
            }
            c => c,
        });
    }
    String::from_utf8(decoded.to_vec()).map_err(|_| AuthError::CallbackMismatch)
}
fn safe_atom(value: &str, max: usize) -> bool {
    !value.is_empty()
        && value.len() <= max
        && !value.chars().any(|c| c.is_control() || c.is_whitespace())
}
fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |d, (a, b)| d | (a ^ b)) == 0
}
fn random_secret() -> Result<Zeroizing<String>, AuthError> {
    let mut bytes = Zeroizing::new([0u8; 32]);
    SystemRandom::new()
        .fill(bytes.as_mut())
        .map_err(|_| AuthError::RandomUnavailable)?;
    Ok(Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref())))
}
fn unix_now() -> Result<u64, AuthError> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .map_err(|_| AuthError::InvalidInput)
}
fn parse_scopes(value: &str) -> Result<BTreeSet<String>, AuthError> {
    if value.len() > 8192 || value.is_empty() {
        return Err(AuthError::InvalidResponse);
    }
    let mut scopes = BTreeSet::new();
    for scope in value.split(' ') {
        // RFC 6749 scope-token excludes quote/backslash and non-ASCII.
        if scope.is_empty()
            || !scope
                .bytes()
                .all(|b| b == 0x21 || (0x23..=0x5b).contains(&b) || (0x5d..=0x7e).contains(&b))
            || !scopes.insert(scope.to_owned())
        {
            return Err(AuthError::InvalidResponse);
        }
    }
    Ok(scopes)
}
#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    authorization_endpoint: String,
    token_endpoint: String,
    revocation_endpoint: String,
    jwks_uri: String,
    id_token_signing_alg_values_supported: Vec<String>,
    code_challenge_methods_supported: Vec<String>,
    token_endpoint_auth_methods_supported: Vec<String>,
}
impl Discovery {
    fn validate(&self) -> Result<(), AuthError> {
        if self.issuer != ISSUER
            || self.authorization_endpoint != "https://auth.openai.com/api/accounts/authorize"
            || self.token_endpoint != "https://auth.openai.com/api/accounts/oauth/token"
            || self.revocation_endpoint != "https://auth.openai.com/api/accounts/oauth/revoke"
            || self.jwks_uri != "https://auth.openai.com/.well-known/jwks.json"
            || !self
                .id_token_signing_alg_values_supported
                .iter()
                .any(|s| s == "RS256")
            || !self
                .code_challenge_methods_supported
                .iter()
                .any(|s| s == "S256")
            || !self
                .token_endpoint_auth_methods_supported
                .iter()
                .any(|s| s == "none")
        {
            return Err(AuthError::DestinationDenied);
        }
        Ok(())
    }
}
#[derive(Deserialize)]
struct RawToken {
    access_token: String,
    refresh_token: String,
    id_token: Option<String>,
    token_type: String,
    expires_in: u64,
    scope: Option<String>,
    earliest_refresh_at: Option<serde_json::Value>,
}
impl Drop for RawToken {
    fn drop(&mut self) {
        self.access_token.zeroize();
        self.refresh_token.zeroize();
        self.id_token.zeroize();
    }
}
fn parse_token(bytes: &[u8]) -> Result<RawToken, AuthError> {
    let token: RawToken = serde_json::from_slice(bytes).map_err(|_| AuthError::InvalidResponse)?;
    if !token.token_type.eq_ignore_ascii_case("Bearer")
        || token.expires_in == 0
        || !safe_atom(&token.access_token, 32768)
        || !safe_atom(&token.refresh_token, 32768)
        || token
            .id_token
            .as_ref()
            .is_some_and(|s| !safe_atom(s, 32768))
    {
        return Err(AuthError::InvalidResponse);
    }
    Ok(token)
}
