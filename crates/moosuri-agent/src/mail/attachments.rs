//! Bounded read-only attachment text; no file paths or executable selection in requests.
use super::*;
use moosuri_connectors::gmail::{AttachmentRead, SecretString};
use moosuri_documents::{MAX_PDF_BYTES, MAX_TEXT_BYTES, PdfError};
use serde_json::{Value, json};
static PDF_READS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
use moosuri_connectors::gmail::attachment_digest as digest;
impl MailService {
    pub(crate) fn with_pdf_extractor(mut self, path: Option<std::path::PathBuf>) -> Self {
        self.pdf_extractor = path.filter(|p| p.is_absolute());
        self
    }
    pub(super) async fn attachment(
        &self,
        token: &SecretString,
        request: &MailToolRequest,
        cancel: &CancellationToken,
    ) -> Result<Value, StoreError> {
        let MailToolRequest::Attachment {
            message_id,
            part_id,
            attachment_id,
            start_byte,
            max_bytes,
            expected_digest,
            ..
        } = request
        else {
            return Err(StoreError::InvalidInput);
        };
        if !(1..=4096).contains(max_bytes)
            || (*start_byte > 0 && expected_digest.is_none())
            || expected_digest
                .as_ref()
                .is_some_and(|v| v.len() != 64 || !v.bytes().all(|c| c.is_ascii_hexdigit()))
        {
            return Err(StoreError::InvalidInput);
        }
        let AttachmentRead {
            bytes,
            message_id: actual_message,
            thread_id,
            metadata,
        } = self
            .client
            .get_attachment(token, message_id, part_id, attachment_id.as_deref(), cancel)
            .await
            .map_err(map)?;
        let base = json!({"message_id":actual_message,"thread_id":thread_id,"attachment":metadata,"untrusted":true});
        let Some(bytes) = bytes else {
            let mut result = base;
            result["status"] = if metadata.size > MAX_PDF_BYTES as u64 {
                "too_large"
            } else {
                "unsupported_type"
            }
            .into();
            return Ok(result);
        };
        let content_digest = digest(&bytes);
        let extracted = if metadata.mime_type == "application/pdf" {
            if let Some(path) = &self.pdf_extractor {
                let _permit = PDF_READS.try_acquire().map_err(|_| StoreError::Busy)?;
                tokio::select! {biased;_=cancel.cancelled()=>return Err(StoreError::Unavailable),result=moosuri_documents::extract_pdf(path,&bytes)=>result.map(|v|(v.text,Some(v.pages)))}
            } else {
                Err(PdfError::Unavailable)
            }
        } else {
            std::str::from_utf8(&bytes)
                .map_err(|_| PdfError::Unsupported)
                .and_then(|text| {
                    if text.len() > MAX_TEXT_BYTES {
                        Err(PdfError::TooLarge)
                    } else if text
                        .chars()
                        .any(|c| c.is_control() && !matches!(c, '\n' | '\r' | '\t'))
                    {
                        Err(PdfError::Unsupported)
                    } else {
                        Ok((text.to_owned(), None))
                    }
                })
        };
        let mut result = base;
        result["content_digest"] = content_digest.clone().into();
        let (text, pages) = match extracted {
            Ok(v) => v,
            Err(e) => {
                result["status"] = match e {
                    PdfError::Invalid => "invalid_document",
                    PdfError::Encrypted => "encrypted",
                    PdfError::NoText => "no_text",
                    PdfError::Unsupported => "unsupported_content",
                    PdfError::TooLarge => "extraction_limit",
                    PdfError::Unavailable => "extractor_unavailable",
                    PdfError::TimedOut => "extraction_timeout",
                }
                .into();
                return Ok(result);
            }
        };
        let text_digest = digest(text.as_bytes());
        let page_digest = digest(
            format!(
                "{message_id}\n{part_id}\n{}\n{content_digest}\n{text_digest}",
                attachment_id.as_deref().unwrap_or("")
            )
            .as_bytes(),
        );
        if expected_digest.as_ref().is_some_and(|v| v != &page_digest) {
            result["status"] = "changed_restart_read".into();
            return Ok(result);
        }
        let start = *start_byte as usize;
        if start > text.len() || !text.is_char_boundary(start) {
            return Err(StoreError::InvalidInput);
        }
        let mut end = start.saturating_add(*max_bytes as usize).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start && start < text.len() {
            return Err(StoreError::InvalidInput);
        }
        result["status"] = "ok".into();
        result["text_digest"] = text_digest.into();
        result["page_digest"] = page_digest.into();
        result["pages"] = json!(pages);
        result["start_byte"] = json!(start);
        result["total_text_bytes"] = json!(text.len());
        loop {
            result["text"] = text[start..end].into();
            result["next_start_byte"] = if end < text.len() {
                json!(end)
            } else {
                Value::Null
            };
            result["partial"] = json!(start > 0 || end < text.len());
            // The durable tool envelope includes JSON escaping and metadata.
            let encoded = serde_json::to_vec(&KnowledgeToolResult::External(result.clone()))
                .map_err(|_| StoreError::Unavailable)?;
            if encoded.len() <= 12 * 1024 {
                break;
            }
            if end == start {
                return Err(StoreError::InvalidInput);
            }
            end -= 1;
            while !text.is_char_boundary(end) {
                end -= 1;
            }
        }
        Ok(result)
    }
}
