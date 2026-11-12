use super::{GmailError, validate_id};
use mail_builder::MessageBuilder;
use mail_parser::{Address, MessageParser, MimeHeaders};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

pub const MAX_MIME_BYTES: usize = 512 * 1024;
const MAX_BODY_BYTES: usize = 128 * 1024;
/// Plain ASCII mailbox addresses only in this foundation. Unsupported fields,
/// including attachment input, are rejected rather than silently dropped. Local
/// drafts belong to moosuri; this type does not create a draft in Gmail.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MailContent {
    pub from: String,
    pub to: Vec<String>,
    #[serde(default)]
    pub cc: Vec<String>,
    #[serde(default)]
    pub bcc: Vec<String>,
    pub subject: String,
    pub text: String,
    pub reply: Option<ReplyContext>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReplyContext {
    pub thread_id: String,
    /// RFC Message-ID without angle brackets, from the original message.
    pub in_reply_to: String,
    #[serde(default)]
    pub references: Vec<String>,
}
/// No Debug/Serialize: the exact plaintext snapshot is private user data.
pub struct FrozenMail {
    raw: Zeroizing<Vec<u8>>,
    digest: [u8; 32],
    thread_id: Option<String>,
}
impl FrozenMail {
    /// `message_id` is allocated once upstream and persisted with this snapshot.
    /// This function validates/builds content, but does not authorize dispatch.
    pub fn prepare(content: &MailContent, message_id: &str) -> Result<Self, GmailError> {
        validate_address(&content.from)?;
        let count = content.to.len() + content.cc.len() + content.bcc.len();
        if !(1..=50).contains(&count)
            || content.subject.len() > 2048
            || content.subject.chars().any(char::is_control)
            || content.text.len() > MAX_BODY_BYTES
            || content
                .text
                .chars()
                .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
        {
            return Err(GmailError::InvalidInput);
        }
        for address in content.to.iter().chain(&content.cc).chain(&content.bcc) {
            validate_address(address)?;
        }
        validate_message_id(message_id)?;
        let mut builder = MessageBuilder::new()
            .from(content.from.as_str())
            .subject(content.subject.as_str())
            .message_id(message_id)
            .text_body(content.text.as_str());
        if !content.to.is_empty() {
            builder = builder.to(content.to.iter().map(String::as_str).collect::<Vec<_>>());
        }
        if !content.cc.is_empty() {
            builder = builder.cc(content.cc.iter().map(String::as_str).collect::<Vec<_>>());
        }
        if !content.bcc.is_empty() {
            builder = builder.bcc(content.bcc.iter().map(String::as_str).collect::<Vec<_>>());
        }
        let thread_id = if let Some(reply) = &content.reply {
            validate_id(&reply.thread_id)?;
            validate_message_id(&reply.in_reply_to)?;
            if reply.references.len() > 50 {
                return Err(GmailError::InvalidInput);
            }
            for reference in &reply.references {
                validate_message_id(reference)?;
            }
            builder = builder.in_reply_to(reply.in_reply_to.as_str());
            let mut references = reply
                .references
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>();
            if references.last() != Some(&reply.in_reply_to.as_str()) {
                references.push(&reply.in_reply_to);
            }
            builder = builder.references(references);
            Some(reply.thread_id.clone())
        } else {
            None
        };
        let raw = builder
            .write_to_vec()
            .map_err(|_| GmailError::InvalidInput)?;
        let digest = snapshot_digest(&raw, thread_id.as_deref());
        Self::restore(raw, digest, thread_id)
    }
    /// Rehydrate only from the trusted caller's owner-scoped immutable storage.
    /// A matching digest proves byte equality, never user approval. The caller
    /// digest includes thread_id as well as the exact MIME bytes.
    pub fn restore(
        raw: Vec<u8>,
        expected_digest: [u8; 32],
        thread_id: Option<String>,
    ) -> Result<Self, GmailError> {
        if raw.is_empty() || raw.len() > MAX_MIME_BYTES {
            return Err(GmailError::LimitExceeded);
        }
        if let Some(thread) = &thread_id {
            validate_id(thread)?;
        }
        let digest = snapshot_digest(&raw, thread_id.as_deref());
        if digest != expected_digest {
            return Err(GmailError::InvalidInput);
        }
        Ok(Self {
            raw: Zeroizing::new(raw),
            digest,
            thread_id,
        })
    }
    pub fn raw_mime(&self) -> &[u8] {
        &self.raw
    }
    pub fn digest(&self) -> [u8; 32] {
        self.digest
    }
    pub fn thread_id(&self) -> Option<&str> {
        self.thread_id.as_deref()
    }
}
#[derive(Serialize)]
pub struct ParsedMail {
    pub message_id: Option<String>,
    pub subject: String,
    pub from: Vec<String>,
    pub to: Vec<String>,
    pub cc: Vec<String>,
    pub bcc: Vec<String>,
    pub text: String,
    pub text_truncated: bool,
    /// Metadata only. Attachment download/storage is a later artifact adapter.
    pub attachment_names: Vec<String>,
}
/// Maintained MIME parser handles header decoding, charsets and transfer
/// encoding. No HTML is returned or loaded. HTML-only mail is converted to text
/// by mail-parser; output remains untrusted plain text, never tool authority.
pub fn parse_mail(raw: &[u8]) -> Result<ParsedMail, GmailError> {
    if raw.is_empty() || raw.len() > MAX_MIME_BYTES {
        return Err(GmailError::LimitExceeded);
    }
    let parsed = MessageParser::default()
        .parse(raw)
        .ok_or(GmailError::InvalidResponse)?;
    let mut text = parsed
        .body_text(0)
        .map(|s| s.into_owned())
        .unwrap_or_default();
    // Multipart/mixed can carry additional logical inline sections. The
    // foundation exposes the first section only; never describe that excerpt
    // as the complete email when further sections exist.
    let text_truncated = text.len() > MAX_BODY_BYTES || parsed.text_body_count() > 1;
    if text.len() > MAX_BODY_BYTES {
        let mut end = MAX_BODY_BYTES;
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        text.truncate(end);
    }
    let bounded = |s: &str, max: usize| {
        let mut end = s.len().min(max);
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        s[..end].to_owned()
    };
    let addresses = |address: Option<&Address<'_>>| -> Vec<String> {
        address
            .into_iter()
            .flat_map(Address::iter)
            .take(100)
            .filter_map(|a| a.address.as_deref())
            .map(|s| bounded(s, 320))
            .collect()
    };
    Ok(ParsedMail {
        message_id: parsed.message_id().map(|s| bounded(s, 998)),
        subject: bounded(parsed.subject().unwrap_or_default(), 2048),
        from: addresses(parsed.from()),
        to: addresses(parsed.to()),
        cc: addresses(parsed.cc()),
        bcc: addresses(parsed.bcc()),
        text,
        text_truncated,
        attachment_names: parsed
            .attachments()
            .take(50)
            .map(|p| bounded(p.attachment_name().unwrap_or("unnamed"), 256))
            .collect(),
    })
}
pub(super) fn validate_address(address: &str) -> Result<(), GmailError> {
    if address.len() > 254 || !address.is_ascii() {
        return Err(GmailError::InvalidInput);
    }
    let Some((local, domain)) = address.split_once('@') else {
        return Err(GmailError::InvalidInput);
    };
    if local.is_empty()
        || local.len() > 64
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
        || !local
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".!#$%&'*+-/=?^_`{|}~".contains(&b))
        || domain.is_empty()
        || domain.len() > 253
        || !domain.contains('.')
        || domain.split('.').any(|s| {
            s.is_empty()
                || s.len() > 63
                || s.starts_with('-')
                || s.ends_with('-')
                || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
    {
        return Err(GmailError::InvalidInput);
    }
    Ok(())
}
fn snapshot_digest(raw: &[u8], thread: Option<&str>) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"moosuri/gmail-frozen/v1\0");
    let thread = thread.unwrap_or_default().as_bytes();
    hash.update((thread.len() as u64).to_be_bytes());
    hash.update(thread);
    hash.update((raw.len() as u64).to_be_bytes());
    hash.update(raw);
    hash.finalize().into()
}
fn validate_message_id(id: &str) -> Result<(), GmailError> {
    // Narrow valid addr-spec form. Angle brackets are supplied by mail-builder.
    validate_address(id)
}
