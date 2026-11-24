//! Optional, narrow member Gmail *write* consent and the fenced delivery worker.
//!
//! A readonly member grant never writes. Writing needs a second, explicit,
//! member-visible consent: the standard compose OAuth (readonly + send +
//! compose, one Google app, one mailbox) started from this member's session.
//! Tokens are exchanged and kept inside this member process; the browser only
//! ever sees the DTOs below (`public_response` re-encodes them), never a grant.
//!
//! All provider IO goes through the existing fixed loopback Google relay using
//! the typed `GmailClient::google_gmail_proxy` transport. The journaled standard
//! `MailService` (final-content approval, draft journal, send dispatch,
//! uncertainty reconciliation) is reused as-is; this module adds only:
//!   * consent routes (`status`, `start`, `capture`, `complete`, `disconnect`),
//!   * `MemberDispatchFence`, which every queued send must pass immediately
//!     before the provider call, and
//!   * `spawn_worker`, the standalone per-member delivery worker.
//!
//! Anything that cannot be verified fails closed: no fence approval, no send.
#![allow(dead_code)] // removable once main mounts `routes`, `public_response` and the worker.
use super::{Failure, hex64, mailbox, registered_client, response};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode},
    response::Response,
    routing::{get, post},
};
use moosuri_agent::mail::{DispatchFence, FenceFuture, MailService};
use moosuri_connectors::{
    gmail::{COMPOSE_SCOPE, COMPOSE_SCOPES, GmailClient, OAuthClient, SecretString},
    model::CancellationToken,
};
use moosuri_core::MailConnection;
use moosuri_store::{
    GoogleAppPin, MailBinding, MailDispatch, Store, StoreError, UserContext,
    personal_volume::VolumeOwner,
};
use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::Path, sync::Arc};

pub(crate) const BASE: &str = "/api/v1/personal/google/mail-write";
/// Private callback target (member daemon only, never routed to the browser).
pub(crate) const CAPTURE: &str = "/internal/google/mail-write/capture";
pub(crate) const ACTIONS: [&str; 4] = ["status", "start", "complete", "disconnect"];

/// Why a member cannot write yet. Each reason maps to exactly one next step.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Reason {
    /// No member Google application is configured in this container.
    GoogleSetupRequired,
    /// Only the readonly grant (or nothing) exists; the member must opt in.
    WriteConsentRequired,
    /// A write grant exists but for another mailbox than the one that was read.
    MailboxMismatch,
    /// The write grant was revoked/disconnected or needs reconnecting.
    ConnectionInactive,
    /// Google returned fewer scopes than the exact write consent requires.
    ScopeIncomplete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum NextStep {
    /// The member (or their operator) must configure the Google app first.
    ConfigureGoogleApp,
    /// Show the consent button: POST `BASE/start`.
    StartWriteConsent,
}
impl Reason {
    fn next(self) -> NextStep {
        match self {
            Reason::GoogleSetupRequired => NextStep::ConfigureGoogleApp,
            _ => NextStep::StartWriteConsent,
        }
    }
}

/// Every success body. Internally tagged, unknown fields rejected.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub(crate) enum Reply {
    NeedsConnection {
        reason: Reason,
        next: NextStep,
    },
    Authorization {
        authorization_url: String,
        flow_id: String,
    },
    Connected {
        connection_id: String,
        epoch: i64,
        mailbox: String,
    },
}
fn needs(reason: Reason) -> Reply {
    Reply::NeedsConnection {
        reason,
        next: reason.next(),
    }
}

/// Pick the one write connection usable for `read_mailbox`. Pure so the
/// consent matrix can be tested without a store.
pub(crate) fn choose(connections: &[MailConnection], read_mailbox: Option<&str>) -> Reply {
    let mut reason = Reason::WriteConsentRequired;
    // native_send connections are dispatched by a different (non-relay) path.
    for c in connections
        .iter()
        .filter(|c| c.compose_enabled && !c.native_send)
    {
        if c.status != "active" {
            reason = Reason::ConnectionInactive;
            continue;
        }
        if read_mailbox.is_some_and(|m| !m.eq_ignore_ascii_case(&c.email)) {
            reason = Reason::MailboxMismatch;
            continue;
        }
        return Reply::Connected {
            connection_id: c.id.clone(),
            epoch: c.epoch,
            mailbox: c.email.clone(),
        };
    }
    needs(reason)
}

