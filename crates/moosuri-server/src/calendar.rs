//! Explicit authenticated Calendar forms. No model/agent mutation admission.
use super::*;
use axum::extract::Query;
use moosuri_agent::calendar::CalendarService;
use moosuri_connectors::{
    calendar::CalendarClient,
    gmail::{GmailClient, GmailError, SecretString},
    model::CancellationToken,
};
use moosuri_core::*;
use moosuri_store::GoogleApp;
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
    let page = CalendarService::new(s.store.clone(), s.mail.clone())
        .calendars(&u, q.connection_id, q.page_token)
        .await?;
    Ok(Json(page).into_response())
}
async fn events(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(q): Json<CalendarRange>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let page = CalendarService::new(s.store.clone(), s.mail.clone())
        .events(&u, q)
        .await?;
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
    let action = CalendarService::new(s.store.clone(), s.mail.clone())
        .change(&u, input)
        .await?;
    Ok(Json(action).into_response())
}
async fn lookup(State(s): State<AppState>, h: HeaderMap, Path(id): Path<String>) -> HttpResult {
    let u = current(&s, &h).await?;
    let action = CalendarService::new(s.store.clone(), s.mail.clone())
        .lookup(&u, id)
        .await?;
    Ok(Json(action).into_response())
}
