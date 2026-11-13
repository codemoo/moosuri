//! Durable, tenant-scoped text turns and opaque dispatch capabilities. No model IO.
use super::*;
use moosuri_core::{
    AccountUsage, AccountUsageResponse, AssistantMessage, ChatChange, ChatEvent, ChatEvents,
    ChatPage, ChatTurn, MessagePhase, ModelChoice, RunState, RunUsage, RunView, UsageSummary,
};
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ContextRole {
    System,
    User,
    Assistant,
}
pub struct ContextMessage {
    pub role: ContextRole,
    pub text: String,
    pub phase: Option<MessagePhase>,
}
pub enum OutputPatch {
    Append {
        item: String,
        text: String,
        phase: Option<MessagePhase>,
        refusal: bool,
    },
    Phase {
        item: String,
        phase: Option<MessagePhase>,
    },
}
#[derive(Clone, Copy)]
pub enum RunFailure {
    ProviderUnavailable,
    CredentialsRejected,
    RateLimited,
    TimedOut,
    ProtocolFailed,
    ConnectionFailed,
    ContextLimit,
    OutputLimit,
    IdentityUnverified,
    AuthorityRevoked,
    ServerRestarted,
    ContinuationUnsupported,
    Interrupted,
    KnowledgeInvalidated,
    BudgetExhausted,
}
impl RunFailure {
    fn code(self) -> &'static str {
        match self {
            Self::ProviderUnavailable => "provider_unavailable",
            Self::CredentialsRejected => "credentials_rejected",
            Self::RateLimited => "rate_limited",
            Self::TimedOut => "timed_out",
            Self::ProtocolFailed => "protocol_failed",
            Self::ConnectionFailed => "connection_failed",
            Self::ContextLimit => "context_limit",
            Self::OutputLimit => "output_limit",
            Self::IdentityUnverified => "identity_unverified",
            Self::AuthorityRevoked => "authority_revoked",
            Self::ServerRestarted => "server_restarted",
            Self::ContinuationUnsupported => "continuation_unsupported",
            Self::Interrupted => "interrupted",
            Self::KnowledgeInvalidated => "knowledge_invalidated",
            Self::BudgetExhausted => "budget_exhausted",
        }
    }
}
/// Unforgeable outside Store. Receipt nonce authorizes usage after content deletion.
#[derive(Clone)]
pub struct RunLease {
    pub(super) tenant: String,
    pub(super) run: String,
    pub(super) attempt: i32,
    pub(super) nonce: [u8; 32],
    pub(super) epoch: i64,
}
impl RunLease {
    pub fn run_id(&self) -> &str {
        &self.run
    }
    pub fn attempt(&self) -> i32 {
        self.attempt
    }
}
pub struct ClaimedRun {
    pub lease: RunLease,
    pub binding: ProviderBinding,
    pub messages: Vec<ContextMessage>,
}
#[derive(Clone)]
pub(super) struct Run {
    pub(super) tenant: String,
    pub(super) id: String,
    pub(super) account: String,
    pub(super) thread: String,
    pub(super) request: String,
    pub(super) sequence: i64,
    pub(super) text: String,
    pub(super) model: ModelChoice,
    pub(super) tool_profile: Option<moosuri_core::RunToolProfile>,
    pub(super) session: [u8; 32],
    pub(super) epoch: i64,
    pub(super) attempt: i32,
    pub(super) state: RunState,
    pub(super) version: i64,
    pub(super) error: Option<String>,
    pub(super) created: i64,
}
#[derive(serde::Serialize, serde::Deserialize)]
struct StoredRunChoice {
    #[serde(flatten)]
    model: ModelChoice,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    agent_profile: Option<moosuri_core::RunToolProfile>,
}
fn json<T: serde::Serialize>(value: &T) -> Result<String> {
    serde_json::to_string(value).map_err(|_| StoreError::Corrupt)
}
fn parse<T: serde::de::DeserializeOwned>(value: &str) -> Result<T> {
    serde_json::from_str(value).map_err(|_| StoreError::Corrupt)
}
fn state_name(state: RunState) -> Result<String> {
    Ok(json(&state)?.trim_matches('"').to_owned())
}
fn phase_name(phase: Option<MessagePhase>) -> Result<Option<String>> {
    phase
        .map(|p| Ok(json(&p)?.trim_matches('"').to_owned()))
        .transpose()
}
fn phase_value(phase: Option<String>) -> Result<Option<MessagePhase>> {
    phase.map(|p| parse(&format!("\"{p}\""))).transpose()
}
fn thread_exists(conn: &Connection, user: &UserContext, thread: &str) -> Result<()> {
    if !conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND id=?2 AND mission_internal=0)",
        params![user.tenant.0, thread],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::NotFound);
    }
    Ok(())
}
type RunRow = (
    String,
    String,
    String,
    String,
    String,
    i64,
    String,
    String,
    Vec<u8>,
    i64,
    i32,
    String,
    i64,
    Option<String>,
    i64,
);
fn run_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RunRow> {
    Ok((
        r.get(0)?,
        r.get(1)?,
        r.get(2)?,
        r.get(3)?,
        r.get(4)?,
        r.get(5)?,
        r.get(6)?,
        r.get(7)?,
        r.get(8)?,
        r.get(9)?,
        r.get(10)?,
        r.get(11)?,
        r.get(12)?,
        r.get(13)?,
        r.get(14)?,
    ))
}
pub(super) fn load_run(conn: &Connection, tenant: &str, id: &str) -> Result<Run> {
    let r=conn.query_row("SELECT tenant_id,id,account_id,thread_id,request_id,sequence,user_text,model,session_digest,auth_epoch,attempt,state,version,error,created_at FROM chat_runs WHERE tenant_id=?1 AND id=?2",params![tenant,id],run_row).optional()?.ok_or(StoreError::NotFound)?;
    let choice: StoredRunChoice = parse(&r.7)?;
    Ok(Run {
        tenant: r.0,
        id: r.1,
        account: r.2,
        thread: r.3,
        request: r.4,
        sequence: r.5,
        text: r.6,
        model: choice.model,
        tool_profile: choice.agent_profile,
        session: r.8.try_into().map_err(|_| StoreError::Corrupt)?,
        epoch: r.9,
        attempt: r.10,
        state: parse(&format!("\"{}\"", r.11))?,
        version: r.12,
        error: r.13,
        created: r.14,
    })
}
fn run_usage(conn: &Connection, run: &Run) -> Result<RunUsage> {
    let all_attempts = agent_execution::is_agent(conn, &run.tenant, &run.id)?;
    let values = conn.prepare("SELECT usage FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND (?4 OR attempt=?3)")?
      .query_map(params![run.tenant,run.id,run.attempt,all_attempts], |r| r.get::<_,String>(0))?
      .collect::<std::result::Result<Vec<_>,_>>()?;
    let values = values
        .iter()
        .map(|v| parse::<RunUsage>(v))
        .collect::<Result<Vec<_>>>()?;
    fn sum(values: &[RunUsage], field: fn(&RunUsage) -> Option<u64>) -> Result<Option<u64>> {
        if values.is_empty() || values.iter().any(|v| field(v).is_none()) {
            return Ok(None);
        }
        let mut total = 0u64;
        for v in values {
            total = total
                .checked_add(field(v).ok_or(StoreError::Corrupt)?)
                .ok_or(StoreError::Corrupt)?;
        }
        Ok(Some(total))
    }
    Ok(RunUsage {
        input_tokens: sum(&values, |v| v.input_tokens)?,
        output_tokens: sum(&values, |v| v.output_tokens)?,
        cache_read_tokens: sum(&values, |v| v.cache_read_tokens)?,
        cache_write_tokens: sum(&values, |v| v.cache_write_tokens)?,
    })
}

