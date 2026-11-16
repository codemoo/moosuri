//! Reusable synthetic owner-visible inputs. No expected answers or grading data.
//! Future live-model drivers may reuse these without importing the grader.
use serde::Serialize;
#[derive(Serialize)]
pub struct Fixture {
    pub id: &'static str,
    pub situation: &'static str,
    pub source: Option<&'static str>,
    pub messages: &'static [&'static str],
}
pub const QUOTE: &str = "Synthetic quote: amount 18.25 shipping 6.50 USD. Delivery: standard or express; ask which option to describe.";
pub const QUESTION: &str =
    "Calculate the saved quote amount plus shipping in USD. Do not purchase anything.";
pub const GUIDANCE: &str = "Also show the shipping amount separately; do not repeat the completed calculation or purchase anything.";
pub const CRITERION_TASK: &str = "Review the saved quote, calculate amount plus shipping, and describe the delivery option I select. Ask me which delivery option to describe; do not purchase anything.";
pub const CRITERION_ANSWER: &str = "Describe standard delivery and emphasize exact cost.";
pub const CREATE_REMINDER: &str = "내일 오전 10시에 치과 예약 확인하라고 알려줘.";
pub const CHANGE_REMINDER: &str = "방금 그 알림을 30분 늦춰줘.";
pub const CANCEL_REMINDER: &str = "방금 그 알림은 취소해 줘.";
pub const FIXTURES: &[Fixture] = &[
    Fixture {
        id: "everyday-quote-inflight-guidance",
        situation: "The owner adds a detail after the assistant has read the quote and calculated the total.",
        source: Some(QUOTE),
        messages: &[QUESTION, GUIDANCE],
    },
    Fixture {
        id: "everyday-quote-owner-clarification",
        situation: "The assistant must keep the already read quote while waiting for the owner's delivery criterion, including across restart.",
        source: Some(QUOTE),
        messages: &[CRITERION_TASK, CRITERION_ANSWER],
    },
    Fixture {
        id: "everyday-reminder-receipt-change-cancel",
        situation: "The owner moves the reminder just created, then cancels it; another reminder exists.",
        source: None,
        messages: &[CREATE_REMINDER, CHANGE_REMINDER, CANCEL_REMINDER],
    },
];
