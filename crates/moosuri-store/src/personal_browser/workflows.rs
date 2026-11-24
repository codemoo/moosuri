//! A member grants one bounded local search. The occurrence, source references
//! and next due time commit together, including when multiple runtimes tick.
use super::*;
use moosuri_core::personal_browser::{
    TaskEvent, WorkAction, WorkReceipt, WorkRequest, WorkSchedule, WorkScope,
};
use moosuri_core::{KnowledgeKind, KnowledgeQuery};
use serde::{Deserialize, Serialize};

mod conversation;
pub(super) mod monitors;
pub(super) use conversation::detail as conversation_detail;
pub(super) use conversation::group_report;
pub(crate) use conversation::member_work_turn;

const MAX_HITS: usize = 8;
/// Memory filters page the ordinary search and keep only records that contain every literal.
const FILTER_PAGE: usize = 32;
const FILTER_PAGES: usize = 8;
const SCHEDULES: &str = "SELECT id,title,scope,query,period,next_due,enabled,revision
 FROM member_workflows WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4 AND removed=0";

fn schedule_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<WorkSchedule> {
    Ok(WorkSchedule {
        id: r.get(0)?,
        title: r.get(1)?,
        scope: if r.get::<_, String>(2)? == "memory" {
            WorkScope::Memory
        } else {
            WorkScope::Conversation
        },
        query: r.get(3)?,
        period: r.get(4)?,
        next_due: r.get(5)?,
        enabled: r.get(6)?,
        revision: r.get(7)?,
    })
}
pub(super) fn schedules(connection: &Connection, owner: VolumeOwner) -> Result<Vec<WorkSchedule>> {
    let mut statement = connection.prepare(&format!(
        "{SCHEDULES} AND period>0 ORDER BY enabled DESC,next_due,id"
    ))?;
    Ok(statement
        .query_map(
            params![
                owner.tenant.to_string(),
                owner.account.to_string(),
                owner.volume.to_string(),
                owner.generation as i64
            ],
            schedule_row,
        )?
        .collect::<rusqlite::Result<_>>()?)
}
fn get(connection: &Connection, owner: VolumeOwner, id: &str) -> Result<WorkSchedule> {
    connection
        .query_row(
            &format!("{SCHEDULES} AND id=?5"),
            params![
                owner.tenant.to_string(),
                owner.account.to_string(),
                owner.volume.to_string(),
                owner.generation as i64,
                id
            ],
            schedule_row,
        )
        .optional()?
        .ok_or(StoreError::NotFound)
}
#[derive(Serialize, Deserialize)]
pub(super) struct SourceRef {
    id: String,
    revision: i64,
    read_at: i64,
}
fn label(scope: &WorkScope) -> &'static str {
    match scope {
        WorkScope::Memory => "기억",
        WorkScope::Conversation => "대화",
    }
}
fn canonical(value: &str) -> Result<()> {
    Uuid::parse_str(value)
        .ok()
        .filter(|id| !id.is_nil() && id.to_string() == value)
        .map(|_| ())
        .ok_or(StoreError::InvalidInput)
}
fn text_valid(text: &str, limit: usize) -> bool {
    !text.trim().is_empty() && text.len() <= limit && !text.chars().any(char::is_control)
}

fn memory_query(text: &str, cursor: Option<String>) -> KnowledgeQuery {
    KnowledgeQuery {
        kind: KnowledgeKind::Memory,
        text: text.to_owned(),
        match_mode: None,
        collection_id: None,
        subject: None,
        speaker: None,
        valid_at: None,
        observed_from: None,
        observed_until: None,
        cursor,
    }
}
fn has_all(filters: &[String], parts: &[&str]) -> bool {
    let haystack = parts.join("\n").to_ascii_lowercase();
    filters
        .iter()
        .all(|f| haystack.contains(&f.to_ascii_lowercase()))
}

