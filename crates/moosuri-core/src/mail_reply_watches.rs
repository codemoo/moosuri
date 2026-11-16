//! Exact Gmail conversation follow-up. No mailbox-wide or send permission.
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailReplyWatchCreate {
    pub request_id: String,
    pub thread_id: String,
    /// Opaque, server-derived completed-read or successful-send receipt.
    pub target_id: String,
    pub cadence_seconds: u32,
    pub duration_days: u32,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MailReplyTarget {
    pub id: String,
    pub title: String,
    pub mailbox: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MailReplyPreview {
    pub message_id: String,
    pub from: Vec<String>,
    pub subject: String,
    pub text: String,
    pub partial: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MailReplyWatch {
    pub id: String,
    pub thread_id: String,
    pub title: String,
    pub mailbox: String,
    pub state: String,
    pub cadence_seconds: u32,
    pub expires_at: i64,
    pub last_checked_at: Option<i64>,
    pub next_check_at: Option<i64>,
    pub reason: Option<String>,
    pub replies: Vec<MailReplyPreview>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct MailReplyWatchInventory {
    pub targets: Vec<MailReplyTarget>,
    pub watches: Vec<MailReplyWatch>,
}
