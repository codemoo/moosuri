//! Origin namespaces are private POST data, never bearer tokens or URL parameters.
use super::*;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OriginRequest {
    origin_scope: String,
    expected_current_scope: String,
}
pub(super) async fn validate_origin(
    State(state): State<AppState>,
    headers: HeaderMap,
    input: std::result::Result<Json<OriginRequest>, axum::extract::rejection::JsonRejection>,
) -> HttpResult {
    // Serialize delivery against logout/password/admin revocation, as private reads do.
    let _delivery = state.delivery.read().await;
    let user = current(&state, &headers).await?;
    let Json(input) = input.map_err(|_| StoreError::InvalidInput)?;
    // Pin the actual cookie session at this final proof, not merely its account.
    // A claimed handoff cannot migrate to a renewed same-account login.
    if user.chat_draft_scope().as_deref() != Some(input.expected_current_scope.as_str()) {
        return Err(StoreError::Unauthorized.into());
    }
    state
        .store
        .validate_share_origin(&user, input.origin_scope)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