fn exclusions(c: &Connection, id: &str) -> Result<Vec<String>> {
    Ok(c.prepare(
        "SELECT word FROM member_conversation_exclusions WHERE task_id=?1 ORDER BY rowid",
    )?
    .query_map([id], |r| r.get(0))?
    .collect::<rusqlite::Result<_>>()?)
}
fn search(
    connection: &Connection,
    owner: VolumeOwner,
    schedule: &WorkSchedule,
    filters: &[String],
) -> Result<(Vec<SourceRef>, bool)> {
    let tenant = owner.tenant.to_string();
    let omitted = exclusions(connection, &schedule.id)?;
    if schedule.scope == WorkScope::Memory && (!filters.is_empty() || !omitted.is_empty()) {
        // Every additional literal is applied to current source text before the hit limit.
        let mut found = Vec::new();
        let mut cursor = None;
        for page in 0..FILTER_PAGES {
            let result = crate::knowledge::search::search_owner(
                connection,
                &tenant,
                memory_query(&schedule.query, cursor.take()),
                FILTER_PAGE,
            )?;
            for hit in result.hits {
                let source = SourceRef {
                    id: hit.id,
                    revision: hit.revision,
                    read_at: 0,
                };
                if let Some((title, body)) =
                    read_source(connection, owner, &WorkScope::Memory, &source)?
                    && has_all(filters, &[title.as_str(), body.as_str()])
                    && omitted.iter().all(|word| {
                        !has_all(std::slice::from_ref(word), &[title.as_str(), body.as_str()])
                    })
                {
                    found.push(source);
                    if found.len() > MAX_HITS {
                        found.truncate(MAX_HITS);
                        return Ok((found, true));
                    }
                }
            }
            match result.next_cursor {
                Some(next) => cursor = Some(next),
                None => return Ok((found, result.truncated)),
            }
            if page + 1 == FILTER_PAGES {
                return Ok((found, true));
            }
        }
        return Ok((found, true));
    }
    if schedule.scope == WorkScope::Memory {
        let result = crate::knowledge::search::search_owner(
            connection,
            &tenant,
            memory_query(&schedule.query, None),
            MAX_HITS,
        )?;
        return Ok((
            result
                .hits
                .into_iter()
                .map(|h| SourceRef {
                    id: h.id,
                    revision: h.revision,
                    read_at: 0,
                })
                .collect(),
            result.truncated || result.next_cursor.is_some(),
        ));
    }
    // Cap original turns before inspecting bodies. The extra row records the scan boundary.
    // Additional literals narrow the same bounded scan before rows are limited to MAX_HITS.
    let narrowing: String = filters.iter().map(|_| ">0").chain(omitted.iter().map(|_| "=0"))
        .enumerate().map(|(i, comparison)| format!(
            " AND instr(lower(r.user_text||coalesce(char(10)||a.text,'')),lower(?{})){comparison}", 6+i
        )).collect();
    let sql=format!("WITH recent AS MATERIALIZED (
        SELECT r.id,r.version,r.user_text,r.admitted_order FROM chat_runs r
        JOIN personal_text_previews p ON p.tenant_id=r.tenant_id AND p.run_id=r.id
        JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id AND t.account_id=r.account_id
        WHERE r.tenant_id=?1 AND r.account_id=?2 AND p.volume_id=?3 AND p.generation=?4 AND t.mission_internal=0 AND r.state IN ('completed','incomplete','refused')
        ORDER BY r.admitted_order DESC LIMIT 501)
        SELECT r.id,r.version,(SELECT count(*) FROM recent)>500 FROM (SELECT * FROM recent ORDER BY admitted_order DESC LIMIT 500) r
        LEFT JOIN chat_output a ON a.tenant_id=?1 AND a.run_id=r.id AND a.item='personal-text-preview-answer'
        WHERE (instr(lower(r.user_text),lower(?5))>0 OR instr(lower(a.text),lower(?5))>0){narrowing}
        UNION ALL SELECT NULL,0,(SELECT count(*) FROM recent)>500 LIMIT 10");
    let mut values: Vec<rusqlite::types::Value> = vec![
        tenant.into(),
        owner.account.to_string().into(),
        owner.volume.to_string().into(),
        (owner.generation as i64).into(),
        schedule.query.clone().into(),
    ];
    values.extend(
        filters
            .iter()
            .chain(&omitted)
            .cloned()
            .map(rusqlite::types::Value::from),
    );
    let rows = connection
        .prepare(&sql)?
        .query_map(rusqlite::params_from_iter(values), |r| {
            Ok((
                r.get::<_, Option<String>>(0)?,
                r.get::<_, i64>(1)?,
                r.get::<_, bool>(2)?,
            ))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let limited =
        rows.iter().any(|r| r.2) || rows.iter().filter(|r| r.0.is_some()).count() > MAX_HITS;
    Ok((
        rows.into_iter()
            .filter_map(|(id, revision, _)| {
                id.map(|id| SourceRef {
                    id,
                    revision,
                    read_at: 0,
                })
            })
            .take(MAX_HITS)
            .collect(),
        limited,
    ))
}

fn execute(
    connection: &Connection,
    owner: VolumeOwner,
    schedule: &WorkSchedule,
    request: Option<&str>,
    due: Option<i64>,
    filters: &[String],
) -> Result<String> {
    if let Some(id) = connection
        .query_row(
            "SELECT id FROM member_work_runs WHERE workflow_id=?1 AND (request_id=?2 OR due_at=?3)",
            params![schedule.id, request, due],
            |r| r.get(0),
        )
        .optional()?
    {
        return Ok(id);
    }
    let started_at = now()?;
    let (matches, limited) = search(connection, owner, schedule, filters)?;
    let searched_at = now()?;
    let mut sources = Vec::new();
    for mut source in matches {
        if read_source(connection, owner, &schedule.scope, &source)?.is_some() {
            source.read_at = now()?;
            sources.push(source);
        }
    }
    let id = Uuid::new_v4().to_string();
    let refs = serde_json::to_string(&sources).map_err(|_| StoreError::Corrupt)?;
    connection.execute("INSERT INTO member_work_runs(id,workflow_id,request_id,due_at,created_at,admitted_order,sources,limited,searched_at,finished_at)
        VALUES(?1,?2,?3,?4,?8,?5,?6,?7,?9,unixepoch())",params![id,schedule.id,request,due,chat::next_order(connection)?,refs,limited,started_at,searched_at])?;
    Ok(id)
}

impl Store {
    /// Only inferred search terms may try synonyms. Explicit member queries keep their exact semantics.
    /// Persist the chosen term in the normal task/run so it remains inspectable.
    pub async fn resolve_interpreted_queries(
        &self,
        scope: Scope,
        mut items: Vec<moosuri_core::personal_browser::WorkItemInput>,
        alternatives: Vec<Vec<String>>,
    ) -> Result<Vec<moosuri_core::personal_browser::WorkItemInput>> {
        self.call(move |db| {
            let tx = db.conn.transaction()?;
            scope.authorize(&tx)?;
            for (item, candidates) in items.iter_mut().zip(alternatives) {
                if item.query.is_empty() {
                    continue;
                }
                let original = item.query.clone();
                let mut terms = vec![original.clone()];
                for term in candidates.into_iter().take(3).chain(
                    original
                        .split_whitespace()
                        .filter(|t| t.chars().count() >= 2)
                        .take(3)
                        .map(str::to_owned),
                ) {
                    if text_valid(&term, 128) && !terms.contains(&term) {
                        terms.push(term);
                    }
                }
                if terms.len() == 1 {
                    continue;
                }
                for term in terms {
                    let schedule = WorkSchedule {
                        id: String::new(),
                        title: item.title.clone(),
                        scope: item.scope.clone(),
                        query: term,
                        period: 0,
                        next_due: None,
                        enabled: true,
                        revision: 1,
                    };
                    if !search(&tx, scope.owner, &schedule, &item.filters)?
                        .0
                        .is_empty()
                    {
                        item.query = schedule.query;
                        break;
                    }
                }
            }
            tx.commit()?;
            Ok(items)
        })
        .await
    }

    pub async fn create_member_workflow(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        request: WorkRequest,
    ) -> Result<WorkReceipt> {
        canonical(&request.request_id)?;
        if !text_valid(&request.title, 240)
            || !text_valid(&request.query, 128)
            || !matches!(request.period, 0 | 1800 | 3600 | 86400 | 604800)
            || request.period == 0 && request.first_due.is_some()
            || request.thread_id.is_some() && request.period != 0
            || request.message.as_ref().is_some_and(|m| {
                request.thread_id.is_none() || m.trim().is_empty() || m.len() > 8192
            })
        {
            return Err(StoreError::InvalidInput);
        }
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            if let Some(thread) = &request.thread_id {
                canonical(thread)?;
                chat::thread_exists(&tx, &user, thread)?;
            }
            let scope = match request.scope {
                WorkScope::Memory => "memory",
                WorkScope::Conversation => "conversation",
            };
            let existing = tx.query_row(
                "SELECT tenant_id=?2 AND account_id=?3 AND volume_id=?4 AND generation=?5
                    AND title=?6 AND scope=?7 AND query=?8 AND period=?9 AND first_due IS ?10
                    AND thread_id IS ?11 AND message IS ?12 AND removed=0
                 FROM member_workflows WHERE id=?1",
                params![request.request_id, user.tenant.0, user.account.id, owner.volume.to_string(), owner.generation as i64,
                    request.title, scope, request.query, request.period, request.first_due, request.thread_id, request.message],
                |row| row.get::<_, bool>(0),
            ).optional()?;
            if existing == Some(false) { return Err(StoreError::Conflict); }
            if existing.is_none() {
                let time = now()?;
                if request.first_due.is_some_and(|due| due < time - 60 || due > time + 366 * 86400) {
                    return Err(StoreError::InvalidInput);
                }
                if request.period > 0 && schedules(&tx, owner)?.len() >= 32 { return Err(StoreError::BudgetExhausted); }
                let next = (request.period > 0).then_some(request.first_due.unwrap_or(time + request.period).max(time));
                tx.execute(
                    "INSERT INTO member_workflows(id,tenant_id,account_id,volume_id,generation,auth_epoch,title,scope,query,
                        period,first_due,next_due,enabled,thread_id,message,host_epoch)
                     VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16)",
                    params![request.request_id,user.tenant.0,user.account.id,owner.volume.to_string(),owner.generation as i64,user.epoch,request.title,
                        scope,request.query,request.period,request.first_due,next,request.period>0,request.thread_id,request.message,host_epoch],
                )?;
            }
            let run_id = if request.period == 0 {
                let schedule = get(&tx, owner, &request.request_id)?;
                let id = execute(&tx, owner, &schedule, Some(&request.request_id), None, &[])?;
                if let Some(thread) = &request.thread_id { add_chat_receipt(&tx, &user, thread, &request, &id)?; }
                Some(id)
            } else { None };
            tx.commit()?;
            Ok(WorkReceipt { workflow_id: request.request_id, run_id })
        }).await
    }

    pub async fn change_member_workflow(
        &self,
        owner: VolumeOwner,
        user: UserContext,
        host_epoch: i64,
        id: Uuid,
        action: WorkAction,
    ) -> Result<WorkReceipt> {
        self.call(move |db| {
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            authorize_member(&tx, owner, &user)?;
            let id = id.to_string();
            let saved = get(&tx, owner, &id)?;
            if saved.period == 0 { return Err(StoreError::NotFound); }
            let run_id = match action {
                WorkAction::Run { request_id } => {
                    canonical(&request_id)?;
                    Some(execute(&tx, owner, &saved, Some(&request_id), None, &[])?)
                }
                WorkAction::Pause { revision } | WorkAction::Resume { revision } | WorkAction::Remove { revision } => {
                    if revision != saved.revision { return Err(StoreError::Conflict); }
                    let enabled = matches!(action, WorkAction::Resume { .. });
                    let removed = matches!(action, WorkAction::Remove { .. });
                    let next = enabled.then_some(now()? + saved.period);
                    tx.execute(
                        "UPDATE member_workflows SET enabled=?2,removed=?3,next_due=?4,auth_epoch=?5,
                            host_epoch=?6,revision=revision+1 WHERE id=?1",
                        params![id,enabled,removed,next,user.epoch,host_epoch],
                    )?;
                    None
                }
            };
            tx.commit()?;
            Ok(WorkReceipt {workflow_id: id, run_id})
        }).await
    }

    pub async fn tick_member_workflows(
        &self,
        owner: VolumeOwner,
        host_epoch: i64,
    ) -> Result<usize> {
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx,None)?;
            // A scheduled search can fail independently. Commit the conversation batch
            // before processing recurring work so that its failure cannot rewind these receipts.
            let conversation = conversation::dispatch(&tx, owner, host_epoch)?;
            tx.commit()?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx,None)?;
            // An older in-flight gateway tick cannot revoke a newer owner grant.
            tx.execute(
                "UPDATE member_workflows SET enabled=0,next_due=NULL,revision=revision+1
                 WHERE tenant_id=?1 AND account_id=?2 AND volume_id=?3 AND generation=?4 AND enabled=1
                   AND (host_epoch<?5 OR NOT EXISTS(SELECT 1 FROM accounts a WHERE a.id=?2 AND a.tenant_id=?1
                       AND a.auth_epoch=member_workflows.auth_epoch AND a.status='active'
                       AND a.must_change_password=0 AND a.recovery_required=0))",
                params![owner.tenant.to_string(),owner.account.to_string(),owner.volume.to_string(),owner.generation as i64,host_epoch],
            )?;
            let time = now()?;
            let sql = format!("{SCHEDULES} AND enabled=1 AND next_due<=?5 AND host_epoch=?6 ORDER BY next_due,id LIMIT 4");
            let due = tx.prepare(&sql)?.query_map(
                params![owner.tenant.to_string(),owner.account.to_string(),owner.volume.to_string(),owner.generation as i64,time,host_epoch],
                schedule_row,
            )?.collect::<rusqlite::Result<Vec<_>>>()?;
            for schedule in &due {
                let first = schedule.next_due.ok_or(StoreError::Corrupt)?;
                let at = first + ((time-first)/schedule.period)*schedule.period;
                execute(&tx, owner, schedule, None, Some(at), &[])?;
                tx.execute("UPDATE member_workflows SET next_due=?2 WHERE id=?1", params![schedule.id, at + schedule.period])?;
            }
            tx.commit()?;
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            owner.check_identity(&tx,None)?;
            let watched = monitors::dispatch(&tx, owner, host_epoch, now()?)?;
            tx.commit()?;
            Ok(due.len() + conversation + watched)
        }).await
    }
}

