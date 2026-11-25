//! Gateway side of `/api/v1/personal/tasks/public-watches`: strict request shape, authenticated
//! allocation, exact reconstruction of the public DTO. The gateway never reads the page and never sees
//! the browser broker; it only forwards the member's explicit grant/stop and returns what the member's own
//! runtime recorded. Access is session + host epoch (`authenticated_allocation`), like every member task.
//!
//! * `GET  /api/v1/personal/tasks/public-watches?thread_id=UUID` -> `{items:[PublicWatchView]}`
//! * `POST /api/v1/personal/tasks/public-watches` -> `PublicWatchView`
//! * `POST /api/v1/personal/tasks/public-watches/{id}/stop` -> `PublicWatchView`
use super::*;
use moosuri_store::personal_browser::{
    PublicWatchPage, PublicWatchRequest, PublicWatchStop, PublicWatchView,
};

pub(super) const BASE: &str = "/api/v1/personal/tasks/public-watches";
const LIST_LIMIT: usize = 30;
const CHECK_LIMIT: usize = 10;

fn text(s: &str, limit: usize) -> bool {
    s.chars().count() <= limit && !s.chars().any(char::is_control)
}

/// Public DTO invariants: bounded, known states only, excerpts only where the store can produce them.
fn view_valid(v: &PublicWatchView) -> bool {
    const STATES: [&str; 4] = ["active", "stopped", "expired", "revoked"];
    const CHECKS: [&str; 7] = [
        "claimed",
        "baseline",
        "unchanged",
        "changed",
        "failed",
        "unknown",
        "withdrawn",
    ];
    task_uuid(&v.id).is_ok()
        && task_uuid(&v.thread_id).is_ok()
        && v.url.starts_with("https://")
        && text(&v.url, 2048)
        && matches!(v.every_seconds, 1800 | 3600 | 86400)
        && (1..=90).contains(&v.days)
        && v.price_condition.is_none_or(|p| p.valid())
        && v.revision > 0
        && STATES.contains(&v.state.as_str())
        && v.checks.len() <= CHECK_LIMIT
        && v.checks.iter().all(|c| {
            CHECKS.contains(&c.state.as_str())
                && c.excerpt.as_ref().is_none_or(|e| {
                    matches!(c.state.as_str(), "baseline" | "changed") && text(e, 256)
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
    // The only query is the list filter, and the list requires it.
    let thread = match uri.query() {
        Some(query) if method == Method::GET && uri.path() == BASE => {
            Some(task_uuid(query.strip_prefix("thread_id=").ok_or(bad)?)?)
        }
        None if method == Method::GET && uri.path() == BASE => return Err(bad),
        None => None,
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
    // (watch id, original request for a create)
    let (id, created) = match (method.as_str(), suffix) {
        ("GET", "") => (None, None),
        ("POST", "") => {
            let input: PublicWatchRequest = serde_json::from_slice(body).map_err(|_| bad)?;
            task_uuid(&input.thread_id)?;
            (Some(task_uuid(&input.request_id)?), Some(input))
        }
        ("POST", s) => {
            let stop: PublicWatchStop = serde_json::from_slice(body).map_err(|_| bad)?;
            task_uuid(&stop.request_id)?;
            (
                Some(task_uuid(
                    s.strip_prefix('/')
                        .and_then(|s| s.strip_suffix("/stop"))
                        .ok_or(bad)?,
                )?),
                None,
            )
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
        let result: PublicWatchView = serde_json::from_slice(&bytes).map_err(invalid)?;
        if result.id != id.to_string()
            || !view_valid(&result)
            || created.as_ref().is_some_and(|r| {
                result.thread_id != r.thread_id
                    || result.every_seconds != r.every_seconds
                    || result.days != r.days
                    || result.price_condition != r.price_condition
            })
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        serde_json::to_vec(&result)
    } else {
        let page: PublicWatchPage = serde_json::from_slice(&bytes).map_err(invalid)?;
        if page.items.len() > LIST_LIMIT
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

#[cfg(test)]
mod tests {
    use super::*;
    use moosuri_store::personal_browser::PublicWatchCheck;

    fn view(state: &str, check: &str, excerpt: Option<&str>) -> PublicWatchView {
        PublicWatchView {
            id: Uuid::new_v4().to_string(),
            thread_id: Uuid::new_v4().to_string(),
            url: "https://status.example.test/service".into(),
            every_seconds: 3600,
            days: 7,
            price_condition: None,
            state: state.into(),
            revision: 1,
            checks: vec![PublicWatchCheck {
                due_at: 1,
                state: check.into(),
                excerpt: excerpt.map(str::to_owned),
            }],
        }
    }

    #[test]
    fn gateway_accepts_only_the_exact_bounded_public_dto() {
        assert!(view_valid(&view("active", "changed", Some("Major outage"))));
        assert!(view_valid(&view("revoked", "unchanged", None)));
        // Unknown states, excerpts on checks that cannot carry one, control characters and oversize
        // text, a non-https address and an off-menu cadence are all refused rather than relayed.
        assert!(!view_valid(&view("paused", "changed", None)));
        assert!(!view_valid(&view("active", "polling", None)));
        assert!(!view_valid(&view("active", "failed", Some("leaked"))));
        assert!(!view_valid(&view("active", "changed", Some("a\u{7}b"))));
        assert!(!view_valid(&view(
            "active",
            "changed",
            Some(&"x".repeat(257))
        )));
        let mut v = view("active", "changed", None);
        v.url = "http://status.example.test/service".into();
        assert!(!view_valid(&v));
        let mut v = view("active", "changed", None);
        v.every_seconds = 60;
        assert!(!view_valid(&v));
        let mut v = view("active", "changed", None);
        v.id = "not-a-uuid".into();
        assert!(!view_valid(&v));
        let mut v = view("active", "changed", None);
        v.checks = (0..11)
            .map(|i| PublicWatchCheck {
                due_at: i,
                state: "unchanged".into(),
                excerpt: None,
            })
            .collect();
        assert!(!view_valid(&v));
    }
}
