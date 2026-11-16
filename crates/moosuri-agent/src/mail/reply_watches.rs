//! Bounded metadata-first follow-up. No provider/model run is admitted here.
use super::*;
use moosuri_core::MailReplyPreview;
use moosuri_store::{MailReplyCredentials, MailReplyLease};
struct Failure(&'static str, bool);
fn read_failure(error: GmailError) -> Failure {
    match error {
        GmailError::Http(404) => Failure("thread_unavailable", true),
        GmailError::InvalidResponse | GmailError::LimitExceeded => {
            Failure("coverage_unavailable", true)
        }
        GmailError::Http(401 | 403) => Failure("connection_unavailable", true),
        _ => Failure("check_failed", false),
    }
}
fn bounded(value: &str, bytes: usize) -> String {
    let mut end = value.len().min(bytes);
    while !value.is_char_boundary(end) {
        end -= 1
    }
    value[..end].to_owned()
}
impl MailService {
    pub(crate) async fn reply_watch_worker(self, stop: CancellationToken) {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! { _=stop.cancelled()=>break, _=tick.tick()=>{} }
            if let Ok(Some(lease)) = self.store.next_mail_reply_watch().await {
                self.poll_reply_watch(&lease, &stop).await;
            }
        }
    }
    async fn reply_credentials(
        &self,
        lease: &MailReplyLease,
        cancel: &CancellationToken,
    ) -> Result<(MailReplyCredentials, GoogleAppPin), Failure> {
        let app = self
            .oauth_client()
            .await
            .map_err(|_| Failure("connection_unavailable", false))?
            .ok_or(Failure("connection_unavailable", false))?;
        let lock = self.lock(&lease.connection_id).await;
        let _guard = lock.lock().await;
        let mut b = self
            .store
            .mail_reply_credentials(lease, app.pin.clone())
            .await
            .map_err(|_| Failure("connection_unavailable", false))?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Failure("check_failed", false))?
            .as_secs() as i64;
        if b.expires_at <= now + 60 {
            let tokens = self
                .client
                .refresh(&app.client, &b.refresh, cancel)
                .await
                .map_err(read_failure)?;
            self.store
                .refresh_mail_reply_credentials(lease, app.pin.clone(), b.revision, tokens)
                .await
                .map_err(|_| Failure("connection_unavailable", false))?;
            b = self
                .store
                .mail_reply_credentials(lease, app.pin.clone())
                .await
                .map_err(|_| Failure("connection_unavailable", false))?;
        }
        Ok((b, app.pin))
    }
    async fn read_reply_watch(
        &self,
        lease: &MailReplyLease,
        cancel: &CancellationToken,
    ) -> Result<(Vec<String>, Vec<MailReplyPreview>), Failure> {
        let (b, pin) = self.reply_credentials(lease, cancel).await?;
        let index = self
            .client
            .thread_index(&b.access, &lease.provider_thread, cancel)
            .await
            .map_err(read_failure)?;
        let incoming = index
            .new_inbound(&lease.baseline_ids, &lease.seen_ids)
            .map_err(|_| Failure("anchor_unavailable", true))?;
        if incoming.len() > 4 {
            return Err(Failure("too_many_replies", true));
        }
        let mut replies = Vec::new();
        for id in incoming {
            // Do not perform a body fetch after stop/disconnect/app change.
            self.store
                .mail_reply_credentials(lease, pin.clone())
                .await
                .map_err(|_| Failure("connection_unavailable", false))?;
            let message = self
                .client
                .get_message(&b.access, &id, cancel)
                .await
                .map_err(read_failure)?;
            if message.id != id || message.thread_id != index.id {
                return Err(Failure("coverage_unavailable", true));
            }
            if message.labels.iter().any(|v| v == "SENT" || v == "DRAFT")
                || message
                    .mail
                    .from
                    .iter()
                    .any(|v| v.eq_ignore_ascii_case(&b.mailbox))
            {
                continue;
            }
            let text = bounded(&message.mail.text, 4096);
            replies.push(MailReplyPreview {
                message_id: id,
                from: message
                    .mail
                    .from
                    .iter()
                    .take(4)
                    .map(|v| bounded(v, 256))
                    .collect(),
                subject: bounded(&message.mail.subject, 512),
                partial: message.mail.text_truncated
                    || text.len() < message.mail.text.len()
                    || !message.attachments.is_empty(),
                text,
            });
        }
        if serde_json::to_vec(&replies).map_or(true, |v| v.len() > 24 * 1024) {
            return Err(Failure("coverage_unavailable", true));
        }
        self.store
            .mail_reply_credentials(lease, pin)
            .await
            .map_err(|_| Failure("connection_unavailable", false))?;
        Ok((index.messages.into_iter().map(|m| m.id).collect(), replies))
    }
    pub(crate) async fn poll_reply_watch(
        &self,
        lease: &MailReplyLease,
        cancel: &CancellationToken,
    ) {
        let result = tokio::time::timeout(
            Duration::from_secs(45),
            self.read_reply_watch(lease, cancel),
        )
        .await
        .unwrap_or(Err(Failure("check_timed_out", false)));
        let _delivery = self.signals.delivery.write().await;
        match result {
            Ok((seen, replies)) => {
                let _ = crate::store_retry::local(|| {
                    self.store
                        .complete_mail_reply_watch(lease, seen.clone(), replies.clone())
                })
                .await;
            }
            Err(Failure(reason, permanent)) => {
                let _ = crate::store_retry::local(|| {
                    self.store.fail_mail_reply_watch(lease, reason, permanent)
                })
                .await;
            }
        }
        self.signals.changed();
    }
}
