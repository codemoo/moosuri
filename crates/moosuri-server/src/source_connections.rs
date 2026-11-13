//! User-owned connection management and a separate, bearer-only collector protocol.
//! Collector grants are never accepted as browser sessions or tool authority.
use super::*;
use moosuri_connectors::{
    gmail::SecretString,
    model::CancellationToken,
    source::{
        ReadError,
        gmail::{GmailScope, GmailSyncClient},
        slack::{SlackClient, SlackScope},
    },
};
use moosuri_core::{
    CreateSourceConnection, DisconnectMode, SourceBatch, SourceConnectionKind, SourceRunOutcome,
};
use moosuri_store::SourceIngestCapability;

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum Create {
    CustomHttp {
        name: String,
        scope: String,
        poll_interval_seconds: Option<u32>,
    },
    Gmail {
        name: String,
        mail_connection_id: String,
        label_ids: Vec<String>,
        after_unix: i64,
        poll_interval_seconds: u32,
    },
    Slack {
        name: String,
        token: SecretString,
        channel_id: String,
        oldest_unix: i64,
        reconcile_lookback_seconds: i64,
        poll_interval_seconds: u32,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configure {
    expected_revision: i64,
    name: String,
    sync_enabled: bool,
    poll_interval_seconds: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Pause {
    expected_revision: i64,
    paused: bool,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Disconnect {
    expected_revision: i64,
    mode: DisconnectMode,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TokenRequest {
    ttl_seconds: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SlackToken {
    token: SecretString,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RunIdentity {
    run_id: String,
    generation: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Finish {
    run_id: String,
    generation: i64,
    outcome: SourceRunOutcome,
    error: Option<String>,
    retry_after_seconds: Option<u32>,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(create))
        .route("/{id}", get(detail).patch(configure))
        .route("/{id}/pause", post(pause))
        .route("/{id}/sync", post(sync_now))
        .route("/{id}/disconnect", post(disconnect))
        .route("/{id}/token", post(token))
        .route("/{id}/revoke-token", post(revoke))
        .route("/{id}/slack-token", post(slack_token))
        .route("/{id}/gmail-connection", post(gmail_connection))
        .route("/{id}/reset-checkpoint", post(reset_checkpoint))
        .route("/{id}/runs", get(runs))
        .route("/{id}/receipts/{request}", get(receipt))
}
pub(super) fn ingest_routes() -> Router<AppState> {
    Router::new()
        .route("/status", get(ingest_status))
        .route("/runs", post(begin))
        .route("/heartbeat", post(heartbeat))
        .route(
            "/batches",
            post(batch).layer(DefaultBodyLimit::max(2 * 1024 * 1024)),
        )
        .route("/finish", post(finish))
        .route("/receipts/{request}", get(ingest_receipt))
}
fn timestamp() -> Result<i64, StoreError> {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::Unavailable)
        .and_then(|v| i64::try_from(v.as_secs()).map_err(|_| StoreError::Unavailable))
}
fn read_error(error: ReadError) -> HttpError {
    HttpError(match error {
        ReadError::InvalidInput => StoreError::InvalidInput,
        ReadError::IdentityMismatch
        | ReadError::AuthenticationRequired
        | ReadError::ScopeDenied => StoreError::Forbidden,
        _ => StoreError::Unavailable,
    })
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(serde_json::json!({"connections":s.store.source_connections(&user).await?,"gmail_configured":s.mail.configured().await})).into_response())
}
async fn detail(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.store.source_connection(&user, id).await?).into_response())
}
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Create>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let cancel = CancellationToken::new();
    let (input, slack_secret) = match input {
        Create::CustomHttp {
            name,
            scope,
            poll_interval_seconds,
        } => (
            CreateSourceConnection {
                name,
                kind: SourceConnectionKind::CustomHttp,
                scope,
                remote_account: "external_producer".into(),
                mail_connection_id: None,
                sync_enabled: true,
                poll_interval_seconds: poll_interval_seconds.unwrap_or(300),
            },
            None,
        ),
        Create::Gmail {
            name,
            mail_connection_id,
            label_ids,
            after_unix,
            poll_interval_seconds,
        } => {
            if !s.mail.configured().await {
                return Ok(failure(StatusCode::CONFLICT, "gmail_setup_required"));
            }
            let binding = s.mail.binding(&user, &mail_connection_id, &cancel).await?;
            let profile = s
                .mail
                .client()
                .profile(&binding.access_token, &cancel)
                .await
                .map_err(|_| StoreError::Unavailable)?;
            let scope = GmailScope {
                account_email: profile.email_address.clone(),
                label_ids,
                after_unix,
            };
            scope.validate(timestamp()?).map_err(read_error)?;
            GmailSyncClient::default()
                .verify_identity(&binding.access_token, &scope.account_email, &cancel)
                .await
                .map_err(read_error)?;
            (
                CreateSourceConnection {
                    name,
                    kind: SourceConnectionKind::Gmail,
                    scope: serde_json::to_string(&scope).map_err(|_| StoreError::InvalidInput)?,
                    remote_account: profile.email_address,
                    mail_connection_id: Some(mail_connection_id),
                    sync_enabled: true,
                    poll_interval_seconds,
                },
                None,
            )
        }
        Create::Slack {
            name,
            token,
            channel_id,
            oldest_unix,
            reconcile_lookback_seconds,
            poll_interval_seconds,
        } => {
            let client = SlackClient::default();
            let identity = client.identity(&token, &cancel).await.map_err(read_error)?;
            let scope = SlackScope {
                workspace_id: identity.workspace_id,
                user_id: identity.user_id,
                channel_id,
                oldest_unix,
                reconciliation_lookback_seconds: reconcile_lookback_seconds,
            };
            scope.validate(timestamp()?).map_err(read_error)?;
            let account = format!("{}:{}", scope.workspace_id, scope.user_id);
            (
                CreateSourceConnection {
                    name,
                    kind: SourceConnectionKind::Slack,
                    scope: serde_json::to_string(&scope).map_err(|_| StoreError::InvalidInput)?,
                    remote_account: account,
                    mail_connection_id: None,
                    sync_enabled: false,
                    poll_interval_seconds,
                },
                Some(token),
            )
        }
    };
    let _delivery = s.delivery.write().await;
    let mut connection = s.store.create_source_connection(&user, input).await?;
    if let Some(secret) = slack_secret {
        connection = s
            .store
            .set_source_slack_token(&user, connection.id, secret)
            .await?;
        connection = s
            .store
            .configure_source_connection(
                &user,
                connection.id,
                connection.config_revision,
                connection.name,
                true,
                connection.poll_interval_seconds,
            )
            .await?;
    }
    s.runtime.wake();
    Ok((StatusCode::CREATED, Json(connection)).into_response())
}
async fn configure(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Configure>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let value = s
        .store
        .configure_source_connection(
            &user,
            id,
            input.expected_revision,
            input.name,
            input.sync_enabled,
            input.poll_interval_seconds,
        )
        .await?;
    s.runtime.wake();
    Ok(Json(value).into_response())
}
async fn pause(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Pause>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let value = s
        .store
        .pause_source_connection(&user, id, input.expected_revision, input.paused)
        .await?;
    s.runtime.wake();
    Ok(Json(value).into_response())
}
async fn sync_now(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Revision>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let value = s
        .store
        .trigger_source_sync(&user, id, input.expected_revision)
        .await?;
    s.runtime.wake();
    Ok(Json(value).into_response())
}
async fn disconnect(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Disconnect>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let value = s
        .store
        .disconnect_source_connection(&user, id, input.expected_revision, input.mode)
        .await?;
    s.runtime.wake();
    Ok(Json(value).into_response())
}
async fn token(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<TokenRequest>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let grant = s
        .store
        .issue_source_token(&user, id, input.ttl_seconds)
        .await?;
    Ok(
        Json(serde_json::json!({"token":grant.token,"expires_at":grant.expires_at}))
            .into_response(),
    )
}
async fn revoke(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    s.store.revoke_source_token(&user, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn slack_token(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<SlackToken>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let connection = s.store.source_connection(&user, id.clone()).await?;
    if connection.kind != SourceConnectionKind::Slack {
        return Err(StoreError::InvalidInput.into());
    }
    let scope: SlackScope =
        serde_json::from_str(&connection.scope).map_err(|_| StoreError::Corrupt)?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    SlackClient::default()
        .verify_identity(
            &input.token,
            &scope.workspace_id,
            &scope.user_id,
            &CancellationToken::new(),
        )
        .await
        .map_err(read_error)?;
    let value = s
        .store
        .set_source_slack_token(&user, id, input.token)
        .await?;
    s.runtime.wake();
    Ok(Json(value).into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GmailRebind {
    expected_revision: i64,
    mail_connection_id: String,
}
async fn gmail_connection(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<GmailRebind>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let connection = s
        .store
        .reconnect_source_gmail(&user, id, input.expected_revision, input.mail_connection_id)
        .await?;
    s.runtime.wake();
    Ok(Json(connection).into_response())
}
async fn reset_checkpoint(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Revision>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let connection = s
        .store
        .reset_source_checkpoint(&user, id, input.expected_revision)
        .await?;
    s.runtime.wake();
    Ok(Json(connection).into_response())
}
async fn runs(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(serde_json::json!({"runs":s.store.source_sync_runs(&user,id).await?})).into_response())
}
async fn receipt(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, request)): Path<(String, String)>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.store.source_batch_receipt(&user, id, request).await?).into_response())
}
async fn producer(s: &AppState, headers: &HeaderMap) -> Result<SourceIngestCapability, HttpError> {
    if headers.contains_key(header::COOKIE)
        || headers.contains_key(header::ORIGIN)
        || headers.get_all(header::AUTHORIZATION).iter().count() != 1
    {
        return Err(StoreError::Unauthorized.into());
    }
    let token = headers
        .get(header::AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .ok_or(StoreError::Unauthorized)?;
    if token.len() != 64 || !token.bytes().all(|v| v.is_ascii_hexdigit()) {
        return Err(StoreError::Unauthorized.into());
    }
    Ok(s.store.authenticate_source_token(token.to_owned()).await?)
}
async fn ingest_status(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let cap = producer(&s, &headers).await?;
    Ok(Json(s.store.source_ingest_status(&cap).await?).into_response())
}
async fn begin(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let cap = producer(&s, &headers).await?;
    let lease = s.store.begin_source_run(&cap).await?;
    Ok((
        StatusCode::CREATED,
        Json(serde_json::json!({"run_id":lease.run_id(),"generation":lease.generation()})),
    )
        .into_response())
}
async fn heartbeat(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<RunIdentity>,
) -> HttpResult {
    let cap = producer(&s, &headers).await?;
    let lease = s
        .store
        .source_run_lease(&cap, input.run_id, input.generation)
        .await?;
    s.store.source_heartbeat(&lease).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn batch(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SourceBatch>,
) -> HttpResult {
    let cap = producer(&s, &headers).await?;
    let lease = s
        .store
        .source_run_lease(&cap, input.run_id.clone(), input.generation)
        .await?;
    let _gate = s.delivery.write().await;
    let receipt = s.store.commit_source_batch(&lease, input).await?;
    s.runtime.wake();
    Ok(Json(receipt).into_response())
}
async fn finish(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<Finish>,
) -> HttpResult {
    let cap = producer(&s, &headers).await?;
    let lease = s
        .store
        .source_run_lease(&cap, input.run_id, input.generation)
        .await?;
    s.store
        .finish_source_run(
            &lease,
            input.outcome,
            input.error,
            input.retry_after_seconds,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn ingest_receipt(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(request): Path<String>,
) -> HttpResult {
    let cap = producer(&s, &headers).await?;
    Ok(Json(s.store.source_ingest_receipt(&cap, request).await?).into_response())
}
