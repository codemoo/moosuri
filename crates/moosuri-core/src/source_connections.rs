//! Safe connection views and bounded ingestion envelopes. Credentials and provider
//! checkpoints deliberately have no Debug implementation.
use crate::SourceInput;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SourceConnectionKind {
    CustomHttp,
    Gmail,
    Slack,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CreateSourceConnection {
    pub name: String,
    pub kind: SourceConnectionKind,
    pub scope: String,
    pub remote_account: String,
    pub mail_connection_id: Option<String>,
    pub sync_enabled: bool,
    pub poll_interval_seconds: u32,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SourceConnection {
    pub id: String,
    pub collection_id: String,
    pub name: String,
    pub kind: SourceConnectionKind,
    pub adapter_version: String,
    pub scope: String,
    pub remote_account: String,
    pub mail_connection_id: Option<String>,
    #[ts(type = "number")]
    pub config_revision: i64,
    #[ts(type = "number")]
    pub credential_epoch: i64,
    pub state: String,
    pub sync_enabled: bool,
    pub poll_interval_seconds: u32,
    #[ts(type = "number")]
    pub local_sequence: i64,
    #[ts(type = "number | null")]
    pub last_heartbeat_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_commit_at: Option<i64>,
    #[ts(type = "number | null")]
    pub last_completed_at: Option<i64>,
    #[ts(type = "number")]
    pub next_due_at: i64,
    pub error: Option<String>,
    pub credential_present: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SourceSyncRun {
    pub id: String,
    #[ts(type = "number")]
    pub generation: i64,
    pub state: String,
    #[ts(type = "number")]
    pub started_at: i64,
    #[ts(type = "number")]
    pub heartbeat_at: i64,
    #[ts(type = "number | null")]
    pub finished_at: Option<i64>,
    pub error: Option<String>,
    /// A custom producer reports coverage; only the local commits are attested.
    pub producer_report: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum SourceRunOutcome {
    Completed,
    Partial,
    NoChange,
    Polled,
    Failed,
    Unknown,
    NeedsReconnect,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SourceBatch {
    pub protocol_version: u32,
    pub request_id: String,
    #[ts(type = "number")]
    pub config_revision: i64,
    pub run_id: String,
    #[ts(type = "number")]
    pub generation: i64,
    #[ts(type = "number")]
    pub expected_sequence: i64,
    pub checkpoint_before: Option<String>,
    pub checkpoint_after: Option<String>,
    pub entries: Vec<SourceInput>,
    #[serde(default)]
    pub metadata: Vec<SourceObjectMetadata>,
    #[serde(default)]
    pub tombstones: Vec<String>,
    pub complete_snapshot: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SourceBatchReceipt {
    pub request_id: String,
    pub run_id: String,
    #[ts(type = "number")]
    pub sequence: i64,
    #[ts(type = "number")]
    pub committed_at: i64,
    pub accepted: u32,
    pub unchanged: u32,
    pub source_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct SourceIngestStatus {
    pub connection_id: String,
    #[ts(type = "number")]
    pub config_revision: i64,
    #[ts(type = "number")]
    pub local_sequence: i64,
    pub checkpoint: Option<String>,
    pub active_run: Option<SourceSyncRun>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SourceProvenance {
    pub connection_id: String,
    pub connection_name: String,
    pub kind: SourceConnectionKind,
    pub scope: String,
    pub external_id: String,
    pub external_version: String,
    pub thread_id: Option<String>,
    pub parent_id: Option<String>,
    pub request_id: String,
    pub run_id: String,
    #[ts(type = "number")]
    pub source_revision: i64,
    #[ts(type = "number")]
    pub received_at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct SourceObjectMetadata {
    pub external_id: String,
    pub thread_id: Option<String>,
    pub parent_id: Option<String>,
}
