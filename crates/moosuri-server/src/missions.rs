//! Explicit authenticated task admission and owner controls.
use super::*;
use moosuri_core::{MissionControl, MissionCreate};
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/missions", get(list).post(create))
        .route("/api/v1/missions/{id}/control", post(control))
        .route("/api/v1/missions/{id}", axum::routing::delete(remove))
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.missions(&u).await?).into_response())
}
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<MissionCreate>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.create_mission(&u, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn control(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<MissionControl>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.control_mission(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}

async fn remove(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    s.store.delete_mission(&u, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
