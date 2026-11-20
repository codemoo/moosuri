//! Historical reads remain available if the browser worker is closed or offline.
use super::*;
use axum::extract::Query;
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
        .with_state(MemberTasks { store, owner })
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    before: Option<i64>,
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
