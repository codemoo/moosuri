//! External validation is outside the SQLite writer; completion rechecks every fence.
use super::*;
use moosuri_connectors::chatgpt_oauth::handoff::{MAX_ENVELOPE, envelope_digest};
use moosuri_store::{HandoffAdmission, HandoffStatus};
impl SubscriptionService {
    pub async fn import_handoff(
        &self,
        user: &UserContext,
        id: String,
        envelope: Vec<u8>,
    ) -> Result<HandoffStatus, StoreError> {
        if !(40..=MAX_ENVELOPE).contains(&envelope.len()) {
            return Err(StoreError::InvalidInput);
        }
        let lease = match self
            .store
            .admit_chatgpt_handoff(user, id, envelope_digest(&envelope))
            .await?
        {
            HandoffAdmission::Imported(receipt) => return Ok(*receipt),
            HandoffAdmission::Validate(l) => l,
        };
        let operation = async {
            let _permit = self
                .logins
                .clone()
                .try_acquire_owned()
                .map_err(|_| StoreError::Busy)?;
            let client = self
                .auth_client()
                .await
                .map_err(|_| StoreError::Unavailable)?;
            client
                .verify_handoff(&envelope, &lease.key, &lease.expected)
                .await
                .map_err(|_| StoreError::InvalidInput)
        };
        let result = tokio::select! {
            _=self.signals.stopping().cancelled()=>Err(StoreError::Unavailable),
            r=operation=>r,
        };
        match result {
            Ok(verified) => {
                let receipt = self
                    .store
                    .complete_chatgpt_handoff(*lease, verified)
                    .await?;
                self.signals.changed();
                Ok(receipt)
            }
            Err(error) => {
                let _ = self.store.fail_chatgpt_handoff(*lease).await;
                Err(error)
            }
        }
    }
}
