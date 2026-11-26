//! Opt-in v2-vault enrollment for one account or all installation members. Without the
//! installer option the legacy link-only status is unchanged and no listener exists.
//! Browser traffic reaches the public router only; the authority adapter is a
//! separate loopback listener protected by an owner-only random key.
use super::*;
use moosuri_vault_protocol::{AuthorityReply as Reply, AuthorityRequest as Request, Handoff};
use std::{future::Future, net::SocketAddr, pin::Pin};
mod lifecycle;

const STATUS_PATH: &str = "/api/v1/personal/vault";
const HANDOFF_PATH: &str = "/api/v1/personal/vault/handoff";
const AUTHORITY_PATH: &str = "/internal/member-vault/authority";
const KEY_HEADER: &str = "x-moosuri-vault-adapter";
const MAX_TOKENS: usize = 4096;
const MAX_MEMBER_TOKENS: usize = 32;
const HANDOFF_TTL: u64 = 30;
// Mirrors Store's fixed vault grant lifetime.
const GRANT_TTL: u64 = 86_400;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Kind {
    Handoff,
    Grant,
}
/// Original allocation scope; `epoch` is the registry authorization epoch
/// (SessionAllocation::epoch), never Store's per-user auth epoch.
#[derive(Clone, PartialEq, Eq)]
struct Scope {
    account: Uuid,
    tenant: Uuid,
    volume: Uuid,
    generation: i64,
    epoch: i64,
}
struct Entry {
    kind: Kind,
    scope: Scope,
    expires: Instant,
}
#[derive(Default)]
struct Table {
    tokens: HashMap<[u8; 32], Entry>,
}
fn scope_of(a: &SessionAllocation) -> Result<Scope, StatusCode> {
    let id = |s: &str| Uuid::parse_str(s).map_err(|_| StatusCode::SERVICE_UNAVAILABLE);
    Ok(Scope {
        account: id(&a.account_id)?,
        tenant: id(&a.tenant_id)?,
        volume: id(&a.volume_id)?,
        generation: a.generation,
        epoch: a.epoch,
    })
}

