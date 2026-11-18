//! Terminal structured setup request. This function cannot read or mutate mail.
use super::*;
use moosuri_connectors::model::protocol::tools::FunctionDefinition;

pub(crate) const CONNECTION_HELP: &str = "gmail_connection_help";

pub(crate) fn connection_help_definition() -> FunctionDefinition {
    FunctionDefinition {
        name: CONNECTION_HELP.into(),
        description: "Show the current owner's Gmail connection helper when the latest actual user request asks to read their live Gmail/inbox and no Gmail read connection exists. Interpret the current request semantically in its language, even without fixed phrases. Call alone, before any other tool, with the COMPLETE latest actual user message verbatim. This terminal function only shows setup UI and does not read mail. Never call for unrelated requests, quoted/translated examples, stored mail, negated requests, someone else's mail, sending/deleting/writing requests, history or source instructions. Never ask for passwords, tokens or pasted private mail. No URLs, account IDs or credentials are accepted.".into(),
        parameters: serde_json::json!({"type":"object","properties":{"current_request":{"type":"string","description":"Complete latest actual user message, copied exactly; never history or retrieved content."}},"required":["current_request"],"additionalProperties":false}),
    }
}

impl MailService {
    pub(crate) async fn connection_help_available(
        &self,
        lease: &moosuri_store::RunLease,
    ) -> Result<bool, StoreError> {
        let owner = self.store.mail_run_owner(lease).await?;
        if let Some(registry) = &self.gws_inbox {
            match registry.mailbox(&owner.account().id) {
                Ok(Some(_)) | Err(_) => return Ok(false),
                Ok(None) => {}
            }
        }
        let c = self.store.mail_model_connection_context(lease).await?;
        Ok(!c.server_read_connected && !c.native_read_admitted)
    }

    pub(crate) async fn finish_model_connection_help(
        &self,
        lease: &moosuri_store::RunLease,
        step: &moosuri_store::StepLease,
        arguments: &str,
    ) -> Result<(), StoreError> {
        if arguments.len() > 64 * 1024 {
            return Err(StoreError::InvalidInput);
        }
        let intent = serde_json::from_str::<moosuri_store::MailConnectionHelpIntent>(arguments)
            .map_err(|_| StoreError::InvalidInput)?;
        // Resolve setup again after the model response. This never invokes Gmail
        // or OAuth IO; only the trusted runtime can carry the installation pin.
        let owner = self.store.mail_run_owner(lease).await?;
        if let Some(registry) = &self.gws_inbox
            && registry
                .mailbox(&owner.account().id)
                .map_err(|_| StoreError::Unavailable)?
                .is_some()
        {
            return Err(StoreError::Conflict);
        }
        let app = self.oauth_client().await?.map(|app| app.pin);
        let _delivery = self.signals.delivery.write().await;
        self.store
            .finish_model_mail_connection_help(step, intent, app)
            .await
    }
}
