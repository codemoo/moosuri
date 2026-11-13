//! Binary routes never deserialize file bytes into ordinary API DTOs.
use super::*;
use axum::body::{Body, to_bytes};
use moosuri_store::media::{self, UploadAdmission};
use tokio::sync::Semaphore;
// Admission occurs before body collection; at most two 2MiB request/decode jobs.
static UPLOADS: Semaphore = Semaphore::const_new(2);
pub(super) async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Body,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _permit = UPLOADS.try_acquire().map_err(|_| StoreError::Busy)?;
    let bytes = tokio::time::timeout(Duration::from_secs(30), to_bytes(body, media::MAX_BYTES))
        .await
        .map_err(|_| StoreError::InvalidInput)?
        .map_err(|_| StoreError::InvalidInput)?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    match state.store.media_begin(&user, id, digest).await? {
        UploadAdmission::Ready(value) => Ok(Json(value).into_response()),
        UploadAdmission::Process(upload) => {
            // Task owns admission through durable settlement. A lost HTTP ACK
            // cannot abandon a reservation or allow unbounded detached decodes.
            let store = state.store.clone();
            tokio::spawn(async move {
                let (image, _permit) =
                    tokio::task::spawn_blocking(move || (media::normalize(&bytes), _permit))
                        .await
                        .map_err(|_| StoreError::Unavailable)?;
                match image {
                    Ok(image) => store.media_finish(upload, Some(image)).await,
                    Err(_) => {
                        let _ = store.media_finish(upload, None).await;
                        Err(StoreError::InvalidInput)
                    }
                }
            })
            .await
            .map_err(|_| StoreError::Unavailable)?
            .map(|value| Json(value).into_response())
            .map_err(Into::into)
        }
    }
}
pub(super) async fn library(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.media_library(&user).await?).into_response())
}
pub(super) async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _delivery = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    state.store.delete_media(&user, id).await?;
    state.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn binary(state: AppState, headers: HeaderMap, id: String, thumb: bool) -> HttpResult {
    let _delivery = state.delivery.read().await;
    let user = current(&state, &headers).await?;
    let bytes = state.store.media_bytes(&user, id, thumb).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "image/jpeg"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"image.jpg\"",
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
pub(super) async fn content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    binary(state, headers, id, false).await
}
pub(super) async fn thumbnail(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    binary(state, headers, id, true).await
}
