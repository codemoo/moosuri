use super::*;
use chrono::{Days, LocalResult, NaiveDate, NaiveTime, TimeZone};
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Action {
    Create,
    Edit,
    Pause,
    Resume,
    Revoke,
}
pub(super) enum Target {
    Generic,
    Title(String),
}
pub(super) struct Parsed {
    pub action: Action,
    pub rule: Option<ScheduleRule>,
    pub message: Option<String>,
    pub target: Option<Target>,
}
pub(super) enum Decision {
    None,
    Clarify(&'static str),
    Mission,
    Command(Parsed),
}
const TIME: &str = "언제 알려드릴까요? 예: ‘내일 오전 9시에 물 마시라고 알려줘’, ‘remind me in 10 minutes to stretch’. 현지 시각에는 브라우저 시간대가 필요합니다.";
const CONTROL: &str = "지금 적용할 한 가지 변경과 알림을 명확히 적어 주세요. 예: ‘pause reminder’, ‘delete water reminder’. 조건·질문·선택지가 있는 문장은 알림을 변경하지 않습니다.";

pub(super) fn parse(text: &str, timezone: Option<&str>, now: i64, selected: bool) -> Decision {
    let raw = text.trim();
    let lower = raw.to_ascii_lowercase();
    // The explicit reminder shell grants creation. Its body is literal saved text,
    // including meta words, negation, quotes and punctuation, never a model job.
    let english = if lower.starts_with("remind me ") {
        Some(10)
    } else if lower.starts_with("set a reminder ") {
        Some(15)
    } else {
        None
    };
    if let Some(prefix) = english {
        let rest = &raw[prefix..];
        let Some(cut) = rest.to_ascii_lowercase().find(" to ") else {
            return Decision::Clarify(TIME);
        };
        let (spec, body) = (&rest[..cut], &rest[cut + 4..]);
        return match (rule(spec, timezone, now), message(body)) {
            (Ok((r, tail)), Some(body)) if tail.trim().is_empty() => Decision::Command(Parsed {
                action: Action::Create,
                rule: Some(r),
                message: Some(body),
                target: None,
            }),
            _ => Decision::Clarify(TIME),
        };
    }
    // Only an outer meta frame bypasses capture; no whole-message keyword blacklist.
    if [
        "translate",
        "summarize",
        "example",
        "quote",
        "retrieved",
        "email says",
        "document says",
        "번역",
        "요약",
        "예시",
        "인용",
        "메일 내용",
        "메일에",
        "본문",
    ]
    .iter()
    .any(|s| lower.starts_with(s))
    {
        return Decision::None;
    }
    let command = raw.trim_end_matches(['.', '!']);
    let korean = [
        "알림 설정해줘",
        "알림해 줘",
        "알림해줘",
        "알려 줘",
        "알려줘",
    ]
    .iter()
    .find_map(|suffix| {
        command
            .strip_suffix(suffix)
            .map(|v| (v.trim(), suffix.starts_with("알림")))
    });
    if let Some((rest, explicit_reminder)) = korean
        && korean_time(rest)
    {
        return match rule(rest, timezone, now) {
            Ok((r, body)) => {
                let body = body.trim();
                if !explicit_reminder && !body.ends_with("라고") && mission_body(body) {
                    return Decision::Mission;
                }
                match message(body) {
                    Some(body) => Decision::Command(Parsed {
                        action: Action::Create,
                        rule: Some(r),
                        message: Some(body),
                        target: None,
                    }),
                    None => Decision::Clarify(
                        "어떤 문구로 알려드릴까요? 시간과 문구를 함께 적어 주세요.",
                    ),
                }
            }
            _ => Decision::Clarify(TIME),
        };
    }
    if let Some(d) = english_control(raw, &raw.to_ascii_lowercase(), timezone, now, selected) {
        return d;
    }
    if let Some(d) = korean_control(command, timezone, now, selected) {
        return d;
    }

    // Known task requests without a complete reminder shell are unsupported for
    // both one-off and recurring Korean date/elapsed prefixes.
    if future_prefix(command) && mission_body(command) {
        return Decision::Mission;
    }
    Decision::None
}

fn parsed_control(
    action: Action,
    target: Target,
    rule: Option<ScheduleRule>,
    message: Option<String>,
) -> Decision {
    Decision::Command(Parsed {
        action,
        target: Some(target),
        rule,
        message,
    })
}
fn shell(value: &str) -> bool {
    let lower = format!(" {} ", value.to_ascii_lowercase());
    !value.chars().any(|c| {
        matches!(
            c,
            '?' | '\n' | '\r' | ';' | '`' | '"' | '\'' | '“' | '”' | '‘' | '’'
        )
    }) && ![
        " if ",
        " when ",
        " unless ",
        " or ",
        " and ",
        " later ",
        " would ",
        " should ",
        " might ",
        " do not ",
        " don't ",
        " never ",
        "만약",
        "나중에",
        "하면",
        "라면",
        "다면",
        "또는",
        "아니면",
        "어떻게",
        "하지 마",
        "하지마",
        "하지 말",
        "하지말",
        "하고 ",
        "한 뒤 ",
    ]
    .iter()
    .any(|s| lower.contains(s))
}
fn title(value: &str) -> Option<Target> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 160 && shell(value)).then(|| Target::Title(value.into()))
}
fn en_target(value: &str) -> Option<Target> {
    let value = value.trim();
    let lower = value.to_ascii_lowercase();
    if matches!(lower.as_str(), "reminder" | "the reminder") {
        Some(Target::Generic)
    } else if let Some(name) = lower.strip_suffix(" reminder") {
        title(&value[..name.len()])
    } else {
        None
    }
}
fn ko_target(value: &str, selected: bool) -> Option<Target> {
    let value = value.trim();
    if matches!(value, "알림" | "알림을") || selected && value.is_empty() {
        return Some(Target::Generic);
    }
    value
        .strip_suffix(" 알림을")
        .or_else(|| value.strip_suffix(" 알림"))
        .and_then(title)
}
fn english_control(
    raw: &str,
    lower: &str,
    timezone: Option<&str>,
    now: i64,
    selected: bool,
) -> Option<Decision> {
    let (action, prefix) = [
        ("pause ", Action::Pause),
        ("resume ", Action::Resume),
        ("delete ", Action::Revoke),
        ("cancel ", Action::Revoke),
        ("change ", Action::Edit),
    ]
    .into_iter()
    .find_map(|(p, a)| lower.starts_with(p).then_some((a, p.len())))?;
    if !selected && !lower.contains("reminder") {
        return None;
    }
    let rest = &raw[prefix..];
    if action != Action::Edit {
        let command = raw.trim_end_matches(['.', '!']);
        let rest = &command[prefix..];
        return Some(if shell(command) {
            en_target(rest).map_or(Decision::Clarify(CONTROL), |t| {
                parsed_control(action, t, None, None)
            })
        } else {
            Decision::Clarify(CONTROL)
        });
    }
    // Body-bearing edits consume the target shell first, leaving payload literal.
    if let Some(cut) = rest.to_ascii_lowercase().find(" message to ") {
        let head = &rest[..cut];
        let body = &rest[cut + 12..];
        return Some(match (shell(head), en_target(head), message(body)) {
            (true, Some(t), Some(body)) => parsed_control(Action::Edit, t, None, Some(body)),
            _ => Decision::Clarify(CONTROL),
        });
    }
    Some(if let Some(cut) = rest.to_ascii_lowercase().find(" to ") {
        let head = &rest[..cut];
        let spec = rest[cut + 4..].trim_end_matches(['.', '!']);
        match (shell(raw), en_target(head), rule(spec, timezone, now)) {
            (true, Some(t), Ok((r, ""))) => parsed_control(Action::Edit, t, Some(r), None),
            _ => Decision::Clarify(CONTROL),
        }
    } else {
        Decision::Clarify(CONTROL)
    })
}
fn korean_control(raw: &str, timezone: Option<&str>, now: i64, selected: bool) -> Option<Decision> {
    if !selected && !raw.contains("알림") {
        return None;
    }
    // Everyday on/off controls still consume a complete imperative and exact target.
    for (ending, action) in [
        ("다시 켜 주세요", Action::Resume),
        ("다시 켜주세요", Action::Resume),
        ("다시 켜 줘", Action::Resume),
        ("다시 켜줘", Action::Resume),
        ("켜 주세요", Action::Resume),
        ("켜주세요", Action::Resume),
        ("켜 줘", Action::Resume),
        ("켜줘", Action::Resume),
        ("꺼 주세요", Action::Pause),
        ("꺼주세요", Action::Pause),
        ("꺼 줘", Action::Pause),
        ("꺼줘", Action::Pause),
    ] {
        if let Some(head) = raw.strip_suffix(ending) {
            return Some(if shell(raw) {
                ko_target(head, selected).map_or(Decision::Clarify(CONTROL), |target| {
                    parsed_control(action, target, None, None)
                })
            } else {
                Decision::Clarify(CONTROL)
            });
        }
    }
    // Exact terminal change markers separate the authority shell from message.
    for suffix in ["바꿔줘", "바꿔 줘", "변경해줘"] {
        if let Some(inner) = raw.strip_suffix(suffix) {
            if let Some((head, body)) = inner.split_once(" 내용을 ") {
                let body = body.trim();
                let body = body
                    .strip_suffix("으로")
                    .or_else(|| body.strip_suffix('로'));
                return Some(
                    match (
                        shell(head),
                        ko_target(head, selected),
                        body.and_then(message),
                    ) {
                        (true, Some(t), Some(body)) => {
                            parsed_control(Action::Edit, t, None, Some(body))
                        }
                        _ => Decision::Clarify(CONTROL),
                    },
                );
            }
            let cut = ["내일", "모레", "오늘", "매일", "매주", "오전", "오후"]
                .iter()
                .filter_map(|s| inner.find(s))
                .min();
            return Some(if let Some(cut) = cut {
                let head = inner[..cut].trim();
                let spec = inner[cut..].trim();
                let spec = spec
                    .strip_suffix("으로")
                    .or_else(|| spec.strip_suffix('로'))
                    .unwrap_or(spec)
                    .trim();
                match (
                    shell(raw),
                    ko_target(head, selected),
                    rule(spec, timezone, now),
                ) {
                    (true, Some(t), Ok((r, ""))) => parsed_control(Action::Edit, t, Some(r), None),
                    _ => Decision::Clarify(CONTROL),
                }
            } else {
                Decision::Clarify(CONTROL)
            });
        }
    }
    let mut tail = raw;
    for suffix in ["해 주세요", "해주세요", "해 줘", "해줘"] {
        if let Some(v) = tail.strip_suffix(suffix) {
            tail = v.trim();
            break;
        }
    }
    for (stem, action) in [
        ("일시정지", Action::Pause),
        ("일시 정지", Action::Pause),
        ("다시 시작", Action::Resume),
        ("재개", Action::Resume),
        ("삭제", Action::Revoke),
        ("취소", Action::Revoke),
    ] {
        if let Some(head) = tail.strip_suffix(stem) {
            if !selected && !head.contains("알림") {
                return None;
            }
            return Some(if shell(raw) {
                ko_target(head, selected).map_or(Decision::Clarify(CONTROL), |t| {
                    parsed_control(action, t, None, None)
                })
            } else {
                Decision::Clarify(CONTROL)
            });
        }
    }
    // Questions/conditionals do not acquire authority merely from a selection.
    let candidate = raw.starts_with("알림") || selected;
    if candidate
        && [
            "일시정지",
            "일시 정지",
            "다시 시작",
            "재개",
            "삭제",
            "취소",
            "바꿔",
            "변경",
            "켜",
            "꺼",
        ]
        .iter()
        .any(|s| raw.contains(s))
    {
        return Some(Decision::Clarify(CONTROL));
    }
    None
}
// Keep the documented absolute-date form on the same bounded admission path.
// Validate the exact date prefix before considering its existing local-time tail.
fn iso_date_tail(value: &str) -> Option<&str> {
    let prefix = value.get(..10)?;
    let date = NaiveDate::parse_from_str(prefix, "%Y-%m-%d").ok()?;
    if date.format("%Y-%m-%d").to_string() != prefix {
        return None;
    }
    Some(value.get(10..)?.trim_start())
}
fn future_prefix(value: &str) -> bool {
    iso_date_tail(value).is_some_and(korean_time)
        || [
            "내일", "모레", "오늘", "매일", "매주", "오전", "오후", "every ", "매 ",
        ]
        .iter()
        .any(|s| value.starts_with(s))
        || number(value).is_some_and(|(_, tail)| {
            ["분", "시간", "초"]
                .iter()
                .any(|s| tail.trim_start().starts_with(s))
        })
}
fn korean_time(mut value: &str) -> bool {
    if let Some(rest) = iso_date_tail(value) {
        value = rest;
    }
    for prefix in ["내일", "모레", "오늘", "매일"] {
        if let Some(v) = value.strip_prefix(prefix) {
            value = v.trim();
            break;
        }
    }
    if value.starts_with("매주") {
        return value.contains("오전")
            || value.contains("오후")
            || value.chars().any(|c| c.is_ascii_digit());
    }
    if let Some(rest) = value.strip_prefix("매 ") {
        value = rest.trim()
    }
    value.starts_with("오전")
        || value.starts_with("오후")
        || number(value).is_some_and(|(_, tail)| {
            ["시", ":", "분", "시간", "초"]
                .iter()
                .any(|s| tail.trim_start().starts_with(s))
        })
}
fn mission_body(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    [
        "조사해",
        "검색해",
        "보고해",
        "보내줘",
        "보내 줘",
        "발송",
        "확인해",
        "research",
        "send ",
        "check prices",
    ]
    .iter()
    .any(|s| lower.contains(s))
}

