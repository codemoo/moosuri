//! Dedicated, owner-adopted report qualification. A mission status, instruction
//! import, or recipe replay is never evidence of a new learned success.
use super::*;
use moosuri_core::{
    CompoundRecipeEdit, CompoundReportAdoption, CompoundReportRecipe, MissionCandidate,
    MissionCompound,
};
const PREFIX: &str = "compound-report-v1:";
pub(super) fn recipe(route: &str) -> Option<CompoundReportRecipe> {
    let raw = route
        .strip_prefix(PREFIX)
        .or_else(|| route.strip_prefix(draft::PREFIX))?;
    if raw.len() > 4096 {
        return None;
    }
    let mut v: CompoundReportRecipe = serde_json::from_str(raw).ok()?;
    v.calculation = v.calculation.canonical()?;
    (v.valid() && encode(&v).ok()?.as_str() == raw).then_some(v)
}
fn route(v: &CompoundReportRecipe) -> Result<String> {
    if !v.valid() {
        return Err(StoreError::InvalidInput);
    }
    Ok(format!("{PREFIX}{}", encode(v)?))
}
pub(super) fn derived(c: &Connection, t: &str, run: &str) -> Result<bool> {
    c.query_row("SELECT EXISTS(SELECT 1 FROM learned_skill_uses WHERE tenant_id=?1 AND run_id=?2) OR EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND json_extract(input,'$.tool') IN('learned_skill','skill_read')) OR EXISTS(SELECT 1 FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2)",params![t,run],|r|r.get(0)).map_err(Into::into)
}
/// Revalidate the exact artifact revision, original source reads, numeric
/// postcondition and every actual arithmetic child. No generic "success" flag.
pub(super) fn proof(c: &Connection, t: &str, a: &str, run: &str) -> Result<Option<Proof>> {
    let row: Option<(String,i64,String,String,i64,Option<String>)> = c.query_row(
        "SELECT d.artifact_id,d.artifact_revision,m.candidate,q.contract,q.acceptance_revision,d.recipe_route FROM compound_report_adoptions d JOIN missions m ON m.tenant_id=d.tenant_id AND m.run_id=d.run_id AND m.account_id=d.account_id JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id JOIN mission_report_contracts q ON q.tenant_id=m.tenant_id AND q.run_id=m.run_id AND q.artifact_id=d.artifact_id AND q.artifact_revision=d.artifact_revision WHERE d.tenant_id=?1 AND d.account_id=?2 AND d.run_id=?3 AND m.state IN('partial','achieved') AND m.uncertain=0 AND r.state IN('completed','incomplete') AND e.invalidated=0 AND NOT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?3 AND state!='completed')",
        params![t,a,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((artifact, revision, candidate, contract, acceptance, bound_route)) = row else {
        return Ok(None);
    };
    if derived(c, t, run)? {
        return Ok(None);
    }
    let contract: MissionCompound =
        serde_json::from_str(&contract).map_err(|_| StoreError::Corrupt)?;
    if contract.schema_version != 1
        || !contract.review_required
        || contract
            .output
            .as_ref()
            .is_none_or(|v| v.id != artifact || v.revision != revision)
        || contract.acceptance_revision != acceptance
    {
        return Ok(None);
    }
    let candidate: MissionCandidate =
        serde_json::from_str(&candidate).map_err(|_| StoreError::Corrupt)?;
    let document = crate::artifacts::version(c, t, &artifact, None)?;
    if document.artifact.revision != revision {
        return Ok(None);
    }
    let Some(draft) = document.draft else {
        return Ok(None);
    };
    if !matches!(draft.payload, moosuri_core::ArtifactPayload::Document)
        || draft.body != candidate.text
    {
        return Ok(None);
    }
    let mut r = chat::load_run(c, t, run)?;
    if r.account != a {
        return Ok(None);
    }
    r.text = crate::missions::learned_task_text(c, &r)?;
    let floor = crate::missions::acceptance_floor(c, t, run)?;
    let rows=c.prepare("SELECT step,call_id,input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step>=?3 ORDER BY step,rowid")?.query_map(params![t,run,floor],|v|Ok((v.get::<_,i64>(0)?,v.get::<_,String>(1)?,v.get::<_,String>(2)?,v.get::<_,Option<String>>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut calls = vec![];
    let mut calc = None;
    for (step, call, input, result) in rows {
        let input: Input = serde_json::from_str(&input).map_err(|_| StoreError::Corrupt)?;
        let Some(result) = result else {
            return Ok(None);
        };
        let result: Output = serde_json::from_str(&result).map_err(|_| StoreError::Corrupt)?;
        if matches!(input, Input::EvidenceCalculate { .. }) {
            if !candidate
                .evidence
                .iter()
                .any(|e| e.step == step && e.call_id == call)
                || !contract.steps.iter().any(|n| {
                    n.id == "calculate"
                        && n.receipts
                            .iter()
                            .any(|e| e.step == step && e.call_id == call && e.ordinal.is_none())
                })
            {
                return Ok(None);
            }
            calc = Some((step, call.clone()));
        }
        calls.push((input, result));
    }
    let Some((_, _)) = calc else { return Ok(None) };
    let receipt = hash(&encode(&(&calls, &artifact, revision))?);
    let Some(mut p) = evidence::qualify(c, &r, a, &calls, &receipt, &draft.body)? else {
        return Ok(None);
    };
    let Some(plan) = evidence::plan(&p.route) else {
        return Ok(None);
    };
    // Revalidation follows the immutable first-adoption binding. Editing a
    // different recipe into this graph must not remap this pin's old proofs.
    if let Some(bound) = bound_route {
        let Some(selected) = recipe(&bound) else {
            return Ok(None);
        };
        if encode(&selected.calculation)? != encode(&plan)? {
            return Ok(None);
        }
        p.route = bound;
        return Ok(Some(p));
    }
    // Only a genuinely new adoption selects the newest matching structure.
    // Removed routes remain tombstones and cannot inherit fresh proof.
    let mut selected = CompoundReportRecipe {
        schema_version: 1,
        revision: 1,
        calculation: plan.clone(),
    };
    for existing in routes(c, t, a)? {
        if !draft::is_route(&existing)
            && let Some(v) = recipe(&existing)
            && encode(&v.calculation)? == encode(&plan)?
            && v.revision > selected.revision
        {
            selected = v;
        }
    }
    p.route = route(&selected)?;
    Ok(Some(p))
}
pub(super) fn record(c: &Connection, r: &chat::Run, p: Proof) -> Result<String> {
    let id = pin(&p.route).id;
    let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM learned_skills WHERE tenant_id=?1 AND account_id=?2 AND id=?3)",params![r.tenant,r.account,id],|v|v.get(0))?;
    if !exists
        && c.query_row(
            "SELECT count(*) FROM learned_skills WHERE tenant_id=?1 AND account_id=?2",
            params![r.tenant, r.account],
            |v| v.get::<_, i64>(0),
        )? >= 64
    {
        return Err(StoreError::Conflict);
    }
    c.execute("INSERT OR IGNORE INTO learned_skills(tenant_id,account_id,id,route,schema_version,state) VALUES(?1,?2,?3,?4,?5,'candidate')",params![r.tenant,r.account,id,p.route,SCHEMA])?;
    refresh(c, &r.tenant, &r.account, &id)?;
    // Invalidated examples were erased; a fresh adoption may start qualification.
    c.execute("UPDATE learned_skills SET state='candidate',schema_version=?4,failures=0,revision=revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='invalidated' AND enabled=1",params![r.tenant,r.account,id,SCHEMA])?;
    let accepting:bool=c.query_row("SELECT state='candidate' AND enabled=1 AND schema_version=?4 FROM learned_skills WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![r.tenant,r.account,id,SCHEMA],|v|v.get(0))?;
    if accepting {
        c.execute(
            "INSERT OR IGNORE INTO learned_skill_proofs VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![r.tenant, r.account, id, p.input, r.id, p.receipt, p.subject],
        )?;
        refresh(c, &r.tenant, &r.account, &id)?;
    }
    Ok(id)
}
impl Store {
    /// An explicit owner adoption is separate from semantic review completion.
    /// It confirms only this immutable numeric/factual output, never its narrative.
    pub async fn adopt_compound_report(
        &self,
        user: &UserContext,
        mission: String,
        input: CompoundReportAdoption,
    ) -> Result<String> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u=validate(&tx,&user,true,false)?;
            let run:Option<String>=tx.query_row("SELECT run_id FROM missions WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,mission],|v|v.get(0)).optional()?;
            let run=run.ok_or(StoreError::NotFound)?;
            let existing:Option<(String,i64)>=tx.query_row("SELECT artifact_id,artifact_revision FROM compound_report_adoptions WHERE tenant_id=?1 AND account_id=?2 AND run_id=?3",params![u.tenant.0,u.account.id,run],|v|Ok((v.get(0)?,v.get(1)?))).optional()?;
            if existing.as_ref().is_some_and(|(id,rev)|*id!=input.artifact_id || *rev!=input.artifact_revision){return Err(StoreError::Conflict)}
            tx.execute("INSERT OR IGNORE INTO compound_report_adoptions(tenant_id,account_id,run_id,artifact_id,artifact_revision) VALUES(?1,?2,?3,?4,?5)",params![u.tenant.0,u.account.id,run,input.artifact_id,input.artifact_revision])?;
            let p=proof(&tx,&u.tenant.0,&u.account.id,&run)?.ok_or(StoreError::InvalidInput)?;
            tx.execute("UPDATE compound_report_adoptions SET recipe_route=?4 WHERE tenant_id=?1 AND account_id=?2 AND run_id=?3 AND recipe_route IS NULL",params![u.tenant.0,u.account.id,run,p.route])?;
            let r=chat::load_run(&tx,&u.tenant.0,&run)?;
            // An invalidated/deleted adoption cannot be harvested again later.
            let observed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM learned_skill_observations WHERE tenant_id=?1 AND run_id=?2)",params![u.tenant.0,run],|v|v.get(0))?;
            let id=if observed {pin(&p.route).id} else {
                let id=record(&tx,&r,p)?;
                tx.execute("INSERT INTO learned_skill_observations VALUES(?1,?2)",params![u.tenant.0,run])?;id
            };
            tx.commit()?;Ok(id)
        }).await
    }
    pub async fn compound_recipe(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<CompoundReportRecipe> {
        let user = user.clone();
        self.call(move|db|{
            let u=validate(&db.conn,&user,true,false)?;
            let route:String=db.conn.query_row("SELECT route FROM learned_skills WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state!='removed'",params![u.tenant.0,u.account.id,id],|v|v.get(0)).optional()?.ok_or(StoreError::NotFound)?;
            recipe(&route).ok_or(StoreError::NotFound)
        }).await
    }
    pub async fn edit_compound_recipe(
        &self,
        user: &UserContext,
        id: String,
        input: CompoundRecipeEdit,
    ) -> Result<String> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;
            let raw:Option<String>=tx.query_row("SELECT route FROM learned_skills WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4 AND state!='removed'",params![u.tenant.0,u.account.id,id,input.expected_revision],|v|v.get(0)).optional()?;
            let raw=raw.ok_or(StoreError::Conflict)?;
            let is_draft=draft::is_route(&raw);
            let mut v=recipe(&raw).ok_or(StoreError::InvalidInput)?;
            v.revision+=1;v.calculation=input.calculation.canonical().ok_or(StoreError::InvalidInput)?;
            let newroute=if is_draft {draft::route(&v)?}else{route(&v)?};let newpin=pin(&newroute);
            if tx.query_row("SELECT count(*) FROM learned_skills WHERE tenant_id=?1 AND account_id=?2",params![u.tenant.0,u.account.id],|v|v.get::<_,i64>(0))?>=64{return Err(StoreError::Conflict)}
            tx.execute("UPDATE learned_skills SET state='removed',enabled=0,revision=revision+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,id])?;
            tx.execute("DELETE FROM learned_skill_proofs WHERE tenant_id=?1 AND account_id=?2 AND skill_id=?3",params![u.tenant.0,u.account.id,id])?;
            let used=affected_runs(&tx,&u.tenant.0,&u.account.id,&id)?;crate::agent_execution::purge_runs(&tx,&u.tenant.0,used)?;
            tx.execute("INSERT INTO learned_skills(tenant_id,account_id,id,route,schema_version,state) VALUES(?1,?2,?3,?4,?5,'candidate')",params![u.tenant.0,u.account.id,newpin.id,newroute,SCHEMA])?;
            crate::recurring_missions::reconcile_skills(&tx,Some(&u.tenant.0))?;
            tx.commit()?;Ok(newpin.id)
        }).await
    }
}
/// Artifact edits/deletion invalidate adoption proof and actually used descendants
/// immediately, including dependent schedules. Discovery-only pins do not count.
pub(crate) fn invalidate_artifact(
    c: &Connection,
    t: &str,
    id: &str,
    revision: Option<i64>,
) -> Result<()> {
    let runs=c.prepare("SELECT d.run_id FROM compound_report_adoptions d JOIN learned_skill_proofs p ON p.tenant_id=d.tenant_id AND p.run_id=d.run_id WHERE d.tenant_id=?1 AND d.artifact_id=?2 AND (?3 IS NULL OR d.artifact_revision=?3)")?.query_map(params![t,id,revision],|v|v.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let origins = runs.clone();
    let affected = invalidate_runs(c, t, runs)?
        .into_iter()
        .filter(|v| !origins.contains(v))
        .collect();
    crate::agent_execution::purge_runs(c, t, affected)?;
    crate::recurring_missions::reconcile_skills(c, Some(t))
}
