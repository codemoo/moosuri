//! Current-request-only semantic proposals. Literal spans establish origin, not intent security.
use super::*;
use moosuri_core::*;
type PlanRow = (String, Option<String>, i64, Option<String>, Option<String>);
#[cfg(test)]
mod tests;
#[derive(Clone, serde::Serialize)]
pub struct Planning {
    pub anchor: i64,
    pub timezone: Option<String>,
    pub selected_kind: Option<String>,
    pub calendar: Option<CalendarRunProfile>,
    pub calendar_only: bool,
}
fn encode<T: serde::Serialize>(v: &T) -> Result<String> {
    serde_json::to_string(v).map_err(|_| StoreError::InvalidInput)
}
fn decode<T: serde::de::DeserializeOwned>(v: &str) -> Result<T> {
    serde_json::from_str(v).map_err(|_| StoreError::Corrupt)
}
pub(crate) fn admit(
    c: &Connection,
    r: &Run,
    zone: Option<&str>,
    context: Option<&MissionChatContext>,
) -> Result<()> {
    if r.tool_profile.is_none() || crate::media::has_context(c, &r.tenant, &r.id)? {
        return Ok(());
    }
    if r.model.source == ModelSource::PersonalChatGpt {
        if context.and_then(|v| v.target.as_ref()).is_some() {
            return Err(StoreError::Forbidden);
        }
        if crate::calendar::agent::profile(c, &r.tenant, &r.id)?.is_none() {
            return Ok(());
        }
    }
    if let Some(t) = context.and_then(|v| v.target.as_ref())
        && (!matches!(t.kind.as_str(), "one_off" | "recurring")
            || Uuid::parse_str(&t.id).is_err()
            || t.revision < 1)
    {
        return Err(StoreError::InvalidInput);
    }
    c.execute("INSERT INTO mission_chat_plans(tenant_id,run_id,anchor,timezone,target,original_session,original_epoch) VALUES(?1,?2,?3,?4,?5,?6,?7)",params![r.tenant,r.id,now()?,zone,context.and_then(|v|v.target.as_ref()).map(encode).transpose()?,r.session.as_slice(),r.epoch])?;
    Ok(())
}
/// An unapplied command retains its original capability even if ordinary Retry rebinds the run.
fn original_authority(c: &Connection, r: &Run) -> Result<UserContext> {
    let (digest, epoch): (Vec<u8>, i64) = c.query_row(
        "SELECT original_session,original_epoch FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id], |row| Ok((row.get(0)?, row.get(1)?)))?;
    let digest: [u8; 32] = digest.try_into().map_err(|_| StoreError::Corrupt)?;
    let u = load_context(c, digest, false)?;
    if u.restricted
        || u.epoch != epoch
        || r.epoch != epoch
        || u.tenant.0 != r.tenant
        || u.account.id != r.account
    {
        return Err(StoreError::Unauthorized);
    }
    Ok(u)
}
pub(crate) fn retry_authority(c: &Connection, r: &Run) -> Result<()> {
    let pending: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2 AND state IN('awaiting','decided'))",
        params![r.tenant, r.id], |row| row.get(0))?;
    if pending {
        original_authority(c, r)?;
    }
    Ok(())
}
fn quote(text: &str, v: &Option<String>, max: usize, required: bool) -> Result<()> {
    match v {
        Some(q) if !q.trim().is_empty() && q.len() <= max && text.contains(q) => Ok(()),
        None if !required => Ok(()),
        _ => Err(StoreError::InvalidInput),
    }
}
fn validate(text: &str, d: &MissionChatDecision) -> Result<()> {
    if encode(d)?.len() > 16384
        || !matches!(
            d.decision.as_str(),
            "no_command" | "clarify" | "command" | "unsupported"
        )
    {
        return Err(StoreError::InvalidInput);
    }
    quote(text, &d.intent_quote, 2048, d.decision == "command")?;
    for (v, max) in [
        (&d.task_quote, 8000),
        (&d.constraints_quote, 4000),
        (&d.time_quote, 1024),
        (&d.target_quote, 256),
        (&d.instruction_quote, 2000),
    ] {
        quote(text, v, max, false)?;
    }
    if d.decision != "command" {
        if d.action.is_some()
            || d.target_kind.is_some()
            || d.task_quote.is_some()
            || d.constraints_quote.is_some()
            || d.time.is_some()
            || d.time_quote.is_some()
            || d.target_quote.is_some()
            || d.instruction_quote.is_some()
        {
            return Err(StoreError::InvalidInput);
        }
        if d.decision == "no_command" && d.clarification.is_some() {
            return Err(StoreError::InvalidInput);
        }
        if d.clarification.as_ref().is_some_and(|v| {
            !matches!(
                v.as_str(),
                "time" | "target" | "task" | "ambiguous" | "unsupported"
            )
        }) {
            return Err(StoreError::InvalidInput);
        }
        return Ok(());
    }
    let action = d.action.as_deref().ok_or(StoreError::InvalidInput)?;
    if d.clarification.is_some()
        || !matches!(
            action,
            "create_now"
                | "create_scheduled"
                | "pause"
                | "resume"
                | "cancel"
                | "steer"
                | "edit_time"
        )
    {
        return Err(StoreError::InvalidInput);
    }
    if matches!(action, "create_now" | "create_scheduled") {
        quote(text, &d.task_quote, 8000, true)?;
        if d.target_kind.is_some() || d.target_quote.is_some() || d.instruction_quote.is_some() {
            return Err(StoreError::InvalidInput);
        }
    } else if !matches!(
        d.target_kind.as_deref(),
        Some("one_off" | "recurring" | "selected")
    ) {
        return Err(StoreError::InvalidInput);
    }
    if !matches!(action, "create_now" | "create_scheduled")
        && (d.task_quote.is_some() || d.constraints_quote.is_some())
    {
        return Err(StoreError::InvalidInput);
    }
    if action != "steer" && d.instruction_quote.is_some() {
        return Err(StoreError::InvalidInput);
    }
    if matches!(action, "create_scheduled" | "edit_time") {
        quote(text, &d.time_quote, 1024, true)?;
        if d.time.is_none() {
            return Err(StoreError::InvalidInput);
        }
    } else if d.time.is_some() || d.time_quote.is_some() {
        return Err(StoreError::InvalidInput);
    }
    if action == "steer" {
        quote(text, &d.instruction_quote, 2000, true)?;
    }
    Ok(())
}
// Personal foreground Calendar shares the envelope, never mission authority.
// Repeat this predicate before effect preflight/application as well as recording.
fn allowed_mission_decision(r: &Run, d: &MissionChatDecision) -> Result<()> {
    if r.model.source == ModelSource::PersonalChatGpt
        && (d.decision != "no_command"
            || d.action.is_some()
            || d.target_kind.is_some()
            || d.intent_quote.is_some()
            || d.task_quote.is_some()
            || d.constraints_quote.is_some()
            || d.time_quote.is_some()
            || d.target_quote.is_some()
            || d.instruction_quote.is_some()
            || d.time.is_some()
            || d.clarification.is_some())
    {
        return Err(StoreError::Forbidden);
    }
    Ok(())
}
/// Called only inside the same transaction that acknowledges the completed model step.
pub(crate) fn record(c: &Connection, r: &Run, bytes: &[u8]) -> Result<()> {
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| StoreError::InvalidInput)?;
    if value.get("kind").and_then(|v| v.as_str()) != Some("mission_chat_decision") {
        return Ok(());
    }
    if value
        .get("decision")
        .and_then(|v| v.get("domain"))
        .and_then(|v| v.as_str())
        == Some("calendar")
    {
        original_authority(c, r)?;
        return crate::calendar::agent::record(
            c,
            r,
            value
                .get("decision")
                .ok_or(StoreError::InvalidInput)?
                .clone(),
        );
    }
    let d: MissionChatDecision = serde_json::from_value(
        value
            .get("decision")
            .cloned()
            .ok_or(StoreError::InvalidInput)?,
    )
    .map_err(|_| StoreError::InvalidInput)?;
    original_authority(c, r)?;
    allowed_mission_decision(r, &d)?;
    validate(&r.text, &d)?;
    let valid:bool=c.query_row("SELECT EXISTS(SELECT 1 FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2 AND state='awaiting') AND EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND remote_calls=1 AND tool_calls=0) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;
    if !valid || crate::media::has_context(c, &r.tenant, &r.id)? {
        return Err(StoreError::Conflict);
    }
    c.execute("UPDATE mission_chat_plans SET state='decided',decision=?3 WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id,encode(&d)?])?;
    Ok(())
}
// Only explicit civil components are checked here; this is not a full command grammar.
fn normalized_time(q: &str) -> String {
    let mut q = q.to_lowercase();
    for (word, n) in [
        ("forty-five", "45"),
        ("forty five", "45"),
        ("thirty", "30"),
        ("fifteen", "15"),
        ("eleven", "11"),
        ("twelve", "12"),
        ("eight", "8"),
        ("seven", "7"),
        ("three", "3"),
        ("four", "4"),
        ("five", "5"),
        ("six", "6"),
        ("nine", "9"),
        ("ten", "10"),
        ("two", "2"),
        ("one", "1"),
        ("열두", "12"),
        ("열한", "11"),
        ("아홉", "9"),
        ("여덟", "8"),
        ("일곱", "7"),
        ("여섯", "6"),
        ("다섯", "5"),
        ("서른", "30"),
        ("열", "10"),
        ("네 시", "4시"),
        ("세 시", "3시"),
        ("두 시", "2시"),
        ("한 시", "1시"),
        ("한 시간", "1시간"),
        ("두 시간", "2시간"),
        ("한 분", "1분"),
    ] {
        q = q.replace(word, n);
    }
    q = q.chars().filter(|c| !c.is_whitespace()).collect();
    let digit = ["", "일", "이", "삼", "사", "오", "육", "칠", "팔", "구"];
    for n in (1..60).rev() {
        let word = if n < 10 {
            digit[n].to_owned()
        } else {
            format!(
                "{}십{}",
                if n < 20 { "" } else { digit[n / 10] },
                digit[n % 10]
            )
        };
        q = q.replace(&format!("{word}분"), &format!("{n}분"));
    }
    for (word, n) in [
        ("네시", "4시"),
        ("세시", "3시"),
        ("두시", "2시"),
        ("한시", "1시"),
        ("한시간", "1시간"),
        ("두시간", "2시간"),
        ("한분", "1분"),
    ] {
        q = q.replace(word, n);
    }
    q
}
fn numbers(q: &str) -> Vec<(usize, usize, u32)> {
    let bytes = q.as_bytes();
    let mut out = vec![];
    let mut i = 0;
    while i < bytes.len() {
        if !bytes[i].is_ascii_digit() {
            i += 1;
            continue;
        }
        let start = i;
        while i < bytes.len() && bytes[i].is_ascii_digit() {
            i += 1
        }
        if let Ok(n) = q[start..i].parse() {
            out.push((start, i, n));
        }
    }
    out
}
fn explicit_clock(q: &str) -> Option<(u32, u32)> {
    let am = ["morning", "a.m.", "am", "아침", "오전"]
        .iter()
        .any(|w| q.contains(w));
    let pm = ["afternoon", "evening", "p.m.", "pm", "오후", "저녁", "밤"]
        .iter()
        .any(|w| q.contains(w));
    let noon = q.replace("afternoon", "").contains("noon") || q.contains("정오");
    let midnight = q.contains("midnight") || q.contains("자정");
    if am && pm || noon && midnight {
        return None;
    }
    if noon || midnight {
        if !numbers(q).is_empty() {
            return None;
        }
        return Some((if noon { 12 } else { 0 }, 0));
    }
    if q.contains("quarterto") || q.contains("halfto") {
        return None;
    }
    let past = if q.contains("halfpast") {
        Some(30)
    } else if q.contains("quarterpast") {
        Some(15)
    } else {
        None
    };
    let mut found = vec![];
    for (start, end, mut hour) in numbers(q) {
        let tail = &q[end..];
        let prefix = &q[..start];
        if prefix.ends_with(':') {
            continue;
        }
        let mut minute = past.unwrap_or(0);
        if let Some(after) = tail.strip_prefix(':') {
            let digits = after.bytes().take_while(u8::is_ascii_digit).count();
            if digits != 2 {
                return None;
            }
            minute = after[..digits].parse().ok()?;
            // The number after ':' is not another hour.
        } else if let Some(after) = tail.strip_prefix('시') {
            if after.starts_with('간') {
                continue;
            }
            if after.starts_with('반') {
                minute = 30
            } else {
                let digits = after.bytes().take_while(u8::is_ascii_digit).count();
                if digits > 0 {
                    if !after[digits..].starts_with('분') {
                        return None;
                    }
                    minute = after[..digits].parse().ok()?;
                }
            }
        } else if !(tail.starts_with("am")
            || tail.starts_with("pm")
            || tail.starts_with("a.m.")
            || tail.starts_with("p.m.")
            || tail.starts_with("o'clock")
            || prefix.ends_with("at")
            || past.is_some() && prefix.ends_with("past")
            || (am || pm) && !prefix.ends_with(':') && numbers(q).len() == 1)
        {
            continue;
        }
        if hour > 23 || minute > 59 {
            return None;
        }
        if am || pm {
            if hour == 0 || hour > 12 {
                return None;
            }
            hour = hour % 12 + if pm { 12 } else { 0 };
        } else if !tail.starts_with(':') && hour <= 12 {
            return None;
        }
        found.push((hour, minute));
    }
    if found.len() == 1 { found.pop() } else { None }
}
pub(crate) fn civil_consistent(d: &MissionChatDecision, anchor: i64, zone: &str) -> bool {
    use chrono::{Days, NaiveDate, NaiveTime, Timelike};
    let raw = d.time_quote.as_deref().unwrap_or("");
    let mut civil = raw.to_owned();
    for token in
        raw.split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '/' | '_' | '+' | '-' | ':'))
    {
        if token.parse::<chrono_tz::Tz>().is_ok() {
            civil = civil.replace(token, "");
        }
    }
    let q = normalized_time(&civil);
    // Keep ISO dates separate from the clock before whitespace normalization;
    // otherwise "2026-12-07 23:15" becomes a spurious hour of 723.
    let explicit_dates = numbers(&civil)
        .into_iter()
        .filter_map(|(start, _, _)| {
            let value = civil.get(start..start + 10)?;
            if civil
                .as_bytes()
                .get(start + 10)
                .is_some_and(u8::is_ascii_digit)
            {
                return None;
            }
            NaiveDate::parse_from_str(value, "%Y-%m-%d")
                .ok()
                .map(|date| (start, start + 10, date))
        })
        .collect::<Vec<_>>();
    let mut clock_text = civil.clone();
    for (start, end, _) in explicit_dates.iter().rev() {
        clock_text.replace_range(*start..*end, " ");
    }
    let clock_quote = normalized_time(&clock_text);
    let mentioned_days = [
        (1, ["monday", "월요일"]),
        (2, ["tuesday", "화요일"]),
        (3, ["wednesday", "수요일"]),
        (4, ["thursday", "목요일"]),
        (5, ["friday", "금요일"]),
        (6, ["saturday", "토요일"]),
        (7, ["sunday", "일요일"]),
    ]
    .into_iter()
    .filter_map(|(day, names)| names.iter().any(|w| q.contains(w)).then_some(day))
    .collect::<Vec<u8>>();
    let time = match d.time.as_ref() {
        Some(
            MissionChatTime::Daily { time }
            | MissionChatTime::Weekly { time, .. }
            | MissionChatTime::LocalOnce { time, .. },
        ) => time,
        Some(MissionChatTime::Relative { seconds } | MissionChatTime::Interval { seconds }) => {
            let mut duration = 0u32;
            let mut count = 0;
            for (_, end, n) in numbers(&q) {
                let tail = &q[end..];
                let factor = if tail.starts_with("hour") || tail.starts_with("시간") {
                    3600
                } else if tail.starts_with("minute") || tail.starts_with('분') {
                    60
                } else if tail.starts_with("second") || tail.starts_with('초') {
                    1
                } else if tail.starts_with("day") || tail.starts_with('일') {
                    86400
                } else {
                    continue;
                };
                let Some(value) = n.checked_mul(factor).and_then(|n| duration.checked_add(n))
                else {
                    return false;
                };
                duration = value;
                count += 1;
            }
            return count > 0 && duration == *seconds;
        }
        None => return false,
    };
    let Ok(proposed) = NaiveTime::parse_from_str(time, "%H:%M") else {
        return false;
    };
    if explicit_clock(&clock_quote) != Some((proposed.hour(), proposed.minute())) {
        return false;
    }
    if let Some(MissionChatTime::Weekly { days, .. }) = &d.time {
        let mut actual = days.clone();
        actual.sort_unstable();
        actual.dedup();
        if actual.len() != days.len() || actual != mentioned_days || actual.is_empty() {
            return false;
        }
    } else if !mentioned_days.is_empty() {
        return false;
    }
    if let Some(MissionChatTime::LocalOnce { date, .. }) = &d.time {
        let Ok(zone) = zone.parse::<chrono_tz::Tz>() else {
            return false;
        };
        let Some(base) = chrono::DateTime::from_timestamp(anchor, 0)
            .map(|v| v.with_timezone(&zone).date_naive())
        else {
            return false;
        };
        let mut dates = vec![];
        for (names, days) in [
            (["tomorrow", "내일"], 1),
            (["today", "오늘"], 0),
            (["dayaftertomorrow", "모레"], 2),
        ] {
            if names.iter().any(|w| q.contains(w)) && !(days == 1 && q.contains("dayaftertomorrow"))
            {
                let Some(date) = base.checked_add_days(Days::new(days)) else {
                    return false;
                };
                dates.push(date);
            }
        }
        dates.extend(explicit_dates.iter().map(|(_, _, date)| *date));
        let actual = match date.as_str() {
            "today" => Some(base),
            "tomorrow" => base.checked_add_days(Days::new(1)),
            _ => NaiveDate::parse_from_str(date, "%Y-%m-%d").ok(),
        };
        if dates.len() != 1 || actual != dates.first().copied() {
            return false;
        }
    } else if !explicit_dates.is_empty()
        || ["today", "tomorrow", "오늘", "내일", "모레"]
            .iter()
            .any(|w| q.contains(w))
    {
        return false;
    }
    true
}
/// An explicit supported zone in the current text overrides browser metadata. Unknown zones clarify.
pub(crate) fn source_zone(text: &str, fallback: Option<&str>) -> Result<Option<String>> {
    let mut zones = vec![];
    let text = text.replace(['−', '–'], "-").replace('＋', "+");
    for token in text
        .split(|c: char| !c.is_ascii_alphanumeric() && !matches!(c, '/' | '_' | '+' | '-' | ':'))
    {
        if token.is_empty() {
            continue;
        }
        let upper = token.to_ascii_uppercase();
        let named = token.contains('/') && !token.contains("://") && !token.starts_with("http");
        if upper == "UTC" || named {
            let zone = if upper == "UTC" {
                "UTC".parse::<chrono_tz::Tz>()
            } else {
                token.parse::<chrono_tz::Tz>()
            }
            .map_err(|_| StoreError::InvalidInput)?;
            zones.push(zone.to_string());
        } else if token == "+"
            || token == "-"
            || token.contains(':') && token.contains('+')
            || upper.starts_with("UTC")
            || upper.starts_with("GMT")
            || ["KST", "JST", "EST", "EDT", "PST", "PDT", "CST", "CET"].contains(&upper.as_str())
            || token.starts_with(['+', '-'])
                && (token.contains(':')
                    || matches!(token.len(), 3 | 5)
                        && token[1..].bytes().all(|c| c.is_ascii_digit()))
        {
            return Err(StoreError::InvalidInput);
        }
    }
    zones.sort();
    zones.dedup();
    if zones.len() > 1 {
        return Err(StoreError::InvalidInput);
    }
    if let Some(zone) = zones.pop() {
        return Ok(Some(zone));
    }
    Ok(fallback.map(str::to_owned))
}
fn rule(
    d: &MissionChatDecision,
    anchor: i64,
    zone: Option<&str>,
    text: &str,
) -> Result<ScheduleRule> {
    let zone = source_zone(text, zone)?;
    if !civil_consistent(d, anchor, zone.as_deref().unwrap_or("UTC")) {
        return Err(StoreError::InvalidInput);
    }
    let time = d.time.as_ref().ok_or(StoreError::InvalidInput)?;
    if matches!(
        time,
        MissionChatTime::Daily { .. }
            | MissionChatTime::Weekly { .. }
            | MissionChatTime::LocalOnce { .. }
    ) && zone.is_none()
    {
        return Err(StoreError::InvalidInput);
    }
    let zone = zone.as_deref().unwrap_or("UTC");
    Ok(match time {
        MissionChatTime::Relative { seconds } if *seconds > 0 && *seconds <= 31536000 => {
            ScheduleRule::Once {
                at: anchor
                    .checked_add(i64::from(*seconds))
                    .ok_or(StoreError::InvalidInput)?,
            }
        }
        MissionChatTime::Interval { seconds } if *seconds >= 60 => ScheduleRule::Interval {
            anchor,
            seconds: *seconds,
        },
        MissionChatTime::Daily { time } => ScheduleRule::Daily {
            time: time.clone(),
            timezone: zone.into(),
        },
        MissionChatTime::Weekly { days, time } => ScheduleRule::Weekly {
            days: days.clone(),
            time: time.clone(),
            timezone: zone.into(),
        },
        MissionChatTime::LocalOnce { date, time } => {
            // All local gap/fold and relative civil-date interpretation uses the existing oracle.
            let spec = format!("{date} at {time}");
            let (rule, tail) = crate::schedules::chat::parser::rule(&spec, Some(zone), anchor)?;
            if !tail.trim().is_empty() {
                return Err(StoreError::InvalidInput);
            }
            rule
        }
        _ => return Err(StoreError::InvalidInput),
    })
}
fn candidates(
    c: &Connection,
    u: &UserContext,
    kind: Option<&str>,
) -> Result<Vec<(MissionChatTarget, String)>> {
    let mut v = Vec::new();
    if kind != Some("recurring") {
        let rows=c.prepare("SELECT id,revision,payload FROM missions WHERE tenant_id=?1 AND account_id=?2 AND state IN('queued','planning','executing','verifying','paused') ORDER BY rowid DESC LIMIT 2")?.query_map(params![u.tenant.0,u.account.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (id, revision, payload) in rows {
            let input: MissionCreate = decode(&payload)?;
            v.push((
                MissionChatTarget {
                    kind: "one_off".into(),
                    id,
                    revision,
                },
                input.task,
            ));
        }
    }
    if kind != Some("one_off") {
        let rows=c.prepare("SELECT s.id,s.revision,s.title FROM recurring_missions x JOIN reminder_schedules s ON s.tenant_id=x.tenant_id AND s.id=x.schedule_id WHERE x.tenant_id=?1 AND x.account_id=?2 AND s.state IN('active','paused') ORDER BY s.created_at DESC,s.id LIMIT 65")?.query_map(params![u.tenant.0,u.account.id],|r|Ok((r.get::<_,String>(0)?,r.get::<_,i64>(1)?,r.get::<_,String>(2)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
        for (id, revision, title) in rows {
            v.push((
                MissionChatTarget {
                    kind: "recurring".into(),
                    id,
                    revision,
                },
                title,
            ));
        }
    }
    Ok(v)
}
fn resolve_target(
    c: &Connection,
    u: &UserContext,
    d: &MissionChatDecision,
    selected: Option<&str>,
    receipt: &mut MissionChatReceipt,
) -> Result<MissionChatTarget> {
    let all = candidates(c, u, None)?;
    receipt.candidates = all
        .iter()
        .take(8)
        .map(|(target, title)| MissionChatCandidate {
            target: target.clone(),
            title: title.chars().take(160).collect(),
        })
        .collect();
    let selected: Option<MissionChatTarget> = selected.map(decode).transpose()?;
    let named = d.target_quote.as_ref().map(|q| {
        all.iter()
            .filter(|(_, title)| title == q)
            .collect::<Vec<_>>()
    });
    let target = if let Some(selected) = selected {
        if selected.kind == "one_off" {
            let row:Option<(String,bool,bool,i64)>=c.query_row(
                "SELECT payload,grant_enabled,uncertain,expires_ms FROM missions WHERE tenant_id=?1 AND id=?2 AND account_id=?3 AND revision=?4 AND state!='deleted'",
                params![u.tenant.0,selected.id,u.account.id,selected.revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
            if let Some((payload, enabled, uncertain, expiry)) = row {
                let input: MissionCreate = decode(&payload)?;
                if d.target_kind
                    .as_deref()
                    .is_some_and(|k| k != "selected" && k != "one_off")
                    || d.target_quote.as_ref().is_some_and(|q| q != &input.task)
                    || d.target_quote.is_none() && d.target_kind.as_deref() != Some("selected")
                {
                    return Err(StoreError::Conflict);
                }
                if !enabled {
                    return Err(StoreError::Unauthorized);
                }
                if uncertain {
                    receipt.state = "unavailable".into();
                    return Err(StoreError::Conflict);
                }
                if expiry <= super::clock()? {
                    return Err(StoreError::BudgetExhausted);
                }
            }
        }
        if !all.iter().any(|(v, _)| {
            v.id == selected.id && v.kind == selected.kind && v.revision == selected.revision
        }) || d
            .target_kind
            .as_deref()
            .is_some_and(|k| k != "selected" && k != selected.kind)
        {
            return Err(StoreError::Conflict);
        }
        if let Some(named) = named {
            if named.len() != 1 || named[0].0.id != selected.id || named[0].0.kind != selected.kind
            {
                return Err(StoreError::Conflict);
            }
        } else if d.target_kind.as_deref() != Some("selected") {
            return Err(StoreError::Conflict);
        }
        selected
    } else {
        if d.target_kind.as_deref() == Some("selected") {
            return Err(StoreError::NotFound);
        }
        let name = d.target_quote.as_ref().ok_or(StoreError::NotFound)?;
        let matching = all
            .iter()
            .filter(|(v, title)| d.target_kind.as_deref() == Some(v.kind.as_str()) && name == title)
            .collect::<Vec<_>>();
        if matching.len() != 1 {
            return Err(StoreError::NotFound);
        }
        matching[0].0.clone()
    };
    receipt.candidates.clear();
    Ok(target)
}
fn new_receipt(r: &Run, d: &MissionChatDecision) -> MissionChatReceipt {
    MissionChatReceipt {
        request_id: r.request.clone(),
        action: d.action.clone().unwrap_or_else(|| d.decision.clone()),
        target: None,
        state: "clarify".into(),
        next_local_slot: None,
        candidates: vec![],
    }
}
/// Commit only target guard transitions. A failed action or source confirmation cannot undo them.
fn preflight(db: &mut Database, l: &crate::RunLease) -> Result<Option<StoreError>> {
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let r = match crate::chat::guard(&tx, &db.provider_key, l) {
        Ok(r) => r,
        Err(e) => {
            tx.commit()?;
            return Err(e);
        }
    };
    let row:Option<(String,Option<String>)>=tx.query_row(
        "SELECT decision,target FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2 AND state='decided'",
        params![r.tenant,r.id],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
    let Some((raw, selected)) = row else {
        tx.commit()?;
        return Ok(None);
    };
    let u = original_authority(&tx, &r)?;
    let d: MissionChatDecision = decode(&raw)?;
    allowed_mission_decision(&r, &d)?;
    validate(&r.text, &d)?;
    let pristine:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND remote_calls=1 AND tool_calls=0) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|row|row.get(0))?;
    if !pristine || crate::media::has_context(&tx, &r.tenant, &r.id)? {
        return Err(StoreError::Conflict);
    }
    let mut error = None;
    if d.decision == "command" && matches!(d.action.as_deref(), Some("resume" | "steer")) {
        let mut receipt = new_receipt(&r, &d);
        match resolve_target(&tx, &u, &d, selected.as_deref(), &mut receipt) {
            Ok(target) if target.kind == "one_off" => {
                error = super::preflight_control_guard(&tx, &db.provider_key, &u, &target).err()
            }
            Err(StoreError::Unauthorized) => error = Some(StoreError::Unauthorized),
            Err(StoreError::BudgetExhausted) => error = Some(StoreError::BudgetExhausted),
            Err(StoreError::Conflict) if receipt.state == "unavailable" => {
                error = Some(StoreError::Conflict)
            }
            _ => {}
        }
    }
    tx.commit()?;
    Ok(error)
}
pub(crate) fn receipt(c: &Connection, t: &str, run: &str) -> Result<Option<MissionChatReceipt>> {
    let row: Option<String> = c
        .query_row(
            "SELECT receipt FROM mission_chat_commands WHERE tenant_id=?1 AND run_id=?2",
            params![t, run],
            |r| r.get(0),
        )
        .optional()?;
    let Some(raw) = row else { return Ok(None) };
    let mut out: MissionChatReceipt = decode(&raw)?;
    if let Some(target) = &out.target {
        let present: bool = if target.kind == "one_off" {
            c.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND id=?2 AND state NOT IN('deleted','quarantined'))",params![t,target.id],|r|r.get(0))?
        } else {
            c.query_row("SELECT EXISTS(SELECT 1 FROM reminder_schedules WHERE tenant_id=?1 AND id=?2 AND state!='revoked')",params![t,target.id],|r|r.get(0))?
        };
        if !present {
            out.target = None;
        }
    }
    out.candidates.retain(|candidate|{let target=&candidate.target;c.query_row("SELECT EXISTS(SELECT 1 FROM missions WHERE tenant_id=?1 AND id=?2 AND revision=?3 AND state NOT IN('deleted','quarantined')) OR EXISTS(SELECT 1 FROM reminder_schedules WHERE tenant_id=?1 AND id=?2 AND revision=?3 AND state IN('active','paused'))",params![t,target.id,target.revision],|r|r.get::<_,bool>(0)).unwrap_or(false)});
    Ok(Some(out))
}
fn finish(c: &Connection, r: &Run, message: &str, receipt: &MissionChatReceipt) -> Result<()> {
    c.execute(
        "INSERT INTO mission_chat_commands VALUES(?1,?2,?3)",
        params![r.tenant, r.id, encode(receipt)?],
    )?;
    c.execute(
        "UPDATE mission_chat_plans SET state='applied' WHERE tenant_id=?1 AND run_id=?2",
        params![r.tenant, r.id],
    )?;
    crate::chat::apply_patches(
        c,
        r,
        vec![crate::OutputPatch::Append {
            item: "mission-admission".into(),
            text: message.into(),
            phase: Some(MessagePhase::FinalAnswer),
            refusal: false,
        }],
    )?;
    crate::chat::transition(c, r, RunState::Completed, None)
}
impl Store {
    /// Retire the unused classifier route before ordinary execution; retries stay ordinary.
    pub async fn skip_mission_chat(&self, lease: &crate::RunLease) -> Result<()> {
        let l = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction()?;let r=match crate::chat::guard(&tx,&db.provider_key,&l){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};tx.execute("UPDATE mission_chat_plans SET state='passed' WHERE tenant_id=?1 AND run_id=?2 AND state='awaiting' AND EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND remote_calls=0)",params![r.tenant,r.id])?;tx.commit()?;Ok(())}).await
    }
    pub async fn mission_chat_planning(&self, lease: &crate::RunLease) -> Result<Option<Planning>> {
        let l = lease.clone();
        self.call(move|db|{let tx=db.conn.transaction()?;let r=match crate::chat::guard(&tx,&db.provider_key,&l){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
        retry_authority(&tx,&r)?;
        let attempted:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM mission_chat_plans p JOIN agent_execution e ON e.tenant_id=p.tenant_id AND e.run_id=p.run_id WHERE p.tenant_id=?1 AND p.run_id=?2 AND p.state='awaiting' AND e.remote_calls>0)",params![r.tenant,r.id],|r|r.get(0))?;if attempted{return Err(StoreError::Conflict)}
        let out=tx.query_row("SELECT anchor,timezone,target FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2 AND state='awaiting'",params![r.tenant,r.id],|r|Ok((r.get::<_,i64>(0)?,r.get::<_,Option<String>>(1)?,r.get::<_,Option<String>>(2)?))).optional()?.map(|(anchor,timezone,target)|Ok::<_,StoreError>(Planning{calendar_only:r.model.source==ModelSource::PersonalChatGpt,calendar:crate::calendar::agent::profile(&tx,&r.tenant,&r.id)?,anchor,timezone,selected_kind:target.map(|v|decode::<MissionChatTarget>(&v)).transpose()?.map(|v|v.kind)})).transpose()?;tx.commit()?;Ok(out)}).await
    }
    /// None means not yet decided; Some(false) continues ordinary chat without reclassification.
    pub async fn apply_mission_chat(&self, lease: &crate::RunLease) -> Result<Option<bool>> {
        let l = lease.clone();
        self.call(move|db|{
            // A fence may become due between preflight and control. Roll back action SQL,
            // settle only the same verified target guard, then retry local confirmation once.
            for pass in 0..2 {
            let preflight_error=preflight(db,&l)?;
            let guard_rejected=preflight_error.is_some();
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let r=match crate::chat::guard(&tx,&db.provider_key,&l){Ok(r)=>r,Err(e)=>{tx.commit()?;return Err(e)}};
            let row:Option<PlanRow>=tx.query_row("SELECT state,decision,anchor,timezone,target FROM mission_chat_plans WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).optional()?;
            let Some((state,decision,anchor,zone,selected))=row else{return Ok(None)};
            if state=="awaiting"{original_authority(&tx,&r)?;return Ok(None)}
 if state=="passed"{return Ok(Some(false))}
 if state!="decided"{return Err(StoreError::Conflict)}
            let u=original_authority(&tx,&r)?;let d:MissionChatDecision=decode(&decision.ok_or(StoreError::Corrupt)?)?;allowed_mission_decision(&r,&d)?;validate(&r.text,&d)?;
            if crate::media::has_context(&tx,&r.tenant,&r.id)?{return Err(StoreError::Conflict)}
            let pristine:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?2 AND remote_calls=1 AND tool_calls=0) AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2)",params![r.tenant,r.id],|r|r.get(0))?;
            if !pristine{return Err(StoreError::Conflict)}
            if d.decision=="no_command"{tx.execute("UPDATE mission_chat_plans SET state='passed' WHERE tenant_id=?1 AND run_id=?2",params![r.tenant,r.id])?;tx.commit()?;return Ok(Some(false))}
            let mut receipt=new_receipt(&r,&d);
            let mut message="언제 실행할지와 어떤 일을 맡길지 구체적으로 말씀해 주세요. / Please include the task and its execution time.".to_owned();
            if d.decision=="clarify" {message=match d.clarification.as_deref(){Some("time")=>"몇 시에 실행할까요? 오전/오후와 시간대를 포함해 자연스럽게 다시 말씀해 주세요. / What execution time should I use? Include AM/PM and timezone if needed.",Some("task")=>"어떤 일을 맡길까요? 수행할 내용을 말씀해 주세요. / What task should I take on?",Some("ambiguous")=>"지금 적용할 한 가지 요청을 말씀해 주세요. / Please state one current task or control request.",_=>"어느 업무에 적용할까요? / Which task do you mean?"}.into();}
            if d.decision=="unsupported"{message="요청하신 외부 발송·브라우저·볼트 쓰기는 이 업무 예약에서 지원하지 않아요. 읽기 업무의 결과는 맡긴 일 기록과 소식함에 남길 수 있어요. / External delivery or writes are not supported by delegated tasks.".into();receipt.state="unsupported".into();}
            else if d.decision=="command" {
                let action=d.action.as_deref().ok_or(StoreError::InvalidInput)?;
                tx.execute_batch("SAVEPOINT mission_chat_apply")?;
                let mut controlled_target=None;
                let attempt=(||->Result<()> {
                    if matches!(action,"create_now"|"create_scheduled"){
                        let task=d.task_quote.clone().ok_or(StoreError::InvalidInput)?;let constraints=d.constraints_quote.clone().unwrap_or_default();
                        let input=MissionCreate{request_id:r.request.clone(),task:task.clone(),constraints,checks:vec![MissionCheck::UserReview{description:"요청과 조건의 내용은 직접 검토가 필요합니다.".into()}],execution_mode:MissionMode::Durable};
                        let profile=r.tool_profile.clone().ok_or(StoreError::InvalidInput)?;
                        if action=="create_now"{let id=super::create_owner_in(&tx,&db.provider_key,&u,input,r.model.clone(),profile)?;receipt.target=Some(MissionChatTarget{kind:"one_off".into(),id,revision:1});receipt.state="queued".into();}
                        else{let rule=rule(&d,anchor,zone.as_deref(),&r.text)?;let title=task.chars().scan(0usize,|n,c|{*n+=c.len_utf8();(*n<=160).then_some(c)}).collect();let out=crate::recurring_missions::create_owner_in(&tx,&db.provider_key,&u,RecurringMissionCreate{request_id:r.request.clone(),template:input,schedule:ScheduleDraft{title,message:"예약한 읽기 업무".into(),rule,valid_until:None},changes_only:false},r.model.clone(),profile)?;receipt.target=Some(MissionChatTarget{kind:"recurring".into(),id:out.schedule.id,revision:out.schedule.revision});receipt.state=out.schedule.state;receipt.next_local_slot=out.schedule.next_local_slot;}
                        message="맡긴 일을 저장했어요. 실제 실행 결과는 맡긴 일에서 확인할 수 있어요. / Task saved. Execution results will appear in your task history.".into();return Ok(())
                    }
                    let target=resolve_target(&tx,&u,&d,selected.as_deref(),&mut receipt);
                    if let Some(error)=preflight_error {return Err(error)}
                    let target=target?;
                    if target.kind=="one_off"{controlled_target=Some(target.clone());let a=match action{"pause"=>MissionAction::Pause,"resume"=>MissionAction::Resume,"cancel"=>MissionAction::Cancel,"steer"=>MissionAction::Steer{instruction:d.instruction_quote.clone().ok_or(StoreError::InvalidInput)?},_=>return Err(StoreError::InvalidInput)};let out=super::control_owner_in(&tx,&db.provider_key,&u,&target.id,MissionControl{request_id:r.request.clone(),expected_revision:target.revision,action:a})?;receipt.target=Some(MissionChatTarget{revision:out.revision,..target});receipt.state=out.state;}
                    else {let out=if action=="edit_time"{let mut draft=ScheduleDraft{title:String::new(),message:"예약한 읽기 업무".into(),rule:rule(&d,anchor,zone.as_deref(),&r.text)?,valid_until:None};let row:(String,Option<i64>)=tx.query_row("SELECT title,valid_until FROM reminder_schedules WHERE tenant_id=?1 AND id=?2",params![r.tenant,target.id],|r|Ok((r.get(0)?,r.get(1)?)))?;draft.title=row.0;draft.valid_until=row.1;crate::recurring_missions::edit_owner_in(&tx,&db.provider_key,&u,&target.id,ScheduleEdit{expected_revision:target.revision,draft})?}else{let action=match action{"pause"=>"pause","resume"=>"resume","cancel"=>"revoke",_=>return Err(StoreError::InvalidInput)};crate::recurring_missions::control_owner_in(&tx,&db.provider_key,&u,&target.id,ScheduleControl{expected_revision:target.revision,action:action.into()})?};receipt.target=Some(MissionChatTarget{revision:out.schedule.revision,..target});receipt.state=out.schedule.state;receipt.next_local_slot=out.schedule.next_local_slot;}
                    message="요청하신 업무 제어를 적용했어요. 현재 실행 상태는 맡긴 일에서 확인할 수 있어요. / Task control applied.".into();Ok(())
                })();
                if attempt.is_ok(){tx.execute_batch("RELEASE mission_chat_apply")?;}
                if let Err(error)=attempt {tx.execute_batch("ROLLBACK TO mission_chat_apply;RELEASE mission_chat_apply")?;
                if pass==0 && matches!(error,StoreError::BudgetExhausted|StoreError::Conflict|StoreError::Unauthorized)
                    && let Some(target)=controlled_target {
                    // The source and exact owned target were validated in this transaction;
                    // no action SQL survives the savepoint rollback above.
                    let _=super::preflight_control_guard(&tx,&db.provider_key,&u,&target);
                    tx.commit()?;continue
                }
                match error{StoreError::Conflict if guard_rejected || receipt.state=="unavailable"=>{receipt.state="unavailable".into();message="대상 업무가 만료되었거나 결과 불명·근거 철회 상태라 변경하지 않았어요. 현재 맡긴 일 상태를 확인해 주세요. / The target task is expired, uncertain or revoked; no control was applied.".into()},StoreError::NotFound|StoreError::Conflict=>{message="어느 현재 업무에 적용할까요? 아래 업무를 선택한 뒤 요청을 다시 말씀해 주세요. / Select the current task and repeat your request.".into()},StoreError::InvalidInput=>{message="실행할 시각을 명확히 말씀해 주세요. 예: 매주 월요일 오전9시에 공개 논문 소식 요약해 줘. / Please include an unambiguous execution time.".into()},StoreError::BudgetExhausted=>{receipt.state="expired".into();message="대상 업무의 실행 시간 예산이 만료되어 재개하지 않았어요. 새 요청으로 맡겨 주세요. / The target task execution budget expired; it was not resumed.".into()},StoreError::Unauthorized=>{receipt.state="revoked".into();message="대상 업무의 실행 권한이 철회되어 변경하지 않았어요. / The target task authority was revoked; no control was applied.".into()},StoreError::Busy=>{message="지금은 맡긴 일의 실행 한도가 가득 찼어요. 새 업무는 저장하지 않았어요. / Task capacity is full; no new task was saved.".into()},_=>return Err(error)}}
            }else if d.clarification.as_deref()==Some("target"){message="어느 업무에 적용할까요? 현재 업무를 선택한 뒤 요청을 다시 말씀해 주세요. / Which task do you mean?".into();receipt.candidates=candidates(&tx,&u,None)?.into_iter().take(8).map(|(target,title)|MissionChatCandidate{target,title:title.chars().take(160).collect()}).collect();}
            finish(&tx,&r,&message,&receipt)?;tx.commit()?;return Ok(Some(true))
            }
            Err(StoreError::Conflict)
        }).await
    }
}