fn read_source(
    connection: &Connection,
    owner: VolumeOwner,
    scope: &WorkScope,
    source: &SourceRef,
) -> Result<Option<(String, String)>> {
    let tenant = owner.tenant.to_string();
    match scope {
        WorkScope::Memory=>Ok(connection.query_row("SELECT json_extract(v.draft,'$.subject'),json_extract(v.draft,'$.text')
            FROM knowledge_memories m JOIN knowledge_memory_versions v ON v.tenant_id=m.tenant_id AND v.memory_id=m.id
            WHERE m.tenant_id=?1 AND m.id=?2 AND v.revision=?3 AND v.status='active' AND m.status!='forgotten'
            AND v.revision=(SELECT max(revision) FROM knowledge_memory_timeline WHERE tenant_id=m.tenant_id AND memory_id=m.id AND (valid_from IS NULL OR valid_from<=unixepoch()))
            AND (json_extract(v.draft,'$.valid_until') IS NULL OR json_extract(v.draft,'$.valid_until')>unixepoch())",
            params![tenant,source.id,source.revision],|r|Ok((r.get(0)?,r.get(1)?))).optional()?),
        WorkScope::Conversation=>Ok(connection.query_row("SELECT t.title,r.user_text||coalesce(char(10)||char(10)||a.text,'')
            FROM chat_runs r JOIN personal_text_previews p ON p.tenant_id=r.tenant_id AND p.run_id=r.id
            JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id AND t.account_id=r.account_id
            LEFT JOIN chat_output a ON a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.item='personal-text-preview-answer'
            WHERE r.tenant_id=?1 AND r.account_id=?2 AND r.id=?3 AND p.volume_id=?4 AND p.generation=?5 AND t.mission_internal=0 AND r.version=?6",
            params![tenant,owner.account.to_string(),source.id,owner.volume.to_string(),owner.generation as i64,source.revision],|r|Ok((r.get(0)?,r.get(1)?))).optional()?)
    }
}
// Include the matching portion in the result when it falls late in an answer.
fn excerpt(text: &str, query: &str, limit: usize) -> String {
    let byte = text
        .to_ascii_lowercase()
        .find(&query.to_ascii_lowercase())
        .unwrap_or(0);
    let start = text[..byte]
        .char_indices()
        .rev()
        .nth(35)
        .map_or(0, |(i, _)| i);
    let mut snippet: String = text[start..].chars().take(limit).collect();
    if start + snippet.len() < text.len() {
        snippet.push('…');
    }
    if start > 0 {
        snippet.insert(0, '…');
    }
    snippet
}

