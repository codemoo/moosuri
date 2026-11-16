//! Authenticated monitor policy operations. No provider polling or model IO.
use super::*;
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/task-monitors", get(list))
        .route("/api/v1/task-monitors/{id}/accept", post(accept))
        .route("/api/v1/task-monitors/{id}/stop", post(stop))
        .route(
            "/api/v1/task-monitors/{id}/observations/{ordinal}/resolve",
            post(resolve),
        )
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.task_monitors(&u).await?).into_response())
}
async fn accept(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::TaskMonitorAccept>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.accept_task_monitor(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn stop(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::TaskMonitorControl>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    Ok(Json(s.store.stop_task_monitor(&u, id, input).await?).into_response())
}
async fn resolve(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path((id, ordinal)): Path<(String, i64)>,
    Json(input): Json<moosuri_core::TaskMonitorResolve>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    Ok(Json(s.store.resolve_task_monitor(&u, id, ordinal, input).await?).into_response())
}