fn message(v: &str) -> Option<String> {
    let v = v.trim();
    if v.is_empty() || v.len() > 2000 || v.chars().any(|c| c.is_control() && c != '\n' && c != '\t')
    {
        None
    } else {
        Some(v.into())
    }
}
fn number(v: &str) -> Option<(u32, &str)> {
    let v = v.trim_start();
    let n = v.bytes().take_while(u8::is_ascii_digit).count();
    if n > 0 {
        return Some((v[..n].parse().ok()?, &v[n..]));
    }
    for (s, n) in [
        ("열두", 12),
        ("열한", 11),
        ("아홉", 9),
        ("여덟", 8),
        ("일곱", 7),
        ("여섯", 6),
        ("다섯", 5),
        ("열", 10),
        ("한", 1),
        ("두", 2),
        ("세", 3),
        ("네", 4),
    ] {
        if let Some(rest) = v.strip_prefix(s) {
            return Some((n, rest));
        }
    }
    None
}
fn clock(v: &str) -> Option<(String, &str)> {
    let mut v = v.trim_start();
    if let Some(rest) = v.strip_prefix("at ") {
        v = rest.trim_start()
    }
    let mut pm = None;
    for (s, p) in [("오전", false), ("오후", true)] {
        if let Some(rest) = v.strip_prefix(s) {
            pm = Some(p);
            v = rest.trim_start();
            break;
        }
    }
    let (h, mut tail) = number(v)?;
    let mut m = 0;
    let mut hour = h;
    if let Some(rest) = tail.strip_prefix(':') {
        let (n, rest) = number(rest)?;
        m = n;
        tail = rest;
    } else if let Some(rest) = tail.strip_prefix('시') {
        tail = rest.trim_start();
        if let Some((n, rest)) = number(tail)
            && let Some(rest) = rest.strip_prefix('분')
        {
            m = n;
            tail = rest;
        }
    } else if pm.is_some() {
        return None;
    }
    let t = tail.trim_start();
    if let Some(rest) = t.strip_prefix("am") {
        pm = Some(false);
        tail = rest
    } else if let Some(rest) = t.strip_prefix("pm") {
        pm = Some(true);
        tail = rest
    }
    if let Some(pm) = pm {
        if !(1..=12).contains(&h) {
            return None;
        }
        hour = h % 12 + if pm { 12 } else { 0 };
    } else if h <= 12 && !v.contains(':') {
        return None;
    }
    if hour > 23 || m > 59 {
        return None;
    }
    let tail = tail.trim_start();
    let tail = tail
        .strip_prefix("에 ")
        .or_else(|| tail.strip_prefix('에'))
        .unwrap_or(tail);
    Some((format!("{hour:02}:{m:02}"), tail.trim_start()))
}
fn weekdays(v: &str) -> Option<Vec<u8>> {
    let v = v.trim().trim_end_matches(',');
    let en = [
        ("monday", 1),
        ("tuesday", 2),
        ("wednesday", 3),
        ("thursday", 4),
        ("friday", 5),
        ("saturday", 6),
        ("sunday", 7),
    ];
    let mut days = vec![];
    let mut rem = v.to_lowercase().replace(" and ", ",").replace(' ', "");
    for (n, d) in en {
        if rem.contains(n) {
            days.push(d);
            rem = rem.replace(n, "");
        }
    }
    for (n, d) in [
        ("월", 1),
        ("화", 2),
        ("수", 3),
        ("목", 4),
        ("금", 5),
        ("토", 6),
        ("일", 7),
    ] {
        if rem.contains(n) {
            days.push(d);
            rem = rem.replace(&format!("{n}요일"), "").replace(n, "");
        }
    }
    if rem.chars().any(|c| c != ',' && c != '·' && c != '/') {
        return None;
    }
    days.sort_unstable();
    days.dedup();
    (!days.is_empty()).then_some(days)
}
pub(crate) fn rule<'a>(
    spec: &'a str,
    tz: Option<&str>,
    now: i64,
) -> Result<(ScheduleRule, &'a str)> {
    let original = spec.trim();
    let lower = original.to_ascii_lowercase();
    let (rule, tail) = rule_lower(&lower, tz, now)?;
    Ok((rule, &original[original.len() - tail.len()..]))
}
fn rule_lower<'a>(spec: &'a str, tz: Option<&str>, now: i64) -> Result<(ScheduleRule, &'a str)> {
    let spec = spec.trim();
    // UTC elapsed intervals do not require a timezone.
    let (interval, repeat) = if let Some(v) = spec.strip_prefix("in ") {
        (Some(v), false)
    } else if let Some(v) = spec.strip_prefix("every ") {
        (Some(v), true)
    } else if let Some(v) = spec.strip_prefix("매 ") {
        (Some(v), true)
    } else {
        (Some(spec), false)
    };
    if let Some(v) = interval
        && let Some((n, tail)) = number(v)
    {
        let tail = tail.trim_start();
        for (unit, mul) in [
            ("minutes", 60),
            ("minute", 60),
            ("hours", 3600),
            ("hour", 3600),
            ("days", 86400),
            ("day", 86400),
            ("seconds", 1),
            ("second", 1),
            ("분", 60),
            ("시간", 3600),
            ("초", 1),
        ] {
            if let Some(rest) = tail.strip_prefix(unit) {
                let rest = rest.trim_start();
                let korean = matches!(unit, "분" | "시간" | "초");
                let repeats = repeat || rest.starts_with("마다");
                let rest = rest
                    .strip_prefix("마다")
                    .or_else(|| rest.strip_prefix("뒤에"))
                    .or_else(|| rest.strip_prefix("후에"))
                    .or_else(|| rest.strip_prefix("뒤"))
                    .or_else(|| rest.strip_prefix("후"))
                    .unwrap_or(rest)
                    .trim();
                if korean && !repeats && !tail.contains('뒤') && !tail.contains('후') {
                    break;
                }
                let secs = n.checked_mul(mul).ok_or(StoreError::InvalidInput)?;
                if secs == 0 || secs > 31_622_400 || repeats && secs < 60 {
                    return Err(StoreError::InvalidInput);
                }
                let at = now
                    .checked_add(i64::from(secs))
                    .ok_or(StoreError::InvalidInput)?;
                return Ok((
                    if repeats {
                        ScheduleRule::Interval {
                            anchor: at,
                            seconds: secs,
                        }
                    } else {
                        ScheduleRule::Once { at }
                    },
                    rest,
                ));
            }
        }
    }
    let timezone = tz.ok_or(StoreError::InvalidInput)?;
    let zone = time::zone(timezone)?;
    let base = chrono::DateTime::from_timestamp(now, 0)
        .ok_or(StoreError::InvalidInput)?
        .with_timezone(&zone);
    let (mut v, daily, weekly) = if let Some(v) = spec.strip_prefix("every day ") {
        (v, true, None)
    } else if let Some(v) = spec.strip_prefix("daily ") {
        (v, true, None)
    } else if let Some(v) = spec.strip_prefix("매일") {
        (v.trim(), true, None)
    } else if let Some(v) = spec.strip_prefix("매주") {
        let cut = v
            .find("오전")
            .or_else(|| v.find("오후"))
            .or_else(|| v.find(|c: char| c.is_ascii_digit()))
            .ok_or(StoreError::InvalidInput)?;
        (
            &v[cut..],
            false,
            Some(weekdays(&v[..cut]).ok_or(StoreError::InvalidInput)?),
        )
    } else if let Some(v) = spec.strip_prefix("every ") {
        let (days, tail) = v.split_once(" at ").ok_or(StoreError::InvalidInput)?;
        (
            tail,
            false,
            Some(weekdays(days).ok_or(StoreError::InvalidInput)?),
        )
    } else {
        (spec, false, None)
    };
    let mut date = base.date_naive();
    let mut explicit = false;
    for (prefix, add) in [
        ("tomorrow ", 1),
        ("today ", 0),
        ("내일", 1),
        ("오늘", 0),
        ("모레", 2),
    ] {
        if let Some(tail) = v.strip_prefix(prefix) {
            date = date
                .checked_add_days(Days::new(add))
                .ok_or(StoreError::InvalidInput)?;
            v = tail.trim();
            explicit = true;
            break;
        }
    }
    if v.len() >= 11
        && let Some(prefix) = v.get(..10)
        && let Ok(d) = NaiveDate::parse_from_str(prefix, "%Y-%m-%d")
    {
        date = d;
        v = v[10..].trim_start();
        explicit = true;
    }
    let (hhmm, rest) = clock(v).ok_or(StoreError::InvalidInput)?;
    if daily {
        return Ok((
            ScheduleRule::Daily {
                time: hhmm,
                timezone: timezone.into(),
            },
            rest,
        ));
    }
    if let Some(days) = weekly {
        return Ok((
            ScheduleRule::Weekly {
                days,
                time: hhmm,
                timezone: timezone.into(),
            },
            rest,
        ));
    }
    let t = NaiveTime::parse_from_str(&hhmm, "%H:%M").map_err(|_| StoreError::InvalidInput)?;
    let local = |d: NaiveDate| match zone.from_local_datetime(&d.and_time(t)) {
        LocalResult::Single(v) => Ok(v.timestamp()),
        LocalResult::Ambiguous(a, b) => Ok(a.timestamp().min(b.timestamp())),
        LocalResult::None => Err(StoreError::InvalidInput),
    };
    let mut at = local(date)?;
    if at <= now && !explicit {
        date = date
            .checked_add_days(Days::new(1))
            .ok_or(StoreError::InvalidInput)?;
        at = local(date)?;
    }
    if at <= now {
        return Err(StoreError::InvalidInput);
    }
    time::instant(at)?;
    Ok((ScheduleRule::Once { at }, rest))
}
