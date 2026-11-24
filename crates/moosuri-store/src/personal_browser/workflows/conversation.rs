//! Durable conversation work. One request is a completed chat receipt plus up to six independent
//! children. Children are read only by the authorized timer (`dispatch`), through the same bounded
//! member workflow execution as every other research run. Status reads never search or dispatch.
use super::*;
use crate::personal_browser::reports;
use moosuri_core::personal_browser::{
    ConversationWorkAction, ConversationWorkGroup, ConversationWorkPage, ConversationWorkRequest,
    TaskDetail, TaskEvent, TaskSummary,
};
use serde_json::{Value, json};

const MAX_ITEMS: usize = 6;
const MAX_FILTERS: usize = 4;
const MAX_PENDING: i64 = 24;
const PAGE_GROUPS: i64 = 40;
/// Children read per timer tick, and the most one group may take while others wait.
const DISPATCH_BATCH: i64 = 4;
const DISPATCH_PER_GROUP: i64 = 2;
const STALE_BATCH: i64 = 16;
const MODEL_ID: &str = "member-conversation-work";

struct Own {
    tenant: String,
    account: String,
    volume: String,
    generation: i64,
}
impl Own {
    fn of(owner: VolumeOwner) -> Self {
        Self {
            tenant: owner.tenant.to_string(),
            account: owner.account.to_string(),
            volume: owner.volume.to_string(),
            generation: owner.generation as i64,
        }
    }
}

struct Item {
    title: String,
    scope: &'static str,
    query: String,
    filters: Vec<String>,
}
struct Draft {
    request_id: String,
    message: String,
    items: Vec<Item>,
}
struct Task {
    id: String,
    group: String,
    title: String,
    scope: String,
    query: String,
    filters: Vec<String>,
    state: String,
    revision: i64,
    question: Option<String>,
}
const TASK: &str = "SELECT t.id,t.group_id,t.title,t.scope,t.query,t.filters,t.state,t.revision,t.question_id FROM member_conversation_tasks t";

fn task_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Task> {
    let raw: String = r.get(5)?;
    let filters = serde_json::from_str(&raw).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, Box::new(e))
    })?;
    Ok(Task {
        id: r.get(0)?,
        group: r.get(1)?,
        title: r.get(2)?,
        scope: r.get(3)?,
        query: r.get(4)?,
        filters,
        state: r.get(6)?,
        revision: r.get(7)?,
        question: r.get(8)?,
    })
}
fn scope_of(value: &str) -> Result<WorkScope> {
    match value {
        "memory" => Ok(WorkScope::Memory),
        "conversation" => Ok(WorkScope::Conversation),
        _ => Err(StoreError::Corrupt),
    }
}
fn encode<T: serde::Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(|_| StoreError::Corrupt)
}
fn finish<T: serde::de::DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(|_| StoreError::Corrupt)
}
fn text_of<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(StoreError::InvalidInput)
}
/// A member message may span lines; every other control character is rejected.
fn message_ok(text: &str) -> bool {
    !text.trim().is_empty()
        && text.len() <= 8192
        && !text
            .chars()
            .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
}
fn literal(text: &str) -> Result<String> {
    let text = text.trim();
    if text_valid(text, 128) {
        Ok(text.to_owned())
    } else {
        Err(StoreError::InvalidInput)
    }
}

