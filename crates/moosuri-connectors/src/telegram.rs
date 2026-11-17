//! Fixed-origin Bot API. Never expose request URLs, tokens or provider error bodies.
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::time::Duration;
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Client {
    http: reqwest::Client,
    base: String,
    token: Zeroizing<String>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unauthorized,
    Conflict,
    Rejected,
    Retry(u32),
    Unknown,
}
#[derive(Deserialize)]
struct Envelope<T> {
    ok: bool,
    result: Option<T>,
    error_code: Option<u16>,
    parameters: Option<Parameters>,
}
#[derive(Deserialize)]
struct Parameters {
    retry_after: Option<u32>,
}
#[derive(Clone, Deserialize)]
pub struct Identity {
    pub id: i64,
    pub is_bot: bool,
    pub username: Option<String>,
}
#[derive(Clone, Deserialize)]
pub struct Chat {
    pub id: i64,
    #[serde(rename = "type")]
    pub kind: String,
}
#[derive(Clone, Deserialize)]
pub struct Message {
    pub message_id: i64,
    pub date: i64,
    pub from: Option<Identity>,
    pub chat: Chat,
    pub text: Option<String>,
}
#[derive(Clone, Deserialize)]
pub struct Update {
    pub update_id: i64,
    pub message: Option<Message>,
}
impl Client {
    pub fn new(token: Zeroizing<String>) -> Result<Self, Error> {
        let (id, secret) = token.split_once(':').ok_or(Error::Invalid)?;
        if id.is_empty()
            || id.len() > 20
            || !id.bytes().all(|b| b.is_ascii_digit())
            || !(20..=128).contains(&secret.len())
            || !secret
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
        {
            return Err(Error::Invalid);
        }
        let http = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(35))
            .build()
            .map_err(|_| Error::Invalid)?;
        Ok(Self {
            http,
            base: "https://api.telegram.org".into(),
            token,
        })
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn fixture(mut self, base: &str) -> Result<Self, Error> {
        let u = url::Url::parse(base).map_err(|_| Error::Invalid)?;
        if u.scheme() != "http"
            || !matches!(u.host_str(), Some("127.0.0.1" | "[::1]"))
            || u.path() != "/"
            || u.query().is_some()
            || u.fragment().is_some()
            || !u.username().is_empty()
            || u.password().is_some()
        {
            return Err(Error::Invalid);
        }
        self.base = u.origin().ascii_serialization();
        Ok(self)
    }
    async fn call<T: DeserializeOwned>(
        &self,
        method: &str,
        body: impl Serialize,
        timeout: u64,
    ) -> Result<T, Error> {
        let url = Zeroizing::new(format!("{}/bot{}/{method}", self.base, self.token.as_str()));
        let mut response = self
            .http
            .post(url.as_str())
            .timeout(Duration::from_secs(timeout))
            .json(&body)
            .send()
            .await
            .map_err(|_| Error::Unknown)?;
        let status = response.status().as_u16();
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unknown)? {
            if bytes.len() + chunk.len() > 512 * 1024 {
                return Err(Error::Unknown);
            }
            bytes.extend_from_slice(&chunk);
        }
        let envelope: Envelope<T> = serde_json::from_slice(&bytes).map_err(|_| Error::Unknown)?;
        if envelope.ok && status == 200 {
            return envelope.result.ok_or(Error::Unknown);
        }
        Err(match envelope.error_code.unwrap_or(status) {
            401 => Error::Unauthorized,
            403 | 400 => Error::Rejected,
            409 => Error::Conflict,
            429 => envelope
                .parameters
                .and_then(|p| p.retry_after)
                .map(|s| Error::Retry(s.clamp(1, 3600)))
                .unwrap_or(Error::Retry(60)),
            _ => Error::Unknown,
        })
    }
    /// Read-only validation. Existing webhooks are never removed automatically.
    pub async fn verify(&self) -> Result<Identity, Error> {
        let me: Identity = self.call("getMe", serde_json::json!({}), 10).await?;
        if !me.is_bot
            || me.id <= 0
            || me.username.as_ref().is_none_or(|u| {
                u.is_empty()
                    || u.len() > 64
                    || !u.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            })
        {
            return Err(Error::Invalid);
        }
        #[derive(Deserialize)]
        struct Webhook {
            url: String,
        }
        let webhook: Webhook = self
            .call("getWebhookInfo", serde_json::json!({}), 10)
            .await?;
        if !webhook.url.is_empty() {
            return Err(Error::Conflict);
        }
        Ok(me)
    }
    pub async fn updates(&self, offset: i64) -> Result<Vec<Update>, Error> {
        self.call("getUpdates", serde_json::json!({"offset":offset,"limit":32,"timeout":25,"allowed_updates":["message"]}), 30).await
    }
    pub async fn send(&self, chat: i64, text: &str) -> Result<(), Error> {
        let _: serde_json::Value = self.call("sendMessage", serde_json::json!({"chat_id":chat,"text":text,"link_preview_options":{"is_disabled":true},"protect_content":true}), 10).await?;
        Ok(())
    }
    pub async fn typing(&self, chat: i64) -> Result<(), Error> {
        let _: bool = self
            .call(
                "sendChatAction",
                serde_json::json!({"chat_id":chat,"action":"typing"}),
                3,
            )
            .await?;
        Ok(())
    }
}
/// Plain text, bounded conservatively in UTF-16 units without splitting a scalar.
pub fn chunk(text: &str, start: usize) -> Option<&str> {
    let remaining = text.get(start..)?;
    if remaining.is_empty() {
        return None;
    }
    let mut units = 0;
    let mut end = 0;
    for (offset, c) in remaining.char_indices() {
        units += c.len_utf16();
        if units > 3500 {
            break;
        }
        end = offset + c.len_utf8();
    }
    remaining.get(..end)
}
