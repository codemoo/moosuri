//! Chat-only frozen grants, opaque observations and sealed current-user commands. No IO.
use super::*;
use crate::{
    agent_execution::{self, StepLease},
    chat::Run,
};
use chrono::{Days, NaiveDate, NaiveTime, TimeZone, Timelike};
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
        "request_continuations",
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
    let runs = c.prepare("SELECT r.id FROM chat_runs r JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE r.tenant_id=?1 AND r.account_id=?2 AND r.state!='completed' AND e.invalidated=0 AND json_extract(r.model,'$.source')='personal_chat_gpt' AND json_extract(r.model,'$.provider_id')=?3 AND json_extract(r.model,'$.provider_revision')=?4 AND (EXISTS(SELECT 1 FROM calendar_command_plans p WHERE p.tenant_id=r.tenant_id AND p.run_id=r.id) OR EXISTS(SELECT 1 FROM calendar_agent_action_origins o WHERE o.tenant_id=r.tenant_id AND o.run_id=r.id) OR EXISTS(SELECT 1 FROM request_continuations q WHERE q.tenant_id=r.tenant_id AND q.run_id=r.id AND (q.calendar_target IS NOT NULL OR json_extract(q.context,'$.draft.domain')='calendar')))")?
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
    c.execute("DELETE FROM request_continuations WHERE tenant_id=?1 AND (json_extract(calendar_target,'$.connection_id')=?2 OR (json_extract(context,'$.draft.domain')='calendar' AND run_id IN(SELECT run_id FROM calendar_agent_profiles WHERE tenant_id=?1 AND EXISTS(SELECT 1 FROM json_each(profile,'$.connections') WHERE json_extract(value,'$.id')=?2))))",params![t,id])?;
    c.execute("DELETE FROM calendar_agent_profiles WHERE tenant_id=?1 AND EXISTS(SELECT 1 FROM json_each(profile,'$.connections') WHERE json_extract(value,'$.id')=?2)",params![t,id])?;
    Ok(())
}
fn admit_pin(c: &Connection, r: &Run, p: &CalendarAgentPin) -> Result<()> {
    if !pin_current(c, r, p)?
        || crate::media::has_context(c, &r.tenant, &r.id)?
        || (crate::missions::is_mission(c, &r.tenant, &r.id)?
            && !crate::missions::connected::current(c, r)?)
    {
        return Err(StoreError::Forbidden);
    }
    c.execute(
        "INSERT OR IGNORE INTO calendar_run_dependencies VALUES(?1,?2,?3,?4,?5,?6)",
        params![r.tenant, r.id, p.id, p.epoch, p.app_revision, p.client_id],
    )?;
    Ok(())
}
pub(crate) fn request_connection(
    c: &Connection,
    r: &Run,
    q: &CalendarReadRequest,
) -> Result<Option<String>> {
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
    if crate::missions::is_mission(c, &r.tenant, &r.id)? {
        return crate::missions::connected::allowed(
            c,
            r,
            &ExternalToolRequest::Calendar(q.clone()),
        );
    }
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
        (&d.move_date_quote, 128),
        (&d.all_day_quote, 32),
        (&d.last_date_quote, 128),
        (&d.start_quote, 256),
        (&d.end_quote, 256),
        (&d.duration_quote, 128),
    ] {
        exact(text, q, max, false)?;
    }
    if d.decision != "command" && !(d.decision == "clarify" && d.operation.is_some()) {
        if d.operation.is_some()
            || d.connection_quote.is_some()
            || d.title_quote.is_some()
            || d.target_quote.is_some()
            || d.date_quote.is_some()
            || d.date.is_some()
            || d.move_date_quote.is_some()
            || d.move_date.is_some()
            || d.start_quote.is_some()
            || d.start.is_some()
            || d.end_quote.is_some()
            || d.end.is_some()
            || d.duration_quote.is_some()
            || d.duration_minutes.is_some()
            || d.all_day_quote.is_some()
            || d.last_date_quote.is_some()
            || d.last_date.is_some()
        {
            return Err(StoreError::InvalidInput);
        }
        return Ok(());
    }
    if !matches!(d.operation.as_deref(), Some("create" | "update" | "delete"))
        || d.decision == "command" && d.clarification.is_some()
    {
        return Err(StoreError::InvalidInput);
    }
    exact(
        text,
        &d.date_quote,
        128,
        d.decision == "command" && d.operation.as_deref() == Some("create"),
    )?;
    if d.move_date.is_some() != d.move_date_quote.is_some()
        || d.move_date.is_some() && d.operation.as_deref() != Some("update")
    {
        return Err(StoreError::InvalidInput);
    }
    if d.last_date.is_some() != d.last_date_quote.is_some()
        || d.all_day_quote
            .as_ref()
            .is_some_and(|q| !matches!(q.to_lowercase().as_str(), "종일" | "all-day" | "all day"))
        || d.all_day_quote.is_some()
            && (d.start.is_some() || d.end.is_some() || d.duration_minutes.is_some())
        || d.last_date.is_some()
            && (d.start.is_some() || d.end.is_some() || d.duration_minutes.is_some())
    {
        return Err(StoreError::InvalidInput);
    }
    if d.operation.as_deref() == Some("create") {
        exact(text, &d.title_quote, 512, d.decision == "command")?;
    } else {
        exact(text, &d.target_quote, 512, d.decision == "command")?;
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
        || d.operation.as_deref() == Some("delete")
            && (d.all_day_quote.is_some() || d.last_date.is_some())
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
fn time_proposal(time: MissionChatTime, q: String) -> MissionChatDecision {
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
fn zone(p: &CalendarRunProfile, _d: &CalendarChatDecision) -> Result<chrono_tz::Tz> {
    // Only the full-request timezone frozen at acknowledgement can authorize IO.
    // A selected date quote cannot recover a rejected or missing timezone.
    p.timezone
        .as_deref()
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
        MissionChatTime::LocalOnce {
            date: d.date.clone().ok_or(StoreError::InvalidInput)?,
            time: time.into(),
        },
        format!("{} {q}", d.date_quote.as_deref().unwrap_or("")),
    );
    if !crate::missions::chat::civil_consistent(&proposal, p.reference, &zone.to_string()) {
        return Err(StoreError::InvalidInput);
    }
    let date = crate::schedules::civil_time::date_value(
        d.date.as_deref().ok_or(StoreError::InvalidInput)?,
        d.date_quote.as_deref().unwrap_or(""),
        p.reference,
        &zone.to_string(),
    )?;
    let time = NaiveTime::parse_from_str(time, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    Ok(zone
        .from_local_datetime(&date.and_time(time))
        .single()
        .ok_or(StoreError::InvalidInput)?
        .with_timezone(&chrono::Utc))
}
/// Validate the date quote using the existing civil oracle, without resolving
/// any local midnight. A civil day is not a UTC instant or a 24-hour duration.
fn civil_date(p: &CalendarRunProfile, date: &str, quote: &str) -> Result<NaiveDate> {
    let zone = p.timezone.as_deref().ok_or(StoreError::InvalidInput)?;
    crate::schedules::civil_time::date_value(date, quote, p.reference, zone)
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
fn receipt_target(
    c: &Connection,
    r: &Run,
    d: &CalendarChatDecision,
) -> Result<Option<crate::request_continuations::CalendarTarget>> {
    if !matches!(d.operation.as_deref(), Some("update" | "delete"))
        || !matches!(
            d.target_quote.as_deref(),
            Some("그 일정" | "그 회의" | "that event" | "that meeting" | "it" | "the event")
        )
    {
        return Ok(None);
    }
    let Some(target) = crate::request_continuations::target(c, r)? else {
        return Err(StoreError::InvalidInput);
    };
    let source = crate::chat::load_run(c, &r.tenant, &target.source_run_id)?;
    if source.thread != r.thread
        || source.account != r.account
        || profile(c, &r.tenant, &source.id)?.is_none()
        || !dependencies_current(c, &source)?
    {
        return Err(StoreError::Forbidden);
    }
    let p = profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
    let pin = selected(&p, d)?;
    if pin.id != target.connection_id || !pin_current(c, r, pin)? {
        return Err(StoreError::Forbidden);
    }
    Ok(Some(target))
}
fn command_range(
    c: &Connection,
    r: &Run,
    p: &CalendarRunProfile,
    d: &CalendarChatDecision,
) -> Result<CalendarRange> {
    if let Some(target) = receipt_target(c, r, d)? {
        let zone = zone(p, d)?;
        let start = if target.event.all_day {
            moosuri_connectors::calendar::date_range(
                &target.event.fields.start,
                &target.event.fields.end,
            )
            .map_err(|_| StoreError::InvalidInput)?
            .0
        } else {
            chrono::DateTime::parse_from_rfc3339(&target.event.fields.start)
                .map_err(|_| StoreError::Corrupt)?
                .with_timezone(&zone)
                .date_naive()
        };
        let next = start
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?;
        let start = zone
            .from_local_datetime(&start.and_hms_opt(0, 0, 0).ok_or(StoreError::InvalidInput)?)
            .single()
            .ok_or(StoreError::InvalidInput)?;
        let end = zone
            .from_local_datetime(&next.and_hms_opt(0, 0, 0).ok_or(StoreError::InvalidInput)?)
            .single()
            .ok_or(StoreError::InvalidInput)?;
        return Ok(CalendarRange {
            connection_id: target.connection_id,
            start: start.to_rfc3339(),
            end: end.to_rfc3339(),
            page_token: None,
        });
    }
    range(p, d)
}
fn fields(
    p: &CalendarRunProfile,
    d: &CalendarChatDecision,
    prior: Option<&CalendarEvent>,
    text: &str,
) -> Result<CalendarFields> {
    if prior.is_some_and(|v| v.all_day) || d.all_day_quote.is_some() {
        return all_day_fields(p, d, prior, text);
    }
    if d.last_date.is_some() {
        return Err(StoreError::InvalidInput);
    }
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
    let old_duration = prior
        .map(|old| {
            let start = chrono::DateTime::parse_from_rfc3339(&old.fields.start)
                .map_err(|_| StoreError::InvalidInput)?;
            let end = chrono::DateTime::parse_from_rfc3339(&old.fields.end)
                .map_err(|_| StoreError::InvalidInput)?;
            Ok::<_, StoreError>(end - start)
        })
        .transpose()?;
    let mut destination = d.clone();
    if let Some(date) = &d.move_date {
        let old = prior.ok_or(StoreError::InvalidInput)?;
        destination.date = Some(date.clone());
        destination.date_quote = d.move_date_quote.clone();
        let clock = chrono::DateTime::parse_from_rfc3339(&old.fields.start)
            .map_err(|_| StoreError::InvalidInput)?
            .with_timezone(&zone(p, &destination)?)
            .format("%H:%M")
            .to_string();
        if d.start.is_none() {
            let old_start = chrono::DateTime::parse_from_rfc3339(&old.fields.start)
                .map_err(|_| StoreError::InvalidInput)?;
            let fraction = chrono::Duration::seconds(i64::from(old_start.second()))
                + chrono::Duration::nanoseconds(i64::from(old_start.nanosecond()));
            f.start = (moment(p, &destination, &clock, &clock)? + fraction).to_rfc3339();
        }
    }
    let d = &destination;
    if d.start.is_some() || d.end.is_some() || d.move_date.is_some() {
        f.time_zone = Some(zone(p, d)?.to_string());
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
    if (d.start.is_some() || d.move_date.is_some())
        && d.end.is_none()
        && d.duration_minutes.is_none()
    {
        let duration = old_duration.ok_or(StoreError::InvalidInput)?;
        if duration <= chrono::Duration::zero() || duration > chrono::Duration::hours(24) {
            return Err(StoreError::InvalidInput);
        }
        let start =
            chrono::DateTime::parse_from_rfc3339(&f.start).map_err(|_| StoreError::InvalidInput)?;
        f.end = (start + duration).to_rfc3339();
    }
    if prior.is_none() && (d.start.is_none() || d.end.is_none() && d.duration_minutes.is_none()) {
        return Err(StoreError::InvalidInput);
    }
    if prior.is_some()
        && d.move_date.is_none()
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
fn all_day_fields(
    p: &CalendarRunProfile,
    d: &CalendarChatDecision,
    prior: Option<&CalendarEvent>,
    text: &str,
) -> Result<CalendarFields> {
    if prior.is_some_and(|v| !v.all_day)
        || d.start.is_some()
        || d.end.is_some()
        || d.duration_minutes.is_some()
        || prior.is_none() && d.all_day_quote.is_none()
    {
        return Err(StoreError::InvalidInput);
    }
    // A single-day default belongs only to a single explicit civil date. If a
    // range was requested, a missing last day must clarify rather
    // than silently truncate it to one day.
    let lower = text.to_lowercase();
    if ["exclusive", "미포함", "제외"]
        .iter()
        .any(|word| lower.contains(word))
    {
        return Err(StoreError::InvalidInput);
    }
    if prior.is_none() && d.last_date.is_none() {
        let iso_dates = lower
            .as_bytes()
            .windows(10)
            .filter(|w| {
                w[4] == b'-'
                    && w[7] == b'-'
                    && w.iter()
                        .enumerate()
                        .all(|(i, b)| matches!(i, 4 | 7) || b.is_ascii_digit())
            })
            .count();
        if iso_dates > 1
            || [
                "부터", "까지", "through", "until", " to ", "between", "days", "일간", "동안",
            ]
            .iter()
            .any(|v| lower.contains(v))
        {
            return Err(StoreError::InvalidInput);
        }
    }
    let old = prior
        .map(|v| {
            moosuri_connectors::calendar::date_range(&v.fields.start, &v.fields.end)
                .map_err(|_| StoreError::InvalidInput)
        })
        .transpose()?;
    let start = if let Some(date) = &d.move_date {
        civil_date(
            p,
            date,
            d.move_date_quote
                .as_deref()
                .ok_or(StoreError::InvalidInput)?,
        )?
    } else if let Some((start, _)) = old {
        start
    } else {
        civil_date(
            p,
            d.date.as_deref().ok_or(StoreError::InvalidInput)?,
            d.date_quote.as_deref().ok_or(StoreError::InvalidInput)?,
        )?
    };
    let end = if let Some(date) = &d.last_date {
        let quote = d
            .last_date_quote
            .as_deref()
            .ok_or(StoreError::InvalidInput)?;
        civil_date(p, date, quote)?
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?
    } else {
        let days = old.map(|(a, b)| (b - a).num_days()).unwrap_or(1);
        start
            .checked_add_days(Days::new(
                days.try_into().map_err(|_| StoreError::InvalidInput)?,
            ))
            .ok_or(StoreError::InvalidInput)?
    };
    if prior.is_some() && d.move_date.is_none() && d.last_date.is_none() && d.title_quote.is_none()
    {
        return Err(StoreError::InvalidInput);
    }
    let f = CalendarFields {
        title: d
            .title_quote
            .clone()
            .or_else(|| prior.map(|v| v.fields.title.clone()))
            .ok_or(StoreError::InvalidInput)?,
        description: prior
            .map(|v| v.fields.description.clone())
            .unwrap_or_default(),
        location: prior.map(|v| v.fields.location.clone()).unwrap_or_default(),
        start: start.format("%Y-%m-%d").to_string(),
        end: end.format("%Y-%m-%d").to_string(),
        time_zone: None,
    };
    moosuri_connectors::calendar::validate_fields(&f).map_err(|_| StoreError::InvalidInput)?;
    Ok(f)
}
fn event_in_range(e: &CalendarEvent, q: &CalendarRange, p: &CalendarRunProfile) -> Result<bool> {
    let start =
        chrono::DateTime::parse_from_rfc3339(&q.start).map_err(|_| StoreError::InvalidInput)?;
    let end = chrono::DateTime::parse_from_rfc3339(&q.end).map_err(|_| StoreError::InvalidInput)?;
    if e.all_day {
        let (date, _) = moosuri_connectors::calendar::date_range(&e.fields.start, &e.fields.end)
            .map_err(|_| StoreError::InvalidInput)?;
        let zone: chrono_tz::Tz = p
            .timezone
            .as_deref()
            .ok_or(StoreError::InvalidInput)?
            .parse()
            .map_err(|_| StoreError::InvalidInput)?;
        Ok(date == start.with_timezone(&zone).date_naive())
    } else {
        let actual = chrono::DateTime::parse_from_rfc3339(&e.fields.start)
            .map_err(|_| StoreError::InvalidInput)?;
        Ok(actual >= start && actual < end)
    }
}
pub(crate) fn record(c: &Connection, r: &Run, raw: serde_json::Value) -> Result<()> {
    let d: CalendarChatDecision =
        serde_json::from_value(raw).map_err(|_| StoreError::InvalidInput)?;
    validate_decision(&crate::request_continuations::text(c, r)?, &d)?;
    let source = crate::request_continuations::text(c, r)?;
    let p = profile(c, &r.tenant, &r.id)?.ok_or(StoreError::Forbidden)?;
    // Literal-span validation must not turn an unsupported timezone into a
    // failed chat. Actual admission below freezes it as unavailable and clarifies.
    let zone = match crate::missions::chat::source_zone(&source, p.timezone.as_deref()) {
        Ok(zone) => zone,
        Err(StoreError::InvalidInput) => None,
        Err(error) => return Err(error),
    };
    let data = [d.title_quote.as_deref(), d.target_quote.as_deref()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    for quote in [&d.date_quote, &d.move_date_quote, &d.last_date_quote]
        .into_iter()
        .flatten()
    {
        if !crate::schedules::civil_time::complete_quote(
            &source,
            quote,
            &data,
            p.reference,
            zone.as_deref().unwrap_or("UTC"),
            false,
        ) {
            return Err(StoreError::InvalidInput);
        }
    }
    for quote in [&d.start_quote, &d.end_quote].into_iter().flatten() {
        if !crate::schedules::civil_time::complete_quote(
            &source,
            quote,
            &data,
            p.reference,
            zone.as_deref().unwrap_or("UTC"),
            true,
        ) {
            return Err(StoreError::InvalidInput);
        }
    }
    crate::request_continuations::validate_scope(c, r, &CurrentChatDecision::Calendar(d.clone()))?;
    if d.decision == "clarify" {
        crate::request_continuations::pending(
            c,
            r,
            CurrentChatDecision::Calendar(d.clone()),
            d.clarification.as_deref(),
        )?;
    }
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
        p.timezone = crate::missions::chat::source_zone(
            &crate::request_continuations::text(c, r)?,
            p.timezone.as_deref(),
        )
        .unwrap_or(None);
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
            "반복·참석자·예약 실행과 일정 종류 변환은 아직 지원하지 않아요. / Recurrence, attendees, scheduled execution and timed/all-day conversion are unsupported."
        }
        _ => {
            "한 개의 캘린더와 일정 제목·날짜를 말씀해 주세요. 시간 일정은 시작·종료(또는 기간), 종일 일정은 마지막 날이 필요해요. / Specify one calendar, exact title/date and timed start/end or all-day last day. No change was made."
        }
    }
}
fn clarification_message(d: &CalendarChatDecision) -> &'static str {
    match d.clarification.as_deref() {
        Some("time") => {
            "몇 시에 시작하나요? 오전/오후를 포함해 알려 주세요. / What start time should I use?"
        }
        Some("date") => "어느 날짜인가요? / Which date should I use?",
        Some("duration") => {
            if d.all_day_quote.is_some() {
                "마지막 날은 언제인가요? 그 날짜까지 포함할게요. / What is the last day, inclusive?"
            } else {
                "언제 끝나나요? 종료 시각이나 기간을 알려 주세요. / What end time or duration should I use?"
            }
        }
        Some("target") => {
            "어느 일정을 변경할까요? 제목과 날짜를 알려 주세요. / Which event do you mean? Give its title and date."
        }
        Some("account") => {
            "어느 Calendar 계정에 적용할까요? / Which Calendar account should I use?"
        }
        _ => message("clarify"),
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
    pub async fn calendar_agent_followup_target(
        &self,
        l: &crate::RunLease,
    ) -> Result<Option<String>> {
        let l = l.clone();
        self.call(move |db| {
            let cmd = command(&db.conn, &db.provider_key, &l)?.ok_or(StoreError::Conflict)?;
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            Ok(receipt_target(&db.conn, &r, &cmd.decision)?.map(|v| v.event.id))
        })
        .await
    }
    pub async fn calendar_agent_range(&self, l: &crate::RunLease) -> Result<CalendarRange> {
        let l = l.clone();
        self.call(move |db| {
            let v = command(&db.conn, &db.provider_key, &l)?.ok_or(StoreError::Conflict)?;
            let r = crate::chat::guard(&db.conn, &db.provider_key, &l)?;
            command_range(&db.conn, &r, &v.profile, &v.decision)
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
            let d=&cmd.decision;validate_decision(&crate::request_continuations::text(&tx,&r)?,d)?;let p=selected(&cmd.profile,d)?;
            if p.id!=conn.id || p.epoch!=conn.epoch || p.client_id!=app.client_id || p.app_revision!=app.revision || cmd.user.digest!=u.digest || cmd.user.epoch!=u.epoch {return Err(StoreError::Forbidden)} admit_pin(&tx,&r,p)?;
            let mutation=match d.operation.as_deref(){Some("create")=>CalendarMutation::Create{fields:fields(&cmd.profile,d,None,&crate::request_continuations::text(&tx,&r)?)?},Some("update"|"delete")=>{let old=prior.as_ref().ok_or(StoreError::InvalidInput)?;let receipt=receipt_target(&tx,&r,d)?;if !old.writable || if let Some(target)=&receipt {target.event.id!=old.id || target.event.etag!=old.etag} else {d.target_quote.as_ref()!=Some(&old.fields.title)} {return Err(StoreError::Conflict)}let q=command_range(&tx,&r,&cmd.profile,d)?;if !event_in_range(old,&q,&cmd.profile)? {return Err(StoreError::Conflict)}
                let proof:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendar_agent_observations WHERE tenant_id=?1 AND run_id=?2 AND handle='sealed-target' AND event_id=?3 AND etag=?4)",params![r.tenant,r.id,old.id,old.etag],|r|r.get(0))?;if !proof {return Err(StoreError::Conflict)}
                if d.operation.as_deref()==Some("delete"){CalendarMutation::Delete{event_id:old.id.clone(),etag:old.etag.clone()}}else{CalendarMutation::Update{event_id:old.id.clone(),etag:old.etag.clone(),fields:fields(&cmd.profile,d,Some(old),&crate::request_continuations::text(&tx,&r)?)?}}
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
            if page.partial || page.next_page_token.is_some() || page.events.len()>5 {return Err(StoreError::InvalidInput)}let receipt=receipt_target(&tx,&r,&cmd.decision)?;let matches=page.events.into_iter().filter(|e|if let Some(target)=&receipt {e.id==target.event.id && e.etag==target.event.etag}else{Some(&e.fields.title)==cmd.decision.target_quote.as_ref()}).collect::<Vec<_>>();if matches.len()!=1 || !matches[0].writable {return Err(StoreError::InvalidInput)}let event=matches.into_iter().next().ok_or(StoreError::InvalidInput)?;
            tx.execute("INSERT OR REPLACE INTO calendar_agent_observations VALUES(?1,?2,'sealed-target',?3,?4,?5,'current-request',?6,?7,?8,?9,?10)",params![r.tenant,r.id,p.id,p.epoch,cmd.step.step,json(&command_range(&tx,&r,&cmd.profile,&cmd.decision)?)?,event.id,event.etag,now()?,format!("{:x}",Sha256::digest(json(&event)?.as_bytes()))])?;tx.commit()?;Ok(event)}).await
    }
    pub async fn finish_calendar_chat(
        &self,
        l: &crate::RunLease,
        state: Option<String>,
    ) -> Result<bool> {
        let l = l.clone();
        self.call(move|db|{let tx=db.conn.transaction()?;let r=match crate::chat::guard(&tx,&db.provider_key,&l) {Ok(r)=>r,Err(error)=>{tx.commit()?;return Err(error)}};let exists:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM calendar_command_plans WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;if !exists{return Ok(false)}
            let receipt=if let Some(state)=state {let cmd=command(&tx,&db.provider_key,&l)?.ok_or(StoreError::Conflict)?;CalendarChatReceipt{action_id:None,operation:cmd.decision.operation.clone().unwrap_or_else(||cmd.decision.decision.clone()),state:state.clone(),event_id:None,provider_etag:None,message:if state=="clarify" {clarification_message(&cmd.decision).into()}else{message(&state).into()}}}else{let Some(receipt)=receipt(&tx,&r)? else{return Ok(false)};receipt};finish(&tx,&r,receipt)?;tx.commit()?;Ok(true)}).await
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
            move_date_quote: None,
            move_date: None,
            all_day_quote: None,
            last_date_quote: None,
            last_date: None,
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
    fn conversational_civil_time_calendar_shared_relative_weekday_and_native_clock_oracles() {
        let p = clock_profile("Asia/Seoul");
        for (date, quote, clock, origin, utc) in [
            (
                "모레",
                "모레",
                "15:30",
                "오후 세 시 반",
                "2027-01-03T06:30:00+00:00",
            ),
            (
                "다음 주 금요일",
                "다음 주 금요일",
                "09:15",
                "오전 아홉 시 십오 분",
                "2027-01-08T00:15:00+00:00",
            ),
            (
                "this Friday",
                "this Friday",
                "09:00",
                "9AM",
                "2027-01-01T00:00:00+00:00",
            ),
        ] {
            assert_eq!(
                moment(&p, &proposal(date, quote), clock, origin)
                    .unwrap()
                    .to_rfc3339(),
                utc
            )
        }
        assert!(moment(&p, &proposal("2027-01-08", "next Friday"), "09:00", "9AM").is_err());
        assert!(
            moment(
                &p,
                &proposal("다음 주 금요일", "다음 주 금요일"),
                "09:00",
                "오전 아홉 시 십오 분"
            )
            .is_err()
        );
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

#[cfg(test)]
mod move_tests {
    use super::*;
    fn event(start: &str, end: &str) -> CalendarEvent {
        CalendarEvent {
            id: "synthetic".into(),
            etag: "etag".into(),
            status: "confirmed".into(),
            html_link: None,
            writable: true,
            all_day: false,
            updated: None,
            fields: CalendarFields {
                title: "Synthetic".into(),
                description: String::new(),
                location: String::new(),
                start: start.into(),
                end: end.into(),
                time_zone: Some("America/Los_Angeles".into()),
            },
        }
    }
    #[test]
    fn request_continuation_date_move_retains_duration_seconds_and_rejects_dst_gap_fold() {
        let p = CalendarRunProfile {
            version: 1,
            reference: 1800000000,
            timezone: Some("America/Los_Angeles".into()),
            connections: vec![],
        };
        let mut d:CalendarChatDecision=serde_json::from_value(serde_json::json!({"domain":"calendar","decision":"command","operation":"update","intent_quote":"move","title_quote":null,"target_quote":"Synthetic","date":"2027-03-13","date_quote":"2027-03-13","move_date":"2027-03-14","move_date_quote":"2027-03-14","start_quote":null,"start":null,"end_quote":null,"end":null,"duration_quote":null,"duration_minutes":null,"connection_quote":null,"clarification":null})).unwrap();
        let old = event("2027-03-13T01:30:30-08:00", "2027-03-13T03:30:30-08:00");
        let moved = fields(&p, &d, Some(&old), "move Synthetic to 2027-03-14").unwrap();
        assert_eq!(moved.start, "2027-03-14T09:30:30+00:00");
        assert_eq!(moved.end, "2027-03-14T11:30:30+00:00");
        let gap = event("2027-03-13T02:30:00-08:00", "2027-03-13T03:15:00-08:00");
        assert!(fields(&p, &d, Some(&gap), "move Synthetic to 2027-03-14").is_err());
        d.move_date = Some("2027-11-07".into());
        d.move_date_quote = d.move_date.clone();
        assert!(fields(&p, &d, Some(&old), "move Synthetic to 2027-11-07").is_err());
    }
}

#[cfg(test)]
mod all_day_tests {
    use super::*;
    fn profile(zone: &str) -> CalendarRunProfile {
        CalendarRunProfile {
            version: 1,
            reference: chrono::DateTime::parse_from_rfc3339("2026-12-31T15:30:00Z")
                .unwrap()
                .timestamp(),
            timezone: Some(zone.into()),
            connections: vec![],
        }
    }
    fn decision(text: &str, date: &str) -> CalendarChatDecision {
        serde_json::from_value(serde_json::json!({"domain":"calendar","decision":"command","operation":"create","intent_quote":text,"title_quote":"Synthetic holiday","date_quote":date,"date":date,"all_day_quote":"all-day","connection_quote":null,"target_quote":null,"start_quote":null,"start":null,"end_quote":null,"end":null,"duration_quote":null,"duration_minutes":null,"clarification":null})).unwrap()
    }
    #[test]
    fn all_day_civil_dates_inclusive_last_day_relative_zone_and_span_preservation() {
        let text = "Create all-day Synthetic holiday on 2027-03-14 through 2027-03-16";
        let mut d = decision(text, "2027-03-14");
        d.last_date = Some("2027-03-16".into());
        d.last_date_quote = d.last_date.clone();
        validate_decision(text, &d).unwrap();
        let f = fields(&profile("America/Los_Angeles"), &d, None, text).unwrap();
        assert_eq!((&*f.start, &*f.end), ("2027-03-14", "2027-03-17"));
        assert!(f.time_zone.is_none());
        let old = CalendarEvent {
            id: "synthetic".into(),
            etag: "etag".into(),
            status: "confirmed".into(),
            fields: f,
            all_day: true,
            writable: true,
            html_link: None,
            updated: None,
        };
        d.operation = Some("update".into());
        d.title_quote = None;
        d.all_day_quote = None;
        d.last_date = None;
        d.last_date_quote = None;
        d.move_date = Some("2027-11-07".into());
        d.move_date_quote = d.move_date.clone();
        let moved = fields(
            &profile("America/Los_Angeles"),
            &d,
            Some(&old),
            "Move that event to 2027-11-07",
        )
        .unwrap();
        assert_eq!((&*moved.start, &*moved.end), ("2027-11-07", "2027-11-10"));
        assert!(moved.time_zone.is_none());
        assert_eq!(
            civil_date(&profile("Asia/Seoul"), "today", "오늘")
                .unwrap()
                .to_string(),
            "2027-01-01"
        );
        assert_eq!(
            civil_date(&profile("America/Los_Angeles"), "today", "today")
                .unwrap()
                .to_string(),
            "2026-12-31"
        );
    }
    #[test]
    fn all_day_invalid_quotes_ranges_and_kind_conversions_do_not_form_payloads() {
        let text = "Create all-day Synthetic holiday on 2027-03-14 through 2027-03-16";
        let mut d = decision(text, "2027-03-14");
        let p = profile("America/Los_Angeles");
        assert!(fields(&p, &d, None, text).is_err()); // Never drop a requested range.
        d.last_date = Some("2027-03-17".into());
        d.last_date_quote = Some("2027-03-16".into());
        assert!(fields(&p, &d, None, text).is_err());
        for last in ["2027-03-13", "2027-04-14", "2027-02-29"] {
            d.last_date = Some(last.into());
            d.last_date_quote = d.last_date.clone();
            assert!(fields(&p, &d, None, text).is_err());
        }
        d.last_date = None;
        d.last_date_quote = None;
        let f = fields(
            &p,
            &d,
            None,
            "Create all-day Synthetic holiday on 2027-03-14",
        )
        .unwrap();
        assert_eq!(f.end, "2027-03-15");
        let mut old = CalendarEvent {
            id: "synthetic".into(),
            etag: "etag".into(),
            status: "confirmed".into(),
            fields: f,
            all_day: true,
            writable: true,
            html_link: None,
            updated: None,
        };
        d.operation = Some("update".into());
        d.all_day_quote = None;
        d.start = Some("10:00".into());
        d.start_quote = d.start.clone();
        assert!(fields(&p, &d, Some(&old), "Move that event to 10:00").is_err());
        old.all_day = false;
        d.start = None;
        d.start_quote = None;
        d.all_day_quote = Some("all-day".into());
        assert!(fields(&p, &d, Some(&old), "Make that event all-day").is_err());
    }
}
