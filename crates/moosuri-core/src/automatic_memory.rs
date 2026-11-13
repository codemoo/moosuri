//! Explicit collection-scoped automatic extraction settings and truthful observations.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MemoryExtractionChange {
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub enabled: bool,
    pub attribution_mode: String,
    /// Owner-selected reported attribution, never proof of account ownership.
    pub reported_speaker: String,
    pub daily_calls: u32,
    pub daily_input_bytes: u32,
    pub daily_output_tokens: u32,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MemoryExtractionStatus {
    #[ts(type = "number")]
    pub revision: i64,
    pub enabled: bool,
    pub attribution_mode: String,
    pub reported_speaker: String,
    pub daily_calls: u32,
    pub daily_input_bytes: u32,
    pub daily_output_tokens: u32,
    pub state: String,
    pub reason: Option<String>,
    pub saved_count: u32,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MemoryExtractionOrigin {
    pub model: String,
    #[ts(type = "number")]
    pub provider_revision: i64,
    pub source_id: String,
    #[ts(type = "number")]
    pub source_revision: i64,
    pub policy_revision: String,
    pub reported_speaker: String,
}
