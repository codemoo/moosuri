//! Ed25519 (RFC8032), ring 0.17.14, Apache-2.0 AND ISC. No server shared key.
use crate::{Error, Result};
use ring::{
    rand::SystemRandom,
    signature::{self, Ed25519KeyPair, KeyPair},
};
use sha2::{Digest, Sha256};
pub struct SigningKey(Ed25519KeyPair);
impl SigningKey {
    pub fn generate_pkcs8() -> Result<Vec<u8>> {
        Ok(Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .map_err(|_| Error::PrivateState)?
            .as_ref()
            .to_vec())
    }
    pub fn from_pkcs8(bytes: &[u8]) -> Result<Self> {
        Ok(Self(
            Ed25519KeyPair::from_pkcs8(bytes).map_err(|_| Error::PrivateState)?,
        ))
    }
    pub fn public_key(&self) -> Vec<u8> {
        self.0.public_key().as_ref().to_vec()
    }
    pub fn sign(&self, bytes: &[u8]) -> Vec<u8> {
        self.0.sign(bytes).as_ref().to_vec()
    }
}
pub fn verify(key: &[u8], bytes: &[u8], signature: &[u8]) -> Result<()> {
    if key.len() != 32 || signature.len() != 64 || bytes.len() > 16384 {
        return Err(Error::Authentication);
    }
    signature::UnparsedPublicKey::new(&signature::ED25519, key)
        .verify(bytes, signature)
        .map_err(|_| Error::Authentication)
}
pub fn hash(bytes: &[u8]) -> [u8; 32] {
    Sha256::digest(bytes).into()
}
/// The canonical protocol tuple includes method/path/body/context. Hash the bounded
/// tuple under a second fixed domain for the signed request digest. Protocol
/// encoding stays crypto-dependency-free for WASM consumers.
pub fn authenticated_bytes(
    c: &moosuri_vault_protocol::broker::AuthChallenge,
    method: &str,
    path: &str,
    command: &moosuri_vault_protocol::broker::Command,
) -> Option<Vec<u8>> {
    let tuple = moosuri_vault_protocol::broker::command_bytes(c, method, path, command)?;
    let mut bytes = Vec::from(b"moosuri/broker/request-sha256/v1\0".as_slice());
    bytes.extend_from_slice(&hash(&tuple));
    Some(bytes)
}

pub fn projection_digest(projection: moosuri_vault_protocol::broker::Projection) -> [u8; 32] {
    let code = match projection {
        moosuri_vault_protocol::broker::Projection::None => 0,
        moosuri_vault_protocol::broker::Projection::FixedProbeOk => 1,
    };
    let mut bytes = Vec::from(b"moosuri/broker/projection/v1\0".as_slice());
    bytes.push(code);
    hash(&bytes)
}

pub fn valid_origin(origin: &str) -> bool {
    let Ok(url) = url::Url::parse(origin) else {
        return false;
    };
    origin.len() <= 256
        && url.scheme() == "https"
        && url.host_str().is_some()
        && url.username().is_empty()
        && url.password().is_none()
        && url.path() == "/"
        && url.query().is_none()
        && url.fragment().is_none()
        && url.origin().ascii_serialization() == origin
}
#[cfg(test)]
mod tests {
    use super::*;
    fn hex(s: &str) -> Vec<u8> {
        s.as_bytes()
            .chunks_exact(2)
            .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
            .collect()
    }
    #[test]
    fn rfc8032_test1_independent_reference() {
        let key = hex("d75a980182b10ab7d54bfed3c964073a0ee172f3daa62325af021a68f707511a");
        let sig = hex(
            "e5564300c360ac729086e2cc806e828a84877f1eb8e5d974d873e065224901555fb8821590a33bacc61e39701cf9b46bd25bf5f0595bbe24655141438e7a100b",
        );
        assert!(verify(&key, b"", &sig).is_ok());
        assert!(verify(&key, b"x", &sig).is_err());
    }
}
