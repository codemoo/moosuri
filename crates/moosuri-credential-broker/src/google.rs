//! Fixed Google OAuth refresh, identity verification and current-account INBOX counts.
//! No provider strings, credentials, token URLs or raw errors cross this adapter.
use crate::*;
use moosuri_vault_core::{ItemFields, ItemKind, SecretText, UnlockedArchive};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use zeroize::Zeroizing;
pub(crate) const LOCAL_POLICY: &[u8] = b"moosuri/local-google-inbox/v1\0https://oauth2.googleapis.com/token\0https://openidconnect.googleapis.com/v1/userinfo\0https://gmail.googleapis.com/gmail/v1/users/me/profile\0https://gmail.googleapis.com/gmail/v1/users/me/labels/INBOX\0openid email https://www.googleapis.com/auth/gmail.readonly";
pub fn resource_digest() -> [u8; 32] {
    auth::hash(b"moosuri/google-inbox-counts/v1")
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GoogleIdentity {
    pub subject: String,
    pub email: String,
}
impl GoogleIdentity {
    pub fn valid(&self) -> bool {
        !self.subject.is_empty()
            && self.subject.len() <= 255
            && self.subject.bytes().all(|b| b.is_ascii_graphic())
            && self.email.len() <= 254
            && self.email.contains('@')
            && self.email.bytes().all(|b| b.is_ascii_graphic())
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Credential {
    version: u8,
    client_id: SecretText,
    #[serde(default)]
    client_secret: Option<SecretText>,
    refresh_token: SecretText,
}
impl Credential {
    fn parse(secret: &str) -> Result<Self> {
        if secret.len() > 16384 {
            return Err(Error::Policy);
        }
        let c: Self = serde_json::from_str(secret).map_err(|_| Error::Policy)?;
        let valid = |s: &str, max: usize| {
            !s.is_empty() && s.len() <= max && s.bytes().all(|b| b.is_ascii_graphic())
        };
        if c.version != 1
            || !valid(c.client_id.expose(), 4096)
            || !valid(c.refresh_token.expose(), 8192)
            || c.client_secret
                .as_ref()
                .is_some_and(|s| !valid(s.expose(), 4096))
        {
            return Err(Error::Policy);
        }
        Ok(c)
    }
}
pub(crate) fn fields(fields: &ItemFields) -> Result<()> {
    if fields.kind != ItemKind::ApiKey
        || fields.url.expose() != "https://oauth2.googleapis.com"
        || !fields.totp.expose().is_empty()
        || !fields.expires_at.expose().is_empty()
    {
        return Err(Error::Policy);
    }
    Credential::parse(fields.secret.expose())?;
    Ok(())
}
#[derive(Deserialize)]
struct Token {
    access_token: SecretText,
    token_type: SecretText,
    scope: SecretText,
    expires_in: u64,
    #[serde(default)]
    refresh_token: Option<SecretText>,
}
#[derive(Deserialize)]
struct Identity {
    sub: SecretText,
    email: SecretText,
    email_verified: bool,
}
#[derive(Deserialize)]
struct Profile {
    #[serde(rename = "emailAddress")]
    email: SecretText,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Inbox {
    id: SecretText,
    messages_total: u32,
    messages_unread: u32,
    threads_total: u32,
    threads_unread: u32,
}
pub struct GoogleAdapter {
    client: reqwest::Client,
    token: String,
    userinfo: String,
    profile: String,
    inbox: String,
}
impl GoogleAdapter {
    pub fn new() -> Result<Self> {
        Self::at(
            "https://oauth2.googleapis.com/token",
            "https://openidconnect.googleapis.com/v1/userinfo",
            "https://gmail.googleapis.com/gmail/v1/users/me/profile",
            "https://gmail.googleapis.com/gmail/v1/users/me/labels/INBOX",
        )
    }
    fn at(token: &str, userinfo: &str, profile: &str, inbox: &str) -> Result<Self> {
        Ok(Self {
            client: reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .connect_timeout(Duration::from_secs(3))
                .timeout(Duration::from_secs(15))
                .build()
                .map_err(|_| Error::PrivateState)?,
            token: token.into(),
            userinfo: userinfo.into(),
            profile: profile.into(),
            inbox: inbox.into(),
        })
    }
    #[cfg(any(test, feature = "test-fixtures"))]
    pub fn synthetic_loopback(address: std::net::SocketAddr) -> Result<Self> {
        if !address.ip().is_loopback() {
            return Err(Error::Policy);
        }
        let base = format!("http://{address}");
        Self::at(
            &format!("{base}/token"),
            &format!("{base}/userinfo"),
            &format!("{base}/profile"),
            &format!("{base}/inbox"),
        )
    }
    async fn response<T: serde::de::DeserializeOwned>(
        request: reqwest::RequestBuilder,
    ) -> Result<T> {
        let mut response = request.send().await.map_err(|_| Error::Unknown)?;
        if !response.status().is_success() {
            return Err(if matches!(response.status().as_u16(), 400 | 401 | 403) {
                Error::Policy
            } else {
                Error::Unknown
            });
        }
        let mut bytes = Zeroizing::new(Vec::new());
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unknown)? {
            if bytes.len() + chunk.len() > 32768 {
                return Err(Error::Unknown);
            }
            bytes.extend_from_slice(&chunk)
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Policy)
    }
    async fn read(&self, identity: &GoogleIdentity, secret: &str) -> Result<Projection> {
        if !identity.valid() {
            return Err(Error::Policy);
        }
        let credential = Credential::parse(secret)?;
        let mut form = url::form_urlencoded::Serializer::new(String::new());
        form.append_pair("grant_type", "refresh_token")
            .append_pair("client_id", credential.client_id.expose())
            .append_pair("refresh_token", credential.refresh_token.expose());
        if let Some(client_secret) = &credential.client_secret {
            form.append_pair("client_secret", client_secret.expose());
        }
        let form = Zeroizing::new(form.finish());
        let token: Token = Self::response(
            self.client
                .post(&self.token)
                .header("content-type", "application/x-www-form-urlencoded")
                .body(form.as_bytes().to_vec()),
        )
        .await?;
        // Scope comes from the actual HTTPS refresh response, never the local claim.
        let scopes: std::collections::HashSet<_> =
            token.scope.expose().split_ascii_whitespace().collect();
        if token.token_type.expose() != "Bearer"
            || token.expires_in == 0
            || token.expires_in > 86400
            || token.access_token.expose().is_empty()
            || token.access_token.expose().len() > 16384
            || !token
                .access_token
                .expose()
                .bytes()
                .all(|b| b.is_ascii_graphic())
            || scopes.len() != 3
            || !scopes.contains("openid")
            || !scopes.contains("https://www.googleapis.com/auth/gmail.readonly")
            || !(scopes.contains("email")
                || scopes.contains("https://www.googleapis.com/auth/userinfo.email"))
            || token
                .refresh_token
                .as_ref()
                .is_some_and(|s| s.expose() != credential.refresh_token.expose())
        {
            return Err(Error::Policy);
        }
        // A rotated refresh token is never silently discarded or written in plaintext.
        let verified: Identity = Self::response(
            self.client
                .get(&self.userinfo)
                .bearer_auth(token.access_token.expose()),
        )
        .await?;
        if !verified.email_verified
            || verified.sub.expose() != identity.subject
            || verified.email.expose() != identity.email
        {
            return Err(Error::Policy);
        }
        let profile: Profile = Self::response(
            self.client
                .get(&self.profile)
                .bearer_auth(token.access_token.expose()),
        )
        .await?;
        if profile.email.expose() != verified.email.expose() {
            return Err(Error::Policy);
        }
        let inbox: Inbox = Self::response(
            self.client
                .get(&self.inbox)
                .bearer_auth(token.access_token.expose()),
        )
        .await?;
        if inbox.id.expose() != "INBOX"
            || inbox.messages_unread > inbox.messages_total
            || inbox.threads_unread > inbox.threads_total
            || [
                inbox.messages_total,
                inbox.messages_unread,
                inbox.threads_total,
                inbox.threads_unread,
            ]
            .iter()
            .any(|v| *v > i32::MAX as u32)
        {
            return Err(Error::Policy);
        }
        Ok(Projection::GoogleInbox {
            messages_total: inbox.messages_total,
            messages_unread: inbox.messages_unread,
            threads_total: inbox.threads_total,
            threads_unread: inbox.threads_unread,
        })
    }
}
impl Engine {
    pub fn archive_google_grant(
        &mut self,
        archive: &mut UnlockedArchive,
        item: uuid::Uuid,
        identity: GoogleIdentity,
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
        fields(&archive.open(item).map_err(|_| Error::Policy)?)?;
        if let Some(at) = self.grants().iter().position(|g| {
            g.google().is_some() && g.binding.expires_at > current_time().unwrap_or(i64::MAX)
        }) {
            // No retained invocation may cross a credential-version change.
            // The caller reconciles original receipts/commitments before reconnect.
            let binding = &self.state.grants[at].binding;
            if (binding.item_epoch != epoch || binding.key_epoch != key)
                && self.retained().iter().any(|r| r.binding.id == binding.id)
            {
                return Err(Error::Policy);
            }
            let g = &mut self.state.grants[at];
            if g.google() != Some(&identity)
                || g.live().is_some()
                || g.vault() != Some(archive.header().binding.vault_id)
                || g.item() != item
            {
                return Err(Error::Policy);
            }
            g.refresh_archive_google(epoch, key)?;
            g.resume_google();
            let binding = g.binding.clone();
            self.save()?;
            return Ok(binding);
        }
        self.select(policy::LocalGrant::select_google(
            self.device().clone(),
            item,
            archive.header().binding.vault_id,
            identity,
            epoch,
            key,
            expires,
        )?)
    }
    pub fn prepare_google(
        &mut self,
        archive: &UnlockedArchive,
        request: Request,
        now: i64,
    ) -> Result<()> {
        if request.operation != Operation::GoogleInboxRead
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
    pub async fn execute_google(
        &mut self,
        archive: &mut UnlockedArchive,
        invocation: &Invocation,
        adapter: &GoogleAdapter,
    ) -> Result<SignedReceipt> {
        let request = &self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request;
        let grant = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .ok_or(Error::Policy)?
            .clone();
        if grant.live().is_some()
            || grant.vault() != Some(archive.header().binding.vault_id)
            || archive.header().binding.owner_id != self.device().owner.account
            || archive.header().binding.tenant_id != self.device().owner.tenant
        {
            return Err(Error::Policy);
        }
        let epoch = archive
            .items()
            .iter()
            .find(|i| i.id == grant.item())
            .ok_or(Error::Policy)?
            .revision;
        let f = archive.open(grant.item()).map_err(|_| Error::Policy)?;
        fields(&f)?;
        self.execute_google_secret(
            invocation,
            adapter,
            f.secret.expose(),
            epoch,
            archive.header().generation,
        )
        .await
    }
    pub(crate) async fn execute_google_secret(
        &mut self,
        invocation: &Invocation,
        adapter: &GoogleAdapter,
        secret: &str,
        item_epoch: u64,
        key_epoch: u64,
    ) -> Result<SignedReceipt> {
        let request = &self
            .state
            .entries
            .iter()
            .find(|e| &e.request.invocation == invocation)
            .ok_or(Error::Policy)?
            .request;
        if request.operation != Operation::GoogleInboxRead {
            return Err(Error::Policy);
        }
        let identity = self
            .grants()
            .iter()
            .find(|g| g.binding.id == request.binding.id)
            .and_then(|g| g.google())
            .ok_or(Error::Policy)?
            .clone();
        let commitment = self.begin_dispatch(invocation, current_time()?, item_epoch, key_epoch)?;
        let (projection, outcome) = match adapter.read(&identity, secret).await {
            Ok(p) => (p, Outcome::ProjectedSuccess),
            Err(Error::Policy) => (Projection::None, Outcome::Rejected),
            Err(_) => (Projection::None, Outcome::Unknown),
        };
        let receipt = Receipt {
            commitment,
            outcome,
            projection,
            projection_digest: auth::projection_digest(projection),
        };
        let signed = SignedReceipt {
            signature: self.key.sign(&receipt_bytes(&receipt)),
            receipt,
        };
        if outcome == Outcome::Rejected {
            self.state
                .grants
                .iter_mut()
                .find(|g| g.binding.id == signed.receipt.commitment.request.binding.id)
                .ok_or(Error::Policy)?
                .suspend_google();
        }
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

#[cfg(test)]
mod tests;