struct RunEvidence {
    query: String,
    message: Option<String>,
    scope: WorkScope,
    sources: Vec<SourceRef>,
    limited: bool,
    thread_title: Option<String>,
    searched_at: i64,
    finished_at: i64,
    /// Extra literal AND conditions captured by a conversation run; empty for every other run.
    filters: Vec<String>,
}

fn run_evidence(connection: &Connection, owner: VolumeOwner, id: &str) -> Result<RunEvidence> {
    let mut stmt = connection.prepare(
        "SELECT w.query,w.scope,r.sources,r.limited,t.title,r.searched_at,r.finished_at,w.message,c.filters
         FROM member_work_runs r JOIN member_workflows w ON w.id=r.workflow_id
         LEFT JOIN member_conversation_runs c ON c.run_id=r.id
         LEFT JOIN threads t ON t.tenant_id=w.tenant_id AND t.account_id=w.account_id AND t.id=w.thread_id
         WHERE r.id=?1 AND w.tenant_id=?2 AND w.account_id=?3 AND w.volume_id=?4 AND w.generation=?5",
    )?;
    let mut rows = stmt.query(params![
        id,
        owner.tenant.to_string(),
        owner.account.to_string(),
        owner.volume.to_string(),
        owner.generation as i64
    ])?;
    let row = rows.next()?.ok_or(StoreError::NotFound)?;
    let scope = match row.get::<_, String>(1)?.as_str() {
        "memory" => WorkScope::Memory,
        "conversation" => WorkScope::Conversation,
        _ => return Err(StoreError::Corrupt),
    };
    let raw: String = row.get(2)?;
    Ok(RunEvidence {
        query: row.get(0)?,
        message: row.get(7)?,
        scope,
        sources: serde_json::from_str(&raw).map_err(|_| StoreError::Corrupt)?,
        limited: row.get(3)?,
        thread_title: row.get(4)?,
        searched_at: row.get(5)?,
        finished_at: row.get(6)?,
        filters: match row.get::<_, Option<String>>(8)? {
            Some(raw) => serde_json::from_str(&raw).map_err(|_| StoreError::Corrupt)?,
            None => Vec::new(),
        },
    })
}
/// The searched terms as shown to the member, with any extra AND conditions.
fn terms(query: &str, filters: &[String]) -> String {
    filters
        .iter()
        .fold(query.to_owned(), |text, f| format!("{text}’ 그리고 ‘{f}"))
}