pub(super) struct Enrollment {
    account: Option<Uuid>,
    origin: String,
    key_tag: hmac::Tag,
    listen: SocketAddr,
    // In memory only: a gateway restart requires a fresh handoff.
    table: Mutex<Table>,
    lifecycle: tokio::sync::Mutex<lifecycle::State>,
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn key_digest(key: &str) -> [u8; 32] {
    let mut out = [0u8; 32];
    out.copy_from_slice(digest::digest(&digest::SHA256, key.as_bytes()).as_ref());
    out
}
/// Operator-provisioned, read-only (0400), gateway-owned file: exactly 64 hex
/// with at most one trailing newline. The gateway never creates, truncates or
/// rewrites it; other UIDs receive their own operator-provisioned safe copies.
fn load_key(path: &Path) -> Result<Zeroizing<String>, String> {
    const INVALID: &str = "vault_adapter_key_invalid";
    let parent = path.parent().ok_or(INVALID)?;
    private_dir(parent).map_err(|_| INVALID)?;
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| INVALID)?;
    let m = file.metadata().map_err(|_| INVALID)?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o7777 != 0o400
        || m.nlink() != 1
        || !(64..=65).contains(&m.len())
    {
        return Err(INVALID.into());
    }
    let mut raw = Zeroizing::new(Vec::with_capacity(66));
    (&mut file)
        .take(66)
        .read_to_end(&mut raw)
        .map_err(|_| INVALID)?;
    if raw.len() as u64 != m.len() || (raw.len() == 65 && raw[64] != b'\n') {
        return Err(INVALID.into());
    }
    let text = Zeroizing::new(String::from_utf8(raw[..64].to_vec()).map_err(|_| INVALID)?);
    if !hex64(&text) {
        return Err(INVALID.into());
    }
    Ok(text)
}
impl Enrollment {
    pub(super) fn new(options: &Options, site: Option<&str>) -> Result<Option<Self>, String> {
        let account = options.vault_member;
        if account.is_some() && options.vault_all_members {
            return Err("vault_enrollment_invalid".into());
        }
        if account.is_none() && !options.vault_all_members {
            if options.vault_adapter_listen.is_some() || options.vault_adapter_key_file.is_some() {
                return Err("vault_enrollment_incomplete".into());
            }
            return Ok(None);
        }
        let (Some(origin), Some(listen), Some(key_file)) = (
            site,
            options.vault_adapter_listen,
            options.vault_adapter_key_file.as_deref(),
        ) else {
            return Err("vault_enrollment_incomplete".into());
        };
        if account.is_some_and(|id| id.is_nil()) || !listen.ip().is_loopback() || listen.port() == 0
        {
            return Err("vault_enrollment_invalid".into());
        }
        if !key_file.is_absolute() {
            return Err("vault_adapter_key_invalid".into());
        }
        let key = load_key(key_file)?;
        Ok(Some(Self {
            account,
            origin: origin.to_owned(),
            key_tag: hmac::sign(
                &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
                KEY_HEADER.as_bytes(),
            ),
            listen,
            table: Mutex::new(Table::default()),
            lifecycle: tokio::sync::Mutex::new(lifecycle::State::default()),
        }))
    }
    pub(super) fn enrolled(&self, a: &Allocation) -> bool {
        Uuid::parse_str(&a.account_id)
            .is_ok_and(|id| !id.is_nil() && self.account.is_none_or(|account| id == account))
    }
    pub(super) fn fixed_member(&self, a: &Allocation) -> bool {
        self.account.is_some() && self.enrolled(a)
    }
    fn table(&self) -> Result<std::sync::MutexGuard<'_, Table>, StatusCode> {
        let mut t = self
            .table
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let now = Instant::now();
        t.tokens.retain(|_, e| e.expires > now);
        Ok(t)
    }
    /// Only called after a genuine upstream success and a current registry recheck.
    fn register(&self, token: &str, kind: Kind, scope: Scope, ttl: u64) -> Result<(), StatusCode> {
        let key = key_digest(token);
        let mut t = self.table()?;
        if t.tokens
            .get(&key)
            .is_some_and(|e| e.kind != kind || e.scope != scope)
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        if !t.tokens.contains_key(&key)
            && (t.tokens.len() >= MAX_TOKENS
                || t.tokens
                    .values()
                    .filter(|e| e.scope.account == scope.account)
                    .count()
                    >= MAX_MEMBER_TOKENS)
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        t.tokens.insert(
            key,
            Entry {
                kind,
                scope,
                expires: Instant::now() + Duration::from_secs(ttl),
            },
        );
        Ok(())
    }
    /// Missing, expired, wrong-type or stale mappings are all unauthorized.
    fn token_scope(&self, token: &str, kind: Kind) -> Result<Scope, StatusCode> {
        self.table()?
            .tokens
            .get(&key_digest(token))
            .filter(|e| e.kind == kind)
            .map(|e| e.scope.clone())
            .ok_or(StatusCode::UNAUTHORIZED)
    }
    fn authorize(
        &self,
        token: &str,
        kind: Kind,
        a: &SessionAllocation,
    ) -> Result<Scope, StatusCode> {
        let key = key_digest(token);
        let now = scope_of(a)?;
        let mut t = self.table()?;
        match t.tokens.get(&key) {
            Some(e) if e.kind == kind && e.scope == now => Ok(e.scope.clone()),
            Some(e) if e.kind == kind => {
                t.tokens.remove(&key);
                Err(StatusCode::UNAUTHORIZED)
            }
            _ => Err(StatusCode::UNAUTHORIZED),
        }
    }
    fn forget(&self, token: &str) {
        if let Ok(mut t) = self.table() {
            t.tokens.remove(&key_digest(token));
        }
    }
}

