//! Authenticated local catalog/lifecycle. Installation never executes a task.
use super::*;
use moosuri_core::CommunitySkillMutation;
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/{id}", post(mutate))
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.store.community_skills(&user).await?).into_response())
}
async fn mutate(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<CommunitySkillMutation>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.store.mutate_community_skill(&user, id, input).await?).into_response())
}
