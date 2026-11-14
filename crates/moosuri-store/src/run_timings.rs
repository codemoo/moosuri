//! Bounded private numeric metadata; no raw trace or cross-clock subtraction.
use super::*;
use crate::chat::Run;
use moosuri_core::{LatencySamples, RunState, RunTimingAggregate, RunTimings};
use serde::{Deserialize, Serialize};
use std::time::Instant;
const FIELD: &str = "run_observations_v1";
static CLOCK: LazyLock<(String, Instant)> =
    LazyLock::new(|| (Uuid::new_v4().to_string(), Instant::now()));
#[derive(Clone, Copy)]
pub struct TimingMark(u64);
pub fn timing_mark() -> TimingMark {
    TimingMark(CLOCK.1.elapsed().as_millis().min(u64::MAX as u128) as u64)
}
#[derive(Default, Serialize, Deserialize)]
struct Saved {
    attempts: Vec<Attempt>,
}
#[derive(Serialize, Deserialize)]
struct Attempt {
    attempt: i32,
    clock: String,
    admitted: u64,
    claimed: Option<u64>,
    context_ms: Option<u64>,
    first_dispatch_ms: Option<u64>,
    calls: Vec<Call>,
    #[serde(default)]
    tools: Vec<Call>,
    bodies: Vec<(i64, u64)>,
    first_final_body_ms: Option<u64>,
    terminal_ms: Option<u64>,
    outcome: Option<String>,
    discontinuous: bool,
}
#[derive(Serialize, Deserialize)]
struct Call {
    step: i64,
    elapsed_ms: Option<u64>,
}
fn load(c: &Connection, t: &str, id: &str) -> Result<Option<(serde_json::Value, Saved)>> {
    let raw: Option<String> = c
        .query_row(
            "SELECT model FROM chat_runs WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let model: serde_json::Value = serde_json::from_str(&raw).map_err(|_| StoreError::Corrupt)?;
    let saved = model
        .get(FIELD)
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|_| StoreError::Corrupt)?
        .unwrap_or_default();
    Ok(Some((model, saved)))
}
fn save(
    c: &Connection,
    t: &str,
    id: &str,
    mut model: serde_json::Value,
    saved: Saved,
) -> Result<()> {
    model[FIELD] = serde_json::to_value(saved).map_err(|_| StoreError::Corrupt)?;
    c.execute(
        "UPDATE chat_runs SET model=?3 WHERE tenant_id=?1 AND id=?2",
        params![
            t,
            id,
            serde_json::to_string(&model).map_err(|_| StoreError::Corrupt)?
        ],
    )?;
    Ok(())
}
fn change(
    c: &Connection,
    t: &str,
    id: &str,
    attempt: i32,
    op: impl FnOnce(&mut Attempt) -> Result<()>,
) -> Result<()> {
    let Some((model, mut saved)) = load(c, t, id)? else {
        return Ok(());
    };
    let Some(a) = saved.attempts.iter_mut().find(|a| a.attempt == attempt) else {
        return Ok(());
    };
    let before = serde_json::to_vec(a).map_err(|_| StoreError::Corrupt)?;
    op(a)?;
    if before == serde_json::to_vec(a).map_err(|_| StoreError::Corrupt)? {
        return Ok(());
    }
    save(c, t, id, model, saved)
}
fn since(a: &Attempt, mark: TimingMark) -> Option<u64> {
    (!a.discontinuous && a.clock == CLOCK.0)
        .then(|| mark.0.checked_sub(a.admitted))
        .flatten()
}
pub(crate) fn admit(c: &Connection, t: &str, id: &str, attempt: i32) -> Result<()> {
    let Some((model, mut saved)) = load(c, t, id)? else {
        return Ok(());
    };
    if saved.attempts.iter().any(|a| a.attempt == attempt) || saved.attempts.len() >= 32 {
        return Ok(());
    }
    saved.attempts.push(Attempt {
        attempt,
        clock: CLOCK.0.clone(),
        admitted: timing_mark().0,
        claimed: None,
        context_ms: None,
        first_dispatch_ms: None,
        calls: vec![],
        tools: vec![],
        bodies: vec![],
        first_final_body_ms: None,
        terminal_ms: None,
        outcome: None,
        discontinuous: false,
    });
    save(c, t, id, model, saved)
}
pub(crate) fn claim(c: &Connection, r: &Run) -> Result<()> {
    change(c, &r.tenant, &r.id, r.attempt, |a| {
        if a.claimed.is_none() {
            a.claimed = since(a, timing_mark());
        }
        Ok(())
    })
}
pub(crate) fn context(c: &Connection, r: &Run, start: Instant) -> Result<()> {
    change(c, &r.tenant, &r.id, r.attempt, |a| {
        a.context_ms = Some(
            a.context_ms
                .unwrap_or(0)
                .saturating_add(start.elapsed().as_millis().min(u64::MAX as u128) as u64),
        );
        Ok(())
    })
}
pub(crate) fn dispatch(c: &Connection, r: &Run, step: i64) -> Result<()> {
    change(c, &r.tenant, &r.id, r.attempt, |a| {
        if !a.calls.iter().any(|s| s.step == step) && a.calls.len() < 32 {
            a.calls.push(Call {
                step,
                elapsed_ms: None,
            });
            if a.first_dispatch_ms.is_none() {
                a.first_dispatch_ms = since(a, timing_mark())
            }
        }
        Ok(())
    })
}
pub(crate) fn body(c: &Connection, r: &Run, item: &str, mark: TimingMark) -> Result<()> {
    let ordinal:Option<i64>=c.query_row("SELECT ordinal FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item=?3 AND length(trim(text))>0 AND refusal=0",params![r.tenant,r.id,item],|r|r.get(0)).optional()?;
    let Some(ordinal) = ordinal else {
        return Ok(());
    };
    change(c, &r.tenant, &r.id, r.attempt, |a| {
        if let Some(ms) = since(a, mark) {
            if let Some(v) = a.bodies.iter_mut().find(|v| v.0 == ordinal) {
                v.1 = v.1.min(ms)
            } else if a.bodies.len() < 128 {
                a.bodies.push((ordinal, ms))
            }
        }
        Ok(())
    })
}
pub(crate) fn terminal(c: &Connection, r: &Run, state: RunState) -> Result<()> {
    if state.active() {
        return Ok(());
    }
    let ordinals=c.prepare("SELECT ordinal FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND phase='final_answer' AND refusal=0 AND length(trim(text))>0")?.query_map(params![r.tenant,r.id],|r|r.get::<_,i64>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    change(c, &r.tenant, &r.id, r.attempt, |a| {
        if a.outcome.is_none() {
            a.outcome = Some(
                serde_json::to_value(state)
                    .map_err(|_| StoreError::Corrupt)?
                    .as_str()
                    .ok_or(StoreError::Corrupt)?
                    .into(),
            );
            a.terminal_ms = since(a, timing_mark());
            if state == RunState::Completed && !a.discontinuous {
                a.first_final_body_ms = a
                    .bodies
                    .iter()
                    .filter(|v| ordinals.contains(&v.0))
                    .map(|v| v.1)
                    .min();
            }
        }
        Ok(())
    })
}
pub(crate) fn restart(c: &Connection) -> Result<()> {
    let rows=c.prepare("SELECT tenant_id,id,attempt FROM chat_runs WHERE state IN('queued','running') AND json_type(model,'$.run_observations_v1') IS NOT NULL")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i32>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, id, attempt) in rows {
        change(c, &t, &id, attempt, |a| {
            a.discontinuous = true;
            Ok(())
        })?;
    }
    Ok(())
}
fn project(a: &Attempt) -> RunTimings {
    let measured = a
        .calls
        .iter()
        .filter_map(|s| s.elapsed_ms)
        .collect::<Vec<_>>();
    let tools = a
        .tools
        .iter()
        .filter_map(|s| s.elapsed_ms)
        .collect::<Vec<_>>();
    RunTimings {
        tool_ms: (!tools.is_empty() && tools.len() == a.tools.len())
            .then(|| tools.iter().copied().fold(0, u64::saturating_add)),
        tool_calls: a.tools.len() as u32,
        measured_tool_calls: tools.len() as u32,
        attempt: a.attempt,
        queue_ms: a.claimed,
        context_ms: a.context_ms,
        first_dispatch_ms: a.first_dispatch_ms,
        model_ms: (!measured.is_empty() && measured.len() == a.calls.len())
            .then(|| measured.iter().copied().fold(0, u64::saturating_add)),
        first_final_body_ms: a.first_final_body_ms,
        terminal_ms: a.terminal_ms,
        model_calls: a.calls.len() as u32,
        measured_model_calls: measured.len() as u32,
        clock_discontinuity: a.discontinuous,
    }
}
pub(crate) fn view(c: &Connection, r: &Run) -> Result<Option<RunTimings>> {
    Ok(load(c, &r.tenant, &r.id)?.and_then(|(_, s)| {
        s.attempts
            .iter()
            .find(|a| a.attempt == r.attempt)
            .map(project)
    }))
}
fn stats(values: Vec<Option<u64>>) -> LatencySamples {
    let total = values.len();
    let mut values = values.into_iter().flatten().collect::<Vec<_>>();
    values.sort_unstable();
    let rank =
        |p: usize| (!values.is_empty()).then(|| values[(values.len() * p).div_ceil(100) - 1]);
    LatencySamples {
        samples: values.len() as u32,
        missing: (total - values.len()) as u32,
        p50_ms: rank(50),
        p95_ms: rank(95),
    }
}
pub(crate) fn aggregate(c: &Connection, tenant: &str) -> Result<RunTimingAggregate> {
    let rows=c.prepare("SELECT id,state,attempt FROM chat_runs WHERE tenant_id=?1 ORDER BY admitted_order DESC LIMIT 129")?.query_map([tenant],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i32>(2)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = RunTimingAggregate {
        truncated: rows.len() > 128,
        outcomes: [
            "queued",
            "running",
            "completed",
            "failed",
            "cancelled",
            "incomplete",
            "refused",
            "outcome_unknown",
            "unobserved",
        ]
        .into_iter()
        .map(|s| (s.to_owned(), 0))
        .collect(),
        ..Default::default()
    };
    let mut observations = vec![];
    for (id, state, attempt) in rows.into_iter().take(128) {
        out.runs += 1;
        let (_, saved) = load(c, tenant, &id)?.ok_or(StoreError::Corrupt)?;
        if saved.attempts.is_empty() {
            out.unmeasured_runs += 1;
        }
        for missing in (1..=attempt).filter(|n| !saved.attempts.iter().any(|a| a.attempt == *n)) {
            out.attempts += 1;
            let outcome = if missing == attempt {
                state.as_str()
            } else {
                "unobserved"
            };
            *out.outcomes.get_mut(outcome).ok_or(StoreError::Corrupt)? += 1;
            observations.push(RunTimings {
                attempt: missing,
                ..Default::default()
            });
        }
        for a in saved.attempts {
            out.attempts += 1;
            let state = a.outcome.clone().unwrap_or_else(|| {
                if a.attempt == attempt {
                    state.clone()
                } else {
                    "unobserved".into()
                }
            });
            *out.outcomes.get_mut(&state).ok_or(StoreError::Corrupt)? += 1;
            let observation = project(&a);
            if observation.model_calls == 0 {
                out.not_dispatched += 1
            }
            observations.push(observation)
        }
    }
    out.queue = stats(observations.iter().map(|v| v.queue_ms).collect());
    out.context = stats(observations.iter().map(|v| v.context_ms).collect());
    out.first_dispatch = stats(observations.iter().map(|v| v.first_dispatch_ms).collect());
    out.model = stats(observations.iter().map(|v| v.model_ms).collect());
    out.tool = stats(observations.iter().map(|v| v.tool_ms).collect());
    out.first_final_body = stats(observations.iter().map(|v| v.first_final_body_ms).collect());
    out.terminal = stats(observations.iter().map(|v| v.terminal_ms).collect());
    Ok(out)
}
impl Store {
    pub async fn record_tool_timing(
        &self,
        lease: &crate::agent_execution::StepLease,
        call_id: String,
        milliseconds: Option<u64>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            let valid:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=?4 AND nonce=?5)",params![lease.run.tenant,lease.run.run,lease.run.attempt,lease.step,lease.nonce.as_slice()],|r|r.get(0))?;
            if !valid{return Err(StoreError::Unauthorized)}
            let ordinal:Option<i64>=db.conn.query_row("SELECT (SELECT count(*) FROM agent_tool_slots x WHERE x.tenant_id=s.tenant_id AND x.run_id=s.run_id AND x.rowid<=s.rowid) FROM agent_tool_slots s WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![lease.run.tenant,lease.run.run,lease.step,call_id],|r|r.get(0)).optional()?;
            let Some(ordinal)=ordinal else{return Ok(())};
            change(&db.conn,&lease.run.tenant,&lease.run.run,lease.run.attempt,|a|{
                if let Some(span)=a.tools.iter_mut().find(|s|s.step==ordinal) {if let Some(ms)=milliseconds{span.elapsed_ms.get_or_insert(ms);}}
                else if a.tools.len()<12 {a.tools.push(Call{step:ordinal,elapsed_ms:milliseconds});} Ok(())
            })
        }).await
    }
    /// Numeric accounting only: the original step receipt authenticates late duration,
    /// with no new execution/read/output authority.
    pub async fn record_model_duration(
        &self,
        lease: &crate::agent_execution::StepLease,
        milliseconds: u64,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{let valid:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3 AND step=?4 AND nonce=?5)",params![lease.run.tenant,lease.run.run,lease.run.attempt,lease.step,lease.nonce.as_slice()],|r|r.get(0))?;if !valid{return Err(StoreError::Unauthorized)}change(&db.conn,&lease.run.tenant,&lease.run.run,lease.run.attempt,|a|{if let Some(call)=a.calls.iter_mut().find(|s|s.step==lease.step){call.elapsed_ms.get_or_insert(milliseconds);}Ok(())})}).await
    }
}
#[cfg(test)]
mod tests;
