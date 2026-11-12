//! Personal mail DTOs. No OAuth secrets, MIME bytes or approval authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MailConnection {
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
}
