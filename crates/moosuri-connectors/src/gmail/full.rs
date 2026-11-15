//! Official full-message MIME tree and exact part retrieval. No attachment URLs.
use super::*;
use base64::engine::general_purpose::STANDARD;
use sha2::Digest;
use std::collections::HashSet;

pub const MAX_ATTACHMENT_BYTES: usize = 2 * 1024 * 1024;
const MAX_BODY_BYTES: usize = 128 * 1024;
const MAX_PARTS: usize = 64;
const MAX_DEPTH: usize = 8;
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct AttachmentMetadata {
    pub part_id: String,
    pub attachment_id: Option<String>,
    pub filename: String,
    pub mime_type: String,
    pub size: u64,
    /// Only PDF and UTF-8 plain text are read by this adapter.
    pub readable: bool,
}
/// Private bytes, never Debug/Serialize or a downloaded local file.
pub struct AttachmentRead {
    pub message_id: String,
    pub thread_id: String,
    pub metadata: AttachmentMetadata,
    /// None means metadata-only: unsupported type or declared size above the cap.
    pub bytes: Option<Zeroizing<Vec<u8>>>,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct FullMessage {
    pub id: String,
    pub thread_id: String,
    #[serde(default)]
    pub label_ids: Vec<String>,
    pub history_id: Option<String>,
    pub internal_date: Option<String>,
    pub payload: Part,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Part {
    #[serde(default)]
    part_id: String,
    mime_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    headers: Vec<Header>,
    #[serde(default)]
    body: PartBody,
    #[serde(default)]
    parts: Vec<Part>,
}
#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "camelCase")]
struct PartBody {
    #[serde(default)]
    size: u64,
    attachment_id: Option<String>,
    #[serde(default)]
    data: String,
}
fn part_id(value: &str) -> Result<(), GmailError> {
    if value.len() > 128
        || !value
            .bytes()
            .all(|v| v.is_ascii_alphanumeric() || b"-_.".contains(&v))
    {
        return Err(GmailError::InvalidInput);
    }
    Ok(())
}
fn external_id(value: &str) -> Result<(), GmailError> {
    if value.is_empty()
        || value.len() > 2048
        || !value
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        Err(GmailError::InvalidInput)
    } else {
        Ok(())
    }
}
fn is_attachment(p: &Part) -> bool {
    !p.filename.is_empty()
        || (!p.mime_type.starts_with("multipart/")
            && !matches!(p.mime_type.as_str(), "text/plain" | "text/html"))
        || p.headers.iter().any(|h| {
            h.name.eq_ignore_ascii_case("content-disposition")
                && h.value.to_ascii_lowercase().starts_with("attachment")
        })
}
fn readable(mime: &str, size: u64) -> bool {
    matches!(mime, "application/pdf" | "text/plain") && size <= MAX_ATTACHMENT_BYTES as u64
}
fn metadata(p: &Part) -> AttachmentMetadata {
    AttachmentMetadata {
        part_id: p.part_id.clone(),
        attachment_id: p.body.attachment_id.clone(),
        filename: if p.filename.is_empty() {
            "unnamed".into()
        } else {
            p.filename.clone()
        },
        mime_type: p.mime_type.clone(),
        size: p.body.size,
        readable: readable(&p.mime_type, p.body.size),
    }
}
impl FullMessage {
    pub(crate) fn decode(bytes: &[u8], id: &str) -> Result<Self, GmailError> {
        let full: Self = super::decode(bytes)?;
        if full.id != id || full.label_ids.len() > 128 {
            return Err(GmailError::InvalidResponse);
        }
        validate_id(&full.thread_id).map_err(|_| GmailError::InvalidResponse)?;
        for label in &full.label_ids {
            validate_atom(label, 256).map_err(|_| GmailError::InvalidResponse)?;
        }
        let mut seen = HashSet::new();
        let mut count = 0;
        let mut headers = 0;
        fn check(
            p: &Part,
            depth: usize,
            count: &mut usize,
            headers: &mut usize,
            seen: &mut HashSet<String>,
        ) -> Result<(), GmailError> {
            *count += 1;
            if *count > MAX_PARTS
                || depth > MAX_DEPTH
                || p.headers.len() > 128
                || p.filename.len() > 256
                || p.filename.chars().any(char::is_control)
                || p.mime_type.len() > 128
                || p.mime_type.chars().any(char::is_control)
                || p.body.data.len() > MAX_ATTACHMENT_BYTES * 4 / 3 + 8
            {
                return Err(GmailError::LimitExceeded);
            }
            part_id(&p.part_id).map_err(|_| GmailError::InvalidResponse)?;
            if !seen.insert(p.part_id.clone()) {
                return Err(GmailError::InvalidResponse);
            }
            if let Some(id) = &p.body.attachment_id {
                external_id(id).map_err(|_| GmailError::InvalidResponse)?;
            }
            for h in &p.headers {
                *headers += h.name.len() + h.value.len();
                if *headers > 64 * 1024
                    || h.value.len() > 8192
                    || h.name.is_empty()
                    || h.name.len() > 128
                    || !h
                        .name
                        .bytes()
                        .all(|c| c.is_ascii_alphanumeric() || c == b'-')
                {
                    return Err(GmailError::LimitExceeded);
                }
            }
            for child in &p.parts {
                check(child, depth + 1, count, headers, seen)?;
            }
            Ok(())
        }
        check(&full.payload, 0, &mut count, &mut headers, &mut seen)?;
        Ok(full)
    }
    pub(crate) fn attachment(&self, id: &str, external: Option<&str>) -> Result<&Part, GmailError> {
        part_id(id)?;
        if let Some(id) = external {
            external_id(id)?;
        }
        fn find<'a>(p: &'a Part, id: &str) -> Option<&'a Part> {
            if p.part_id == id {
                Some(p)
            } else {
                p.parts.iter().find_map(|p| find(p, id))
            }
        }
        let p = find(&self.payload, id).ok_or(GmailError::InvalidInput)?;
        if !is_attachment(p) || p.body.attachment_id.as_deref() != external {
            return Err(GmailError::InvalidInput);
        }
        Ok(p)
    }
    /// Bounded external body requests, selecting the same MIME alternatives as
    /// projection. Include inline sizes in the shared body budget; unhydrated
    /// excess parts remain explicitly partial in project().
    pub(crate) fn external_bodies(&self) -> Vec<(String, String)> {
        fn gather(p: &Part, remaining: &mut u64, out: &mut Vec<(String, String)>) {
            if is_attachment(p) {
                return;
            }
            if p.mime_type == "multipart/alternative" {
                if let Some(p) = p
                    .parts
                    .iter()
                    .find(|p| p.mime_type == "text/plain")
                    .or_else(|| p.parts.first())
                {
                    gather(p, remaining, out);
                }
            } else if p.mime_type.starts_with("multipart/") {
                for p in &p.parts {
                    gather(p, remaining, out);
                }
            } else if matches!(p.mime_type.as_str(), "text/plain" | "text/html")
                && p.body.size <= *remaining
            {
                *remaining -= p.body.size;
                if out.len() < 4
                    && let Some(id) = &p.body.attachment_id
                {
                    out.push((p.part_id.clone(), id.clone()));
                }
            }
        }
        let mut out = vec![];
        gather(&self.payload, &mut (MAX_BODY_BYTES as u64), &mut out);
        out
    }
    pub(crate) fn hydrate_body(&mut self, part: &str, bytes: &[u8]) -> Result<(), GmailError> {
        fn find<'a>(p: &'a mut Part, id: &str) -> Option<&'a mut Part> {
            if p.part_id == id {
                Some(p)
            } else {
                p.parts.iter_mut().find_map(|p| find(p, id))
            }
        }
        let p = find(&mut self.payload, part).ok_or(GmailError::InvalidResponse)?;
        if is_attachment(p) || !matches!(p.mime_type.as_str(), "text/plain" | "text/html") {
            return Err(GmailError::InvalidResponse);
        }
        let body: PartBody = super::decode(bytes)?;
        if body.size != p.body.size
            || body
                .attachment_id
                .as_ref()
                .is_some_and(|v| Some(v) != p.body.attachment_id.as_ref())
        {
            return Err(GmailError::InvalidResponse);
        }
        decode_body(&body, MAX_BODY_BYTES)?;
        p.body.data = body.data;
        p.body.attachment_id = None;
        Ok(())
    }
    /// Sync and interactive reads share this body-only projection. Binary parts
    /// are never downloaded or represented as successfully extracted source text.
    pub(crate) fn project(&self) -> Result<MailMessage, GmailError> {
        let mut raw = Vec::new();
        for h in &self.payload.headers {
            if ["subject", "from", "to", "cc", "bcc", "message-id"]
                .contains(&h.name.to_ascii_lowercase().as_str())
            {
                raw.extend_from_slice(
                    format!("{}: {}\r\n", h.name, h.value.replace(['\r', '\n'], " ")).as_bytes(),
                );
            }
        }
        raw.extend_from_slice(b"Content-Type: text/plain; charset=utf-8\r\n\r\n");
        let parsed = mail_parser::MessageParser::default()
            .parse(raw.as_slice())
            .ok_or(GmailError::InvalidResponse)?;
        for address in [parsed.from(), parsed.to(), parsed.cc(), parsed.bcc()]
            .into_iter()
            .flatten()
        {
            if address.iter().count() > 100
                || address
                    .iter()
                    .any(|v| v.address.as_deref().is_some_and(|v| v.len() > 320))
            {
                return Err(GmailError::LimitExceeded);
            }
        }
        let mut mail = parse_mail(&raw)?;
        let mut text = String::new();
        let mut truncated = false;
        let mut attachments = vec![];
        fn body(p: &Part, text: &mut String, truncated: &mut bool) -> Result<(), GmailError> {
            if is_attachment(p) {
                return Ok(());
            }
            if p.mime_type.starts_with("multipart/") {
                // Prefer one representation within multipart/alternative.
                if p.mime_type == "multipart/alternative" {
                    if let Some(part) = p
                        .parts
                        .iter()
                        .find(|v| v.mime_type == "text/plain")
                        .or_else(|| p.parts.first())
                    {
                        body(part, text, truncated)?;
                    }
                } else {
                    for part in &p.parts {
                        body(part, text, truncated)?;
                    }
                }
            } else if matches!(p.mime_type.as_str(), "text/plain" | "text/html") {
                if p.body.attachment_id.is_some() || p.body.size > MAX_BODY_BYTES as u64 {
                    *truncated = true;
                    return Ok(());
                }
                let bytes = decode_body(&p.body, MAX_BODY_BYTES)?;
                let content_type = p
                    .headers
                    .iter()
                    .find(|h| h.name.eq_ignore_ascii_case("content-type"))
                    .map(|h| h.value.replace(['\r', '\n'], " "))
                    .unwrap_or_else(|| p.mime_type.clone());
                let raw = format!(
                    "Content-Type: {content_type}\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
                    STANDARD.encode(&bytes)
                );
                let parsed = parse_mail(raw.as_bytes())?;
                *truncated |= parsed.text_truncated;
                let remaining = MAX_BODY_BYTES.saturating_sub(text.len());
                if !text.is_empty() && remaining > 0 {
                    text.push('\n');
                }
                let mut end = parsed
                    .text
                    .len()
                    .min(MAX_BODY_BYTES.saturating_sub(text.len()));
                while !parsed.text.is_char_boundary(end) {
                    end -= 1;
                }
                *truncated |= end < parsed.text.len();
                text.push_str(&parsed.text[..end]);
            }
            Ok(())
        }
        fn collect(p: &Part, attachments: &mut Vec<AttachmentMetadata>) {
            if is_attachment(p) {
                attachments.push(metadata(p));
            } else {
                for p in &p.parts {
                    collect(p, attachments);
                }
            }
        }
        collect(&self.payload, &mut attachments);
        body(&self.payload, &mut text, &mut truncated)?;
        mail.text = text;
        mail.text_truncated = truncated;
        mail.attachment_names = attachments.iter().map(|v| v.filename.clone()).collect();
        Ok(MailMessage {
            id: self.id.clone(),
            thread_id: self.thread_id.clone(),
            labels: self.label_ids.clone(),
            mail,
            attachments,
        })
    }
}
fn decode_body(body: &PartBody, limit: usize) -> Result<Zeroizing<Vec<u8>>, GmailError> {
    if body.size > limit as u64 || body.data.len() > limit * 4 / 3 + 8 {
        return Err(GmailError::LimitExceeded);
    }
    let bytes = Zeroizing::new(
        URL_SAFE_NO_PAD
            .decode(body.data.trim_end_matches('='))
            .map_err(|_| GmailError::InvalidResponse)?,
    );
    if bytes.len() != body.size as usize {
        return Err(GmailError::InvalidResponse);
    }
    Ok(bytes)
}
impl GmailClient {
    pub(super) async fn full_message(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<FullMessage, GmailError> {
        validate_id(id)?;
        let bytes = self
            .get(
                &format!("messages/{id}?format=full&fields=id,threadId,labelIds,payload"),
                token,
                cancel,
            )
            .await?;
        FullMessage::decode(&bytes, id)
    }
    pub(super) async fn read_full_message(
        &self,
        token: &SecretString,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MailMessage, GmailError> {
        let mut full = self.full_message(token, id, cancel).await?;
        for (part, attachment) in full.external_bodies() {
            let bytes = self
                .get(
                    &format!("messages/{id}/attachments/{attachment}"),
                    token,
                    cancel,
                )
                .await?;
            full.hydrate_body(&part, &bytes)?;
        }
        full.project()
    }
    pub async fn get_attachment(
        &self,
        token: &SecretString,
        message: &str,
        part: &str,
        attachment: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<AttachmentRead, GmailError> {
        let full = self.full_message(token, message, cancel).await?;
        let p = full.attachment(part, attachment)?;
        if !readable(&p.mime_type, p.body.size) {
            return Ok(AttachmentRead {
                message_id: full.id.clone(),
                thread_id: full.thread_id.clone(),
                metadata: metadata(p),
                bytes: None,
            });
        }
        let bytes = if let Some(id) = &p.body.attachment_id {
            let bytes = self
                .transport
                .request(
                    Endpoint::GmailAttachment,
                    reqwest::Method::GET,
                    &format!("messages/{message}/attachments/{id}"),
                    Some(token),
                    Body::Empty,
                    cancel,
                )
                .await?;
            let body: PartBody = super::decode(&bytes)?;
            if body.size != p.body.size || body.attachment_id.as_deref().is_some_and(|v| v != id) {
                return Err(GmailError::InvalidResponse);
            }
            decode_body(&body, MAX_ATTACHMENT_BYTES)?
        } else {
            decode_body(&p.body, MAX_ATTACHMENT_BYTES)?
        };
        Ok(AttachmentRead {
            message_id: full.id.clone(),
            thread_id: full.thread_id.clone(),
            metadata: metadata(p),
            bytes: Some(bytes),
        })
    }
}

/// Stable SHA-256 content identity, never a grant or provider receipt.
pub fn attachment_digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
