//! Saved research reports retain the completed run. Excerpts are resolved from
//! current member sources on every read, including reads of older reports.
use super::*;
use moosuri_core::personal_browser::{ConversationReport, TaskSummary};

pub(super) fn report_id(
    c: &Connection,
    owner: VolumeOwner,
    thread: &str,
    task: &str,
    run: Option<&str>,
) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT id FROM member_conversation_reports WHERE tenant_id=?1 AND account_id=?2
         AND volume_id=?3 AND generation=?4 AND thread_id=?5 AND task_id=?6 AND run_id=?7",
        params![
            owner.tenant.to_string(),
            owner.account.to_string(),
            owner.volume.to_string(),
            owner.generation as i64,
            thread,
            task,
            run
        ],
        |r| r.get(0),
    )
    .optional()?)
}

pub(super) fn save_report(
    c: &Connection,
    owner: VolumeOwner,
    thread: &str,
    task: &str,
    run: &str,
) -> Result<String> {
    if let Some(id) = report_id(c, owner, thread, task, Some(run))? {
        return Ok(id);
    }
    let id = Uuid::new_v4().to_string();
    let saved = c.execute(
        "INSERT INTO member_conversation_reports(id,tenant_id,account_id,volume_id,generation,thread_id,task_id,run_id,title)
         SELECT ?1,w.tenant_id,w.account_id,w.volume_id,w.generation,?2,?3,r.id,w.title || ' · 조사 보고서'
         FROM member_conversation_runs cr JOIN member_conversation_tasks t ON t.id=cr.task_id
         JOIN member_work_runs r ON r.id=cr.run_id JOIN member_workflows w ON w.id=r.workflow_id AND w.id=t.id
         WHERE t.id=?3 AND t.thread_id=?2 AND t.state='completed' AND r.id=?4 AND w.tenant_id=?5 AND w.account_id=?6 AND w.volume_id=?7 AND w.generation=?8",
        params![id, thread, task, run, owner.tenant.to_string(), owner.account.to_string(), owner.volume.to_string(), owner.generation as i64],
    )?;
    if saved != 1 {
        return Err(StoreError::NotFound);
    }
    Ok(id)
}

impl Store {
    pub async fn conversation_work_report(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        thread: Uuid,
        id: Uuid,
    ) -> Result<ConversationReport> {
        self.call(move |db| {
            let tx = db.conn.transaction()?;
            let thread = thread.to_string();
            authorize_member(&tx, owner, &user)?;
            chat::thread_exists(&tx, &user, &thread)?;
            let id = id.to_string();
            if let Some(report) = workflows::group_report(&tx, owner, &thread, &id)? {
                tx.commit()?;
                return Ok(report);
            }
            let (title, run_id, work_title, created_at) = tx.query_row(
                "SELECT d.title,r.id,w.title,r.created_at FROM member_conversation_reports d
                 JOIN member_conversation_tasks t ON t.id=d.task_id AND t.thread_id=d.thread_id
                 JOIN member_conversation_runs cr ON cr.task_id=t.id AND cr.run_id=d.run_id
                 JOIN member_work_runs r ON r.id=cr.run_id JOIN member_workflows w ON w.id=r.workflow_id AND w.id=t.id
                 WHERE d.id=?1 AND d.thread_id=?2 AND d.tenant_id=?3 AND d.account_id=?4
                 AND d.volume_id=?5 AND d.generation=?6 AND w.tenant_id=d.tenant_id
                 AND w.account_id=d.account_id AND w.volume_id=d.volume_id AND w.generation=d.generation",
                params![id, thread, user.tenant.0, user.account.id, owner.volume.to_string(), owner.generation as i64],
                |r| Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get(2)?,r.get(3)?)),
            ).optional()?.ok_or(StoreError::NotFound)?;
            let detail = workflows::detail(&tx, owner, TaskSummary {
                id: run_id.clone(), thread_id: thread, title: work_title, state: "completed".into(),
                next_actor: "none".into(), created_at, kind: "research".into(), preview: None,
                scheduled_at: None, schedule_id: None,
            })?;
            let mut body = detail.result.ok_or(StoreError::Corrupt)?;
            let sources: Vec<_> = detail.history.iter().filter(|event| event.state == "source_read").collect();
            if !sources.is_empty() {
                body.push_str("\n\n읽은 근거");
                for (index, source) in sources.iter().enumerate() {
                    use std::fmt::Write;
                    write!(body, "\n{}. {}\n{}", index + 1, source.title.as_deref().unwrap_or("자료"), source.detail.as_deref().unwrap_or_default())
                        .map_err(|_| StoreError::Corrupt)?;
                }
            }
            tx.commit()?;
            Ok(ConversationReport { id, title, body, run_id })
        }).await
    }
}
