//! Pure UTC/wall-clock recurrence. Gaps are local-slot skips, never invented UTC executions.
use super::*;
use chrono::{
    DateTime, Datelike, Days, LocalResult, NaiveDate, NaiveTime, TimeDelta, TimeZone, Timelike, Utc,
};
use chrono_tz::Tz;
pub(super) const TZDB: &str = chrono_tz::IANA_TZDB_VERSION;
const MAX_TIME: i64 = 4_102_444_800; // 2100-01-01; explicit bounded preview horizon.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(super) struct Slot {
    pub at: Option<i64>,
    pub due: i64,
    pub local: String,
}
pub(super) struct Plan {
    pub latest: Option<Slot>,
    pub gaps: Vec<Slot>,
    pub skipped: Option<(String, String, i64)>,
    pub next: Option<Slot>,
}
fn utc(value: i64) -> Result<DateTime<Utc>> {
    if !(0..=MAX_TIME).contains(&value) {
        return Err(StoreError::InvalidInput);
    }
    DateTime::from_timestamp(value, 0).ok_or(StoreError::InvalidInput)
}
pub(super) fn instant(value: i64) -> Result<()> {
    utc(value).map(|_| ())
}
pub(super) fn zone(value: &str) -> Result<Tz> {
    if value.len() > 64 {
        return Err(StoreError::InvalidInput);
    }
    value.parse().map_err(|_| StoreError::InvalidInput)
}
fn daily_parts(time: &str, timezone: &str) -> Result<(NaiveTime, Tz)> {
    let time_value =
        NaiveTime::parse_from_str(time, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    if time_value.format("%H:%M").to_string() != time {
        return Err(StoreError::InvalidInput);
    }
    Ok((time_value, zone(timezone)?))
}
fn daily_slot(date: NaiveDate, time: NaiveTime, tz: Tz) -> Result<Slot> {
    let local = date.and_time(time);
    let label = format!("{}@{}", local.format("%Y-%m-%dT%H:%M"), tz.name());
    match tz.from_local_datetime(&local) {
        LocalResult::Single(at) => Ok(Slot {
            at: Some(at.timestamp()),
            due: at.timestamp(),
            local: label,
        }),
        LocalResult::Ambiguous(a, b) => {
            let at = a.timestamp().min(b.timestamp());
            Ok(Slot {
                at: Some(at),
                due: at,
                local: label,
            })
        }
        LocalResult::None => {
            // The first valid local minute after the gap is only a wakeup deadline.
            // Even a historical skipped date is bounded by one day, not a backlog scan.
            for minute in 1..=1440 {
                let candidate = local
                    .checked_add_signed(TimeDelta::minutes(minute))
                    .ok_or(StoreError::InvalidInput)?;
                if let Some(at) = tz.from_local_datetime(&candidate).earliest() {
                    return Ok(Slot {
                        at: None,
                        due: at.timestamp(),
                        local: label,
                    });
                }
            }
            Err(StoreError::InvalidInput)
        }
    }
}
fn next_day(date: NaiveDate) -> Result<NaiveDate> {
    date.checked_add_days(Days::new(1))
        .ok_or(StoreError::InvalidInput)
}
fn previous_day(date: NaiveDate) -> Result<NaiveDate> {
    date.checked_sub_days(Days::new(1))
        .ok_or(StoreError::InvalidInput)
}
fn utc_slot(at: i64) -> Result<Slot> {
    utc(at)?;
    Ok(Slot {
        at: Some(at),
        due: at,
        local: format!("utc:{at}"),
    })
}
pub(super) fn after(rule: &ScheduleRule, after: i64) -> Result<Option<Slot>> {
    utc(after)?;
    match rule {
        ScheduleRule::Once { at } => {
            utc(*at)?;
            Ok(if *at > after {
                Some(utc_slot(*at)?)
            } else {
                None
            })
        }
        ScheduleRule::Interval { anchor, seconds } => {
            utc(*anchor)?;
            if !(60..=31_622_400).contains(seconds) {
                return Err(StoreError::InvalidInput);
            }
            let step = i64::from(*seconds);
            let n = if *anchor > after {
                0
            } else {
                (after - anchor) / step + 1
            };
            Ok(Some(utc_slot(
                anchor
                    .checked_add(n.checked_mul(step).ok_or(StoreError::InvalidInput)?)
                    .ok_or(StoreError::InvalidInput)?,
            )?))
        }
        ScheduleRule::Weekly {
            days,
            time,
            timezone,
        } => {
            weekly_days(days)?;
            let (time, tz) = daily_parts(time, timezone)?;
            let mut date = utc(after)?.with_timezone(&tz).date_naive();
            for _ in 0..15 {
                if selected(days, date) {
                    let slot = daily_slot(date, time, tz)?;
                    if slot.due > after {
                        if slot.due > MAX_TIME {
                            return Ok(None);
                        }
                        utc(slot.due)?;
                        return Ok(Some(slot));
                    }
                }
                date = next_day(date)?;
            }
            Err(StoreError::InvalidInput)
        }
        ScheduleRule::Daily { time, timezone } => {
            let (time, tz) = daily_parts(time, timezone)?;
            let mut date = utc(after)?.with_timezone(&tz).date_naive();
            for _ in 0..3 {
                let slot = daily_slot(date, time, tz)?;
                if slot.due > after {
                    utc(slot.due)?;
                    return Ok(Some(slot));
                }
                date = next_day(date)?;
            }
            Err(StoreError::InvalidInput)
        }
    }
}
pub(super) fn preview(rule: &ScheduleRule, now: i64) -> Result<SchedulePreview> {
    let mut slot = after(rule, now)?.ok_or(StoreError::InvalidInput)?;
    let mut skipped_gap = None;
    for _ in 0..3 {
        if let Some(at) = slot.at {
            return Ok(SchedulePreview {
                next_fire_at: at,
                local_slot: slot.local,
                timezone_db: TZDB.into(),
                skipped_gap,
            });
        }
        skipped_gap = Some(slot.local);
        slot = after(rule, slot.due)?.ok_or(StoreError::InvalidInput)?;
    }
    Err(StoreError::InvalidInput)
}
pub(super) fn plan(rule: &ScheduleRule, first: &Slot, now: i64) -> Result<Plan> {
    utc(now)?;
    if first.due > now {
        return Err(StoreError::Conflict);
    }
    match rule {
        ScheduleRule::Once { .. } => Ok(Plan {
            latest: Some(first.clone()),
            gaps: vec![],
            skipped: None,
            next: None,
        }),
        ScheduleRule::Interval { anchor, seconds } => {
            let step = i64::from(*seconds);
            if step < 60 {
                return Err(StoreError::Corrupt);
            }
            let at = anchor + ((now - anchor) / step) * step;
            let skipped_count = (at - first.due) / step;
            Ok(Plan {
                latest: Some(utc_slot(at)?),
                gaps: vec![],
                skipped: if skipped_count > 0 {
                    Some((
                        first.local.clone(),
                        utc_slot(at - step)?.local,
                        skipped_count,
                    ))
                } else {
                    None
                },
                next: at
                    .checked_add(step)
                    .filter(|at| *at <= MAX_TIME)
                    .map(utc_slot)
                    .transpose()?,
            })
        }
        ScheduleRule::Weekly {
            days,
            time,
            timezone,
        } => {
            weekly_days(days)?;
            let (time, tz) = daily_parts(time, timezone)?;
            let from = NaiveDate::parse_from_str(
                first.local.get(..10).ok_or(StoreError::Corrupt)?,
                "%Y-%m-%d",
            )
            .map_err(|_| StoreError::Corrupt)?;
            let mut date = utc(now)?.with_timezone(&tz).date_naive();
            for _ in 0..8 {
                if selected(days, date) && daily_slot(date, time, tz)?.due <= now {
                    break;
                }
                date = previous_day(date)?;
            }
            let mut gaps = if first.at.is_none() {
                vec![first.clone()]
            } else {
                vec![]
            };
            let mut latest = None;
            for _ in 0..15 {
                if date < from {
                    break;
                }
                if selected(days, date) {
                    let slot = daily_slot(date, time, tz)?;
                    if slot.at.is_some() {
                        latest = Some(slot);
                        break;
                    }
                    if !gaps.iter().any(|s| s.local == slot.local) {
                        gaps.push(slot);
                    }
                }
                date = previous_day(date)?;
            }
            let begin = if first.at.is_none() {
                next_day(from)?
            } else {
                from
            };
            let skipped = if date > begin {
                let end = previous_day(date)?;
                let count = weekly_count(days, begin, end);
                if count > 0 {
                    let mut start = begin;
                    for _ in 0..7 {
                        if selected(days, start) {
                            break;
                        }
                        start = next_day(start)?;
                    }
                    let mut end = end;
                    for _ in 0..7 {
                        if selected(days, end) {
                            break;
                        }
                        end = previous_day(end)?;
                    }
                    Some((
                        daily_slot(start, time, tz)?.local,
                        daily_slot(end, time, tz)?.local,
                        count,
                    ))
                } else {
                    None
                }
            } else {
                None
            };
            Ok(Plan {
                latest,
                gaps,
                skipped,
                next: after(rule, now)?,
            })
        }
        ScheduleRule::Daily { time, timezone } => {
            let (time, tz) = daily_parts(time, timezone)?;
            let from = NaiveDate::parse_from_str(
                first.local.get(..10).ok_or(StoreError::Corrupt)?,
                "%Y-%m-%d",
            )
            .map_err(|_| StoreError::Corrupt)?;
            let mut last = utc(now)?.with_timezone(&tz).date_naive();
            if daily_slot(last, time, tz)?.due > now {
                last = previous_day(last)?;
            }
            let mut date = last;
            let mut gaps = if first.at.is_none() {
                vec![first.clone()]
            } else {
                vec![]
            };
            let mut latest = None;
            for _ in 0..8 {
                if date < from {
                    break;
                }
                let slot = daily_slot(date, time, tz)?;
                if slot.at.is_some() {
                    latest = Some(slot);
                    break;
                }
                if !gaps.iter().any(|old| old.local == slot.local) {
                    gaps.push(slot);
                }
                date = previous_day(date)?;
            }
            let range_from = if first.at.is_none() {
                next_day(from)?
            } else {
                from
            };
            let skipped = if date >= range_from && date > range_from {
                Some((
                    daily_slot(range_from, time, tz)?.local,
                    daily_slot(previous_day(date)?, time, tz)?.local,
                    (date - range_from).num_days(),
                ))
            } else {
                None
            };
            Ok(Plan {
                latest,
                gaps,
                skipped,
                next: {
                    let slot = daily_slot(next_day(last)?, time, tz)?;
                    (slot.due <= MAX_TIME).then_some(slot)
                },
            })
        }
    }
}
pub(super) fn quiet(settings: &QuietHours, now: i64) -> Result<bool> {
    if !settings.enabled {
        return Ok(false);
    }
    let local = utc(now)?.with_timezone(&zone(&settings.timezone)?);
    let minute = (local.hour() * 60 + local.minute()) as u16;
    Ok(if settings.start_minute < settings.end_minute {
        minute >= settings.start_minute && minute < settings.end_minute
    } else {
        minute >= settings.start_minute || minute < settings.end_minute
    })
}

fn weekly_days(days: &[u8]) -> Result<()> {
    if days.is_empty()
        || days.len() > 7
        || days.iter().any(|d| !(1..=7).contains(d))
        || days.windows(2).any(|v| v[0] >= v[1])
    {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn selected(days: &[u8], date: NaiveDate) -> bool {
    days.contains(&(date.weekday().number_from_monday() as u8))
}
fn weekly_count(days: &[u8], from: NaiveDate, to: NaiveDate) -> i64 {
    if to < from {
        return 0;
    }
    let n = (to - from).num_days() + 1;
    let mut count = (n / 7) * days.len() as i64;
    for i in 0..n % 7 {
        if let Some(date) = from.checked_add_days(Days::new(i as u64))
            && selected(days, date)
        {
            count += 1
        }
    }
    count
}
