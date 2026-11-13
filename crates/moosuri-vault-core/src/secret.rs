use crate::{Error, Result};
use serde::{Deserialize, Deserializer, Serialize};
use zeroize::{Zeroize, Zeroizing};

pub(crate) const ITEM_LIMIT: usize = 65_536;

/// Explicit exposure is only for trusted local UI/adapter code, never tool output.
///
/// ```compile_fail
/// let secret = moosuri_vault_core::SecretText::new("fixture".into()).unwrap();
/// println!("{secret:?}"); // secrets cannot enter Debug logging
/// ```
/// ```compile_fail
/// let secret = moosuri_vault_core::SecretText::new("fixture".into()).unwrap();
/// let _copy = secret.clone();
/// ```
/// ```compile_fail
/// let secret = moosuri_vault_core::SecretText::new("fixture".into()).unwrap();
/// let _wire = serde_json::to_vec(&secret);
/// ```
pub struct SecretText(pub(crate) Zeroizing<String>);
impl SecretText {
    pub fn new(value: String) -> Result<Self> {
        let value = Zeroizing::new(value);
        if value.len() > ITEM_LIMIT {
            return Err(Error::Limit);
        }
        Ok(Self(value))
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}
impl<'de> Deserialize<'de> for SecretText {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        // The authenticated plaintext is capped before serde sees it. No raw
        // serde error ever crosses the core error boundary.
        let value = String::deserialize(d)?;
        Self::new(value).map_err(|_| serde::de::Error::custom("vault_limit"))
    }
}
pub(crate) struct Key(pub Zeroizing<[u8; 32]>);
impl Key {
    pub fn random(entropy: &mut impl crate::TrustedEntropy) -> Result<Self> {
        let mut bytes = Zeroizing::new([0; 32]);
        entropy.fill(bytes.as_mut())?;
        Ok(Self(bytes))
    }
}
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Login,
    ApiKey,
    Note,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ItemFields {
    pub kind: ItemKind,
    pub title: SecretText,
    pub username: SecretText,
    pub url: SecretText,
    pub secret: SecretText,
    pub notes: SecretText,
    pub favorite: bool,
    pub expires_at: SecretText,
    pub totp: SecretText,
}
#[derive(Serialize)]
struct Plain<'a> {
    expires_at: &'a str,
    favorite: bool,
    kind: ItemKind,
    notes: &'a str,
    secret: &'a str,
    title: &'a str,
    totp: &'a str,
    url: &'a str,
    username: &'a str,
}
impl ItemFields {
    pub(crate) fn encode(&self) -> Result<Zeroizing<Vec<u8>>> {
        for (value, cap) in [
            (&self.title, 512),
            (&self.username, 4096),
            (&self.url, 8192),
            (&self.secret, 16_384),
            (&self.notes, 32_768),
            (&self.expires_at, 128),
            (&self.totp, 4096),
        ] {
            if value.expose().len() > cap {
                return Err(Error::Limit);
            }
        }
        let plain = Plain {
            expires_at: self.expires_at.expose(),
            favorite: self.favorite,
            kind: self.kind,
            notes: self.notes.expose(),
            secret: self.secret.expose(),
            title: self.title.expose(),
            totp: self.totp.expose(),
            url: self.url.expose(),
            username: self.username.expose(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&plain).map_err(|_| Error::Invalid)?);
        if bytes.len() > ITEM_LIMIT {
            return Err(Error::Limit);
        }
        Ok(bytes)
    }
    pub(crate) fn decode(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > ITEM_LIMIT {
            return Err(Error::Limit);
        }
        let result: Self = serde_json::from_slice(bytes).map_err(|_| Error::Authentication)?;
        result.encode()?;
        Ok(result)
    }
}
pub(crate) fn utf8(mut bytes: Zeroizing<Vec<u8>>) -> Result<SecretText> {
    match String::from_utf8(std::mem::take(&mut *bytes)) {
        Ok(s) => SecretText::new(s),
        Err(e) => {
            let mut bad = e.into_bytes();
            bad.zeroize();
            Err(Error::Csv)
        }
    }
}

/// url::Url owns its serialized URI; wipe that buffer on every exit path.
pub(crate) struct SecretUrl(Option<url::Url>);
impl SecretUrl {
    pub fn parse(value: &str) -> Result<Self> {
        Ok(Self(Some(
            url::Url::parse(value).map_err(|_| Error::Invalid)?,
        )))
    }
}
impl std::ops::Deref for SecretUrl {
    type Target = url::Url;
    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("live secret URL")
    }
}
impl Drop for SecretUrl {
    fn drop(&mut self) {
        if let Some(url) = self.0.take() {
            let mut value: String = url.into();
            value.zeroize();
        }
    }
}