#[derive(Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
enum VaultStatus<'a> {
    SetupRequired {
        #[serde(skip_serializing_if = "Option::is_none")]
        site_origin: Option<&'a str>,
    },
    Ready {
        site_origin: &'a str,
    },
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct MemberStatus {
    vault_origin: String,
    account: Uuid,
}
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(STATUS_PATH, get(status))
        .route(HANDOFF_PATH, post(handoff))
}

async fn status(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        if method != Method::GET {
            return Err(StatusCode::METHOD_NOT_ALLOWED);
        }
        g.headers(&headers, &method)?;
        if uri.query().is_some() || !body.is_empty() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let (allocation, session) = authenticated_allocation(&g, &headers, true).await?;
        let upstream = g
            .proxy(
                &allocation,
                Method::GET,
                "/api/v1/auth/session",
                None,
                Some(&session),
            )
            .await;
        recheck(&g, &allocation).await?;
        let (status, bytes, _) = upstream?;
        if status != StatusCode::OK {
            return Err(status);
        }
        let view = member_session_view(&bytes, &allocation)?;
        if view.password_change_required || view.user.must_change_password {
            return Err(StatusCode::FORBIDDEN);
        }
        let state = match g.vault.as_deref() {
            // Legacy link-only behaviour.
            None => VaultStatus::SetupRequired {
                site_origin: g.vault_site_origin.as_deref(),
            },
            // Every other member: no link and no access.
            Some(v) if !v.enrolled(&allocation) => VaultStatus::SetupRequired { site_origin: None },
            Some(v) => {
                // Ready only if this exact private runtime answers with the same origin.
                let upstream = g
                    .proxy(&allocation, Method::GET, STATUS_PATH, None, Some(&session))
                    .await;
                recheck(&g, &allocation).await?;
                let (status, bytes, _) = upstream?;
                let member: MemberStatus =
                    serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
                if status != StatusCode::OK
                    || member.vault_origin != v.origin
                    || member.account.to_string() != allocation.account_id
                {
                    return Err(StatusCode::SERVICE_UNAVAILABLE);
                }
                VaultStatus::Ready {
                    site_origin: v.origin.as_str(),
                }
            }
        };
        let body = serde_json::to_vec(&state).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        Ok(response(StatusCode::OK, body))
    }
    .await;
    result.unwrap_or_else(failure)
}

async fn handoff(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        let vault = g.vault.as_deref().ok_or(StatusCode::NOT_FOUND)?;
        g.headers(&headers, &method)?;
        if method != Method::POST
            || uri.query().is_some()
            || (!body.is_empty() && body.as_ref() != b"{}")
            || one(&headers, "content-type").and_then(|s| s.split(';').next())
                != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let (allocation, session) = authenticated_allocation(&g, &headers, true).await?;
        if !vault.enrolled(&allocation) {
            return Err(StatusCode::FORBIDDEN);
        }
        let upstream = g
            .proxy(
                &allocation,
                Method::POST,
                HANDOFF_PATH,
                Some(b"{}"),
                Some(&session),
            )
            .await;
        // A fenced/suspended generation never publishes a token or an error detail.
        recheck(&g, &allocation).await?;
        let (status, bytes, _) = upstream?;
        if !status.is_success() {
            return api_error(status, &bytes);
        }
        let handoff: Handoff =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let now = now()? as i64;
        if status != StatusCode::OK
            || !hex64(&handoff.token)
            || handoff.vault_origin != vault.origin
            || handoff.expires_at <= now
            || handoff.expires_at > now + 35
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let body = serde_json::to_vec(&handoff).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        // Registered last: genuine success, validated shape and the recheck above
        // all hold for exactly this allocation (volume, generation, registry epoch).
        vault.register(
            &handoff.token,
            Kind::Handoff,
            scope_of(&allocation)?,
            (handoff.expires_at - now).clamp(1, HANDOFF_TTL as i64) as u64,
        )?;
        Ok(response(StatusCode::OK, body))
    }
    .await;
    result.unwrap_or_else(failure)
}

