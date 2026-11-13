//! Owner-only capability/status metadata. Subscription endpoints/keys are absent.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct PushDeviceView {
    pub binding_id: String,
    pub state: String,
    pub last_outcome: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct PushConfiguration {
    pub application_server_key: String,
    pub device: Option<PushDeviceView>,
}
