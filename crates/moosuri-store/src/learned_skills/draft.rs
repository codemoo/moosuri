//! Qualified local draft preparation. The current D contract and MailAction own
//! every draft; recipe structure never carries a recipient, content or approval.
use super::*;
use moosuri_core::{
    CompoundDraftAdoption, CompoundReportRecipe, EvidenceDecimalReceipt, MissionActionRequest,
    MissionCandidate, MissionEvidence,
};
pub(super) const PREFIX: &str = "compound-draft-v1:";
pub(super) fn is_route(route: &str) -> bool {
    route.starts_with(PREFIX) && compound::recipe(route).is_some()
}
pub(super) fn route(v: &CompoundReportRecipe) -> Result<String> {
    if !v.valid() {
        return Err(StoreError::InvalidInput);
    }
    Ok(format!("{PREFIX}{}", encode(v)?))
}
/// Resolve only the exact actual source address carried by a calculation receipt.
/// This supports D citing a learned parent without fabricating a top-level read.
pub(crate) fn source_observation(
    c: &Connection,
    r: &chat::Run,
    parent: &MissionEvidence,
    b: &EvidenceDecimalReceipt,
) -> Result<serde_json::Value> {
    let address = &b.source;
    if i64::from(address.step) < crate::missions::acceptance_floor(c, &r.tenant, &r.id)?
        || i64::from(address.step) > parent.step
    {
        return Err(StoreError::InvalidInput);
    }
    let row: Option<(String, String)> = if let Some(ordinal) = address.ordinal {
        if i64::from(address.step) == parent.step && address.call_id != parent.call_id {
            return Err(StoreError::InvalidInput);
        }
        c.query_row("SELECT t.input,t.result FROM learned_skill_receipts t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND t.ordinal=?5 AND s.state='completed'",params![r.tenant,r.id,address.step,address.call_id,ordinal],|v|Ok((v.get(0)?,v.get(1)?))).optional()?
    } else {
        c.query_row("SELECT t.input,t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed' AND t.result IS NOT NULL",params![r.tenant,r.id,address.step,address.call_id],|v|Ok((v.get(0)?,v.get(1)?))).optional()?
    };
    let Some((input, result)) = row else {
        return Err(StoreError::InvalidInput);
    };
    let input: Input = serde_json::from_str(&input).map_err(|_| StoreError::Corrupt)?;
    let result: Output = serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
    let (Input::ReadSource { id, revision, .. }, Output::Source(source)) = (input, result) else {
        return Err(StoreError::InvalidInput);
    };
    if id != b.source_id
        || revision != b.source_revision
        || source.source.id != id
        || source.source.revision != revision
    {
        return Err(StoreError::InvalidInput);
    }
    let current = knowledge::sources::document(c, &r.tenant, &id, Some(revision))?;
    if current.source.revision != revision || current.text != source.text || source.has_more {
        return Err(StoreError::InvalidInput);
    }
    serde_json::to_value(source).map_err(|_| StoreError::Corrupt)
}
// Qualification concerns the already prepared immutable local draft. A normal
// send commitment preserves that revision/content; failure or unknown settlement
// explicitly withdraws proof in mail::finish_mail_dispatch. It grants no send.
type DraftProofRow = (String, i64, String, Option<String>, String, String, String);
pub(super) fn proof(c: &Connection, t: &str, a: &str, run: &str) -> Result<Option<Proof>> {
    let row:Option<DraftProofRow> = c.query_row("SELECT d.action_id,d.action_revision,d.action_digest,d.recipe_route,m.candidate,x.content,l.evidence FROM compound_draft_adoptions d JOIN missions m ON m.tenant_id=d.tenant_id AND m.account_id=d.account_id AND m.run_id=d.run_id JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id JOIN mission_action_contracts l ON l.tenant_id=d.tenant_id AND l.run_id=d.run_id AND l.account_id=d.account_id AND l.kind='mail' AND l.action_id=d.action_id AND l.action_revision=d.action_revision AND l.action_digest=d.action_digest JOIN mail_actions x ON x.tenant_id=d.tenant_id AND x.account_id=d.account_id AND x.id=d.action_id AND x.revision=d.action_revision AND x.digest=d.action_digest AND x.origin_run=d.run_id JOIN mail_connections k ON k.tenant_id=x.tenant_id AND k.account_id=x.account_id AND k.id=x.connection_id AND k.epoch=x.connection_epoch JOIN accounts owner ON owner.tenant_id=x.tenant_id AND owner.id=x.account_id AND owner.auth_epoch=x.auth_epoch WHERE d.tenant_id=?1 AND d.account_id=?2 AND d.run_id=?3 AND m.state IN('partial','achieved') AND m.uncertain=0 AND r.state IN('completed','incomplete') AND e.invalidated=0 AND x.content IS NOT NULL AND x.state IN('pending_approval','approved','dispatch_committed','sent') AND k.status='active' AND owner.status='active' AND NOT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?3 AND state!='completed')",params![t,a,run],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?,v.get(3)?,v.get(4)?,v.get(5)?,v.get(6)?))).optional()?;
    let Some((action, revision, digest, bound, candidate, content, refs)) = row else {
        return Ok(None);
    };
    if compound::derived(c, t, run)? {
        return Ok(None);
    }
    let candidate: MissionCandidate =
        serde_json::from_str(&candidate).map_err(|_| StoreError::Corrupt)?;
    let content: moosuri_core::MailDraftContent =
        serde_json::from_str(&content).map_err(|_| StoreError::Corrupt)?;
    let linked_refs: Vec<MissionEvidence> =
        serde_json::from_str(&refs).map_err(|_| StoreError::Corrupt)?;
    let mut r = chat::load_run(c, t, run)?;
    r.text = crate::missions::learned_task_text(c, &r)?;
    let floor = crate::missions::acceptance_floor(c, t, run)?;
    let rows=c.prepare("SELECT step,call_id,input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step>=?3 ORDER BY step,rowid")?.query_map(params![t,run,floor],|v|Ok((v.get::<_,i64>(0)?,v.get::<_,String>(1)?,v.get::<_,String>(2)?,v.get::<_,Option<String>>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut calls = vec![];
    let mut prepare = None;
    let mut calculation = None;
    for (step, call, input, result) in rows {
        let input: Input = serde_json::from_str(&input).map_err(|_| StoreError::Corrupt)?;
        let Some(result) = result else {
            return Ok(None);
        };
        let result: Output = serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
        if let Input::MissionAction {
            request:
                MissionActionRequest::PrepareMail {
                    input: proposal,
                    evidence,
                    ..
                },
        } = &input
        {
            if prepare.is_some()
                || encode(&proposal.content)? != encode(&content)?
                || encode(evidence)? != encode(&linked_refs)?
            {
                return Ok(None);
            }
            let Output::External(value) = &result else {
                return Ok(None);
            };
            if value["action_id"] != action
                || value["revision"] != revision
                || value["digest"] != digest
                || value["local_draft"] != true
                || value["requires_user_approval"] != true
                || value["sent"] != false
            {
                return Ok(None);
            }
            if !candidate
                .evidence
                .iter()
                .any(|e| e.step == step && e.call_id == call)
            {
                return Ok(None);
            }
            prepare = Some((step, call));
            continue;
        }
        if matches!(input, Input::EvidenceCalculate { .. }) {
            if !linked_refs
                .iter()
                .any(|e| e.step == step && e.call_id == call)
                || !candidate
                    .evidence
                    .iter()
                    .any(|e| e.step == step && e.call_id == call)
            {
                return Ok(None);
            }
            calculation = Some(step);
        }
        calls.push((input, result));
    }
    if prepare
        .as_ref()
        .is_none_or(|(step, _)| calculation.is_none_or(|calc| calc >= *step))
    {
        return Ok(None);
    }
    let receipt = hash(&encode(&(&calls, &action, revision, &digest))?);
    // The exact current draft body independently adopts source facts and number.
    let Some(mut p) = evidence::qualify(c, &r, a, &calls, &receipt, &content.text)? else {
        return Ok(None);
    };
    let Some(plan) = evidence::plan(&p.route) else {
        return Ok(None);
    };
    let Some(check) = evidence::qualify(c, &r, a, &calls, &receipt, &candidate.text)? else {
        return Ok(None);
    };
    if check.input != p.input {
        return Ok(None);
    }
    if let Some(bound) = bound {
        let Some(v) = compound::recipe(&bound).filter(|_| is_route(&bound)) else {
            return Ok(None);
        };
        if encode(&v.calculation)? != encode(&plan)? {
            return Ok(None);
        }
        p.route = bound;
    } else {
        let mut selected = CompoundReportRecipe {
            schema_version: 1,
            revision: 1,
            calculation: plan.clone(),
        };
        for existing in routes(c, t, a)? {
            if is_route(&existing)
                && let Some(v) = compound::recipe(&existing)
                && encode(&v.calculation)? == encode(&plan)?
                && v.revision > selected.revision
            {
                selected = v;
            }
        }
        p.route = route(&selected)?;
    }
    Ok(Some(p))
}
impl Store {
    pub async fn adopt_compound_draft(
        &self,
        user: &UserContext,
        mission: String,
        input: CompoundDraftAdoption,
    ) -> Result<String> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;
            let run:Option<String>=tx.query_row("SELECT run_id FROM missions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,mission],|v|v.get(0)).optional()?;let run=run.ok_or(StoreError::NotFound)?;
            let old:Option<(String,i64,String)>=tx.query_row("SELECT action_id,action_revision,action_digest FROM compound_draft_adoptions WHERE tenant_id=?1 AND account_id=?2 AND run_id=?3",params![u.tenant.0,u.account.id,run],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?))).optional()?;
            if old.is_some_and(|v|v!=(input.action_id.clone(),input.revision,input.digest.clone())){return Err(StoreError::Conflict)}
            tx.execute("INSERT OR IGNORE INTO compound_draft_adoptions(tenant_id,account_id,run_id,action_id,action_revision,action_digest) VALUES(?1,?2,?3,?4,?5,?6)",params![u.tenant.0,u.account.id,run,input.action_id,input.revision,input.digest])?;
            let p=proof(&tx,&u.tenant.0,&u.account.id,&run)?.ok_or(StoreError::InvalidInput)?;
            tx.execute("UPDATE compound_draft_adoptions SET recipe_route=?4 WHERE tenant_id=?1 AND account_id=?2 AND run_id=?3 AND recipe_route IS NULL",params![u.tenant.0,u.account.id,run,p.route])?;
            let observed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM learned_skill_observations WHERE tenant_id=?1 AND run_id=?2)",params![u.tenant.0,run],|v|v.get(0))?;
            let id=if observed{pin(&p.route).id}else{
                let r=chat::load_run(&tx,&u.tenant.0,&run)?;let id=compound::record(&tx,&r,p)?;
                tx.execute("INSERT INTO learned_skill_observations VALUES(?1,?2)",params![u.tenant.0,run])?;id
            };
            tx.commit()?;Ok(id)
        }).await
    }
}
pub(crate) fn invalidate_action(c: &Connection, t: &str, id: &str) -> Result<()> {
    let runs=c.prepare("SELECT d.run_id FROM compound_draft_adoptions d JOIN learned_skill_proofs p ON p.tenant_id=d.tenant_id AND p.run_id=d.run_id WHERE d.tenant_id=?1 AND d.action_id=?2")?.query_map(params![t,id],|v|v.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let origins = runs.clone();
    let affected = invalidate_runs(c, t, runs)?
        .into_iter()
        .filter(|v| !origins.contains(v))
        .collect();
    crate::agent_execution::purge_runs(c, t, affected)?;
    crate::recurring_missions::reconcile_skills(c, Some(t))
}