impl Reply {
    fn valid(&self) -> bool {
        match self {
            Reply::NeedsConnection { reason, next } => reason.next() == *next,
            Reply::Authorization {
                authorization_url,
                flow_id,
            } => authorization_valid(authorization_url, flow_id),
            Reply::Connected {
                connection_id,
                epoch,
                mailbox: m,
            } => {
                !connection_id.is_empty()
                    && connection_id.len() <= 128
                    && !connection_id.chars().any(char::is_control)
                    && *epoch > 0
                    && mailbox(m)
            }
        }
    }
}
/// The URL must be exactly the Google compose consent for this app, with a
/// state whose public flow tag matches. Anything else is a gateway error.
fn authorization_valid(raw: &str, flow_id: &str) -> bool {
    let Ok(url) = url::Url::parse(raw) else {
        return false;
    };
    if raw.len() > 8192
        || !hex64(flow_id)
        || url.origin().ascii_serialization() != "https://accounts.google.com"
        || url.path() != "/o/oauth2/v2/auth"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
    {
        return false;
    }
    let mut f = std::collections::BTreeMap::new();
    for (k, v) in url.query_pairs() {
        if f.insert(k.into_owned(), v.into_owned()).is_some() {
            return false;
        }
    }
    let get = |k: &str| f.get(k).map(String::as_str);
    let Some(Ok(redirect)) = get("redirect_uri").map(url::Url::parse) else {
        return false;
    };
    f.len() == 10
        && redirect.scheme() == "https"
        && redirect.path() == "/api/v1/mail/oauth/callback"
        && redirect.query().is_none()
        && redirect.fragment().is_none()
        && get("client_id").is_some_and(|c| !c.is_empty() && c.len() <= 1024)
        && get("scope") == Some(COMPOSE_SCOPES.join(" ").as_str())
        && get("response_type") == Some("code")
        && get("access_type") == Some("offline")
        && get("include_granted_scopes") == Some("false")
        && get("prompt") == Some("consent select_account")
        && get("code_challenge_method") == Some("S256")
        && get("code_challenge").is_some_and(|c| c.len() == 43)
        && get("state").is_some_and(|s| hex64(s) && Store::mail_oauth_flow_id(s) == flow_id)
}

/// Gateway allowlist hook; `None` means "not a mail-write path". Success bodies
/// are parsed and re-encoded; unknown fields/states are a bad gateway.
pub(crate) fn public_response(
    path: &str,
    method: &Method,
    status: StatusCode,
    bytes: &[u8],
) -> Option<Result<Vec<u8>, StatusCode>> {
    let action = path.strip_prefix(BASE)?.strip_prefix('/')?;
    Some(checked(action, method, status, bytes))
}
fn checked(
    action: &str,
    method: &Method,
    status: StatusCode,
    bytes: &[u8],
) -> Result<Vec<u8>, StatusCode> {
    let invalid = StatusCode::BAD_GATEWAY;
    let expected = if action == "status" {
        Method::GET
    } else {
        Method::POST
    };
    if method != expected || !ACTIONS.contains(&action) {
        return Err(invalid);
    }
    if action == "disconnect" {
        return (status == StatusCode::NO_CONTENT && bytes.is_empty())
            .then(Vec::new)
            .ok_or(invalid);
    }
    if status != StatusCode::OK || bytes.len() > 16 * 1024 {
        return Err(invalid);
    }
    let reply: Reply = serde_json::from_slice(bytes).map_err(|_| invalid)?;
    let allowed = match (&reply, action) {
        (Reply::Authorization { .. }, "start") => true,
        (Reply::Authorization { .. }, _) => false,
        (_, "start") => matches!(reply, Reply::NeedsConnection { .. }),
        _ => true,
    };
    if !allowed || !reply.valid() {
        return Err(invalid);
    }
    serde_json::to_vec(&reply).map_err(|_| invalid)
}

