//! External read provenance and withdrawal. Receipt identity is derived from actual
//! admitted slots, never model citations or browser/native display-only data.
use super::*;
use crate::agent_execution::{KnowledgeToolInput, KnowledgeToolResult};
use moosuri_core::{
    ArtifactExternalEvidence, CalendarReadRequest, ExternalToolRequest, MailToolRequest,
    WebToolRequest,
};

fn supported(request: &ExternalToolRequest) -> Option<&'static str> {
    match request {
        ExternalToolRequest::Web(WebToolRequest::Read { .. } | WebToolRequest::Search { .. }) => {
            Some("web")
        }
        ExternalToolRequest::Mail(
            MailToolRequest::Search { .. }
            | MailToolRequest::Read { .. }
            | MailToolRequest::Thread { .. }
            | MailToolRequest::Attachments { .. }
            | MailToolRequest::Attachment { .. },
        ) => Some("mail"),
        ExternalToolRequest::Calendar(
            CalendarReadRequest::List { .. } | CalendarReadRequest::Read { .. },
        ) => Some("calendar"),
        _ => None,
    }
}
fn sha(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

/// Store-side gate in addition to provider-visible tool definitions.
pub(crate) fn allowed(
    c: &Connection,
    run: &crate::chat::Run,
    request: &ExternalToolRequest,
) -> Result<bool> {
    let Some(p) = agent::profile(c, &run.tenant, &run.id)? else {
        return Ok(true);
    };
    Ok(!p.write
        || p.external_reports
            && p.kind == Some(ArtifactKind::Document)
            && supported(request).is_some())
}

/// Called only when an actual earlier assistant output is selected for model context.
/// Keep origins even for web-only runs, which have no connection dependencies.
pub(crate) fn inherit(c: &Connection, t: &str, run: &str, prior: &str) -> Result<()> {
    c.execute("INSERT OR IGNORE INTO artifact_external_context SELECT r.tenant_id,?2,r.id FROM chat_runs r JOIN chat_runs n ON n.tenant_id=r.tenant_id AND n.account_id=r.account_id AND n.thread_id=r.thread_id WHERE r.tenant_id=?1 AND r.id=?3 AND n.id=?2 AND r.state='completed' AND (EXISTS(SELECT 1 FROM external_tool_dispatches d WHERE d.tenant_id=r.tenant_id AND d.run_id=r.id) OR EXISTS(SELECT 1 FROM credential_chat_waits w WHERE w.tenant_id=r.tenant_id AND w.run_id=r.id) OR EXISTS(SELECT 1 FROM calendar_run_dependencies x WHERE x.tenant_id=r.tenant_id AND x.run_id=r.id) OR EXISTS(SELECT 1 FROM mail_run_dependencies x WHERE x.tenant_id=r.tenant_id AND x.run_id=r.id) OR EXISTS(SELECT 1 FROM calendar_command_plans x WHERE x.tenant_id=r.tenant_id AND x.run_id=r.id) OR EXISTS(SELECT 1 FROM calendar_agent_action_origins x WHERE x.tenant_id=r.tenant_id AND x.run_id=r.id) OR EXISTS(SELECT 1 FROM calendar_agent_action_links x WHERE x.tenant_id=r.tenant_id AND x.run_id=r.id)) AND NOT EXISTS(SELECT 1 FROM agent_execution a WHERE a.tenant_id=r.tenant_id AND a.run_id=r.id AND a.invalidated=1)",params![t,run,prior])?;
    c.execute("INSERT OR IGNORE INTO artifact_external_context SELECT e.tenant_id,?2,e.origin_run FROM artifact_external_context e JOIN chat_runs r ON r.tenant_id=e.tenant_id AND r.id=e.run_id JOIN chat_runs n ON n.tenant_id=r.tenant_id AND n.account_id=r.account_id AND n.thread_id=r.thread_id WHERE e.tenant_id=?1 AND e.run_id=?3 AND n.id=?2",params![t,run,prior])?;
    Ok(())
}
fn origins(c: &Connection, run: &crate::chat::Run) -> Result<Vec<String>> {
    Ok(c.prepare("SELECT ?2 UNION SELECT origin_run FROM artifact_external_context WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![run.tenant,run.id],|r|r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
type ConnectionIdentity = (String, String, i64);
fn connections(c: &Connection, tenant: &str, run: &str) -> Result<BTreeSet<ConnectionIdentity>> {
    Ok(c.prepare("SELECT 'mail',connection_id,epoch FROM mail_run_dependencies WHERE tenant_id=?1 AND run_id=?2 UNION SELECT 'calendar',connection_id,epoch FROM calendar_run_dependencies WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![tenant,run], |r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?.collect::<rusqlite::Result<BTreeSet<_>>>()?)
}
/// Validate exact current source receipts. Failed/unknown calls cannot become evidence;
/// unsupported external context is rejected rather than silently copied into a record.
pub(crate) fn current(
    c: &Connection,
    run: &crate::chat::Run,
) -> Result<Vec<ArtifactExternalEvidence>> {
    let mut out = vec![];
    let mut required = connections(c, &run.tenant, &run.id)?;
    let mut covered = BTreeSet::new();
    for origin in origins(c, run)? {
        let source_connections = connections(c, &run.tenant, &origin)?;
        required.extend(source_connections.iter().cloned());
        // A completed Calendar command/action is an effect receipt, not a read
        // receipt. Never reinterpret its selected answer as persistable evidence.
        let command_context: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2) OR EXISTS(SELECT 1 FROM calendar_agent_action_origins WHERE tenant_id=?1 AND run_id=?2) OR EXISTS(SELECT 1 FROM calendar_agent_action_links WHERE tenant_id=?1 AND run_id=?2)", params![run.tenant,origin], |r|r.get(0))?;
        if command_context {
            return Err(StoreError::Forbidden);
        }
        let source = crate::chat::load_run(c, &run.tenant, &origin)?;
        let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM chat_runs r WHERE r.tenant_id=?1 AND r.id=?2 AND r.account_id=?3 AND r.thread_id=?4 AND r.auth_epoch=?5) AND NOT EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND invalidated=1)",params![run.tenant,origin,run.account,run.thread,run.epoch],|r|r.get(0))?;
        if !valid || !crate::calendar::agent::dependencies_current(c, &source)? {
            return Err(StoreError::Conflict);
        }
        if origin != run.id && source.state != RunState::Completed {
            return Err(StoreError::Conflict);
        }
        if crate::credential_chat::display_only_preview(c, &run.tenant, &origin)?
            || c.query_row("SELECT EXISTS(SELECT 1 FROM credential_chat_waits WHERE tenant_id=?1 AND run_id=?2)", params![run.tenant,origin], |r|r.get::<_,bool>(0))?
        {
            return Err(StoreError::Forbidden);
        }
        let stale_mail:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mail_run_dependencies d LEFT JOIN mail_connections m ON m.tenant_id=d.tenant_id AND m.id=d.connection_id WHERE d.tenant_id=?1 AND d.run_id=?2 AND (m.id IS NULL OR m.account_id!=?3 OR m.status!='active' OR m.epoch!=d.epoch))",params![run.tenant,origin,run.account],|r|r.get(0))?;
        if stale_mail {
            return Err(StoreError::Conflict);
        }
        let rows=c.prepare("SELECT s.step,s.call_id,s.input,s.result,d.state FROM agent_tool_slots s JOIN external_tool_dispatches d USING(tenant_id,run_id,step,call_id) WHERE s.tenant_id=?1 AND s.run_id=?2 ORDER BY s.step,s.call_id")?.query_map(params![run.tenant,origin],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,String>(1)?,r.get::<_,String>(2)?,r.get::<_,Option<String>>(3)?,r.get::<_,String>(4)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        for (step, call_id, input, result, state) in rows {
            let KnowledgeToolInput::External { request } = decode(&input)? else {
                return Err(StoreError::Forbidden);
            };
            let k = supported(&request).ok_or(StoreError::Forbidden)?;
            let Some(result) = result.filter(|_| state == "completed") else {
                return Err(StoreError::Conflict);
            };
            match decode::<KnowledgeToolResult>(&result)? {
                KnowledgeToolResult::Error(_) => continue,
                KnowledgeToolResult::External(_) => {}
                _ => return Err(StoreError::Corrupt),
            }
            let connection = match &request {
                ExternalToolRequest::Mail(q) => Some(q.connection_id().to_owned()),
                ExternalToolRequest::Calendar(q) => {
                    crate::calendar::agent::request_connection(c, &source, q)?
                }
                ExternalToolRequest::Web(_) => None,
            };
            if let Some(connection) = connection {
                let pins = source_connections
                    .iter()
                    .filter(|(kind, id, _)| kind.as_str() == k && id == &connection)
                    .cloned()
                    .collect::<Vec<_>>();
                if pins.is_empty() {
                    return Err(StoreError::Forbidden);
                }
                covered.extend(pins);
            }
            let request_json = encode(&request)?;
            out.push(ArtifactExternalEvidence {
                kind: k.into(),
                run_id: origin.clone(),
                step,
                call_id,
                request_sha256: sha(&request_json),
                request: request_json,
                result_sha256: sha(&result),
            });
            if out.len() > 64 {
                return Err(StoreError::InvalidInput);
            }
        }
    }
    // Inherited connection authority alone is insufficient. Every connection
    // dependency must have an exact successful supported receipt among origins.
    if !required.is_subset(&covered) {
        return Err(StoreError::Forbidden);
    }
    Ok(out)
}
pub(crate) fn evidence(
    c: &Connection,
    t: &str,
    id: &str,
    revision: i64,
) -> Result<Vec<ArtifactExternalEvidence>> {
    Ok(c.prepare("SELECT kind,origin_run,step,call_id,request,request_sha256,result_sha256 FROM artifact_external_evidence WHERE tenant_id=?1 AND artifact_id=?2 AND artifact_revision=?3 ORDER BY origin_run,step,call_id")?.query_map(params![t,id,revision],|r|Ok(ArtifactExternalEvidence{kind:r.get(0)?,run_id:r.get(1)?,step:r.get(2)?,call_id:r.get(3)?,request:r.get(4)?,request_sha256:r.get(5)?,result_sha256:r.get(6)?}))?.collect::<rusqlite::Result<Vec<_>>>()?)
}
/// Common savepoint path also preserves provenance through editor revisions and
/// stored-document derivation. The caller cannot submit external receipt handles.
pub(super) fn save(
    c: &Connection,
    t: &str,
    id: &str,
    revision: i64,
    refs: &BTreeSet<ArtifactDependency>,
    origin: Option<&str>,
) -> Result<()> {
    let mut out = if let Some(run) = origin {
        current(c, &crate::chat::load_run(c, t, run)?)?
    } else {
        vec![]
    };
    for dep in refs
        .iter()
        .filter(|d| d.kind == ArtifactDependencyKind::Artifact)
    {
        out.extend(evidence(c, t, &dep.id, dep.revision)?);
    }
    if revision > 1 {
        out.extend(evidence(c, t, id, revision - 1)?);
    }
    let mut unique = BTreeSet::new();
    for e in out {
        if !unique.insert((e.run_id.clone(), e.step, e.call_id.clone())) {
            continue;
        }
        if unique.len() > 64 {
            return Err(StoreError::InvalidInput);
        }
        c.execute(
            "INSERT INTO artifact_external_evidence VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10)",
            params![
                t,
                id,
                revision,
                e.run_id,
                e.step,
                e.call_id,
                e.kind,
                e.request,
                e.request_sha256,
                e.result_sha256
            ],
        )?;
    }
    Ok(())
}
/// Actual withdrawal retains opaque receipt identity/hashes, never typed query
/// plaintext. Ordinary version editing does not enter this redaction helper.
pub(super) fn redact(c: &Connection, tenant: &str, id: &str, revision: Option<i64>) -> Result<()> {
    c.execute("UPDATE artifact_external_evidence SET request='' WHERE tenant_id=?1 AND artifact_id=?2 AND (?3 IS NULL OR artifact_revision=?3)", params![tenant,id,revision])?;
    Ok(())
}
/// Expand the existing purge fixed point without recursively entering purge_runs.
/// Active version marking makes repeated/cyclic origin edges idempotent.
pub(crate) fn invalidate_runs(
    c: &Connection,
    t: &str,
    mut runs: Vec<String>,
) -> Result<Vec<String>> {
    let mut versions = BTreeSet::new();
    for run in &runs {
        let rows=c.prepare("SELECT DISTINCT e.artifact_id,e.artifact_revision FROM artifact_external_evidence e JOIN personal_artifact_versions v ON v.tenant_id=e.tenant_id AND v.artifact_id=e.artifact_id AND v.revision=e.artifact_revision WHERE e.tenant_id=?1 AND e.origin_run=?2 AND v.status='active' UNION SELECT p.artifact_id,v.revision FROM mission_report_contracts p JOIN personal_artifact_versions v ON v.tenant_id=p.tenant_id AND v.artifact_id=p.artifact_id WHERE p.tenant_id=?1 AND p.run_id=?2 AND v.status='active'")?.query_map(params![t,run],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?;
        versions.extend(rows);
    }
    let mut pending = versions.into_iter().collect::<Vec<_>>();
    while let Some((id, revision)) = pending.pop() {
        let changed=c.execute("UPDATE personal_artifact_versions SET status='withdrawn',draft=NULL WHERE tenant_id=?1 AND artifact_id=?2 AND revision=?3 AND status='active'",params![t,id,revision])?;
        if changed == 0 {
            continue;
        }
        redact(c, t, &id, Some(revision))?;
        retire_writers(c, t, &id, Some(revision), None)?;
        c.execute("UPDATE personal_artifacts SET status='withdrawn',updated_at=?4 WHERE tenant_id=?1 AND id=?2 AND revision=?3",params![t,id,revision,now()?])?;
        crate::schedules::followups::retire(c, t, &id, Some(revision), now()?)?;
        runs.extend(c.prepare("SELECT run_id FROM personal_artifact_run_dependencies WHERE tenant_id=?1 AND artifact_id=?2 AND revision=?3")?.query_map(params![t,id,revision],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?);
        pending.extend(c.prepare("SELECT DISTINCT d.artifact_id,d.artifact_revision FROM personal_artifact_dependencies d JOIN personal_artifact_versions v ON v.tenant_id=d.tenant_id AND v.artifact_id=d.artifact_id AND v.revision=d.artifact_revision WHERE d.tenant_id=?1 AND d.kind='artifact' AND d.object_id=?2 AND d.revision=?3 AND v.status='active'")?.query_map(params![t,id,revision],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?)))?.collect::<rusqlite::Result<Vec<_>>>()?);
    }
    let selected = runs.clone();
    for run in selected {
        runs.extend(
            c.prepare(
                "SELECT run_id FROM artifact_external_context WHERE tenant_id=?1 AND origin_run=?2",
            )?
            .query_map(params![t, run], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?,
        );
    }
    Ok(runs
        .into_iter()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect())
}
/// An artifact version change retires only the terminal write projection. The
/// independent admitted read slots remain valid sources until their own revoke.
/// This avoids a source→document→source cycle on ordinary owner edits.
pub(super) fn retire_writers(
    c: &Connection,
    t: &str,
    id: &str,
    revision: Option<i64>,
    except: Option<&str>,
) -> Result<()> {
    let writers=c.prepare("SELECT run_id FROM artifact_external_profiles WHERE tenant_id=?1 AND artifact_id=?2 AND (?3 IS NULL OR artifact_revision=?3)")?.query_map(params![t,id,revision],|r|r.get::<_,String>(0))?.collect::<rusqlite::Result<Vec<_>>>()?;
    for run in writers.into_iter().filter(|r| Some(r.as_str()) != except) {
        c.execute(
            "DELETE FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
            params![t, run],
        )?;
        c.execute("DELETE FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND json_extract(input,'$.tool')='artifact' AND json_extract(input,'$.request.operation')='write'",params![t,run])?;
        c.execute("UPDATE agent_steps SET checkpoint=NULL,completion_digest=NULL WHERE tenant_id=?1 AND run_id=?2",params![t,run])?;
        c.execute(
            "DELETE FROM personal_artifact_command_plans WHERE tenant_id=?1 AND run_id=?2",
            params![t, run],
        )?;
        let thread: String = c.query_row(
            "SELECT thread_id FROM chat_runs WHERE tenant_id=?1 AND id=?2",
            params![t, run],
            |r| r.get(0),
        )?;
        c.execute(
            "DELETE FROM chat_events WHERE tenant_id=?1 AND thread_id=?2",
            params![t, thread],
        )?;
        c.execute("UPDATE threads SET chat_event_seq=chat_event_seq+1,chat_reset_floor=chat_event_seq+1 WHERE tenant_id=?1 AND id=?2",params![t,thread])?;
    }
    Ok(())
}
pub(crate) fn delete_thread(c: &Connection, t: &str, thread: &str) -> Result<()> {
    let runs = c
        .prepare("SELECT id FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2")?
        .query_map(params![t, thread], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    agent_execution::purge_runs(c, t, runs)
}
pub(crate) fn quarantine_restore(c: &Connection) -> Result<()> {
    let tenants = c
        .prepare("SELECT DISTINCT tenant_id FROM artifact_external_evidence UNION SELECT tenant_id FROM mission_report_contracts WHERE artifact_id IS NOT NULL")?
        .query_map([], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    for t in tenants {
        let runs = c
            .prepare(
                "SELECT DISTINCT origin_run FROM artifact_external_evidence WHERE tenant_id=?1 UNION SELECT run_id FROM mission_report_contracts WHERE tenant_id=?1 AND artifact_id IS NOT NULL",
            )?
            .query_map([&t], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        // Withdraw every retained external version, including any source whose
        // conversation has already been removed, before filtering live run IDs.
        let expanded = invalidate_runs(c, &t, runs)?;
        let mut runs = vec![];
        for id in expanded {
            if c.query_row(
                "SELECT EXISTS(SELECT 1 FROM chat_runs WHERE tenant_id=?1 AND id=?2)",
                params![t, id],
                |r| r.get::<_, bool>(0),
            )? {
                runs.push(id);
            }
        }
        agent_execution::purge_runs(c, &t, runs)?;
    }
    c.execute("DELETE FROM artifact_external_profiles", [])?;
    Ok(())
}
