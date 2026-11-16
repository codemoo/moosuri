//! Exact synchronized-source follow-up permission, never external-effect authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TaskMonitorScope {
    ExactSources {
        source_ids: Vec<String>,
        connection_pins: Vec<TaskMonitorConnection>,
    },
    ConnectedThread {
        connection_id: String,
        #[ts(type = "number")]
        config_revision: i64,
        #[ts(type = "number")]
        credential_epoch: i64,
        thread_id: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct TaskMonitorConnection {
    pub id: String,
    #[ts(type = "number")]
    pub config_revision: i64,
    #[ts(type = "number")]
    pub credential_epoch: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct TaskMonitorSource {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct TaskMonitorObservation {
    #[ts(type = "number")]
    pub ordinal: i64,
    pub mission_id: String,
    pub state: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number | null")]
    pub resolved_at: Option<i64>,
    pub note: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct TaskMonitorView {
    pub id: String,
    pub parent_mission_id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub state: String,
    pub sources: Vec<TaskMonitorSource>,
    pub scope: TaskMonitorScope,
    pub linked_mission_ids: Vec<String>,
    pub check_template: String,
    pub cadence_seconds: Option<u32>,
    #[ts(type = "number | null")]
    pub expires_at: Option<i64>,
    pub reusable: bool,
    pub reason: Option<String>,
    pub policy_id: Option<String>,
    pub existing_monitor_id: Option<String>,
    pub observations: Vec<TaskMonitorObservation>,
    #[ts(type = "number | null")]
    pub last_checked_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TaskMonitorAccept {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub cadence_seconds: u32,
    #[ts(type = "number")]
    pub expires_at: i64,
    pub reuse: bool,
    pub read: bool,
    pub check: bool,
    pub notify: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TaskMonitorControl {
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub revoke_policy: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct TaskMonitorResolve {
    pub note: String,
}
