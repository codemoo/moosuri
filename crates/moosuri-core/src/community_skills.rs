//! User-facing community lifecycle. Package instructions are not authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CommunitySkillCard {
    pub id: String,
    pub name: String,
    pub description: String,
    pub source: String,
    pub source_commit: String,
    pub version: String,
    pub digest: String,
    pub license: String,
    pub capabilities: Vec<String>,
    pub installed_version: Option<String>,
    pub installed_digest: Option<String>,
    pub enabled: bool,
    pub hidden: bool,
    #[ts(type = "number")]
    pub revision: i64,
    pub can_rollback: bool,
    pub readiness: String,
    pub missing: Vec<String>,
}
#[derive(Clone, Deserialize, Serialize, TS)]
#[serde(deny_unknown_fields)]
pub struct CommunitySkillMutation {
    pub action: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub expected_digest: Option<String>,
}
