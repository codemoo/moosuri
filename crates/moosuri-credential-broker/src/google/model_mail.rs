//! Device-local model-mail adapter. This module never returns provider IDs/tokens.
//! Selections are local journal state scoped to the exact grant and its expiry.
use super::*;
use moosuri_connectors::{
    gmail::{GmailError, MailMessage, SecretString},
    model::CancellationToken,
};
use uuid::Uuid;
pub(crate) const MODEL_POLICY: &[u8] = b"moosuri/native-model-gmail/v1\0explicit-llm-content-consent;readonly;search5;thread5;opaque-selectors;selected-pdf-utf8;local-parser;no-display-reuse";
pub fn resource_digest() -> [u8; 32] {
    auth::hash(&Resource::google_model_read(Uuid::nil()).bytes())
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
enum Selected {
    Message {
        id: String,
        thread: String,
    },
    Thread {
        id: String,
    },
    Cursor {
        query: String,
        token: String,
    },
    Attachment {
        message: String,
        thread: String,
        part: String,
        attachment: Option<String>,
        metadata: moosuri_connectors::gmail::AttachmentMetadata,
        content_digest: Option<[u8; 32]>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    id: Uuid,
    binding: Binding,
    expires_at: i64,
    selected: Selected,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct Selections(Vec<Selection>);
impl Selections {
    fn add(
        &mut self,
        grant: &policy::LocalGrant,
        expires_at: i64,
        selected: Selected,
    ) -> Result<Uuid> {
        if self.0.len() >= 512 {
            return Err(Error::PrivateState);
        }
        let id = Uuid::new_v4();
        self.0.push(Selection {
            id,
            binding: grant.binding.clone(),
            expires_at: expires_at.min(grant.binding.expires_at),
            selected,
        });
        Ok(id)
    }
    fn get(&self, grant: &policy::LocalGrant, id: Uuid, now: i64) -> Result<&Selected> {
        self.0
            .iter()
            .find(|s| s.id == id && s.binding == grant.binding && s.expires_at > now)
            .map(|s| &s.selected)
            .ok_or(Error::Request)
    }
    fn validate(
        &self,
        grant: &policy::LocalGrant,
        request: &MailModelRequest,
        now: i64,
    ) -> Result<()> {
        let valid = match &request.read {
            MailModelRead::Search { cursor: None, .. } => true,
            MailModelRead::Search {
                query,
                cursor: Some(cursor),
            } => matches!(self.get(grant,*cursor,now)?,Selected::Cursor{query:old,..}if old==query),
            MailModelRead::Message { selection } => {
                matches!(self.get(grant, *selection, now)?, Selected::Message { .. })
            }
            MailModelRead::Thread { selection } => matches!(
                self.get(grant, *selection, now)?,
                Selected::Message { .. } | Selected::Thread { .. }
            ),
            MailModelRead::Attachment { selection, page } => {
                matches!(self.get(grant,*selection,now)?,Selected::Attachment{content_digest,..}if *page==0||content_digest.is_some())
            }
        };
        if valid { Ok(()) } else { Err(Error::Request) }
    }
    fn find_message(
        &mut self,
        grant: &policy::LocalGrant,
        expires: i64,
        id: &str,
        thread: &str,
    ) -> Result<Uuid> {
        if let Some(s) = self.0.iter().find(|s| s.binding == grant.binding && s.expires_at >= expires && matches!(&s.selected, Selected::Message { id: m, thread: t } if m == id && t == thread)) { return Ok(s.id); }
        self.add(
            grant,
            expires,
            Selected::Message {
                id: id.into(),
                thread: thread.into(),
            },
        )
    }
    fn find_thread(&mut self, grant: &policy::LocalGrant, expires: i64, id: &str) -> Result<Uuid> {
        if let Some(s) = self.0.iter().find(|s| {
            s.binding == grant.binding
                && s.expires_at >= expires
                && matches!(&s.selected, Selected::Thread { id: t } if t == id)
        }) {
            return Ok(s.id);
        }
        self.add(grant, expires, Selected::Thread { id: id.into() })
    }
}
fn content_error(e: GmailError) -> Error {
    match e {
        GmailError::Http(401) => Error::Policy,
        GmailError::Http(404) => Error::Unavailable,
        GmailError::Http(403) => Error::ProviderContent,
        GmailError::Transport | GmailError::Timeout | GmailError::Cancelled => Error::Unknown,
        _ => Error::ProviderContent,
    }
}
fn bounded(s: &str, max: usize, secret: &impl Fn(&str) -> bool) -> Result<(String, bool)> {
    if secret(s)
        || s.chars()
            .any(|c| c.is_control() && !matches!(c, '\r' | '\n' | '\t'))
    {
        return Err(Error::ProviderContent);
    }
    let mut end = s.len().min(max);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    Ok((s[..end].into(), end != s.len()))
}
impl GoogleAdapter {
    async fn model_message(
        &self,
        mail: MailMessage,
        grant: &policy::LocalGrant,
        expires: i64,
        selections: &mut Selections,
        secret: &impl Fn(&str) -> bool,
        body_limit: usize,
    ) -> Result<MailModelMessage> {
        // Check original fields before truncating. These are the *currently used*
        // credentials, not a claim that arbitrary mail secrets are redacted.
        if [
            &mail.id,
            &mail.thread_id,
            &mail.mail.subject,
            &mail.mail.text,
        ]
        .into_iter()
        .chain(mail.mail.from.iter())
        .chain(mail.mail.to.iter())
        .chain(mail.mail.cc.iter())
        .chain(mail.mail.bcc.iter())
        .any(|s| secret(s))
        {
            return Err(Error::ProviderContent);
        }
        let (subject, a) = bounded(&mail.mail.subject, 256, secret)?;
        let (from, b) = bounded(&mail.mail.from.join(", "), 256, secret)?;
        let (text, c) = bounded(&mail.mail.text, body_limit, secret)?;
        let selection = selections.find_message(grant, expires, &mail.id, &mail.thread_id)?;
        let thread = selections.find_thread(grant, expires, &mail.thread_id)?;
        let mut attachments = Vec::new();
        for meta in mail.attachments.iter().take(8) {
            if [
                &meta.part_id,
                meta.attachment_id.as_deref().unwrap_or(""),
                &meta.filename,
                &meta.mime_type,
            ]
            .into_iter()
            .any(secret)
            {
                return Err(Error::ProviderContent);
            }
            let selected = selections.add(
                grant,
                expires,
                Selected::Attachment {
                    message: mail.id.clone(),
                    thread: mail.thread_id.clone(),
                    part: meta.part_id.clone(),
                    attachment: meta.attachment_id.clone(),
                    metadata: meta.clone(),
                    content_digest: None,
                },
            )?;
            attachments.push(MailModelAttachment {
                selection: selected,
                filename: bounded(&meta.filename, 128, secret)?.0,
                media_type: if matches!(meta.mime_type.as_str(), "application/pdf" | "text/plain") {
                    meta.mime_type.clone()
                } else {
                    "unsupported".into()
                },
                readable: meta.readable,
            });
        }
        let content_digest =
            auth::hash(&serde_json::to_vec(&mail.mail).map_err(|_| Error::ProviderContent)?);
        Ok(MailModelMessage {
            selection,
            thread,
            observation: Uuid::new_v4(),
            subject,
            from,
            text,
            partial: a || b || c || mail.mail.text_truncated || mail.attachments.len() > 8,
            content_digest,
            attachments,
        })
    }
    async fn model_read(
        &self,
        grant: &policy::LocalGrant,
        request: &MailModelRequest,
        secret: &str,
        expires: i64,
        selections: &mut Selections,
        pdf: Option<&std::path::Path>,
    ) -> Result<MailModelResult> {
        if !grant.google_model_read()
            || !request.valid()
            || (request.background && !grant.google_model_background())
        {
            return Err(Error::Policy);
        }
        if current_time()? >= expires {
            return Err(Error::Request);
        }
        selections.validate(grant, request, current_time()?)?;
        let (credential, token) = self
            .authorize_model(grant.google().ok_or(Error::Policy)?, secret)
            .await?;
        let contains_secret = |s: &str| {
            [
                Some(secret),
                Some(credential.client_id.expose()),
                credential.client_secret.as_ref().map(|s| s.expose()),
                Some(credential.refresh_token.expose()),
                Some(token.access_token.expose()),
            ]
            .into_iter()
            .flatten()
            .any(|value| !value.is_empty() && s.contains(value))
        };
        if current_time()? >= expires {
            return Err(Error::Request);
        }
        let token =
            SecretString::new(token.access_token.expose().to_owned()).map_err(|_| Error::Policy)?;
        let cancel = CancellationToken::new();
        let mut result = MailModelResult::status(MailModelStatus::Ready);
        match &request.read {
            MailModelRead::Search { query, cursor } => {
                if contains_secret(query) {
                    return Err(Error::ProviderContent);
                }
                let page = if let Some(cursor) = cursor {
                    match selections.get(grant, *cursor, current_time()?)? {
                        Selected::Cursor { query: q, token } if q == query => Some(token.clone()),
                        _ => return Err(Error::Request),
                    }
                } else {
                    None
                };
                let page = self
                    .mail
                    .list_messages(&token, query, page.as_deref(), 5, &cancel)
                    .await
                    .map_err(content_error)?;
                for item in page.messages {
                    if contains_secret(&item.id) || contains_secret(&item.thread_id) {
                        return Err(Error::ProviderContent);
                    }
                    let message = self
                        .mail
                        .get_message(&token, &item.id, &cancel)
                        .await
                        .map_err(content_error)?;
                    if message.id != item.id || message.thread_id != item.thread_id {
                        return Err(Error::ProviderContent);
                    }
                    result.messages.push(
                        self.model_message(
                            message,
                            grant,
                            expires,
                            selections,
                            &contains_secret,
                            512,
                        )
                        .await?,
                    );
                }
                if let Some(next) = page.next_page_token {
                    if contains_secret(&next) {
                        return Err(Error::ProviderContent);
                    }
                    result.next_cursor = Some(selections.add(
                        grant,
                        expires,
                        Selected::Cursor {
                            query: query.clone(),
                            token: next,
                        },
                    )?);
                }
            }
            MailModelRead::Message { selection } => {
                let Selected::Message { id, thread } =
                    selections.get(grant, *selection, current_time()?)?.clone()
                else {
                    return Err(Error::Request);
                };
                let message = self
                    .mail
                    .get_message(&token, &id, &cancel)
                    .await
                    .map_err(content_error)?;
                if message.thread_id != thread {
                    return Err(Error::ProviderContent);
                }
                result.messages.push(
                    self.model_message(message, grant, expires, selections, &contains_secret, 4096)
                        .await?,
                );
            }
            MailModelRead::Thread { selection } => {
                let id = match selections.get(grant, *selection, current_time()?)? {
                    Selected::Thread { id } => id.clone(),
                    Selected::Message { thread, .. } => thread.clone(),
                    _ => return Err(Error::Request),
                };
                let thread = self
                    .mail
                    .get_thread(&token, &id, &cancel)
                    .await
                    .map_err(content_error)?;
                result.partial = thread.truncated || thread.messages.len() > 5;
                let skip = thread.messages.len().saturating_sub(5);
                for message in thread.messages.into_iter().skip(skip) {
                    result.messages.push(
                        self.model_message(
                            message,
                            grant,
                            expires,
                            selections,
                            &contains_secret,
                            512,
                        )
                        .await?,
                    );
                }
            }
            MailModelRead::Attachment { selection, page } => {
                let Selected::Attachment {
                    message,
                    thread,
                    part,
                    attachment,
                    metadata,
                    content_digest,
                } = selections.get(grant, *selection, current_time()?)?.clone()
                else {
                    return Err(Error::Request);
                };
                if !metadata.readable {
                    return Ok(MailModelResult::status(MailModelStatus::UnsupportedFormat));
                }
                if metadata.mime_type == "application/pdf" && pdf.is_none() {
                    return Ok(MailModelResult::status(MailModelStatus::ParserUnavailable));
                }
                if *page > 0 && content_digest.is_none() {
                    return Err(Error::Request);
                }
                let read = self
                    .mail
                    .get_attachment(&token, &message, &part, attachment.as_deref(), &cancel)
                    .await
                    .map_err(content_error)?;
                if read.message_id != message
                    || read.thread_id != thread
                    || read.metadata != metadata
                {
                    return Err(Error::ProviderContent);
                }
                let Some(bytes) = read.bytes else {
                    return Ok(MailModelResult::status(MailModelStatus::UnsupportedFormat));
                };
                let digest = auth::hash(&bytes);
                if content_digest.is_some_and(|prior| prior != digest) {
                    return Ok(MailModelResult::status(MailModelStatus::SourceChanged));
                }
                let text = if metadata.mime_type == "application/pdf" {
                    match moosuri_documents::extract_pdf(pdf.ok_or(Error::Policy)?, &bytes).await {
                        Ok(text) => text.text,
                        Err(moosuri_documents::PdfError::Unavailable) => {
                            return Ok(MailModelResult::status(MailModelStatus::ParserUnavailable));
                        }
                        Err(_) => {
                            return Ok(MailModelResult::status(MailModelStatus::UnsupportedFormat));
                        }
                    }
                } else {
                    if bytes.len() > 24 * 1024 {
                        return Ok(MailModelResult::status(MailModelStatus::UnsupportedFormat));
                    }
                    std::str::from_utf8(&bytes)
                        .map_err(|_| Error::ProviderContent)?
                        .to_owned()
                };
                bounded(&text, text.len(), &contains_secret)?;
                let chunks = text_pages(&text);
                let Some(text) = chunks.get(*page as usize) else {
                    return Err(Error::Request);
                };
                let selected = selections
                    .0
                    .iter_mut()
                    .find(|s| s.id == *selection)
                    .ok_or(Error::Policy)?;
                if let Selected::Attachment { content_digest, .. } = &mut selected.selected {
                    *content_digest = Some(digest);
                }
                let parent = selections.find_message(grant, expires, &message, &thread)?;
                let thread = selections.find_thread(grant, expires, &thread)?;
                result.attachment = Some(MailModelAttachmentPage {
                    selection: *selection,
                    message: parent,
                    thread,
                    observation: Uuid::new_v4(),
                    page: *page,
                    total_pages: chunks.len() as u16,
                    text: (*text).into(),
                    content_digest: digest,
                    partial: chunks.len() > 1,
                });
            }
        }
        result.partial |= result.messages.iter().any(|m| m.partial);
        // JSON escaping and attachment metadata count toward the signed result cap.
        // Trim bounded returned text/metadata, retaining explicit partial markers.
        while result.bytes().len() > 12000 {
            let Some(message) = result
                .messages
                .iter_mut()
                .max_by_key(|m| m.text.len() + m.attachments.len() * 512)
            else {
                return Err(Error::ProviderContent);
            };
            if !message.attachments.is_empty() {
                message.attachments.pop();
            } else if !message.text.is_empty() {
                let mut end = message.text.len() / 2;
                while !message.text.is_char_boundary(end) {
                    end -= 1;
                }
                message.text.truncate(end);
            } else {
                return Err(Error::ProviderContent);
            }
            message.partial = true;
            result.partial = true;
        }
        if !result.valid() {
            return Err(Error::ProviderContent);
        }
        Ok(result)
    }
}
fn text_pages(text: &str) -> Vec<&str> {
    if text.is_empty() {
        return vec![""];
    }
    let mut rest = text;
    let mut pages = Vec::new();
    while !rest.is_empty() {
        let mut end = rest.len().min(4096);
        while !rest.is_char_boundary(end) {
            end -= 1;
        }
        pages.push(&rest[..end]);
        rest = &rest[end..];
    }
    pages
}
impl Engine {
    pub(crate) fn attach_model_mail_payload(
        &mut self,
        invocation: &Invocation,
        payload: MailModelRequest,
    ) -> Result<()> {
        let r = &self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request;
        let g = self
            .grants()
            .iter()
            .find(|g| g.binding == r.binding)
            .ok_or(Error::Policy)?;
        if !payload.valid()
            || !matches!(r.operation, Operation::GoogleMailModelRead { .. })
            || auth::hash(&payload.bytes()) != r.digest
            || !g.google_model_read()
            || (payload.background && !g.google_model_background())
        {
            return Err(Error::Policy);
        }
        self.model_mail_payloads.retain(|(i, _)| i != invocation);
        self.model_mail_payloads.push((invocation.clone(), payload));
        Ok(())
    }
    pub(crate) async fn execute_model_mail_secret(
        &mut self,
        invocation: &Invocation,
        adapter: &GoogleAdapter,
        secret: &str,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<SignedReceipt> {
        let request = self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request
            .clone();
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding == request.binding)
            .ok_or(Error::Policy)?
            .clone();
        let payload = self
            .model_mail_payloads
            .iter()
            .find(|(i, _)| i == invocation)
            .map(|(_, p)| p.clone())
            .ok_or(Error::Policy)?;
        self.attach_model_mail_payload(invocation, payload.clone())?;
        grant.check(&request, current_time()?, item_epoch, key_epoch)?;
        let commitment = self.begin_dispatch(invocation, current_time()?, item_epoch, key_epoch)?;
        let mut selections = self.state.model_mail_selections.clone();
        selections
            .0
            .retain(|s| s.expires_at > current_time().unwrap_or(i64::MAX));
        let result = tokio::time::timeout(
            Duration::from_secs(
                (request.expires_at.saturating_sub(current_time()?) as u64).min(60),
            ),
            adapter.model_read(
                &grant,
                &payload,
                secret,
                request.expires_at,
                &mut selections,
                self.pdf_extractor.as_deref(),
            ),
        )
        .await
        .unwrap_or(Err(Error::Unknown));
        let (projection, outcome) = match result {
            Ok(result) => {
                self.state.model_mail_selections = selections;
                (
                    Projection::GoogleMailModelRead {
                        result: Box::new(result),
                    },
                    Outcome::ProjectedSuccess,
                )
            }
            Err(Error::GoogleScope) => (
                Projection::GoogleMailModelRead {
                    result: Box::new(MailModelResult::status(MailModelStatus::ScopeMissing)),
                },
                Outcome::Rejected,
            ),
            Err(Error::GoogleAccount) => (
                Projection::GoogleMailModelRead {
                    result: Box::new(MailModelResult::status(MailModelStatus::AccountMismatch)),
                },
                Outcome::Rejected,
            ),
            Err(Error::Unavailable) => (
                Projection::GoogleMailModelRead {
                    result: Box::new(MailModelResult::status(MailModelStatus::Missing)),
                },
                Outcome::ProjectedSuccess,
            ),
            Err(Error::Request) => (Projection::None, Outcome::NotDispatched),
            Err(Error::Policy) => (Projection::None, Outcome::Rejected),
            Err(_) => (Projection::None, Outcome::Unknown),
        };
        if outcome == Outcome::Rejected {
            self.state
                .grants
                .iter_mut()
                .find(|g| g.binding == grant.binding)
                .ok_or(Error::Policy)?
                .suspend_google();
        }
        let receipt = Receipt {
            commitment,
            outcome,
            projection_digest: auth::projection_digest(projection.clone()),
            projection,
        };
        let signed = SignedReceipt {
            signature: self.key.sign(&receipt_bytes(&receipt)),
            receipt,
        };
        self.state
            .entries
            .iter_mut()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .phase = Phase::Settled(signed.clone());
        self.save()?;
        Ok(signed)
    }
}
#[cfg(test)]
mod tests;
