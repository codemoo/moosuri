//! Account administration metadata. No private account content is included.
use super::{AccountSummary, Role};
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Enrollment policy; existing password hashes keep their normal login path.
pub fn account_password_valid(password: &str) -> bool {
    password.len() <= 256 && password.chars().count() >= 8
}

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
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub deletion: Option<AccountDeletionStatus>,
    #[ts(optional)]
    #[serde(skip_serializing_if = "Option::is_none")]
    pub vault_delivery: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct AccountDeletionStatus {
    pub local_complete: bool,
    pub vault_acknowledged: bool,
    pub encrypted_history_retained: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum AccountManagementAction {
    Suspend,
    Resume,
    ChangeRole { role: Role },
    ResetPassword,
    EndSessions,
    Delete,
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
