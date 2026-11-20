use super::*;
use moosuri_core::personal_browser::{
    TaskDetail, TaskPage, WorkAction, WorkReceipt, WorkRequest, task_cursor,
};

const BASE: &str = "/api/v1/personal/tasks";
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(BASE, get(handler))
        .route(
            "/api/v1/personal/tasks/workflows",
            axum::routing::post(handler),
        )
        .route(
            "/api/v1/personal/tasks/workflows/{id}",
            axum::routing::post(handler),
        )
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
    if method == Method::POST {
        return mutate(g, headers, &uri, body).await;
    }
    if method != Method::GET || !body.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut path = BASE.to_owned();
    let id = if uri.path() == BASE {
        if let Some(query) = uri.query() {
            let before = query
                .strip_prefix("before=")
                .filter(|v| task_cursor(v).is_some())
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

async fn mutate(
    g: &Gateway,
    headers: &HeaderMap,
    uri: &Uri,
    body: &[u8],
) -> Result<Response, StatusCode> {
    let bad = StatusCode::BAD_REQUEST;
    if uri.query().is_some()
        || body.len() > 16 * 1024
        || one(headers, "content-type").and_then(|v| v.split(';').next())
            != Some("application/json")
    {
        return Err(bad);
    }
    let base = "/api/v1/personal/tasks/workflows";
    let id = if uri.path() == base {
        let q: WorkRequest = serde_json::from_slice(body).map_err(|_| bad)?;
        q.request_id
    } else {
        let id = uri
            .path()
            .strip_prefix("/api/v1/personal/tasks/workflows/")
            .ok_or(bad)?;
        Uuid::parse_str(id)
            .ok()
            .filter(|v| !v.is_nil() && v.to_string() == id)
            .ok_or(bad)?;
        serde_json::from_slice::<WorkAction>(body).map_err(|_| bad)?;
        id.to_owned()
    };
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (allocation, session) = authenticated_allocation(g, headers, true).await?;
    let upstream = g
        .proxy(
            &allocation,
            Method::POST,
            uri.path(),
            Some(body),
            Some(&session),
        )
        .await;
    recheck(g, &allocation).await?;
    let (status, bytes, _) = upstream?;
    if status != StatusCode::OK {
        return api_error(status, &bytes);
    }
    let receipt: WorkReceipt =
        serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if receipt.workflow_id != id {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(response(
        status,
        serde_json::to_vec(&receipt).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
    ))
}

/// Only the gateway owns the registry. The member never receives installation
/// credentials, and the installation timer never receives search terms or results.
pub(super) async fn timer(g: Gateway) {
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut cursor = String::new();
    loop {
        interval.tick().await;
        let root = g.registry.clone();
        let after = cursor.clone();
        let Ok(Ok(ids)) = tokio::task::spawn_blocking(move || {
            runtime_registry::scheduled_member_page(&root, after)
        })
        .await
        else {
            continue;
        };
        cursor = if ids.len() == 16 {
            ids.last().unwrap().to_string()
        } else {
            String::new()
        };
        for batch in ids.chunks(4) {
            let mut active = tokio::task::JoinSet::new();
            for &id in batch {
                let g = g.clone();
                active.spawn(async move {
                    let Ok(route) =
                        resolve(&g, runtime_registry::SessionDestination::Account(id)).await
                    else {
                        return;
                    };
                    let pin = moosuri_core::personal_browser::WorkTick {
                        tenant: route.tenant_id.clone(),
                        volume: route.volume_id.clone(),
                        generation: route.generation as u64,
                        epoch: route.epoch,
                    };
                    if let Ok(body) = serde_json::to_vec(&pin) {
                        let _ = g
                            .proxy(
                                &route,
                                Method::POST,
                                "/internal/member-work/tick",
                                Some(&body),
                                None,
                            )
                            .await;
                    }
                });
            }
            while active.join_next().await.is_some() {}
        }
    }
}
