//! Private media metadata; bytes travel only through authenticated binary routes.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MediaRef {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MediaSummary {
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub digest: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub filename: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub media_type: Option<String>,
    pub state: String,
    pub width: u32,
    pub height: u32,
    pub bytes: u32,
    #[ts(type = "number")]
    pub created_at: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MediaLibrary {
    pub items: Vec<MediaSummary>,
    pub quota_bytes: u32,
    pub used_bytes: u32,
}
