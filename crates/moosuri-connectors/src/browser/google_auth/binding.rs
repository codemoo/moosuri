//! Public, nonsecret identity of one authentication attempt. Never model input.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL: &str = "moosuri-google-runtime-v1";

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub installation: Uuid,
    pub account: Uuid,
    pub tenant: Uuid,
    pub volume: Uuid,
    pub generation: i64,
    pub epoch: i64,
    pub challenge: Uuid,
    pub email: String,
    pub origin: String,
}

pub fn valid_email(email: &str) -> bool {
    email.len() <= 254
        && email.is_ascii()
        && !email
            .bytes()
            .any(|c| c.is_ascii_whitespace() || c.is_ascii_control())
        && email.split_once('@').is_some_and(|(local, domain)| {
            !local.is_empty() && !domain.is_empty() && !domain.contains('@')
        })
}

impl Binding {
    pub fn valid(&self) -> bool {
        [
            self.installation,
            self.account,
            self.tenant,
            self.volume,
            self.challenge,
        ]
        .iter()
        .all(|id| !id.is_nil())
            && self.generation > 0
            && self.epoch >= 0
            && valid_email(&self.email)
            && url::Url::parse(&self.origin).is_ok_and(|url| {
                url.scheme() == "https"
                    && url.host_str().is_some()
                    && url.username().is_empty()
                    && url.password().is_none()
                    && self.origin == url.origin().ascii_serialization()
            })
    }

    /// The vault uses the same ordered UTF-8 fields for OAEP and GCM AAD.
    pub fn label(&self, google_email: &str) -> Result<Vec<u8>, super::Error> {
        if !self.valid() || !valid_email(google_email) {
            return Err(super::Error::Invalid);
        }
        Ok(format!(
            "moosuri-google-v3\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}\n{}",
            self.installation,
            self.account,
            self.tenant,
            self.volume,
            self.generation,
            self.epoch,
            self.challenge,
            self.email,
            google_email,
            self.origin
        )
        .into_bytes())
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub key: String,
    pub iv: String,
    pub body: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Recipient {
    pub protocol: String,
    pub binding: Binding,
    pub spki: String,
}