fn preview_text(scope: &WorkScope, count: usize, snippet: Option<&str>) -> String {
    let summary = format!("{} {count}건 확인", label(scope));
    match snippet {
        Some(text) => format!("{summary} · {text}"),
        None => summary,
    }
}

pub(super) fn preview(connection: &Connection, owner: VolumeOwner, id: &str) -> Result<String> {
    let evidence = run_evidence(connection, owner, id)?;
    let mut snippet = None;
    for source in &evidence.sources {
        if let Some((_, body)) = read_source(connection, owner, &evidence.scope, source)? {
            snippet = Some(excerpt(&body, &evidence.query, 110));
            break;
        }
    }
    Ok(preview_text(
        &evidence.scope,
        evidence.sources.len(),
        snippet.as_deref(),
    ))
}

pub(super) fn detail(
    connection: &Connection,
    owner: VolumeOwner,
    mut task: moosuri_core::personal_browser::TaskSummary,
) -> Result<moosuri_core::personal_browser::TaskDetail> {
    let RunEvidence {
        query,
        message,
        scope,
        sources,
        limited,
        thread_title,
        searched_at,
        finished_at,
        filters,
    } = run_evidence(connection, owner, &task.id)?;
    let shown = terms(&query, &filters);
    let event = |state: &str, at: i64, title: String, detail: String| TaskEvent {
        state: state.into(),
        at: Some(at),
        title: Some(title),
        detail: Some(detail),
    };
    let mut history = vec![
        event(
            "starting",
            task.created_at,
            if task.scheduled_at.is_some() {
                "예약 실행 시작".into()
            } else {
                "조사 시작".into()
            },
            task.title.clone(),
        ),
        event(
            "searched",
            searched_at,
            format!("내 {}에서 검색", label(&scope)),
            format!(
                "검색어 ‘{shown}’ · 확인한 항목 {}개{}",
                sources.len(),
                if limited {
                    " · 조회 한도 도달"
                } else {
                    ""
                }
            ),
        ),
    ];
    let mut findings = Vec::new();
    let mut first_snippet = None;
    for (i, source) in sources.iter().enumerate() {
        let current = read_source(connection, owner, &scope, source)?;
        if let Some((title, body)) = &current
            && findings.len() < 3
        {
            let snippet = excerpt(body, &query, 110);
            if findings.is_empty() {
                first_snippet = Some(snippet.clone());
            }
            findings.push(format!("• {title} — {snippet}"));
        }
        let (title, text) = current.unwrap_or_else(|| {
            (
                format!("확인한 {} {}", label(&scope), i + 1),
                "원본이 변경되었거나 더 이상 볼 수 없습니다.".into(),
            )
        });
        history.push(event(
            "source_read",
            source.read_at,
            title,
            excerpt(&text, &query, 4096),
        ));
    }
    task.preview = Some(preview_text(
        &scope,
        sources.len(),
        first_snippet.as_deref(),
    ));
    let mut result = format!(
        "검색어 ‘{shown}’에 해당하는 {} {}건을 확인했습니다.{}",
        label(&scope),
        sources.len(),
        if limited {
            " 조회 한도 안에서 확인한 결과입니다."
        } else {
            ""
        }
    );
    if !findings.is_empty() {
        result.push_str("\n\n");
        result.push_str(&findings.join("\n"));
    }
    history.push(event(
        "completed",
        finished_at,
        "조사 완료".into(),
        result.clone(),
    ));
    let (origin_turn_id, report_id) = connection.query_row(
        "SELECT g.turn_id,d.id FROM member_conversation_runs cr JOIN member_conversation_tasks t ON t.id=cr.task_id
         JOIN member_conversation_groups g ON g.id=t.group_id
         LEFT JOIN member_conversation_reports d ON d.task_id=t.id AND d.run_id=cr.run_id AND d.thread_id=g.thread_id
           AND d.tenant_id=g.tenant_id AND d.account_id=g.account_id AND d.volume_id=g.volume_id AND d.generation=g.generation
         WHERE cr.run_id=?1 AND g.tenant_id=?2 AND g.account_id=?3 AND g.volume_id=?4 AND g.generation=?5",
        params![task.id, owner.tenant.to_string(), owner.account.to_string(), owner.volume.to_string(), owner.generation as i64],
        |r| Ok((Some(r.get::<_, String>(0)?), r.get::<_, Option<String>>(1)?)),
    ).optional()?.unwrap_or_default();
    Ok(moosuri_core::personal_browser::TaskDetail {
        origin_turn_id,
        report_id,
        runs: Vec::new(),
        task,
        request: message.unwrap_or_else(|| format!("내 {}에서 ‘{shown}’ 검색", label(&scope))),
        thread_title: thread_title.unwrap_or_default(),
        reason: None,
        history,
        result: Some(result),
        observation: None,
    })
}

