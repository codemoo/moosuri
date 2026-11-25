//! A saved follow-up is closed for the original dispatch. Only a separate,
//! explicit text-preview submission with its receipt ID can consume it.
use super::*;
use moosuri_core::{RunInstructionReceipt, RunInstructionRequest};

pub(super) fn saved_follow_up(
    conn: &Connection,
    user: &UserContext,
    thread: &str,
    request: &str,
    text: &str,
) -> Result<Option<String>> {
    let row = conn
        .query_row(
            "SELECT r.id,r.account_id,r.thread_id,i.text FROM run_instructions i
         JOIN chat_runs r ON r.tenant_id=i.tenant_id AND r.id=i.run_id
         WHERE i.tenant_id=?1 AND i.request_id=?2 AND i.reason='text_follow_up_saved'",
            params![user.tenant.0, request],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                ))
            },
        )
        .optional()?;
    match row {
        Some((id, a, t, value)) if a == user.account.id && t == thread && value == text => {
            Ok(Some(id))
        }
        Some(_) => Err(StoreError::Conflict),
        None => Ok(None),
    }
}

pub(super) fn add_context(
    tx: &Connection,
    user: &UserContext,
    source: &str,
    messages: &mut Vec<Message>,
    allow_memory: bool,
) -> Result<()> {
    let run = chat::load_run(tx, &user.tenant.0, source)?;
    messages.push(Message {
        role: "user",
        text: run.text,
    });
    if run.state == RunState::Completed
        && recall::can_use_history(tx, &run.tenant, source, allow_memory)?
    {
        let answer: String = tx.query_row(
            "SELECT text FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item='personal-text-preview-answer'",
            params![run.tenant,source], |row| row.get(0),
        )?;
        // Preserve the existing IPC bound for a long prior answer.
        let mut end = answer.len().min(32 * 1024);
        while !answer.is_char_boundary(end) {
            end -= 1;
        }
        messages.push(Message {
            role: "assistant",
            text: answer[..end].to_owned(),
        });
    }
    Ok(())
}

impl Store {
    pub async fn save_text_follow_up(
        &self,
        user: &UserContext,
        owner: personal_volume::VolumeOwner,
        thread: String,
        run_id: String,
        input: RunInstructionRequest,
    ) -> Result<RunInstructionReceipt> {
        let request = request_id(&input.request_id, &input.text)?;
        if request != input.request_id || input.text.len() > 2000 {
            return Err(StoreError::InvalidInput);
        }
        let user = user.clone();
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let user = authority(&tx,&user,owner)?;
            let run = chat::load_run(&tx,&user.tenant.0,&run_id)?;
            let matches_volume: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM personal_text_previews WHERE tenant_id=?1 AND run_id=?2 AND volume_id=?3 AND generation=?4)",
                params![user.tenant.0,run_id,owner.volume.to_string(),owner.generation as i64], |r|r.get(0))?;
            if run.account != user.account.id || run.thread != thread || !matches_volume {
                return Err(StoreError::NotFound);
            }
            // This is private conversation content. A new, validated member session
            // may store it without acquiring any authority over the old dispatch.
            let receipts = crate::run_instructions::receipts(&tx,&run)?;
            if let Some(previous) = receipts.iter().find(|r|r.request_id == request) {
                if previous.text != input.text || previous.attempt != input.expected_attempt {
                    return Err(StoreError::Conflict);
                }
                return Ok(previous.clone());
            }
            if !run.state.active() || run.attempt != input.expected_attempt || receipts.len() >= 8 {
                return Err(StoreError::Conflict);
            }
            let taken: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND request_id=?2)
                 OR EXISTS(SELECT 1 FROM run_instructions WHERE tenant_id=?1 AND request_id=?2 AND reason LIKE 'text_follow_up_%')",
                params![user.tenant.0,request], |r|r.get(0))?;
            if taken { return Err(StoreError::Conflict); }
            tx.execute("INSERT INTO run_instructions(tenant_id,run_id,request_id,attempt,execution_attempt,sequence,text,state,reason,session_digest,auth_epoch)
                VALUES(?1,?2,?3,?4,?4,?5,?6,'closed','text_follow_up_saved',?7,?8)",
                params![run.tenant,run.id,request,run.attempt,receipts.len() as i64 + 1,input.text,user.digest.as_slice(),user.epoch])?;
            crate::run_instructions::publish(&tx,&run)?;
            let receipt = crate::run_instructions::receipts(&tx,&run)?.pop().ok_or(StoreError::Corrupt)?;
            tx.commit()?;
            Ok(receipt)
        }).await
    }
}
