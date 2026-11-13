//! Client-only cryptography/import core. No server, storage, model tool or broker.
//! Secrets intentionally implement neither Debug, Clone nor Serialize.
#![forbid(unsafe_code)]
mod apple;
mod archive;
mod crypto;
mod secret;
mod totp;
mod wire;

pub use apple::{AppleImport, ImportPreview, ImportRow, ImportStatus, UsernameFilter};
pub use archive::{ARCHIVE_ITEMS, ARCHIVE_LIMIT, ArchiveImport, ArchivePreview, ArchiveRow};
pub use crypto::{
    ActiveVault, ClientVault, CommittedHeader, HeaderReceipt, PendingChange, PendingVault,
    RecoveryKey, StorageReceipt, inventory_digest, storage_commitment,
};
pub use secret::{ItemFields, ItemKind, SecretText};
pub use totp::{OtpCode, Totp, TotpAlgorithm};
pub use wire::{Binding, EncryptedItem, Header, PasswordKdf};

/// Fixed diagnostics never carry a parser/provider error or secret value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Invalid,
    Limit,
    Unsupported,
    KdfPolicy,
    Authentication,
    State,
    Conflict,
    Entropy,
    Csv,
    Cancelled,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Invalid => "vault_invalid_input",
            Self::Limit => "vault_limit",
            Self::Unsupported => "vault_unsupported",
            Self::KdfPolicy => "vault_kdf_policy",
            Self::Authentication => "vault_authentication_failed",
            Self::State => "vault_state",
            Self::Conflict => "vault_conflict",
            Self::Entropy => "vault_entropy_failed",
            Self::Csv => "vault_csv_invalid",
            Self::Cancelled => "vault_cancelled",
        })
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;

/// TRUSTED adapter: fill every byte from an OS/browser CSPRNG or return an error.
/// This trait cannot prove an implementation is cryptographically random.
pub trait TrustedEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<()>;
}
#[cfg(feature = "native-entropy")]
pub struct OsEntropy;
#[cfg(feature = "native-entropy")]
impl TrustedEntropy for OsEntropy {
    fn fill(&mut self, output: &mut [u8]) -> Result<()> {
        use rand::RngCore;
        rand::rngs::OsRng
            .try_fill_bytes(output)
            .map_err(|_| Error::Entropy)
    }
}

/// Cancellation is checked during parsing and each selected-row encryption.
pub trait Cancellation {
    fn cancelled(&self) -> bool;
}
pub struct Continue;
impl Cancellation for Continue {
    fn cancelled(&self) -> bool {
        false
    }
}
#[cfg(test)]
mod tests;
