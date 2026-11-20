//! Owner-scoped history tools and transitive destructive provenance. No network IO.
use super::*;
use crate::chat::{self, Run};
use moosuri_core::{
    ConversationHit, ConversationScope, ConversationSearch, ConversationTurn, HistoricalMessage,
    HistoricalRole,
};
const SCAN: usize = 4096;
const MAX_HITS: usize = 5;
const WHOLE_BYTES: usize = 10 * 1024;
fn enabled(r: &Run) -> bool {
    r.tool_profile
        .as_ref()
        .is_some_and(|p| matches!(p.version, 1..=5) && p.conversation_recall)
}
fn excerpt(text: &str, query: &str) -> String {
    let lowered = text.to_lowercase();
    let offset = lowered.find(query).unwrap_or(0); // case fold can change byte width; only use a safe original boundary
    let start = if text.is_char_boundary(offset) {
        offset
    } else {
        0
    };
    text[start..].chars().take(240).collect()
}
fn eligible_base(c: &Connection, r: &Run, id: &str) -> Result<Option<Run>> {
    let exists:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs p JOIN threads t ON t.tenant_id=p.tenant_id AND t.id=p.thread_id WHERE p.tenant_id=?1 AND p.account_id=?2 AND p.id=?3 AND p.rowid<(SELECT rowid FROM chat_runs WHERE tenant_id=?1 AND id=?4) AND p.created_at<=?5 AND p.state='completed' AND NOT EXISTS(SELECT 1 FROM agent_execution a WHERE a.tenant_id=p.tenant_id AND a.run_id=p.id AND a.invalidated=1))",params![r.tenant,r.account,id,r.id,r.created],|v|v.get(0))?;
    if !exists || crate::credential_chat::display_only_preview(c, &r.tenant, id)? {
        return Ok(None);
    }
    Ok(Some(chat::load_run(c, &r.tenant, id)?))
}
fn eligible(c: &Connection, r: &Run, id: &str) -> Result<Option<Run>> {
    let Some(source) = eligible_base(c, r, id)? else {
        return Ok(None);
    };
    if !current(c, &source)?
        || !crate::calendar::agent::dependencies_current(c, &source)?
        || !crate::learned_skills::dependencies_current(c, &source)?
    {
        return Ok(None);
    }
    Ok(Some(source))
}
fn whole(c: &Connection, r: &Run) -> Result<ConversationTurn> {
    if crate::credential_chat::display_only_preview(c, &r.tenant, &r.id)? {
        return Err(StoreError::NotFound);
    }
    let output_bytes: i64 = c.query_row(
        "SELECT coalesce(sum(length(CAST(text AS BLOB))),0) FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant,r.id], |v| v.get(0))?;
    if output_bytes > WHOLE_BYTES as i64
        || r.text.len().saturating_add(output_bytes as usize) > WHOLE_BYTES
    {
        return Err(StoreError::InvalidInput);
    }
    let mut messages = vec![HistoricalMessage {
        role: HistoricalRole::User,
        text: r.text.clone(),
        phase: None,
    }];
    let out=c.prepare("SELECT text,phase FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND text!='' ORDER BY ordinal")?.query_map(params![r.tenant,r.id],|v|Ok((v.get::<_,String>(0)?,v.get::<_,Option<String>>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    if out.is_empty() {
        return Err(StoreError::NotFound);
    }
    for (text, phase) in out {
        messages.push(HistoricalMessage {
            role: HistoricalRole::Assistant,
            text,
            phase: match phase.as_deref() {
                Some("commentary") => Some(moosuri_core::MessagePhase::Commentary),
                Some("final_answer") => Some(moosuri_core::MessagePhase::FinalAnswer),
                None => None,
                _ => return Err(StoreError::Corrupt),
            },
        })
    }
    if messages.iter().map(|m| m.text.len()).sum::<usize>() > WHOLE_BYTES {
        return Err(StoreError::InvalidInput);
    }
    let result = ConversationTurn {
        turn_id: r.id.clone(),
        thread_id: r.thread.clone(),
        revision: r.version,
        created_at: r.created,
        messages,
    };
    if serde_json::to_vec(&result)
        .map_err(|_| StoreError::Corrupt)?
        .len()
        > 14 * 1024
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(result)
}
fn adopt(c: &Connection, r: &Run, source: &Run) -> Result<()> {
    let old:Option<i64>=c.query_row("SELECT source_version FROM conversation_recall_dependencies WHERE tenant_id=?1 AND run_id=?2 AND source_run_id=?3",params![r.tenant,r.id,source.id],|v|v.get(0)).optional()?;
    if old.is_some_and(|v| v != source.version) {
        return Err(StoreError::Conflict);
    }
    c.execute("INSERT INTO conversation_recall_dependencies VALUES(?1,?2,?3,?4,1) ON CONFLICT(tenant_id,run_id,source_run_id) DO UPDATE SET discovered=1",params![r.tenant,r.id,source.id,source.version])?;
    crate::agent_execution::inherit_dependencies(c, &r.tenant, &r.id, &source.id)?;
    Ok(())
}
// This prefilter never grants a discovery pin. Match the exact text seen by
// whole(): load_run reads user_text verbatim; materialize only changes a tool profile.
fn candidate_score(
    statement: &mut rusqlite::Statement<'_>,
    tenant: &str,
    id: &str,
    query: &str,
    qualifiers: &[&str],
) -> Result<Option<usize>> {
    let texts = statement.query_row(params![tenant, id, WHOLE_BYTES as i64], |row| {
        let assistant: Option<String> = row.get(1)?;
        assistant
            .map(|text| Ok((row.get::<_, String>(0)?, text)))
            .transpose()
    })?;
    let Some((user, assistant)) = texts else {
        return Ok(None);
    };
    // SQLite lower() does not implement Rust's Unicode folding.
    let user = user.to_lowercase();
    let assistant = assistant.to_lowercase();
    if !user.contains(query) && !assistant.contains(query) {
        return Ok(None);
    }
    Ok(Some(
        qualifiers
            .iter()
            .filter(|&&word| user.contains(word) || assistant.contains(word))
            .count(),
    ))
}
pub(crate) fn search(
    c: &Connection,
    r: &Run,
    query: &str,
    scope: ConversationScope,
) -> Result<ConversationSearch> {
    if !enabled(r)
        || query.trim().is_empty()
        || query.len() > 128
        || query.chars().any(char::is_control)
    {
        return Err(StoreError::InvalidInput);
    }
    let query = query.trim().to_lowercase();
    let question = r.text.to_lowercase();
    let mut context = Vec::new();
    for word in question.split(|c: char| !c.is_alphanumeric()) {
        if word.chars().nth(1).is_some() && !query.contains(word) && !context.contains(&word) {
            context.push(word);
            if context.len() == 16 {
                break;
            }
        }
    }
    let mut recent = c.prepare(
        "SELECT p.id FROM chat_runs p
         JOIN threads t ON t.tenant_id=p.tenant_id AND t.id=p.thread_id
         WHERE p.tenant_id=?1 AND p.account_id=?2
           AND p.rowid<(SELECT rowid FROM chat_runs WHERE tenant_id=?1 AND id=?3)
           AND p.created_at<=?4 AND p.state='completed' AND (?5 OR p.thread_id=?6)
           AND NOT EXISTS(SELECT 1 FROM agent_execution a WHERE a.tenant_id=p.tenant_id
                          AND a.run_id=p.id AND a.invalidated=1)
         ORDER BY p.rowid DESC LIMIT ?7",
    )?;
    let ids = recent
        .query_map(
            params![
                r.tenant,
                r.account,
                r.id,
                r.created,
                matches!(scope, ConversationScope::OwnedConversations),
                r.thread,
                SCAN as i64 + 1
            ],
            |row| row.get::<_, String>(0),
        )?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let scanned_limit = ids.len() > SCAN;
    let mut scorer = c.prepare(
        "SELECT user_text,
             CASE WHEN length(CAST(user_text AS BLOB)) +
                 (SELECT sum(length(CAST(text AS BLOB))) FROM chat_output WHERE tenant_id=?1 AND run_id=?2) <= ?3
             THEN (SELECT group_concat(text, char(10)) FROM
                    (SELECT text FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND text!='' ORDER BY ordinal))
             END
         FROM chat_runs WHERE tenant_id=?1 AND id=?2",
    )?;
    let mut ranked: Vec<(usize, Run, ConversationHit)> = Vec::new();
    let mut truncated = scanned_limit;
    for id in ids.into_iter().take(SCAN) {
        if truncated
            && ranked
                .get(MAX_HITS - 1)
                .is_some_and(|item| item.0 == context.len())
        {
            break;
        }
        if crate::credential_chat::display_only_preview(c, &r.tenant, &id)? {
            continue;
        }
        let Some(score) = candidate_score(&mut scorer, &r.tenant, &id, &query, &context)? else {
            continue;
        };
        // With recency as the tie-breaker, a later equal score cannot displace
        // a kept result. Once overflow is established it needs no full read.
        let index = ranked.partition_point(|(old, _, _)| *old >= score);
        if truncated && index == MAX_HITS {
            continue;
        }
        let Some(source) = eligible(c, r, &id)? else {
            continue;
        };
        let turn = match whole(c, &source) {
            Ok(v) => v,
            Err(StoreError::InvalidInput | StoreError::NotFound) => continue,
            Err(e) => return Err(e),
        };
        if ranked.len() == MAX_HITS {
            truncated = true;
            if index == MAX_HITS {
                continue;
            }
            ranked.pop();
        }
        let assistant = turn
            .messages
            .iter()
            .filter(|m| matches!(m.role, HistoricalRole::Assistant))
            .map(|m| m.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let hit = ConversationHit {
            turn_id: source.id.clone(),
            thread_id: source.thread.clone(),
            revision: source.version,
            created_at: source.created,
            user_excerpt: excerpt(&source.text, &query),
            assistant_excerpt: excerpt(&assistant, &query),
        };
        ranked.insert(index, (score, source, hit));
    }
    let mut hits = Vec::with_capacity(ranked.len());
    for (_, source, hit) in ranked {
        // Only eligible, whole turns returned to this run grant discovery pins.
        adopt(c, r, &source)?;
        hits.push(hit);
    }
    Ok(ConversationSearch {
        hits,
        truncated,
        scanned_limit,
    })
}
pub(crate) fn read(c: &Connection, r: &Run, id: &str, revision: i64) -> Result<ConversationTurn> {
    if !enabled(r) || Uuid::parse_str(id).is_err() || revision <= 0 {
        return Err(StoreError::InvalidInput);
    }
    // Read accepts only an exact candidate actually returned to this run, not a
    // caller-invented owner's turn ID or an old pin from another request.
    let pin:Option<i64>=c.query_row("SELECT source_version FROM conversation_recall_dependencies WHERE tenant_id=?1 AND run_id=?2 AND source_run_id=?3 AND discovered=1",params![r.tenant,r.id,id],|v|v.get(0)).optional()?;
    if pin != Some(revision) {
        return Err(StoreError::NotFound);
    }
    let source = eligible(c, r, id)?.ok_or(StoreError::NotFound)?;
    if source.version != revision {
        return Err(StoreError::Conflict);
    }
    let result = whole(c, &source)?;
    adopt(c, r, &source)?;
    Ok(result)
}
pub(crate) fn inherit(c: &Connection, tenant: &str, run: &str, prior: &str) -> Result<bool> {
    Ok(c.execute("INSERT OR IGNORE INTO conversation_recall_dependencies SELECT d.tenant_id,?2,d.source_run_id,d.source_version,0 FROM conversation_recall_dependencies d JOIN chat_runs p ON p.tenant_id=d.tenant_id AND p.id=d.run_id JOIN chat_runs r ON r.tenant_id=p.tenant_id AND r.account_id=p.account_id WHERE d.tenant_id=?1 AND d.run_id=?3 AND r.id=?2",params![tenant,run,prior])?>0)
}
pub(crate) fn current(c: &Connection, r: &Run) -> Result<bool> {
    let rows=c.prepare("SELECT source_run_id,source_version FROM conversation_recall_dependencies WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![r.tenant,r.id],|v|Ok((v.get::<_,String>(0)?,v.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for (id, version) in rows {
        if eligible_base(c, r, &id)?.is_none_or(|v| v.version != version) {
            return Ok(false);
        }
    }
    Ok(true)
}
/// Fixed point over actual recall edges. Sources are older than targets, so cycles
/// are not minted. Ignore missing already-deleted targets rather than global IDs.
pub(crate) fn expand(c: &Connection, tenant: &str, ids: Vec<String>) -> Result<Vec<String>> {
    let mut all: std::collections::BTreeSet<_> = ids.into_iter().collect();
    let mut pending = all.iter().cloned().collect::<Vec<_>>();
    while let Some(source) = pending.pop() {
        let descendants=c.prepare("SELECT d.run_id FROM conversation_recall_dependencies d JOIN chat_runs r ON r.tenant_id=d.tenant_id AND r.id=d.run_id WHERE d.tenant_id=?1 AND d.source_run_id=?2")?.query_map(params![tenant,source],|v|v.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for id in descendants {
            if all.insert(id.clone()) {
                pending.push(id)
            }
        }
    }
    Ok(all.into_iter().collect())
}
pub(crate) fn delete_thread(c: &Connection, tenant: &str, thread: &str) -> Result<()> {
    let ids = c
        .prepare("SELECT id FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2")?
        .query_map(params![tenant, thread], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    let expanded = expand(c, tenant, ids.clone())?;
    let descendants = expanded
        .into_iter()
        .filter(|id| !ids.contains(id))
        .collect();
    crate::agent_execution::purge_runs(c, tenant, descendants)
}
#[cfg(test)]
mod tests;
