use super::*;
use moosuri_core::personal_browser::{TaskDetail, TaskEvent, TaskPage, TaskSummary};

const PAGE_SIZE: usize = 20;
// The run id remains valid when another workspace replaces this thread's active browser.
const TASKS: &str = "
    SELECT r.id,r.thread_id,substr(r.user_text,1,96),r.created_at,
           CASE WHEN json_extract(b.view,'$.state')='closed' AND r.state='completed'
                THEN 'completed' ELSE json_extract(b.view,'$.state') END,
           r.admitted_order
    FROM chat_runs r
    JOIN personal_browser_jobs b ON b.tenant_id=r.tenant_id AND b.run_id=r.id AND b.thread_id=r.thread_id
    JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id AND t.account_id=r.account_id
    WHERE r.tenant_id=?1 AND r.account_id=?2 AND b.volume_id=?3 AND b.generation=?4 AND t.mission_internal=0";

fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskSummary> {
    let state: String = row.get(4)?;
    let actor = match state.as_str() {
        "ready" | "user_control" | "outcome_unknown" => "member",
        "starting" | "reading" | "resuming" | "stopping" => "assistant",
        _ => "none",
    };
    Ok(TaskSummary {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        title: row.get(2)?,
        created_at: row.get(3)?,
        state,
        next_actor: actor.into(),
    })
}

impl Store {
    pub async fn personal_task_page(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        before: Option<i64>,
    ) -> Result<TaskPage> {
        if before.is_some_and(|value| value < 1) {
            return Err(StoreError::InvalidInput);
        }
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            expire_workers(&tx, owner)?;
            let sql = format!(
                "{TASKS} AND r.admitted_order < ?5 ORDER BY r.admitted_order DESC LIMIT ?6"
            );
            let mut rows = tx
                .prepare(&sql)?
                .query_map(
                    params![
                        user.tenant.0,
                        user.account.id,
                        owner.volume.to_string(),
                        owner.generation as i64,
                        before.unwrap_or(i64::MAX),
                        (PAGE_SIZE + 1) as i64
                    ],
                    |row| Ok((summary(row)?, row.get::<_, i64>(5)?)),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let next_before = (rows.len() > PAGE_SIZE).then(|| rows[PAGE_SIZE - 1].1.to_string());
            rows.truncate(PAGE_SIZE);
            tx.commit()?;
            Ok(TaskPage {
                items: rows.into_iter().map(|(task, _)| task).collect(),
                next_before,
            })
        })
        .await
    }

    pub async fn personal_task_detail(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        id: Uuid,
    ) -> Result<TaskDetail> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            expire_workers(&tx, owner)?;
            let id = id.to_string();
            let task = tx
                .query_row(
                    &format!("{TASKS} AND r.id=?5"),
                    params![user.tenant.0, user.account.id, owner.volume.to_string(), owner.generation as i64, id],
                    summary,
                )
                .optional()?
                .ok_or(StoreError::NotFound)?;
            let (request, thread_title, reason, result, observation) = tx.query_row(
                "SELECT r.user_text,t.title,json_extract(b.view,'$.reason'),a.text,o.text
                 FROM chat_runs r JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id
                 JOIN personal_browser_jobs b ON b.tenant_id=r.tenant_id AND b.run_id=r.id
                 LEFT JOIN chat_output a ON a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.item='browser-answer'
                 LEFT JOIN chat_output o ON o.tenant_id=r.tenant_id AND o.run_id=r.id AND o.item='browser-observation'
                 WHERE r.tenant_id=?1 AND r.id=?2",
                params![user.tenant.0, id],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?;
            let history = tx
                .prepare("SELECT state,at FROM personal_task_history WHERE tenant_id=?1 AND run_id=?2 ORDER BY id")?
                .query_map(params![user.tenant.0,id], |row| {
                    Ok(TaskEvent { state: row.get(0)?, at: row.get(1)? })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tx.commit()?;
            Ok(TaskDetail {
                task, request, thread_title, reason, history, result, observation,
            })
        })
        .await
    }
}

#[cfg(test)]
mod tests;
