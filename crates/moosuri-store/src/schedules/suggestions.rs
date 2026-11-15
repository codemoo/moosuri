//! A fixed product topic backed by literal saved records; no model or execution.
use super::*;
const TOPIC: &str = "followup_review";
const TITLE: &str = "날짜 없는 요청·약속 검토";
const MESSAGE: &str = "저장한 요청·약속에 날짜가 없습니다. 다시 살펴볼 날짜를 정해 보세요.";

pub(super) fn internal(c: &Connection, t: &str, s: &str) -> Result<bool> {
    Ok(c.query_row("SELECT EXISTS(SELECT 1 FROM proactive_suggestions WHERE tenant_id=?1 AND schedule_id=?2 UNION ALL SELECT 1 FROM proactive_topics WHERE tenant_id=?1 AND schedule_id=?2)",params![t,s],|r|r.get(0))?)
}
pub(super) fn current(c: &Connection, t: &str, s: &str) -> Result<bool> {
    Ok(c.query_row("SELECT NOT EXISTS(SELECT 1 FROM proactive_suggestions WHERE tenant_id=?1 AND schedule_id=?2) OR EXISTS(SELECT 1 FROM proactive_suggestions p JOIN personal_artifacts a ON a.tenant_id=p.tenant_id AND a.id=p.artifact_id AND a.revision=p.artifact_revision JOIN personal_artifact_versions v ON v.tenant_id=a.tenant_id AND v.artifact_id=a.id AND v.revision=a.revision WHERE p.tenant_id=?1 AND p.schedule_id=?2 AND a.status='active' AND v.status='active' AND v.draft IS NOT NULL)",params![t,s],|r|r.get(0))?)
}
pub(crate) fn retire(
    c: &Connection,
    t: &str,
    artifact: &str,
    revision: Option<i64>,
    at: i64,
) -> Result<()> {
    let id:Option<String>=c.query_row("SELECT schedule_id FROM proactive_suggestions WHERE tenant_id=?1 AND artifact_id=?2 AND (?3 IS NULL OR artifact_revision=?3)",params![t,artifact,revision],|r|r.get(0)).optional()?;
    if let Some(id) = id {
        fence(c, t, &id, "artifact_changed", at)?;
        c.execute("UPDATE reminder_schedules SET state='revoked',reason='artifact_changed',title='변경되거나 철회된 기록',message='현재 기록에서 확인해 주세요.',next_slot=NULL,next_due=NULL,updated_at=?3 WHERE tenant_id=?1 AND id=?2",params![t,id,at])?;
        c.execute("UPDATE reminder_occurrences SET title='',message='' WHERE tenant_id=?1 AND schedule_id=?2",params![t,id])?;
        c.execute("UPDATE local_notifications SET title='변경되거나 철회된 기록',message='현재 기록에서 확인해 주세요.',dismissed=1,push_suppressed=1 WHERE tenant_id=?1 AND schedule_id=?2",params![t,id])?;
        c.execute("UPDATE push_queue SET state='suppressed',nonce=NULL,deadline=NULL,finished_at=?3,reason='artifact_changed' WHERE tenant_id=?1 AND state IN('queued','claimed') AND notification_id IN(SELECT id FROM local_notifications WHERE tenant_id=?1 AND schedule_id=?2)",params![t,id,at])?;
    }
    Ok(())
}
/// Called solely by explicit artifact save. No scan, migration or idle backfill.
pub(crate) fn save(
    c: &Connection,
    t: &str,
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
        || due.is_some()
        || review.is_some()
        || snooze.is_some()
    {
        return Ok(());
    }
    let (account,epoch):(String,i64)=c.query_row("SELECT id,auth_epoch FROM accounts WHERE tenant_id=?1 AND status='active' AND recovery_required=0 AND must_change_password=0 AND (temporary_expires_at IS NULL OR temporary_expires_at>?2)",params![t,at],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let existing: Option<String> = c
        .query_row(
            "SELECT schedule_id FROM proactive_suggestions WHERE tenant_id=?1 AND artifact_id=?2",
            params![t, artifact],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = &existing {
        let delivered:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM reminder_occurrences WHERE tenant_id=?1 AND schedule_id=?2 AND state='delivered')",params![t,id],|r|r.get(0))?;
        if delivered {
            return Ok(());
        }
    }
    // Pending + currently visible, valid suggestions share one small admission cap.
    let count:i64=c.query_row("SELECT count(*) FROM proactive_suggestions p JOIN reminder_schedules s ON s.tenant_id=p.tenant_id AND s.id=p.schedule_id JOIN personal_artifacts a ON a.tenant_id=p.tenant_id AND a.id=p.artifact_id AND a.revision=p.artifact_revision WHERE p.tenant_id=?1 AND s.account_id=?2 AND a.status='active' AND (s.state IN('active','paused') OR (s.state='completed' AND EXISTS(SELECT 1 FROM local_notifications n WHERE n.tenant_id=s.tenant_id AND n.schedule_id=s.id AND n.dismissed=0 AND n.source_suppressed=0)))",params![t,account],|r|r.get(0))?;
    if count >= 5 {
        return Ok(());
    }
    let last: Option<i64> = if let Some(id) = &existing {
        c.query_row("SELECT max(scheduled_for) FROM reminder_occurrences WHERE tenant_id=?1 AND schedule_id=?2",params![t,id],|r|r.get(0))?
    } else {
        None
    };
    let rule = ScheduleRule::Once {
        at: (at + 60).max(last.unwrap_or(0) + 1),
    };
    let slot = time::after(&rule, at)?.ok_or(StoreError::InvalidInput)?;
    let topic:Option<String>=c.query_row("SELECT schedule_id FROM proactive_topics WHERE tenant_id=?1 AND account_id=?2 AND topic=?3",params![t,account,TOPIC],|r|r.get(0)).optional()?;
    if topic.is_none() {
        let id = Uuid::new_v4().to_string();
        c.execute("INSERT INTO reminder_schedules VALUES(?1,?2,?3,?2,zeroblob(32),1,?4,'revoked','topic',?5,?6,?7,NULL,?8,NULL,NULL,?9,?9)",params![t,id,account,epoch,TITLE,MESSAGE,encode(&rule)?,time::TZDB,at])?;
        c.execute(
            "INSERT INTO proactive_topics VALUES(?1,?2,?3,?4)",
            params![t, account, TOPIC, id],
        )?;
    }
    let mut title = draft.title.clone();
    while title.len() > 160 {
        title.pop();
    }
    if let Some(id) = existing {
        c.execute("UPDATE proactive_suggestions SET artifact_revision=?3 WHERE tenant_id=?1 AND artifact_id=?2",params![t,artifact,revision])?;
        c.execute("UPDATE reminder_schedules SET revision=revision+1,auth_epoch=?3,state='active',reason=NULL,title=?4,message=?5,rule=?6,next_slot=?7,next_due=?8,updated_at=?9 WHERE tenant_id=?1 AND id=?2",params![t,id,epoch,title,MESSAGE,encode(&rule)?,encode(&slot)?,slot.due,at])?;
    } else {
        let id = Uuid::new_v4().to_string();
        c.execute("INSERT INTO reminder_schedules VALUES(?1,?2,?3,?2,zeroblob(32),1,?4,'active',NULL,?5,?6,?7,NULL,?8,?9,?10,?11,?11)",params![t,id,account,epoch,title,MESSAGE,encode(&rule)?,time::TZDB,encode(&slot)?,slot.due,at])?;
        c.execute(
            "INSERT INTO proactive_suggestions VALUES(?1,?2,?3,?4)",
            params![t, artifact, revision, id],
        )?;
    }
    c.execute(
        "INSERT OR IGNORE INTO reminder_owners VALUES(?1,?2,0)",
        params![t, account],
    )?;
    Ok(())
}
pub(super) fn for_notification(
    c: &Connection,
    t: &str,
    a: &str,
    id: &str,
) -> Result<Option<ProactiveSuggestion>> {
    Ok(c.query_row("SELECT p.artifact_id FROM local_notifications n JOIN proactive_suggestions p ON p.tenant_id=n.tenant_id AND p.schedule_id=n.schedule_id JOIN reminder_schedules s ON s.tenant_id=p.tenant_id AND s.id=p.schedule_id JOIN personal_artifacts a ON a.tenant_id=p.tenant_id AND a.id=p.artifact_id AND a.revision=p.artifact_revision JOIN personal_artifact_versions v ON v.tenant_id=a.tenant_id AND v.artifact_id=a.id AND v.revision=a.revision WHERE n.tenant_id=?1 AND n.account_id=?2 AND n.id=?3 AND a.status='active' AND v.status='active' AND v.draft IS NOT NULL AND s.state IN('active','completed')",params![t,a,id],|r|Ok(ProactiveSuggestion{topic:TOPIC.into(),artifact_id:r.get(0)?})).optional()?)
}
#[cfg(test)]
mod tests;
