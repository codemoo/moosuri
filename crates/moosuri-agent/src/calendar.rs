//! Shared typed Calendar IO for authenticated owner forms.
//! This facade grants no agent/tool rights and owns no background worker.
use crate::mail::MailService;
use moosuri_connectors::{
    calendar::{CalendarClient, CalendarWrite, EventObservation},
    gmail::{GmailError, SecretString},
    model::CancellationToken,
};
use moosuri_core::*;
use moosuri_store::{
    CalendarAdmission, CalendarBinding, CalendarDispatch, GoogleApp, Store, StoreError, UserContext,
};
use std::sync::Arc;

/// Only authenticated UserContext callers can use this service. Tokens and wire
/// observations stay in the trusted IO layer; no model-facing API is added.
#[derive(Clone)]
pub struct CalendarService {
    store: Store,
    mail: MailService,
    io: Arc<CalendarIo>,
}

// Production always uses the existing fixed-origin connector. The private test
// variant counts typed IO without adding a configurable production destination.
enum CalendarIo {
    Google(Box<CalendarClient>),
    #[cfg(test)]
    Fixture(Arc<tests::FixtureIo>),
}
enum Observation {
    Google(Box<EventObservation>),
    #[cfg(test)]
    Fixture(Box<tests::FixtureObservation>),
}
impl Observation {
    fn event(&self) -> &CalendarEvent {
        match self {
            Self::Google(v) => &v.event,
            #[cfg(test)]
            Self::Fixture(v) => &v.event,
        }
    }
    fn into_event(self) -> CalendarEvent {
        match self {
            Self::Google(v) => v.event,
            #[cfg(test)]
            Self::Fixture(v) => v.event,
        }
    }
    fn matches(
        &self,
        event: &str,
        operation: &str,
        digest: &str,
        mutation: &CalendarMutation,
    ) -> bool {
        match self {
            Self::Google(v) => {
                moosuri_connectors::calendar::observed_change(v, event, operation, digest, mutation)
            }
            #[cfg(test)]
            Self::Fixture(v) => v.matches(event, operation, digest, mutation),
        }
    }
}
struct MutationResult {
    status: u16,
    event: Option<CalendarEvent>,
}
impl CalendarIo {
    async fn primary(
        &self,
        token: &SecretString,
        cancel: &CancellationToken,
    ) -> Result<CalendarInfo, GmailError> {
        match self {
            Self::Google(client) => client.primary(token, cancel).await,
            #[cfg(test)]
            Self::Fixture(io) => io.primary().await,
        }
    }
    async fn calendars(
        &self,
        token: &SecretString,
        page: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<CalendarList, GmailError> {
        match self {
            Self::Google(client) => client.calendars(token, page, cancel).await,
            #[cfg(test)]
            Self::Fixture(io) => io.calendars(page),
        }
    }
    async fn events(
        &self,
        token: &SecretString,
        primary: &str,
        range: &CalendarRange,
        cancel: &CancellationToken,
    ) -> Result<CalendarEvents, GmailError> {
        match self {
            Self::Google(client) => client.events(token, primary, range, cancel).await,
            #[cfg(test)]
            Self::Fixture(io) => io.events(primary, range).await,
        }
    }
    async fn event(
        &self,
        token: &SecretString,
        primary: &str,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<Observation, GmailError> {
        match self {
            Self::Google(client) => client
                .event(token, primary, id, cancel)
                .await
                .map(|v| Observation::Google(Box::new(v))),
            #[cfg(test)]
            Self::Fixture(io) => io.event(primary, id),
        }
    }
    async fn mutate(
        &self,
        token: &SecretString,
        dispatch: &CalendarDispatch,
        prior: Option<&Observation>,
        cancel: &CancellationToken,
    ) -> Result<MutationResult, GmailError> {
        match self {
            Self::Google(client) => {
                let prior = prior.map(|v| match v {
                    Observation::Google(v) => v.as_ref(),
                    #[cfg(test)]
                    Observation::Fixture(_) => {
                        unreachable!("fixture observation cannot reach production IO")
                    }
                });
                client
                    .mutate(
                        token,
                        CalendarWrite {
                            primary: &dispatch.primary_id,
                            id: &dispatch.event_id,
                            change: &dispatch.change.mutation,
                            operation: &dispatch.change.request_id,
                            digest: &dispatch.digest,
                            prior,
                        },
                        cancel,
                    )
                    .await
                    .map(|v| MutationResult {
                        status: v.status,
                        event: v.event.map(|v| v.event),
                    })
            }
            #[cfg(test)]
            Self::Fixture(io) => io.mutate(dispatch, prior).await,
        }
    }
}
fn remote(error: GmailError) -> StoreError {
    match error {
        GmailError::InvalidInput => StoreError::InvalidInput,
        GmailError::Http(401 | 403) => StoreError::Forbidden,
        GmailError::Http(404 | 410) => StoreError::NotFound,
        GmailError::Http(409 | 412) => StoreError::Conflict,
        _ => StoreError::Unavailable,
    }
}
impl CalendarService {
    pub fn new(store: Store, mail: MailService) -> Self {
        Self {
            store,
            mail,
            io: Arc::new(CalendarIo::Google(Box::new(CalendarClient::google()))),
        }
    }
    async fn app(&self) -> Result<GoogleApp, StoreError> {
        self.mail
            .oauth_client()
            .await?
            .ok_or(StoreError::Unavailable)
    }
    async fn binding(&self, u: &UserContext, id: String) -> Result<CalendarBinding, StoreError> {
        let app = self.app().await?;
        let mut b = self
            .store
            .calendar_binding(u, id.clone(), app.pin.clone())
            .await?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if b.expires_at <= now + 60 {
            let token = self
                .mail
                .client()
                .refresh(&app.client, &b.refresh, &CancellationToken::new())
                .await;
            match token {
                Ok(tokens) => self.store.refresh_calendar(&b, tokens).await?,
                Err(e) => {
                    if matches!(e, GmailError::Http(400 | 401 | 403)) {
                        let _ = self.store.calendar_needs_reconnect(&b).await;
                    }
                    return Err(remote(e));
                }
            }
            b = self.store.calendar_binding(u, id, app.pin).await?;
        }
        Ok(b)
    }
    async fn primary(&self, b: &CalendarBinding) -> Result<CalendarInfo, StoreError> {
        let p = self
            .io
            .primary(&b.access, &CancellationToken::new())
            .await
            .map_err(remote)?;
        self.store.check_calendar_binding(b).await?;
        if p.id != b.connection.primary_id {
            return Err(StoreError::Conflict);
        }
        Ok(p)
    }
    pub async fn calendars(
        &self,
        u: &UserContext,
        id: String,
        page_token: Option<String>,
    ) -> Result<CalendarList, StoreError> {
        let b = self.binding(u, id).await?;
        self.primary(&b).await?;
        let page = self
            .io
            .calendars(&b.access, page_token.as_deref(), &CancellationToken::new())
            .await
            .map_err(remote)?;
        self.store.check_calendar_binding(&b).await?;
        Ok(page)
    }
    pub async fn events(
        &self,
        u: &UserContext,
        q: CalendarRange,
    ) -> Result<CalendarEvents, StoreError> {
        let b = self.binding(u, q.connection_id.clone()).await?;
        let p = self.primary(&b).await?;
        let page = self
            .io
            .events(&b.access, &p.id, &q, &CancellationToken::new())
            .await
            .map_err(remote)?;
        self.store.check_calendar_binding(&b).await?;
        Ok(page)
    }
    pub async fn change(
        &self,
        u: &UserContext,
        input: CalendarChange,
    ) -> Result<CalendarAction, StoreError> {
        if let Some(existing) = self
            .store
            .existing_calendar_change(u, input.clone())
            .await?
        {
            return Ok(existing);
        }
        let b = self.binding(u, input.connection_id.clone()).await?;
        let p = self.primary(&b).await?;
        let cancel = CancellationToken::new();
        let prior = match &input.mutation {
            CalendarMutation::Create { .. } => None,
            CalendarMutation::Update { event_id, .. }
            | CalendarMutation::Delete { event_id, .. } => Some(
                self.io
                    .event(&b.access, &p.id, event_id, &cancel)
                    .await
                    .map_err(remote)?,
            ),
        };
        self.store.check_calendar_binding(&b).await?;
        let admission = self
            .store
            .admit_calendar_change(&b, input, p, prior.as_ref().map(|v| v.event().clone()))
            .await?;
        let dispatch = match admission {
            CalendarAdmission::Recorded(v) => return Ok(v),
            CalendarAdmission::Dispatch(d) => d,
        };
        // This is the only write invocation. No retry or refresh is performed after commitment.
        let result = self
            .io
            .mutate(&b.access, &dispatch, prior.as_ref(), &cancel)
            .await;
        let (state, event, receipt, error) = match result {
            Ok(result) => {
                let etag = result.event.as_ref().map(|e| e.etag.clone());
                let receipt=serde_json::json!({"provider":"google_calendar","http_status":result.status,"operation_id":dispatch.change.request_id,"event_id":dispatch.event_id,"etag":etag}).to_string();
                ("applied", result.event, receipt, None)
            }
            Err(e) => {
                let state = match e {
                    GmailError::Http(409 | 412) => "conflict",
                    GmailError::Http(400 | 401 | 403 | 404 | 410) | GmailError::InvalidInput => {
                        "failed_definite"
                    }
                    _ => "outcome_unknown",
                };
                let receipt = serde_json::json!({
                    "provider": "google_calendar",
                    "state": state,
                    "http_status": match e { GmailError::Http(status) => Some(status), _ => None },
                })
                .to_string();
                let error = if state == "outcome_unknown" {
                    "lookup_required"
                } else {
                    "provider_rejected"
                };
                (state, None, receipt, Some(error.into()))
            }
        };
        let action = self
            .store
            .settle_calendar_change(&dispatch, state.into(), event, receipt, error)
            .await?;
        Ok(action)
    }
    pub async fn lookup(&self, u: &UserContext, id: String) -> Result<CalendarAction, StoreError> {
        let app = self.app().await?;
        let d = self.store.calendar_reconcile(u, id, app.pin).await?;
        let b = self.binding(u, d.change.connection_id.clone()).await?;
        let d = self
            .store
            .calendar_reconcile(u, d.change.request_id.clone(), b.app.clone())
            .await?;
        let p = self.primary(&b).await?;
        let observed = self
            .io
            .event(&b.access, &p.id, &d.event_id, &CancellationToken::new())
            .await;
        self.store.check_calendar_binding(&b).await?;
        let (state, event, receipt) = lookup_result(
            &d.change.mutation,
            &d.event_id,
            &d.change.request_id,
            &d.digest,
            observed,
        );
        let action = self
            .store
            .settle_calendar_change(
                &d,
                state.into(),
                event,
                receipt.into(),
                if state == "outcome_unknown" {
                    Some("lookup_inconclusive".into())
                } else {
                    None
                },
            )
            .await?;
        Ok(action)
    }
}

fn lookup_result(
    mutation: &CalendarMutation,
    event_id: &str,
    request_id: &str,
    digest: &str,
    observed: Result<Observation, GmailError>,
) -> (&'static str, Option<CalendarEvent>, &'static str) {
    match observed {
        Ok(v) if v.matches(event_id, request_id, digest, mutation) => (
            "applied",
            Some(v.into_event()),
            "matching_event_and_operation_observed",
        ),
        Err(GmailError::Http(404 | 410)) if matches!(mutation, CalendarMutation::Delete { .. }) => {
            (
                "observed_absent",
                None,
                "current_absence_observed_causality_unknown",
            )
        }
        _ => ("outcome_unknown", None, "no_conclusive_operation_evidence"),
    }
}

#[cfg(test)]
mod tests;
