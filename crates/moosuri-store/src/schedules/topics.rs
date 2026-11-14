//! Delivery preferences never modify schedule execution grants.
use super::*;

pub(super) fn muted(conn: &Connection, tenant: &str, account: &str, id: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM notification_source_preferences WHERE tenant_id=?1 AND account_id=?2 AND schedule_id=?3 AND muted=1)",
        params![tenant, account, id], |r| r.get(0),
    )?)
}
fn source(conn: &Connection, tenant: &str, account: &str, id: &str) -> Result<NotificationSource> {
    conn.query_row(
        "SELECT s.id,s.title,EXISTS(SELECT 1 FROM recurring_missions r WHERE r.tenant_id=s.tenant_id AND r.account_id=s.account_id AND r.schedule_id=s.id),coalesce(p.revision,0),coalesce(p.muted,0) FROM reminder_schedules s LEFT JOIN notification_source_preferences p ON p.tenant_id=s.tenant_id AND p.account_id=s.account_id AND p.schedule_id=s.id WHERE s.tenant_id=?1 AND s.account_id=?2 AND s.id=?3",
        params![tenant, account, id], |r| Ok(NotificationSource {
            id:r.get(0)?,title:r.get(1)?,kind:if r.get::<_,bool>(2)? {"recurring_mission"} else {"reminder"}.into(),revision:r.get(3)?,muted:r.get(4)?,
        }),
    ).optional()?.ok_or(StoreError::NotFound)
}
pub(super) fn for_notification(
    conn: &Connection,
    tenant: &str,
    account: &str,
    id: &str,
) -> Result<Option<NotificationSource>> {
    let schedule: Option<String> = conn.query_row(
        "SELECT schedule_id FROM local_notifications WHERE tenant_id=?1 AND account_id=?2 AND id=?3",
        params![tenant,account,id], |r| r.get(0),
    ).optional()?;
    schedule
        .map(|id| source(conn, tenant, account, &id))
        .transpose()
}
impl Store {
    /// Complete list, bounded by admission; revoked/restored sources remain manageable.
    pub async fn muted_notification_sources(
        &self,
        user: &UserContext,
    ) -> Result<Vec<NotificationSource>> {
        let user = user.clone();
        self.call(move |db| {
            let u=validate(&db.conn,&user,true,false)?;
            let ids=db.conn.prepare("SELECT schedule_id FROM notification_source_preferences WHERE tenant_id=?1 AND account_id=?2 AND muted=1 ORDER BY schedule_id LIMIT 128")?
                .query_map(params![u.tenant.0,u.account.id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            ids.iter().map(|id|source(&db.conn,&u.tenant.0,&u.account.id,id)).collect()
        }).await
    }
    pub async fn set_notification_source(
        &self,
        user: &UserContext,
        id: String,
        input: NotificationSourceChange,
    ) -> Result<NotificationSource> {
        let user = user.clone();
        self.call(move |db| {
            uuid(&id)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let u=validate(&tx,&user,true,false)?;
            let old=source(&tx,&u.tenant.0,&u.account.id,&id)?;
            if old.revision!=input.expected_revision {return Err(StoreError::Conflict)}
            if input.muted&&!old.muted {
                let count:i64=tx.query_row("SELECT count(*) FROM notification_source_preferences WHERE tenant_id=?1 AND account_id=?2 AND muted=1",params![u.tenant.0,u.account.id],|r|r.get(0))?;
                if count>=128 {return Err(StoreError::Conflict)}
            }
            tx.execute("INSERT INTO notification_source_preferences(tenant_id,account_id,schedule_id,revision,muted) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(tenant_id,account_id,schedule_id) DO UPDATE SET revision=excluded.revision,muted=excluded.muted",params![u.tenant.0,u.account.id,id,old.revision+1,input.muted])?;
            if input.muted {
                // Latch even unscanned notices. Unmute must never replay old Push.
                tx.execute("UPDATE local_notifications SET push_suppressed=1 WHERE tenant_id=?1 AND account_id=?2 AND schedule_id=?3 AND push_suppressed=0",params![u.tenant.0,u.account.id,id])?;
                tx.execute("UPDATE push_queue SET state='suppressed',nonce=NULL,deadline=NULL,finished_at=?4,reason='source_muted' WHERE tenant_id=?1 AND account_id=?2 AND state IN('queued','claimed') AND notification_id IN(SELECT id FROM local_notifications WHERE tenant_id=?1 AND account_id=?2 AND schedule_id=?3)",params![u.tenant.0,u.account.id,id,clock()?])?;
            }
            let out=source(&tx,&u.tenant.0,&u.account.id,&id)?;
            tx.commit()?;
            Ok(out)
        }).await
    }
}
