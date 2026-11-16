//! Only the immediately preceding explicit creation draft can consume a literal answer.
use super::*;
use parser::{Action, Decision, Parsed};
mod replies;
use replies::{body_answer, cancelled};
const EXHAUSTED: &str = "답변 횟수가 끝나 초안을 닫았습니다. 새 요청에 시간과 알림 내용을 함께 적어 주세요. 저장한 알림은 없습니다.";
const WHEN: &str = "언제 알려드릴까요? 다음 답변에 ‘내일 오전 9시’ 또는 ‘in 10 minutes’처럼 시간을 적어 주세요. 취소하려면 ‘취소’라고 하세요.";
const BODY: &str = "어떤 문구로 알려드릴까요? ‘물 마시기’, ‘stretch’처럼 짧은 문구를 적거나 ‘내용: ...’로 정확한 알림 내용을 알려주세요. 취소하려면 ‘취소’라고 하세요.";
const AMBIGUOUS: &str = "날짜와 오전·오후를 명확히 알려주세요. 이미 적은 날짜·시각은 유지합니다. 예: ‘오전’, ‘내일’, 또는 빠진 부분만 답해 주세요. 취소하려면 ‘취소’라고 하세요.";
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Draft {
    message: Option<String>,
    rule: Option<ScheduleRule>,
    spec: Option<String>,
    timezone: Option<String>,
    reference: i64,
    expires: i64,
    answers: u8,
}
fn command(d: Draft) -> Decision {
    Decision::Command(Parsed {
        action: Action::Create,
        rule: d.rule,
        shift_seconds: None,
        message: d.message,
        target: None,
    })
}
fn save(c: &Connection, r: &Run, d: &Draft) -> Result<()> {
    let json = encode(d)?;
    if json.len() > 8192 {
        return Err(StoreError::InvalidInput);
    }
    c.execute(
        "INSERT INTO schedule_chat_drafts VALUES(?1,?2,?3,1)",
        params![r.tenant, r.id, json],
    )?;
    Ok(())
}
fn prompt(d: &Draft) -> Decision {
    Decision::Clarify(if d.rule.is_some() {
        BODY
    } else if d.spec.is_some() {
        AMBIGUOUS
    } else {
        WHEN
    })
}
fn time_candidate(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    s.len() <= 160
        && !s.contains(['\n', '\r', '?', '"', '\'', '`', ';'])
        && (crate::schedules::civil_time::day_cue(&s)
            || parser::future_prefix(&s)
            || matches!(s.as_str(), "tomorrow" | "today" | "내일" | "오늘" | "모레")
            || s.starts_with("in ")
            || s.starts_with("tomorrow ")
            || s.starts_with("today ")
            || s.starts_with("daily ")
            || s.starts_with("at ")
            || s.as_bytes().first().is_some_and(u8::is_ascii_digit))
}
fn has_day(s: &str) -> bool {
    let s = s.to_ascii_lowercase();
    crate::schedules::civil_time::day_cue(&s)
        || [
            "tomorrow ",
            "today ",
            "내일",
            "오늘",
            "모레",
            "매일",
            "매주",
            "every ",
            "daily ",
            "in ",
        ]
        .iter()
        .any(|p| s.starts_with(p))
        || s.get(..10)
            .is_some_and(|p| chrono::NaiveDate::parse_from_str(p, "%Y-%m-%d").is_ok())
        || ["뒤", "뒤에", "후", "후에", "마다"]
            .iter()
            .any(|suffix| s.ends_with(suffix))
}
fn exact_rule(s: &str, zone: Option<&str>, reference: i64) -> Option<ScheduleRule> {
    if !time_candidate(s) || !has_day(s) {
        return None;
    }
    let (rule, tail) = parser::rule(s, zone, reference).ok()?;
    tail.trim().is_empty().then_some(rule)
}
fn initial(text: &str, timezone: Option<&str>, now: i64) -> Option<Draft> {
    let raw = text.trim();
    let lower = raw.to_ascii_lowercase();
    let mut d = Draft {
        message: None,
        rule: None,
        spec: None,
        timezone: timezone.map(str::to_owned),
        reference: now,
        expires: now + 900,
        answers: 0,
    };
    if let Some(prefix) = ["remind me ", "set a reminder "]
        .iter()
        .find(|p| lower.starts_with(**p))
    {
        let rest = &raw[prefix.len()..];
        let (spec, body) = if rest.to_ascii_lowercase().starts_with("to ") {
            ("", Some(&rest[3..]))
        } else if let Some(at) = rest.to_ascii_lowercase().find(" to ") {
            (&rest[..at], Some(&rest[at + 4..]))
        } else {
            (rest, None)
        };
        d.message = body.and_then(parser::message);
        let spec = spec.trim_end_matches(['.', '!']);
        if !spec.is_empty() {
            if !time_candidate(spec) {
                return None;
            }
            d.rule = exact_rule(spec, timezone, now);
            if d.rule.is_none() {
                d.spec = Some(spec.trim().into())
            }
        }
        if d.message.is_none() && body.is_some_and(|b| !b.trim().is_empty()) {
            return None;
        }
    } else {
        let raw = raw.trim_end_matches(['.', '!']);
        // This missing-time Korean shell has no leading time marker. Do not
        // reinterpret outer quotation/translation/source prose as creation.
        if raw.contains(['\n', '\r', '\"', '\'', '`', '“', '”', '‘', '’', '?', ':'])
            || [
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
                "문서",
                "메일 내용",
                "메일에",
                "본문",
            ]
            .iter()
            .any(|p| lower.starts_with(p))
        {
            return None;
        }
        let (rest, explicit) = [
            "알림 설정해줘",
            "알림해 줘",
            "알림해줘",
            "알려 줘",
            "알려줘",
        ]
        .iter()
        .find_map(|suffix| {
            raw.strip_suffix(suffix)
                .map(|s| (s.trim(), suffix.starts_with("알림")))
        })?;
        if let Ok((_, body)) = parser::rule(rest, timezone, now) {
            let spec = &rest[..rest.len() - body.len()];
            if !body.trim().is_empty() {
                if has_day(spec.trim()) || !explicit && !body.trim().ends_with("라고") {
                    return None;
                }
                d.message = parser::message(body);
            }
            d.rule = exact_rule(spec.trim(), timezone, now);
            if d.rule.is_none() {
                d.spec = Some(spec.trim().into())
            }
        } else if let Some((spec, body)) = rest.split_once("에 ").filter(|(s, _)| time_candidate(s))
        {
            if !explicit && !body.ends_with("라고") {
                return None;
            }
            d.message = parser::message(body);
            d.spec = Some(spec.into());
        } else if !time_candidate(rest) && (explicit || rest.ends_with("라고")) {
            d.message = parser::message(rest);
        } else {
            return None;
        }
    }
    if d.message.is_some() && d.rule.is_some()
        || d.message.is_none() && d.rule.is_none() && d.spec.is_none()
    {
        return None;
    }
    Some(d)
}
fn answer_time(d: &Draft, text: &str, now: i64) -> Option<ScheduleRule> {
    let text = text.trim().trim_end_matches(['.', '!']);
    if let Some(spec) = &d.spec {
        let lower = text.to_ascii_lowercase();
        let joined = if matches!(lower.as_str(), "am" | "pm" | "오전" | "오후") {
            if spec.contains("오전")
                || spec.contains("오후")
                || spec.to_ascii_lowercase().contains("am")
                || spec.to_ascii_lowercase().contains("pm")
            {
                return None;
            }
            if text.is_ascii() {
                format!("{spec}{lower}")
            } else {
                let consumed = d
                    .timezone
                    .as_deref()
                    .and_then(|z| {
                        crate::schedules::civil_time::date_prefix(spec, d.reference, z)
                            .ok()
                            .flatten()
                            .map(|(_, n)| n)
                    })
                    .unwrap_or(0);
                let remainder = spec[consumed..].trim_start();
                let split = spec.len() - remainder.len();
                format!("{}{} {}", &spec[..split], text, &spec[split..])
            }
        } else if crate::schedules::civil_time::day_cue(&lower) && !has_day(spec) {
            format!("{text} {spec}")
        } else {
            return None;
        };
        exact_rule(&joined, d.timezone.as_deref(), d.reference)
    } else {
        exact_rule(text, d.timezone.as_deref(), now)
    }
}
pub(super) fn decide(
    c: &Connection,
    r: &Run,
    context: Option<&ScheduleChatContext>,
    now: i64,
    independent_target: bool,
) -> Result<(Decision, Option<String>)> {
    let zone = context.map(|c| c.timezone.clone());
    let selected = context.and_then(|c| c.target.as_ref()).is_some();
    let direct = parser::parse(&r.text, zone.as_deref(), now, selected);
    let prior:Option<(String,String)>=c.query_row("SELECT p.id,d.draft FROM chat_runs p JOIN schedule_chat_drafts d ON d.tenant_id=p.tenant_id AND d.run_id=p.id WHERE p.tenant_id=?1 AND p.thread_id=?2 AND p.sequence=?3 AND p.account_id=?4 AND p.session_digest=?5 AND p.auth_epoch=?6 AND p.state='completed' AND d.pending=1",params![r.tenant,r.thread,r.sequence-1,r.account,r.session.as_slice(),r.epoch],|row|Ok((row.get(0)?,row.get(1)?))).optional()?;
    if let Some((id, _)) = &prior {
        c.execute(
            "UPDATE schedule_chat_drafts SET pending=0 WHERE tenant_id=?1 AND run_id=?2",
            params![r.tenant, id],
        )?;
    }
    if independent_target || crate::media::has_context(c, &r.tenant, &r.id)? {
        return Ok((direct, zone));
    }
    let eligible = prior
        .as_ref()
        .map(|(_, raw)| decode::<Draft>(raw))
        .transpose()?
        .filter(|d| now < d.expires && d.answers < 2 && !selected);
    if eligible.is_some() && cancelled(r.text.trim().trim_end_matches(['.', '!']).trim()) {
        return Ok((
            Decision::Clarify("이 알림 초안을 취소했습니다. 새 알림은 저장하지 않았습니다."),
            zone,
        ));
    }
    // An independent explicit command always gets its own authority/fields.
    if !selected && let Some(d) = initial(&r.text, zone.as_deref(), now) {
        save(c, r, &d)?;
        return Ok((prompt(&d), d.timezone));
    }
    if !matches!(direct, Decision::None) {
        return Ok((direct, zone));
    }
    let Some(mut d) = eligible else {
        return Ok((direct, zone));
    };
    if d.rule.is_none() {
        if let Some(rule) = answer_time(&d, &r.text, now) {
            d.rule = Some(rule);
            d.spec = None;
        } else if d.spec.is_none() && time_candidate(r.text.trim()) {
            d.spec = Some(r.text.trim().into());
            d.reference = now;
            d.answers += 1;
            if d.answers >= 2 {
                return Ok((Decision::Clarify(EXHAUSTED), d.timezone));
            }
            save(c, r, &d)?;
            return Ok((prompt(&d), d.timezone));
        } else {
            return Ok((direct, zone));
        }
    } else {
        let Some(body) = body_answer(&r.text) else {
            return Ok((direct, zone));
        };
        d.message = Some(body);
    }
    d.answers += 1;
    if d.message.is_some() && d.rule.is_some() {
        if matches!(d.rule,Some(ScheduleRule::Once{at})if at<=now) {
            return Ok((
                Decision::Clarify(
                    "앞서 지정한 시각이 지났습니다. 새 알림을 시간과 내용으로 다시 요청해 주세요.",
                ),
                d.timezone,
            ));
        }
        let zone = d.timezone.clone();
        return Ok((command(d), zone));
    }
    if d.answers >= 2 {
        return Ok((Decision::Clarify(EXHAUSTED), d.timezone));
    }
    save(c, r, &d)?;
    Ok((prompt(&d), d.timezone))
}

#[cfg(test)]
mod tests;
