//! Literal whole replies and already established field gaps; never new authority.
use moosuri_core::{CalendarChatDecision, CurrentChatDecision};

pub(crate) fn cancelled(s: &str) -> bool {
    let s = s.trim().trim_end_matches(['.', '!', '。']).trim();
    let compact: String = s.chars().filter(|c| !c.is_whitespace()).collect();
    let remainder = ["됐어", "아니야", "아니요", "괜찮아"]
        .iter()
        .find_map(|prefix| compact.strip_prefix(prefix))
        .map(|rest| rest.strip_prefix(',').unwrap_or(rest))
        .unwrap_or(&compact);
    if matches!(
        remainder,
        "" | "취소" | "취소해줘" | "취소해주세요" | "그만"
    ) && !s.is_empty()
    {
        return true;
    }
    matches!(
        s.to_ascii_lowercase().as_str(),
        "cancel" | "never mind" | "nevermind" | "stop"
    )
}

pub(super) fn field(value: &str) -> bool {
    matches!(
        value,
        "time" | "target" | "task" | "date" | "duration" | "account"
    )
}
fn calendar_missing(d: &CalendarChatDecision, field: &str, missing_timezone: bool) -> bool {
    match field {
        "time" => d.start.is_none() || missing_timezone,
        "date" => d.date.is_none(),
        "duration" if d.all_day_quote.is_some() => d.last_date.is_none(),
        "duration" => d.end.is_none() && d.duration_minutes.is_none(),
        "target" => d.target_quote.is_none(),
        "account" => d.connection_quote.is_none(),
        _ => false,
    }
}
pub(super) fn still_missing(d: &CurrentChatDecision, field: &str, missing_timezone: bool) -> bool {
    match d {
        CurrentChatDecision::Calendar(d) => calendar_missing(d, field, missing_timezone),
        CurrentChatDecision::Mission(d) => match field {
            "time" => d.time.is_none() || missing_timezone,
            "task" => d.task_quote.is_none(),
            "target" => d.target_quote.is_none(),
            _ => false,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_whole_generic_question_cancellations_match() {
        for text in [
            "취소",
            "됐어, 취소해 줘",
            "그만!",
            "Never mind.",
            "cancel",
            "stop",
        ] {
            assert!(cancelled(text), "{text}");
        }
        for text in [
            "",
            "'취소'",
            "만약 취소하면",
            "취소하고 새 일정 만들어",
            "회의 취소",
            "cancel reminder",
            "cancel Synthetic meeting",
            "cancel if tomorrow",
        ] {
            assert!(!cancelled(text), "{text}");
        }
    }
}
