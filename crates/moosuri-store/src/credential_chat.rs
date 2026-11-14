//! One terminal typed GitHub read per explicit chat run. No HTTP or model polling.
use super::*;
use crate::agent_execution::{KnowledgeToolInput, KnowledgeToolResult, StepLease};
use crate::chat::{self, Run};
use moosuri_core::{CredentialChatStatus, MessagePhase, RunState};
use moosuri_vault_protocol::broker::{
    GitHubRepository, Operation, Outcome, Projection, SignedReceipt,
};
fn encode<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::Corrupt)
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
pub(crate) fn has(c: &Connection, tenant: &str, run: &str) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2)",
        params![tenant, run],
        |r| r.get(0),
    )?)
}
pub(crate) fn status(c: &Connection, r: &Run) -> Result<Option<CredentialChatStatus>> {
    let row:Option<(String,String,String)>=c.query_row("SELECT request_id,repository,status FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|v|Ok((v.get(0)?,v.get(1)?,v.get(2)?))).optional()?;
    row.map(|(request_id, repo, status)| {
        let repo: GitHubRepository = decode(&repo)?;
        Ok(CredentialChatStatus {
            request_id,
            repository: format!("{}/{}", repo.owner, repo.repository),
            status: if status == "settled" {
                match r.state {
                    RunState::Completed => "complete",
                    RunState::OutcomeUnknown => "outcome_unknown",
                    _ => "not_completed",
                }
                .into()
            } else {
                status
            },
        })
    })
    .transpose()
}
pub(crate) fn cancel(c: &Connection, r: &Run) -> Result<()> {
    if !has(c, &r.tenant, &r.id)? {
        return Ok(());
    }
    c.execute("UPDATE credential_broker_jobs SET expires_at=min(expires_at,?3) WHERE tenant_id=?1 AND run_id=?2 AND EXISTS(SELECT 1 FROM credential_chat_waits w WHERE w.tenant_id=?1 AND w.run_id=?2 AND w.status='waiting')",params![r.tenant,r.id,now()?])?;
    c.execute("UPDATE credential_broker_invocations SET cancelled=1 WHERE tenant_id=?1 AND run_id=?2 AND EXISTS(SELECT 1 FROM credential_chat_waits w WHERE w.tenant_id=?1 AND w.run_id=?2 AND w.status='waiting')",params![r.tenant,r.id])?;
    c.execute("UPDATE credential_chat_waits SET status='cancelled' WHERE tenant_id=?1 AND run_id=?2 AND status='waiting'",params![r.tenant,r.id])?;
    Ok(())
}
pub(crate) fn quarantine(c: &Connection) -> Result<()> {
    c.execute_batch("UPDATE credential_broker_jobs SET expires_at=min(expires_at,unixepoch()) WHERE (tenant_id,run_id) IN(SELECT tenant_id,run_id FROM credential_chat_waits);UPDATE credential_broker_invocations SET cancelled=1 WHERE (tenant_id,run_id) IN(SELECT tenant_id,run_id FROM credential_chat_waits);UPDATE credential_chat_waits SET status='cancelled' WHERE status='waiting';")?;
    Ok(())
}
fn literal_target(text: &str, repo: &GitHubRepository) -> bool {
    let target = format!("{}/{}", repo.owner, repo.repository);
    let text = text.to_ascii_lowercase();
    text.match_indices(&target).any(|(at, _)| {
        let end = at + target.len();
        let valid = |b: u8| b.is_ascii_alphanumeric() || b"._-/".contains(&b);
        (at == 0 || !valid(text.as_bytes()[at - 1]) || text[..at].ends_with("https://github.com/"))
            && (end == text.len() || !valid(text.as_bytes()[end]))
    })
}
impl Store {
    /// Called only for an already stored current model tool slot. Returns false for
    /// unsupported/mixed plans, so the ordinary bounded tool error path can answer.
    pub async fn wait_for_github_tool(&self, lease: &StepLease, call_id: String) -> Result<bool> {
        let lease = lease.clone();
        self.call(move|db|{
   let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
   let run=match agent_execution::guard_step(&tx,&db.provider_key,&lease){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
   if !cfg!(feature="credential-broker-chat") || run.tool_profile.as_ref().is_none_or(|p|!p.credential_github) || crate::missions::is_mission(&tx,&run.tenant,&run.id)? || crate::media::has_context(&tx,&run.tenant,&run.id)? || crate::artifacts::agent::profile(&tx,&run.tenant,&run.id)?.is_some_and(|p|p.write) {return Ok(false)}
   let (input,saved):(String,Option<String>)=tx.query_row("SELECT input,result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![run.tenant,run.id,lease.step(),call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::NotFound)?;
   if saved.is_some() {return Ok(false)}
   let KnowledgeToolInput::CredentialGitHub{owner,repository}=decode(&input)? else{return Ok(false)};
   let repo=GitHubRepository{owner,repository};if !repo.valid()||!literal_target(&run.text,&repo){return Ok(false)}
   let (calls,used):(i64,i64)=tx.query_row("SELECT (SELECT count(*) FROM agent_tool_slots t WHERE t.tenant_id=?1 AND t.run_id=?2),tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
   if calls!=1||used!=0||has(&tx,&run.tenant,&run.id)? {return Ok(false)}
   let (bytes,items):(i64,i64)=tx.query_row("SELECT coalesce(sum(length(CAST(text AS BLOB))),0),count(*) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
   if bytes>252*1024 || items>=127{return Ok(false)}

   // Run/session authority is actual admission, never a caller-selected user ID.
   let user=chat::authority(&tx,&run)?;
   // Waiting returns worker/model capacity but never extends the run's original
   // cumulative deadline. Native mint/commit/entry inherit this same expiry.
   let deadline:i64=tx.query_row("SELECT deadline_ms FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|r|r.get(0))?;
   let expires=deadline/1000;
   if expires<=now()?{return Ok(false)}
   let request_id=Uuid::new_v4().to_string();let broker_call=Uuid::new_v4().to_string();let nonce=Uuid::new_v4().to_string();let repository=encode(&repo)?;
   tx.execute("INSERT INTO credential_broker_jobs(tenant_id,account_id,request_id,run_id,call_id,invocation_nonce,repository,auth_epoch,expires_at,request,step) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,?10)",params![run.tenant,user.account.id,request_id,run.id,broker_call,nonce,repository,run.epoch,expires,lease.step()])?;
   tx.execute("INSERT INTO credential_chat_waits VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'waiting',?10)",params![run.tenant,run.id,run.attempt,lease.step(),call_id,broker_call,nonce,request_id,repository,expires])?;
   tx.execute("UPDATE agent_execution SET tool_calls=tool_calls+1,checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
   // A completed model plan stays durable; no new attempt/checkpoint or model is
   // started to wait for the device. Deleting this lease fences a retired runner.
   tx.execute("DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
   chat::transition(&tx,&run,RunState::Queued,None)?;
   tx.commit()?;Ok(true)
  }).await
    }
    /// Store-only result observation, bounded and slot-free. The narrow terminal
    /// result is formatted from the original signed projection, never another LLM.
    pub async fn settle_credential_chats(&self) -> Result<usize> {
        if !cfg!(feature = "credential-broker-chat") {
            return Ok(0);
        }
        self.call(|db|{
   let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
   let rows=tx.prepare("SELECT tenant_id,run_id,attempt,step,call_id,broker_call,invocation_nonce,request_id,repository,expires_at FROM credential_chat_waits WHERE status='waiting' LIMIT 32")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,i32>(2)?,r.get::<_,i64>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,String>(7)?,r.get::<_,String>(8)?,r.get::<_,i64>(9)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
   let mut changed=0;
   for (tenant,id,attempt,step,call,broker_call,nonce,request_id,repo,expires) in rows {
    let run=chat::load_run(&tx,&tenant,&id)?;
    if run.attempt!=attempt||run.state!=RunState::Queued {cancel(&tx,&run)?;changed+=1;continue}
    if chat::authority(&tx,&run).is_err(){chat::transition(&tx,&run,RunState::Cancelled,Some(RunFailure::AuthorityRevoked))?;changed+=1;continue}
    // No saved-source/other-tool plan was admitted for this terminal read. Still
    // honor explicit invalidation, and never fabricate a result for a missing slot.
    let invalid:bool=tx.query_row("SELECT invalidated FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![tenant,id],|r|r.get(0))?;
    if invalid {chat::transition(&tx,&run,RunState::Cancelled,Some(RunFailure::AuthorityRevoked))?;changed+=1;continue}
    let commitment:Option<Option<String>>=tx.query_row("SELECT receipt FROM credential_broker_commitments WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 AND invocation_nonce=?5",params![tenant,id,step,broker_call,nonce],|r|r.get(0)).optional()?;
    let repo:GitHubRepository=decode(&repo)?;
    let (text,result,state,status)=if let Some(Some(receipt))=commitment.as_ref() {
     let receipt:SignedReceipt=decode(receipt)?;let r=&receipt.receipt;let request=&r.commitment.request;
     let key:Vec<u8>=tx.query_row("SELECT public_key FROM credential_broker_devices WHERE id=?1 AND account_id=?2 AND tenant_id=?3",params![request.device.id.to_string(),run.account,tenant],|row|row.get(0))?;
     moosuri_credential_broker::auth::verify(&key,&moosuri_vault_protocol::broker::receipt_bytes(r),&receipt.signature).map_err(|_|StoreError::Unauthorized)?;
     let stored:String=tx.query_row("SELECT request FROM credential_broker_jobs WHERE tenant_id=?1 AND request_id=?2 AND run_id=?3",params![tenant,request_id,id],|row|row.get(0))?;
     let original:moosuri_vault_protocol::broker::Request=decode(&stored)?;
     if original!=*request{return Err(StoreError::Corrupt)}

     if request.operation!=Operation::GitHubRepositoryRead||request.repository.as_ref()!=Some(&repo)||request.device.owner.account.to_string()!=run.account||request.device.owner.tenant.to_string()!=tenant||request.device.owner.epoch!=run.epoch||request.invocation.run.to_string()!=id||request.invocation.step as i64!=step||request.invocation.call.to_string()!=broker_call||request.invocation.nonce.to_string()!=nonce {return Err(StoreError::Corrupt)}
     let (text,state,status)=match (r.outcome,r.projection) {
      (Outcome::ProjectedSuccess,Projection::GitHubRepository{id:repository_id,private,archived,stars,open_issues})=>(format!("GitHub {}/{}\nRepository ID: {}\nPrivate: {} · Archived: {}\nStars: {} · Open issues: {}\n신뢰 기기의 인증된 메타데이터 읽기 영수증으로 확인했습니다.",repo.owner,repo.repository,repository_id,private,archived,stars,open_issues),RunState::Completed,"complete"),
      (Outcome::Unknown,_) =>("GitHub 읽기 결과가 미확정입니다. 원래 요청의 영수증만 확인하며 다시 실행하지 않습니다.".into(),RunState::OutcomeUnknown,"outcome_unknown"),
      (Outcome::Rejected,_) =>("기기 credential이 거절되었습니다. 신뢰 기기 연결에서 credential을 갱신한 뒤 새로 요청하세요. 이 요청을 재전송하지 않았습니다.".into(),RunState::Incomplete,"credential_rejected"),
      _ =>("원래 GitHub 요청은 기기에서 실행되지 않았습니다. 다시 실행하지 않았으며, 필요하면 새 요청을 작성하세요.".into(),RunState::Incomplete,"not_dispatched"),
     };
     (text,KnowledgeToolResult::External(serde_json::json!({"service":"github","repository":repo,"request_id":request_id,"status":status,"projection":r.projection})),state,status)
    }else if expires<=now()? && commitment.is_some() {
     ("GitHub 읽기가 이미 시작됐을 수 있지만 영수증이 도착하지 않았습니다. 미확정으로 보존하며 다시 실행하지 않습니다.".into(),KnowledgeToolResult::Error(agent_execution::ToolReadError::OutcomeUnknown),RunState::OutcomeUnknown,"outcome_unknown")
    }else if expires<=now()? {
     ("GitHub credential 대기 시간이 만료되었습니다. 원래 요청을 실행하지 않았으며 다시 보내지 않습니다.".into(),KnowledgeToolResult::Error(agent_execution::ToolReadError::Unavailable),RunState::Incomplete,"expired")
    }else{continue};
    let old:Option<String>=tx.query_row("SELECT result FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![tenant,id,step,call],|r|r.get(0)).optional()?.ok_or(StoreError::Corrupt)?;
    if old.is_some(){return Err(StoreError::Corrupt)}
    tx.execute("UPDATE agent_tool_slots SET result=?5 WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4",params![tenant,id,step,call,encode(&result)?])?;
    tx.execute("UPDATE credential_chat_waits SET status='settled' WHERE tenant_id=?1 AND run_id=?2",params![tenant,id])?;
    chat::apply_patches(&tx,&run,vec![OutputPatch::Append{item:"credential-github-result".into(),text,phase:Some(MessagePhase::FinalAnswer),refusal:false}])?;
    chat::transition(&tx,&run,state,if state==RunState::OutcomeUnknown{Some(RunFailure::ConnectionFailed)}else{None})?;
    let _=status;changed+=1;
   }
   tx.commit()?;Ok(changed)
  }).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn credential_chat_literal_target_is_exact_and_accepts_the_canonical_github_url() {
        let repo = GitHubRepository {
            owner: "owner".into(),
            repository: "repo".into(),
        };
        assert!(literal_target("GitHub owner/repo metadata", &repo));
        assert!(literal_target("https://github.com/owner/repo 상태", &repo));
        for text in [
            "otherowner/repo metadata",
            "owner/repository",
            "https://evil.invalid/owner/repo",
            "owner/repo/issues",
        ] {
            assert!(!literal_target(text, &repo));
        }
    }
}