fn is_latest(conn: &Connection, run: &Run) -> Result<bool> {
    Ok(!conn.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND sequence>?3)",params![run.tenant,run.thread,run.sequence],|r|r.get::<_,bool>(0))?)
}
pub(super) fn view(conn: &Connection, run: &Run) -> Result<RunView> {
    Ok(RunView {
        id: run.id.clone(),
        request_id: run.request.clone(),
        thread_id: run.thread.clone(),
        attempt: run.attempt,
        state: run.state,
        version: run.version,
        model: run.model.clone(),
        error: run.error.clone(),
        usage: run_usage(conn, run)?,
        evidence: agent_execution::evidence(conn, &run.tenant, &run.id)?,
        web_evidence: {
            let refs = crate::web::evidence(conn, &run.tenant, &run.id)?;
            (!refs.is_empty()).then_some(refs)
        },
        artifact_receipts: {
            let r = crate::artifacts::agent::receipts(conn, &run.tenant, &run.id)?;
            (!r.is_empty()).then_some(r)
        },
        schedule_receipts: {
            let r = crate::schedules::chat::receipts(conn, &run.tenant, &run.id)?;
            (!r.is_empty()).then_some(r)
        },
        can_retry: !run.state.active()
            && run.state != RunState::Completed
            && run.attempt < 32
            && agent_execution::retry_allowed(conn, &run.tenant, &run.id)?
            && is_latest(conn, run)?,
        can_cancel: run.state.active(),
        created_at: run.created,
    })
}
pub(super) fn turn(conn: &Connection, run: &Run) -> Result<ChatTurn> {
    let mut statement=conn.prepare("SELECT id,text,phase,refusal FROM chat_output WHERE tenant_id=?1 AND run_id=?2 ORDER BY ordinal")?;
    let rows = statement.query_map(params![run.tenant, run.id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, Option<String>>(2)?,
            r.get::<_, bool>(3)?,
        ))
    })?;
    let assistant = rows
        .map(|r| {
            let (id, text, phase, refusal) = r?;
            Ok(AssistantMessage {
                id,
                text,
                phase: phase_value(phase)?,
                refusal,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(ChatTurn {
        attachments: crate::media::turn_media(conn, &run.tenant, &run.id)?,
        sequence: run.sequence,
        user_text: run.text.clone(),
        assistant,
        run: view(conn, run)?,
    })
}
pub(super) fn emit(conn: &Connection, run: &Run, change: ChatChange) -> Result<()> {
    conn.execute("UPDATE threads SET chat_event_seq=chat_event_seq+1,updated_at=?1 WHERE tenant_id=?2 AND id=?3",params![now()?,run.tenant,run.thread])?;
    let sequence: i64 = conn.query_row(
        "SELECT chat_event_seq FROM threads WHERE tenant_id=?1 AND id=?2",
        params![run.tenant, run.thread],
        |r| r.get(0),
    )?;
    conn.execute(
        "INSERT INTO chat_events(tenant_id,thread_id,sequence,change) VALUES (?1,?2,?3,?4)",
        params![run.tenant, run.thread, sequence, json(&change)?],
    )?;
    conn.execute(
        "DELETE FROM chat_events WHERE tenant_id=?1 AND thread_id=?2 AND sequence<=?3",
        params![run.tenant, run.thread, sequence - 512],
    )?;
    Ok(())
}
pub(super) fn transition(
    conn: &Connection,
    run: &Run,
    state: RunState,
    error: Option<RunFailure>,
) -> Result<()> {
    conn.execute(
        "UPDATE chat_runs SET state=?1,error=?2,version=version+1 WHERE tenant_id=?3 AND id=?4",
        params![
            state_name(state)?,
            error.map(RunFailure::code),
            run.tenant,
            run.id
        ],
    )?;
    crate::missions::carrier_transition(conn, run, state)?;
    let current = load_run(conn, &run.tenant, &run.id)?;
    emit(
        conn,
        &current,
        ChatChange::State {
            run: view(conn, &current)?,
        },
    )
}
pub(super) fn authority(conn: &Connection, run: &Run) -> Result<UserContext> {
    let user = load_context(conn, run.session, false)?;
    if user.epoch != run.epoch
        || user.restricted
        || user.tenant.0 != run.tenant
        || user.account.id != run.account
    {
        return Err(StoreError::Unauthorized);
    }
    Ok(user)
}
pub(super) fn execution_authority(conn: &Connection, run: &Run) -> Result<()> {
    if crate::missions::is_mission(conn, &run.tenant, &run.id)? {
        crate::missions::authority(conn, run)
    } else {
        authority(conn, run).map(|_| ())
    }
}
pub(super) fn queue_capacity(conn: &Connection, tenant: &str) -> Result<()> {
    let (personal,global):(i64,i64)=conn.query_row("SELECT sum(CASE WHEN tenant_id=?1 THEN 1 ELSE 0 END),count(*) FROM chat_runs WHERE state='queued'",[tenant],|r|Ok((r.get::<_,Option<i64>>(0)?.unwrap_or(0),r.get(1)?)))?;
    if personal >= 8 || global >= 64 {
        return Err(StoreError::Busy);
    }
    Ok(())
}
fn next_order(conn: &Connection) -> Result<i64> {
    conn.execute(
        "UPDATE chat_admission_counter SET value=value+1 WHERE singleton=1",
        [],
    )?;
    Ok(conn.query_row(
        "SELECT value FROM chat_admission_counter WHERE singleton=1",
        [],
        |r| r.get(0),
    )?)
}
pub(super) fn receipt_valid(conn: &Connection, lease: &RunLease) -> Result<()> {
    let nonce: Option<Vec<u8>> = conn
        .query_row(
            "SELECT nonce FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=0",
            params![lease.tenant, lease.run, lease.attempt],
            |r| r.get(0),
        )
        .optional()?;
    if nonce.as_deref() != Some(lease.nonce.as_slice()) {
        let agent: Option<Vec<u8>> = conn.query_row("SELECT nonce FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3",params![lease.tenant,lease.run,lease.attempt],|r|r.get(0)).optional()?;
        if agent.as_deref() != Some(lease.nonce.as_slice()) {
            return Err(StoreError::Unauthorized);
        }
    }
    Ok(())
}
// Caller commits even when this returns an error: authority revocation is a durable state change.
pub(super) fn guard(conn: &Connection, key: &[u8; 32], lease: &RunLease) -> Result<Run> {
    receipt_valid(conn, lease)?;
    let run = load_run(conn, &lease.tenant, &lease.run)?;
    if run.attempt != lease.attempt || run.state != RunState::Running || run.epoch != lease.epoch {
        return Err(StoreError::Conflict);
    }
    // Expiry is not a causal owner cancellation. Classify only this validated
    // current mission/run before the generic authority-revocation transition.
    if crate::missions::is_mission(conn, &run.tenant, &run.id)?
        && crate::missions::expire_run(conn, &run)?
    {
        return Err(StoreError::BudgetExhausted);
    }
    if execution_authority(conn, &run).is_err() {
        transition(
            conn,
            &run,
            RunState::Cancelled,
            Some(RunFailure::AuthorityRevoked),
        )?;
        return Err(StoreError::Unauthorized);
    }
    if providers::binding_for_choice(conn, key, &run.model).is_err() {
        transition(
            conn,
            &run,
            RunState::Failed,
            Some(RunFailure::ProviderUnavailable),
        )?;
        return Err(StoreError::Conflict);
    }
    agent_execution::guard_execution(conn, &run)?;
    Ok(run)
}
impl Store {
    pub async fn submit_turn(
        &self,
        user: &UserContext,
        thread_id: String,
        request_id: String,
        text: String,
    ) -> Result<ChatTurn> {
        let user = user.clone();
        self.call(move |db| submit_turn_inner(db, user, thread_id, request_id, text))
            .await
    }
    pub async fn conversation(
        &self,
        user: &UserContext,
        thread_id: String,
        before: Option<i64>,
    ) -> Result<ChatPage> {
        let user = user.clone();
        self.call(move |db| conversation_inner(db, user, thread_id, before))
            .await
    }
    pub async fn chat_events(
        &self,
        user: &UserContext,
        thread_id: String,
        after: i64,
    ) -> Result<ChatEvents> {
        let user = user.clone();
        self.call(move |db| chat_events_inner(db, user, thread_id, after))
            .await
    }
    pub async fn cancel_run(
        &self,
        user: &UserContext,
        run_id: String,
        expected_attempt: i32,
    ) -> Result<ChatTurn> {
        let user = user.clone();
        self.call(move |db| cancel_run_inner(db, user, run_id, expected_attempt))
            .await
    }
    pub async fn retry_run(
        &self,
        user: &UserContext,
        run_id: String,
        expected_attempt: i32,
    ) -> Result<ChatTurn> {
        let user = user.clone();
        self.call(move |db| retry_run_inner(db, user, run_id, expected_attempt))
            .await
    }
    pub async fn claim_run(&self) -> Result<Option<ClaimedRun>> {
        self.call(claim_run_inner).await
    }
    pub async fn check_run(&self, lease: &RunLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let result = guard(&tx, &db.provider_key, &lease).map(|_| ());
            tx.commit()?;
            result
        })
        .await
    }
    pub async fn finish_run(
        &self,
        lease: &RunLease,
        state: RunState,
        error: Option<RunFailure>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            if state.active()
                || (state == RunState::OutcomeUnknown
                    && !matches!(
                        error,
                        Some(
                            RunFailure::Interrupted
                                | RunFailure::TimedOut
                                | RunFailure::ProtocolFailed
                                | RunFailure::ConnectionFailed
                        )
                    ))
            {
                return Err(StoreError::InvalidInput);
            }
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            match guard(&tx, &db.provider_key, &lease) {
                Ok(run) => {
                    if agent_execution::is_agent(&tx, &run.tenant, &run.id)? {
                        return Err(StoreError::Conflict);
                    }
                    transition(&tx, &run, state, error)?;
                    tx.commit()?;
                    Ok(())
                }
                Err(error) => {
                    tx.commit()?;
                    Err(error)
                }
            }
        })
        .await
    }
    pub async fn append_output(&self, lease: &RunLease, patches: Vec<OutputPatch>) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            if patches.is_empty() || patches.len() > 128 {
                return Err(StoreError::InvalidInput);
            }
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match guard(&tx, &db.provider_key, &lease) {
                Ok(run) => run,
                Err(error) => {
                    tx.commit()?;
                    return Err(error);
                }
            };
            if agent_execution::is_agent(&tx, &run.tenant, &run.id)? {
                return Err(StoreError::Conflict);
            }
            apply_patches(&tx, &run, patches)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn update_run_usage(&self, lease: &RunLease, usage: RunUsage) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| update_run_usage_inner(db, lease, usage))
            .await
    }
    pub async fn usage_summary(&self, user: &UserContext) -> Result<UsageSummary> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            usage(&db.conn, &user.tenant.0)
        })
        .await
    }
    pub async fn account_usage(&self, user: &UserContext) -> Result<AccountUsageResponse> {
        let user = user.clone();
        self.call(move |db| account_usage_inner(db, user)).await
    }
}
pub(super) fn context(conn: &Connection, run: &Run) -> Result<Vec<ContextMessage>> {
    let ids=conn.prepare("SELECT id FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND sequence<?3 AND state='completed' ORDER BY sequence DESC LIMIT 128")?.query_map(params![run.tenant,run.thread,run.sequence],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut turns = Vec::new();
    let mut count = 1usize;
    let mut bytes = run.text.len();
    let mut omitted = false;
    for id in ids {
        let prior = load_run(conn, &run.tenant, &id)?;
        let prior_id = prior.id.clone();
        let prior = turn(conn, &prior)?;
        let output = prior
            .assistant
            .into_iter()
            .filter(|m| !m.text.is_empty())
            .collect::<Vec<_>>();
        if output.is_empty() {
            continue;
        }
        let turn_bytes = prior.user_text.len() + output.iter().map(|m| m.text.len()).sum::<usize>();
        if output.iter().any(|m| m.text.len() > 64 * 1024) {
            // Keep a contiguous suffix. Skipping a recent correction and then
            // restoring older statements would create misleading history.
            omitted = true;
            break;
        }
        if count + 1 + output.len() > 126 || bytes + turn_bytes > 256 * 1024 - 128 {
            omitted = true;
            break;
        }
        let mut messages = vec![ContextMessage {
            role: ContextRole::User,
            text: prior.user_text,
            phase: None,
        }];
        messages.extend(output.into_iter().map(|m| ContextMessage {
            role: ContextRole::Assistant,
            text: m.text,
            phase: m.phase,
        }));
        agent_execution::inherit_dependencies(conn, &run.tenant, &run.id, &prior_id)?;
        count += messages.len();
        bytes += turn_bytes;
        turns.push(messages);
    }
    let mut result = Vec::new();
    if omitted {
        result.push(ContextMessage {
            role: ContextRole::System,
            text: "Some older conversation turns were omitted to fit the context limit.".into(),
            phase: None,
        });
    }
    for messages in turns.into_iter().rev() {
        result.extend(messages);
    }
    result.push(ContextMessage {
        role: ContextRole::User,
        text: run.text.clone(),
        phase: None,
    });
    Ok(result)
}
pub(super) fn apply_patches(conn: &Connection, run: &Run, patches: Vec<OutputPatch>) -> Result<()> {
    let mut bytes:i64=conn.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
    let mut items: i64 = conn.query_row(
        "SELECT count(*) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
        params![run.tenant, run.id],
        |r| r.get(0),
    )?;
    for patch in patches {
        let (item, phase) = match &patch {
            OutputPatch::Append { item, phase, .. } | OutputPatch::Phase { item, phase } => {
                (item, phase)
            }
        };
        if item.is_empty() || item.len() > 256 || item.chars().any(char::is_control) {
            return Err(StoreError::InvalidInput);
        }
        let existing:Option<(String,Option<String>,bool,i64)>=conn.query_row("SELECT id,phase,refusal,length(CAST(text AS BLOB)) FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item=?3",params![run.tenant,run.id,item],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
        let (message_id, old_phase, old_refusal, old_length) = if let Some(row) = existing {
            row
        } else {
            if items >= 128 {
                return Err(StoreError::InvalidInput);
            }
            let id = Uuid::new_v4().to_string();
            let refusal = matches!(&patch, OutputPatch::Append { refusal: true, .. });
            conn.execute("INSERT INTO chat_output(tenant_id,run_id,item,id,ordinal,text,phase,refusal) VALUES (?1,?2,?3,?4,?5,'',NULL,?6)",params![run.tenant,run.id,item,id,items,refusal])?;
            items += 1;
            (id, None, refusal, 0)
        };
        let encoded_phase = phase_name(*phase)?;
        match patch {
            OutputPatch::Append {
                item,
                text,
                phase,
                refusal,
            } => {
                if text.len() > 8192 || (old_length > 0 && old_refusal != refusal) {
                    return Err(StoreError::InvalidInput);
                }
                bytes += i64::try_from(text.len()).map_err(|_| StoreError::InvalidInput)?;
                if bytes > 256 * 1024 {
                    return Err(StoreError::InvalidInput);
                }
                let phase = phase.or(phase_value(old_phase)?);
                let encoded_phase = phase_name(phase)?;
                conn.execute("UPDATE chat_output SET text=text||?1,phase=?2,refusal=?6 WHERE tenant_id=?3 AND run_id=?4 AND item=?5",params![text,encoded_phase,run.tenant,run.id,item,refusal])?;
                emit(
                    conn,
                    run,
                    ChatChange::Delta {
                        run_id: run.id.clone(),
                        attempt: run.attempt,
                        message_id,
                        text,
                        phase,
                        refusal,
                    },
                )?;
            }
            OutputPatch::Phase { item, phase } => {
                if old_phase != encoded_phase {
                    conn.execute("UPDATE chat_output SET phase=?1 WHERE tenant_id=?2 AND run_id=?3 AND item=?4",params![encoded_phase,run.tenant,run.id,item])?;
                    emit(
                        conn,
                        run,
                        ChatChange::Phase {
                            run_id: run.id.clone(),
                            attempt: run.attempt,
                            message_id,
                            phase,
                        },
                    )?;
                }
            }
        }
    }
    conn.execute(
        "UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",
        params![run.tenant, run.id],
    )?;
    Ok(())
}
fn usage(conn: &Connection, tenant: &str) -> Result<UsageSummary> {
    let mut result = UsageSummary::default();
    let mut statement = conn.prepare("SELECT usage FROM model_call_receipts WHERE tenant_id=?1")?;
    let rows = statement.query_map([tenant], |r| r.get::<_, String>(0))?;
    for row in rows {
        let value: RunUsage = parse(&row?)?;
        result.dispatched_calls = result
            .dispatched_calls
            .checked_add(1)
            .ok_or(StoreError::Corrupt)?;
        if value.input_tokens.is_none() || value.output_tokens.is_none() {
            result.unreported_calls += 1;
        }
        result.reported_input_tokens = result
            .reported_input_tokens
            .checked_add(value.input_tokens.unwrap_or(0))
            .ok_or(StoreError::Corrupt)?;
        result.reported_output_tokens = result
            .reported_output_tokens
            .checked_add(value.output_tokens.unwrap_or(0))
            .ok_or(StoreError::Corrupt)?;
        result.reported_cache_read_tokens = result
            .reported_cache_read_tokens
            .checked_add(value.cache_read_tokens.unwrap_or(0))
            .ok_or(StoreError::Corrupt)?;
        result.reported_cache_write_tokens = result
            .reported_cache_write_tokens
            .checked_add(value.cache_write_tokens.unwrap_or(0))
            .ok_or(StoreError::Corrupt)?;
    }
    Ok(result)
}
pub(super) fn restart(conn: &mut Connection) -> Result<()> {
    let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
    crate::missions::restart(&tx)?;
    tx.execute(
        "UPDATE agent_steps SET state='unknown' WHERE state='dispatched'",
        [],
    )?;
    let rows = tx
        .prepare("SELECT tenant_id,id FROM chat_runs WHERE state='running'")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (tenant, id) in rows {
        let run = load_run(&tx, &tenant, &id)?;
        transition(
            &tx,
            &run,
            RunState::OutcomeUnknown,
            Some(RunFailure::ServerRestarted),
        )?;
    }
    tx.commit()?;
    Ok(())
}
#[cfg(test)]
mod tests;
fn submit_turn_inner(
    db: &mut Database,
    user: UserContext,
    thread_id: String,
    request_id: String,
    text: String,
) -> Result<ChatTurn> {
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let result = submit_turn_in(
        &tx,
        &db.provider_key,
        &db.web_profile,
        user,
        thread_id,
        request_id,
        text,
    )?;
    tx.commit()?;
    Ok(result)
}
/// Caller owns the transaction; allows browser completion and turn admission to commit together.
pub(super) fn submit_turn_in(
    tx: &Connection,
    provider_key: &[u8; 32],
    web_profile: &moosuri_core::WebToolProfile,
    user: UserContext,
    thread_id: String,
    request_id: String,
    text: String,
) -> Result<ChatTurn> {
    submit_turn_artifact_in(
        tx,
        provider_key,
        web_profile,
        user,
        thread_id,
        request_id,
        text,
        None,
    )
}
#[allow(clippy::too_many_arguments)]
pub(super) fn submit_turn_artifact_in(
    tx: &Connection,
    provider_key: &[u8; 32],
    web_profile: &moosuri_core::WebToolProfile,
    user: UserContext,
    thread_id: String,
    request_id: String,
    text: String,
    target: Option<moosuri_core::ArtifactTarget>,
) -> Result<ChatTurn> {
    submit_turn_context_in(
        tx,
        provider_key,
        web_profile,
        user,
        thread_id,
        request_id,
        text,
        target,
        vec![],
        None,
    )
}
#[allow(clippy::too_many_arguments)]
fn submit_turn_context_in(
    tx: &Connection,
    provider_key: &[u8; 32],
    web_profile: &moosuri_core::WebToolProfile,
    user: UserContext,
    thread_id: String,
    request_id: String,
    text: String,
    target: Option<moosuri_core::ArtifactTarget>,
    attachments: Vec<moosuri_core::MediaRef>,
    context: Option<moosuri_core::ScheduleChatContext>,
) -> Result<ChatTurn> {
    let user = validate(tx, &user, true, false)?;
    thread_exists(tx, &user, &thread_id)?;
    if attachments.len() > 4 {
        return Err(StoreError::InvalidInput);
    }
    let mut seen = std::collections::BTreeSet::new();
    for media in &attachments {
        if media.revision != 1 || Uuid::parse_str(&media.id).is_err() || !seen.insert(&media.id) {
            return Err(StoreError::InvalidInput);
        }
    }
    if text.trim().is_empty() || text.len() > 8192 {
        return Err(StoreError::InvalidInput);
    }
    let request_id = Uuid::parse_str(&request_id)
        .map_err(|_| StoreError::InvalidInput)?
        .to_string();
    if let Some(c) = &context {
        crate::schedules::chat::validate_timezone(&c.timezone)?;
        if c.target.is_some() && target.is_some() {
            return Err(StoreError::InvalidInput);
        }
    }
    let payload: [u8; 32] = if context.is_some() {
        Sha256::digest(
            json(&(
                "turn-context-v1",
                &thread_id,
                &text,
                &target,
                &attachments,
                &context,
            ))?
            .as_bytes(),
        )
        .into()
    } else if !attachments.is_empty() {
        Sha256::digest(json(&(&thread_id, &text, &target, &attachments))?.as_bytes()).into()
    } else if target.is_some() {
        Sha256::digest(json(&(&thread_id, &text, &target))?.as_bytes()).into()
    } else {
        Sha256::digest(json(&(&thread_id, &text))?.as_bytes()).into()
    };
    let existing: Option<(String, Vec<u8>)> = tx
        .query_row(
            "SELECT id,payload_digest FROM chat_runs WHERE tenant_id=?1 AND request_id=?2",
            params![user.tenant.0, request_id],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((id, old)) = existing {
        if old != payload {
            return Err(StoreError::Conflict);
        }
        return turn(tx, &load_run(tx, &user.tenant.0, &id)?);
    }
    if let Some(t) = &target {
        let v = crate::artifacts::version(tx, &user.tenant.0, &t.id, None)?;
        if v.artifact.revision != t.revision || v.draft.is_none() {
            return Err(StoreError::Conflict);
        }
    }
    queue_capacity(tx, &user.tenant.0)?;
    let choice = providers::selected_choice(tx, &user)?;
    providers::binding_for_choice(tx, provider_key, &choice)?;
    let mut needs_vision = false;
    for media in &attachments {
        let image:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM media_objects WHERE tenant_id=?1 AND id=?2 AND filename IS NULL)",params![user.tenant.0,media.id],|r|r.get(0))?;
        needs_vision |= image;
        let valid:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM media_objects WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND revision=?4 AND state='ready')",params![user.tenant.0,user.account.id,media.id,media.revision],|r|r.get(0))?;
        if !valid {
            return Err(StoreError::Conflict);
        }
    }
    if needs_vision {
        let vision:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM provider_vision_probes WHERE provider_id=?1 AND revision=?2 AND model=?3 AND effort=?4)",params![choice.provider_id,choice.provider_revision,choice.model,choice.effort.as_deref().unwrap_or("")],|r|r.get(0))?;
        if !vision {
            return Err(StoreError::InvalidInput);
        }
        if target.is_some() {
            return Err(StoreError::InvalidInput);
        }
    }
    if !attachments.is_empty() && target.is_some() {
        return Err(StoreError::InvalidInput);
    }
    let id = Uuid::new_v4().to_string();
    tx.execute(
        "UPDATE threads SET chat_turn_seq=chat_turn_seq+1 WHERE tenant_id=?1 AND id=?2",
        params![user.tenant.0, thread_id],
    )?;
    let sequence: i64 = tx.query_row(
        "SELECT chat_turn_seq FROM threads WHERE tenant_id=?1 AND id=?2",
        params![user.tenant.0, thread_id],
        |r| r.get(0),
    )?;
    if sequence == 1 {
        let title: String = text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .chars()
            .filter(|c| !c.is_control())
            .take(80)
            .collect();
        if !title.is_empty() {
            tx.execute("UPDATE threads SET title=?1 WHERE tenant_id=?2 AND id=?3 AND title='New conversation'",
                params![title,user.tenant.0,thread_id])?;
        }
    }
    let order = next_order(tx)?;
    tx.execute("INSERT INTO chat_runs(tenant_id,id,account_id,thread_id,request_id,payload_digest,sequence,user_text,model,session_digest,auth_epoch,attempt,state,version,created_at,admitted_order) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,'queued',1,?12,?13)",params![user.tenant.0,id,user.account.id,thread_id,request_id,payload.as_slice(),sequence,text,json(&StoredRunChoice { agent_profile: choice.function_tools.then(||crate::web::admitted_profile(tx,&user,web_profile)).transpose()?, model: choice })?,user.digest.as_slice(),user.epoch,now()?,order])?;
    for (ordinal, media) in attachments.iter().enumerate() {
        tx.execute(
            "INSERT INTO chat_media VALUES(?1,?2,?3,?4,?5)",
            params![user.tenant.0, id, media.id, media.revision, ordinal as i64],
        )?;
        tx.execute(
            "INSERT INTO media_run_dependencies VALUES(?1,?2,?3,?4)",
            params![user.tenant.0, id, media.id, media.revision],
        )?;
    }
    // Inherit even queued/running ancestors: their output may arrive before dispatch.
    tx.execute("INSERT OR IGNORE INTO media_run_dependencies SELECT d.tenant_id,?2,d.media_id,d.revision FROM media_run_dependencies d JOIN chat_runs r ON r.tenant_id=d.tenant_id AND r.id=d.run_id JOIN media_objects m ON m.tenant_id=d.tenant_id AND m.id=d.media_id WHERE r.tenant_id=?1 AND r.thread_id=?3 AND r.id!=?2 AND m.state='ready' AND NOT EXISTS(SELECT 1 FROM agent_execution a WHERE a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.invalidated=1)",params![user.tenant.0,id,thread_id])?;
    let run = load_run(tx, &user.tenant.0, &id)?;
    if run.tool_profile.is_some() {
        tx.execute(
            "INSERT INTO personal_artifact_run_profiles VALUES(?1,?2,1,?3,?4,0)",
            params![
                user.tenant.0,
                id,
                target.as_ref().map(|t| &t.id),
                target.as_ref().map(|t| t.revision)
            ],
        )?;
    }
    crate::schedules::chat::finish(tx, &user, &run, context.as_ref())?;
    let run = load_run(tx, &run.tenant, &run.id)?;
    let result = turn(tx, &run)?;
    emit(
        tx,
        &run,
        ChatChange::Turn {
            turn: result.clone(),
        },
    )?;
    Ok(result)
}
fn conversation_inner(
    db: &mut Database,
    user: UserContext,
    thread_id: String,
    before: Option<i64>,
) -> Result<ChatPage> {
    let user = validate(&db.conn, &user, true, false)?;
    thread_exists(&db.conn, &user, &thread_id)?;
    if before.is_some_and(|b| b <= 0) {
        return Err(StoreError::InvalidInput);
    }
    let tx = db.conn.transaction()?;
    let ids=tx.prepare("SELECT id FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND (?3 IS NULL OR sequence<?3) ORDER BY sequence DESC LIMIT 20")?.query_map(params![user.tenant.0,thread_id,before],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut turns = ids
        .iter()
        .map(|id| turn(&tx, &load_run(&tx, &user.tenant.0, id)?))
        .collect::<Result<Vec<_>>>()?;
    turns.reverse();
    let next_before = if let Some(first) = turns.first() {
        let older=tx.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND sequence<?3)",params![user.tenant.0,thread_id,first.sequence],|r|r.get::<_,bool>(0))?;
        older.then_some(first.sequence)
    } else {
        None
    };
    let cursor = tx.query_row(
        "SELECT chat_event_seq FROM threads WHERE tenant_id=?1 AND id=?2",
        params![user.tenant.0, thread_id],
        |r| r.get(0),
    )?;
    tx.commit()?;
    Ok(ChatPage {
        thread_id,
        cursor,
        turns,
        next_before,
    })
}
fn chat_events_inner(
    db: &mut Database,
    user: UserContext,
    thread_id: String,
    after: i64,
) -> Result<ChatEvents> {
    let user = validate(&db.conn, &user, true, false)?;
    thread_exists(&db.conn, &user, &thread_id)?;
    let tx = db.conn.transaction()?;
    let head: i64 = tx.query_row(
        "SELECT chat_event_seq FROM threads WHERE tenant_id=?1 AND id=?2",
        params![user.tenant.0, thread_id],
        |r| r.get(0),
    )?;
    let floor: i64 = tx.query_row(
        "SELECT chat_reset_floor FROM threads WHERE tenant_id=?1 AND id=?2",
        params![user.tenant.0, thread_id],
        |r| r.get(0),
    )?;
    if after < 0 || after > head || after < head.saturating_sub(512) || after < floor {
        return Ok(ChatEvents {
            events: vec![],
            reset: true,
            cursor: head,
        });
    }
    let mut stmt=tx.prepare("SELECT sequence,change FROM chat_events WHERE tenant_id=?1 AND thread_id=?2 AND sequence>?3 ORDER BY sequence LIMIT 64")?;
    let events = stmt
        .query_map(params![user.tenant.0, thread_id, after], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?
        .map(|r| {
            let (sequence, change) = r?;
            Ok(ChatEvent {
                sequence,
                change: parse(&change)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let cursor = events.last().map(|e| e.sequence).unwrap_or(head);
    drop(stmt);
    tx.commit()?;
    Ok(ChatEvents {
        events,
        reset: false,
        cursor,
    })
}
fn cancel_run_inner(
    db: &mut Database,
    user: UserContext,
    run_id: String,
    expected_attempt: i32,
) -> Result<ChatTurn> {
    let user = validate(&db.conn, &user, true, false)?;
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = load_run(&tx, &user.tenant.0, &run_id)?;
    if run.attempt != expected_attempt || !run.state.active() {
        return Err(StoreError::Conflict);
    }
    transition(&tx, &run, RunState::Cancelled, None)?;
    let result = turn(&tx, &load_run(&tx, &run.tenant, &run.id)?)?;
    tx.commit()?;
    Ok(result)
}
fn retry_run_inner(
    db: &mut Database,
    user: UserContext,
    run_id: String,
    expected_attempt: i32,
) -> Result<ChatTurn> {
    let user = validate(&db.conn, &user, true, false)?;
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let run = load_run(&tx, &user.tenant.0, &run_id)?;
    if run.attempt != expected_attempt
        || run.state.active()
        || run.state == RunState::Completed
        || run.attempt >= 32
        || !agent_execution::retry_allowed(&tx, &run.tenant, &run.id)?
        || !is_latest(&tx, &run)?
    {
        return Err(StoreError::Conflict);
    }
    queue_capacity(&tx, &user.tenant.0)?;
    providers::binding_for_choice(&tx, &db.provider_key, &run.model)?;
    let order = next_order(&tx)?;
    tx.execute(
        "DELETE FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
        params![run.tenant, run.id],
    )?;
    agent_execution::reset_attempt(&tx, &run.tenant, &run.id)?;
    tx.execute("UPDATE chat_runs SET attempt=attempt+1,state='queued',version=version+1,error=NULL,session_digest=?1,auth_epoch=?2,admitted_order=?3 WHERE tenant_id=?4 AND id=?5",params![user.digest.as_slice(),user.epoch,order,run.tenant,run.id])?;
    let current = load_run(&tx, &run.tenant, &run.id)?;
    let result = turn(&tx, &current)?;
    emit(
        &tx,
        &current,
        ChatChange::Turn {
            turn: result.clone(),
        },
    )?;
    tx.commit()?;
    Ok(result)
}
fn claim_run_inner(db: &mut Database) -> Result<Option<ClaimedRun>> {
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let running: i64 = tx.query_row(
        "SELECT count(*) FROM chat_runs WHERE state='running'",
        [],
        |r| r.get(0),
    )?;
    if running >= 2 {
        return Ok(None);
    }
    let queued=tx.prepare("SELECT tenant_id,id FROM chat_runs WHERE state='queued' AND NOT EXISTS(SELECT 1 FROM agent_execution a WHERE a.tenant_id=chat_runs.tenant_id AND a.run_id=chat_runs.id AND a.agent=1) AND tenant_id NOT IN (SELECT tenant_id FROM chat_runs WHERE state='running') ORDER BY admitted_order LIMIT 64")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (tenant, id) in queued {
        let run = load_run(&tx, &tenant, &id)?;
        if authority(&tx, &run).is_err() {
            transition(
                &tx,
                &run,
                RunState::Cancelled,
                Some(RunFailure::AuthorityRevoked),
            )?;
            continue;
        }
        let binding = match providers::binding_for_choice(&tx, &db.provider_key, &run.model) {
            Ok(binding) => binding,
            Err(_) => {
                transition(
                    &tx,
                    &run,
                    RunState::Failed,
                    Some(RunFailure::ProviderUnavailable),
                )?;
                continue;
            }
        };
        let messages = context(&tx, &run)?;
        let mut nonce = [0; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| StoreError::Unavailable)?;
        tx.execute("INSERT INTO model_call_receipts(tenant_id,account_id,run_id,attempt,nonce,usage,dispatched_at) VALUES (?1,?2,?3,?4,?5,?6,?7)",params![run.tenant,run.account,run.id,run.attempt,nonce.as_slice(),json(&RunUsage::default())?,now()?])?;
        transition(&tx, &run, RunState::Running, None)?;
        let lease = RunLease {
            tenant: run.tenant,
            run: run.id,
            attempt: run.attempt,
            nonce,
            epoch: run.epoch,
        };
        tx.commit()?;
        return Ok(Some(ClaimedRun {
            lease,
            binding,
            messages,
        }));
    }
    tx.commit()?;
    Ok(None)
}
fn update_run_usage_inner(db: &mut Database, lease: RunLease, usage: RunUsage) -> Result<()> {
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let nonce: Option<Vec<u8>> = tx.query_row("SELECT nonce FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=0",params![lease.tenant,lease.run,lease.attempt],|r|r.get(0)).optional()?;
    if nonce.as_deref() != Some(lease.nonce.as_slice()) {
        return Err(StoreError::Unauthorized);
    }
    let encoded: String = tx.query_row(
        "SELECT usage FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=0",
        params![lease.tenant, lease.run, lease.attempt],
        |r| r.get(0),
    )?;
    let old: RunUsage = parse(&encoded)?;
    // Provider snapshots are cumulative observations, not additive charges.
    let merged = RunUsage {
        input_tokens: usage.input_tokens.or(old.input_tokens),
        output_tokens: usage.output_tokens.or(old.output_tokens),
        cache_read_tokens: usage.cache_read_tokens.or(old.cache_read_tokens),
        cache_write_tokens: usage.cache_write_tokens.or(old.cache_write_tokens),
    };
    if old != merged {
        tx.execute("UPDATE model_call_receipts SET usage=?1 WHERE tenant_id=?2 AND run_id=?3 AND attempt=?4 AND step=0",params![json(&merged)?,lease.tenant,lease.run,lease.attempt])?;
        if let Ok(run) = load_run(&tx, &lease.tenant, &lease.run)
            && run.attempt == lease.attempt
        {
            tx.execute(
                "UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",
                params![run.tenant, run.id],
            )?;
            let current = load_run(&tx, &run.tenant, &run.id)?;
            emit(
                &tx,
                &current,
                ChatChange::State {
                    run: view(&tx, &current)?,
                },
            )?;
        }
    }
    tx.commit()?;
    Ok(())
}
fn account_usage_inner(db: &mut Database, user: UserContext) -> Result<AccountUsageResponse> {
    validate(&db.conn, &user, true, true)?;
    let rows = db
        .conn
        .prepare("SELECT id,name,email,tenant_id FROM accounts ORDER BY email")?
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, String>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let accounts = rows
        .into_iter()
        .map(|(account_id, name, email, tenant)| {
            Ok(AccountUsage {
                account_id,
                name,
                email,
                usage: usage(&db.conn, &tenant)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(AccountUsageResponse { accounts })
}

impl Store {
    pub async fn submit_turn_with_artifact(
        &self,
        user: &UserContext,
        thread: String,
        request: String,
        text: String,
        target: Option<moosuri_core::ArtifactTarget>,
    ) -> Result<ChatTurn> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let result = submit_turn_artifact_in(
                &tx,
                &db.provider_key,
                &db.web_profile,
                user,
                thread,
                request,
                text,
                target,
            )?;
            tx.commit()?;
            Ok(result)
        })
        .await
    }
}

impl Store {
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_turn_with_media(
        &self,
        user: &UserContext,
        thread: String,
        request: String,
        text: String,
        target: Option<moosuri_core::ArtifactTarget>,
        attachments: Vec<moosuri_core::MediaRef>,
    ) -> Result<ChatTurn> {
        self.submit_turn_with_context(user, thread, request, text, target, attachments, None)
            .await
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_turn_with_schedule_context(
        &self,
        user: &UserContext,
        thread: String,
        request: String,
        text: String,
        target: Option<moosuri_core::ArtifactTarget>,
        context: Option<moosuri_core::ScheduleChatContext>,
    ) -> Result<ChatTurn> {
        self.submit_turn_with_context(user, thread, request, text, target, vec![], context)
            .await
    }
}

impl Store {
    #[allow(clippy::too_many_arguments)]
    pub async fn submit_turn_with_context(
        &self,
        user: &UserContext,
        thread: String,
        request: String,
        text: String,
        target: Option<moosuri_core::ArtifactTarget>,
        attachments: Vec<moosuri_core::MediaRef>,
        context: Option<moosuri_core::ScheduleChatContext>,
    ) -> Result<ChatTurn> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let result = submit_turn_context_in(
                &tx,
                &db.provider_key,
                &db.web_profile,
                user,
                thread,
                request,
                text,
                target,
                attachments,
                context,
            )?;
            tx.commit()?;
            Ok(result)
        })
        .await
    }
}