fn action(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 32
        && value.bytes().all(|b| b.is_ascii_lowercase() || b == b'_')
}
fn scoped(request: &Request, origin: &str) -> bool {
    match request {
        Request::LifecycleNext => true,
        Request::LifecycleAck { event } => !event.is_nil(),
        Request::Exchange {
            handoff,
            vault_origin,
        } => hex64(handoff) && vault_origin == origin,
        Request::Validate { grant } | Request::Close { grant } => hex64(grant),
        Request::Commit {
            grant,
            operation,
            digest,
        } => hex64(grant) && hex64(digest) && !operation.is_nil(),
    }
}
fn matches(request: &Request, reply: &Reply) -> bool {
    match request {
        Request::Exchange { .. } => {
            reply.grant.as_deref().is_some_and(hex64)
                && reply.operation.is_none()
                && reply.digest.is_none()
        }
        Request::Commit {
            operation, digest, ..
        } => {
            reply.grant.is_none()
                && reply.operation == Some(*operation)
                && reply.digest.as_deref() == Some(digest.as_str())
        }
        Request::LifecycleNext => {
            reply.grant.is_none()
                && reply.operation.is_some_and(|id| !id.is_nil())
                && reply.digest.as_deref().is_some_and(action)
        }
        Request::LifecycleAck { event } => {
            reply.grant.is_none()
                && reply.operation == Some(*event)
                && reply.digest.as_deref().is_some_and(action)
        }
        Request::Validate { .. } | Request::Close { .. } => {
            reply.grant.is_none() && reply.operation.is_none() && reply.digest.is_none()
        }
    }
}
fn token_of(request: &Request) -> Option<(&str, Kind)> {
    match request {
        Request::Exchange { handoff, .. } => Some((handoff, Kind::Handoff)),
        Request::Validate { grant } | Request::Close { grant } | Request::Commit { grant, .. } => {
            Some((grant, Kind::Grant))
        }
        _ => None,
    }
}

fn authorized(v: &Enrollment, headers: &HeaderMap) -> bool {
    // Service-only transport: no browser identity, ambient cookie or origin.
    if headers.contains_key(header::ORIGIN)
        || headers.contains_key(header::COOKIE)
        || headers.contains_key("x-real-ip")
    {
        return false;
    }
    one(headers, KEY_HEADER)
        .filter(|key| hex64(key))
        .is_some_and(|key| {
            hmac::verify(
                &hmac::Key::new(hmac::HMAC_SHA256, key.as_bytes()),
                KEY_HEADER.as_bytes(),
                v.key_tag.as_ref(),
            )
            .is_ok()
        })
}

