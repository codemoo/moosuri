//! Authenticated human controls. These routes are never registered as model tools.
use super::*;
use moosuri_connectors::browser::capsule::WorkspaceInput;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Open {
    url: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Version {
    workspace_id: String,
    revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    workspace_id: String,
    revision: i64,
    control_token: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Control {
    workspace_id: String,
    revision: i64,
    control_token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Navigate {
    workspace_id: String,
    revision: i64,
    control_token: String,
    url: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    workspace_id: String,
    revision: i64,
    control_token: String,
    frame_token: String,
    input: WorkspaceInput,
}

pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/v1/threads/{id}/browser", get(status).post(open))
        .route("/api/v1/threads/{id}/browser/takeover", post(takeover))
        .route("/api/v1/threads/{id}/browser/frame", post(frame))
        .route("/api/v1/threads/{id}/browser/input", post(input))
        .route("/api/v1/threads/{id}/browser/navigate", post(navigate))
        .route("/api/v1/threads/{id}/browser/resume", post(resume))
        .route("/api/v1/threads/{id}/browser/close", post(close))
        .layer(DefaultBodyLimit::max(16 * 1024))
}
async fn status(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(s.browser.status(&user, &thread).await?).into_response())
}
async fn open(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Open>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let result = s.browser.open(&user, &thread, &input.url).await?;
    s.runtime.wake();
    Ok((StatusCode::CREATED, Json(result)).into_response())
}
async fn takeover(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Version>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(
        s.browser
            .takeover(&user, &thread, &input.workspace_id, input.revision)
            .await?,
    )
    .into_response())
}
async fn frame(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Frame>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(
        s.browser
            .frame(
                &user,
                &thread,
                &input.workspace_id,
                input.revision,
                input.control_token,
            )
            .await?,
    )
    .into_response())
}
async fn input(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Input>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    s.browser
        .input(
            &user,
            &thread,
            &input.workspace_id,
            input.revision,
            input.control_token,
            &input.frame_token,
            input.input,
        )
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn navigate(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Navigate>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(
        s.browser
            .navigate(
                &user,
                &thread,
                &input.workspace_id,
                input.revision,
                input.control_token,
                &input.url,
            )
            .await?,
    )
    .into_response())
}
async fn resume(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Control>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    let turn = s
        .browser
        .resume(
            &user,
            &thread,
            &input.workspace_id,
            input.revision,
            input.control_token,
        )
        .await?;
    s.runtime.wake();
    Ok(Json(turn).into_response())
}
async fn close(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(thread): Path<String>,
    Json(input): Json<Version>,
) -> HttpResult {
    let user = current(&s, &headers).await?;
    Ok(Json(
        s.browser
            .close(&user, &thread, &input.workspace_id, input.revision)
            .await?,
    )
    .into_response())
}
