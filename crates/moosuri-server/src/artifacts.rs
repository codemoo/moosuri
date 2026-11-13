//! Authenticated private local records. Exports never fetch remote content.
use super::*;
use axum::extract::Query;
use moosuri_core::{ArtifactDelete, ArtifactKind, ArtifactWrite};
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ListQuery {
    kind: Option<ArtifactKind>,
    after: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadQuery {
    revision: Option<i64>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    before: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ExportQuery {
    revision: i64,
    format: String,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list).post(write))
        .route("/briefing", get(briefing))
        .route("/{id}", get(read).delete(delete))
        .route("/{id}/history", get(history))
        .route("/{id}/export", get(export))
        .route("/{id}/export-data", get(export_data))
        .layer(DefaultBodyLimit::max(96 * 1024))
}
async fn list(State(s): State<AppState>, h: HeaderMap, Query(q): Query<ListQuery>) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.artifacts(&u, q.kind, q.after).await?).into_response())
}
async fn briefing(
    State(s): State<AppState>,
    h: HeaderMap,
    Query(q): Query<ListQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    if q.kind.is_some() {
        return Err(StoreError::InvalidInput.into());
    }
    Ok(Json(s.store.followup_briefing(&u, q.after).await?).into_response())
}
async fn read(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ReadQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.artifact(&u, id, q.revision).await?).into_response())
}
async fn history(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<HistoryQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.artifact_history(&u, id, q.before).await?).into_response())
}
async fn write(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(input): Json<ArtifactWrite>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let _gate = s.delivery.write().await;
    let r = s.store.write_artifact(&u, input).await?;
    s.runtime.wake();
    Ok(Json(r).into_response())
}
async fn delete(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ArtifactDelete>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let _gate = s.delivery.write().await;
    let r = s.store.delete_artifact(&u, id, input).await?;
    s.runtime.wake();
    Ok(Json(r).into_response())
}
async fn export(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    if q.format != "markdown" && q.format != "html" {
        return Err(StoreError::InvalidInput.into());
    }
    let _gate = s.delivery.read().await;
    let html = q.format == "html";
    let body = s.store.export_artifact(&u, id, q.revision, html).await?;
    Ok(([("content-type",if html{"text/html; charset=utf-8"}else{"text/markdown; charset=utf-8"}),("content-disposition",if html{"attachment; filename=artifact.html"}else{"attachment; filename=artifact.md"}),("content-security-policy","default-src 'none'; base-uri 'none'; form-action 'none'; frame-ancestors 'none'; sandbox"),("cache-control","no-store")],body).into_response())
}

async fn export_data(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Query(q): Query<ExportQuery>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    if q.format != "markdown" && q.format != "html" {
        return Err(StoreError::InvalidInput.into());
    }
    let _gate = s.delivery.read().await;
    Ok(Json(serde_json::json!({"content":s.store.export_artifact(&u,id,q.revision,q.format=="html").await?})).into_response())
}