enum App {
    Config(std::path::PathBuf),
    Missing,
    Invalid,
    Ready(Arc<OAuthClient>),
}
#[derive(Clone)]
struct Consent {
    store: Store,
    owner: VolumeOwner,
    app: Arc<App>,
    /// Typed relay client: token endpoint + Gmail v1 only, loopback proxy.
    client: GmailClient,
    serial: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    state: String,
    code: Option<SecretString>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handle {
    handle: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selected {
    connection_id: String,
    epoch: i64,
}

/// Mount beside `personal_google::local_routes`. `config` is the same member
/// `gws-client.json`, `proxy` the member's existing relay address.
pub(crate) fn routes(
    store: Store,
    owner: VolumeOwner,
    config: &Path,
    proxy: SocketAddr,
) -> Result<Router, &'static str> {
    let app = App::Config(config.to_owned());
    let client =
        GmailClient::google_gmail_proxy(proxy).map_err(|_| "member_google_proxy_invalid")?;
    Ok(router(store, owner, app, client))
}
fn router(store: Store, owner: VolumeOwner, app: App, client: GmailClient) -> Router {
    let state = Consent {
        store,
        owner,
        app: Arc::new(app),
        client,
        serial: Default::default(),
    };
    Router::new()
        .route(&format!("{BASE}/status"), get(status))
        .route(&format!("{BASE}/start"), post(start))
        .route(&format!("{BASE}/complete"), post(complete))
        .route(&format!("{BASE}/disconnect"), post(disconnect))
        .route(CAPTURE, post(capture))
        .with_state(state)
        .layer(DefaultBodyLimit::max(8192))
}

fn unavailable() -> Failure {
    Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable")
}
fn reply(action: &str, value: &Reply) -> Result<Response, Failure> {
    let method = if action == "status" {
        Method::GET
    } else {
        Method::POST
    };
    let bytes = serde_json::to_vec(value).map_err(|_| unavailable())?;
    let bytes = checked(action, &method, StatusCode::OK, &bytes).map_err(|_| unavailable())?;
    Ok(response(StatusCode::OK, bytes))
}
impl Consent {
    async fn user(&self, headers: &HeaderMap) -> Result<UserContext, Failure> {
        let user = moosuri_server::personal_request_user(&self.store, headers).await?;
        if user.account().id != self.owner.account.to_string() {
            return Err(StoreError::Forbidden.into());
        }
        Ok(user)
    }
    fn serial(&self) -> Result<tokio::sync::MutexGuard<'_, ()>, Failure> {
        self.serial
            .try_lock()
            .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))
    }
    /// `Ok(None)` is an actionable needs_connection, `Err` a broken config.
    fn app(&self) -> Result<Result<Arc<OAuthClient>, Reply>, Failure> {
        match &*self.app {
            App::Ready(app) => Ok(Ok(app.clone())),
            App::Config(path) => match registered_client(path) {
                Ok((_, app, _)) => Ok(Ok(Arc::new(app))),
                Err("google_setup_required") => Ok(Err(needs(Reason::GoogleSetupRequired))),
                Err(_) => Err(unavailable()),
            },
            App::Missing => Ok(Err(needs(Reason::GoogleSetupRequired))),
            App::Invalid => Err(unavailable()),
        }
    }
    async fn read_mailbox(&self, user: &UserContext) -> Result<Option<String>, Failure> {
        let s = self
            .store
            .member_google_status_local(user, self.owner)
            .await?;
        Ok(if s.connected && s.status == "active" {
            s.mailbox
        } else {
            None
        })
    }
    async fn current(&self, user: &UserContext) -> Result<Reply, Failure> {
        if let Err(reply) = self.app()? {
            return Ok(reply);
        }
        let read = self.read_mailbox(user).await?;
        let all = self.store.mail_connections(user).await?;
        Ok(choose(&all, read.as_deref()))
    }
}

