//! Sealed, current-user-only, terminal memory commands. No model or external IO.
use super::*;
use moosuri_core::{MemoryCommandAction, MemoryCommandIntent, MemoryDraft, RunState};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCommandClarification {
    NoTarget,
    AmbiguousTarget,
    TargetChanged,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MemoryCommandOutcome {
    Applied {
        action: MemoryCommandAction,
        memory_id: Option<String>,
        revision: Option<i64>,
    },
    Clarify {
        reason: MemoryCommandClarification,
    },
}
/// A sealed slot plus a current execution lease. No public constructor, Debug or serde.
#[derive(Clone)]
pub struct PreparedMemoryCommand {
    lease: RunLease,
    command: String,
    nonce: [u8; 32],
}
impl PreparedMemoryCommand {
    pub fn run_id(&self) -> &str {
        self.lease.run_id()
    }
}
struct Slot {
    command: String,
    nonce: Vec<u8>,
    account: String,
    digest: Vec<u8>,
    action: String,
    target: Option<String>,
    revision: Option<i64>,
    effective_revision: Option<i64>,
    clarification: Option<String>,
    attempt: i32,
    bound_nonce: Vec<u8>,
    session: Vec<u8>,
    epoch: i64,
    state: String,
    outcome: Option<String>,
}
fn encode<T: Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|_| StoreError::Corrupt)
}
fn action_name(action: MemoryCommandAction) -> &'static str {
    match action {
        MemoryCommandAction::Remember => "remember",
        MemoryCommandAction::Correct => "correct",
        MemoryCommandAction::Forget => "forget",
    }
}
fn slot(conn: &Connection, tenant: &str, run: &str) -> Result<Option<Slot>> {
    Ok(conn.query_row("SELECT command_id,nonce,account_id,intent_digest,action,target_id,target_revision,clarification,bound_attempt,bound_nonce,session_digest,auth_epoch,state,outcome,target_effective_revision FROM memory_command_slots WHERE tenant_id=?1 AND run_id=?2 AND slot=0",params![tenant,run],|r|Ok(Slot {command:r.get(0)?,nonce:r.get(1)?,account:r.get(2)?,digest:r.get(3)?,action:r.get(4)?,target:r.get(5)?,revision:r.get(6)?,clarification:r.get(7)?,attempt:r.get(8)?,bound_nonce:r.get(9)?,session:r.get(10)?,epoch:r.get(11)?,state:r.get(12)?,outcome:r.get(13)?,effective_revision:r.get(14)?})).optional()?)
}
pub(super) fn has_slot(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM memory_command_slots WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get(0),
    )?)
}
fn intent_digest(text: &str, intent: &MemoryCommandIntent) -> Result<[u8; 32]> {
    Ok(Sha256::digest(encode(&(text, intent))?.as_bytes()).into())
}
fn quote(text: &str, value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty()
        || value.len() > max
        || !text.contains(value)
        || value.chars().any(|c| c.is_control() && !c.is_whitespace())
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn label(value: &str, max: usize) -> Result<()> {
    if value.trim().is_empty() || value.len() > max || value.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn validate_intent(text: &str, intent: &MemoryCommandIntent) -> Result<()> {
    if encode(intent)?.len() > 8192 {
        return Err(StoreError::InvalidInput);
    }
    quote(text, &intent.intent_quote, 512)?;
    match intent.action {
        MemoryCommandAction::Remember => {
            if intent.target_quote.is_some() {
                return Err(StoreError::InvalidInput);
            }
            quote(
                text,
                intent
                    .fact_quote
                    .as_deref()
                    .ok_or(StoreError::InvalidInput)?,
                4096,
            )?;
            label(
                intent.subject.as_deref().ok_or(StoreError::InvalidInput)?,
                200,
            )?;
        }
        MemoryCommandAction::Correct => {
            quote(
                text,
                intent
                    .fact_quote
                    .as_deref()
                    .ok_or(StoreError::InvalidInput)?,
                4096,
            )?;
            label(
                intent.subject.as_deref().ok_or(StoreError::InvalidInput)?,
                200,
            )?;
            let target = intent
                .target_quote
                .as_deref()
                .ok_or(StoreError::InvalidInput)?;
            quote(text, target, 128)?;
            label(target, 128)?;
        }
        MemoryCommandAction::Forget => {
            if intent.fact_quote.is_some() || intent.subject.is_some() {
                return Err(StoreError::InvalidInput);
            }
            let target = intent
                .target_quote
                .as_deref()
                .ok_or(StoreError::InvalidInput)?;
            quote(text, target, 128)?;
            label(target, 128)?;
        }
    }
    Ok(())
}
fn no_tools(conn: &Connection, tenant: &str, run: &str) -> Result<()> {
    let (tools,pending):(i64,bool)=conn.query_row("SELECT tool_calls,EXISTS(SELECT 1 FROM agent_tool_slots s WHERE s.tenant_id=a.tenant_id AND s.run_id=a.run_id) FROM agent_execution a WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?)))?;
    if tools != 0 || pending {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
fn normalize(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_whitespace())
        .flat_map(char::to_lowercase)
        .collect()
}
fn active(draft: &MemoryDraft, time: i64) -> bool {
    draft.valid_from.is_none_or(|v| v <= time) && draft.valid_until.is_none_or(|v| v > time)
}
/// The effective revision determines the literal match; the separately pinned
/// newest revision protects the mutation CAS, including future corrections.
type Target = (
    Option<(String, i64, i64)>,
    Option<MemoryCommandClarification>,
);
fn resolve(conn: &Connection, tenant: &str, text: &str) -> Result<Target> {
    resolve_at(conn, tenant, text, now()?)
}
fn resolve_at(conn: &Connection, tenant: &str, text: &str, time: i64) -> Result<Target> {
    // Match the temporal search contract: choose the newest begun revision first,
    // then require its surviving active version and apply expiry. Never revive
    // an older assertion when the selected version expired or was purged.
    let rows=conn.prepare("SELECT m.id,m.revision,v.revision,v.draft FROM knowledge_memories m JOIN knowledge_memory_versions v ON v.tenant_id=m.tenant_id AND v.memory_id=m.id AND v.revision=(SELECT max(t.revision) FROM knowledge_memory_timeline t WHERE t.tenant_id=m.tenant_id AND t.memory_id=m.id AND (t.valid_from IS NULL OR t.valid_from<=?2)) WHERE m.tenant_id=?1 AND m.status!='forgotten' AND v.status='active' ORDER BY m.id LIMIT 4097")?
      .query_map(params![tenant,time],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    if rows.len() > 4096 {
        return Ok((None, Some(MemoryCommandClarification::AmbiguousTarget)));
    }
    let needle = normalize(text);
    let mut found = None;
    for (id, head, effective, encoded) in rows {
        let draft: MemoryDraft = decode(&encoded)?;
        if active(&draft, time)
            && (normalize(&draft.text).contains(&needle)
                || normalize(&draft.subject).contains(&needle))
        {
            if found.is_some() {
                return Ok((None, Some(MemoryCommandClarification::AmbiguousTarget)));
            }
            found = Some((id, head, effective));
        }
    }
    Ok(if found.is_some() {
        (found, None)
    } else {
        (None, Some(MemoryCommandClarification::NoTarget))
    })
}
fn value_run(conn: &Connection, tenant: &str, value: &Slot) -> Result<String> {
    Ok(conn.query_row(
        "SELECT run_id FROM memory_command_slots WHERE tenant_id=?1 AND command_id=?2",
        params![tenant, value.command],
        |r| r.get(0),
    )?)
}
pub(super) fn has_target_plan(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    targets::has_plan(conn, tenant, run)
}
pub(super) fn guard_target_dispatch(conn: &Connection, run: &chat::Run) -> Result<()> {
    targets::guard_dispatch(conn, run)
}
fn current_target(conn: &Connection, tenant: &str, value: &Slot, quote: &str) -> Result<bool> {
    current_target_at(conn, tenant, value, quote, now()?)
}
fn current_target_at(
    conn: &Connection,
    tenant: &str,
    value: &Slot,
    quote: &str,
    time: i64,
) -> Result<bool> {
    let (Some(id), Some(head), Some(effective)) =
        (&value.target, value.revision, value.effective_revision)
    else {
        return Ok(false);
    };
    if targets::has_plan(conn, tenant, &value_run(conn, tenant, value)?)? {
        return targets::current_selection(conn, tenant, value, time);
    }
    let (selected, clarification) = resolve_at(conn, tenant, quote, time)?;
    Ok(clarification.is_none()
        && selected
            .as_ref()
            .is_some_and(|(current_id, current_head, current_effective)| {
                current_id == id && *current_head == head && *current_effective == effective
            }))
}
fn load_intent(conn: &Connection, run: &chat::Run, value: &Slot) -> Result<MemoryCommandIntent> {
    let encoded: String = conn
        .query_row(
            "SELECT intent FROM memory_command_plans WHERE tenant_id=?1 AND run_id=?2",
            params![run.tenant, run.id],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::Conflict)?;
    let intent: MemoryCommandIntent = decode(&encoded)?;
    validate_intent(&run.text, &intent)?;
    if intent_digest(&run.text, &intent)?.as_slice() != value.digest
        || action_name(intent.action) != value.action
    {
        return Err(StoreError::Corrupt);
    }
    Ok(intent)
}
fn capability(lease: &RunLease, value: &Slot) -> Result<PreparedMemoryCommand> {
    Ok(PreparedMemoryCommand {
        lease: lease.clone(),
        command: value.command.clone(),
        nonce: value
            .nonce
            .clone()
            .try_into()
            .map_err(|_| StoreError::Corrupt)?,
    })
}
fn receipt_access(conn: &Connection, prepared: &PreparedMemoryCommand, value: &Slot) -> Result<()> {
    if prepared.command != value.command
        || prepared.nonce.as_slice() != value.nonce
        || prepared.lease.attempt != value.attempt
        || prepared.lease.nonce.as_slice() != value.bound_nonce
        || prepared.lease.epoch != value.epoch
    {
        return Err(StoreError::Unauthorized);
    }
    let user = load_context(
        conn,
        value
            .session
            .clone()
            .try_into()
            .map_err(|_| StoreError::Corrupt)?,
        false,
    )?;
    if user.restricted
        || user.tenant.0 != prepared.lease.tenant
        || user.account.id != value.account
        || user.epoch != value.epoch
    {
        return Err(StoreError::Unauthorized);
    }
    Ok(())
}
fn confirmation(value: &MemoryCommandOutcome) -> &'static str {
    match value {
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Remember,
            ..
        } => "기억을 저장했습니다.",
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Correct,
            ..
        } => "기억을 바로잡았습니다.",
        MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            ..
        } => "기억과 수정 이력을 지웠습니다. 원문 자료는 유지됩니다.",
        MemoryCommandOutcome::Clarify {
            reason: MemoryCommandClarification::NoTarget,
        } => {
            "해당하는 현재 기억을 찾지 못했습니다. 어떤 기억을 바꾸거나 지울지 내용을 더 구체적으로 알려 주세요."
        }
        MemoryCommandOutcome::Clarify {
            reason: MemoryCommandClarification::AmbiguousTarget,
        } => {
            "해당하는 기억이 여러 개이거나 범위를 좁혀야 합니다. 어떤 기억을 바꾸거나 지울지 내용을 더 구체적으로 알려 주세요."
        }
        MemoryCommandOutcome::Clarify {
            reason: MemoryCommandClarification::TargetChanged,
        } => {
            "대상 기억이 변경되어 요청을 적용하지 않았습니다. 현재 내용을 확인하고 다시 알려 주세요."
        }
    }
}
fn finalize(
    conn: &Connection,
    tenant: &str,
    run_id: &str,
    outcome: &MemoryCommandOutcome,
) -> Result<()> {
    // Receipt identity remains even if the conversation vanished after journal adoption.
    conn.execute("UPDATE memory_command_slots SET state='committed',outcome=?3 WHERE tenant_id=?1 AND run_id=?2 AND slot=0",params![tenant,run_id,encode(outcome)?])?;
    conn.execute(
        "DELETE FROM memory_command_plans WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run_id],
    )?;
    conn.execute(
        "DELETE FROM memory_target_plans WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run_id],
    )?;
    let run = match chat::load_run(conn, tenant, run_id) {
        Ok(v) => v,
        Err(StoreError::NotFound) => return Ok(()),
        Err(e) => return Err(e),
    };
    // This is unconditional: an old preview can exist even if the mutation itself
    // did not invalidate a dependency. Reset replay before independent confirmation.
    agent_execution::purge_runs(conn, tenant, vec![run_id.to_owned()])?;
    conn.execute(
        "DELETE FROM agent_dependencies WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run_id],
    )?;
    let generation = knowledge::generation(conn, tenant)?;
    conn.execute("UPDATE agent_execution SET invalidated=0,generation=?3,tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![tenant,run_id,generation])?;
    if let MemoryCommandOutcome::Applied {
        action: MemoryCommandAction::Remember | MemoryCommandAction::Correct,
        memory_id: Some(id),
        revision: Some(revision),
    } = outcome
    {
        conn.execute(
            "INSERT INTO agent_dependencies VALUES(?1,?2,'memory',?3,?4)",
            params![tenant, run_id, id, revision],
        )?;
    }
    chat::apply_patches(
        conn,
        &run,
        vec![OutputPatch::Append {
            item: "memory-command-confirmation".into(),
            text: confirmation(outcome).into(),
            phase: Some(moosuri_core::MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    let current = chat::load_run(conn, tenant, run_id)?;
    chat::transition(conn, &current, RunState::Completed, None)
}
/// Invoked ONLY by action5, inside the same transaction as journal metadata replay.
pub(super) fn apply_journal_forget(conn: &Connection, tenant: &str, command: &str) -> Result<()> {
    let run: String = conn
        .query_row(
            "SELECT run_id FROM memory_command_slots WHERE tenant_id=?1 AND command_id=?2",
            params![tenant, command],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::Corrupt)?;
    let value = slot(conn, tenant, &run)?.ok_or(StoreError::Corrupt)?;
    if value.state == "committed" {
        return Ok(());
    }
    if value.action != "forget" || value.clarification.is_some() {
        return Err(StoreError::Corrupt);
    }
    let target = value.target.ok_or(StoreError::Corrupt)?;
    let expected = value.revision.ok_or(StoreError::Corrupt)?;
    let effective = value.effective_revision.ok_or(StoreError::Corrupt)?;
    apply_direct_forget(conn, tenant, command, &target, expected, effective)
}
/// The durable successor record carries target/revisions independently of this
/// ordinary receipt lookup. A future historical replayer need not resolve target
/// through a command slot that may postdate its snapshot.
pub(crate) fn apply_direct_forget(
    conn: &Connection,
    tenant: &str,
    command: &str,
    target: &str,
    expected: i64,
    effective: i64,
) -> Result<()> {
    let run: String = conn
        .query_row(
            "SELECT run_id FROM memory_command_slots WHERE tenant_id=?1 AND command_id=?2",
            params![tenant, command],
            |r| r.get(0),
        )
        .optional()?
        .ok_or(StoreError::Corrupt)?;
    let value = slot(conn, tenant, &run)?.ok_or(StoreError::Corrupt)?;
    if value.action != "forget"
        || value.clarification.is_some()
        || value.target.as_deref() != Some(target)
        || value.revision != Some(expected)
        || value.effective_revision != Some(effective)
    {
        return Err(StoreError::Corrupt);
    }
    if value.state == "committed" {
        return Ok(());
    }
    let revision = expected.checked_add(1).ok_or(StoreError::Corrupt)?;
    let current: Option<(i64, String)> = conn
        .query_row(
            "SELECT revision,status FROM knowledge_memories WHERE tenant_id=?1 AND id=?2",
            params![tenant, target],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match current {
        Some((r, status))
            if r == expected && matches!(status.as_str(), "active" | "invalidated") =>
        {
            knowledge::destructive::forget(conn, tenant, target)?
        }
        Some((r, status)) if r == revision && status == "forgotten" => {}
        _ => return Err(StoreError::Corrupt),
    }
    finalize(
        conn,
        tenant,
        &run,
        &MemoryCommandOutcome::Applied {
            action: MemoryCommandAction::Forget,
            memory_id: Some(target.into()),
            revision: Some(revision),
        },
    )
}

fn prepare_slot(
    conn: &Connection,
    run: &chat::Run,
    step: &StepLease,
    intent: &MemoryCommandIntent,
    resolution: Option<Target>,
) -> Result<PreparedMemoryCommand> {
    let completed: bool = conn.query_row(
        "SELECT state='completed' FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",
        params![run.tenant, run.id, step.step],
        |r| r.get(0),
    )?;
    if !completed {
        return Err(StoreError::Conflict);
    }
    no_tools(conn, &run.tenant, &run.id)?;
    validate_intent(&run.text, intent)?;
    let digest = intent_digest(&run.text, intent)?;
    if let Some(value) = slot(conn, &run.tenant, &run.id)? {
        if value.digest != digest {
            return Err(StoreError::Conflict);
        }
        let cap = capability(&step.run, &value)?;
        return Ok(cap);
    }
    if targets::abandoned(conn, &run.tenant, &run.id)? {
        return Err(StoreError::Conflict);
    }
    if resolution.is_none() && targets::has_plan(conn, &run.tenant, &run.id)? {
        return Err(StoreError::Conflict);
    }
    let (target, clarification) = match resolution {
        Some(value) => value,
        None => match &intent.target_quote {
            Some(target) => resolve(conn, &run.tenant, target)?,
            None => (None, None),
        },
    };
    let command = Uuid::new_v4().to_string();
    let mut nonce = [0; 32];
    OsRng
        .try_fill_bytes(&mut nonce)
        .map_err(|_| StoreError::Unavailable)?;
    let (target, revision, effective) = target
        .map(|(id, head, effective)| (Some(id), Some(head), Some(effective)))
        .unwrap_or((None, None, None));
    conn.execute("INSERT INTO memory_command_slots(tenant_id,run_id,command_id,nonce,account_id,intent_digest,action,target_id,target_revision,target_effective_revision,clarification,proof_attempt,proof_step,bound_attempt,bound_nonce,session_digest,auth_epoch,state) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?16,?10,?11,?12,?11,?13,?14,?15,'prepared')",params![run.tenant,run.id,command,nonce.as_slice(),run.account,digest.as_slice(),action_name(intent.action),target,revision,clarification.map(|v|encode(&v)).transpose()?,run.attempt,step.step,step.run.nonce.as_slice(),run.session.as_slice(),run.epoch,effective])?;
    conn.execute(
        "INSERT INTO memory_command_plans VALUES(?1,?2,?3)",
        params![run.tenant, run.id, encode(intent)?],
    )?;
    Ok(PreparedMemoryCommand {
        lease: step.run.clone(),
        command,
        nonce,
    })
}

impl Store {
    pub async fn prepare_memory_command(
        &self,
        step: &StepLease,
        intent: MemoryCommandIntent,
    ) -> Result<PreparedMemoryCommand> {
        let step = step.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match agent_execution::guard_step(&tx, &db.provider_key, &step) {
                Ok(run) => run,
                Err(e) => {
                    tx.commit()?;
                    return Err(e);
                }
            };
            let prepared = prepare_slot(&tx, &run, &step, &intent, None)?;
            tx.commit()?;
            Ok(prepared)
        })
        .await
    }
    pub async fn resume_memory_command(
        &self,
        lease: &RunLease,
    ) -> Result<Option<PreparedMemoryCommand>> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            if let Some(value)=slot(&tx,&lease.tenant,&lease.run)? && value.state=="committed" {
                let cap=capability(&lease,&value)?;receipt_access(&tx,&cap,&value)?;tx.commit()?;return Ok(None)
            }
            let run=match chat::guard(&tx,&db.provider_key,&lease){Ok(run)=>run,Err(e)=>{tx.commit()?;return Err(e)}};
            let Some(value)=slot(&tx,&run.tenant,&run.id)? else {tx.commit()?;return Ok(None)};
            no_tools(&tx,&run.tenant,&run.id)?;load_intent(&tx,&run,&value)?;
            tx.execute("UPDATE memory_command_slots SET bound_attempt=?3,bound_nonce=?4,session_digest=?5,auth_epoch=?6 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,run.attempt,lease.nonce.as_slice(),run.session.as_slice(),run.epoch])?;
            let cap=capability(&lease,&value)?;tx.commit()?;Ok(Some(cap))
        }).await
    }
    pub async fn execute_memory_command(
        &self,
        prepared: &PreparedMemoryCommand,
    ) -> Result<MemoryCommandOutcome> {
        let prepared = prepared.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let value = slot(&tx, &prepared.lease.tenant, &prepared.lease.run)?
                .ok_or(StoreError::NotFound)?;
            receipt_access(&tx, &prepared, &value)?;
            if let Some(result) = &value.outcome {
                let result = decode(result)?;
                tx.commit()?;
                return Ok(result);
            }
            let run = match chat::guard(&tx, &db.provider_key, &prepared.lease) {
                Ok(run) => run,
                Err(e) => {
                    tx.commit()?;
                    return Err(e);
                }
            };
            no_tools(&tx, &run.tenant, &run.id)?;
            let intent = load_intent(&tx, &run, &value)?;
            let clarification = value.clarification.as_deref().map(decode).transpose()?.or(
                if value.action != "remember"
                    && !current_target(
                        &tx,
                        &run.tenant,
                        &value,
                        intent.target_quote.as_deref().ok_or(StoreError::Corrupt)?,
                    )?
                {
                    Some(MemoryCommandClarification::TargetChanged)
                } else {
                    None
                },
            );
            if let Some(reason) = clarification {
                let outcome = MemoryCommandOutcome::Clarify { reason };
                finalize(&tx, &run.tenant, &run.id, &outcome)?;
                tx.commit()?;
                return Ok(outcome);
            }
            if intent.action == MemoryCommandAction::Forget {
                // No actor yield or await can occur between final guard, durable
                // adoption and apply. Any post-append failure poisons readiness.
                let intent = destructive_intents::Effect::ForgetCommand {
                    target: value.target.clone().ok_or(StoreError::Corrupt)?,
                    revision: value.revision.ok_or(StoreError::Corrupt)?,
                    effective_revision: value.effective_revision.ok_or(StoreError::Corrupt)?,
                    command: value.command.clone(),
                };
                let prepared_record = db.destructive_journal.prepare(&run.tenant, intent)?;
                db.poisoned = true;
                let record = db.destructive_journal.append(prepared_record)?;
                destructive_intents::apply(&tx, &record)?;
                let result = slot(&tx, &run.tenant, &run.id)?
                    .and_then(|v| v.outcome)
                    .ok_or(StoreError::Corrupt)?;
                let outcome = decode(&result)?;
                tx.commit()?;
                db.poisoned = false;
                return Ok(outcome);
            }
            let target = if intent.action == MemoryCommandAction::Correct {
                Some((
                    value.target.ok_or(StoreError::Corrupt)?,
                    value.revision.ok_or(StoreError::Corrupt)?,
                ))
            } else {
                None
            };
            let draft = MemoryDraft {
                text: intent.fact_quote.ok_or(StoreError::Corrupt)?,
                subject: intent.subject.ok_or(StoreError::Corrupt)?,
                speaker: None,
                valid_from: None,
                valid_until: None,
                evidence: vec![],
            };
            let memory = knowledge::memories::write_in_transaction(
                &tx,
                &run.tenant,
                target,
                value.command,
                draft,
            )?;
            let outcome = MemoryCommandOutcome::Applied {
                action: intent.action,
                memory_id: Some(memory.id),
                revision: Some(memory.revision),
            };
            finalize(&tx, &run.tenant, &run.id, &outcome)?;
            tx.commit()?;
            Ok(outcome)
        })
        .await
    }
    /// Authenticated receipt recovery never authorizes execution or recreates chat.
    pub async fn memory_command_receipt(
        &self,
        user: &UserContext,
        run_id: String,
    ) -> Result<Option<MemoryCommandOutcome>> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let Some(value) = slot(&db.conn, &user.tenant.0, &run_id)? else {
                return Ok(None);
            };
            if value.account != user.account.id {
                return Err(StoreError::NotFound);
            }
            value.outcome.as_deref().map(decode).transpose()
        })
        .await
    }
}
mod targets;
#[cfg(test)]
mod tests;
pub use targets::{
    MemoryCommandPreparation, MemoryTargetCandidate, MemoryTargetCandidates, MemoryTargetChoice,
};
