//! Account-owned local reminders/inbox. No model tasks, mail or Push dispatch.
use super::*;
use axum::extract::Query;
use moosuri_core::{
    QuietHoursChange, ScheduleControl, ScheduleCreate, ScheduleDraft, ScheduleEdit,
};
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/schedules", get(list).post(create))
        .route("/api/v1/schedules/preview", post(preview))
        .route("/api/v1/schedules/{id}", axum::routing::put(edit))
        .route("/api/v1/schedules/{id}/control", post(control))
        .route("/api/v1/schedules/{id}/history", get(history))
        .route("/api/v1/notifications", get(inbox))
        .route(
            "/api/v1/notifications/settings",
            get(settings).put(set_settings),
        )
        .route("/api/v1/notifications/{id}/read", post(read))
        .route("/api/v1/notifications/{id}/dismiss", post(dismiss))
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.schedules(&u).await?).into_response())
}
async fn preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ScheduleDraft>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.preview_schedule(&u, input).await?).into_response())
}
async fn create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ScheduleCreate>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.create_schedule(&u, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn edit(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ScheduleEdit>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.edit_schedule(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn control(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ScheduleControl>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.control_schedule(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn history(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.schedule_history(&u, id).await?).into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct InboxQuery {
    before: Option<String>,
    #[serde(default)]
    dismissed: bool,
}
async fn inbox(
    State(s): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<InboxQuery>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(
        s.store
            .notifications(&u, query.before, query.dismissed)
            .await?,
    )
    .into_response())
}
async fn settings(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.quiet_hours(&u).await?).into_response())
}
async fn set_settings(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<QuietHoursChange>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.set_quiet_hours(&u, input).await?).into_response())
}
async fn read(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.mark_notification(&u, id, false).await?).into_response())
}
async fn dismiss(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.mark_notification(&u, id, true).await?).into_response())
}
