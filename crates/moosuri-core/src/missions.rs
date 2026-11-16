//! Explicit owner requests. Model proposals never carry execution authority.
use serde::{Deserialize, Serialize};
use ts_rs::TS;
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum MissionMode {
    CurrentSession,
    Durable,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionCheck {
    /// Explicit current owner authority to create one private report.
    PrivateReport {
        title: String,
    },
    DecimalEquals {
        expected: String,
    },
    SourceQuote {
        id: String,
        #[ts(type = "number")]
        revision: i64,
        quote: String,
    },
    OutputContains {
        text: String,
    },
    UserReview {
        description: String,
    },
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionCreate {
    pub request_id: String,
    pub task: String,
    pub constraints: String,
    pub checks: Vec<MissionCheck>,
    pub execution_mode: MissionMode,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionControl {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub action: MissionAction,
}
/// Explicit owner save, separate from additive steering and budget changes.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionAcceptanceUpdate {
    pub request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub task: String,
    pub constraints: String,
    pub checks: Vec<MissionCheck>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionAcceptanceSnapshot {
    #[ts(type = "number")]
    pub revision: i64,
    pub request: MissionCreate,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionAction {
    Pause,
    Resume,
    Cancel,
    Steer { instruction: String },
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionEvidence {
    #[ts(type = "number")]
    pub step: i64,
    pub call_id: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionCandidate {
    pub text: String,
    pub evidence: Vec<MissionEvidence>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionReceipt {
    pub label: String,
    pub detail: String,
}
/// Values for already admitted one-off actions; never new operation authority.
#[derive(Clone, Serialize, Deserialize, TS, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MissionInputField {
    MailTo,
    CalendarDate,
    CalendarStart,
    CalendarDuration,
    Context,
    SourceChoice,
    CalendarTarget,
}
/// A choice is a real current-run receipt observation, never model-created authority.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionInputCandidate {
    pub id: String,
    pub revision: String,
    #[ts(type = "number")]
    pub step: i64,
    pub call_id: String,
    #[serde(default)]
    pub label: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionInputRequest {
    #[serde(default)]
    #[ts(as = "Option<Vec<MissionInputCandidate>>", optional)]
    pub candidates: Vec<MissionInputCandidate>,
    pub field: MissionInputField,
    pub question: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionInputWait {
    #[serde(default)]
    #[ts(as = "Option<Vec<MissionInputCandidate>>", optional)]
    pub candidates: Vec<MissionInputCandidate>,
    pub request_id: String,
    pub field: MissionInputField,
    pub question: String,
    pub state: String,
    pub value: Option<String>,
    #[ts(type = "number")]
    pub acceptance_revision: i64,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionInputReply {
    pub request_id: String,
    pub wait_request_id: String,
    #[ts(type = "number")]
    pub expected_revision: i64,
    pub value: String,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instruction_target: Option<crate::RunInstructionTarget>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub instructions: Option<Vec<crate::RunInstructionReceipt>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub input_wait: Option<MissionInputWait>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<MissionInputWait>>", optional)]
    pub inputs: Vec<MissionInputWait>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<MissionLinkedAction>>", optional)]
    pub actions: Vec<MissionLinkedAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub compound: Option<MissionCompound>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub wait: Option<crate::MissionWaitView>,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub state: String,
    pub request: MissionCreate,
    /// Absent for unchanged/recurring missions; request is the active snapshot.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub acceptance_history: Option<Vec<MissionAcceptanceSnapshot>>,
    pub steering: Vec<String>,
    pub plan: Vec<String>,
    #[ts(type = "number")]
    pub completed_steps: i64,
    pub result: Option<MissionCandidate>,
    pub receipts: Vec<MissionReceipt>,
    pub report: Vec<String>,
    pub uncertain: bool,
    #[ts(type = "number")]
    pub expires_at_ms: i64,
}

/// Narrow proposals; authority comes from the originally accepted owner request.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionActionRequest {
    SaveProviderDraft {
        intent_quote: String,
        action_id: String,
        expected: crate::MailApproval,
    },
    PrepareMail {
        intent_quote: String,
        input: Box<crate::MailDraftInput>,
        evidence: Vec<MissionEvidence>,
    },
    Calendar {
        decision: Box<crate::CalendarChatDecision>,
        evidence: Vec<MissionEvidence>,
    },
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionLinkedAction {
    pub kind: String,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
    pub digest: String,
    pub state: String,
    pub current: bool,
    pub url: String,
    pub provider_receipt: Option<String>,
}

/// A saved immutable mission template and separately authorized schedule.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct RecurringMissionCreate {
    pub request_id: String,
    pub template: MissionCreate,
    pub schedule: crate::ScheduleDraft,
    pub changes_only: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct RecurringMissionView {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub missing_skills: Option<Vec<String>>,
    pub schedule: crate::ScheduleView,
    pub template: MissionCreate,
    pub model: crate::ModelChoice,
    pub changes_only: bool,
    pub recent: Vec<RecurringMissionOccurrence>,
    pub history: crate::ScheduleHistory,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct RecurringMissionOccurrence {
    pub occurrence_id: String,
    pub mission_id: String,
    pub state: String,
    pub notification_id: Option<String>,
    pub exact_unchanged: bool,
    #[ts(type = "number")]
    pub scheduled_for: i64,
}

/// Original current-request selector, never a model-supplied identity.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionChatTarget {
    pub kind: String,
    pub id: String,
    #[ts(type = "number")]
    pub revision: i64,
}
#[derive(Clone, Default, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionChatContext {
    pub target: Option<MissionChatTarget>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionChatReceipt {
    pub request_id: String,
    pub action: String,
    pub target: Option<MissionChatTarget>,
    pub state: String,
    pub next_local_slot: Option<String>,
    pub candidates: Vec<MissionChatCandidate>,
}
/// Shared model-schema and Store admission vocabulary, never model-written prose.
pub const MISSION_CHAT_CLARIFICATIONS: &[&str] =
    &["time", "target", "task", "ambiguous", "unsupported"];

/// A bounded semantic proposal. Quotes establish origin, not semantic authorization.
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(deny_unknown_fields)]
pub struct MissionChatDecision {
    pub decision: String,
    pub action: Option<String>,
    pub target_kind: Option<String>,
    pub intent_quote: Option<String>,
    pub task_quote: Option<String>,
    pub constraints_quote: Option<String>,
    pub time_quote: Option<String>,
    pub target_quote: Option<String>,
    pub instruction_quote: Option<String>,
    pub time: Option<MissionChatTime>,
    pub clarification: Option<String>,
}
#[derive(Clone, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MissionChatTime {
    Relative { seconds: u32 },
    LocalOnce { date: String, time: String },
    Interval { seconds: u32 },
    Daily { time: String },
    Weekly { days: Vec<u8>, time: String },
}

#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionChatCandidate {
    pub target: MissionChatTarget,
    pub title: String,
}

/// Store-built bounded structure; tool outputs cannot add a node or an effect.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionCompound {
    pub schema_version: u32,
    pub calculation_required: bool,
    #[ts(type = "number")]
    pub acceptance_revision: i64,
    pub capability_profile_sha256: String,
    pub steps: Vec<MissionCompoundStep>,
    pub output: Option<crate::ArtifactReceipt>,
    pub review_required: bool,
}
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionCompoundStep {
    pub id: String,
    pub purpose: String,
    pub capability: String,
    pub input_slots: Vec<String>,
    pub depends_on: Vec<String>,
    pub receipts: Vec<MissionCompoundReceipt>,
    pub output: String,
    pub checks: Vec<String>,
    pub state: String,
}
/// Cost routing only. Actual command interpretation is isolated from evidence.
pub fn compound_report_candidate(text: &str) -> bool {
    let low = text.to_lowercase();
    let operations = outside_quoted_literals(&low);
    // Reading before an explicit edit is still one existing-record mutation.
    // Keep its isolated artifact planner reachable instead of sealing a
    // read-only profile for the new-report route. Quoted names/bodies are data.
    if existing_record_edit_request(&operations) {
        return false;
    }
    // "saved document" / "저장된 문서" describes input, not a request
    // to save new output. Keep separate active save occurrences eligible.
    let english_output = low
        .split(|c: char| !c.is_alphabetic())
        .any(|word| matches!(word, "save" | "create" | "write"));
    let compact = low.split_whitespace().collect::<String>();
    let korean_output = ["저장", "작성", "만들"].iter().any(|stem| {
        compact.match_indices(stem).any(|(at, _)| {
            let tail = &compact[at + stem.len()..];
            let modifiers: &[&str] = if *stem == "만들" {
                &["어진", "어둔", "어놓은", "었던"]
            } else {
                &["된", "한", "되어", "돼", "해둔", "해놓은", "했던", "하던"]
            };
            !modifiers.iter().any(|modifier| tail.starts_with(modifier))
        })
    });
    ["report", "document", "보고서", "문서"]
        .iter()
        .any(|v| low.contains(v))
        && (english_output || korean_output)
        && [
            "research",
            "compare",
            "read",
            "calculate",
            "조사",
            "비교",
            "읽",
            "확인",
            "계산",
        ]
        .iter()
        .any(|v| operations.contains(v))
}

fn existing_record_edit_request(operations: &str) -> bool {
    let words = operations
        .split(|c: char| !c.is_ascii_alphabetic())
        .filter(|word| !word.is_empty())
        .collect::<Vec<_>>();
    let english = words.iter().enumerate().any(|(at, word)| {
        matches!(
            *word,
            "edit" | "revise" | "update" | "rename" | "append" | "correct"
        ) && (at == 0
            || matches!(words[at - 1], "and" | "then" | "please" | "to")
            || words.get(at + 1).is_some_and(|next| {
                matches!(
                    *next,
                    "it" | "its" | "this" | "that" | "the" | "my" | "our" | "existing"
                )
            }))
    });
    let compact = operations.split_whitespace().collect::<String>();
    english
        || [
            "수정해",
            "수정하",
            "고쳐",
            "고치",
            "바꿔",
            "바꾸",
            "변경해",
            "변경하",
            "추가해",
            "추가하",
            "덧붙여",
        ]
        .iter()
        .any(|verb| {
            compact.match_indices(verb).any(|(at, _)| {
                // Completed input modifiers such as 수정해 둔 describe a source.
                !["둔", "놓은"]
                    .iter()
                    .any(|suffix| compact[at + verb.len()..].starts_with(suffix))
            })
        })
}

/// Routing cues inside a quoted title/body are data. Unmatched quotes stay
/// visible; apostrophes inside words are not quotation delimiters.
fn outside_quoted_literals(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut cursor = 0;
    while cursor < text.len() {
        let c = text[cursor..].chars().next().unwrap();
        let close = match c {
            '"' => Some('"'),
            '“' => Some('”'),
            '‘' => Some('’'),
            '\'' if cursor == 0
                || !text[..cursor].ends_with(|c: char| c.is_ascii_alphanumeric()) =>
            {
                Some('\'')
            }
            _ => None,
        };
        if let Some(close) = close {
            let start = cursor + c.len_utf8();
            let end = text[start..]
                .char_indices()
                .find_map(|(offset, candidate)| {
                    let after = start + offset + candidate.len_utf8();
                    (candidate == close
                        && (close != '\''
                            || !text[after..].starts_with(|c: char| c.is_ascii_alphanumeric())))
                    .then_some(after)
                });
            if let Some(end) = end {
                output.push(' ');
                cursor = end;
                continue;
            }
        }
        output.push(c);
        cursor += c.len_utf8();
    }
    output
}

/// Exact top-level or trusted learned-child receipt identity. Legacy candidate
/// references remain unchanged; only Store may resolve a child reference.
#[derive(Clone, Serialize, Deserialize, TS)]
pub struct MissionCompoundReceipt {
    #[ts(type = "number")]
    pub step: i64,
    pub call_id: String,
    pub ordinal: Option<u8>,
}

/// Narrow latency routing only: complete generic answer/explanation follow-ups.
/// Unknown wording or named task titles must retain ordinary task interpretation.
pub fn ordinary_answer_continuation(text: &str) -> bool {
    let compact: String = text
        .chars()
        .filter(|c| !c.is_whitespace() && !matches!(c, ',' | '.' | '!' | '?' | '，' | '。'))
        .collect();
    [
        "답변",
        "답변을",
        "설명",
        "설명을",
        "대답",
        "대답을",
        "이야기",
        "이야기를",
    ]
    .iter()
    .any(|object| {
        ["", "앞선", "이전", "앞의", "이전의", "두번째"]
            .iter()
            .any(|prefix| {
                ["이어줘", "이어주세요", "계속해줘", "계속해주세요"]
                    .iter()
                    .any(|verb| compact == format!("{prefix}{object}{verb}"))
            })
    }) || [
        "이어서설명해줘",
        "이어서설명해주세요",
        "계속설명해줘",
        "계속설명해주세요",
    ]
    .contains(&compact.as_str())
        || ["방금말한", "앞서말한", "앞에서말한", "앞서설명한"]
            .iter()
            .filter_map(|prefix| compact.strip_prefix(prefix))
            .any(|rest| {
                ["내용을", "두가지를", "두개를", "세가지를"]
                    .iter()
                    .filter_map(|object| rest.strip_prefix(object))
                    .any(|rest| {
                        ["", "한문장으로", "짧게", "간단히"]
                            .iter()
                            .filter_map(|style| rest.strip_prefix(style))
                            .any(|rest| {
                                [
                                    "이어서설명해줘",
                                    "이어서설명해주세요",
                                    "이어설명해줘",
                                    "이어설명해주세요",
                                ]
                                .contains(&rest)
                            })
                    })
            })
}

#[cfg(test)]
mod compound_report_tests {
    use super::{compound_report_candidate, ordinary_answer_continuation};

    #[test]
    fn ordinary_answer_continuation_only_matches_complete_generic_followups() {
        for text in [
            "두 번째 답변을 이어줘",
            "이어서 설명해 주세요.",
            "방금 말한 두 가지를 한 문장으로 이어서 설명해 줘.",
            "앞서 말한 내용을 짧게 이어서 설명해 주세요.",
            "앞에서 말한 세 가지를 간단히 이어 설명해 줘",
        ] {
            assert!(ordinary_answer_continuation(text), "{text}");
        }
        for text in [
            "고객 답변 조사 이어줘",
            "Coastal digest 이어줘",
            "그 업무 이어줘",
            "계속해 줘",
            "방금 말한 두 가지 업무를 이어서 설명해 줘",
            "앞서 말한 내용을 짧게 이어서 설명하고 내일 업무로 맡아 줘",
        ] {
            assert!(!ordinary_answer_continuation(text), "{text}");
        }
    }

    #[test]
    fn quoted_record_names_are_not_new_research_instructions() {
        for text in [
            "그 내용을 '여행 취소 계산' 문서로 저장해 줘.",
            "그 내용을 ‘견적 비교’ 문서로 저장해 줘.",
            "그 내용을 '여행 취소 계산'이라는 문서로 저장해 줘.",
            "Save a document named \"Read and compare\" with this note.",
            "Save a document named 'Don't calculate' with this note.",
        ] {
            assert!(!compound_report_candidate(text), "{text}");
        }
        for text in [
            "자료를 읽고 '여행 계산' 문서로 저장해 줘.",
            "Compare the quotes and save the report as 'Travel'.",
            "Don't send anything; calculate the fee and save a document.",
        ] {
            assert!(compound_report_candidate(text), "{text}");
        }
    }
    #[test]
    fn existing_document_modifiers_do_not_request_a_new_report_but_explicit_output_still_routes() {
        for text in [
            "저장된 syntheticcapacity 문서를 찾아 읽고 기준량을 알려 줘.",
            "작성한 보고서를 읽고 요약해 줘.",
            "Read the saved document and summarize it.",
            "Read the created report and compare its claims.",
        ] {
            assert!(!compound_report_candidate(text), "{text}");
        }
        for text in [
            "자료를 읽고 보고서를 저장해 줘.",
            "저장된 문서를 읽고 비교 보고서로 저장해 줘.",
            "작성한 보고서를 비교하고 새 보고서를 작성해 줘.",
            "Read the saved document and save a comparison report.",
        ] {
            assert!(compound_report_candidate(text), "{text}");
        }
    }

    #[test]
    fn existing_record_edit_requests_stay_in_the_document_domain() {
        for text in [
            "'주간 보고서' 문서를 읽고 내용을 수정해서 저장해 줘.",
            "그 문서를 확인하고 제목을 바꿔서 저장해 줘.",
            "Read the 'Weekly' report, revise it and save the document.",
            "Read the document and update its title, then save it.",
        ] {
            assert!(!compound_report_candidate(text), "{text}");
        }
        for text in [
            "자료를 읽고 '수정하고 저장' 보고서를 새로 작성해 줘.",
            "수정한 문서를 읽고 새 비교 보고서를 작성해 줘.",
            "Read the revised document and create a comparison report.",
            "Read the source and create a report named 'Edit and update'.",
            "Read the release notes and create a software update report.",
            "Calculate the fee and save a document named 'Append'.",
            "Calculate the correct total and save a report.",
        ] {
            assert!(compound_report_candidate(text), "{text}");
        }
    }
}
