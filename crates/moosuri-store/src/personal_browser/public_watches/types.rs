//! JSON contract of `/api/v1/personal/tasks/public-watches` and the typed reader hand-off.
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// `POST /api/v1/personal/tasks/public-watches`. Unknown fields are refused, never ignored.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicWatchRequest {
    /// Stable id: the same request id with the same body returns the same watch.
    pub request_id: String,
    /// The original conversation. The grant and its worklog are shown there.
    pub thread_id: String,
    /// The member's own words; recorded as the thread turn that created the grant.
    pub message: String,
    /// Explicit public `https://` page. No credentials, port, query or fragment.
    pub url: String,
    /// 1800, 3600 or 86400.
    pub every_seconds: i64,
    /// Lifetime in days, 1..=90 (the UI defaults to 7). The watch expires on its own.
    pub days: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicWatchStop {
    pub request_id: String,
    pub revision: i64,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicWatchCheck {
    pub due_at: i64,
    /// claimed, baseline, unchanged, changed, failed, unknown or withdrawn.
    pub state: String,
    /// Short visible-text excerpt; only on `baseline` and `changed`, and only while the grant is live.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub excerpt: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicWatchView {
    pub id: String,
    pub thread_id: String,
    pub url: String,
    pub every_seconds: i64,
    pub days: i64,
    /// active, stopped, expired or revoked.
    pub state: String,
    pub revision: i64,
    /// Newest first, at most 10.
    pub checks: Vec<PublicWatchCheck>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct PublicWatchPage {
    pub items: Vec<PublicWatchView>,
}

/// One bounded observation of the pinned page, as read by the broker.
#[derive(Clone, Debug)]
pub struct PublicPage {
    /// The page's final address. Must still be on the pinned host.
    pub url: String,
    pub title: String,
    /// Visible text as the broker projected it. Bounded and normalized before use.
    pub text: String,
    /// Unix seconds at which the broker read the page.
    pub observed_at: i64,
}

#[derive(Clone, Debug)]
pub enum PublicReadOutcome {
    Page(PublicPage),
    /// Any read failure. It can only ever become a `failed` check, never a change.
    Failed(&'static str),
}

/// What the typed reader reports for one lease. `closed` is true only once the broker confirmed that the
/// disposable workspace was stopped (or never existed); otherwise the timer stops it later.
#[derive(Clone, Debug)]
pub struct PublicRead {
    pub outcome: PublicReadOutcome,
    pub closed: bool,
}

/// One claimed slot. Carries no secrets, no session and no cookies.
#[derive(Clone)]
pub struct PublicWatchLease {
    pub(in crate::personal_browser) owner: crate::personal_volume::VolumeOwner,
    pub watch_id: Uuid,
    pub due_at: i64,
    pub claimed_at: i64,
    /// Pinned grant URL: the only address the reader may open.
    pub url: String,
    /// Pinned host: the observed page must still be on it.
    pub host: String,
    /// Fresh disposable workspace id for this slot only.
    pub workspace_id: Uuid,
}
