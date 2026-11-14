//! Opt-in private Web Push subscriptions and bounded, accounted delivery intent.
use super::*;
use chrono::Timelike;
use moosuri_connectors::push::{PushOutcome, PushSubscription};
use moosuri_core::{PushConfiguration, PushDeviceView};
mod key;
pub use key::{KEY_FILE, validate_optional_file};
type DeviceRow = (String, String, Option<String>, Option<Vec<u8>>);
const AAD: &[u8] = b"moosuri/web-push/subscription/v1";
#[derive(Clone)]
pub struct PushLease {
    tenant: String,
    account: String,
    id: String,
    binding: String,
    notification: String,
    nonce: String,
}
pub struct PushClaim {
    pub lease: PushLease,
    pub subscription: PushSubscription,
    pub vapid: Zeroizing<Vec<u8>>,
    pub contact: String,
    pub payload: Vec<u8>,
}
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PushEnrollment {
    pub request_id: String,
    pub device_id: String,
    pub operation_revision: i64,
    pub subscription: PushSubscription,
}
/// Ordered current-account/device off intent; contains no subscription authority.
#[derive(serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct PushDeactivation {
    pub request_id: String,
    pub device_id: String,
    pub operation_revision: i64,
}
fn revision(value: i64) -> Result<()> {
    if !(1..=9_007_199_254_740_991).contains(&value) {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn require_new_intent(
    conn: &Connection,
    user: &UserContext,
    device: &str,
    revision: i64,
) -> Result<()> {
    // Only a successful enrollment with a private subscription carrier can order
    // across owners of this shared browser. Off receipts fence their owner only.
    let latest: i64 = conn.query_row("SELECT coalesce(max(operation_revision),0) FROM push_enrollment_receipts WHERE device_id=?1 AND (binding_id IS NOT NULL OR (tenant_id=?2 AND account_id=?3))",params![device,user.tenant.0,user.account.id],|r|r.get(0))?;
    if revision <= latest {
        return Err(StoreError::Conflict);
    }
    let count: i64 = conn.query_row(
        "SELECT count(*) FROM push_enrollment_receipts WHERE tenant_id=?1 AND account_id=?2",
        params![user.tenant.0, user.account.id],
        |r| r.get(0),
    )?;
    if count >= 4096 {
        return Err(StoreError::Busy);
    }
    Ok(())
}
fn uuid(id: &str) -> Result<()> {
    if Uuid::parse_str(id).is_err() {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn json<T: serde::Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| StoreError::Unavailable)
}
fn clock() -> Result<i64> {
    now()
}
fn counter(conn: &Connection) -> Result<i64> {
    conn.execute(
        "UPDATE push_worker_state SET counter=counter+1 WHERE singleton=1",
        [],
    )?;
    Ok(conn.query_row(
        "SELECT counter FROM push_worker_state WHERE singleton=1",
        [],
        |r| r.get(0),
    )?)
}
fn eligible(conn: &Connection, binding: &str, time: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM push_subscriptions p JOIN accounts a ON a.tenant_id=p.tenant_id AND a.id=p.account_id JOIN sessions s ON s.digest=p.session_digest AND s.tenant_id=p.tenant_id AND s.account_id=p.account_id WHERE p.binding_id=?1 AND p.state='active' AND p.credentials IS NOT NULL AND a.auth_epoch=p.auth_epoch AND s.auth_epoch=a.auth_epoch AND s.expires_at>?2 AND s.password_change_only=0 AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?2))",params![binding,time],|r|r.get(0))?)
}
fn quiet(conn: &Connection, tenant: &str, account: &str, time: i64) -> Result<bool> {
    let row:Option<(bool,String,u16,u16)>=conn.query_row("SELECT enabled,timezone,start_minute,end_minute FROM notification_settings WHERE tenant_id=?1 AND account_id=?2",params![tenant,account],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((true, zone, start, end)) = row else {
        return Ok(false);
    };
    let zone: chrono_tz::Tz = zone.parse().map_err(|_| StoreError::Corrupt)?;
    let local = chrono::DateTime::from_timestamp(time, 0)
        .ok_or(StoreError::Unavailable)?
        .with_timezone(&zone);
    let minute = (local.hour() * 60 + local.minute()) as u16;
    Ok(if start < end {
        minute >= start && minute < end
    } else {
        minute >= start || minute < end
    })
}
fn device(
    conn: &Connection,
    user: &UserContext,
    device_id: &str,
) -> Result<Option<PushDeviceView>> {
    let value:Option<DeviceRow>=conn.query_row("SELECT binding_id,state,last_outcome,session_digest FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND device_id=?3",params![user.tenant.0,user.account.id,device_id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    value
        .map(|(binding_id, state, last_outcome, session)| {
            let state = if state == "active"
                && (session.as_deref() != Some(user.digest.as_slice())
                    || !eligible(conn, &binding_id, clock()?)?)
            {
                "reconnect".into()
            } else {
                state
            };
            Ok(PushDeviceView {
                binding_id,
                state,
                last_outcome,
            })
        })
        .transpose()
}
fn retire(conn: &Connection, binding: &str, reason: &str) -> Result<()> {
    conn.execute("UPDATE push_subscriptions SET state='off',credentials=NULL,endpoint_hash=NULL,session_digest=NULL,last_outcome=?2 WHERE binding_id=?1",params![binding,reason])?;
    conn.execute("UPDATE push_queue SET state='cancelled',nonce=NULL,finished_at=?2,reason=?3 WHERE binding_id=?1 AND state IN('queued','claimed')",params![binding,clock()?,reason])?;
    Ok(())
}
/// Export hook runs on staged DB only. The runtime signing file is never archived.
pub fn strip_for_export(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE push_subscriptions SET state='restore_required',credentials=NULL,endpoint_hash=NULL,session_digest=NULL,last_outcome='restore_required'",[])?;
    conn.execute("UPDATE push_queue SET state='quarantined',nonce=NULL,finished_at=?1,reason='restore_quarantine' WHERE state IN('queued','claimed','dispatched')",[clock()?])?;
    conn.execute("UPDATE push_vapid_meta SET retired=1", [])?;
    Ok(())
}
pub fn quarantine_for_restore(conn: &Connection) -> Result<()> {
    strip_for_export(conn)
}
pub(crate) fn restart(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE push_subscriptions SET last_outcome='unknown' WHERE state='active' AND binding_id IN(SELECT binding_id FROM push_queue WHERE state='dispatched')",[])?;
    conn.execute("UPDATE push_queue SET state='unknown',finished_at=?1,reason='server_restarted' WHERE state='dispatched'",[clock()?])?;
    conn.execute(
        "UPDATE push_queue SET state='queued',nonce=NULL,deadline=NULL WHERE state='claimed'",
        [],
    )?;
    Ok(())
}
impl Store {
    /// Service-worker authorization must not refresh sliding login expiry.
    pub async fn authenticate_push_passive(&self, token: String) -> Result<UserContext> {
        let token = Zeroizing::new(token);
        if token.len() != 64 || !token.bytes().all(|v| v.is_ascii_hexdigit()) {
            return Err(StoreError::Unauthorized);
        }
        let digest = digest(&token);
        self.call(move |db| load_context(&db.conn, digest, false))
            .await
    }
    pub async fn push_configuration(
        &self,
        user: &UserContext,
        device_id: String,
        contact: String,
    ) -> Result<PushConfiguration> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            uuid(&device_id)?;
            let public = key::ensure(&db.conn, &contact)?;
            Ok(PushConfiguration {
                application_server_key: public,
                device: device(&db.conn, &user, &device_id)?,
            })
        })
        .await
    }
    pub async fn enroll_push(
        &self,
        user: &UserContext,
        input: PushEnrollment,
        contact: String,
    ) -> Result<PushDeviceView> {
        let user = user.clone();
        moosuri_connectors::push::validate_subscription(&input.subscription)
            .map_err(|_| StoreError::InvalidInput)?;
        uuid(&input.device_id)?;
        uuid(&input.request_id)?;
        revision(input.operation_revision)?;
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let hash=Sha256::digest(json(&input)?).to_vec();key::ensure(&db.conn,&contact)?;
   let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
   let old:Option<(Vec<u8>,Option<String>)>=tx.query_row("SELECT request_digest,binding_id FROM push_enrollment_receipts WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![user.tenant.0,user.account.id,input.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
   if let Some((digest,binding))=old{if digest!=hash{return Err(StoreError::Conflict)}let binding=binding.ok_or(StoreError::Conflict)?;return Ok(device(&tx,&user,&input.device_id)?.filter(|v|v.binding_id==binding).unwrap_or(PushDeviceView{binding_id:binding,state:"off".into(),last_outcome:None}));}
   require_new_intent(&tx,&user,&input.device_id,input.operation_revision)?;
   let current:Option<String>=tx.query_row("SELECT binding_id FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND device_id=?3",params![user.tenant.0,user.account.id,input.device_id],|r|r.get(0)).optional()?;
   let active:i64=tx.query_row("SELECT count(*) FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND state='active' AND device_id!=?3",params![user.tenant.0,user.account.id,input.device_id],|r|r.get(0))?;if active>=8{return Err(StoreError::Busy)}
   if let Some(old)=current{retire(&tx,&old,"replaced")?;tx.execute("DELETE FROM push_subscriptions WHERE binding_id=?1",[old])?;}
   let endpoint_hash=Sha256::digest(input.subscription.endpoint.as_bytes()).to_vec();let foreign:Option<String>=tx.query_row("SELECT binding_id FROM push_subscriptions WHERE endpoint_hash=?1",[&endpoint_hash],|r|r.get(0)).optional()?;if let Some(old)=foreign{retire(&tx,&old,"replaced")?;}
   tx.execute("DELETE FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND state!='active' AND binding_id IN(SELECT binding_id FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND state!='active' ORDER BY created_at LIMIT 32)",params![user.tenant.0,user.account.id])?;
   let binding=Uuid::new_v4().to_string();let credentials=providers::encrypt(&db.provider_key,&json(&input.subscription)?,AAD)?;let cursor:i64=tx.query_row("SELECT coalesce(max(rowid),0) FROM local_notifications",[],|r|r.get(0))?;
   tx.execute("INSERT INTO push_subscriptions(binding_id,tenant_id,account_id,device_id,endpoint_hash,credentials,auth_epoch,session_digest,state,cursor_after,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'active',?9,?10)",params![binding,user.tenant.0,user.account.id,input.device_id,endpoint_hash,credentials,user.epoch,user.digest.as_slice(),cursor,clock()?])?;
   tx.execute("INSERT OR IGNORE INTO push_owners VALUES(?1,?2,0)",params![user.tenant.0,user.account.id])?;
   tx.execute("INSERT INTO push_enrollment_receipts(tenant_id,account_id,request_id,request_digest,binding_id,device_id,operation_revision) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![user.tenant.0,user.account.id,input.request_id,hash,binding,input.device_id,input.operation_revision])?;
   let view=PushDeviceView{binding_id:binding,state:"active".into(),last_outcome:None};tx.commit()?;Ok(view)
  }).await
    }
    pub async fn deactivate_push(&self, user: &UserContext, input: PushDeactivation) -> Result<()> {
        uuid(&input.request_id)?;
        uuid(&input.device_id)?;
        revision(input.operation_revision)?;
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let hash = Sha256::digest(json(&input)?).to_vec();
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let old: Option<Vec<u8>> = tx.query_row("SELECT request_digest FROM push_enrollment_receipts WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![user.tenant.0,user.account.id,input.request_id],|r|r.get(0)).optional()?;
            if let Some(old) = old { return if old == hash { Ok(()) } else { Err(StoreError::Conflict) }; }
            require_new_intent(&tx, &user, &input.device_id, input.operation_revision)?;
            let binding: Option<String> = tx.query_row("SELECT binding_id FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND device_id=?3",params![user.tenant.0,user.account.id,input.device_id],|r|r.get(0)).optional()?;
            if let Some(binding) = binding { retire(&tx, &binding, "off")?; }
            tx.execute("INSERT INTO push_enrollment_receipts(tenant_id,account_id,request_id,request_digest,binding_id,device_id,operation_revision) VALUES(?1,?2,?3,?4,NULL,?5,?6)",params![user.tenant.0,user.account.id,input.request_id,hash,input.device_id,input.operation_revision])?;
            tx.commit()?;
            Ok(())
        }).await
    }
    pub async fn disable_push(&self, user: &UserContext, binding: String) -> Result<()> {
        let user = user.clone();
        uuid(&binding)?;
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let owned:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM push_subscriptions WHERE tenant_id=?1 AND account_id=?2 AND binding_id=?3)",params![user.tenant.0,user.account.id,binding],|r|r.get(0))?;if owned{retire(&tx,&binding,"off")?;}tx.commit()?;Ok(())}).await
    }
    pub async fn authorize_push_display(
        &self,
        user: &UserContext,
        binding: String,
        notification: String,
    ) -> Result<()> {
        let user = user.clone();
        uuid(&binding)?;
        uuid(&notification)?;
        self.call(move|db|{let user=validate(&db.conn,&user,true,false)?;let row:bool=db.conn.query_row("SELECT EXISTS(SELECT 1 FROM push_subscriptions p JOIN local_notifications n ON n.tenant_id=p.tenant_id AND n.account_id=p.account_id WHERE p.tenant_id=?1 AND p.account_id=?2 AND p.binding_id=?3 AND p.session_digest=?4 AND n.id=?5 AND n.is_read=0 AND n.dismissed=0 AND n.quiet=0 AND n.source_suppressed=0 AND n.push_suppressed=0 AND NOT EXISTS(SELECT 1 FROM notification_source_preferences sp WHERE sp.tenant_id=n.tenant_id AND sp.account_id=n.account_id AND sp.schedule_id=n.schedule_id AND sp.muted=1) AND n.delivered_at>?6 AND EXISTS(SELECT 1 FROM push_queue q WHERE q.tenant_id=n.tenant_id AND q.notification_id=n.id AND q.binding_id=p.binding_id AND q.state IN('dispatched','accepted','unknown')))",params![user.tenant.0,user.account.id,binding,user.digest.as_slice(),notification,clock()?-3600],|r|r.get(0))?;if !row||!eligible(&db.conn,&binding,clock()?)?||quiet(&db.conn,&user.tenant.0,&user.account.id,clock()?)?{return Err(StoreError::NotFound)}Ok(())}).await
    }
}

