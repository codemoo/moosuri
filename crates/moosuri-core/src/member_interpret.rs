//! Closed request/reply contract for `POST /api/v1/threads/{thread}/work-interpret`.
//!
//! The reply never carries commands, SQL, member identifiers supplied by a model or
//! derived text. A `work` reply holds the member's own current projection of a group.
use crate::personal_browser::ConversationWorkGroup;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemberMemoryOperation {
    Create,
    Edit,
    Forget,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemberMemoryCandidate {
    pub memory_id: String,
    pub revision: i64,
    pub subject: String,
}
/// Same-thread reply / replay shape (mirrored by `web/src/MemberMemoryReceipt.tsx`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MemberMemoryView {
    /// Current live text. `superseded` means a later revision replaced the one acted on.
    Remembered {
        operation: MemberMemoryOperation,
        memory_id: String,
        revision: i64,
        current_revision: i64,
        subject: String,
        text: String,
        superseded: bool,
    },
    /// Forgotten (or no longer visible): no text is ever shown.
    Withdrawn {
        memory_id: String,
    },
    NoChange {
        reason: String,
        candidates: Vec<MemberMemoryCandidate>,
    },
    Stale,
}

/// The only mail read this route can express: the current first Gmail inbox page, sender/subject/date
/// metadata, at most five entries (the existing `/personal/google/inbox` result scope).
pub const INBOX_READ_SCOPE: &str = "one_inbox_page_metadata_only";
pub const INBOX_READ_LIMIT: u8 = 5;

/// Default member broker reservation must fit the interpretation policy and a
/// bounded member message as well as an answer. This is a dispatch budget, not
/// a token usage estimate; the provider's measured usage remains authoritative.
pub const DEFAULT_MEMBER_MODEL_RESERVATION_TOKENS: u64 = 32_768;

pub const WORK_INTERPRET_MAX_TEXT: usize = 8192;
pub const WORK_INTERPRET_MAX_CLARIFY: usize = 600;

/// Lowercase hyphenated 8-4-4-4-12 hexadecimal, not nil.
fn canonical_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_digit() || (b'a'..=b'f').contains(&b),
        })
        && value.bytes().any(|b| b != b'0' && b != b'-')
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct WorkInterpretRequest {
    pub request_id: String,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub group_id: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub read_only: bool,
}

impl WorkInterpretRequest {
    /// Canonical lowercase hyphenated UUID and a bounded, non-blank message.
    pub fn valid(&self) -> bool {
        canonical_uuid(&self.request_id)
            && self.group_id.as_deref().is_none_or(canonical_uuid)
            && !self.text.trim().is_empty()
            && self.text.len() <= WORK_INTERPRET_MAX_TEXT
            && !self
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
    }
}

#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum WorkFocus {
    Tasks,
    Status,
    Evidence,
    Report,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkInterpretReply {
    /// Ordinary text; the client continues with the existing text preview.
    Chat {},
    Work {
        group: Box<ConversationWorkGroup>,
        focus: WorkFocus,
        /// Always serialized, `null` when the focus covers the whole group.
        task_id: Option<String>,
    },
    Preference {
        thread_id: String,
        view: MemberMemoryView,
    },
    Clarify {
        message: String,
    },
    /// The member directly asked to see their Gmail inbox. This is only an instruction to the
    /// authenticated client to run its existing inbox read for `thread_id` with the request id it
    /// already holds. No account, connection, query or message identifier is ever carried here:
    /// the target is the member's own current selection, resolved by the existing read route.
    InboxRead {
        thread_id: String,
        scope: String,
        limit: u8,
    },
}

impl WorkInterpretReply {
    pub fn inbox_read(thread: &str) -> Self {
        Self::InboxRead {
            thread_id: thread.to_owned(),
            scope: INBOX_READ_SCOPE.to_owned(),
            limit: INBOX_READ_LIMIT,
        }
    }

