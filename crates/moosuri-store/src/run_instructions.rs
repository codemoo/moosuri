//! Durable current-owner guidance. Receipt text is private conversation content.
use super::*;
use crate::chat::{self, Run};
use moosuri_core::{
    ChatChange, RunInstructionReceipt, RunInstructionRequest, RunInstructionTarget,
};

pub(crate) fn supports(c: &Connection, r: &Run) -> Result<bool> {
    if !r.state.active() || !r.model.function_tools {
        return Ok(false);
    }
    if crate::missions::is_mission(c, &r.tenant, &r.id)?
        && !crate::missions::supports_live_instructions(c, r)?
    {
        return Ok(false);
    }
    Ok(c.query_row(
        "SELECT (EXISTS(SELECT 1 FROM run_instruction_channels WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3) OR EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND run_id=?2 AND state IN('queued','planning','executing','verifying') AND grant_enabled=1 AND uncertain=0 AND expires_ms>?4)) AND EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND remote_calls<6 AND (deadline_ms IS NULL OR deadline_ms>?4)) AND (SELECT count(*) FROM run_instructions WHERE tenant_id=?1 AND run_id=?2)<8",
        params![r.tenant,r.id,r.attempt,now()?*1000], |row|row.get(0),
    )?)
}
pub(crate) fn receipts(c: &Connection, r: &Run) -> Result<Vec<RunInstructionReceipt>> {
    if !r.state.active() {
        c.execute("UPDATE run_instructions SET state='closed',reason=coalesce(reason,'run_finished') WHERE tenant_id=?1 AND run_id=?2 AND state='received'",params![r.tenant,r.id])?;
    }
    c.prepare("SELECT request_id,attempt,sequence,text,state,reason FROM run_instructions WHERE tenant_id=?1 AND run_id=?2 ORDER BY sequence")?
        .query_map(params![r.tenant,r.id], |row| Ok(RunInstructionReceipt {
            request_id: row.get(0)?, run_id:r.id.clone(), thread_id:r.thread.clone(),
            attempt:row.get(1)?, sequence:row.get(2)?, text:row.get(3)?, state:row.get(4)?, reason:row.get(5)?,
        }))?.collect::<std::result::Result<Vec<_>,_>>().map_err(Into::into)
}
pub(crate) fn target(c: &Connection, r: &Run) -> Result<Option<RunInstructionTarget>> {
    let own: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND run_id=?2 AND state NOT IN('deleted','quarantined'))", params![r.tenant,r.id], |row|row.get(0))?;
    let carrier = if own {
        Some(r.id.clone())
    } else {
        c.query_row("SELECT m.run_id FROM mission_chat_commands p JOIN missions m ON m.tenant_id=p.tenant_id AND m.id=json_extract(p.receipt,'$.target.id') WHERE p.tenant_id=?1 AND p.run_id=?2 AND json_extract(p.receipt,'$.target.kind')='one_off' AND json_extract(p.receipt,'$.action')='create_now' AND m.account_id=?3 AND m.state NOT IN('deleted','quarantined')",params![r.tenant,r.id,r.account],|row|row.get::<_,String>(0)).optional()?
    };
    carrier
        .map(|id| {
            let actual = chat::load_run(c, &r.tenant, &id)?;
            Ok(RunInstructionTarget {
                run_id: actual.id.clone(),
                thread_id: actual.thread.clone(),
                attempt: actual.attempt,
                supports_instructions: supports(c, &actual)?,
            })
        })
        .transpose()
}
pub(crate) fn displayed(c: &Connection, r: &Run) -> Result<Vec<RunInstructionReceipt>> {
    if let Some(target) = target(c, r)? {
        receipts(c, &chat::load_run(c, &r.tenant, &target.run_id)?)
    } else {
        receipts(c, r)
    }
}
fn publish(c: &Connection, r: &Run) -> Result<()> {
    let mut ids = vec![r.id.clone()];
    let mut origins=c.prepare("SELECT p.run_id FROM mission_chat_commands p JOIN missions m ON m.tenant_id=p.tenant_id AND m.id=json_extract(p.receipt,'$.target.id') WHERE m.tenant_id=?1 AND m.run_id=?2 AND json_extract(p.receipt,'$.target.kind')='one_off' AND json_extract(p.receipt,'$.action')='create_now'")?.query_map(params![r.tenant,r.id],|row|row.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    ids.append(&mut origins);
    for id in ids {
        c.execute(
            "UPDATE chat_runs SET version=version+1 WHERE tenant_id=?1 AND id=?2",
            params![r.tenant, id],
        )?;
        let current = chat::load_run(c, &r.tenant, &id)?;
        chat::emit(
            c,
            &current,
            ChatChange::State {
                run: chat::view(c, &current)?,
            },
        )?;
    }
    Ok(())
}
pub(crate) fn pending(c: &Connection, r: &Run) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM run_instructions WHERE tenant_id=?1 AND run_id=?2 AND execution_attempt=?3 AND state='received')", params![r.tenant,r.id,r.attempt],|row|row.get(0))?)
}
pub(crate) fn assign(c: &Connection, r: &Run, step: i64, through: i64) -> Result<()> {
    c.execute("UPDATE run_instructions SET dispatch_step=?4 WHERE tenant_id=?1 AND run_id=?2 AND execution_attempt=?3 AND sequence<=?5 AND state='received' AND dispatch_step IS NULL",params![r.tenant,r.id,r.attempt,step,through])?;
    Ok(())
}
pub(crate) fn close(c: &Connection, r: &Run, reason: &str) -> Result<()> {
    if c.execute("UPDATE run_instructions SET state='closed',reason=?4 WHERE tenant_id=?1 AND run_id=?2 AND execution_attempt=?3 AND state='received'",params![r.tenant,r.id,r.attempt,reason])? > 0 { publish(c,r)?; }
    Ok(())
}
pub(crate) fn quarantine(c: &Connection) -> Result<()> {
    c.execute("DELETE FROM run_instruction_channels", [])?;
    c.execute("UPDATE run_instructions SET state='closed',reason='restore_quarantine' WHERE state='received'",[])?;
    Ok(())
}
impl Store {
    pub async fn applied_run_instructions(
        &self,
        lease: &RunLease,
    ) -> Result<Vec<RunInstructionReceipt>> {
        let lease = lease.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match chat::guard(&tx,&db.provider_key,&lease) {Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            let eligible=tx.prepare("SELECT sequence FROM run_instructions WHERE tenant_id=?1 AND run_id=?2 AND execution_attempt=?3 AND state='applied'")?.query_map(params![run.tenant,run.id,run.attempt],|row|row.get::<_,i64>(0))?.collect::<std::result::Result<std::collections::BTreeSet<_>,_>>()?;
            let result=receipts(&tx,&run)?.into_iter().filter(|r|eligible.contains(&r.sequence)).collect();
            tx.commit()?;Ok(result)
        }).await
    }
    pub async fn supersede_step_output(&self, lease: &StepLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match crate::agent_execution::guard_step(&tx,&db.provider_key,&lease) {Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            let items=tx.prepare("SELECT item FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item LIKE ?3")?.query_map(params![run.tenant,run.id,format!("step-{}-%",lease.step())],|row|row.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            if !items.is_empty() {chat::apply_patches(&tx,&run,items.into_iter().map(|item|OutputPatch::Phase{item,phase:Some(moosuri_core::MessagePhase::Commentary)}).collect())?;}
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn submit_run_instruction(
        &self,
        user: &UserContext,
        id: String,
        input: RunInstructionRequest,
    ) -> Result<RunInstructionReceipt> {
        let user = user.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let user=validate(&tx,&user,true,false)?;
            Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
            if input.text.trim().is_empty() || input.text.len()>2000 {return Err(StoreError::InvalidInput)}
            let run=chat::load_run(&tx,&user.tenant.0,&id)?;
            if run.account!=user.account.id {return Err(StoreError::NotFound)}
            let durable:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND run_id=?2 AND account_id=?3 AND mode='durable')",params![run.tenant,run.id,run.account],|row|row.get(0))?;
            if run.epoch!=user.epoch || (!durable && run.session!=user.digest) {return Err(StoreError::Unauthorized)}
            if let Some(old)=receipts(&tx,&run)?.into_iter().find(|r|r.request_id==input.request_id) {
                if old.attempt!=input.expected_attempt || old.text!=input.text {return Err(StoreError::Conflict)}
                return Ok(old);
            }
            if run.attempt!=input.expected_attempt || !supports(&tx,&run)? {return Err(StoreError::Conflict)}
            chat::execution_authority(&tx,&run)?;
            crate::agent_execution::guard_execution(&tx,&run)?;
            let sequence:i64=tx.query_row("SELECT count(*)+1 FROM run_instructions WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
            tx.execute("INSERT INTO run_instructions(tenant_id,run_id,request_id,attempt,execution_attempt,sequence,text,state,session_digest,auth_epoch) VALUES(?1,?2,?3,?4,?4,?5,?6,'received',?7,?8)",params![run.tenant,run.id,input.request_id,run.attempt,sequence,input.text,user.digest.as_slice(),user.epoch])?;
            publish(&tx,&run)?;
            let receipt=receipts(&tx,&run)?.pop().ok_or(StoreError::Corrupt)?;
            tx.commit()?;Ok(receipt)
        }).await
    }
    pub async fn run_instruction(
        &self,
        user: &UserContext,
        id: String,
        request: String,
    ) -> Result<RunInstructionReceipt> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let run = chat::load_run(&db.conn, &user.tenant.0, &id)?;
            if run.account != user.account.id {
                return Err(StoreError::NotFound);
            }
            receipts(&db.conn, &run)?
                .into_iter()
                .find(|r| r.request_id == request)
                .ok_or(StoreError::NotFound)
        })
        .await
    }
    /// Called only after typed local intent/credential branches have finished.
    pub async fn enable_run_instructions(&self, lease: &RunLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match chat::guard(&tx, &db.provider_key, &lease) {
                Ok(r) => r,
                Err(e) => {
                    tx.commit()?;
                    return Err(e);
                }
            };
            if !run.model.function_tools
                || crate::credential_chat::has(&tx, &run.tenant, &run.id)?
                || crate::native_task_mail::has(&tx, &run.tenant, &run.id)?
            {
                return Err(StoreError::Conflict);
            }
            tx.execute(
                "INSERT OR IGNORE INTO run_instruction_channels VALUES(?1,?2,?3)",
                params![run.tenant, run.id, run.attempt],
            )?;
            publish(&tx, &run)?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
    /// Read-only preparation; application is acknowledged after a valid round.
    pub async fn pending_run_instructions(
        &self,
        lease: &RunLease,
    ) -> Result<Vec<RunInstructionReceipt>> {
        let lease = lease.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match chat::guard(&tx,&db.provider_key,&lease) {Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            let eligible=tx.prepare("SELECT sequence FROM run_instructions WHERE tenant_id=?1 AND run_id=?2 AND execution_attempt=?3 AND dispatch_step IS NULL AND state='received'")?.query_map(params![run.tenant,run.id,run.attempt],|row|row.get::<_,i64>(0))?.collect::<std::result::Result<std::collections::BTreeSet<_>,_>>()?;
            let result=receipts(&tx,&run)?.into_iter().filter(|r|eligible.contains(&r.sequence)).collect();
            tx.commit()?;Ok(result)
        }).await
    }
    pub async fn apply_step_instructions(&self, lease: &StepLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match crate::agent_execution::guard_step(&tx,&db.provider_key,&lease) {Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            let completed:bool=tx.query_row("SELECT state='completed' FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",params![run.tenant,run.id,lease.step()],|r|r.get(0))?;
            if !completed {return Err(StoreError::Conflict)}
            if tx.execute("UPDATE run_instructions SET state='applied' WHERE tenant_id=?1 AND run_id=?2 AND execution_attempt=?3 AND dispatch_step=?4 AND state='received'",params![run.tenant,run.id,run.attempt,lease.step()])?>0 {publish(&tx,&run)?;}
            tx.commit()?;Ok(())
        }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn live_instruction_schema72_forward_rollback_and_cascade() {
        let c = Connection::open_in_memory().unwrap();
        c.execute_batch("PRAGMA foreign_keys=ON;CREATE TABLE chat_runs(tenant_id TEXT,id TEXT,PRIMARY KEY(tenant_id,id));INSERT INTO chat_runs VALUES('synthetic','original');PRAGMA user_version=71;").unwrap();
        let migration = include_str!("../../../migrations/assistant/0072_run_instructions.sql");
        c.execute_batch("BEGIN IMMEDIATE").unwrap();
        c.execute_batch(migration).unwrap();
        c.execute_batch("ROLLBACK").unwrap();
        assert_eq!(
            c.query_row("PRAGMA user_version", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            71
        );
        assert!(c.prepare("SELECT * FROM run_instructions").is_err());
        c.execute_batch(migration).unwrap();
        c.execute(
            "INSERT INTO run_instruction_channels VALUES('synthetic','original',1)",
            [],
        )
        .unwrap();
        c.execute("INSERT INTO run_instructions VALUES('synthetic','original','receipt',1,1,1,'exact owner text','received',NULL,zeroblob(32),1,NULL)",[]).unwrap();
        assert!(
            c.execute("UPDATE run_instructions SET text=?1", ["한".repeat(667)])
                .is_err()
        );
        c.execute("DELETE FROM chat_runs WHERE id='original'", [])
            .unwrap();
        assert_eq!(
            c.query_row("SELECT count(*) FROM run_instructions", [], |r| r
                .get::<_, i64>(0))
                .unwrap(),
            0
        );
        assert_eq!(
            c.query_row("SELECT count(*) FROM run_instruction_channels", [], |r| r
                .get::<_, i64>(
                0
            ))
            .unwrap(),
            0
        );
        assert!(
            !c.prepare("PRAGMA foreign_key_check")
                .unwrap()
                .exists([])
                .unwrap()
        );
    }
}
