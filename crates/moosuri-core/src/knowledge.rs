//! Private knowledge DTOs. Source and memory text deliberately do not implement Debug.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum DisconnectMode {
    Keep,
    Delete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeKind {
    Source,
    Memory,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MemoryStatus {
    Active,
    Invalidated,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct KnowledgeCollection {
    pub id: String,
    pub name: String,
    pub scope: String,
    #[ts(type = "number")]
    pub revision: i64,
    #[ts(type = "number")]
    pub cursor: i64,
    pub connected: bool,
    pub deleted: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SourceInput {
    pub external_id: String,
    pub version: String,
    #[ts(type = "number | null")]
    pub expected_revision: Option<i64>,
    pub title: String,
    pub text: String,
    pub speaker: Option<String>,
    #[ts(type = "number")]
    pub observed_at: i64,
    #[ts(type = "number | null")]
    pub valid_from: Option<i64>,
    #[ts(type = "number | null")]
    pub valid_until: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct IngestBatch {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_cursor: i64,
    #[ts(type = "number")]
    pub next_cursor: i64,
    pub complete_snapshot: bool,
    pub entries: Vec<SourceInput>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct IngestReceipt {
    pub request_id: String,
    #[ts(type = "number")]
    pub cursor: i64,
    pub source_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct SourceSummary {
    pub id: String,
    pub collection_id: String,
    pub external_id: String,
    pub version: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub title: String,
    pub speaker: Option<String>,
    #[ts(type = "number")]
    pub observed_at: i64,
    #[ts(type = "number | null")]
    pub valid_from: Option<i64>,
    #[ts(type = "number | null")]
    pub valid_until: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct SourceDocument {
    pub source: SourceSummary,
    pub text: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct SourcePage {
    pub items: Vec<SourceSummary>,
    pub next_after: Option<String>,
    #[ts(type = "number")]
    pub generation: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct EvidenceRef {
    pub source_id: String,
    #[ts(type = "number")]
    pub source_revision: i64,
    pub start_byte: u32,
    pub end_byte: u32,
    pub quote: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MemoryDraft {
    pub text: String,
    pub subject: String,
    pub speaker: Option<String>,
    #[ts(type = "number | null")]
    pub valid_from: Option<i64>,
    #[ts(type = "number | null")]
    pub valid_until: Option<i64>,
    /// Empty means an explicit user assertion; nonempty means source-dependent.
    pub evidence: Vec<EvidenceRef>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MemoryDocument {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub status: MemoryStatus,
    pub draft: MemoryDraft,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MemoryPage {
    pub items: Vec<MemoryDocument>,
    pub next_after: Option<String>,
    #[ts(type = "number")]
    pub generation: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MemoryHistory {
    pub items: Vec<MemoryDocument>,
    #[ts(type = "number | null")]
    pub next_before: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeQuery {
    pub kind: KnowledgeKind,
    pub text: String,
    pub collection_id: Option<String>,
    pub subject: Option<String>,
    pub speaker: Option<String>,
    #[ts(type = "number | null")]
    pub valid_at: Option<i64>,
    pub cursor: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct KnowledgeHit {
    pub kind: KnowledgeKind,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub title: String,
    pub snippet: String,
    pub subject: Option<String>,
    pub speaker: Option<String>,
    #[ts(type = "number | null")]
    pub valid_from: Option<i64>,
    #[ts(type = "number | null")]
    pub valid_until: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct KnowledgeSearch {
    pub hits: Vec<KnowledgeHit>,
    pub next_cursor: Option<String>,
    pub truncated: bool,
    #[ts(type = "number")]
    pub generation: i64,
}
