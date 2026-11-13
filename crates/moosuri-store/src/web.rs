//! Admission and public observations for public HTTP research. No network IO.
use super::*;
use moosuri_core::{WebEvidence, WebEvidenceKind, WebToolProfile, WebToolRequest};

pub(crate) fn admitted_profile(
    conn: &Connection,
    user: &UserContext,
    current: &WebToolProfile,
) -> Result<moosuri_core::RunToolProfile> {
    let mut profile = agent_execution::skills::profile();
    profile.mail_connections = crate::mail::pins(conn, user)?;
    if !profile.mail_connections.is_empty() {
        profile.version = 2;
    }
    if current.read || current.search {
        profile.version = 3;
        profile.web = Some(current.clone());
    }
    if current.browser_pin.is_some() {
        profile.version = 5;
        profile.web = Some(current.clone());
    }
    crate::community_skills::admit(conn, user, &mut profile)?;
    Ok(profile)
}

pub(crate) fn allowed(
    profile: Option<&moosuri_core::RunToolProfile>,
    current: &WebToolProfile,
    request: &WebToolRequest,
) -> bool {
    let Some(pin) = profile
        .filter(|p| matches!(p.version, 3..=5))
        .and_then(|p| p.web.as_ref())
    else {
        return false;
    };
    match request {
        WebToolRequest::Read { .. } => pin.read && current.read,
        WebToolRequest::Search { .. } => pin.search && current.search,
        WebToolRequest::BrowserOpen { .. }
        | WebToolRequest::BrowserObserve {}
        | WebToolRequest::BrowserHandoff { .. } => {
            profile.is_some_and(|p| p.version == 5)
                && pin
                    .browser_pin
                    .as_ref()
                    .is_some_and(|pin| Some(pin) == current.browser_pin.as_ref())
        }
        WebToolRequest::BrowserRead { .. } => {
            profile.is_some_and(|p| matches!(p.version, 4 | 5))
                && pin
                    .browser_pin
                    .as_ref()
                    .is_some_and(|pin| Some(pin) == current.browser_pin.as_ref())
        }
    }
}

fn observation(
    value: &serde_json::Value,
    kind: WebEvidenceKind,
    time: u64,
    truncated: bool,
) -> Option<WebEvidence> {
    let raw = value.get("url")?.as_str()?;
    let url = url::Url::parse(raw).ok()?;
    if raw.len() > 2048
        || !matches!(url.scheme(), "http" | "https")
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
    {
        return None;
    }
    let title = value.get("title")?.as_str()?;
    if title.len() > 512 {
        return None;
    }
    Some(WebEvidence {
        url: url.into(),
        title: title.into(),
        retrieved_at_unix_ms: time,
        kind,
        truncated,
    })
}

/// Derive only from typed slots completed by the trusted adapter for this run.
/// Prior-turn context may have other observations; this list makes no claim about it.
pub(crate) fn evidence(conn: &Connection, tenant: &str, run: &str) -> Result<Vec<WebEvidence>> {
    let values=conn.prepare("SELECT t.input,t.result FROM agent_tool_slots t JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.result IS NOT NULL AND d.state='completed' ORDER BY t.step,t.call_id LIMIT 12")?.query_map(params![tenant,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut evidence = Vec::new();
    for (input, result) in values {
        let input: agent_execution::KnowledgeToolInput =
            serde_json::from_str(&input).map_err(|_| StoreError::Corrupt)?;
        let result: agent_execution::KnowledgeToolResult =
            serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
        let agent_execution::KnowledgeToolResult::External(value) = result else {
            continue;
        };
        match input {
            agent_execution::KnowledgeToolInput::External {
                request: moosuri_core::ExternalToolRequest::Web(WebToolRequest::Read { .. }),
            } => {
                if let Some(page) = value.get("web_page")
                    && let (Some(time), Some(truncated)) = (
                        page.get("retrieved_at_unix_ms").and_then(|v| v.as_u64()),
                        page.get("truncated").and_then(|v| v.as_bool()),
                    )
                    && let Some(item) = observation(page, WebEvidenceKind::Page, time, truncated)
                {
                    evidence.push(item);
                }
            }
            agent_execution::KnowledgeToolInput::External {
                request: moosuri_core::ExternalToolRequest::Web(WebToolRequest::Search { .. }),
            } => {
                if let Some(search) = value.get("web_search")
                    && let (Some(time), Some(truncated), Some(hits)) = (
                        search.get("retrieved_at_unix_ms").and_then(|v| v.as_u64()),
                        search.get("truncated").and_then(|v| v.as_bool()),
                        search.get("hits").and_then(|v| v.as_array()),
                    )
                {
                    for hit in hits.iter().take(5) {
                        if let Some(item) =
                            observation(hit, WebEvidenceKind::SearchHit, time, truncated)
                        {
                            evidence.push(item);
                        }
                    }
                }
            }
            agent_execution::KnowledgeToolInput::External {
                request:
                    moosuri_core::ExternalToolRequest::Web(
                        WebToolRequest::BrowserRead { .. }
                        | WebToolRequest::BrowserOpen { .. }
                        | WebToolRequest::BrowserObserve {},
                    ),
            } => {
                if let Some(page) = value.get("browser_page")
                    && let (Some(time), Some(truncated)) = (
                        page.get("retrieved_at_unix_ms").and_then(|v| v.as_u64()),
                        page.get("truncated").and_then(|v| v.as_bool()),
                    )
                    && let Some(item) =
                        observation(page, WebEvidenceKind::BrowserPage, time, truncated)
                {
                    evidence.push(item);
                }
            }
            _ => {}
        }
    }
    Ok(evidence)
}

#[cfg(test)]
mod tests;
