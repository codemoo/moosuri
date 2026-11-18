//! Device-owned immutable grant. Server metadata cannot create a local selection.
use crate::{Error, Result, auth::hash};
use moosuri_vault_protocol::broker::*;
use serde::{Deserialize, Serialize};
use uuid::Uuid;
/// Ciphertext-only freshness pin, retained on the device and never the assistant.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LivePin {
    pub origin: String,
    pub head: moosuri_vault_protocol::VaultHead,
    pub item_sha256: String,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LocalGrant {
    pub device: Device,
    pub binding: Binding,
    // Opaque local item reference; never uploaded to the assistant.
    item: Uuid,
    service: String,
    adapter_version: u32,
    origin: String,
    resource: Uuid,
    revoked: bool,
    #[serde(default)]
    repository: Option<GitHubRepository>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    gitea: Option<GiteaRepository>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    google: Option<crate::google::GoogleIdentity>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    google_suspended: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    google_preview: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    google_body: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    google_send: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    google_model_read: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    google_model_background: bool,
    #[serde(default)]
    vault: Option<Uuid>,
    #[serde(default)]
    live: Option<LivePin>,
    #[serde(default)]
    previous: Option<Binding>,
}
impl LocalGrant {
    /// Actual local connection/selection step. Foundation supports only a fixed,
    /// non-network synthetic operation; no model/server-facing grant constructor.
    pub fn select_fixed_probe(
        device: Device,
        item: Uuid,
        resource: Uuid,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        if item.is_nil()
            || resource.is_nil()
            || item_epoch == 0
            || key_epoch == 0
            || device.generation == 0
        {
            return Err(Error::Policy);
        }
        let mut g = Self {
            device,
            binding: Binding {
                id: Uuid::new_v4(),
                epoch: 1,
                grant_version: 1,
                grant_hash: [0; 32],
                item_epoch,
                key_epoch,
                expires_at,
            },
            item,
            service: "foundation-fixed-probe".into(),
            adapter_version: 1,
            origin: "https://fixed-probe.invalid".into(),
            resource,
            revoked: false,
            repository: None,
            gitea: None,
            google: None,
            google_suspended: false,
            google_preview: false,
            google_body: false,
            google_send: false,
            google_model_read: false,
            google_model_background: false,
            vault: None,
            live: None,
            previous: None,
        };
        // Fixed-order local commitment includes the otherwise hidden grant ceiling.
        let mut data = Vec::from(b"moosuri/local-fixed-probe/v1".as_slice());
        data.extend_from_slice(g.item.as_bytes());
        data.extend_from_slice(g.resource.as_bytes());
        data.extend_from_slice(&request_bytes(&Request {
            device: g.device.clone(),
            invocation: Invocation {
                run: Uuid::nil(),
                step: 0,
                call: Uuid::nil(),
                nonce: Uuid::nil(),
            },
            binding: g.binding.clone(),
            operation: Operation::FixedProbe,
            repository: None,
            digest: [0; 32],
            expires_at,
        }));
        g.binding.grant_hash = hash(&data);
        Ok(g)
    }
    #[expect(
        clippy::too_many_arguments,
        reason = "keep the public constructor and explicit vault, item and key epochs"
    )]
    pub fn select_gitea(
        device: Device,
        item: Uuid,
        vault: Uuid,
        origin: String,
        resource: GiteaRepository,
        item_epoch: u64,
        key_epoch: u64,
        expires: i64,
    ) -> Result<Self> {
        if !crate::gitea::valid_origin(&origin) || !resource.valid() || vault.is_nil() {
            return Err(Error::Policy);
        }
        let mut g =
            Self::select_fixed_probe(device, item, Uuid::new_v4(), item_epoch, key_epoch, expires)?;
        g.service = "gitea-browser-issues".into();
        g.origin = origin;
        g.repository = Some(resource.repository.clone());
        g.gitea = Some(resource);
        g.vault = Some(vault);
        let mut bytes = Vec::from(b"moosuri/local-gitea-grant/v1".as_slice());
        bytes.extend_from_slice(g.item.as_bytes());
        bytes.extend_from_slice(vault.as_bytes());
        bytes.extend_from_slice(g.origin.as_bytes());
        bytes.extend_from_slice(&g.gitea.as_ref().unwrap().bytes());
        bytes.extend_from_slice(&g.binding.item_epoch.to_be_bytes());
        bytes.extend_from_slice(&g.binding.key_epoch.to_be_bytes());
        bytes.extend_from_slice(&expires.to_be_bytes());
        bytes.extend_from_slice(g.device.id.as_bytes());
        g.binding.grant_hash = hash(&bytes);
        Ok(g)
    }
    pub fn gitea(&self) -> Option<&GiteaRepository> {
        self.gitea.as_ref()
    }
    pub(crate) fn site_origin(&self) -> &str {
        &self.origin
    }
    pub fn select_github(
        device: Device,
        item: Uuid,
        vault: Uuid,
        repository: GitHubRepository,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        if !repository.valid() {
            return Err(Error::Policy);
        }
        let mut g = Self::select_fixed_probe(
            device,
            item,
            Uuid::new_v4(),
            item_epoch,
            key_epoch,
            expires_at,
        )?;
        g.service = "github".into();
        g.origin = "https://api.github.com".into();
        g.repository = Some(repository);
        g.vault = Some(vault);
        let mut bytes = Vec::from(b"moosuri/local-github-grant/v1".as_slice());
        bytes.extend_from_slice(g.item.as_bytes());
        bytes.extend_from_slice(vault.as_bytes());
        bytes.extend_from_slice(&g.repository.as_ref().unwrap().bytes());
        bytes.extend_from_slice(&g.binding.item_epoch.to_be_bytes());
        bytes.extend_from_slice(&g.binding.key_epoch.to_be_bytes());
        bytes.extend_from_slice(&g.binding.expires_at.to_be_bytes());
        bytes.extend_from_slice(g.device.id.as_bytes());
        g.binding.grant_hash = hash(&bytes);
        Ok(g)
    }
    pub fn select_google(
        device: Device,
        item: Uuid,
        vault: Uuid,
        identity: crate::google::GoogleIdentity,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        if !identity.valid() || vault.is_nil() {
            return Err(Error::Policy);
        }
        let mut g = Self::select_fixed_probe(
            device,
            item,
            Uuid::new_v4(),
            item_epoch,
            key_epoch,
            expires_at,
        )?;
        g.service = "google-inbox".into();
        g.origin = "https://gmail.googleapis.com".into();
        g.vault = Some(vault);
        g.google = Some(identity);
        g.bind_archive_google()?;
        Ok(g)
    }
    pub fn select_google_preview(
        device: Device,
        item: Uuid,
        vault: Uuid,
        identity: crate::google::GoogleIdentity,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        let mut g = Self::select_google(
            device, item, vault, identity, item_epoch, key_epoch, expires_at,
        )?;
        g.google_preview = true;
        g.service = "google-inbox-preview".into();
        g.bind_archive_google()?;
        Ok(g)
    }
    pub fn select_google_body(
        device: Device,
        item: Uuid,
        vault: Uuid,
        identity: crate::google::GoogleIdentity,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        let mut g = Self::select_google(
            device, item, vault, identity, item_epoch, key_epoch, expires_at,
        )?;
        g.google_body = true;
        g.service = "google-inbox-body".into();
        g.bind_archive_google()?;
        Ok(g)
    }
    pub fn select_google_send(
        device: Device,
        item: Uuid,
        vault: Uuid,
        identity: crate::google::GoogleIdentity,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
    ) -> Result<Self> {
        let mut g = Self::select_google(
            device, item, vault, identity, item_epoch, key_epoch, expires_at,
        )?;
        g.google_send = true;
        g.service = "google-approved-mail".into();
        g.bind_archive_google()?;
        Ok(g)
    }
    /// Explicit device-local consent to send read mail content to the configured model.
    /// A display-only grant cannot call this constructor or acquire this flag.
    #[allow(clippy::too_many_arguments)]
    pub fn select_google_model_read(
        device: Device,
        item: Uuid,
        vault: Uuid,
        identity: crate::google::GoogleIdentity,
        item_epoch: u64,
        key_epoch: u64,
        expires_at: i64,
        background: bool,
    ) -> Result<Self> {
        let mut g = Self::select_google(
            device, item, vault, identity, item_epoch, key_epoch, expires_at,
        )?;
        g.google_model_read = true;
        g.google_model_background = background;
        g.service = "google-mail-model-read".into();
        g.bind_archive_google()?;
        Ok(g)
    }
    pub fn google_model_read(&self) -> bool {
        self.google_model_read
    }
    pub fn google_model_background(&self) -> bool {
        self.google_model_background
    }
    pub fn google_send(&self) -> bool {
        self.google_send
    }
    pub fn google_body(&self) -> bool {
        self.google_body
    }
    pub fn google_preview(&self) -> bool {
        self.google_preview
    }
    fn google_policy(&self) -> &'static [u8] {
        if self.google_model_read {
            crate::google::model_mail::MODEL_POLICY
        } else if self.google_send {
            crate::google::SEND_POLICY
        } else if self.google_body {
            crate::google::BODY_POLICY
        } else if self.google_preview {
            crate::google::PREVIEW_POLICY
        } else {
            crate::google::LOCAL_POLICY
        }
    }
    fn bind_archive_google(&mut self) -> Result<()> {
        let mut binding = self.binding.clone();
        binding.grant_hash = [0; 32];
        let mut bytes = Vec::from(self.google_policy());
        if let Some(r) = &self.gitea {
            bytes.extend_from_slice(b"moosuri/live-gitea/v1");
            bytes.extend_from_slice(&r.bytes());
            bytes.extend_from_slice(self.origin.as_bytes());
        }
        bytes.extend(
            serde_json::to_vec(&(
                &self.device,
                &binding,
                self.item,
                self.vault,
                self.google.as_ref(),
            ))
            .map_err(|_| Error::Policy)?,
        );
        if self.google_model_read {
            bytes.push(self.google_model_background as u8);
        }
        self.binding.grant_hash = hash(&bytes);
        Ok(())
    }
    /// Explicit snapshot reconnection. The original account/item/vault/expiry
    /// ceiling stays unchanged and the previous binding remains until exact ACK.
    pub(crate) fn refresh_archive_google(&mut self, item_epoch: u64, key_epoch: u64) -> Result<()> {
        if self.revoked
            || self.live.is_some()
            || self.google.is_none()
            || item_epoch < self.binding.item_epoch
            || key_epoch < self.binding.key_epoch
        {
            return Err(Error::Policy);
        }
        if item_epoch == self.binding.item_epoch && key_epoch == self.binding.key_epoch {
            return Ok(());
        }
        if self.previous.is_some() {
            return Err(Error::Policy);
        }
        let mut next = self.clone();
        next.previous = Some(self.binding.clone());
        next.binding.epoch = next.binding.epoch.checked_add(1).ok_or(Error::Policy)?;
        next.binding.grant_version = next
            .binding
            .grant_version
            .checked_add(1)
            .ok_or(Error::Policy)?;
        next.binding.item_epoch = item_epoch;
        next.binding.key_epoch = key_epoch;
        next.bind_archive_google()?;
        *self = next;
        Ok(())
    }
    pub(crate) fn suspend_google(&mut self) {
        self.google_suspended = true;
    }
    pub(crate) fn resume_google(&mut self) {
        self.google_suspended = false;
    }
    pub fn google(&self) -> Option<&crate::google::GoogleIdentity> {
        self.google.as_ref()
    }
    pub fn live(&self) -> Option<&LivePin> {
        self.live.as_ref()
    }
    pub fn pending_refresh(&self) -> Option<&Binding> {
        self.previous.as_ref()
    }
    pub(crate) fn confirm_refresh(&mut self) {
        self.previous = None;
    }
    pub(crate) fn select_live(mut self, pin: LivePin) -> Result<Self> {
        if !crate::live_vault::valid_origin(&pin.origin)
            || pin.head.header_sha256.len() != 64
            || pin.item_sha256.len() != 64
        {
            return Err(Error::Policy);
        }
        self.live = Some(pin);
        self.bind_live()?;
        Ok(self)
    }
    fn bind_live(&mut self) -> Result<()> {
        let pin = self.live.as_ref().ok_or(Error::Policy)?;
        let mut bytes = if self.google.is_some() {
            Vec::from(self.google_policy())
        } else {
            Vec::from(b"moosuri/local-live-github-grant/v1".as_slice())
        };
        if self.google_model_read {
            bytes.push(self.google_model_background as u8);
        }
        if let Some(google) = self.google.as_ref() {
            bytes.extend(serde_json::to_vec(google).map_err(|_| Error::Policy)?)
        }
        bytes.extend(
            serde_json::to_vec(&(
                self.item,
                self.vault,
                self.repository.as_ref(),
                &self.device,
                &self.binding.id,
                self.binding.epoch,
                self.binding.grant_version,
                self.binding.item_epoch,
                self.binding.key_epoch,
                self.binding.expires_at,
                pin,
            ))
            .map_err(|_| Error::Policy)?,
        );
        self.binding.grant_hash = hash(&bytes);
        Ok(())
    }
    /// Explicit local reconnection only; item, repository, vault and expiry
    /// ceiling cannot change. Retain the old binding until exact server ACK.
    pub(crate) fn refresh_live(
        &mut self,
        item_epoch: u64,
        key_epoch: u64,
        pin: LivePin,
    ) -> Result<()> {
        if self.revoked
            || self
                .live
                .as_ref()
                .is_some_and(|old| old.origin != pin.origin)
            || item_epoch < self.binding.item_epoch
            || key_epoch < self.binding.key_epoch
        {
            return Err(Error::Policy);
        }
        if self.live.as_ref() == Some(&pin)
            && item_epoch == self.binding.item_epoch
            && key_epoch == self.binding.key_epoch
        {
            return Ok(());
        }
        if self.previous.is_some() {
            return Err(Error::Policy);
        }
        self.previous = Some(self.binding.clone());
        self.binding.epoch = self.binding.epoch.checked_add(1).ok_or(Error::Policy)?;
        self.binding.grant_version = self
            .binding
            .grant_version
            .checked_add(1)
            .ok_or(Error::Policy)?;
        self.binding.item_epoch = item_epoch;
        self.binding.key_epoch = key_epoch;
        self.live = Some(pin);
        self.bind_live()
    }
    pub fn repository(&self) -> Option<&GitHubRepository> {
        self.repository.as_ref()
    }
    pub fn vault(&self) -> Option<Uuid> {
        self.vault
    }
    pub fn item(&self) -> Uuid {
        self.item
    }
    pub fn digest(&self) -> [u8; 32] {
        if let Some(r) = &self.gitea {
            return hash(&r.bytes());
        }
        if self.google.is_some() {
            return if self.google_model_read {
                crate::google::model_mail::resource_digest()
            } else if self.google_send {
                crate::google::send_digest()
            } else if self.google_body {
                crate::google::body_digest()
            } else if self.google_preview {
                crate::google::preview_digest()
            } else {
                crate::google::resource_digest()
            };
        }
        self.repository
            .as_ref()
            .map(|r| hash(&r.bytes()))
            .unwrap_or_else(|| self.probe_digest())
    }
    pub fn revoke(&mut self) {
        self.revoked = true;
    }
    pub fn check(
        &self,
        request: &Request,
        now: i64,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<()> {
        if self.revoked
            || self.google_suspended
            || self.previous.is_some()
            || request.device != self.device
            || request.binding != self.binding
            || request.expires_at <= now
            || request.expires_at > self.binding.expires_at
            || self.binding.expires_at <= now
            || item_epoch != self.binding.item_epoch
            || key_epoch != self.binding.key_epoch
            || self.adapter_version != 1
        {
            return Err(Error::Policy);
        }
        // Digest is derived locally from a typed fixed resource, not a URL supplied by server.
        let scope = match request.operation {
            Operation::GiteaRepositoryRead { site } => {
                self.service == "gitea-browser-issues"
                    && crate::gitea::valid_origin(&self.origin)
                    && self.gitea.as_ref().is_some_and(|r| {
                        r.valid()
                            && r.gitea_site == site
                            && Some(&r.repository) == request.repository.as_ref()
                    })
                    && self.google.is_none()
            }
            Operation::GoogleMailModelRead { selection } => {
                self.google_model_read
                    && !selection.is_nil()
                    && self.service == "google-mail-model-read"
                    && self.origin == "https://gmail.googleapis.com"
                    && self.google.as_ref().is_some_and(|g| g.valid())
                    && self.repository.is_none()
                    && request.repository.is_none()
            }
            Operation::GoogleMailSend { effect, .. }
            | Operation::GoogleMailLookup { effect, .. } => {
                self.google_send
                    && !effect.is_nil()
                    && self.service == "google-approved-mail"
                    && self.origin == "https://gmail.googleapis.com"
                    && self.google.as_ref().is_some_and(|g| g.valid())
                    && self.repository.is_none()
                    && request.repository.is_none()
            }
            Operation::FixedProbe => {
                self.service == "foundation-fixed-probe"
                    && self.origin == "https://fixed-probe.invalid"
                    && request.repository.is_none()
                    && self.repository.is_none()
                    && self.google.is_none()
            }
            Operation::GoogleInboxBody { selection } => {
                self.google_body
                    && !self.google_send
                    && !selection.is_nil()
                    && self.service == "google-inbox-body"
                    && self.origin == "https://gmail.googleapis.com"
                    && self.google.as_ref().is_some_and(|g| g.valid())
                    && self.repository.is_none()
                    && request.repository.is_none()
            }
            Operation::GoogleInboxRead | Operation::GoogleInboxPreview => {
                !self.google_model_read
                    && !self.google_send
                    && !self.google_body
                    && self.google_preview == (request.operation == Operation::GoogleInboxPreview)
                    && self.service
                        == if self.google_preview {
                            "google-inbox-preview"
                        } else {
                            "google-inbox"
                        }
                    && self.origin == "https://gmail.googleapis.com"
                    && self.google.as_ref().is_some_and(|g| g.valid())
                    && self.repository.is_none()
                    && request.repository.is_none()
            }
            Operation::GitHubRepositoryRead => {
                self.google.is_none()
                    && self.service == "github"
                    && self.origin == "https://api.github.com"
                    && request.repository == self.repository
                    && self.repository.as_ref().is_some_and(|r| r.valid())
            }
        };
        if !scope
            || (!matches!(request.operation, Operation::GoogleMailModelRead { .. })
                && request.digest != self.digest())
        {
            return Err(Error::Policy);
        }
        Ok(())
    }
    pub fn probe_digest(&self) -> [u8; 32] {
        let mut b = Vec::from(b"moosuri/fixed-probe/v1".as_slice());
        b.extend_from_slice(self.resource.as_bytes());
        hash(&b)
    }
}
