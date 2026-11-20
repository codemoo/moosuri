//! A member grants one bounded local search. The occurrence, source references
//! and next due time commit together, including when multiple runtimes tick.
use super::*;
use moosuri_core::personal_browser::{
    TaskEvent, WorkAction, WorkReceipt, WorkRequest, WorkSchedule, WorkScope,
};
use moosuri_core::{KnowledgeKind, KnowledgeQuery};
use serde::{Deserialize, Serialize};

const MAX_HITS: usize = 8;
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
    Ok(connection
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
        .ok_or(StoreError::NotFound)?)
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

fn search(
    connection: &Connection,
    owner: VolumeOwner,
    schedule: &WorkSchedule,
) -> Result<(Vec<SourceRef>, bool)> {
    let tenant = owner.tenant.to_string();
    if schedule.scope == WorkScope::Memory {
        let result = crate::knowledge::search::search_owner(
            connection,
            &tenant,
            KnowledgeQuery {
                kind: KnowledgeKind::Memory,
                text: schedule.query.clone(),
                match_mode: None,
                collection_id: None,
                subject: None,
                speaker: None,
                valid_at: None,
                observed_from: None,
                observed_until: None,
                cursor: None,
            },
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
    let sql="WITH recent AS MATERIALIZED (
        SELECT r.id,r.version,r.user_text,r.admitted_order FROM chat_runs r
        JOIN personal_text_previews p ON p.tenant_id=r.tenant_id AND p.run_id=r.id
        JOIN threads t ON t.tenant_id=r.tenant_id AND t.id=r.thread_id AND t.account_id=r.account_id
        WHERE r.tenant_id=?1 AND r.account_id=?2 AND p.volume_id=?3 AND p.generation=?4 AND t.mission_internal=0 AND r.state IN ('completed','incomplete','refused')
        ORDER BY r.admitted_order DESC LIMIT 501)
        SELECT r.id,r.version,(SELECT count(*) FROM recent)>500 FROM (SELECT * FROM recent ORDER BY admitted_order DESC LIMIT 500) r
        LEFT JOIN chat_output a ON a.tenant_id=?1 AND a.run_id=r.id AND a.item='personal-text-preview-answer'
        WHERE instr(lower(r.user_text),lower(?5))>0 OR instr(lower(a.text),lower(?5))>0
        UNION ALL SELECT NULL,0,(SELECT count(*) FROM recent)>500 LIMIT 10";
    let rows = connection
        .prepare(sql)?
        .query_map(
            params![
                tenant,
                owner.account.to_string(),
                owner.volume.to_string(),
                owner.generation as i64,
                schedule.query
            ],
            |r| {
                Ok((
                    r.get::<_, Option<String>>(0)?,
                    r.get::<_, i64>(1)?,
                    r.get::<_, bool>(2)?,
                ))
            },
        )?
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
    let (matches, limited) = search(connection, owner, schedule)?;
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
                let id = execute(&tx, owner, &schedule, Some(&request.request_id), None)?;
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
                    Some(execute(&tx, owner, &saved, Some(&request_id), None)?)
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
                execute(&tx, owner, schedule, None, Some(at))?;
                tx.execute("UPDATE member_workflows SET next_due=?2 WHERE id=?1", params![schedule.id, at + schedule.period])?;
            }
            tx.commit()?;
            Ok(due.len())
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

pub(super) fn detail(
    connection: &Connection,
    owner: VolumeOwner,
    task: moosuri_core::personal_browser::TaskSummary,
) -> Result<moosuri_core::personal_browser::TaskDetail> {
    let (query,scope,raw,limited,due,thread_title,searched_at,finished_at):(String,String,String,bool,Option<i64>,Option<String>,i64,i64)=connection.query_row("SELECT w.query,w.scope,r.sources,r.limited,r.due_at,t.title,r.searched_at,r.finished_at FROM member_work_runs r
        JOIN member_workflows w ON w.id=r.workflow_id LEFT JOIN threads t ON t.tenant_id=w.tenant_id AND t.id=w.thread_id
        WHERE r.id=?1 AND w.tenant_id=?2 AND w.account_id=?3 AND w.volume_id=?4 AND w.generation=?5",
        params![task.id,owner.tenant.to_string(),owner.account.to_string(),owner.volume.to_string(),owner.generation as i64],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?,r.get(5)?,r.get(6)?,r.get(7)?)))?;
    let scope = if scope == "memory" {
        WorkScope::Memory
    } else {
        WorkScope::Conversation
    };
    let sources: Vec<SourceRef> = serde_json::from_str(&raw).map_err(|_| StoreError::Corrupt)?;
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
            if due.is_some() {
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
                "검색어 ‘{query}’ · 확인한 항목 {}개{}",
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
    for (i, source) in sources.iter().enumerate() {
        let current = read_source(connection, owner, &scope, source)?;
        if let Some((title, body)) = &current
            && findings.len() < 3
        {
            findings.push(format!("• {title} — {}", excerpt(body, &query, 110)));
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
    let mut result = format!(
        "검색어 ‘{query}’에 해당하는 {} {}건을 확인했습니다.{}",
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
    Ok(moosuri_core::personal_browser::TaskDetail {
        task,
        request: query,
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
