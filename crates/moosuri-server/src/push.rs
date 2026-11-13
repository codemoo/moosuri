//! Current-account-only registration and content-free SW authorization.
use super::*;
use axum::extract::Query;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DeviceQuery {
    device_id: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct DisplayInput {
    binding_id: String,
    notification_id: String,
}
pub(super) async fn configuration(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<DeviceQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(
        state
            .store
            .push_configuration(&user, query.device_id, state.config.origin.clone())
            .await?,
    )
    .into_response())
}
pub(super) async fn enroll(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<moosuri_store::push::PushEnrollment>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let value = state
        .store
        .enroll_push(&user, input, state.config.origin.clone())
        .await?;
    state.runtime.wake();
    Ok(Json(value).into_response())
}
pub(super) async fn deactivate(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<moosuri_store::push::PushDeactivation>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    state.store.deactivate_push(&user, input).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
pub(super) async fn disable(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    state.store.disable_push(&user, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
pub(super) async fn authorize(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<DisplayInput>,
) -> HttpResult {
    let user = state
        .store
        .authenticate_push_passive(cookie_token(&headers)?)
        .await?;
    expected_account(&headers, &user)?;
    state
        .store
        .authorize_push_display(&user, input.binding_id, input.notification_id)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
