//! Finite owner value waits on the original mission. These are not new requests,
//! acceptance revisions, authenticated carriers or external-effect journals.
use super::*;
use moosuri_core::{MissionInputField, MissionInputReply, MissionInputRequest, MissionInputWait};
fn field_name(field: &MissionInputField) -> &'static str {
    match field {
        MissionInputField::MailTo => "mail_to",
        MissionInputField::CalendarDate => "calendar_date",
        MissionInputField::CalendarStart => "calendar_start",
        MissionInputField::CalendarDuration => "calendar_duration",
    }
}
fn clock_count(text: &str) -> usize {
    let mut next = 0;
    let mut count = 0;
    for (i, _) in text.char_indices() {
        if i < next {
            continue;
        }
        if let Some((_, tail)) = crate::schedules::civil_time::clock_prefix(&text[i..]) {
            count += 1;
            next = text.len() - tail.len();
        }
    }
    count
}
pub(crate) fn view(c: &Connection, t: &str, id: &str) -> Result<Vec<MissionInputWait>> {
    let rows=c.prepare("SELECT request_id,field,question,status,value,acceptance_revision,expires_ms FROM mission_input_waits WHERE tenant_id=?1 AND mission_id=?2 ORDER BY step DESC")?.query_map(params![t,id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    rows.into_iter()
        .map(
            |(request_id, field, question, state, value, acceptance_revision, expires_at_ms)| {
                Ok(MissionInputWait {
                    request_id,
                    field: parse(&format!("\"{field}\""))?,
                    question,
                    state,
                    value,
                    acceptance_revision,
                    expires_at_ms,
                })
            },
        )
        .collect()
}
/// Only current-acceptance typed values can supply action parameters. The caller
/// still independently checks original owner intent, action kind and operation.
pub(crate) fn values(c: &Connection, r: &Run) -> Result<Vec<MissionInputWait>> {
    let id: String = c.query_row(
        "SELECT id FROM missions WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |r| r.get(0),
    )?;
    let revision = acceptance::revision(c, &r.tenant, &r.id)?;
    Ok(view(c, &r.tenant, &id)?
        .into_iter()
        .filter(|w| w.state == "resumed" && w.acceptance_revision == revision)
        .collect())
}
fn value(field: &MissionInputField, raw: &str) -> Result<String> {
    let raw = raw.trim();
    let valid = match field {
        MissionInputField::MailTo => {
            raw.len() <= 254
                && raw.is_ascii()
                && raw.split_once('@').is_some_and(|(local, domain)| {
                    !local.is_empty()
                        && !domain.is_empty()
                        && !domain.contains('@')
                        && domain.contains('.')
                        && local.bytes().all(|b| {
                            b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b)
                        })
                        && domain
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b".-".contains(&b))
                })
        }
        MissionInputField::CalendarDate => {
            raw.len() == 10 && chrono::NaiveDate::parse_from_str(raw, "%Y-%m-%d").is_ok()
        }
        MissionInputField::CalendarStart => {
            raw.len() == 5 && chrono::NaiveTime::parse_from_str(raw, "%H:%M").is_ok()
        }
        MissionInputField::CalendarDuration => {
            raw.bytes().all(|b| b.is_ascii_digit())
                && raw.parse::<u16>().is_ok_and(|v| (1..=1440).contains(&v))
        }
    };
    if !valid {
        return Err(StoreError::InvalidInput);
    }
    Ok(if *field == MissionInputField::CalendarDuration {
        format!(
            "{} minutes",
            raw.parse::<u16>().map_err(|_| StoreError::InvalidInput)?
        )
    } else {
        raw.into()
    })
}
pub(crate) fn expire(c: &Connection, time: i64) -> Result<()> {
    let rows=c.prepare("SELECT i.tenant_id,i.mission_id,i.run_id,i.request_id,i.expires_ms<=?1 OR m.expires_ms<=?1 OR coalesce(e.deadline_ms<=?1,0) FROM mission_input_waits i JOIN missions m ON m.tenant_id=i.tenant_id AND m.id=i.mission_id JOIN accounts a ON a.tenant_id=m.tenant_id AND a.id=m.account_id JOIN agent_execution e ON e.tenant_id=i.tenant_id AND e.run_id=i.run_id WHERE i.status IN('waiting','ready') AND (i.expires_ms<=?1 OR m.expires_ms<=?1 OR coalesce(e.deadline_ms<=?1,0) OR e.invalidated=1 OR m.state!='waiting' OR m.grant_enabled=0 OR m.uncertain=1 OR m.revision!=i.mission_revision OR a.auth_epoch!=m.auth_epoch OR a.status!='active' OR a.recovery_required=1 OR a.must_change_password=1) LIMIT 64")?.query_map([time],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, id, run, request, expired) in rows {
        c.execute(
            "UPDATE mission_input_waits SET status=?3 WHERE tenant_id=?1 AND request_id=?2",
            params![t, request, if expired { "expired" } else { "cancelled" }],
        )?;
        c.execute("UPDATE missions SET state='partial',grant_enabled=0,report='[\"Original input wait expired or authority changed. Review saved results or create a new task.\"]' WHERE tenant_id=?1 AND id=?2 AND state='waiting'",params![t,id])?;
        c.execute("UPDATE chat_runs SET state='incomplete',error='budget_exhausted',version=version+1 WHERE tenant_id=?1 AND id=?2 AND state='queued'",params![t,run])?;
    }
    Ok(())
}
pub(crate) fn promote(c: &Connection, t: &str, id: &str, run: &str, request: &str) -> Result<()> {
    let r = crate::chat::load_run(c, t, run)?;
    // Restoring temporary runnable state permits the ordinary carrier guards;
    // rejection retires the wait, never swaps the original profile or session.
    c.execute(
        "UPDATE missions SET state='queued' WHERE tenant_id=?1 AND id=?2 AND state='waiting'",
        params![t, id],
    )?;
    let valid = authority(c, &r).and_then(|()| crate::agent_execution::guard_execution(c, &r));
    if valid.is_err() {
        c.execute("UPDATE mission_input_waits SET status='cancelled' WHERE tenant_id=?1 AND request_id=?2",params![t,request])?;
        c.execute("UPDATE missions SET state='partial',grant_enabled=0 WHERE tenant_id=?1 AND id=?2 AND state='queued'",params![t,id])?;
        c.execute("UPDATE chat_runs SET state='incomplete',error='interrupted' WHERE tenant_id=?1 AND id=?2 AND state='queued'",params![t,run])?;
        return Ok(());
    }
    c.execute(
        "DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",
        params![t, run],
    )?;
    requeue_stable(c, t, run)?;
    c.execute("UPDATE mission_input_waits SET status='resumed' WHERE tenant_id=?1 AND request_id=?2 AND status='ready'",params![t,request])?;
    Ok(())
}
impl Store {
    pub async fn mission_wait_for_input(
        &self,
        lease: &crate::StepLease,
        revision: i64,
        input: MissionInputRequest,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=match crate::agent_execution::guard_step(&tx,&db.provider_key,&lease){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            if revision!=acceptance::revision(&tx,&r.tenant,&r.id)? || input.question.trim().is_empty() || input.question.chars().count()>500 || input.question.chars().any(|c|c.is_control() && c!='\n') {return Err(StoreError::InvalidInput)}
            let (id,mission_revision,expiry):(String,i64,i64)=tx.query_row("SELECT id,revision,expires_ms FROM missions WHERE tenant_id=?1 AND run_id=?2 AND state IN('planning','executing','verifying') AND uncertain=0",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(StoreError::Conflict)?;
            let recurring:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM recurring_mission_occurrences WHERE tenant_id=?1 AND mission_id=?2)",params![r.tenant,id],|r|r.get(0))?;
            if recurring{return Err(StoreError::Forbidden)}
            let field=field_name(&input.field);let kind=if input.field==MissionInputField::MailTo{"mail"}else{"calendar"};
            let admitted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3 AND kind=?4 AND payload_digest IS NULL)",params![r.tenant,r.id,revision,kind],|r|r.get(0))?;
            if !admitted{return Err(StoreError::Forbidden)}
            let current=acceptance::current(&tx,&r.tenant,&r.id)?;let text=format!("{}\n{}",current.task,current.constraints);
            // A literal recipient is already a complete selection. Never ask again.
            if input.field==MissionInputField::MailTo && crate::mailbox_intent::has_non_account_address(&text)?{return Err(StoreError::Conflict)}
            if matches!(input.field,MissionInputField::CalendarStart|MissionInputField::CalendarDuration) && actions::calendar_operation(&text)!=Some("create"){return Err(StoreError::Forbidden)}
            if input.field!=MissionInputField::MailTo {
                let profile=crate::calendar::agent::profile(&tx,&r.tenant,&r.id)?.ok_or(StoreError::Forbidden)?;
                let normalized=crate::schedules::civil_time::normalized_time(&text);
                let supplied=match input.field {
                    MissionInputField::CalendarDate => !crate::schedules::civil_time::date_mentions(&text,profile.reference,profile.timezone.as_deref().unwrap_or("UTC"))?.is_empty(),
                    MissionInputField::CalendarStart => text.char_indices().any(|(i,_)|crate::schedules::civil_time::clock_prefix(&text[i..]).is_some()) || ["all-day","all day","종일"].iter().any(|s|text.to_lowercase().contains(s)),
                    MissionInputField::CalendarDuration => clock_count(&text)>1 || text.contains("부터") || text.contains("까지") || text.to_lowercase().contains("until") || text.to_lowercase().contains("through") || (text.to_lowercase().contains("from ") && text.to_lowercase().contains(" to ")) || ["minute","hour","분","시간","all-day","allday","종일"].iter().any(|s|normalized.contains(s)),
                    _=>false,
                };
                if supplied {return Err(StoreError::Conflict)}
            }
            let unstable:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state IN('dispatched','unknown')) OR EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started') OR EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND result IS NULL AND step>=?3) OR EXISTS(SELECT 1 FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=0)",params![r.tenant,r.id,acceptance::floor(&tx,&r.tenant,&r.id)?],|r|r.get(0))?;
            if unstable{return Err(StoreError::Conflict)}
            let unknown:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mission_action_contracts l JOIN calendar_actions a ON a.tenant_id=l.tenant_id AND a.id=l.action_id WHERE l.tenant_id=?1 AND l.run_id=?2 AND a.state IN('dispatch_committed','outcome_unknown','quarantined')) OR EXISTS(SELECT 1 FROM mission_action_contracts l JOIN mail_provider_saves s ON s.tenant_id=l.tenant_id AND s.action_id=l.action_id WHERE l.tenant_id=?1 AND l.run_id=?2 AND s.state IN('started','outcome_unknown'))",params![r.tenant,r.id],|r|r.get(0))?;
            if unknown{return Err(StoreError::Conflict)}
            let exhausted:bool=tx.query_row("SELECT remote_calls>=6 OR tool_calls>=12 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;
            if exhausted{return Err(StoreError::BudgetExhausted)}
            let already:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mission_input_waits WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3 AND field=?4)",params![r.tenant,r.id,revision,field],|r|r.get(0))?;
            if already{return Err(StoreError::Conflict)}
            let waiting:i64=tx.query_row("SELECT count(*) FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state='waiting'",params![r.tenant,r.account],|r|r.get(0))?;
            if waiting>=2{return Err(StoreError::Busy)}
            let deadline:Option<i64>=tx.query_row("SELECT deadline_ms FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;let expiry=expiry.min(deadline.unwrap_or(expiry));
            if expiry<=clock()? {return Err(StoreError::BudgetExhausted)}
            tx.execute("INSERT INTO mission_input_waits VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,NULL,'waiting',?10)",params![r.tenant,id,r.id,Uuid::new_v4().to_string(),revision,mission_revision,lease.step,field,input.question,expiry])?;
            tx.execute("UPDATE missions SET state='waiting',round_text=NULL WHERE tenant_id=?1 AND id=?2",params![r.tenant,id])?;
            tx.execute("UPDATE chat_runs SET state='queued' WHERE tenant_id=?1 AND id=?2",params![r.tenant,r.id])?;
            tx.execute("DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn reply_mission_input(
        &self,
        user: &UserContext,
        id: String,
        input: MissionInputReply,
    ) -> Result<MissionView> {
        let user = user.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;
            let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND auth_epoch=?4)",params![u.tenant.0,id,u.account.id,u.epoch],|r|r.get(0))?;
            if !owned{return Err(StoreError::NotFound)}
            Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
            let payload=json(&input)?;
            let old:Option<String>=tx.query_row("SELECT payload FROM mission_input_replies WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![u.tenant.0,id,input.request_id],|r|r.get(0)).optional()?;
            if let Some(old)=old {if old!=payload{return Err(StoreError::Conflict)}return super::view(&tx,&u.tenant.0,&id)}
            expire(&tx,clock()?)?;tx.commit()?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let wait=view(&tx,&u.tenant.0,&id)?.into_iter().find(|w|w.request_id==input.wait_request_id).ok_or(StoreError::Conflict)?;
            let current=super::view(&tx,&u.tenant.0,&id)?;
            if current.state!="waiting"||current.revision!=input.expected_revision||wait.state!="waiting"||wait.acceptance_revision!=acceptance::revision(&tx,&u.tenant.0,&tx.query_row("SELECT run_id FROM missions WHERE tenant_id=?1 AND id=?2",params![u.tenant.0,id],|r|r.get::<_,String>(0))?)? {return Err(StoreError::Conflict)}
            let value=value(&wait.field,&input.value)?;
            tx.execute("UPDATE mission_input_waits SET value=?3,status='ready' WHERE tenant_id=?1 AND request_id=?2 AND status='waiting'",params![u.tenant.0,wait.request_id,value])?;
            tx.execute("INSERT INTO mission_input_replies VALUES(?1,?2,?3,?4)",params![u.tenant.0,id,input.request_id,payload])?;
            crate::connection_waits::poll(&tx,clock()?)?;
            let out=super::view(&tx,&u.tenant.0,&id)?;tx.commit()?;Ok(out)
        }).await
    }
}
