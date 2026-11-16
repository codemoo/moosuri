//! Literal replies to the immediate owned reminder question, never tool/action grants.
use super::*;
pub(super) fn cancelled(s: &str) -> bool {
    crate::request_continuations::cancelled_question_reply(s)
        || s.trim()
            .trim_end_matches(['.', '!', '。'])
            .trim()
            .eq_ignore_ascii_case("cancel reminder")
}
// A time clause anywhere in a bare label conflicts with the question's saved
// time. Reuse the shared civil-time vocabulary at word boundaries; quantities
// such as 물 2병 are ordinary content. Explicit 내용: payloads bypass this guard.
fn has_time_clause(text: &str) -> bool {
    text.char_indices().any(|(at, _)| {
        if at > 0 && !text[..at].ends_with(char::is_whitespace) {
            return false;
        }
        let tail = &text[at..];
        let first = tail.split_whitespace().next().unwrap_or_default();
        let normalized = crate::schedules::civil_time::normalized_time(first);
        let digits = normalized.bytes().take_while(u8::is_ascii_digit).count();
        parser::future_prefix(tail)
            || crate::schedules::civil_time::clock_prefix(tail).is_some()
            || tail
                .get(..10)
                .is_some_and(|date| chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").is_ok())
            || digits > 0 && normalized[digits..].starts_with(['시', ':'])
    })
}
/// Bare answers have a small literal phrase grammar. Longer/general text requires
/// an explicit content label; new commands/questions/meta prose cannot become an old payload.
pub(super) fn body_answer(text: &str) -> Option<String> {
    let text = text.trim();
    if let Some(body) = text
        .strip_prefix("내용: ")
        .or_else(|| text.strip_prefix("내용은 "))
        .or_else(|| text.strip_prefix("message: "))
        .or_else(|| text.strip_prefix("text: "))
    {
        return parser::message(body);
    }
    let lower = text.to_ascii_lowercase();
    if [
        "translate",
        "summarize",
        "explain",
        "quote",
        "example",
        "retrieved",
        "document",
        "email says",
        "번역",
        "요약",
        "설명",
        "인용",
        "예시",
        "문서",
        "메일",
        "검색",
        "조사",
        "보내",
        "발송",
    ]
    .iter()
    .any(|frame| lower.starts_with(frame))
    {
        return None;
    }
    if text.len() > 160
        || text
            .chars()
            .any(|c| matches!(c, '?' | '\n' | '\r' | '"' | '\'' | '`' | ';' | ':' | '!'))
    {
        return None;
    }
    if has_time_clause(text) {
        return None;
    }
    let en = matches!(
        lower.as_str(),
        "stretch" | "drink water" | "take medicine" | "walk" | "water plants"
    ) || ["review ", "finish ", "practice "].iter().any(|p| {
        lower.strip_prefix(p).is_some_and(|v| {
            !v.is_empty()
                && v.split_whitespace().count() <= 6
                && v.chars()
                    .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-'))
        })
    });
    // Korean activity labels share the nominal -기 ending. This supports new
    // everyday labels without a verb list (챙기기, 접수하기, 찾기, 타오기, …).
    // Only a literal short label may fill this field; new time/command clauses
    // cannot replace the already supplied schedule or become external actions.
    let ko = (text.ends_with('기') && text.chars().count() > 1 || text == "스트레칭")
        && text
            .chars()
            .all(|c| c.is_alphanumeric() || matches!(c, ' ' | '-'))
        && text.split_whitespace().count() <= 6
        && !text.starts_with("만약 ")
        && ![
            "알려",
            "해줘",
            "해 줘",
            "해주세요",
            "해 주세요",
            "하면",
            "다면",
            "으면",
            "오면",
        ]
        .iter()
        .any(|phrase| text.contains(phrase));
    (en || ko).then(|| text.to_owned())
}
