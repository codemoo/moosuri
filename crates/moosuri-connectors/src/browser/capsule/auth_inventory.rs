//! Read-only confirmation for an interrupted private authentication attempt.
use super::*;

impl CapsuleReader {
    /// The caller holds the installation's shared authentication startup lock.
    /// A live or stopped member/Google capsule is retained and means "not yet";
    /// nothing is adopted, removed or inferred from a missing in-memory handle.
    pub async fn authentication_inventory_empty(
        &self,
        cancel: &CancellationToken,
    ) -> Result<bool, BrowserError> {
        if self.config.engine != Engine::Kiri || self.config.recover_workspaces {
            return Err(BrowserError::IsolationRequired);
        }
        self.validate_resources(cancel).await?;
        empty_docker_config(Path::new(DOCKER_CONFIG))?;
        let prefix = format!("moosuri-google-{}-", self.config.owner.simple());
        let names = self
            .docker_output(
                &[
                    "container",
                    "ls",
                    "--all",
                    "--format",
                    "{{.Names}}",
                    "--filter",
                    &format!("name=^/{prefix}[01]$"),
                ],
                cancel,
                Duration::from_secs(3),
            )
            .await?;
        Ok(names.iter().all(u8::is_ascii_whitespace))
    }
}
