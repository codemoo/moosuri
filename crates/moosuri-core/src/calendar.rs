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
    pub start: String,
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
/// One isolated current-message proposal. Nullable fields must stay absent for noncommands.
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
    pub start_quote: Option<String>,
    pub start: Option<String>,
    pub end_quote: Option<String>,
    pub end: Option<String>,
    pub duration_quote: Option<String>,
    pub duration_minutes: Option<u32>,
    pub clarification: Option<String>,
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
