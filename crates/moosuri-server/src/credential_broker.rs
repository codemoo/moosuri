//! Opt-in signed device protocol and explicit typed credential read requests.
use super::*;
#[cfg(not(feature = "credential-broker-foundation"))]
pub(super) fn routes() -> Router<AppState> {
    Router::new()
}
#[cfg(feature = "credential-broker-foundation")]
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/credential-broker/status", get(enabled::status))
        .route(
            "/api/v1/credential-broker/google-body",
            post(enabled::google_body),
        )
        .route("/api/v1/credential-broker/github", post(enabled::github))
        .route(
            "/api/v1/credential-broker/google-preview",
            post(enabled::google_preview),
        )
        .route(
            "/api/v1/credential-broker/google-inbox",
            post(enabled::google_inbox),
        )
        .route("/api/v1/credential-broker/pair", post(enabled::challenge))
        .route(
            "/api/v1/credential-broker/{id}/revoke",
            post(enabled::revoke),
        )
        .route("/api/v1/credential-broker/device/pair", post(enabled::pair))
        .route(
            "/api/v1/credential-broker/device/challenge",
            post(enabled::auth),
        )
        .route(
            moosuri_vault_protocol::broker::DEVICE_PATH,
            post(enabled::command),
        )
        .layer(DefaultBodyLimit::max(16_384))
}
#[cfg(feature = "credential-broker-foundation")]
mod enabled {
    use super::*;
    use moosuri_vault_protocol::broker::*;
    pub(super) async fn status(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
        let u = current(&s, &headers).await?;
        Ok(Json(serde_json::json!({"devices":s.store.broker_devices(&u).await?,"jobs":s.store.github_metadata_jobs(&u).await?,"google_jobs":s.store.google_inbox_jobs(&u).await?,"google_preview_jobs":s.store.google_preview_jobs(&u).await?,"google_body_jobs":s.store.google_body_jobs(&u).await?})).into_response())
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct BodyInput {
        request_id: String,
        preview_request_id: String,
        selection: uuid::Uuid,
    }
    pub(super) async fn google_body(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(input): Json<BodyInput>,
    ) -> HttpResult {
        let u = current(&s, &headers).await?;
        Ok(Json(
            moosuri_agent::credential_broker::google_body(
                &s.store,
                &u,
                input.request_id,
                input.preview_request_id,
                input.selection,
            )
            .await?,
        )
        .into_response())
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct GitHubInput {
        request_id: String,
        repository: GitHubRepository,
    }
    pub(super) async fn github(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(input): Json<GitHubInput>,
    ) -> HttpResult {
        let u = current(&s, &headers).await?;
        Ok(Json(
            moosuri_agent::credential_broker::github_metadata(
                &s.store,
                &u,
                input.request_id,
                input.repository,
            )
            .await?,
        )
        .into_response())
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct GoogleInput {
        request_id: String,
    }
    pub(super) async fn google_inbox(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(input): Json<GoogleInput>,
    ) -> HttpResult {
        let u = current(&s, &headers).await?;
        Ok(Json(
            moosuri_agent::credential_broker::google_inbox(&s.store, &u, input.request_id).await?,
        )
        .into_response())
    }
    pub(super) async fn google_preview(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(input): Json<GoogleInput>,
    ) -> HttpResult {
        let u = current(&s, &headers).await?;
        Ok(Json(
            moosuri_agent::credential_broker::google_preview(&s.store, &u, input.request_id)
                .await?,
        )
        .into_response())
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct PairInput {
        public_key: Vec<u8>,
    }
    pub(super) async fn challenge(
        State(s): State<AppState>,
        headers: HeaderMap,
        Json(input): Json<PairInput>,
    ) -> HttpResult {
        let u = current(&s, &headers).await?;
        Ok(Json(
            s.store
                .broker_pair_challenge(&u, s.config.origin, input.public_key)
                .await?,
        )
        .into_response())
    }
    pub(super) async fn revoke(
        State(s): State<AppState>,
        headers: HeaderMap,
        axum::extract::Path(id): axum::extract::Path<String>,
    ) -> HttpResult {
        let u = current(&s, &headers).await?;
        let id = id.parse().map_err(|_| StoreError::InvalidInput)?;
        s.store.broker_revoke(&u, id).await?;
        Ok(StatusCode::NO_CONTENT.into_response())
    }
    pub(super) async fn pair(
        State(s): State<AppState>,
        Json(input): Json<PairProof>,
    ) -> HttpResult {
        Ok(Json(s.store.broker_pair(input).await?).into_response())
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    pub(super) struct DeviceInput {
        device: String,
    }
    pub(super) async fn auth(
        State(s): State<AppState>,
        Json(input): Json<DeviceInput>,
    ) -> HttpResult {
        let id = input.device.parse().map_err(|_| StoreError::InvalidInput)?;
        Ok(Json(s.store.broker_auth_challenge(id).await?).into_response())
    }
    pub(super) async fn command(
        State(s): State<AppState>,
        Json(input): Json<SignedCommand>,
    ) -> HttpResult {
        Ok(Json(s.store.broker_command(input).await?).into_response())
    }
}
