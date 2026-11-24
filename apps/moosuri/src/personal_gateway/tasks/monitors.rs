use super::*;
use moosuri_store::personal_browser::{MonitorPage, MonitorRequest, MonitorStop, MonitorView};

pub(super) const BASE: &str = "/api/v1/personal/tasks/monitors";

pub(super) async fn forward(
    g: &Gateway,
    headers: &HeaderMap,
    method: Method,
    uri: &Uri,
    body: &[u8],
) -> Result<Response, StatusCode> {
    let bad = StatusCode::BAD_REQUEST;
    if uri.query().is_some()
        || (method == Method::GET && !body.is_empty())
        || body.len() > 16 * 1024
    {
        return Err(bad);
    }
    if method == Method::POST
        && one(headers, "content-type").and_then(|v| v.split(';').next())
            != Some("application/json")
    {
        return Err(bad);
    }
    let suffix = uri.path().strip_prefix(BASE).ok_or(bad)?;
    let id = match (method.as_str(), suffix) {
        ("GET", "") => None,
        ("POST", "") => {
            let input: MonitorRequest = serde_json::from_slice(body).map_err(|_| bad)?;
            Some(task_uuid(&input.request_id)?)
        }
        ("GET", s) => Some(task_uuid(s.strip_prefix('/').ok_or(bad)?)?),
        ("POST", s) => {
            let stop: MonitorStop = serde_json::from_slice(body).map_err(|_| bad)?;
            task_uuid(&stop.request_id)?;
            Some(task_uuid(
                s.strip_prefix('/')
                    .and_then(|s| s.strip_suffix("/stop"))
                    .ok_or(bad)?,
            )?)
        }
        _ => return Err(bad),
    };
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (allocation, session) = authenticated_allocation(g, headers, true).await?;
    let payload = (method == Method::POST).then_some(body);
    let upstream = g
        .proxy(&allocation, method, uri.path(), payload, Some(&session))
        .await;
    recheck(g, &allocation).await?;
    let (status, bytes, _) = upstream?;
    if status != StatusCode::OK {
        return api_error(status, &bytes);
    }
    let invalid = |_| StatusCode::SERVICE_UNAVAILABLE;
    let value = if let Some(id) = id {
        let result: MonitorView = serde_json::from_slice(&bytes).map_err(invalid)?;
        if result.id != id.to_string() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&result)
    } else {
        serde_json::to_vec(&serde_json::from_slice::<MonitorPage>(&bytes).map_err(invalid)?)
    }
    .map_err(invalid)?;
    Ok(response(status, value))
}
