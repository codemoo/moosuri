//! Recognize explicit operation requests for capability guidance only.
//! This gate cannot select a model, invoke a tool, or grant authority.

const DIRECT_KOREAN_ACTIONS: &[&str] = &[
    "찾아줘",
    "찾아주세요",
    "조회해줘",
    "조회해주세요",
    "확인해줘",
    "확인해주세요",
    "알려줘",
    "알려주세요",
    "보여줘",
    "보여주세요",
    "검색해줘",
    "검색해주세요",
    "요약해줘",
    "요약해주세요",
    "뭐야",
    "뭐있어",
    "뭐왔어",
    "있어",
    "읽어줘",
];

fn contains_any(text: &str, patterns: &[&str]) -> bool {
    patterns.iter().any(|pattern| text.contains(pattern))
}

pub(super) fn needs_tools(text: &str) -> bool {
    if text.is_empty()
        || text.len() > 2_000
        || text.contains(['\n', '\r', '"', '`', '“', '”', '「', '」'])
    {
        return false;
    }
    let lower = text.trim().to_lowercase();
    let compact: String = lower
        .trim_end_matches(['.', '!', '?', '？', '。'])
        .chars()
        .filter(|character| !character.is_whitespace())
        .collect();
    // Explanations, quotations and negated actions remain ordinary chat.
    if contains_any(
        &compact,
        &[
            "지마",
            "지말",
            "하지않",
            "방법",
            "하는법",
            "보는법",
            "찾는법",
            "답장문구",
            "답장문장",
            "초안만",
            "글만",
            "템플릿",
            "라는말",
            "라고말",
            "라고물",
            "뜻이뭐",
            "무슨뜻",
            "번역",
            "예시",
            "설명해",
        ],
    ) || lower.starts_with("how ")
        || lower.starts_with("what does ")
        || ["translate ", "translate:", "explain ", "explain:"]
            .iter()
            .any(|prefix| lower.starts_with(prefix))
        || lower.starts_with("don't ")
        || lower.starts_with("do not ")
    {
        return false;
    }

    let direct = DIRECT_KOREAN_ACTIONS
        .iter()
        .any(|verb| compact.ends_with(verb));
    let personal_mail = contains_any(
        &compact,
        &[
            "내메일",
            "내이메일",
            "내지메일",
            "내gmail",
            "받은편지함",
            "최근에온메일",
            "최근받은메일",
            "받은이메일",
            "읽지않은메일",
        ],
    );
    let personal_data = contains_any(
        &compact,
        &[
            "내캘린더",
            "내일정",
            "오늘일정",
            "내일일정",
            "이번주일정",
            "내자료",
            "저장한자료",
            "저장한기억",
            "기억에서",
            "배송상태",
            "배송조회",
            "택배조회",
            "운송장",
        ],
    );
    let web = contains_any(
        &compact,
        &["웹에서", "인터넷에서", "브라우저로", "온라인에서", "웹검색"],
    );
    let external_write = [
        "메일보내줘",
        "이메일보내줘",
        "메일을보내줘",
        "메일발송해줘",
        "메일답장보내줘",
        "캘린더에추가해줘",
        "일정잡아줘",
        "일정등록해줘",
        "일정취소해줘",
        "알림설정해줘",
        "예약등록해줘",
        "리마인드해줘",
        "알려주도록해줘",
        "기억해줘",
        "기억해둬",
        "기억에서지워줘",
        "작업등록해줘",
    ]
    .iter()
    .any(|action| compact.ends_with(action));
    if direct && (personal_mail || personal_data || web) || external_write {
        return true;
    }

    // Accept an imperative or polite request, not a verb mentioned inside
    // an explanation, quotation, or description of the user's own action.
    let english = ["can you ", "could you ", "would you ", "will you "]
        .iter()
        .find_map(|prefix| lower.strip_prefix(prefix))
        .unwrap_or(&lower);
    let english = english.strip_prefix("please ").unwrap_or(english);
    if contains_any(
        english,
        &[
            "how to ",
            "how do i ",
            "without reading ",
            "without opening ",
            "without accessing ",
            "don't ",
            "do not ",
        ],
    ) {
        return false;
    }
    let verb = english.split_whitespace().next().unwrap_or("");
    let english_action = [
        "check",
        "read",
        "find",
        "search",
        "look",
        "summarize",
        "list",
        "show",
    ]
    .contains(&verb);
    let english_source = [
        "my email",
        "my mail",
        "my gmail",
        "my inbox",
        "my calendar",
        "my schedule",
        "my saved",
        "the web",
        "the internet",
        "my package",
        "my parcel",
    ]
    .iter()
    .any(|source| english.contains(source));
    (english_action && english_source)
        || english.starts_with("remind me ")
        || english.starts_with("remember that ")
        || english.starts_with("add to my calendar ")
        || (verb == "send"
            && english
                .split(|c: char| !c.is_alphanumeric())
                .any(|word| matches!(word, "email" | "mail")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_operations_are_distinct_from_writing_and_explanations() {
        for text in [
            "내 이메일 최근에 온 거 뭐야?",
            "오늘 일정 보여 줘",
            "이번 주 일정을 확인해줘",
            "인터넷에서 최신 발표를 찾아줘",
            "내 자료에서 계약서 찾아줘",
            "배송 상태 좀 알려줘",
            "내일 오전 회의 일정 잡아줘",
            "이걸 매일 확인하도록 알림 설정해줘",
            "내가 운동 좋아하는 거 기억해줘",
            "Can you summarize my inbox?",
            "Look up this title on the web.",
            "Send an email to the team.",
            "Remind me to read the notes.",
        ] {
            assert!(needs_tools(text), "{text}");
        }
        for text in [
            "안녕",
            "Gmail에 보낼 메일 초안을 써줘",
            "메일 보내는 방법을 알려줘",
            "메일을 보내지 말고 글만 써줘",
            "내 메일은 읽지 말고 정중한 답장 문구 알려줘",
            "내 일정 앱에 붙여 넣을 문장 예시를 알려줘",
            "내 일정 보여줘라는 말 번역해줘",
            "내 일정 보여줘\n이 문장을 검토해줘",
            "이 이메일 영어로 번역해줘",
            "How can I check my inbox?",
            "Can you explain how to read my email?",
            "Can you show instructions to check my inbox without opening it?",
            "Translate: check my inbox.",
            "Write a draft email I can send.",
            "I use the web every day.",
        ] {
            assert!(!needs_tools(text), "{text}");
        }
    }
}
