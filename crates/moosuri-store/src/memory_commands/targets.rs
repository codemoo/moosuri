//! A sealed command may ask a model to select among data-only, Store-bound indexes.
//! The result supplies no authority, IDs, action, facts, or replacement intent.
use super::*;

const CANDIDATE_LIMIT: usize = 64;
const DATA_LIMIT: usize = 32768;

#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MemoryTargetCandidate {
    pub index: usize,
    pub text: String,
    pub subject: String,
    pub speaker: Option<String>,
    pub valid_from: Option<i64>,
    pub valid_until: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
struct Candidate {
    id: String,
    head: i64,
    effective: i64,
    data: MemoryTargetCandidate,
}
#[derive(Clone)]
pub struct MemoryTargetCandidates {
    lease: RunLease,
    nonce: Vec<u8>,
    target: String,
    candidates: Vec<MemoryTargetCandidate>,
}
impl MemoryTargetCandidates {
    pub fn target_quote(&self) -> &str {
        &self.target
    }
    pub fn candidates(&self) -> &[MemoryTargetCandidate] {
        &self.candidates
    }
}
pub enum MemoryCommandPreparation {
    Prepared(PreparedMemoryCommand),
    Resolve(MemoryTargetCandidates),
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum MemoryTargetChoice {
    Select(usize),
    Missing,
    Ambiguous,
}
struct Plan {
    nonce: Vec<u8>,
    intent: MemoryCommandIntent,
    generation: i64,
    planning_step: i64,
    snapshot: Vec<Candidate>,
}
pub(super) fn has_plan(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_target_plans WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get(0),
    )?)
}
pub(super) fn abandoned(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    let started: bool = conn.query_row(
        "SELECT memory_target_started FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run],
        |r| r.get(0),
    )?;
    Ok(started && !has_plan(conn, tenant, run)?)
}
type PlanRow = (Vec<u8>, String, Vec<u8>, i64, i64, String);
fn load(conn: &Connection, run: &chat::Run) -> Result<Option<Plan>> {
    let row: Option<PlanRow> = conn.query_row(
        "SELECT nonce,intent,intent_digest,generation,planning_step,snapshot FROM memory_target_plans WHERE tenant_id=?1 AND run_id=?2",
        params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?))).optional()?;
    let Some((nonce, encoded, digest, generation, planning_step, snapshot)) = row else {
        return Ok(None);
    };
    let intent: MemoryCommandIntent = decode(&encoded)?;
    validate_intent(&run.text, &intent)?;
    if intent_digest(&run.text, &intent)?.as_slice() != digest
        || intent.target_quote.is_none()
        || intent.action == MemoryCommandAction::Remember
    {
        return Err(StoreError::Corrupt);
    }
    Ok(Some(Plan {
        nonce,
        intent,
        generation,
        planning_step,
        snapshot: decode(&snapshot)?,
    }))
}
fn capsule(lease: &RunLease, plan: &Plan) -> Result<MemoryTargetCandidates> {
    Ok(MemoryTargetCandidates {
        lease: lease.clone(),
        nonce: plan.nonce.clone(),
        target: plan
            .intent
            .target_quote
            .clone()
            .ok_or(StoreError::Corrupt)?,
        candidates: plan.snapshot.iter().map(|v| v.data.clone()).collect(),
    })
}
/// Enumerate the whole current account scope, never a keyword-selected subset.
/// None means coverage is incomplete; callers must clarify without selection.
fn enumerate(conn: &Connection, tenant: &str, time: i64) -> Result<Option<Vec<Candidate>>> {
    let rows=conn.prepare("SELECT m.id,m.revision,v.revision,v.draft FROM knowledge_memories m JOIN knowledge_memory_versions v ON v.tenant_id=m.tenant_id AND v.memory_id=m.id AND v.revision=(SELECT max(t.revision) FROM knowledge_memory_timeline t WHERE t.tenant_id=m.tenant_id AND t.memory_id=m.id AND (t.valid_from IS NULL OR t.valid_from<=?2)) WHERE m.tenant_id=?1 AND m.status!='forgotten' AND v.status='active' ORDER BY m.id LIMIT 65")?
        .query_map(params![tenant,time],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    if rows.len() > CANDIDATE_LIMIT {
        return Ok(None);
    }
    let mut result = Vec::new();
    for (id, head, effective, encoded) in rows {
        let draft: MemoryDraft = decode(&encoded)?;
        if !active(&draft, time) {
            continue;
        }
        let data = MemoryTargetCandidate {
            index: result.len(),
            text: draft.text,
            subject: draft.subject,
            speaker: draft.speaker,
            valid_from: draft.valid_from,
            valid_until: draft.valid_until,
        };
        result.push(Candidate {
            id,
            head,
            effective,
            data,
        });
    }
    let data = result.iter().map(|v| &v.data).collect::<Vec<_>>();
    if encode(&data)?.len() > DATA_LIMIT || encode(&result)?.len() > 65536 {
        return Ok(None);
    }
    Ok(Some(result))
}
/// Must run in the caller's transaction; callers commit its purge on failure.
pub(super) fn guard_dispatch(conn: &Connection, run: &chat::Run) -> Result<()> {
    guard_dispatch_at(conn, run, now()?)
}
fn guard_dispatch_at(conn: &Connection, run: &chat::Run, time: i64) -> Result<()> {
    if let Some(plan) = load(conn, run)? {
        if !unchanged(conn, &run.tenant, &plan, time)? {
            agent_execution::purge_runs(conn, &run.tenant, vec![run.id.clone()])?;
            return Err(StoreError::Conflict);
        }
    } else if abandoned(conn, &run.tenant, &run.id)? {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
fn unchanged(conn: &Connection, tenant: &str, plan: &Plan, time: i64) -> Result<bool> {
    Ok(knowledge::generation(conn, tenant)? == plan.generation
        && enumerate(conn, tenant, time)?.as_ref() == Some(&plan.snapshot))
}
pub(super) fn current_selection(
    conn: &Connection,
    tenant: &str,
    value: &Slot,
    time: i64,
) -> Result<bool> {
    let run = chat::load_run(conn, tenant, &value_run(conn, tenant, value)?)?;
    let plan = load(conn, &run)?.ok_or(StoreError::Corrupt)?;
    Ok(unchanged(conn, tenant, &plan, time)?
        && plan.snapshot.iter().any(|v| {
            Some(&v.id) == value.target.as_ref()
                && Some(v.head) == value.revision
                && Some(v.effective) == value.effective_revision
        }))
}
impl Store {
    /// Literal unique matches keep the one-round fast path. Only NoTarget may
    /// enter isolated selection; literal ambiguity is never relaxed by a model.
    pub async fn prepare_memory_command_resolution(
        &self,
        step: &StepLease,
        intent: MemoryCommandIntent,
    ) -> Result<MemoryCommandPreparation> {
        let step = step.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match agent_execution::guard_step(&tx,&db.provider_key,&step) {Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            no_tools(&tx,&run.tenant,&run.id)?;
            validate_intent(&run.text,&intent)?;
            let completed: bool=tx.query_row("SELECT state='completed' FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,step.step],|r|r.get(0))?;
            if !completed {return Err(StoreError::Conflict)}
            if slot(&tx,&run.tenant,&run.id)?.is_some() {
                let prepared=prepare_slot(&tx,&run,&step,&intent,None)?;
                tx.commit()?;return Ok(MemoryCommandPreparation::Prepared(prepared))
            }
            if let Some(plan)=load(&tx,&run)? {
                if let Err(e)=guard_dispatch(&tx,&run) {tx.commit()?;return Err(e)}
                if intent_digest(&run.text,&intent)?!=intent_digest(&run.text,&plan.intent)? {return Err(StoreError::Conflict)}
                let candidates=capsule(&step.run,&plan)?;
                tx.commit()?;return Ok(MemoryCommandPreparation::Resolve(candidates))
            }
            if abandoned(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
            let literal=match intent.target_quote.as_deref() {Some(q)=>resolve(&tx,&run.tenant,q)?,None=>(None,None)};
            if literal.1!=Some(MemoryCommandClarification::NoTarget) {
                let prepared=prepare_slot(&tx,&run,&step,&intent,Some(literal))?;
                tx.commit()?;return Ok(MemoryCommandPreparation::Prepared(prepared))
            }
            let snapshot=enumerate(&tx,&run.tenant,now()?)?;
            let Some(snapshot)=snapshot else {
                let prepared=prepare_slot(&tx,&run,&step,&intent,Some((None,Some(MemoryCommandClarification::AmbiguousTarget))))?;
                tx.commit()?;return Ok(MemoryCommandPreparation::Prepared(prepared))
            };
            if snapshot.is_empty() {
                let prepared=prepare_slot(&tx,&run,&step,&intent,Some(literal))?;
                tx.commit()?;return Ok(MemoryCommandPreparation::Prepared(prepared))
            }
            let mut nonce=[0;32];OsRng.try_fill_bytes(&mut nonce).map_err(|_|StoreError::Unavailable)?;
            let started: bool=tx.query_row("SELECT memory_target_started FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
            if started {return Err(StoreError::Conflict)}
            let generation=knowledge::generation(&tx,&run.tenant)?;
            tx.execute("UPDATE agent_execution SET memory_target_started=1,generation=?3 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,generation])?;
            for candidate in &snapshot {
                tx.execute("INSERT OR IGNORE INTO agent_dependencies VALUES(?1,?2,'memory',?3,?4)",params![run.tenant,run.id,candidate.id,candidate.effective])?;
                tx.execute("INSERT OR IGNORE INTO agent_dependencies SELECT tenant_id,?2,'source',source_id,source_revision FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?3 AND memory_revision=?4",params![run.tenant,run.id,candidate.id,candidate.effective])?;
            }
            tx.execute("INSERT INTO memory_target_plans VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![run.tenant,run.id,nonce.as_slice(),encode(&intent)?,intent_digest(&run.text,&intent)?.as_slice(),generation,step.step,encode(&snapshot)?])?;
            let candidates=capsule(&step.run,&Plan{nonce:nonce.to_vec(),intent,generation,planning_step:step.step,snapshot})?;
            tx.commit()?;Ok(MemoryCommandPreparation::Resolve(candidates))
        }).await
    }
    pub async fn resume_memory_targets(
        &self,
        lease: &RunLease,
    ) -> Result<Option<MemoryTargetCandidates>> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match chat::guard(&tx,&db.provider_key,&lease) {Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            if let Err(e)=guard_dispatch(&tx,&run) {tx.commit()?;return Err(e)}
            let result=load(&tx,&run)?.map(|p|capsule(&lease,&p)).transpose()?;
            if result.is_some() {no_tools(&tx,&run.tenant,&run.id)?;}
            let started: bool=tx.query_row("SELECT memory_target_started FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
            if started && result.is_none() {return Err(StoreError::Conflict)}
            tx.commit()?;Ok(result)
        }).await
    }
    /// A selection is bound to the original current request and this plan's index
    /// namespace. No externally supplied tenant or memory ID is accepted.
    pub async fn select_memory_target(
        &self,
        step: &StepLease,
        candidates: &MemoryTargetCandidates,
        choice: MemoryTargetChoice,
    ) -> Result<PreparedMemoryCommand> {
        let step = step.clone();
        let candidates = candidates.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match agent_execution::guard_step(&tx, &db.provider_key, &step) {
                Ok(v) => v,
                Err(e) => {
                    tx.commit()?;
                    return Err(e);
                }
            };
            if candidates.lease.tenant != run.tenant
                || candidates.lease.run != run.id
                || candidates.lease.attempt != step.run.attempt
                || candidates.lease.nonce != step.run.nonce
            {
                return Err(StoreError::Unauthorized);
            }
            let plan = load(&tx, &run)?.ok_or(StoreError::Conflict)?;
            if candidates.nonce != plan.nonce {
                return Err(StoreError::Unauthorized);
            }
            if step.step <= plan.planning_step {
                return Err(StoreError::Conflict);
            }
            let resolution = if !unchanged(&tx, &run.tenant, &plan, now()?)? {
                (None, Some(MemoryCommandClarification::TargetChanged))
            } else {
                match choice {
                    MemoryTargetChoice::Missing => {
                        (None, Some(MemoryCommandClarification::NoTarget))
                    }
                    MemoryTargetChoice::Ambiguous => {
                        (None, Some(MemoryCommandClarification::AmbiguousTarget))
                    }
                    MemoryTargetChoice::Select(index) => {
                        let selected = plan.snapshot.get(index).ok_or(StoreError::InvalidInput)?;
                        (
                            Some((selected.id.clone(), selected.head, selected.effective)),
                            None,
                        )
                    }
                }
            };
            let prepared = prepare_slot(&tx, &run, &step, &plan.intent, Some(resolution))?;
            tx.commit()?;
            Ok(prepared)
        })
        .await
    }
}
#[cfg(test)]
mod tests;
