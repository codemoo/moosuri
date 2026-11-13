//! Fixed-origin Google Calendar. IO is not authority; caller must persist dispatch first.
use crate::gmail::{
    GmailError, SecretString,
    transport::{Body, Transport},
};
use moosuri_core::*;
use reqwest::Method;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;
pub const SCOPES: [&str; 2] = [
    "https://www.googleapis.com/auth/calendar.calendarlist.readonly",
    "https://www.googleapis.com/auth/calendar.events.owned",
];
#[derive(Clone)]
pub struct CalendarClient {
    transport: Transport,
}
/// Trusted wire observation, not a public mutation approval.
pub struct EventObservation {
    pub event: CalendarEvent,
    pub primary_id: String,
    pub operation: Option<String>,
    pub digest: Option<String>,
    private: Value,
}
pub struct CalendarMutationResult {
    pub status: u16,
    pub event: Option<EventObservation>,
}
/// A single already-admitted owner mutation and its exact reconciliation identity.
pub struct CalendarWrite<'a> {
    pub primary: &'a str,
    pub id: &'a str,
    pub change: &'a CalendarMutation,
    pub operation: &'a str,
    pub digest: &'a str,
    pub prior: Option<&'a EventObservation>,
}
impl CalendarClient {
    pub fn google() -> Self {
        Self {
            transport: Transport::google(),
        }
    }
    #[cfg(test)]
    fn fixture(base: String) -> Self {
        Self {
            transport: Transport::fixture(base, 1024 * 1024),
        }
    }
    async fn request(
        &self,
        token: &SecretString,
        method: Method,
        path: &str,
        body: Body<'_>,
        etag: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<(u16, Value), GmailError> {
        let (status, _, bytes) = self
            .transport
            .calendar(method, path, token, body, etag, cancel)
            .await?;
        let value = if status == 204 {
            Value::Null
        } else {
            serde_json::from_slice(&bytes).map_err(|_| GmailError::InvalidResponse)?
        };
        Ok((status, value))
    }
    pub async fn primary(
        &self,
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<CalendarInfo, GmailError> {
        let (_, v) = self
            .request(
                token,
                Method::GET,
                "users/me/calendarList/primary",
                Body::Empty,
                None,
                cancel,
            )
            .await?;
        let p = info(&v)?;
        if !p.primary || p.access_role != "owner" {
            return Err(GmailError::InvalidResponse);
        }
        Ok(p)
    }
    pub async fn calendars(
        &self,
        token: &SecretString,
        page: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<CalendarList, GmailError> {
        let path = query(
            "users/me/calendarList",
            vec![("maxResults", "100"), ("showDeleted", "false")],
            page,
        )?;
        let (_, v) = self
            .request(token, Method::GET, &path, Body::Empty, None, cancel)
            .await?;
        let rows = v["items"].as_array().ok_or(GmailError::InvalidResponse)?;
        if rows.len() > 100 {
            return Err(GmailError::LimitExceeded);
        }
        let next = optional(&v, "nextPageToken", 4096)?;
        Ok(CalendarList {
            items: rows.iter().map(info).collect::<Result<_, _>>()?,
            partial: next.is_some(),
            next_page_token: next,
        })
    }
    pub async fn events(
        &self,
        token: &SecretString,
        primary: &str,
        range: &CalendarRange,
        cancel: &CancellationToken,
    ) -> Result<CalendarEvents, GmailError> {
        validate_range(&range.start, &range.end)?;
        let path = query(
            "calendars/primary/events",
            vec![
                ("maxResults", "100"),
                ("singleEvents", "true"),
                ("showDeleted", "false"),
                ("orderBy", "startTime"),
                ("timeMin", &range.start),
                ("timeMax", &range.end),
            ],
            range.page_token.as_deref(),
        )?;
        let (_, v) = self
            .request(token, Method::GET, &path, Body::Empty, None, cancel)
            .await?;
        let rows = v["items"].as_array().ok_or(GmailError::InvalidResponse)?;
        if rows.len() > 100 {
            return Err(GmailError::LimitExceeded);
        }
        let next = optional(&v, "nextPageToken", 4096)?;
        Ok(CalendarEvents {
            events: rows
                .iter()
                .filter(|v| v["status"] != "cancelled")
                .map(|v| observation(v, primary).map(|v| v.event))
                .collect::<Result<_, _>>()?,
            partial: next.is_some(),
            next_page_token: next,
            primary_id: primary.into(),
        })
    }
    pub async fn event(
        &self,
        token: &SecretString,
        primary: &str,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<EventObservation, GmailError> {
        event_id(id)?;
        let (_, v) = self
            .request(
                token,
                Method::GET,
                &format!("calendars/primary/events/{id}"),
                Body::Empty,
                None,
                cancel,
            )
            .await?;
        if v["status"] == "cancelled" {
            return Err(GmailError::Http(410));
        }
        let result = observation(&v, primary)?;
        if result.event.id != id {
            return Err(GmailError::InvalidResponse);
        }
        Ok(result)
    }
    /// Exactly one mutation; no retry/refresh loop belongs after dispatch admission.
    pub async fn mutate(
        &self,
        token: &SecretString,
        write: CalendarWrite<'_>,
        cancel: &CancellationToken,
    ) -> Result<CalendarMutationResult, GmailError> {
        let CalendarWrite {
            primary,
            id,
            change,
            operation,
            digest,
            prior,
        } = write;
        event_id(id)?;
        if operation.len() != 36 || digest.len() != 64 {
            return Err(GmailError::InvalidInput);
        }
        let base = format!("calendars/primary/events/{id}");
        let query = "sendUpdates=none&sendNotifications=false";
        let (method, path, body, etag) = match change {
            CalendarMutation::Create { fields } => {
                validate_fields(fields)?;
                let mut b = event_body(fields, operation, digest, &json!({}));
                b["id"] = json!(id);
                (
                    Method::POST,
                    format!("calendars/primary/events?{query}"),
                    Body::Json(b),
                    None,
                )
            }
            CalendarMutation::Update {
                event_id,
                etag,
                fields,
            } => {
                validate_fields(fields)?;
                let p = prior.ok_or(GmailError::InvalidInput)?;
                if p.primary_id != primary
                    || p.event.id != *event_id
                    || event_id != id
                    || !p.event.writable
                    || p.event.etag != *etag
                {
                    return Err(GmailError::InvalidInput);
                }
                (
                    Method::PATCH,
                    format!("{base}?{query}"),
                    Body::Json(event_body(fields, operation, digest, &p.private)),
                    Some(etag.as_str()),
                )
            }
            CalendarMutation::Delete { event_id, etag } => {
                let p = prior.ok_or(GmailError::InvalidInput)?;
                if p.primary_id != primary
                    || event_id != id
                    || p.event.id != id
                    || !p.event.writable
                    || p.event.etag != *etag
                {
                    return Err(GmailError::InvalidInput);
                }
                (
                    Method::DELETE,
                    format!("{base}?{query}"),
                    Body::Empty,
                    Some(etag.as_str()),
                )
            }
        };
        let (status, v) = self
            .request(token, method, &path, body, etag, cancel)
            .await?;
        if matches!(change, CalendarMutation::Delete { .. }) {
            return if status == 204 {
                Ok(CalendarMutationResult {
                    status,
                    event: None,
                })
            } else {
                Err(GmailError::InvalidResponse)
            };
        }
        let observed = observation(&v, primary)?;
        if observed.event.id != id
            || observed.operation.as_deref() != Some(operation)
            || observed.digest.as_deref() != Some(digest)
            || !matches_fields(&observed.event, change)
        {
            return Err(GmailError::InvalidResponse);
        }
        Ok(CalendarMutationResult {
            status,
            event: Some(observed),
        })
    }
}
fn event_body(f: &CalendarFields, operation: &str, digest: &str, private: &Value) -> Value {
    let mut p = private.as_object().cloned().unwrap_or_default();
    p.insert("moosuri_operation".into(), json!(operation));
    p.insert("moosuri_digest".into(), json!(digest));
    let stamp = |v: &str| {
        let mut t = json!({"dateTime":v});
        if let Some(z) = &f.time_zone {
            t["timeZone"] = json!(z)
        }
        t
    };
    json!({"summary":f.title,"description":f.description,"location":f.location,"start":stamp(&f.start),"end":stamp(&f.end),"reminders":{"useDefault":false,"overrides":[]},"extendedProperties":{"private":p}})
}
fn text(v: &Value, k: &str, max: usize) -> Result<String, GmailError> {
    let s = v[k].as_str().ok_or(GmailError::InvalidResponse)?;
    if s.is_empty() || s.len() > max || s.chars().any(|c| c == '\0') {
        return Err(GmailError::InvalidResponse);
    }
    Ok(s.into())
}
fn optional(v: &Value, k: &str, max: usize) -> Result<Option<String>, GmailError> {
    match v.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) if s.len() <= max && !s.contains('\0') => Ok(Some(s.clone())),
        _ => Err(GmailError::InvalidResponse),
    }
}
fn info(v: &Value) -> Result<CalendarInfo, GmailError> {
    Ok(CalendarInfo {
        id: text(v, "id", 1024)?,
        title: optional(v, "summary", 1024)?.unwrap_or_default(),
        time_zone: optional(v, "timeZone", 128)?.unwrap_or_default(),
        primary: v["primary"].as_bool().unwrap_or(false),
        access_role: text(v, "accessRole", 32)?,
        email_notifications: v
            .pointer("/notificationSettings/notifications")
            .and_then(|v| v.as_array())
            .is_some_and(|v| v.iter().any(|n| n["method"] == "email")),
    })
}
fn observation(v: &Value, primary: &str) -> Result<EventObservation, GmailError> {
    let id = text(v, "id", 1024)?;
    event_id(&id)?;
    let etag = text(v, "etag", 512)?;
    if etag.chars().any(char::is_control) {
        return Err(GmailError::InvalidResponse);
    }
    let all_day = v.pointer("/start/date").is_some();
    let stamp = |name: &str| -> Result<String, GmailError> {
        let s = v[name]
            .get("dateTime")
            .or_else(|| v[name].get("date"))
            .and_then(|v| v.as_str())
            .ok_or(GmailError::InvalidResponse)?;
        if s.len() > 128 {
            return Err(GmailError::InvalidResponse);
        }
        Ok(s.into())
    };
    let status = optional(v, "status", 32)?.unwrap_or_else(|| "confirmed".into());
    let no_attendees = v
        .get("attendees")
        .is_none_or(|a| a.as_array().is_some_and(|a| a.is_empty()))
        && !v["attendeesOmitted"].as_bool().unwrap_or(false);
    let writable = !all_day
        && status == "confirmed"
        && no_attendees
        && v.get("recurrence").is_none()
        && v.get("recurringEventId").is_none()
        && v.get("eventType")
            .and_then(|v| v.as_str())
            .is_none_or(|s| s == "default")
        && v.pointer("/organizer/self").and_then(|v| v.as_bool()) == Some(true)
        && v.pointer("/organizer/email").and_then(|v| v.as_str()) == Some(primary);
    let private = v
        .pointer("/extendedProperties/private")
        .cloned()
        .unwrap_or_else(|| json!({}));
    if serde_json::to_vec(&private)
        .map_err(|_| GmailError::InvalidResponse)?
        .len()
        > 8192
    {
        return Err(GmailError::LimitExceeded);
    }
    Ok(EventObservation {
        event: CalendarEvent {
            id,
            etag,
            status,
            fields: CalendarFields {
                title: optional(v, "summary", 1024)?.unwrap_or_default(),
                description: optional(v, "description", 12000)?.unwrap_or_default(),
                location: optional(v, "location", 2000)?.unwrap_or_default(),
                start: stamp("start")?,
                end: stamp("end")?,
                time_zone: optional(&v["start"], "timeZone", 128)?,
            },
            html_link: optional(v, "htmlLink", 2048)?,
            writable,
            all_day,
            updated: optional(v, "updated", 128)?,
        },
        primary_id: primary.into(),
        operation: private
            .get("moosuri_operation")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        digest: private
            .get("moosuri_digest")
            .and_then(|v| v.as_str())
            .map(str::to_owned),
        private,
    })
}
pub fn validate_range(start: &str, end: &str) -> Result<(), GmailError> {
    let a = chrono::DateTime::parse_from_rfc3339(start).map_err(|_| GmailError::InvalidInput)?;
    let b = chrono::DateTime::parse_from_rfc3339(end).map_err(|_| GmailError::InvalidInput)?;
    let duration = b.signed_duration_since(a);
    if duration.num_seconds() <= 0 || duration.num_seconds() > 31 * 86400 {
        return Err(GmailError::InvalidInput);
    }
    Ok(())
}
pub fn validate_fields(f: &CalendarFields) -> Result<(), GmailError> {
    if f.title.trim().is_empty()
        || f.title.len() > 1024
        || f.description.len() > 12000
        || f.location.len() > 2000
        || [&f.title, &f.description, &f.location]
            .iter()
            .any(|s| s.contains('\0'))
        || f.time_zone
            .as_ref()
            .is_some_and(|v| v.is_empty() || v.len() > 128 || v.chars().any(char::is_control))
    {
        return Err(GmailError::InvalidInput);
    }
    validate_range(&f.start, &f.end)
}
/// Lookup can prove only this exact operation's requested current fields.
pub fn observed_change(
    v: &EventObservation,
    id: &str,
    operation: &str,
    digest: &str,
    change: &CalendarMutation,
) -> bool {
    v.event.id == id
        && v.operation.as_deref() == Some(operation)
        && v.digest.as_deref() == Some(digest)
        && matches_fields(&v.event, change)
}
pub fn matches_fields(e: &CalendarEvent, m: &CalendarMutation) -> bool {
    let f = match m {
        CalendarMutation::Create { fields } | CalendarMutation::Update { fields, .. } => fields,
        CalendarMutation::Delete { .. } => return false,
    };
    let same = |a: &str, b: &str| {
        chrono::DateTime::parse_from_rfc3339(a)
            .ok()
            .zip(chrono::DateTime::parse_from_rfc3339(b).ok())
            .is_some_and(|(a, b)| a == b)
    };
    e.fields.title == f.title
        && e.fields.description == f.description
        && e.fields.location == f.location
        && same(&e.fields.start, &f.start)
        && same(&e.fields.end, &f.end)
        && f.time_zone
            .as_ref()
            .is_none_or(|z| e.fields.time_zone.as_ref() == Some(z))
        && e.writable
}
pub fn event_id(id: &str) -> Result<(), GmailError> {
    if id.is_empty()
        || id.len() > 1024
        || !id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'_' | b'-'))
    {
        return Err(GmailError::InvalidInput);
    }
    Ok(())
}
fn query(path: &str, values: Vec<(&str, &str)>, page: Option<&str>) -> Result<String, GmailError> {
    let mut q = url::form_urlencoded::Serializer::new(String::new());
    for (k, v) in values {
        q.append_pair(k, v);
    }
    if let Some(page) = page {
        if page.is_empty() || page.len() > 4096 {
            return Err(GmailError::InvalidInput);
        }
        q.append_pair("pageToken", page);
    }
    Ok(format!("{path}?{}", q.finish()))
}
#[cfg(test)]
mod tests;
