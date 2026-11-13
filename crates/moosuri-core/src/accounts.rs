//! Account administration metadata. No private account content is included.
use super::{AccountSummary, Role};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum AccountStatus {
    Active,
    Suspended,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ManagedAccount {
    pub user: AccountSummary,
    pub status: AccountStatus,
    #[ts(type = "number")]
    pub revision: i64,
    pub recovery_required: bool,
    #[ts(type = "number | null")]
    pub temporary_expires_at: Option<i64>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountManagementAction {
    Suspend,
    Resume,
    ChangeRole { role: Role },
    ResetPassword,
    EndSessions,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct AccountManagementInput {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub action: AccountManagementAction,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct AccountManagementReceipt {
    pub request_id: String,
    pub account_id: String,
    #[ts(type = "number")]
    pub revision: i64,
}