async fn status(State(c): State<Consent>, headers: HeaderMap) -> Result<Response, Failure> {
    let user = c.user(&headers).await?;
    let current = c.current(&user).await?;
    c.user(&headers).await?;
    reply("status", &current)
}
/// The explicit opt-in. Never called implicitly; a session is required.
async fn start(State(c): State<Consent>, headers: HeaderMap) -> Result<Response, Failure> {
    let user = c.user(&headers).await?;
    let _serial = c.serial()?;
    let app = match c.app()? {
        Ok(app) => app,
        Err(reply_) => return reply("start", &reply_),
    };
    if c.read_mailbox(&user).await?.is_none() {
        return reply("start", &needs(Reason::WriteConsentRequired));
    }
    let pin = GoogleAppPin {
        revision: None,
        client_id: app.client_id.clone(),
        redirect_uri: app.redirect_uri.clone(),
    };
    let pending = c
        .store
        .start_mail_compose_oauth_with_app(&user, pin)
        .await?;
    let url = GmailClient::authorization_url_scoped(
        &app.client_id,
        &app.redirect_uri,
        &pending.state,
        &pending.verifier,
        &COMPOSE_SCOPES,
    )
    .map_err(|_| unavailable())?;
    // No URL for a session revoked while the flow was being created.
    c.user(&headers).await?;
    reply(
        "start",
        &Reply::Authorization {
            authorization_url: url,
            flow_id: pending.flow_id(),
        },
    )
}
/// Google's redirect lands here without a session; it only stores the code
/// under the one-use state. Claiming it still needs the original session.
async fn capture(
    State(c): State<Consent>,
    Json(input): Json<Capture>,
) -> Result<Response, Failure> {
    let Some(code) = input.code else {
        c.store
            .capture_member_compose_result(input.state, None)
            .await?;
        return Ok(response(StatusCode::OK, b"{}".to_vec()));
    };
    if !hex64(&input.state)
        || code.expose().len() > 4096
        || code.expose().chars().any(char::is_control)
    {
        return Err(StoreError::InvalidInput.into());
    }
    let handle = c
        .store
        .capture_member_compose_result(input.state, Some(code))
        .await?
        .ok_or(StoreError::Conflict)?;
    Ok(response(
        StatusCode::OK,
        serde_json::json!({"handle": handle})
            .to_string()
            .into_bytes(),
    ))
}
async fn complete(
    State(c): State<Consent>,
    headers: HeaderMap,
    Json(input): Json<Handle>,
) -> Result<Response, Failure> {
    if !hex64(&input.handle) {
        return Err(StoreError::InvalidInput.into());
    }
    let user = c.user(&headers).await?;
    let _serial = c.serial()?;
    let app = match c.app()? {
        Ok(app) => app,
        Err(reply_) => return reply("complete", &reply_),
    };
    let pin = GoogleAppPin {
        revision: None,
        client_id: app.client_id.clone(),
        redirect_uri: app.redirect_uri.clone(),
    };
    // Claim is pinned to this member's session, the state's app pin and PKCE.
    let exchange = c
        .store
        .claim_mail_oauth_with_app(&user, input.handle, pin)
        .await?;
    if !exchange.compose {
        return Err(StoreError::Forbidden.into());
    }
    let cancel = CancellationToken::new();
    let tokens = c
        .client
        .exchange_code(&app, &exchange.code, &exchange.verifier, &cancel)
        .await
        .map_err(|_| unavailable())?;
    let granted = tokens.scopes.as_deref().unwrap_or(&[]);
    if !COMPOSE_SCOPES
        .iter()
        .all(|s| granted.iter().any(|g| g == s))
    {
        return reply("complete", &needs(Reason::ScopeIncomplete));
    }
    let email = c
        .client
        .profile(&tokens.access_token, &cancel)
        .await
        .map_err(|_| unavailable())?
        .email_address;
    if !c
        .read_mailbox(&user)
        .await?
        .is_some_and(|m| m.eq_ignore_ascii_case(&email))
    {
        return reply("complete", &needs(Reason::MailboxMismatch));
    }
    let connection = c.store.complete_mail_oauth(exchange, tokens, email).await?;
    if c.user(&headers).await.is_err() {
        // Session ended mid-flow: do not leave a write grant behind.
        let _ = c.store.disconnect_mail(&user, connection.id).await;
        return Err(StoreError::Unauthorized.into());
    }
    reply("complete", &choose(&[connection], None))
}
/// Withdraw write consent. Readonly stays; queued sends then fail the fence.
async fn disconnect(
    State(c): State<Consent>,
    headers: HeaderMap,
    Json(input): Json<Selected>,
) -> Result<Response, Failure> {
    let user = c.user(&headers).await?;
    if input.connection_id.is_empty() || input.epoch <= 0 {
        return Err(StoreError::InvalidInput.into());
    }
    let all = c.store.mail_connections(&user).await?;
    if !all.iter().any(|x| {
        x.id == input.connection_id && x.epoch == input.epoch && x.compose_enabled && !x.native_send
    }) {
        return Err(StoreError::Conflict.into());
    }
    c.store.disconnect_mail(&user, input.connection_id).await?;
    Ok(response(StatusCode::NO_CONTENT, Vec::new()))
}

