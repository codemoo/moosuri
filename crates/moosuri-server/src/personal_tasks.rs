//! Historical reads remain available if the browser worker is closed or offline.
use super::*;
use axum::extract::Query;
use moosuri_core::personal_browser::{WorkAction, WorkRequest};
use moosuri_store::personal_volume::VolumeOwner;

#[derive(Clone)]
struct MemberTasks {
    store: Store,
    owner: VolumeOwner,
}

pub fn routes(store: Store, owner: VolumeOwner) -> Router {
    Router::new()
        .route("/api/v1/personal/tasks", get(page))
        .route("/api/v1/personal/tasks/{id}", get(detail))
        .route(
            "/api/v1/personal/tasks/workflows",
            axum::routing::post(create),
        )
        .route(
            "/api/v1/personal/tasks/workflows/{id}",
            axum::routing::post(change),
        )
        .route("/internal/member-work/tick", axum::routing::post(tick))
        .with_state(MemberTasks { store, owner })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    before: Option<String>,
}

async fn page(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(s.store.personal_task_page(s.owner, user, q.before).await?).into_response())
}
async fn detail(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(s.store.personal_task_detail(s.owner, user, id).await?).into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Json(q): Json<WorkRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(
        s.store
            .create_member_workflow(s.owner, user, super::personal_text::gateway_epoch(&h)?, q)
            .await?,
    )
    .into_response())
}
async fn change(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Json(action): Json<WorkAction>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(
        s.store
            .change_member_workflow(
                s.owner,
                user,
                super::personal_text::gateway_epoch(&h)?,
                id,
                action,
            )
            .await?,
    )
    .into_response())
}

async fn tick(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Json(pin): Json<moosuri_core::personal_browser::WorkTick>,
) -> HttpResult {
    if h.get("x-moosuri-account").and_then(|v| v.to_str().ok())
        != Some(s.owner.account.to_string().as_str())
        || pin.tenant != s.owner.tenant.to_string()
        || pin.volume != s.owner.volume.to_string()
        || pin.generation != s.owner.generation
        || pin.epoch < 0
    {
        return Err(HttpError(StoreError::Forbidden));
    }
    s.store.tick_member_workflows(s.owner, pin.epoch).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
