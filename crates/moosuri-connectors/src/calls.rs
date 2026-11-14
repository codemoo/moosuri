//! Explicit normalized call exports. Only selected text reaches source ingest;
//! optional owner-chosen STT sends bounded audio chunks to the fixed Audio API.
use crate::{audio::AudioClient, work_sessions};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub(crate) mod format;
#[cfg(test)]
mod tests;
pub(crate) mod wav;
pub struct Options {
    pub source: work_sessions::Options,
    /// Explicit opt-in for the fixed bounded Audio API; never discovered from env.
    pub transcription_key_file: Option<PathBuf>,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Counts {
    pub attempted: u32,
    pub reused: u32,
    pub unknown: u32,
    #[serde(default)]
    pub retryable: u32,
    pub requires_key: u32,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum RetryReason {
    Invalid,
    Unavailable,
    Credentials,
    RateLimited,
}
impl RetryReason {
    pub(crate) fn from_error(error: &crate::audio::AudioError) -> Option<Self> {
        use crate::audio::AudioError;
        match error {
            AudioError::Invalid => Some(Self::Invalid),
            AudioError::Unavailable => Some(Self::Unavailable),
            AudioError::Credentials => Some(Self::Credentials),
            AudioError::RateLimited => Some(Self::RateLimited),
            AudioError::Unknown | AudioError::Response => None,
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rejection {
    pub request_id: String,
    pub reason: RetryReason,
}
/// Fixed-size history: original and latest known rejection plus exact count.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Rejections {
    pub count: u32,
    pub first: Rejection,
    pub last: Rejection,
}
impl Rejections {
    pub(crate) fn valid(&self) -> bool {
        self.count > 0
            && uuid::Uuid::parse_str(&self.first.request_id).is_ok()
            && uuid::Uuid::parse_str(&self.last.request_id).is_ok()
    }
    pub(crate) fn record(
        previous: Option<Self>,
        request_id: String,
        reason: RetryReason,
    ) -> work_sessions::Result<Self> {
        let next = Rejection { request_id, reason };
        match previous {
            Some(mut history) => {
                history.count = history
                    .count
                    .checked_add(1)
                    .ok_or("transcription_rejection_counter_exhausted")?;
                history.last = next;
                Ok(history)
            }
            None => Ok(Self {
                count: 1,
                first: next.clone(),
                last: next,
            }),
        }
    }
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Transcription {
    Intent {
        request_id: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rejections: Option<Rejections>,
    },
    Retryable {
        rejections: Rejections,
    },
    Complete {
        request_id: String,
        text: String,
        text_sha256: String,
        duration_ms: u32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        rejections: Option<Rejections>,
    },
}
pub async fn run_once(options: &Options) -> work_sessions::Result<work_sessions::Receipt> {
    work_sessions::run_calls(
        &options.source,
        options.transcription_key_file.as_deref(),
        AudioClient::default(),
        false,
    )
    .await
}
/// Synthetic loopback only; never exposed by the shipped CLI.
#[cfg(feature = "test-fixtures")]
#[doc(hidden)]
pub async fn run_loopback_fixture(
    options: &Options,
    audio: AudioClient,
) -> work_sessions::Result<work_sessions::Receipt> {
    work_sessions::run_calls(
        &options.source,
        options.transcription_key_file.as_deref(),
        audio,
        true,
    )
    .await
}
