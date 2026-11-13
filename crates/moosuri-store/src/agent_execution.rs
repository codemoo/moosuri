//! Durable read-only agent execution. This module performs no external IO.
//! Payloads are private runtime state, never chat events or diagnostic Debug text.
use super::*;
use crate::chat::{self, Run};
use moosuri_core::{
    KnowledgeQuery, KnowledgeSearch, MemoryDocument, RunState, RunUsage, SourceSummary,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

mod arithmetic;
pub(crate) mod skills;

const CHECKPOINT_LIMIT: usize = 256 * 1024;
pub const TOOL_INPUT_LIMIT: usize = 4096;
pub const TOOL_RESULT_LIMIT: usize = 16 * 1024;

#[derive(Clone)]
pub struct PrivateCheckpoint {
    pub bytes: Vec<u8>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "tool", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeToolInput {
    #[serde(rename = "knowledge_search")]
    Search {
        query: KnowledgeQuery,
    },
    #[serde(rename = "knowledge_read_source")]
    ReadSource {
        id: String,
        revision: i64,
        start_byte: u32,
        max_bytes: u32,
    },
    #[serde(rename = "knowledge_read_memory")]
    ReadMemory {
        id: String,
        revision: i64,
    },
    SkillRead {
        pin: moosuri_core::SkillPin,
    },
    DecimalCalculate {
        calculation: moosuri_core::DecimalCalculation,
    },
    External {
        request: moosuri_core::ExternalToolRequest,
    },
    InvalidArguments,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PendingKnowledgeCall {
    pub call_id: String,
    pub input: KnowledgeToolInput,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct SourceExcerpt {
    pub source: SourceSummary,
    pub text: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub has_more: bool,
    pub next_start_byte: Option<u32>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolReadError {
    NotFound,
    InvalidInput,
    TooLargeNarrowRead,
    Unavailable,
    ArithmeticOverflow,
    OutcomeUnknown,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum KnowledgeToolResult {
    Search(KnowledgeSearch),
    Source(SourceExcerpt),
    Memory(MemoryDocument),
    Skill(moosuri_core::SkillDocument),
    Decimal { exact: String },
    External(serde_json::Value),
    Error(ToolReadError),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentStepStatus {
    Dispatched,
    Completed,
    Unknown,
    Purged,
}
pub struct AgentToolSlot {
    pub step: i64,
    pub call_id: String,
    pub input: KnowledgeToolInput,
    pub result: Option<KnowledgeToolResult>,
}
pub struct AgentContext {
    pub checkpoint_seq: i64,
    /// True only before any run-wide local tool execution or sealed target plan.
    /// Both fences survive retry, including private target-plan purging.
    pub memory_planning_allowed: bool,
    pub checkpoint: Option<PrivateCheckpoint>,
    pub current_step_status: Option<AgentStepStatus>,
    pub calls: Vec<AgentToolSlot>,
}
pub struct AgentClaim {
    pub lease: RunLease,
    pub binding: ProviderBinding,
    pub model: moosuri_core::ModelChoice,
    pub messages: Vec<ContextMessage>,
    pub context: AgentContext,
    pub tool_profile: Option<moosuri_core::RunToolProfile>,
}
/// One model dispatch capability. Its receipt nonce also permits late accounting,
/// even after the user deleted the content. It cannot be constructed by callers.
#[derive(Clone)]
pub struct StepLease {
    pub(super) run: RunLease,
    pub(super) step: i64,
    pub(super) nonce: [u8; 32],
}
impl StepLease {
    pub fn run_id(&self) -> &str {
        self.run.run_id()
    }
    pub fn attempt(&self) -> i32 {
        self.run.attempt()
    }
    pub fn step(&self) -> i64 {
        self.step
    }
}
fn encoded<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decoded<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn millis() -> Result<i64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StoreError::Unavailable)?
        .as_millis()
        .try_into()
        .map_err(|_| StoreError::Unavailable)
}
fn nonce() -> Result<[u8; 32]> {
    let mut value = [0; 32];
    OsRng
        .try_fill_bytes(&mut value)
        .map_err(|_| StoreError::Unavailable)?;
    Ok(value)
}
pub(super) fn is_agent(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(conn
        .query_row(
            "SELECT agent FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
            |r| r.get(0),
        )
        .optional()?
        .unwrap_or(false))
}
pub(super) fn evidence(
    conn: &Connection,
    tenant: &str,
    run: &str,
) -> Result<Vec<moosuri_core::ChatEvidence>> {
    let visible: bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND length(text)>0)",params![tenant,run],|r|r.get(0))?;
    if !visible {
        return Ok(vec![]);
    }
    let rows=conn.prepare("SELECT d.kind,d.object_id,d.revision,CASE d.kind WHEN 'source' THEN s.title ELSE json_extract(m.draft,'$.subject') END FROM agent_dependencies d LEFT JOIN knowledge_source_versions s ON d.kind='source' AND s.tenant_id=d.tenant_id AND s.source_id=d.object_id AND s.revision=d.revision LEFT JOIN knowledge_memory_versions m ON d.kind='memory' AND m.tenant_id=d.tenant_id AND m.memory_id=d.object_id AND m.revision=d.revision WHERE d.tenant_id=?1 AND d.run_id=?2 ORDER BY d.kind,d.object_id,d.revision LIMIT 12")?
      .query_map(params![tenant,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?,r.get::<_,Option<String>>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    rows.into_iter()
        .map(|(kind, id, revision, title)| {
            Ok(moosuri_core::ChatEvidence {
                kind: if kind == "source" {
                    moosuri_core::KnowledgeKind::Source
                } else {
                    moosuri_core::KnowledgeKind::Memory
                },
                id,
                revision,
                title: title.ok_or(StoreError::Corrupt)?,
            })
        })
        .collect()
}
type RetryBounds = (bool, i64, i64, Option<i64>, Option<i64>);
type ExecutionFence = (bool, Option<i64>, Option<i64>, Option<i64>);
pub(super) fn retry_allowed(conn: &Connection, tenant: &str, run: &str) -> Result<bool> {
    if conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM community_run_revocations WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(false);
    }
    let row: Option<RetryBounds> = conn.query_row("SELECT invalidated,remote_calls,tool_calls,deadline_ms,last_clock_ms FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let Some((invalid, remote, tools, deadline, last)) = row else {
        return Ok(true);
    };
    let clock = millis()?;
    Ok(!invalid
        && remote < 6
        && tools < 12
        && deadline.is_none_or(|d| clock < d)
        && last.is_none_or(|v| clock >= v))
}
pub(super) fn guard_execution(conn: &Connection, run: &Run) -> Result<()> {
    let row: Option<ExecutionFence>=conn.query_row("SELECT invalidated,generation,deadline_ms,last_clock_ms FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    if let Some((invalid, generation, deadline, last)) = row {
        let changed = match generation {
            Some(g) => g != knowledge::generation(conn, &run.tenant)?,
            None => false,
        };
        if invalid || changed {
            if !invalid {
                purge_runs(conn, &run.tenant, vec![run.id.clone()])?;
            }
            return Err(StoreError::Conflict);
        }
        let clock = millis()?;
        if deadline.is_some_and(|d| clock >= d) || last.is_some_and(|v| clock < v) {
            conn.execute("UPDATE agent_execution SET deadline_ms=min(coalesce(deadline_ms,?3),?3) WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,clock])?;
            chat::transition(
                conn,
                run,
                RunState::Incomplete,
                Some(RunFailure::BudgetExhausted),
            )?;
            return Err(StoreError::BudgetExhausted);
        }
        conn.execute("UPDATE agent_execution SET last_clock_ms=?3 WHERE tenant_id=?1 AND run_id=?2 AND deadline_ms IS NOT NULL",params![run.tenant,run.id,clock])?;
    }
    Ok(())
}
pub(super) fn reset_attempt(conn: &Connection, tenant: &str, run: &str) -> Result<()> {
    conn.execute(
        "DELETE FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run],
    )?;
    conn.execute("UPDATE agent_steps SET checkpoint=NULL,completion_digest=NULL,state=CASE state WHEN 'dispatched' THEN 'unknown' WHEN 'completed' THEN 'purged' ELSE state END WHERE tenant_id=?1 AND run_id=?2",params![tenant,run])?;
    let generation = knowledge::generation(conn, tenant)?;
    conn.execute("UPDATE agent_execution SET generation=CASE WHEN generation IS NULL THEN NULL ELSE ?3 END WHERE tenant_id=?1 AND run_id=?2",params![tenant,run,generation])?;
    Ok(())
}
pub(super) fn inherit_dependencies(
    conn: &Connection,
    tenant: &str,
    run: &str,
    prior: &str,
) -> Result<()> {
    let exists: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_dependencies WHERE tenant_id=?1 AND run_id=?2) OR EXISTS(SELECT 1 FROM mail_run_dependencies WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, prior],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let generation = knowledge::generation(conn, tenant)?;
    conn.execute("INSERT OR IGNORE INTO agent_execution(tenant_id,run_id,agent,generation) VALUES(?1,?2,0,?3)",params![tenant,run,generation])?;
    conn.execute("INSERT OR IGNORE INTO mail_run_dependencies SELECT tenant_id,?2,connection_id,epoch FROM mail_run_dependencies WHERE tenant_id=?1 AND run_id=?3",params![tenant,run,prior])?;
    conn.execute("INSERT OR IGNORE INTO agent_dependencies SELECT tenant_id,?2,kind,object_id,revision FROM agent_dependencies WHERE tenant_id=?1 AND run_id=?3",params![tenant,run,prior])?;
    Ok(())
}
/// Called before the underlying object/version plaintext is removed or replaced.
/// No plaintext is needed in the destructive journal to replay this purge.
pub(super) fn invalidate_object(
    conn: &Connection,
    tenant: &str,
    kind: &str,
    object: &str,
    revision: Option<i64>,
) -> Result<()> {
    let runs=conn.prepare("SELECT DISTINCT run_id FROM agent_dependencies WHERE tenant_id=?1 AND kind=?2 AND object_id=?3 AND (?4 IS NULL OR revision=?4)")?
      .query_map(params![tenant,kind,object,revision],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    purge_runs(conn, tenant, runs)
}
pub(super) fn invalidate_source(conn: &Connection, tenant: &str, source: &str) -> Result<()> {
    invalidate_object(conn, tenant, "source", source, None)?;
    let deps=conn.prepare("SELECT DISTINCT memory_id,memory_revision FROM knowledge_evidence WHERE tenant_id=?1 AND source_id=?2")?
      .query_map(params![tenant,source],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (memory, revision) in deps {
        invalidate_object(conn, tenant, "memory", &memory, Some(revision))?;
    }
    Ok(())
}
pub(super) fn purge_runs(conn: &Connection, tenant: &str, ids: Vec<String>) -> Result<()> {
    let mut threads = BTreeSet::new();
    for id in &ids {
        let run = chat::load_run(conn, tenant, id)?;
        crate::mail::purge_run(conn, tenant, id)?;
        threads.insert(run.thread);
        conn.execute(
            "DELETE FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, id],
        )?;
        conn.execute(
            "DELETE FROM memory_command_plans WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, id],
        )?;
        conn.execute(
            "DELETE FROM memory_target_plans WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, id],
        )?;
        conn.execute(
            "DELETE FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, id],
        )?;
        conn.execute("UPDATE agent_steps SET state='purged',checkpoint=NULL,completion_digest=NULL WHERE tenant_id=?1 AND run_id=?2",params![tenant,id])?;
        conn.execute(
            "UPDATE agent_execution SET invalidated=1 WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, id],
        )?;
        conn.execute("UPDATE chat_runs SET state='incomplete',error='knowledge_invalidated',version=version+1 WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
    }
    for thread in threads {
        // Even a client positioned exactly at the pre-purge head must resnapshot.
        conn.execute("UPDATE threads SET chat_event_seq=chat_event_seq+1,chat_reset_floor=chat_event_seq+1 WHERE tenant_id=?1 AND id=?2",params![tenant,thread])?;
        conn.execute(
            "DELETE FROM chat_events WHERE tenant_id=?1 AND thread_id=?2",
            params![tenant, thread],
        )?;
    }
    for id in ids {
        let run = chat::load_run(conn, tenant, &id)?;
        chat::emit(
            conn,
            &run,
            moosuri_core::ChatChange::State {
                run: chat::view(conn, &run)?,
            },
        )?;
    }
    Ok(())
}
fn context(conn: &Connection, tenant: &str, run: &str) -> Result<AgentContext> {
    let (seq, tool_calls, memory_target_started): (i64, i64, bool) = conn.query_row(
        "SELECT checkpoint_seq,tool_calls,memory_target_started FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
        params![tenant, run],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    let latest: Option<(i64,String,Option<Vec<u8>>)>=conn.query_row("SELECT step,state,checkpoint FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 ORDER BY step DESC LIMIT 1",params![tenant,run],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let mut calls = Vec::new();
    let (status, checkpoint) = if let Some((_step, state, checkpoint)) = latest {
        let status = match state.as_str() {
            "dispatched" => AgentStepStatus::Dispatched,
            "completed" => AgentStepStatus::Completed,
            "unknown" => AgentStepStatus::Unknown,
            "purged" => AgentStepStatus::Purged,
            _ => return Err(StoreError::Corrupt),
        };
        let completed_step: i64=conn.query_row("SELECT coalesce(max(step),0) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state='completed'",params![tenant,run],|r|r.get(0))?;
        let rows=conn.prepare("SELECT call_id,input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 ORDER BY call_id")?
          .query_map(params![tenant,run,completed_step],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (call_id, input, result) in rows {
            calls.push(AgentToolSlot {
                step: completed_step,
                call_id,
                input: decoded(&input)?,
                result: result.as_deref().map(decoded).transpose()?,
            });
        }
        // A just-dispatched step has no result yet: retain the preceding successful capsule.
        let checkpoint = if checkpoint.is_some() {
            checkpoint
        } else {
            conn.query_row("SELECT checkpoint FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND checkpoint IS NOT NULL ORDER BY step DESC LIMIT 1",params![tenant,run],|r|r.get::<_,Vec<u8>>(0)).optional()?
        };
        (
            Some(status),
            checkpoint.map(|bytes| PrivateCheckpoint { bytes }),
        )
    } else {
        (None, None)
    };
    Ok(AgentContext {
        checkpoint_seq: seq,
        memory_planning_allowed: tool_calls == 0 && !memory_target_started,
        checkpoint,
        current_step_status: status,
        calls,
    })
}
pub(crate) fn guard_step(conn: &Connection, key: &[u8; 32], lease: &StepLease) -> Result<Run> {
    let run = chat::guard(conn, key, &lease.run)?;
    let row: Option<(Vec<u8>,i32,String)>=conn.query_row("SELECT nonce,attempt,state FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,lease.step],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
    let Some((nonce, attempt, state)) = row else {
        return Err(StoreError::Conflict);
    };
    if nonce != lease.nonce || attempt != run.attempt {
        return Err(StoreError::Unauthorized);
    }
    let latest: i64 = conn.query_row(
        "SELECT remote_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",
        params![run.tenant, run.id],
        |r| r.get(0),
    )?;
    if latest != lease.step || !matches!(state.as_str(), "dispatched" | "completed") {
        return Err(StoreError::Conflict);
    }
    Ok(run)
}
fn validate_call(call: &PendingKnowledgeCall) -> Result<()> {
    if call.call_id.is_empty()
        || call.call_id.len() > 256
        || !call.call_id.bytes().all(|b| b.is_ascii_graphic())
    {
        return Err(StoreError::InvalidInput);
    }
    // Semantic parameter failures are persisted tool results, allowing a model
    // repair within the same budget. Only the wire/storage bound is structural.
    if encoded(&call.input)?.len() > TOOL_INPUT_LIMIT {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn add_dependency(conn: &Connection, run: &Run, kind: &str, id: &str, revision: i64) -> Result<()> {
    conn.execute(
        "INSERT OR IGNORE INTO agent_dependencies VALUES(?1,?2,?3,?4,?5)",
        params![run.tenant, run.id, kind, id, revision],
    )?;
    Ok(())
}
fn read_tool(
    conn: &Connection,
    run: &Run,
    input: &KnowledgeToolInput,
) -> Result<KnowledgeToolResult> {
    let read = match input {
        KnowledgeToolInput::SkillRead { pin } => Ok(skills::read(conn, run, pin)),
        KnowledgeToolInput::DecimalCalculate { calculation } => {
            if run
                .tool_profile
                .as_ref()
                .is_none_or(|profile| !matches!(profile.version, 1..=5))
            {
                Ok(KnowledgeToolResult::Error(ToolReadError::Unavailable))
            } else {
                Ok(match arithmetic::calculate(calculation) {
                    Ok(exact) => KnowledgeToolResult::Decimal { exact },
                    Err(error) => KnowledgeToolResult::Error(error),
                })
            }
        }
        KnowledgeToolInput::External {
            request:
                moosuri_core::ExternalToolRequest::Mail(moosuri_core::MailToolRequest::Draft { input }),
        } => {
            let user = chat::authority(conn, run)?;
            let allowed = run
                .tool_profile
                .as_ref()
                .filter(|p| matches!(p.version, 2..=5))
                .and_then(|p| {
                    p.mail_connections
                        .iter()
                        .find(|p| p.id == input.connection_id)
                });
            if allowed.is_some_and(|p| {
                crate::mail::connection(conn, &user, &p.id)
                    .is_ok_and(|c| c.epoch == p.epoch && c.status == "active")
            }) {
                let mut input = input.clone();
                input.origin_thread_id = Some(run.thread.clone());
                let pin = allowed.ok_or(StoreError::Conflict)?;
                conn.execute(
                    "INSERT OR IGNORE INTO mail_run_dependencies VALUES(?1,?2,?3,?4)",
                    params![run.tenant, run.id, pin.id, pin.epoch],
                )?;
                let a = crate::mail::draft(conn, &user, input, Some(&run.id), None)?;
                Ok(KnowledgeToolResult::External(
                    serde_json::json!({"action_id":a.id,"state":a.state,"local_draft":true,"review_url":"/settings/mail","requires_user_approval":true}),
                ))
            } else {
                Ok(KnowledgeToolResult::Error(ToolReadError::Unavailable))
            }
        }
        KnowledgeToolInput::External { .. } => {
            Ok(KnowledgeToolResult::Error(ToolReadError::Unavailable))
        }
        KnowledgeToolInput::InvalidArguments => {
            Ok(KnowledgeToolResult::Error(ToolReadError::InvalidInput))
        }
        KnowledgeToolInput::ReadSource { max_bytes, .. }
            if *max_bytes == 0 || *max_bytes > 4096 =>
        {
            Err(StoreError::InvalidInput)
        }
        KnowledgeToolInput::Search { query } => {
            // Authority and outer run-generation fences remain execution errors.
            // A cursor reused with different search fields is a repairable tool
            // input mistake, not evidence that the user's authority was revoked.
            let user = chat::authority(conn, run)?;
            knowledge::search::search_limit(conn, user, query.clone(), 5)
                .map(KnowledgeToolResult::Search)
                .map_err(|error| match error {
                    StoreError::Conflict if query.cursor.is_some() => StoreError::InvalidInput,
                    other => other,
                })
        }
        KnowledgeToolInput::ReadSource {
            id,
            revision,
            start_byte,
            max_bytes,
        } => knowledge::sources::document(conn, &run.tenant, id, Some(*revision)).and_then(
            |document| {
                let start = *start_byte as usize;
                if start > document.text.len() || !document.text.is_char_boundary(start) {
                    return Err(StoreError::InvalidInput);
                }
                let mut end = start
                    .saturating_add(*max_bytes as usize)
                    .min(document.text.len());
                while !document.text.is_char_boundary(end) {
                    end -= 1;
                }
                if end == start && start < document.text.len() {
                    return Err(StoreError::InvalidInput);
                }
                let text = document.text[start..end].to_owned();
                let has_more = end < document.text.len();
                Ok(KnowledgeToolResult::Source(SourceExcerpt {
                    source: document.source,
                    text,
                    start_byte: *start_byte,
                    end_byte: end as u32,
                    has_more,
                    next_start_byte: has_more.then_some(end as u32),
                }))
            },
        ),
        KnowledgeToolInput::ReadMemory { id, revision } => {
            knowledge::memories::document(conn, &run.tenant, id, Some(*revision))
                .map(KnowledgeToolResult::Memory)
        }
    };
    let mut result = match read {
        Ok(value) => value,
        Err(StoreError::NotFound) => KnowledgeToolResult::Error(ToolReadError::NotFound),
        Err(StoreError::InvalidInput) => KnowledgeToolResult::Error(ToolReadError::InvalidInput),
        Err(error) => return Err(error),
    };
    if let KnowledgeToolResult::Search(page) = &mut result {
        // Search validates each continuation against its tenant, query, time
        // and generation. Preserve that actual cursor within the existing run
        // budgets; byte truncation below cannot skip undisclosed hits.
        while encoded(page)?.len() + 64 > TOOL_RESULT_LIMIT && !page.hits.is_empty() {
            page.hits.pop();
            page.truncated = true;
            page.next_cursor = None;
        }
    }
    if encoded(&result)?.len() > TOOL_RESULT_LIMIT {
        result = KnowledgeToolResult::Error(ToolReadError::TooLargeNarrowRead)
    }
    // Only payload that actually fits and is returned participates in provenance.
    match &result {
        KnowledgeToolResult::Source(v) => {
            add_dependency(conn, run, "source", &v.source.id, v.source.revision)?
        }
        KnowledgeToolResult::Memory(v) => {
            add_dependency(conn, run, "memory", &v.id, v.revision)?;
            for evidence in &v.draft.evidence {
                add_dependency(
                    conn,
                    run,
                    "source",
                    &evidence.source_id,
                    evidence.source_revision,
                )?;
            }
        }
        KnowledgeToolResult::Search(v) => {
            for hit in &v.hits {
                add_dependency(
                    conn,
                    run,
                    if hit.kind == moosuri_core::KnowledgeKind::Source {
                        "source"
                    } else {
                        "memory"
                    },
                    &hit.id,
                    hit.revision,
                )?;
                if hit.kind == moosuri_core::KnowledgeKind::Memory {
                    // Delivered assertions inherit their stored evidence, even when the
                    // search snippet itself does not include the evidence quote.
                    let evidence=conn.prepare("SELECT source_id,source_revision FROM knowledge_evidence WHERE tenant_id=?1 AND memory_id=?2 AND memory_revision=?3")?
                  .query_map(params![run.tenant,hit.id,hit.revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
                    for (id, revision) in evidence {
                        add_dependency(conn, run, "source", &id, revision)?;
                    }
                }
            }
        }
        KnowledgeToolResult::External(_)
        | KnowledgeToolResult::Error(_)
        | KnowledgeToolResult::Skill(_)
        | KnowledgeToolResult::Decimal { .. } => {}
    }
    Ok(result)
}

impl Store {
    pub async fn claim_agent_run(&self) -> Result<Option<AgentClaim>> {
        self.call(|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let running: i64=tx.query_row("SELECT count(*) FROM chat_runs WHERE state='running'",[],|r|r.get(0))?;
            if running>=2 {return Ok(None)}
            let queued=tx.prepare("SELECT tenant_id,id FROM chat_runs WHERE state='queued' AND tenant_id NOT IN(SELECT tenant_id FROM chat_runs WHERE state='running') ORDER BY admitted_order LIMIT 64")?
              .query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
            for (tenant,id) in queued {
                let run=chat::load_run(&tx,&tenant,&id)?;
                if chat::authority(&tx,&run).is_err(){chat::transition(&tx,&run,RunState::Cancelled,Some(RunFailure::AuthorityRevoked))?;continue}
                let binding=match providers::binding_for_choice(&tx,&db.provider_key,&run.model){Ok(v)=>v,Err(_)=>{chat::transition(&tx,&run,RunState::Failed,Some(RunFailure::ProviderUnavailable))?;continue}};
                if !retry_allowed(&tx,&tenant,&id)? {chat::transition(&tx,&run,RunState::Incomplete,Some(RunFailure::BudgetExhausted))?;continue}
                match guard_execution(&tx,&run) {Ok(())=>{},Err(StoreError::Conflict|StoreError::BudgetExhausted)=>continue,Err(e)=>return Err(e)}
                let messages=chat::context(&tx,&run)?;
                tx.execute("INSERT INTO agent_execution(tenant_id,run_id,agent) VALUES(?1,?2,1) ON CONFLICT(tenant_id,run_id) DO UPDATE SET agent=1",params![tenant,id])?;
                let legacy:(i64,Option<i64>)=tx.query_row("SELECT count(*),min(dispatched_at)*1000 FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=0",params![tenant,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
                if legacy.0>=6 {tx.execute("UPDATE agent_execution SET remote_calls=6 WHERE tenant_id=?1 AND run_id=?2",params![tenant,id])?;chat::transition(&tx,&run,RunState::Incomplete,Some(RunFailure::BudgetExhausted))?;continue}
                tx.execute("UPDATE agent_execution SET remote_calls=max(remote_calls,?3),started_ms=coalesce(started_ms,?4),deadline_ms=coalesce(deadline_ms,?4+120000) WHERE tenant_id=?1 AND run_id=?2",params![tenant,id,legacy.0,legacy.1])?;
                match guard_execution(&tx,&run) {Ok(())=>{},Err(StoreError::Conflict|StoreError::BudgetExhausted)=>continue,Err(e)=>return Err(e)}
                // Explicit retry discarded prior private continuations. Retain
                // old dispatch intents as unknown, never re-authorize them.
                tx.execute("UPDATE agent_steps SET state='unknown' WHERE tenant_id=?1 AND run_id=?2 AND attempt<?3 AND state='dispatched'",params![tenant,id,run.attempt])?;
                let n=nonce()?;
                tx.execute("INSERT INTO agent_run_leases VALUES(?1,?2,?3,?4)",params![tenant,id,run.attempt,n.as_slice()])?;
                chat::transition(&tx,&run,RunState::Running,None)?;
                let lease=RunLease{tenant:tenant.clone(),run:id.clone(),attempt:run.attempt,nonce:n,epoch:run.epoch};
                let context=context(&tx,&tenant,&id)?;
                tx.commit()?;
                return Ok(Some(AgentClaim{lease,binding,model:run.model,messages,context,tool_profile:run.tool_profile}));
            }
            tx.commit()?;Ok(None)
        }).await
    }
    pub async fn agent_context(&self, lease: &RunLease) -> Result<AgentContext> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let value = chat::guard(&tx, &db.provider_key, &lease)
                .and_then(|run| context(&tx, &run.tenant, &run.id));
            tx.commit()?;
            value
        })
        .await
    }
    pub async fn begin_agent_step(
        &self,
        lease: &RunLease,
        expected_checkpoint_seq: i64,
    ) -> Result<StepLease> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match chat::guard(&tx,&db.provider_key,&lease){Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            if !is_agent(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
            if let Err(e)=memory_commands::guard_target_dispatch(&tx,&run) {tx.commit()?;return Err(e)}
            if memory_commands::has_slot(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
            let (seq,remote):(i64,i64)=tx.query_row("SELECT checkpoint_seq,remote_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if seq!=expected_checkpoint_seq {return Err(StoreError::Conflict)}
            if remote>=6 {return Err(StoreError::BudgetExhausted)}
            let pending: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND result IS NULL)",params![run.tenant,run.id],|r|r.get(0))?;
            let dispatched: bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state='dispatched')",params![run.tenant,run.id],|r|r.get(0))?;
            if pending || dispatched {return Err(StoreError::Conflict)}
            let clock=millis()?;let step=remote+1;let n=nonce()?;
            tx.execute("UPDATE agent_execution SET remote_calls=?3,checkpoint_seq=checkpoint_seq+1,started_ms=coalesce(started_ms,?4),deadline_ms=coalesce(deadline_ms,?4+120000),last_clock_ms=?4 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,step,clock])?;
            tx.execute("INSERT INTO agent_steps(tenant_id,run_id,step,attempt,nonce,state) VALUES(?1,?2,?3,?4,?5,'dispatched')",params![run.tenant,run.id,step,run.attempt,n.as_slice()])?;
            tx.execute("INSERT INTO model_call_receipts(tenant_id,account_id,run_id,attempt,step,nonce,usage,dispatched_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8)",params![run.tenant,run.account,run.id,run.attempt,step,n.as_slice(),encoded(&RunUsage::default())?,now()?])?;
            tx.commit()?;Ok(StepLease{run:lease,step,nonce:n})
        }).await
    }
    pub async fn commit_agent_step(
        &self,
        lease: &StepLease,
        checkpoint: PrivateCheckpoint,
        calls: Vec<PendingKnowledgeCall>,
    ) -> Result<AgentContext> {
        let lease = lease.clone();
        self.call(move|db|{
            if checkpoint.bytes.len()>CHECKPOINT_LIMIT || serde_json::from_slice::<serde_json::Value>(&checkpoint.bytes).is_err() {return Err(StoreError::InvalidInput)}
            if calls.len()>12 {return Err(StoreError::BudgetExhausted)}
            let mut ids=BTreeSet::new();for call in &calls {validate_call(call)?;if !ids.insert(&call.call_id){return Err(StoreError::InvalidInput)}}
            let digest: [u8;32]=Sha256::digest(encoded(&(&checkpoint.bytes,&calls))?.as_bytes()).into();
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match guard_step(&tx,&db.provider_key,&lease){Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            if !calls.is_empty() && memory_commands::has_target_plan(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
            let (state,old):(String,Option<Vec<u8>>)=tx.query_row("SELECT state,completion_digest FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,lease.step],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if state=="completed" {
                if old.as_deref()!=Some(digest.as_slice()){return Err(StoreError::Conflict)}
            }else{
                let used: i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
                if used+calls.len() as i64>12{return Err(StoreError::BudgetExhausted)}
                tx.execute("UPDATE agent_steps SET state='completed',completion_digest=?4,checkpoint=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,lease.step,digest.as_slice(),checkpoint.bytes])?;
                for call in calls {tx.execute("INSERT INTO agent_tool_slots(tenant_id,run_id,step,call_id,input) VALUES(?1,?2,?3,?4,?5)",params![run.tenant,run.id,lease.step,call.call_id,encoded(&call.input)?])?;}
                tx.execute("UPDATE agent_execution SET checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
            }
            let value=context(&tx,&run.tenant,&run.id)?;tx.commit()?;Ok(value)
        }).await
    }
    pub async fn execute_knowledge_tool(
        &self,
        lease: &StepLease,
        call_id: String,
    ) -> Result<KnowledgeToolResult> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match guard_step(&tx,&db.provider_key,&lease){Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            if memory_commands::has_slot(&tx,&run.tenant,&run.id)? || memory_commands::has_target_plan(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
            let state: String=tx.query_row("SELECT state FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,lease.step],|r|r.get(0))?;
            if state!="completed" {return Err(StoreError::Conflict)}
            let (input,saved):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
            if let Some(saved)=saved {let result=decoded(&saved)?;tx.commit()?;return Ok(result)}
            let used: i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
            if used>=12 {return Err(StoreError::BudgetExhausted)}
            let input=decoded(&input)?;let result=read_tool(&tx,&run,&input)?;
            let generation=knowledge::generation(&tx,&run.tenant)?;
            tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1,generation=?3 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,generation])?;
            tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id,encoded(&result)?])?;
            tx.commit()?;Ok(result)
        }).await
    }
    pub async fn append_agent_output(
        &self,
        lease: &StepLease,
        patches: Vec<OutputPatch>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            if patches.is_empty() || patches.len() > 128 {
                return Err(StoreError::InvalidInput);
            }
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match guard_step(&tx, &db.provider_key, &lease) {
                Ok(v) => v,
                Err(e) => {
                    tx.commit()?;
                    return Err(e);
                }
            };
            if memory_commands::has_slot(&tx, &run.tenant, &run.id)?
                || memory_commands::has_target_plan(&tx, &run.tenant, &run.id)?
            {
                return Err(StoreError::Conflict);
            }
            chat::apply_patches(&tx, &run, patches)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    /// Settle a supervisor failure before the first step or between steps.
    /// Successful completion requires finish_agent_run and a completed no-call step.
    pub async fn finish_agent_execution(
        &self,
        lease: &RunLease,
        state: RunState,
        error: Option<RunFailure>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            if state.active() || state==RunState::Completed || (state==RunState::OutcomeUnknown && !matches!(error,Some(RunFailure::Interrupted|RunFailure::TimedOut|RunFailure::ProtocolFailed|RunFailure::ConnectionFailed))) {return Err(StoreError::InvalidInput)}
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match chat::guard(&tx,&db.provider_key,&lease){Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            if !is_agent(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
            tx.execute("UPDATE agent_steps SET state='unknown' WHERE tenant_id=?1 AND run_id=?2 AND state='dispatched'",params![run.tenant,run.id])?;
            chat::transition(&tx,&run,state,error)?;tx.commit()?;Ok(())
        }).await
    }
    pub async fn finish_agent_run(
        &self,
        lease: &StepLease,
        state: RunState,
        error: Option<RunFailure>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            if state.active() || (state==RunState::OutcomeUnknown && !matches!(error,Some(RunFailure::Interrupted|RunFailure::TimedOut|RunFailure::ProtocolFailed|RunFailure::ConnectionFailed))) {return Err(StoreError::InvalidInput)}
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match guard_step(&tx,&db.provider_key,&lease){Ok(v)=>v,Err(e)=>{tx.commit()?;return Err(e)}};
            if state==RunState::Completed {
                if memory_commands::has_slot(&tx,&run.tenant,&run.id)? || memory_commands::has_target_plan(&tx,&run.tenant,&run.id)? {return Err(StoreError::Conflict)}
                let complete: bool=tx.query_row("SELECT state='completed' AND NOT EXISTS(SELECT 1 FROM agent_tool_slots t WHERE t.tenant_id=agent_steps.tenant_id AND t.run_id=agent_steps.run_id AND t.step=agent_steps.step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,lease.step],|r|r.get(0))?;
                if !complete {return Err(StoreError::Conflict)}
            }
            if state!=RunState::Completed {tx.execute("UPDATE agent_steps SET state='unknown' WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='dispatched'",params![run.tenant,run.id,lease.step])?;}
            chat::transition(&tx,&run,state,error)?;tx.commit()?;Ok(())
        }).await
    }
    pub async fn update_agent_step_usage(&self, lease: &StepLease, usage: RunUsage) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let row: Option<(Vec<u8>,String)>=tx.query_row("SELECT nonce,usage FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=?4",params![lease.run.tenant,lease.run.run,lease.run.attempt,lease.step],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
            let Some((nonce,value))=row else{return Err(StoreError::Unauthorized)};
            if nonce!=lease.nonce {return Err(StoreError::Unauthorized)}
            let old: RunUsage=decoded(&value)?;
            let merged=RunUsage{input_tokens:usage.input_tokens.or(old.input_tokens),output_tokens:usage.output_tokens.or(old.output_tokens),cache_read_tokens:usage.cache_read_tokens.or(old.cache_read_tokens),cache_write_tokens:usage.cache_write_tokens.or(old.cache_write_tokens)};
            if old!=merged {
                tx.execute("UPDATE model_call_receipts SET usage=?5 WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=?4",params![lease.run.tenant,lease.run.run,lease.run.attempt,lease.step,encoded(&merged)?])?;
                tx.execute("UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",params![lease.run.tenant,lease.run.run])?;
                if let Ok(run)=chat::load_run(&tx,&lease.run.tenant,&lease.run.run) {chat::emit(&tx,&run,moosuri_core::ChatChange::State{run:chat::view(&tx,&run)?})?;}
            }
            tx.commit()?;Ok(())
        }).await
    }
}

#[cfg(test)]
mod tests;
