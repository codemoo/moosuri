//! Private member browser responses. Only the takeover response carries a controller capability.
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tab {
    pub id: String,
    pub url: String,
    pub title: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Link {
    pub text: String,
    pub url: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub request_id: String,
    pub state: String,
    pub revision: i64,
    pub url: String,
    pub title: String,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<Tab>,
    pub links: Vec<Link>,
    pub reason: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct Status {
    pub configured: bool,
    pub workspace: Option<Workspace>,
}
#[derive(Serialize, Deserialize)]
pub struct Control {
    pub workspace: Workspace,
    pub control_token: String,
}
#[derive(Serialize, Deserialize)]
pub struct Frame {
    pub workspace_id: String,
    pub revision: i64,
    pub tab_id: String,
    pub frame_token: String,
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub url: String,
}

/// Member task projections contain no browser controls, page handles or session data.
#[derive(Serialize, Deserialize)]
pub struct TaskSummary {
    pub id: String,
    pub thread_id: String,
    pub title: String,
    pub state: String,
    pub next_actor: String,
    pub created_at: i64,
    #[serde(default = "browser_kind")]
    pub kind: String,
    #[serde(default)]
    pub preview: Option<String>,
    /// Scheduled slot of this occurrence, even when the timer caught up later.
    #[serde(default)]
    pub scheduled_at: Option<i64>,
    /// The recurring assignment that owns this run, including manual runs after a pause.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedule_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct TaskPage {
    #[serde(default)]
    pub schedules: Vec<WorkSchedule>,
    pub items: Vec<TaskSummary>,
    pub next_before: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct TaskEvent {
    pub state: String,
    /// Older jobs recorded the start time but no timestamp for their last state.
    pub at: Option<i64>,
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub detail: Option<String>,
}

#[derive(Serialize, Deserialize)]
pub struct TaskDetail {
    pub task: TaskSummary,
    pub request: String,
    pub thread_title: String,
    pub reason: Option<String>,
    pub history: Vec<TaskEvent>,
    pub result: Option<String>,
    pub observation: Option<String>,
}

fn browser_kind() -> String {
    "browser".into()
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkScope {
    Memory,
    Conversation,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkRequest {
    pub thread_id: Option<String>,
    pub message: Option<String>,
    pub request_id: String,
    pub title: String,
    pub scope: WorkScope,
    pub query: String,
    pub period: i64,
    pub first_due: Option<i64>,
}
#[derive(Serialize, Deserialize)]
pub struct WorkSchedule {
    pub id: String,
    pub title: String,
    pub scope: WorkScope,
    pub query: String,
    pub period: i64,
    pub next_due: Option<i64>,
    pub enabled: bool,
    pub revision: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkAction {
    Run { request_id: String },
    Pause { revision: i64 },
    Resume { revision: i64 },
    Remove { revision: i64 },
}
#[derive(Serialize, Deserialize)]
pub struct WorkReceipt {
    pub workflow_id: String,
    pub run_id: Option<String>,
}

/// Older clients used admission order alone. New cursors also freeze the
/// creation timestamp so deleting the last visible row cannot move the boundary.
pub fn task_cursor(value: &str) -> Option<(i64, i64)> {
    fn positive(s: &str) -> Option<i64> {
        s.parse::<i64>()
            .ok()
            .filter(|n| *n > 0 && n.to_string() == s)
    }
    if let Some((time, order)) = value.split_once('.') {
        Some((positive(time)?, positive(order)?))
    } else {
        Some((i64::MAX, positive(value)?))
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkTick {
    pub tenant: String,
    pub volume: String,
    pub generation: u64,
    pub epoch: i64,
}
