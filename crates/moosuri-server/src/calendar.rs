//! Explicit authenticated Calendar forms. No model/agent mutation admission.
use super::*;
use axum::extract::Query;
use moosuri_connectors::{
    calendar::{CalendarClient, CalendarWrite},
    gmail::{GmailClient, GmailError, SecretString},
    model::CancellationToken,
};
use moosuri_core::*;
use moosuri_store::{CalendarAdmission, CalendarBinding, GoogleApp};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handle {
    handle: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ConnectionQuery {
    connection_id: String,
    page_token: Option<String>,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/calendar/connections", get(connections))
        .route(
            "/api/v1/calendar/connections/{id}",
            axum::routing::delete(disconnect),
        )
        .route("/api/v1/calendar/oauth/start", post(start))
        .route("/api/v1/calendar/oauth/complete", post(complete))
        .route("/api/v1/calendar/calendars", get(calendars))
        .route("/api/v1/calendar/events", post(events))
        .route("/api/v1/calendar/actions", get(actions).post(change))
        .route("/api/v1/calendar/actions/{id}/lookup", post(lookup))
        .layer(DefaultBodyLimit::max(32 * 1024))
}
fn remote(error: GmailError) -> HttpError {
    HttpError(match error {
        GmailError::InvalidInput => StoreError::InvalidInput,
        GmailError::Http(401 | 403) => StoreError::Forbidden,
        GmailError::Http(404 | 410) => StoreError::NotFound,
        GmailError::Http(409 | 412) => StoreError::Conflict,
        _ => StoreError::Unavailable,
    })
}
async fn app(s: &AppState) -> Result<GoogleApp, HttpError> {
    s.mail
        .oauth_client()
        .await?
        .ok_or(HttpError(StoreError::Unavailable))
}
async fn binding(s: &AppState, u: &UserContext, id: String) -> Result<CalendarBinding, HttpError> {
    let app = app(s).await?;
    let mut b = s
        .store
        .calendar_binding(u, id.clone(), app.pin.clone())
        .await?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Unavailable)?
        .as_secs() as i64;
    if b.expires_at <= now + 60 {
        let token = s
            .mail
            .client()
            .refresh(&app.client, &b.refresh, &CancellationToken::new())
            .await;
        match token {
            Ok(tokens) => s.store.refresh_calendar(&b, tokens).await?,
            Err(e) => {
                if matches!(e, GmailError::Http(400 | 401 | 403)) {
                    let _ = s.store.calendar_needs_reconnect(&b).await;
                }
                return Err(remote(e));
            }
        }
        b = s.store.calendar_binding(u, id, app.pin).await?;
    }
    Ok(b)
}
async fn primary(s: &AppState, b: &CalendarBinding) -> Result<CalendarInfo, HttpError> {
    let p = CalendarClient::google()
        .primary(&b.access, &CancellationToken::new())
        .await
        .map_err(remote)?;
    s.store.check_calendar_binding(b).await?;
    if p.id != b.connection.primary_id {
        return Err(HttpError(StoreError::Conflict));
    }
    Ok(p)
}
async fn connections(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(CalendarConnections {
        configured: matches!(s.mail.oauth_client().await, Ok(Some(_))),
        connections: s.store.calendar_connections(&u).await?,
    })
    .into_response())
}
async fn disconnect(State(s): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> HttpResult {
    let u = current(&s, &h).await?;
    let _gate = s.delivery.write().await;
    s.store.disconnect_calendar(&u, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn start(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    let app = match s.mail.oauth_client().await {
        Ok(Some(app)) => app,
        Ok(None) => return Ok(failure(StatusCode::CONFLICT, "google_setup_required")),
        Err(e) => return Err(e.into()),
    };
    let flow = s.store.start_calendar_oauth(&u, app.pin).await?;
    let url = GmailClient::authorization_url_scoped(
        &app.client.client_id,
        &app.client.redirect_uri,
        &flow.state,
        &flow.verifier,
        &moosuri_connectors::calendar::SCOPES,
    )
    .map_err(remote)?;
    Ok(Json(serde_json::json!({"authorization_url":url})).into_response())
}
pub(super) async fn callback(
    s: &AppState,
    state: String,
    code: Option<String>,
    denied: bool,
) -> HttpResult {
    let code = if denied {
        None
    } else {
        Some(SecretString::new(code.ok_or(StoreError::InvalidInput)?).map_err(remote)?)
    };
    let handle = s.store.capture_calendar_oauth(state, code).await?;
    let path = match handle {
        Some(handle) => format!("/settings/calendar?oauth={handle}"),
        None => "/settings/calendar?oauth_error=denied".into(),
    };
    Ok(axum::response::Redirect::to(&path).into_response())
}
async fn complete(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(input): Json<Handle>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let app = app(&s).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let exchange = s
        .store
        .claim_calendar_oauth(&u, input.handle, app.pin)
        .await?;
    let cancel = CancellationToken::new();
    let tokens = match s
        .mail
        .client()
        .exchange_code(&app.client, &exchange.code, &exchange.verifier, &cancel)
        .await
    {
        Ok(t) => t,
        Err(_) => {
            s.store.cancel_calendar_oauth(&exchange).await?;
            return Ok(failure(
                StatusCode::BAD_GATEWAY,
                "google_oauth_exchange_failed",
            ));
        }
    };
    if tokens.refresh_token.is_none()
        || tokens.scopes.as_ref().is_none_or(|v| {
            moosuri_connectors::calendar::SCOPES
                .iter()
                .any(|scope| !v.iter().any(|s| s == scope))
                || v.iter()
                    .any(|s| !moosuri_connectors::calendar::SCOPES.contains(&s.as_str()))
        })
    {
        s.store.cancel_calendar_oauth(&exchange).await?;
        return Ok(failure(
            StatusCode::BAD_REQUEST,
            "google_consent_incomplete",
        ));
    }
    let p = match CalendarClient::google()
        .primary(&tokens.access_token, &cancel)
        .await
    {
        Ok(p) => p,
        Err(_) => {
            s.store.cancel_calendar_oauth(&exchange).await?;
            return Ok(failure(
                StatusCode::BAD_GATEWAY,
                "calendar_primary_unavailable",
            ));
        }
    };
    let connected = s.store.complete_calendar_oauth(exchange, tokens, p).await?;
    Ok(Json(connected).into_response())
}
async fn calendars(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<ConnectionQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let b = binding(&s, &u, q.connection_id).await?;
    primary(&s, &b).await?;
    let page = CalendarClient::google()
        .calendars(
            &b.access,
            q.page_token.as_deref(),
            &CancellationToken::new(),
        )
        .await
        .map_err(remote)?;
    s.store.check_calendar_binding(&b).await?;
    Ok(Json(page).into_response())
}
async fn events(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(q): Json<CalendarRange>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let b = binding(&s, &u, q.connection_id.clone()).await?;
    let p = primary(&s, &b).await?;
    let page = CalendarClient::google()
        .events(&b.access, &p.id, &q, &CancellationToken::new())
        .await
        .map_err(remote)?;
    s.store.check_calendar_binding(&b).await?;
    Ok(Json(page).into_response())
}
async fn actions(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.calendar_actions(&u).await?).into_response())
}
async fn change(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(input): Json<CalendarChange>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    if let Some(existing) = s.store.existing_calendar_change(&u, input.clone()).await? {
        return Ok(Json(existing).into_response());
    }
    let b = binding(&s, &u, input.connection_id.clone()).await?;
    let p = primary(&s, &b).await?;
    let client = CalendarClient::google();
    let cancel = CancellationToken::new();
    let prior = match &input.mutation {
        CalendarMutation::Create { .. } => None,
        CalendarMutation::Update { event_id, .. } | CalendarMutation::Delete { event_id, .. } => {
            Some(
                client
                    .event(&b.access, &p.id, event_id, &cancel)
                    .await
                    .map_err(remote)?,
            )
        }
    };
    s.store.check_calendar_binding(&b).await?;
    let admission = s
        .store
        .admit_calendar_change(&b, input, p, prior.as_ref().map(|v| v.event.clone()))
        .await?;
    let dispatch = match admission {
        CalendarAdmission::Recorded(v) => return Ok(Json(v).into_response()),
        CalendarAdmission::Dispatch(d) => d,
    };
    // This is the only write invocation. No retry or refresh is performed after commitment.
    let result = client
        .mutate(
            &b.access,
            CalendarWrite {
                primary: &dispatch.primary_id,
                id: &dispatch.event_id,
                change: &dispatch.change.mutation,
                operation: &dispatch.change.request_id,
                digest: &dispatch.digest,
                prior: prior.as_ref(),
            },
            &cancel,
        )
        .await;
    let (state, event, receipt, error) = match result {
        Ok(result) => {
            let etag = result.event.as_ref().map(|e| e.event.etag.clone());
            let receipt=serde_json::json!({"provider":"google_calendar","http_status":result.status,"operation_id":dispatch.change.request_id,"event_id":dispatch.event_id,"etag":etag}).to_string();
            ("applied", result.event.map(|v| v.event), receipt, None)
        }
        Err(e) => {
            let state = match e {
                GmailError::Http(409 | 412) => "conflict",
                GmailError::Http(400 | 401 | 403 | 404 | 410) | GmailError::InvalidInput => {
                    "failed_definite"
                }
                _ => "outcome_unknown",
            };
            (
                state,
                None,
                serde_json::json!({"provider":"google_calendar","state":state,"http_status":match e{GmailError::Http(status)=>Some(status),_=>None}}).to_string(),
                Some(
                    if state == "outcome_unknown" {
                        "lookup_required"
                    } else {
                        "provider_rejected"
                    }
                    .into(),
                ),
            )
        }
    };
    let action = s
        .store
        .settle_calendar_change(&dispatch, state.into(), event, receipt, error)
        .await?;
    Ok(Json(action).into_response())
}
async fn lookup(State(s): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> HttpResult {
    let u = current(&s, &h).await?;
    let app = app(&s).await?;
    let d = s.store.calendar_reconcile(&u, id, app.pin).await?;
    let b = binding(&s, &u, d.change.connection_id.clone()).await?;
    let d = s
        .store
        .calendar_reconcile(&u, d.change.request_id.clone(), b.app.clone())
        .await?;
    let p = primary(&s, &b).await?;
    let observed = CalendarClient::google()
        .event(&b.access, &p.id, &d.event_id, &CancellationToken::new())
        .await;
    s.store.check_calendar_binding(&b).await?;
    let (state, event, receipt) = lookup_result(
        &d.change.mutation,
        &d.event_id,
        &d.change.request_id,
        &d.digest,
        observed,
    );
    let action = s
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
    Ok(Json(action).into_response())
}

fn lookup_result(
    mutation: &CalendarMutation,
    event_id: &str,
    request_id: &str,
    digest: &str,
    observed: Result<moosuri_connectors::calendar::EventObservation, GmailError>,
) -> (&'static str, Option<CalendarEvent>, &'static str) {
    match observed {
        Ok(v)
            if moosuri_connectors::calendar::observed_change(
                &v, event_id, request_id, digest, mutation,
            ) =>
        {
            (
                "applied",
                Some(v.event),
                "matching_event_and_operation_observed",
            )
        }
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
mod tests {
    use super::*;
    #[test]
    fn missing_event_is_causal_unknown_for_create_and_only_absence_for_delete() {
        let fields = CalendarFields {
            title: "Synthetic".into(),
            description: "".into(),
            location: "".into(),
            start: "2026-11-15T10:00:00Z".into(),
            end: "2026-11-15T11:00:00Z".into(),
            time_zone: None,
        };
        for status in [404, 410] {
            let create = CalendarMutation::Create {
                fields: fields.clone(),
            };
            let result = lookup_result(
                &create,
                "synthetic1",
                "request",
                "digest",
                Err(GmailError::Http(status)),
            );
            assert_eq!(result.0, "outcome_unknown");
            let delete = CalendarMutation::Delete {
                event_id: "synthetic1".into(),
                etag: "etag".into(),
            };
            let result = lookup_result(
                &delete,
                "synthetic1",
                "request",
                "digest",
                Err(GmailError::Http(status)),
            );
            assert_eq!(result.0, "observed_absent");
            assert_eq!(result.2, "current_absence_observed_causality_unknown");
        }
    }
}
