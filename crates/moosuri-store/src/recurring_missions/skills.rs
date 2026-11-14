//! Actual instruction reads, not advertised discovery, establish recurring dependencies.
use super::*;
const FIELD: &str = "used_skill_pins";
fn saved(c: &Connection, t: &str, id: &str) -> Result<serde_json::Value> {
    let text: String = c.query_row(
        "SELECT model FROM recurring_missions WHERE tenant_id=?1 AND schedule_id=?2",
        params![t, id],
        |r| r.get(0),
    )?;
    decode(&text)
}
fn pins(model: &serde_json::Value) -> Result<Vec<SkillPin>> {
    model
        .get(FIELD)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| StoreError::Corrupt)
        .map(|v| v.unwrap_or_default())
}
fn save(c: &Connection, t: &str, id: &str, model: &serde_json::Value) -> Result<()> {
    c.execute(
        "UPDATE recurring_missions SET model=?3 WHERE tenant_id=?1 AND schedule_id=?2",
        params![t, id, encode(model)?],
    )?;
    Ok(())
}
// Existing installations need no eager migration or discovery-wide dependency.
// Backfill only still-retained successful typed reads of their own real children.
fn ensure(c: &Connection, t: &str, id: &str) -> Result<serde_json::Value> {
    let mut model = saved(c, t, id)?;
    if model.get(FIELD).is_none() {
        let rows=c.prepare("SELECT DISTINCT json_extract(s.result,'$.value.pin') FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id JOIN agent_tool_slots s ON s.tenant_id=m.tenant_id AND s.run_id=m.run_id WHERE o.tenant_id=?1 AND o.schedule_id=?2 AND json_extract(s.result,'$.kind')='skill' LIMIT 128")?.query_map(params![t,id],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut used = Vec::<SkillPin>::new();
        for row in rows {
            let pin: SkillPin = decode(&row)?;
            if !used.contains(&pin) {
                used.push(pin)
            }
        }
        model[FIELD] = serde_json::to_value(used).map_err(|_| StoreError::Corrupt)?;
        save(c, t, id, &model)?;
    }
    Ok(model)
}
fn eligible(c: &Connection, t: &str, model: &serde_json::Value) -> Result<bool> {
    for pin in pins(model)? {
        if !crate::community_skills::eligible_pin(c, t, &pin)? {
            return Ok(false);
        }
    }
    Ok(true)
}
fn stop(c: &Connection, t: &str, id: &str, time: i64) -> Result<()> {
    c.execute("UPDATE reminder_schedules SET state='paused',reason='needs_skill',revision=revision+1,updated_at=?3 WHERE tenant_id=?1 AND id=?2 AND state='active'",params![t,id,time])?;
    c.execute("UPDATE reminder_occurrences SET state='skipped',reason='needs_skill',nonce=NULL,deadline=NULL,updated_at=?3 WHERE tenant_id=?1 AND schedule_id=?2 AND state IN('queued','claimed')",params![t,id,time])?;
    fence_children(c, t, id)
}
pub(super) fn reconcile(c: &Connection, tenant: Option<&str>, time: i64) -> Result<()> {
    let ids=c.prepare("SELECT x.tenant_id,x.schedule_id FROM recurring_missions x JOIN reminder_schedules s ON s.tenant_id=x.tenant_id AND s.id=x.schedule_id WHERE (?1 IS NULL OR x.tenant_id=?1) AND s.state='active' ORDER BY x.tenant_id,x.schedule_id LIMIT 64")?.query_map([tenant],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, id) in ids {
        let model = ensure(c, &t, &id)?;
        if !eligible(c, &t, &model)? {
            stop(c, &t, &id, time)?;
        }
    }
    Ok(())
}
pub(super) fn admit_eligible(c: &Connection, t: &str, id: &str, time: i64) -> Result<bool> {
    if !eligible(c, t, &ensure(c, t, id)?)? {
        stop(c, t, id, time)?;
        return Ok(false);
    }
    Ok(true)
}
pub(super) fn resume_eligible(c: &Connection, t: &str, id: &str) -> Result<()> {
    if !eligible(c, t, &ensure(c, t, id)?)? {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
pub(super) fn run_eligible(c: &Connection, run: &crate::chat::Run) -> Result<bool> {
    let id:Option<String>=c.query_row("SELECT o.schedule_id FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2",params![run.tenant,run.id],|r|r.get(0)).optional()?;
    match id {
        None => Ok(true),
        Some(id) => eligible(c, &run.tenant, &ensure(c, &run.tenant, &id)?),
    }
}
pub(super) fn admission_model(c: &Connection, t: &str, text: &str) -> Result<serde_json::Value> {
    let mut model: serde_json::Value = decode(text)?;
    let mut profile: RunToolProfile =
        serde_json::from_value(model["agent_profile"].clone()).map_err(|_| StoreError::Corrupt)?;
    let mut allowed = Vec::new();
    for pin in profile.skills {
        if crate::community_skills::eligible_pin(c, t, &pin)? {
            allowed.push(pin)
        }
    }
    profile.skills = allowed;
    profile
        .community_metadata
        .retain(|m| profile.skills.contains(&m.pin));
    model["agent_profile"] = serde_json::to_value(profile).map_err(|_| StoreError::Corrupt)?;
    Ok(model)
}
pub(super) fn read(
    c: &Connection,
    run: &crate::chat::Run,
    pin: &SkillPin,
) -> Result<KnowledgeToolResult> {
    let parent:Option<String>=c.query_row("SELECT o.schedule_id FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2",params![run.tenant,run.id],|r|r.get(0)).optional()?;
    let Some(parent) = parent else {
        return Ok(crate::agent_execution::skills::read(c, run, pin));
    };
    if !crate::community_skills::eligible_pin(c, &run.tenant, pin)? {
        // A stored call is a concrete dependency even if a disable raced its read.
        // Preserve its exact identity for explicit recovery, never a catalog substitute.
        let available = run
            .tool_profile
            .as_ref()
            .is_some_and(|p| p.skills.contains(pin));
        if available {
            record(c, &run.tenant, &parent, pin)?;
            stop(c, &run.tenant, &parent, now()?)?;
        }
        return Ok(KnowledgeToolResult::Error(
            crate::agent_execution::ToolReadError::Unavailable,
        ));
    }
    let result = crate::agent_execution::skills::read(c, run, pin);
    if encode(&result)?.len() > crate::agent_execution::TOOL_RESULT_LIMIT {
        return Ok(KnowledgeToolResult::Error(
            crate::agent_execution::ToolReadError::TooLargeNarrowRead,
        ));
    }
    if matches!(&result,KnowledgeToolResult::Skill(doc) if doc.pin==*pin) {
        record(c, &run.tenant, &parent, pin)?;
    }
    Ok(result)
}
fn record(c: &Connection, t: &str, id: &str, pin: &SkillPin) -> Result<()> {
    let mut model = ensure(c, t, id)?;
    let mut used = pins(&model)?;
    if !used.contains(pin) {
        used.push(pin.clone());
        model[FIELD] = serde_json::to_value(used).map_err(|_| StoreError::Corrupt)?;
        save(c, t, id, &model)?;
    }
    Ok(())
}

pub(super) fn missing(c: &Connection, tenant: &str, text: &str) -> Result<Option<Vec<String>>> {
    let model: serde_json::Value = decode(text)?;
    let mut missing = Vec::new();
    for pin in pins(&model)? {
        if !crate::community_skills::eligible_pin(c, tenant, &pin)? {
            missing.push(crate::community_skills::pin_label(c, &pin)?);
        }
    }
    Ok((!missing.is_empty()).then_some(missing))
}
