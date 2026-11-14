//! Chat-only frozen grants, opaque observations and sealed current-user commands. No IO.
use super::*;
use crate::{
    agent_execution::{self, StepLease},
    chat::Run,
};
use chrono::{Days, NaiveDate, NaiveTime, TimeZone};
pub struct CalendarAgentCommand {
    pub user: UserContext,
    pub profile: CalendarRunProfile,
    pub decision: CalendarChatDecision,
    pub action_id: String,
    step: StepLease,
}
pub(crate) fn profile(c: &Connection, t: &str, r: &str) -> Result<Option<CalendarRunProfile>> {
    c.query_row(
        "SELECT profile FROM calendar_agent_profiles WHERE tenant_id=?1 AND run_id=?2",
        params![t, r],
        |r| r.get::<_, String>(0),
    )
    .optional()?
    .map(|v| parse(&v))
    .transpose()
}
pub(crate) fn admit(c: &Connection, r: &Run, zone: Option<&str>) -> Result<()> {
    if r.tool_profile.is_none()
        || crate::media::has_context(c, &r.tenant, &r.id)?
        || crate::missions::is_mission(c, &r.tenant, &r.id)?
    {
        return Ok(());
    }
    let connections=c.prepare("SELECT id,title,epoch,client_id,(SELECT revision FROM google_setup WHERE singleton=1) FROM calendar_connections WHERE tenant_id=?1 AND account_id=?2 AND auth_epoch=?3 AND status='active' ORDER BY id LIMIT 9")?.query_map(params![r.tenant,r.account,r.epoch],|r|Ok(CalendarAgentPin{id:r.get(0)?,title:r.get(1)?,epoch:r.get(2)?,client_id:r.get(3)?,app_revision:r.get(4)?}))?.collect::<std::result::Result<Vec<_>,_>>()?;
    if connections.len() > 8 {
        return Ok(());
    }
    let timezone = zone
        .map(|s| {
            s.parse::<chrono_tz::Tz>()
                .map(|z| z.to_string())
                .map_err(|_| StoreError::InvalidInput)
        })
        .transpose()?;
    let p = CalendarRunProfile {
        version: 1,
        reference: now()?,
        timezone,
        connections,
    };
    c.execute(
        "INSERT INTO calendar_agent_profiles VALUES(?1,?2,?3,1,?4)",
        params![r.tenant, r.id, r.account, json(&p)?],
    )?;
    Ok(())
}
fn pin_current(c: &Connection, r: &Run, p: &CalendarAgentPin) -> Result<bool> {
    let revision: Option<i64> = c
        .query_row(
            "SELECT revision FROM google_setup WHERE singleton=1",
            [],
            |r| r.get(0),
        )
        .optional()?;
    Ok(revision==p.app_revision && c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_connections WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND epoch=?4 AND status='active' AND client_id=?5)",params![r.tenant,r.account,p.id,p.epoch,p.client_id],|r|r.get::<_,bool>(0))?)
}
pub(crate) fn dependencies_current(c: &Connection, r: &Run) -> Result<bool> {
    let pins=c.prepare("SELECT connection_id,epoch,client_id,app_revision FROM calendar_run_dependencies WHERE tenant_id=?1 AND run_id=?2")?.query_map(params![r.tenant,r.id],|r|Ok(CalendarAgentPin{id:r.get(0)?,epoch:r.get(1)?,client_id:r.get(2)?,app_revision:r.get(3)?,title:String::new()}))?.collect::<std::result::Result<Vec<_>,_>>()?;
    for p in pins {
        if !pin_current(c, r, &p)? {
            return Ok(false);
        }
    }
    Ok(true)
}
pub(crate) fn persistence_context(c: &Connection, r: &Run) -> Result<bool> {
    Ok(c.query_row(
        "SELECT EXISTS(SELECT 1 FROM calendar_run_dependencies WHERE tenant_id=?1 AND run_id=?2)",
        params![r.tenant, r.id],
        |r| r.get(0),
    )?)
}
pub(crate) fn purge(c: &Connection, t: &str, r: &str) -> Result<()> {
    c.execute("UPDATE calendar_actions SET payload=NULL,event=NULL,state=CASE WHEN state='dispatch_committed' THEN 'outcome_unknown' ELSE state END,version=version+1 WHERE tenant_id=?1 AND id IN(SELECT action_id FROM calendar_agent_action_origins WHERE tenant_id=?1 AND run_id=?2)",params![t,r])?;
    for table in [
        "calendar_agent_profiles",
        "calendar_agent_observations",
        "calendar_command_plans",
    ] {
        c.execute(
            &format!("DELETE FROM {table} WHERE tenant_id=?1 AND run_id=?2"),
            params![t, r],
        )?;
    }
    c.execute("UPDATE mission_chat_plans SET decision=NULL,state='quarantined' WHERE tenant_id=?1 AND run_id=?2 AND json_extract(decision,'$.domain')='calendar'",params![t,r])?;
    Ok(())
}
/// Called only inside active-run cancellation's writer transaction. Ordinary
/// read/chat profiles remain usable on Retry, but the current Calendar command
/// and private effect result are permanently retired independently of run.state.
pub(crate) fn cancel(c: &Connection, r: &Run) -> Result<()> {
    let owns_calendar:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2) OR EXISTS(SELECT 1 FROM calendar_agent_action_origins WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;
    if !owns_calendar {
        return Ok(());
    }
    c.execute(
        "UPDATE calendar_agent_action_origins SET withdrawn=1 WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
    )?;
    // Preserve exact UUID, nonce, state, versioned minimum receipt and linkage.
    // A committed provider call is not undone and must still settle privately.
    c.execute("UPDATE calendar_actions SET payload=NULL,event=NULL,version=version+1 WHERE tenant_id=?1 AND id IN(SELECT action_id FROM calendar_agent_action_origins WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id])?;
    c.execute(
        "DELETE FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
    )?;
    c.execute(
        "DELETE FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
    )?;
    // 'passed' is a durable no-reclassification decision. Removing the private
    // Calendar proposal must never restore the classifier's awaiting state.
    c.execute("UPDATE mission_chat_plans SET decision=NULL,state='passed' WHERE tenant_id=?1 AND run_id=?2 AND json_extract(decision,'$.domain')='calendar'",params![r.tenant,r.id])?;
    c.execute(
        "UPDATE agent_steps SET checkpoint=NULL WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
    )?;
    c.execute(
        "DELETE FROM chat_output WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
    )?;
    // Drop any already-delivered private delta; retained completed turns are
    // still in the snapshot, while clients must resnapshot this canceled run.
    c.execute("UPDATE threads SET chat_event_seq=chat_event_seq+1,chat_reset_floor=chat_event_seq+1 WHERE tenant_id=?1 AND id=?2",params![r.tenant,r.thread])?;
    c.execute(
        "DELETE FROM chat_events WHERE tenant_id=?1 AND thread_id=?2",
        params![r.tenant, r.thread],
    )?;
    Ok(())
}
/// A personal model disconnect retires unfinished Calendar commands, including
/// retryable attempts. Completed history belongs to the user and remains intact.
pub(crate) fn invalidate_subscription(
    c: &Connection,
    tenant: &str,
    account: &str,
    binding: &str,
    revision: i64,
) -> Result<()> {
    let runs = c.prepare("SELECT r.id FROM chat_runs r JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE r.tenant_id=?1 AND r.account_id=?2 AND r.state!='completed' AND e.invalidated=0 AND json_extract(r.model,'$.source')='personal_chat_gpt' AND json_extract(r.model,'$.provider_id')=?3 AND json_extract(r.model,'$.provider_revision')=?4 AND (EXISTS(SELECT 1 FROM calendar_command_plans p WHERE p.tenant_id=r.tenant_id AND p.run_id=r.id) OR EXISTS(SELECT 1 FROM calendar_agent_action_origins o WHERE o.tenant_id=r.tenant_id AND o.run_id=r.id))")?
        .query_map(params![tenant, account, binding, revision], |r| r.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    for run in &runs {
        c.execute(
            "UPDATE calendar_agent_action_origins SET withdrawn=1 WHERE tenant_id=?1 AND run_id=?2",
            params![tenant, run],
        )?;
    }
    agent_execution::purge_runs(c, tenant, runs)
}
pub(crate) fn action_private_current(c: &Connection, t: &str, id: &str) -> Result<bool> {
    let current: bool = c.query_row("SELECT NOT EXISTS(SELECT 1 FROM calendar_agent_action_origins WHERE tenant_id=?1 AND action_id=?2) OR EXISTS(SELECT 1 FROM calendar_agent_action_origins o JOIN calendar_agent_profiles p ON p.tenant_id=o.tenant_id AND p.run_id=o.run_id JOIN chat_runs r ON r.tenant_id=p.tenant_id AND r.id=p.run_id JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE o.tenant_id=?1 AND o.action_id=?2 AND o.withdrawn=0 AND e.invalidated=0 AND r.state IN('running','completed'))",params![t,id],|r|r.get(0))?;
    if !current {
        return Ok(false);
    }
    // Fence late in-flight settlement even when personal authority disappears
    // through another lifecycle path. Do not hide already-completed history.
    let running: Option<String> = c.query_row("SELECT r.id FROM calendar_agent_action_origins o JOIN chat_runs r ON r.tenant_id=o.tenant_id AND r.id=o.run_id WHERE o.tenant_id=?1 AND o.action_id=?2 AND r.state='running'",params![t,id],|r|r.get(0)).optional()?;
    if let Some(id) = running {
        let r = crate::chat::load_run(c, t, &id)?;
        if r.model.source == ModelSource::PersonalChatGpt {
            return Ok(crate::subscriptions::binding(c, &r.tenant, &r.account, &r.model).is_ok());
        }
    }
    Ok(true)
}
pub(crate) fn purge_thread(c: &Connection, t: &str, thread: &str) -> Result<()> {
    c.execute("UPDATE calendar_actions SET payload=NULL,event=NULL,state=CASE WHEN state='dispatch_committed' THEN 'outcome_unknown' ELSE state END,version=version+1 WHERE tenant_id=?1 AND id IN(SELECT o.action_id FROM calendar_agent_action_origins o JOIN chat_runs r ON r.tenant_id=o.tenant_id AND r.id=o.run_id WHERE r.tenant_id=?1 AND r.thread_id=?2)",params![t,thread])?;
    Ok(())
}
pub(crate) fn quarantine_restore(c: &Connection) -> Result<()> {
    let rows=c.prepare("SELECT tenant_id,run_id FROM calendar_run_dependencies UNION SELECT tenant_id,run_id FROM calendar_command_plans")?.query_map([],|r|Ok((r.get::<_,String>(0)?,r.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    let mut tenants = std::collections::BTreeMap::<String, Vec<String>>::new();
    for (t, r) in rows {
        tenants.entry(t).or_default().push(r);
    }
    for (t, runs) in tenants {
        agent_execution::purge_runs(c, &t, runs)?;
    }
    c.execute_batch("DELETE FROM calendar_agent_observations; DELETE FROM calendar_command_plans; DELETE FROM calendar_agent_profiles; DELETE FROM calendar_run_dependencies;")?;
    Ok(())
}
pub(crate) fn invalidate(c: &Connection, t: &str, id: &str) -> Result<()> {
    let runs=c.prepare("SELECT DISTINCT run_id FROM calendar_run_dependencies WHERE tenant_id=?1 AND connection_id=?2 UNION SELECT p.run_id FROM calendar_agent_profiles p JOIN calendar_command_plans q USING(tenant_id,run_id) WHERE p.tenant_id=?1 AND EXISTS(SELECT 1 FROM json_each(profile,'$.connections') WHERE json_extract(value,'$.id')=?2)")?.query_map(params![t,id],|r|r.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?;
    agent_execution::purge_runs(c, t, runs)?;
    c.execute("DELETE FROM calendar_agent_profiles WHERE tenant_id=?1 AND EXISTS(SELECT 1 FROM json_each(profile,'$.connections') WHERE json_extract(value,'$.id')=?2)",params![t,id])?;
    Ok(())
}
fn admit_pin(c: &Connection, r: &Run, p: &CalendarAgentPin) -> Result<()> {
    if !pin_current(c, r, p)?
        || crate::media::has_context(c, &r.tenant, &r.id)?
        || crate::missions::is_mission(c, &r.tenant, &r.id)?
    {
        return Err(StoreError::Forbidden);
    }
    c.execute(
        "INSERT OR IGNORE INTO calendar_run_dependencies VALUES(?1,?2,?3,?4,?5,?6)",
        params![r.tenant, r.id, p.id, p.epoch, p.app_revision, p.client_id],
    )?;
    Ok(())
}
fn request_connection(c: &Connection, r: &Run, q: &CalendarReadRequest) -> Result<Option<String>> {
    match q {
        CalendarReadRequest::List{range}=>{
            moosuri_connectors::calendar::validate_range(&range.start,&range.end).map_err(|_|StoreError::InvalidInput)?;
            if let Some(token)=&range.page_token {
                let prior:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2 AND event_id IS NULL AND json_extract(request,'$.connection_id')=?3 AND json_extract(request,'$.start')=?4 AND json_extract(request,'$.end')=?5 AND json_extract(request,'$.page_token')=?6)",params![r.tenant,r.id,range.connection_id,range.start,range.end,token],|r|r.get(0))?; if !prior {return Ok(None)}
            } Ok(Some(range.connection_id.clone()))
        },
        CalendarReadRequest::Read{observation_ref}=>Ok(c.query_row("SELECT connection_id FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2 AND handle=?3 AND event_id IS NOT NULL",params![r.tenant,r.id,observation_ref],|r|r.get::<_,String>(0)).optional()?),
        CalendarReadRequest::Status{action_id}=>Ok(c.query_row("SELECT a.connection_id FROM calendar_agent_action_links l JOIN chat_runs old ON old.tenant_id=l.tenant_id AND old.id=l.run_id JOIN calendar_actions a ON a.tenant_id=l.tenant_id AND a.id=l.action_id WHERE l.tenant_id=?1 AND old.account_id=?2 AND old.thread_id=?3 AND l.action_id=?4",params![r.tenant,r.account,r.thread,action_id],|r|r.get::<_,String>(0)).optional()?),
    }
}
pub(crate) fn allowed(c: &Connection, r: &Run, q: &CalendarReadRequest) -> Result<bool> {
    let Some(p) = profile(c, &r.tenant, &r.id)? else {
        return Ok(false);
    };
    let Some(id) = request_connection(c, r, q)? else {
        return Ok(false);
    };
    match p.connections.iter().find(|p| p.id == id) {
        Some(pin) => Ok(pin_current(c, r, pin)?
            && !crate::media::has_context(c, &r.tenant, &r.id)?
            && !crate::missions::is_mission(c, &r.tenant, &r.id)?),
        None => Ok(false),
    }
}
pub(crate) fn admit_read(c: &Connection, r: &Run, q: &CalendarReadRequest) -> Result<()> {
    if !allowed(c, r, q)? {
        return Err(StoreError::Forbidden);
    }
    let p = profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
    let id = request_connection(c, r, q)?.ok_or(StoreError::Forbidden)?;
    admit_pin(
        c,
        r,
        p.connections
            .iter()
            .find(|p| p.id == id)
            .ok_or(StoreError::Forbidden)?,
    )
}
fn exact(text: &str, q: &Option<String>, max: usize, required: bool) -> Result<()> {
    match q {
        Some(q) if !q.trim().is_empty() && q.len() <= max && text.contains(q) => Ok(()),
        None if !required => Ok(()),
        _ => Err(StoreError::InvalidInput),
    }
}
fn validate_decision(text: &str, d: &CalendarChatDecision) -> Result<()> {
    if d.domain != "calendar"
        || !matches!(d.decision.as_str(), "command" | "clarify" | "unsupported")
        || json(d)?.len() > 16384
    {
        return Err(StoreError::InvalidInput);
    }
    exact(text, &d.intent_quote, 2048, d.decision == "command")?;
    for (q, max) in [
        (&d.connection_quote, 256),
        (&d.title_quote, 512),
        (&d.target_quote, 512),
        (&d.date_quote, 128),
        (&d.start_quote, 256),
        (&d.end_quote, 256),
        (&d.duration_quote, 128),
    ] {
        exact(text, q, max, false)?;
    }
    if d.decision != "command" {
        if d.operation.is_some()
            || d.connection_quote.is_some()
            || d.title_quote.is_some()
            || d.target_quote.is_some()
            || d.date_quote.is_some()
            || d.date.is_some()
            || d.start_quote.is_some()
            || d.start.is_some()
            || d.end_quote.is_some()
            || d.end.is_some()
            || d.duration_quote.is_some()
            || d.duration_minutes.is_some()
        {
            return Err(StoreError::InvalidInput);
        }
        return Ok(());
    }
    if !matches!(d.operation.as_deref(), Some("create" | "update" | "delete"))
        || d.clarification.is_some()
    {
        return Err(StoreError::InvalidInput);
    }
    exact(text, &d.date_quote, 128, true)?;
    if d.operation.as_deref() == Some("create") {
        exact(text, &d.title_quote, 512, true)?;
    } else {
        exact(text, &d.target_quote, 512, true)?;
    }
    if d.start.is_some() != d.start_quote.is_some()
        || d.end.is_some() != d.end_quote.is_some()
        || d.duration_minutes.is_some() != d.duration_quote.is_some()
        || d.end.is_some() && d.duration_minutes.is_some()
    {
        return Err(StoreError::InvalidInput);
    }
    if d.operation.as_deref() == Some("delete")
        && (d.title_quote.is_some()
            || d.start.is_some()
            || d.end.is_some()
            || d.duration_minutes.is_some())
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}
fn selected<'a>(
    p: &'a CalendarRunProfile,
    d: &CalendarChatDecision,
) -> Result<&'a CalendarAgentPin> {
    let matches = p
        .connections
        .iter()
        .filter(|p| {
            d.connection_quote
                .as_ref()
                .is_none_or(|q| q == &p.id || q == &p.title)
        })
        .collect::<Vec<_>>();
    if matches.len() != 1 {
        return Err(StoreError::InvalidInput);
    }
    Ok(matches[0])
}
fn time_proposal(
    _d: &CalendarChatDecision,
    time: MissionChatTime,
    q: String,
) -> MissionChatDecision {
    MissionChatDecision {
        decision: "command".into(),
        action: None,
        target_kind: None,
        intent_quote: None,
        task_quote: None,
        constraints_quote: None,
        time_quote: Some(q),
        target_quote: None,
        instruction_quote: None,
        time: Some(time),
        clarification: None,
    }
}
fn zone(p: &CalendarRunProfile, d: &CalendarChatDecision) -> Result<chrono_tz::Tz> {
    crate::missions::chat::source_zone(
        d.date_quote.as_deref().unwrap_or(""),
        p.timezone.as_deref(),
    )?
    .ok_or(StoreError::InvalidInput)?
    .parse()
    .map_err(|_| StoreError::InvalidInput)
}
fn moment(
    p: &CalendarRunProfile,
    d: &CalendarChatDecision,
    time: &str,
    q: &str,
) -> Result<chrono::DateTime<chrono::Utc>> {
    let zone = zone(p, d)?;
    let proposal = time_proposal(
        d,
        MissionChatTime::LocalOnce {
            date: d.date.clone().ok_or(StoreError::InvalidInput)?,
            time: time.into(),
        },
        format!("{} {q}", d.date_quote.as_deref().unwrap_or("")),
    );
    if !crate::missions::chat::civil_consistent(&proposal, p.reference, &zone.to_string()) {
        return Err(StoreError::InvalidInput);
    }
    let base = chrono::DateTime::from_timestamp(p.reference, 0)
        .ok_or(StoreError::Corrupt)?
        .with_timezone(&zone)
        .date_naive();
    let date = match d.date.as_deref() {
        Some("today") => base,
        Some("tomorrow") => base
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?,
        Some(v) => {
            NaiveDate::parse_from_str(v, "%Y-%m-%d").map_err(|_| StoreError::InvalidInput)?
        }
        None => return Err(StoreError::InvalidInput),
    };
    let time = NaiveTime::parse_from_str(time, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    Ok(zone
        .from_local_datetime(&date.and_time(time))
        .single()
        .ok_or(StoreError::InvalidInput)?
        .with_timezone(&chrono::Utc))
}
pub(crate) fn range(p: &CalendarRunProfile, d: &CalendarChatDecision) -> Result<CalendarRange> {
    let start = moment(p, d, "00:00", "00:00")?;
    let zone = zone(p, d)?;
    let next = start
        .with_timezone(&zone)
        .date_naive()
        .checked_add_days(Days::new(1))
        .ok_or(StoreError::InvalidInput)?;
    let end = zone
        .from_local_datetime(&next.and_hms_opt(0, 0, 0).ok_or(StoreError::InvalidInput)?)
        .single()
        .ok_or(StoreError::InvalidInput)?
        .with_timezone(&chrono::Utc);
    Ok(CalendarRange {
        connection_id: selected(p, d)?.id.clone(),
        start: start.to_rfc3339(),
        end: end.to_rfc3339(),
        page_token: None,
    })
}
fn end_quote(text: &str, d: &CalendarChatDecision) -> Result<String> {
    let end = d.end_quote.as_deref().ok_or(StoreError::InvalidInput)?;
    let start = d.start_quote.as_deref().unwrap_or("");
    let compact = |s: &str| s.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    if !end.contains("오전")
        && !end.contains("오후")
        && end.contains('시')
        && compact(text).contains(&format!("{}부터{}", compact(start), compact(end)))
    {
        if start.contains("오전") && !start.contains("오후") {
            return Ok(format!("오전{end}"));
        }
        if start.contains("오후") && !start.contains("오전") {
            return Ok(format!("오후{end}"));
        }
    }
    Ok(end.into())
}
fn fields(
    p: &CalendarRunProfile,
    d: &CalendarChatDecision,
    prior: Option<&CalendarEvent>,
    text: &str,
) -> Result<CalendarFields> {
    let mut f = prior.map(|v| v.fields.clone()).unwrap_or(CalendarFields {
        title: String::new(),
        description: String::new(),
        location: String::new(),
        start: String::new(),
        end: String::new(),
        time_zone: Some(zone(p, d)?.to_string()),
    });
    if let Some(title) = &d.title_quote {
        f.title = title.clone();
    }
    if let Some(s) = &d.start {
        f.start = moment(
            p,
            d,
            s,
            d.start_quote.as_deref().ok_or(StoreError::InvalidInput)?,
        )?
        .to_rfc3339();
    }
    if let Some(e) = &d.end {
        f.end = moment(p, d, e, &end_quote(text, d)?)?.to_rfc3339();
    }
    if let Some(minutes) = d.duration_minutes {
        if minutes == 0 || minutes > 24 * 60 {
            return Err(StoreError::InvalidInput);
        }
        let proposal = time_proposal(
            d,
            MissionChatTime::Relative {
                seconds: minutes * 60,
            },
            d.duration_quote.clone().ok_or(StoreError::InvalidInput)?,
        );
        if !crate::missions::chat::civil_consistent(
            &proposal,
            p.reference,
            p.timezone.as_deref().unwrap_or("UTC"),
        ) {
            return Err(StoreError::InvalidInput);
        }
        let start =
            chrono::DateTime::parse_from_rfc3339(&f.start).map_err(|_| StoreError::InvalidInput)?;
        f.end = (start + chrono::Duration::minutes(i64::from(minutes))).to_rfc3339();
    }
    if d.start.is_some() && d.end.is_none() && d.duration_minutes.is_none() {
        return Err(StoreError::InvalidInput);
    }
    if prior.is_none() && (d.start.is_none() || d.end.is_none() && d.duration_minutes.is_none()) {
        return Err(StoreError::InvalidInput);
    }
    if prior.is_some()
        && d.title_quote.is_none()
        && d.start.is_none()
        && d.end.is_none()
        && d.duration_minutes.is_none()
    {
        return Err(StoreError::InvalidInput);
    }
    moosuri_connectors::calendar::validate_fields(&f).map_err(|_| StoreError::InvalidInput)?;
    Ok(f)
}
pub(crate) fn record(c: &Connection, r: &Run, raw: serde_json::Value) -> Result<()> {
    let d: CalendarChatDecision =
        serde_json::from_value(raw).map_err(|_| StoreError::InvalidInput)?;
    validate_decision(&r.text, &d)?;
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mission_chat_plans p JOIN agent_execution e USING(tenant_id,run_id) WHERE p.tenant_id=?1 AND p.run_id=?2 AND p.state='awaiting' AND e.remote_calls=1 AND e.tool_calls=0) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;
    if !valid
        || profile(c, &r.tenant, &r.id)?.is_none()
        || crate::media::has_context(c, &r.tenant, &r.id)?
    {
        return Err(StoreError::Conflict);
    }
    // Freeze an explicit current-message IANA zone in the same acknowledged
    // transaction; invalid zone metadata will clarify before provider IO.
    if d.decision == "command" {
        let mut p = profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
        p.timezone =
            crate::missions::chat::source_zone(&r.text, p.timezone.as_deref()).unwrap_or(None);
        c.execute(
            "UPDATE calendar_agent_profiles SET profile=?3 WHERE tenant_id=?1 AND run_id=?2",
            params![r.tenant, r.id, json(&p)?],
        )?;
    }
    c.execute(
        "INSERT INTO calendar_command_plans VALUES(?1,?2,?3,?4,'sealed',?5,NULL,1)",
        params![
            r.tenant,
            r.id,
            Uuid::new_v4().to_string(),
            json(&d)?,
            now()? + 120
        ],
    )?;
    c.execute(
        "UPDATE mission_chat_plans SET state='passed',decision=?3 WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id, json(&d)?],
    )?;
    Ok(())
}
fn original(c: &Connection, r: &Run) -> Result<UserContext> {
    let(session,epoch):(Vec<u8>,i64)=c.query_row("SELECT original_session,original_epoch FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?)))?;
    let u = load_context(
        c,
        session.try_into().map_err(|_| StoreError::Corrupt)?,
        false,
    )?;
    if u.epoch != epoch
        || r.epoch != epoch
        || u.tenant.0 != r.tenant
        || u.account.id != r.account
        || u.restricted
    {
        return Err(StoreError::Unauthorized);
    }
    Ok(u)
}
pub(crate) fn retry_authority(c: &Connection, r: &Run) -> Result<()> {
    let pending:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2 AND state='sealed')",params![r.tenant,r.id],|r|r.get(0))?;
    if pending {
        original(c, r)?;
    }
    Ok(())
}
fn command(
    c: &Connection,
    key: &[u8; 32],
    l: &crate::RunLease,
) -> Result<Option<CalendarAgentCommand>> {
    let r = crate::chat::guard(c, key, l)?;
    let row:Option<(String,String,i64,i64)>=c.query_row("SELECT action_id,decision,expires_at,step FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2 AND state='sealed'",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((action_id, raw, expiry, source_step)) = row else {
        return Ok(None);
    };
    if now()? > expiry {
        return Err(StoreError::Conflict);
    }
    let p = profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
    let user = original(c, &r)?;
    // Retry may rebind the run lease, never the immutable completed decision step.
    // The original authenticated session is independently checked above.
    let nonce:Vec<u8>=c.query_row("SELECT s.nonce FROM agent_steps s JOIN agent_execution e USING(tenant_id,run_id) WHERE s.tenant_id=?1 AND s.run_id=?2 AND s.step=?3 AND s.state IN('completed','purged') AND e.remote_calls=?3 AND e.tool_calls=0 AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id,source_step],|r|r.get(0))?;
    let step = StepLease {
        run: l.clone(),
        step: source_step,
        nonce: nonce.try_into().map_err(|_| StoreError::Corrupt)?,
    };
    Ok(Some(CalendarAgentCommand {
        user,
        profile: p,
        decision: parse(&raw)?,
        action_id,
        step,
    }))
}
pub(crate) fn receipt(c: &Connection, r: &Run) -> Result<Option<CalendarChatReceipt>> {
    let id: Option<String> = c
        .query_row(
            "SELECT action_id FROM calendar_agent_action_links WHERE tenant_id=?1 AND run_id=?2",
            params![r.tenant, r.id],
            |r| r.get(0),
        )
        .optional()?;
    if id.is_none() {
        let saved:Option<String>=c.query_row("SELECT receipt FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2 AND receipt IS NOT NULL",params![r.tenant,r.id],|r|r.get(0)).optional()?;
        if let Some(v) = saved {
            return parse(&v).map(Some);
        }
        return Ok(None);
    }
    id.map(|id|{let(state,event_id,etag,operation):(String,String,Option<String>,String)=c.query_row("SELECT state,event_id,provider_etag,kind FROM calendar_actions WHERE tenant_id=?1 AND id=?2",params![r.tenant,id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?)))?; Ok(CalendarChatReceipt{action_id:Some(id),operation,state:state.clone(),event_id:Some(event_id),provider_etag:etag,message:message(&state).into()})}).transpose()
}
fn message(state: &str) -> &'static str {
    match state {
        "connection_needed" => {
            "Calendar 연결이 필요해요. /settings/calendar 에서 내 Google Calendar를 연결한 뒤 새 요청을 보내 주세요. 아직 변경하지 않았어요. / Connect your Calendar in settings and send a new request. No change was made."
        }
        "applied" => "일정 변경을 적용했어요. / Calendar change applied.",
        "conflict" => "일정이 변경되어 적용하지 않았어요. / Event changed; no update was applied.",
        "failed_definite" => "일정 변경이 거절되었어요. / Calendar change was rejected.",
        "outcome_unknown" | "dispatch_committed" => {
            "일정 변경 결과가 불명확해요. 같은 작업을 다시 보내지 않고 결과만 조회할 수 있어요. / Outcome unknown; lookup only, no resend."
        }
        "quarantined" => {
            "복원된 일정 작업은 격리되었어요. 이전 효과를 다시 보내지 않아요. / Restored action quarantined; no automatic replay."
        }
        "observed_absent" => {
            "현재 일정이 없음을 확인했어요. 이전 삭제의 성공 여부는 확정할 수 없어요. / Event currently absent; deletion causality unknown."
        }
        "unsupported" => {
            "반복·종일·참석자·예약 실행은 아직 지원하지 않아요. / Recurrence, all-day, attendees and scheduled execution are unsupported."
        }
        _ => {
            "한 개의 캘린더와 일정 제목·날짜·명확한 시작 및 종료 시각(또는 기간)을 말씀해 주세요. / Specify one calendar, exact event title/date, and explicit start/end or duration. No change was made."
        }
    }
}
fn finish(c: &Connection, r: &Run, receipt: CalendarChatReceipt) -> Result<()> {
    c.execute("UPDATE calendar_command_plans SET state='finished',receipt=?3 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id,json(&receipt)?])?;
    crate::chat::apply_patches(
        c,
        r,
        vec![crate::OutputPatch::Append {
            item: "calendar-receipt".into(),
            text: receipt.message,
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    crate::chat::transition(c, r, RunState::Completed, None)
}
impl Store {
    pub async fn calendar_agent_command(
        &self,
        l: &crate::RunLease,
    ) -> Result<Option<CalendarAgentCommand>> {
        let l = l.clone();
        self.call(move |db| {
            let tx = db.conn.transaction()?;
            let v = command(&tx, &db.provider_key, &l)?;
            tx.commit()?;
            Ok(v)
        })
        .await
    }
    pub async fn calendar_agent_range(&self, l: &crate::RunLease) -> Result<CalendarRange> {
        let l = l.clone();
        self.call(move |db| {
            let v = command(&db.conn, &db.provider_key, &l)?.ok_or(StoreError::Conflict)?;
            range(&v.profile, &v.decision)
        })
        .await
    }
    pub async fn admit_calendar_agent_change(
        &self,
        l: &crate::RunLease,
        b: &CalendarBinding,
        primary: CalendarInfo,
        prior: Option<CalendarEvent>,
    ) -> Result<CalendarAdmission> {
        let l = l.clone();
        let u = b.user.clone();
        let conn = b.connection.clone();
        let app = b.app.clone();
        self.call(move|db|{let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;let cmd=command(&tx,&db.provider_key,&l)?.ok_or(StoreError::Conflict)?;let r=crate::chat::guard(&tx,&db.provider_key,&l)?;
            let d=&cmd.decision;validate_decision(&r.text,d)?;let p=selected(&cmd.profile,d)?;
            if p.id!=conn.id || p.epoch!=conn.epoch || p.client_id!=app.client_id || p.app_revision!=app.revision || cmd.user.digest!=u.digest || cmd.user.epoch!=u.epoch {return Err(StoreError::Forbidden)} admit_pin(&tx,&r,p)?;
            let mutation=match d.operation.as_deref(){Some("create")=>CalendarMutation::Create{fields:fields(&cmd.profile,d,None,&r.text)?},Some("update"|"delete")=>{let old=prior.as_ref().ok_or(StoreError::InvalidInput)?;if !old.writable || d.target_quote.as_ref()!=Some(&old.fields.title){return Err(StoreError::Conflict)}let q=range(&cmd.profile,d)?;let start=chrono::DateTime::parse_from_rfc3339(&old.fields.start).map_err(|_|StoreError::InvalidInput)?;if start<chrono::DateTime::parse_from_rfc3339(&q.start).map_err(|_|StoreError::InvalidInput)? || start>=chrono::DateTime::parse_from_rfc3339(&q.end).map_err(|_|StoreError::InvalidInput)? {return Err(StoreError::Conflict)}
                let proof:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2 AND handle='sealed-target' AND event_id=?3 AND etag=?4)",params![r.tenant,r.id,old.id,old.etag],|r|r.get(0))?;if !proof {return Err(StoreError::Conflict)}
                if d.operation.as_deref()==Some("delete"){CalendarMutation::Delete{event_id:old.id.clone(),etag:old.etag.clone()}}else{CalendarMutation::Update{event_id:old.id.clone(),etag:old.etag.clone(),fields:fields(&cmd.profile,d,Some(old),&r.text)?}}
            },_=>return Err(StoreError::InvalidInput)};
            let change=CalendarChange{request_id:cmd.action_id.clone(),connection_id:p.id.clone(),expected_epoch:conn.epoch,expected_revision:conn.revision,mutation}; let result=admit_change_in(&tx,&u,&conn,&app,change,primary,prior)?;
            tx.execute("INSERT INTO calendar_agent_action_origins(tenant_id,action_id,run_id) VALUES(?1,?2,?3)",params![r.tenant,cmd.action_id,r.id])?;
            tx.execute("INSERT INTO calendar_agent_action_links VALUES(?1,?2,?3,?4)",params![r.tenant,r.id,cmd.action_id,cmd.step.step])?;tx.execute("UPDATE calendar_command_plans SET state='committed' WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;tx.commit()?;Ok(result)
        }).await
    }
    pub async fn calendar_agent_target(
        &self,
        l: &crate::RunLease,
        page: CalendarEvents,
    ) -> Result<CalendarEvent> {
        let l = l.clone();
        self.call(move|db|{let tx=db.conn.transaction()?;let cmd=command(&tx,&db.provider_key,&l)?.ok_or(StoreError::Conflict)?;let r=crate::chat::guard(&tx,&db.provider_key,&l)?;let p=selected(&cmd.profile,&cmd.decision)?;admit_pin(&tx,&r,p)?;
            if page.partial || page.next_page_token.is_some() || page.events.len()>5 {return Err(StoreError::InvalidInput)}let matches=page.events.into_iter().filter(|e|Some(&e.fields.title)==cmd.decision.target_quote.as_ref()).collect::<Vec<_>>();if matches.len()!=1 || !matches[0].writable {return Err(StoreError::InvalidInput)}let event=matches.into_iter().next().ok_or(StoreError::InvalidInput)?;
            tx.execute("INSERT OR REPLACE INTO calendar_agent_observations VALUES(?1,?2,'sealed-target',?3,?4,?5,'current-request',?6,?7,?8,?9,?10)",params![r.tenant,r.id,p.id,p.epoch,cmd.step.step,json(&range(&cmd.profile,&cmd.decision)?)?,event.id,event.etag,now()?,format!("{:x}",Sha256::digest(json(&event)?.as_bytes()))])?;tx.commit()?;Ok(event)}).await
    }
    pub async fn finish_calendar_chat(
        &self,
        l: &crate::RunLease,
        state: Option<String>,
    ) -> Result<bool> {
        let l = l.clone();
        self.call(move|db|{let tx=db.conn.transaction()?;let r=match crate::chat::guard(&tx,&db.provider_key,&l) {Ok(r)=>r,Err(error)=>{tx.commit()?;return Err(error)}};let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;if !exists{return Ok(false)}
            let receipt=if let Some(state)=state {let cmd=command(&tx,&db.provider_key,&l)?.ok_or(StoreError::Conflict)?;CalendarChatReceipt{action_id:None,operation:cmd.decision.operation.unwrap_or_else(||cmd.decision.decision.clone()),state:state.clone(),event_id:None,provider_etag:None,message:message(&state).into()}}else{let Some(receipt)=receipt(&tx,&r)? else{return Ok(false)};receipt};finish(&tx,&r,receipt)?;tx.commit()?;Ok(true)}).await
    }
    pub async fn calendar_read_event_id(
        &self,
        l: &crate::RunLease,
        handle: String,
    ) -> Result<(String, String)> {
        let l = l.clone();
        self.call(move|db|{let r=crate::chat::guard(&db.conn,&db.provider_key,&l)?;let q=CalendarReadRequest::Read{observation_ref:handle.clone()};if !allowed(&db.conn,&r,&q)?{return Err(StoreError::Forbidden)}db.conn.query_row("SELECT connection_id,event_id FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2 AND handle=?3",params![r.tenant,r.id,handle],|r|Ok((r.get(0)?,r.get(1)?))).map_err(Into::into)}).await
    }
    pub async fn calendar_persistence_context(&self, l: &crate::RunLease) -> Result<bool> {
        let l = l.clone();
        self.call(move |db| {
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            persistence_context(&db.conn, &r)
        })
        .await
    }
}

pub(crate) fn complete_read(
    c: &Connection,
    r: &Run,
    step: i64,
    call: &str,
    q: &CalendarReadRequest,
    result: &mut crate::KnowledgeToolResult,
) -> Result<()> {
    use crate::KnowledgeToolResult;
    if json(result)?.len() > agent_execution::TOOL_RESULT_LIMIT {
        *result = KnowledgeToolResult::Error(crate::ToolReadError::TooLargeNarrowRead);
        return Ok(());
    }
    let KnowledgeToolResult::External(value) = result else {
        return Ok(());
    };
    let CalendarReadRequest::List { range } = q else {
        return Ok(());
    };
    let page: CalendarEvents =
        serde_json::from_value(value.clone()).map_err(|_| StoreError::InvalidInput)?;
    if page.events.len() > 5 || page.partial != page.next_page_token.is_some() {
        return Err(StoreError::InvalidInput);
    }
    let p = profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
    let pin = p
        .connections
        .iter()
        .find(|p| p.id == range.connection_id)
        .ok_or(StoreError::Forbidden)?;
    let observed = now()?;
    let events = page
        .events
        .into_iter()
        .map(|e| (Uuid::new_v4().to_string(), e))
        .collect::<Vec<_>>();
    let projected = KnowledgeToolResult::External(
        serde_json::json!({"events":events.iter().map(|(handle,e)|serde_json::json!({"observation_ref":handle,"event":e})).collect::<Vec<_>>(),"partial":page.partial,"next_page_token":page.next_page_token,"observed_at":observed,"untrusted":true}),
    );
    // Unpublished oversized data must not mint handles or continuation authority.
    if json(&projected)?.len() > agent_execution::TOOL_RESULT_LIMIT {
        *result = KnowledgeToolResult::Error(crate::ToolReadError::TooLargeNarrowRead);
        return Ok(());
    }
    for (handle, e) in events {
        c.execute("INSERT INTO calendar_agent_observations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",params![r.tenant,r.id,handle,pin.id,pin.epoch,step,call,json(range)?,e.id,e.etag,observed,format!("{:x}",Sha256::digest(json(&e)?.as_bytes()))])?;
    }
    if let Some(token) = &page.next_page_token {
        let mut next = range.clone();
        next.page_token = Some(token.clone());
        c.execute("INSERT INTO calendar_agent_observations VALUES(?1,?2,?3,?4,?5,?6,?7,?8,NULL,NULL,?9,?10)",params![r.tenant,r.id,Uuid::new_v4().to_string(),pin.id,pin.epoch,step,call,json(&next)?,observed,format!("{:x}",Sha256::digest(json(&next)?.as_bytes()))])?;
    }
    *result = projected;
    Ok(())
}

impl Store {
    pub async fn calendar_agent_action(
        &self,
        l: &crate::RunLease,
        id: String,
    ) -> Result<CalendarAction> {
        let l = l.clone();
        self.call(move |db| {
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            if !allowed(
                &db.conn,
                &r,
                &CalendarReadRequest::Status {
                    action_id: id.clone(),
                },
            )? {
                return Err(StoreError::Forbidden);
            }
            let u = crate::chat::authority(&db.conn, &r)?;
            db.conn.execute("UPDATE calendar_actions SET state='outcome_unknown',error='dispatch_receipt_missing',version=version+1 WHERE tenant_id=?1 AND account_id=?2 AND id=?3 AND state='dispatch_committed' AND created_at<=?4",params![r.tenant,r.account,id,now()?-90])?;
            action(&db.conn, &u, &id)
        })
        .await
    }
}

#[cfg(test)]
mod civil_tests {
    use super::*;
    fn proposal(date: &str, q: &str) -> CalendarChatDecision {
        CalendarChatDecision {
            domain: "calendar".into(),
            decision: "command".into(),
            operation: Some("create".into()),
            connection_quote: None,
            intent_quote: None,
            title_quote: None,
            target_quote: None,
            date_quote: Some(q.into()),
            date: Some(date.into()),
            start_quote: None,
            start: None,
            end_quote: None,
            end: None,
            duration_quote: None,
            duration_minutes: None,
            clarification: None,
        }
    }
    fn clock_profile(zone: &str) -> CalendarRunProfile {
        CalendarRunProfile {
            version: 1,
            reference: chrono::DateTime::parse_from_rfc3339("2026-12-31T15:30:00Z")
                .unwrap()
                .timestamp(),
            timezone: Some(zone.into()),
            connections: vec![],
        }
    }
    #[test]
    fn calendar_civil_oracle_month_boundary_dst_gap_fold_and_explicit_clock_consistency() {
        let p = clock_profile("Asia/Seoul");
        let d = proposal("tomorrow", "내일");
        assert_eq!(
            moment(&p, &d, "09:30", "오전9시30분").unwrap().to_rfc3339(),
            "2027-01-02T00:30:00+00:00"
        );
        assert!(moment(&p, &d, "09:00", "오전9시30분").is_err());
        assert!(moment(&p, &proposal("2027-01-01", "2027-01-02"), "09:00", "09:00").is_err());
        let p = clock_profile("America/Los_Angeles");
        assert!(moment(&p, &proposal("2027-03-14", "2027-03-14"), "02:30", "02:30").is_err());
        assert!(moment(&p, &proposal("2027-11-07", "2027-11-07"), "01:30", "01:30").is_err());
    }
}
