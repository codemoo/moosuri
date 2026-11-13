use super::*;
use axum::extract::Query;
use moosuri_connectors::gmail::{GmailClient, SecretString};
use moosuri_connectors::model::CancellationToken;
use moosuri_core::{MailApproval, MailConnections, MailDraftInput, MailToolRequest};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Handle {
    handle: String,
}
#[derive(Deserialize)]
pub(super) struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
    scope: Option<String>,
    authuser: Option<String>,
    prompt: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Search {
    connection_id: String,
    query: String,
    page_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Revision {
    revision: i64,
    input: MailDraftInput,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Read {
    connection_id: String,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/connections", get(connections))
        .route("/connections/{id}", delete(disconnect))
        .route("/oauth/start", post(start))
        .route("/oauth/callback", get(callback))
        .route("/oauth/complete", post(complete))
        .route("/search", post(search))
        .route("/messages/{id}", get(read))
        .route("/threads/{id}", get(thread))
        .route("/actions", get(actions).post(draft))
        .route("/actions/{id}", get(action).put(edit))
        .route("/actions/{id}/approve", post(approve))
        .route("/actions/{id}/reject", post(reject))
        .route("/actions/{id}/check-sent", post(check_sent))
        .layer(DefaultBodyLimit::max(64 * 1024))
}
async fn connections(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(MailConnections {
        configured: s.mail.configured().await,
        connections: s.store.mail_connections(&u).await?,
    })
    .into_response())
}
async fn disconnect(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    {
        let _delivery = s.delivery.write().await;
        s.store.disconnect_mail(&u, id).await?;
    }
    s.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn start(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&s, &headers).await?;
    start_for(&s, &user, false).await
}
pub(super) async fn start_for(s: &AppState, user: &UserContext, setup_return: bool) -> HttpResult {
    let app = match s.mail.oauth_client().await {
        Ok(Some(app)) => app,
        Ok(None) => {
            return Ok(failure(
                StatusCode::CONFLICT,
                if setup_return {
                    "google_setup_required"
                } else {
                    "gmail_setup_required"
                },
            ));
        }
        Err(StoreError::Conflict) => {
            return Ok(failure(StatusCode::CONFLICT, "google_origin_changed"));
        }
        Err(error) => return Err(error.into()),
    };
    let start = s
        .store
        .start_mail_oauth_with_app(user, app.pin, setup_return)
        .await?;
    let url = GmailClient::authorization_url(
        &app.client.client_id,
        &app.client.redirect_uri,
        &start.state,
        &start.verifier,
    )
    .map_err(|_| HttpError(StoreError::Unavailable))?;
    Ok(Json(serde_json::json!({"authorization_url":url})).into_response())
}
async fn callback(State(s): State<AppState>, Query(input): Query<Callback>) -> HttpResult {
    if input.state.starts_with("cal.") {
        return crate::calendar::callback(&s, input.state, input.code, input.error.is_some()).await;
    }
    let _ = (input.scope, input.authuser, input.prompt);
    if input.error.is_some() {
        let setup = s.store.cancel_mail_oauth_state(input.state).await?;
        let path = if setup {
            "/admin/google?oauth_error=denied"
        } else {
            "/settings/mail?oauth_error=denied"
        };
        return Ok(axum::response::Redirect::to(path).into_response());
    }
    let code = SecretString::new(input.code.ok_or(StoreError::InvalidInput)?)
        .map_err(|_| HttpError(StoreError::InvalidInput))?;
    let (handle, setup) = s
        .store
        .capture_mail_oauth_with_return(input.state, code)
        .await?;
    let path = if setup {
        "/admin/google"
    } else {
        "/settings/mail"
    };
    Ok(axum::response::Redirect::to(&format!("{path}?oauth={handle}")).into_response())
}
async fn complete(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Handle>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let Some(app) = s.mail.oauth_client().await? else {
        return Ok(failure(StatusCode::CONFLICT, "google_setup_required"));
    };
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let exchange = s
        .store
        .claim_mail_oauth_with_app(&user, input.handle, app.pin)
        .await?;
    let cancel = CancellationToken::new();
    let tokens = match s
        .mail
        .client()
        .exchange_code(&app.client, &exchange.code, &exchange.verifier, &cancel)
        .await
    {
        Ok(tokens) => tokens,
        Err(_) => {
            s.store.cancel_mail_oauth_exchange(&exchange).await?;
            return Ok(failure(
                StatusCode::BAD_GATEWAY,
                "google_oauth_exchange_failed",
            ));
        }
    };
    // A partial consent cannot establish a durable mailbox connection. Release
    // the claimed flow so a failed consent does not block installer edits.
    if tokens.refresh_token.is_none()
        || tokens.scopes.as_ref().is_some_and(|scopes| {
            moosuri_connectors::gmail::MINIMAL_SCOPES
                .iter()
                .any(|required| !scopes.iter().any(|scope| scope == required))
        })
    {
        s.store.cancel_mail_oauth_exchange(&exchange).await?;
        return Ok(failure(
            StatusCode::BAD_REQUEST,
            "google_consent_incomplete",
        ));
    }
    let profile = match s.mail.client().profile(&tokens.access_token, &cancel).await {
        Ok(profile) => profile,
        Err(_) => {
            s.store.cancel_mail_oauth_exchange(&exchange).await?;
            return Ok(failure(
                StatusCode::BAD_GATEWAY,
                "google_profile_unavailable",
            ));
        }
    };
    let connection = s
        .store
        .complete_mail_oauth(exchange, tokens, profile.email_address)
        .await?;
    Ok(Json(connection).into_response())
}
async fn search(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Search>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let v = s
        .mail
        .read(
            &u,
            &MailToolRequest::Search {
                connection_id: input.connection_id,
                query: input.query,
                page_token: input.page_token,
            },
            &CancellationToken::new(),
        )
        .await?;
    Ok(Json(v).into_response())
}
async fn read(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(input): Query<Read>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(
        s.mail
            .read(
                &u,
                &MailToolRequest::Read {
                    connection_id: input.connection_id,
                    message_id: id,
                },
                &CancellationToken::new(),
            )
            .await?,
    )
    .into_response())
}
async fn thread(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(input): Query<Read>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(
        s.mail
            .read(
                &u,
                &MailToolRequest::Thread {
                    connection_id: input.connection_id,
                    thread_id: id,
                },
                &CancellationToken::new(),
            )
            .await?,
    )
    .into_response())
}
async fn actions(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(serde_json::json!({"actions":s.store.mail_actions(&u).await?})).into_response())
}
async fn action(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.mail_action(&u, id).await?).into_response())
}
async fn draft(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MailDraftInput>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.create_mail_draft(&u, input, None).await?).into_response())
}
async fn edit(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Revision>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(
        s.store
            .create_mail_draft(&u, input.input, Some((id, input.revision)))
            .await?,
    )
    .into_response())
}
async fn approve(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<MailApproval>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let a = s.store.decide_mail(&u, id, input, true).await?;
    s.runtime.wake();
    Ok(Json(a).into_response())
}
async fn reject(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<MailApproval>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.decide_mail(&u, id, input, false).await?).into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SentCheck {}
async fn check_sent(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(_input): Json<SentCheck>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    Ok(Json(
        s.mail
            .check_unknown(&u, id, &CancellationToken::new())
            .await?,
    )
    .into_response())
}
