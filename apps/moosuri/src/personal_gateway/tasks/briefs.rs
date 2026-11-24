//! Gateway side of `/api/v1/personal/tasks/briefs`: strict request shape, authenticated allocation,
//! exact reconstruction of the public DTO. The gateway never sees Google tokens; it only forwards the
//! member's explicit grant/stop and returns counts, states and the minimal evidence the member's own
//! runtime already decided is still live.
use super::*;
use moosuri_store::{
    MEMBER_BRIEF_MAX_EVENTS, MEMBER_BRIEF_MAX_MESSAGES, MemberBriefPage, MemberBriefRequest,
    MemberBriefStop, MemberBriefView,
};

pub(super) const BASE: &str = "/api/v1/personal/tasks/briefs";

fn text(s: &str) -> bool {
    s.chars().count() <= 512 && !s.chars().any(char::is_control)
}
/// Public DTO invariants: read-only, bounded, no unknown sources/states, evidence only when completed.
fn view_valid(v: &MemberBriefView) -> bool {
    const STATES: [&str; 4] = ["active", "stopped", "expired", "revoked"];
    const RUNS: [&str; 8] = [
        "claimed",
        "completed",
        "partial",
        "unavailable",
        "needs_connection",
        "missed",
        "unknown",
        "withdrawn",
    ];
    v.read_only
        && STATES.contains(&v.state.as_str())
        && !v.sources.is_empty()
        && v.sources.iter().all(|s| s == "gmail" || s == "calendar")
        && v.runs.len() <= 30
        && v.runs.iter().all(|r| {
            RUNS.contains(&r.state.as_str())
                && r.freshness
                    .as_deref()
                    .is_none_or(|f| f == "live" || f == "projection")
                && r.detail.as_ref().is_none_or(|d| {
                    matches!(r.state.as_str(), "completed" | "partial")
                        && d.messages.as_ref().is_none_or(|m| {
                            m.len() <= MEMBER_BRIEF_MAX_MESSAGES
                                && m.iter().all(|m| text(&m.subject) && text(&m.from))
                        })
                        && d.events.as_ref().is_none_or(|e| {
                            e.len() <= MEMBER_BRIEF_MAX_EVENTS
                                && e.iter()
                                    .all(|e| text(&e.title) && text(&e.start) && text(&e.end))
                        })
                })
        })
}

pub(super) async fn forward(
    g: &Gateway,
    headers: &HeaderMap,
    method: Method,
    uri: &Uri,
    body: &[u8],
) -> Result<Response, StatusCode> {
    let bad = StatusCode::BAD_REQUEST;
    let thread = match uri.query() {
        None => None,
        Some(query) if method == Method::GET && uri.path() == BASE => {
            Some(task_uuid(query.strip_prefix("thread_id=").ok_or(bad)?)?)
        }
        Some(_) => return Err(bad),
    };
    if (method == Method::GET && !body.is_empty()) || body.len() > 16 * 1024 {
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
            let input: MemberBriefRequest = serde_json::from_slice(body).map_err(|_| bad)?;
            task_uuid(&input.thread_id)?;
            Some(task_uuid(&input.request_id)?)
        }
        ("GET", s) => Some(task_uuid(s.strip_prefix('/').ok_or(bad)?)?),
        ("POST", s) => {
            let stop: MemberBriefStop = serde_json::from_slice(body).map_err(|_| bad)?;
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
        .proxy(
            &allocation,
            method,
            uri.path_and_query().ok_or(bad)?.as_str(),
            payload,
            Some(&session),
        )
        .await;
    recheck(g, &allocation).await?;
    let (status, bytes, _) = upstream?;
    if status != StatusCode::OK {
        return api_error(status, &bytes);
    }
    let invalid = |_| StatusCode::SERVICE_UNAVAILABLE;
    let value = if let Some(id) = id {
        let result: MemberBriefView = serde_json::from_slice(&bytes).map_err(invalid)?;
        if result.id != id.to_string() || !view_valid(&result) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&result)
    } else {
        let page: MemberBriefPage = serde_json::from_slice(&bytes).map_err(invalid)?;
        if page.items.len() > 30
            || !page
                .items
                .iter()
                .all(|v| view_valid(v) && thread.is_none_or(|id| v.thread_id == id.to_string()))
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&page)
    }
    .map_err(invalid)?;
    Ok(response(status, value))
}
