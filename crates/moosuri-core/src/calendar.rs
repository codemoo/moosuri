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
