//! Personal mail DTOs. No OAuth secrets, MIME bytes or approval authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MailConnection {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub native_send: bool,
    pub compose_enabled: bool,
    pub id: String,
    pub email: String,
    #[ts(type = "number")]
    pub epoch: i64,
    pub status: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MailConnections {
    pub configured: bool,
    pub connections: Vec<MailConnection>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailReply {
    pub thread_id: String,
    pub in_reply_to: String,
    #[serde(default)]
    pub references: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailDraftContent {
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: String,
    pub reply: Option<MailReply>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailDraftInput {
    pub connection_id: String,
    pub content: MailDraftContent,
    pub origin_thread_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MailAction {
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    #[ts(as = "Option<bool>", optional)]
    pub native_send: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub schedule: Option<MailSchedule>,
    pub id: String,
    #[ts(type = "number")]
    pub version: i64,
    #[ts(type = "number")]
    pub revision: i64,
    pub connection_id: String,
    pub content: Option<MailDraftContent>,
    pub digest: String,
    pub state: String,
    #[ts(type = "number")]
    pub expires_at: i64,
    pub message_id: String,
    pub provider_message_id: Option<String>,
    pub provider_thread_id: Option<String>,
    pub origin_thread_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub reconciliation: Option<MailReconciliation>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MailReconciliation {
    pub status: String,
    pub last_result: String,
    #[ts(type = "number | null")]
    pub last_checked_at: Option<i64>,
    pub candidate_count: u32,
    pub automatic_attempts: u32,
    #[ts(type = "number")]
    pub manual_after: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailApproval {
    #[ts(type = "number")]
    pub revision: i64,
    pub digest: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailConnectionPin {
    #[serde(default)]
    pub native_send: bool,
    pub id: String,
    pub epoch: i64,
    pub email: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MailToolRequest {
    Search {
        connection_id: String,
        query: String,
        page_token: Option<String>,
    },
    Read {
        connection_id: String,
        message_id: String,
    },
    Attachments {
        connection_id: String,
        message_id: String,
        start_index: u32,
    },
    Attachment {
        connection_id: String,
        message_id: String,
        part_id: String,
        attachment_id: Option<String>,
        start_byte: u32,
        max_bytes: u32,
        /// Required for continuation pages, binds the exact extracted document.
        expected_digest: Option<String>,
    },
    Thread {
        connection_id: String,
        thread_id: String,
    },
    Draft {
        input: MailDraftInput,
    },
}
impl MailToolRequest {
    pub fn connection_id(&self) -> &str {
        match self {
            Self::Search { connection_id, .. }
            | Self::Read { connection_id, .. }
            | Self::Attachment { connection_id, .. }
            | Self::Attachments { connection_id, .. }
            | Self::Thread { connection_id, .. } => connection_id,
            Self::Draft { input } => &input.connection_id,
        }
    }
}
// Preserve the existing typed Mail carrier API. Durable input bytes are bounded
// separately and ExternalToolPermit already boxes this union during IO.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Serialize, Deserialize)]
#[serde(
    tag = "adapter",
    content = "request",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum ExternalToolRequest {
    Mail(MailToolRequest),
    Web(crate::WebToolRequest),
    Calendar(crate::CalendarReadRequest),
}

/// Fixed one-off send timing. This is separate from provider-free reminders.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailSchedule {
    #[ts(type = "number")]
    pub due_at: i64,
    pub timezone: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailScheduleInput {
    pub expected: MailApproval,
    pub schedule: Option<MailSchedule>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct GmailDraftRef {
    pub id: String,
    pub message_id: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct GmailDraftPage {
    pub drafts: Vec<GmailDraftRef>,
    pub next_page_token: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct GmailDraft {
    pub id: String,
    pub digest: String,
    pub subject: String,
    pub text: String,
    pub text_truncated: bool,
    pub attachment_names: Vec<String>,
    /// None means unsupported MIME; never silently remove attachments/HTML/reply metadata.
    pub content: Option<MailDraftContent>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct GmailDraftSave {
    pub expected: MailApproval,
    pub draft_id: Option<String>,
    pub expected_provider_digest: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MailSchedulePreviewInput {
    pub local_datetime: String,
    pub timezone: String,
}

/// Server-recorded setup action; model text and links cannot create this receipt.
#[derive(Clone, Copy, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MailConnectionRecovery {
    GwsSetup,
    AdminSetup,
    MemberSetup,
    Connect,
    Reconnect,
    ChooseAccount,
}