/// Main's work module answers whether the approved action is still bound to
/// the current source, session, Google app, connection/epoch and exactly this
/// final digest. There is deliberately no default implementation.
#[async_trait::async_trait]
pub(crate) trait ApprovalCheck: Send + Sync {
    async fn approved(
        &self,
        action_id: &str,
        connection_id: &str,
        connection_epoch: i64,
        digest: [u8; 32],
    ) -> bool;
}
/// Everything the worker knows about one committed send, as plain values.
pub(crate) struct Facts<'a> {
    pub owner_account: &'a str,
    pub dispatch_account: &'a str,
    pub has_compose: bool,
    pub app_matches: bool,
    pub read_mailbox: Option<&'a str>,
    pub binding_mailbox: &'a str,
    pub approved: bool,
}
impl Facts<'_> {
    pub(crate) fn allow(&self) -> bool {
        self.owner_account == self.dispatch_account
            && self.has_compose
            && self.app_matches
            && self.approved
            // The mailbox that was read must still be connected and identical.
            && self
                .read_mailbox
                .is_some_and(|m| m.eq_ignore_ascii_case(self.binding_mailbox))
    }
}
/// Checked by the standard dispatcher right before `send_frozen`. A `false`
/// settles the action as a definite non-send; nothing reaches Google.
pub(crate) struct MemberDispatchFence {
    store: Store,
    owner: VolumeOwner,
    app: GoogleAppPin,
    approvals: Option<Arc<dyn ApprovalCheck>>,
}
impl MemberDispatchFence {
    /// `approvals: None` is a valid, fully closed fence (nothing is sent).
    pub(crate) fn new(
        store: Store,
        owner: VolumeOwner,
        app: &OAuthClient,
        approvals: Option<Arc<dyn ApprovalCheck>>,
    ) -> Self {
        Self {
            store,
            owner,
            app: GoogleAppPin {
                revision: None,
                client_id: app.client_id.clone(),
                redirect_uri: app.redirect_uri.clone(),
            },
            approvals,
        }
    }
    async fn check(&self, d: &MailDispatch, b: &MailBinding) -> bool {
        let Some(approvals) = &self.approvals else {
            return false;
        };
        // The dispatcher carries a background capability; its approved read source is
        // checked transactionally by ApprovalCheck, without impersonating a browser session.
        let read_mailbox = Some(b.email.clone());
        let approved = approvals
            .approved(&d.action_id, &d.connection_id, b.epoch, d.frozen.digest())
            .await;
        Facts {
            owner_account: &self.owner.account.to_string(),
            dispatch_account: &d.user.account().id,
            has_compose: b.has_compose(),
            app_matches: b.app_pin().is_some_and(|p| {
                p.client_id == self.app.client_id && p.redirect_uri == self.app.redirect_uri
            }),
            read_mailbox: read_mailbox.as_deref(),
            binding_mailbox: &b.email,
            approved,
        }
        .allow()
    }
}
impl DispatchFence for MemberDispatchFence {
    fn permit<'a>(&'a self, d: &'a MailDispatch, b: &'a MailBinding) -> FenceFuture<'a> {
        Box::pin(self.check(d, b))
    }
}

/// Standalone per-member delivery worker: standard dispatch + reconciliation
/// on the member's own journal, relay transport, and the fence above.
/// `service` must come from `MailService::for_member_relay`.
pub(crate) fn spawn_worker(
    service: MailService,
    fence: Arc<MemberDispatchFence>,
    stop: CancellationToken,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(service.member_worker(fence, stop))
}
/// Draft saves reuse the existing journaled writer (see `mail_work::MailWriter`).
pub(crate) fn writer(service: MailService) -> Arc<dyn super::mail_work::MailWriter> {
    Arc::new(super::mail_work::StandardMailWriter(service))
}
/// The write scopes the consent requests, for UI copy and audits.
pub(crate) fn requested_scopes() -> &'static [&'static str] {
    debug_assert!(COMPOSE_SCOPES.contains(&COMPOSE_SCOPE));
    &COMPOSE_SCOPES
}

#[cfg(test)]
mod tests;
