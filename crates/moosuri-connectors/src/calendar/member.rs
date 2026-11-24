//! Member primary-calendar timed create/move on the existing `CalendarClient`.
//! Typed operations only; no caller path/URL. The caller must have committed its
//! dispatch ledger row before calling a write. Writes are sent exactly once: a
//! lost acknowledgement is resolved by `readback`, never by a second write.
//! Provider ids are stable (derived from the request id) so create cannot duplicate.
use super::*;
use sha2::{Digest, Sha256};

/// Google event ids accept lowercase base32hex (a-v, 0-9), 5..=1024 chars.
pub fn stable_event_id(request_id: &str) -> String {
    const A: &[u8; 32] = b"0123456789abcdefghijklmnopqrstuv";
    let mut h = Sha256::new();
    h.update(b"moosuri/member-calendar/event-id/v1\0");
    h.update(request_id.as_bytes());
    let d = h.finalize();
    let (mut out, mut acc, mut bits) = (String::new(), 0u32, 0u32);
    for b in &d[..16] {
        acc = (acc << 8) | u32::from(*b);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(A[((acc >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(A[((acc << (5 - bits)) & 31) as usize] as char);
    }
    out
}

pub enum MemberWriteError {
    /// No write was sent: the event is no longer at the approved ETag.
    Stale(Box<CalendarEvent>),
    /// No write was sent: not a writable timed owner event.
    NotWritable,
    /// Failure before any mutation request was made.
    BeforeWrite(GmailError),
    /// The single mutation was (or may have been) sent. Outcome is `readback`'s job.
    Write(GmailError),
}
pub enum Readback {
    /// Carries this exact operation marker, digest and fields.
    Applied(Box<CalendarEvent>),
    /// Exists, but is not proven to be this operation's effect.
    Different(Box<CalendarEvent>),
}
#[derive(Clone)]
pub struct MemberCalendar {
    client: CalendarClient,
}
impl MemberCalendar {
    /// Real Google endpoint. Constructing this is the only way real IO is enabled.
    pub fn google() -> Self {
        Self {
            client: CalendarClient::google(),
        }
    }
    /// Member-local egress through the existing local Google proxy.
    pub fn google_proxy(address: std::net::SocketAddr) -> Result<Self, GmailError> {
        Ok(Self {
            client: CalendarClient::google_proxy(address)?,
        })
    }
    #[cfg(feature = "test-fixtures")]
    #[doc(hidden)]
    pub fn synthetic_loopback(address: std::net::SocketAddr) -> Result<Self, GmailError> {
        Ok(Self {
            client: CalendarClient::synthetic_loopback(address)?,
        })
    }
    #[cfg(test)]
    fn fixture(base: String) -> Self {
        Self {
            client: CalendarClient::fixture(base),
        }
    }
    /// One page (<=100) of the member's primary calendar. `partial` is never hidden.
    pub async fn list(
        &self,
        token: &SecretString,
        primary: &str,
        start: &str,
        end: &str,
        cancel: &CancellationToken,
    ) -> Result<CalendarEvents, GmailError> {
        let range = CalendarRange {
            connection_id: String::new(),
            start: start.into(),
            end: end.into(),
            page_token: None,
        };
        self.client
            .events_page(token, primary, &range, 100, cancel)
            .await
    }
    pub async fn read(
        &self,
        token: &SecretString,
        primary: &str,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<CalendarEvent, GmailError> {
        Ok(self.client.event(token, primary, id, cancel).await?.event)
    }
    /// Single POST with a caller-stable id. 409 (id exists) is an `Err(Write)` the
    /// caller must treat as unknown and reconcile by readback.
    pub async fn create(
        &self,
        token: &SecretString,
        primary: &str,
        id: &str,
        fields: &CalendarFields,
        operation: &str,
        digest: &str,
        cancel: &CancellationToken,
    ) -> Result<CalendarEvent, MemberWriteError> {
        timed(fields).map_err(MemberWriteError::BeforeWrite)?;
        let change = CalendarMutation::Create {
            fields: fields.clone(),
        };
        let r = self
            .client
            .mutate(
                token,
                CalendarWrite {
                    primary,
                    id,
                    change: &change,
                    operation,
                    digest,
                    prior: None,
                },
                cancel,
            )
            .await
            .map_err(MemberWriteError::Write)?;
        r.event
            .map(|o| o.event)
            .ok_or(MemberWriteError::Write(GmailError::InvalidResponse))
    }
    /// Fresh GET, compare with the approved ETag, then one PATCH with `If-Match`.
    pub async fn move_event(
        &self,
        token: &SecretString,
        primary: &str,
        id: &str,
        expected_etag: &str,
        fields: &CalendarFields,
        operation: &str,
        digest: &str,
        cancel: &CancellationToken,
    ) -> Result<CalendarEvent, MemberWriteError> {
        timed(fields).map_err(MemberWriteError::BeforeWrite)?;
        let prior = self
            .client
            .event(token, primary, id, cancel)
            .await
            .map_err(MemberWriteError::BeforeWrite)?;
        if prior.event.etag != expected_etag {
            return Err(MemberWriteError::Stale(Box::new(prior.event)));
        }
        if !prior.event.writable || prior.event.all_day {
            return Err(MemberWriteError::NotWritable);
        }
        let change = CalendarMutation::Update {
            event_id: id.into(),
            etag: expected_etag.into(),
            fields: fields.clone(),
        };
        let r = self
            .client
            .mutate(
                token,
                CalendarWrite {
                    primary,
                    id,
                    change: &change,
                    operation,
                    digest,
                    prior: Some(&prior),
                },
                cancel,
            )
            .await
            .map_err(|e| match e {
                // A precondition failure is a definite no-write.
                GmailError::Http(412) => MemberWriteError::Stale(Box::new(prior.event.clone())),
                e => MemberWriteError::Write(e),
            })?;
        r.event
            .map(|o| o.event)
            .ok_or(MemberWriteError::Write(GmailError::InvalidResponse))
    }
    /// Read-only reconciliation of a possibly-lost write.
    pub async fn readback(
        &self,
        token: &SecretString,
        primary: &str,
        id: &str,
        operation: &str,
        digest: &str,
        fields: &CalendarFields,
        cancel: &CancellationToken,
    ) -> Result<Readback, GmailError> {
        let o = self.client.event(token, primary, id, cancel).await?;
        let change = CalendarMutation::Create {
            fields: fields.clone(),
        };
        Ok(if observed_change(&o, id, operation, digest, &change) {
            Readback::Applied(Box::new(o.event))
        } else {
            Readback::Different(Box::new(o.event))
        })
    }
}
/// Timed events only: all-day civil-date shapes are refused here, not reinterpreted.
fn timed(f: &CalendarFields) -> Result<(), GmailError> {
    validate_fields(f)?;
    if date_range(&f.start, &f.end).is_ok() || f.time_zone.is_none() {
        return Err(GmailError::InvalidInput);
    }
    Ok(())
}
#[cfg(test)]
mod tests;
