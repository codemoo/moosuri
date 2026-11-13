//! Explicit owner requests. Model proposals never carry execution authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MissionMode {
    CurrentSession,
    Durable,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionCheck {
    DecimalEquals {
        expected: String,
    },
    SourceQuote {
        id: String,
        #[ts(type = "number")]
        revision: i64,
        quote: String,
    },
    OutputContains {
        text: String,
    },
    UserReview {
        description: String,
    },
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionCreate {
    pub request_id: String,
    pub task: String,
    pub constraints: String,
    pub checks: Vec<MissionCheck>,
    pub execution_mode: MissionMode,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionControl {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub action: MissionAction,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionAction {
    Pause,
    Resume,
    Cancel,
    Steer { instruction: String },
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionEvidence {
    #[ts(type = "number")]
    pub step: i64,
    pub call_id: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionCandidate {
    pub text: String,
    pub evidence: Vec<MissionEvidence>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionReceipt {
    pub label: String,
    pub detail: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionView {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub state: String,
    pub request: MissionCreate,
    pub steering: Vec<String>,
    pub plan: Vec<String>,
    #[ts(type = "number")]
    pub completed_steps: i64,
    pub result: Option<MissionCandidate>,
    pub receipts: Vec<MissionReceipt>,
    pub report: Vec<String>,
    pub uncertain: bool,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
}

/// A saved immutable mission template and separately authorized schedule.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RecurringMissionCreate {
    pub request_id: String,
    pub template: MissionCreate,
    pub schedule: crate::ScheduleDraft,
    pub changes_only: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct RecurringMissionView {
    pub schedule: crate::ScheduleView,
    pub template: MissionCreate,
    pub model: crate::ModelChoice,
    pub changes_only: bool,
    pub recent: Vec<RecurringMissionOccurrence>,
    pub history: crate::ScheduleHistory,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct RecurringMissionOccurrence {
    pub occurrence_id: String,
    pub mission_id: String,
    pub state: String,
    pub notification_id: Option<String>,
    pub exact_unchanged: bool,
    #[ts(type = "number")]
    pub scheduled_for: i64,
}
