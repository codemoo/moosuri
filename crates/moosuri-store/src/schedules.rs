//! Fixed local reminders with separately persisted owner authority; no synthetic session.
use super::*;
use moosuri_core::*;
use serde::{Serialize, de::DeserializeOwned};
pub(crate) mod chat;
#[cfg(test)]
mod tests;
mod time;
#[cfg(test)]
thread_local! { static TEST_TIME: std::cell::Cell<Option<i64>> = const { std::cell::Cell::new(None) }; }
fn clock() -> Result<i64> {
    #[cfg(test)]
    if let Some(time) = TEST_TIME.with(|v| v.get()) {
        return Ok(time);
    }
    now()
}

#[derive(Clone)]
pub struct ReminderLease {
    tenant: String,
    account: String,
    occurrence: String,
    nonce: Vec<u8>,
}
struct Schedule {
    view: ScheduleView,
    account: String,
    epoch: i64,
    slot: Option<time::Slot>,
}
fn encode<T: Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
fn uuid(v: &str) -> Result<()> {
    Uuid::parse_str(v)
        .map(|_| ())
        .map_err(|_| StoreError::InvalidInput)
}
fn draft(input: &ScheduleDraft, now: i64) -> Result<SchedulePreview> {
    for (text, limit) in [(&input.title, 160), (&input.message, 2000)] {
        if text.trim().is_empty()
            || text.len() > limit
            || text
                .chars()
                .any(|c| c.is_control() && c != '\n' && c != '\t')
        {
            return Err(StoreError::InvalidInput);
        }
    }
    let preview = time::preview(&input.rule, now)?;
    if input
        .valid_until
        .is_some_and(|end| end < preview.next_fire_at || end > 4_102_444_800)
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(preview)
}
fn load(conn: &Connection, tenant: &str, id: &str) -> Result<Schedule> {
    let row=conn.query_row("SELECT account_id,auth_epoch,revision,title,message,rule,state,reason,valid_until,timezone_db,next_slot,created_at,updated_at FROM reminder_schedules WHERE tenant_id=?1 AND id=?2",params![tenant,id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,i64>(2)?,r.get::<_,String>(3)?,r.get::<_,String>(4)?,r.get::<_,String>(5)?,r.get::<_,String>(6)?,r.get::<_,Option<String>>(7)?,r.get::<_,Option<i64>>(8)?,r.get::<_,String>(9)?,r.get::<_,Option<String>>(10)?,r.get::<_,i64>(11)?,r.get::<_,i64>(12)?))).optional()?.ok_or(StoreError::NotFound)?;
    let slot: Option<time::Slot> = row.10.as_deref().map(decode).transpose()?;
    Ok(Schedule {
        account: row.0,
        epoch: row.1,
        view: ScheduleView {
            id: id.into(),
            revision: row.2,
            title: row.3,
            message: row.4,
            rule: decode(&row.5)?,
            state: row.6,
            reason: row.7,
            valid_until: row.8,
            timezone_db: row.9,
            next_fire_at: slot.as_ref().and_then(|s| s.at),
            next_local_slot: slot.as_ref().map(|s| s.local.clone()),
            created_at: row.11,
            updated_at: row.12,
        },
        slot,
    })
}
fn owned(conn: &Connection, user: &UserContext, id: &str) -> Result<Schedule> {
    uuid(id)?;
    if crate::recurring_missions::is_recurring(conn, &user.tenant.0, id)? {
        return Err(StoreError::NotFound);
    }
    let s = load(conn, &user.tenant.0, id)?;
    if s.account != user.account.id {
        return Err(StoreError::NotFound);
    }
    Ok(s)
}
// Current account state and auth_epoch are checked at enqueue, claim and publication.
fn eligible(conn: &Connection, tenant: &str, s: &Schedule, now: i64) -> Result<bool> {
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts WHERE tenant_id=?1 AND id=?2 AND auth_epoch=?3 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>?4))",params![tenant,s.account,s.epoch,now],|r|r.get(0))?)
}
fn owner_view(conn: &Connection, tenant: &str, mut s: Schedule) -> Result<ScheduleView> {
    let time = clock()?;
    if s.view.state == "active" && !eligible(conn, tenant, &s, time)? {
        s.view.state = "paused".into();
        s.view.reason = Some("account_unavailable".into());
    } else if s.view.state == "active" && s.view.valid_until.is_some_and(|end| time > end) {
        s.view.state = "expired".into();
        s.view.reason = Some("validity_expired".into());
    }
    Ok(s.view)
}

