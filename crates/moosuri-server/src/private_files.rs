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
    let pdf = mime == "application/pdf";
    let extractor = state.config.pdf_extractor.clone();
    if pdf && extractor.is_none() {
        return Ok(failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "pdf_extractor_unavailable",
        ));
    }
    let _permit = UPLOADS.try_acquire().map_err(|_| StoreError::Busy)?;
    let bytes = tokio::time::timeout(
        Duration::from_secs(30),
        to_bytes(body, private_files::upload_limit(&mime)),
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
            let outcome = tokio::spawn(async move {
                let _permit = _permit;
                let result = if pdf {
                    match moosuri_documents::extract_pdf(
                        extractor.as_deref().ok_or(StoreError::Unavailable)?,
                        &bytes,
                    )
                    .await
                    {
                        Ok(value) => private_files::extracted_pdf(bytes.to_vec(), value.text)
                            .map_err(|_| moosuri_documents::PdfError::Invalid),
                        Err(error) => Err(error),
                    }
                } else {
                    tokio::task::spawn_blocking(move || {
                        private_files::extract(bytes.to_vec(), &mime)
                    })
                    .await
                    .map_err(|_| StoreError::Unavailable)?
                    .map_err(|_| moosuri_documents::PdfError::Invalid)
                };
                match result {
                    Ok(file) => store.file_finish(upload, Some(file)).await.map(Ok),
                    Err(error) => {
                        store.file_finish(upload, None).await?;
                        Ok(Err(error))
                    }
                }
            })
            .await
            .map_err(|_| StoreError::Unavailable)??;
            match outcome {
                Ok(value) => Ok(Json(value).into_response()),
                Err(error) => {
                    use moosuri_documents::PdfError;
                    let code = if !pdf {
                        "invalid_file"
                    } else {
                        match error {
                            PdfError::Encrypted => "pdf_encrypted",
                            PdfError::NoText => "pdf_no_text",
                            PdfError::Unsupported => "pdf_unsupported_content",
                            PdfError::TooLarge => "pdf_extraction_limit",
                            PdfError::TimedOut => "pdf_extraction_timeout",
                            PdfError::Unavailable => "pdf_extractor_unavailable",
                            PdfError::Invalid => "pdf_invalid",
                        }
                    };
                    Ok(failure(
                        if error == PdfError::Unavailable {
                            StatusCode::SERVICE_UNAVAILABLE
                        } else {
                            StatusCode::BAD_REQUEST
                        },
                        code,
                    ))
                }
            }
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
    let (bytes, mime) = state.store.file_download(&user, id).await?;
    Ok((
        [
            (header::CONTENT_TYPE, "application/octet-stream"),
            (header::CACHE_CONTROL, "no-store"),
            (
                header::CONTENT_DISPOSITION,
                if mime == "application/pdf" {
                    "attachment; filename=\"document.pdf\""
                } else {
                    "attachment; filename=\"document.txt\""
                },
            ),
            (header::X_CONTENT_TYPE_OPTIONS, "nosniff"),
        ],
        bytes,
    )
        .into_response())
}
