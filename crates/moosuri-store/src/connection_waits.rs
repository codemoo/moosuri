//! Writer-owned connection admission and event driven continuation. No inference polling.
use super::*;
use moosuri_core::{
    MissionConnectionSelection, MissionMailPin, MissionWaitView, ScopedConnectionDescriptor,
};
pub(crate) const MAIL_SLOT: &str = "mission-mail-slot-v1";
fn json<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::Corrupt)
}
fn parse<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
pub(crate) fn unresolved(p: &moosuri_core::RunToolProfile) -> bool {
    p.mission_reads
        .as_ref()
        .is_some_and(|p| p.mail.iter().any(|p| p.connection.id == MAIL_SLOT))
}
/// A logical slot stays in the model-facing profile for the lifetime of this run.
/// Only the exact private grant behind it is materialized; no current profile is fetched.
pub(crate) fn materialize(c: &Connection, r: &mut crate::chat::Run) -> Result<()> {
    let Some(p) = r
        .tool_profile
        .as_mut()
        .and_then(|p| p.mission_reads.as_mut())
    else {
        return Ok(());
    };
    if p.mail.iter().any(|p| p.connection.id == MAIL_SLOT) {
        let selected:Option<String>=c.query_row("SELECT selection FROM mission_connection_slots WHERE tenant_id=?1 AND run_id=?2 AND slot_id=?3 AND operation='gmail.read' AND resource='mailbox' AND schema_version=1",params![r.tenant,r.id,MAIL_SLOT],|r|r.get(0)).optional()?.flatten();
        if let Some(selected) = selected {
            p.mail = vec![parse(&selected)?];
        }
    }
    Ok(())
}
pub(crate) fn mail_id(r: &crate::chat::Run, id: &str) -> Option<String> {
    let p = r.tool_profile.as_ref()?.mission_reads.as_ref()?;
    if id == MAIL_SLOT
        && r.tool_profile
            .as_ref()?
            .mail_connections
            .iter()
            .any(|p| p.id == MAIL_SLOT)
    {
        p.mail
            .first()
            .filter(|p| p.connection.id != MAIL_SLOT)
            .map(|p| p.connection.id.clone())
    } else {
        p.mail
            .iter()
            .find(|p| p.connection.id == id)
            .map(|p| p.connection.id.clone())
    }
}
pub(crate) fn capacity(c: &Connection, t: &str, a: &str, waiting: bool) -> Result<bool> {
    let global:i64=c.query_row("SELECT count(*) FROM missions WHERE state IN('queued','planning','executing','verifying','paused','waiting')",[],|r|r.get(0))?;
    let owner:i64=c.query_row(if waiting {"SELECT count(*) FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state='waiting'"}else{"SELECT count(*) FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state IN('queued','planning','executing','verifying','paused')"},params![t,a],|r|r.get(0))?;
    Ok(global < 16 && owner < if waiting { 2 } else { 1 })
}
fn runnable_free(c: &Connection, t: &str, a: &str, except: &str) -> Result<bool> {
    Ok(!c.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND account_id=?2 AND id!=?3 AND state IN('queued','planning','executing','verifying','paused'))",params![t,a,except],|r|r.get::<_,bool>(0))?)
}
fn pins(c: &Connection, t: &str, a: &str, app: Option<i64>) -> Result<Vec<MissionMailPin>> {
    let revision: Option<i64> = c
        .query_row(
            "SELECT revision FROM google_setup WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    if revision != app {
        return Ok(vec![]);
    }
    let rows=c.prepare("SELECT id,epoch,email,coalesce(oauth_client_id,''),scopes FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND status='active' AND native_send=0 ORDER BY id LIMIT 32")?.query_map(params![t,a],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let mut out = Vec::new();
    for (id, epoch, email, client, scopes) in rows {
        let scopes: Vec<String> = parse(&scopes)?;
        if !client.is_empty()
            && scopes
                .iter()
                .any(|s| s == moosuri_connectors::gmail::READ_SCOPE)
        {
            out.push(MissionMailPin {
                connection: moosuri_core::MailConnectionPin {
                    id,
                    epoch,
                    email,
                    native_send: false,
                },
                client_id: client,
            })
        }
    }
    Ok(out)
}
fn eligible(c: &Connection, t: &str, run: &str) -> Result<Vec<MissionMailPin>> {
    let r = crate::chat::load_run(c, t, run)?;
    let Some(p) = r
        .tool_profile
        .as_ref()
        .and_then(|p| p.mission_reads.as_ref())
    else {
        return Ok(vec![]);
    };
    let scope:String=c.query_row("SELECT account_scope FROM mission_connection_slots WHERE tenant_id=?1 AND run_id=?2 AND slot_id=?3",params![t,run,MAIL_SLOT],|r|r.get(0))?;
    let scope: Vec<String> = parse(&scope)?;
    let selected:Option<String>=c.query_row("SELECT selection FROM mission_connection_slots WHERE tenant_id=?1 AND run_id=?2 AND slot_id=?3",params![t,run,MAIL_SLOT],|r|r.get(0))?;
    let selected: Option<MissionMailPin> = selected.as_deref().map(parse).transpose()?;
    Ok(pins(c, t, &r.account, p.app_revision)?
        .into_iter()
        .filter(|p| {
            selected.as_ref().map_or_else(
                || scope.is_empty() || scope.contains(&p.connection.id),
                |selected| {
                    selected.connection.id == p.connection.id
                        && selected.connection.epoch == p.connection.epoch
                        && selected.client_id == p.client_id
                },
            )
        })
        .collect())
}
fn descriptor(
    c: &Connection,
    t: &str,
    a: &str,
    id: &str,
    expiry: i64,
) -> Result<ScopedConnectionDescriptor> {
    let alias=c.query_row("SELECT alias FROM connection_aliases WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3",params![t,a,id],|r|r.get(0)).optional()?;
    Ok(ScopedConnectionDescriptor {
        binding_id: id.into(),
        device_id: "server-oauth".into(),
        operation: "gmail.read".into(),
        resource: "mailbox".into(),
        schema_version: 1,
        state: "ready".into(),
        expires_at_ms: expiry,
        alias,
    })
}
pub(crate) fn view(c: &Connection, t: &str, id: &str) -> Result<Option<MissionWaitView>> {
    let row=c.query_row("SELECT w.request_id,w.slot_id,w.run_id,w.step,w.acceptance_revision,w.cause,w.status,w.expires_ms,m.account_id FROM mission_connection_waits w JOIN missions m ON m.tenant_id=w.tenant_id AND m.id=w.mission_id WHERE w.tenant_id=?1 AND w.mission_id=?2 ORDER BY w.acceptance_revision DESC LIMIT 1",params![t,id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get(3)?,r.get(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,i64>(7)?,r.get::<_,String>(8)?))).optional()?;
    let Some((
        request_id,
        slot_id,
        run_id,
        step,
        acceptance_revision,
        cause,
        state,
        expires_at_ms,
        account,
    )) = row
    else {
        return Ok(None);
    };
    let choices = if state == "waiting" {
        eligible(c, t, &run_id)?
            .iter()
            .map(|p| descriptor(c, t, &account, &p.connection.id, expires_at_ms))
            .collect::<Result<_>>()?
    } else {
        vec![]
    };
    Ok(Some(MissionWaitView {
        request_id,
        slot_id,
        run_id,
        step,
        acceptance_revision,
        cause,
        state,
        expires_at_ms,
        choices,
    }))
}
/// Admission fixes operation/schema and account-selection bounds before a worker can claim.
pub(crate) fn admit(
    c: &Connection,
    t: &str,
    id: &str,
    run: &str,
    scope: Vec<String>,
    expiry: i64,
) -> Result<()> {
    let r = crate::chat::load_run(c, t, run)?;
    let Some(profile) = r.tool_profile.as_ref() else {
        return Ok(());
    };
    if !profile.mail_connections.iter().any(|p| p.id == MAIL_SLOT) {
        return Ok(());
    }
    let pending = unresolved(profile);
    let selection = if pending {
        None
    } else {
        profile
            .mission_reads
            .as_ref()
            .and_then(|p| p.mail.first())
            .map(json)
            .transpose()?
    };
    let order: i64 = c.query_row(
        "SELECT admitted_order FROM chat_runs WHERE tenant_id=?1 AND id=?2",
        params![t, run],
        |r| r.get(0),
    )?;
    c.execute(
        "INSERT INTO mission_connection_slots VALUES(?1,?2,?3,'gmail.read','mailbox',1,?4,?6,?5)",
        params![t, run, MAIL_SLOT, json(&scope)?, expiry, selection],
    )?;
    c.execute("INSERT INTO mission_connection_waits VALUES(?1,?2,?3,?4,1,1,0,?5,'account_selection',?8,?6,?7)",params![t,id,run,MAIL_SLOT,Uuid::new_v4().to_string(),expiry,order,if pending{"waiting"}else{"resumed"}])?;
    if pending {
        c.execute(
            "UPDATE missions SET state='waiting' WHERE tenant_id=?1 AND id=?2",
            params![t, id],
        )?;
    }
    // The carrier remains queued but both claim paths exclude live wait records.
    Ok(())
}
/// The existing due loop and claim loop call this even with no connected UI.
pub(crate) fn poll(c: &Connection, time: i64) -> Result<()> {
    crate::missions::inputs::expire(c, time)?;
    let stale=c.prepare("SELECT w.tenant_id,w.mission_id,w.run_id,w.expires_ms<=?1,w.request_id FROM mission_connection_waits w JOIN missions m ON m.tenant_id=w.tenant_id AND m.id=w.mission_id JOIN accounts a ON a.tenant_id=m.tenant_id AND a.id=m.account_id WHERE w.status IN('waiting','ready') AND (w.expires_ms<=?1 OR m.expires_ms<=?1 OR m.state!='waiting' OR m.grant_enabled=0 OR m.uncertain=1 OR m.revision!=w.mission_revision OR a.auth_epoch!=m.auth_epoch OR a.status!='active' OR a.recovery_required=1 OR a.must_change_password=1) LIMIT 64")?.query_map([time],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,bool>(3)?,r.get::<_,String>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, id, run, expired, wait_id) in stale {
        c.execute("UPDATE mission_connection_waits SET status=?3 WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?4 AND status IN('waiting','ready')",params![t,id,if expired{"expired"}else{"cancelled"},wait_id])?;
        c.execute("UPDATE missions SET state='partial',grant_enabled=0,report='[\"Original connection wait expired or authority changed. Review saved results or create a new task.\"]' WHERE tenant_id=?1 AND id=?2 AND state='waiting'",params![t,id])?;
        c.execute("UPDATE chat_runs SET state='incomplete',error='budget_exhausted',version=version+1 WHERE tenant_id=?1 AND id=?2 AND state='queued'",params![t,run])?;
        crate::run_instructions::publish_target_changes(c, &t, &run)?;
    }
    let rows=c.prepare("SELECT w.tenant_id,w.mission_id,w.run_id,w.status,m.account_id,w.request_id,0 AS native,r.admitted_order FROM mission_connection_waits w JOIN missions m ON m.tenant_id=w.tenant_id AND m.id=w.mission_id JOIN chat_runs r ON r.tenant_id=m.tenant_id AND r.id=m.run_id WHERE w.status IN('waiting','ready') AND m.state='waiting' UNION ALL SELECT n.tenant_id,m.id,n.run_id,'ready',m.account_id,n.request_id,1,r.admitted_order FROM native_task_mail_reads n JOIN missions m ON m.tenant_id=n.tenant_id AND m.run_id=n.run_id JOIN chat_runs r ON r.tenant_id=n.tenant_id AND r.id=n.run_id WHERE n.status='ready' AND m.state='waiting' UNION ALL SELECT i.tenant_id,m.id,i.run_id,'ready',m.account_id,i.request_id,2,r.admitted_order FROM mission_input_waits i JOIN missions m ON m.tenant_id=i.tenant_id AND m.id=i.mission_id JOIN chat_runs r ON r.tenant_id=i.tenant_id AND r.id=i.run_id WHERE i.status='ready' AND m.state='waiting' ORDER BY admitted_order LIMIT 64")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,i64>(6)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, id, run, status, account, wait_id, native) in rows {
        if native == 2 {
            if runnable_free(c, &t, &account, &id)? {
                crate::missions::inputs::promote(c, &t, &id, &run, &wait_id)?;
            }
            continue;
        }
        if native == 1 {
            if runnable_free(c, &t, &account, &id)? {
                crate::native_task_mail::promote(c, &t, &run, &wait_id)?;
            }
            continue;
        }
        if status == "waiting" {
            let initial:bool=c.query_row("SELECT cause='account_selection' FROM mission_connection_waits WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![t,id,wait_id],|r|r.get(0))?;
            let choices = eligible(c, &t, &run)?;
            if initial && choices.len() == 1 {
                fill(c, &t, &run, &choices[0])?;
                c.execute("UPDATE mission_connection_waits SET status='ready' WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3 AND status='waiting'",params![t,id,wait_id])?;
            }
        }
        let ready:bool=c.query_row("SELECT status='ready' FROM mission_connection_waits WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![t,id,wait_id],|r|r.get(0))?;
        if ready && runnable_free(c, &t, &account, &id)? {
            let r = crate::chat::load_run(c, &t, &run)?;
            // A revoked filled binding never falls back to another account.
            if !crate::missions::connected::current(c, &r)? {
                c.execute("UPDATE mission_connection_waits SET status='cancelled' WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![t,id,wait_id])?;
                c.execute("UPDATE missions SET state='partial',grant_enabled=0,report='[\"Selected connection was withdrawn. Create a new task.\"]' WHERE tenant_id=?1 AND id=?2",params![t,id])?;
                crate::run_instructions::publish_target_changes(c, &t, &run)?;
                continue;
            }
            c.execute(
                "DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",
                params![t, run],
            )?;
            c.execute("UPDATE missions SET state='queued' WHERE tenant_id=?1 AND id=?2 AND state='waiting'",params![t,id])?;
            crate::missions::requeue_stable(c, &t, &run)?;
            c.execute("UPDATE mission_connection_waits SET status='resumed' WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3 AND status='ready'",params![t,id,wait_id])?;
            crate::missions::connected::admit_run(c, &r)?;
            crate::run_instructions::publish_target_changes(c, &t, &run)?;
        }
    }
    Ok(())
}
fn fill(c: &Connection, t: &str, run: &str, p: &MissionMailPin) -> Result<()> {
    let old:Option<String>=c.query_row("SELECT selection FROM mission_connection_slots WHERE tenant_id=?1 AND run_id=?2 AND slot_id=?3",params![t,run,MAIL_SLOT],|r|r.get(0))?;
    if let Some(old) = old {
        return if old == json(p)? {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        };
    }
    if c.execute("UPDATE mission_connection_slots SET selection=?4 WHERE tenant_id=?1 AND run_id=?2 AND slot_id=?3 AND selection IS NULL",params![t,run,MAIL_SLOT,json(p)?])?!=1{return Err(StoreError::Conflict)}
    Ok(())
}
impl Store {
    pub async fn mission_wait_for_connection(
        &self,
        lease: &crate::RunLease,
        input: moosuri_core::MissionConnectionWait,
    ) -> Result<MissionWaitView> {
        let lease = lease.clone();
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=match crate::chat::guard(&tx,&db.provider_key,&lease){Ok(run)=>run,Err(error)=>{tx.commit()?;return Err(error)}};
            if !crate::missions::is_mission(&tx,&r.tenant,&r.id)? {return Err(StoreError::Forbidden)}
            Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
            if input.slot_id!=MAIL_SLOT || !matches!(input.cause.as_str(),"vault_unlock"|"connection_ready") {return Err(StoreError::InvalidInput)}
            let (id,revision,expiry):(String,i64,i64)=tx.query_row("SELECT id,revision,expires_ms FROM missions WHERE tenant_id=?1 AND run_id=?2 AND state IN('planning','executing','verifying') AND uncertain=0",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?.ok_or(StoreError::Conflict)?;
            let acceptance:i64=tx.query_row("SELECT coalesce(max(a.revision),1) FROM mission_acceptances a WHERE a.tenant_id=?1 AND a.mission_id=?2",params![r.tenant,id],|r|r.get(0))?;
            if input.acceptance_revision!=acceptance {return Err(StoreError::Conflict)}
            let unstable:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state IN('dispatched','unknown')) OR EXISTS(SELECT 1 FROM external_tool_dispatches WHERE tenant_id=?1 AND run_id=?2 AND state='started') OR EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND result IS NULL) OR EXISTS(SELECT 1 FROM model_call_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=0)",params![r.tenant,r.id],|r|r.get(0))?;
            if unstable {return Err(StoreError::Conflict)}
            let waiting:i64=tx.query_row("SELECT count(*) FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state='waiting'",params![r.tenant,r.account],|r|r.get(0))?;if waiting>=2{return Err(StoreError::Busy)}
            let (slot_expiry,selection):(i64,Option<String>)=tx.query_row("SELECT expires_ms,selection FROM mission_connection_slots WHERE tenant_id=?1 AND run_id=?2 AND slot_id=?3 AND operation='gmail.read' AND resource='mailbox' AND schema_version=1",params![r.tenant,r.id,input.slot_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?.ok_or(StoreError::Forbidden)?;
            if selection.is_none(){return Err(StoreError::Forbidden)}
            let deadline:Option<i64>=tx.query_row("SELECT deadline_ms FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|r.get(0))?;
            let expiry=expiry.min(slot_expiry).min(deadline.unwrap_or(expiry)).min(input.operation_expires_at_ms);
            if expiry<=now()?.checked_mul(1000).ok_or(StoreError::Unavailable)?{return Err(StoreError::BudgetExhausted)}
            let(step,order):(i64,i64)=tx.query_row("SELECT coalesce((SELECT max(step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2),0)+1,admitted_order FROM chat_runs WHERE tenant_id=?1 AND id=?2",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            tx.execute("INSERT INTO mission_connection_waits VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,'waiting',?10,?11) ON CONFLICT(tenant_id,mission_id,acceptance_revision) DO UPDATE SET request_id=excluded.request_id,cause=excluded.cause,status='waiting',step=excluded.step,mission_revision=excluded.mission_revision,expires_ms=min(mission_connection_waits.expires_ms,excluded.expires_ms)",params![r.tenant,id,r.id,input.slot_id,acceptance,revision,step,input.request_id,input.cause,expiry,order])?;
            tx.execute("UPDATE missions SET state='waiting' WHERE tenant_id=?1 AND id=?2",params![r.tenant,id])?;
            tx.execute("UPDATE chat_runs SET state='queued' WHERE tenant_id=?1 AND id=?2",params![r.tenant,r.id])?;
            tx.execute("DELETE FROM agent_run_leases WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;
            crate::run_instructions::publish_target_changes(&tx,&r.tenant,&r.id)?;
            // The execution deadline, accounting and checkpoint rows are deliberately untouched.
            let out=view(&tx,&r.tenant,&id)?.ok_or(StoreError::Corrupt)?;
            poll(&tx,now()?.checked_mul(1000).ok_or(StoreError::Unavailable)?)?;
            tx.commit()?;Ok(out)
        }).await
    }
    pub async fn select_mission_connection(
        &self,
        user: &UserContext,
        id: String,
        input: MissionConnectionSelection,
    ) -> Result<moosuri_core::MissionView> {
        let user = user.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let u=validate(&tx,&user,true,false)?;
            let owner:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND auth_epoch=?4)",params![u.tenant.0,id,u.account.id,u.epoch],|r|r.get(0))?;if !owner{return Err(StoreError::NotFound)}
            Uuid::parse_str(&input.request_id).map_err(|_|StoreError::InvalidInput)?;
            poll(&tx,now()?.checked_mul(1000).ok_or(StoreError::Unavailable)?)?;tx.commit()?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let payload=json(&input)?;let old:Option<String>=tx.query_row("SELECT payload FROM mission_connection_selections WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3",params![u.tenant.0,id,input.request_id],|r|r.get(0)).optional()?;if let Some(old)=old {if old!=payload{return Err(StoreError::Conflict)}return crate::missions::view(&tx,&u.tenant.0,&id)}
            let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND auth_epoch=?4 AND revision=?5 AND state='waiting')",params![u.tenant.0,id,u.account.id,u.epoch,input.expected_revision],|r|r.get(0))?;if !owned{return Err(StoreError::Conflict)}
            let wait=view(&tx,&u.tenant.0,&id)?.ok_or(StoreError::Conflict)?;if wait.request_id!=input.wait_request_id||wait.slot_id!=input.slot_id||wait.state!="waiting"{return Err(StoreError::Conflict)}
            let pin=eligible(&tx,&u.tenant.0,&wait.run_id)?.into_iter().find(|p|p.connection.id==input.binding_id).ok_or(StoreError::Forbidden)?;fill(&tx,&u.tenant.0,&wait.run_id,&pin)?;
            tx.execute("UPDATE mission_connection_waits SET status='ready' WHERE tenant_id=?1 AND mission_id=?2 AND request_id=?3 AND status='waiting'",params![u.tenant.0,id,wait.request_id])?;
            tx.execute("INSERT INTO mission_connection_selections VALUES(?1,?2,?3,?4)",params![u.tenant.0,id,input.request_id,payload])?;poll(&tx,now()?.checked_mul(1000).ok_or(StoreError::Unavailable)?)?;let out=crate::missions::view(&tx,&u.tenant.0,&id)?;tx.commit()?;Ok(out)
        }).await
    }
    pub async fn set_connection_alias(
        &self,
        user: &UserContext,
        input: moosuri_core::ConnectionAlias,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;if input.alias.chars().count()>64||input.alias.chars().any(char::is_control){return Err(StoreError::InvalidInput)}let owned:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM mail_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND status='active') OR EXISTS(SELECT 1 FROM credential_broker_devices d JOIN credential_broker_bindings b ON b.device_id=d.id WHERE d.tenant_id=?1 AND d.account_id=?2 AND b.binding_id=?3 AND d.status='active' AND b.active=1)",params![u.tenant.0,u.account.id,input.binding_id],|r|r.get(0))?;if !owned{return Err(StoreError::NotFound)}db.conn.execute("INSERT INTO connection_aliases VALUES(?1,?2,?3,?4) ON CONFLICT(tenant_id,account_id,binding_id) DO UPDATE SET alias=excluded.alias",params![u.tenant.0,u.account.id,input.binding_id,input.alias])?;Ok(())}).await
    }
}
