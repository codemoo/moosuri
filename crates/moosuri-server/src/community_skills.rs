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
        .route("/catalog/status", get(catalog_status))
        .route("/catalog/refresh", post(catalog_refresh))
        .route("/catalog/{id}/preview", post(catalog_preview))
        .route("/{id}", post(mutate))
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&s, &headers).await?;
    refresh_catalog(&s, &user, false).await?;
    Ok(Json(catalog_cards(&s, &user).await?).into_response())
}
async fn mutate(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<CommunitySkillMutation>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    if matches!(input.action.as_str(), "hide" | "show")
        && let Some(source) = &s.config.community_catalog
        && let Some(digest) = &input.expected_digest
        && s.store
            .community_catalog_preview(&user, source.clone(), id.clone(), digest.clone())
            .await
            .is_ok()
    {
        s.store
            .set_community_catalog_hidden(
                &user,
                source.clone(),
                id,
                digest.clone(),
                input.expected_revision,
                input.action == "hide",
            )
            .await?;
        return Ok(Json(catalog_cards(&s, &user).await?).into_response());
    }
    if id.starts_with("private:") {
        s.store.mutate_learned_skill(&user, id, input).await?;
    } else {
        s.store.mutate_community_skill(&user, id, input).await?;
    }
    Ok(Json(catalog_cards(&s, &user).await?).into_response())
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

async fn catalog_cards(
    s: &AppState,
    u: &UserContext,
) -> Result<Vec<moosuri_core::CommunitySkillCard>, StoreError> {
    let mut cards = if let Some(source) = &s.config.community_catalog {
        s.store.community_catalog_cards(u, source.clone()).await?
    } else {
        s.store.community_skills(u).await?
    };
    cards.extend(s.store.learned_skills(u).await?);
    Ok(cards)
}
async fn refresh_catalog(s: &AppState, u: &UserContext, manual: bool) -> Result<(), StoreError> {
    let Some(source) = &s.config.community_catalog else {
        return Ok(());
    };
    let Ok(_permit) = DOWNLOADS.try_acquire() else {
        return Ok(());
    };
    let Some(lease) = s
        .store
        .begin_community_catalog_refresh(u, source.clone(), manual)
        .await?
    else {
        return Ok(());
    };
    let result = match catalog_client(s) {
        Ok(client) => client.index(source, lease.etag.as_deref()).await,
        Err(error) => Err(error),
    };
    s.store
        .complete_community_catalog_refresh(u, lease, result)
        .await
}
async fn catalog_status(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(
        s.store
            .community_catalog_status(&u, s.config.community_catalog.clone())
            .await?,
    )
    .into_response())
}
async fn catalog_refresh(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    refresh_catalog(&s, &u, true).await?;
    Ok(Json(
        s.store
            .community_catalog_status(&u, s.config.community_catalog.clone())
            .await?,
    )
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CatalogPreviewInput {
    expected_digest: String,
}
async fn catalog_preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<CatalogPreviewInput>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let source = s
        .config
        .community_catalog
        .clone()
        .ok_or(StoreError::NotFound)?;
    let p = s
        .store
        .community_catalog_preview(&u, source.clone(), id, input.expected_digest)
        .await?;
    if let Some(card) = s.store.reuse_community_catalog_preview(&u, &p).await? {
        return Ok(Json(card).into_response());
    }
    let _permit = DOWNLOADS.try_acquire().map_err(|_| StoreError::Busy)?;
    let client = catalog_client(&s).map_err(|_| StoreError::Unavailable)?;
    let artifact = client
        .artifact(&source, &p.item)
        .await
        .map_err(|_| StoreError::Unavailable)?;
    // Artifact claims cannot manufacture the existing verified-public-source carrier.
    let input = GitHubSkillSource {
        repository: p.repository.clone(),
        directory: p.item.directory.clone(),
        reference: p.commit.clone(),
    };
    #[cfg(feature = "test-fixtures")]
    let downloaded = if let Some((_, github)) = &s.config.community_catalog_fixtures {
        moosuri_connectors::github_skills::acquire_github_skill_fixture(input, github).await
    } else {
        acquire_github_skill(input).await
    };
    #[cfg(not(feature = "test-fixtures"))]
    let downloaded = acquire_github_skill(input).await;
    let downloaded = downloaded.map_err(|_| StoreError::Unavailable)?;
    Ok(Json(
        s.store
            .complete_community_catalog_preview(&u, p, artifact, downloaded)
            .await?,
    )
    .into_response())
}

fn catalog_client(
    s: &AppState,
) -> Result<
    moosuri_connectors::github_catalog::CatalogClient,
    moosuri_connectors::github_catalog::Error,
> {
    #[cfg(feature = "test-fixtures")]
    if let Some((raw, _)) = &s.config.community_catalog_fixtures {
        return moosuri_connectors::github_catalog::CatalogClient::synthetic(raw);
    }
    let _ = s;
    moosuri_connectors::github_catalog::CatalogClient::new()
}
