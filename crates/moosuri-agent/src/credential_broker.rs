//! Explicit user-requested credential operation, without a model credential path.
use moosuri_store::{Store, StoreError, UserContext};
use moosuri_vault_protocol::broker::GitHubRepository;
pub async fn github_metadata(
    store: &Store,
    user: &UserContext,
    request_id: String,
    repository: GitHubRepository,
) -> Result<moosuri_store::credential_broker::github::GitHubJob, StoreError> {
    store
        .request_github_metadata(user, request_id, repository)
        .await
}

pub async fn google_inbox(
    store: &Store,
    user: &UserContext,
    request_id: String,
) -> Result<moosuri_store::credential_broker::github::ScopedJob, StoreError> {
    store.request_google_inbox(user, request_id).await
}
