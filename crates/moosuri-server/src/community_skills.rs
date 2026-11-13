//! Authenticated local catalog/lifecycle. Installation never executes a task.
use super::*;
use moosuri_connectors::github_skills::{
    GitHubSkillError, GitHubSkillSource, acquire_github_skill,
};
use moosuri_core::{CommunitySkillMutation, GitHubSkillPreviewInput};
static DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/github/preview", post(preview))
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

async fn preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<GitHubSkillPreviewInput>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _permit = DOWNLOADS.try_acquire().map_err(|_| StoreError::Busy)?;
    let downloaded = acquire_github_skill(GitHubSkillSource {
        repository: input.repository,
        directory: input.directory,
        reference: input.reference,
    })
    .await
    .map_err(|error| match error {
        GitHubSkillError::InvalidSource
        | GitHubSkillError::Unsupported
        | GitHubSkillError::InvalidResponse
        | GitHubSkillError::TooLarge => StoreError::InvalidInput,
        GitHubSkillError::NotFound => StoreError::NotFound,
        GitHubSkillError::RateLimited => StoreError::Busy,
        _ => StoreError::Unavailable,
    })?;
    Ok(Json(s.store.preview_github_skill(&user, downloaded).await?).into_response())
}
