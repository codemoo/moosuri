//! Exact civil expressions shared by reminders, mission proposals and Calendar.
//! No history, provider or authority creation. All dates use a supplied instant
//! and IANA zone; folds/gaps are rejected for conversational one-shot admission.
use super::*;
use chrono::{Datelike, Days, NaiveDate, NaiveTime, TimeZone};
pub(crate) fn normalized_time(q: &str) -> String {
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
pub(crate) fn numbers(q: &str) -> Vec<(usize, usize, u32)> {
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
pub(crate) fn explicit_clock(q: &str) -> Option<(u32, u32)> {
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

fn compact(text: &str) -> String {
    text.to_ascii_lowercase()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect()
}
const WEEKDAYS: [(&str, &str, u32); 7] = [
    ("monday", "월요일", 0),
    ("tuesday", "화요일", 1),
    ("wednesday", "수요일", 2),
    ("thursday", "목요일", 3),
    ("friday", "금요일", 4),
    ("saturday", "토요일", 5),
    ("sunday", "일요일", 6),
];
fn expression(value: &str, base: NaiveDate) -> Result<Option<NaiveDate>> {
    let q = compact(value);
    let relative = match q.as_str() {
        "today" | "오늘" => Some(0),
        "tomorrow" | "내일" => Some(1),
        "dayaftertomorrow" | "모레" => Some(2),
        "글피" => Some(3),
        _ => None,
    };
    if let Some(days) = relative {
        return base
            .checked_add_days(Days::new(days))
            .map(Some)
            .ok_or(StoreError::InvalidInput);
    }
    if let Ok(date) = NaiveDate::parse_from_str(&q, "%Y-%m-%d")
        && date.format("%Y-%m-%d").to_string() == q
    {
        return Ok(Some(date));
    }
    for (en, ko, day) in WEEKDAYS {
        let week = if q == en || q == ko {
            None
        } else if [
            format!("this{en}"),
            format!("thisweek{en}"),
            format!("이번{ko}"),
            format!("이번주{ko}"),
        ]
        .contains(&q)
        {
            Some(0)
        } else if [format!("nextweek{en}"), format!("다음주{ko}")].contains(&q) {
            Some(1)
        } else {
            continue;
        };
        let current = base.weekday().num_days_from_monday();
        let date = if let Some(week) = week {
            base.checked_sub_days(Days::new(u64::from(current)))
                .and_then(|m| m.checked_add_days(Days::new(u64::from(day) + week * 7)))
        } else {
            base.checked_add_days(Days::new(u64::from((day + 7 - current) % 7)))
        };
        return date.map(Some).ok_or(StoreError::InvalidInput);
    }
    Ok(None)
}
pub(crate) fn ambiguous(text: &str) -> bool {
    let q = compact(text);
    q.contains("weekend")
        || q.contains("주말")
        || WEEKDAYS
            .iter()
            .any(|(en, ko, _)| q.contains(&format!("next{en}")) || q.contains(&format!("다음{ko}")))
}
pub(crate) fn weekly_authority(text: &str) -> bool {
    let q = compact(text);
    q.contains("매주")
        || q.contains("weekly")
        || WEEKDAYS.iter().any(|(en, ko, _)| {
            [
                format!("every{en}"),
                format!("each{en}"),
                format!("on{en}s"),
                format!("{ko}마다"),
            ]
            .iter()
            .any(|v| q.contains(v))
        })
}
pub(crate) fn scoped_week(text: &str) -> bool {
    let q = compact(text);
    ["nextweek", "thisweek", "다음주", "이번주"]
        .iter()
        .any(|v| q.contains(v))
        || WEEKDAYS
            .iter()
            .any(|(en, ko, _)| q.contains(&format!("this{en}")) || q.contains(&format!("이번{ko}")))
}
fn boundaries(text: &str) -> impl Iterator<Item = usize> + '_ {
    text.char_indices().map(|(i, c)| i + c.len_utf8())
}
pub(crate) fn date_prefix(
    text: &str,
    anchor: i64,
    zone: &str,
) -> Result<Option<(NaiveDate, usize)>> {
    let z: chrono_tz::Tz = zone.parse().map_err(|_| StoreError::InvalidInput)?;
    let base = chrono::DateTime::from_timestamp(anchor, 0)
        .ok_or(StoreError::InvalidInput)?
        .with_timezone(&z)
        .date_naive();
    let mut found = None;
    for end in boundaries(text).take_while(|n| *n <= 128) {
        if text[..end].is_ascii()
            && text[end..]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic())
        {
            continue;
        }
        if let Some(date) = expression(&text[..end], base)? {
            found = Some((date, end));
        }
    }
    Ok(found)
}
pub(crate) fn date_mentions(text: &str, anchor: i64, zone: &str) -> Result<Vec<NaiveDate>> {
    if ambiguous(text) {
        return Err(StoreError::InvalidInput);
    }
    let mut found = vec![];
    let mut next = 0;
    let mut next_week = false;
    let mut this_week = false;
    for (at, _) in text.char_indices() {
        if at < next || at > 0 && text.as_bytes()[at - 1].is_ascii_alphanumeric() {
            continue;
        }
        if let Some((date, len)) = date_prefix(&text[at..], anchor, zone)? {
            let q = compact(&text[at..at + len]);
            next_week |= q.starts_with("nextweek") || q.starts_with("다음주");
            this_week |= q.starts_with("thisweek") || q.starts_with("이번주");
            found.push(date);
            next = at + len;
        }
    }
    let q = compact(text);
    if (q.contains("nextweek") || q.contains("다음주")) && !next_week
        || (q.contains("thisweek") || q.contains("이번주")) && !this_week
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(found)
}
/// Proposed normalized dates are checked against the exact quoted civil origin.
/// Raw supported expression values avoid asking the model to compute civil dates.
pub(crate) fn date_value(value: &str, quote: &str, anchor: i64, zone: &str) -> Result<NaiveDate> {
    let dates = date_mentions(quote, anchor, zone)?;
    let [expected] = dates.as_slice() else {
        return Err(StoreError::InvalidInput);
    };
    let (actual, len) = date_prefix(value, anchor, zone)?.ok_or(StoreError::InvalidInput)?;
    if !value[len..].trim().is_empty() || actual != *expected {
        return Err(StoreError::InvalidInput);
    }
    Ok(actual)
}
pub(crate) fn local_instant(date: NaiveDate, time: NaiveTime, zone: &str) -> Result<i64> {
    let zone: chrono_tz::Tz = zone.parse().map_err(|_| StoreError::InvalidInput)?;
    zone.from_local_datetime(&date.and_time(time))
        .single()
        .map(|t| t.timestamp())
        .ok_or(StoreError::InvalidInput)
}
/// Cheap cue only; this never creates a date or action and never invokes a model.
pub(crate) fn day_cue(text: &str) -> bool {
    let q = compact(text);
    [
        "today",
        "tomorrow",
        "dayaftertomorrow",
        "today",
        "오늘",
        "내일",
        "모레",
        "글피",
        "thisweek",
        "nextweek",
        "이번주",
        "다음주",
        "weekend",
        "주말",
    ]
    .iter()
    .any(|p| q.starts_with(p))
        || WEEKDAYS.iter().any(|(en, ko, _)| {
            [
                en.to_string(),
                ko.to_string(),
                format!("this{en}"),
                format!("next{en}"),
                format!("이번{ko}"),
                format!("다음{ko}"),
            ]
            .iter()
            .any(|p| q.starts_with(p))
        })
}
/// Exact prefix grammar around the shared normalization/clock oracle. Candidate
/// scanning retains raw UTF8 offsets and can never consume reminder body prose.
fn exact_clock(raw: &str) -> Option<(u32, u32)> {
    let q = normalized_time(raw);
    let mut v = q.as_str();
    if let Some(rest) = v.strip_prefix("at") {
        v = rest
    }
    for marker in ["오전", "오후", "아침", "저녁", "밤"] {
        if let Some(rest) = v.strip_prefix(marker) {
            v = rest;
            break;
        }
    }
    if matches!(v, "noon" | "midnight" | "정오" | "자정") {
        return explicit_clock(&q);
    }
    if let Some(rest) = v
        .strip_prefix("halfpast")
        .or_else(|| v.strip_prefix("quarterpast"))
    {
        v = rest
    }
    let korean = raw.chars().any(|c| ('가'..='힣').contains(&c));
    let digits = v.bytes().take_while(u8::is_ascii_digit).count();
    if digits == 0 {
        return None;
    }
    v = &v[digits..];
    if korean && !v.starts_with(['시', ':']) {
        return None;
    }
    if let Some(rest) = v.strip_prefix(':') {
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        if n != 2 {
            return None;
        }
        v = &rest[n..];
    } else if let Some(rest) = v.strip_prefix('시') {
        v = rest;
        if let Some(rest) = v.strip_prefix('반') {
            v = rest
        } else {
            let n = v.bytes().take_while(u8::is_ascii_digit).count();
            if n > 0 {
                v = v[n..].strip_prefix('분')?
            }
        }
    }
    if ![
        "",
        "am",
        "pm",
        "a.m.",
        "p.m.",
        "inthemorning",
        "intheafternoon",
        "intheevening",
        "o'clock",
    ]
    .contains(&v)
    {
        return None;
    }
    explicit_clock(&q)
}
pub(crate) fn clock_prefix(text: &str) -> Option<(String, &str)> {
    let text = text.trim_start();
    let mut found = None;
    for end in boundaries(text).take_while(|n| *n <= 128) {
        if text[end..]
            .chars()
            .next()
            .is_some_and(|c| c.is_ascii_digit())
            || text[..end].is_ascii()
                && text[end..]
                    .chars()
                    .next()
                    .is_some_and(|c| c.is_ascii_alphabetic())
        {
            continue;
        }
        if let Some((h, m)) = exact_clock(&text[..end]) {
            found = Some((format!("{h:02}:{m:02}"), end))
        }
    }
    let (time, end) = found?;
    let tail = text[end..].trim_start();
    // A malformed supplied minute/hour is not reminder content. Never fall
    // back from 09:60/오전13시/9시60분 to an earlier valid numeric prefix.
    // A Sino-Korean numeral immediately followed by 분 is still a supplied
    // minute component. Check before stripping 에, which explicitly starts body.
    let minute_tail = tail.trim_start_matches(|c: char| {
        c.is_whitespace() || "영공일이삼사오육칠팔구십백천만".contains(c)
    });
    let invalid_native_minute = minute_tail.len() < tail.len() && minute_tail.starts_with('분');
    if tail.chars().next().is_some_and(|c| c.is_ascii_digit())
        || invalid_native_minute
        || tail.starts_with([':', '시', '분', '반'])
    {
        return None;
    }
    if ["am", "pm", "a.m.", "p.m.", "오전", "오후"]
        .iter()
        .any(|p| tail.starts_with(p))
    {
        return None;
    }
    let tail = tail.strip_prefix('에').unwrap_or(tail).trim_start();
    Some((time, tail))
}

/// A selected temporal quote must contain its complete maximal source span.
/// Only overlapping spans matter: dates inside unrelated task/title prose never
/// supply temporal authority. Explicitly quoted typed data is excluded as data.
pub(crate) fn complete_quote(
    text: &str,
    quote: &str,
    data: &[&str],
    anchor: i64,
    zone: &str,
    clocks: bool,
) -> bool {
    if quote.trim().is_empty() {
        return false;
    }
    let mut ignored = vec![];
    for literal in data.iter().filter(|v| !v.is_empty() && **v != text) {
        for (at, _) in text.match_indices(*literal) {
            let end = at + literal.len();
            let before = text[..at].chars().next_back();
            let after = text[end..].chars().next();
            if matches!(
                (before, after),
                (Some('"'), Some('"'))
                    | (Some('\''), Some('\''))
                    | (Some('`'), Some('`'))
                    | (Some('“'), Some('”'))
                    | (Some('‘'), Some('’'))
            ) {
                ignored.push((at, end));
            }
        }
    }
    let mut used = false;
    for (start, _) in text.match_indices(quote) {
        let end = start + quote.len();
        if ignored.iter().any(|(a, b)| *a <= start && end <= *b) {
            continue;
        }
        used = true;
        let mut lower = start.saturating_sub(128);
        while !text.is_char_boundary(lower) {
            lower += 1
        }
        let mut upper = (end + 128).min(text.len());
        while !text.is_char_boundary(upper) {
            upper -= 1
        }
        let mut source_clocks = vec![];
        for (offset, ch) in text[lower..upper].char_indices() {
            let at = lower + offset;
            if ch.is_whitespace() || at > 0 && text.as_bytes()[at - 1].is_ascii_alphanumeric() {
                continue;
            }
            if ignored.iter().any(|(a, b)| *a <= at && at < *b) {
                continue;
            }
            let raw = &text[at..upper];
            let mut dates = vec![];
            if let Ok(Some((_, n))) = date_prefix(raw, anchor, zone) {
                let suffix = raw[n..].trim_start();
                dates.push(if suffix.starts_with("마다") {
                    raw.len() - suffix.len() + "마다".len()
                } else {
                    n
                })
            }
            for prefix in ["every ", "each ", "weekly ", "매주", "next ", "다음"] {
                if raw.to_ascii_lowercase().starts_with(prefix) {
                    let rest = &raw[prefix.len()..];
                    if let Ok(Some((_, n))) = date_prefix(rest, anchor, zone) {
                        dates.push(prefix.len() + n)
                    }
                }
            }
            for n in dates {
                let stop = at + raw[..n].trim_end().len();
                if at < end && start < stop && !(start <= at && stop <= end) {
                    return false;
                }
            }
            if clocks {
                // Retain even an invalid leading marker, e.g. 오후13시:
                // selecting only the valid 13시 may not erase that marker.
                for marker in ["오전", "오후", "아침", "저녁", "밤", "am ", "pm "] {
                    if raw.to_ascii_lowercase().starts_with(marker)
                        && let Some((_, tail)) = clock_prefix(&raw[marker.len()..])
                    {
                        let stop = at + raw.len() - tail.len();
                        let stop = at
                            + text[at..stop]
                                .trim_end_matches(|c: char| c.is_whitespace() || c == '에')
                                .len();
                        if at < end && start < stop && !(start <= at && stop <= end) {
                            return false;
                        }
                    }
                }
            }
            if clocks && let Some((_, tail)) = clock_prefix(raw) {
                // "at" is a delimiter, not civil authority: Calendar may quote
                // just "9AM" from "at 9AM". AM/PM and native markers stay bound.
                let clock_start = if raw.to_ascii_lowercase().starts_with("at") {
                    at + 2 + raw[2..].len() - raw[2..].trim_start().len()
                } else {
                    at
                };
                let stop = at + raw.len() - tail.len();
                let stop = at
                    + text[at..stop]
                        .trim_end_matches(|c: char| c.is_whitespace() || c == '에')
                        .len();
                if clock_start < end && start < stop && !(start <= clock_start && stop <= end) {
                    return false;
                }
                source_clocks.push((at, stop));
            }
        }
        if clocks {
            for (offset, ch) in quote.char_indices() {
                if ch.is_whitespace()
                    || offset > 0 && quote.as_bytes()[offset - 1].is_ascii_alphanumeric()
                {
                    continue;
                }
                let raw = &quote[offset..];
                if let Some((_, tail)) = clock_prefix(raw) {
                    let stop = offset + raw.len() - tail.len();
                    let stop = offset
                        + quote[offset..stop]
                            .trim_end_matches(|c: char| c.is_whitespace() || c == '에')
                            .len();
                    if !source_clocks
                        .iter()
                        .any(|(a, b)| *a <= start + offset && start + stop <= *b)
                    {
                        return false;
                    }
                }
            }
        }
    }
    used
}
