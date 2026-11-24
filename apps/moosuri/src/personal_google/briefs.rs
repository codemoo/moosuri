//! Live, read-only reader for the connected morning brief.
//!
//! Runs only from the member timer. It never has a member session and never creates one: authority is
//! the pinned background grant from `moosuri_store` (`member_brief_binding`), which fails closed on any
//! connection id / epoch / app revision / auth epoch change. Provider access reuses the existing typed
//! adapters: the broker/local `InboxPage` metadata page (<=5 messages), or, for an `important_only`
//! grant, the local-only `ImportantInboxPage` (Gmail `INBOX`+`IMPORTANT` labels, selected by the
//! provider so newer ordinary mail cannot crowd it out; a broker backend is honestly unavailable), and the Calendar `list` of the
//! member's primary calendar for one local day. No send, no create/move, no readback, no model call.
//!
//! Boundary: the only Calendar grant in this product is the write-scope `calendar.events.owned` grant.
//! This module only ever calls `list` with it. A read-only Calendar scope would be a separate consent.
use super::*;
use calendar::{CalendarProvider, provider_for};
use moosuri_connectors::gmail::GmailError;
use moosuri_core::CalendarEvent;
use moosuri_server::personal_tasks::{BriefReads, ReadFuture};
use moosuri_store::{
    MEMBER_BRIEF_MAX_EVENTS, MEMBER_BRIEF_MAX_MESSAGES, MemberBackgroundBinding, MemberBriefEvent,
    MemberBriefEvidence, MemberBriefLease, MemberBriefMessage, MemberBriefOutcome,
    MemberBriefSource,
};
use std::time::Duration;

const SERIAL_WAIT: Duration = Duration::from_secs(20);
/// Upper bound for one dispatched token refresh before it is settled as withdrawn (`needs_reauth`).
const REFRESH_BOUND: Duration = Duration::from_secs(30);

/// Failures that mean the member must reconnect, not that Google was briefly unreadable.
fn needs_reauth(code: &str) -> bool {
    matches!(
        code,
        "refresh_failed" | "credential_unavailable" | "permission_denied" | "source_changed"
    )
}

pub(super) struct Reader {
    google: Google,
    provider: Arc<dyn CalendarProvider>,
}

pub(super) fn reader(google: Google) -> Arc<dyn BriefReads> {
    Arc::new(Reader {
        provider: provider_for(&google.backend),
        google,
    })
}

/// Latest inbox metadata relevant to this slot: received since the previous slot, newest first,
/// at most five, subject/sender/time only. Importance is not guessed (no body is read).
pub(super) fn select_messages(inbox: &Inbox, since_ms: i64) -> Vec<MemberBriefMessage> {
    let mut kept: Vec<_> = inbox
        .messages
        .iter()
        .filter(|m| m.received_at_ms >= since_ms)
        .collect();
    kept.sort_by(|a, b| b.received_at_ms.cmp(&a.received_at_ms));
    kept.into_iter()
        .take(MEMBER_BRIEF_MAX_MESSAGES)
        .map(|m| MemberBriefMessage {
            subject: m.subject.clone(),
            from: m.from.clone(),
            received_at_ms: m.received_at_ms,
        })
        .collect()
}
/// Title and times only: no description, location, attendees, links or conference data.
pub(super) fn select_events(events: &[CalendarEvent]) -> Vec<MemberBriefEvent> {
    events
        .iter()
        .filter(|e| e.status != "cancelled")
        .take(MEMBER_BRIEF_MAX_EVENTS)
        .map(|e| MemberBriefEvent {
            title: e.fields.title.clone(),
            start: e.fields.start.clone(),
            end: e.fields.end.clone(),
            all_day: e.all_day,
        })
        .collect()
}

impl BriefReads for Reader {
    fn read<'a>(&'a self, lease: &'a MemberBriefLease) -> ReadFuture<'a> {
        Box::pin(async move {
            let mut reauth = false;
            let messages = match lease.gmail {
                Some(source) => self
                    .messages(lease, source)
                    .await
                    .map_err(|e| reauth |= needs_reauth(e))
                    .ok(),
                None => None,
            };
            let events = match lease.calendar {
                Some(source) => self
                    .events(lease, source)
                    .await
                    .map_err(|e| reauth |= needs_reauth(e))
                    .ok(),
                None => None,
            };
            if reauth && messages.is_none() && events.is_none() {
                // Never "completed": tell the member the connection must be re-authorised.
                return MemberBriefOutcome::Unavailable("needs_reauth");
            }
            MemberBriefOutcome::Evidence(MemberBriefEvidence { messages, events })
        })
    }
}

impl Reader {
    /// Whether this runtime can serve the service at all. A backend that cannot must never be asked
    /// to refresh: a failed refresh withdraws the member's grant.
    fn supports(&self, service: Service) -> bool {
        match service {
            Service::Gmail => true,
            Service::Calendar => matches!(self.google.backend, Backend::Local { .. }),
            _ => false,
        }
    }

