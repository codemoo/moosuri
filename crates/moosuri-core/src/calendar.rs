//! Personal Calendar DTOs. No token, raw Google endpoint or invitation authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarConnection {
    pub id: String,
    pub primary_id: String,
    pub title: String,
    pub time_zone: String,
    #[ts(type = "number")]
    pub epoch: i64,
    #[ts(type = "number")]
    pub revision: i64,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarConnections {
    pub configured: bool,
    pub connections: Vec<CalendarConnection>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarInfo {
    pub id: String,
    pub title: String,
    pub time_zone: String,
    pub primary: bool,
    pub access_role: String,
    pub email_notifications: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarList {
    pub items: Vec<CalendarInfo>,
    pub next_page_token: Option<String>,
    pub partial: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CalendarFields {
    pub title: String,
    pub description: String,
    pub location: String,
    /// RFC3339 timestamps, or two strict YYYY-MM-DD civil dates for an all-day event.
    pub start: String,
    /// Exclusive in both representations. All-day spans count civil dates, never elapsed hours.
    pub end: String,
    pub time_zone: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarEvent {
    pub id: String,
    pub etag: String,
    pub status: String,
    pub fields: CalendarFields,
    pub html_link: Option<String>,
    pub writable: bool,
    pub all_day: bool,
    pub updated: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarEvents {
    pub events: Vec<CalendarEvent>,
    pub next_page_token: Option<String>,
    pub partial: bool,
    pub primary_id: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CalendarRange {
    pub connection_id: String,
    pub start: String,
    pub end: String,
    pub page_token: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalendarMutation {
    Create {
        fields: CalendarFields,
    },
    Update {
        event_id: String,
        etag: String,
        fields: CalendarFields,
    },
    Delete {
        event_id: String,
        etag: String,
    },
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CalendarChange {
    pub request_id: String,
    pub connection_id: String,
    #[ts(type = "number")]
    pub expected_epoch: i64,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub mutation: CalendarMutation,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarAction {
    pub id: String,
    pub connection_id: String,
    pub state: String,
    pub event_id: String,
    pub before_etag: Option<String>,
    pub provider_etag: Option<String>,
    pub event: Option<CalendarEvent>,
    pub receipt: Option<String>,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub version: i64,
}

/// Only created for newly submitted ordinary chat. No credential or provider URL.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarRunProfile {
    pub version: u32,
    #[ts(type = "number")]
    pub reference: i64,
    pub timezone: Option<String>,
    pub connections: Vec<CalendarAgentPin>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarAgentPin {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub epoch: i64,
    pub client_id: String,
    #[ts(type = "number | null")]
    pub app_revision: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalendarReadRequest {
    List { range: CalendarRange },
    Read { observation_ref: String },
    Status { action_id: String },
}
/// Shared current-model vocabulary. Stored DTO decoding stays backward compatible.
pub const CALENDAR_CHAT_DECISIONS: &[&str] = &["command", "clarify", "unsupported"];
pub const CALENDAR_CHAT_OPERATIONS: &[&str] = &["create", "update", "delete"];
pub const CALENDAR_CHAT_CLARIFICATIONS: &[&str] = &[
    "time",
    "date",
    "duration",
    "target",
    "account",
    "ambiguous",
    "unsupported",
];
pub const CALENDAR_CHAT_FIELDS: &[&str] = &[
    "domain",
    "decision",
    "operation",
    "connection_quote",
    "intent_quote",
    "title_quote",
    "target_quote",
    "date_quote",
    "date",
    "move_date_quote",
    "move_date",
    "all_day_quote",
    "last_date_quote",
    "last_date",
    "start_quote",
    "start",
    "end_quote",
    "end",
    "duration_quote",
    "duration_minutes",
    "clarification",
];

/// One isolated current-message proposal. Current-model keys are explicit;
/// nullable values retain decision/partial-draft semantics and legacy decoding.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalendarChatDecision {
    pub domain: String,
    pub decision: String,
    pub operation: Option<String>,
    pub connection_quote: Option<String>,
    pub intent_quote: Option<String>,
    pub title_quote: Option<String>,
    pub target_quote: Option<String>,
    pub date_quote: Option<String>,
    pub date: Option<String>,
    #[serde(default)]
    pub move_date_quote: Option<String>,
    #[serde(default)]
    pub move_date: Option<String>,
    /// Exact explicit all-day request; absent for edits that retain the observed event kind.
    #[serde(default)]
    pub all_day_quote: Option<String>,
    /// Optional explicitly supplied inclusive last day for an all-day create/span edit.
    #[serde(default)]
    pub last_date_quote: Option<String>,
    #[serde(default)]
    pub last_date: Option<String>,
    pub start_quote: Option<String>,
    pub start: Option<String>,
    pub end_quote: Option<String>,
    pub end: Option<String>,
    pub duration_quote: Option<String>,
    pub duration_minutes: Option<u32>,
    pub clarification: Option<String>,
}
impl CalendarChatDecision {
    /// New proposals must use machine codes. This does not validate authority,
    /// quotes or civil values and is not applied to already stored replay data.
    pub fn known_model_codes(&self) -> bool {
        self.domain == "calendar"
            && CALENDAR_CHAT_DECISIONS.contains(&self.decision.as_str())
            && (self.decision != "clarify" || self.clarification.is_some())
            && self
                .operation
                .as_deref()
                .is_none_or(|v| CALENDAR_CHAT_OPERATIONS.contains(&v))
            && self
                .clarification
                .as_deref()
                .is_none_or(|v| CALENDAR_CHAT_CLARIFICATIONS.contains(&v))
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum CurrentChatDecision {
    Calendar(CalendarChatDecision),
    Mission(crate::MissionChatDecision),
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct CalendarChatReceipt {
    pub action_id: Option<String>,
    pub operation: String,
    pub state: String,
    pub event_id: Option<String>,
    pub provider_etag: Option<String>,
    pub message: String,
}
