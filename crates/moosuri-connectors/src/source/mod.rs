//! Read-only official provider protocols. Callers must obtain explicit collection
//! consent, bind credentials to immutable account/scope identity, and atomically
//! persist each page and checkpoint. Network success never commits a checkpoint.
//! These types intentionally do not implement Debug: bodies and cursors are private.
pub mod gmail;
pub mod slack;
#[cfg(test)]
mod tests;
mod transport;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ReadError {
    #[error("source_invalid_input")]
    InvalidInput,
    #[error("source_invalid_response")]
    InvalidResponse,
    #[error("source_identity_mismatch")]
    IdentityMismatch,
    #[error("source_authentication_required")]
    AuthenticationRequired,
    #[error("source_scope_denied")]
    ScopeDenied,
    #[error("source_checkpoint_expired_resync_required")]
    ResyncRequired,
    #[error("source_rate_limited")]
    RateLimited { retry_after_seconds: u64 },
    #[error("source_destination_denied")]
    DestinationDenied,
    #[error("source_transport_failed")]
    Transport,
    #[error("source_timeout")]
    Timeout,
    #[error("source_cancelled")]
    Cancelled,
    #[error("source_limit_exceeded")]
    LimitExceeded,
    #[error("source_http_{0}")]
    Http(u16),
}

#[derive(Serialize, Deserialize)]
pub struct SourceItem {
    pub external_id: String,
    pub remote_version: String,
    pub subject: String,
    pub text: String,
    pub speaker: String,
    pub thread_id: String,
    pub parent_id: Option<String>,
    /// Original provider timestamp in Unix seconds, not server receipt time.
    pub observed_at: i64,
}
#[derive(Serialize, Deserialize)]
pub enum SourceChange {
    Upsert(SourceItem),
    /// Only explicit remote deletion or a positively observed scope departure.
    Delete {
        external_id: String,
        remote_version: String,
    },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Coverage {
    MorePages,
    GmailHistoryComplete,
    /// Polling does not prove deletions or replies on arbitrarily old threads.
    SlackPollingWindow,
}
pub struct SyncPage<C> {
    pub changes: Vec<SourceChange>,
    pub checkpoint: C,
    pub coverage: Coverage,
}
impl<C> SyncPage<C> {
    pub fn is_no_change(&self) -> bool {
        self.changes.is_empty()
    }
}
fn atom(value: &str, max: usize) -> Result<(), ReadError> {
    if value.is_empty() || value.len() > max || value.chars().any(char::is_control) {
        Err(ReadError::InvalidInput)
    } else {
        Ok(())
    }
}
fn id(value: &str) -> Result<(), ReadError> {
    atom(value, 256)?;
    if value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        Ok(())
    } else {
        Err(ReadError::InvalidInput)
    }
}
fn decimal(value: &str) -> Result<(), ReadError> {
    if value.is_empty() || value.len() > 32 || !value.bytes().all(|b| b.is_ascii_digit()) {
        Err(ReadError::InvalidInput)
    } else {
        Ok(())
    }
}
fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, ReadError> {
    serde_json::from_slice(bytes).map_err(|_| ReadError::InvalidResponse)
}

fn bounded<C>(mut page: SyncPage<C>) -> Result<SyncPage<C>, ReadError> {
    if page.changes.len() > 32 {
        return Err(ReadError::LimitExceeded);
    }
    for change in &mut page.changes {
        match change {
            SourceChange::Delete {
                external_id,
                remote_version,
            } => {
                output_field(external_id, 128)?;
                output_field(remote_version, 128)?;
            }
            SourceChange::Upsert(item) => {
                // These explicit metadata notes preserve text-only coverage
                // without silently dropping attachment-only messages.
                if item.subject.trim().is_empty() {
                    item.subject = "Message".into();
                }
                if item.text.trim().is_empty() {
                    item.text = "[No plain-text content. Attachments are not synchronized.]".into();
                }
                output_field(&item.external_id, 128)?;
                output_field(&item.remote_version, 128)?;
                output_field(&item.subject, 200)?;
                output_field(&item.speaker, 200)?;
                if item.text.len() > 64 * 1024 {
                    return Err(ReadError::LimitExceeded);
                }
                if item
                    .text
                    .chars()
                    .any(|c| c.is_control() && !c.is_whitespace())
                {
                    return Err(ReadError::InvalidResponse);
                }
            }
        }
    }
    let size = serde_json::to_vec(&page.changes)
        .map_err(|_| ReadError::InvalidResponse)?
        .len();
    if size > 256 * 1024 {
        return Err(ReadError::LimitExceeded);
    }
    Ok(page)
}

fn output_field(value: &str, limit: usize) -> Result<(), ReadError> {
    if value.len() > limit {
        return Err(ReadError::LimitExceeded);
    }
    atom(value, limit).map_err(|_| ReadError::InvalidResponse)
}
