//! JSON contract of `/api/v1/personal/tasks/monitors`. Plain data only; no source text appears here.
use moosuri_core::personal_browser::WorkScope;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MonitorKind {
    /// Notify only when the verified source references (id + revision) change.
    Watch,
    /// Summarise once per local day at `at` in `timezone`.
    Brief,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MonitorRequest {
    /// Stable id: the same request id with the same body returns the same monitor.
    pub request_id: String,
    #[serde(default)]
    pub thread_id: Option<String>,
    #[serde(default)]
    pub message: Option<String>,
    pub kind: MonitorKind,
    pub title: String,
    pub scope: WorkScope,
    pub query: String,
    /// Watch only: 1800, 3600 or 86400 seconds.
    #[serde(default)]
    pub every_seconds: Option<i64>,
    /// Brief only: local clock `HH:MM`.
    #[serde(default)]
    pub at: Option<String>,
    /// Brief only: browser IANA zone (`Asia/Seoul`) or fixed UTC offset (`+09:00`, `UTC-05:30`).
    #[serde(default)]
    pub timezone: Option<String>,
    /// Lifetime in days, 1..=90. The monitor expires on its own.
    pub days: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct MonitorStop {
    pub request_id: String,
    pub revision: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorSourceRef {
    pub id: String,
    pub revision: i64,
    /// Re-read with live authority on every response. When false no title is returned.
    pub available: bool,
    pub title: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorEvidence {
    /// Open through `/api/v1/personal/tasks/{run_id}` for the permission-checked worklog.
    pub run_id: String,
    pub read_at: i64,
    pub limited: bool,
    pub sources: Vec<MonitorSourceRef>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorCheck {
    pub due_at: i64,
    pub checked_at: i64,
    /// baseline, unchanged, changed, briefed, missed or failed.
    pub outcome: String,
    pub source_count: i64,
    /// Earlier occurrences folded into this one (app closed, restart).
    pub skipped: i64,
    pub run_id: Option<String>,
    pub notified: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorView {
    pub id: String,
    pub kind: MonitorKind,
    pub title: String,
    pub scope: WorkScope,
    pub query: String,
    /// active, stopped, expired or revoked.
    pub state: String,
    pub every_seconds: Option<i64>,
    pub at: Option<String>,
    pub timezone: Option<String>,
    pub days: i64,
    pub revision: i64,
    pub created_at: i64,
    pub expires_at: i64,
    pub next_due: Option<i64>,
    pub last_checked_at: Option<i64>,
    pub last_changed_at: Option<i64>,
    pub alerts: i64,
    /// What the monitor actually reads, in the member's language.
    pub basis: String,
    pub latest: Option<MonitorEvidence>,
    pub recent: Vec<MonitorCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MonitorPage {
    pub items: Vec<MonitorView>,
    pub active_limit: i64,
}
