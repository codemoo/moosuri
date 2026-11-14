//! Explicit current-user memory instructions. Never derive authority from retrieved text.
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MemoryCommandAction {
    Remember,
    Correct,
    Forget,
}

// Literal private user text must never be formatted by diagnostic Debug output.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MemoryCommandIntent {
    pub action: MemoryCommandAction,
    pub intent_quote: String,
    pub fact_quote: Option<String>,
    pub subject: Option<String>,
    pub target_quote: Option<String>,
}

/// Closed, literal date-only scope grammar. Timestamps are never model output.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct MemoryDate {
    pub year: i32,
    pub month: u32,
    pub day: u32,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemoryTemporal {
    Undated,
    Dated {
        from: Option<MemoryDate>,
        through: Option<MemoryDate>,
    },
    Clarify,
}
/// Guidance for recognized unsupported scope forms; not a general semantic parser.
pub const MEMORY_TEMPORAL_GUIDANCE: &str = "날짜는 메시지 앞에 ‘2027-01-01부터 2027-12-31까지, 기억할 사실을 기억해 줘’처럼 써 주세요. ‘2027년 1월 1일부터,’ 또는 종료일만도 지원합니다. 종료일은 그날까지 포함됩니다. 시간·상대 날짜·다른 표현은 기억 편집기에서 적용 시각을 지정해 주세요. 메시지의 시간대가 없으면 편집기를 이용해 주세요.";

fn literal_date(value: &str) -> Option<MemoryDate> {
    fn number(v: &str) -> Option<u32> {
        (!v.is_empty() && v.bytes().all(|c| c.is_ascii_digit()))
            .then(|| v.parse().ok())
            .flatten()
    }
    let (year, month, day) = if value.contains('-') {
        let parts: Vec<_> = value.split('-').collect();
        if parts.len() != 3 || parts[0].len() != 4 || parts[1].len() != 2 || parts[2].len() != 2 {
            return None;
        }
        (number(parts[0])?, number(parts[1])?, number(parts[2])?)
    } else {
        let (year, rest) = value.split_once('년')?;
        let (month, day) = rest.split_once('월')?;
        let (year, month, day) = (year.trim(), month.trim(), day.strip_suffix('일')?.trim());
        if year.len() != 4 || month.len() > 2 || day.len() > 2 {
            return None;
        }
        (number(year)?, number(month)?, number(day)?)
    };
    let leap = year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if year == 0 || year > 9999 || day == 0 || day > days {
        return None;
    }
    Some(MemoryDate {
        year: year as i32,
        month,
        day,
    })
}
// Ambiguous words (기준, 적용, through, until) also occur in ordinary facts.
// Require a nearby explicit calendar/relative anchor or an explicit scope label.
fn korean_time_context(value: &str) -> bool {
    let chars: Vec<_> = value.chars().collect();
    chars.windows(2).any(|pair| {
        pair[0].is_ascii_digit()
            && matches!(
                pair[1],
                '-' | '/' | ':' | '년' | '월' | '일' | '시' | '분' | '초'
            )
    }) || [
        "오늘",
        "내일",
        "모레",
        "어제",
        "다음주",
        "다음달",
        "다음해",
        "이번주",
        "이번달",
        "올해",
        "내년",
        "현재",
        "지금",
        "앞으로",
    ]
    .iter()
    .any(|v| value.contains(v))
}
fn english_time_context(tail: &str) -> bool {
    let tail = tail.trim_start();
    let token = tail
        .split_whitespace()
        .next()
        .unwrap_or("")
        .trim_matches(|c: char| c.is_ascii_punctuation());
    tail.starts_with(|c: char| c.is_ascii_digit())
        || [
            "today",
            "tomorrow",
            "next",
            "this",
            "yesterday",
            "now",
            "immediately",
            "jan",
            "january",
            "feb",
            "february",
            "mar",
            "march",
            "apr",
            "april",
            "may",
            "jun",
            "june",
            "jul",
            "july",
            "aug",
            "august",
            "sep",
            "september",
            "oct",
            "october",
            "nov",
            "november",
            "dec",
            "december",
            "monday",
            "tuesday",
            "wednesday",
            "thursday",
            "friday",
            "saturday",
            "sunday",
        ]
        .contains(&token)
        || tail.starts_with("further notice")
}
fn effective_expression(text: &str) -> bool {
    let compact: String = text.chars().filter(|c| !c.is_whitespace()).collect();
    if [
        "유효기간",
        "유효기한",
        "적용시점",
        "적용날짜",
        "적용일",
        "만료일",
        "만료시점",
        "효력발생일",
    ]
    .iter()
    .any(|v| compact.contains(v))
    {
        return true;
    }
    if [
        "부터", "까지", "이후", "이전", "기준", "유효", "적용", "만료", "효력",
    ]
    .iter()
    .any(|word| {
        compact.match_indices(word).any(|(i, word)| {
            // A short local window limits unrelated dates elsewhere in a fact.
            let before: String = compact[..i]
                .chars()
                .rev()
                .take(24)
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .collect();
            let after: String = compact[i + word.len()..].chars().take(24).collect();
            korean_time_context(&before)
                || (["유효", "적용", "만료", "효력"].contains(&word) && korean_time_context(&after))
        })
    }) {
        return true;
    }
    let lower = text.to_lowercase();
    let spaced = lower.split_whitespace().collect::<Vec<_>>().join(" ");
    if [
        "expiry date",
        "expiration date",
        "validity period",
        "effective date",
    ]
    .iter()
    .any(|v| spaced.contains(v))
    {
        return true;
    }
    [
        "from ",
        "between ",
        "after ",
        "before ",
        "through ",
        "until ",
        "starting ",
        "starting on ",
        "starts on ",
        "effective ",
        "effective on ",
        "effective from ",
        "valid from ",
        "valid until ",
        "valid through ",
        "expires ",
        "expires on ",
        "as of ",
    ]
    .iter()
    .any(|prefix| {
        spaced.match_indices(prefix).any(|(i, prefix)| {
            (i == 0 || !spaced[..i].ends_with(char::is_alphanumeric))
                && english_time_context(&spaced[i + prefix.len()..])
        })
    })
}
/// Only a leading comma/newline-separated directive is accepted:
/// DATE부터 [DATE까지], FACT / DATE까지, FACT. ISO and Korean dates may mix.
/// Scan the complete remainder for recognized conflicting/unsupported scope forms.
pub fn memory_temporal(text: &str) -> MemoryTemporal {
    let Some((header, rest)) = text.trim_start().split_once([',', '\n']) else {
        return if effective_expression(text) {
            MemoryTemporal::Clarify
        } else {
            MemoryTemporal::Undated
        };
    };
    let header = header.trim();
    let parsed = if let Some((from, rest)) = header.split_once("부터") {
        let rest = rest.trim();
        literal_date(from.trim()).and_then(|from| {
            if rest.is_empty() {
                Some((Some(from), None))
            } else {
                literal_date(rest.strip_suffix("까지")?.trim()).map(|end| (Some(from), Some(end)))
            }
        })
    } else {
        header
            .strip_suffix("까지")
            .map(str::trim)
            .and_then(literal_date)
            .map(|end| (None, Some(end)))
    };
    match parsed {
        Some((from, through))
            if !rest.trim().is_empty()
                && !effective_expression(rest)
                && !matches!((from, through), (Some(a), Some(b)) if a > b) =>
        {
            MemoryTemporal::Dated { from, through }
        }
        _ if effective_expression(text) => MemoryTemporal::Clarify,
        _ => MemoryTemporal::Undated,
    }
}