fn fence(conn: &Connection, tenant: &str, id: &str, reason: &str, now: i64) -> Result<()> {
    conn.execute("UPDATE reminder_occurrences SET state=?5,reason=?3,nonce=NULL,deadline=NULL,updated_at=?4 WHERE tenant_id=?1 AND schedule_id=?2 AND state IN('queued','claimed')",params![tenant,id,reason,now,if reason=="expired"{"expired"}else{"skipped"}])?;
    Ok(())
}
fn save_slot(conn: &Connection, tenant: &str, id: &str, slot: Option<&time::Slot>) -> Result<()> {
    conn.execute(
        "UPDATE reminder_schedules SET next_slot=?3,next_due=?4 WHERE tenant_id=?1 AND id=?2",
        params![
            tenant,
            id,
            slot.map(encode).transpose()?,
            slot.map(|s| s.due)
        ],
    )?;
    Ok(())
}
fn range(
    conn: &Connection,
    tenant: &str,
    schedule: &Schedule,
    bounds: (&str, &str, i64),
    reason: &str,
    now: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO reminder_skipped_ranges VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
        params![
            tenant,
            Uuid::new_v4().to_string(),
            schedule.view.id,
            schedule.view.revision,
            bounds.0,
            bounds.1,
            bounds.2,
            reason,
            now
        ],
    )?;
    Ok(())
}

