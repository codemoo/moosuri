use super::*;
use moosuri_store::MailReconcilePermit;
impl MailService {
    pub async fn check_unknown(
        &self,
        user: &UserContext,
        id: String,
        cancel: &CancellationToken,
    ) -> Result<moosuri_core::MailAction, StoreError> {
        let p = self
            .store
            .claim_mail_reconciliation(user, id.clone())
            .await?;
        self.reconcile(p, cancel).await;
        self.store.mail_action(user, id).await
    }
    pub(super) async fn reconcile(&self, mut p: MailReconcilePermit, cancel: &CancellationToken) {
        let lock = self.lock(p.connection_id()).await;
        let _guard = lock.lock().await;
        let result = self.lookup_unknown(&mut p, cancel).await;
        // Only the local save may retry. A future lookup is a separately leased
        // read attempt; neither path can construct a send carrier.
        let result = result.ok();
        for _ in 0..5 {
            let observation = result.clone();
            match self.store.finish_mail_reconciliation(&p, observation).await {
                Err(StoreError::Busy) => {
                    // Never replay network IO while retrying a local receipt.
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                _ => break,
            }
        }
        self.signals.changed();
    }
    async fn lookup_unknown(
        &self,
        p: &mut MailReconcilePermit,
        cancel: &CancellationToken,
    ) -> Result<moosuri_connectors::gmail::SentObservation, StoreError> {
        let app = self.oauth_client().await?.ok_or(StoreError::Unavailable)?;
        *p = self
            .store
            .pin_mail_reconciliation(p.clone(), app.pin.clone())
            .await?;
        let mut b = self
            .store
            .mail_reconcile_credentials(p, app.pin.clone())
            .await?;
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs() as i64;
        if b.expires_at <= now + 60 {
            let tokens = self
                .client
                .refresh(&app.client, &b.refresh_token, cancel)
                .await
                .map_err(map)?;
            self.store
                .save_mail_reconcile_refresh(p, &b, tokens)
                .await?;
            b = self
                .store
                .mail_reconcile_credentials(p, app.pin.clone())
                .await?;
        }
        // Fresh lease/account/app/connection/scope checks immediately before IO.
        self.store.mail_reconcile_credentials(p, app.pin).await?;
        let search = self
            .client
            .search_sent(&b.access_token, p.message_id(), cancel)
            .await
            .map_err(map)?;
        match search {
            moosuri_connectors::gmail::SentSearch::Observed(result) => Ok(result),
            moosuri_connectors::gmail::SentSearch::Candidate(candidate) => {
                // Revocation during list forbids a later raw-content fetch.
                let app = self.oauth_client().await?.ok_or(StoreError::Unavailable)?;
                let b = self.store.mail_reconcile_credentials(p, app.pin).await?;
                self.client
                    .inspect_sent(
                        &b.access_token,
                        candidate,
                        p.fingerprint(),
                        p.reply_thread(),
                        cancel,
                    )
                    .await
                    .map_err(map)
            }
        }
    }
}

#[cfg(test)]
mod tests;
