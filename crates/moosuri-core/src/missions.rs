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
    /// Explicit current owner authority to create one private report.
    PrivateReport {
        title: String,
    },
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
/// Explicit owner save, separate from additive steering and budget changes.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionAcceptanceUpdate {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub task: String,
    pub constraints: String,
    pub checks: Vec<MissionCheck>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionAcceptanceSnapshot {
    #[ts(type = "number")]
    pub revision: i64,
    pub request: MissionCreate,
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
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<MissionLinkedAction>>", optional)]
    pub actions: Vec<MissionLinkedAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compound: Option<MissionCompound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wait: Option<crate::MissionWaitView>,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub state: String,
    pub request: MissionCreate,
    /// Absent for unchanged/recurring missions; request is the active snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub acceptance_history: Option<Vec<MissionAcceptanceSnapshot>>,
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

/// Narrow proposals; authority comes from the originally accepted owner request.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionActionRequest {
    SaveProviderDraft {
        intent_quote: String,
        action_id: String,
        expected: crate::MailApproval,
    },
    PrepareMail {
        intent_quote: String,
        input: Box<crate::MailDraftInput>,
        evidence: Vec<MissionEvidence>,
    },
    Calendar {
        decision: Box<crate::CalendarChatDecision>,
        evidence: Vec<MissionEvidence>,
    },
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionLinkedAction {
    pub kind: String,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub digest: String,
    pub state: String,
    pub current: bool,
    pub url: String,
    pub provider_receipt: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub missing_skills: Option<Vec<String>>,
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

/// Original current-request selector, never a model-supplied identity.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionChatTarget {
    pub kind: String,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionChatContext {
    pub target: Option<MissionChatTarget>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionChatReceipt {
    pub request_id: String,
    pub action: String,
    pub target: Option<MissionChatTarget>,
    pub state: String,
    pub next_local_slot: Option<String>,
    pub candidates: Vec<MissionChatCandidate>,
}
/// A bounded semantic proposal. Quotes establish origin, not semantic authorization.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionChatDecision {
    pub decision: String,
    pub action: Option<String>,
    pub target_kind: Option<String>,
    pub intent_quote: Option<String>,
    pub task_quote: Option<String>,
    pub constraints_quote: Option<String>,
    pub time_quote: Option<String>,
    pub target_quote: Option<String>,
    pub instruction_quote: Option<String>,
    pub time: Option<MissionChatTime>,
    pub clarification: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionChatTime {
    Relative { seconds: u32 },
    LocalOnce { date: String, time: String },
    Interval { seconds: u32 },
    Daily { time: String },
    Weekly { days: Vec<u8>, time: String },
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionChatCandidate {
    pub target: MissionChatTarget,
    pub title: String,
}

/// Store-built bounded structure; tool outputs cannot add a node or an effect.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionCompound {
    pub schema_version: u32,
    pub calculation_required: bool,
    #[ts(type = "number")]
    pub acceptance_revision: i64,
    pub capability_profile_sha256: String,
    pub steps: Vec<MissionCompoundStep>,
    pub output: Option<crate::ArtifactReceipt>,
    pub review_required: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionCompoundStep {
    pub id: String,
    pub purpose: String,
    pub capability: String,
    pub input_slots: Vec<String>,
    pub depends_on: Vec<String>,
    pub receipts: Vec<MissionCompoundReceipt>,
    pub output: String,
    pub checks: Vec<String>,
    pub state: String,
}
/// Cost routing only. Actual command interpretation is isolated from evidence.
pub fn compound_report_candidate(text: &str) -> bool {
    let low = text.to_lowercase();
    ["report", "document", "보고서", "문서"]
        .iter()
        .any(|v| low.contains(v))
        && ["save", "create", "write", "저장", "만들", "작성"]
            .iter()
            .any(|v| low.contains(v))
        && [
            "research",
            "compare",
            "read",
            "calculate",
            "조사",
            "비교",
            "읽",
            "확인",
            "계산",
        ]
        .iter()
        .any(|v| low.contains(v))
}

/// Exact top-level or trusted learned-child receipt identity. Legacy candidate
/// references remain unchanged; only Store may resolve a child reference.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionCompoundReceipt {
    #[ts(type = "number")]
    pub step: i64,
    pub call_id: String,
    pub ordinal: Option<u8>,
}
