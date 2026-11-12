//! Explicit current-user memory instructions. Never derive authority from retrieved text.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCommandAction {
    Remember,
    Correct,
    Forget,
}

// Literal private user text must never be formatted by diagnostic Debug output.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MemoryCommandIntent {
    pub action: MemoryCommandAction,
    pub intent_quote: String,
    pub fact_quote: Option<String>,
    pub subject: Option<String>,
    pub target_quote: Option<String>,
}
