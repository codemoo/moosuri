//! Exact owned conversation evidence, never current user commands or authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum ConversationScope {
    CurrentThread,
    OwnedConversations,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ConversationHit {
    pub turn_id: String,
    pub thread_id: String,
    #[ts(type = "number")]
    pub revision: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    pub user_excerpt: String,
    pub assistant_excerpt: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ConversationSearch {
    pub hits: Vec<ConversationHit>,
    pub truncated: bool,
    pub scanned_limit: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalRole {
    User,
    Assistant,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct HistoricalMessage {
    pub role: HistoricalRole,
    pub text: String,
    pub phase: Option<crate::MessagePhase>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct ConversationTurn {
    pub turn_id: String,
    pub thread_id: String,
    #[ts(type = "number")]
    pub revision: i64,
    #[ts(type = "number")]
    pub created_at: i64,
    pub messages: Vec<HistoricalMessage>,
}
