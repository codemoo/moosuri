//! Thread and task navigation for work whose receipt lives in a separate member domain.
use super::*;
use moosuri_core::personal_browser::{TaskDetail, TaskEvent, TaskSummary};

pub(super) const ROWS: &str = "SELECT l.id,l.thread_id,
 CASE l.kind WHEN 'application' THEN '지원 신청 준비' WHEN 'shopping' THEN '구매 조건 비교'
 WHEN 'mail_draft' THEN 'Gmail 회신 초안' ELSE '개인 일정' END,l.created_at,
 CASE WHEN l.auth_epoch!=a.auth_epoch THEN 'withdrawn' ELSE coalesce((SELECT e.state
 FROM member_linked_work_events e WHERE e.work_id=l.id ORDER BY sequence DESC LIMIT 1),'ready') END,
 l.admitted_order AS ord,l.kind,NULL,NULL,NULL FROM member_linked_work l
 JOIN accounts a ON a.tenant_id=l.tenant_id AND a.id=l.account_id
 WHERE l.tenant_id=?1 AND l.account_id=?2 AND l.volume_id=?3 AND l.generation=?4";

pub(crate) fn admit(
    c: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    thread: &str,
    operation: &str,
    kind: &str,
    message: &str,
    state: &str,
) -> Result<()> {
    authorize_member(c, owner, user)?;
    chat::thread_exists(c, user, thread)?;
    let existing: Option<String> = c.query_row("SELECT thread_id FROM member_linked_work WHERE tenant_id=?1 AND account_id=?2 AND kind=?3 AND operation_id=?4",
        params![user.tenant.0,user.account.id,kind,operation], |row|row.get(0)).optional()?;
    if let Some(original) = existing {
        return if original == thread {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        };
    }
    let turn = Uuid::new_v4().to_string();
    let work = Uuid::new_v4().to_string();
    workflows::member_work_turn(
        c,
        user,
        thread,
        &turn,
        operation,
        message,
        "member-linked-work",
        "이 대화에서 작업 내용을 확인할 수 있습니다.".into(),
    )?;
    c.execute("INSERT INTO member_linked_work(id,kind,operation_id,origin_turn_id,tenant_id,account_id,volume_id,generation,thread_id,auth_epoch,created_at,admitted_order)
      SELECT ?1,?2,?3,id,tenant_id,account_id,?4,?5,thread_id,auth_epoch,created_at,admitted_order FROM chat_runs WHERE id=?6 AND tenant_id=?7",
      params![work,kind,operation,owner.volume.to_string(),owner.generation as i64,turn,user.tenant.0])?;
    c.execute(
        "INSERT INTO member_linked_work_events(work_id,state,at) VALUES(?1,?2,unixepoch())",
        params![work, state],
    )?;
    Ok(())
}
impl Store {
    pub async fn link_member_calendar(
        &self,
        owner: VolumeOwner,
        user: &UserContext,
        thread: Uuid,
        operation: Uuid,
        message: String,
    ) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            admit(
                &tx,
                owner,
                &user,
                &thread.to_string(),
                &operation.to_string(),
                "calendar",
                &message,
                "preview",
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

pub(super) fn detail(
    c: &Connection,
    key: &[u8; 32],
    owner: VolumeOwner,
    user: &UserContext,
    task: TaskSummary,
) -> Result<TaskDetail> {
    let (thread_title, turn, request, operation): (String, String, String, String) = c.query_row(
        "SELECT t.title,l.origin_turn_id,r.user_text,l.operation_id
       FROM member_linked_work l JOIN threads t ON l.tenant_id=t.tenant_id AND l.thread_id=t.id
       JOIN chat_runs r ON r.tenant_id=l.tenant_id AND r.id=l.origin_turn_id
       WHERE l.tenant_id=?1 AND l.account_id=?2 AND l.id=?3",
        params![user.tenant.0, user.account.id, task.id],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
    )?;
    let history=c.prepare("SELECT state,at FROM (SELECT sequence,state,at FROM member_linked_work_events WHERE work_id=?1
       ORDER BY sequence DESC LIMIT 100) ORDER BY sequence")?.query_map([&task.id],|r|Ok(TaskEvent{state:r.get(0)?,at:Some(r.get(1)?),title:None,detail:None}))?
       .collect::<rusqlite::Result<Vec<_>>>()?;
    let result = if task.kind == "calendar" {
        let id = Uuid::parse_str(&operation).map_err(|_| StoreError::Corrupt)?;
        crate::mail::member_calendar_work_result(c, key, user, owner, id)?
    } else {
        None
    };
    Ok(TaskDetail {
        origin_turn_id: Some(turn),
        report_id: None,
        runs: vec![],
        task,
        request,
        thread_title,
        reason: None,
        history,
        result,
        observation: None,
    })
}
