//! Mission authority and deterministic verification live in the SQLite writer.
//! A durable grant is never converted to a session UserContext.
use super::*;
use crate::agent_execution::{KnowledgeToolInput, KnowledgeToolResult};
use crate::chat::Run;
use moosuri_core::{
    MissionAction, MissionCandidate, MissionCheck, MissionControl, MissionCreate, MissionMode,
    MissionReceipt, MissionView, RunState,
};
fn json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn parse<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn clock() -> Result<i64> {
    now()?.checked_mul(1000).ok_or(StoreError::Unavailable)
}
pub(crate) fn is_mission(c: &Connection, t: &str, r: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND run_id=?2)",
        params![t, r],
        |r| r.get(0),
    )?)
}
pub(crate) fn authority(c: &Connection, r: &Run) -> Result<()> {
    let (state,mode,enabled,revision,expiry):(String,String,bool,i64,i64)=c.query_row("SELECT state,mode,grant_enabled,revision,expires_ms FROM missions WHERE tenant_id=?1 AND run_id=?2 AND account_id=?3 AND auth_epoch=?4",params![r.tenant,r.id,r.account,r.epoch],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?.ok_or(StoreError::Unauthorized)?;
    let eligible:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1 AND id=?2 AND auth_epoch=?3 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>?4))",params![r.tenant,r.account,r.epoch,now()?],|r|r.get(0))?;
    if !enabled
        || !eligible
        || !matches!(
            state.as_str(),
            "queued" | "planning" | "executing" | "verifying"
        )
        || expiry <= clock()?
        || revision < 1
    {
        return Err(StoreError::Unauthorized);
    }
    if mode == "current_session" {
        crate::chat::authority(c, r)?;
    }
    // Pinned instruction skill withdrawals fence even proposals that did not read the pin yet.
    if let Some(p) = &r.tool_profile {
        for pin in &p.skills {
            if matches!(
                crate::agent_execution::skills::read(c, r, pin),
                KnowledgeToolResult::Error(_)
            ) {
                return Err(StoreError::Conflict);
            }
        }
    }
    Ok(())
}
pub(crate) fn tool_allowed(v: &KnowledgeToolInput) -> bool {
    matches!(
        v,
        KnowledgeToolInput::Search { .. }
            | KnowledgeToolInput::ReadSource { .. }
            | KnowledgeToolInput::ReadMemory { .. }
            | KnowledgeToolInput::SkillRead { .. }
            | KnowledgeToolInput::DecimalCalculate { .. }
            | KnowledgeToolInput::External {
                request: moosuri_core::ExternalToolRequest::Web(
                    moosuri_core::WebToolRequest::Read { .. }
                        | moosuri_core::WebToolRequest::Search { .. }
                )
            }
    )
}
pub(crate) fn carrier_transition(c: &Connection, r: &Run, s: RunState) -> Result<()> {
    let state = match s {
        RunState::Queued => "queued",
        RunState::Running => "planning",
        RunState::OutcomeUnknown => "unknown",
        RunState::Cancelled => "cancelled",
        RunState::Failed | RunState::Refused => "failed",
        RunState::Incomplete => "partial",
        RunState::Completed => return Ok(()),
    };
    c.execute("UPDATE missions SET state=CASE WHEN ?3='planning' AND plan!='[]' THEN 'executing' ELSE ?3 END,uncertain=CASE WHEN ?3='unknown' THEN 1 ELSE uncertain END WHERE tenant_id=?1 AND run_id=?2 AND state IN('queued','planning','executing','verifying')",params![r.tenant,r.id,state])?;
    Ok(())
}
// A stable mission continuation is not a fresh model attempt. The legacy chat
// attempt column is capped at32; at that boundary retire the old lease inside
// this transaction and let claim mint a fresh nonce. Step accounting receipts
// remain immutable and cannot authorize execution. Ordinary chat is unaffected.
fn requeue_stable(c: &Connection, tenant: &str, run: &str) -> Result<()> {
    let eligible: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM missions m JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id WHERE m.tenant_id=?1 AND m.run_id=?2 AND m.grant_enabled=1 AND m.uncertain=0 AND m.state IN('paused','queued','planning','executing','verifying') AND r.state IN('queued','running','cancelled')) AND NOT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state IN('dispatched','unknown')) AND NOT EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started') AND NOT EXISTS(SELECT 1 FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=0)",
        params![tenant, run],
        |r| r.get(0),
    )?;
    if !eligible {
        return Err(StoreError::Conflict);
    }
    c.execute(
        "DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2 AND attempt=32 AND EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND id=?2 AND attempt=32)",
        params![tenant, run],
    )?;
    if c.execute(
        "UPDATE chat_runs SET state='queued',attempt=min(attempt+1,32) WHERE tenant_id=?1 AND id=?2 AND state IN('queued','running','cancelled')",
        params![tenant, run],
    )? != 1
    {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
pub(crate) fn restart(c: &Connection) -> Result<()> {
    // A stable checkpoint contains only committed local results. No started remote IO is replayed.
    c.execute("UPDATE missions SET state='unknown',uncertain=1,report='[\"A committed dispatch has an unknown outcome. Create a new task.\"]' WHERE state IN('planning','executing','verifying') AND (EXISTS(SELECT 1 FROM agent_steps s WHERE s.tenant_id=missions.tenant_id AND s.run_id=missions.run_id AND s.state IN('dispatched','unknown')) OR EXISTS(SELECT 1 FROM external_tool_dispatches d WHERE d.tenant_id=missions.tenant_id AND d.run_id=missions.run_id AND d.state='started'))",[])?;
    let stable = c.prepare("SELECT r.tenant_id,r.id FROM chat_runs r JOIN missions m ON m.tenant_id=r.tenant_id AND m.run_id=r.id WHERE r.state='running' AND m.state IN('planning','executing','verifying') AND m.grant_enabled=1 AND m.uncertain=0")?
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for (tenant, run) in stable {
        requeue_stable(c, &tenant, &run)?;
    }
    c.execute(
        "UPDATE missions SET state='queued' WHERE state IN('planning','executing','verifying') AND EXISTS(SELECT 1 FROM chat_runs r WHERE r.tenant_id=missions.tenant_id AND r.id=missions.run_id AND r.state='queued')",
        [],
    )?;
    Ok(())
}
type MissionRow = (
    String,
    i64,
    String,
    String,
    String,
    String,
    Option<String>,
    String,
    bool,
    i64,
    String,
);
fn view(c: &Connection, t: &str, id: &str) -> Result<MissionView> {
    let raw:MissionRow=c.query_row("SELECT id,revision,state,payload,steering,plan,candidate,report,uncertain,expires_ms,run_id FROM missions WHERE tenant_id=?1 AND id=?2",params![t,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?,r.get(9)?,r.get(10)?))).optional()?.ok_or(StoreError::NotFound)?;
    // Withdrawal erases derived visible text even for a historical achieved outcome.
    if raw.2 == "deleted" {
        return Err(StoreError::NotFound);
    }
    let invalid = raw.2 == "quarantined";
    let completed_steps=c.query_row("SELECT count(*) FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND result IS NOT NULL",params![t,raw.10],|r|r.get(0))?;
    let candidate: Option<MissionCandidate> = if invalid {
        None
    } else {
        raw.6.as_deref().map(parse).transpose()?
    };
    let mut receipts = Vec::new();
    if let Some(candidate) = &candidate {
        for e in &candidate.evidence {
            let raw:Option<String>=c.query_row("SELECT t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed'",params![t,raw.10,e.step,e.call_id],|r|r.get(0)).optional()?.flatten();
            if let Some(raw) = raw {
                let receipt: KnowledgeToolResult = parse(&raw)?;
                let (label, detail) = match receipt {
                    KnowledgeToolResult::Decimal { exact } => ("정확한 계산".into(), exact),
                    KnowledgeToolResult::Source(s) => (
                        format!("{} · revision {}", s.source.title, s.source.revision),
                        format!("{}\n{}", s.source.id, s.text),
                    ),
                    KnowledgeToolResult::Memory(m) => (
                        "보관된 기억".into(),
                        format!("{}\n{}", m.draft.subject, m.draft.text),
                    ),
                    KnowledgeToolResult::Skill(s) => (
                        "읽은 지침".into(),
                        format!("{} {}", s.pin.id, s.pin.revision),
                    ),
                    KnowledgeToolResult::Search(s) => (
                        "검색한 자료".into(),
                        format!(
                            "{}{}",
                            s.hits
                                .iter()
                                .map(|h| format!("{}\n{}", h.title, h.snippet))
                                .collect::<Vec<_>>()
                                .join("\n\n"),
                            if s.truncated || s.next_cursor.is_some() {
                                " · 일부 결과만 확인됨"
                            } else {
                                ""
                            }
                        ),
                    ),
                    KnowledgeToolResult::External(v) => {
                        let page = v.get("web_page");
                        let detail = if let Some(page) = page {
                            format!(
                                "{}\n{}\n{}",
                                page["title"].as_str().unwrap_or(""),
                                page["url"].as_str().unwrap_or(""),
                                page["text"].as_str().unwrap_or("")
                            )
                        } else {
                            v.pointer("/web_search/hits")
                                .and_then(|v| v.as_array())
                                .map(|hits| {
                                    hits.iter()
                                        .map(|h| {
                                            format!(
                                                "{}\n{}\n{}",
                                                h["title"].as_str().unwrap_or(""),
                                                h["url"].as_str().unwrap_or(""),
                                                h["description"].as_str().unwrap_or("")
                                            )
                                        })
                                        .collect::<Vec<_>>()
                                        .join("\n\n")
                                })
                                .unwrap_or_default()
                        };
                        ("공개 웹 관측".into(), detail)
                    }
                    KnowledgeToolResult::Error(_) => continue,
                };
                receipts.push(MissionReceipt {
                    label,
                    detail: detail.chars().take(4096).collect(),
                });
            }
        }
    }
    Ok(MissionView {
        id: raw.0,
        revision: raw.1,
        state: if invalid { "quarantined".into() } else { raw.2 },
        request: parse(&raw.3)?,
        steering: parse(&raw.4)?,
        plan: parse(&raw.5)?,
        completed_steps,
        result: candidate,
        receipts,
        report: if invalid {
            vec!["Evidence was withdrawn. Create a new task using current sources.".into()]
        } else {
            parse(&raw.7)?
        },
        uncertain: raw.8,
        expires_at_ms: raw.9,
    })
}
/// Typed response data from an acknowledged model step. It grants no authority.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionPendingRound {
    pub step: i64,
    pub planning: bool,
    pub text: String,
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct MissionRoundInput {
    kind: String,
    planning: bool,
    text: String,
}
pub(crate) fn record_round(c: &Connection, run: &Run, step: i64, bytes: &[u8]) -> Result<()> {
    if !is_mission(c, &run.tenant, &run.id)? {
        return Ok(());
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| StoreError::InvalidInput)?;
    if value.get("kind").and_then(|v| v.as_str()) != Some("mission_round") {
        return Ok(());
    }
    let proposal: MissionRoundInput =
        serde_json::from_value(value).map_err(|_| StoreError::InvalidInput)?;
    if proposal.kind != "mission_round" || proposal.text.len() > 24000 {
        return Err(StoreError::InvalidInput);
    }
    let record = MissionPendingRound {
        step,
        planning: proposal.planning,
        text: proposal.text,
    };
    c.execute(
        "UPDATE missions SET round_text=?3 WHERE tenant_id=?1 AND run_id=?2",
        params![run.tenant, run.id, json(&record)?],
    )?;
    Ok(())
}
/// A completed final response needs only local receipt verification, not spare
/// dispatch capacity. Never admit pending tools or an unknown/started effect on
/// this exception; the caller still checks authority/provider/deadline fences.
pub(crate) fn local_final_recovery(c: &Connection, tenant: &str, run: &str) -> Result<bool> {
    let raw: Option<String> = c.query_row(
        "SELECT round_text FROM missions WHERE tenant_id=?1 AND run_id=?2 AND uncertain=0 AND grant_enabled=1 AND state='queued'",
        params![tenant, run], |r| r.get(0),
    ).optional()?.flatten();
    let Some(raw) = raw else {
        return Ok(false);
    };
    let pending: MissionPendingRound = parse(&raw)?;
    if pending.planning || pending.step < 1 {
        return Ok(false);
    }
    let Ok(candidate) = serde_json::from_str::<MissionCandidate>(&pending.text) else {
        return Ok(false);
    };
    if candidate.text.trim().is_empty()
        || candidate.text.len() > 24000
        || candidate.evidence.len() > 12
    {
        return Ok(false);
    }
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='completed' AND step=(SELECT max(step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2)) AND NOT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state IN('dispatched','unknown')) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND result IS NULL) AND NOT EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started')",
        params![tenant, run, pending.step], |r| r.get(0),
    )?)
}
/// The execution projection contains no credentials or session capability.
pub struct MissionExecution {
    pub view: MissionView,
    pub receipts: serde_json::Value,
    pub repairs: i64,
    pub pending_round: Option<MissionPendingRound>,
}
pub(crate) fn apply_delete(c: &Connection, t: &str, id: &str) -> Result<()> {
    let thread: Option<String> = c
        .query_row(
            "SELECT thread_id FROM missions WHERE tenant_id=?1 AND id=?2",
            params![t, id],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(thread) = thread {
        c.execute("UPDATE missions SET state='deleted',grant_enabled=0,payload='',steering='[]',plan='[]',candidate=NULL,report='[]',round_text=NULL WHERE tenant_id=?1 AND id=?2",params![t,id])?;
        c.execute(
            "DELETE FROM mission_revisions WHERE tenant_id=?1 AND mission_id=?2",
            params![t, id],
        )?;
        c.execute(
            "DELETE FROM mission_controls WHERE tenant_id=?1 AND mission_id=?2",
            params![t, id],
        )?;
        c.execute(
            "DELETE FROM threads WHERE tenant_id=?1 AND id=?2",
            params![t, thread],
        )?;
    }
    Ok(())
}
impl Store {
    pub async fn mission_tool_failure(&self, lease: &crate::StepLease) -> Result<bool> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=crate::agent_execution::guard_step(&tx,&db.provider_key,&lease)?;
            let repairs:i64=tx.query_row("SELECT repairs FROM missions WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
            let inputs=tx.prepare("SELECT input FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3")?.query_map(params![run.tenant,run.id,lease.step],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            let unsupported=inputs.iter().map(|raw|parse::<KnowledgeToolInput>(raw)).collect::<Result<Vec<_>>>()?.iter().any(|input|!matches!(input,KnowledgeToolInput::InvalidArguments) && !tool_allowed(input));
            let stop=unsupported || repairs>=2;
            let report=if unsupported {"This request needs an unsupported action. Choose a task using supported read tools."}else{"A tool failed. Narrow the request without changing original checks."};
            let state=if unsupported {"unsupported"}else if stop {"failed"}else{"executing"};
            tx.execute("UPDATE missions SET repairs=min(2,repairs+?3),state=?4,report=?5,round_text=NULL WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,if unsupported{0}else{1},state,json(&vec![report])?])?;
            if stop {tx.execute("UPDATE chat_runs SET state='failed' WHERE tenant_id=?1 AND id=?2",params![run.tenant,run.id])?;}
            tx.commit()?;
            Ok(stop)
        }).await
    }

    pub async fn mission_clear_round(&self, lease: &crate::StepLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = crate::agent_execution::guard_step(&tx, &db.provider_key, &lease)?;
            tx.execute(
                "UPDATE missions SET round_text=NULL WHERE tenant_id=?1 AND run_id=?2",
                params![run.tenant, run.id],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    pub async fn delete_mission(&self, u: &UserContext, id: String) -> Result<()> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;let exists:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND account_id=?2 AND id=?3)",params![u.tenant.0,u.account.id,id],|r|r.get(0))?;if !exists{return Err(StoreError::NotFound)}crate::destructive_intents::commit(db,&u.tenant.0,crate::destructive_intents::Effect::DeleteMission{target:id})}).await
    }

    pub async fn mission_step_lease(
        &self,
        lease: &crate::RunLease,
        step: i64,
    ) -> Result<crate::StepLease> {
        let l = lease.clone();
        self.call(move|db|{let r=crate::chat::guard(&db.conn,&db.provider_key,&l)?;if !is_mission(&db.conn,&r.tenant,&r.id)?{return Err(StoreError::Forbidden)}db.conn.execute("UPDATE agent_steps SET attempt=?4 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='completed' AND NOT EXISTS(SELECT 1 FROM external_tool_dispatches d WHERE d.tenant_id=?1 AND d.run_id=?2 AND d.step=?3 AND d.state='started')",params![r.tenant,r.id,step,r.attempt])?;let bytes:Vec<u8>=db.conn.query_row("SELECT nonce FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='completed'",params![r.tenant,r.id,step],|r|r.get(0))?;Ok(crate::StepLease{run:l,step,nonce:bytes.try_into().map_err(|_|StoreError::Corrupt)?})}).await
    }

    pub async fn create_mission(
        &self,
        u: &UserContext,
        input: MissionCreate,
    ) -> Result<MissionView> {
        let u = u.clone();
        self.call(move|db|{
 let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&u,true,false)?;
 Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
 if input.task.trim().is_empty()||input.task.len()>8000||input.constraints.len()>4000||input.checks.is_empty()||input.checks.len()>12||json(&input)?.len()>16000{return Err(StoreError::InvalidInput)}
 for check in &input.checks {let valid=match check{MissionCheck::DecimalEquals{expected}=>!expected.is_empty()&&expected.len()<=128,MissionCheck::SourceQuote{id,revision,quote}=>Uuid::parse_str(id).is_ok()&&*revision>0&&!quote.is_empty()&&quote.len()<=4096,MissionCheck::OutputContains{text}=>!text.is_empty()&&text.len()<=1000,MissionCheck::UserReview{description}=>!description.trim().is_empty()&&description.len()<=1000};if !valid{return Err(StoreError::InvalidInput)}}
 let payload=json(&input)?;let digest=Sha256::digest(payload.as_bytes()).to_vec();
 if let Some((id,old))=tx.query_row("SELECT id,payload_digest FROM missions WHERE tenant_id=?1 AND request_id=?2",params![u.tenant.0,input.request_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,Vec<u8>>(1)?))).optional()?{if old!=digest{return Err(StoreError::Conflict)}return view(&tx,&u.tenant.0,&id)}
 let active:i64=tx.query_row("SELECT count(*) FROM missions WHERE tenant_id=?1 AND state IN('queued','planning','executing','verifying','paused')",[&u.tenant.0],|r|r.get(0))?;if active>=1{return Err(StoreError::Busy)}
 let global:i64=tx.query_row("SELECT count(*) FROM missions WHERE state IN('queued','planning','executing','verifying')",[],|r|r.get(0))?;if global>=16{return Err(StoreError::Busy)}
 let choice=providers::selected_choice(&tx,&u)?;if !choice.function_tools{return Err(StoreError::InvalidInput)}providers::binding_for_choice(&tx,&db.provider_key,&choice)?;
 let mut profile=crate::web::admitted_profile(&tx,&u,&db.web_profile)?;profile.mail_connections.clear();if let Some(w)=&mut profile.web{w.browser_pin=None;}profile.version=if profile.web.is_some(){3}else{1};
 let id=Uuid::new_v4().to_string();let thread=Uuid::new_v4().to_string();let run=Uuid::new_v4().to_string();let time=now()?;
 tx.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at,mission_internal) VALUES(?1,?2,?4,'Mission',?3,?3,1)",params![u.tenant.0,thread,time,u.account.id])?;
 tx.execute("UPDATE chat_admission_counter SET value=value+1 WHERE singleton=1",[])?;
 let order:i64=tx.query_row("SELECT value FROM chat_admission_counter WHERE singleton=1",[],|r|r.get(0))?;
 let mut model=serde_json::to_value(choice).map_err(|_|StoreError::InvalidInput)?;model["agent_profile"]=serde_json::to_value(profile).map_err(|_|StoreError::InvalidInput)?;
 tx.execute("INSERT INTO chat_runs(tenant_id,id,account_id,thread_id,request_id,payload_digest,sequence,user_text,model,session_digest,auth_epoch,attempt,state,version,created_at,admitted_order) VALUES(?1,?2,?3,?4,?5,?6,1,?7,?8,?9,?10,1,'queued',1,?11,?12)",params![u.tenant.0,run,u.account.id,thread,Uuid::new_v4().to_string(),digest,payload,json(&model)?,u.digest.as_slice(),u.epoch,time,order])?;
 let expiry=clock()?.checked_add(86400000).ok_or(StoreError::Unavailable)?;
 tx.execute("INSERT INTO missions(tenant_id,id,account_id,run_id,thread_id,request_id,payload,payload_digest,mode,auth_epoch,expires_ms) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",params![u.tenant.0,id,u.account.id,run,thread,input.request_id,payload,digest,match input.execution_mode{MissionMode::Durable=>"durable",MissionMode::CurrentSession=>"current_session"},u.epoch,expiry])?;
 tx.execute("INSERT INTO mission_revisions VALUES(?1,?2,1,?3,?4)",params![u.tenant.0,id,input.request_id,payload])?;
 tx.execute("INSERT INTO agent_execution(tenant_id,run_id,agent,deadline_ms) VALUES(?1,?2,1,?3)",params![u.tenant.0,run,expiry])?;
 let result=view(&tx,&u.tenant.0,&id)?;tx.commit()?;Ok(result)
 }).await
    }
    pub async fn missions(&self, u: &UserContext) -> Result<Vec<MissionView>> {
        let u = u.clone();
        self.call(move|db|{let u=validate(&db.conn,&u,true,false)?;db.conn.execute("UPDATE missions SET state='partial',grant_enabled=0,report='[\"The original execution period expired. Review the result or create a new task.\"]' WHERE tenant_id=?1 AND state IN('queued','planning','executing','verifying','paused') AND expires_ms<=?2",params![u.tenant.0,clock()?])?;let ids=db.conn.prepare("SELECT id FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state!='deleted' ORDER BY rowid DESC LIMIT 100")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;ids.iter().map(|id|view(&db.conn,&u.tenant.0,id)).collect()}).await
    }
    pub async fn control_mission(
        &self,
        u: &UserContext,
        id: String,
        input: MissionControl,
    ) -> Result<MissionView> {
        let u = u.clone();
        self.call(move|db|{
 let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&u,true,false)?;Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;let payload=json(&input)?;
 if let Some((old,response))=tx.query_row("SELECT payload,response FROM mission_controls WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![u.tenant.0,id,input.request_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?))).optional()?{if old!=payload{return Err(StoreError::Conflict)}let _=response;return view(&tx,&u.tenant.0,&id)}
 let current=view(&tx,&u.tenant.0,&id)?;if current.revision!=input.expected_revision{return Err(StoreError::Conflict)}
 let run:String=tx.query_row("SELECT run_id FROM missions WHERE tenant_id=?1 AND id=?2 AND account_id=?3",params![u.tenant.0,id,u.account.id],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)?;
 let dispatched:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state='dispatched') OR EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started')",params![u.tenant.0,run],|r|r.get(0))?;
 match &input.action {
 MissionAction::Pause|MissionAction::Cancel=>{if !matches!(current.state.as_str(),"queued"|"planning"|"executing"|"verifying"|"paused"){return Err(StoreError::Conflict)}let state=if matches!(input.action,MissionAction::Pause){"paused"}else{"cancelled"};tx.execute("UPDATE missions SET state=?3,uncertain=max(uncertain,?4),revision=revision+1,grant_enabled=CASE WHEN ?3='cancelled' THEN 0 ELSE grant_enabled END WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id,state,dispatched])?;tx.execute("UPDATE chat_runs SET state='cancelled' WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,run])?;},
 MissionAction::Resume|MissionAction::Steer{..}=>{
 if current.uncertain||dispatched||current.expires_at_ms<=clock()?||!matches!(current.state.as_str(),"paused"|"queued"){return Err(StoreError::Conflict)}
 let carrier=crate::chat::load_run(&tx,&u.tenant.0,&run)?;if carrier.epoch!=u.epoch{return Err(StoreError::Unauthorized)}providers::binding_for_choice(&tx,&db.provider_key,&carrier.model)?;crate::agent_execution::guard_execution(&tx,&carrier)?;
 let mut steering=current.steering;
 if let MissionAction::Steer{instruction}=&input.action{if instruction.trim().is_empty()||instruction.len()>2000||steering.len()>=8{return Err(StoreError::InvalidInput)}steering.push(instruction.clone());tx.execute("UPDATE missions SET round_text=NULL WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id])?;}
 requeue_stable(&tx,&u.tenant.0,&run)?;
 if tx.execute("UPDATE missions SET state='queued',revision=revision+1,steering=?3,candidate=NULL,report='[]' WHERE tenant_id=?1 AND id=?2 AND grant_enabled=1",params![u.tenant.0,id,json(&steering)?])?!=1{return Err(StoreError::Conflict)}
 if matches!(current.request.execution_mode,MissionMode::CurrentSession){tx.execute("UPDATE chat_runs SET session_digest=?3 WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,run,u.digest.as_slice()])?;}
 }
 }
 let result=view(&tx,&u.tenant.0,&id)?;
 tx.execute("INSERT INTO mission_revisions VALUES(?1,?2,?3,?4,?5)",params![u.tenant.0,id,result.revision,input.request_id,payload])?;
 tx.execute("INSERT INTO mission_controls VALUES(?1,?2,?3,?4,?5)",params![u.tenant.0,id,input.request_id,payload,"{}"])?;tx.commit()?;Ok(result)
 }).await
    }
    pub async fn mission_execution(
        &self,
        lease: &crate::RunLease,
    ) -> Result<Option<MissionExecution>> {
        let l = lease.clone();
        self.call(move|db|{if !is_mission(&db.conn,&l.tenant,&l.run)?{return Ok(None)}let r=crate::chat::guard(&db.conn,&db.provider_key,&l)?;let(id,repairs):(String,i64)=db.conn.query_row("SELECT id,repairs FROM missions WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
 let receipts=db.conn.prepare("SELECT step,call_id,input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 ORDER BY step,call_id LIMIT 12")?.query_map(params![r.tenant,r.id],|r|Ok(serde_json::json!({"step":r.get::<_,i64>(0)?,"call_id":r.get::<_,String>(1)?,"input":r.get::<_,Option<String>>(2)?,"result":r.get::<_,Option<String>>(3)?})))?.collect::<std::result::Result<Vec<_>,_>>()?;
 Ok(Some(MissionExecution{view:view(&db.conn,&r.tenant,&id)?,receipts:serde_json::json!(receipts),repairs,pending_round:db.conn.query_row("SELECT round_text FROM missions WHERE tenant_id=?1 AND id=?2",params![r.tenant,id],|r|r.get::<_,Option<String>>(0))?.as_deref().map(parse).transpose()?}))}).await
    }
    pub async fn mission_save_plan(
        &self,
        lease: &crate::StepLease,
        plan: Vec<String>,
    ) -> Result<()> {
        let l = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&l)?;if plan.is_empty()||plan.len()>5||plan.iter().any(|s|s.trim().is_empty()||s.len()>300){return Err(StoreError::InvalidInput)}tx.execute("UPDATE missions SET plan=?3,state='executing',round_text=NULL WHERE tenant_id=?1 AND run_id=?2 AND plan='[]'",params![r.tenant,r.id,json(&plan)?])?;tx.commit()?;Ok(())}).await
    }
    /// Evidence references are resolved exclusively against this run's committed typed slots.
    pub async fn mission_verify(
        &self,
        lease: &crate::StepLease,
        candidate: MissionCandidate,
    ) -> Result<bool> {
        let l = lease.clone();
        self.call(move|db|{
 let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let r=crate::agent_execution::guard_step(&tx,&db.provider_key,&l)?;
 if candidate.text.trim().is_empty()||candidate.text.len()>24000||candidate.evidence.len()>12{return Err(StoreError::InvalidInput)}
 let(id,payload,repairs):(String,String,i64)=tx.query_row("SELECT id,payload,repairs FROM missions WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;let input:MissionCreate=parse(&payload)?;
 let mut evidence=Vec::new();let mut report=Vec::new();let mut unknown=false;
 for e in &candidate.evidence {
 let raw:Option<String>=tx.query_row("SELECT t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed'",params![r.tenant,r.id,e.step,e.call_id],|r|r.get(0)).optional()?.flatten();
 match raw {Some(raw)=>{let v:KnowledgeToolResult=parse(&raw)?;if matches!(v,KnowledgeToolResult::Error(crate::ToolReadError::OutcomeUnknown)){unknown=true;}
if matches!(v,KnowledgeToolResult::Error(_)){report.push(format!("Receipt {}:{} is a failed tool result",e.step,e.call_id));}else{evidence.push(v)}},None=>report.push(format!("Missing receipt {}:{}",e.step,e.call_id))}
 }
 let mut passed=0;
 for (i,check) in input.checks.iter().enumerate(){let valid=match check {
 MissionCheck::DecimalEquals{expected}=>candidate.text.contains(expected)&&evidence.iter().any(|v|matches!(v,KnowledgeToolResult::Decimal{exact} if exact==expected)),
 MissionCheck::SourceQuote{id,revision,quote}=>candidate.text.contains(quote)&&evidence.iter().any(|v|matches!(v,KnowledgeToolResult::Source(s) if s.source.id==*id && s.source.revision==*revision && s.text.contains(quote))),
 MissionCheck::OutputContains{text}=>candidate.text.contains(text)&&!evidence.is_empty(),
 MissionCheck::UserReview{..}=>false,
 };if valid{passed+=1}else{report.push(format!("Check {} needs valid evidence or owner review",i+1));}}
 let implicit_review=!input.constraints.trim().is_empty()&&!input.checks.iter().any(|c|matches!(c,MissionCheck::UserReview{..}));
 if implicit_review{report.push("Additional constraints need owner review".into());}
 let achieved=report.is_empty()&&!unknown&&!evidence.is_empty();
 if evidence.is_empty(){report.push("No current-run evidence. A model's declaration is insufficient.".into())}
 let reviews=input.checks.iter().filter(|c|matches!(c,MissionCheck::UserReview{..})).count();
 let review_count=reviews+usize::from(implicit_review);
 let review_only=review_count>0 && passed+reviews==input.checks.len() && report.len()==review_count && !evidence.is_empty();
 let terminal=achieved||review_only||repairs>=2||unknown;
 let state=if unknown{"unknown"}else if achieved{"achieved"}else if passed>0 || (!evidence.is_empty() && input.checks.iter().any(|c|matches!(c,MissionCheck::UserReview{..}))){"partial"}else if evidence.is_empty(){"missing_evidence"}else{"failed"};
 tx.execute("UPDATE missions SET candidate=?3,report=?4,state=?5,round_text=NULL,repairs=repairs+?6,uncertain=max(uncertain,?7) WHERE tenant_id=?1 AND id=?2",params![r.tenant,id,json(&candidate)?,json(&report)?,if terminal{state}else{"executing"},if terminal{0}else{1},unknown])?;
 if terminal{tx.execute("UPDATE chat_runs SET state=?3 WHERE tenant_id=?1 AND id=?2",params![r.tenant,r.id,if achieved{"completed"}else{"incomplete"}])?;}
 tx.commit()?;Ok(terminal)
 }).await
    }
}
#[cfg(test)]
mod tests;
