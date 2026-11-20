use super::*;
use moosuri_core::personal_browser::{TaskDetail, TaskEvent, TaskPage, TaskSummary, task_cursor};

const PAGE_SIZE: usize = 20;
// Source rows already bind their own authority and ownership before pagination.
const CHAT_TASKS:&str="
SELECT r.id,r.thread_id,substr(r.user_text,1,96) AS title,r.created_at,
 CASE WHEN b.run_id IS NULL THEN r.state WHEN json_extract(b.view,'$.state')='closed' AND r.state='completed' THEN 'completed' ELSE json_extract(b.view,'$.state') END AS state,
 r.admitted_order AS ord,CASE WHEN b.run_id IS NULL THEN 'text' ELSE 'browser' END AS kind,
 substr(coalesce(a.text,obs.text),1,140) AS preview,NULL AS scheduled_at,NULL AS schedule_id
 FROM chat_runs r JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id AND t.account_id=r.account_id
 LEFT JOIN personal_browser_jobs b ON b.tenant_id=r.tenant_id AND b.run_id=r.id AND b.thread_id=r.thread_id AND b.volume_id=?3 AND b.generation=?4
 LEFT JOIN personal_text_previews p ON p.tenant_id=r.tenant_id AND p.run_id=r.id AND p.volume_id=?3 AND p.generation=?4
 LEFT JOIN chat_output a ON a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.item=CASE WHEN b.run_id IS NULL THEN 'personal-text-preview-answer' ELSE 'browser-answer' END
 LEFT JOIN chat_output obs ON obs.tenant_id=b.tenant_id AND obs.run_id=b.run_id AND obs.item='browser-observation'
 WHERE r.tenant_id=?1 AND r.account_id=?2 AND t.mission_internal=0 AND (b.run_id IS NOT NULL OR p.run_id IS NOT NULL)";
const WORK_TASKS: &str = "SELECT r.id,coalesce(w.thread_id,''),w.title,r.created_at,'completed',
 r.admitted_order AS ord,'research',NULL,r.due_at,CASE WHEN w.period>0 THEN w.id END
 FROM member_work_runs r JOIN member_workflows w ON w.id=r.workflow_id
 WHERE w.tenant_id=?1 AND w.account_id=?2 AND w.volume_id=?3 AND w.generation=?4";
const INBOX_TASKS:&str="SELECT g.work_id,g.thread_id,'Gmail 받은편지함 조회',g.created_at,g.phase,g.work_order,'gmail',NULL,NULL,NULL FROM member_google_thread_reads g
 JOIN threads t ON t.tenant_id=g.tenant_id AND t.id=g.thread_id AND t.account_id=g.account_id
 WHERE g.tenant_id=?1 AND g.account_id=?2 AND g.volume_id=?3 AND g.generation=?4 AND t.mission_internal=0";

fn all_tasks() -> String {
    format!("{CHAT_TASKS} UNION ALL {WORK_TASKS} UNION ALL {INBOX_TASKS}")
}

fn summary(row: &rusqlite::Row<'_>) -> rusqlite::Result<TaskSummary> {
    let state: String = row.get(4)?;
    let actor = match state.as_str() {
        "ready" | "user_control" | "outcome_unknown" | "unknown" | "awaiting_consent" => "member",
        "starting" | "reading" | "resuming" | "stopping" | "running" | "queued" | "dispatched" => {
            "assistant"
        }
        _ => "none",
    };
    Ok(TaskSummary {
        id: row.get(0)?,
        thread_id: row.get(1)?,
        title: row.get(2)?,
        created_at: row.get(3)?,
        next_actor: actor.into(),
        state,
        kind: row.get(6)?,
        preview: row.get(7)?,
        scheduled_at: row.get(8)?,
        schedule_id: row.get(9)?,
    })
}
impl Store {
    pub async fn personal_task_page(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        before: Option<String>,
    ) -> Result<TaskPage> {
        self.member_task_history_page(owner, user, None, before)
            .await
    }

