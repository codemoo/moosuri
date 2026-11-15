//! Exact installation eligibility and bounded collection of unreferenced public bytes.
use super::*;
pub(super) fn eligible(c: &Connection, tenant: &str, pin: &SkillPin) -> Result<bool> {
    if pin.id.starts_with("community:") {
        let entry = match stored(c, &pin.digest) {
            Ok(v) => v,
            Err(StoreError::NotFound) => return Ok(false),
            Err(e) => return Err(e),
        };
        if entry.pin() != *pin {
            return Ok(false);
        }
        // Updating never rewrites an existing recipe: the retained previous pin
        // remains eligible while the installation is enabled. A third version
        // that drops that pointer requires exact recovery or a new schedule.
        return Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM community_installs WHERE tenant_id=?1 AND package_id=?2 AND managed=1 AND enabled=1 AND (active_digest=?3 OR previous_digest=?3))",params![tenant,entry.id(),pin.digest],|r|r.get(0))?);
    }
    if pin.id.starts_with("private:") {
        let account: Option<String> = c
            .query_row(
                "SELECT id FROM accounts WHERE tenant_id=?1",
                [tenant],
                |row| row.get(0),
            )
            .optional()?;
        return match account {
            Some(account) => crate::learned_skills::mission_eligible(c, tenant, &account, pin),
            None => Ok(false),
        };
    }
    if !crate::agent_execution::skills::profile()
        .skills
        .contains(pin)
    {
        return Ok(false);
    }
    let identity = format!("{SOURCE}#community/skills/{}", pin.id);
    Ok(!c.query_row("SELECT EXISTS(SELECT 1 FROM community_installs WHERE tenant_id=?1 AND package_id=?2 AND managed=1)",params![tenant,identity],|r|r.get::<_,bool>(0))?)
}
/// An uninstalled preview is a cache, not durable execution authority. A cache
/// miss requires exact re-preview; immutable identity is never substituted.
pub(super) fn collect(c: &Connection, keep: Option<&str>) -> Result<usize> {
    Ok(c.execute("DELETE FROM community_packages WHERE digest IN(SELECT p.digest FROM community_packages p WHERE (?1 IS NULL OR p.digest!=?1) AND NOT EXISTS(SELECT 1 FROM community_installs i WHERE i.active_digest=p.digest OR i.previous_digest=p.digest) AND NOT EXISTS(SELECT 1 FROM chat_runs r WHERE instr(r.model,p.digest)>0) AND NOT EXISTS(SELECT 1 FROM recurring_missions r WHERE instr(r.model,p.digest)>0) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots s WHERE instr(s.input,p.digest)>0 OR instr(s.result,p.digest)>0) AND NOT EXISTS(SELECT 1 FROM agent_steps s WHERE instr(CAST(s.checkpoint AS TEXT),p.digest)>0) AND NOT EXISTS(SELECT 1 FROM community_catalog_cache x WHERE instr(x.index_json,p.digest)>0) ORDER BY p.digest LIMIT 8)",[keep])?)
}
