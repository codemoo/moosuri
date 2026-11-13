//! Durable text conversation DTOs. Credentials and execution leases stay private.
use crate::{KnowledgeKind, ModelChoice};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Queued,
    Running,
    Completed,
    Failed,
    Cancelled,
    Incomplete,
    Refused,
    OutcomeUnknown,
}
impl RunState {
    pub fn active(self) -> bool {
        matches!(self, Self::Queued | Self::Running)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MessagePhase {
    Commentary,
    FinalAnswer,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RunUsage {
    #[ts(type = "number | null")]
    pub input_tokens: Option<u64>,
    #[ts(type = "number | null")]
    pub output_tokens: Option<u64>,
    #[ts(type = "number | null")]
    pub cache_read_tokens: Option<u64>,
    #[ts(type = "number | null")]
    pub cache_write_tokens: Option<u64>,
}

// No Debug: private user/model text must not accidentally enter a trace.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct AssistantMessage {
    pub id: String,
    pub text: String,
    pub phase: Option<MessagePhase>,
    pub refusal: bool,
}

/// Backend-recorded input provenance, not a claim that every consulted item
/// proves the generated answer. Titles are private user content.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ChatEvidence {
    pub kind: KnowledgeKind,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub title: String,
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct RunView {
    pub id: String,
    pub request_id: String,
    pub thread_id: String,
    pub attempt: i32,
    pub state: RunState,
    #[ts(type = "number")]
    pub version: i64,
    pub model: ModelChoice,
    /// Fixed application error category, never provider text.
    pub error: Option<String>,
    pub usage: RunUsage,
    #[serde(default)]
    pub evidence: Vec<ChatEvidence>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub web_evidence: Option<Vec<crate::WebEvidence>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub artifact_receipts: Option<Vec<crate::ArtifactReceipt>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mission_receipt: Option<crate::MissionChatReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub calendar_receipt: Option<crate::CalendarChatReceipt>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schedule_receipts: Option<Vec<crate::ScheduleChatReceipt>>,
    pub can_retry: bool,
    pub can_cancel: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ChatTurn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub attachments: Option<Vec<crate::MediaSummary>>,
    #[ts(type = "number")]
    pub sequence: i64,
    pub user_text: String,
    pub assistant: Vec<AssistantMessage>,
    pub run: RunView,
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ChatPage {
    pub thread_id: String,
    /// Cursor of the committed event log at the same snapshot as these turns.
    #[ts(type = "number")]
    pub cursor: i64,
    pub turns: Vec<ChatTurn>,
    #[ts(type = "number | null")]
    pub next_before: Option<i64>,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatChange {
    Turn {
        turn: ChatTurn,
    },
    Delta {
        run_id: String,
        attempt: i32,
        message_id: String,
        text: String,
        phase: Option<MessagePhase>,
        refusal: bool,
    },
    Phase {
        run_id: String,
        attempt: i32,
        message_id: String,
        phase: Option<MessagePhase>,
    },
    State {
        run: RunView,
    },
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ChatEvent {
    #[ts(type = "number")]
    pub sequence: i64,
    pub change: ChatChange,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ChatEvents {
    pub events: Vec<ChatEvent>,
    pub reset: bool,
    #[ts(type = "number")]
    pub cursor: i64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize, TS)]
pub struct UsageSummary {
    #[ts(type = "number")]
    pub dispatched_calls: i64,
    #[ts(type = "number")]
    pub unreported_calls: i64,
    #[ts(type = "number")]
    pub reported_input_tokens: u64,
    #[ts(type = "number")]
    pub reported_output_tokens: u64,
    #[ts(type = "number")]
    pub reported_cache_read_tokens: u64,
    #[ts(type = "number")]
    pub reported_cache_write_tokens: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct AccountUsage {
    pub account_id: String,
    pub name: String,
    pub email: String,
    pub usage: UsageSummary,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct AccountUsageResponse {
    pub accounts: Vec<AccountUsage>,
}
