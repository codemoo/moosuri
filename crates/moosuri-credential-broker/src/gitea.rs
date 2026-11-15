//! Pinned Gitea 1.24.6 browser adapter. Only local vault fields enter Chromium.
use crate::*;
use moosuri_vault_core::{ItemFields, ItemKind, UnlockedArchive};
use uuid::Uuid;

pub fn valid_origin(origin: &str) -> bool {
    let Ok(u) = url::Url::parse(origin) else {
        return false;
    };
    u.origin().ascii_serialization() == origin
        && u.path() == "/"
        && u.query().is_none()
        && u.fragment().is_none()
        && u.username().is_empty()
        && u.password().is_none()
        && (u.scheme() == "https"
            || (u.scheme() == "http"
                && matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"))))
}
pub(crate) fn fields(fields: &ItemFields, origin: &str) -> Result<()> {
    if fields.kind != ItemKind::Login
        || fields.url.expose().trim_end_matches('/') != origin
        || fields.username.expose().is_empty()
        || fields.username.expose().len() > 256
        || fields.secret.expose().is_empty()
        || fields.secret.expose().len() > 4096
        || fields.totp.expose().is_empty()
        || !fields.expires_at.expose().is_empty()
    {
        return Err(Error::Policy);
    }
    totp(fields)?;
    Ok(())
}
pub(crate) fn totp(fields: &ItemFields) -> Result<moosuri_vault_core::Totp> {
    let seed = moosuri_vault_core::SecretText::new(fields.totp.expose().to_owned())
        .map_err(|_| Error::Policy)?;
    if seed.expose().starts_with("otpauth://") {
        moosuri_vault_core::Totp::parse_uri(&seed)
    } else {
        moosuri_vault_core::Totp::from_base32(&seed, moosuri_vault_core::TotpAlgorithm::Sha1, 6, 30)
    }
    .map_err(|_| Error::Policy)
}
impl Engine {
    pub fn archive_gitea_grant(
        &mut self,
        vault: &mut UnlockedArchive,
        item: Uuid,
        origin: String,
        repository: GitHubRepository,
        expires: i64,
    ) -> Result<(Binding, GiteaRepository)> {
        if vault.header().binding.owner_id != self.device().owner.account
            || vault.header().binding.tenant_id != self.device().owner.tenant
            || !valid_origin(&origin)
            || !repository.valid()
        {
            return Err(Error::Policy);
        }
        let selected = vault
            .items()
            .iter()
            .find(|i| i.id == item)
            .ok_or(Error::Policy)?;
        let revision = selected.revision;
        let key = vault.header().generation;
        fields(&vault.open(item).map_err(|_| Error::Policy)?, &origin)?;
        if let Some(g) = self.grants().iter().find(|g| {
            g.gitea().is_some_and(|r| r.repository == repository)
                && g.site_origin() == origin
                && g.binding.expires_at > current_time().unwrap_or(i64::MAX)
        }) {
            if g.live().is_some()
                || g.item() != item
                || g.vault() != Some(vault.header().binding.vault_id)
                || g.binding.item_epoch != revision
                || g.binding.key_epoch != key
            {
                return Err(Error::Policy);
            }
            return Ok((g.binding.clone(), g.gitea().unwrap().clone()));
        }
        let resource = GiteaRepository {
            gitea_site: Uuid::new_v4(),
            repository,
        };
        let grant = policy::LocalGrant::select_gitea(
            self.device().clone(),
            item,
            vault.header().binding.vault_id,
            origin,
            resource.clone(),
            revision,
            key,
            expires,
        )?;
        Ok((self.select(grant)?, resource))
    }
    pub fn prepare_gitea(
        &mut self,
        vault: &UnlockedArchive,
        request: Request,
        now: i64,
    ) -> Result<()> {
        let g = self
            .grants()
            .iter()
            .find(|g| g.binding == request.binding && g.gitea().is_some())
            .ok_or(Error::Policy)?;
        if g.live().is_some()
            || g.vault() != Some(vault.header().binding.vault_id)
            || vault.header().binding.owner_id != self.device().owner.account
            || vault.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let revision = vault
            .items()
            .iter()
            .find(|i| i.id == g.item())
            .ok_or(Error::Policy)?
            .revision;
        self.prepare(request, now, revision, vault.header().generation)
    }
    pub async fn execute_gitea(
        &mut self,
        vault: &mut UnlockedArchive,
        invocation: &Invocation,
    ) -> Result<SignedReceipt> {
        let request = self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request
            .clone();
        let g = self
            .grants()
            .iter()
            .find(|g| g.binding == request.binding && g.gitea().is_some())
            .ok_or(Error::Policy)?
            .clone();
        if g.live().is_some()
            || g.vault() != Some(vault.header().binding.vault_id)
            || vault.header().binding.owner_id != self.device().owner.account
            || vault.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let revision = vault
            .items()
            .iter()
            .find(|i| i.id == g.item())
            .ok_or(Error::Policy)?
            .revision;
        let f = vault.open(g.item()).map_err(|_| Error::Policy)?;
        fields(&f, g.site_origin())?;
        self.execute_gitea_fields(invocation, &g, &f, revision, vault.header().generation)
            .await
    }
    pub(crate) async fn execute_gitea_fields(
        &mut self,
        invocation: &Invocation,
        g: &policy::LocalGrant,
        f: &ItemFields,
        revision: u64,
        key: u64,
    ) -> Result<SignedReceipt> {
        // Local browser and original lifetime must still be usable before commitment entry.
        self.browser_adapter_ready(current_time()?)?;
        fields(f, g.site_origin())?;
        let commitment = self.begin_dispatch(invocation, current_time()?, revision, key)?;
        let result = self
            .browser_gitea(
                g.site_origin(),
                &g.gitea().ok_or(Error::Policy)?.repository,
                f,
            )
            .await;
        let (outcome, projection) = match result {
            Ok(open_issues) => (
                Outcome::ProjectedSuccess,
                Projection::GiteaRepository { open_issues },
            ),
            Err(_) => (Outcome::Unknown, Projection::None),
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
