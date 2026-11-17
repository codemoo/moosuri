use super::*;
use moosuri_core::{RunInstructionRequest, ScheduleChatContext};

pub(super) fn enqueue(
    c: &Connection,
    b: &Binding,
    key: &str,
    body: Option<&str>,
    run: Option<(&str, i32)>,
    notice: Option<&str>,
) -> Result<()> {
    let pending:i64=c.query_row("SELECT count(*) FROM telegram_outbox WHERE binding_id=?1 AND state IN('queued','dispatched')",[&b.id],|r|r.get(0))?;
    if pending >= 128 {
        return Err(StoreError::Busy);
    }
    c.execute("INSERT OR IGNORE INTO telegram_outbox(tenant_id,id,binding_id,dedupe,body,run_id,attempt,notification_id,state,ready_at,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'queued',?9,?9)",params![b.user.tenant.0,Uuid::new_v4().to_string(),b.id,key,body,run.map(|r|r.0),run.map(|r|r.1),notice,now()?])?;
    Ok(())
}
fn reply(c: &Connection, b: &Binding, update: i64, text: &str) -> Result<()> {
    enqueue(c, b, &format!("update:{update}"), Some(text), None, None)
}
fn pair(
    c: &Connection,
    revision: i64,
    code: &str,
    sender: i64,
    chat: i64,
) -> Result<Option<Binding>> {
    if code.len() != 64 || !code.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(None);
    }
    let row:Option<(String,String,Vec<u8>,String,bool)>=c.query_row("SELECT tenant_id,account_id,session_digest,timezone,notifications FROM telegram_pairings WHERE digest=?1 AND bot_revision=?2 AND expires_at>?3",params![digest(code).as_slice(),revision,now()?],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
    let Some((tenant, account, session, timezone, notifications)) = row else {
        return Ok(None);
    };
    let owner = load_context(
        c,
        session.try_into().map_err(|_| StoreError::Corrupt)?,
        false,
    )?;
    if owner.tenant.0 != tenant || owner.account.id != account || owner.restricted {
        return Ok(None);
    }
    let foreign:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM telegram_bindings WHERE (telegram_user_id=?1 OR chat_id=?2) AND account_id!=?3)",params![sender,chat,account],|r|r.get(0))?;
    if foreign {
        return Ok(None);
    }
    unlink(c, &tenant, &account)?;
    // Independent session: web logout does not disconnect Telegram. Epoch changes
    // and end-all-sessions revoke it through the normal Store authority checks.
    let login = load_login(c, &owner.account.email)?.ok_or(StoreError::Unauthorized)?;
    let session = issue_session(c, login)?;
    let _unused_token = Zeroizing::new(session.token);
    let id = Uuid::new_v4().to_string();
    c.execute("INSERT INTO telegram_bindings(tenant_id,account_id,id,telegram_user_id,chat_id,session_digest,bot_revision,timezone,notifications,notification_cursor,created_at) VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,(SELECT coalesce(max(rowid),0) FROM local_notifications),?10)",params![tenant,account,id,sender,chat,session.user.digest.as_slice(),revision,timezone,notifications,now()?])?;
    Ok(Some(binding(c, &id, revision, false)?))
}
fn thread(c: &Connection, b: &Binding) -> Result<String> {
    if let Some(t) = &b.thread
        && c.query_row("SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND mission_internal=0)",params![b.user.tenant.0,b.user.account.id,t],|r|r.get::<_,bool>(0))? {return Ok(t.clone())}
    let id = Uuid::new_v4().to_string();
    c.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) VALUES(?1,?2,?3,'Telegram',?4,?4)",params![b.user.tenant.0,id,b.user.account.id,now()?])?;
    c.execute(
        "UPDATE telegram_bindings SET thread_id=?2 WHERE id=?1",
        params![b.id, id],
    )?;
    Ok(id)
}
fn latest(c: &Connection, b: &Binding, t: &str) -> Result<Option<crate::chat::Run>> {
    let id:Option<String>=c.query_row("SELECT id FROM chat_runs WHERE tenant_id=?1 AND account_id=?2 AND thread_id=?3 ORDER BY sequence DESC LIMIT 1",params![b.user.tenant.0,b.user.account.id,t],|r|r.get(0)).optional()?;
    id.map(|id| crate::chat::load_run(c, &b.user.tenant.0, &id))
        .transpose()
}
fn handle(
    c: &Connection,
    key: &[u8; 32],
    web: &moosuri_core::WebToolProfile,
    revision: i64,
    update: &Update,
) -> Result<bool> {
    let Some(m) = &update.message else {
        return Ok(false);
    };
    let Some(from) = &m.from else {
        return Ok(false);
    };
    if m.chat.kind != "private"
        || from.is_bot
        || from.id <= 0
        || m.chat.id != from.id
        || m.message_id <= 0
        || m.date > now()? + 300
        || m.date < now()? - 86400
    {
        return Ok(false);
    }
    if let Some(code) = m.text.as_deref().and_then(|t| t.strip_prefix("/start ")) {
        if let Some(b) = pair(c, revision, code, from.id, m.chat.id)? {
            reply(
                c,
                &b,
                update.update_id,
                "무수리와 연결했습니다. 메시지를 보내면 기존 기억과 도구로 도와드려요. /new 새 대화 · /stop 중지 · /status 상태 · /help 도움말",
            )?;
        }
        return Ok(false);
    }
    let id:Option<String>=c.query_row("SELECT id FROM telegram_bindings WHERE telegram_user_id=?1 AND chat_id=?2 AND bot_revision=?3",params![from.id,m.chat.id,revision],|r|r.get(0)).optional()?;
    let Some(id) = id else { return Ok(false) };
    let b = binding(c, &id, revision, true)?;
    let Some(text) = m.text.as_deref() else {
        reply(
            c,
            &b,
            update.update_id,
            "지금은 텍스트 대화를 지원합니다. 사진·음성·파일은 무수리 웹에서 보내주세요.",
        )?;
        return Ok(false);
    };
    if text.trim().is_empty() || text.len() > 8192 {
        reply(
            c,
            &b,
            update.update_id,
            "메시지가 너무 깁니다. 나누어 보내주세요.",
        )?;
        return Ok(false);
    }
    match text.trim() {
        "/disconnect" => {
            unlink(c, &b.user.tenant.0, &b.user.account.id)?;
            return Ok(false);
        }
        "/new" => {
            c.execute(
                "UPDATE telegram_bindings SET thread_id=NULL WHERE id=?1",
                [&b.id],
            )?;
            reply(
                c,
                &b,
                update.update_id,
                "새 대화를 시작합니다. 이전 대화는 무수리 웹에 보관됩니다.",
            )?;
            return Ok(false);
        }
        "/start" | "/help" => {
            reply(
                c,
                &b,
                update.update_id,
                "메시지를 보내면 무수리와 대화합니다.\n/new 새 대화\n/stop 현재 응답 중지\n/status 상태와 웹에서 열기\n/disconnect 연결 해제\n사진·음성·파일과 메일 발송 최종 확인은 웹에서 이용하세요.",
            )?;
            return Ok(false);
        }
        _ => (),
    }
    let t = thread(c, &b)?;
    let previous = latest(c, &b, &t)?;
    // A completed chat turn can own a still-running delegated task. Resolve the
    // same carrier used by live guidance; instruction capacity is not cancel authority.
    let mission=previous.as_ref().map(|r|crate::run_instructions::target(c,r)).transpose()?.flatten()
        .map(|target|c.query_row("SELECT id,revision,state FROM missions WHERE tenant_id=?1 AND account_id=?2 AND run_id=?3 AND state IN('queued','planning','executing','verifying','paused','waiting')",params![b.user.tenant.0,b.user.account.id,target.run_id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?))).optional()).transpose()?.flatten();
    if text.trim() == "/status" {
        reply(
            c,
            &b,
            update.update_id,
            if mission
                .as_ref()
                .is_some_and(|m| matches!(m.2.as_str(), "paused" | "waiting"))
            {
                "맡긴 작업이 확인을 기다리고 있습니다. 웹에서 이어가거나 /stop으로 취소할 수 있어요."
            } else if mission.is_some() || previous.as_ref().is_some_and(|r| r.state.active()) {
                "진행 중입니다. 추가 지침을 보내거나 /stop으로 중지할 수 있어요."
            } else {
                "대화할 준비가 됐습니다. 마지막 답변과 작업 상세는 웹에서 확인할 수 있어요."
            },
        )?;
        return Ok(false);
    }
    if text.trim() == "/stop" {
        let stopped = if let Some((id, revision, _)) = mission {
            crate::missions::control_owner_in(
                c,
                key,
                &b.user,
                &id,
                moosuri_core::MissionControl {
                    request_id: Uuid::new_v4().to_string(),
                    expected_revision: revision,
                    action: moosuri_core::MissionAction::Cancel,
                },
            )?;
            true
        } else if let Some(r) = previous.filter(|r| r.state.active()) {
            crate::chat::cancel_run_in(c, &b.user, &r.id, r.attempt)?;
            true
        } else {
            false
        };
        reply(
            c,
            &b,
            update.update_id,
            if stopped {
                "중지를 요청했습니다. 이미 완료된 외부 작업은 되돌리지 않습니다."
            } else {
                "현재 대화에서 중지할 실행이 없습니다."
            },
        )?;
        return Ok(true);
    }
    if text.starts_with('/') {
        reply(
            c,
            &b,
            update.update_id,
            "지원하는 명령은 /help에서 확인하세요.",
        )?;
        return Ok(false);
    }
    let request = Uuid::new_v4().to_string();
    if let Some(r) = &previous {
        let v = crate::chat::view(c, r)?;
        let target = v
            .instruction_target
            .filter(|t| t.supports_instructions)
            .map(|t| (t.run_id, t.attempt))
            .or_else(|| v.supports_instructions.then(|| (r.id.clone(), r.attempt)));
        if let Some((id, attempt)) = target {
            crate::run_instructions::submit_in(
                c,
                &b.user,
                &id,
                RunInstructionRequest {
                    request_id: request,
                    expected_attempt: attempt,
                    text: text.into(),
                },
            )?;
            reply(
                c,
                &b,
                update.update_id,
                "추가 지침을 접수했습니다. 진행 중인 작업에 반영하겠습니다.",
            )?;
            return Ok(true);
        }
        if r.state.active() || mission.is_some() {
            reply(
                c,
                &b,
                update.update_id,
                "지금 응답은 추가 지침을 받을 수 없습니다. 완료 뒤 보내거나 /stop으로 중지해주세요.",
            )?;
            return Ok(false);
        }
    }
    let turn = crate::chat::submit_turn_context_in(
        c,
        key,
        web,
        b.user.clone(),
        t,
        request,
        text.into(),
        None,
        vec![],
        Some(ScheduleChatContext {
            timezone: b.timezone.clone(),
            target: None,
        }),
        None,
    )?;
    c.execute(
        "INSERT INTO telegram_turns(tenant_id,binding_id,run_id,attempt) VALUES(?1,?2,?3,?4)",
        params![b.user.tenant.0, b.id, turn.run.id, turn.run.attempt],
    )?;
    Ok(true)
}
impl Store {
    /// Admission and offset advance share one transaction, including ignored updates.
    pub async fn ingest_telegram(&self, revision: i64, update: Update) -> Result<bool> {
        self.call(move|db|{
            guard_bot(&db.conn,revision)?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let offset:i64=tx.query_row("SELECT offset FROM telegram_bot WHERE singleton=1",[],|r|r.get(0))?;
            if update.update_id<offset{return Ok(false)}
            if update.update_id<0||update.update_id==i64::MAX{return Err(StoreError::InvalidInput)}
            tx.execute_batch("SAVEPOINT telegram_input;")?;
            let wake=match handle(&tx,&db.provider_key,&db.web_profile,revision,&update) {
                Ok(w)=>{tx.execute_batch("RELEASE telegram_input;")?;w},
                Err(StoreError::Busy|StoreError::Unavailable|StoreError::Corrupt)=>return Err(StoreError::Busy),
                Err(_)=>{
                    tx.execute_batch("ROLLBACK TO telegram_input; RELEASE telegram_input;")?;
                    if let Some(m)=&update.message
                        && m.chat.kind=="private"&&m.from.as_ref().is_some_and(|f|!f.is_bot&&f.id==m.chat.id) {
                            let id:Option<String>=tx.query_row("SELECT id FROM telegram_bindings WHERE chat_id=?1 AND bot_revision=?2",params![m.chat.id,revision],|r|r.get(0)).optional()?;
                            if let Some(b)=id.and_then(|id|binding(&tx,&id,revision,false).ok()) {reply(&tx,&b,update.update_id,"요청을 시작하지 못했습니다. 웹에서 모델 연결·계정 상태를 확인한 뒤 다시 보내주세요.")?;}
                    }false
                }
            };
            tx.execute("UPDATE telegram_bot SET offset=?1 WHERE singleton=1",[update.update_id+1])?;
            tx.commit()?;Ok(wake)
        }).await
    }
}
