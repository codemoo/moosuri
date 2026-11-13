use super::{AuthError, ISSUER, constant_eq, safe_atom};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use ring::signature::{RSA_PKCS1_2048_8192_SHA256, RsaPublicKeyComponents};
use serde::Deserialize;
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Header {
    alg: String,
    kid: String,
    typ: Option<String>,
}
#[derive(Deserialize)]
struct Jwks {
    keys: Vec<Jwk>,
}
#[derive(Deserialize)]
struct Jwk {
    kid: Option<String>,
    kty: String,
    alg: Option<String>,
    #[serde(rename = "use")]
    usage: Option<String>,
    key_ops: Option<Vec<String>>,
    n: Option<String>,
    e: Option<String>,
    // Private key material must never be accepted as a verification JWK.
    d: Option<serde_json::Value>,
    p: Option<serde_json::Value>,
    q: Option<serde_json::Value>,
    k: Option<serde_json::Value>,
    dp: Option<serde_json::Value>,
    dq: Option<serde_json::Value>,
    qi: Option<serde_json::Value>,
    oth: Option<serde_json::Value>,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Audience {
    One(String),
    Many(Vec<String>),
}
#[derive(Deserialize)]
struct Claims {
    iss: String,
    sub: String,
    aud: Audience,
    exp: u64,
    iat: u64,
    nonce: Option<String>,
    azp: Option<String>,
    nbf: Option<u64>,
}
/// Verification is internal; unverified claims/keys cannot become public identity.
pub(super) fn verify(
    token: &str,
    jwks: &[u8],
    client_id: &str,
    nonce: &str,
    require_nonce: bool,
    now: u64,
) -> Result<String, AuthError> {
    let invalid = || AuthError::IdentityInvalid;
    if token.len() > 32768 {
        return Err(invalid());
    }
    let parts: Vec<_> = token.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|s| s.is_empty()) {
        return Err(invalid());
    }
    let header_bytes = Zeroizing::new(URL_SAFE_NO_PAD.decode(parts[0]).map_err(|_| invalid())?);
    let header: Header = serde_json::from_slice(&header_bytes).map_err(|_| invalid())?;
    if header.alg != "RS256"
        || !safe_atom(&header.kid, 512)
        || header.typ.as_ref().is_some_and(|s| s != "JWT")
    {
        return Err(invalid());
    }
    let jwks: Jwks = serde_json::from_slice(jwks).map_err(|_| invalid())?;
    if jwks.keys.is_empty() || jwks.keys.len() > 64 {
        return Err(invalid());
    }
    let mut ids = std::collections::BTreeSet::new();
    for key in &jwks.keys {
        if let Some(id) = &key.kid
            && (!safe_atom(id, 512) || !ids.insert(id))
        {
            return Err(invalid());
        }
    }
    let key = jwks
        .keys
        .iter()
        .find(|k| k.kid.as_deref() == Some(&header.kid))
        .ok_or_else(invalid)?;
    if key.kty != "RSA"
        || key.alg.as_ref().is_some_and(|s| s != "RS256")
        || key.usage.as_ref().is_some_and(|s| s != "sig")
        || key
            .key_ops
            .as_ref()
            .is_some_and(|ops| ops.as_slice() != ["verify"])
        || key.d.is_some()
        || key.p.is_some()
        || key.q.is_some()
        || key.k.is_some()
        || key.dp.is_some()
        || key.dq.is_some()
        || key.qi.is_some()
        || key.oth.is_some()
    {
        return Err(invalid());
    }
    let n = URL_SAFE_NO_PAD
        .decode(key.n.as_ref().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    let e = URL_SAFE_NO_PAD
        .decode(key.e.as_ref().ok_or_else(invalid)?)
        .map_err(|_| invalid())?;
    // Minimal unsigned encodings prevent ambiguous key representations. Ring also
    // enforces modulus bit length/exponent range and PKCS#1 v1.5 SHA-256 padding.
    if !(256..=1024).contains(&n.len())
        || n.first() == Some(&0)
        || e.is_empty()
        || e.len() > 4
        || e.first() == Some(&0)
    {
        return Err(invalid());
    }
    let signature = Zeroizing::new(URL_SAFE_NO_PAD.decode(parts[2]).map_err(|_| invalid())?);
    let signed_len = parts[0].len() + 1 + parts[1].len();
    RsaPublicKeyComponents { n: &n, e: &e }
        .verify(
            &RSA_PKCS1_2048_8192_SHA256,
            &token.as_bytes()[..signed_len],
            &signature,
        )
        .map_err(|_| invalid())?;
    let payload = Zeroizing::new(URL_SAFE_NO_PAD.decode(parts[1]).map_err(|_| invalid())?);
    let claims: Claims = serde_json::from_slice(&payload).map_err(|_| invalid())?;
    let audience_valid = match &claims.aud {
        Audience::One(a) => a == client_id,
        Audience::Many(a) => {
            !a.is_empty()
                && a.len() <= 16
                && a.iter().collect::<std::collections::BTreeSet<_>>().len() == a.len()
                && a.iter().any(|a| a == client_id)
                && (a.len() == 1 || claims.azp.as_deref() == Some(client_id))
        }
    };
    if claims.iss != ISSUER
        || !safe_atom(&claims.sub, 1024)
        || !audience_valid
        || claims.azp.as_ref().is_some_and(|s| s != client_id)
        || claims.exp <= now
        || claims.iat > now.saturating_add(60)
        || claims.exp <= claims.iat
        || claims.nbf.is_some_and(|n| n > now)
        || (require_nonce && claims.nonce.is_none())
        || claims
            .nonce
            .as_ref()
            .is_some_and(|n| !constant_eq(n.as_bytes(), nonce.as_bytes()))
    {
        return Err(invalid());
    }
    Ok(claims.sub)
}
