use super::{task_uuid, *};
use moosuri_core::personal_browser::{
    ConversationActionKind, ConversationReport, ConversationWorkAction, ConversationWorkGroup,
    ConversationWorkPage, ConversationWorkRequest,
};

pub(super) const PREFIX: &str = "/api/v1/personal/tasks/conversations/";

enum Expected {
    Page(Option<String>),
    Group {
        request: Option<String>,
        child: Option<String>,
    },
    Report(String),
}

pub(super) async fn forward(
    g: &Gateway,
    headers: &HeaderMap,
    method: Method,
    uri: &Uri,
    body: &[u8],
) -> Result<Response, StatusCode> {
    let bad = StatusCode::BAD_REQUEST;
    let tail = uri.path().strip_prefix(PREFIX).ok_or(bad)?;
    let (thread, resource) = tail.split_once('/').unwrap_or((tail, ""));
    task_uuid(thread)?;
    if method == Method::POST
        && (uri.query().is_some()
            || body.len() > 16 * 1024
            || one(headers, "content-type").and_then(|v| v.split(';').next())
                != Some("application/json"))
    {
        return Err(bad);
    }
    if method == Method::GET && !body.is_empty() {
        return Err(bad);
    }
    let expected = match (method.as_str(), resource) {
        ("GET", "") => {
            let filter = uri
                .query()
                .map(|q| {
                    let id = q.strip_prefix("request_id=").ok_or(bad)?;
                    task_uuid(id)?;
                    Ok::<_, StatusCode>(id.to_owned())
                })
                .transpose()?;
            Expected::Page(filter)
        }
        ("POST", "") => {
            let item: ConversationWorkRequest = serde_json::from_slice(body).map_err(|_| bad)?;
            task_uuid(&item.request_id)?;
            Expected::Group {
                request: Some(item.request_id),
                child: None,
            }
        }
        ("POST", "control") => {
            let action: ConversationWorkAction = serde_json::from_slice(body).map_err(|_| bad)?;
            task_uuid(&action.task_id)?;
            task_uuid(&action.request_id)?;
            if action.action == ConversationActionKind::Reuse {
                Expected::Group {
                    request: Some(action.request_id),
                    child: None,
                }
            } else {
                Expected::Group {
                    request: None,
                    child: Some(action.task_id),
                }
            }
        }
        ("GET", path) if uri.query().is_none() && path.starts_with("reports/") => {
            let id = task_uuid(&path["reports/".len()..])?;
            Expected::Report(id.to_string())
        }
        _ => return Err(bad),
    };
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let (allocation, session) = authenticated_allocation(g, headers, true).await?;
    let path = uri.path_and_query().ok_or(bad)?.as_str();
    let payload = (method == Method::POST).then_some(body);
    let upstream = g
        .proxy(&allocation, method, path, payload, Some(&session))
        .await;
    recheck(g, &allocation).await?;
    let (status, bytes, _) = upstream?;
    if status != StatusCode::OK {
        return api_error(status, &bytes);
    }
    let invalid = |_| StatusCode::SERVICE_UNAVAILABLE;
    let value = match expected {
        Expected::Page(filter) => {
            let page: ConversationWorkPage = serde_json::from_slice(&bytes).map_err(invalid)?;
            if page.thread_id != thread
                || page.groups.iter().any(|group| {
                    group.thread_id != thread || filter.as_ref().is_some_and(|id| id != &group.id)
                })
            {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::to_vec(&page)
        }
        Expected::Group { request, child } => {
            let group: ConversationWorkGroup = serde_json::from_slice(&bytes).map_err(invalid)?;
            if group.thread_id != thread
                || request.is_some_and(|id| group.id != id)
                || child.is_some_and(|id| !group.tasks.iter().any(|task| task.id == id))
            {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::to_vec(&group)
        }
        Expected::Report(id) => {
            let report: ConversationReport = serde_json::from_slice(&bytes).map_err(invalid)?;
            if report.id != id {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::to_vec(&report)
        }
    }
    .map_err(invalid)?;
    Ok(response(status, value))
}
