//! Opt-in protocol foundation. No production adapter or invocation mint route.
use super::*;
#[cfg(not(feature = "credential-broker-foundation"))]
pub(super) fn routes() -> Router<AppState> {
    Router::new()
}
#[cfg(feature = "credential-broker-foundation")]
pub(super) fn routes() -> Router<AppState> {
    Router::new()
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
