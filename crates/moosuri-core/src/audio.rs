//! Bounded dictation is independent of chat model/attachment selection.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AudioSelection {
    pub provider_id: String,
    #[ts(type = "number")]
    pub provider_revision: i64,
    pub model: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct AudioSettings {
    pub selection: Option<AudioSelection>,
    pub available: bool,
    pub max_seconds: u32,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct AudioJob {
    pub id: String,
    pub state: String,
    pub transcript: Option<String>,
    pub error: Option<String>,
    #[ts(type = "number")]
    pub expires_at: i64,
}
