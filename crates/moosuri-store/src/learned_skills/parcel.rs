//! One structural Japan Post procedure. Training numbers/status live only in
//! existing private typed receipts, never in route/discovery/export metadata.
use super::*;
use crate::ToolReadError;
use moosuri_core::{ExternalToolRequest, WebToolRequest};

pub(crate) fn request(
    c: &Connection,
    r: &chat::Run,
    invocation: &moosuri_core::LearnedSkillInvocation,
) -> Result<std::result::Result<ExternalToolRequest, ToolReadError>> {
    if crate::missions::is_mission(c, &r.tenant, &r.id)?
        || !r
            .tool_profile
            .as_ref()
            .is_some_and(|p| matches!(p.version, 3..=5) && p.skills.contains(&invocation.pin))
        || available(c, &r.tenant, &r.account, &invocation.pin)?.as_deref() != Some(PARCEL)
    {
        return Ok(Err(ToolReadError::Unavailable));
    }
    c.execute(
        "INSERT OR IGNORE INTO learned_skill_uses VALUES(?1,?2,?3)",
        params![r.tenant, r.id, invocation.pin.id],
    )?;
    let LearnedSkillArguments::Parcel { tracking_number } = &invocation.arguments else {
        return Ok(Err(ToolReadError::InvalidInput));
    };
    let literals = crate::web::current_numbers(&r.text);
    let number = moosuri_connectors::parcel::normalize(tracking_number).ok();
    if !crate::web::explicit_carrier(&r.text)
        || literals.len() != 1
        || number.as_ref() != literals.first()
    {
        // Missing/current authority is not a learned procedure failure. A new
        // explicit request may supply the number without quarantining the route.
        return Ok(Err(ToolReadError::InvalidInput));
    }
    Ok(Ok(ExternalToolRequest::Web(WebToolRequest::Parcel {
        carrier: "japan_post".into(),
        tracking_number: number,
    })))
}
/// Called only after exact dispatch nonce/saved-result checks in the same writer
/// transaction that settles the parent external slot. Replays never append twice.
pub(crate) fn complete(
    c: &Connection,
    r: &chat::Run,
    pin: &SkillPin,
    step: i64,
    call: &str,
    request: &ExternalToolRequest,
    result: &Output,
) -> Result<()> {
    if available(c, &r.tenant, &r.account, pin)?.as_deref() != Some(PARCEL) {
        return Err(StoreError::Conflict);
    }
    c.execute(
        "INSERT INTO learned_skill_receipts VALUES(?1,?2,?3,?4,0,?5,?6)",
        params![
            r.tenant,
            r.id,
            step,
            call,
            encode(&Input::External {
                request: request.clone()
            })?,
            encode(result)?
        ],
    )?;
    // Provider failures/absence/layout changes are not route defects.
    Ok(())
}
/// A successful tool flag is insufficient: require an actual completed external
/// dispatch, exact server-recorded current-number provenance, complete supported
/// observation and adoption of the number/date/status/source in finalized output.
pub(super) fn proof(
    c: &Connection,
    tenant: &str,
    account: &str,
    run_id: &str,
    value: &serde_json::Value,
    receipt: &str,
) -> Result<Option<Proof>> {
    let r = crate::chat::load_run(c, tenant, run_id)?;
    if r.account != account {
        return Ok(None);
    }
    let Some(p) = value.get("parcel") else {
        return Ok(None);
    };
    let number = p
        .get("tracking_number")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let normalized = moosuri_connectors::parcel::normalize(number).ok();
    let literals = crate::web::current_numbers(&r.text);
    if normalized.as_deref() != Some(number)
        || !crate::web::explicit_carrier(&r.text)
        || literals.len() != 1
        || literals.first().map(String::as_str) != Some(number)
        || p["carrier"] != "japan_post"
        || p["state"] != "observed"
        || p["coverage"] != "latest_summary_only"
    {
        return Ok(None);
    }
    let provenance = &p["provenance"];
    if provenance["current_request_id"] != r.request
        || provenance["number_source_request_id"] != r.request
        || provenance["number_source_run_id"] != r.id
        || provenance["tracking_number"] != number
        || provenance["carrier"] != "japan_post"
        || provenance["used_previous_number"] != false
    {
        return Ok(None);
    }
    let source = &p["source"];
    let url = moosuri_connectors::parcel::url(number)
        .map_err(|_| StoreError::Corrupt)?
        .to_string();
    let source_url = source["url"].as_str().unwrap_or("");
    if source_url != url
        || source["truncated"] != false
        || source["retrieved_at_unix_ms"]
            .as_u64()
            .is_none_or(|v| v == 0)
    {
        return Ok(None);
    }
    let Some(events) = p["events"].as_array().filter(|v| v.len() == 1) else {
        return Ok(None);
    };
    let event = &events[0];
    let Some(date) = event["reported_time_text"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 512)
    else {
        return Ok(None);
    };
    let Some(status) = event["status_text"]
        .as_str()
        .filter(|v| !v.is_empty() && v.len() <= 512 && !v.starts_with("**"))
    else {
        return Ok(None);
    };
    let excerpt = source["visible_excerpt"].as_str().unwrap_or("");
    if ![number, date, status].iter().all(|v| excerpt.contains(v)) {
        return Ok(None);
    }
    let raw = encode(&Output::External(value.clone()))?;
    let completed:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM agent_tool_slots t JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.result=?3 AND d.state='completed')",params![tenant,run_id,raw],|row|row.get(0))?;
    if !completed {
        return Ok(None);
    }
    let terminal: Option<i64> = c.query_row(
        "SELECT max(step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state='completed'",
        params![tenant, run_id],
        |row| row.get(0),
    )?;
    let Some(terminal) = terminal else {
        return Ok(None);
    };
    // Runtime output IDs are scoped to their acknowledged model step. Earlier
    // tool-round text, even if it quoted a status, is not terminal adoption.
    let prefix = format!("step-{terminal}-%");
    let output=c.prepare("SELECT text FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item LIKE ?3 AND refusal=0 AND (phase IS NULL OR phase='final_answer') ORDER BY ordinal")?.query_map(params![tenant,run_id,prefix],|row|row.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?.join("\n");
    if !crate::web::current_numbers(&output).contains(number)
        || ![date, status, url.as_str()]
            .iter()
            .all(|v| output.contains(v))
    {
        return Ok(None);
    }
    Ok(Some(Proof {
        route: PARCEL.into(),
        input: hash(number),
        receipt: receipt.into(),
        subject: None,
    }))
}
#[cfg(test)]
mod tests;
