//! Saved followups reuse fixed reminders; artifact revisions fence all publications.
use super::*;

pub(crate) fn is_followup(conn: &Connection, tenant: &str, schedule: &str) -> Result<bool> {
    Ok(conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM followup_reminders WHERE tenant_id=?1 AND schedule_id=?2)",
        params![tenant, schedule],
        |r| r.get(0),
    )?)
}
pub(super) fn current(conn: &Connection, tenant: &str, schedule: &str, at: i64) -> Result<bool> {
    if !is_followup(conn, tenant, schedule)? {
        return Ok(true);
    }
    Ok(conn.query_row("SELECT EXISTS(SELECT 1 FROM followup_reminders f JOIN personal_artifacts a ON a.tenant_id=f.tenant_id AND a.id=f.artifact_id AND a.revision=f.artifact_revision JOIN personal_artifact_versions v ON v.tenant_id=a.tenant_id AND v.artifact_id=a.id AND v.revision=a.revision WHERE f.tenant_id=?1 AND f.schedule_id=?2 AND a.status='active' AND v.status='active' AND v.draft IS NOT NULL AND NOT EXISTS(SELECT 1 FROM followup_reminders newer JOIN reminder_schedules ns ON ns.tenant_id=newer.tenant_id AND ns.id=newer.schedule_id WHERE newer.tenant_id=f.tenant_id AND newer.artifact_id=f.artifact_id AND newer.artifact_revision=f.artifact_revision AND newer.scheduled_for>f.scheduled_for AND newer.scheduled_for<=?3 AND ns.state IN('active','completed')))",params![tenant,schedule,at],|r|r.get(0))?)
}
/// Every artifact edit/deletion and recursive provenance withdrawal enters here in
/// the mutation transaction. Already dispatched Push outcomes remain factual;
/// passive display and retries lose authority through the permanent notice latch.
pub(crate) fn retire(
    conn: &Connection,
    tenant: &str,
    artifact: &str,
    revision: Option<i64>,
    at: i64,
) -> Result<()> {
    super::suggestions::retire(conn, tenant, artifact, revision, at)?;
    let ids=conn.prepare("SELECT schedule_id FROM followup_reminders WHERE tenant_id=?1 AND artifact_id=?2 AND (?3 IS NULL OR artifact_revision=?3)")?.query_map(params![tenant,artifact,revision],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for id in ids {
        fence(conn, tenant, &id, "artifact_changed", at)?;
        conn.execute("UPDATE reminder_schedules SET state='revoked',reason='artifact_changed',title='변경되거나 철회된 기록',message='현재 기록에서 확인해 주세요.',next_slot=NULL,next_due=NULL,updated_at=?3 WHERE tenant_id=?1 AND id=?2",params![tenant,id,at])?;
        conn.execute("UPDATE reminder_occurrences SET title='',message='' WHERE tenant_id=?1 AND schedule_id=?2",params![tenant,id])?;
        conn.execute("UPDATE local_notifications SET title='변경되거나 철회된 기록',message='현재 기록에서 확인해 주세요.',dismissed=1,push_suppressed=1 WHERE tenant_id=?1 AND schedule_id=?2",params![tenant,id])?;
        conn.execute("UPDATE push_queue SET state='suppressed',nonce=NULL,deadline=NULL,finished_at=?3,reason='artifact_changed' WHERE tenant_id=?1 AND state IN('queued','claimed') AND notification_id IN(SELECT id FROM local_notifications WHERE tenant_id=?1 AND schedule_id=?2)",params![tenant,id,at])?;
    }
    Ok(())
}
/// No migration backfill: only an explicit admitted save reaches this function.
pub(crate) fn schedule(
    conn: &Connection,
    tenant: &str,
    artifact: &str,
    revision: i64,
    draft: &ArtifactDraft,
    at: i64,
) -> Result<()> {
    let ArtifactPayload::Followup {
        lifecycle,
        muted,
        due,
        review,
        snooze,
        ..
    } = &draft.payload
    else {
        return Ok(());
    };
    if *muted
        || matches!(
            lifecycle,
            FollowupLifecycle::Resolved | FollowupLifecycle::Cancelled
        )
    {
        return Ok(());
    }
    let owner:Option<(String,i64)>=conn.query_row("SELECT id,auth_epoch FROM accounts WHERE tenant_id=?1 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>?2)",params![tenant,at],|r|Ok((r.get(0)?,r.get(1)?))).optional()?;
    let Some((account, epoch)) = owner else {
        return Err(StoreError::Forbidden);
    };
    let mut events = std::collections::BTreeMap::<i64, (&str, ArtifactDate)>::new();
    let snoozed = snooze.as_ref().filter(|d| d.utc > at);
    if let Some(d) = snoozed {
        events.insert(d.utc, ("snooze", d.clone()));
    }
    for (kind, date) in [("due", due), ("review", review)] {
        if let Some(d) = date
            .as_ref()
            .filter(|d| d.utc > at && snoozed.is_none_or(|s| d.utc > s.utc))
        {
            if let Some(event) = events.get_mut(&d.utc) {
                event.0 = "due_review";
            } else {
                events.insert(d.utc, (kind, d.clone()));
            }
        }
    }
    let mut source:Option<String>=conn.query_row("SELECT source_schedule_id FROM followup_reminders WHERE tenant_id=?1 AND artifact_id=?2 LIMIT 1",params![tenant,artifact],|r|r.get(0)).optional()?;
    // Artifact labels allow 200 bytes; reminder labels allow 160. Never split UTF-8.
    let mut title = draft.title.clone();
    while title.len() > 160 {
        title.pop();
    }
    for (fire, (kind, date)) in events {
        // Existing reminder oracle has an explicit 2100 horizon. Later record
        // dates remain saved and briefing-visible, without execution authority.
        if fire > 4_102_444_800 {
            continue;
        }
        let delivered:bool=conn.query_row("SELECT EXISTS(SELECT 1 FROM followup_reminders f JOIN reminder_occurrences o ON o.tenant_id=f.tenant_id AND o.schedule_id=f.schedule_id WHERE f.tenant_id=?1 AND f.artifact_id=?2 AND f.scheduled_for=?3 AND o.state='delivered')",params![tenant,artifact,fire],|r|r.get(0))?;
        if delivered {
            continue;
        }
        let rule = ScheduleRule::Once { at: fire };
        let slot = time::after(&rule, at)?.ok_or(StoreError::InvalidInput)?;
        let id = Uuid::new_v4().to_string();
        let source_id = source.get_or_insert_with(|| id.clone()).clone();
        let message = match kind {
            "due" => "기한이 되었습니다.",
            "review" => "다시 살펴볼 시간입니다.",
            "due_review" => "기한과 재검토 시각이 되었습니다.",
            _ => "미뤄 둔 기록을 살펴볼 시간입니다.",
        };
        conn.execute("INSERT INTO reminder_schedules VALUES(?1,?2,?3,?2,zeroblob(32),1,?4,'active',NULL,?5,?6,?7,NULL,?8,?9,?10,?11,?11)",params![tenant,id,account,epoch,title,message,encode(&rule)?,time::TZDB,encode(&slot)?,slot.due,at])?;
        conn.execute(
            "INSERT INTO followup_reminders VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
            params![
                tenant,
                id,
                source_id,
                artifact,
                revision,
                kind,
                date.utc,
                date.timezone,
                fire
            ],
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO reminder_owners VALUES(?1,?2,0)",
            params![tenant, account],
        )?;
    }
    if let Some(source) = source {
        conn.execute(
            "UPDATE reminder_schedules SET title=?3 WHERE tenant_id=?1 AND id=?2",
            params![tenant, source, title],
        )?;
    }
    Ok(())
}
fn receipt_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<FollowupReminder> {
    Ok(FollowupReminder {
        artifact_id: r.get(0)?,
        artifact_revision: r.get(1)?,
        date_kind: r.get(2)?,
        date: ArtifactDate {
            utc: r.get(3)?,
            timezone: r.get(4)?,
        },
        scheduled_for: r.get(5)?,
        state: r.get(6)?,
    })
}
pub(crate) fn receipts(
    conn: &Connection,
    tenant: &str,
    artifact: &str,
    revision: i64,
) -> Result<Vec<FollowupReminder>> {
    Ok(conn.prepare("SELECT f.artifact_id,f.artifact_revision,f.date_kind,f.date_utc,f.date_timezone,f.scheduled_for,s.state FROM followup_reminders f JOIN reminder_schedules s ON s.tenant_id=f.tenant_id AND s.id=f.schedule_id WHERE f.tenant_id=?1 AND f.artifact_id=?2 AND f.artifact_revision=?3 ORDER BY f.scheduled_for")?.query_map(params![tenant,artifact,revision],receipt_row)?.collect::<std::result::Result<Vec<_>,_>>()?)
}
pub(super) fn for_notification(
    conn: &Connection,
    tenant: &str,
    account: &str,
    id: &str,
) -> Result<Option<FollowupReminder>> {
    Ok(conn.query_row("SELECT f.artifact_id,f.artifact_revision,f.date_kind,f.date_utc,f.date_timezone,f.scheduled_for,s.state FROM local_notifications n JOIN followup_reminders f ON f.tenant_id=n.tenant_id AND f.schedule_id=n.schedule_id JOIN reminder_schedules s ON s.tenant_id=f.tenant_id AND s.id=f.schedule_id JOIN personal_artifacts a ON a.tenant_id=f.tenant_id AND a.id=f.artifact_id AND a.revision=f.artifact_revision WHERE n.tenant_id=?1 AND n.account_id=?2 AND n.id=?3 AND a.status='active' AND s.state IN('active','completed')",params![tenant,account,id],receipt_row).optional()?)
}
/// Offline catch-up sends the latest elapsed date for each record, at most one;
/// earlier elapsed pending dates become factual skipped receipts, never a flood.
pub(super) fn coalesce(conn: &Connection, at: i64) -> Result<()> {
    let ids=conn.prepare("SELECT f.tenant_id,f.schedule_id FROM followup_reminders f JOIN reminder_schedules s ON s.tenant_id=f.tenant_id AND s.id=f.schedule_id WHERE s.state='active' AND f.scheduled_for<=?1 AND EXISTS(SELECT 1 FROM followup_reminders newer JOIN reminder_schedules ns ON ns.tenant_id=newer.tenant_id AND ns.id=newer.schedule_id WHERE newer.tenant_id=f.tenant_id AND newer.artifact_id=f.artifact_id AND newer.artifact_revision=f.artifact_revision AND newer.scheduled_for>f.scheduled_for AND newer.scheduled_for<=?1 AND ns.state IN('active','completed')) LIMIT 512")?.query_map([at],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for (tenant, id) in ids {
        fence(conn, &tenant, &id, "followup_coalesced", at)?;
        let schedule = load(conn, &tenant, &id)?;
        if let Some(slot) = &schedule.slot {
            occurrence(
                conn,
                &tenant,
                &schedule,
                slot,
                "skipped",
                Some("followup_coalesced"),
                at,
            )?;
        }
        conn.execute("UPDATE reminder_schedules SET state='completed',reason='followup_coalesced',next_slot=NULL,next_due=NULL WHERE tenant_id=?1 AND id=?2",params![tenant,id])?;
    }
    Ok(())
}
#[cfg(test)]
mod tests;
