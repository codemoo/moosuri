//! Evaluator-only, read-only SQLite observations. Never passed to the agent.
use crate::{EvalResult, model::Dispatch};
use rusqlite::{Connection, OpenFlags, OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;
use std::{path::Path, time::Duration};

#[derive(Clone, Serialize)]
pub struct Evidence {
    pub kind: String,
    pub id: String,
    pub revision: i64,
    pub owner: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct Slot {
    pub step: i64,
    pub call_id: String,
    pub input: Value,
    pub result: Option<Value>,
}
#[derive(Clone, Serialize)]
pub struct MissionObservation {
    pub state: String,
    pub mode: String,
    pub candidate: Value,
    pub plan: Value,
    pub report: Value,
}
#[derive(Clone, Serialize)]
pub struct Observation {
    pub tenant: String,
    pub account: String,
    pub run_id: String,
    pub state: String,
    pub output: String,
    pub evidence: Vec<Evidence>,
    pub returned_candidates: Vec<Evidence>,
    pub slots: Vec<Slot>,
    pub finalized_no_call_step: bool,
    pub model_receipts: usize,
    pub model_receipt_steps: Vec<i64>,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub ledger: Vec<Dispatch>,
    pub mission: Option<MissionObservation>,
}
pub fn open(root: &Path) -> EvalResult<Connection> {
    let conn = Connection::open_with_flags(
        root.join("assistant.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .map_err(|_| "observer_open")?;
    conn.busy_timeout(Duration::from_secs(2))
        .map_err(|_| "observer_timeout")?;
    Ok(conn)
}
pub fn snapshot(root: &Path, run_id: &str, ledger: Vec<Dispatch>) -> EvalResult<Observation> {
    let mut conn = open(root)?;
    let tx = conn.transaction().map_err(|_| "observer_snapshot")?;
    let (tenant, account, state): (String, String, String) = tx
        .query_row(
            "SELECT tenant_id,account_id,state FROM chat_runs WHERE id=?1",
            [run_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .map_err(|_| "observer_run")?;
    let mut output = tx
        .prepare("SELECT text FROM chat_output WHERE tenant_id=?1 AND run_id=?2 ORDER BY ordinal")
        .map_err(|_| "observer_output")?
        .query_map(params![tenant, run_id], |r| r.get::<_, String>(0))
        .map_err(|_| "observer_output")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "observer_output")?
        .concat();
    let mission=tx.query_row("SELECT state,mode,candidate,plan,report FROM missions WHERE tenant_id=?1 AND run_id=?2",params![tenant,run_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?))).optional().map_err(|_|"observer_mission")?.map(|(state,mode,candidate,plan,report)| {
        let candidate=candidate.and_then(|s|serde_json::from_str::<Value>(&s).ok()).unwrap_or(Value::Null);
        output=candidate["text"].as_str().unwrap_or("").to_owned();
        MissionObservation {state,mode,candidate,plan:serde_json::from_str(&plan).unwrap_or(Value::Null),report:serde_json::from_str(&report).unwrap_or(Value::Null)}
    });
    let dependencies = tx.prepare("SELECT kind,object_id,revision FROM agent_dependencies WHERE tenant_id=?1 AND run_id=?2 ORDER BY kind,object_id").map_err(|_| "observer_evidence")?.query_map(params![tenant,run_id], |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i64>(2)?))).map_err(|_| "observer_evidence")?.collect::<Result<Vec<_>,_>>().map_err(|_| "observer_evidence")?;
    let mut evidence = vec![];
    for (kind, id, revision) in dependencies {
        let sql = if kind == "source" {
            "SELECT tenant_id FROM knowledge_sources WHERE id=?1 AND tenant_id=?2 AND deleted=0 AND revision=?3"
        } else {
            "SELECT tenant_id FROM knowledge_memories WHERE id=?1 AND tenant_id=?2 AND status='active' AND revision=?3"
        };
        let owner = tx
            .query_row(sql, params![id, tenant, revision], |r| r.get(0))
            .optional()
            .map_err(|_| "observer_owner")?;
        evidence.push(Evidence {
            kind,
            id,
            revision,
            owner,
        });
    }
    let records = tx.prepare("SELECT step,call_id,input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 ORDER BY step,call_id").map_err(|_| "observer_slots")?.query_map(params![tenant,run_id], |r| Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?))).map_err(|_| "observer_slots")?.collect::<Result<Vec<_>,_>>().map_err(|_| "observer_slots")?;
    let mut slots = vec![];
    for (step, call_id, input, result) in records {
        let input = input
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or(Value::Null);
        let result = result.and_then(|s| serde_json::from_str(&s).ok());
        slots.push(Slot {
            step,
            call_id,
            input,
            result,
        });
    }
    // Search candidates are observations even when the agent never reads or
    // cites them. Late final-answer filtering cannot conceal tenant leakage.
    let mut returned_candidates = vec![];
    for slot in &slots {
        if let Some(result) = &slot.result
            && result["kind"] == "search"
            && let Some(hits) = result["value"]["hits"].as_array()
        {
            for hit in hits {
                let kind = hit["kind"].as_str().unwrap_or("invalid").to_owned();
                let id = hit["id"].as_str().unwrap_or("invalid").to_owned();
                let revision = hit["revision"].as_i64().unwrap_or(0);
                let sql = match kind.as_str() {
                    "source" => {
                        "SELECT tenant_id FROM knowledge_sources WHERE id=?1 AND tenant_id=?2 AND deleted=0 AND revision=?3"
                    }
                    "memory" => {
                        "SELECT tenant_id FROM knowledge_memories WHERE id=?1 AND tenant_id=?2 AND status='active' AND revision=?3"
                    }
                    _ => {
                        "SELECT tenant_id FROM knowledge_memories WHERE 0 AND id=?1 AND tenant_id=?2 AND revision=?3"
                    }
                };
                let owner = tx
                    .query_row(sql, params![id, tenant, revision], |r| r.get(0))
                    .optional()
                    .map_err(|_| "observer_candidate_owner")?;
                returned_candidates.push(Evidence {
                    kind,
                    id,
                    revision,
                    owner,
                });
            }
        }
    }
    let finalized_no_call_step = tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps s WHERE s.tenant_id=?1 AND s.run_id=?2 AND s.state='completed' AND s.step=(SELECT max(step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots t WHERE t.tenant_id=s.tenant_id AND t.run_id=s.run_id AND t.step=s.step))",params![tenant,run_id],|r|r.get(0)).map_err(|_| "observer_finalization")?;
    let usage = tx
        .prepare("SELECT step,usage FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 ORDER BY step")
        .map_err(|_| "observer_usage")?
        .query_map(params![tenant, run_id], |r| Ok((r.get::<_,i64>(0)?,r.get::<_, String>(1)?)))
        .map_err(|_| "observer_usage")?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|_| "observer_usage")?;
    let mut input_tokens = Some(0_u64);
    let mut output_tokens = Some(0_u64);
    for (_, row) in &usage {
        let value: Value = serde_json::from_str(row).map_err(|_| "observer_usage_json")?;
        input_tokens = input_tokens
            .zip(value["input_tokens"].as_u64())
            .map(|(a, b)| a + b);
        output_tokens = output_tokens
            .zip(value["output_tokens"].as_u64())
            .map(|(a, b)| a + b);
    }
    Ok(Observation {
        tenant,
        account,
        run_id: run_id.into(),
        state,
        output,
        evidence,
        returned_candidates,
        slots,
        finalized_no_call_step,
        model_receipts: usage.len(),
        model_receipt_steps: usage.iter().map(|(step, _)| *step).collect(),
        input_tokens,
        output_tokens,
        ledger,
        mission,
    })
}