    /// Shape check used by the gateway: same thread, a task that belongs to the group,
    /// and a bounded, non-empty clarification.
    pub fn valid_for(&self, thread: &str) -> bool {
        match self {
            Self::Chat {} => true,
            Self::Preference { thread_id, view } => {
                thread_id == thread
                    && match view {
                        MemberMemoryView::Remembered {
                            operation,
                            memory_id,
                            revision,
                            current_revision,
                            subject,
                            text,
                            ..
                        } => {
                            *operation != MemberMemoryOperation::Forget
                                && canonical_uuid(memory_id)
                                && *revision > 0
                                && current_revision >= revision
                                && !subject.is_empty()
                                && subject.len() <= 512
                                && text.len() <= 8192
                        }
                        MemberMemoryView::Withdrawn { memory_id } => canonical_uuid(memory_id),
                        MemberMemoryView::NoChange { reason, candidates } => {
                            reason.len() <= 64
                                && candidates.len() <= 5
                                && candidates.iter().all(|item| {
                                    canonical_uuid(&item.memory_id)
                                        && item.revision > 0
                                        && item.subject.len() <= 512
                                })
                        }
                        MemberMemoryView::Stale => true,
                    }
            }
            Self::Work { group, task_id, .. } => {
                group.thread_id == thread
                    && !group.tasks.is_empty()
                    && task_id
                        .as_ref()
                        .is_none_or(|id| group.tasks.iter().any(|task| &task.id == id))
            }
            Self::InboxRead {
                thread_id,
                scope,
                limit,
            } => thread_id == thread && scope == INBOX_READ_SCOPE && *limit == INBOX_READ_LIMIT,
            Self::Clarify { message } => {
                !message.trim().is_empty()
                    && message.chars().count() <= WORK_INTERPRET_MAX_CLARIFY
                    && !message.chars().any(|c| c.is_control() && c != '\n')
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closed_shapes_round_trip_and_reject_extras() {
        assert_eq!(
            serde_json::to_value(WorkInterpretReply::Chat {}).unwrap(),
            serde_json::json!({"kind":"chat"})
        );
        let clarify = serde_json::json!({"kind":"clarify","message":"몇 번째 작업인가요?"});
        let parsed: WorkInterpretReply = serde_json::from_value(clarify.clone()).unwrap();
        assert!(parsed.valid_for("t"));
        assert_eq!(serde_json::to_value(parsed).unwrap(), clarify);
        for bad in [
            serde_json::json!({"kind":"chat","extra":1}),
            serde_json::json!({"kind":"clarify","message":"x","sql":"drop"}),
            serde_json::json!({"kind":"shell"}),
        ] {
            assert!(serde_json::from_value::<WorkInterpretReply>(bad).is_err());
        }
        assert!(
            !WorkInterpretReply::Clarify {
                message: " ".into()
            }
            .valid_for("t")
        );
        assert!(
            serde_json::from_str::<WorkInterpretRequest>(
                r#"{"request_id":"x","text":"a","model":"m"}"#
            )
            .is_err()
        );
    }

    #[test]
    fn inbox_read_is_one_fixed_scope_for_this_thread_and_carries_no_target() {
        let reply = WorkInterpretReply::inbox_read("t");
        let wire = serde_json::to_value(&reply).unwrap();
        assert_eq!(
            wire,
            serde_json::json!({"kind":"inbox_read","thread_id":"t","scope":"one_inbox_page_metadata_only","limit":5})
        );
        assert!(reply.valid_for("t") && !reply.valid_for("other"));
        for bad in [
            serde_json::json!({"kind":"inbox_read","thread_id":"t","scope":"one_inbox_page_metadata_only","limit":5,"connection_id":"c"}),
            serde_json::json!({"kind":"inbox_read","thread_id":"t","scope":"one_inbox_page_metadata_only","limit":5,"query":"from:boss"}),
        ] {
            assert!(serde_json::from_value::<WorkInterpretReply>(bad).is_err());
        }
        for (scope, limit) in [("all_mail", 5), ("one_inbox_page_metadata_only", 50)] {
            let widened: WorkInterpretReply = serde_json::from_value(
                serde_json::json!({"kind":"inbox_read","thread_id":"t","scope":scope,"limit":limit}),
            )
            .unwrap();
            assert!(!widened.valid_for("t"), "{scope} {limit}");
        }
    }

    #[test]
    fn request_validation_is_canonical_and_bounded() {
        let id = "3f2c1d4e-5a6b-4c7d-8e9f-0a1b2c3d4e5f".to_string();
        let ok = |text: &str| {
            WorkInterpretRequest {
                request_id: id.clone(),
                group_id: None,
                read_only: false,
                text: text.into(),
            }
            .valid()
        };
        assert!(ok("여행 대화 찾아줘"));
        assert!(!ok("  "));
        assert!(!ok("a\u{0}b"));
        assert!(!ok(&"가".repeat(WORK_INTERPRET_MAX_TEXT)));
        assert!(
            !WorkInterpretRequest {
                request_id: id.to_uppercase(),
                group_id: None,
                read_only: false,
                text: "a".into()
            }
            .valid()
        );
    }
}