fn parse_request(request: &ConversationWorkRequest) -> Result<Draft> {
    canonical(&request.request_id)?;
    if !message_ok(&request.message) || request.items.is_empty() || request.items.len() > MAX_ITEMS
    {
        return Err(StoreError::InvalidInput);
    }
    let mut items = Vec::with_capacity(request.items.len());
    for input in &request.items {
        let title = input.title.trim();
        let query = input.query.trim();
        if !text_valid(title, 240)
            || (!query.is_empty() && !text_valid(query, 128))
            || input.filters.len() > MAX_FILTERS
        {
            return Err(StoreError::InvalidInput);
        }
        let filters = input
            .filters
            .iter()
            .map(|term| literal(term))
            .collect::<Result<Vec<_>>>()?;
        items.push(Item {
            title: title.to_owned(),
            query: query.to_owned(),
            filters,
            scope: match input.scope {
                WorkScope::Memory => "memory",
                WorkScope::Conversation => "conversation",
            },
        });
    }
    Ok(Draft {
        request_id: request.request_id.clone(),
        message: request.message.clone(),
        items,
    })
}
fn payload_of(draft: &Draft) -> String {
    json!({
        "message": draft.message,
        "items": draft.items.iter().map(|i| json!({"title": i.title, "scope": i.scope, "query": i.query, "filters": i.filters})).collect::<Vec<_>>(),
    })
    .to_string()
}

fn thread_owned(c: &Connection, user: &UserContext, thread: &str) -> Result<()> {
    canonical(thread)?;
    let found: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND mission_internal=0)",
        params![user.tenant.0, thread, user.account.id],
        |r| r.get(0),
    )?;
    if found {
        Ok(())
    } else {
        Err(StoreError::NotFound)
    }
}

