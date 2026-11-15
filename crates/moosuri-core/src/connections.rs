//! Opaque connection choices. Provider addresses and vault metadata never belong here.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ScopedConnectionDescriptor {
    pub binding_id: String,
    pub device_id: String,
    pub operation: String,
    pub resource: String,
    pub schema_version: u32,
    pub state: String,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
    pub alias: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionWaitView {
    pub request_id: String,
    pub slot_id: String,
    pub run_id: String,
    #[ts(type = "number")]
    pub step: i64,
    #[ts(type = "number")]
    pub acceptance_revision: i64,
    pub cause: String,
    pub state: String,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
    pub choices: Vec<ScopedConnectionDescriptor>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionConnectionSelection {
    pub request_id: String,
    pub wait_request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub slot_id: String,
    pub binding_id: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct ConnectionAlias {
    pub binding_id: String,
    pub alias: String,
}

/// Runtime-only request to yield at a completed, undispatched step boundary.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionConnectionWait {
    pub request_id: String,
    pub slot_id: String,
    pub acceptance_revision: i64,
    pub cause: String,
    pub operation_expires_at_ms: i64,
}