fn allowed(conn: &Connection, lease: &PushLease, time: i64) -> Result<bool> {
    if !eligible(conn, &lease.binding, time)? || quiet(conn, &lease.tenant, &lease.account, time)? {
        return Ok(false);
    }
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM push_queue q JOIN local_notifications n ON n.tenant_id=q.tenant_id AND n.id=q.notification_id WHERE q.tenant_id=?1 AND q.id=?2 AND q.binding_id=?3 AND q.nonce=?4 AND q.expires_at>?5 AND n.account_id=?6 AND n.is_read=0 AND n.dismissed=0 AND n.quiet=0 AND n.source_suppressed=0 AND n.push_suppressed=0 AND NOT EXISTS(SELECT 1 FROM notification_source_preferences sp WHERE sp.tenant_id=n.tenant_id AND sp.account_id=n.account_id AND sp.schedule_id=n.schedule_id AND sp.muted=1))",params![lease.tenant,lease.id,lease.binding,lease.nonce,time,lease.account],|r|r.get(0))?)
}
impl Store {
    pub async fn poll_push_queue(&self) -> Result<()> {
        self.call(move|db|{
  let time=clock()?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  tx.execute("UPDATE push_queue SET state='unknown',reason='lease_expired_after_dispatch',finished_at=?1 WHERE state='dispatched' AND deadline<?1",[time])?;
  tx.execute("UPDATE push_queue SET state='queued',nonce=NULL,deadline=NULL WHERE state='claimed' AND deadline<?1",[time])?;
  tx.execute("UPDATE push_queue SET state='expired',nonce=NULL,finished_at=?1,reason='notification_expired' WHERE state IN('queued','claimed') AND expires_at<=?1",[time])?;
  tx.execute("UPDATE push_queue SET state='suppressed',nonce=NULL,finished_at=?1,reason='read_or_dismissed' WHERE rowid IN(SELECT q.rowid FROM push_queue q JOIN local_notifications n ON n.tenant_id=q.tenant_id AND n.id=q.notification_id WHERE q.state='queued' AND (n.is_read=1 OR n.dismissed=1) LIMIT 64)",[time])?;
  let mut global_pending:i64=tx.query_row("SELECT count(*) FROM push_queue WHERE state IN('queued','claimed','dispatched')",[],|r|r.get(0))?;
  let bindings=tx.prepare("SELECT binding_id,tenant_id,account_id,cursor_after FROM push_subscriptions WHERE state='active' ORDER BY last_scan,binding_id LIMIT 16")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,i64>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
  for(binding,tenant,account,cursor)in bindings{
   let order=counter(&tx)?;tx.execute("UPDATE push_subscriptions SET last_scan=?2 WHERE binding_id=?1",params![binding,order])?;
   if !eligible(&tx,&binding,time)?{retire(&tx,&binding,"account_unavailable")?;continue;}
   tx.execute("DELETE FROM push_queue WHERE tenant_id=?1 AND account_id=?2 AND state NOT IN('queued','claimed','dispatched') AND id IN(SELECT id FROM push_queue WHERE tenant_id=?1 AND account_id=?2 AND state NOT IN('queued','claimed','dispatched') ORDER BY created_at LIMIT max(0,(SELECT count(*) FROM push_queue WHERE tenant_id=?1 AND account_id=?2)-1920))",params![tenant,account])?;
   let mut pending:i64=tx.query_row("SELECT count(*) FROM push_queue WHERE tenant_id=?1 AND account_id=?2 AND state IN('queued','claimed','dispatched')",params![tenant,account],|r|r.get(0))?;
   let rows=tx.prepare("SELECT rowid,tenant_id,account_id,id,is_read,dismissed,quiet,delivered_at,push_suppressed OR source_suppressed OR EXISTS(SELECT 1 FROM notification_source_preferences sp WHERE sp.tenant_id=local_notifications.tenant_id AND sp.account_id=local_notifications.account_id AND sp.schedule_id=local_notifications.schedule_id AND sp.muted=1) FROM local_notifications WHERE rowid>?1 ORDER BY rowid LIMIT 64")?.query_map([cursor],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?,r.get::<_,bool>(4)?,r.get::<_,bool>(5)?,r.get::<_,bool>(6)?,r.get::<_,i64>(7)?,r.get::<_,bool>(8)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
   let mut after=cursor;for(rowid,owner_tenant,owner_account,id,read,dismissed,was_quiet,delivered,suppressed)in rows{after=rowid;if owner_tenant!=tenant||owner_account!=account||read||dismissed||was_quiet||suppressed||delivered+3600<=time||pending>=512||global_pending>=4096{continue}
    let id_job=Uuid::new_v4().to_string();let quiet=quiet(&tx,&tenant,&account,time)?;
    let inserted=tx.execute("INSERT OR IGNORE INTO push_queue(tenant_id,id,account_id,binding_id,notification_id,state,ready_at,expires_at,created_at,reason) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?7,?9)",params![tenant,id_job,account,binding,id,if quiet{"suppressed"}else{"queued"},time,delivered+3600,if quiet{Some("quiet_hours")}else{None}])? as i64; if !quiet{pending+=inserted;global_pending+=inserted;}
   }tx.execute("UPDATE push_subscriptions SET cursor_after=?2 WHERE binding_id=?1",params![binding,after])?;
  }tx.commit()?;Ok(())
 }).await
    }
    pub async fn claim_push(&self) -> Result<Option<PushClaim>> {
        self.call(move|db|{
  let time=clock()?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  let running:i64=tx.query_row("SELECT count(*) FROM push_queue WHERE state IN('claimed','dispatched')",[],|r|r.get(0))?;if running>=2{return Ok(None)}
  let candidate:Option<(String,String,String,String,String)>=tx.query_row("SELECT q.tenant_id,q.account_id,q.id,q.binding_id,q.notification_id FROM push_queue q JOIN push_owners o ON o.tenant_id=q.tenant_id AND o.account_id=q.account_id WHERE q.state='queued' AND q.ready_at<=?1 AND q.expires_at>?1 AND NOT EXISTS(SELECT 1 FROM push_queue p WHERE p.tenant_id=q.tenant_id AND p.account_id=q.account_id AND p.state IN('claimed','dispatched')) ORDER BY o.last_claim,q.created_at,q.id LIMIT 1",[time],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
  let Some((tenant,account,id,binding,notification))=candidate else{return Ok(None)};let nonce=Uuid::new_v4().to_string();let lease=PushLease{tenant,account,id,binding,notification,nonce};
  tx.execute("UPDATE push_queue SET nonce=?3,state='claimed',deadline=?4 WHERE tenant_id=?1 AND id=?2 AND state='queued'",params![lease.tenant,lease.id,lease.nonce,time+30])?;
  let order=counter(&tx)?;tx.execute("UPDATE push_owners SET last_claim=?3 WHERE tenant_id=?1 AND account_id=?2",params![lease.tenant,lease.account,order])?;
  if !allowed(&tx,&lease,time)?{tx.execute("UPDATE push_queue SET state='suppressed',reason='authority_or_notification_changed',finished_at=?3 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.id,time])?;tx.commit()?;return Ok(None)}
  let cipher:Vec<u8>=tx.query_row("SELECT credentials FROM push_subscriptions WHERE binding_id=?1",[&lease.binding],|r|r.get(0))?;
  let plain=providers::decrypt(&db.provider_key,&cipher,AAD)?;let subscription:PushSubscription=serde_json::from_slice(&plain).map_err(|_|StoreError::Corrupt)?;let(vapid,contact)=key::private(&tx)?;
  let payload=json(&serde_json::json!({"v":1,"notification_id":lease.notification,"binding_id":lease.binding}))?;
  tx.commit()?;Ok(Some(PushClaim{lease,subscription,vapid,contact,payload}))
 }).await
    }
    pub async fn commit_push_dispatch(&self, lease: &PushLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
  let time=clock()?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
  if !allowed(&tx,&lease,time)?{tx.execute("UPDATE push_queue SET state='suppressed',reason='authority_or_notification_changed',finished_at=?3 WHERE tenant_id=?1 AND id=?2 AND nonce=?4 AND state='claimed'",params![lease.tenant,lease.id,time,lease.nonce])?;tx.commit()?;return Err(StoreError::Conflict)}
  let changed=tx.execute("UPDATE push_queue SET state='dispatched',attempt=attempt+1,deadline=?4 WHERE tenant_id=?1 AND id=?2 AND nonce=?3 AND state='claimed' AND attempt<4",params![lease.tenant,lease.id,lease.nonce,time+20])?;if changed!=1{return Err(StoreError::Conflict)}tx.commit()?;Ok(())
 }).await
    }
    pub async fn fail_push_preparation(&self, lease: &PushLease) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{let changed=db.conn.execute("UPDATE push_queue SET state='rejected',reason='transport_unavailable',finished_at=?4 WHERE tenant_id=?1 AND id=?2 AND nonce=?3 AND state='claimed'",params![lease.tenant,lease.id,lease.nonce,clock()?])?;if changed==1{db.conn.execute("UPDATE push_subscriptions SET last_outcome='unavailable' WHERE binding_id=?1 AND state='active'",[lease.binding])?;}Ok(())}).await
    }
    pub async fn finish_push(&self, lease: &PushLease, outcome: PushOutcome) -> Result<()> {
        let lease = lease.clone();
        self.call(move|db|{
  let time=clock()?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let row:Option<(String,i64,i64)>=tx.query_row("SELECT state,attempt,expires_at FROM push_queue WHERE tenant_id=?1 AND id=?2 AND nonce=?3",params![lease.tenant,lease.id,lease.nonce],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;let Some((state,attempt,expires))=row else{return Err(StoreError::Conflict)};if state!="dispatched"{return if matches!(state.as_str(),"accepted"|"expired"|"rejected"|"unknown"|"queued"|"cancelled"|"quarantined"){Ok(())}else{Err(StoreError::Conflict)}}
  let(mut next,reason,wait)=match outcome{PushOutcome::Accepted=>("accepted","push_service_accepted",0),PushOutcome::Expired=>("expired","subscription_expired",0),PushOutcome::Rejected=>("rejected","push_service_rejected",0),PushOutcome::Unknown=>("unknown","network_outcome_unknown",0),PushOutcome::Retry(seconds)=>("queued","push_service_backoff",i64::from(seconds.clamp(30,3600)).max(30*(1<<attempt.min(4))))};
  if next=="queued"&&(attempt>=4||time+wait>=expires||!allowed(&tx,&lease,time)?){next="rejected";}
  tx.execute("UPDATE push_queue SET state=?4,ready_at=?5,finished_at=?6,reason=?7,deadline=NULL WHERE tenant_id=?1 AND id=?2 AND nonce=?3",params![lease.tenant,lease.id,lease.nonce,next,time+wait,if next=="queued"{None}else{Some(time)},reason])?;
  tx.execute("UPDATE push_subscriptions SET last_outcome=?2 WHERE binding_id=?1",params![lease.binding,next])?;
  if matches!(outcome,PushOutcome::Expired|PushOutcome::Rejected){retire(&tx,&lease.binding,if outcome==PushOutcome::Expired{"expired"}else{"rejected"})?;}
  tx.commit()?;Ok(())
 }).await
    }
}
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod tests;
