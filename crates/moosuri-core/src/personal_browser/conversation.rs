//! Per-member research groups. A group belongs to one ordinary turn; each child
//! can be answered, refined, and re-run without changing its siblings.
use super::WorkScope;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkItemInput {
    pub title: String,
    pub scope: WorkScope,
    pub query: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub filters: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConversationWorkRequest {
    pub request_id: String,
    pub message: String,
    pub items: Vec<WorkItemInput>,
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ConversationActionKind {
    Refine,
    Answer,
    Cancel,
    Reuse,
    SaveReport,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ConversationWorkAction {
    /// For controls this identifies the original group. The task, revision and payload
    /// identify the exact transition. Reuse supplies a new group UUID for the new search.
    pub request_id: String,
    pub task_id: String,
    pub revision: i64,
    pub action: ConversationActionKind,
    pub text: Option<String>,
    pub question_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationEvent {
    pub state: String,
    pub text: String,
    pub at: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationTask {
    pub id: String,
    pub title: String,
    pub scope: WorkScope,
    pub query: String,
    pub filter: Option<String>,
    pub state: String,
    pub revision: i64,
    pub question_id: Option<String>,
    pub run_id: Option<String>,
    pub runs: Vec<String>,
    pub events: Vec<ConversationEvent>,
    pub source_count: usize,
    pub can_reuse: bool,
    pub summary: Option<String>,
    pub report_id: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationWorkGroup {
    pub id: String,
    pub thread_id: String,
    pub turn_id: String,
    pub message: String,
    pub tasks: Vec<ConversationTask>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConversationWorkPage {
    pub thread_id: String,
    pub groups: Vec<ConversationWorkGroup>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ConversationReport {
    pub id: String,
    pub title: String,
    pub body: String,
    pub run_id: String,
}
