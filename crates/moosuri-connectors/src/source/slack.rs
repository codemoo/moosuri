//! Slack user-token polling, with explicit bounded old-thread reconciliation.
//! Polling cannot establish deletions or replies on parents outside its lookback.
//! Imported tokens require auth.test identity verification and channel history
//! scopes. No token is manufactured, consented or granted by this library.
use super::transport::{Endpoint, Transport};
use super::{Coverage, ReadError, SourceChange, SourceItem, SyncPage, atom, decode, id};
use crate::gmail::SecretString;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio_util::sync::CancellationToken;

pub const USER_HISTORY_SCOPES: [&str; 4] = [
    "channels:history",
    "groups:history",
    "im:history",
    "mpim:history",
];
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlackScope {
    pub workspace_id: String,
    pub user_id: String,
    pub channel_id: String,
    pub oldest_unix: i64,
    /// Parents within this many seconds are re-enumerated every cycle, including
    /// parents absent from the new-message window. Maximum 90 days.
    pub reconciliation_lookback_seconds: i64,
}
impl SlackScope {
    pub fn validate(&self, now: i64) -> Result<(), ReadError> {
        for value in [&self.workspace_id, &self.user_id, &self.channel_id] {
            id(value)?;
        }
        if self.oldest_unix <= 0
            || self.oldest_unix > now
            || now.saturating_sub(self.oldest_unix) > 90 * 86400
            || !(1..=90 * 86400).contains(&self.reconciliation_lookback_seconds)
        {
            return Err(ReadError::InvalidInput);
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlackCheckpoint {
    /// Only advances when both incremental polling and reconciliation finish.
    pub high_water_unix: i64,
    pub phase: SlackPhase,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum SlackPhase {
    Idle,
    History {
        window: SlackWindow,
    },
    Replies {
        window: SlackWindow,
        next_history_cursor: Option<String>,
        roots: Vec<String>,
        root_index: usize,
        reply_cursor: Option<String>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SlackWindow {
    pub oldest_unix: i64,
    pub latest_unix: i64,
    pub cursor: Option<String>,
    pub reconciliation: bool,
}
#[derive(Serialize, Deserialize)]
pub struct SlackIdentity {
    pub workspace_id: String,
    pub user_id: String,
}
#[derive(Clone)]
pub struct SlackClient {
    transport: Transport,
}
impl Default for SlackClient {
    fn default() -> Self {
        Self::slack()
    }
}
impl SlackClient {
    pub fn slack() -> Self {
        Self {
            transport: Transport::fixed(),
        }
    }
    #[cfg(test)]
    pub(super) fn fixture(transport: Transport) -> Self {
        Self { transport }
    }
    pub async fn identity(
        &self,
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<SlackIdentity, ReadError> {
        #[derive(Deserialize)]
        struct Identity {
            team_id: String,
            user_id: String,
            bot_id: Option<String>,
        }
        let identity: Identity = self.get("auth.test", &[], token, cancel).await?;
        id(&identity.team_id).map_err(|_| ReadError::InvalidResponse)?;
        id(&identity.user_id).map_err(|_| ReadError::InvalidResponse)?;
        if identity.bot_id.is_some() {
            return Err(ReadError::ScopeDenied);
        }
        Ok(SlackIdentity {
            workspace_id: identity.team_id,
            user_id: identity.user_id,
        })
    }
    pub async fn verify_identity(
        &self,
        token: &SecretString,
        workspace: &str,
        user: &str,
        cancel: &CancellationToken,
    ) -> Result<String, ReadError> {
        id(workspace)?;
        id(user)?;
        let identity = self.identity(token, cancel).await?;
        if identity.workspace_id != workspace || identity.user_id != user {
            return Err(ReadError::IdentityMismatch);
        }
        Ok(format!("{}:{}", identity.workspace_id, identity.user_id))
    }
    /// A page is bounded to 15 messages. Pending roots are bounded to one history
    /// page; reply and history cursors never replace the timestamp watermark.
    pub async fn next_page(
        &self,
        token: &SecretString,
        scope: &SlackScope,
        checkpoint: Option<&SlackCheckpoint>,
        now: i64,
        cancel: &CancellationToken,
    ) -> Result<SyncPage<SlackCheckpoint>, ReadError> {
        scope.validate(if checkpoint.is_some() {
            scope.oldest_unix
        } else {
            now
        })?;
        if checkpoint.is_none() && now.saturating_sub(scope.oldest_unix) > 90 * 86400 {
            return Err(ReadError::InvalidInput);
        }
        self.verify_identity(token, &scope.workspace_id, &scope.user_id, cancel)
            .await?;
        let cp = checkpoint.cloned().unwrap_or(SlackCheckpoint {
            high_water_unix: scope.oldest_unix,
            phase: SlackPhase::Idle,
        });
        if cp.high_water_unix < scope.oldest_unix || cp.high_water_unix > now {
            return Err(ReadError::InvalidInput);
        }
        let phase = match cp.phase {
            SlackPhase::Idle => SlackPhase::History {
                window: SlackWindow {
                    oldest_unix: scope.oldest_unix.max(cp.high_water_unix.saturating_sub(1)),
                    latest_unix: now,
                    cursor: None,
                    reconciliation: false,
                },
            },
            phase => phase,
        };
        match phase {
            SlackPhase::History { window } => {
                validate_window(&window, scope, now)?;
                let mut params = window_params(&window);
                params.push(("channel", scope.channel_id.clone()));
                let page: Messages = self
                    .get("conversations.history", &params, token, cancel)
                    .await?;
                let cursor = next_cursor(&page, &window.cursor)?;
                let mut changes = Vec::new();
                let mut roots = Vec::new();
                if page.messages.len() > 15 {
                    return Err(ReadError::LimitExceeded);
                }
                for message in page.messages {
                    if message.reply_count.unwrap_or(0) > 0
                        && message.thread_ts.as_ref().is_none_or(|t| t == &message.ts)
                    {
                        timestamp(&message.ts)?;
                        roots.push(message.ts.clone());
                    }
                    if let Some(change) = map_message(scope, &window, message, None)? {
                        changes.push(change);
                    }
                }
                let checkpoint = if roots.is_empty() {
                    after_history(scope, cp.high_water_unix, window, cursor)
                } else {
                    SlackCheckpoint {
                        high_water_unix: cp.high_water_unix,
                        phase: SlackPhase::Replies {
                            window,
                            next_history_cursor: cursor,
                            roots,
                            root_index: 0,
                            reply_cursor: None,
                        },
                    }
                };
                super::bounded(finish(changes, checkpoint))
            }
            SlackPhase::Replies {
                window,
                next_history_cursor,
                roots,
                root_index,
                reply_cursor,
            } => {
                validate_window(&window, scope, now)?;
                if roots.len() > 15 || root_index >= roots.len() {
                    return Err(ReadError::InvalidInput);
                }
                for root in &roots {
                    let root_time = timestamp(root)?;
                    if root_time < window.oldest_unix || root_time > window.latest_unix {
                        return Err(ReadError::InvalidInput);
                    }
                }
                let root = &roots[root_index];
                let mut params = window_params(&SlackWindow {
                    cursor: reply_cursor.clone(),
                    ..window.clone()
                });
                params.push(("channel", scope.channel_id.clone()));
                params.push(("ts", root.clone()));
                let page: Messages = self
                    .get("conversations.replies", &params, token, cancel)
                    .await?;
                let cursor = next_cursor(&page, &reply_cursor)?;
                if page.messages.len() > 15 {
                    return Err(ReadError::LimitExceeded);
                }
                let mut changes = Vec::new();
                for message in page.messages {
                    if let Some(change) = map_message(scope, &window, message, Some(root))? {
                        changes.push(change);
                    }
                }
                let checkpoint = if cursor.is_none() && root_index + 1 == roots.len() {
                    after_history(scope, cp.high_water_unix, window, next_history_cursor)
                } else {
                    SlackCheckpoint {
                        high_water_unix: cp.high_water_unix,
                        phase: SlackPhase::Replies {
                            window,
                            next_history_cursor,
                            roots,
                            root_index: if cursor.is_some() {
                                root_index
                            } else {
                                root_index + 1
                            },
                            reply_cursor: cursor,
                        },
                    }
                };
                super::bounded(finish(changes, checkpoint))
            }
            SlackPhase::Idle => unreachable!(),
        }
    }
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        method: &str,
        params: &[(&str, String)],
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<T, ReadError> {
        let query = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query.extend_pairs(params.iter().map(|(k, v)| (*k, v.as_str())));
            query.finish()
        };
        let bytes = self
            .transport
            .request(
                Endpoint::Slack,
                &format!("{method}?{query}"),
                Some(token),
                cancel,
            )
            .await?;
        #[derive(Deserialize)]
        struct Status {
            ok: bool,
            error: Option<String>,
        }
        let status: Status = decode(&bytes)?;
        if !status.ok {
            return Err(match status.error.as_deref() {
                Some(
                    "invalid_auth" | "not_authed" | "token_revoked" | "account_inactive"
                    | "token_expired",
                ) => ReadError::AuthenticationRequired,
                Some(
                    "missing_scope" | "not_in_channel" | "channel_not_found" | "no_permission",
                ) => ReadError::ScopeDenied,
                Some("invalid_cursor") => ReadError::ResyncRequired,
                Some("ratelimited") => ReadError::RateLimited {
                    retry_after_seconds: 60,
                },
                _ => ReadError::InvalidResponse,
            });
        }
        decode(&bytes)
    }
}
fn window_params(window: &SlackWindow) -> Vec<(&'static str, String)> {
    let mut p = vec![
        ("oldest", window.oldest_unix.to_string()),
        ("latest", window.latest_unix.to_string()),
        ("inclusive", "true".into()),
        ("limit", "15".into()),
    ];
    if let Some(c) = &window.cursor {
        p.push(("cursor", c.clone()));
    }
    p
}
fn validate_window(window: &SlackWindow, scope: &SlackScope, now: i64) -> Result<(), ReadError> {
    if window.oldest_unix < scope.oldest_unix
        || window.oldest_unix > window.latest_unix
        || window.latest_unix > now
    {
        return Err(ReadError::InvalidInput);
    }
    if let Some(c) = &window.cursor {
        atom(c, 2048)?;
    }
    Ok(())
}
fn after_history(
    scope: &SlackScope,
    high_water_unix: i64,
    mut window: SlackWindow,
    cursor: Option<String>,
) -> SlackCheckpoint {
    if cursor.is_some() {
        window.cursor = cursor;
        return SlackCheckpoint {
            high_water_unix,
            phase: SlackPhase::History { window },
        };
    }
    if !window.reconciliation {
        window.oldest_unix = scope.oldest_unix.max(
            window
                .latest_unix
                .saturating_sub(scope.reconciliation_lookback_seconds),
        );
        window.cursor = None;
        window.reconciliation = true;
        SlackCheckpoint {
            high_water_unix,
            phase: SlackPhase::History { window },
        }
    } else {
        SlackCheckpoint {
            high_water_unix: window.latest_unix,
            phase: SlackPhase::Idle,
        }
    }
}
fn finish(changes: Vec<SourceChange>, checkpoint: SlackCheckpoint) -> SyncPage<SlackCheckpoint> {
    let coverage = if matches!(checkpoint.phase, SlackPhase::Idle) {
        Coverage::SlackPollingWindow
    } else {
        Coverage::MorePages
    };
    SyncPage {
        changes,
        checkpoint,
        coverage,
    }
}
#[derive(Deserialize)]
struct Messages {
    #[serde(default)]
    messages: Vec<Message>,
    #[serde(default)]
    has_more: bool,
    response_metadata: Option<Metadata>,
}
#[derive(Deserialize)]
struct Metadata {
    next_cursor: Option<String>,
}
#[derive(Deserialize)]
struct Message {
    ts: String,
    text: Option<String>,
    user: Option<String>,
    bot_id: Option<String>,
    thread_ts: Option<String>,
    reply_count: Option<u64>,
    edited: Option<Edited>,
    subtype: Option<String>,
    deleted_ts: Option<String>,
}
#[derive(Deserialize)]
struct Edited {
    ts: String,
}
fn next_cursor(page: &Messages, previous: &Option<String>) -> Result<Option<String>, ReadError> {
    let next = page
        .response_metadata
        .as_ref()
        .and_then(|m| m.next_cursor.as_ref())
        .filter(|s| !s.is_empty())
        .cloned();
    if let Some(c) = &next {
        atom(c, 2048).map_err(|_| ReadError::InvalidResponse)?;
        if previous.as_ref() == Some(c) {
            return Err(ReadError::InvalidResponse);
        }
    }
    // Never declare coverage when Slack says there are more results without a
    // continuation. A caller can retry/restart; no checkpoint is advanced.
    if page.has_more && next.is_none() {
        return Err(ReadError::InvalidResponse);
    }
    Ok(next)
}
fn timestamp(value: &str) -> Result<i64, ReadError> {
    let (sec, frac) = value.split_once('.').ok_or(ReadError::InvalidResponse)?;
    if sec.is_empty()
        || sec.len() > 12
        || frac.len() != 6
        || !sec.bytes().chain(frac.bytes()).all(|b| b.is_ascii_digit())
    {
        return Err(ReadError::InvalidResponse);
    }
    sec.parse().map_err(|_| ReadError::InvalidResponse)
}
fn map_message(
    scope: &SlackScope,
    window: &SlackWindow,
    message: Message,
    root: Option<&String>,
) -> Result<Option<SourceChange>, ReadError> {
    let observed_at = timestamp(&message.ts)?;
    if observed_at < window.oldest_unix || observed_at > window.latest_unix {
        return Err(ReadError::InvalidResponse);
    }
    let external = |ts: &str| format!("{}:{ts}", scope.channel_id);
    if message.subtype.as_deref() == Some("message_deleted") {
        let deleted = message.deleted_ts.ok_or(ReadError::InvalidResponse)?;
        timestamp(&deleted)?;
        return Ok(Some(SourceChange::Delete {
            external_id: external(&deleted),
            remote_version: message.ts,
        }));
    }
    let thread = message.thread_ts.unwrap_or_else(|| message.ts.clone());
    timestamp(&thread)?;
    if root.is_some_and(|root| root != &thread) {
        return Err(ReadError::InvalidResponse);
    }
    let text = message.text.unwrap_or_default();
    if text.len() > 128 * 1024 {
        return Err(ReadError::LimitExceeded);
    }
    let speaker = message
        .user
        .or(message.bot_id)
        .unwrap_or_else(|| "unknown".into());
    id(&speaker).map_err(|_| ReadError::InvalidResponse)?;
    let version = if let Some(edited) = message.edited {
        timestamp(&edited.ts)?;
        edited.ts
    } else {
        format!("{}:{:x}", message.ts, Sha256::digest(text.as_bytes()))
    };
    Ok(Some(SourceChange::Upsert(SourceItem {
        external_id: external(&message.ts),
        remote_version: version,
        subject: "Slack message".into(),
        text,
        speaker,
        thread_id: external(&thread),
        parent_id: if thread == message.ts {
            None
        } else {
            Some(external(&thread))
        },
        observed_at,
    })))
}
