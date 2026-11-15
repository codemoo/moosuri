//! Fixed Google refresh/identity checks, INBOX counts and separately selected preview.
//! Only typed numeric counts or bounded untrusted mail fields cross this adapter.
use crate::*;
pub mod model_mail;
use moosuri_vault_core::{ItemFields, ItemKind, SecretText, UnlockedArchive};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zeroize::Zeroizing;
pub(crate) const LOCAL_POLICY: &[u8] = b"moosuri/local-google-inbox/v1\0https://oauth2.googleapis.com/token\0https://openidconnect.googleapis.com/v1/userinfo\0https://gmail.googleapis.com/gmail/v1/users/me/profile\0https://gmail.googleapis.com/gmail/v1/users/me/labels/INBOX\0openid email https://www.googleapis.com/auth/gmail.readonly";
pub(crate) const PREVIEW_POLICY: &[u8] = b"moosuri/local-google-inbox-preview/v1\0https://oauth2.googleapis.com/token\0https://openidconnect.googleapis.com/v1/userinfo\0https://gmail.googleapis.com/gmail/v1/users/me/profile\0https://gmail.googleapis.com/gmail/v1/users/me/messages?labelIds=INBOX&maxResults=5&includeSpamTrash=false\0metadata:Subject,From,Date;snippet\0openid email https://www.googleapis.com/auth/gmail.readonly";
pub(crate) const SEND_POLICY:&[u8]=b"moosuri/native-approved-google-send/v1\0openid email gmail.readonly gmail.send\0POST:messages/send;GET:original-Sent-equality-only";
pub fn send_digest() -> [u8; 32] {
    auth::hash(b"moosuri/google-approved-mail/v1")
}
pub(crate) const BODY_POLICY: &[u8] = b"moosuri/local-google-inbox-body/v1\0selected-preview-only;GET:messages/{local-id}?format=full;INBOX;utf8-text/plain;4096bytes;no-attachments";
pub fn body_digest() -> [u8; 32] {
    auth::hash(&Resource::google_body(uuid::Uuid::nil()).bytes())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BodySelection {
    selection: uuid::Uuid,
    id: String,
    preview_binding: Binding,
    item: uuid::Uuid,
    vault: uuid::Uuid,
    identity: GoogleIdentity,
    expires_at: i64,
}
pub fn preview_digest() -> [u8; 32] {
    auth::hash(&Resource::google_preview().bytes())
}
pub fn resource_digest() -> [u8; 32] {
    auth::hash(b"moosuri/google-inbox-counts/v1")
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleIdentity {
    pub subject: String,
    pub email: String,
}
impl GoogleIdentity {
    pub fn valid(&self) -> bool {
        !self.subject.is_empty()
            && self.subject.len() <= 255
            && self.subject.bytes().all(|b| b.is_ascii_graphic())
            && self.email.len() <= 254
            && self.email.contains('@')
            && self.email.bytes().all(|b| b.is_ascii_graphic())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    version: u8,
    client_id: SecretText,
    #[serde(default)]
    client_secret: Option<SecretText>,
    refresh_token: SecretText,
}
impl Credential {
    fn parse(secret: &str) -> Result<Self> {
        if secret.len() > 16384 {
            return Err(Error::Policy);
        }
        let c: Self = serde_json::from_str(secret).map_err(|_| Error::Policy)?;
        let valid = |s: &str, max: usize| {
            !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_graphic())
        };
        if c.version != 1
            || !valid(c.client_id.expose(), 4096)
            || !valid(c.refresh_token.expose(), 8192)
            || c.client_secret
                .as_ref()
                .is_some_and(|s| !valid(s.expose(), 4096))
        {
            return Err(Error::Policy);
        }
        Ok(c)
    }
}
pub(crate) fn fields(fields: &ItemFields) -> Result<()> {
    if fields.kind != ItemKind::ApiKey
        || fields.url.expose() != "https://oauth2.googleapis.com"
        || !fields.totp.expose().is_empty()
        || !fields.expires_at.expose().is_empty()
    {
        return Err(Error::Policy);
    }
    Credential::parse(fields.secret.expose())?;
    Ok(())
}
#[derive(Deserialize)]
struct Token {
    access_token: SecretText,
    token_type: SecretText,
    scope: SecretText,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<SecretText>,
}
#[derive(Deserialize)]
struct Identity {
    sub: SecretText,
    email: SecretText,
    email_verified: bool,
}
#[derive(Deserialize)]
struct Profile {
    #[serde(rename = "emailAddress")]
    email: SecretText,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inbox {
    id: SecretText,
    messages_total: u32,
    messages_unread: u32,
    threads_total: u32,
    threads_unread: u32,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MimePart {
    mime_type: SecretText,
    #[serde(default)]
    filename: Option<SecretText>,
    #[serde(default)]
    headers: Vec<MimeHeader>,
    #[serde(default)]
    body: Option<MimeBody>,
    #[serde(default)]
    parts: Vec<MimePart>,
}
#[derive(Deserialize)]
struct MimeHeader {
    name: SecretText,
    value: SecretText,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct MimeBody {
    #[serde(default)]
    data: Option<SecretText>,
    #[serde(default)]
    attachment_id: Option<SecretText>,
    size: u64,
}
impl MimePart {
    fn plain<'a>(
        &'a self,
        depth: usize,
        visited: &mut usize,
        out: &mut Vec<&'a SecretText>,
    ) -> Result<()> {
        *visited += 1;
        if depth > 4 || *visited > 32 || self.headers.len() > 64 {
            return Err(Error::ProviderContent);
        }
        if self
            .filename
            .as_ref()
            .is_some_and(|s| !s.expose().is_empty())
        {
            return Ok(());
        }
        let mut content_type = None;
        let mut disposition = None;
        for h in &self.headers {
            if h.name.expose().eq_ignore_ascii_case("content-type")
                && content_type.replace(h.value.expose()).is_some()
            {
                return Err(Error::ProviderContent);
            }
            if h.name.expose().eq_ignore_ascii_case("content-disposition")
                && disposition.replace(h.value.expose()).is_some()
            {
                return Err(Error::ProviderContent);
            }
        }
        if disposition.is_some_and(|s| {
            !s.split(';')
                .next()
                .unwrap_or("")
                .trim()
                .eq_ignore_ascii_case("inline")
        }) {
            return Ok(());
        }
        if self.mime_type.expose().eq_ignore_ascii_case("text/plain") {
            if !self.parts.is_empty() {
                return Err(Error::ProviderContent);
            }
            if let Some(ct) = content_type {
                let mut fields = ct.split(';');
                if !fields
                    .next()
                    .unwrap_or("")
                    .trim()
                    .eq_ignore_ascii_case("text/plain")
                {
                    return Err(Error::ProviderContent);
                }
                for parameter in fields {
                    let (name, value) = parameter
                        .trim()
                        .split_once('=')
                        .ok_or(Error::ProviderContent)?;
                    if !name.trim().eq_ignore_ascii_case("charset")
                        || !matches!(
                            value.trim().trim_matches('"').to_ascii_lowercase().as_str(),
                            "utf-8" | "us-ascii"
                        )
                    {
                        return Err(Error::ProviderContent);
                    }
                }
            }
            let body = self.body.as_ref().ok_or(Error::ProviderContent)?;
            if body.attachment_id.is_some() {
                return Ok(());
            }
            if body.size > 16384 {
                return Err(Error::ProviderContent);
            }
            let data = body.data.as_ref().ok_or(Error::ProviderContent)?;
            if decode_mail_base64(data.expose())?.len() as u64 != body.size {
                return Err(Error::ProviderContent);
            }
            out.push(data);
        } else if matches!(
            self.mime_type.expose().to_ascii_lowercase().as_str(),
            "multipart/alternative" | "multipart/mixed"
        ) {
            for part in &self.parts {
                part.plain(depth + 1, visited, out)?;
            }
        }
        Ok(())
    }
}
// Strict bounded URL-safe base64 with optional canonical padding. No new dependency.
fn decode_mail_base64(s: &str) -> Result<Zeroizing<Vec<u8>>> {
    if s.len() > 21848 {
        return Err(Error::ProviderContent);
    }
    let raw = s.trim_end_matches('=');
    let padding = s.len() - raw.len();
    if padding > 2
        || raw.len() % 4 == 1
        || (padding != 0 && (s.len() % 4 != 0 || padding != (4 - raw.len() % 4) % 4))
    {
        return Err(Error::ProviderContent);
    }
    let mut out = Zeroizing::new(Vec::new());
    let mut acc = 0u32;
    let mut bits = 0;
    for b in raw.bytes() {
        let value = match b {
            b'A'..=b'Z' => b - b'A',
            b'a'..=b'z' => b - b'a' + 26,
            b'0'..=b'9' => b - b'0' + 52,
            b'-' => 62,
            b'_' => 63,
            _ => return Err(Error::ProviderContent),
        };
        acc = (acc << 6 | value as u32) & 0xffffff;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if bits > 0 && acc & ((1 << bits) - 1) != 0 || out.len() > 16384 {
        return Err(Error::ProviderContent);
    }
    Ok(out)
}
pub struct GoogleAdapter {
    client: reqwest::Client,
    token: String,
    userinfo: String,
    profile: String,
    inbox: String,
    messages: String,
    mail: moosuri_connectors::gmail::GmailClient,
}
impl GoogleAdapter {
    pub fn new() -> Result<Self> {
        Self::at(
            "https://oauth2.googleapis.com/token",
            "https://openidconnect.googleapis.com/v1/userinfo",
            "https://gmail.googleapis.com/gmail/v1/users/me/profile",
            "https://gmail.googleapis.com/gmail/v1/users/me/labels/INBOX",
        )
    }
    fn at(token: &str, userinfo: &str, profile: &str, inbox: &str) -> Result<Self> {
        let mail = moosuri_connectors::gmail::GmailClient::google();
        #[cfg(any(test, feature = "test-fixtures"))]
        let mail = if let Some(base) = token
            .strip_suffix("/token")
            .filter(|s| s.starts_with("http://"))
        {
            moosuri_connectors::gmail::GmailClient::synthetic_loopback(
                base.strip_prefix("http://")
                    .unwrap()
                    .parse()
                    .map_err(|_| Error::Policy)?,
            )
            .map_err(|_| Error::Policy)?
        } else {
            mail
        };
        Ok(Self {
            mail,
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| Error::PrivateState)?,
            token: token.into(),
            userinfo: userinfo.into(),
            profile: profile.into(),
            inbox: inbox.into(),
            messages: format!(
                "{}/messages",
                profile.strip_suffix("/profile").ok_or(Error::Policy)?
            ),
        })
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn synthetic_loopback(address: std::net::SocketAddr) -> Result<Self> {
        if !address.ip().is_loopback() {
            return Err(Error::Policy);
        }
        let base = format!("http://{address}");
        Self::at(
            &format!("{base}/token"),
            &format!("{base}/userinfo"),
            &format!("{base}/profile"),
            &format!("{base}/inbox"),
        )
    }
    async fn response<T: serde::de::DeserializeOwned>(
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        Self::decode_response(request.send().await.map_err(|_| Error::Unknown)?).await
    }
    async fn decode_response<T: serde::de::DeserializeOwned>(
        response: reqwest::Response,
    ) -> Result<T> {
        Self::decode_body(response, false).await
    }
    async fn preview_response<T: serde::de::DeserializeOwned>(
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        Self::decode_body(request.send().await.map_err(|_| Error::Unknown)?, true).await
    }
    async fn decode_body<T: serde::de::DeserializeOwned>(
        mut response: reqwest::Response,
        content: bool,
    ) -> Result<T> {
        if !response.status().is_success() {
            if content {
                // Mail/provider failures (including quota-style403) do not
                // invalidate the OAuth grant verified before this read.
                return Err(match response.status().as_u16() {
                    401 => Error::Policy,
                    400 | 403 => Error::ProviderContent,
                    _ => Error::Unknown,
                });
            }
            return Err(if matches!(response.status().as_u16(), 400 | 401 | 403) {
                Error::Policy
            } else {
                Error::Unknown
            });
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unknown)? {
            if bytes.len() + chunk.len() > 32768 {
                return Err(Error::Unknown);
            }
            bytes.extend_from_slice(&chunk)
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            if content {
                Error::ProviderContent
            } else {
                Error::Policy
            }
        })
    }
    async fn preview(
        &self,
        credential: &Credential,
        token: &Token,
        secret: &str,
        selections: &mut Vec<(uuid::Uuid, String)>,
    ) -> Result<Projection> {
        #[derive(Deserialize)]
        struct Listed {
            id: SecretText,
        }
        #[derive(Deserialize)]
        struct List {
            #[serde(default)]
            messages: Vec<Listed>,
            #[serde(rename = "nextPageToken")]
            next: Option<SecretText>,
        }
        #[derive(Deserialize)]
        struct Header {
            name: SecretText,
            value: SecretText,
        }
        #[derive(Deserialize)]
        struct Payload {
            #[serde(default)]
            headers: Vec<Header>,
        }
        #[derive(Deserialize)]
        struct Message {
            id: SecretText,
            #[serde(rename = "labelIds")]
            labels: Vec<SecretText>,
            snippet: SecretText,
            payload: Payload,
        }
        let list:List=Self::preview_response(self.client.get(format!("{}?labelIds=INBOX&maxResults=5&includeSpamTrash=false&fields=messages%28id%29%2CnextPageToken",self.messages)).bearer_auth(token.access_token.expose())).await?;
        if list.messages.len() > 5 {
            return Err(Error::ProviderContent);
        }
        let mut seen = std::collections::HashSet::new();
        let mut messages = Vec::new();
        let mut unavailable = 0;
        let contains_secret = |value: &str| {
            [
                Some(secret),
                Some(credential.refresh_token.expose()),
                Some(credential.client_id.expose()),
                credential.client_secret.as_ref().map(|s| s.expose()),
                Some(token.access_token.expose()),
            ]
            .into_iter()
            .flatten()
            .any(|s| !s.is_empty() && value.contains(s))
        };
        for item in list.messages {
            let id = item.id.expose();
            if id.is_empty()
                || id.len() > 128
                || contains_secret(id)
                || !id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
                || !seen.insert(id.to_owned())
            {
                return Err(Error::ProviderContent);
            }
            let response=self.client.get(format!("{}/{id}?format=metadata&metadataHeaders=Subject&metadataHeaders=From&metadataHeaders=Date&fields=id%2ClabelIds%2Csnippet%2Cpayload%28headers%29",self.messages)).bearer_auth(token.access_token.expose())
                .send().await.map_err(|_|Error::Unknown)?;
            // A listed item can disappear or leave INBOX before this bounded read.
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                unavailable += 1;
                continue;
            }
            let message: Message = Self::decode_body(response, true).await?;
            if message.id.expose() != id {
                return Err(Error::ProviderContent);
            }
            if !message.labels.iter().any(|l| l.expose() == "INBOX") {
                unavailable += 1;
                continue;
            }
            let truncated = std::cell::Cell::new(false);
            let safe = |value: &str, max: usize| -> Result<String> {
                // Check the entire original value BEFORE normalization/bounds. A
                // malicious provider must not echo credential material via mail text.
                if contains_secret(value) {
                    return Err(Error::ProviderContent);
                }
                let value = value.replace(['\r', '\n', '\t'], " ");
                if value.chars().any(char::is_control) {
                    return Err(Error::ProviderContent);
                }
                if value.len() > max {
                    let mut end = max;
                    while !value.is_char_boundary(end) {
                        end -= 1;
                    }
                    truncated.set(true);
                    return Ok(value[..end].to_owned());
                }
                Ok(value)
            };
            let header = |name: &str, max| -> Result<String> {
                let values = message
                    .payload
                    .headers
                    .iter()
                    .filter(|h| h.name.expose().eq_ignore_ascii_case(name))
                    .collect::<Vec<_>>();
                if values.len() > 1 {
                    return Err(Error::ProviderContent);
                }
                safe(values.first().map(|h| h.value.expose()).unwrap_or(""), max)
            };
            messages.push(moosuri_vault_protocol::broker::GoogleMessagePreview {
                selection: Some({
                    let selection = uuid::Uuid::new_v4();
                    selections.push((selection, id.to_owned()));
                    selection
                }),
                subject: header("subject", 256)?,
                from: header("from", 256)?,
                date: header("date", 128)?,
                snippet: safe(message.snippet.expose(), 512)?,
                truncated: truncated.get(),
            });
        }
        Ok(Projection::GoogleInboxPreview {
            messages,
            unavailable,
            more_available: list.next.is_some_and(|s| !s.expose().is_empty()),
        })
    }
    async fn body(
        &self,
        id: &str,
        credential: &Credential,
        token: &Token,
        secret: &str,
    ) -> Result<Projection> {
        #[derive(Deserialize)]
        struct Message {
            id: SecretText,
            #[serde(rename = "labelIds")]
            labels: Vec<SecretText>,
            payload: MimePart,
        }
        let response = self
            .client
            .get(format!(
                "{}/{id}?format=full&fields=id%2ClabelIds%2Cpayload",
                self.messages
            ))
            .bearer_auth(token.access_token.expose())
            .send()
            .await
            .map_err(|_| Error::Unknown)?;
        let unavailable = || Projection::GoogleInboxBody {
            text: String::new(),
            truncated: false,
            unavailable: true,
        };
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            return Ok(unavailable());
        }
        let message: Message = Self::decode_body(response, true).await?;
        if message.id.expose() != id {
            return Err(Error::ProviderContent);
        }
        if !message.labels.iter().any(|l| l.expose() == "INBOX") {
            return Ok(unavailable());
        }
        let mut visited = 0;
        let mut plain = Vec::new();
        message.payload.plain(0, &mut visited, &mut plain)?;
        // Exactly one inline plaintext part. No HTML conversion or attachment fetch.
        if plain.len() != 1 {
            return Err(Error::ProviderContent);
        }
        let bytes = decode_mail_base64(plain[0].expose())?;
        let text = std::str::from_utf8(&bytes).map_err(|_| Error::ProviderContent)?;
        if [
            Some(secret),
            Some(credential.refresh_token.expose()),
            Some(credential.client_id.expose()),
            credential.client_secret.as_ref().map(|s| s.expose()),
            Some(token.access_token.expose()),
        ]
        .into_iter()
        .flatten()
        .any(|s| !s.is_empty() && text.contains(s))
            || text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return Err(Error::ProviderContent);
        }
        let mut end = text.len().min(4096);
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok(Projection::GoogleInboxBody {
            text: text[..end].to_owned(),
            truncated: end != text.len(),
            unavailable: false,
        })
    }
    async fn authorize(
        &self,
        identity: &GoogleIdentity,
        secret: &str,
        send: bool,
    ) -> Result<(Credential, Token)> {
        self.authorize_for(identity, secret, send, false).await
    }
    async fn authorize_model(
        &self,
        identity: &GoogleIdentity,
        secret: &str,
    ) -> Result<(Credential, Token)> {
        self.authorize_for(identity, secret, false, true).await
    }
    async fn authorize_for(
        &self,
        identity: &GoogleIdentity,
        secret: &str,
        send: bool,
        model: bool,
    ) -> Result<(Credential, Token)> {
        if !identity.valid() {
            return Err(Error::Policy);
        }
        let credential = Credential::parse(secret)?;
        let form = Zeroizing::new({
            let mut form = url::form_urlencoded::Serializer::new(String::new());
            form.append_pair("grant_type", "refresh_token")
                .append_pair("client_id", credential.client_id.expose())
                .append_pair("refresh_token", credential.refresh_token.expose());
            if let Some(client_secret) = &credential.client_secret {
                form.append_pair("client_secret", client_secret.expose());
            }
            form.finish()
        });
        let token: Token = Self::response(
            self.client
                .post(&self.token)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form.as_bytes().to_vec()),
        )
        .await?;
        // Scope comes from the actual HTTPS refresh response, never the local claim.
        let scopes: std::collections::HashSet<_> =
            token.scope.expose().split_ascii_whitespace().collect();
        if token.token_type.expose() != "Bearer"
            || token.expires_in == 0
            || token.expires_in > 86400
            || token.access_token.expose().is_empty()
            || token.access_token.expose().len() > 16384
            || !token
                .access_token
                .expose()
                .bytes()
                .all(|b| b.is_ascii_graphic())
            || scopes.len() != if send { 4 } else { 3 }
            || (send && !scopes.contains("https://www.googleapis.com/auth/gmail.send"))
            || !scopes.contains("openid")
            || !scopes.contains("https://www.googleapis.com/auth/gmail.readonly")
            || !(scopes.contains("email")
                || scopes.contains("https://www.googleapis.com/auth/userinfo.email"))
            || token
                .refresh_token
                .as_ref()
                .is_some_and(|s| s.expose() != credential.refresh_token.expose())
        {
            return Err(if model {
                Error::GoogleScope
            } else {
                Error::Policy
            });
        }
        // A rotated refresh token is never silently discarded or written in plaintext.
        let verified: Identity = Self::response(
            self.client
                .get(&self.userinfo)
                .bearer_auth(token.access_token.expose()),
        )
        .await?;
        if !verified.email_verified
            || verified.sub.expose() != identity.subject
            || verified.email.expose() != identity.email
        {
            return Err(if model {
                Error::GoogleAccount
            } else {
                Error::Policy
            });
        }
        let profile: Profile = Self::response(
            self.client
                .get(&self.profile)
                .bearer_auth(token.access_token.expose()),
        )
        .await?;
        if profile.email.expose() != verified.email.expose() {
            return Err(if model {
                Error::GoogleAccount
            } else {
                Error::Policy
            });
        }
        Ok((credential, token))
    }
    async fn read(
        &self,
        identity: &GoogleIdentity,
        secret: &str,
        preview: bool,
    ) -> Result<Projection> {
        self.read_for(identity, secret, preview, None, &mut Vec::new())
            .await
    }
    async fn read_for(
        &self,
        identity: &GoogleIdentity,
        secret: &str,
        preview: bool,
        selected_id: Option<&str>,
        selections: &mut Vec<(uuid::Uuid, String)>,
    ) -> Result<Projection> {
        let (credential, token) = self.authorize(identity, secret, false).await?;
        if let Some(id) = selected_id {
            return self.body(id, &credential, &token, secret).await;
        }
        if preview {
            return self.preview(&credential, &token, secret, selections).await;
        }
        let inbox: Inbox = Self::response(
            self.client
                .get(&self.inbox)
                .bearer_auth(token.access_token.expose()),
        )
        .await?;
        if inbox.id.expose() != "INBOX"
            || inbox.messages_unread > inbox.messages_total
            || inbox.threads_unread > inbox.threads_total
            || [
                inbox.messages_total,
                inbox.messages_unread,
                inbox.threads_total,
                inbox.threads_unread,
            ]
            .iter()
            .any(|v| *v > i32::MAX as u32)
        {
            return Err(Error::Policy);
        }
        Ok(Projection::GoogleInbox {
            messages_total: inbox.messages_total,
            messages_unread: inbox.messages_unread,
            threads_total: inbox.threads_total,
            threads_unread: inbox.threads_unread,
        })
    }
}
impl Engine {
    pub fn archive_google_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
        expires: i64,
    ) -> Result<Binding> {
        self.archive_google_grant_for(archive, item, identity, expires, 0)
    }
    pub fn archive_google_preview_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
        expires: i64,
    ) -> Result<Binding> {
        self.archive_google_grant_for(archive, item, identity, expires, 1)
    }
    pub fn archive_google_body_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
        expires: i64,
    ) -> Result<Binding> {
        self.archive_google_grant_for(archive, item, identity, expires, 2)
    }
    pub fn archive_google_send_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
        expires: i64,
    ) -> Result<Binding> {
        self.archive_google_grant_for(archive, item, identity, expires, 3)
    }
    pub fn archive_google_model_read_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
        expires: i64,
        background: bool,
    ) -> Result<Binding> {
        self.archive_google_grant_for(
            archive,
            item,
            identity,
            expires,
            if background { 5 } else { 4 },
        )
    }
    fn archive_google_grant_for(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
        expires: i64,
        mode: u8,
    ) -> Result<Binding> {
        if archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let encrypted = archive
            .items()
            .iter()
            .find(|i| i.id == item)
            .ok_or(Error::Policy)?;
        let epoch = encrypted.revision;
        let key = archive.header().generation;
        fields(&archive.open(item).map_err(|_| Error::Policy)?)?;
        if let Some(at) = self.grants().iter().position(|g| {
            g.google().is_some()
                && g.google_preview() == (mode == 1)
                && g.google_body() == (mode == 2)
                && g.google_send() == (mode == 3)
                && g.google_model_read() == (mode >= 4)
                && g.google_model_background() == (mode == 5)
                && g.binding.expires_at > current_time().unwrap_or(i64::MAX)
        }) {
            // No retained invocation may cross a credential-version change.
            // The caller reconciles original receipts/commitments before reconnect.
            let binding = &self.state.grants[at].binding;
            if (binding.item_epoch != epoch || binding.key_epoch != key)
                && self.retained().iter().any(|r| r.binding.id == binding.id)
            {
                return Err(Error::Policy);
            }
            let g = &mut self.state.grants[at];
            if g.google() != Some(&identity)
                || g.live().is_some()
                || g.vault() != Some(archive.header().binding.vault_id)
                || g.item() != item
            {
                return Err(Error::Policy);
            }
            g.refresh_archive_google(epoch, key)?;
            g.resume_google();
            let binding = g.binding.clone();
            self.save()?;
            return Ok(binding);
        }
        if mode >= 4 {
            return self.select(policy::LocalGrant::select_google_model_read(
                self.device().clone(),
                item,
                archive.header().binding.vault_id,
                identity,
                epoch,
                key,
                expires,
                mode == 5,
            )?);
        }
        self.select((if mode == 3 {
            policy::LocalGrant::select_google_send
        } else if mode == 2 {
            policy::LocalGrant::select_google_body
        } else if mode == 1 {
            policy::LocalGrant::select_google_preview
        } else {
            policy::LocalGrant::select_google
        })(
            self.device().clone(),
            item,
            archive.header().binding.vault_id,
            identity,
            epoch,
            key,
            expires,
        )?)
    }
    pub fn prepare_google(
        &mut self,
        archive: &UnlockedArchive,
        request: Request,
        now: i64,
    ) -> Result<()> {
        if !matches!(
            request.operation,
            Operation::GoogleInboxRead
                | Operation::GoogleInboxPreview
                | Operation::GoogleInboxBody { .. }
                | Operation::GoogleMailModelRead { .. }
                | Operation::GoogleMailSend { .. }
                | Operation::GoogleMailLookup { .. }
        ) || archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?;
        if grant.live().is_some() || grant.vault() != Some(archive.header().binding.vault_id) {
            return Err(Error::Policy);
        }
        let item = archive
            .items()
            .iter()
            .find(|i| i.id == grant.item())
            .ok_or(Error::Policy)?;
        self.prepare(request, now, item.revision, archive.header().generation)
    }
    pub async fn execute_google(
        &mut self,
        archive: &mut UnlockedArchive,
        invocation: &Invocation,
        adapter: &GoogleAdapter,
    ) -> Result<SignedReceipt> {
        let request = &self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request;
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        if grant.live().is_some()
            || grant.vault() != Some(archive.header().binding.vault_id)
            || archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let epoch = archive
            .items()
            .iter()
            .find(|i| i.id == grant.item())
            .ok_or(Error::Policy)?
            .revision;
        let f = archive.open(grant.item()).map_err(|_| Error::Policy)?;
        fields(&f)?;
        self.execute_google_secret(
            invocation,
            adapter,
            f.secret.expose(),
            epoch,
            archive.header().generation,
        )
        .await
    }
    pub(crate) async fn execute_google_secret(
        &mut self,
        invocation: &Invocation,
        adapter: &GoogleAdapter,
        secret: &str,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<SignedReceipt> {
        let request = &self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request;
        if !matches!(
            request.operation,
            Operation::GoogleInboxRead
                | Operation::GoogleInboxPreview
                | Operation::GoogleInboxBody { .. }
                | Operation::GoogleMailModelRead { .. }
                | Operation::GoogleMailSend { .. }
                | Operation::GoogleMailLookup { .. }
        ) {
            return Err(Error::Policy);
        }
        if matches!(request.operation, Operation::GoogleMailModelRead { .. }) {
            return self
                .execute_model_mail_secret(invocation, adapter, secret, item_epoch, key_epoch)
                .await;
        }
        if matches!(
            request.operation,
            Operation::GoogleMailSend { .. } | Operation::GoogleMailLookup { .. }
        ) {
            return self
                .execute_mail_secret(invocation, adapter, secret, item_epoch, key_epoch, None)
                .await;
        }
        let preview = request.operation == Operation::GoogleInboxPreview;
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        let selected = if let Operation::GoogleInboxBody { selection } = request.operation {
            let local = self
                .state
                .google_selections
                .iter()
                .find(|s| s.selection == selection)
                .ok_or(Error::Policy)?;
            let original = self
                .grants()
                .iter()
                .find(|g| g.binding == local.preview_binding)
                .ok_or(Error::Policy)?;
            if local.expires_at <= current_time()?
                || local.item != grant.item()
                || Some(local.vault) != grant.vault()
                || Some(&local.identity) != grant.google()
                || local.preview_binding.item_epoch != item_epoch
                || local.preview_binding.key_epoch != key_epoch
                || original.live().map(|p| &p.origin) != grant.live().map(|p| &p.origin)
            {
                return Err(Error::Policy);
            }
            // A revoked/suspended/changed preview cannot be used to cross into body authority.
            let mut check = request.clone();
            check.operation = Operation::GoogleInboxPreview;
            check.binding = original.binding.clone();
            check.digest = preview_digest();
            check.expires_at = check.expires_at.min(original.binding.expires_at);
            original.check(&check, current_time()?, item_epoch, key_epoch)?;
            Some(local.id.clone())
        } else {
            None
        };

        let identity = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .and_then(|g| g.google())
            .ok_or(Error::Policy)?
            .clone();
        if preview {
            self.state
                .google_selections
                .retain(|s| s.expires_at > current_time().unwrap_or(i64::MAX));
            if self.state.google_selections.len() > 507 {
                return Err(Error::PrivateState);
            }
        }
        let commitment = self.begin_dispatch(invocation, current_time()?, item_epoch, key_epoch)?;
        let mut selections = Vec::new();
        let result = if preview || selected.is_some() {
            tokio::time::timeout(
                Duration::from_secs(60),
                adapter.read_for(
                    &identity,
                    secret,
                    preview,
                    selected.as_deref(),
                    &mut selections,
                ),
            )
            .await
            .unwrap_or(Err(Error::Unknown))
        } else {
            adapter.read(&identity, secret, false).await
        };
        let (projection, outcome) = match result {
            Ok(p) => (p, Outcome::ProjectedSuccess),
            Err(Error::Policy) => (Projection::None, Outcome::Rejected),
            Err(_) => (Projection::None, Outcome::Unknown),
        };
        let receipt = Receipt {
            commitment,
            outcome,
            projection: projection.clone(),
            projection_digest: auth::projection_digest(projection),
        };
        let signed = SignedReceipt {
            signature: self.key.sign(&receipt_bytes(&receipt)),
            receipt,
        };
        if outcome == Outcome::Rejected {
            self.state
                .grants
                .iter_mut()
                .find(|g| g.binding.id == signed.receipt.commitment.request.binding.id)
                .ok_or(Error::Policy)?
                .suspend_google();
        }
        if outcome == Outcome::ProjectedSuccess && preview {
            self.state
                .google_selections
                .retain(|s| s.expires_at > current_time().unwrap_or(i64::MAX));
            if self.state.google_selections.len() + selections.len() > 512 {
                return Err(Error::PrivateState);
            }
            for (selection, id) in selections {
                self.state.google_selections.push(BodySelection {
                    selection,
                    id,
                    preview_binding: grant.binding.clone(),
                    item: grant.item(),
                    vault: grant.vault().ok_or(Error::Policy)?,
                    identity: identity.clone(),
                    expires_at: signed.receipt.commitment.request.expires_at,
                });
            }
        }
        self.state
            .entries
            .iter_mut()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .phase = Phase::Settled(signed.clone());
        self.save()?;
        Ok(signed)
    }
}

#[cfg(test)]
mod tests;

mod send;
