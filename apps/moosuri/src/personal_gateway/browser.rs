//! Forward only this session's personal browser operations; frames are never cached.
use super::*;
use moosuri_core::personal_browser::{Control, Frame, Status};

const BASE: &str = "/api/v1/threads/{id}/browser";
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(BASE, get(handler).post(handler))
        .route("/api/v1/threads/{id}/browser/google-sign-in", post(handler))
        .route("/api/v1/threads/{id}/browser/frame", get(handler))
        .route("/api/v1/threads/{id}/browser/takeover", post(handler))
        .route("/api/v1/threads/{id}/browser/input", post(handler))
        .route("/api/v1/threads/{id}/browser/resume", post(handler))
        .route("/api/v1/threads/{id}/browser/close", post(handler))
        .route("/api/v1/threads/{id}/browser/tabs", post(handler))
        .layer(DefaultBodyLimit::max(64 * 1024))
}

fn target(method: &Method, uri: &Uri) -> Result<(Uuid, &'static str), StatusCode> {
    let (raw, tail) = uri
        .path()
        .strip_prefix("/api/v1/threads/")
        .and_then(|path| path.split_once('/'))
        .ok_or(StatusCode::NOT_FOUND)?;
    let id = Uuid::parse_str(raw)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == raw)
        .ok_or(StatusCode::BAD_REQUEST)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let operation = match tail {
        "browser" if method == Method::GET || method == Method::POST => "",
        "browser/frame" if method == Method::GET => "frame",
        "browser/input" if method == Method::POST => "input",
        "browser/takeover" if method == Method::POST => "takeover",
        "browser/resume" if method == Method::POST => "resume",
        "browser/close" if method == Method::POST => "close",
        "browser/tabs" if method == Method::POST => "tabs",
        "browser/google-sign-in" if method == Method::POST => "google-sign-in",
        _ => return Err(StatusCode::NOT_FOUND),
    };
    Ok((id, operation))
}

async fn handler(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    handle(&g, method, uri, &headers, &body)
        .await
        .unwrap_or_else(failure)
}
async fn handle(
    g: &Gateway,
    method: Method,
    uri: Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Response, StatusCode> {
    let (thread, operation) = target(&method, &uri)?;
    g.headers(headers, &method)?;
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (owner, session) = authenticated_allocation(g, headers, true).await?;
    let payload = if method == Method::GET {
        if !body.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        None
    } else {
        if one(headers, "content-type").and_then(|v| v.split(';').next())
            != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        Some(body)
    };
    let upstream = g
        .proxy(&owner, method, uri.path(), payload, Some(&session))
        .await;
    recheck(g, &owner).await?;
    let (status, bytes, _) = upstream?;
    if !status.is_success() {
        return api_error(status, &bytes);
    }
    let invalid = |_| StatusCode::SERVICE_UNAVAILABLE;
    let bytes = match operation {
        "frame" => {
            let frame: Frame = serde_json::from_slice(&bytes).map_err(invalid)?;
            serde_json::to_vec(&frame).map_err(invalid)?
        }
        "takeover" => {
            let control: Control = serde_json::from_slice(&bytes).map_err(invalid)?;
            if control.workspace.thread_id != thread.to_string() {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::to_vec(&control).map_err(invalid)?
        }
        _ => {
            let view: Status = serde_json::from_slice(&bytes).map_err(invalid)?;
            if view
                .workspace
                .as_ref()
                .is_some_and(|w| w.thread_id != thread.to_string())
            {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::to_vec(&view).map_err(invalid)?
        }
    };
    Ok(response(status, bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn routes_only_canonical_thread_operations_without_query_tokens() {
        let id = Uuid::new_v4();
        let path = format!("/api/v1/threads/{id}/browser");
        for (method, suffix) in [
            (Method::GET, ""),
            (Method::POST, ""),
            (Method::GET, "/frame"),
            (Method::POST, "/resume"),
            (Method::POST, "/tabs"),
            (Method::POST, "/google-sign-in"),
        ] {
            assert_eq!(
                target(&method, &format!("{path}{suffix}").parse().unwrap())
                    .unwrap()
                    .0,
                id
            );
        }
        for (method, suffix) in [
            (Method::GET, "/input"),
            (Method::POST, "/frame"),
            (Method::GET, "/frame?lease=x"),
            (Method::GET, "/debug"),
            (Method::GET, "/google-sign-in"),
        ] {
            assert!(target(&method, &format!("{path}{suffix}").parse().unwrap()).is_err());
        }
    }
}
