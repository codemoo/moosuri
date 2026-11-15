//! Model-readable native mail is a separate opt-in from legacy display projections.
//! Provider IDs, provider page tokens, OAuth fields and account identities never belong here.
use serde::{Deserialize, Serialize};
use uuid::Uuid;
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailModelRequest {
    pub version: u8,
    pub background: bool,
    pub read: MailModelRead,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum MailModelRead {
    Search { query: String, cursor: Option<Uuid> },
    Message { selection: Uuid },
    Thread { selection: Uuid },
    Attachment { selection: Uuid, page: u16 },
}
impl MailModelRequest {
    pub fn valid(&self) -> bool {
        self.version == 1
            && match &self.read {
                MailModelRead::Search { query, cursor } => {
                    !query.is_empty()
                        && query.len() <= 512
                        && !query.chars().any(char::is_control)
                        && cursor.is_none_or(|c| !c.is_nil())
                }
                MailModelRead::Message { selection } | MailModelRead::Thread { selection } => {
                    !selection.is_nil()
                }
                MailModelRead::Attachment { selection, page } => !selection.is_nil() && *page < 50,
            }
    }
    /// Structs/enums only, with fixed field order and no maps or arbitrary JSON.
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("typed mail request")
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MailModelStatus {
    Ready,
    Missing,
    UnsupportedFormat,
    ParserUnavailable,
    ScopeMissing,
    AccountMismatch,
    SourceChanged,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailModelAttachment {
    pub selection: Uuid,
    pub filename: String,
    pub media_type: String,
    pub readable: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailModelMessage {
    pub selection: Uuid,
    pub thread: Uuid,
    pub observation: Uuid,
    pub subject: String,
    pub from: String,
    pub text: String,
    pub partial: bool,
    pub content_digest: [u8; 32],
    pub attachments: Vec<MailModelAttachment>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailModelResult {
    pub version: u8,
    pub status: MailModelStatus,
    pub messages: Vec<MailModelMessage>,
    pub next_cursor: Option<Uuid>,
    pub partial: bool,
    /// Attachment pages reuse the selected parent message/thread identities.
    pub attachment: Option<MailModelAttachmentPage>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailModelAttachmentPage {
    pub selection: Uuid,
    pub message: Uuid,
    pub thread: Uuid,
    pub observation: Uuid,
    pub page: u16,
    pub total_pages: u16,
    pub text: String,
    pub content_digest: [u8; 32],
    pub partial: bool,
}
impl MailModelResult {
    pub fn bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).expect("typed mail result")
    }
    pub fn valid(&self) -> bool {
        fn text(s: &str, max: usize) -> bool {
            s.len() <= max
                && !s
                    .chars()
                    .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        }
        self.version == 1
            && self.bytes().len() <= 12000
            && self.messages.len() <= 5
            && self.next_cursor.is_none_or(|c| !c.is_nil())
            && (self.status == MailModelStatus::Ready
                || (self.messages.is_empty()
                    && self.attachment.is_none()
                    && self.next_cursor.is_none()))
            && self.messages.iter().all(|m| {
                !m.selection.is_nil()
                    && !m.thread.is_nil()
                    && !m.observation.is_nil()
                    && text(&m.subject, 256)
                    && text(&m.from, 256)
                    && text(&m.text, 4096)
                    && m.attachments.len() <= 8
                    && m.attachments.iter().all(|a| {
                        !a.selection.is_nil()
                            && text(&a.filename, 128)
                            && matches!(
                                a.media_type.as_str(),
                                "application/pdf" | "text/plain" | "unsupported"
                            )
                    })
            })
            && self.attachment.as_ref().is_none_or(|a| {
                !a.selection.is_nil()
                    && !a.message.is_nil()
                    && !a.thread.is_nil()
                    && !a.observation.is_nil()
                    && a.page < a.total_pages
                    && a.total_pages <= 50
                    && text(&a.text, 4096)
            })
    }
    pub fn status(status: MailModelStatus) -> Self {
        Self {
            version: 1,
            status,
            messages: vec![],
            next_cursor: None,
            partial: false,
            attachment: None,
        }
    }
}
impl MailModelRequest {
    /// Backend selector lineage. This adds no signed fields or wire discriminants.
    pub fn accepts_selector(&self, origin: &Self, result: &MailModelResult) -> bool {
        if !self.valid()
            || !origin.valid()
            || !result.valid()
            || self.background != origin.background
        {
            return false;
        }
        match &self.read {
            MailModelRead::Search {
                query,
                cursor: Some(cursor),
            } => {
                result.next_cursor == Some(*cursor)
                    && matches!(&origin.read,MailModelRead::Search{query:old,..}if old==query)
            }
            MailModelRead::Search { cursor: None, .. } => true,
            MailModelRead::Message { selection } => {
                result.messages.iter().any(|m| m.selection == *selection)
            }
            MailModelRead::Thread { selection } => result
                .messages
                .iter()
                .any(|m| m.thread == *selection || m.selection == *selection),
            MailModelRead::Attachment { selection, page: 0 } => result
                .messages
                .iter()
                .any(|m| m.attachments.iter().any(|a| a.selection == *selection)),
            MailModelRead::Attachment { selection, page } => {
                result.attachment.as_ref().is_some_and(|a| {
                    a.selection == *selection
                        && a.page.checked_add(1) == Some(*page)
                        && *page < a.total_pages
                })
            }
        }
    }
}
