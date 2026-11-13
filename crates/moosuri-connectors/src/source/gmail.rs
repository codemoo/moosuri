//! Gmail collection is separate from interactive read/send authority. No sender
//! code or automatic consent is exposed here. Gmail history retention is finite.
use super::transport::{Endpoint, Transport};
use super::{Coverage, ReadError, SourceChange, SourceItem, SyncPage, atom, decimal, decode, id};
use crate::gmail::{SecretString, parse_mail};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use tokio_util::sync::CancellationToken;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GmailScope {
    pub account_email: String,
    /// All labels must match. Empty/unrestricted mailbox scope is rejected.
    pub label_ids: Vec<String>,
    pub after_unix: i64,
}
impl GmailScope {
    pub fn validate(&self, now: i64) -> Result<(), ReadError> {
        atom(&self.account_email, 320)?;
        if !self.account_email.contains('@')
            || !(1..=16).contains(&self.label_ids.len())
            || self.after_unix <= 0
            || self.after_unix > now
            || now.saturating_sub(self.after_unix) > 366 * 86400
        {
            return Err(ReadError::InvalidInput);
        }
        for label in &self.label_ids {
            id(label)?;
        }
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GmailCheckpoint {
    pub history_id: String,
    pub page_token: Option<String>,
    pub backfill: bool,
}
#[derive(Clone)]
pub struct GmailSyncClient {
    transport: Transport,
}
impl Default for GmailSyncClient {
    fn default() -> Self {
        Self::google()
    }
}
impl GmailSyncClient {
    pub fn google() -> Self {
        Self {
            transport: Transport::fixed(),
        }
    }
    #[cfg(test)]
    pub(super) fn fixture(transport: Transport) -> Self {
        Self { transport }
    }
    pub async fn verify_identity(
        &self,
        token: &SecretString,
        expected_email: &str,
        cancel: &CancellationToken,
    ) -> Result<String, ReadError> {
        let profile: Profile = self
            .get("profile?fields=emailAddress,historyId", token, cancel)
            .await?;
        if !profile.email_address.eq_ignore_ascii_case(expected_email) {
            return Err(ReadError::IdentityMismatch);
        }
        decimal(&profile.history_id).map_err(|_| ReadError::InvalidResponse)?;
        Ok(profile.email_address)
    }
    /// One bounded page. Persist changes and checkpoint together; discard both
    /// on any error. A new initial sync requires explicit collection consent.
    pub async fn next_page(
        &self,
        token: &SecretString,
        scope: &GmailScope,
        checkpoint: Option<&GmailCheckpoint>,
        now: i64,
        cancel: &CancellationToken,
    ) -> Result<SyncPage<GmailCheckpoint>, ReadError> {
        // Backfill horizon is immutable after creation. It may age beyond the
        // initial maximum; validate initial age only on a fresh checkpoint.
        scope.validate(if checkpoint.is_some() {
            scope.after_unix
        } else {
            now
        })?;
        if now < scope.after_unix {
            return Err(ReadError::InvalidInput);
        }
        let profile: Profile = self
            .get("profile?fields=emailAddress,historyId", token, cancel)
            .await?;
        if !profile
            .email_address
            .eq_ignore_ascii_case(&scope.account_email)
        {
            return Err(ReadError::IdentityMismatch);
        }
        decimal(&profile.history_id).map_err(|_| ReadError::InvalidResponse)?;
        let cp = checkpoint.cloned().unwrap_or(GmailCheckpoint {
            history_id: profile.history_id,
            page_token: None,
            backfill: true,
        });
        decimal(&cp.history_id)?;
        if let Some(p) = &cp.page_token {
            atom(p, 2048)?;
        }
        let page = if cp.backfill {
            self.backfill(token, scope, cp, cancel).await?
        } else {
            self.incremental(token, scope, cp, cancel).await?
        };
        super::bounded(page)
    }
    async fn backfill(
        &self,
        token: &SecretString,
        scope: &GmailScope,
        cp: GmailCheckpoint,
        cancel: &CancellationToken,
    ) -> Result<SyncPage<GmailCheckpoint>, ReadError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            messages: Vec<MessageId>,
            next_page_token: Option<String>,
        }
        let query = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query
                .append_pair("q", &format!("after:{}", scope.after_unix))
                .append_pair("maxResults", "15")
                .append_pair("includeSpamTrash", "true");
            for label in &scope.label_ids {
                query.append_pair("labelIds", label);
            }
            if let Some(p) = &cp.page_token {
                query.append_pair("pageToken", p);
            }
            query.finish()
        };
        let page: Page = self
            .get(&format!("messages?{query}"), token, cancel)
            .await
            .map_err(|e| {
                if cp.page_token.is_some() && e == ReadError::Http(400) {
                    ReadError::ResyncRequired
                } else {
                    e
                }
            })?;
        if page.messages.len() > 15 {
            return Err(ReadError::LimitExceeded);
        }
        check_next(&cp.page_token, &page.next_page_token)?;
        let mut changes = Vec::new();
        for message in page.messages {
            if let Some(change) = self
                .message(token, scope, &message.id, false, cancel)
                .await?
            {
                changes.push(change);
            }
        }
        let more = page.next_page_token.is_some();
        Ok(SyncPage {
            changes,
            checkpoint: GmailCheckpoint {
                history_id: cp.history_id,
                page_token: page.next_page_token,
                backfill: more,
            },
            // Backfill finishes into history catch-up; it is not yet complete.
            coverage: Coverage::MorePages,
        })
    }
    async fn incremental(
        &self,
        token: &SecretString,
        scope: &GmailScope,
        cp: GmailCheckpoint,
        cancel: &CancellationToken,
    ) -> Result<SyncPage<GmailCheckpoint>, ReadError> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Page {
            #[serde(default)]
            history: Vec<Record>,
            history_id: String,
            next_page_token: Option<String>,
        }
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Record {
            id: String,
            #[serde(default)]
            messages_added: Vec<Event>,
            #[serde(default)]
            messages_deleted: Vec<Event>,
            #[serde(default)]
            labels_added: Vec<Event>,
            #[serde(default)]
            labels_removed: Vec<Event>,
        }
        #[derive(Deserialize)]
        struct Event {
            message: MessageId,
        }
        let query = {
            let mut query = url::form_urlencoded::Serializer::new(String::new());
            query
                .append_pair("startHistoryId", &cp.history_id)
                .append_pair("maxResults", "15");
            if let Some(p) = &cp.page_token {
                query.append_pair("pageToken", p);
            }
            query.finish()
        };
        let page: Page = self
            .get(&format!("history?{query}"), token, cancel)
            .await
            .map_err(|e| {
                if e == ReadError::Http(404) {
                    ReadError::ResyncRequired
                } else {
                    e
                }
            })?;
        decimal(&page.history_id).map_err(|_| ReadError::InvalidResponse)?;
        check_next(&cp.page_token, &page.next_page_token)?;
        if page.history.len() > 15 {
            return Err(ReadError::LimitExceeded);
        }
        let mut touched = BTreeSet::new();
        let mut deleted = BTreeMap::new();
        for record in page.history {
            decimal(&record.id).map_err(|_| ReadError::InvalidResponse)?;
            for event in record
                .messages_added
                .iter()
                .chain(&record.labels_added)
                .chain(&record.labels_removed)
            {
                id(&event.message.id).map_err(|_| ReadError::InvalidResponse)?;
                touched.insert(event.message.id.clone());
            }
            for event in record.messages_deleted {
                id(&event.message.id).map_err(|_| ReadError::InvalidResponse)?;
                deleted.insert(event.message.id, record.id.clone());
            }
        }
        if touched.len() + deleted.len() > 128 {
            return Err(ReadError::LimitExceeded);
        }
        let mut changes = Vec::new();
        for (external_id, remote_version) in &deleted {
            changes.push(SourceChange::Delete {
                external_id: external_id.clone(),
                remote_version: remote_version.clone(),
            });
        }
        for message in touched {
            if !deleted.contains_key(&message)
                && let Some(change) = self.message(token, scope, &message, true, cancel).await?
            {
                changes.push(change);
            }
        }
        let more = page.next_page_token.is_some();
        Ok(SyncPage {
            changes,
            checkpoint: GmailCheckpoint {
                history_id: if more { cp.history_id } else { page.history_id },
                page_token: page.next_page_token,
                backfill: false,
            },
            coverage: if more {
                Coverage::MorePages
            } else {
                Coverage::GmailHistoryComplete
            },
        })
    }
    async fn message(
        &self,
        token: &SecretString,
        scope: &GmailScope,
        message_id: &str,
        departure: bool,
        cancel: &CancellationToken,
    ) -> Result<Option<SourceChange>, ReadError> {
        id(message_id)?;
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct Wire {
            id: String,
            thread_id: String,
            history_id: String,
            internal_date: String,
            #[serde(default)]
            label_ids: Vec<String>,
            raw: String,
        }
        let wire: Wire = match self.get(&format!("messages/{message_id}?format=raw&fields=id,threadId,historyId,internalDate,labelIds,raw"), token, cancel).await {
            // A race during backfill is no evidence that other records vanished.
            Err(ReadError::Http(404)) => return Ok(None), result => result?,
        };
        if wire.id != message_id || wire.label_ids.len() > 128 {
            return Err(ReadError::InvalidResponse);
        }
        id(&wire.thread_id).map_err(|_| ReadError::InvalidResponse)?;
        decimal(&wire.history_id).map_err(|_| ReadError::InvalidResponse)?;
        let millis: i64 = wire
            .internal_date
            .parse()
            .map_err(|_| ReadError::InvalidResponse)?;
        if millis < 0 {
            return Err(ReadError::InvalidResponse);
        }
        if millis / 1000 <= scope.after_unix
            || !scope
                .label_ids
                .iter()
                .all(|label| wire.label_ids.contains(label))
        {
            return Ok(if departure {
                Some(SourceChange::Delete {
                    external_id: wire.id,
                    remote_version: wire.history_id,
                })
            } else {
                None
            });
        }
        if wire.raw.len() > 700_000 {
            return Err(ReadError::LimitExceeded);
        }
        let raw = URL_SAFE_NO_PAD
            .decode(wire.raw.trim_end_matches('='))
            .map_err(|_| ReadError::InvalidResponse)?;
        let parsed = mail_parser::MessageParser::default()
            .parse(raw.as_slice())
            .ok_or(ReadError::InvalidResponse)?;
        if parsed.subject().is_some_and(|subject| subject.len() > 200)
            || parsed.from().is_some_and(|from| {
                from.iter().count() > 100
                    || from
                        .iter()
                        .any(|address| address.address.as_deref().is_some_and(|a| a.len() > 320))
            })
        {
            return Err(ReadError::LimitExceeded);
        }
        let mail = parse_mail(&raw).map_err(|_| ReadError::InvalidResponse)?;
        if mail.text_truncated {
            return Err(ReadError::LimitExceeded);
        }
        Ok(Some(SourceChange::Upsert(SourceItem {
            external_id: wire.id,
            remote_version: wire.history_id,
            subject: mail.subject,
            text: mail.text,
            speaker: mail.from.join(", "),
            thread_id: wire.thread_id,
            parent_id: None,
            observed_at: millis / 1000,
        })))
    }
    async fn get<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<T, ReadError> {
        let bytes = self
            .transport
            .request(Endpoint::Gmail, path, Some(token), cancel)
            .await?;
        decode(&bytes)
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Profile {
    email_address: String,
    history_id: String,
}
#[derive(Deserialize)]
struct MessageId {
    id: String,
}
fn check_next(previous: &Option<String>, next: &Option<String>) -> Result<(), ReadError> {
    if let Some(next) = next {
        atom(next, 2048).map_err(|_| ReadError::InvalidResponse)?;
        if previous.as_ref() == Some(next) {
            return Err(ReadError::InvalidResponse);
        }
    }
    Ok(())
}
