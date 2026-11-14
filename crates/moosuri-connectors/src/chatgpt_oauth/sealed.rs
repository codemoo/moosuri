//! Trusted private-runtime persistence, never an external identity import API.
use super::*;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};
/// Possession of this key is the explicit trusted-runtime persistence authority.
/// Neither the key nor plaintext records may enter HTTP DTOs or ordinary logging.
pub struct SessionSealer(Zeroizing<[u8; 32]>);
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    client: String,
    subject: String,
    host: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    version: u8,
    identity: Identity,
    access: String,
    refresh: String,
    id_token: String,
    nonce: String,
    scopes: BTreeSet<String>,
    received: u64,
    expires: u64,
    hint: Option<serde_json::Value>,
}
impl Drop for Wire {
    fn drop(&mut self) {
        self.access.zeroize();
        self.refresh.zeroize();
        self.id_token.zeroize();
        self.nonce.zeroize();
    }
}
impl SessionSealer {
    pub fn new(key: Zeroizing<[u8; 32]>) -> Self {
        Self(key)
    }
    fn seal_bytes(&self, plain: &[u8], context: &[u8]) -> Result<Vec<u8>, AuthError> {
        if plain.len() > 128 * 1024 || context.len() > 4096 {
            return Err(AuthError::LimitExceeded);
        }
        let mut n = [0; 24];
        SystemRandom::new()
            .fill(&mut n)
            .map_err(|_| AuthError::RandomUnavailable)?;
        let mut result = n.to_vec();
        result.extend(
            XChaCha20Poly1305::new((&*self.0).into())
                .encrypt(
                    XNonce::from_slice(&n),
                    Payload {
                        msg: plain,
                        aad: context,
                    },
                )
                .map_err(|_| AuthError::InvalidResponse)?,
        );
        Ok(result)
    }
    fn open_bytes(&self, cipher: &[u8], context: &[u8]) -> Result<Zeroizing<Vec<u8>>, AuthError> {
        if cipher.len() < 40 || cipher.len() > 128 * 1024 + 40 || context.len() > 4096 {
            return Err(AuthError::InvalidResponse);
        }
        XChaCha20Poly1305::new((&*self.0).into())
            .decrypt(
                XNonce::from_slice(&cipher[..24]),
                Payload {
                    msg: &cipher[24..],
                    aad: context,
                },
            )
            .map(Zeroizing::new)
            .map_err(|_| AuthError::IdentityInvalid)
    }
    pub fn seal(&self, b: &TokenBundle, context: &[u8]) -> Result<Vec<u8>, AuthError> {
        let w = Wire {
            version: 1,
            identity: Identity {
                client: b.identity.client_id.clone(),
                subject: b.identity.subject.clone(),
                host: b.identity.host_id.clone(),
            },
            access: b.access.to_string(),
            refresh: b.refresh.to_string(),
            id_token: b.id_token.to_string(),
            nonce: b.nonce.to_string(),
            scopes: b.scopes.clone(),
            received: b.received_at,
            expires: b.expires_at,
            hint: b.earliest_refresh_at.clone(),
        };
        let bytes = Zeroizing::new(serde_json::to_vec(&w).map_err(|_| AuthError::InvalidResponse)?);
        self.seal_bytes(&bytes, context)
    }
    /// Authenticate a previously verified local record. An expired retained ID
    /// token is valid historical evidence; this does not perform fresh OAuth.
    pub fn open(&self, cipher: &[u8], context: &[u8]) -> Result<TokenBundle, AuthError> {
        let plain = self.open_bytes(cipher, context)?;
        let mut w: Wire = serde_json::from_slice(&plain).map_err(|_| AuthError::InvalidResponse)?;
        if w.version != 1
            || !safe_atom(&w.identity.client, 512)
            || w.identity.client == DYNAMIC_CLIENT
            || !safe_atom(&w.identity.subject, 1024)
            || !safe_atom(&w.identity.host, 256)
            || w.expires <= w.received
            || w.access.is_empty()
            || w.refresh.is_empty()
            || w.id_token.is_empty()
            || w.nonce.is_empty()
        {
            return Err(AuthError::IdentityInvalid);
        }
        Ok(TokenBundle {
            identity: VerifiedIdentity {
                client_id: w.identity.client.clone(),
                subject: w.identity.subject.clone(),
                host_id: w.identity.host.clone(),
            },
            access: Zeroizing::new(std::mem::take(&mut w.access)),
            refresh: Zeroizing::new(std::mem::take(&mut w.refresh)),
            id_token: Zeroizing::new(std::mem::take(&mut w.id_token)),
            nonce: Zeroizing::new(std::mem::take(&mut w.nonce)),
            scopes: std::mem::take(&mut w.scopes),
            received_at: w.received,
            expires_at: w.expires,
            earliest_refresh_at: w.hint.take(),
        })
    }
    pub fn seal_identity(
        &self,
        i: &VerifiedIdentity,
        context: &[u8],
    ) -> Result<Vec<u8>, AuthError> {
        let p = serde_json::to_vec(&Identity {
            client: i.client_id.clone(),
            subject: i.subject.clone(),
            host: i.host_id.clone(),
        })
        .map_err(|_| AuthError::InvalidResponse)?;
        self.seal_bytes(&p, context)
    }
    pub fn open_identity(
        &self,
        cipher: &[u8],
        context: &[u8],
    ) -> Result<VerifiedIdentity, AuthError> {
        let p = self.open_bytes(cipher, context)?;
        let i: Identity = serde_json::from_slice(&p).map_err(|_| AuthError::IdentityInvalid)?;
        if !safe_atom(&i.client, 512)
            || i.client == DYNAMIC_CLIENT
            || !safe_atom(&i.subject, 1024)
            || !safe_atom(&i.host, 256)
        {
            return Err(AuthError::IdentityInvalid);
        }
        Ok(VerifiedIdentity {
            client_id: i.client,
            subject: i.subject,
            host_id: i.host,
        })
    }
}
