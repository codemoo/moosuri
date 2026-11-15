//! Dedicated, labeled Docker profiles. No host path or browser profile is accepted.
use super::*;
use std::collections::HashSet;
const PROFILE: &str = "io.moosuri.browser.workspace";
const ACCOUNT: &str = "io.moosuri.browser.account";
const KIND: &str = "io.moosuri.browser.kind";
const VALUE: &str = "bounded-profile-v1";
impl CapsuleReader {
    pub(super) fn profile_name(&self, workspace: Uuid) -> String {
        format!("moosuri-profile-{}-{workspace}", self.config.owner)
    }
    async fn profile_inspect(&self, workspace: Uuid, account: Uuid) -> Result<(), BrowserError> {
        let name = self.profile_name(workspace);
        let bytes = docker_output(
            &["volume", "inspect", &name],
            &CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await?;
        let rows: Value = serde_json::from_slice(&bytes).map_err(|_| BrowserError::Setup)?;
        let row = rows
            .as_array()
            .filter(|r| r.len() == 1)
            .and_then(|r| r.first())
            .ok_or(BrowserError::Setup)?;
        if row["Name"] != name
            || row["Driver"] != "local"
            || !labels(&row["Labels"], self, workspace, Some(account))
        {
            return Err(BrowserError::IsolationRequired);
        }
        // Options could redirect a local volume to arbitrary host storage.
        if !row["Options"].is_null() && row["Options"].as_object().is_none_or(|o| !o.is_empty()) {
            return Err(BrowserError::IsolationRequired);
        }
        Ok(())
    }
    pub async fn prepare_profile(
        &self,
        workspace: Uuid,
        account: Uuid,
        create: bool,
    ) -> Result<(), BrowserError> {
        #[cfg(test)]
        if self.fixture.is_some() {
            return Ok(());
        }
        if !self.config.recover_workspaces {
            return Err(BrowserError::IsolationRequired);
        }
        self.config.validate()?;
        self.validate_resources(&CancellationToken::new()).await?;
        self.stop_profile_workers(workspace).await?;
        if create {
            // Never adopt an existing volume for a fresh workspace, even with matching labels.
            let found = self.list_profiles().await?;
            if found.len() >= 64 {
                return Err(BrowserError::Capacity);
            }
            if found
                .iter()
                .any(|name| name == &self.profile_name(workspace))
            {
                return Err(BrowserError::IsolationRequired);
            }
            docker_output(
                &[
                    "volume",
                    "create",
                    "--driver=local",
                    "--label",
                    &format!("{OWNER}={}", self.config.owner),
                    "--label",
                    &format!("{PROJECT}={}", self.config.project),
                    "--label",
                    &format!("{PROFILE}={workspace}"),
                    "--label",
                    &format!("{ACCOUNT}={account}"),
                    "--label",
                    &format!("{KIND}={VALUE}"),
                    &self.profile_name(workspace),
                ],
                &CancellationToken::new(),
                Duration::from_secs(2),
            )
            .await?;
        }
        self.profile_inspect(workspace, account).await
    }
    async fn list_profiles(&self) -> Result<Vec<String>, BrowserError> {
        let bytes = docker_output(
            &[
                "volume",
                "ls",
                "--format",
                "{{.Name}}",
                "--filter",
                &format!("label={OWNER}={}", self.config.owner),
                "--filter",
                &format!("label={PROJECT}={}", self.config.project),
                "--filter",
                &format!("label={KIND}={VALUE}"),
            ],
            &CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await?;
        let text = String::from_utf8(bytes).map_err(|_| BrowserError::Setup)?;
        let mut rows = text.lines().map(str::to_owned).collect::<Vec<_>>();
        rows.sort();
        Ok(rows)
    }
    async fn stop_profile_workers(&self, workspace: Uuid) -> Result<(), BrowserError> {
        let bytes = docker_output(
            &[
                "container",
                "ls",
                "--all",
                "--no-trunc",
                "--format",
                "{{.ID}}",
                "--filter",
                &format!("label={OWNER}={}", self.config.owner),
                "--filter",
                &format!("label={PROJECT}={}", self.config.project),
                "--filter",
                &format!("label={PROFILE}={workspace}"),
                "--filter",
                &format!("label={KIND}={VALUE}"),
            ],
            &CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await?;
        let text = String::from_utf8(bytes).map_err(|_| BrowserError::Setup)?;
        if text.lines().count() > 16 {
            return Err(BrowserError::Capacity);
        }
        for id in text.lines() {
            if !hex64(id) {
                return Err(BrowserError::IsolationRequired);
            }
            let bytes = docker_output(
                &["container", "inspect", id],
                &CancellationToken::new(),
                Duration::from_secs(2),
            )
            .await?;
            let rows: Value = serde_json::from_slice(&bytes).map_err(|_| BrowserError::Setup)?;
            let row = &rows[0];
            if row["Id"] != id || !labels(&row["Config"]["Labels"], self, workspace, None) {
                return Err(BrowserError::IsolationRequired);
            }
            docker_output(
                &["container", "rm", "--force", id],
                &CancellationToken::new(),
                Duration::from_secs(2),
            )
            .await?;
        }
        Ok(())
    }
    pub async fn delete_profile(&self, workspace: Uuid, account: Uuid) -> Result<(), BrowserError> {
        #[cfg(test)]
        if self.fixture.is_some() {
            return Ok(());
        }
        let name = self.profile_name(workspace);
        let found = docker_output(
            &[
                "volume",
                "ls",
                "--format",
                "{{.Name}}",
                "--filter",
                &format!("name={name}"),
            ],
            &CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await?;
        if !String::from_utf8(found)
            .map_err(|_| BrowserError::Setup)?
            .lines()
            .any(|v| v == name)
        {
            return Ok(());
        }
        self.profile_inspect(workspace, account).await?;
        self.stop_profile_workers(workspace).await?;
        docker_output(
            &["volume", "rm", &name],
            &CancellationToken::new(),
            Duration::from_secs(2),
        )
        .await?;
        Ok(())
    }
    /// Keep leases are derived from current DB authority. Invalid/deleted leases stay
    /// discoverable by ownership labels even when Docker is temporarily unavailable.
    pub async fn reconcile_profiles(
        &self,
        keep: &HashSet<(Uuid, Uuid)>,
        active: &HashSet<Uuid>,
    ) -> Result<(), BrowserError> {
        #[cfg(test)]
        if self.fixture.is_some() {
            return Ok(());
        }
        let names = self.list_profiles().await?;
        if names.is_empty() {
            return Ok(());
        }
        let mut failure = None;
        for name in batch(&names, &self.profile_cursor) {
            let result = async {
                let prefix = format!("moosuri-profile-{}-", self.config.owner);
                let id = name
                    .strip_prefix(&prefix)
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .ok_or(BrowserError::IsolationRequired)?;
                let bytes = docker_output(
                    &["volume", "inspect", name],
                    &CancellationToken::new(),
                    Duration::from_secs(2),
                )
                .await?;
                let rows: Value =
                    serde_json::from_slice(&bytes).map_err(|_| BrowserError::Setup)?;
                let account = rows[0]["Labels"][ACCOUNT]
                    .as_str()
                    .and_then(|v| Uuid::parse_str(v).ok())
                    .ok_or(BrowserError::IsolationRequired)?;
                self.profile_inspect(id, account).await?;
                if !keep.contains(&(id, account)) {
                    self.delete_profile(id, account).await?;
                } else if !active.contains(&id) {
                    self.stop_profile_workers(id).await?;
                }
                Ok::<(), BrowserError>(())
            }
            .await;
            if let Err(error) = result {
                failure = Some(error)
            }
        }
        if let Some(error) = failure {
            return Err(error);
        }
        Ok(())
    }
}
fn labels(value: &Value, reader: &CapsuleReader, workspace: Uuid, account: Option<Uuid>) -> bool {
    value[OWNER] == reader.config.owner.to_string()
        && value[PROJECT] == reader.config.project
        && value[PROFILE] == workspace.to_string()
        && value[KIND] == VALUE
        && account.is_none_or(|a| value[ACCOUNT] == a.to_string())
}

// Rotate bounded inspection batches so overflow and failing candidates cannot
// permanently starve independent expired resources.
fn batch<'a>(names: &'a [String], cursor: &std::sync::atomic::AtomicUsize) -> Vec<&'a str> {
    if names.is_empty() {
        return Vec::new();
    }
    let start = cursor.fetch_add(8, std::sync::atomic::Ordering::Relaxed) % names.len();
    (0..names.len().min(8))
        .map(|n| names[(start + n) % names.len()].as_str())
        .collect()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn overflow_inventory_rotates_past_malformed_and_failing_candidates() {
        let cursor = std::sync::atomic::AtomicUsize::new(0);
        let names = (0..65)
            .map(|n| format!("candidate-{n}"))
            .collect::<Vec<_>>();
        let mut visited = HashSet::new();
        for _ in 0..9 {
            let current = batch(&names, &cursor);
            assert!(current.len() <= 8);
            for name in current {
                visited.insert(name);
            }
        }
        assert_eq!(visited.len(), 65);
        // An empty pass remains bounded and never divides by zero.
        assert!(batch(&[], &cursor).is_empty());
    }
}
