//! Current-account extraction controls. Background authority is minted only in Store.
use super::*;
use moosuri_core::MemoryExtractionChange;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    expected_revision: i64,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/collections/{id}/extraction", get(status).put(change))
        .route("/sources/{id}/extraction", get(source_status).post(extract))
}
async fn status(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.store.memory_extraction_status(&user, id).await?).into_response())
}
async fn change(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<MemoryExtractionChange>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let result = s.store.set_memory_extraction(&user, id, input).await?;
    s.runtime.wake();
    Ok(Json(result).into_response())
}
async fn extract(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<Revision>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let _gate = s.delivery.write().await;
    let result = s
        .store
        .request_memory_extraction(&user, id, input.expected_revision)
        .await?;
    s.runtime.wake();
    Ok(Json(result).into_response())
}

async fn source_status(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.store.source_memory_extraction_status(&user, id).await?).into_response())
}
