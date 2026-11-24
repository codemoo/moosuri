//! Standard Gmail journal and worker inside one member's relay. A rotated app
//! cancels the old worker and the final send gate rechecks the current revision.
use super::{
    mail_work::{self, MailWriter},
    mail_write::{self, ApprovalCheck},
    registered_client,
};
use moosuri_agent::mail::MailService;
use moosuri_connectors::{gmail::GmailClient, model::CancellationToken};
use moosuri_core::{GmailDraftRef, GmailDraftSave};
use moosuri_store::{Store, StoreError, UserContext, personal_volume::VolumeOwner};
use std::{net::SocketAddr, path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use uuid::Uuid;

struct Approval {
    store: Store,
    owner: VolumeOwner,
    config: PathBuf,
    revision: Uuid,
}
#[async_trait::async_trait]
impl ApprovalCheck for Approval {
    async fn approved(&self, action: &str, connection: &str, epoch: i64, digest: [u8; 32]) -> bool {
        if !registered_client(&self.config).is_ok_and(|(id, _, _)| id == self.revision) {
            return false;
        }
        self.store
            .member_mail_dispatch_mailbox(
                self.owner,
                self.revision,
                action.into(),
                connection.into(),
                epoch,
                digest,
            )
            .await
            .is_ok_and(|mailbox| mailbox.is_some())
    }
}
struct Active {
    revision: Uuid,
    service: MailService,
    stop: CancellationToken,
}
impl Drop for Active {
    fn drop(&mut self) {
        self.stop.cancel();
    }
}
struct Relay {
    store: Store,
    owner: VolumeOwner,
    config: PathBuf,
    proxy: SocketAddr,
    active: Mutex<Option<Active>>,
}
impl Relay {
    async fn service(&self) -> Result<MailService, StoreError> {
        let mut active = self.active.lock().await;
        let (revision, oauth, _) = match registered_client(&self.config) {
            Ok(app) => app,
            Err(_) => {
                *active = None;
                return Err(StoreError::Unavailable);
            }
        };
        if let Some(old) = active.as_ref().filter(|old| old.revision == revision) {
            return Ok(old.service.clone());
        }
        *active = None;
        let client =
            GmailClient::google_gmail_proxy(self.proxy).map_err(|_| StoreError::Unavailable)?;
        let check = Arc::new(Approval {
            store: self.store.clone(),
            owner: self.owner,
            config: self.config.clone(),
            revision,
        });
        let fence = Arc::new(mail_write::MemberDispatchFence::new(
            self.store.clone(),
            self.owner,
            &oauth,
            Some(check),
        ));
        let service = MailService::for_member_relay(self.store.clone(), Arc::new(oauth), client);
        let stop = CancellationToken::new();
        mail_write::spawn_worker(service.clone(), fence, stop.clone());
        *active = Some(Active {
            revision,
            service: service.clone(),
            stop,
        });
        Ok(service)
    }
}
#[async_trait::async_trait]
impl MailWriter for Relay {
    async fn ready(&self) -> bool {
        self.service().await.is_ok()
    }
    async fn save_draft(
        &self,
        user: &UserContext,
        action: String,
        request: GmailDraftSave,
    ) -> Result<GmailDraftRef, StoreError> {
        self.service()
            .await?
            .save_gmail_draft(user, action, request, &CancellationToken::new())
            .await
    }
}

pub(super) fn routes(
    store: Store,
    owner: VolumeOwner,
    config: PathBuf,
    proxy: SocketAddr,
) -> axum::Router {
    let relay = Arc::new(Relay {
        store: store.clone(),
        owner,
        config,
        proxy,
        active: Default::default(),
    });
    let startup = relay.clone();
    tokio::spawn(async move {
        let _ = startup.service().await;
    });
    mail_work::routes(store, owner, Some(relay), None)
}
