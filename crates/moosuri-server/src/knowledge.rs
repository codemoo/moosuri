//! Tenant-scoped knowledge management. No external fetch, extraction or chat tool.
use super::*;
use axum::extract::Query;
use moosuri_core::{DisconnectMode, IngestBatch, KnowledgeQuery, MemoryDraft};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CollectionInput {
    name: String,
    scope: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DisconnectInput {
    expected_revision: i64,
    mode: DisconnectMode,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    after: Option<String>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionQuery {
    revision: Option<i64>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryQuery {
    before: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RevisionInput {
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct NewMemory {
    request_id: String,
    draft: MemoryDraft,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemoryCorrection {
    request_id: String,
    expected_revision: i64,
    draft: MemoryDraft,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .merge(crate::automatic_memory::routes())
        .route("/collections", get(collections).post(create_collection))
        .route("/collections/{id}/disconnect", post(disconnect))
        .route("/collections/{id}/sources", get(sources))
        .route(
            "/collections/{id}/batches",
            post(ingest).layer(DefaultBodyLimit::max(2 * 1024 * 1024)),
        )
        .route("/sources/{id}", get(source).delete(delete_source))
        .route("/search", post(search))
        .route(
            "/memories",
            get(memories)
                .post(create_memory)
                .layer(DefaultBodyLimit::max(128 * 1024)),
        )
        .route(
            "/memories/{id}",
            get(memory)
                .put(correct_memory)
                .delete(forget_memory)
                .layer(DefaultBodyLimit::max(128 * 1024)),
        )
        .route("/memories/{id}/history", get(memory_history))
}
async fn collections(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(
        Json(serde_json::json!({"collections":state.store.collections(&user).await?}))
            .into_response(),
    )
}
async fn create_collection(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CollectionInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    let response = Ok((
        StatusCode::CREATED,
        Json(
            state
                .store
                .create_collection(&user, input.name, input.scope)
                .await?,
        ),
    )
        .into_response());
    state.runtime.wake();
    response
}
async fn disconnect(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<DisconnectInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    let response = Ok(Json(
        state
            .store
            .disconnect_collection(&user, id, input.expected_revision, input.mode)
            .await?,
    )
    .into_response());
    state.runtime.wake();
    response
}
async fn sources(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<PageQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.sources(&user, id, query.after).await?).into_response())
}
async fn ingest(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<IngestBatch>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    let response = Ok((
        StatusCode::CREATED,
        Json(state.store.ingest_text(&user, id, input).await?),
    )
        .into_response());
    state.runtime.wake();
    response
}
async fn source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<RevisionQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.source(&user, id, query.revision).await?).into_response())
}
async fn delete_source(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RevisionInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    state
        .store
        .delete_source(&user, id, input.expected_revision)
        .await?;
    state.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(query): Json<KnowledgeQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.search_knowledge(&user, query).await?).into_response())
}
async fn memories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<PageQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.memories(&user, query.after).await?).into_response())
}
async fn memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<RevisionQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.memory(&user, id, query.revision).await?).into_response())
}
async fn memory_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<HistoryQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.memory_history(&user, id, query.before).await?).into_response())
}
async fn create_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<NewMemory>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    let response = Ok((
        StatusCode::CREATED,
        Json(
            state
                .store
                .create_memory(&user, input.request_id, input.draft)
                .await?,
        ),
    )
        .into_response());
    state.runtime.wake();
    response
}
async fn correct_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<MemoryCorrection>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    let response = Ok(Json(
        state
            .store
            .correct_memory(
                &user,
                id,
                input.expected_revision,
                input.request_id,
                input.draft,
            )
            .await?,
    )
    .into_response());
    state.runtime.wake();
    response
}
async fn forget_memory(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<RevisionInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let _delivery = state.delivery.write().await;
    state
        .store
        .forget_memory(&user, id, input.expected_revision)
        .await?;
    state.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