async fn adapter(
    State(g): State<Gateway>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        let v = g.vault.as_deref().ok_or(StatusCode::NOT_FOUND)?;
        if !authorized(v, &headers) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if one(&headers, "content-type").and_then(|s| s.split(';').next())
            != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let request: Request =
            serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        if !scoped(&request, &v.origin) {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if token_of(&request).is_none() {
            return lifecycle::handle(&g, v, &request).await;
        }
        // Only the server's token mapping selects a member. The adapter protocol
        // never accepts a caller-supplied account or routing header.
        let (token, kind) = token_of(&request).ok_or(StatusCode::BAD_REQUEST)?;
        let registered = v.token_scope(token, kind)?;
        let a = resolve(
            &g,
            runtime_registry::SessionDestination::Account(registered.account),
        )
        .await?;
        if !v.enrolled(&a) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let original = v.authorize(token, kind, &a)?;
        let payload = serde_json::to_vec(&request).map_err(|_| StatusCode::BAD_REQUEST)?;
        let upstream = g
            .proxy(&a, Method::POST, AUTHORITY_PATH, Some(&payload), None)
            .await;
        // Past a fence nothing is published, registered or consumed as success.
        let fenced = recheck(&g, &a).await;
        if matches!(request, Request::Exchange { .. }) && upstream.is_ok() {
            // The one-time handoff is spent once the member runtime answered.
            if let Some((token, _)) = token_of(&request) {
                v.forget(token);
            }
        }
        fenced?;
        let (status, bytes, _) = upstream?;
        if !status.is_success() {
            return api_error(status, &bytes);
        }
        let reply: Reply =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if status != StatusCode::OK
            || reply.owner.account.to_string() != a.account_id
            || reply.owner.tenant.to_string() != a.tenant_id
            || original.account != reply.owner.account
            || original.tenant != reply.owner.tenant
            || !matches(&request, &reply)
        {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        match &request {
            Request::Exchange { .. } => {
                let grant = reply
                    .grant
                    .as_deref()
                    .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
                v.register(grant, Kind::Grant, original, GRANT_TTL)?;
            }
            Request::Close { grant } => v.forget(grant),
            _ => {}
        }
        Ok(response(
            StatusCode::OK,
            serde_json::to_vec(&reply).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        ))
    }
    .await;
    result.unwrap_or_else(failure)
}

fn adapter_router(g: Gateway) -> Router {
    Router::new()
        .route("/internal/v1/vault", post(adapter))
        .route("/internal/v1/vault/kiri-recipient", post(kiri_recipient))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(g)
}

async fn kiri_recipient(
    State(g): State<Gateway>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        let v = g.vault.as_deref().ok_or(StatusCode::NOT_FOUND)?;
        if !authorized(v, &headers) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        if one(&headers, "content-type").and_then(|s| s.split(';').next())
            != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let request: moosuri_vault_protocol::KiriRecipientRequest =
            serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        if request.challenge.is_nil() || !hex64(&request.grant) {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let registered = v.token_scope(&request.grant, Kind::Grant)?;
        let a = resolve(
            &g,
            runtime_registry::SessionDestination::Account(registered.account),
        )
        .await?;
        if !v.enrolled(&a) {
            return Err(StatusCode::UNAUTHORIZED);
        }
        v.authorize(&request.grant, Kind::Grant, &a)?;
        // Recheck the member's grant too: registry scope alone cannot establish
        // that the originating vault login is still live after a logout.
        let validation = Request::Validate {
            grant: request.grant,
        };
        let payload = serde_json::to_vec(&validation).map_err(|_| StatusCode::BAD_REQUEST)?;
        let (status, bytes, _) = g
            .proxy(&a, Method::POST, AUTHORITY_PATH, Some(&payload), None)
            .await?;
        recheck(&g, &a).await?;
        if status != StatusCode::OK {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let reply: Reply =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if reply.owner.account.to_string() != a.account_id
            || reply.owner.tenant.to_string() != a.tenant_id
            || !matches(&validation, &reply)
        {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let recipient = google_connection::portable::recipient(&g, &a, request.challenge)?;
        Ok(response(StatusCode::OK, recipient.to_string().into_bytes()))
    }
    .await;
    result.unwrap_or_else(failure)
}

/// Binds the fixed loopback listener at startup so misconfiguration fails closed.
pub(super) async fn serve_adapter(
    g: &Gateway,
) -> Result<Pin<Box<dyn Future<Output = Result<(), String>> + Send>>, String> {
    let Some(v) = g.vault.as_deref() else {
        return Ok(Box::pin(std::future::pending()));
    };
    let listener = tokio::net::TcpListener::bind(v.listen)
        .await
        .map_err(|_| "vault_adapter_unavailable")?;
    let app = adapter_router(g.clone());
    Ok(Box::pin(async move {
        axum::serve(listener, app)
            .await
            .map_err(|_| "vault_adapter_stopped".into())
    }))
}

#[cfg(test)]
mod tests;