fn occurrence(
    conn: &Connection,
    tenant: &str,
    s: &Schedule,
    slot: &time::Slot,
    state: &str,
    reason: Option<&str>,
    now: i64,
) -> Result<()> {
    conn.execute("INSERT OR IGNORE INTO reminder_occurrences(tenant_id,id,schedule_id,account_id,revision,auth_epoch,scheduled_for,local_slot,state,reason,delayed,title,message,created_at,updated_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?14)",params![tenant,Uuid::new_v4().to_string(),s.view.id,s.account,s.view.revision,s.epoch,slot.at,slot.local,state,reason,slot.at.is_some_and(|at|now-at>5),if state=="skipped"{""}else{&s.view.title},if state=="skipped"{""}else{&s.view.message},now])?;
    Ok(())
}
fn requeue_claims(conn: &Connection, now: Option<i64>) -> Result<()> {
    let rows=conn.prepare("SELECT tenant_id,id,schedule_id FROM reminder_occurrences WHERE state='claimed' AND (?1 IS NULL OR deadline<=?1)")?.query_map([now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (tenant, id, schedule) in rows {
        let newer:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM reminder_occurrences WHERE tenant_id=?1 AND schedule_id=?2 AND state='queued')",params![tenant,schedule],|r|r.get(0))?;
        conn.execute("UPDATE reminder_occurrences SET state=?3,reason=?4,nonce=NULL,deadline=NULL WHERE tenant_id=?1 AND id=?2",params![tenant,id,if newer{"skipped"}else{"queued"},if newer{Some("restart_coalesced")}else{None}])?;
    }
    Ok(())
}
pub(crate) fn restart(conn: &Connection) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    requeue_claims(&tx, None)?;
    let now = clock()?;
    let rows=tx.prepare("SELECT tenant_id,id FROM reminder_schedules WHERE state='active' AND timezone_db<>?1 AND next_due>?2")?.query_map(params![time::TZDB,now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (tenant, id) in rows {
        let s = load(&tx, &tenant, &id)?;
        save_slot(&tx, &tenant, &id, time::after(&s.view.rule, now)?.as_ref())?;
        tx.execute(
            "UPDATE reminder_schedules SET timezone_db=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, id, time::TZDB],
        )?;
    }
    tx.commit()?;
    Ok(())
}
/// Root cutover18 integration must call inside its quarantine transaction, never
/// during ordinary startup. Preserves committed inbox/receipts; reactivation is owner action.
#[allow(dead_code)]
pub(crate) fn quarantine_for_restore(conn: &Connection) -> Result<()> {
    conn.execute("UPDATE reminder_occurrences SET state='skipped',reason='restore_quarantine',nonce=NULL,deadline=NULL WHERE state IN('queued','claimed')",[])?;
    conn.execute("UPDATE reminder_schedules SET state='paused',reason='restore_quarantine',revision=revision+1 WHERE state IN('active','paused')",[])?;
    Ok(())
}
fn enqueue(conn: &Connection, now: i64) -> Result<u32> {
    requeue_claims(conn, Some(now))?;
    let rows=conn.prepare("SELECT tenant_id,id FROM reminder_schedules WHERE state='active' AND next_due<=?1 ORDER BY next_due,tenant_id,id LIMIT 64")?.query_map([now],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut count = 0;
    for (tenant, id) in rows {
        let s = load(conn, &tenant, &id)?;
        if !eligible(conn, &tenant, &s, now)? {
            fence(conn, &tenant, &id, "account_unavailable", now)?;
            conn.execute("UPDATE reminder_schedules SET state='paused',reason='account_unavailable' WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
            continue;
        }
        if s.view.valid_until.is_some_and(|end| now > end) {
            fence(conn, &tenant, &id, "expired", now)?;
            if matches!(s.view.rule, ScheduleRule::Once { .. })
                && let Some(slot) = &s.slot
            {
                occurrence(conn, &tenant, &s, slot, "expired", Some("expired"), now)?;
            }
            conn.execute("UPDATE reminder_schedules SET state='expired',reason='expired',next_slot=NULL,next_due=NULL WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
            continue;
        }
        let plan = time::plan(
            &s.view.rule,
            s.slot.as_ref().ok_or(StoreError::Corrupt)?,
            now,
        )?;
        if let Some((from, to, n)) = &plan.skipped {
            range(conn, &tenant, &s, (from, to, *n), "downtime_coalesced", now)?;
        }
        for gap in plan.gaps {
            occurrence(conn, &tenant, &s, &gap, "skipped", Some("dst_gap"), now)?;
        }
        if let Some(slot) = plan.latest {
            conn.execute("UPDATE reminder_occurrences SET state='skipped',reason='latest_coalesced',updated_at=?3 WHERE tenant_id=?1 AND schedule_id=?2 AND state='queued'",params![tenant,id,now])?;
            occurrence(conn, &tenant, &s, &slot, "queued", None, now)?;
            count += 1;
        }
        save_slot(conn, &tenant, &id, plan.next.as_ref())?;
    }
    Ok(count)
}
fn claim(conn: &Connection, now: i64) -> Result<Option<ReminderLease>> {
    // Persistent round-robin owner order; account IDs never act as credentials.
    let rows=conn.prepare("SELECT o.tenant_id,o.id,o.schedule_id,o.account_id FROM reminder_occurrences o JOIN reminder_owners f ON f.tenant_id=o.tenant_id AND f.account_id=o.account_id WHERE o.state='queued' AND NOT EXISTS(SELECT 1 FROM recurring_missions x WHERE x.tenant_id=o.tenant_id AND x.schedule_id=o.schedule_id) AND NOT EXISTS(SELECT 1 FROM reminder_occurrences a WHERE a.tenant_id=o.tenant_id AND a.schedule_id=o.schedule_id AND a.state='claimed') ORDER BY f.last_claim,o.scheduled_for,o.id LIMIT 128")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,String>(3)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (tenant, occurrence, schedule, account) in rows {
        let s = load(conn, &tenant, &schedule)?;
        let revision: i64 = conn.query_row(
            "SELECT revision FROM reminder_occurrences WHERE tenant_id=?1 AND id=?2",
            params![tenant, occurrence],
            |r| r.get(0),
        )?;
        if !eligible(conn, &tenant, &s, now)? {
            fence(conn, &tenant, &schedule, "account_unavailable", now)?;
            conn.execute("UPDATE reminder_schedules SET state='paused',reason='account_unavailable' WHERE tenant_id=?1 AND id=?2",params![tenant,schedule])?;
            continue;
        }
        if s.view.valid_until.is_some_and(|end| now > end) {
            fence(conn, &tenant, &schedule, "expired", now)?;
            conn.execute("UPDATE reminder_schedules SET state='expired',reason='expired' WHERE tenant_id=?1 AND id=?2",params![tenant,schedule])?;
            continue;
        }
        if s.view.state != "active" || s.view.revision != revision {
            fence(conn, &tenant, &schedule, "authority_changed", now)?;
            continue;
        }
        let mut nonce = vec![0; 32];
        OsRng
            .try_fill_bytes(&mut nonce)
            .map_err(|_| StoreError::Unavailable)?;
        conn.execute("UPDATE reminder_occurrences SET state='claimed',nonce=?3,deadline=?4,updated_at=?5 WHERE tenant_id=?1 AND id=?2",params![tenant,occurrence,nonce,now+30,now])?;
        conn.execute("UPDATE reminder_clock SET claim_order=claim_order+1 WHERE singleton=1 AND claim_order<9223372036854775807",[])?;
        conn.execute("UPDATE reminder_owners SET last_claim=(SELECT claim_order FROM reminder_clock WHERE singleton=1) WHERE tenant_id=?1 AND account_id=?2",params![tenant,account])?;
        return Ok(Some(ReminderLease {
            tenant,
            account,
            occurrence,
            nonce,
        }));
    }
    Ok(None)
}
fn quiet_settings(conn: &Connection, tenant: &str, account: &str) -> Result<QuietHours> {
    Ok(conn.query_row("SELECT revision,enabled,timezone,start_minute,end_minute FROM notification_settings WHERE tenant_id=?1 AND account_id=?2",params![tenant,account],|r|Ok(QuietHours{revision:r.get(0)?,enabled:r.get(1)?,timezone:r.get(2)?,start_minute:r.get(3)?,end_minute:r.get(4)?})).optional()?.unwrap_or(QuietHours{revision:0,enabled:false,timezone:"UTC".into(),start_minute:1320,end_minute:420}))
}
fn notification(
    conn: &Connection,
    tenant: &str,
    account: &str,
    id: &str,
) -> Result<LocalNotification> {
    let mission_id: Option<String>=conn.query_row("SELECT m.id FROM recurring_mission_occurrences o JOIN missions m ON m.tenant_id=o.tenant_id AND m.id=o.mission_id JOIN reminder_schedules s ON s.tenant_id=o.tenant_id AND s.id=o.schedule_id JOIN accounts a ON a.tenant_id=m.tenant_id AND a.id=m.account_id WHERE o.tenant_id=?1 AND o.notification_id=?2 AND m.account_id=?3 AND m.state NOT IN('deleted','quarantined') AND s.state IN('active','completed') AND s.revision=o.schedule_revision AND s.auth_epoch=m.auth_epoch AND a.auth_epoch=m.auth_epoch AND a.status='active' AND a.recovery_required=0 AND a.must_change_password=0 AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?4) AND (s.valid_until IS NULL OR s.valid_until>?4)",params![tenant,id,account,now()?],|r|r.get(0)).optional()?;
    conn.query_row("SELECT id,schedule_id,occurrence_id,title,message,scheduled_for,delivered_at,delayed,quiet,is_read,dismissed FROM local_notifications WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![tenant,account,id],|r|Ok(LocalNotification{mission_id:mission_id.clone(),id:r.get(0)?,schedule_id:r.get(1)?,occurrence_id:r.get(2)?,title:r.get(3)?,message:r.get(4)?,scheduled_for:r.get(5)?,delivered_at:r.get(6)?,delayed:r.get(7)?,quiet:r.get(8)?,read:r.get(9)?,dismissed:r.get(10)?})).optional()?.ok_or(StoreError::NotFound)
}

fn finish(conn: &Connection, lease: &ReminderLease, now: i64) -> Result<LocalNotification> {
    let(schedule,revision,state,deadline,id,title,message,at,delayed):(String,i64,String,Option<i64>,Option<String>,String,String,i64,bool)=conn.query_row("SELECT schedule_id,revision,state,deadline,notification_id,title,message,scheduled_for,delayed FROM reminder_occurrences WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND nonce=?4",params![lease.tenant,lease.account,lease.occurrence,lease.nonce],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?,r.get(8)?))).optional()?.ok_or(StoreError::Conflict)?;
    if crate::recurring_missions::is_recurring(conn, &lease.tenant, &schedule)? {
        return Err(StoreError::Forbidden);
    }
    if state == "delivered" {
        return notification(
            conn,
            &lease.tenant,
            &lease.account,
            &id.ok_or(StoreError::Corrupt)?,
        );
    }
    let s = load(conn, &lease.tenant, &schedule)?;
    if now < at
        || state != "claimed"
        || deadline.is_none_or(|end| now >= end)
        || s.view.state != "active"
        || s.view.revision != revision
        || !eligible(conn, &lease.tenant, &s, now)?
        || s.view.valid_until.is_some_and(|end| now > end)
    {
        return Err(StoreError::Conflict);
    }
    let delayed = delayed || now - at > 5;
    let settings = quiet_settings(conn, &lease.tenant, &lease.account)?;
    let quiet = time::quiet(&settings, now)?;
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO local_notifications(tenant_id,id,account_id,schedule_id,occurrence_id,title,message,scheduled_for,delivered_at,delayed,quiet,quiet_revision) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![lease.tenant,id,lease.account,schedule,lease.occurrence,title,message,at,now,delayed,quiet,settings.revision])?;
    conn.execute("UPDATE reminder_occurrences SET state='delivered',notification_id=?3,deadline=NULL,updated_at=?4,delayed=?5 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,lease.occurrence,id,now,delayed])?;
    if s.slot.is_none() {
        let reason = if matches!(s.view.rule, ScheduleRule::Once { .. }) {
            None
        } else {
            Some("time_horizon")
        };
        conn.execute("UPDATE reminder_schedules SET state='completed',reason=?4,updated_at=?3 WHERE tenant_id=?1 AND id=?2",params![lease.tenant,schedule,now,reason])?;
    }
    notification(conn, &lease.tenant, &lease.account, &id)
}

impl Store {
    pub async fn preview_schedule(
        &self,
        user: &UserContext,
        input: ScheduleDraft,
    ) -> Result<SchedulePreview> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            draft(&input, clock()?)
        })
        .await
    }
    pub async fn create_schedule(
        &self,
        user: &UserContext,
        input: ScheduleCreate,
    ) -> Result<ScheduleView> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = create_in(&tx, &user, &input, clock()?)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn schedules(&self, user: &UserContext) -> Result<Vec<ScheduleView>> {
        let user = user.clone();
        self.call(move |db| {
            let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT id FROM reminder_schedules WHERE tenant_id=?1 AND account_id=?2 AND state<>'revoked' AND NOT EXISTS(SELECT 1 FROM recurring_missions x WHERE x.tenant_id=reminder_schedules.tenant_id AND x.schedule_id=reminder_schedules.id) ORDER BY created_at DESC,id")?.query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            ids.iter().map(|id| owner_view(&db.conn,&u.tenant.0,load(&db.conn,&u.tenant.0,id)?)).collect()
        }).await
    }
    pub async fn edit_schedule(
        &self,
        user: &UserContext,
        id: String,
        input: ScheduleEdit,
    ) -> Result<ScheduleView> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = edit_in(&tx, &user, &id, &input, clock()?)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn control_schedule(
        &self,
        user: &UserContext,
        id: String,
        input: ScheduleControl,
    ) -> Result<ScheduleView> {
        let user = user.clone();
        self.call(move |db| {
            let user = validate(&db.conn, &user, true, false)?;
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let out = control_in(&tx, &user, &id, &input, clock()?)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn schedule_history(
        &self,
        user: &UserContext,
        id: String,
    ) -> Result<ScheduleHistory> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;owned(&db.conn,&u,&id)?;let occurrences=db.conn.prepare("SELECT id,revision,scheduled_for,local_slot,state,reason,notification_id,delayed FROM reminder_occurrences WHERE tenant_id=?1 AND schedule_id=?2 ORDER BY created_at DESC,id DESC LIMIT 50")?.query_map(params![u.tenant.0,id],|r|Ok(ScheduleOccurrence{id:r.get(0)?,revision:r.get(1)?,scheduled_for:r.get(2)?,local_slot:r.get(3)?,state:r.get(4)?,reason:r.get(5)?,notification_id:r.get(6)?,delayed:r.get(7)?}))?.collect::<std::result::Result<Vec<_>,_>>()?;let skipped_ranges=db.conn.prepare("SELECT from_slot,to_slot,count,reason,revision FROM reminder_skipped_ranges WHERE tenant_id=?1 AND schedule_id=?2 ORDER BY created_at DESC,id DESC LIMIT 50")?.query_map(params![u.tenant.0,id],|r|Ok(ScheduleSkippedRange{from_slot:r.get(0)?,to_slot:r.get(1)?,count:r.get(2)?,reason:r.get(3)?,revision:r.get(4)?}))?.collect::<std::result::Result<Vec<_>,_>>()?;Ok(ScheduleHistory{occurrences,skipped_ranges})}).await
    }
    /// Trusted runtime clock entry point; never registered as an HTTP/model tool.
    pub async fn enqueue_scheduled_reminders(&self) -> Result<u32> {
        self.enqueue_scheduled_reminders_at(clock()?).await
    }
    #[doc(hidden)]
    pub async fn enqueue_scheduled_reminders_at(&self, time: i64) -> Result<u32> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            time::instant(time)?;
            let n = enqueue(&tx, time)?;
            tx.commit()?;
            Ok(n)
        })
        .await
    }
    #[doc(hidden)]
    pub async fn claim_scheduled_reminder_at(&self, time: i64) -> Result<Option<ReminderLease>> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            time::instant(time)?;
            let out = claim(&tx, time)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn claim_scheduled_reminder(&self) -> Result<Option<ReminderLease>> {
        self.claim_scheduled_reminder_at(clock()?).await
    }
    #[doc(hidden)]
    pub async fn finish_scheduled_reminder_at(
        &self,
        lease: &ReminderLease,
        time: i64,
    ) -> Result<LocalNotification> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            time::instant(time)?;
            let out = finish(&tx, &lease, time)?;
            tx.commit()?;
            Ok(out)
        })
        .await
    }
    pub async fn finish_scheduled_reminder(
        &self,
        lease: &ReminderLease,
    ) -> Result<LocalNotification> {
        self.finish_scheduled_reminder_at(lease, clock()?).await
    }
    pub async fn quiet_hours(&self, user: &UserContext) -> Result<QuietHours> {
        let user = user.clone();
        self.call(move |db| {
            let u = validate(&db.conn, &user, true, false)?;
            quiet_settings(&db.conn, &u.tenant.0, &u.account.id)
        })
        .await
    }
    pub async fn set_quiet_hours(
        &self,
        user: &UserContext,
        input: QuietHoursChange,
    ) -> Result<QuietHours> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;time::zone(&input.timezone)?;if input.start_minute>=1440||input.end_minute>=1440||input.enabled&&input.start_minute==input.end_minute{return Err(StoreError::InvalidInput)}let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let old=quiet_settings(&tx,&u.tenant.0,&u.account.id)?;if old.revision!=input.expected_revision{return Err(StoreError::Conflict)}tx.execute("INSERT INTO notification_settings VALUES(?1,?2,?3,?4,?5,?6,?7) ON CONFLICT(tenant_id,account_id) DO UPDATE SET revision=excluded.revision,enabled=excluded.enabled,timezone=excluded.timezone,start_minute=excluded.start_minute,end_minute=excluded.end_minute",params![u.tenant.0,u.account.id,old.revision+1,input.enabled,input.timezone,input.start_minute,input.end_minute])?;let out=quiet_settings(&tx,&u.tenant.0,&u.account.id)?;tx.commit()?;Ok(out)}).await
    }
    pub async fn notifications(
        &self,
        user: &UserContext,
        before: Option<String>,
        dismissed: bool,
    ) -> Result<NotificationPage> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;let cursor=before.as_deref().map(|v|{if v.len()>80{return Err(StoreError::InvalidInput)}let(t,id)=v.split_once(':').ok_or(StoreError::InvalidInput)?;uuid(id)?;Ok::<_,StoreError>((t.parse::<i64>().map_err(|_|StoreError::InvalidInput)?,id.to_owned()))}).transpose()?;let ids=db.conn.prepare("SELECT id,delivered_at FROM local_notifications WHERE tenant_id=?1 AND account_id=?2 AND (?3 OR dismissed=0) AND (?4 IS NULL OR delivered_at<?4 OR(delivered_at=?4 AND id<?5)) ORDER BY delivered_at DESC,id DESC LIMIT 101")?.query_map(params![u.tenant.0,u.account.id,dismissed,cursor.as_ref().map(|c|c.0),cursor.as_ref().map(|c|c.1.as_str())],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;let next_cursor=if ids.len()>100{let(id,time)=&ids[99];Some(format!("{time}:{id}"))}else{None};let items=ids.iter().take(100).map(|(id,_)|notification(&db.conn,&u.tenant.0,&u.account.id,id)).collect::<Result<Vec<_>>>()?;Ok(NotificationPage{items,next_cursor})}).await
    }
    pub async fn mark_notification(
        &self,
        user: &UserContext,
        id: String,
        dismiss: bool,
    ) -> Result<LocalNotification> {
        let user = user.clone();
        self.call(move|db|{let u=validate(&db.conn,&user,true,false)?;uuid(&id)?;if db.conn.execute("UPDATE local_notifications SET is_read=1,dismissed=CASE WHEN ?4 THEN 1 ELSE dismissed END WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![u.tenant.0,u.account.id,id,dismiss])?!=1{return Err(StoreError::NotFound)}notification(&db.conn,&u.tenant.0,&u.account.id,&id)}).await
    }
}

pub(super) fn create_in(
    conn: &Connection,
    user: &UserContext,
    input: &ScheduleCreate,
    time: i64,
) -> Result<ScheduleView> {
    uuid(&input.request_id)?;
    let encoded = encode(&input.draft)?;
    let digest: [u8; 32] = Sha256::digest(encoded.as_bytes()).into();

    let old:Option<(String,Vec<u8>)>=conn.query_row("SELECT id,request_digest FROM reminder_schedules WHERE tenant_id=?1 AND account_id=?2 AND request_id=?3",params![user.tenant.0,user.account.id,input.request_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    if let Some((id, old)) = old {
        if old != digest {
            return Err(StoreError::Conflict);
        }
        if crate::recurring_missions::is_recurring(conn, &user.tenant.0, &id)? {
            return Err(StoreError::Conflict);
        }
        return owner_view(conn, &user.tenant.0, load(conn, &user.tenant.0, &id)?);
    }
    draft(&input.draft, time)?;
    let slot = time::after(&input.draft.rule, time)?.ok_or(StoreError::InvalidInput)?;
    let count:i64=conn.query_row("SELECT count(*) FROM reminder_schedules WHERE tenant_id=?1 AND account_id=?2 AND state<>'revoked'",params![user.tenant.0,user.account.id],|r|r.get(0))?;
    if count >= 64 {
        return Err(StoreError::BudgetExhausted);
    }
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO reminder_schedules VALUES(?1,?2,?3,?4,?5,1,?6,'active',NULL,?7,?8,?9,?10,?11,?12,?13,?14,?14)",params![user.tenant.0,id,user.account.id,input.request_id,digest.as_slice(),user.epoch,input.draft.title,input.draft.message,encode(&input.draft.rule)?,input.draft.valid_until,time::TZDB,encode(&slot)?,slot.due,time])?;
    conn.execute(
        "INSERT OR IGNORE INTO reminder_owners VALUES(?1,?2,0)",
        params![user.tenant.0, user.account.id],
    )?;
    let out = load(conn, &user.tenant.0, &id)?.view;
    Ok(out)
}

fn edit_scoped(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    input: &ScheduleEdit,
    time: i64,
    recurring: bool,
) -> Result<ScheduleView> {
    let s = if recurring {
        let s = load(conn, &user.tenant.0, id)?;
        if s.account != user.account.id
            || !crate::recurring_missions::is_recurring(conn, &user.tenant.0, id)?
        {
            return Err(StoreError::NotFound);
        }
        s
    } else {
        owned(conn, user, id)?
    };
    if s.view.revision != input.expected_revision || s.view.state == "revoked" {
        return Err(StoreError::Conflict);
    }
    draft(&input.draft, time)?;
    let slot = time::after(&input.draft.rule, time)?.ok_or(StoreError::InvalidInput)?;
    fence(conn, &user.tenant.0, id, "edited", time)?;
    conn.execute("UPDATE reminder_schedules SET revision=revision+1,auth_epoch=?3,title=?4,message=?5,rule=?6,valid_until=?7,timezone_db=?8,state=?9,reason=NULL,updated_at=?10 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,user.epoch,input.draft.title,input.draft.message,encode(&input.draft.rule)?,input.draft.valid_until,time::TZDB,if s.view.state=="paused"||!eligible(conn,&user.tenant.0,&s,time)?{"paused"}else{"active"},time])?;
    save_slot(conn, &user.tenant.0, id, Some(&slot))?;
    let out = load(conn, &user.tenant.0, id)?.view;
    Ok(out)
}

fn control_scoped(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    input: &ScheduleControl,
    time: i64,
    recurring: bool,
) -> Result<ScheduleView> {
    let s = if recurring {
        let s = load(conn, &user.tenant.0, id)?;
        if s.account != user.account.id
            || !crate::recurring_missions::is_recurring(conn, &user.tenant.0, id)?
        {
            return Err(StoreError::NotFound);
        }
        s
    } else {
        owned(conn, user, id)?
    };
    let target = match input.action.as_str() {
        "pause" => "paused",
        "resume" => "active",
        "revoke" => "revoked",
        _ => return Err(StoreError::InvalidInput),
    };
    let expired_resume = input.action == "resume"
        && s.view.state == "expired"
        && matches!(
            s.view.reason.as_deref(),
            Some("pause_period" | "validity_expired")
        );
    if Some(s.view.revision) == input.expected_revision.checked_add(1)
        && (s.view.state == target || expired_resume)
    {
        return owner_view(conn, &user.tenant.0, s);
    }
    if s.view.revision != input.expected_revision || s.view.state == "revoked" {
        return Err(StoreError::Conflict);
    }

    let stale_grant = s.view.state == "active" && !eligible(conn, &user.tenant.0, &s, time)?;
    if (input.action == "pause" && s.view.state != "active")
        || (input.action == "resume" && s.view.state != "paused" && !stale_grant)
    {
        return Err(StoreError::Conflict);
    }

    fence(conn, &user.tenant.0, id, &input.action, time)?;
    let slot = if target == "active" {
        time::after(&s.view.rule, time)?
    } else {
        s.slot.clone()
    };
    let mut state = target;
    let mut reason = None;
    if target == "active" && s.view.valid_until.is_some_and(|end| time > end) {
        state = "expired";
        reason = Some("validity_expired");
    } else if target == "active" && slot.is_none() {
        state = "expired";
        reason = Some("pause_period");
    }
    if target == "active"
        && let Some(first) = &s.slot
        && first.due <= time
    {
        let plan = time::plan(&s.view.rule, first, time)?;
        let count = plan.skipped.as_ref().map(|v| v.2).unwrap_or(0)
            + plan.gaps.len() as i64
            + i64::from(plan.latest.is_some());
        let last = plan
            .gaps
            .iter()
            .chain(plan.latest.iter())
            .max_by_key(|slot| slot.due)
            .map(|slot| slot.local.as_str())
            .unwrap_or(&first.local);
        if count > 0 {
            range(
                conn,
                &user.tenant.0,
                &s,
                (&first.local, last, count),
                "pause_period",
                time,
            )?;
        }
    }
    conn.execute("UPDATE reminder_schedules SET revision=revision+1,auth_epoch=?3,state=?4,reason=?6,updated_at=?5 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,user.epoch,state,time,reason])?;
    save_slot(
        conn,
        &user.tenant.0,
        id,
        if matches!(state, "active" | "paused") {
            slot.as_ref()
        } else {
            None
        },
    )?;
    let out = load(conn, &user.tenant.0, id)?.view;

    Ok(out)
}

pub(super) fn edit_in(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    input: &ScheduleEdit,
    time: i64,
) -> Result<ScheduleView> {
    edit_scoped(conn, user, id, input, time, false)
}
pub(super) fn edit_recurring_in(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    input: &ScheduleEdit,
    time: i64,
) -> Result<ScheduleView> {
    edit_scoped(conn, user, id, input, time, true)
}

pub(super) fn control_in(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    input: &ScheduleControl,
    time: i64,
) -> Result<ScheduleView> {
    control_scoped(conn, user, id, input, time, false)
}
pub(super) fn control_recurring_in(
    conn: &Connection,
    user: &UserContext,
    id: &str,
    input: &ScheduleControl,
    time: i64,
) -> Result<ScheduleView> {
    control_scoped(conn, user, id, input, time, true)
}

pub(super) fn recurring_view(
    conn: &Connection,
    user: &UserContext,
    id: &str,
) -> Result<ScheduleView> {
    let s = load(conn, &user.tenant.0, id)?;
    if s.account != user.account.id
        || !crate::recurring_missions::is_recurring(conn, &user.tenant.0, id)?
    {
        return Err(StoreError::NotFound);
    }
    owner_view(conn, &user.tenant.0, s)
}
#[allow(clippy::too_many_arguments)]
pub(super) fn publish_mission_notice(
    conn: &Connection,
    tenant: &str,
    account: &str,
    schedule: &str,
    occurrence: &str,
    at: i64,
    time: i64,
    message: &str,
) -> Result<String> {
    let settings = quiet_settings(conn, tenant, account)?;
    let quiet = time::quiet(&settings, time)?;
    let id = Uuid::new_v4().to_string();
    conn.execute("INSERT INTO local_notifications(tenant_id,id,account_id,schedule_id,occurrence_id,title,message,scheduled_for,delivered_at,delayed,quiet,quiet_revision) VALUES(?1,?2,?3,?4,?5,'예약한 일의 실행 기록',?6,?7,?8,?9,?10,?11)",params![tenant,id,account,schedule,occurrence,message,at,time,time-at>5,quiet,settings.revision])?;
    Ok(id)
}
