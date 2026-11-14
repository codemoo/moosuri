//! Admission and public observations for public HTTP research. No network IO.
use super::*;
use moosuri_core::{WebEvidence, WebEvidenceKind, WebToolProfile, WebToolRequest};

pub(crate) fn admitted_profile(
    conn: &Connection,
    user: &UserContext,
    current: &WebToolProfile,
) -> Result<moosuri_core::RunToolProfile> {
    let mut profile = agent_execution::skills::profile();
    profile.credential_github = cfg!(feature = "credential-broker-chat");
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
        WebToolRequest::Parcel { .. } => {
            pin.read
                && current.read
                && crate::agent_execution::skills::current_skill(profile, "parcel-tracking")
        }
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
                request: moosuri_core::ExternalToolRequest::Web(WebToolRequest::Parcel { .. }),
            } => {
                if let Some(page) = value.pointer("/parcel/source")
                    && let Some(time) = page.get("retrieved_at_unix_ms").and_then(|v| v.as_u64())
                    && let Some(item) = observation(page, WebEvidenceKind::Page, time, false)
                {
                    evidence.push(item);
                }
            }
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

#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct ParcelOrigin {
    current_request_id: String,
    number_source_request_id: String,
    number_source_run_id: String,
    carrier: String,
    tracking_number: String,
    used_previous_number: bool,
}
fn current_numbers(text: &str) -> std::collections::BTreeSet<String> {
    text.split(|c: char| !c.is_ascii_alphanumeric() && c != '-')
        .filter_map(|token| moosuri_connectors::parcel::normalize(token).ok())
        .collect()
}
fn explicit_carrier(text: &str) -> bool {
    let compact = text.to_lowercase().split_whitespace().collect::<String>();
    [
        "japanpost",
        "일본우편",
        "일본우정",
        "재팬포스트",
        "日本郵便",
    ]
    .iter()
    .any(|v| compact.contains(v))
}
/// Materialize only a literal current user number or the latest unique actual
/// typed lookup from this owned thread. No model/source ID is accepted.
pub(crate) fn parcel_origin(
    c: &Connection,
    r: &crate::chat::Run,
    request: &mut moosuri_core::ExternalToolRequest,
) -> Result<Option<ParcelOrigin>> {
    let moosuri_core::ExternalToolRequest::Web(WebToolRequest::Parcel {
        carrier,
        tracking_number,
    }) = request
    else {
        return Ok(None);
    };
    if carrier != "japan_post" {
        return Err(StoreError::InvalidInput);
    }
    let requested = tracking_number
        .as_deref()
        .map(moosuri_connectors::parcel::normalize)
        .transpose()
        .map_err(|_| StoreError::InvalidInput)?;
    let literals = current_numbers(&r.text);
    if !literals.is_empty() && explicit_carrier(&r.text) {
        let number = match &requested {
            Some(n) if literals.contains(n) => n.clone(),
            None if literals.len() == 1 => {
                literals.first().ok_or(StoreError::InvalidInput)?.clone()
            }
            _ => return Err(StoreError::InvalidInput),
        };
        *tracking_number = Some(number.clone());
        return Ok(Some(ParcelOrigin {
            current_request_id: r.request.clone(),
            number_source_request_id: r.request.clone(),
            number_source_run_id: r.id.clone(),
            carrier: carrier.clone(),
            tracking_number: number,
            used_previous_number: false,
        }));
    }
    if literals.len() > 1 {
        return Err(StoreError::InvalidInput);
    }
    let rows=c.prepare("SELECT old.sequence,t.result FROM chat_runs old JOIN agent_execution e ON e.tenant_id=old.tenant_id AND e.run_id=old.id JOIN agent_tool_slots t ON t.tenant_id=old.tenant_id AND t.run_id=old.id JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE old.tenant_id=?1 AND old.account_id=?2 AND old.thread_id=?3 AND old.sequence<?4 AND old.sequence>=?4-32 AND old.state='completed' AND e.invalidated=0 AND d.state='completed' AND json_extract(t.input,'$.request.adapter')='web' AND json_extract(t.input,'$.request.request.operation')='parcel' AND t.result IS NOT NULL ORDER BY old.sequence DESC,t.step DESC,t.call_id LIMIT 12")?.query_map(params![r.tenant,r.account,r.thread,r.sequence],|row|Ok((row.get::<_,i64>(0)?,row.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let latest = rows.first().map(|row| row.0);
    let mut candidates = std::collections::BTreeMap::new();
    for (sequence, raw) in rows {
        if Some(sequence) != latest {
            break;
        }
        let result: crate::KnowledgeToolResult =
            serde_json::from_str(&raw).map_err(|_| StoreError::Corrupt)?;
        if let crate::KnowledgeToolResult::External(v) = result
            && let Some(v) = v.pointer("/parcel/provenance")
        {
            let p: ParcelOrigin =
                serde_json::from_value(v.clone()).map_err(|_| StoreError::Corrupt)?;
            if p.carrier == "japan_post" {
                candidates.insert(p.tracking_number.clone(), p);
            }
        }
    }
    if candidates.len() != 1 {
        return Err(StoreError::InvalidInput);
    }
    let (_, mut origin) = candidates
        .into_iter()
        .next()
        .ok_or(StoreError::InvalidInput)?;
    if literals.iter().any(|n| n != &origin.tracking_number) {
        return Err(StoreError::InvalidInput);
    }
    if requested
        .as_ref()
        .is_some_and(|n| n != &origin.tracking_number)
    {
        return Err(StoreError::InvalidInput);
    }
    origin.current_request_id = r.request.clone();
    origin.used_previous_number = true;
    *tracking_number = Some(origin.tracking_number.clone());
    Ok(Some(origin))
}
pub(crate) fn complete_parcel(
    origin: &ParcelOrigin,
    result: &mut crate::KnowledgeToolResult,
) -> Result<()> {
    let crate::KnowledgeToolResult::External(value) = result else {
        return Ok(());
    };
    let p = value
        .get_mut("parcel")
        .and_then(|v| v.as_object_mut())
        .ok_or(StoreError::Corrupt)?;
    let expected = moosuri_connectors::parcel::url(&origin.tracking_number)
        .map_err(|_| StoreError::Corrupt)?
        .to_string();
    if p.get("carrier").and_then(|v| v.as_str()) != Some("japan_post")
        || p.get("tracking_number").and_then(|v| v.as_str())
            != Some(origin.tracking_number.as_str())
        || p.get("source")
            .and_then(|v| v.get("url"))
            .and_then(|v| v.as_str())
            != Some(expected.as_str())
    {
        return Err(StoreError::Corrupt);
    }
    p.insert(
        "provenance".into(),
        serde_json::to_value(origin).map_err(|_| StoreError::Corrupt)?,
    );
    Ok(())
}
