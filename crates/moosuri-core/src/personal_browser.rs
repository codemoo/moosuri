//! Private member browser responses. Only the takeover response carries a controller capability.
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Tab {
    pub id: String,
    pub url: String,
    pub title: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Link {
    pub text: String,
    pub url: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub run_id: String,
    pub thread_id: String,
    pub request_id: String,
    pub state: String,
    pub revision: i64,
    pub url: String,
    pub title: String,
    pub active_tab_id: Option<String>,
    pub tabs: Vec<Tab>,
    pub links: Vec<Link>,
    pub reason: Option<String>,
}
#[derive(Serialize, Deserialize)]
pub struct Status {
    pub configured: bool,
    pub workspace: Option<Workspace>,
}
#[derive(Serialize, Deserialize)]
pub struct Control {
    pub workspace: Workspace,
    pub control_token: String,
}
#[derive(Serialize, Deserialize)]
pub struct Frame {
    pub workspace_id: String,
    pub revision: i64,
    pub tab_id: String,
    pub frame_token: String,
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub url: String,
}
