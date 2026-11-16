//! Durable exact-thread Gmail follow-up. No model, session fabrication or sends.
use super::*;
use moosuri_core::{
    MailReplyPreview, MailReplyWatch, MailReplyWatchCreate, MailReplyWatchInventory,
};
mod chat;
mod credentials;
mod targets;
#[cfg(test)]
mod tests;
pub(crate) use chat::finish_chat;
pub use credentials::MailReplyCredentials;

#[derive(Clone)]
pub struct MailReplyLease {
    tenant: String,
    account: String,
    id: String,
    nonce: Vec<u8>,
    pub connection_id: String,
    pub provider_thread: String,
    pub baseline_ids: Vec<String>,
    pub seen_ids: Vec<String>,
}

// Rechecked at claim, credential read/refresh, each provider read, commit and display.
fn current(c: &Connection, tenant: &str, id: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM mail_reply_watches w JOIN accounts a ON a.tenant_id=w.tenant_id AND a.id=w.account_id JOIN threads t ON t.tenant_id=w.tenant_id AND t.id=w.thread_id AND t.account_id=w.account_id JOIN mail_connections m ON m.tenant_id=w.tenant_id AND m.account_id=w.account_id AND m.id=w.connection_id WHERE w.tenant_id=?1 AND w.id=?2 AND a.status='active' AND a.auth_epoch=w.auth_epoch AND a.must_change_password=0 AND a.recovery_required=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?3) AND m.status='active' AND m.native_send=0 AND m.credential IS NOT NULL AND m.epoch=w.connection_epoch AND m.oauth_client_id=w.client_id AND w.app_revision IS (SELECT revision FROM google_setup WHERE singleton=1) AND (w.origin_run IS NULL OR EXISTS(SELECT 1 FROM chat_runs r JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE r.tenant_id=w.tenant_id AND r.id=w.origin_run AND r.account_id=w.account_id AND r.auth_epoch=w.auth_epoch AND r.state='completed' AND e.invalidated=0)) AND (w.origin_action IS NULL OR EXISTS(SELECT 1 FROM mail_actions x WHERE x.tenant_id=w.tenant_id AND x.id=w.origin_action AND x.account_id=w.account_id AND x.state='sent' AND x.auth_epoch=w.auth_epoch AND x.connection_epoch=w.connection_epoch)))", params![tenant,id,now()?], |r|r.get(0))?)
}
fn guard(c: &Connection, lease: &MailReplyLease) -> Result<()> {
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='active' AND lease_nonce=?4 AND lease_deadline>?5 AND expires_at>?5)",params![lease.tenant,lease.account,lease.id,lease.nonce,now()?],|r|r.get(0))?;
    if !valid || !current(c, &lease.tenant, &lease.id)? {
        return Err(StoreError::Conflict);
    }
    Ok(())
}
fn view(c: &Connection, t: &str, a: &str, id: &str) -> Result<MailReplyWatch> {
    let (mut out, replies) = c.query_row("SELECT id,thread_id,title,mailbox,state,cadence_seconds,expires_at,last_checked_at,next_due,reason,replies FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![t,a,id],|r| Ok((MailReplyWatch{id:r.get(0)?,thread_id:r.get(1)?,title:r.get(2)?,mailbox:r.get(3)?,state:r.get(4)?,cadence_seconds:r.get(5)?,expires_at:r.get(6)?,last_checked_at:r.get(7)?,next_check_at:r.get(8)?,reason:r.get(9)?,replies:vec![]},r.get::<_,String>(10)?))).optional()?.ok_or(StoreError::NotFound)?;
    if out.state == "replied" && current(c, t, id)? {
        out.replies = parse(&replies)?;
    }
    Ok(out)
}
fn withdraw(c: &Connection, t: &str, id: &str, state: &str, reason: &str) -> Result<()> {
    c.execute("UPDATE mail_reply_watches SET state=?3,reason=?4,next_due=NULL,lease_nonce=NULL,lease_deadline=NULL,replies='[]' WHERE tenant_id=?1 AND id=?2",params![t,id,state,reason])?;
    c.execute("UPDATE local_notifications SET dismissed=1,push_suppressed=1,message='메일 답장 감시가 중지되었습니다.' WHERE tenant_id=?1 AND schedule_id=?2",params![t,id])?;
    Ok(())
}
fn sweep(c: &Connection, tenant: Option<&str>) -> Result<()> {
    let ids=c.prepare("SELECT tenant_id,id,state,expires_at FROM mail_reply_watches WHERE (?1 IS NULL OR tenant_id=?1) AND state IN('active','replied','blocked')")?.query_map([tenant],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (t, id, state, expires) in ids {
        if !current(c, &t, &id)? {
            withdraw(c, &t, &id, "withdrawn", "authority_changed")?;
        } else if state == "active" && expires <= now()? {
            withdraw(c, &t, &id, "expired", "expired")?;
        }
    }
    Ok(())
}
fn create(c: &Connection, u: &UserContext, input: &MailReplyWatchCreate) -> Result<MailReplyWatch> {
    if !(300..=86400).contains(&input.cadence_seconds) || !(1..=30).contains(&input.duration_days) {
        return Err(StoreError::InvalidInput);
    }
    Uuid::parse_str(&input.request_id).map_err(|_| StoreError::InvalidInput)?;
    let digest = hash(&json(input)?);
    if let Some((old,id))=c.query_row("SELECT request_digest,watch_id FROM mail_reply_watch_requests WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![u.tenant.0,u.account.id,input.request_id],|r|Ok((r.get::<_,Vec<u8>>(0)?,r.get::<_,String>(1)?))).optional()? {
        if old!=digest { return Err(StoreError::Conflict) }
        return view(c,&u.tenant.0,&u.account.id,&id)
    }
    let target = targets::list(c, u, &input.thread_id)?
        .into_iter()
        .find(|t| t.view.id == input.target_id)
        .ok_or(StoreError::NotFound)?;
    sweep(c, Some(&u.tenant.0))?;
    if let Some((id,thread,cadence))=c.query_row("SELECT id,thread_id,cadence_seconds FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND connection_id=?3 AND provider_thread=?4 AND state='active'",params![u.tenant.0,u.account.id,target.connection_id,target.provider_thread],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,u32>(2)?))).optional()? {
        // Do not silently relocate a watch or change an existing delegation.
        if thread!=input.thread_id || cadence!=input.cadence_seconds { return Err(StoreError::Conflict) }
        c.execute("INSERT INTO mail_reply_watch_requests VALUES(?1,?2,?3,?4,?5)",params![u.tenant.0,u.account.id,input.request_id,digest,id])?;
        return view(c,&u.tenant.0,&u.account.id,&id)
    }
    let count:i64=c.query_row("SELECT count(*) FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND state='active'",params![u.tenant.0,u.account.id],|r|r.get(0))?;
    if count >= 16 {
        return Err(StoreError::Conflict);
    }
    // A new explicit grant has its own lifetime. Reuse only the last successful
    // observation cursor, so re-arming cannot announce the same reply again.
    let prior: Option<String> = c.query_row("SELECT seen_ids FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND connection_id=?3 AND provider_thread=?4 AND auth_epoch=?5 AND connection_epoch=?6 AND app_revision IS ?7 AND client_id=?8 AND last_checked_at IS NOT NULL ORDER BY last_checked_at DESC,rowid DESC LIMIT 1",params![u.tenant.0,u.account.id,target.connection_id,target.provider_thread,u.epoch,target.connection_epoch,target.app_revision,target.client_id],|r|r.get(0)).optional()?;
    let mut seen: Vec<String> = prior.as_deref().map(parse).transpose()?.unwrap_or_default();
    for id in &target.seen {
        if !seen.contains(id) {
            seen.push(id.clone())
        }
    }
    if seen.len() > 1024 {
        return Err(StoreError::Conflict);
    }
    let id = Uuid::new_v4().to_string();
    let now = super::now()?;
    c.execute("INSERT INTO mail_reply_watches(tenant_id,account_id,id,request_id,request_digest,thread_id,origin_run,origin_action,auth_epoch,connection_id,connection_epoch,app_revision,client_id,provider_thread,baseline_ids,seen_ids,title,mailbox,state,cadence_seconds,expires_at,next_due,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,'active',?19,?20,?21,?21)",params![u.tenant.0,u.account.id,id,input.request_id,digest,input.thread_id,target.run,target.action,u.epoch,target.connection_id,target.connection_epoch,target.app_revision,target.client_id,target.provider_thread,json(&target.seen)?,json(&seen)?,target.view.title,target.view.mailbox,input.cadence_seconds,now+i64::from(input.duration_days)*86400,now])?;
    if !current(c, &u.tenant.0, &id)? {
        return Err(StoreError::Conflict);
    }
    c.execute(
        "INSERT INTO mail_reply_watch_requests VALUES(?1,?2,?3,?4,?5)",
        params![u.tenant.0, u.account.id, input.request_id, digest, id],
    )?;
    view(c, &u.tenant.0, &u.account.id, &id)
}
impl Store {
    pub async fn mail_reply_watches(
        &self,
        user: &UserContext,
        thread: Option<String>,
    ) -> Result<MailReplyWatchInventory> {
        let user = user.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u=validate(&tx,&user,true,false)?; sweep(&tx,Some(&u.tenant.0))?;
            let targets=thread.as_ref().map(|id|targets::list(&tx,&u,id)).transpose()?.unwrap_or_default().into_iter().map(|t|t.view).collect();
            let ids=tx.prepare("SELECT id FROM mail_reply_watches WHERE tenant_id=?1 AND account_id=?2 AND (?3 IS NULL OR thread_id=?3) ORDER BY state='active' DESC,created_at DESC,id DESC LIMIT 128")?.query_map(params![u.tenant.0,u.account.id,thread],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let watches=ids.iter().map(|id|view(&tx,&u.tenant.0,&u.account.id,id)).collect::<Result<Vec<_>>>()?;
            tx.commit()?; Ok(MailReplyWatchInventory{targets,watches})
        }).await
    }
    pub async fn create_mail_reply_watch(
        &self,
        user: &UserContext,
        input: MailReplyWatchCreate,
    ) -> Result<MailReplyWatch> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &user, true, false)?;
            let out = create(&tx, &u, &input)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn stop_mail_reply_watch(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<MailReplyWatch> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u = validate(&tx, &user, true, false)?;
            view(&tx, &u.tenant.0, &u.account.id, &id)?;
            withdraw(&tx, &u.tenant.0, &id, "stopped", "owner_stopped")?;
            let out = view(&tx, &u.tenant.0, &u.account.id, &id)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn next_mail_reply_watch(&self) -> Result<Option<MailReplyLease>> {
        self.call(|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?; sweep(&tx,None)?; let now=super::now()?;
            let lease=tx.query_row("SELECT tenant_id,account_id,id,connection_id,provider_thread,baseline_ids,seen_ids FROM mail_reply_watches WHERE state='active' AND next_due<=?1 AND (lease_deadline IS NULL OR lease_deadline<=?1) ORDER BY next_due,created_at,id LIMIT 1",[now],|r|Ok((MailReplyLease{tenant:r.get(0)?,account:r.get(1)?,id:r.get(2)?,nonce:vec![],connection_id:r.get(3)?,provider_thread:r.get(4)?,baseline_ids:vec![],seen_ids:vec![]},r.get::<_,String>(5)?,r.get::<_,String>(6)?))).optional()?;
            let Some((mut lease,baseline,seen))=lease else {tx.commit()?;return Ok(None)};
            lease.baseline_ids=parse(&baseline)?; lease.seen_ids=parse(&seen)?; lease.nonce=hash(&random()?);
            tx.execute("UPDATE mail_reply_watches SET lease_nonce=?3,lease_deadline=?4 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,lease.nonce,now+90])?;
            tx.commit()?; Ok(Some(lease))
        }).await
    }
    pub async fn check_mail_reply_watch(&self, lease: &MailReplyLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| guard(&db.conn, &lease)).await
    }
    pub async fn complete_mail_reply_watch(
        &self,
        lease: &MailReplyLease,
        seen: Vec<String>,
        replies: Vec<MailReplyPreview>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;guard(&tx,&lease)?;
            if seen.is_empty() || seen.len()>512 || replies.len()>4 || json(&replies)?.len()>24*1024 || lease.baseline_ids.iter().any(|id|!seen.contains(id)) || replies.iter().any(|r|!seen.contains(&r.message_id)||lease.seen_ids.contains(&r.message_id)) {return Err(StoreError::InvalidInput)}
            let now=super::now()?;
            if replies.is_empty() {
                tx.execute("UPDATE mail_reply_watches SET seen_ids=?3,last_checked_at=?4,next_due=min(expires_at,?4+cadence_seconds),failures=0,reason=NULL,lease_nonce=NULL,lease_deadline=NULL WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,json(&seen)?,now])?;
            } else {
                let notification=Uuid::new_v4().to_string();
                tx.execute("UPDATE mail_reply_watches SET state='replied',seen_ids=?3,replies=?4,notification_id=?5,last_checked_at=?6,next_due=NULL,failures=0,reason=NULL,lease_nonce=NULL,lease_deadline=NULL WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,json(&seen)?,json(&replies)?,notification,now])?;
                // Push contains a generic notice. Private verified excerpts are shown in the original chat.
                crate::schedules::direct_notice(&tx,&lease.tenant,&lease.account,&lease.id,&notification,"기다리던 메일에 새 답장이 왔어요","원래 대화에서 새 답장의 내용을 확인하세요. 이 답장 감시는 완료되었습니다.")?;
            }
            tx.commit()?;Ok(())
        }).await
    }
    pub async fn fail_mail_reply_watch(
        &self,
        lease: &MailReplyLease,
        reason: &'static str,
        permanent: bool,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // Revoked/expired/old workers cannot change the replacement state.
            guard(&tx,&lease)?;let count:i64=tx.query_row("SELECT failures FROM mail_reply_watches WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id],|r|r.get(0))?;
            if permanent || count>=5 {
                tx.execute("UPDATE mail_reply_watches SET state='blocked',reason=?3,next_due=NULL,lease_nonce=NULL,lease_deadline=NULL,failures=failures+1 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,reason])?;
                let id=Uuid::new_v4().to_string();
                tx.execute("UPDATE mail_reply_watches SET notification_id=?3 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,id])?;
                crate::schedules::direct_notice(&tx,&lease.tenant,&lease.account,&lease.id,&id,"메일 답장 감시를 확인해 주세요","메일 조회를 마치지 못해 감시를 멈췄어요. 원래 대화에서 상태를 확인하세요.")?;
            } else {
                tx.execute("UPDATE mail_reply_watches SET reason=?3,failures=failures+1,next_due=?4,lease_nonce=NULL,lease_deadline=NULL WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,reason,now()?+(60_i64<<count).min(1800)])?;
            }
            tx.commit()?;Ok(())
        }).await
    }
}
/// None means this is another kind of notification.
pub(crate) fn notice_current(c: &Connection, t: &str, notification: &str) -> Result<Option<bool>> {
    let row = c
        .query_row(
            "SELECT id,state FROM mail_reply_watches WHERE tenant_id=?1 AND notification_id=?2",
            params![t, notification],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
        )
        .optional()?;
    row.map(
        |(id, state)| Ok(matches!(state.as_str(), "replied" | "blocked") && current(c, t, &id)?),
    )
    .transpose()
}
