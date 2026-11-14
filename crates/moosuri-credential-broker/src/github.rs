//! Exactly one trusted API operation. Provider text/headers/errors never escape.
use crate::*;
use moosuri_vault_core::{ItemKind, UnlockedArchive};
use std::time::Duration;
pub struct GitHubAdapter {
    client: reqwest::Client,
    origin: String,
}
impl GitHubAdapter {
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn synthetic_loopback(address: std::net::SocketAddr) -> Result<Self> {
        if !address.ip().is_loopback() {
            return Err(Error::Policy);
        }
        Self::at(&format!("http://{address}"))
    }
    pub fn new() -> Result<Self> {
        Self::at("https://api.github.com")
    }
    fn at(origin: &str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| Error::PrivateState)?,
            origin: origin.into(),
        })
    }
    async fn read(&self, repository: &GitHubRepository, token: &str) -> Result<Projection> {
        if !repository.valid()
            || token.is_empty()
            || token.len() > 16384
            || !token.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(Error::Policy);
        }
        let mut response = self
            .client
            .get(format!(
                "{}/repos/{}/{}",
                self.origin, repository.owner, repository.repository
            ))
            .bearer_auth(token)
            .header("accept", "application/vnd.github+json")
            .header("x-github-api-version", "2022-11-28")
            .header("user-agent", "moosuri-native-credential/1")
            .send()
            .await
            .map_err(|_| Error::Unknown)?;
        if matches!(response.status().as_u16(), 401 | 403) {
            return Err(Error::Policy);
        }
        if !response.status().is_success() {
            return Err(Error::Unknown);
        }
        let mut bytes = zeroize::Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unknown)? {
            if bytes.len() + chunk.len() > 65536 {
                return Err(Error::Unknown);
            }
            bytes.extend_from_slice(&chunk);
        }
        let value: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| Error::Unknown)?;
        let id = value["id"]
            .as_u64()
            .filter(|v| *v > 0)
            .ok_or(Error::Unknown)?;
        // Reconstruct allowlisted numeric/boolean fields; no provider string is
        // copied, even a token echo in a repository/error/header/name field.
        Ok(Projection::GitHubRepository {
            id,
            private: value["private"].as_bool().ok_or(Error::Unknown)?,
            archived: value["archived"].as_bool().ok_or(Error::Unknown)?,
            stars: value["stargazers_count"].as_u64().ok_or(Error::Unknown)?,
            open_issues: value["open_issues_count"].as_u64().ok_or(Error::Unknown)?,
        })
    }
}
impl Engine {
    pub fn archive_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        repo: GitHubRepository,
        expires: i64,
    ) -> Result<Binding> {
        if archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let encrypted = archive
            .items()
            .iter()
            .find(|i| i.id == item)
            .ok_or(Error::Policy)?;
        let epoch = encrypted.revision;
        let key = archive.header().generation;
        let fields = archive.open(item).map_err(|_| Error::Policy)?;
        if fields.kind != ItemKind::ApiKey
            || fields.url.expose().trim_end_matches('/') != "https://api.github.com"
            || fields.secret.expose().is_empty()
            || !fields.totp.expose().is_empty()
            || !fields.expires_at.expose().is_empty()
        {
            return Err(Error::Policy);
        }
        if let Some(g) = self.grants().iter().find(|g| {
            g.repository() == Some(&repo)
                && g.binding.expires_at
                    > std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|v| v.as_secs() as i64)
                        .unwrap_or(i64::MAX)
        }) {
            if g.live().is_some()
                || g.vault() != Some(archive.header().binding.vault_id)
                || g.item() != item
                || g.binding.item_epoch != epoch
                || g.binding.key_epoch != key
            {
                return Err(Error::Policy);
            }
            return Ok(g.binding.clone());
        }
        self.select(policy::LocalGrant::select_github(
            self.device().clone(),
            item,
            archive.header().binding.vault_id,
            repo,
            epoch,
            key,
            expires,
        )?)
    }
    pub fn prepare_github(
        &mut self,
        archive: &UnlockedArchive,
        request: Request,
        now: i64,
    ) -> Result<()> {
        if request.operation != Operation::GitHubRepositoryRead
            || archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?;
        if grant.live().is_some() || grant.vault() != Some(archive.header().binding.vault_id) {
            return Err(Error::Policy);
        }
        let item = archive
            .items()
            .iter()
            .find(|i| i.id == grant.item())
            .ok_or(Error::Policy)?;
        self.prepare(request, now, item.revision, archive.header().generation)
    }
    pub async fn execute_github(
        &mut self,
        archive: &mut UnlockedArchive,
        invocation: &Invocation,
        adapter: &GitHubAdapter,
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
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?;
        if grant.vault() != Some(archive.header().binding.vault_id) {
            return Err(Error::Policy);
        }
        let id = grant.item();
        let epoch = archive
            .items()
            .iter()
            .find(|i| i.id == id)
            .ok_or(Error::Policy)?
            .revision;
        if archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let fields = archive.open(id).map_err(|_| Error::Policy)?;
        if fields.kind != ItemKind::ApiKey
            || fields.url.expose().trim_end_matches('/') != "https://api.github.com"
        {
            return Err(Error::Policy);
        }
        if grant.live().is_some() {
            return Err(Error::Policy);
        }
        self.execute_github_secret(
            invocation,
            adapter,
            fields.secret.expose(),
            epoch,
            archive.header().generation,
        )
        .await
    }
    pub(crate) async fn execute_github_secret(
        &mut self,
        invocation: &Invocation,
        adapter: &GitHubAdapter,
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
        let repository = request
            .repository
            .as_ref()
            .filter(|r| r.valid())
            .ok_or(Error::Policy)?;
        let commitment = self.begin_dispatch(invocation, current_time()?, item_epoch, key_epoch)?;
        let (projection, outcome) = match adapter.read(repository, secret).await {
            Ok(p) => (p, Outcome::ProjectedSuccess),
            Err(Error::Policy) => (Projection::None, Outcome::Rejected),
            Err(_) => (Projection::None, Outcome::Unknown),
        };
        let receipt = Receipt {
            commitment,
            outcome,
            projection: projection.clone(),
            projection_digest: auth::projection_digest(projection),
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
