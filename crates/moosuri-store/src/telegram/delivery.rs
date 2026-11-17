use super::*;
use moosuri_core::{MessagePhase, RunState};
/// No Debug/Serialize: this carrier contains private conversation output.
pub struct Dispatch {
    id: String,
    nonce: String,
    binding: String,
    revision: i64,
    pub chat: i64,
    pub text: String,
}
fn notice_allowed(c: &Connection, b: &Binding, id: &str) -> Result<bool> {
    let enabled: bool = c.query_row(
        "SELECT notifications FROM telegram_bindings WHERE id=?1",
        [&b.id],
        |r| r.get(0),
    )?;
    if !enabled || crate::push::quiet(c, &b.user.tenant.0, &b.user.account.id, now()?)? {
        return Ok(false);
    }
    let suppressed:bool=c.query_row("SELECT source_suppressed OR push_suppressed FROM local_notifications WHERE tenant_id=?1 AND account_id=?2 AND id=?3",params![b.user.tenant.0,b.user.account.id,id],|r|r.get(0)).optional()?.unwrap_or(true);
    if suppressed {
        return Ok(false);
    }
    match crate::schedules::notification(c, &b.user.tenant.0, &b.user.account.id, id) {
        Ok(n) => Ok(!n.read
            && !n.dismissed
            && !n.quiet
            && n.delivered_at + 3600 > now()?
            && n.source.is_none_or(|s| !s.muted)),
        Err(StoreError::NotFound) => Ok(false),
        Err(e) => Err(e),
    }
}
fn run_text(c: &Connection, b: &Binding, id: &str, attempt: i32) -> Result<String> {
    let r = crate::chat::load_run(c, &b.user.tenant.0, id)?;
    if r.account != b.user.account.id || r.attempt != attempt || r.state.active() {
        return Err(StoreError::Conflict);
    }
    let t = crate::chat::turn(c, &r)?;
    let mut text = t
        .assistant
        .iter()
        .filter(|m| m.phase != Some(MessagePhase::Commentary))
        .map(|m| m.text.as_str())
        .collect::<Vec<_>>()
        .join("\n\n");
    if text.trim().is_empty() {
        text=match r.state {
        RunState::Completed=>"처리를 마쳤습니다. 작업 상세는 웹에서 확인해주세요.",
        RunState::Cancelled=>"응답을 중지했습니다.",
        RunState::OutcomeUnknown=>"처리 결과를 확인할 수 없습니다. 웹에서 실행 기록을 확인해주세요. 같은 작업을 자동으로 다시 실행하지 않습니다.",
        _=>"응답을 마치지 못했습니다. 웹에서 오류와 재시도 가능 여부를 확인해주세요.",
    }.into();
    }
    if t.run.connection_wait.is_some() {
        text.push_str("\n\n추가 연결이나 확인이 필요합니다. 무수리 웹에서 이어주세요.");
    }
    if text.len() > 128 * 1024 {
        return Err(StoreError::InvalidInput);
    }
    Ok(text)
}
impl Store {
    /// Scan durable run results and opted-in inbox events. No model calls.
    pub async fn poll_telegram_delivery(&self, revision: i64) -> Result<Vec<i64>> {
        self.call(move|db|{
            guard_bot(&db.conn,revision)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            tx.execute("UPDATE telegram_bindings SET last_delivery='unknown' WHERE id IN(SELECT binding_id FROM telegram_outbox WHERE state='dispatched' AND dispatched_at<?1)",[now()?-40])?;
            tx.execute("UPDATE telegram_outbox SET state='unknown',body=NULL WHERE state='dispatched' AND dispatched_at<?1",[now()?-40])?;
            tx.execute("DELETE FROM telegram_outbox WHERE state IN('accepted','unknown','rejected','cancelled') AND created_at<?1",[now()?-86400*7])?;
            let ids=tx.prepare("SELECT id FROM telegram_bindings WHERE bot_revision=?1")?.query_map([revision],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
            let mut typing=vec![];
            for id in ids {
                let b=match binding(&tx,&id,revision,false){Ok(b)=>b,Err(StoreError::Unauthorized|StoreError::Forbidden)=>continue,Err(e)=>return Err(e)};
                let rows=tx.prepare("SELECT t.run_id,t.attempt,r.state FROM telegram_turns t JOIN chat_runs r ON r.tenant_id=t.tenant_id AND r.id=t.run_id WHERE t.binding_id=?1 AND t.settled=0 ORDER BY r.created_at LIMIT 16")?.query_map([&id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i32>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
                for(run,attempt,state)in rows {
                    if matches!(state.as_str(),"queued"|"running") {if !typing.contains(&b.chat) && typing.len()<16 {typing.push(b.chat);}continue;}
                    incoming::enqueue(&tx,&b,&format!("run:{run}:{attempt}"),None,Some((&run,attempt)),None)?;
                    tx.execute("UPDATE telegram_turns SET settled=1 WHERE tenant_id=?1 AND run_id=?2 AND attempt=?3",params![b.user.tenant.0,run,attempt])?;
                }
                let (enabled,cursor):(bool,i64)=tx.query_row("SELECT notifications,notification_cursor FROM telegram_bindings WHERE id=?1",[&id],|r|Ok((r.get(0)?,r.get(1)?)))?;
                let notices=tx.prepare("SELECT rowid,id FROM local_notifications WHERE tenant_id=?1 AND account_id=?2 AND rowid>?3 ORDER BY rowid LIMIT 32")?.query_map(params![b.user.tenant.0,b.user.account.id,cursor],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
                let mut after=cursor;
                for(row,n)in notices {if enabled&&notice_allowed(&tx,&b,&n)? {incoming::enqueue(&tx,&b,&format!("notice:{n}"),None,None,Some(&n))?;}after=row;}
                tx.execute("UPDATE telegram_bindings SET notification_cursor=?2 WHERE id=?1",params![id,after])?;
            }
            tx.commit()?;Ok(typing)
        }).await
    }
    /// Final authorization and dispatch commitment are atomic. After this point a
    /// network failure is unknown; only an explicit 429 permits retransmission.
    pub async fn claim_telegram_dispatch(
        &self,
        revision: i64,
        origin: Option<String>,
    ) -> Result<Option<Dispatch>> {
        self.call(move|db|{
            guard_bot(&db.conn,revision)?;let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            // One send per second across this bot also obeys per-chat flood guidance.
            let recent:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM telegram_outbox WHERE dispatched_at>=?1)",[now()?],|r|r.get(0))?;
            if recent{return Ok(None)}
            let row=tx.query_row("SELECT id,binding_id,body,run_id,attempt,notification_id,position,body_digest FROM telegram_outbox q WHERE state='queued' AND ready_at<=?1 AND NOT EXISTS(SELECT 1 FROM telegram_outbox p WHERE p.binding_id=q.binding_id AND p.state IN('queued','dispatched') AND p.rowid<q.rowid) ORDER BY rowid LIMIT 1",[now()?],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?,r.get::<_,Option<String>>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,Option<i32>>(4)?,r.get::<_,Option<String>>(5)?,r.get::<_,i64>(6)?,r.get::<_,Option<Vec<u8>>>(7)?))).optional()?;
            let Some((id,link,body,run,attempt,notice,position,prior_digest))=row else{return Ok(None)};
            let position=usize::try_from(position).map_err(|_|StoreError::Corrupt)?;
            let prepare=||->Result<(Binding,String)>{
                let b=binding(&tx,&link,revision,false)?;
                let mut text=if let Some(run)=&run {run_text(&tx,&b,run,attempt.ok_or(StoreError::Corrupt)?)?}
                else if let Some(n)=&notice {if !notice_allowed(&tx,&b,n)?{return Err(StoreError::Conflict)}"무수리에 새 소식이 있습니다. 웹 소식함에서 확인해주세요.".into()}
                else{body.ok_or(StoreError::Corrupt)?};
                if let Some(origin)=&origin {
                    let path=if notice.is_some(){"/app/schedules".into()}else if let Some(run)=&run{
                        let r=crate::chat::load_run(&tx,&b.user.tenant.0,run)?;format!("/app?thread={}",r.thread)
                    }else{b.thread.as_ref().map(|t|format!("/app?thread={t}")).unwrap_or("/app".into())};
                    text.push_str(&format!("\n\n{origin}{path}"));
                }
                Ok((b,text))
            };
            let (b,text)=match prepare(){Ok(v)=>v,Err(StoreError::Busy|StoreError::Unavailable|StoreError::Corrupt)=>return Err(StoreError::Unavailable),Err(_)=>{tx.execute("UPDATE telegram_outbox SET state='cancelled',body=NULL WHERE id=?1",[id])?;tx.commit()?;return Ok(None)}};
            let hash=digest(&text).to_vec();
            if prior_digest.is_some_and(|h|h!=hash){tx.execute("UPDATE telegram_outbox SET state='cancelled',body=NULL WHERE id=?1",[id])?;tx.commit()?;return Ok(None)}
            if position==text.len(){tx.execute("UPDATE telegram_outbox SET state='accepted',body=NULL WHERE id=?1",[id])?;tx.commit()?;return Ok(None)}
            let Some(part)=moosuri_connectors::telegram::chunk(&text,position) else{return Err(StoreError::Corrupt)};
            let nonce=Uuid::new_v4().to_string();
            tx.execute("UPDATE telegram_outbox SET state='dispatched',nonce=?2,body_digest=?3,part_bytes=?4,attempts=attempts+1,dispatched_at=?5 WHERE id=?1",params![id,nonce,hash,part.len() as i64,now()?])?;
            let result=Dispatch{id,nonce,binding:link,revision,chat:b.chat,text:part.into()};tx.commit()?;Ok(Some(result))
        }).await
    }
    pub async fn finish_telegram_dispatch(
        &self,
        d: &Dispatch,
        outcome: std::result::Result<(), TransportError>,
    ) -> Result<()> {
        let (id, nonce, link, revision) =
            (d.id.clone(), d.nonce.clone(), d.binding.clone(), d.revision);
        self.call(move|db|{
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let row:Option<(i64,i64,i64)>=tx.query_row("SELECT position,part_bytes,attempts FROM telegram_outbox WHERE id=?1 AND nonce=?2 AND state='dispatched'",params![id,nonce],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?))).optional()?;
            let Some((position,bytes,attempts))=row else{return Ok(())};
            // A full chunk may be the final one. A cheap later claim resolves the
            // exact final byte offset without repeating the previous network call.
            let (state,next,wait)=match outcome {
                Ok(())=>("queued",position+bytes,0),
                Err(TransportError::Retry(s)) if attempts<5=>("queued",position,i64::from(s)),
                Err(TransportError::Rejected|TransportError::Unauthorized|TransportError::Conflict|TransportError::Invalid|TransportError::Retry(_))=>("rejected",position,0),
                Err(TransportError::Unknown)=>("unknown",position,0),
            };
            tx.execute("UPDATE telegram_outbox SET state=?3,position=?4,ready_at=?5,attempts=CASE WHEN ?6 THEN 0 ELSE attempts END WHERE id=?1 AND nonce=?2",params![id,nonce,state,next,now()?+wait,outcome.is_ok()])?;
            tx.execute("UPDATE telegram_bindings SET last_delivery=?2 WHERE id=?1",params![link,if outcome.is_ok(){"accepted"}else{state}])?;
            if matches!(outcome,Err(TransportError::Unauthorized|TransportError::Conflict)){tx.execute("UPDATE telegram_bot SET status=?2 WHERE revision=?1",params![revision,if outcome==Err(TransportError::Unauthorized){"unauthorized"}else{"conflict"}])?;}
            tx.commit()?;Ok(())
        }).await
    }
}
