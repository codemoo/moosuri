//! Interactive provider-draft writes are separate from local send authority.
use super::*;
use moosuri_core::{GmailDraft, GmailDraftPage, GmailDraftRef, GmailDraftSave, MailDraftContent};
impl MailService {
    async fn compose_binding(
        &self,
        user: &UserContext,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<MailBinding, StoreError> {
        let b = self.binding(user, id, cancel).await?;
        if !b.has_compose() {
            return Err(StoreError::Forbidden);
        }
        Ok(b)
    }
    async fn compose_current(&self, user: &UserContext, b: &MailBinding) -> Result<(), StoreError> {
        let latest = self.store.mail_binding(user, b.id.clone()).await?;
        if latest.epoch != b.epoch || !latest.has_compose() {
            return Err(StoreError::Conflict);
        }
        if let Some(pin) = b.app_pin() {
            self.store.pin_mail_binding(latest, pin.clone()).await?;
        }
        Ok(())
    }
    pub async fn gmail_drafts(
        &self,
        user: &UserContext,
        id: &str,
        page: Option<&str>,
        cancel: &CancellationToken,
    ) -> Result<GmailDraftPage, StoreError> {
        let b = self.compose_binding(user, id, cancel).await?;
        let result = self
            .client
            .list_drafts(&b.access_token, page, cancel)
            .await
            .map_err(map)?;
        self.compose_current(user, &b).await?;
        Ok(GmailDraftPage {
            drafts: result
                .drafts
                .into_iter()
                .map(|d| GmailDraftRef {
                    id: d.id,
                    message_id: d.message.id,
                })
                .collect(),
            next_page_token: result.next_page_token,
        })
    }
    pub async fn gmail_draft(
        &self,
        user: &UserContext,
        connection: &str,
        id: &str,
        cancel: &CancellationToken,
    ) -> Result<GmailDraft, StoreError> {
        let b = self.compose_binding(user, connection, cancel).await?;
        let d = self
            .client
            .get_draft(&b.access_token, id, cancel)
            .await
            .map_err(map)?;
        self.compose_current(user, &b).await?;
        let content = if d.editable && d.mail.from == [b.email.clone()] {
            let c = MailDraftContent {
                from: b.email,
                to: d.mail.to,
                cc: d.mail.cc,
                bcc: d.mail.bcc,
                subject: d.mail.subject.clone(),
                text: d.mail.text.clone(),
                reply: None,
            };
            // Reuse the exact supported outbound validator, not the lossy read projection.
            let m = moosuri_connectors::gmail::MailContent {
                from: c.from.clone(),
                to: c.to.clone(),
                cc: c.cc.clone(),
                bcc: c.bcc.clone(),
                subject: c.subject.clone(),
                text: c.text.clone(),
                reply: None,
            };
            moosuri_connectors::gmail::FrozenMail::prepare(&m, "validate@moosuri.local")
                .ok()
                .map(|_| c)
        } else {
            None
        };
        Ok(GmailDraft {
            id: d.id,
            digest: d.digest,
            subject: d.mail.subject,
            text: d.mail.text,
            text_truncated: d.mail.text_truncated,
            attachment_names: d.mail.attachment_names,
            content,
        })
    }
    pub async fn save_gmail_draft(
        &self,
        user: &UserContext,
        action: String,
        input: GmailDraftSave,
        cancel: &CancellationToken,
    ) -> Result<GmailDraftRef, StoreError> {
        let (connection, frozen) = self
            .store
            .mail_provider_snapshot(user, action.clone(), input.expected.clone())
            .await?;
        let b = self.compose_binding(user, &connection, cancel).await?;
        let lock = self.lock(&connection).await;
        let _guard = lock.lock().await;
        self.compose_current(user, &b).await?;
        // A second local CAS after refresh/lock rejects a now-withdrawn snapshot.
        self.store
            .mail_provider_snapshot(user, action.clone(), input.expected.clone())
            .await?;
        if let Some(id) = &input.draft_id {
            let d = self
                .client
                .get_draft(&b.access_token, id, cancel)
                .await
                .map_err(map)?;
            if !d.editable || input.expected_provider_digest.as_deref() != Some(&d.digest) {
                return Err(StoreError::Conflict);
            }
            self.compose_current(user, &b).await?;
        } else if input.expected_provider_digest.is_some() {
            return Err(StoreError::InvalidInput);
        }
        // Gmail drafts have no conditional revision write. No automatic retry:
        // a lost write response means refresh Gmail drafts before another write.
        // The provider GET above may have waited while the user edited,
        // rejected or withdrew this local action. Recheck the exact frozen
        // snapshot after every provider await, immediately before external write.
        self.store
            .mail_provider_snapshot(user, action, input.expected)
            .await?;
        let d = self
            .client
            .save_draft(&b.access_token, input.draft_id.as_deref(), &frozen, cancel)
            .await
            .map_err(map)?;
        self.compose_current(user, &b).await?;
        Ok(GmailDraftRef {
            id: d.id,
            message_id: d.message.id,
        })
    }
    pub async fn delete_gmail_draft(
        &self,
        user: &UserContext,
        connection: &str,
        id: &str,
        digest: &str,
        cancel: &CancellationToken,
    ) -> Result<(), StoreError> {
        let b = self.compose_binding(user, connection, cancel).await?;
        let lock = self.lock(connection).await;
        let _guard = lock.lock().await;
        let d = self
            .client
            .get_draft(&b.access_token, id, cancel)
            .await
            .map_err(map)?;
        if d.digest != digest {
            return Err(StoreError::Conflict);
        }
        self.compose_current(user, &b).await?;
        self.client
            .delete_draft(&b.access_token, id, cancel)
            .await
            .map_err(map)?;
        self.compose_current(user, &b).await
    }
}
#[cfg(test)]
mod tests;
