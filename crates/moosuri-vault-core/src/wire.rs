use crate::{Error, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub(crate) const HEADER_LIMIT: usize = 16_384;
pub(crate) const MAX_ITEMS: usize = 1000;
pub(crate) const MAX_REVISION: u64 = 9_007_199_254_740_991;
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub owner_id: Uuid,
    pub tenant_id: Uuid,
    pub vault_id: Uuid,
}
impl Binding {
    pub(crate) fn validate(&self) -> Result<()> {
        if [self.owner_id, self.tenant_id, self.vault_id]
            .iter()
            .any(Uuid::is_nil)
        {
            return Err(Error::Invalid);
        }
        Ok(())
    }
}
/// The only admitted password KDF policy. Check it before running Argon2.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordKdf {
    pub iterations: u32,
    pub lanes: u32,
    pub memory_kib: u32,
    pub output_bytes: u32,
    pub version: u32,
}
impl Default for PasswordKdf {
    fn default() -> Self {
        Self {
            iterations: 3,
            lanes: 1,
            memory_kib: 65_536,
            output_bytes: 32,
            version: 0x13,
        }
    }
}
impl PasswordKdf {
    pub(crate) fn validate(&self) -> Result<()> {
        if self != &Self::default() {
            Err(Error::KdfPolicy)
        } else {
            Ok(())
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "algorithm", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Kdf {
    Argon2id13 { params: PasswordKdf },
    HkdfSha256,
}
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum EnvelopeKind {
    Password,
    Recovery,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Envelope {
    pub ciphertext: String,
    pub created_version: u8,
    pub generation: u64,
    pub id: Uuid,
    pub kdf: Kdf,
    pub kind: EnvelopeKind,
    pub nonce: String,
    pub salt: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Header {
    pub authentication: String,
    pub binding: Binding,
    pub envelope_epoch: u64,
    pub(crate) envelopes: Vec<Envelope>,
    pub generation: u64,
    pub revision: u64,
    pub version: u8,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EncryptedItem {
    pub ciphertext: String,
    pub generation: u64,
    pub id: Uuid,
    pub key_nonce: String,
    pub nonce: String,
    pub revision: u64,
    pub version: u8,
    pub wrapped_key: String,
}
pub(crate) fn rev(n: u64) -> Result<()> {
    if n == 0 || n > MAX_REVISION {
        Err(Error::Invalid)
    } else {
        Ok(())
    }
}
pub(crate) fn next(n: u64) -> Result<u64> {
    let n = n.checked_add(1).ok_or(Error::Limit)?;
    rev(n)?;
    Ok(n)
}
pub(crate) fn b64(bytes: &[u8]) -> String {
    STANDARD.encode(bytes)
}
pub(crate) fn unb64(s: &str, min: usize, max: usize) -> Result<Vec<u8>> {
    if s.len() > max.div_ceil(3).checked_mul(4).ok_or(Error::Limit)? {
        return Err(Error::Limit);
    }
    let bytes = STANDARD.decode(s).map_err(|_| Error::Invalid)?;
    if !(min..=max).contains(&bytes.len()) || STANDARD.encode(&bytes) != s {
        return Err(Error::Invalid);
    }
    Ok(bytes)
}
pub(crate) fn json<T: Serialize>(value: &T, cap: usize) -> Result<Vec<u8>> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Invalid)?;
    if bytes.len() > cap {
        return Err(Error::Limit);
    }
    Ok(bytes)
}
impl Header {
    pub fn from_json(bytes: &[u8], expected: Binding) -> Result<Self> {
        if bytes.len() > HEADER_LIMIT {
            return Err(Error::Limit);
        }
        let h: Self = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        h.validate(expected)?;
        Ok(h)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate(self.binding)?;
        json(self, HEADER_LIMIT)
    }
    pub(crate) fn validate(&self, expected: Binding) -> Result<()> {
        expected.validate()?;
        if self.version != 2 {
            return Err(Error::Unsupported);
        }
        if self.binding != expected {
            return Err(Error::Authentication);
        }
        rev(self.generation)?;
        rev(self.envelope_epoch)?;
        rev(self.revision)?;
        unb64(&self.authentication, 32, 32)?;
        if self.envelopes.len() != 2
            || self.envelopes[0].kind != EnvelopeKind::Password
            || self.envelopes[1].kind != EnvelopeKind::Recovery
            || self.envelopes[0].id == self.envelopes[1].id
        {
            return Err(Error::Invalid);
        }
        for e in &self.envelopes {
            if e.created_version != 2 || e.generation != self.generation || e.id.is_nil() {
                return Err(Error::Invalid);
            }
            match (&e.kdf, e.kind) {
                (Kdf::Argon2id13 { params }, EnvelopeKind::Password) => params.validate()?,
                (Kdf::HkdfSha256, EnvelopeKind::Recovery) => (),
                _ => return Err(Error::KdfPolicy),
            }
            unb64(&e.salt, 16, 16)?;
            unb64(&e.nonce, 12, 12)?;
            unb64(&e.ciphertext, 48, 48)?;
        }
        Ok(())
    }
}
impl EncryptedItem {
    pub fn from_json(bytes: &[u8]) -> Result<Self> {
        if bytes.len() > 100_000 {
            return Err(Error::Limit);
        }
        let item: Self = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        item.validate()?;
        Ok(item)
    }
    pub fn to_json(&self) -> Result<Vec<u8>> {
        self.validate()?;
        json(self, 100_000)
    }
    pub(crate) fn validate(&self) -> Result<()> {
        if self.version != 2 {
            return Err(Error::Unsupported);
        }
        rev(self.revision)?;
        rev(self.generation)?;
        if self.id.is_nil() {
            return Err(Error::Invalid);
        }
        unb64(&self.key_nonce, 12, 12)?;
        unb64(&self.nonce, 12, 12)?;
        unb64(&self.wrapped_key, 48, 48)?;
        unb64(&self.ciphertext, 16, crate::secret::ITEM_LIMIT + 16)?;
        Ok(())
    }
}
