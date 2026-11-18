//! Manual memory management uses the ordinary private Store and its revision checks.
use super::*;
use moosuri_core::{MemoryDocument, MemoryHistory, MemoryPage};

const COLLECTION: &str = "/api/v1/knowledge/memories";
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(COLLECTION, get(handler).post(handler))
        .route(
            "/api/v1/knowledge/memories/{id}",
            get(handler).put(handler).delete(handler),
        )
        .route("/api/v1/knowledge/memories/{id}/history", get(handler))
        .layer(DefaultBodyLimit::max(128 * 1024))
}
async fn handler(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    match handle(&g, method, uri, &headers, &body).await {
        Ok(result) => result,
        Err(code) => failure(code),
    }
}
async fn handle(
    g: &Gateway,
    method: Method,
    uri: Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Response, StatusCode> {
    g.headers(headers, &method)?;
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (allocation, session) = authenticated_allocation(g, headers, true).await?;
    let tail = uri
        .path()
        .strip_prefix(COLLECTION)
        .ok_or(StatusCode::NOT_FOUND)?;
    let history = tail.ends_with("/history");
    let id = if tail.is_empty() {
        None
    } else {
        let raw = tail
            .trim_start_matches('/')
            .strip_suffix("/history")
            .unwrap_or(tail.trim_start_matches('/'));
        Some(
            Uuid::parse_str(raw)
                .ok()
                .filter(|id| !id.is_nil() && id.to_string() == raw)
                .ok_or(StatusCode::BAD_REQUEST)?,
        )
    };
    let mut path = match id {
        None => COLLECTION.to_owned(),
        Some(id) if history => format!("{COLLECTION}/{id}/history"),
        Some(id) => format!("{COLLECTION}/{id}"),
    };
    if let Some(query) = uri.query() {
        let key = if history {
            "before"
        } else if id.is_some() {
            "revision"
        } else {
            "after"
        };
        let (name, value) = query.split_once('=').ok_or(StatusCode::BAD_REQUEST)?;
        if method != Method::GET
            || name != key
            || value.is_empty()
            || value.len() > 2048
            || !value.bytes().all(|b| {
                if key == "after" {
                    b.is_ascii_hexdigit()
                } else {
                    b.is_ascii_digit()
                }
            })
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        path.push('?');
        path.push_str(query);
    }
    let payload = if method == Method::GET {
        if !body.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        None
    } else {
        if one(headers, "content-type").and_then(|s| s.split(';').next())
            != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        Some(body)
    };
    let upstream = g
        .proxy(&allocation, method.clone(), &path, payload, Some(&session))
        .await;
    recheck(g, &allocation).await?;
    let (status, bytes, _) = upstream?;
    if !status.is_success() {
        return api_error(status, &bytes);
    }
    let invalid = |_| StatusCode::SERVICE_UNAVAILABLE;
    let bytes = if method == Method::DELETE && status == StatusCode::NO_CONTENT && bytes.is_empty()
    {
        bytes
    } else if method == Method::GET && status == StatusCode::OK && id.is_none() {
        serde_json::to_vec(&serde_json::from_slice::<MemoryPage>(&bytes).map_err(invalid)?)
            .map_err(invalid)?
    } else if history && status == StatusCode::OK {
        let result: MemoryHistory = serde_json::from_slice(&bytes).map_err(invalid)?;
        if id.is_none_or(|id| result.items.iter().any(|m| m.id != id.to_string())) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&result).map_err(invalid)?
    } else if (method == Method::POST && status == StatusCode::CREATED)
        || ((method == Method::GET || method == Method::PUT) && status == StatusCode::OK)
    {
        let result: MemoryDocument = serde_json::from_slice(&bytes).map_err(invalid)?;
        if id.is_some_and(|id| result.id != id.to_string()) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&result).map_err(invalid)?
    } else {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    };
    Ok(response(status, bytes))
}
