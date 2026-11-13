//! Safe Google installation and personal connection views. Never carries secrets.
use crate::MailConnection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct GoogleSetupStatus {
    #[ts(type = "number")]
    pub revision: i64,
    pub mode: String,
    pub configured: bool,
    pub client_id: Option<String>,
    pub redirect_uri: String,
    pub saved_redirect_uri: Option<String>,
    pub redirect_matches: bool,
    pub personal_connections: Vec<MailConnection>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct GoogleSetupValidation {
    pub client_id: String,
    pub redirect_uri: String,
    pub local_valid: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct GoogleConnectionCheck {
    pub email: String,
    #[ts(type = "number")]
    pub checked_at: i64,
}
