//! Durable conversation work. One request is a completed chat receipt plus up to six independent
//! children. Children are read only by the authorized timer (`dispatch`), through the same bounded
//! member workflow execution as every other research run. Status reads never search or dispatch.
use super::*;
use crate::personal_browser::reports;
use moosuri_core::personal_browser::{
    ConversationReport, ConversationWorkAction, ConversationWorkGroup, ConversationWorkPage,
    ConversationWorkRequest, TaskDetail, TaskEvent, TaskSummary,
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
/// The flag is part of the payload only when set, so every group created without a report keeps
/// the exact payload it always had, and a replay that changes the flag conflicts.
fn payload_of(draft: &Draft, save_report: bool) -> String {
    let mut payload = json!({
        "message": draft.message,
        "items": draft.items.iter().map(|i| json!({"title": i.title, "scope": i.scope, "query": i.query, "filters": i.filters})).collect::<Vec<_>>(),
    });
    if save_report {
        payload["save_report"] = Value::Bool(true);
    }
    payload.to_string()
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
pub(crate) fn member_work_turn(
    c: &Connection,
    user: &UserContext,
    thread: &str,
    turn: &str,
    request: &str,
    message: &str,
    provider: &str,
    answer: String,
) -> Result<()> {
    let mut model = observation_model();
    model.provider_id = provider.into();
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
    chat::apply_patches(
        c,
        &loaded,
        vec![chat::OutputPatch::Append {
            item: "member-conversation-work-result".into(),
            text: answer,
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
    let note = if needs > 0 {
        format!(" 검색어가 필요한 {needs}건은 답을 기다립니다.")
    } else {
        String::new()
    };
    member_work_turn(
        tx,
        user,
        thread,
        &turn,
        id,
        message,
        MODEL_ID,
        format!(
            "조사 {}건을 접수했습니다.{note} 업무 기록에서 진행 상황과 결과를 확인할 수 있습니다.",
            items.len()
        ),
    )?;
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

// ------------------------------------------------------- departure evidence
//
// Only when the member asks about a departure ("출발"). Every value comes from an explicit time
// written next to the word 출발 in a source that is read again under current authority, so an
// edited or forgotten original contributes nothing. Arrival times and times not attached to 출발
// are never read as departures; nothing is guessed.

const TIME_GAP: usize = 6;
/// Shown for a record that carries no date; it is never offered as a date choice.
const NO_DATE: &str = "날짜 미상";

struct Found {
    start: usize,
    end: usize,
    text: String,
}
fn digits(c: &[char], at: usize, max: usize) -> Option<(u32, usize)> {
    let mut end = at;
    while end < c.len() && c[end].is_ascii_digit() && end - at < max + 1 {
        end += 1;
    }
    let n = end - at;
    if n == 0 || n > max {
        return None;
    }
    let value = c[at..end]
        .iter()
        .fold(0u32, |v, d| v * 10 + d.to_digit(10).unwrap_or(0));
    Some((value, end))
}
fn spaces(c: &[char], mut at: usize) -> usize {
    while at < c.len() && c[at] == ' ' {
        at += 1;
    }
    at
}
/// Explicit clock times: `H:MM`, `N시`, `N시 M분`, optionally after 오전/오후.
fn times_in(c: &[char]) -> Vec<Found> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if !c[i].is_ascii_digit() || (i > 0 && c[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        let Some((hour, after)) = digits(c, i, 2) else {
            i += 1;
            continue;
        };
        let parsed = if after < c.len() && c[after] == ':' {
            match digits(c, after + 1, 2) {
                Some((m, end))
                    if end - (after + 1) == 2
                        && !c.get(end).is_some_and(|x| x.is_ascii_digit()) =>
                {
                    Some((m, end))
                }
                _ => None,
            }
        } else if after < c.len() && c[after] == '시' {
            let next = spaces(c, after + 1);
            match digits(c, next, 2) {
                Some((m, end)) if c.get(end) == Some(&'분') => Some((m, end + 1)),
                _ => Some((0, after + 1)),
            }
        } else {
            None
        };
        let Some((minute, end)) = parsed else {
            i = after.max(i + 1);
            continue;
        };
        let mut begin = i;
        let mut hour = hour;
        let mut back = i;
        while back > 0 && c[back - 1] == ' ' {
            back -= 1;
        }
        if back >= 2 {
            match (c[back - 2], c[back - 1]) {
                ('오', '후') => {
                    begin = back - 2;
                    if hour < 12 {
                        hour += 12;
                    }
                }
                ('오', '전') => {
                    begin = back - 2;
                    if hour == 12 {
                        hour = 0;
                    }
                }
                _ => {}
            }
        }
        if hour < 24 && minute < 60 {
            out.push(Found {
                start: begin,
                end,
                text: format!("{hour:02}:{minute:02}"),
            });
        }
        i = end.max(i + 1);
    }
    out
}
/// Explicit dates: `YYYY-MM-DD`, `YYYY.MM.DD`, `YYYY/MM/DD`, `YYYY년 M월 D일`, `M월 D일`.
/// The text is kept exactly as written so it can be used as a literal condition.
fn dates_in(c: &[char]) -> Vec<(Option<u32>, u32, u32, String)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < c.len() {
        if !c[i].is_ascii_digit() || (i > 0 && c[i - 1].is_ascii_digit()) {
            i += 1;
            continue;
        }
        let mut found = None;
        if let Some((year, a)) = digits(c, i, 4).filter(|(_, a)| a - i == 4) {
            if matches!(c.get(a), Some('-' | '.' | '/'))
                && let Some((m, b)) = digits(c, a + 1, 2)
                && c.get(b) == c.get(a)
                && let Some((d, e)) = digits(c, b + 1, 2)
            {
                found = Some((Some(year), m, d, e));
            } else if c.get(a) == Some(&'년') {
                let s = spaces(c, a + 1);
                if let Some((m, b)) = digits(c, s, 2)
                    && c.get(b) == Some(&'월')
                {
                    let s = spaces(c, b + 1);
                    if let Some((d, e)) = digits(c, s, 2)
                        && c.get(e) == Some(&'일')
                    {
                        found = Some((Some(year), m, d, e + 1));
                    }
                }
            }
        } else if let Some((m, b)) = digits(c, i, 2)
            && c.get(b) == Some(&'월')
        {
            let s = spaces(c, b + 1);
            if let Some((d, e)) = digits(c, s, 2)
                && c.get(e) == Some(&'일')
            {
                found = Some((None, m, d, e + 1));
            }
        }
        match found {
            Some((y, m, d, end)) if (1..=12).contains(&m) && (1..=31).contains(&d) => {
                out.push((y, m, d, c[i..end].iter().collect()));
                i = end;
            }
            _ => i += 1,
        }
    }
    out
}
fn first_date(text: &str) -> Option<(Option<u32>, u32, u32, String)> {
    dates_in(&text.chars().collect::<Vec<_>>())
        .into_iter()
        .next()
}
fn same_date(a: &(Option<u32>, u32, u32, String), b: &(Option<u32>, u32, u32, String)) -> bool {
    a.1 == b.1 && a.2 == b.2 && (a.0.is_none() || b.0.is_none() || a.0 == b.0)
}
fn keyword_at(c: &[char], word: [char; 2]) -> Vec<(usize, usize)> {
    (0..c.len().saturating_sub(1))
        .filter(|i| c[*i] == word[0] && c[*i + 1] == word[1])
        .map(|i| (i, i + 2))
        .collect()
}
fn gap(t: &Found, k: (usize, usize)) -> usize {
    if k.1 <= t.start {
        t.start - k.1
    } else {
        k.0.saturating_sub(t.end)
    }
}
/// Explicit departure times of one record, as `HH:MM`, in order and without repeats. A time counts
/// only when 출발 is its single nearest keyword in the same clause (so 도착 times are excluded).
fn departure_times(body: &str) -> Vec<String> {
    let chars: Vec<char> = body.chars().collect();
    let mut out: Vec<String> = Vec::new();
    let mut from = 0;
    for i in 0..=chars.len() {
        let stop = i == chars.len()
            || matches!(chars[i], '\n' | '\r' | ',' | ';' | '，' | '·' | '/')
            || (chars[i] == '.' && chars.get(i + 1).is_none_or(|n| n.is_whitespace()));
        if !stop {
            continue;
        }
        let clause = &chars[from..i];
        from = i + 1;
        let leaves = keyword_at(clause, ['출', '발']);
        if leaves.is_empty() {
            continue;
        }
        let arrives = keyword_at(clause, ['도', '착']);
        for t in times_in(clause) {
            let near = |ks: &[(usize, usize)]| ks.iter().map(|k| gap(&t, *k)).min();
            let (Some(leave), arrive) = (near(&leaves), near(&arrives)) else {
                continue;
            };
            if leave <= TIME_GAP && arrive.is_none_or(|a| leave < a) && !out.contains(&t.text) {
                out.push(t.text);
            }
        }
    }
    out
}

struct Record {
    title: String,
    id: String,
    date: Option<(Option<u32>, u32, u32, String)>,
    times: Vec<String>,
}
fn names(records: &[&Record], f: impl Fn(&Record) -> String) -> String {
    records.iter().map(|r| f(r)).collect::<Vec<_>>().join(", ")
}
/// Same-thread answer for "출발 시각". Different dates are different trips: both are offered and
/// nothing is chosen. Incompatible times for one trip are shown with their sources as a conflict.
fn departure_block(records: &[Record]) -> Option<String> {
    let mut groups: Vec<Vec<&Record>> = Vec::new();
    for r in records.iter().filter(|r| !r.times.is_empty()) {
        let slot = groups.iter_mut().find(|g| match (&g[0].date, &r.date) {
            (Some(a), Some(b)) => same_date(a, b),
            (None, None) => true,
            _ => false,
        });
        match slot {
            Some(g) => g.push(r),
            None => groups.push(vec![r]),
        }
    }
    if groups.is_empty() {
        return None;
    }
    let label = |g: &[&Record]| {
        g[0].date
            .as_ref()
            .map_or(NO_DATE.to_owned(), |d| d.3.clone())
    };
    let distinct = |g: &[&Record]| {
        let mut all: Vec<String> = Vec::new();
        for r in g {
            for t in &r.times {
                if !all.contains(t) {
                    all.push(t.clone());
                }
            }
        }
        all
    };
    let detail = |g: &[&Record]| -> String {
        g.iter()
            .map(|r| {
                format!(
                    "\n  - 출발 {} · {} · 출처 {}",
                    r.times.join(" / "),
                    r.title,
                    r.id
                )
            })
            .collect()
    };
    let mut text = String::new();
    if groups.len() > 1 {
        text.push_str(&format!(
            "출발 시각 선택 필요: 날짜가 다른 기록 {}건이 있어 하나로 정하지 않았습니다. 기록은 바꾸지 않았습니다. 어느 날짜인지 골라 주세요.",
            groups.len()
        ));
        for g in &groups {
            let all = distinct(g);
            if all.len() == 1 {
                text.push_str(&format!(
                    "\n선택 {} · 출발 {} · {}",
                    label(g),
                    all[0],
                    names(g, |r| r.title.clone())
                ));
            } else {
                text.push_str(&format!(
                    "\n선택 {} · 출발 시각 충돌 {}",
                    label(g),
                    all.join(" / ")
                ));
                text.push_str(&detail(g));
            }
        }
    } else {
        let g = &groups[0];
        let all = distinct(g);
        if all.len() == 1 {
            text.push_str(&format!(
                "출발 시각: {} ({}) · {}",
                all[0],
                label(g),
                names(g, |r| format!("{} 출처 {}", r.title, r.id))
            ));
        } else {
            text.push_str(&format!(
                "출발 시각 충돌 ({}): 같은 출장 기록이 서로 다른 출발 시각 {}을(를) 가리켜 하나로 정하지 않았습니다.",
                label(g),
                all.join(" / ")
            ));
            text.push_str(&detail(g));
        }
    }
    Some(text)
}

/// Source text is read again under current authority; the count stays as it was when searched.
/// `asks` is what the member asked (title, request); departure analysis runs only for "출발".
fn summarize(
    c: &Connection,
    owner: VolumeOwner,
    run: &str,
    asks: &str,
) -> Result<(i64, String, bool)> {
    let evidence = run_evidence(c, owner, run)?;
    let mut shown = Vec::new();
    let mut records = Vec::new();
    let departures = asks.contains("출발") || evidence.query.contains("출발");
    for source in &evidence.sources {
        if shown.len() == 3 && !departures {
            break;
        }
        if let Some((title, body)) = read_source(c, owner, &evidence.scope, source)? {
            if shown.len() < 3 {
                shown.push(format!(
                    "• {title} — {}",
                    excerpt(&body, &evidence.query, 110)
                ));
            }
            if departures {
                records.push(Record {
                    date: first_date(&title).or_else(|| first_date(&body)),
                    times: departure_times(&body),
                    id: source.id.to_string(),
                    title,
                });
            }
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
    if let Some(block) = departure_block(&records) {
        text.push('\n');
        text.push_str(&block);
        if records.len() < evidence.sources.len() {
            text.push_str(
                "\n일부 원본이 변경되었거나 볼 수 없어 지금 읽을 수 있는 기록만 반영했습니다.",
            );
        }
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
        let (count, text, readable) =
            summarize(c, owner, run, &format!("{} {}", t.title, t.query))?;
        source_count = count;
        if t.state == "completed" {
            can_reuse = readable;
            summary = Some(text);
            report = reports::report_id(c, owner, thread, &t.id, Some(run.as_str()))?;
        }
    }
    let mut conditions = t.filters.clone();
    conditions.extend(
        exclusions(c, &t.id)?
            .into_iter()
            .map(|word| format!("{word} 제외")),
    );
    Ok(json!({
        "id": t.id, "title": t.title, "scope": t.scope, "query": t.query,
        "filter": if conditions.is_empty() { Value::Null } else { Value::String(conditions.join(" AND ")) },
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
    let mut group = json!({"id": id, "thread_id": thread, "turn_id": turn, "message": message, "tasks": rendered});
    // Optional metadata is added only when present so older clients see the old shape.
    let previous = c.prepare("SELECT id FROM member_conversation_group_reports WHERE group_id=?1 AND superseded=1 AND state='ready' ORDER BY rowid DESC LIMIT 24")?
        .query_map([id], |r| r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if !previous.is_empty() {
        group["previous_reports"] = json!(previous);
    }
    let (requested, report) = group_report_state(c, owner, thread, id)?;
    if requested {
        group["report_requested"] = Value::Bool(true);
    }
    if let Some(report) = report {
        group["group_report_id"] = Value::String(report);
    }
    Ok(group)
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
        "refine" | "exclude" => literal(text.ok_or(StoreError::InvalidInput)?)?,
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
        "refine" | "exclude" => {
            if !matches!(task.state.as_str(), "queued" | "completed") {
                return Err(StoreError::Conflict);
            }
            let omitted = exclusions(tx, &task.id)?;
            if task.filters.len() + omitted.len() >= MAX_FILTERS {
                return Err(StoreError::BudgetExhausted);
            }
            if task
                .filters
                .iter()
                .chain(&omitted)
                .any(|f| f.eq_ignore_ascii_case(&argument))
            {
                return Err(StoreError::InvalidInput);
            }
            let mut filters = task.filters.clone();
            let instruction = if kind == "exclude" {
                tx.execute(
                    "INSERT INTO member_conversation_exclusions(task_id,word) VALUES(?1,?2)",
                    params![task.id, argument],
                )?;
                format!("이번 조사에서 ‘{argument}’을(를) 제외합니다.")
            } else {
                filters.push(argument.clone());
                format!("추가 조건 ‘{argument}’을(를) 모두 포함하도록 다시 조회합니다.")
            };
            requeue(
                tx,
                &task,
                None,
                &filters,
                host_epoch,
                user.epoch,
                &instruction,
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
    if matches!(kind, "refine" | "exclude" | "answer") {
        let revised = tx.execute(
            "UPDATE member_conversation_group_reports SET superseded=1 WHERE group_id=?1 AND state='ready' AND superseded=0",
            [&task.group],
        )?;
        if revised == 1 {
            insert_report_request(
                tx,
                owner,
                user,
                host_epoch,
                thread,
                &task.group,
                &Uuid::new_v4().to_string(),
            )?;
        }
    }
    if kind == "cancel" {
        // A cancelled last child can complete a pending combined report as partial or unavailable.
        guarded_finalize(tx, owner, &task.group, host_epoch)?;
    }
    Ok(task.group)
}

// ------------------------------------------------------------ combined reports
//
// A group report is requested at creation or later. It becomes `ready` only when every child is
// terminal (queued and waiting children never yield an early result), by the authorized timer or by
// the member's own cancel. It records which verified child runs it covers; text is never copied.

const REPORT_SWEEP: i64 = 16;
const REPORT_AUTHORIZED: &str = "SELECT EXISTS(SELECT 1 FROM accounts a WHERE a.id=?1 AND a.tenant_id=?2
    AND a.auth_epoch=?3 AND a.status='active' AND a.must_change_password=0 AND a.recovery_required=0)";

fn tasks_of(c: &Connection, group: &str) -> Result<Vec<Task>> {
    Ok(
        c.prepare(&format!("{TASK} WHERE t.group_id=?1 ORDER BY t.position"))?
            .query_map([group], task_row)?
            .collect::<rusqlite::Result<Vec<_>>>()?,
    )
}

fn group_owned(c: &Connection, owner: VolumeOwner, thread: &str, id: &str) -> Result<()> {
    let own = Own::of(owner);
    let found: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM member_conversation_groups WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6)",
        params![id, own.tenant, own.account, own.volume, own.generation, thread],
        |r| r.get(0),
    )?;
    if found {
        Ok(())
    } else {
        Err(StoreError::NotFound)
    }
}

fn report_state(c: &Connection, owner: VolumeOwner, group: &str) -> Result<Option<String>> {
    let own = Own::of(owner);
    Ok(c.query_row(
        "SELECT state FROM member_conversation_group_reports WHERE group_id=?1 AND superseded=0 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5",
        params![group, own.tenant, own.account, own.volume, own.generation],
        |r| r.get(0),
    )
    .optional()?)
}

/// `(requested, id)`: requested while pending or ready; the id only while ready and at least one
/// covered child run still exists.
fn group_report_state(
    c: &Connection,
    owner: VolumeOwner,
    thread: &str,
    group: &str,
) -> Result<(bool, Option<String>)> {
    let own = Own::of(owner);
    let row: Option<(String, String, bool)> = c
        .query_row(
            "SELECT r.id,r.state,EXISTS(SELECT 1 FROM member_conversation_group_report_items i
                JOIN member_conversation_runs cr ON cr.run_id=i.run_id AND cr.task_id=i.task_id WHERE i.report_id=r.id)
             FROM member_conversation_group_reports r
             WHERE r.group_id=?1 AND r.superseded=0 AND r.tenant_id=?2 AND r.account_id=?3 AND r.volume_id=?4 AND r.generation=?5 AND r.thread_id=?6",
            params![group, own.tenant, own.account, own.volume, own.generation, thread],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(match row {
        Some((id, state, true)) if state == "ready" => (true, Some(id)),
        Some((_, state, _)) if state == "pending" => (true, None),
        _ => (false, None),
    })
}

/// A request id names exactly one group: it may not already belong to another group's report or
/// be the id of another group.
fn claim_check(c: &Connection, group: &str, request: &str) -> Result<()> {
    canonical(request)?;
    let claimed: Option<String> = c
        .query_row(
            "SELECT group_id FROM member_conversation_group_reports WHERE request_id=?1",
            [request],
            |r| r.get(0),
        )
        .optional()?;
    let other_group: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM member_conversation_groups WHERE id=?1 AND id!=?2)",
        params![request, group],
        |r| r.get(0),
    )?;
    if other_group || claimed.is_some_and(|g| g != group) {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn insert_report_request(
    c: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    thread: &str,
    group: &str,
    request: &str,
) -> Result<()> {
    claim_check(c, group, request)?;
    let own = Own::of(owner);
    c.execute(
        "INSERT INTO member_conversation_group_reports(id,group_id,request_id,tenant_id,account_id,volume_id,generation,thread_id,state,host_epoch,auth_epoch,created_at)
         VALUES(?1,?2,?3,?4,?5,?6,?7,?8,'pending',?9,?10,?11)",
        params![
            Uuid::new_v4().to_string(),
            group,
            request,
            own.tenant,
            own.account,
            own.volume,
            own.generation,
            thread,
            host_epoch,
            user.epoch,
            now()?
        ],
    )?;
    Ok(())
}

/// Builds or closes a pending report once every child is terminal. Returns without effect while
/// any child is queued or waiting for an answer, or when a newer host epoch owns the work.
fn finalize_group(c: &Connection, owner: VolumeOwner, group: &str, host_epoch: i64) -> Result<()> {
    let own = Own::of(owner);
    let row: Option<(String, i64, i64)> = c
        .query_row(
            "SELECT id,host_epoch,auth_epoch FROM member_conversation_group_reports
             WHERE group_id=?1 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND state='pending'",
            params![group, own.tenant, own.account, own.volume, own.generation],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    let Some((report, report_host, report_auth)) = row else {
        return Ok(());
    };
    if report_host > host_epoch {
        return Ok(());
    }
    let tasks = tasks_of(c, group)?;
    if tasks
        .iter()
        .any(|t| matches!(t.state.as_str(), "queued" | "needs_input"))
    {
        return Ok(());
    }
    let authorized: bool = c.query_row(
        REPORT_AUTHORIZED,
        params![own.account, own.tenant, report_auth],
        |r| r.get(0),
    )?;
    let mut covered = Vec::new();
    if authorized && report_host == host_epoch {
        for task in &tasks {
            if task.state == "completed"
                && let Some(run) = latest_run(c, &task.id)?
            {
                covered.push((task, run));
            }
        }
    }
    let time = now()?;
    if covered.is_empty() {
        c.execute(
            "UPDATE member_conversation_group_reports SET state='unavailable',finalized_at=?2 WHERE id=?1 AND state='pending'",
            params![report, time],
        )?;
        for task in &tasks {
            event(
                c,
                &task.id,
                task.revision,
                &task.state,
                "확인된 완료 결과가 없어 묶음 보고서를 만들지 않았습니다.",
            )?;
        }
        return Ok(());
    }
    for (position, (task, run)) in covered.iter().enumerate() {
        c.execute(
            "INSERT INTO member_conversation_group_report_items(report_id,task_id,run_id,position) VALUES(?1,?2,?3,?4)",
            params![report, task.id, run, position as i64],
        )?;
    }
    c.execute(
        "UPDATE member_conversation_group_reports SET state='ready',title=?2,finalized_at=?3 WHERE id=?1 AND state='pending'",
        params![report, format!("조사 {}건 묶음 보고서", covered.len()), time],
    )?;
    for (task, _) in &covered {
        event(
            c,
            &task.id,
            task.revision,
            "completed",
            "묶음 보고서에 포함했습니다.",
        )?;
    }
    Ok(())
}

/// A report that cannot be built now stays pending and is retried; it never fails the caller's work.
fn guarded_finalize(
    c: &Connection,
    owner: VolumeOwner,
    group: &str,
    host_epoch: i64,
) -> Result<()> {
    c.execute_batch("SAVEPOINT group_report")?;
    match finalize_group(c, owner, group, host_epoch) {
        Ok(()) => c.execute_batch("RELEASE group_report")?,
        Err(_) => c.execute_batch("ROLLBACK TO group_report; RELEASE group_report")?,
    }
    Ok(())
}

fn sweep_reports(c: &Connection, owner: VolumeOwner, host_epoch: i64) -> Result<()> {
    let own = Own::of(owner);
    let groups = c
        .prepare(
            "SELECT group_id FROM member_conversation_group_reports
             WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4 AND state='pending'
             ORDER BY created_at,id LIMIT ?5",
        )?
        .query_map(
            params![own.tenant, own.account, own.volume, own.generation, REPORT_SWEEP],
            |r| r.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for group in groups {
        guarded_finalize(c, owner, &group, host_epoch)?;
    }
    Ok(())
}

fn request_group_report(
    tx: &Connection,
    owner: VolumeOwner,
    user: &UserContext,
    host_epoch: i64,
    thread: &str,
    group: &str,
    request: &str,
) -> Result<()> {
    canonical(group)?;
    canonical(request)?;
    group_owned(tx, owner, thread, group)?;
    claim_check(tx, group, request)?;
    match report_state(tx, owner, group)?.as_deref() {
        // The same request again, or another request for a report that already exists, is a replay.
        Some("pending" | "ready") => return Ok(()),
        Some(_) => return Err(StoreError::Conflict),
        None => {}
    }
    // Only work that has produced, or can still produce, a verified result can be reported.
    if !tasks_of(tx, group)?
        .iter()
        .any(|t| matches!(t.state.as_str(), "queued" | "needs_input" | "completed"))
    {
        return Err(StoreError::Conflict);
    }
    insert_report_request(tx, owner, user, host_epoch, thread, group, request)?;
    finalize_group(tx, owner, group, host_epoch)?;
    if report_state(tx, owner, group)?.as_deref() == Some("unavailable") {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

/// The combined report as one object. Every child's text and evidence is read again from current,
/// owner-scoped sources, so edited or withdrawn originals leave no old excerpt behind.
pub(in crate::personal_browser) fn group_report(
    c: &Connection,
    owner: VolumeOwner,
    thread: &str,
    id: &str,
) -> Result<Option<ConversationReport>> {
    use std::fmt::Write;
    let own = Own::of(owner);
    let row: Option<(String, String)> = c
        .query_row(
            "SELECT title,group_id FROM member_conversation_group_reports
             WHERE id=?1 AND tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5 AND thread_id=?6 AND state='ready'",
            params![id, own.tenant, own.account, own.volume, own.generation, thread],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    let Some((title, group)) = row else {
        return Ok(None);
    };
    let covered = c
        .prepare(
            "SELECT i.task_id,i.run_id,t.title,r.created_at
             FROM member_conversation_group_report_items i
             JOIN member_conversation_tasks t ON t.id=i.task_id AND t.group_id=?7 AND t.thread_id=?6
             JOIN member_conversation_runs cr ON cr.run_id=i.run_id AND cr.task_id=i.task_id
             JOIN member_work_runs r ON r.id=i.run_id
             JOIN member_workflows w ON w.id=t.id AND w.tenant_id=?2 AND w.account_id=?3 AND w.volume_id=?4 AND w.generation=?5
             WHERE i.report_id=?1 ORDER BY i.position",
        )?
        .query_map(
            params![id, own.tenant, own.account, own.volume, own.generation, thread, group],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, i64>(3)?,
                ))
            },
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let first_run = covered.first().ok_or(StoreError::NotFound)?.1.clone();
    let tasks = tasks_of(c, &group)?;
    let mut body = format!(
        "조사 {}건 중 {}건의 확인된 결과를 묶었습니다.",
        tasks.len(),
        covered.len()
    );
    for (index, (task, run, task_title, created_at)) in covered.iter().enumerate() {
        let detail = super::detail(
            c,
            owner,
            TaskSummary {
                id: run.clone(),
                thread_id: thread.to_owned(),
                title: task_title.clone(),
                state: "completed".into(),
                next_actor: "none".into(),
                created_at: *created_at,
                kind: "research".into(),
                preview: None,
                scheduled_at: None,
                schedule_id: None,
            },
        )?;
        // The report and the card must resolve the same dates, conflicts and withdrawn
        // sources. Use this report's pinned occurrence even after a later search.
        let (_, result, _) = summarize(c, owner, run, task_title)?;
        write!(body, "\n\n{}. {task_title}\n{result}", index + 1)
            .map_err(|_| StoreError::Corrupt)?;
        let sources: Vec<_> = detail
            .history
            .iter()
            .filter(|e| e.state == "source_read")
            .collect();
        if !sources.is_empty() {
            body.push_str("\n읽은 근거");
            for (n, source) in sources.iter().enumerate() {
                write!(
                    body,
                    "\n{}) {}\n{}",
                    n + 1,
                    source.title.as_deref().unwrap_or("자료"),
                    source.detail.as_deref().unwrap_or_default()
                )
                .map_err(|_| StoreError::Corrupt)?;
            }
        }
        if latest_run(c, task)?.as_deref() != Some(run.as_str()) {
            body.push_str("\n이 보고서를 만든 뒤 다시 조사해 현재 결과와 다를 수 있습니다.");
        }
    }
    let left_out: Vec<_> = tasks
        .iter()
        .filter(|t| !covered.iter().any(|(id, ..)| *id == t.id))
        .collect();
    if !left_out.is_empty() {
        body.push_str("\n\n포함하지 못한 조사");
        for task in left_out {
            let why = match task.state.as_str() {
                "cancelled" => "취소됨",
                "failed" => "조회하지 못함",
                "completed" => "보고서에 담긴 조사 기록을 더 이상 볼 수 없음",
                _ => "아직 끝나지 않음",
            };
            write!(body, "\n- {}: {why}", task.title).map_err(|_| StoreError::Corrupt)?;
        }
    }
    Ok(Some(ConversationReport {
        id: id.to_owned(),
        title,
        body,
        run_id: first_run,
    }))
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
    // Reports finish with the timer, so no browser has to be open and no child is read twice.
    sweep_reports(tx, owner, host_epoch)?;
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
        ("completed", Some(run)) => {
            Some(summarize(c, owner, run, &format!("{} {}", task.title, request))?.1)
        }
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
        self.create_conversation_work_with_report(owner, user, host_epoch, thread, request, false)
            .await
    }

    /// Creates the group and, when asked, durably requests one combined report for it in the same
    /// transaction. The report is built later, when every child is terminal.
    pub async fn create_conversation_work_with_report(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: String,
        draft: ConversationWorkRequest,
        save_report: bool,
    ) -> Result<ConversationWorkGroup> {
        let draft = parse_request(&draft)?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            let payload = payload_of(&draft, save_report);
            match existing(&tx, owner, &thread, &draft.request_id)? {
                Some((stored, None)) if stored == payload => {}
                Some(_) => return Err(StoreError::Conflict),
                None => {
                    new_group(
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
                    )?;
                    if save_report {
                        insert_report_request(
                            &tx,
                            owner,
                            &user,
                            host_epoch,
                            &thread,
                            &draft.request_id,
                            &draft.request_id,
                        )?;
                    }
                }
            }
            let group = group_json(&tx, owner, &thread, &draft.request_id)?;
            tx.commit()?;
            finish(group)
        })
        .await
    }

    /// Requests one combined report for an existing group. An exact replay, or another request for
    /// a report that already exists, returns the current group; a request id that belongs to a
    /// different group conflicts; a group with no verifiable work is never reported as a success.
    pub async fn request_conversation_group_report(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        thread: String,
        group_id: String,
        request_id: String,
    ) -> Result<ConversationWorkGroup> {
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            thread_owned(&tx, &user, &thread)?;
            request_group_report(
                &tx,
                owner,
                &user,
                host_epoch,
                &thread,
                &group_id,
                &request_id,
            )?;
            let group = group_json(&tx, owner, &thread, &group_id)?;
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