/// The stored payload and origin of a group id; the same id under another owner or thread conflicts.
fn existing(
    c: &Connection,
    owner: VolumeOwner,
    thread: &str,
    id: &str,
) -> Result<Option<(String, Option<String>)>> {
    let own = Own::of(owner);
    let row: Option<(bool, String, Option<String>)> = c
        .query_row(
            "SELECT tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6,payload,origin_task
             FROM member_conversation_groups WHERE id=?1",
            params![id, own.tenant, own.account, own.volume, own.generation, thread],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    match row {
        None => Ok(None),
        Some((false, ..)) => Err(StoreError::Conflict),
        Some((true, payload, origin)) => Ok(Some((payload, origin))),
    }
}

fn event(c: &Connection, task: &str, revision: i64, state: &str, text: &str) -> Result<()> {
    c.execute(
        "INSERT INTO member_conversation_events(task_id,revision,state,text,at) VALUES(?1,?2,?3,?4,?5)",
        params![task, revision, state, text, now()?],
    )?;
    Ok(())
}

/// The receipt is an ordinary completed turn. It is not a text preview, so no source search sees it,
/// and it never holds the thread's single active-run slot.
#[allow(clippy::too_many_arguments)]
fn receipt(
    c: &Connection,
    user: &UserContext,
    thread: &str,
    turn: &str,
    request: &str,
    message: &str,
    count: usize,
    needs: usize,
) -> Result<()> {
    let mut model = observation_model();
    model.provider_id = MODEL_ID.into();
    model.provider_name = "Member workspace".into();
    model.model = "conversation-work".into();
    let sequence: i64 = c.query_row(
        "UPDATE threads SET chat_turn_seq=chat_turn_seq+1 WHERE tenant_id=?1 AND id=?2 AND account_id=?3 RETURNING chat_turn_seq",
        params![user.tenant.0, thread, user.account.id],
        |r| r.get(0),
    )?;
    c.execute(
        "INSERT INTO chat_runs(tenant_id,id,account_id,thread_id,request_id,payload_digest,sequence,user_text,model,session_digest,auth_epoch,attempt,state,version,created_at,admitted_order)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,'completed',1,?12,?13)",
        params![
            user.tenant.0,
            turn,
            user.account.id,
            thread,
            request,
            Sha256::digest(message.as_bytes()).as_slice(),
            sequence,
            message,
            serde_json::to_string(&model).map_err(|_| StoreError::Corrupt)?,
            user.digest.as_slice(),
            user.epoch,
            now()?,
            chat::next_order(c)?
        ],
    )?;
    let loaded = chat::load_run(c, &user.tenant.0, turn)?;
    let note = if needs > 0 {
        format!(" 검색어가 필요한 {needs}건은 답을 기다립니다.")
    } else {
        String::new()
    };
    chat::apply_patches(
        c,
        &loaded,
        vec![chat::OutputPatch::Append {
            item: "member-conversation-work-result".into(),
            text: format!(
                "조사 {count}건을 접수했습니다.{note} 업무 기록에서 진행 상황과 결과를 확인할 수 있습니다."
            ),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    chat::emit(
        c,
        &loaded,
        ChatChange::Turn {
            turn: chat::turn(c, &loaded)?,
        },
    )?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn new_group(
    tx: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    thread: &str,
    id: &str,
    message: &str,
    payload: &str,
    items: &[Item],
    origin: Option<&str>,
) -> Result<()> {
    let own = Own::of(owner);
    let pending: i64 = tx.query_row(
        "SELECT count(*) FROM member_conversation_tasks WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4 AND state IN ('queued','needs_input')",
        params![own.tenant, own.account, own.volume, own.generation],
        |r| r.get(0),
    )?;
    if pending + items.len() as i64 > MAX_PENDING {
        return Err(StoreError::BudgetExhausted);
    }
    // The request id is also the chat request id; a different chat turn must not already own it.
    if tx.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND request_id=?2)",
        params![user.tenant.0, id],
        |r| r.get::<_, bool>(0),
    )? {
        return Err(StoreError::Conflict);
    }
    let turn = Uuid::new_v4().to_string();
    let needs = items.iter().filter(|i| i.query.is_empty()).count();
    receipt(tx, user, thread, &turn, id, message, items.len(), needs)?;
    let time = now()?;
    tx.execute(
        "INSERT INTO member_conversation_groups(id,turn_id,tenant_id,account_id,volume_id,generation,thread_id,host_epoch,auth_epoch,message,payload,origin_task,created_at,admitted_order)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        params![
            id,
            turn,
            own.tenant,
            own.account,
            own.volume,
            own.generation,
            thread,
            host_epoch,
            user.epoch,
            message,
            payload,
            origin,
            time,
            chat::next_order(tx)?
        ],
    )?;
    for (position, item) in items.iter().enumerate() {
        let task = Uuid::new_v4().to_string();
        let asking = item.query.is_empty();
        let (state, question, text) = if asking {
            (
                "needs_input",
                Some(Uuid::new_v4().to_string()),
                "검색어가 필요합니다. 찾을 말을 알려 주세요.",
            )
        } else {
            ("queued", None, "조회 대기열에 접수했습니다.")
        };
        tx.execute(
            "INSERT INTO member_workflows(id,tenant_id,account_id,volume_id,generation,auth_epoch,host_epoch,thread_id,title,scope,query,period,enabled)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,0,0)",
            params![
                task,
                own.tenant,
                own.account,
                own.volume,
                own.generation,
                user.epoch,
                host_epoch,
                thread,
                item.title,
                item.scope,
                item.query
            ],
        )?;
        tx.execute(
            "INSERT INTO member_conversation_tasks(id,group_id,tenant_id,account_id,volume_id,generation,thread_id,position,title,scope,query,state,question_id,host_epoch,auth_epoch,queued_seq,task_order,created_at,filters)
             VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,?19)",
            params![
                task,
                id,
                own.tenant,
                own.account,
                own.volume,
                own.generation,
                thread,
                position as i64,
                item.title,
                item.scope,
                item.query,
                state,
                question,
                host_epoch,
                user.epoch,
                chat::next_order(tx)?,
                chat::next_order(tx)?,
                time, serde_json::to_string(&item.filters).map_err(|_| StoreError::InvalidInput)?
            ],
        )?;
        event(tx, &task, 1, state, text)?;
    }
    Ok(())
}

// ---------------------------------------------------------------- projection

fn latest_run(c: &Connection, task: &str) -> Result<Option<String>> {
    Ok(c.query_row(
        "SELECT c.run_id FROM member_conversation_runs c JOIN member_work_runs r ON r.id=c.run_id
         WHERE c.task_id=?1 ORDER BY r.admitted_order DESC LIMIT 1",
        [task],
        |r| r.get(0),
    )
    .optional()?)
}

/// Source text is read again under current authority; the count stays as it was when searched.
fn summarize(c: &Connection, owner: VolumeOwner, run: &str) -> Result<(i64, String, bool)> {
    let evidence = run_evidence(c, owner, run)?;
    let mut shown = Vec::new();
    for source in &evidence.sources {
        if shown.len() == 3 {
            break;
        }
        if let Some((title, body)) = read_source(c, owner, &evidence.scope, source)? {
            shown.push(format!(
                "• {title} — {}",
                excerpt(&body, &evidence.query, 110)
            ));
        }
    }
    let count = evidence.sources.len();
    let mut text = format!(
        "{} {count}건 확인{}",
        label(&evidence.scope),
        if evidence.limited {
            " · 조회 한도 안에서 확인한 결과입니다."
        } else {
            ""
        }
    );
    if !shown.is_empty() {
        text.push('\n');
        text.push_str(&shown.join("\n"));
    } else if count > 0 {
        text.push_str("\n원본이 변경되었거나 더 이상 볼 수 없습니다.");
    }
    Ok((count as i64, text, !shown.is_empty()))
}

fn run_ids(c: &Connection, task: &str) -> Result<Vec<String>> {
    Ok(c.prepare(
        "SELECT c.run_id FROM member_conversation_runs c JOIN member_work_runs r ON r.id=c.run_id
             WHERE c.task_id=?1 ORDER BY r.admitted_order",
    )?
    .query_map([task], |r| r.get::<_, String>(0))?
    .collect::<rusqlite::Result<Vec<_>>>()?)
}

fn task_json(c: &Connection, owner: VolumeOwner, thread: &str, t: &Task) -> Result<Value> {
    let runs = run_ids(c, &t.id)?;
    let events = c
        .prepare("SELECT state,text,at FROM member_conversation_events WHERE task_id=?1 ORDER BY id")?
        .query_map([&t.id], |r| {
            Ok(json!({"state": r.get::<_, String>(0)?, "text": r.get::<_, String>(1)?, "at": r.get::<_, i64>(2)?}))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let latest = runs.last().cloned();
    let mut can_reuse = false;
    let mut source_count = 0;
    let mut summary = None;
    let mut report = None;
    if let Some(run) = &latest {
        let (count, text, readable) = summarize(c, owner, run)?;
        source_count = count;
        if t.state == "completed" {
            can_reuse = readable;
            summary = Some(text);
            report = reports::report_id(c, owner, thread, &t.id, Some(run.as_str()))?;
        }
    }
    Ok(json!({
        "id": t.id, "title": t.title, "scope": t.scope, "query": t.query,
        "filter": if t.filters.is_empty() { Value::Null } else { Value::String(t.filters.join(" AND ")) },
        "state": t.state, "revision": t.revision, "question_id": t.question,
        "run_id": latest, "runs": runs, "events": events,
        "source_count": source_count, "can_reuse": can_reuse, "summary": summary, "report_id": report,
    }))
}

fn group_json(c: &Connection, owner: VolumeOwner, thread: &str, id: &str) -> Result<Value> {
    let own = Own::of(owner);
    let (turn, message): (String, String) = c
        .query_row(
            "SELECT turn_id,message FROM member_conversation_groups
             WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6",
            params![id, own.tenant, own.account, own.volume, own.generation, thread],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    let tasks = c
        .prepare(&format!("{TASK} WHERE t.group_id=?1 ORDER BY t.position"))?
        .query_map([id], task_row)?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let mut rendered = Vec::new();
    for task in &tasks {
        rendered.push(task_json(c, owner, thread, task)?);
    }
    Ok(
        json!({"id": id, "thread_id": thread, "turn_id": turn, "message": message, "tasks": rendered}),
    )
}

fn page_json(
    c: &Connection,
    owner: VolumeOwner,
    thread: &str,
    request: Option<Uuid>,
) -> Result<Value> {
    let own = Own::of(owner);
    let ids: Vec<String> = match request {
        Some(id) => {
            let id = id.to_string();
            let found: bool = c.query_row(
                "SELECT EXISTS(SELECT 1 FROM member_conversation_groups WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6)",
                params![id, own.tenant, own.account, own.volume, own.generation, thread],
                |r| r.get(0),
            )?;
            if !found {
                return Err(StoreError::NotFound);
            }
            vec![id]
        }
        None => {
            let mut recent = c
                .prepare(
                    "SELECT id FROM member_conversation_groups
                     WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4 AND thread_id=?5
                     ORDER BY admitted_order DESC LIMIT ?6",
                )?
                .query_map(
                    params![own.tenant, own.account, own.volume, own.generation, thread, PAGE_GROUPS],
                    |r| r.get::<_, String>(0),
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            recent.reverse();
            recent
        }
    };
    let mut groups = Vec::new();
    for id in ids {
        groups.push(group_json(c, owner, thread, &id)?);
    }
    Ok(json!({"thread_id": thread, "groups": groups}))
}

// ------------------------------------------------------------------- actions

fn load_task(c: &Connection, owner: VolumeOwner, thread: &str, id: &str) -> Result<Task> {
    let own = Own::of(owner);
    c.query_row(
        &format!(
            "{TASK} JOIN member_conversation_groups g ON g.id=t.group_id
             WHERE t.id=?1 AND g.tenant_id=?2 AND g.account_id=?3 AND g.volume_id=?4 AND g.generation=?5 AND g.thread_id=?6"
        ),
        params![id, own.tenant, own.account, own.volume, own.generation, thread],
        task_row,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

fn requeue(
    c: &Connection,
    t: &Task,
    query: Option<&str>,
    filters: &[String],
    host_epoch: i64,
    auth_epoch: i64,
    text: &str,
) -> Result<()> {
    let encoded = serde_json::to_string(filters).map_err(|_| StoreError::Corrupt)?;
    let changed = c.execute(
        "UPDATE member_conversation_tasks SET query=coalesce(?3,query),filters=?4,state='queued',question_id=NULL,
            revision=revision+1,queued_seq=?5,host_epoch=?6,auth_epoch=?7 WHERE id=?1 AND revision=?2",
        params![t.id, t.revision, query, encoded, chat::next_order(c)?, host_epoch, auth_epoch],
    )?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    if let Some(query) = query {
        c.execute(
            "UPDATE member_workflows SET query=?2 WHERE id=?1",
            params![t.id, query],
        )?;
    }
    event(c, &t.id, t.revision + 1, "queued", text)
}
fn settle(c: &Connection, t: &Task, state: &str, text: &str) -> Result<()> {
    let changed = c.execute(
        "UPDATE member_conversation_tasks SET state=?3,question_id=NULL,revision=revision+1 WHERE id=?1 AND revision=?2",
        params![t.id, t.revision, state],
    )?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    event(c, &t.id, t.revision + 1, state, text)
}

/// A completed search is reusable only when one of its sources can still be read by this member.
fn proven(c: &Connection, owner: VolumeOwner, task: &Task) -> Result<bool> {
    let Some(run) = latest_run(c, &task.id)? else {
        return Ok(false);
    };
    let evidence = run_evidence(c, owner, &run)?;
    for source in &evidence.sources {
        if read_source(c, owner, &evidence.scope, source)?.is_some() {
            return Ok(true);
        }
    }
    Ok(false)
}

#[allow(clippy::too_many_arguments)]
fn reuse(
    tx: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    thread: &str,
    request: &str,
    source: &str,
    revision: i64,
    text: Option<&str>,
) -> Result<()> {
    let query = literal(text.ok_or(StoreError::InvalidInput)?)?;
    let payload = json!({"reuse": source, "query": query}).to_string();
    if let Some((stored, origin)) = existing(tx, owner, thread, request)? {
        return if stored == payload && origin.as_deref() == Some(source) {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        };
    }
    let task = load_task(tx, owner, thread, source)?;
    if task.state != "completed" || task.revision != revision || !proven(tx, owner, &task)? {
        return Err(StoreError::Conflict);
    }
    let scope = match scope_of(&task.scope)? {
        WorkScope::Memory => "memory",
        WorkScope::Conversation => "conversation",
    };
    let message = format!("‘{}’ 조사를 이어서 ‘{query}’ 다시 찾기", task.title);
    let item = Item {
        title: format!("{} 조사", query),
        scope,
        query,
        filters: Vec::new(),
    };
    new_group(
        tx,
        owner,
        user,
        host_epoch,
        thread,
        request,
        &message,
        &payload,
        &[item],
        Some(source),
    )
}

/// Applies one member action; returns the id of the group to project.
fn control(
    tx: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    thread: &str,
    action: &ConversationWorkAction,
) -> Result<String> {
    let value = encode(action)?;
    let request = text_of(&value, "request_id")?;
    let task_id = text_of(&value, "task_id")?;
    let kind = text_of(&value, "action")?;
    canonical(request)?;
    canonical(task_id)?;
    let revision = value
        .get("revision")
        .and_then(Value::as_i64)
        .filter(|r| *r > 0)
        .ok_or(StoreError::InvalidInput)?;
    let text = value.get("text").and_then(Value::as_str);
    let question = value.get("question_id").and_then(Value::as_str);
    if kind == "reuse" {
        reuse(
            tx, owner, user, host_epoch, thread, request, task_id, revision, text,
        )?;
        return Ok(request.to_owned());
    }
    let task = load_task(tx, owner, thread, task_id)?;
    if task.group != request {
        return Err(StoreError::NotFound);
    }
    let argument = match kind {
        "refine" => literal(text.ok_or(StoreError::InvalidInput)?)?,
        "answer" => {
            let question = question.ok_or(StoreError::InvalidInput)?;
            canonical(question)?;
            format!(
                "{question}\n{}",
                literal(text.ok_or(StoreError::InvalidInput)?)?
            )
        }
        "cancel" | "save_report" => String::new(),
        _ => return Err(StoreError::InvalidInput),
    };
    // An exact replay of an applied action succeeds; the same revision used differently is stale.
    let applied: Option<(String, String)> = tx
        .query_row(
            "SELECT action,arg FROM member_conversation_actions WHERE task_id=?1 AND from_revision=?2",
            params![task.id, revision],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((done, arg)) = applied {
        return if done == kind && arg == argument {
            Ok(task.group)
        } else {
            Err(StoreError::Conflict)
        };
    }
    if task.revision != revision {
        return Err(StoreError::Conflict);
    }
    match kind {
        "refine" => {
            if !matches!(task.state.as_str(), "queued" | "completed") {
                return Err(StoreError::Conflict);
            }
            if task.filters.len() >= MAX_FILTERS {
                return Err(StoreError::BudgetExhausted);
            }
            if task
                .filters
                .iter()
                .any(|f| f.eq_ignore_ascii_case(&argument))
            {
                return Err(StoreError::InvalidInput);
            }
            let mut filters = task.filters.clone();
            filters.push(argument.clone());
            requeue(
                tx,
                &task,
                None,
                &filters,
                host_epoch,
                user.epoch,
                &format!("추가 조건 ‘{argument}’을(를) 모두 포함하도록 다시 조회합니다."),
            )?;
        }
        "answer" => {
            if task.state != "needs_input" || task.question.as_deref() != question {
                return Err(StoreError::Conflict);
            }
            let search = argument.split_once('\n').map_or("", |(_, t)| t).to_owned();
            requeue(
                tx,
                &task,
                Some(&search),
                &task.filters,
                host_epoch,
                user.epoch,
                &format!("검색어 ‘{search}’을(를) 받아 조회 대기열에 접수했습니다."),
            )?;
        }
        "cancel" => {
            if !matches!(task.state.as_str(), "queued" | "needs_input") {
                return Err(StoreError::Conflict);
            }
            settle(tx, &task, "cancelled", "사용자가 취소했습니다.")?;
        }
        _ => {
            // save_report: the Markdown report is bound to the child's current completed run.
            let run = latest_run(tx, &task.id)?.ok_or(StoreError::Conflict)?;
            if task.state != "completed" {
                return Err(StoreError::Conflict);
            }
            reports::save_report(tx, owner, thread, &task.id, &run)?;
            settle(tx, &task, "completed", "보고서를 저장했습니다.")?;
        }
    }
    tx.execute(
        "INSERT INTO member_conversation_actions(task_id,from_revision,action,arg) VALUES(?1,?2,?3,?4)",
        params![task.id, revision, kind, argument],
    )?;
    Ok(task.group)
}

// ------------------------------------------------------------------ the timer

fn fail(c: &Connection, t: &Task, text: &str) -> Result<()> {
    settle(c, t, "failed", text)
}

fn run_one(tx: &Connection, owner: VolumeOwner, t: &Task) -> Result<()> {
    let scope = scope_of(&t.scope)?;
    let schedule = WorkSchedule {
        id: t.id.clone(),
        title: t.title.clone(),
        scope,
        query: t.query.clone(),
        period: 0,
        next_due: None,
        enabled: false,
        revision: t.revision,
    };
    let request = format!("{}:{}", t.id, t.revision);
    let run = execute(tx, owner, &schedule, Some(&request), None, &t.filters)?;
    let (count, limited): (i64, bool) = tx.query_row(
        "SELECT json_array_length(sources),limited FROM member_work_runs WHERE id=?1",
        [&run],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let filters = serde_json::to_string(&t.filters).map_err(|_| StoreError::Corrupt)?;
    tx.execute(
        "INSERT OR IGNORE INTO member_conversation_runs(run_id,task_id,revision,filters) VALUES(?1,?2,?3,?4)",
        params![run, t.id, t.revision, filters],
    )?;
    settle(
        tx,
        t,
        "completed",
        &format!(
            "‘{}’ {} {count}건을 확인했습니다.{}",
            terms(&t.query, &t.filters),
            label(&schedule.scope),
            if limited {
                " 조회 한도 안에서 확인한 결과입니다."
            } else {
                ""
            }
        ),
    )
}

const AUTHORIZED: &str = "EXISTS(SELECT 1 FROM accounts a WHERE a.id=t.account_id AND a.tenant_id=t.tenant_id
    AND a.auth_epoch=t.auth_epoch AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)";

/// Reads a bounded, fair batch of queued children. Work granted under an older host epoch or an
/// auth epoch that has since changed is closed with an honest event instead of running late.
pub(super) fn dispatch(tx: &Connection, owner: VolumeOwner, host_epoch: i64) -> Result<usize> {
    let own = Own::of(owner);
    let stale = tx
        .prepare(&format!(
            "{TASK} WHERE t.tenant_id=?1 AND t.account_id=?2 AND t.volume_id=?3 AND t.generation=?4 AND t.state='queued'
             AND (t.host_epoch<?5 OR NOT {AUTHORIZED}) ORDER BY t.queued_seq LIMIT ?6"
        ))?
        .query_map(
            params![own.tenant, own.account, own.volume, own.generation, host_epoch, STALE_BATCH],
            task_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in &stale {
        fail(
            tx,
            task,
            "권한이 바뀌어 조회하지 않았습니다. 새 요청으로 다시 시작해 주세요.",
        )?;
    }
    // Oldest first, but one group never takes more than its share of a batch.
    let due = tx
        .prepare(&format!(
            "{TASK} WHERE t.id IN (SELECT id FROM (
                SELECT t.id,t.queued_seq,row_number() OVER (PARTITION BY t.group_id ORDER BY t.queued_seq) AS turn
                FROM member_conversation_tasks t
                WHERE t.tenant_id=?1 AND t.account_id=?2 AND t.volume_id=?3 AND t.generation=?4
                  AND t.state='queued' AND t.host_epoch=?5 AND {AUTHORIZED})
              WHERE turn<=?6 ORDER BY queued_seq LIMIT ?7) ORDER BY t.queued_seq"
        ))?
        .query_map(
            params![
                own.tenant,
                own.account,
                own.volume,
                own.generation,
                host_epoch,
                DISPATCH_PER_GROUP,
                DISPATCH_BATCH
            ],
            task_row,
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for task in &due {
        // A child that cannot be read fails alone and leaves its siblings untouched.
        tx.execute_batch("SAVEPOINT conversation_run")?;
        match run_one(tx, owner, task) {
            Ok(()) => tx.execute_batch("RELEASE conversation_run")?,
            Err(_) => {
                tx.execute_batch("ROLLBACK TO conversation_run; RELEASE conversation_run")?;
                fail(
                    tx,
                    task,
                    "조회하지 못했습니다. 검색어를 바꿔 새 요청으로 다시 시도해 주세요.",
                )?;
            }
        }
    }
    Ok(due.len())
}

/// Stable worklog for a child across all of its executions.
pub(in crate::personal_browser) fn detail(
    c: &Connection,
    owner: VolumeOwner,
    task: TaskSummary,
) -> Result<TaskDetail> {
    let own = Own::of(owner);
    let (request, thread_title, turn): (String, String, String) = c
        .query_row(
            "SELECT g.message,th.title,g.turn_id FROM member_conversation_tasks t
             JOIN member_conversation_groups g ON g.id=t.group_id
             JOIN threads th ON th.tenant_id=g.tenant_id AND th.id=g.thread_id
             WHERE t.id=?1 AND g.tenant_id=?2 AND g.account_id=?3 AND g.volume_id=?4 AND g.generation=?5",
            params![task.id, own.tenant, own.account, own.volume, own.generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?
        .ok_or(StoreError::NotFound)?;
    let history = c
        .prepare(
            "SELECT state,text,at FROM member_conversation_events WHERE task_id=?1 ORDER BY id",
        )?
        .query_map([&task.id], |r| {
            Ok(TaskEvent {
                state: r.get(0)?,
                detail: Some(r.get(1)?),
                at: Some(r.get(2)?),
                title: None,
            })
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let runs = run_ids(c, &task.id)?;
    let result = match (task.state.as_str(), runs.last()) {
        ("completed", Some(run)) => Some(summarize(c, owner, run)?.1),
        _ => None,
    };
    Ok(TaskDetail {
        origin_turn_id: Some(turn),
        report_id: None,
        runs,
        task,
        request,
        thread_title,
        reason: None,
        history,
        result,
        observation: None,
    })
}

impl Store {
    pub async fn conversation_work_page(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        thread: String,
        request_id: Option<Uuid>,
    ) -> Result<ConversationWorkPage> {
        self.call(move |db| {
            let tx = db.conn.transaction()?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            let page = page_json(&tx, owner, &thread, request_id)?;
            tx.commit()?;
            finish(page)
        })
        .await
    }

    pub async fn create_conversation_work(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: String,
        request: ConversationWorkRequest,
    ) -> Result<ConversationWorkGroup> {
        let draft = parse_request(&request)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            let payload = payload_of(&draft);
            match existing(&tx, owner, &thread, &draft.request_id)? {
                Some((stored, None)) if stored == payload => {}
                Some(_) => return Err(StoreError::Conflict),
                None => new_group(
                    &tx,
                    owner,
                    &user,
                    host_epoch,
                    &thread,
                    &draft.request_id,
                    &draft.message,
                    &payload,
                    &draft.items,
                    None,
                )?,
            }
            let group = group_json(&tx, owner, &thread, &draft.request_id)?;
            tx.commit()?;
            finish(group)
        })
        .await
    }

    pub async fn control_conversation_work(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: String,
        action: ConversationWorkAction,
    ) -> Result<ConversationWorkGroup> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            let group = control(&tx, owner, &user, host_epoch, &thread, &action)?;
            let group = group_json(&tx, owner, &thread, &group)?;
            tx.commit()?;
            finish(group)
        })
        .await
    }
}

#[cfg(test)]
mod tests;
