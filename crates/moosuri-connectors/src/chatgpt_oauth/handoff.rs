//! One-use external session transfer. Local sealer reopening is deliberately absent.
//! The authorized local helper is trusted for the endpoint's opaque token pair;
//! fresh signed identity proves account/client/nonce, not refresh-token binding.
use super::*;
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, Payload},
};
use serde::{Deserialize, Serialize};

pub const MAX_ENVELOPE: usize = 128 * 1024 + 40;
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HandoffContext {
    pub version: u8,
    pub ticket: String,
    pub runtime: String,
    pub tenant: String,
    pub account: String,
    pub target_host: String,
    pub binding: String,
    pub expected_revision: i64,
    pub staged_revision: i64,
}
impl HandoffContext {
    pub fn aad(&self) -> Result<Vec<u8>, AuthError> {
        if self.version != 1
            || self.expected_revision < 0
            || self.staged_revision <= self.expected_revision
            || [
                &self.ticket,
                &self.runtime,
                &self.tenant,
                &self.account,
                &self.target_host,
                &self.binding,
            ]
            .iter()
            .any(|s| !safe_atom(s, 256))
        {
            return Err(AuthError::InvalidInput);
        }
        serde_json::to_vec(&("moosuri/chatgpt/handoff/v1/source-to-target", self))
            .map_err(|_| AuthError::InvalidInput)
    }
}
/// No Debug/Clone/serde: only private native transport and encrypted persistence.
pub struct HandoffKey(Zeroizing<[u8; 32]>);
impl HandoffKey {
    pub fn generate() -> Result<Self, AuthError> {
        let mut key = Zeroizing::new([0; 32]);
        SystemRandom::new()
            .fill(key.as_mut())
            .map_err(|_| AuthError::RandomUnavailable)?;
        Ok(Self(key))
    }
    pub fn from_private_bytes(bytes: &[u8]) -> Result<Self, AuthError> {
        Ok(Self(Zeroizing::new(
            bytes.try_into().map_err(|_| AuthError::InvalidInput)?,
        )))
    }
    pub fn from_private_transport(s: &str) -> Result<Self, AuthError> {
        let bytes = Zeroizing::new(
            URL_SAFE_NO_PAD
                .decode(s)
                .map_err(|_| AuthError::InvalidInput)?,
        );
        Self::from_private_bytes(&bytes)
    }
    pub fn with_private_bytes<T>(&self, f: impl FnOnce(&[u8]) -> T) -> T {
        f(self.0.as_ref())
    }
    pub fn for_private_transport(&self) -> Zeroizing<String> {
        Zeroizing::new(URL_SAFE_NO_PAD.encode(self.0.as_ref()))
    }
    /// Encrypt a local pending journal (including its short-lived owner session).
    /// The key is kept in the helper's owner-private state directory.
    pub fn seal_private_journal(&self, plain: &[u8]) -> Result<Vec<u8>, AuthError> {
        encrypt(self, plain, b"moosuri/chatgpt/helper-journal/v1")
    }
    pub fn open_private_journal(&self, cipher: &[u8]) -> Result<Zeroizing<Vec<u8>>, AuthError> {
        decrypt(self, cipher, b"moosuri/chatgpt/helper-journal/v1")
    }
}
fn encrypt(key: &HandoffKey, plain: &[u8], aad: &[u8]) -> Result<Vec<u8>, AuthError> {
    if plain.len() > 128 * 1024 {
        return Err(AuthError::LimitExceeded);
    }
    let mut nonce = [0; 24];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| AuthError::RandomUnavailable)?;
    let mut out = nonce.to_vec();
    out.extend(
        XChaCha20Poly1305::new((&*key.0).into())
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: plain, aad })
            .map_err(|_| AuthError::InvalidResponse)?,
    );
    Ok(out)
}
fn decrypt(key: &HandoffKey, cipher: &[u8], aad: &[u8]) -> Result<Zeroizing<Vec<u8>>, AuthError> {
    if !(40..=MAX_ENVELOPE).contains(&cipher.len()) {
        return Err(AuthError::LimitExceeded);
    }
    XChaCha20Poly1305::new((&*key.0).into())
        .decrypt(
            XNonce::from_slice(&cipher[..24]),
            Payload {
                msg: &cipher[24..],
                aad,
            },
        )
        .map(Zeroizing::new)
        .map_err(|_| AuthError::IdentityInvalid)
}
/// Exact domain-separated commitment, fixed at target before browser launch.
fn commitment(ticket: &str, nonce: &str) -> [u8; 32] {
    let bytes = serde_json::to_vec(&("moosuri/chatgpt/handoff/nonce/v1", ticket, nonce))
        .expect("string tuple serialization");
    Sha256::digest(bytes).into()
}
impl AuthorizationAttempt {
    pub fn handoff_nonce_commitment(&self, ticket: &str) -> Result<[u8; 32], AuthError> {
        if !safe_atom(ticket, 256) {
            return Err(AuthError::InvalidInput);
        }
        Ok(commitment(ticket, &self.nonce))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    context: HandoffContext,
    source_host: String,
    client: String,
    access: String,
    refresh: String,
    id_token: String,
    nonce: String,
    scope: String,
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
/// Consumes a fresh verified local exchange; never exposes renewable JSON.
pub fn seal_handoff(
    bundle: TokenBundle,
    context: &HandoffContext,
    key: &HandoffKey,
) -> Result<Vec<u8>, AuthError> {
    let aad = context.aad()?;
    if bundle.identity.host_id == context.target_host
        || unix_now()?.saturating_sub(bundle.received_at) > 300
    {
        return Err(AuthError::InvalidInput);
    }
    let wire = Wire {
        context: context.clone(),
        source_host: bundle.identity.host_id.clone(),
        client: bundle.identity.client_id.clone(),
        access: bundle.access.to_string(),
        refresh: bundle.refresh.to_string(),
        id_token: bundle.id_token.to_string(),
        nonce: bundle.nonce.to_string(),
        scope: bundle.scopes.iter().cloned().collect::<Vec<_>>().join(" "),
        received: bundle.received_at,
        expires: bundle.expires_at,
        hint: bundle.earliest_refresh_at.clone(),
    };
    let plain = Zeroizing::new(serde_json::to_vec(&wire).map_err(|_| AuthError::InvalidResponse)?);
    encrypt(key, &plain, &aad)
}
pub struct HandoffExpectation {
    pub context: HandoffContext,
    pub commitment: [u8; 32],
    pub client_id: String,
    pub subject: Option<String>,
    pub source_host: String,
    pub issued_at: u64,
}
/// Constructor private: external bytes must pass AEAD, commitment and fresh JWKS.
pub struct VerifiedImportedSession {
    bundle: TokenBundle,
    context: HandoffContext,
    digest: [u8; 32],
}
impl VerifiedImportedSession {
    /// The verified import already assigned the target's existing host identity.
    pub fn into_target_bundle(
        self,
        context: &HandoffContext,
        digest: &[u8],
    ) -> Result<TokenBundle, AuthError> {
        if &self.context != context || self.digest.as_slice() != digest {
            return Err(AuthError::IdentityInvalid);
        }
        Ok(self.bundle)
    }
}
impl ChatGptAuthClient {
    pub async fn verify_handoff(
        &self,
        envelope: &[u8],
        key: &HandoffKey,
        expected: &HandoffExpectation,
    ) -> Result<VerifiedImportedSession, AuthError> {
        let plain = decrypt(key, envelope, &expected.context.aad()?)?;
        let mut wire: Wire =
            serde_json::from_slice(&plain).map_err(|_| AuthError::InvalidResponse)?;
        let now = unix_now()?;
        if wire.context != expected.context
            || wire.client != expected.client_id
            || !safe_atom(&wire.source_host, 256)
            || wire.source_host == expected.context.target_host
            || wire.source_host != expected.source_host
            || !safe_atom(&wire.access, 32768)
            || !safe_atom(&wire.refresh, 32768)
            || !safe_atom(&wire.nonce, 1024)
            || wire.received > now.saturating_add(60)
            || wire.received.saturating_add(600) < now
            || wire.received.saturating_add(60) < expected.issued_at
            || wire.expires > i64::MAX as u64
            || wire.expires <= now
            || wire.expires <= wire.received
            || !constant_eq(
                &commitment(&expected.context.ticket, &wire.nonce),
                &expected.commitment,
            )
        {
            return Err(AuthError::IdentityInvalid);
        }
        let scopes = parse_scopes(&wire.scope)?;
        let jwks = self.transport.get(transport::Endpoint::Jwks).await?;
        let subject = jwt::verify_fresh(
            &wire.id_token,
            &jwks,
            &expected.client_id,
            &wire.nonce,
            unix_now()?,
            expected.issued_at,
        )?;
        if expected.subject.as_ref().is_some_and(|s| s != &subject) {
            return Err(AuthError::RegistrationMismatch);
        }
        Ok(VerifiedImportedSession {
            context: expected.context.clone(),
            digest: envelope_digest(envelope),
            bundle: TokenBundle {
                identity: VerifiedIdentity {
                    client_id: wire.client.clone(),
                    subject,
                    host_id: expected.context.target_host.clone(),
                },
                access: Zeroizing::new(std::mem::take(&mut wire.access)),
                refresh: Zeroizing::new(std::mem::take(&mut wire.refresh)),
                id_token: Zeroizing::new(std::mem::take(&mut wire.id_token)),
                nonce: Zeroizing::new(std::mem::take(&mut wire.nonce)),
                scopes,
                received_at: wire.received,
                expires_at: wire.expires,
                earliest_refresh_at: wire.hint.take(),
            },
        })
    }
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct HandoffReceipt {
    pub version: u8,
    pub receipt_id: String,
    pub context: HandoffContext,
    pub generation: i64,
    pub envelope_digest: Vec<u8>,
}
pub fn envelope_digest(cipher: &[u8]) -> [u8; 32] {
    Sha256::digest(cipher).into()
}
impl HandoffReceipt {
    fn canonical(&self) -> Result<Vec<u8>, AuthError> {
        self.context.aad()?;
        if self.version != 1
            || self.generation != 1
            || !safe_atom(&self.receipt_id, 256)
            || self.envelope_digest.len() != 32
        {
            return Err(AuthError::InvalidInput);
        }
        serde_json::to_vec(&("moosuri/chatgpt/handoff/receipt/v1", self))
            .map_err(|_| AuthError::InvalidInput)
    }
    pub fn authenticate(&self, key: &HandoffKey) -> Result<Vec<u8>, AuthError> {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.0.as_ref());
        Ok(ring::hmac::sign(&key, &self.canonical()?).as_ref().to_vec())
    }
    pub fn verify(&self, key: &HandoffKey, mac: &[u8]) -> Result<(), AuthError> {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, key.0.as_ref());
        ring::hmac::verify(&key, &self.canonical()?, mac).map_err(|_| AuthError::IdentityInvalid)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    fn context() -> HandoffContext {
        HandoffContext {
            version: 1,
            ticket: "ticket".into(),
            runtime: "runtime".into(),
            tenant: "owner".into(),
            account: "account".into(),
            target_host: "urn:uuid:target".into(),
            binding: "binding".into(),
            expected_revision: 1,
            staged_revision: 2,
        }
    }
    #[test]
    fn protected_journal_and_receipt_reject_tampering_and_wrong_key() {
        let key = HandoffKey::generate().unwrap();
        let wrong = HandoffKey::generate().unwrap();
        let cipher = key
            .seal_private_journal(b"synthetic private session")
            .unwrap();
        assert!(wrong.open_private_journal(&cipher).is_err());
        assert_eq!(
            &*key.open_private_journal(&cipher).unwrap(),
            b"synthetic private session"
        );
        let mut r = HandoffReceipt {
            version: 1,
            receipt_id: "receipt".into(),
            context: context(),
            generation: 1,
            envelope_digest: vec![1; 32],
        };
        let mac = r.authenticate(&key).unwrap();
        r.verify(&key, &mac).unwrap();
        assert!(r.verify(&wrong, &mac).is_err());
        r.context.account = "another-owner".into();
        assert!(r.verify(&key, &mac).is_err());
    }
    #[test]
    fn context_and_commitment_are_unambiguous() {
        assert_ne!(commitment("ab", "c"), commitment("a", "bc"));
        let mut c = context();
        let aad = c.aad().unwrap();
        c.target_host = "other".into();
        assert_ne!(aad, c.aad().unwrap());
        c.staged_revision = c.expected_revision;
        assert!(c.aad().is_err());
    }
}
