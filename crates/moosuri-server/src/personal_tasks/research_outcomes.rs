//! Member research outcomes. Every route authenticates the real member session and the gateway epoch
//! exactly like conversation work; the store re-checks owner, tenant, volume, generation and auth epoch.
use super::*;
use moosuri_core::personal_browser::research_outcomes::{OutcomeAction, OutcomeRequest};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct OutcomeQuery {
    request_id: Option<Uuid>,
}

pub(super) fn routes() -> Router<MemberTasks> {
    Router::new()
        .route(
            "/api/v1/personal/tasks/research-outcomes/{thread}",
            get(page).post(create),
        )
        .route(
            "/api/v1/personal/tasks/research-outcomes/{thread}/control",
            axum::routing::post(control),
        )
}

async fn page(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(thread): Path<Uuid>,
    Query(q): Query<OutcomeQuery>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .research_outcome_page(s.owner, user, thread.to_string(), q.request_id)
            .await?,
    )
    .into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(thread): Path<Uuid>,
    Json(request): Json<OutcomeRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .create_research_outcome(s.owner, user, epoch, thread.to_string(), request)
            .await?,
    )
    .into_response())
}

async fn control(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(thread): Path<Uuid>,
    Json(action): Json<OutcomeAction>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .control_research_outcome(s.owner, user, epoch, thread.to_string(), action)
            .await?,
    )
    .into_response())
}
