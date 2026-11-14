//! Fixed timing/background grant and provider-draft snapshot boundaries.
use super::*;
pub(super) fn schedule(
    conn: &Connection,
    user: &UserContext,
    id: &str,
) -> Result<Option<MailSchedule>> {
    Ok(conn
        .query_row(
            "SELECT due_at,timezone FROM mail_schedules WHERE tenant_id=?1 AND action_id=?2",
            params![user.tenant.0, id],
            |r| {
                Ok(MailSchedule {
                    due_at: r.get(0)?,
                    timezone: r.get(1)?,
                })
            },
        )
        .optional()?)
}
pub(super) fn grant_valid(conn: &Connection, user: &UserContext, id: &str) -> Result<bool> {
    // The action approval is the immediate-send authority. Scheduled mail adds a
    // separate revision/epoch-pinned background grant; it is never a session.
    Ok(conn.query_row("SELECT NOT EXISTS(SELECT 1 FROM mail_schedules s JOIN mail_actions a ON a.tenant_id=s.tenant_id AND a.id=s.action_id WHERE s.tenant_id=?1 AND s.action_id=?2 AND (s.approved_revision IS NULL OR s.approved_revision!=a.revision OR s.auth_epoch IS NULL OR s.auth_epoch!=a.auth_epoch))",params![user.tenant.0,id],|r|r.get(0))?)
}
impl Store {
    pub async fn preview_mail_schedule(
        &self,
        user: &UserContext,
        input: MailSchedulePreviewInput,
    ) -> Result<MailSchedule> {
        use chrono::TimeZone;
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            let zone = input
                .timezone
                .parse::<chrono_tz::Tz>()
                .map_err(|_| StoreError::InvalidInput)?;
            let local =
                chrono::NaiveDateTime::parse_from_str(&input.local_datetime, "%Y-%m-%dT%H:%M")
                    .map_err(|_| StoreError::InvalidInput)?;
            let due = zone
                .from_local_datetime(&local)
                .earliest()
                .ok_or(StoreError::InvalidInput)?
                .timestamp();
            if due <= now()? || due > now()? + 366 * 86400 {
                return Err(StoreError::InvalidInput);
            }
            Ok(MailSchedule {
                due_at: due,
                timezone: input.timezone,
            })
        })
        .await
    }

    pub async fn schedule_mail(
        &self,
        user: &UserContext,
        id: String,
        input: MailScheduleInput,
    ) -> Result<MailAction> {
        let user = user.clone();
        self.call(move|db|{
            validate(&db.conn,&user,true,false)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let a=action(&tx,&user,&id)?;
            if a.revision!=input.expected.revision || a.digest!=input.expected.digest || !matches!(a.state.as_str(),"pending_approval"|"approved") || a.content.is_none() || a.expires_at<=now()? {return Err(StoreError::Conflict)}
            let c=connection(&tx,&user,&a.connection_id)?;
            let pin:(i64,i64)=tx.query_row("SELECT connection_epoch,auth_epoch FROM mail_actions WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id],|r|Ok((r.get(0)?,r.get(1)?)))?;
            if c.status!="active" || pin!=(c.epoch,user.epoch) {return Err(StoreError::Conflict)}
            let expires=if let Some(s)=&input.schedule {
                // One instant, <= one year; retain an explicit IANA zone for the
                // preview. F36 permits at most five minutes of lateness.
                if s.due_at<=now()? || s.due_at>now()?+366*86400 || s.timezone.parse::<chrono_tz::Tz>().is_err() {return Err(StoreError::InvalidInput)}
                tx.execute("INSERT INTO mail_schedules(tenant_id,action_id,due_at,timezone) VALUES(?1,?2,?3,?4) ON CONFLICT(tenant_id,action_id) DO UPDATE SET due_at=excluded.due_at,timezone=excluded.timezone,approved_revision=NULL,auth_epoch=NULL",params![user.tenant.0,id,s.due_at,s.timezone])?;
                s.due_at+300
            }else{
                tx.execute("DELETE FROM mail_schedules WHERE tenant_id=?1 AND action_id=?2",params![user.tenant.0,id])?;
                now()?+3600
            };
            tx.execute("UPDATE mail_actions SET revision=revision+1,version=version+1,state='pending_approval',expires_at=?3 WHERE tenant_id=?1 AND id=?2",params![user.tenant.0,id,expires])?;
            let a=action(&tx,&user,&id)?;
            tx.commit()?;
            Ok(a)
        }).await
    }
    /// Return actual immutable bytes, never rebuild from a lossy provider read.
    /// Provider draft mutation does not decide/approve this action.
    pub async fn mail_provider_snapshot(
        &self,
        user: &UserContext,
        id: String,
        expected: MailApproval,
    ) -> Result<(String, FrozenMail)> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, false)?;
            let a = action(&db.conn, &user, &id)?;
            if a.revision != expected.revision
                || a.digest != expected.digest
                || !matches!(a.state.as_str(), "pending_approval" | "approved")
                || a.content.is_none()
                || a.expires_at <= now()?
            {
                return Err(StoreError::Conflict);
            }
            let (raw, reply): (Vec<u8>, Option<String>) = db.conn.query_row(
                "SELECT raw,reply_thread FROM mail_actions WHERE tenant_id=?1 AND id=?2",
                params![user.tenant.0, id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            Ok((
                a.connection_id,
                FrozenMail::restore(raw, digest_bytes(&a.digest)?, reply)
                    .map_err(|_| StoreError::Corrupt)?,
            ))
        })
        .await
    }
}