    pub async fn member_task_history_page(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        schedule_id: Option<Uuid>,
        before: Option<String>,
    ) -> Result<TaskPage> {
        let (time, order) = match before.as_deref() {
            Some(v) => task_cursor(v).ok_or(StoreError::InvalidInput)?,
            None => (i64::MAX, i64::MAX),
        };
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            expire_workers(&tx, owner)?;
            let filter = if time == i64::MAX {
                "ord<?6 AND created_at<=?5"
            } else {
                "(created_at,ord)<(?5,?6)"
            };
            let mut parameters: Vec<rusqlite::types::Value> = vec![
                user.tenant.0.into(),
                user.account.id.into(),
                owner.volume.to_string().into(),
                (owner.generation as i64).into(),
                time.into(),
                order.into(),
                ((PAGE_SIZE + 1) as i64).into(),
            ];
            let source = if let Some(id) = schedule_id {
                parameters.push(id.to_string().into());
                format!("{WORK_TASKS} AND w.period>0 AND w.id=?8")
            } else {
                all_tasks()
            };
            let sql = format!(
                "SELECT * FROM ({source}) WHERE {filter} ORDER BY created_at DESC,ord DESC LIMIT ?7"
            );
            let mut rows = tx
                .prepare(&sql)?
                .query_map(rusqlite::params_from_iter(parameters), |r| {
                    Ok((summary(r)?, r.get::<_, i64>(5)?))
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let next_before = (rows.len() > PAGE_SIZE).then(|| {
                format!(
                    "{}.{}",
                    rows[PAGE_SIZE - 1].0.created_at,
                    rows[PAGE_SIZE - 1].1
                )
            });
            rows.truncate(PAGE_SIZE);
            for (task, _) in &mut rows {
                if task.kind == "research" {
                    task.preview = Some(workflows::preview(&tx, owner, &task.id)?);
                }
            }
            let schedules = if before.is_none() && schedule_id.is_none() {
                workflows::schedules(&tx, owner)?
            } else {
                vec![]
            };
            tx.commit()?;
            Ok(TaskPage {
                items: rows.into_iter().map(|r| r.0).collect(),
                next_before,
                schedules,
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
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            expire_workers(&tx, owner)?;
            let id = id.to_string();
            let task = tx
                .query_row(
                    &format!("SELECT * FROM ({}) WHERE id=?5", all_tasks()),
                    params![
                        user.tenant.0,
                        user.account.id,
                        owner.volume.to_string(),
                        owner.generation as i64,
                        id
                    ],
                    summary,
                )
                .optional()?
                .ok_or(StoreError::NotFound)?;
            let detail = match task.kind.as_str() {
                "research" => workflows::detail(&tx, owner, task)?,
                "gmail" => mail_detail(&tx, &user.tenant.0, task)?,
                _ => chat_detail(&tx, &user.tenant.0, task)?,
            };
            tx.commit()?;
            Ok(detail)
        })
        .await
    }
}
fn event(state: &str, at: Option<i64>, title: Option<&str>, detail: Option<String>) -> TaskEvent {
    TaskEvent {
        state: state.into(),
        at,
        title: title.map(str::to_owned),
        detail,
    }
}
fn chat_detail(c: &Connection, tenant: &str, task: TaskSummary) -> Result<TaskDetail> {
    let (request,thread_title,reason,result,observation):(String,String,Option<String>,Option<String>,Option<String>)=c.query_row(
        "SELECT r.user_text,t.title,coalesce(json_extract(b.view,'$.reason'),r.error),a.text,o.text
         FROM chat_runs r JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id
         LEFT JOIN personal_browser_jobs b ON b.tenant_id=r.tenant_id AND b.run_id=r.id
         LEFT JOIN chat_output a ON a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.item=CASE WHEN b.run_id IS NULL THEN 'personal-text-preview-answer' ELSE 'browser-answer' END
         LEFT JOIN chat_output o ON o.tenant_id=b.tenant_id AND o.run_id=b.run_id AND o.item='browser-observation'
         WHERE r.tenant_id=?1 AND r.id=?2",params![tenant,task.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?)))?;
    let history = if task.kind == "browser" {
        c.prepare("SELECT h.state,h.at,h.page_host,h.output_id,o.text FROM personal_task_history h
            LEFT JOIN chat_output o ON o.tenant_id=h.tenant_id AND o.run_id=h.run_id AND o.id=h.output_id
            WHERE h.tenant_id=?1 AND h.run_id=?2 ORDER BY h.id")?
            .query_map(params![tenant,task.id],|r|{
                let host:Option<String>=r.get(2)?;
                let output:Option<String>=r.get(4)?;
                let linked:Option<String>=r.get(3)?;
                let mut details=Vec::new();
                if let Some(host)=host{details.push(format!("확인한 사이트 · {host}"));}
                if let Some(text)=output{details.push(text);}
                else if linked.is_some(){details.push("이 단계의 본문은 이후 브라우저 응답으로 갱신되었습니다.".into());}
                Ok(event(&r.get::<_,String>(0)?,r.get(1)?,None,(!details.is_empty()).then(||details.join("\n\n"))))
            })?.collect::<rusqlite::Result<Vec<_>>>()?
    } else {
        let (count,finished,model):(Option<i64>,Option<i64>,String)=c.query_row("SELECT p.context_messages,p.finished_at,json_extract(r.model,'$.model') FROM personal_text_previews p JOIN chat_runs r ON r.tenant_id=p.tenant_id AND r.id=p.run_id WHERE p.tenant_id=?1 AND p.run_id=?2",
            params![tenant,task.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
        let mut events = vec![event(
            "starting",
            Some(task.created_at),
            Some("요청 접수"),
            Some(request.clone()),
        )];
        if let Some(n) = count {
            events.push(event(
                "prepared",
                Some(task.created_at),
                Some("대화 문맥 준비"),
                Some(format!("지정 모델 {model} · 이전 메시지 {n}개")),
            ));
        }
        if !matches!(task.state.as_str(), "running" | "queued") {
            events.push(event(
                &task.state,
                finished,
                None,
                result.clone().or_else(|| reason.clone()),
            ));
        }
        events
    };
    Ok(TaskDetail {
        task,
        request,
        thread_title,
        reason,
        history,
        result,
        observation,
    })
}
fn mail_detail(c: &Connection, tenant: &str, task: TaskSummary) -> Result<TaskDetail> {
    let (thread_title,request_id):(String,String)=c.query_row("SELECT t.title,g.request_id FROM member_google_thread_reads g JOIN threads t ON t.tenant_id=g.tenant_id AND t.id=g.thread_id WHERE g.tenant_id=?1 AND g.work_id=?2",params![tenant,task.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let history=c.prepare("SELECT phase,at FROM member_inbox_task_events WHERE tenant_id=?1 AND request_id=?2 ORDER BY id")?
        .query_map(params![tenant,request_id],|r|Ok(event(&r.get::<_,String>(0)?,Some(r.get(1)?),None,None)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    let result = (task.state == "completed").then(|| {
        "Gmail 받은편지함을 확인했습니다. 원래 대화에서 저장된 조회 결과를 열 수 있습니다.".into()
    });
    Ok(TaskDetail {
        task,
        request: "Gmail 받은편지함 조회".into(),
        thread_title,
        reason: None,
        history,
        result,
        observation: None,
    })
}
#[cfg(test)]
mod tests;
