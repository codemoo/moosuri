//! Explicit user-local v1 authenticated export migration, never raw-header admission.
//! Contract-derived implementation; no old runtime/source code or user data is bundled.
use crate::{
    crypto::{decrypt, password_key},
    secret::Key,
    wire::{EnvelopeKind, b64, json, rev, unb64},
    *,
};
use hkdf::Hkdf;
use serde::{Deserialize, Deserializer, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use uuid::Uuid;
use zeroize::Zeroizing;
const FORMAT: &str = "musuri-vault";
#[cfg(test)]
thread_local! {static ROOT_OPENS:std::cell::Cell<usize>=const{std::cell::Cell::new(0)};}
fn uuid<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Uuid, D::Error> {
    let s = String::deserialize(d)?;
    let id = Uuid::parse_str(&s).map_err(|_| serde::de::Error::custom("vault_uuid"))?;
    if id.to_string() != s {
        return Err(serde::de::Error::custom("vault_uuid"));
    }
    Ok(id)
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "algorithm", rename_all = "snake_case", deny_unknown_fields)]
enum Kdf {
    Argon2id13 {
        iterations: u32,
        memory: u32,
    },
    #[serde(rename = "hkdf-sha256")]
    HkdfSha256,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    ciphertext: String,
    created_version: u8,
    generation: u64,
    #[serde(deserialize_with = "uuid")]
    id: Uuid,
    kdf: Kdf,
    kind: EnvelopeKind,
    nonce: String,
    salt: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    envelope_epoch: u64,
    envelopes: [Envelope; 2],
    generation: u64,
    #[serde(deserialize_with = "uuid")]
    id: Uuid,
    revision: u64,
    version: u8,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Item {
    ciphertext: String,
    generation: u64,
    #[serde(deserialize_with = "uuid")]
    id: Uuid,
    key_nonce: String,
    nonce: String,
    revision: u64,
    version: u8,
    wrapped_key: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ManifestCipher {
    ciphertext: String,
    nonce: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Archive {
    format: String,
    header: Header,
    items: Vec<Item>,
    manifest: ManifestCipher,
    version: u8,
}
// All metadata structs use ASCII sorted keys, exactly the old canonical JSON rule.
#[derive(Serialize)]
struct EnvelopeMeta<'a> {
    created_version: u8,
    generation: u64,
    id: Uuid,
    kdf: &'a Kdf,
    kind: EnvelopeKind,
    salt: &'a str,
}
#[derive(Serialize)]
struct EnvelopeAad<'a> {
    domain: &'static str,
    envelope: EnvelopeMeta<'a>,
    vault_id: Uuid,
}
#[derive(Serialize)]
struct ItemAad {
    domain: &'static str,
    generation: u64,
    item_id: Uuid,
    revision: u64,
    vault_id: Uuid,
    version: u8,
}
#[derive(Serialize)]
struct ArchiveAad<'a> {
    domain: &'static str,
    header: &'a Header,
    version: u8,
}
#[derive(Serialize)]
struct Inventory<'a> {
    header: &'a Header,
    items: &'a [Item],
}
#[derive(Serialize)]
struct Manifest {
    count: usize,
    digest: String,
    ids: Vec<Uuid>,
    version: u8,
}
fn item_aad(h: &Header, i: &Item, key: bool) -> Result<Vec<u8>> {
    json(
        &ItemAad {
            domain: if key {
                "musuri-vault/v1/item/key"
            } else {
                "musuri-vault/v1/item/payload"
            },
            generation: h.generation,
            item_id: i.id,
            revision: i.revision,
            vault_id: h.id,
            version: 1,
        },
        4096,
    )
}
impl Archive {
    fn validate(&self, cancel: &impl Cancellation) -> Result<()> {
        if self.format != FORMAT || self.version != 1 || self.header.version != 1 {
            return Err(Error::Unsupported);
        }
        if self.items.len() > LEGACY_ARCHIVE_ITEMS {
            return Err(Error::Limit);
        }
        let h = &self.header;
        rev(h.generation)?;
        rev(h.envelope_epoch)?;
        rev(h.revision)?;
        if h.envelopes[0].id == h.envelopes[1].id || h.envelopes[0].kind == h.envelopes[1].kind {
            return Err(Error::Invalid);
        }
        for e in &h.envelopes {
            rev(e.generation)?;
            if e.created_version != 1 || e.generation != h.generation {
                return Err(Error::Invalid);
            }
            unb64(&e.salt, 16, 16)?;
            unb64(&e.nonce, 12, 12)?;
            unb64(&e.ciphertext, 48, 48)?;
            match (&e.kind, &e.kdf) {
                (
                    EnvelopeKind::Password,
                    Kdf::Argon2id13 {
                        iterations: 3,
                        memory: 67108864,
                    },
                )
                | (EnvelopeKind::Recovery, Kdf::HkdfSha256) => (),
                _ => return Err(Error::KdfPolicy),
            }
        }
        json(h, 16384)?;
        let mut ids = HashSet::new();
        let mut nonces = HashSet::new();
        for item in &self.items {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            if item.version != 1
                || item.generation != h.generation
                || !ids.insert(item.id)
                || !nonces.insert(&item.key_nonce)
            {
                return Err(Error::Conflict);
            }
            rev(item.revision)?;
            unb64(&item.key_nonce, 12, 12)?;
            unb64(&item.nonce, 12, 12)?;
            unb64(&item.wrapped_key, 48, 48)?;
            unb64(&item.ciphertext, 16, 65552)?;
            json(item, 100000)?;
        }
        unb64(&self.manifest.nonce, 12, 12)?;
        unb64(&self.manifest.ciphertext, 16, 262144)?;
        Ok(())
    }
    fn open(
        &self,
        secret: &SecretText,
        recovery: bool,
        cancel: &impl Cancellation,
    ) -> Result<Vec<ItemFields>> {
        #[cfg(test)]
        ROOT_OPENS.with(|v| v.set(v.get() + 1));
        let h = &self.header;
        let kind = if recovery {
            EnvelopeKind::Recovery
        } else {
            EnvelopeKind::Password
        };
        let e = h
            .envelopes
            .iter()
            .find(|e| e.kind == kind)
            .ok_or(Error::Invalid)?;
        let wrapping = if recovery {
            use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
            let text = secret.expose().strip_prefix("MR1-").ok_or(Error::Invalid)?;
            if text.len() != 43 {
                return Err(Error::Invalid);
            }
            let raw = Zeroizing::new(URL_SAFE_NO_PAD.decode(text).map_err(|_| Error::Invalid)?);
            if raw.len() != 32 || Zeroizing::new(URL_SAFE_NO_PAD.encode(&*raw)).as_str() != text {
                return Err(Error::Invalid);
            }
            let mut key = Zeroizing::new([0; 32]);
            Hkdf::<Sha256>::new(Some(&unb64(&e.salt, 16, 16)?), &raw)
                .expand(
                    format!("musuri-vault/v1/recovery/{}/{}", h.id, e.id).as_bytes(),
                    key.as_mut(),
                )
                .map_err(|_| Error::Invalid)?;
            Key(key)
        } else {
            password_key(secret, &unb64(&e.salt, 16, 16)?, &PasswordKdf::default())?
        };
        let aad = json(
            &EnvelopeAad {
                domain: "musuri-vault/v1/envelope",
                envelope: EnvelopeMeta {
                    created_version: e.created_version,
                    generation: e.generation,
                    id: e.id,
                    kdf: &e.kdf,
                    kind: e.kind,
                    salt: &e.salt,
                },
                vault_id: h.id,
            },
            4096,
        )?;
        let raw = decrypt(&wrapping, &e.nonce, &aad, &e.ciphertext, 32)?;
        if raw.len() != 32 {
            return Err(Error::Authentication);
        }
        let mut root = Zeroizing::new([0; 32]);
        root.copy_from_slice(&raw);
        drop(raw);
        let root = Key(root);
        let mut manifest_key = Zeroizing::new([0; 32]);
        Hkdf::<Sha256>::new(Some(&unb64(&self.manifest.nonce, 12, 12)?), root.0.as_ref())
            .expand(
                format!("musuri-vault/v1/archive-key/{}/{}", h.id, h.generation).as_bytes(),
                manifest_key.as_mut(),
            )
            .map_err(|_| Error::Invalid)?;
        let plain = decrypt(
            &Key(manifest_key),
            &self.manifest.nonce,
            &json(
                &ArchiveAad {
                    domain: "musuri-vault/v1/archive",
                    header: h,
                    version: 1,
                },
                16384,
            )?,
            &self.manifest.ciphertext,
            262128,
        )?;
        let digest = b64(&Sha256::digest(json(
            &Inventory {
                header: h,
                items: &self.items,
            },
            ARCHIVE_LIMIT,
        )?));
        let expected = json(
            &Manifest {
                count: self.items.len(),
                digest,
                ids: self.items.iter().map(|i| i.id).collect(),
                version: 1,
            },
            262128,
        )?;
        if plain.as_slice() != expected.as_slice() {
            return Err(Error::Authentication);
        }
        drop(plain);
        let mut fields = Vec::with_capacity(self.items.len());
        for item in &self.items {
            if cancel.cancelled() {
                return Err(Error::Cancelled);
            }
            let raw = decrypt(
                &root,
                &item.key_nonce,
                &item_aad(h, item, true)?,
                &item.wrapped_key,
                32,
            )?;
            if raw.len() != 32 {
                return Err(Error::Authentication);
            }
            let mut key = Zeroizing::new([0; 32]);
            key.copy_from_slice(&raw);
            drop(raw);
            let plain = decrypt(
                &Key(key),
                &item.nonce,
                &item_aad(h, item, false)?,
                &item.ciphertext,
                65536,
            )?;
            let f: Fields = serde_json::from_slice(&plain).map_err(|_| Error::Authentication)?;
            let f = ItemFields {
                kind: f.kind,
                title: f.title,
                username: f.username,
                url: f.url,
                secret: f.secret,
                notes: f.notes,
                favorite: f.favorite,
                expires_at: f.expires_at,
                totp: f.totp,
            };
            f.encode()?;
            fields.push(f);
        }
        Ok(fields)
    }
}
fn empty() -> SecretText {
    SecretText(Zeroizing::new(String::new()))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Fields {
    kind: ItemKind,
    title: SecretText,
    username: SecretText,
    url: SecretText,
    secret: SecretText,
    notes: SecretText,
    favorite: bool,
    expires_at: SecretText,
    #[serde(default = "empty")]
    totp: SecretText,
}
impl ArchiveImport {
    /// A selected old authenticated export is opened entirely on this device.
    /// It authenticates supplied export coverage, never original-server completeness/latestness.
    pub fn preview_legacy_v1(
        bytes: &[u8],
        secret: &SecretText,
        recovery: bool,
        current: &mut ActiveVault,
        existing: &[EncryptedItem],
        cancel: &impl Cancellation,
    ) -> Result<Self> {
        if bytes.len() > ARCHIVE_LIMIT || existing.len() > MAX_ITEMS {
            return Err(Error::Limit);
        }
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        let archive: Archive = serde_json::from_slice(bytes).map_err(|_| Error::Invalid)?;
        archive.validate(cancel)?;
        if cancel.cancelled() {
            return Err(Error::Cancelled);
        }
        let fields = archive.open(secret, recovery, cancel)?;
        Self::from_authenticated_fields(
            fields,
            archive.items.iter().map(|i| i.id).collect(),
            current,
            existing,
            cancel,
        )
    }
}

#[cfg(all(test, feature = "native-entropy"))]
mod tests;