fn add_chat_receipt(
    connection: &Connection,
    user: &UserContext,
    thread: &str,
    request: &WorkRequest,
    run: &str,
) -> Result<()> {
    if connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND id=?2)",
        params![user.tenant.0, run],
        |r| r.get::<_, bool>(0),
    )? {
        return Ok(());
    }
    let busy:bool=connection.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND state IN ('running','queued'))",params![user.tenant.0,thread],|r|r.get(0))?;
    if busy {
        return Err(StoreError::Conflict);
    }
    let text = request.message.as_deref().unwrap_or(&request.title);
    let mut model = observation_model();
    model.provider_id = "member-workflow".into();
    model.provider_name = "Member workspace".into();
    model.model = "local-search".into();
    let count: i64 = connection.query_row(
        "SELECT json_array_length(sources) FROM member_work_runs WHERE id=?1",
        [run],
        |r| r.get(0),
    )?;
    let sequence:i64=connection.query_row("UPDATE threads SET chat_turn_seq=chat_turn_seq+1 WHERE tenant_id=?1 AND id=?2 AND account_id=?3 RETURNING chat_turn_seq",params![user.tenant.0,thread,user.account.id],|r|r.get(0))?;
    connection.execute("INSERT INTO chat_runs(tenant_id,id,account_id,thread_id,request_id,payload_digest,sequence,user_text,model,session_digest,auth_epoch,attempt,state,version,created_at,admitted_order)
        SELECT ?1,id,?3,?4,?5,?6,?7,?8,?9,?10,?11,1,'completed',1,created_at,admitted_order FROM member_work_runs WHERE id=?2",
        params![user.tenant.0,run,user.account.id,thread,request.request_id,Sha256::digest(text.as_bytes()).as_slice(),sequence,text,serde_json::to_string(&model).map_err(|_|StoreError::Corrupt)?,user.digest.as_slice(),user.epoch])?;
    let loaded = chat::load_run(connection, &user.tenant.0, run)?;
    chat::apply_patches(
        connection,
        &loaded,
        vec![chat::OutputPatch::Append {
            item: "member-work-result".into(),
            text: format!(
                "{} 검색 결과 {count}건을 확인했습니다. 업무 기록에서 각 항목을 열 수 있습니다.",
                label(&request.scope)
            ),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    chat::emit(
        connection,
        &loaded,
        ChatChange::Turn {
            turn: chat::turn(connection, &loaded)?,
        },
    )?;
    Ok(())
}
