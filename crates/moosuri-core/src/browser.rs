//! Human browser workspace contracts. No CDP, browser profile or secret DTO.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum BrowserWorkspaceState {
    Starting,
    Ready,
    WaitingForUser,
    UserControl,
    Closed,
    Expired,
    OutcomeUnknown,
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct BrowserWorkspaceView {
    pub id: String,
    pub thread_id: String,
    pub state: BrowserWorkspaceState,
    #[ts(type = "number")]
    pub revision: i64,
    /// Once human input is enabled, model navigation remains disabled.
    pub human_touched: bool,
    pub url: Option<String>,
    pub title: Option<String>,
    pub reason: Option<String>,
    #[ts(type = "number")]
    pub expires_at: i64,
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct BrowserWorkspaceStatus {
    pub configured: bool,
    pub workspace: Option<BrowserWorkspaceView>,
}

#[derive(Serialize, Deserialize, TS)]
pub struct BrowserControl {
    pub workspace: BrowserWorkspaceView,
    /// Human controller capability: never persist in browser storage or a model turn.
    pub control_token: String,
}

#[derive(Serialize, Deserialize, TS)]
pub struct BrowserFrame {
    pub workspace_id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub frame_token: String,
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub url: String,
}
