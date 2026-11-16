//! Tenant-owned local records. Content has no diagnostic Debug representation.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactKind {
    Document,
    Goal,
    Followup,
}
impl ArtifactKind {
    pub const ALL: [Self; 3] = [Self::Document, Self::Goal, Self::Followup];
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactStatus {
    Active,
    Withdrawn,
    Deleted,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum GoalLifecycle {
    Active,
    Paused,
    Achieved,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FollowupKind {
    Request,
    Promise,
    WaitingForReply,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FollowupLifecycle {
    Requested,
    Committed,
    Waiting,
    Resolved,
    Cancelled,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum FollowupProgress {
    None,
    Partial,
    Delegated,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDate {
    #[ts(type = "number")]
    pub utc: i64,
    pub timezone: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactPayload {
    Document,
    Goal {
        outcome: String,
        success_criteria: String,
        progress: String,
        lifecycle: GoalLifecycle,
    },
    Followup {
        record_kind: FollowupKind,
        person: String,
        lifecycle: FollowupLifecycle,
        progress: FollowupProgress,
        muted: bool,
        due: Option<ArtifactDate>,
        review: Option<ArtifactDate>,
        snooze: Option<ArtifactDate>,
    },
}
impl ArtifactPayload {
    pub fn kind(&self) -> ArtifactKind {
        match self {
            Self::Document => ArtifactKind::Document,
            Self::Goal { .. } => ArtifactKind::Goal,
            Self::Followup { .. } => ArtifactKind::Followup,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDependency {
    pub kind: ArtifactDependencyKind,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactDependencyKind {
    Source,
    Memory,
    Artifact,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDraft {
    pub title: String,
    pub body: String,
    pub payload: ArtifactPayload,
    pub references: Vec<ArtifactDependency>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ArtifactWrite {
    pub request_id: String,
    pub id: Option<String>,
    #[ts(type = "number | null")]
    pub expected_revision: Option<i64>,
    pub draft: ArtifactDraft,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ArtifactDelete {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ArtifactSummary {
    pub id: String,
    pub kind: ArtifactKind,
    #[ts(type = "number")]
    pub revision: i64,
    pub status: ArtifactStatus,
    pub title: String,
    #[ts(type = "number")]
    pub updated_at: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ArtifactVersion {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<ArtifactExternalEvidence>>", optional)]
    pub external_evidence: Vec<ArtifactExternalEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reminders: Option<Vec<FollowupReminder>>,
    pub artifact: ArtifactSummary,
    pub draft: Option<ArtifactDraft>,
}
/// Store-derived exact external tool receipt identity. No credential or copied body.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ArtifactExternalEvidence {
    pub kind: String,
    pub run_id: String,
    #[ts(type = "number")]
    pub step: i64,
    pub call_id: String,
    pub request: String,
    pub request_sha256: String,
    pub result_sha256: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct FollowupReminder {
    pub artifact_id: String,
    #[ts(type = "number")]
    pub artifact_revision: i64,
    pub date_kind: String,
    pub date: ArtifactDate,
    #[ts(type = "number")]
    pub scheduled_for: i64,
    pub state: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ArtifactReceipt {
    pub request_id: String,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub status: ArtifactStatus,
    pub url: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ArtifactPage {
    pub items: Vec<ArtifactSummary>,
    pub next_after: Option<String>,
    pub coverage_complete: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ArtifactHistory {
    pub items: Vec<ArtifactVersion>,
    #[ts(type = "number | null")]
    pub next_before: Option<i64>,
}
/// Private run admission capability, never browser-provided authority.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactTarget {
    pub id: String,
    pub revision: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactRunProfile {
    #[serde(default)]
    pub external_reports: bool,
    pub schema_version: u32,
    pub selected: Option<ArtifactTarget>,
    pub planned: bool,
    pub write: bool,
    pub kind: Option<ArtifactKind>,
}
/// Shared classifier schema and Store admission action vocabulary.
pub const ARTIFACT_INTENT_ACTIONS: &[&str] = &["create", "edit"];
pub const ARTIFACT_INTENT_FIELDS: &[&str] =
    &["decision", "action", "kind", "intent_quote", "target_quote"];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactIntent {
    pub action: String,
    pub kind: ArtifactKind,
    pub intent_quote: String,
    pub target_quote: Option<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ArtifactToolRequest {
    Search {
        kind: Option<ArtifactKind>,
        after: Option<String>,
    },
    Read {
        id: String,
        revision: i64,
        start_byte: u32,
        max_bytes: u32,
    },
    Write {
        title: String,
        body: String,
        payload: ArtifactPayload,
        anchor: Option<String>,
    },
    Briefing {
        after: Option<String>,
    },
}
