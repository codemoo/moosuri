//! Keep the two sources a member explicitly selected even when the classifier
//! merges them or asks for details the searches themselves can resolve.
use super::{Act, Decision, DraftItem, Intent, ReportMode, guard, has, outside_quotes};
use moosuri_core::personal_browser::WorkScope;

fn search_term(clause: &str) -> String {
    // A short lookup term may be absent from the request ("챙길 것" → "준비물").
    // The store tests it against the selected member source before using a fallback.
    for (stems, noun) in [
        (&["챙기", "챙길", "챙겨", "준비물"][..], "준비물"),
        (&["출발", "몇 시에 떠"][..], "출발"),
        (&["시각", "일정", "시간표"][..], "일정"),
        (&["가격", "비용", "얼마"][..], "비용"),
    ] {
        if has(clause, stems) {
            return noun.to_owned();
        }
    }
    clause
        .split_whitespace()
        .map(|word| word.trim_matches(|c: char| !c.is_alphanumeric()))
        .map(|word| {
            [
                "에서", "으로", "이랑", "까지", "만", "을", "를", "은", "는", "도",
            ]
            .iter()
            .find_map(|suffix| word.strip_suffix(suffix))
            .unwrap_or(word)
        })
        .find(|word| {
            word.chars().count() >= 2
                && !has(
                    word,
                    &[
                        "확인",
                        "부탁",
                        "찾아",
                        "꺼내",
                        "대화",
                        "이야기",
                        "얘기",
                        "기억",
                        "메모",
                        "하고",
                        "지난",
                        "전에",
                        "적어",
                        "나눈",
                    ],
                )
        })
        .unwrap_or("")
        .to_owned()
}

pub(super) fn respect_sources(mut decision: Decision, text: &str, has_group: bool) -> Decision {
    let plain = outside_quotes(text);
    // An explicit search remains actionable when a separate sentence rules out an external write.
    // Derive only the particular private read the member named.
    if has(&plain, &["하지 마", "하지 말", "하지마", "하지말"])
        && has(&plain, &["달력", "캘린더", "등록", "메일 발송"])
    {
        for sentence in plain.split(['.', '!', '\n', ',']) {
            if guard(sentence, Act::Request).is_err()
                || !has(sentence, &["찾아", "검색", "읽어", "조회"])
            {
                continue;
            }
            let scope = if has(sentence, &["대화에서", "채팅에서", "얘기에서"]) {
                WorkScope::Conversation
            } else if has(sentence, &["기억에서", "메모에서"]) {
                WorkScope::Memory
            } else {
                continue;
            };
            decision.intent = Intent::Create;
            decision.report = ReportMode::None;
            decision.target = None;
            decision.preference = None;
            let query = search_term(sentence);
            decision.items = vec![DraftItem {
                title: format!("{} 조회", query),
                scope,
                query,
                alternatives: vec![],
            }];
            return decision;
        }
    }
    if (has_group
        && !matches!(
            decision.intent,
            Intent::Create | Intent::Chat | Intent::Clarify
        ))
        || guard(text, Act::Request).is_err()
    {
        return decision;
    }
    let unquoted = outside_quotes(text);
    let read = has(
        &unquoted,
        &["찾", "확인", "꺼내", "검색", "조회", "알아", "살펴", "정리"],
    );
    let combined = has(
        &unquoted,
        &["한 장", "한장", "한 문서", "하나로", "한 페이지"],
    ) && has(&unquoted, &["저장", "정리", "만들"]);
    if read && combined && decision.intent == Intent::Create {
        decision.report = ReportMode::Combined;
    }
    let conversation = [
        "대화에서",
        "얘기에서",
        "이야기에서",
        "채팅에서",
        "나눈 대화",
        "나눈 얘기",
        "나눈 이야기",
    ];
    let memory = [
        "기억에서",
        "메모에서",
        "기억 속",
        "기억속",
        "저장된 기억",
        "적어 둔 기억",
    ];
    if !read || !has(&unquoted, &conversation) || !has(&unquoted, &memory) {
        return decision;
    }
    let parts: Vec<_> = unquoted
        .split([',', '.', '\n'])
        .flat_map(|p| p.split("그리고"))
        .collect();
    if !combined && !has(&unquoted, &["보고서", "문서", "저장"]) {
        decision.report = ReportMode::None;
    }
    if decision.intent != Intent::Create {
        decision.items.clear();
        decision.intent = Intent::Create;
    }
    for (scope, words, label) in [
        (
            WorkScope::Conversation,
            conversation.as_slice(),
            "이전 대화",
        ),
        (WorkScope::Memory, memory.as_slice(), "저장한 기억"),
    ] {
        if decision.items.iter().any(|item| item.scope == scope) {
            continue;
        }
        let clause = parts
            .iter()
            .copied()
            .find(|part| has(part, words))
            .unwrap_or(&unquoted);
        let query = search_term(clause);
        let title = if query.is_empty() {
            label.into()
        } else {
            format!("{label} · {query}")
        };
        decision.items.push(DraftItem {
            title,
            scope,
            query,
            alternatives: vec![],
        });
    }
    if combined {
        decision.report = ReportMode::Combined;
    }
    decision
}
