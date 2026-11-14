//! Store-owned scope, independently derived from the complete immutable message.
use super::*;
use chrono::{Days, LocalResult, NaiveDate, TimeZone};
use moosuri_core::{MemoryDate, MemoryTemporal, memory_temporal};

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Scope {
    Undated,
    Dated {
        valid_from: Option<i64>,
        valid_until: Option<i64>,
        timezone: String,
    },
    Clarify,
}
fn midnight(date: MemoryDate, zone: chrono_tz::Tz, next: bool) -> Option<i64> {
    let date = NaiveDate::from_ymd_opt(date.year, date.month, date.day)?;
    let date = if next {
        date.checked_add_days(Days::new(1))?
    } else {
        date
    };
    match zone.from_local_datetime(&date.and_hms_opt(0, 0, 0)?) {
        LocalResult::Single(value) => Some(value.timestamp()),
        LocalResult::Ambiguous(_, _) | LocalResult::None => None,
    }
}
pub(super) fn derive(run: &chat::Run, intent: &MemoryCommandIntent) -> Scope {
    let (from, through) = match memory_temporal(&run.text) {
        MemoryTemporal::Undated => return Scope::Undated,
        MemoryTemporal::Clarify => return Scope::Clarify,
        MemoryTemporal::Dated { from, through } => (from, through),
    };
    if intent.action == MemoryCommandAction::Forget {
        return Scope::Clarify;
    }
    let Some(timezone) = run.original_timezone.as_deref() else {
        return Scope::Clarify;
    };
    let Ok(zone) = timezone.parse::<chrono_tz::Tz>() else {
        return Scope::Clarify;
    };
    let start = from.map(|date| midnight(date, zone, false));
    let end = through.map(|date| midnight(date, zone, true));
    if start.is_some_and(|v| v.is_none()) || end.is_some_and(|v| v.is_none()) {
        return Scope::Clarify;
    }
    let (valid_from, valid_until) = (start.flatten(), end.flatten());
    if valid_from
        .into_iter()
        .chain(valid_until)
        .any(|v| !(-62_135_596_800..=253_402_300_799).contains(&v))
    {
        return Scope::Clarify;
    }
    if matches!((valid_from, valid_until), (Some(a), Some(b)) if a >= b) {
        return Scope::Clarify;
    }
    Scope::Dated {
        valid_from,
        valid_until,
        timezone: zone.to_string(),
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SealedIntent {
    version: u8,
    pub intent: MemoryCommandIntent,
    pub scope: Scope,
}
pub(super) fn seal(run: &chat::Run, intent: &MemoryCommandIntent) -> Result<String> {
    encode(&SealedIntent {
        version: 1,
        intent: intent.clone(),
        scope: derive(run, intent),
    })
}
pub(super) fn load(run: &chat::Run, encoded: &str) -> Result<SealedIntent> {
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Stored {
        Sealed(SealedIntent),
        Legacy(MemoryCommandIntent),
    }
    let sealed = match decode::<Stored>(encoded)? {
        Stored::Sealed(sealed) => sealed,
        Stored::Legacy(intent) => {
            // Old undated commands remain recoverable. Old temporal commands
            // have no sealed scope and must never fall back to null bounds.
            if derive(run, &intent) != Scope::Undated {
                return Err(StoreError::Corrupt);
            }
            SealedIntent {
                version: 1,
                intent,
                scope: Scope::Undated,
            }
        }
    };
    validate_intent(&run.text, &sealed.intent)?;
    if sealed.version != 1 || sealed.scope != derive(run, &sealed.intent) {
        return Err(StoreError::Corrupt);
    }
    Ok(sealed)
}
pub(super) fn confirmation(
    conn: &Connection,
    run: &chat::Run,
    outcome: &MemoryCommandOutcome,
) -> Result<String> {
    let mut text = super::confirmation(outcome).to_owned();
    let MemoryCommandOutcome::Applied {
        action: MemoryCommandAction::Remember | MemoryCommandAction::Correct,
        memory_id: Some(id),
        revision: Some(revision),
    } = outcome
    else {
        return Ok(text);
    };
    let encoded: String = conn.query_row("SELECT draft FROM knowledge_memory_versions WHERE tenant_id=?1 AND memory_id=?2 AND revision=?3", params![run.tenant,id,revision], |r|r.get(0))?;
    let draft: MemoryDraft = decode(&encoded)?;
    if draft.valid_from.is_none() && draft.valid_until.is_none() {
        return Ok(text);
    }
    let zone = run
        .original_timezone
        .as_deref()
        .ok_or(StoreError::Corrupt)?
        .parse::<chrono_tz::Tz>()
        .map_err(|_| StoreError::Corrupt)?;
    let local_date = |bound: i64| -> Result<String> {
        Ok(chrono::DateTime::from_timestamp(bound, 0)
            .ok_or(StoreError::Corrupt)?
            .with_timezone(&zone)
            .format("%Y-%m-%d")
            .to_string())
    };
    let start = draft.valid_from.map(local_date).transpose()?;
    let end = draft
        .valid_until
        .map(|bound| local_date(bound.checked_sub(1).ok_or(StoreError::Corrupt)?))
        .transpose()?;
    let dates = match (start, end) {
        (Some(start), Some(end)) => format!("{start}부터 {end}까지"),
        (Some(start), None) => format!("{start}부터"),
        (None, Some(end)) => format!("{end}까지"),
        (None, None) => return Err(StoreError::Corrupt),
    };
    text.push_str(&format!("\n{dates} 적용할게요. ({zone})"));
    Ok(text)
}
