//! Shared durable admission seam for asynchronous typed tools. No HTTP here.
use super::*;

use crate::agent_execution::{
    self, KnowledgeToolInput, KnowledgeToolResult, StepLease, ToolReadError,
};

use moosuri_core::ExternalToolRequest;

pub struct ExternalToolPermit {
    lease: StepLease,
    call_id: String,
    nonce: Vec<u8>,
    request: Box<ExternalToolRequest>,
    user: Option<UserContext>,
    browser_pin: Option<String>,
    thread_id: String,
}

impl ExternalToolPermit {
    pub fn thread_id(&self) -> &str {
        &self.thread_id
    }
    pub fn request(&self) -> &ExternalToolRequest {
        &self.request
    }

    pub fn browser_pin(&self) -> Option<&str> {
        self.browser_pin.as_deref()
    }
    pub fn user(&self) -> Option<&UserContext> {
        self.user.as_ref()
    }
}

pub enum ExternalToolStart {
    Complete(KnowledgeToolResult),
    Dispatch(ExternalToolPermit),
}

impl Store {
    pub async fn check_external_tool(&self, permit: &ExternalToolPermit) -> Result<()> {
        let lease = permit.lease.clone();
        let request = permit.request.as_ref().clone();

        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;

            let result =
                agent_execution::guard_step(&tx, &db.provider_key, &lease).and_then(|run| {
                    if let ExternalToolRequest::Web(request) = &request
                        && !crate::web::allowed(run.tool_profile.as_ref(), &db.web_profile, request)
                    {
                        return Err(StoreError::Conflict);
                    }
                    Ok(())
                });

            tx.commit()?;

            result
        })
        .await
    }

    pub async fn prepare_external_tool(
        &self,
        lease: &StepLease,
        call_id: String,
    ) -> Result<ExternalToolStart> {
        let lease = lease.clone();

        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match agent_execution::guard_step(&tx,&db.provider_key,&lease){
                Ok(r)=>r,Err(e)=>{
                    tx.commit()?;
                    return Err(e)}
            };
            let(input,saved):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
            if let Some(saved)=saved{
                return Ok(ExternalToolStart::Complete(parse(&saved)?))}
            let KnowledgeToolInput::External{
                request}
            =parse(&input)? else{
                return Err(StoreError::InvalidInput)};

            let started:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4)",params![run.tenant,run.id,lease.step,call_id],|r|r.get(0))?;

            if started {
                let result=KnowledgeToolResult::Error(ToolReadError::OutcomeUnknown);
                tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id,json(&result)?])?;
                tx.commit()?;
                return Ok(ExternalToolStart::Complete(result))}

            let used:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
            if used>=12{
                return Err(StoreError::BudgetExhausted)}

            let user=if crate::missions::is_mission(&tx,&run.tenant,&run.id)? {None}else{Some(crate::chat::authority(&tx,&run)?)};

            let valid=(!crate::missions::is_mission(&tx,&run.tenant,&run.id)? || crate::missions::tool_allowed(&KnowledgeToolInput::External{request:request.clone()})) && match &request {
                ExternalToolRequest::Mail(mail)=>run.tool_profile.as_ref().filter(|p|matches!(p.version,2..=5)).and_then(|p|p.mail_connections.iter().find(|p|p.id==mail.connection_id())).is_some_and(|pin|user.as_ref().is_some_and(|u|connection(&tx,u,&pin.id).is_ok_and(|c|c.status=="active" && c.epoch==pin.epoch && c.email==pin.email))),
                ExternalToolRequest::Web(request)=>crate::web::allowed(run.tool_profile.as_ref(),&db.web_profile,request),
            };

            let generation=knowledge::generation(&tx,&run.tenant)?;

            tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1,generation=?3 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,generation])?;

            if !valid {
                let result=KnowledgeToolResult::Error(ToolReadError::Unavailable);
                tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id,json(&result)?])?;
                tx.commit()?;
                return Ok(ExternalToolStart::Complete(result))}

            if let ExternalToolRequest::Mail(mail)=&request {
                let pin=run.tool_profile.as_ref().and_then(|p|p.mail_connections.iter().find(|p|p.id==mail.connection_id())).ok_or(StoreError::Conflict)?;
                tx.execute("INSERT OR IGNORE INTO mail_run_dependencies VALUES(?1,?2,?3,?4)",params![run.tenant,run.id,pin.id,pin.epoch])?;
            }

            let browser_pin=run.tool_profile.as_ref().filter(|p|matches!(p.version,4|5)).and_then(|p|p.web.as_ref()).and_then(|p|p.browser_pin.clone());
            let nonce=hash(&random()?);
            tx.execute("INSERT INTO external_tool_dispatches VALUES(?1,?2,?3,?4,?5,'started')",params![run.tenant,run.id,lease.step,call_id,nonce])?;
            tx.commit()?;
            Ok(ExternalToolStart::Dispatch(ExternalToolPermit{
                lease,call_id,nonce,request:Box::new(request),user,browser_pin,thread_id:run.thread}
            ))}
        ).await
    }

    pub async fn complete_external_tool(
        &self,
        permit: &ExternalToolPermit,
        mut result: KnowledgeToolResult,
    ) -> Result<KnowledgeToolResult> {
        let lease = permit.lease.clone();

        let call_id = permit.call_id.clone();

        let nonce = permit.nonce.clone();

        let request = permit.request.as_ref().clone();

        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=match agent_execution::guard_step(&tx,&db.provider_key,&lease){
                Ok(r)=>r,Err(e)=>{
                    tx.commit()?;
                    return Err(e)}
            };
            let user=if crate::missions::is_mission(&tx,&run.tenant,&run.id)? {None}else{Some(crate::chat::authority(&tx,&run)?)};
            let valid=(!crate::missions::is_mission(&tx,&run.tenant,&run.id)? || crate::missions::tool_allowed(&KnowledgeToolInput::External{request:request.clone()})) && match &request {
                ExternalToolRequest::Mail(mail)=>run.tool_profile.as_ref().filter(|p|matches!(p.version,2..=5)).and_then(|p|p.mail_connections.iter().find(|p|p.id==mail.connection_id())).is_some_and(|pin|user.as_ref().is_some_and(|u|connection(&tx,u,&pin.id).is_ok_and(|c|c.epoch==pin.epoch&&c.email==pin.email&&c.status=="active"))),
                ExternalToolRequest::Web(request)=>crate::web::allowed(run.tool_profile.as_ref(),&db.web_profile,request),
            };
            if !valid {
                agent_execution::purge_runs(&tx,&run.tenant,vec![run.id.clone()])?;
                tx.commit()?;
                return Err(StoreError::Conflict)
            }

            if json(&result)?.len()>agent_execution::TOOL_RESULT_LIMIT{
                result=KnowledgeToolResult::Error(ToolReadError::TooLargeNarrowRead)}
            let(saved,matching):(Option<String>,bool)=tx.query_row("SELECT t.result,d.nonce=?5 FROM agent_tool_slots t JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4",params![run.tenant,run.id,lease.step,call_id,nonce],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
            if !matching{
                return Err(StoreError::Unauthorized)}

            if let Some(saved)=saved{
                return parse(&saved)}
            tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id,json(&result)?])?;
            tx.execute("UPDATE external_tool_dispatches SET state='completed' WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step,call_id])?;
            tx.execute("UPDATE agent_execution SET checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
            tx.commit()?;
            Ok(result)}
        ).await
    }
}
