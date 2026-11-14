//! Admin-only invitation delivery DTOs; never model tools or ordinary account metadata.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct InvitationDelivery {
    pub id: String,
    pub account_id: String,
    pub state: String,
    pub digest: String,
    #[ts(type = "number")]
    pub expires_at: i64,
    pub from: String,
    pub to: String,
    pub subject: String,
    pub text: Option<String>,
    pub provider_message_id: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct InvitationPreviewInput {
    pub request_id: String,
    pub account_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    /// Available only from the one-time creation/reset UI. Not recoverable later.
    pub temporary_password: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct InvitationApproval {
    pub digest: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct InvitationSenderStatus {
    pub configured: bool,
    pub from: Option<String>,
}
