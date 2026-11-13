//! Fixed-origin Gmail protocol adapter. This library does not grant authority.
//!
//! The trusted caller must scope credentials to the current user, persist an
//! immutable mail snapshot, obtain that user's approval of every send, and
//! commit a durable dispatch record before calling `send_frozen`. A timeout or
//! lost response must be reconciled, never blindly retried. Google acceptance is
//! not proof of recipient delivery. No model-facing raw send tool belongs here.
mod mime;
#[cfg(test)]
mod tests;
pub(crate) mod transport;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
pub use mime::{FrozenMail, MailContent, ParsedMail, ReplyContext, parse_mail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;
use transport::{Body, Endpoint};
use zeroize::Zeroizing;

pub const READ_SCOPE: &str = "https://www.googleapis.com/auth/gmail.readonly";
pub const SEND_SCOPE: &str = "https://www.googleapis.com/auth/gmail.send";
pub const MINIMAL_SCOPES: [&str; 2] = [READ_SCOPE, SEND_SCOPE];

/// Intentionally neither Debug nor Serialize. Never put its contents in a tool
/// result or model context. `expose` is for the trusted token store/transport.
pub struct SecretString(Zeroizing<String>);
impl SecretString {
    pub fn new(value: String) -> Result<Self, GmailError> {
        let value = Zeroizing::new(value);
        if value.is_empty() || value.len() > 8192 || value.chars().any(char::is_control) {
            return Err(GmailError::InvalidInput);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for SecretString {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::new(value).map_err(|_| serde::de::Error::custom("invalid_secret_value"))
    }
}
/// The caller must persist token rotation atomically and retain an existing
/// refresh token if a successful response omits `refresh_token`.
pub struct TokenSet {
    pub access_token: SecretString,
    pub refresh_token: Option<SecretString>,
    pub expires_in: u64,
    pub scopes: Option<Vec<String>>,
}
/// Client secret is server configuration, not the user's shared LLM provider key.
pub struct OAuthClient {
    pub client_id: String,
    pub client_secret: SecretString,
    pub redirect_uri: String,
}
impl OAuthClient {
    /// Parse Google's downloaded web-client file. Endpoints in the file never
    /// select network destinations; transport uses the fixed Google endpoints.
    pub fn from_web_client_json(
        bytes: &str,
        redirect_uri: &str,
    ) -> Result<Self, GoogleClientFileError> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct File {
            web: Web,
        }
        #[derive(Deserialize)]
        struct Web {
            client_id: String,
            client_secret: SecretString,
            redirect_uris: Vec<String>,
        }
        if bytes.len() > 16384 {
            return Err(GoogleClientFileError::Invalid);
        }
        let file: File = serde_json::from_str(bytes).map_err(|_| GoogleClientFileError::Invalid)?;
        if file.web.redirect_uris.is_empty()
            || file.web.redirect_uris.len() > 32
            || file.web.redirect_uris.iter().any(|uri| uri.len() > 2048)
        {
            return Err(GoogleClientFileError::Invalid);
        }
        if !file.web.redirect_uris.iter().any(|uri| uri == redirect_uri) {
            return Err(GoogleClientFileError::RedirectMismatch);
        }
        let client = Self {
            client_id: file.web.client_id,
            client_secret: file.web.client_secret,
            redirect_uri: redirect_uri.into(),
        };
        client
            .validate()
            .map_err(|_| GoogleClientFileError::Invalid)?;
        Ok(client)
    }
    fn validate(&self) -> Result<(), GmailError> {
        validate_atom(&self.client_id, 1024)?;
        validate_redirect(&self.redirect_uri)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoogleClientFileError {
    Invalid,
    RedirectMismatch,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum GmailError {
    #[error("invalid_gmail_input")]
    InvalidInput,
    #[error("invalid_gmail_response")]
    InvalidResponse,
    #[error("gmail_destination_denied")]
    DestinationDenied,
    #[error("gmail_transport_failed")]
    Transport,
    #[error("gmail_timeout")]
    Timeout,
    #[error("gmail_cancelled")]
    Cancelled,
    #[error("gmail_limit_exceeded")]
    LimitExceeded,
    #[error("gmail_http_{0}")]
    Http(u16),
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MailboxProfile {
    pub email_address: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageRef {
    pub id: String,
    pub thread_id: String,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    #[serde(default)]
    pub messages: Vec<MessageRef>,
    pub next_page_token: Option<String>,
    pub result_size_estimate: Option<u64>,
}
pub struct MailMessage {
    pub id: String,
    pub thread_id: String,
    pub labels: Vec<String>,
    pub mail: ParsedMail,
}
pub struct MailThread {
    pub id: String,
    pub messages: Vec<MailMessage>,
    pub truncated: bool,
}
#[derive(Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SendReceipt {
    pub id: String,
    pub thread_id: String,
}

/// No production endpoint override. Cloning the client never clones a token.
#[derive(Clone)]
pub struct GmailClient {
    transport: transport::Transport,
}
impl Default for GmailClient {
    fn default() -> Self {
        Self::google()
    }
}
impl GmailClient {
    pub fn google() -> Self {
        Self {
            transport: transport::Transport::google(),
        }
    }
    /// CSRF state and PKCE verifier storage/one-use callback handling remain the
    /// application's responsibility. S256 only. This helper never starts OAuth.
    pub fn authorization_url(
        client_id: &str,
        redirect_uri: &str,
        state: &str,
        verifier: &str,
    ) -> Result<String, GmailError> {
        Self::authorization_url_scoped(client_id, redirect_uri, state, verifier, &MINIMAL_SCOPES)
    }
    /// Explicit separate consent; never unions an existing Gmail grant.
    pub fn authorization_url_scoped(
        client_id: &str,
        redirect_uri: &str,
        state: &str,
        verifier: &str,
        scopes: &[&str],
    ) -> Result<String, GmailError> {
        if scopes.is_empty()
            || scopes.len() > 8
            || scopes
                .iter()
                .any(|s| !s.starts_with("https://www.googleapis.com/auth/") || s.len() > 200)
        {
            return Err(GmailError::InvalidInput);
        }
        validate_atom(client_id, 1024)?;
        validate_redirect(redirect_uri)?;
        if state.len() < 32 || state.len() > 256 || !state.bytes().all(unreserved) {
            return Err(GmailError::InvalidInput);
        }
        let challenge = pkce_challenge(verifier)?;
        let mut url = url::Url::parse("https://accounts.google.com/o/oauth2/v2/auth")
            .map_err(|_| GmailError::InvalidInput)?;
        url.query_pairs_mut().extend_pairs([
            ("client_id", client_id),
            ("redirect_uri", redirect_uri),
            ("response_type", "code"),
            ("scope", &scopes.join(" ")),
            ("include_granted_scopes", "false"),
            ("state", state),
            ("access_type", "offline"),
            ("code_challenge", &challenge),
            ("code_challenge_method", "S256"),
            ("prompt", "consent select_account"),
        ]);
        Ok(url.into())
    }
    pub async fn exchange_code(
        &self,
        client: &OAuthClient,
        code: &SecretString,
        verifier: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<TokenSet, GmailError> {
        client.validate()?;
        pkce_challenge(verifier.expose())?;
        self.tokens(
            vec![
                ("grant_type", "authorization_code"),
                ("client_id", &client.client_id),
                ("client_secret", client.client_secret.expose()),
                ("redirect_uri", &client.redirect_uri),
                ("code", code.expose()),
                ("code_verifier", verifier.expose()),
            ],
            cancel,
        )
        .await
    }
    pub async fn refresh(
        &self,
        client: &OAuthClient,
        refresh_token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<TokenSet, GmailError> {
        client.validate()?;
        self.tokens(
            vec![
                ("grant_type", "refresh_token"),
                ("client_id", &client.client_id),
                ("client_secret", client.client_secret.expose()),
                ("refresh_token", refresh_token.expose()),
            ],
            cancel,
        )
        .await
    }
    async fn tokens(
        &self,
        fields: Vec<(&str, &str)>,
        cancel: &CancellationToken,
    ) -> Result<TokenSet, GmailError> {
        #[derive(Deserialize)]
        struct Wire {
            access_token: SecretString,
            refresh_token: Option<SecretString>,
            expires_in: u64,
            token_type: String,
            scope: Option<String>,
        }
        let bytes = self
            .transport
            .request(
                Endpoint::Token,
                reqwest::Method::POST,
                "",
                None,
                Body::Form(fields),
                cancel,
            )
            .await?;
        let wire: Wire = serde_json::from_slice(&bytes).map_err(|_| GmailError::InvalidResponse)?;
        if !wire.token_type.eq_ignore_ascii_case("Bearer")
            || wire.expires_in == 0
            || wire.expires_in > 31_536_000
        {
            return Err(GmailError::InvalidResponse);
        }
        let scopes = wire.scope.map(|scope| {
            scope
                .split_ascii_whitespace()
                .map(str::to_owned)
                .collect::<Vec<_>>()
        });
        if scopes
            .as_ref()
            .is_some_and(|s| s.len() > 64 || s.iter().any(|s| s.len() > 512))
        {
            return Err(GmailError::InvalidResponse);
        }
        Ok(TokenSet {
            access_token: wire.access_token,
            refresh_token: wire.refresh_token,
            expires_in: wire.expires_in,
            scopes,
        })
    }
    pub async fn profile(
        &self,
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<MailboxProfile, GmailError> {
        let bytes = self
            .get("profile?fields=emailAddress", token, cancel)
            .await?;
        let p: MailboxProfile = decode(&bytes)?;
        mime::validate_address(&p.email_address).map_err(|_| GmailError::InvalidResponse)?;
        Ok(p)
    }
    pub async fn list_messages(
        &self,
        token: &SecretString,
        query: &str,
        page_token: Option<&str>,
        max_results: u16,
        cancel: &CancellationToken,
    ) -> Result<MessagePage, GmailError> {
        if query.len() > 2048
            || query.chars().any(char::is_control)
            || !(1..=50).contains(&max_results)
        {
            return Err(GmailError::InvalidInput);
        }
        let query_string = {
            let mut serializer = url::form_urlencoded::Serializer::new(String::new());
            serializer
                .append_pair("q", query)
                .append_pair("maxResults", &max_results.to_string());
            if let Some(p) = page_token {
                validate_atom(p, 2048)?;
                serializer.append_pair("pageToken", p);
            }
            serializer.finish()
        };
        let bytes = self
            .get(&format!("messages?{query_string}"), token, cancel)
            .await?;
        let p: MessagePage = decode(&bytes)?;
        if p.messages.len() > usize::from(max_results) {
            return Err(GmailError::InvalidResponse);
        }
        for m in &p.messages {
            validate_id(&m.id)?;
            validate_id(&m.thread_id)?;
        }
        if let Some(next) = &p.next_page_token {
            validate_atom(next, 2048).map_err(|_| GmailError::InvalidResponse)?;
        }
        Ok(p)
    }
    pub async fn get_message(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MailMessage, GmailError> {
        validate_id(id)?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: String,
            thread_id: String,
            #[serde(default)]
            label_ids: Vec<String>,
            raw: String,
        }
        let bytes = self
            .get(
                &format!("messages/{id}?format=raw&fields=id,threadId,labelIds,raw"),
                token,
                cancel,
            )
            .await?;
        let w: Wire = decode(&bytes)?;
        if w.id != id || w.raw.len() > mime::MAX_MIME_BYTES * 4 / 3 + 8 || w.label_ids.len() > 128 {
            return Err(GmailError::InvalidResponse);
        }
        validate_id(&w.thread_id).map_err(|_| GmailError::InvalidResponse)?;
        for label in &w.label_ids {
            validate_atom(label, 256).map_err(|_| GmailError::InvalidResponse)?;
        }
        let raw = URL_SAFE_NO_PAD
            .decode(w.raw.trim_end_matches('='))
            .map_err(|_| GmailError::InvalidResponse)?;
        let mail = parse_mail(&raw)?;
        Ok(MailMessage {
            id: w.id,
            thread_id: w.thread_id,
            labels: w.label_ids,
            mail,
        })
    }
    /// Threads have no `raw` format. Fetch bounded IDs, then use Gmail's raw
    /// message endpoint and the maintained MIME parser for each selected item.
    pub async fn get_thread(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MailThread, GmailError> {
        validate_id(id)?;
        #[derive(Deserialize)]
        struct Wire {
            id: String,
            #[serde(default)]
            messages: Vec<Id>,
        }
        #[derive(Deserialize)]
        struct Id {
            id: String,
        }
        let bytes = self
            .get(
                &format!("threads/{id}?format=minimal&fields=id,messages(id)"),
                token,
                cancel,
            )
            .await?;
        let w: Wire = decode(&bytes)?;
        if w.id != id || w.messages.len() > 4096 {
            return Err(GmailError::InvalidResponse);
        }
        let truncated = w.messages.len() > 20;
        let mut messages = Vec::new();
        for item in w.messages.iter().rev().take(20).rev() {
            let message = self.get_message(token, &item.id, cancel).await?;
            if message.thread_id != id {
                return Err(GmailError::InvalidResponse);
            }
            messages.push(message);
        }
        Ok(MailThread {
            id: w.id,
            messages,
            truncated,
        })
    }
    async fn get(
        &self,
        path: &str,
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<Zeroizing<Vec<u8>>, GmailError> {
        self.transport
            .request(
                Endpoint::Gmail,
                reqwest::Method::GET,
                path,
                Some(token),
                Body::Empty,
                cancel,
            )
            .await
    }
    /// TRUSTED CALLER ONLY: pass the exact persisted and user-approved snapshot
    /// after durable dispatch commitment. There is deliberately no retry here.
    /// Any response loss after request dispatch requires status reconciliation.
    pub async fn send_frozen(
        &self,
        token: &SecretString,
        mail: &FrozenMail,
        cancel: &CancellationToken,
    ) -> Result<SendReceipt, GmailError> {
        let mut body = serde_json::json!({"raw":URL_SAFE_NO_PAD.encode(mail.raw_mime())});
        if let Some(thread) = mail.thread_id() {
            body["threadId"] = thread.into();
        }
        let bytes = self
            .transport
            .request(
                Endpoint::Gmail,
                reqwest::Method::POST,
                "messages/send",
                Some(token),
                Body::Json(body),
                cancel,
            )
            .await?;
        let receipt: SendReceipt = decode(&bytes)?;
        validate_id(&receipt.id).map_err(|_| GmailError::InvalidResponse)?;
        validate_id(&receipt.thread_id).map_err(|_| GmailError::InvalidResponse)?;
        Ok(receipt)
    }
}
pub fn pkce_challenge(verifier: &str) -> Result<String, GmailError> {
    if !(43..=128).contains(&verifier.len()) || !verifier.bytes().all(unreserved) {
        return Err(GmailError::InvalidInput);
    }
    Ok(URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())))
}
fn unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b"-._~".contains(&b)
}
fn validate_atom(s: &str, max: usize) -> Result<(), GmailError> {
    if s.is_empty() || s.len() > max || s.chars().any(char::is_control) {
        Err(GmailError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_id(s: &str) -> Result<(), GmailError> {
    if s.is_empty()
        || s.len() > 256
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        Err(GmailError::InvalidInput)
    } else {
        Ok(())
    }
}
fn validate_redirect(s: &str) -> Result<(), GmailError> {
    if s.len() > 2048 {
        return Err(GmailError::InvalidInput);
    }
    let u = url::Url::parse(s).map_err(|_| GmailError::InvalidInput)?;
    let loopback = matches!(
        u.host_str(),
        Some("localhost") | Some("127.0.0.1") | Some("[::1]")
    );
    if !(u.scheme() == "https" || u.scheme() == "http" && loopback)
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err(GmailError::InvalidInput);
    }
    Ok(())
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, GmailError> {
    serde_json::from_slice(bytes).map_err(|_| GmailError::InvalidResponse)
}
