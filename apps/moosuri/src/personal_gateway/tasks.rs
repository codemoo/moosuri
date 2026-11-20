use super::*;
use moosuri_core::personal_browser::{
    TaskDetail, TaskPage, WorkAction, WorkReceipt, WorkRequest, task_cursor,
};

const BASE: &str = "/api/v1/personal/tasks";
const WORKFLOWS: &str = "/api/v1/personal/tasks/workflows";
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(BASE, get(handler))
        .route(WORKFLOWS, axum::routing::post(handler))
        .route(
            "/api/v1/personal/tasks/workflows/{id}",
            get(handler).post(handler),
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
    let mut path = uri.path().to_owned();
    let (id, schedule) = if path == BASE {
        (None, None)
    } else if let Some(raw) = path.strip_prefix("/api/v1/personal/tasks/workflows/") {
        (None, Some(task_uuid(raw)?))
    } else {
        let raw = path
            .strip_prefix("/api/v1/personal/tasks/")
            .ok_or(StatusCode::NOT_FOUND)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        (Some(task_uuid(raw)?), None)
    };
    if let Some(query) = uri.query() {
        query
            .strip_prefix("before=")
            .filter(|v| task_cursor(v).is_some())
            .ok_or(StatusCode::BAD_REQUEST)?;
        path.push('?');
        path.push_str(query);
    }
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
        let page: TaskPage = serde_json::from_slice(&bytes).map_err(invalid)?;
        if let Some(schedule) = schedule {
            let expected = schedule.to_string();
            if !page.schedules.is_empty()
                || page
                    .items
                    .iter()
                    .any(|row| row.schedule_id.as_deref() != Some(expected.as_str()))
            {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        serde_json::to_vec(&page)
    }
    .map_err(invalid)?;
    Ok(response(status, value))
}

fn task_uuid(raw: &str) -> Result<Uuid, StatusCode> {
    Uuid::parse_str(raw)
        .ok()
        .filter(|v| !v.is_nil() && v.to_string() == raw)
        .ok_or(StatusCode::BAD_REQUEST)
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
    let id = if uri.path() == WORKFLOWS {
        let q: WorkRequest = serde_json::from_slice(body).map_err(|_| bad)?;
        q.request_id
    } else {
        let id = uri
            .path()
            .strip_prefix("/api/v1/personal/tasks/workflows/")
            .ok_or(bad)?;
        task_uuid(id)?;
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
