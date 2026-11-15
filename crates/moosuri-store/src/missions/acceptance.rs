//! Owner-written acceptance snapshots; never model proposals or expanded grants.
use super::*;
use moosuri_core::{MissionAcceptanceSnapshot, MissionAcceptanceUpdate};

pub(crate) fn floor(c: &Connection, tenant: &str, run: &str) -> Result<i64> {
    Ok(c.query_row("SELECT coalesce(max(a.first_step),1) FROM mission_acceptances a JOIN missions m ON m.tenant_id=a.tenant_id AND m.id=a.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2",params![tenant,run],|r|r.get(0))?)
}
pub(super) fn revision(c: &Connection, tenant: &str, run: &str) -> Result<i64> {
    Ok(c.query_row("SELECT coalesce(max(a.revision),1) FROM mission_acceptances a JOIN missions m ON m.tenant_id=a.tenant_id AND m.id=a.mission_id WHERE m.tenant_id=?1 AND m.run_id=?2",params![tenant,run],|r|r.get(0))?)
}
pub(super) fn history(
    c: &Connection,
    tenant: &str,
    id: &str,
) -> Result<Vec<MissionAcceptanceSnapshot>> {
    c.prepare("SELECT revision,payload FROM mission_acceptances WHERE tenant_id=?1 AND mission_id=?2 ORDER BY revision")?.query_map(params![tenant,id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.map(|v|{let(revision,raw)=v?;Ok(MissionAcceptanceSnapshot{revision,request:parse(&raw)?})}).collect()
}
pub(super) fn current(c: &Connection, tenant: &str, run: &str) -> Result<MissionCreate> {
    let raw:String=c.query_row("SELECT coalesce((SELECT a.payload FROM mission_acceptances a WHERE a.tenant_id=m.tenant_id AND a.mission_id=m.id ORDER BY a.revision DESC LIMIT 1),m.payload) FROM missions m WHERE tenant_id=?1 AND run_id=?2",params![tenant,run],|r|r.get(0))?;
    parse(&raw)
}
pub(super) fn check(c: &Connection, run: &Run, step: i64, expected: i64) -> Result<()> {
    if expected != revision(c, &run.tenant, &run.id)? || step < floor(c, &run.tenant, &run.id)? {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
impl Store {
    pub async fn revise_mission_acceptance(
        &self,
        user: &UserContext,
        id: String,
        input: MissionAcceptanceUpdate,
    ) -> Result<MissionView> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u=validate(&tx,&user,true,false)?;
            Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
            let run:String=tx.query_row("SELECT run_id FROM missions WHERE tenant_id=?1 AND id=?2 AND account_id=?3",params![u.tenant.0,id,u.account.id],|r|r.get(0)).optional()?.ok_or(StoreError::NotFound)?;
            let operation=json(&input)?;
            let old:Option<String>=tx.query_row("SELECT operation FROM mission_acceptances WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![u.tenant.0,id,input.request_id],|r|r.get(0)).optional()?;
            if let Some(old)=old {if old!=operation{return Err(StoreError::Conflict)}return view(&tx,&u.tenant.0,&id)}
            let before=view(&tx,&u.tenant.0,&id)?;
            if before.revision!=input.expected_revision || before.state!="paused" || before.uncertain {return Err(StoreError::Conflict)}
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM recurring_mission_occurrences WHERE tenant_id=?1 AND mission_id=?2)",params![u.tenant.0,id],|r|r.get::<_,bool>(0))? {return Err(StoreError::Forbidden)}
            preflight_control_guard(&tx,&db.provider_key,&u,&moosuri_core::MissionChatTarget{kind:"mission".into(),id:id.clone(),revision:before.revision})?;
            let carrier=crate::chat::load_run(&tx,&u.tenant.0,&run)?;
            if !connected::current(&tx,&carrier)? {return Err(StoreError::Conflict)}
            actions::require_revisable(&tx,&carrier)?;
            if matches!(before.request.execution_mode,MissionMode::CurrentSession) {crate::chat::authority(&tx,&carrier)?;if carrier.session!=u.digest{return Err(StoreError::Unauthorized)}}
            let mut next=before.request.clone();next.task=input.task;next.constraints=input.constraints;next.checks=input.checks;
            validate_input(&next)?;
            if reports::requested(&next).is_some() && !reports::originally_requested(&tx,&carrier)? {return Err(StoreError::Forbidden)}
            let text=format!("{}\n{}",next.task,next.constraints);
            let (mail,calendar)=moosuri_core::mission_read_domains(&text);
            let connected=carrier.tool_profile.as_ref().and_then(|p|p.mission_reads.as_ref());
            let native=carrier.tool_profile.as_ref().is_some_and(|p|p.native_mail_model_read&&p.native_mail_scope.as_ref().is_some_and(|s|s.version==1&&s.operation==8));
            if connected::unsupported_effect(&text) || mail && !native && connected.is_none_or(|p|p.mail.is_empty()) || calendar && connected.is_none_or(|p|p.calendar.is_empty()) {return Err(StoreError::Forbidden)}
            let public_web=text.contains("https://") || text.contains("http://") || text.to_lowercase().contains("public web") || text.contains("웹");
            if public_web && carrier.tool_profile.as_ref().and_then(|p|p.web.as_ref()).is_none_or(|p|!p.read) {return Err(StoreError::Forbidden)}
            // A revision cannot turn unavailable execution modes into a read plan.
            let lower=text.to_lowercase();
            if ["browser", "브라우저", "vault", "볼트", "shell", "셸", "슬랙 발송", "slack send"].iter().any(|v|lower.contains(v)) {return Err(StoreError::Forbidden)}
            let delivery=lower.replace("do not send", "").replace("don't send", "").replace("never send", "");
            let words=delivery.split(|c:char|!c.is_ascii_alphabetic()).collect::<Vec<_>>();
            let sending=words.iter().any(|w|matches!(*w,"send"|"forward"|"upload"|"post")) || ["보내줘","보내 줘","발송해","전송해"].iter().any(|v|delivery.contains(v));
            let local=(words.contains(&"here") || delivery.contains("여기")) && !delivery.contains('@');
            if sending && !local {return Err(StoreError::Forbidden)}
            let rev=revision(&tx,&u.tenant.0,&run)?;
            if rev>=9{return Err(StoreError::Busy)}
            if rev==1 {
                let original:String=tx.query_row("SELECT payload FROM missions WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id],|r|r.get(0))?;
                let first:MissionCreate=parse(&original)?;
                tx.execute("INSERT INTO mission_acceptances VALUES(?1,?2,1,?3,?4,?4,1)",params![u.tenant.0,id,first.request_id,original])?;
            }
            let first:i64=tx.query_row("SELECT coalesce(max(step),0)+1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2",params![u.tenant.0,run],|r|r.get(0))?;
            tx.execute("INSERT INTO mission_acceptances VALUES(?1,?2,?3,?4,?5,?6,?7)",params![u.tenant.0,id,rev+1,input.request_id,operation,json(&next)?,first])?;
            // Old callbacks fail even when the legacy attempt counter is saturated.
            tx.execute("DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",params![u.tenant.0,run])?;
            requeue_stable(&tx,&u.tenant.0,&run)?;
            tx.execute("UPDATE missions SET revision=revision+1,state='queued',steering='[]',plan='[]',candidate=NULL,report='[]',round_text=NULL WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id])?;
            tx.execute("UPDATE agent_execution SET checkpoint_seq=checkpoint_seq+1 WHERE tenant_id=?1 AND run_id=?2",params![u.tenant.0,run])?;
            let out=view(&tx,&u.tenant.0,&id)?;tx.commit()?;Ok(out)
        }).await
    }
}
