use super::*;
use moosuri_core::personal_browser::{ConversationWorkAction, ConversationWorkRequest};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GroupQuery {
    request_id: Option<Uuid>,
}

pub(super) fn routes() -> Router<MemberTasks> {
    Router::new()
        .route(
            "/api/v1/personal/tasks/conversations/{thread}",
            get(page).post(create),
        )
        .route(
            "/api/v1/personal/tasks/conversations/{thread}/control",
            axum::routing::post(control),
        )
        .route(
            "/api/v1/personal/tasks/conversations/{thread}/reports/{id}",
            get(report),
        )
}

async fn page(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(thread): Path<Uuid>,
    Query(q): Query<GroupQuery>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .conversation_work_page(s.owner, user, thread.to_string(), q.request_id)
            .await?,
    )
    .into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(thread): Path<Uuid>,
    Json(request): Json<ConversationWorkRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .create_conversation_work(s.owner, user, epoch, thread.to_string(), request)
            .await?,
    )
    .into_response())
}

async fn control(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(thread): Path<Uuid>,
    Json(action): Json<ConversationWorkAction>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .control_conversation_work(s.owner, user, epoch, thread.to_string(), action)
            .await?,
    )
    .into_response())
}

async fn report(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path((thread, id)): Path<(Uuid, Uuid)>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .conversation_work_report(s.owner, user, thread, id)
            .await?,
    )
    .into_response())
}
