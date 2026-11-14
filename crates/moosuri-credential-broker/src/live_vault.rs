//! Native authenticated live ciphertext client. Master passwords never enter HTTP.
use crate::*;
use moosuri_vault_core::{
    ActiveVault, CommittedHeader, HeaderReceipt, ItemFields, ItemKind, SecretText, inventory_digest,
};
use moosuri_vault_protocol::{Handoff, Owner, VaultHead, VaultState};
use reqwest::{
    Client, Response,
    header::{HeaderValue, SET_COOKIE},
};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;
use uuid::Uuid;
use zeroize::Zeroizing;

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn digest(bytes: &[u8]) -> String {
    hex(&auth::hash(bytes))
}
fn unhex(text: &str) -> Result<[u8; 32]> {
    if text.len() != 64
        || !text
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err(Error::Reconnect);
    }
    let mut out = [0; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&text[i * 2..i * 2 + 2], 16).map_err(|_| Error::Reconnect)?;
    }
    Ok(out)
}
fn cookie_header(cookie: &str) -> Result<HeaderValue> {
    let mut h = HeaderValue::from_str(cookie).map_err(|_| Error::Reconnect)?;
    h.set_sensitive(true);
    Ok(h)
}
fn cookie(response: &Response, name: &str) -> Result<Zeroizing<String>> {
    let mut found = None;
    for line in response.headers().get_all(SET_COOKIE) {
        let line = line.to_str().map_err(|_| Error::Reconnect)?;
        if let Some(pair) = line.split(';').next()
            && pair.split_once('=').is_some_and(|(n, _)| n == name)
        {
            if found.is_some() || pair.len() > 512 || !pair.bytes().all(|b| b.is_ascii_graphic()) {
                return Err(Error::Reconnect);
            }
            found = Some(Zeroizing::new(pair.to_string()));
        }
    }
    found.ok_or(Error::Reconnect)
}
async fn json<T: DeserializeOwned>(mut response: Response, limit: usize) -> Result<T> {
    if matches!(response.status().as_u16(), 401 | 403) {
        return Err(Error::Reconnect);
    }
    if !response.status().is_success() {
        return Err(
            if response.status().is_server_error() || response.status().as_u16() == 429 {
                Error::Unavailable
            } else {
                Error::Reconnect
            },
        );
    }
    let mut bytes = Zeroizing::new(Vec::new());
    while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
        if bytes.len() + chunk.len() > limit {
            return Err(Error::Reconnect);
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::Reconnect)
}
pub(crate) fn valid_origin(origin: &str) -> bool {
    if auth::valid_origin(origin) {
        return true;
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    if let Ok(u) = url::Url::parse(origin) {
        return u.scheme() == "http"
            && matches!(u.host_str(), Some("localhost" | "127.0.0.1"))
            && u.username().is_empty()
            && u.password().is_none()
            && u.query().is_none()
            && u.fragment().is_none()
            && u.path() == "/"
            && u.origin().ascii_serialization() == origin;
    }
    false
}
/// Owns only memory sessions and the real vault-core root. A transient outage
/// suspends dispatch; it does not discard a still-valid unlocked root/session.
pub struct LiveVault {
    client: Client,
    origin: String,
    owner: Owner,
    cookie: Zeroizing<String>,
    active: Option<ActiveVault>,
    head: Option<VaultHead>,
    pub(crate) pending: Option<Invocation>,
}
impl LiveVault {
    pub async fn login(
        assistant: &str,
        vault: &str,
        owner: Owner,
        email: &str,
        password: &SecretText,
    ) -> Result<Self> {
        if !auth::valid_origin(assistant) || !auth::valid_origin(vault) {
            return Err(Error::Policy);
        }
        Self::login_inner(assistant, vault, owner, email, password).await
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub async fn synthetic_login(
        assistant: &str,
        vault: &str,
        owner: Owner,
        email: &str,
        password: &SecretText,
    ) -> Result<Self> {
        Self::login_inner(assistant, vault, owner, email, password).await
    }
    async fn login_inner(
        assistant: &str,
        vault: &str,
        owner: Owner,
        email: &str,
        password: &SecretText,
    ) -> Result<Self> {
        if !valid_origin(assistant)
            || !valid_origin(vault)
            || url::Url::parse(assistant)
                .map_err(|_| Error::Policy)?
                .host_str()
                == url::Url::parse(vault)
                    .map_err(|_| Error::Policy)?
                    .host_str()
        {
            return Err(Error::Policy);
        }
        let client = Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(3))
            .timeout(Duration::from_secs(15))
            .build()
            .map_err(|_| Error::PrivateState)?;
        #[derive(Serialize)]
        struct Login<'a> {
            email: &'a str,
            password: &'a str,
        }
        let body = Zeroizing::new(
            serde_json::to_vec(&Login {
                email,
                password: password.expose(),
            })
            .map_err(|_| Error::Policy)?,
        );
        let response = client
            .post(format!("{assistant}/api/v1/auth/login"))
            .header("Origin", assistant)
            .header("Content-Type", "application/json")
            .body(body.to_vec())
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        let assistant_cookie = cookie(&response, "moosuri_session")?;
        let session: serde_json::Value = json(response, 16384).await?;
        if session["user"]["id"].as_str() != Some(owner.account.to_string().as_str())
            || session["user"]["must_change_password"].as_bool() != Some(false)
        {
            return Err(Error::Reconnect);
        }
        let handoff: Handoff = json(
            client
                .post(format!("{assistant}/api/v1/vault/handoff"))
                .header("Origin", assistant)
                .header("Cookie", cookie_header(&assistant_cookie)?)
                .header("X-Moosuri-Account", owner.account.to_string())
                .send()
                .await
                .map_err(|_| Error::Unavailable)?,
            4096,
        )
        .await?;
        if handoff.vault_origin != vault
            || handoff.token.len() != 64
            || !handoff.token.bytes().all(|b| b.is_ascii_hexdigit())
            || handoff.expires_at <= current_time()?
        {
            return Err(Error::Reconnect);
        }
        let response = client
            .post(format!("{vault}/session"))
            .header("Origin", assistant)
            .header("Content-Type", "application/x-www-form-urlencoded")
            .body(Zeroizing::new(format!("handoff={}", handoff.token)).to_string())
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        if response.status() != reqwest::StatusCode::SEE_OTHER
            || response
                .headers()
                .get("location")
                .and_then(|h| h.to_str().ok())
                != Some("/")
        {
            return Err(Error::Reconnect);
        }
        let name = if vault.starts_with("https:") {
            "__Host-moosuri_vault"
        } else {
            "moosuri_vault_dev"
        };
        let cookie = cookie(&response, name)?;
        // No assistant account cookie is retained or ever sent to the vault.
        drop(assistant_cookie);
        let mut live = Self {
            client,
            origin: vault.into(),
            owner,
            cookie,
            active: None,
            head: None,
            pending: None,
        };
        live.state().await?;
        Ok(live)
    }
    pub async fn validate(&mut self) -> Result<()> {
        let result = async {
            let session: moosuri_vault_protocol::CurrentSession = json(
                self.client
                    .get(format!("{}/session/current", self.origin))
                    .header("Cookie", cookie_header(&self.cookie)?)
                    .send()
                    .await
                    .map_err(|_| Error::Unavailable)?,
                4096,
            )
            .await?;
            if session.owner != self.owner || self.head.as_ref().is_some_and(|h| *h != session.head)
            {
                return Err(Error::Reconnect);
            }
            Ok(())
        }
        .await;
        if matches!(result, Err(Error::Reconnect)) {
            self.retire();
        }
        result
    }
    pub fn is_unlocked(&self) -> bool {
        self.active.is_some()
    }
    pub fn retire(&mut self) {
        self.active = None;
        self.head = None;
        self.cookie.clear();
        self.pending = None;
    }
    async fn state_inner(&self) -> Result<VaultState> {
        let state: VaultState = json(
            self.client
                .get(format!("{}/api/state", self.origin))
                .header("Cookie", cookie_header(&self.cookie)?)
                .header("X-Moosuri-Vault-Account", self.owner.account.to_string())
                .send()
                .await
                .map_err(|_| Error::Unavailable)?,
            32 * 1024 * 1024,
        )
        .await?;
        if state.owner != self.owner || state.items.len() > 256 {
            return Err(Error::Reconnect);
        }
        let header = state.header.as_ref().ok_or(Error::Reconnect)?;
        let binding = state.binding.ok_or(Error::Reconnect)?;
        if header.binding != binding
            || binding.owner_id != self.owner.account
            || binding.tenant_id != self.owner.tenant
            || digest(&header.to_json().map_err(|_| Error::Reconnect)?) != state.header_sha256
            || hex(&inventory_digest(&state.items).map_err(|_| Error::Reconnect)?)
                != state.inventory_sha256
        {
            return Err(Error::Reconnect);
        }
        if let Some(head) = &self.head
            && *head != head_of(&state)?
        {
            return Err(Error::Reconnect);
        }
        Ok(state)
    }
    async fn state(&mut self) -> Result<VaultState> {
        match self.state_inner().await {
            Err(Error::Reconnect) => {
                self.retire();
                Err(Error::Reconnect)
            }
            other => other,
        }
    }
    pub async fn unlock(&mut self, password: &SecretText) -> Result<()> {
        self.active = None;
        self.head = None;
        let state = self.state().await?;
        let binding = state.binding.ok_or(Error::Reconnect)?;
        let committed = CommittedHeader::admit(
            state.header.clone().ok_or(Error::Reconnect)?,
            binding,
            HeaderReceipt::from_verified_current_storage(binding, unhex(&state.header_sha256)?),
        )
        .map_err(|_| Error::Reconnect)?;
        let active = ActiveVault::unlock_password(committed, binding, password)
            .map_err(|_| Error::Reconnect)?;
        self.head = Some(head_of(&state)?);
        self.active = Some(active);
        // A delayed initial read is not enough: revalidate after KDF/unlock.
        self.state().await?;
        Ok(())
    }
    pub async fn items(&mut self) -> Result<Vec<(Uuid, String)>> {
        let result = self.items_inner().await;
        if matches!(result, Err(Error::Reconnect)) {
            self.retire();
        }
        result
    }
    async fn items_inner(&mut self) -> Result<Vec<(Uuid, String)>> {
        let state = self.state().await?;
        let active = self.active.as_mut().ok_or(Error::Locked)?;
        let mut items = vec![];
        for encrypted in state.items {
            let item = active.open_item(&encrypted).map_err(|_| Error::Reconnect)?;
            if item.kind == ItemKind::ApiKey {
                items.push((encrypted.id, item.title.expose().to_owned()));
            }
        }
        self.state().await?;
        Ok(items)
    }
    async fn selection(
        &mut self,
        item: Uuid,
        google: bool,
    ) -> Result<(Uuid, u64, u64, policy::LivePin)> {
        let result = self.selection_inner(item, google).await;
        if matches!(result, Err(Error::Reconnect)) {
            self.retire();
        }
        result
    }
    async fn selection_inner(
        &mut self,
        item: Uuid,
        google: bool,
    ) -> Result<(Uuid, u64, u64, policy::LivePin)> {
        let state = self.state().await?;
        let encrypted = state
            .items
            .iter()
            .find(|i| i.id == item)
            .ok_or(Error::Reconnect)?;
        let fields = self
            .active
            .as_mut()
            .ok_or(Error::Locked)?
            .open_item(encrypted)
            .map_err(|_| Error::Reconnect)?;
        if google {
            crate::google::fields(&fields).map_err(|_| Error::Reconnect)?;
        } else {
            github_fields(&fields)?;
        }
        let pin = policy::LivePin {
            origin: self.origin.clone(),
            head: head_of(&state)?,
            item_sha256: digest(&encrypted.to_json().map_err(|_| Error::Reconnect)?),
        };
        Ok((
            state.binding.ok_or(Error::Reconnect)?.vault_id,
            encrypted.revision,
            state.header.ok_or(Error::Reconnect)?.generation,
            pin,
        ))
    }
    pub(crate) async fn check(&mut self, grant: &policy::LocalGrant) -> Result<VaultState> {
        let result = async {
            let state = self.state().await?;
            let selected = state
                .items
                .iter()
                .find(|i| i.id == grant.item())
                .ok_or(Error::Reconnect)?;
            let pin = grant.live().ok_or(Error::Policy)?;
            if state.binding.ok_or(Error::Reconnect)?.vault_id
                != grant.vault().ok_or(Error::Policy)?
                || selected.revision != grant.binding.item_epoch
                || state.header.as_ref().ok_or(Error::Reconnect)?.generation
                    != grant.binding.key_epoch
                || pin.origin != self.origin
                || pin.head != head_of(&state)?
                || pin.item_sha256 != digest(&selected.to_json().map_err(|_| Error::Reconnect)?)
            {
                return Err(Error::Reconnect);
            }
            Ok(state)
        }
        .await;
        if matches!(result, Err(Error::Reconnect)) {
            self.retire();
        }
        result
    }
}
fn head_of(state: &VaultState) -> Result<VaultHead> {
    let h = state.header.as_ref().ok_or(Error::Reconnect)?;
    Ok(VaultHead {
        header_sha256: state.header_sha256.clone(),
        revision: h.revision,
        envelope_epoch: h.envelope_epoch,
        generation: h.generation,
    })
}
fn github_fields(fields: &ItemFields) -> Result<()> {
    if fields.kind != ItemKind::ApiKey
        || fields.url.expose().trim_end_matches('/') != "https://api.github.com"
        || fields.secret.expose().is_empty()
        || !fields.totp.expose().is_empty()
        || !fields.expires_at.expose().is_empty()
    {
        return Err(Error::Reconnect);
    }
    Ok(())
}
impl Engine {
    pub async fn live_grant(
        &mut self,
        vault: &mut LiveVault,
        item: Uuid,
        repository: GitHubRepository,
        expires: i64,
    ) -> Result<Binding> {
        if vault.owner != self.device().owner {
            return Err(Error::Policy);
        }
        let (id, item_epoch, key_epoch, pin) = vault.selection(item, false).await?;
        vault.state().await?;
        if let Some(at) = self.state.grants.iter().position(|g| {
            g.repository() == Some(&repository)
                && g.binding.expires_at > current_time().unwrap_or(i64::MAX)
        }) {
            let grant = &mut self.state.grants[at];
            if grant.item() != item || grant.vault() != Some(id) {
                return Err(Error::Policy);
            }
            grant.refresh_live(item_epoch, key_epoch, pin)?;
            let binding = grant.binding.clone();
            self.save()?;
            return Ok(binding);
        }
        self.select(
            policy::LocalGrant::select_github(
                self.device().clone(),
                item,
                id,
                repository,
                item_epoch,
                key_epoch,
                expires,
            )?
            .select_live(pin)?,
        )
    }
    pub async fn live_google_grant(
        &mut self,
        vault: &mut LiveVault,
        item: Uuid,
        identity: crate::google::GoogleIdentity,
        expires: i64,
    ) -> Result<Binding> {
        if vault.owner != self.device().owner {
            return Err(Error::Policy);
        }
        let (id, item_epoch, key_epoch, pin) = vault.selection(item, true).await?;
        vault.state().await?;
        if let Some(at) = self.state.grants.iter().position(|g| {
            g.google().is_some() && g.binding.expires_at > current_time().unwrap_or(i64::MAX)
        }) {
            let grant = &mut self.state.grants[at];
            if grant.google() != Some(&identity)
                || grant.item() != item
                || grant.vault() != Some(id)
            {
                return Err(Error::Policy);
            }
            grant.refresh_live(item_epoch, key_epoch, pin)?;
            grant.resume_google();
            let binding = grant.binding.clone();
            self.save()?;
            return Ok(binding);
        }
        self.select(
            policy::LocalGrant::select_google(
                self.device().clone(),
                item,
                id,
                identity,
                item_epoch,
                key_epoch,
                expires,
            )?
            .select_live(pin)?,
        )
    }
    pub(crate) async fn execute_live_google(
        &mut self,
        vault: &mut LiveVault,
        invocation: &Invocation,
        adapter: &crate::google::GoogleAdapter,
    ) -> Result<SignedReceipt> {
        let request = &self
            .state
            .entries
            .iter()
            .find(|e| e.request.invocation == *invocation)
            .ok_or(Error::Policy)?
            .request;
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        let state = vault.check(&grant).await?;
        let encrypted = state
            .items
            .iter()
            .find(|i| i.id == grant.item())
            .ok_or(Error::Reconnect)?;
        let fields = vault
            .active
            .as_mut()
            .ok_or(Error::Locked)?
            .open_item(encrypted)
            .map_err(|_| Error::Reconnect)?;
        crate::google::fields(&fields).map_err(|_| Error::Reconnect)?;
        vault.check(&grant).await?;
        let receipt = self
            .execute_google_secret(
                invocation,
                adapter,
                fields.secret.expose(),
                grant.binding.item_epoch,
                grant.binding.key_epoch,
            )
            .await?;
        if receipt.receipt.outcome == Outcome::Rejected {
            vault.retire()
        }
        Ok(receipt)
    }
    pub(crate) fn confirm_live_refresh(&mut self, binding: &Binding) -> Result<()> {
        self.state
            .grants
            .iter_mut()
            .find(|g| g.binding == *binding)
            .ok_or(Error::Policy)?
            .confirm_refresh();
        self.save()
    }
    pub(crate) async fn prepare_live_github(
        &mut self,
        vault: &mut LiveVault,
        request: Request,
    ) -> Result<()> {
        if !matches!(
            request.operation,
            Operation::GitHubRepositoryRead | Operation::GoogleInboxRead
        ) || vault.owner != self.device().owner
        {
            return Err(Error::Policy);
        }
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        vault.check(&grant).await?;
        self.prepare(
            request,
            current_time()?,
            grant.binding.item_epoch,
            grant.binding.key_epoch,
        )
    }
    pub(crate) async fn execute_live_github(
        &mut self,
        vault: &mut LiveVault,
        invocation: &Invocation,
        adapter: &github::GitHubAdapter,
    ) -> Result<SignedReceipt> {
        let request = self
            .state
            .entries
            .iter()
            .find(|e| e.request.invocation == *invocation)
            .ok_or(Error::Policy)?
            .request
            .clone();
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        let state = vault.check(&grant).await?;
        let encrypted = state
            .items
            .iter()
            .find(|i| i.id == grant.item())
            .ok_or(Error::Reconnect)?;
        let fields = vault
            .active
            .as_mut()
            .ok_or(Error::Locked)?
            .open_item(encrypted)
            .map_err(|_| Error::Reconnect)?;
        github_fields(&fields)?;
        // Output/adaptor admission point: fresh header AND selected item after
        // decrypt/async work. Later peer changes cannot recall an entered read.
        vault.check(&grant).await?;
        let receipt = self
            .execute_github_secret(
                invocation,
                adapter,
                fields.secret.expose(),
                grant.binding.item_epoch,
                grant.binding.key_epoch,
            )
            .await?;
        if receipt.receipt.outcome == Outcome::Rejected {
            vault.retire();
        }
        Ok(receipt)
    }
}
