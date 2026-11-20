use super::*;
use moosuri_core::personal_browser::{TaskDetail, TaskPage};

const BASE: &str = "/api/v1/personal/tasks";
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(BASE, get(handler))
        .route("/api/v1/personal/tasks/{id}", get(handler))
}
async fn handler(
    State(g): State<Gateway>,
    headers: HeaderMap,
    method: Method,
    uri: Uri,
    body: axum::body::Bytes,
) -> Response {
    handle(&g, &headers, method, uri, &body)
        .await
        .unwrap_or_else(failure)
}
async fn handle(
    g: &Gateway,
    headers: &HeaderMap,
    method: Method,
    uri: Uri,
    body: &[u8],
) -> Result<Response, StatusCode> {
    g.headers(headers, &method)?;
    if method != Method::GET || !body.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut path = BASE.to_owned();
    let id = if uri.path() == BASE {
        if let Some(query) = uri.query() {
            let before: i64 = query
                .strip_prefix("before=")
                .and_then(|v| v.parse().ok())
                .filter(|v| *v > 0)
                .ok_or(StatusCode::BAD_REQUEST)?;
            path = format!("{BASE}?before={before}");
        }
        None
    } else {
        let raw = uri
            .path()
            .strip_prefix(BASE)
            .and_then(|v| v.strip_prefix('/'))
            .ok_or(StatusCode::NOT_FOUND)?;
        let id = Uuid::parse_str(raw)
            .ok()
            .filter(|id| !id.is_nil() && id.to_string() == raw)
            .ok_or(StatusCode::BAD_REQUEST)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        path = format!("{BASE}/{id}");
        Some(id)
    };
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (allocation, session) = authenticated_allocation(g, headers, true).await?;
    let upstream = g
        .proxy(&allocation, method, &path, None, Some(&session))
        .await;
    recheck(g, &allocation).await?;
    let (status, bytes, _) = upstream?;
    if status != StatusCode::OK {
        return api_error(status, &bytes);
    }
    let invalid = |_| StatusCode::SERVICE_UNAVAILABLE;
    let value = if let Some(id) = id {
        let detail: TaskDetail = serde_json::from_slice(&bytes).map_err(invalid)?;
        if detail.task.id != id.to_string() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&detail)
    } else {
        serde_json::to_vec(&serde_json::from_slice::<TaskPage>(&bytes).map_err(invalid)?)
    }
    .map_err(invalid)?;
    Ok(response(status, value))
}
