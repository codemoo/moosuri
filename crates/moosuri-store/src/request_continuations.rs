//! Bounded explicit request + literal answers. History supplies no new action authority.
use super::*;
use crate::chat::Run;
use moosuri_core::*;
mod replies;
pub(crate) use replies::cancelled as cancelled_question_reply;
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Continuation {
    pub source_run_id: String,
    pub initiating_request: String,
    pub answers: Vec<String>,
    pub missing_field: String,
    pub draft: CurrentChatDecision,
    pub expires_at: i64,
}
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct CalendarTarget {
    pub source_run_id: String,
    pub connection_id: String,
    pub event: CalendarEvent,
}
fn encode<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
pub(crate) fn context(c: &Connection, r: &Run) -> Result<Option<Continuation>> {
    c.query_row(
        "SELECT context FROM request_continuations WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()?
    .flatten()
    .map(|s| decode(&s))
    .transpose()
}
pub(crate) fn text(c: &Connection, r: &Run) -> Result<String> {
    Ok(if let Some(v) = context(c, r)? {
        std::iter::once(v.initiating_request)
            .chain(v.answers)
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        r.text.clone()
    })
}
pub(crate) fn target(c: &Connection, r: &Run) -> Result<Option<CalendarTarget>> {
    c.query_row(
        "SELECT calendar_target FROM request_continuations WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |row| row.get::<_, Option<String>>(0),
    )
    .optional()?
    .flatten()
    .map(|s| decode(&s))
    .transpose()
}
fn same_calendar(c: &Connection, old: &Run, r: &Run) -> Result<bool> {
    let a = crate::calendar::agent::profile(c, &old.tenant, &old.id)?;
    let b = crate::calendar::agent::profile(c, &r.tenant, &r.id)?;
    Ok(match (a, b) {
        (Some(a), Some(b)) => encode(&a.connections)? == encode(&b.connections)?,
        _ => false,
    })
}
fn task_control_candidate(text: &str) -> bool {
    if moosuri_core::ordinary_answer_continuation(text) {
        return false;
    }
    let low = text.to_lowercase();
    [
        "pause",
        "resume",
        "cancel",
        "stop",
        "continue",
        "change",
        "edit",
        "steer",
        "멈춰",
        "멈추",
        "중단",
        "보류",
        "재개",
        "취소",
        "다시",
        "이어",
        "계속",
        "변경",
        "바꿔",
        "수정",
        "옮겨",
        "추가 지시",
    ]
    .iter()
    .any(|word| low.contains(word))
}
fn previous(c: &Connection, r: &Run) -> Result<Option<Run>> {
    let prior:Option<String>=c.query_row("SELECT id FROM chat_runs WHERE tenant_id=?1 AND thread_id=?2 AND sequence=?3 AND state='completed' AND account_id=?4 AND session_digest=?5 AND auth_epoch=?6 AND created_at>=?7 AND EXISTS(SELECT 1 FROM agent_execution e WHERE e.tenant_id=chat_runs.tenant_id AND e.run_id=chat_runs.id AND e.invalidated=0)",params![r.tenant,r.thread,r.sequence-1,r.account,r.session.as_slice(),r.epoch,now()?-1800],|row|row.get(0)).optional()?;
    let Some(id) = prior else { return Ok(None) };
    let old = crate::chat::load_run(c, &r.tenant, &id)?;
    Ok((encode(&old.model)? == encode(&r.model)?).then_some(old))
}
fn pending_context(c: &Connection, old: &Run, r: &Run) -> Result<Option<Continuation>> {
    let pending:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM request_continuations WHERE tenant_id=?1 AND run_id=?2 AND pending=1)",params![r.tenant,old.id],|row|row.get(0))?;
    let Some(v) = pending.then(|| context(c, old)).transpose()?.flatten() else {
        return Ok(None);
    };
    if v.expires_at < now()?
        || matches!(v.draft, CurrentChatDecision::Calendar(_)) && !same_calendar(c, old, r)?
    {
        return Ok(None);
    }
    Ok(Some(v))
}
/// Consume only a complete literal reply to the immediate owned question. This
/// cancels no saved mission, schedule or event and grants no action authority.
pub(crate) fn finish_cancelled_question(c: &Connection, r: &Run) -> Result<bool> {
    if !cancelled_question_reply(&r.text) {
        return Ok(false);
    }
    let Some(old) = previous(c, r)? else {
        return Ok(false);
    };
    if pending_context(c, &old, r)?.is_none() {
        return Ok(false);
    }
    c.execute(
        "UPDATE request_continuations SET pending=0 WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, old.id],
    )?;
    crate::chat::apply_patches(c, r, vec![crate::OutputPatch::Append {
        item: "pending-question-cancelled".into(),
        text: "이 질문을 취소했어요. 저장된 업무·알림·일정은 변경하지 않았습니다. / This question is cancelled. No saved task, reminder or event was changed.".into(),
        phase: Some(MessagePhase::FinalAnswer), refusal: false,
    }])?;
    crate::chat::transition(c, r, RunState::Completed, None)?;
    Ok(true)
}
pub(crate) fn admit(c: &Connection, r: &Run) -> Result<()> {
    let Some(old) = previous(c, r)? else {
        return Ok(());
    };
    let id = old.id.clone();
    // An actual immediately preceding task receipt supplies identity only. The
    // current message still must independently request the control operation.
    let selected: Option<String> = c.query_row(
        "SELECT target FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
        |row| row.get(0),
    )?;
    if selected.is_none()
        && crate::missions::chat::commands_allowed(r)
        && task_control_candidate(&r.text)
    {
        let receipt: Option<String> = c
            .query_row(
                "SELECT receipt FROM mission_chat_commands WHERE tenant_id=?1 AND run_id=?2",
                params![r.tenant, old.id],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(raw) = receipt {
            let receipt: MissionChatReceipt = decode(&raw)?;
            if !matches!(
                receipt.state.as_str(),
                "clarify" | "unsupported" | "unavailable" | "expired" | "revoked"
            ) && let Some(target) = receipt.target
            {
                c.execute(
                    "UPDATE mission_chat_plans SET target=?3 WHERE tenant_id=?1 AND run_id=?2",
                    params![r.tenant, r.id, encode(&target)?],
                )?;
            }
        }
    }
    let selections_compatible:bool=c.query_row("SELECT a.target IS NULL OR b.target IS NULL OR a.target=b.target FROM mission_chat_plans a JOIN mission_chat_plans b ON b.tenant_id=a.tenant_id WHERE a.tenant_id=?1 AND a.run_id=?2 AND b.run_id=?3",params![r.tenant,old.id,r.id],|row|row.get(0)).optional()?.unwrap_or(true);
    let mut inherited = None;
    if selections_compatible && let Some(mut v) = pending_context(c, &old, r)? {
        let calendar = matches!(v.draft, CurrentChatDecision::Calendar(_));
        if v.answers.len() < 4 {
            c.execute("UPDATE mission_chat_plans SET anchor=(SELECT anchor FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?3),timezone=(SELECT timezone FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?3),target=COALESCE(target,(SELECT target FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?3)) WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id,old.id])?;
            if calendar {
                let a = crate::calendar::agent::profile(c, &old.tenant, &old.id)?
                    .ok_or(StoreError::Forbidden)?;
                let mut b = crate::calendar::agent::profile(c, &r.tenant, &r.id)?
                    .ok_or(StoreError::Forbidden)?;
                b.reference = a.reference;
                b.timezone = a.timezone;
                c.execute("UPDATE calendar_agent_profiles SET profile=?3 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id,encode(&b)?])?;
            }
            v.answers.push(r.text.clone());
            if encode(&v)?.len() <= 32768 {
                inherited = Some(v)
            }
        }
    }
    let mut receipt_target = if inherited.is_some() {
        target(c, &old)?
    } else {
        None
    };
    if same_calendar(c, &old, r)? {
        let raw:Option<(String,String)>=c.query_row("SELECT a.connection_id,a.event FROM calendar_agent_action_links l JOIN calendar_actions a ON a.tenant_id=l.tenant_id AND a.id=l.action_id JOIN calendar_agent_action_origins o ON o.tenant_id=a.tenant_id AND o.action_id=a.id WHERE l.tenant_id=?1 AND l.run_id=?2 AND a.account_id=?3 AND a.state='applied' AND a.kind IN('create','update') AND a.event IS NOT NULL AND o.withdrawn=0",params![r.tenant,id,r.account],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
        if let Some((connection_id, event)) = raw {
            receipt_target = Some(CalendarTarget {
                source_run_id: id,
                connection_id,
                event: decode(&event)?,
            })
        }
    }
    if inherited.is_some() || receipt_target.is_some() {
        c.execute("INSERT INTO request_continuations(tenant_id,run_id,context,calendar_target) VALUES(?1,?2,?3,?4)",params![r.tenant,r.id,inherited.as_ref().map(encode).transpose()?,receipt_target.as_ref().map(encode).transpose()?])?;
    }
    Ok(())
}
/// Preserve every already supplied field. Only missing fields may be supplied by an answer.
pub(crate) fn validate_scope(c: &Connection, r: &Run, d: &CurrentChatDecision) -> Result<()> {
    let Some(v) = context(c, r)? else {
        return Ok(());
    };
    if v.expires_at < now()? {
        return Err(StoreError::Conflict);
    }
    let old = serde_json::to_value(&v.draft).map_err(|_| StoreError::Corrupt)?;
    let new = serde_json::to_value(d).map_err(|_| StoreError::InvalidInput)?;
    // An unrelated answer does not inherit the old request. It continues as ordinary chat.
    if new.get("decision").and_then(|s| s.as_str()) == Some("no_command") {
        return Ok(());
    }
    for (k, value) in old.as_object().ok_or(StoreError::Corrupt)? {
        if !matches!(k.as_str(), "decision" | "clarification")
            && !value.is_null()
            && new.get(k) != Some(value)
        {
            return Err(StoreError::Conflict);
        }
    }
    let zone_answer = matches!(v.missing_field.as_str(), "time")
        && crate::missions::chat::source_zone(&r.text, None)
            .ok()
            .flatten()
            .is_some();
    let selected_answer = v.missing_field == "target"
        && c.query_row(
            "SELECT target IS NOT NULL FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",
            params![r.tenant, r.id],
            |row| row.get::<_, bool>(0),
        )?;
    let mut supplied_answer = zone_answer || selected_answer;
    for (k, value) in new.as_object().ok_or(StoreError::InvalidInput)? {
        if k.ends_with("_quote")
            && k != "intent_quote"
            && old.get(k).is_none_or(|v| v.is_null())
            && !value.is_null()
        {
            let q = value.as_str().ok_or(StoreError::InvalidInput)?;
            let from_answer = if k == "time_quote" && matches!(d, CurrentChatDecision::Mission(_)) {
                // This grounds the new clock only; the complete lineage below
                // independently constrains its original weekdays/date/frequency.
                q.contains(r.text.as_str())
            } else {
                r.text.contains(q)
            };
            if !from_answer && !zone_answer {
                return Err(StoreError::InvalidInput);
            }
            supplied_answer = true;
        }
    }
    if new.get("decision").and_then(|v| v.as_str()) == Some("command") && !supplied_answer {
        return Err(StoreError::InvalidInput);
    }
    if let CurrentChatDecision::Mission(proposed) = d
        && proposed.time.is_some()
    {
        let (anchor, zone): (i64, Option<String>) = c.query_row(
            "SELECT anchor,timezone FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",
            params![r.tenant, r.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        let effective = time_decision(c, r, proposed)?;
        let zone = crate::missions::chat::source_zone(&text(c, r)?, zone.as_deref())?;
        if !crate::missions::chat::civil_consistent(
            &effective,
            anchor,
            zone.as_deref().unwrap_or("UTC"),
        ) {
            return Err(StoreError::InvalidInput);
        }
    }
    Ok(())
}
/// A split answer quote stays small, while civil validation also sees the full
/// bounded original request. This preserves incomplete weekday/date/frequency
/// constraints without copying a long task into the persisted1024-byte quote.
pub(crate) fn time_decision(
    c: &Connection,
    r: &Run,
    d: &MissionChatDecision,
) -> Result<MissionChatDecision> {
    let mut effective = d.clone();
    if context(c, r)?.is_some() && d.time.is_some() {
        effective.time_quote = Some(text(c, r)?);
    }
    Ok(effective)
}
/// A complete local clock still needs an admitted zone. Use the same frozen
/// metadata and literal-zone validator as execution; browser changes on later
/// turns cannot fill the original gap, and relative/interval times need no zone.
fn missing_timezone(c: &Connection, r: &Run, d: &CurrentChatDecision) -> Result<bool> {
    let fallback = match d {
        CurrentChatDecision::Calendar(_) => {
            crate::calendar::agent::profile(c, &r.tenant, &r.id)?
                .ok_or(StoreError::Forbidden)?
                .timezone
        }
        CurrentChatDecision::Mission(d)
            if matches!(
                d.time,
                Some(
                    MissionChatTime::Daily { .. }
                        | MissionChatTime::Weekly { .. }
                        | MissionChatTime::LocalOnce { .. }
                )
            ) =>
        {
            c.query_row(
                "SELECT timezone FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",
                params![r.tenant, r.id],
                |row| row.get::<_, Option<String>>(0),
            )?
        }
        CurrentChatDecision::Mission(_) => return Ok(false),
    };
    match crate::missions::chat::source_zone(&text(c, r)?, fallback.as_deref()) {
        Ok(zone) => Ok(zone.is_none()),
        Err(StoreError::InvalidInput) => Ok(true),
        Err(error) => Err(error),
    }
}
pub(crate) fn pending(
    c: &Connection,
    r: &Run,
    d: CurrentChatDecision,
    field: Option<&str>,
) -> Result<()> {
    let (intent, operation) = match &d {
        CurrentChatDecision::Calendar(v) => (v.intent_quote.as_ref(), v.operation.as_ref()),
        CurrentChatDecision::Mission(v) => (v.intent_quote.as_ref(), v.action.as_ref()),
    };
    if intent.is_none() || operation.is_none() {
        return Ok(());
    }
    let inherited = context(c, r)?;
    let field = match field {
        Some(field) if replies::field(field) => field.to_owned(),
        Some("ambiguous") => {
            let Some(prior) = inherited.as_ref() else {
                return Ok(());
            };
            if !replies::field(&prior.missing_field)
                || !replies::still_missing(
                    &d,
                    &prior.missing_field,
                    prior.missing_field == "time" && missing_timezone(c, r, &d)?,
                )
            {
                return Ok(());
            }
            if prior.missing_field == "target" && c.query_row(
                "SELECT target IS NOT NULL FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",
                params![r.tenant, r.id], |row| row.get::<_, bool>(0),
            )? { return Ok(()); }
            prior.missing_field.clone()
        }
        _ => return Ok(()),
    };
    let mut v = inherited.unwrap_or(Continuation {
        source_run_id: r.id.clone(),
        initiating_request: r.text.clone(),
        answers: vec![],
        missing_field: String::new(),
        draft: d.clone(),
        expires_at: now()? + 1800,
    });
    v.draft = d;
    v.missing_field = field;
    c.execute("INSERT INTO request_continuations(tenant_id,run_id,context,pending) VALUES(?1,?2,?3,1) ON CONFLICT(tenant_id,run_id) DO UPDATE SET context=excluded.context,pending=1",params![r.tenant,r.id,encode(&v)?])?;
    Ok(())
}

/// Display an ambiguity follow-up using the established pending field, never by
/// normalizing prose or altering the acknowledged decision/replay payload.
pub(crate) fn clarification_field(
    c: &Connection,
    r: &Run,
    field: Option<&str>,
) -> Result<Option<String>> {
    if field == Some("ambiguous") {
        let pending:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM request_continuations WHERE tenant_id=?1 AND run_id=?2 AND pending=1)",params![r.tenant,r.id],|row|row.get(0))?;
        if pending && let Some(v) = context(c, r)? {
            return Ok(Some(v.missing_field));
        }
    }
    Ok(field.map(str::to_owned))
}
