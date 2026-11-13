//! Public-web tool requests and server-recorded observations. No credentials.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum WebToolRequest {
    Read { url: String },
    Search { query: String },
    BrowserRead { url: String },
    BrowserOpen { url: String },
    BrowserObserve {},
    BrowserHandoff { reason: String },
}

/// Capabilities are admitted by the installation, never by model arguments.
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WebToolProfile {
    pub read: bool,
    pub search: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub browser_pin: Option<String>,
}

/// An observed page or search hit, not a claim that its content proves the answer.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct WebEvidence {
    pub url: String,
    pub title: String,
    #[ts(type = "number")]
    pub retrieved_at_unix_ms: u64,
    pub kind: WebEvidenceKind,
    pub truncated: bool,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum WebEvidenceKind {
    Page,
    SearchHit,
    BrowserPage,
}