#[cfg(test)]
mod temporal_tests {
    use super::*;
    #[test]
    fn closed_dates_and_full_message_gate() {
        for text in [
            "2027-01-01부터 2027-12-31까지, 나는 차를 좋아해. 기억해",
            "2027년 1월 1일부터, 기억해",
            "2027-01-01까지, 기억해",
        ] {
            assert!(matches!(
                memory_temporal(text),
                MemoryTemporal::Dated { .. }
            ));
        }
        for text in [
            "2027-02-29부터, 기억해",
            "2027-02-01부터 2027-01-01까지, 기억해",
            "기억해. 2027-01-01부터",
            "2027-01-01부터, 내일부터 적용해서 기억해",
            "from 2027-01-01 remember tea",
            "내일부터 기억해",
            "2027/01/01부터, 기억해",
            "2027-01-01T12:00:00Z부터, 기억해",
            "20 27-01-01부터, 기억해",
            "2027 -01-01부터, 기억해",
            "2027년 1 1월 1일부터, 기억해",
            "from January 1 remember tea",
            "after 2027-01-01 remember tea",
        ] {
            assert_eq!(memory_temporal(text), MemoryTemporal::Clarify);
        }
        for text in [
            "내 생일은 2000-01-01이야. 기억해",
            "The event was on 2020-01-01. Remember this.",
            "Remember I buy tea from markets.",
            "나는 원칙과 기준을 중요하게 생각해. 기억해 줘.",
            "처음부터 끝까지 꼼꼼해. 기억해 줘.",
            "나는 원칙을 일에 적용해. 기억해 줘.",
            "Remember I commute through Seoul.",
            "Remember I enjoy starting conversations.",
        ] {
            assert_eq!(memory_temporal(text), MemoryTemporal::Undated);
            let dated = format!("2098-01-01부터, {text}");
            assert!(matches!(
                memory_temporal(&dated),
                MemoryTemporal::Dated { .. }
            ));
        }
    }
}
