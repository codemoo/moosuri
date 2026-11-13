//! Private UTF-8 source routes. Binary bodies remain outside ordinary DTOs.
use super::*;
use axum::{
    body::{Body, to_bytes},
    extract::Query,
};
use moosuri_store::{media::UploadAdmission, private_files};
use tokio::sync::Semaphore;
static UPLOADS: Semaphore = Semaphore::const_new(2);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Name {
    filename: String,
}
pub(super) async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(name): Query<Name>,
    body: Body,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let (_, mime) = private_files::filename(&name.filename)?;
    let _permit = UPLOADS.try_acquire().map_err(|_| StoreError::Busy)?;
    let bytes = tokio::time::timeout(
        Duration::from_secs(30),
        to_bytes(body, private_files::MAX_FILE_BYTES),
    )
    .await
    .map_err(|_| StoreError::InvalidInput)?
    .map_err(|_| StoreError::InvalidInput)?;
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    match state
        .store
        .file_begin(&user, id, digest, name.filename)
        .await?
    {
        UploadAdmission::Ready(value) => Ok(Json(value).into_response()),
        UploadAdmission::Process(upload) => {
            let store = state.store.clone();
            tokio::spawn(async move {
                let (result, _permit) = tokio::task::spawn_blocking(move || {
                    (private_files::extract(bytes.to_vec(), &mime), _permit)
                })
                .await
                .map_err(|_| StoreError::Unavailable)?;
                match result {
                    Ok(file) => store.file_finish(upload, Some(file)).await,
                    Err(_) => {
                        let _ = store.file_finish(upload, None).await;
                        Err(StoreError::InvalidInput)
                    }
                }
            })
            .await
            .map_err(|_| StoreError::Unavailable)?
            .map(|v| Json(v).into_response())
            .map_err(Into::into)
        }
    }
}
pub(super) async fn preview(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _delivery = state.delivery.read().await;
    let user = current(&state, &headers).await?;
    let text = state.store.file_preview(&user, id).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/json"),
            (header::CACHE_CONTROL, "no-store"),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        Json(text),
    )
        .into_response())
}
pub(super) async fn content(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _delivery = state.delivery.read().await;
    let user = current(&state, &headers).await?;
    // Reject image IDs on this route; original bytes are immutable and account owned.
    state.store.file_preview(&user, id.clone()).await?;
    let bytes = state.store.file_bytes(&user, id).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_DISPOSITION,
                "attachment; filename=\"document.txt\"",
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
