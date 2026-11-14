//! Fixed-content local reminders. These contracts grant no model/tool/send authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ScheduleRule {
    Once {
        #[ts(type = "number")]
        at: i64,
    },
    Interval {
        #[ts(type = "number")]
        anchor: i64,
        seconds: u32,
    },
    Daily {
        time: String,
        timezone: String,
    },
    Weekly {
        days: Vec<u8>,
        time: String,
        timezone: String,
    },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ScheduleDraft {
    pub title: String,
    pub message: String,
    pub rule: ScheduleRule,
    #[ts(type = "number | null")]
    pub valid_until: Option<i64>,
}
#[derive(Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ScheduleCreate {
    pub request_id: String,
    pub draft: ScheduleDraft,
}
#[derive(Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ScheduleEdit {
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub draft: ScheduleDraft,
}
#[derive(Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ScheduleControl {
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub action: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ScheduleView {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub title: String,
    pub message: String,
    pub rule: ScheduleRule,
    pub state: String,
    pub reason: Option<String>,
    #[ts(type = "number | null")]
    pub next_fire_at: Option<i64>,
    pub next_local_slot: Option<String>,
    #[ts(type = "number | null")]
    pub valid_until: Option<i64>,
    pub timezone_db: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct SchedulePreview {
    #[ts(type = "number")]
    pub next_fire_at: i64,
    pub local_slot: String,
    pub timezone_db: String,
    pub skipped_gap: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ScheduleOccurrence {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    #[ts(type = "number | null")]
    pub scheduled_for: Option<i64>,
    pub local_slot: String,
    pub state: String,
    pub reason: Option<String>,
    pub notification_id: Option<String>,
    pub notification_suppressed: bool,
    pub delayed: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ScheduleSkippedRange {
    pub from_slot: String,
    pub to_slot: String,
    #[ts(type = "number")]
    pub count: i64,
    pub reason: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ScheduleHistory {
    pub occurrences: Vec<ScheduleOccurrence>,
    pub skipped_ranges: Vec<ScheduleSkippedRange>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct QuietHours {
    #[ts(type = "number")]
    pub revision: i64,
    pub enabled: bool,
    pub timezone: String,
    pub start_minute: u16,
    pub end_minute: u16,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct QuietHoursChange {
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub enabled: bool,
    pub timezone: String,
    pub start_minute: u16,
    pub end_minute: u16,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct NotificationSource {
    pub id: String,
    pub title: String,
    pub kind: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub muted: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct NotificationSourceChange {
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub muted: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct LocalNotification {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub source: Option<NotificationSource>,
    /// Resolved from current owned mission state, never copied result text.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub mission_id: Option<String>,
    pub id: String,
    pub schedule_id: String,
    pub occurrence_id: String,
    pub title: String,
    pub message: String,
    #[ts(type = "number")]
    pub scheduled_for: i64,
    #[ts(type = "number")]
    pub delivered_at: i64,
    pub delayed: bool,
    pub quiet: bool,
    pub read: bool,
    pub dismissed: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct NotificationPage {
    pub items: Vec<LocalNotification>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ScheduleTarget {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ScheduleChatContext {
    pub timezone: String,
    pub target: Option<ScheduleTarget>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ScheduleChatReceipt {
    pub request_id: String,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub action: String,
    pub state: String,
    pub title: String,
    pub next_local_slot: Option<String>,
    pub timezone: String,
    pub url: String,
}
