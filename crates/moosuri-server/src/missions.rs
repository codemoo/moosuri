//! Explicit authenticated task admission and owner controls.
use super::*;
use moosuri_core::{MissionControl, MissionCreate};
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/recurring-missions",
            get(recurring_list).post(recurring_create),
        )
        .route(
            "/api/v1/recurring-missions/{id}",
            axum::routing::put(recurring_edit),
        )
        .route(
            "/api/v1/recurring-missions/{id}/control",
            post(recurring_control),
        )
        .route("/api/v1/missions", get(list).post(create))
        .route("/api/v1/missions/{id}/control", post(control))
        .route("/api/v1/missions/{id}/connection", post(select_connection))
        .route(
            "/api/v1/native-mail/{id}/connection",
            post(select_native_connection),
        )
        .route("/api/v1/connections", get(connections))
        .route(
            "/api/v1/connections/alias",
            axum::routing::put(connection_alias),
        )
        .route("/api/v1/missions/{id}/adopt-report", post(adopt_report))
        .route("/api/v1/missions/{id}/adopt-draft", post(adopt_draft))
        .route(
            "/api/v1/missions/{id}/acceptance",
            axum::routing::put(revise),
        )
        .route("/api/v1/missions/{id}", axum::routing::delete(remove))
}
async fn adopt_draft(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::CompoundDraftAdoption>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    Ok(Json(s.store.adopt_compound_draft(&u, id, input).await?).into_response())
}
async fn adopt_report(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::CompoundReportAdoption>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    Ok(Json(s.store.adopt_compound_report(&u, id, input).await?).into_response())
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

async fn recurring_list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.recurring_missions(&u).await?).into_response())
}
async fn recurring_create(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<moosuri_core::RecurringMissionCreate>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.create_recurring_mission(&u, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn recurring_control(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::ScheduleControl>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.control_recurring_mission(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn recurring_edit(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::ScheduleEdit>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.edit_recurring_mission(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}

async fn revise(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::MissionAcceptanceUpdate>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = s.store.revise_mission_acceptance(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(out).into_response())
}

async fn select_connection(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::MissionConnectionSelection>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let out = if input.slot_id == "native-mail-slot-v1" {
        s.store
            .select_native_mail_connection(&u, id.clone(), input)
            .await?;
        s.store
            .missions(&u)
            .await?
            .into_iter()
            .find(|m| m.id == id)
            .ok_or(StoreError::NotFound)?
    } else {
        s.store.select_mission_connection(&u, id, input).await?
    };
    s.runtime.wake();
    Ok(Json(out).into_response())
}
async fn connection_alias(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<moosuri_core::ConnectionAlias>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    s.store.set_connection_alias(&u, input).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

async fn connections(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.scoped_connections(&u).await?).into_response())
}

async fn select_native_connection(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<moosuri_core::MissionConnectionSelection>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    s.store.select_native_mail_connection(&u, id, input).await?;
    s.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
