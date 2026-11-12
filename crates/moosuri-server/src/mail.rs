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
        .layer(DefaultBodyLimit::max(64 * 1024))
}
async fn connections(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(MailConnections {
        configured: s.mail.configured(),
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
    let u = current(&s, &headers).await?;
    let Some(client) = s.mail.oauth_client() else {
        return Ok(failure(StatusCode::CONFLICT, "gmail_setup_required"));
    };
    let start = s.store.start_mail_oauth(&u).await?;
    let url = GmailClient::authorization_url(
        &client.client_id,
        &client.redirect_uri,
        &start.state,
        &start.verifier,
    )
    .map_err(|_| HttpError(StoreError::Unavailable))?;
    Ok(Json(serde_json::json!({"authorization_url":url})).into_response())
}
async fn callback(State(s): State<AppState>, Query(input): Query<Callback>) -> HttpResult {
    if !s.mail.configured() {
        return Ok(failure(StatusCode::CONFLICT, "gmail_setup_required"));
    }
    let _ = (input.scope, input.authuser, input.prompt);
    if input.error.is_some() {
        return Ok(
            axum::response::Redirect::to("/settings/mail?oauth_error=denied").into_response(),
        );
    }
    let code = SecretString::new(input.code.ok_or(StoreError::InvalidInput)?)
        .map_err(|_| HttpError(StoreError::InvalidInput))?;
    let handle = s.store.capture_mail_oauth(input.state, code).await?;
    Ok(axum::response::Redirect::to(&format!("/settings/mail?oauth={handle}")).into_response())
}
async fn complete(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Handle>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let Some(client) = s.mail.oauth_client() else {
        return Ok(failure(StatusCode::CONFLICT, "gmail_setup_required"));
    };
    let exchange = s.store.claim_mail_oauth(&u, input.handle).await?;
    let tokens = s
        .mail
        .client()
        .exchange_code(
            client,
            &exchange.code,
            &exchange.verifier,
            &CancellationToken::new(),
        )
        .await
        .map_err(|_| HttpError(StoreError::Unavailable))?;
    let profile = s
        .mail
        .client()
        .profile(&tokens.access_token, &CancellationToken::new())
        .await
        .map_err(|_| HttpError(StoreError::Unavailable))?;
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