    /// Only the local gws backend implements the typed `ImportantInboxPage` operation.
    fn supports_important(&self) -> bool {
        matches!(self.google.backend, Backend::Local { .. })
    }

    async fn binding(
        &self,
        lease: &MemberBriefLease,
        source: MemberBriefSource,
    ) -> Result<MemberBackgroundBinding, &'static str> {
        if !self.supports(source.service) {
            return Err("provider_unsupported");
        }
        let store = &self.google.store;
        let current = store
            .member_brief_binding(lease, source)
            .await
            .map_err(|_| "credential_unavailable")?;
        if !current.needs_refresh {
            return Ok(current);
        }
        let google = Google {
            service: source.service,
            ..self.google.clone()
        };
        let pinned_lease = lease.clone();
        // Claim, dispatch and settlement run in a spawned, bounded task
        // that is independent of this (cancellable / timed-out) read future, so the marker can never be
        // left `dispatched`. Any failure, timeout or lost reply settles `None` => `needs_reauth`.
        let settle_store = store.clone();
        let task = tokio::spawn(async move {
            let refresh = settle_store
                .claim_member_brief_refresh(&pinned_lease, source)
                .await
                .map_err(|_| "refresh_busy")?;
            let id = refresh.request_id;
            let attempt = tokio::time::timeout(REFRESH_BOUND, async {
                google
                    .raw(
                        id,
                        Some(refresh.app_revision),
                        Operation::Refresh {
                            refresh_token: Secret(&refresh.refresh_token),
                            refresh_binding: &refresh.refresh_binding,
                        },
                    )
                    .await
                    .and_then(|raw| {
                        moosuri_store::MemberGoogleGrant::from_broker_reply(id, raw)
                            .map_err(Failure::from)
                    })
            })
            .await;
            match attempt {
                Ok(Ok(grant)) => settle_store
                    .settle_member_brief_refresh(refresh, Some(grant))
                    .await
                    .map_err(|_| "refresh_failed"),
                _ => {
                    // Rotation may have happened upstream: withdraw, never retry the old token.
                    let _ = settle_store
                        .settle_member_brief_refresh(refresh, None)
                        .await;
                    Err("refresh_failed")
                }
            }
        });
        task.await.map_err(|_| "refresh_failed")??;
        store
            .member_brief_binding(lease, source)
            .await
            .map_err(|_| "credential_unavailable")
    }

    async fn messages(
        &self,
        lease: &MemberBriefLease,
        source: MemberBriefSource,
    ) -> Result<Vec<MemberBriefMessage>, &'static str> {
        // Important-only needs the typed local operation; the closed broker protocol is not extended.
        // Refuse before any binding/refresh so an unsupported runtime never touches the credential.
        if lease.important_only && !self.supports_important() {
            return Err("provider_unsupported");
        }
        let _serial = tokio::time::timeout(SERIAL_WAIT, self.google.serial.lock())
            .await
            .map_err(|_| "busy")?;
        let binding = self.binding(lease, source).await?;
        let (access_token, access_binding) = (
            Secret(&binding.access_token),
            binding.access_binding.as_str(),
        );
        let inbox = if lease.important_only {
            let page: BrokerReply<ImportantInbox> = self
                .google
                .call(
                    Some(source.app_revision),
                    Operation::ImportantInboxPage {
                        access_token,
                        access_binding,
                    },
                )
                .await
                .map_err(|_| "gmail_read_failed")?;
            page.result.into_inbox()
        } else {
            let page: BrokerReply<Inbox> = self
                .google
                .call(
                    Some(source.app_revision),
                    Operation::InboxPage {
                        access_token,
                        access_binding,
                    },
                )
                .await
                .map_err(|_| "gmail_read_failed")?;
            Some(page.result)
        };
        // A disconnect or epoch change during the read discards the result.
        self.google
            .store
            .check_member_brief_binding(&binding)
            .await
            .map_err(|_| "source_changed")?;
        let inbox = inbox
            .filter(|i| i.valid() && i.mailbox.eq_ignore_ascii_case(&binding.mailbox))
            .ok_or("gmail_read_failed")?;
        Ok(select_messages(&inbox, lease.since_ms))
    }

    async fn events(
        &self,
        lease: &MemberBriefLease,
        source: MemberBriefSource,
    ) -> Result<Vec<MemberBriefEvent>, &'static str> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Verified {
            mailbox: String,
        }
        let _serial = tokio::time::timeout(SERIAL_WAIT, self.google.serial.lock())
            .await
            .map_err(|_| "busy")?;
        let binding = self.binding(lease, source).await?;
        let google = Google {
            service: Service::Calendar,
            ..self.google.clone()
        };
        // The signed access binding is verified for this owner/app/purpose before the token leaves.
        let verified: BrokerReply<Verified> = google
            .call(
                Some(source.app_revision),
                Operation::VerifyAccess {
                    access_token: Secret(&binding.access_token),
                    access_binding: &binding.access_binding,
                },
            )
            .await
            .map_err(|_| "calendar_read_failed")?;
        if !verified
            .result
            .mailbox
            .eq_ignore_ascii_case(&binding.mailbox)
        {
            return Err("calendar_read_failed");
        }
        google
            .store
            .check_member_brief_binding(&binding)
            .await
            .map_err(|_| "source_changed")?;
        let primary = binding.mailbox.to_ascii_lowercase();
        let (start, end) = &lease.window;
        let page = self
            .provider
            .list(&binding.access_token, &primary, start, end)
            .await
            .map_err(|e| match e {
                GmailError::Http(401 | 403) => "permission_denied",
                _ => "calendar_read_failed",
            })?;
        self.google
            .store
            .check_member_brief_binding(&binding)
            .await
            .map_err(|_| "source_changed")?;
        Ok(select_events(&page.events))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use moosuri_core::CalendarFields;
    use moosuri_store::MemberGoogleInboxMessage;

    fn message(id: &str, at: i64, subject: &str) -> moosuri_store::MemberGoogleInboxMessage {
        MemberGoogleInboxMessage {
            id: id.into(),
            received_at_ms: at,
            subject: subject.into(),
            from: "sender@example.test".into(),
            date: "synthetic date".into(),
        }
    }
    fn event(id: &str, status: &str, title: &str) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            etag: "\"e\"".into(),
            status: status.into(),
            fields: CalendarFields {
                title: title.into(),
                description: "PRIVATE-DESCRIPTION".into(),
                location: "PRIVATE-PLACE".into(),
                start: "2026-11-26T10:00:00+09:00".into(),
                end: "2026-11-26T11:00:00+09:00".into(),
                time_zone: Some("Asia/Seoul".into()),
            },
            html_link: Some("https://example.test/private".into()),
            writable: true,
            all_day: false,
            updated: None,
        }
    }

    #[test]
    fn important_reply_is_parsed_only_with_its_own_scope_and_stays_bounded_and_empty_safe() {
        let wire = |scope: &str, count: usize| {
            let messages: Vec<_> = (0..count)
                .map(|i| serde_json::json!({"id": format!("m{i}"), "received_at_ms": 1000 + i as i64, "subject": format!("important {i}"), "from": "sender@example.test", "date": "d"}))
                .collect();
            serde_json::json!({"mailbox": "member@example.test", "messages": messages, "has_more": true, "scope": scope})
        };
        let parse = |v: serde_json::Value| {
            serde_json::from_value::<ImportantInbox>(v)
                .ok()
                .and_then(ImportantInbox::into_inbox)
        };
        // A reply from a backend that ignored the label (ordinary scope) is never accepted as important.
        assert!(parse(wire("one_inbox_page_metadata_only", 2)).is_none());
        // Unknown fields are rejected: the closed shape cannot carry extra data (bodies, snippets).
        let mut extra = wire(crate::personal_google::IMPORTANT_SCOPE, 1);
        extra["snippet"] = serde_json::json!("body text");
        assert!(parse(extra).is_none());
        // More than the page bound is invalid even with the right scope.
        assert!(parse(wire(crate::personal_google::IMPORTANT_SCOPE, 6)).is_none());
        let page = parse(wire(crate::personal_google::IMPORTANT_SCOPE, 5)).unwrap();
        assert_eq!(select_messages(&page, 0).len(), MEMBER_BRIEF_MAX_MESSAGES);
        // Nothing important since the previous slot (or at all) is an honest empty list, not an error.
        assert!(select_messages(&page, 10_000).is_empty());
        assert!(
            select_messages(
                &parse(wire(crate::personal_google::IMPORTANT_SCOPE, 0)).unwrap(),
                0
            )
            .is_empty()
        );
    }

    #[test]
    fn synthetic_provider_pages_are_reduced_to_the_minimum_needed() {
        let inbox = Inbox {
            mailbox: "member@example.test".into(),
            messages: vec![
                message("old", 10, "stale"),
                message("a", 500, "first"),
                message("b", 900, "newest"),
                message("c", 700, "third"),
            ],
            has_more: true,
            scope: "one_inbox_page_metadata_only".into(),
        };
        assert!(inbox.valid());
        let picked = select_messages(&inbox, 100);
        // Older than the previous slot is dropped; newest first; only subject/sender/time survive.
        assert_eq!(
            picked
                .iter()
                .map(|m| m.subject.as_str())
                .collect::<Vec<_>>(),
            ["newest", "third", "first"]
        );
        assert!(picked.len() <= MEMBER_BRIEF_MAX_MESSAGES);

        let picked = select_events(&[
            event("1", "confirmed", "standup"),
            event("2", "cancelled", "dropped"),
        ]);
        assert_eq!(picked.len(), 1);
        let json = serde_json::to_string(&picked).unwrap();
        assert!(
            json.contains("standup")
                && !json.contains("PRIVATE")
                && !json.contains("example.test/private")
        );
    }
}
