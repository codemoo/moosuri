//! Separate-origin ciphertext service. No decryption or plaintext DTO exists here.
#![forbid(unsafe_code)]
pub mod database;
pub mod http;
pub use moosuri_vault_protocol as protocol;
use sha2::{Digest, Sha256};
pub const MAX_BODY: usize = 32 * 1024 * 1024;
pub const MAX_ITEMS: usize = 256;
pub const MAX_RETAINED: usize = 256 * 1024 * 1024;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Unauthorized,
    NotFound,
    Conflict,
    Unavailable,
    Limit,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "vault_invalid",
            Self::Unauthorized => "vault_unauthorized",
            Self::NotFound => "vault_not_found",
            Self::Conflict => "vault_conflict",
            Self::Unavailable => "vault_unavailable",
            Self::Limit => "vault_limit",
        })
    }
}
impl std::error::Error for Error {}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Unavailable
    }
}
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self::Unavailable
    }
}
pub type Result<T> = std::result::Result<T, Error>;
pub fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
pub fn unhex(value: &str) -> Result<[u8; 32]> {
    if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Invalid);
    }
    let mut out = [0; 32];
    for (i, v) in out.iter_mut().enumerate() {
        *v = u8::from_str_radix(&value[i * 2..i * 2 + 2], 16).map_err(|_| Error::Invalid)?
    }
    Ok(out)
}
pub fn now() -> Result<i64> {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_secs(),
    )
    .map_err(|_| Error::Unavailable)
}
pub fn token() -> Result<String> {
    use rand::RngCore;
    let mut bytes = [0; 32];
    rand::rngs::OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| Error::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
