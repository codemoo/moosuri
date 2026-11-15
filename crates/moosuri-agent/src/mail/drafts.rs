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
        let (connection, _frozen) = self
            .store
            .mail_provider_snapshot(user, action.clone(), input.expected.clone())
            .await?;
        let b = self.compose_binding(user, &connection, cancel).await?;
        let lock = self.lock(&connection).await;
        let _guard = lock.lock().await;
        self.compose_current(user, &b).await?;
        if let Some(saved) = self
            .store
            .recorded_provider_draft_save(user, action.clone(), input.clone())
            .await?
        {
            if let (Some(id), moosuri_store::ProviderDraftSave::Recorded { mime_sha256, .. }) =
                (&input.draft_id, &saved)
            {
                let observed = self
                    .client
                    .get_draft(&b.access_token, id, cancel)
                    .await
                    .map_err(map)?;
                self.compose_current(user, &b).await?;
                if observed.digest != *mime_sha256 {
                    return Err(StoreError::Conflict);
                }
            }
            let receipt = self
                .provider_draft_readback(user, &b, saved, cancel)
                .await?;
            self.store
                .mail_provider_snapshot(user, action, input.expected)
                .await?;
            return Ok(receipt);
        }
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
            .mail_provider_snapshot(user, action.clone(), input.expected.clone())
            .await?;
        let (permit, draft) = match self
            .store
            .begin_provider_draft_save(
                user,
                action.clone(),
                input.clone(),
                b.app_pin().cloned().ok_or(StoreError::Conflict)?,
                b.epoch,
                b.revision,
            )
            .await?
        {
            moosuri_store::ProviderDraftSave::Recorded { receipt, .. } => return Ok(receipt),
            moosuri_store::ProviderDraftSave::Dispatch { permit, draft } => (permit, draft),
            saved @ moosuri_store::ProviderDraftSave::Reconcile { .. } => {
                return self.provider_draft_readback(user, &b, saved, cancel).await;
            }
        };
        let result = self
            .client
            .save_draft(&b.access_token, input.draft_id.as_deref(), &draft, cancel)
            .await;
        let receipt = result.as_ref().ok().map(|d| GmailDraftRef {
            id: d.id.clone(),
            message_id: d.message.id.clone(),
        });
        self.store
            .finish_provider_draft_save(permit, receipt.clone())
            .await?;
        result.map_err(map)?;
        self.compose_current(user, &b).await?;
        self.store
            .mail_provider_snapshot(user, action, input.expected)
            .await?;
        receipt.ok_or(StoreError::Unavailable)
    }
    /// At most one bounded page, no POST and no cross-account fallback. A partial
    /// list or different provider bytes cannot prove the original write completed.
    async fn provider_draft_readback(
        &self,
        user: &UserContext,
        binding: &MailBinding,
        saved: moosuri_store::ProviderDraftSave,
        cancel: &CancellationToken,
    ) -> Result<GmailDraftRef, StoreError> {
        let (permit, digest) = match saved {
            moosuri_store::ProviderDraftSave::Recorded { receipt, .. } => return Ok(receipt),
            moosuri_store::ProviderDraftSave::Reconcile {
                permit,
                mime_sha256,
            } => (permit, mime_sha256),
            moosuri_store::ProviderDraftSave::Dispatch { .. } => return Err(StoreError::Conflict),
        };
        let page = self
            .client
            .list_drafts(&binding.access_token, None, cancel)
            .await
            .map_err(map)?;
        self.compose_current(user, binding).await?;
        if page.next_page_token.is_some() {
            return Err(StoreError::Conflict);
        }
        let mut matches = Vec::new();
        for candidate in page.drafts {
            if permit
                .target_draft()
                .is_some_and(|target| target != candidate.id)
            {
                continue;
            }
            let observation = self
                .client
                .get_draft(&binding.access_token, &candidate.id, cancel)
                .await
                .map_err(map)?;
            self.compose_current(user, binding).await?;
            if observation.digest == digest {
                matches.push(GmailDraftRef {
                    id: candidate.id,
                    message_id: candidate.message.id,
                });
            }
        }
        if matches.len() != 1 {
            return Err(StoreError::Conflict);
        }
        let receipt = matches.remove(0);
        self.store
            .finish_provider_draft_save(permit, Some(receipt.clone()))
            .await?;
        Ok(receipt)
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

impl MailService {
    /// Original mission authority, existing provider journal, no session invention.
    pub(crate) async fn save_mission_provider_draft(
        &self,
        p: &moosuri_store::MissionActionPermit,
        cancel: &CancellationToken,
    ) -> Result<moosuri_store::KnowledgeToolResult, StoreError> {
        let app = self.oauth_client().await?.ok_or(StoreError::Unavailable)?;
        let mut b = self
            .store
            .mission_provider_credentials(p, app.pin.clone())
            .await?;
        let lock = self.lock(&b.connection_id).await;
        let _guard = lock.lock().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if b.expires_at <= now + 60 {
            let tokens = self
                .client
                .refresh(&app.client, &b.refresh, cancel)
                .await
                .map_err(map)?;
            self.store.refresh_mission_provider(p, &b, tokens).await?;
            b = self.store.mission_provider_credentials(p, app.pin).await?;
        }
        let saved = match self.store.mission_provider_save(p, &b, false).await? {
            Some(saved) => saved,
            None => self
                .store
                .mission_provider_save(p, &b, true)
                .await?
                .ok_or(StoreError::Conflict)?,
        };
        let receipt = match saved {
            moosuri_store::ProviderDraftSave::Recorded { receipt, .. } => receipt,
            moosuri_store::ProviderDraftSave::Dispatch { permit, draft } => {
                let result = self
                    .client
                    .save_draft(&b.access, None, &draft, cancel)
                    .await;
                let receipt = result.as_ref().ok().map(|v| GmailDraftRef {
                    id: v.id.clone(),
                    message_id: v.message.id.clone(),
                });
                self.store
                    .finish_provider_draft_save(permit, receipt.clone())
                    .await?;
                result.map_err(map)?;
                receipt.ok_or(StoreError::Unavailable)?
            }
            moosuri_store::ProviderDraftSave::Reconcile {
                permit,
                mime_sha256,
            } => {
                let page = self
                    .client
                    .list_drafts(&b.access, None, cancel)
                    .await
                    .map_err(map)?;
                self.store.mission_provider_save(p, &b, false).await?;
                if page.next_page_token.is_some() {
                    return Err(StoreError::Conflict);
                }
                let mut matches = vec![];
                for candidate in page.drafts {
                    let observed = self
                        .client
                        .get_draft(&b.access, &candidate.id, cancel)
                        .await
                        .map_err(map)?;
                    self.store.mission_provider_save(p, &b, false).await?;
                    if observed.digest == mime_sha256 {
                        matches.push(GmailDraftRef {
                            id: candidate.id,
                            message_id: candidate.message.id,
                        });
                    }
                }
                if matches.len() != 1 {
                    return Err(StoreError::Conflict);
                }
                let receipt = matches.remove(0);
                self.store
                    .finish_provider_draft_save(permit, Some(receipt.clone()))
                    .await?;
                receipt
            }
        };
        self.store.mission_provider_save(p, &b, false).await?;
        Ok(moosuri_store::KnowledgeToolResult::External(
            serde_json::json!({"provider_draft":receipt,"state":"saved","sent":false,"requires_user_approval":true}),
        ))
    }
}
