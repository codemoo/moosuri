//! Private member Google adapter. Browser DTOs never carry broker grants.
mod local_gws;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
pub(crate) use local_gws::registered_client;
use moosuri_connectors::gmail::{READ_SCOPE, SecretString};
use moosuri_store::{Store, StoreError, UserContext, personal_volume::VolumeOwner};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::{path::PathBuf, sync::Arc};
use uuid::Uuid;
use zeroize::Zeroizing;

const BASE: &str = "/api/v1/personal/google";
#[cfg(test)]
const INBOX_SCOPE: &str = "one_inbox_page_metadata_only";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Connection {
    connection_id: Uuid,
    epoch: i64,
    mailbox: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Status {
    connection: Option<Connection>,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Started {
    authorization_url: String,
    flow_id: String,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Completed {
    connection_id: Uuid,
    epoch: i64,
    mailbox: String,
    flow_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    continuation: Option<moosuri_store::MemberGoogleReadIntent>,
}
type Inbox = moosuri_store::MemberGoogleInbox;
fn hex64(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
}
fn mailbox(s: &str) -> bool {
    !s.is_empty() && s.len() <= 320 && s.contains('@') && !s.chars().any(char::is_control)
}
impl Connection {
    fn valid(&self) -> bool {
        !self.connection_id.is_nil() && self.epoch > 0 && mailbox(&self.mailbox)
    }
}
impl Started {
    fn valid(&self) -> bool {
        if !hex64(&self.flow_id) || self.authorization_url.len() > 8192 {
            return false;
        }
        let Ok(url) = url::Url::parse(&self.authorization_url) else {
            return false;
        };
        if url.origin().ascii_serialization() != "https://accounts.google.com"
            || url.path() != "/o/oauth2/v2/auth"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return false;
        }
        let mut fields = std::collections::BTreeMap::new();
        for (k, v) in url.query_pairs() {
            if fields.insert(k.into_owned(), v.into_owned()).is_some() {
                return false;
            }
        }
        let names = [
            "client_id",
            "redirect_uri",
            "response_type",
            "scope",
            "include_granted_scopes",
            "state",
            "access_type",
            "code_challenge",
            "code_challenge_method",
            "prompt",
        ];
        if fields.len() != names.len() || names.iter().any(|k| !fields.contains_key(*k)) {
            return false;
        }
        let Ok(redirect) = url::Url::parse(&fields["redirect_uri"]) else {
            return false;
        };
        if redirect.scheme() != "https"
            || redirect.path() != "/api/v1/mail/oauth/callback"
            || redirect.query().is_some()
            || redirect.fragment().is_some()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
            || fields["client_id"].is_empty()
            || fields["client_id"].len() > 1024
        {
            return false;
        }
        fields.get("scope").map(String::as_str) == Some(READ_SCOPE)
            && fields.get("response_type").map(String::as_str) == Some("code")
            && fields.get("access_type").map(String::as_str) == Some("offline")
            && fields.get("prompt").map(String::as_str) == Some("consent select_account")
            && fields.get("code_challenge").is_some_and(|s| {
                s.len() == 43
                    && s.bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
            && fields.get("include_granted_scopes").map(String::as_str) == Some("false")
            && fields.get("code_challenge_method").map(String::as_str) == Some("S256")
            && fields
                .get("state")
                .is_some_and(|s| hex64(s) && Store::mail_oauth_flow_id(s) == self.flow_id)
    }
}
fn decode<T: DeserializeOwned>(bytes: &[u8]) -> Result<T, StatusCode> {
    if bytes.len() > 128 * 1024 {
        return Err(StatusCode::BAD_GATEWAY);
    }
    serde_json::from_slice(bytes).map_err(|_| StatusCode::BAD_GATEWAY)
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>, StatusCode> {
    serde_json::to_vec(value).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
/// Gateway allowlist: parse and reconstruct each permitted public success DTO.
/// Private capture and unknown fields/statuses never pass through this boundary.
pub fn public_response(
    path: &str,
    method: &Method,
    status: StatusCode,
    bytes: &[u8],
) -> Result<Vec<u8>, StatusCode> {
    let invalid = StatusCode::BAD_GATEWAY;
    if method == Method::POST
        && path == format!("{BASE}/disconnect")
        && status == StatusCode::NO_CONTENT
    {
        return if bytes.is_empty() {
            Ok(Vec::new())
        } else {
            Err(invalid)
        };
    }
    if status != StatusCode::OK {
        return Err(invalid);
    }
    if method == Method::GET && path == BASE {
        let value: Status = decode(bytes)?;
        if value.connection.as_ref().is_some_and(|c| !c.valid()) {
            return Err(invalid);
        }
        return encode(&value);
    }
    if method != Method::POST {
        return Err(invalid);
    }
    match path {
        "/api/v1/personal/google/start" => {
            let v: Started = decode(bytes)?;
            if !v.valid() {
                return Err(invalid);
            }
            encode(&v)
        }
        "/api/v1/personal/google/complete" => {
            let v: Completed = decode(bytes)?;
            if v.connection_id.is_nil()
                || v.epoch <= 0
                || !mailbox(&v.mailbox)
                || !hex64(&v.flow_id)
                || v.continuation
                    .as_ref()
                    .is_some_and(|r| r.thread_id.is_nil() || r.request_id.is_nil())
            {
                return Err(invalid);
            }
            encode(&v)
        }
        "/api/v1/personal/google/inbox" => {
            #[derive(Deserialize, Serialize)]
            #[serde(untagged)]
            enum Answer {
                Live(Inbox),
                Saved(moosuri_store::MemberGoogleThreadRead),
            }
            let v: Answer = decode(bytes)?;
            if !match &v {
                Answer::Live(v) => v.valid(),
                Answer::Saved(v) => v.valid(),
            } {
                return Err(invalid);
            }
            encode(&v)
        }
        "/api/v1/personal/google/history" => {
            if bytes.len() > 1024 * 1024 {
                return Err(invalid);
            }
            let v: ReadHistory = serde_json::from_slice(bytes).map_err(|_| invalid)?;
            if v.reads.len() > 20 || v.reads.iter().any(|r| !r.valid()) {
                return Err(invalid);
            }
            encode(&v)
        }
        _ => Err(invalid),
    }
}

struct Failure(StatusCode, &'static str);
fn configuration_failure(code: &'static str) -> Failure {
    if code == "google_setup_required" {
        Failure(StatusCode::NOT_FOUND, code)
    } else {
        Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable")
    }
}
impl From<StoreError> for Failure {
    fn from(error: StoreError) -> Self {
        match error {
            StoreError::Unauthorized => Self(StatusCode::UNAUTHORIZED, "unauthorized"),
            StoreError::Forbidden => Self(StatusCode::FORBIDDEN, "forbidden"),
            StoreError::InvalidInput => Self(StatusCode::BAD_REQUEST, "invalid_input"),
            StoreError::NotFound => Self(StatusCode::NOT_FOUND, "not_found"),
            StoreError::Conflict => Self(StatusCode::CONFLICT, "conflict"),
            _ => Self(StatusCode::SERVICE_UNAVAILABLE, "unavailable"),
        }
    }
}
impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        response(
            self.0,
            serde_json::json!({"code":self.1}).to_string().into_bytes(),
        )
    }
}
fn response(status: StatusCode, bytes: Vec<u8>) -> Response {
    let mut reply = (status, bytes).into_response();
    reply.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("application/json"),
    );
    reply.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-store"),
    );
    reply
}

#[derive(Clone)]
struct Google {
    store: Store,
    owner: VolumeOwner,
    backend: Backend,
    serial: Arc<tokio::sync::Mutex<()>>,
}
#[derive(Clone)]
enum Backend {
    Broker {
        socket: PathBuf,
        uid: u32,
    },
    Local {
        config: PathBuf,
        proxy: Arc<crate::gws_relay::MemberProxy>,
    },
    #[cfg(test)]
    Fixture(TestTransport),
}
#[cfg(test)]
pub(crate) type TestTransport = Arc<
    dyn Fn(
            Vec<u8>,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Zeroizing<Vec<u8>>, &'static str>> + Send>,
        > + Send
        + Sync,
>;

#[derive(Serialize)]
struct Owner {
    account_id: Uuid,
    tenant_id: Uuid,
    volume_id: Uuid,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerOwner {
    account_id: Uuid,
    tenant_id: Uuid,
    volume_id: Uuid,
    generation: i64,
    peer_uid: u32,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BrokerReply<T> {
    v: u32,
    request_id: Uuid,
    owner: BrokerOwner,
    app_revision: Uuid,
    state: String,
    result: T,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Description {
    client_id: String,
    redirect_uri: String,
    scope: String,
    operations: Vec<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Authorization {
    authorization_url: String,
}
struct Secret<'a>(&'a SecretString);
impl Serialize for Secret<'_> {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.0.expose())
    }
}
#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Operation<'a> {
    Describe,
    AuthorizationUrl {
        state: &'a str,
        verifier: Secret<'a>,
    },
    Exchange {
        code: Secret<'a>,
        verifier: Secret<'a>,
    },
    Refresh {
        refresh_token: Secret<'a>,
        refresh_binding: &'a str,
    },
    InboxPage {
        access_token: Secret<'a>,
        access_binding: &'a str,
    },
}
#[derive(Serialize)]
struct BrokerRequest<'a> {
    v: u32,
    request_id: Uuid,
    owner: Owner,
    generation: u64,
    app_revision: Option<Uuid>,
    operation: &'a Operation<'a>,
}
impl Google {
    async fn user(&self, headers: &HeaderMap) -> Result<UserContext, Failure> {
        let user = moosuri_server::personal_request_user(&self.store, headers).await?;
        if user.account().id != self.owner.account.to_string() {
            return Err(StoreError::Forbidden.into());
        }
        Ok(user)
    }
    async fn raw(
        &self,
        id: Uuid,
        revision: Option<Uuid>,
        operation: Operation<'_>,
    ) -> Result<Zeroizing<Vec<u8>>, Failure> {
        let broker_request = |operation| {
            serde_json::to_vec(&BrokerRequest {
                v: 1,
                request_id: id,
                owner: Owner {
                    account_id: self.owner.account,
                    tenant_id: self.owner.tenant,
                    volume_id: self.owner.volume,
                },
                generation: self.owner.generation,
                app_revision: revision,
                operation,
            })
            .map(Zeroizing::new)
            .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))
        };
        let unavailable = |_| Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable");
        let reply = match &self.backend {
            Backend::Local { config, proxy } => {
                let local = local_gws::LocalGws::load(
                    config,
                    std::path::Path::new("/usr/local/bin/gws"),
                    proxy.address(),
                )
                .map_err(configuration_failure)?;
                local.raw(self.owner, id, revision, operation).await?
            }
            Backend::Broker { socket, uid } => {
                crate::runtime_google::personal_call(socket, *uid, &broker_request(&operation)?)
                    .await
                    .map_err(unavailable)?
            }
            #[cfg(test)]
            Backend::Fixture(call) => call(broker_request(&operation)?.to_vec())
                .await
                .map_err(unavailable)?,
        };
        let carrier: BrokerReply<serde::de::IgnoredAny> = decode(&reply)
            .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable"))?;
        if carrier.v != 1
            || carrier.request_id != id
            || carrier.state != "ok"
            || carrier.owner.account_id != self.owner.account
            || carrier.owner.tenant_id != self.owner.tenant
            || carrier.owner.volume_id != self.owner.volume
            || carrier.owner.generation != self.owner.generation as i64
            || carrier.owner.peer_uid != unsafe { libc::geteuid() }
            || carrier.app_revision.is_nil()
            || revision.is_some_and(|r| carrier.app_revision != r)
        {
            return Err(Failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "google_unavailable",
            ));
        }
        Ok(reply)
    }
    async fn call<T: DeserializeOwned>(
        &self,
        revision: Option<Uuid>,
        operation: Operation<'_>,
    ) -> Result<BrokerReply<T>, Failure> {
        let bytes = self.raw(Uuid::new_v4(), revision, operation).await?;
        decode(&bytes).map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable"))
    }
    async fn describe(&self) -> Result<BrokerReply<Description>, Failure> {
        let reply: BrokerReply<Description> = self.call(None, Operation::Describe).await?;
        let app = &reply.result;
        let redirect = url::Url::parse(&app.redirect_uri)
            .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable"))?;
        if app.client_id.is_empty()
            || app.client_id.len() > 1024
            || app.client_id.chars().any(char::is_control)
            || redirect.scheme() != "https"
            || redirect.path() != "/api/v1/mail/oauth/callback"
            || redirect.query().is_some()
            || redirect.fragment().is_some()
            || !redirect.username().is_empty()
            || redirect.password().is_some()
            || app.scope != READ_SCOPE
            || app.operations != ["authorization_url", "exchange", "refresh", "inbox_page"]
        {
            return Err(Failure(
                StatusCode::SERVICE_UNAVAILABLE,
                "google_unavailable",
            ));
        }
        Ok(reply)
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StartInput {
    #[serde(default)]
    read: Option<moosuri_store::MemberGoogleReadIntent>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadSelected {
    connection_id: Uuid,
    epoch: i64,
    #[serde(default)]
    read: Option<moosuri_store::MemberGoogleReadIntent>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryInput {
    thread_id: Uuid,
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReadHistory {
    reads: Vec<moosuri_store::MemberGoogleThreadRead>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Handle {
    handle: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selected {
    connection_id: Uuid,
    epoch: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    state: String,
    code: Option<SecretString>,
}

pub fn routes(store: Store, owner: VolumeOwner, socket: PathBuf, broker_uid: u32) -> Router {
    router(Google {
        store,
        owner,
        backend: Backend::Broker {
            socket,
            uid: broker_uid,
        },
        serial: Default::default(),
    })
}
pub fn local_routes(
    store: Store,
    owner: VolumeOwner,
    config: PathBuf,
    proxy: crate::gws_relay::MemberProxy,
) -> Router {
    router(Google {
        store,
        owner,
        backend: Backend::Local {
            config,
            proxy: Arc::new(proxy),
        },
        serial: Default::default(),
    })
}
#[cfg(test)]
pub(crate) fn routes_with_test_transport(
    store: Store,
    owner: VolumeOwner,
    transport: TestTransport,
) -> Router {
    router(Google {
        store,
        owner,
        backend: Backend::Fixture(transport),
        serial: Default::default(),
    })
}
/// Deterministic private broker fixture for gateway/session acceptance only.
fn router(state: Google) -> Router {
    Router::new()
        .route(BASE, get(status))
        .route("/api/v1/personal/google/start", post(start))
        .route("/internal/google/capture", post(capture))
        .route("/api/v1/personal/google/complete", post(complete))
        .route("/api/v1/personal/google/inbox", post(inbox))
        .route("/api/v1/personal/google/history", post(thread_history))
        .route("/api/v1/personal/google/disconnect", post(disconnect))
        .layer(DefaultBodyLimit::max(8192))
        .with_state(state)
}
fn browser<T: Serialize>(path: &str, value: &T) -> Result<Response, Failure> {
    let bytes = encode(value).map_err(|s| Failure(s, "unavailable"))?;
    let bytes = public_response(
        path,
        &if path == BASE {
            Method::GET
        } else {
            Method::POST
        },
        StatusCode::OK,
        &bytes,
    )
    .map_err(|s| Failure(s, "unavailable"))?;
    Ok(response(StatusCode::OK, bytes))
}
async fn status(State(g): State<Google>, headers: HeaderMap) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    let status = if let Backend::Local { config, .. } = &g.backend {
        let (revision, _, _) = registered_client(config).map_err(configuration_failure)?;
        g.store
            .member_google_status(&user, g.owner, revision)
            .await?
    } else {
        g.store.member_google_status_local(&user, g.owner).await?
    };
    browser(
        BASE,
        &Status {
            connection: connection(status)?,
        },
    )
}
async fn start(
    State(g): State<Google>,
    headers: HeaderMap,
    Json(input): Json<StartInput>,
) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    let _serial = g
        .serial
        .try_lock()
        .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))?;
    let app = g.describe().await?;
    let pending = g
        .store
        .start_member_google_oauth(&user, g.owner, app.app_revision)
        .await?;
    if let Some(read) = input.read {
        if let Err(e) = g
            .store
            .bind_member_google_read_oauth(
                &user,
                g.owner,
                app.app_revision,
                pending.state.clone(),
                read,
            )
            .await
        {
            let _ = g.store.cancel_mail_oauth_state(pending.state.clone()).await;
            return Err(e.into());
        }
    }
    let verifier = SecretString::new(pending.verifier.to_string())
        .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))?;
    let answer: BrokerReply<Authorization> = g
        .call(
            Some(app.app_revision),
            Operation::AuthorizationUrl {
                state: &pending.state,
                verifier: Secret(&verifier),
            },
        )
        .await?;
    // The session is rechecked after the broker; no revoked login gets a URL.
    g.user(&headers).await?;
    let value = Started {
        authorization_url: answer.result.authorization_url,
        flow_id: pending.flow_id(),
    };
    let url = url::Url::parse(&value.authorization_url)
        .map_err(|_| Failure(StatusCode::SERVICE_UNAVAILABLE, "google_unavailable"))?;
    let fields: std::collections::BTreeMap<_, _> = url.query_pairs().into_owned().collect();
    if fields.get("client_id") != Some(&app.result.client_id)
        || fields.get("redirect_uri") != Some(&app.result.redirect_uri)
        || fields.get("state") != Some(&pending.state)
    {
        return Err(Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "google_unavailable",
        ));
    }
    browser("/api/v1/personal/google/start", &value)
}
async fn capture(State(g): State<Google>, Json(input): Json<Capture>) -> Result<Response, Failure> {
    if !hex64(&input.state) {
        return Err(StoreError::InvalidInput.into());
    }
    let result = if let Some(code) = input.code {
        if code.expose().len() > 4096 || code.expose().chars().any(char::is_control) {
            return Err(StoreError::InvalidInput.into());
        }
        Some(g.store.capture_mail_oauth(input.state, code).await?)
    } else {
        g.store.cancel_mail_oauth_state(input.state).await?;
        None
    };
    Ok(response(
        StatusCode::OK,
        serde_json::json!({"handle":result})
            .to_string()
            .into_bytes(),
    ))
}
async fn complete(
    State(g): State<Google>,
    headers: HeaderMap,
    Json(input): Json<Handle>,
) -> Result<Response, Failure> {
    if !hex64(&input.handle) {
        return Err(StoreError::InvalidInput.into());
    }
    let user = g.user(&headers).await?;
    let _serial = g
        .serial
        .try_lock()
        .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))?;
    let app = g.describe().await?;
    let exchange = g
        .store
        .claim_member_google_oauth(&user, g.owner, app.app_revision, input.handle)
        .await?;
    let flow_id = exchange.flow_id();
    let id = exchange
        .member_google_request_id()
        .ok_or(Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))?;
    let raw = g
        .raw(
            id,
            Some(app.app_revision),
            Operation::Exchange {
                code: Secret(&exchange.code),
                verifier: Secret(&exchange.verifier),
            },
        )
        .await?;
    let grant = moosuri_store::MemberGoogleGrant::from_broker_reply(id, raw)?;
    let settled = g
        .store
        .complete_member_google_oauth(exchange, grant)
        .await?;
    let continuation = g
        .store
        .member_google_read_continuation(&user, g.owner, flow_id.clone())
        .await?;
    let c = connection(settled)?.ok_or(Failure(StatusCode::CONFLICT, "conflict"))?;
    browser(
        "/api/v1/personal/google/complete",
        &Completed {
            connection_id: c.connection_id,
            epoch: c.epoch,
            mailbox: c.mailbox,
            flow_id,
            continuation,
        },
    )
}
async fn inbox(
    State(g): State<Google>,
    headers: HeaderMap,
    Json(input): Json<ReadSelected>,
) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    let _serial = g
        .serial
        .try_lock()
        .map_err(|_| Failure(StatusCode::CONFLICT, "conflict"))?;
    let app = g.describe().await?;
    let current = g
        .store
        .member_google_status(&user, g.owner, app.app_revision)
        .await?;
    let refresh = current.needs_refresh;
    let chosen = Selected {
        connection_id: input.connection_id,
        epoch: input.epoch,
    };
    selected(&chosen, connection(current)?.as_ref())?;
    let lease = if let Some(read) = input.read {
        match g
            .store
            .begin_member_google_thread_read(
                &user,
                g.owner,
                app.app_revision,
                read,
                input.connection_id,
                input.epoch,
            )
            .await?
        {
            moosuri_store::MemberGoogleReadAdmission::Existing(saved) => {
                return browser("/api/v1/personal/google/inbox", &saved);
            }
            moosuri_store::MemberGoogleReadAdmission::Fresh(lease) => Some(lease),
        }
    } else {
        None
    };
    match live_inbox(&g, &user, app.app_revision, &chosen, refresh).await {
        Ok(page) => {
            if let Some(lease) = lease {
                let saved = g
                    .store
                    .settle_member_google_thread_read(lease, Some(page))
                    .await?;
                browser("/api/v1/personal/google/inbox", &saved)
            } else {
                browser("/api/v1/personal/google/inbox", &page)
            }
        }
        Err(error) => {
            if let Some(lease) = lease {
                let _ = g.store.settle_member_google_thread_read(lease, None).await;
            }
            Err(error)
        }
    }
}
async fn thread_history(
    State(g): State<Google>,
    headers: HeaderMap,
    Json(input): Json<HistoryInput>,
) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    let reads = g
        .store
        .member_google_thread_history(&user, g.owner, input.thread_id)
        .await?;
    browser("/api/v1/personal/google/history", &ReadHistory { reads })
}
async fn live_inbox(
    g: &Google,
    user: &UserContext,
    revision: Uuid,
    input: &Selected,
    refresh: bool,
) -> Result<Inbox, Failure> {
    if refresh {
        let lease = g
            .store
            .claim_member_google_refresh(user, g.owner, revision)
            .await?;
        let id = lease.request_id;
        let attempt = g
            .raw(
                id,
                Some(revision),
                Operation::Refresh {
                    refresh_token: Secret(&lease.refresh_token),
                    refresh_binding: &lease.refresh_binding,
                },
            )
            .await
            .and_then(|raw| {
                moosuri_store::MemberGoogleGrant::from_broker_reply(id, raw).map_err(Failure::from)
            });
        let updated = match attempt {
            Ok(grant) => {
                g.store
                    .settle_member_google_refresh(lease, Some(grant))
                    .await?
            }
            Err(error) => {
                // Rotation may have occurred upstream. Withdraw this grant;
                // never retry an unknown refresh or keep using its old token.
                let _ = g.store.settle_member_google_refresh(lease, None).await;
                return Err(error);
            }
        };
        selected(input, connection(updated)?.as_ref())?;
    }
    let binding = g
        .store
        .member_google_read_binding(user, g.owner, revision)
        .await?;
    selected(
        input,
        Some(&Connection {
            connection_id: binding.connection_id,
            epoch: binding.epoch,
            mailbox: binding.mailbox.clone(),
        }),
    )?;
    let page: BrokerReply<Inbox> = g
        .call(
            Some(revision),
            Operation::InboxPage {
                access_token: Secret(&binding.access_token),
                access_binding: &binding.access_binding,
            },
        )
        .await?;
    g.store.check_member_google_read(&binding).await?;
    if !page.result.mailbox.eq_ignore_ascii_case(&binding.mailbox) {
        return Err(Failure(
            StatusCode::SERVICE_UNAVAILABLE,
            "google_unavailable",
        ));
    }
    Ok(page.result)
}
async fn disconnect(
    State(g): State<Google>,
    headers: HeaderMap,
    Json(input): Json<Selected>,
) -> Result<Response, Failure> {
    let user = g.user(&headers).await?;
    // Do not serialize behind provider IO: withdrawal must fence a late read.
    if input.connection_id.is_nil() || input.epoch <= 0 {
        return Err(StoreError::InvalidInput.into());
    }
    g.store
        .disconnect_member_google(&user, g.owner, input.connection_id, input.epoch)
        .await?;
    Ok(response(StatusCode::NO_CONTENT, Vec::new()))
}
fn selected(input: &Selected, current: Option<&Connection>) -> Result<(), Failure> {
    if input.connection_id.is_nil() || input.epoch <= 0 {
        return Err(StoreError::InvalidInput.into());
    }
    if current.is_none_or(|c| c.connection_id != input.connection_id || c.epoch != input.epoch) {
        return Err(StoreError::Conflict.into());
    }
    Ok(())
}
fn connection(status: moosuri_store::MemberGoogleStatus) -> Result<Option<Connection>, Failure> {
    if !status.read_only || status.send_enabled {
        return Err(Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"));
    }
    if !status.connected {
        return Ok(None);
    }
    let c = Connection {
        connection_id: status
            .connection_id
            .ok_or(Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))?,
        epoch: status.epoch,
        mailbox: status
            .mailbox
            .ok_or(Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"))?,
    };
    if status.status != "active" || !c.valid() {
        return Err(Failure(StatusCode::SERVICE_UNAVAILABLE, "unavailable"));
    }
    Ok(Some(c))
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
    use moosuri_connectors::gmail::GmailClient;
    use serde_json::json;
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        sync::atomic::{AtomicBool, AtomicUsize, Ordering},
        time::{SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::Notify;
    use tower::ServiceExt;

    #[derive(Default)]
    pub(super) struct Fake {
        unavailable: AtomicBool,
        reads: AtomicUsize,
        refreshes: AtomicUsize,
        hold: AtomicBool,
        entered: Notify,
        release: Notify,
        fail_read: AtomicBool,
    }
    fn receipt(
        owner: &serde_json::Value,
        revision: Uuid,
        purpose: &str,
        mailbox: &str,
        token: &str,
        lifetime: u64,
    ) -> String {
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &[41; 32]);
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let payload=URL_SAFE_NO_PAD.encode(serde_json::to_vec(&json!({"v":1,"owner":owner,"app_revision":revision,"purpose":purpose,"mailbox":mailbox,"token_tag":URL_SAFE_NO_PAD.encode(ring::hmac::sign(&key,token.as_bytes()).as_ref()),"expires_at":now+lifetime})).unwrap());
        format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(ring::hmac::sign(&key, payload.as_bytes()).as_ref())
        )
    }
    fn transport(fake: Arc<Fake>, revision: Uuid) -> TestTransport {
        transport_for(
            fake,
            revision,
            "a@example.test".into(),
            "https://assistant.example.test/api/v1/mail/oauth/callback".into(),
        )
    }
    pub(super) fn transport_for(
        fake: Arc<Fake>,
        revision: Uuid,
        mailbox: String,
        redirect_uri: String,
    ) -> TestTransport {
        Arc::new(move |bytes| {
            let fake = fake.clone();
            let mailbox = mailbox.clone();
            let redirect_uri = redirect_uri.clone();
            Box::pin(async move {
                if fake.unavailable.load(Ordering::SeqCst) {
                    return Err("synthetic_unavailable");
                }
                let r: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                let op = &r["operation"];
                let mut owner = r["owner"].clone();
                owner["generation"] = r["generation"].clone();
                owner["peer_uid"] = json!(unsafe { libc::geteuid() });
                let mailbox = mailbox.as_str();
                let result = match op["kind"].as_str().unwrap() {
                    "describe" => {
                        json!({"client_id":"synthetic-client","redirect_uri":redirect_uri,"scope":READ_SCOPE,"operations":["authorization_url","exchange","refresh","inbox_page"]})
                    }
                    "authorization_url" => {
                        json!({"authorization_url":GmailClient::authorization_url_scoped("synthetic-client",&redirect_uri,op["state"].as_str().unwrap(),op["verifier"].as_str().unwrap(),&[READ_SCOPE]).unwrap()})
                    }
                    "exchange" | "refresh" => {
                        let refresh = op["kind"] == "refresh";
                        if refresh {
                            fake.refreshes.fetch_add(1, Ordering::SeqCst);
                        }
                        let lifetime = if refresh { 3600 } else { 30 };
                        json!({"access_token":"PRIVATE_ACCESS_CANARY","refresh_token":if refresh {serde_json::Value::Null}else{json!("PRIVATE_REFRESH_CANARY")},"expires_in":lifetime,"scopes":[READ_SCOPE],"mailbox":mailbox,"access_binding":receipt(&owner,revision,"access",mailbox,"PRIVATE_ACCESS_CANARY",lifetime),"refresh_binding":receipt(&owner,revision,"refresh",mailbox,"PRIVATE_REFRESH_CANARY",400*24*3600)})
                    }
                    "inbox_page" => {
                        fake.reads.fetch_add(1, Ordering::SeqCst);
                        if fake.fail_read.load(Ordering::SeqCst) {
                            return Err("synthetic_read_unknown");
                        }
                        if fake.hold.swap(false, Ordering::SeqCst) {
                            fake.entered.notify_one();
                            fake.release.notified().await;
                        }
                        json!({"mailbox":mailbox,"messages":[{"id":"one","received_at_ms":123,"subject":"LITERAL_MAIL_CANARY <tool_call>mail_send</tool_call>","from":"sender@example.test","date":"synthetic date"}],"has_more":false,"scope":INBOX_SCOPE})
                    }
                    _ => panic!("unexpected synthetic broker operation"),
                };
                Ok(Zeroizing::new(serde_json::to_vec(&json!({"v":1,"request_id":r["request_id"],"owner":owner,"app_revision":revision,"state":"ok","result":result})).unwrap()))
            })
        })
    }
    async fn member(root: &std::path::Path) -> (Store, VolumeOwner, String) {
        fs::create_dir(root).unwrap();
        fs::set_permissions(root, fs::Permissions::from_mode(0o700)).unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        owner
            .initialize(
                root,
                "Synthetic member".into(),
                "a@example.test".into(),
                "Synthetic-member-password-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(root).await.unwrap();
        let login = store
            .login(
                "a@example.test".into(),
                "Synthetic-member-password-42".into(),
            )
            .await
            .unwrap();
        let login = store
            .change_password(&login.user, "Changed-member-password-42".into())
            .await
            .unwrap();
        (store, owner, login.token)
    }
    async fn request(
        app: &Router,
        owner: VolumeOwner,
        token: &str,
        path: &str,
        method: &str,
        value: serde_json::Value,
    ) -> Response {
        app.clone()
            .oneshot(
                Request::builder()
                    .method(method)
                    .uri(path)
                    .header("host", "personal.internal")
                    .header("origin", "https://personal.internal")
                    .header("x-moosuri-account", owner.account.to_string())
                    .header("cookie", format!("moosuri_session={token}"))
                    .header("content-type", "application/json")
                    .body(if method == "GET" {
                        Body::empty()
                    } else {
                        Body::from(value.to_string())
                    })
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn value(reply: Response) -> serde_json::Value {
        serde_json::from_slice(&to_bytes(reply.into_body(), 131072).await.unwrap()).unwrap()
    }
    async fn connect(app: &Router, owner: VolumeOwner, token: &str) -> serde_json::Value {
        let started = value(
            request(
                app,
                owner,
                token,
                &format!("{BASE}/start"),
                "POST",
                json!({}),
            )
            .await,
        )
        .await;
        let url = url::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let captured = value(
            request(
                app,
                owner,
                token,
                "/internal/google/capture",
                "POST",
                json!({"state":state,"code":"synthetic-code"}),
            )
            .await,
        )
        .await;
        let reply = request(
            app,
            owner,
            token,
            &format!("{BASE}/complete"),
            "POST",
            json!({"handle":captured["handle"]}),
        )
        .await;
        assert_eq!(reply.status(), StatusCode::OK);
        let v = value(reply).await;
        assert_eq!(v["flow_id"], started["flow_id"]);
        v
    }
    async fn thread_connect(
        app: &Router,
        owner: VolumeOwner,
        token: &str,
        read: &serde_json::Value,
    ) -> serde_json::Value {
        let started = value(
            request(
                app,
                owner,
                token,
                &format!("{BASE}/start"),
                "POST",
                json!({"read":read}),
            )
            .await,
        )
        .await;
        let url = url::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let captured = value(
            request(
                app,
                owner,
                token,
                "/internal/google/capture",
                "POST",
                json!({"state":state,"code":"synthetic-code"}),
            )
            .await,
        )
        .await;
        let reply = request(
            app,
            owner,
            token,
            &format!("{BASE}/complete"),
            "POST",
            json!({"handle":captured["handle"]}),
        )
        .await;
        assert_eq!(reply.status(), StatusCode::OK);
        let v = value(reply).await;
        assert_eq!(v["continuation"], *read);
        v
    }
    #[tokio::test]
    async fn unregistered_member_can_open_chat_and_sees_setup_status_only_after_login() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("member");
        let (store, owner, token) = member(&root).await;
        let config = owner
            .verified_member_directory(&root.canonicalize().unwrap())
            .unwrap()
            .join(moosuri_store::personal_volume::GWS_DIRECTORY)
            .join("gws-client.json");
        let proxy = crate::gws_relay::start_member("/unused-fixture.sock".into(), 0)
            .await
            .unwrap();
        let app = local_routes(store.clone(), owner, config, proxy);
        let user = store.authenticate(token.clone()).await.unwrap();
        assert!(
            store
                .create_thread(&user, "First chat".into())
                .await
                .is_ok()
        );
        let result = request(&app, owner, &token, BASE, "GET", json!({})).await;
        assert_eq!(result.status(), StatusCode::NOT_FOUND);
        assert_eq!(value(result).await["code"], "google_setup_required");
        assert_eq!(
            request(&app, owner, "other-session", BASE, "GET", json!({}))
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    #[tokio::test]
    async fn thread_read_is_encrypted_durable_idempotent_and_outside_model_history() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("member");
        let (store, owner, token) = member(&root).await;
        let user = store.authenticate(token.clone()).await.unwrap();
        let thread = store
            .create_thread(&user, "Original thread".into())
            .await
            .unwrap();
        let fake = Arc::new(Fake::default());
        let revision = Uuid::new_v4();
        let app =
            routes_with_test_transport(store.clone(), owner, transport(fake.clone(), revision));
        let read = json!({"thread_id":thread.id,"request_id":Uuid::new_v4()});
        let grant = thread_connect(&app, owner, &token, &read).await;
        assert_eq!(fake.reads.load(Ordering::SeqCst), 0);
        let input =
            json!({"connection_id":grant["connection_id"],"epoch":grant["epoch"],"read":read});
        let first = request(
            &app,
            owner,
            &token,
            &format!("{BASE}/inbox"),
            "POST",
            input.clone(),
        )
        .await;
        assert_eq!(first.status(), StatusCode::OK);
        let first = value(first).await;
        assert_eq!(first["state"], "completed");
        assert_eq!(first["thread_id"], thread.id);
        assert_eq!(
            value(request(&app, owner, &token, &format!("{BASE}/inbox"), "POST", input).await)
                .await,
            first
        );
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
        assert!(
            store
                .conversation(&user, thread.id.clone(), None)
                .await
                .unwrap()
                .turns
                .is_empty()
        );
        drop(app);
        drop(store);
        let store = owner.open_verified(&root).await.unwrap();
        let user = store.authenticate(token.clone()).await.unwrap();
        let history = store
            .member_google_thread_history(&user, owner, thread.id.parse().unwrap())
            .await
            .unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].state, "completed");
        assert!(
            history[0].result.as_ref().unwrap().messages[0]
                .subject
                .contains("LITERAL_MAIL_CANARY")
        );
        let db = std::fs::read_dir(&root)
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("assistant.sqlite");
        let bytes = std::fs::read(db).unwrap();
        assert!(
            !bytes
                .windows(b"LITERAL_MAIL_CANARY".len())
                .any(|v| v == b"LITERAL_MAIL_CANARY")
        );
        let app =
            routes_with_test_transport(store.clone(), owner, transport(fake.clone(), revision));
        let reconnected = connect(&app, owner, &token).await;
        let replaced = store
            .member_google_thread_history(&user, owner, thread.id.parse().unwrap())
            .await
            .unwrap();
        assert_eq!(replaced[0].state, "withdrawn");
        assert!(replaced[0].result.is_none());
        store
            .disconnect_member_google(
                &user,
                owner,
                reconnected["connection_id"]
                    .as_str()
                    .unwrap()
                    .parse()
                    .unwrap(),
                reconnected["epoch"].as_i64().unwrap(),
            )
            .await
            .unwrap();
        let withdrawn = store
            .member_google_thread_history(&user, owner, thread.id.parse().unwrap())
            .await
            .unwrap();
        assert_eq!(withdrawn[0].state, "withdrawn");
        assert!(withdrawn[0].result.is_none());
        assert!(withdrawn[0].source.is_none());
        store.delete_thread(&user, thread.id.clone()).await.unwrap();
        assert!(
            store
                .member_google_thread_history(&user, owner, thread.id.parse().unwrap())
                .await
                .is_err()
        );
    }
    #[tokio::test]
    async fn restart_retires_a_dispatched_read_without_a_second_provider_read() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().join("member");
        let (store, owner, token) = member(&root).await;
        let user = store.authenticate(token.clone()).await.unwrap();
        let thread = store.create_thread(&user, "Original".into()).await.unwrap();
        let fake = Arc::new(Fake::default());
        let revision = Uuid::new_v4();
        let app =
            routes_with_test_transport(store.clone(), owner, transport(fake.clone(), revision));
        let grant = connect(&app, owner, &token).await;
        let read = moosuri_store::MemberGoogleReadIntent {
            thread_id: thread.id.parse().unwrap(),
            request_id: Uuid::new_v4(),
        };
        let admission = store
            .begin_member_google_thread_read(
                &user,
                owner,
                revision,
                read.clone(),
                grant["connection_id"].as_str().unwrap().parse().unwrap(),
                grant["epoch"].as_i64().unwrap(),
            )
            .await
            .unwrap();
        assert!(matches!(
            &admission,
            moosuri_store::MemberGoogleReadAdmission::Fresh(_)
        ));
        drop(admission);
        drop(app);
        drop(store);
        let store = owner.open_verified(&root).await.unwrap();
        let app =
            routes_with_test_transport(store.clone(), owner, transport(fake.clone(), revision));
        let input =
            json!({"connection_id":grant["connection_id"],"epoch":grant["epoch"],"read":read});
        let reply = request(&app, owner, &token, &format!("{BASE}/inbox"), "POST", input).await;
        assert_eq!(reply.status(), StatusCode::OK);
        assert_eq!(value(reply).await["state"], "unknown");
        assert_eq!(fake.reads.load(Ordering::SeqCst), 0);
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 0);
    }
    #[tokio::test]
    async fn uncertain_thread_read_never_dispatches_twice_and_manual_connect_has_no_continuation() {
        let temp = tempfile::tempdir().unwrap();
        let (store, owner, token) = member(&temp.path().join("member")).await;
        let user = store.authenticate(token.clone()).await.unwrap();
        let thread = store.create_thread(&user, "Original".into()).await.unwrap();
        let fake = Arc::new(Fake::default());
        let app = routes_with_test_transport(
            store.clone(),
            owner,
            transport(fake.clone(), Uuid::new_v4()),
        );
        let grant = connect(&app, owner, &token).await;
        assert!(grant.get("continuation").is_none());
        fake.fail_read.store(true, Ordering::SeqCst);
        let input = json!({"connection_id":grant["connection_id"],"epoch":grant["epoch"],"read":{"thread_id":thread.id,"request_id":Uuid::new_v4()}});
        assert_eq!(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/inbox"),
                "POST",
                input.clone()
            )
            .await
            .status(),
            StatusCode::SERVICE_UNAVAILABLE
        );
        fake.fail_read.store(false, Ordering::SeqCst);
        let second = request(
            &app,
            owner,
            &token,
            &format!("{BASE}/inbox"),
            "POST",
            input.clone(),
        )
        .await;
        assert_eq!(second.status(), StatusCode::OK);
        assert_eq!(value(second).await["state"], "unknown");
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
        let other = temp.path().join("other");
        let (other_store, other_owner, other_token) = member(&other).await;
        let other_user = other_store.authenticate(other_token).await.unwrap();
        assert!(
            other_store
                .member_google_thread_history(&other_user, other_owner, thread.id.parse().unwrap())
                .await
                .is_err()
        );
        let history = value(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/history"),
                "POST",
                json!({"thread_id":thread.id}),
            )
            .await,
        )
        .await;
        assert_eq!(history["reads"][0]["state"], "unknown");
    }

    #[tokio::test]
    async fn complete_immediate_status_refresh_read_and_brokerless_disconnect() {
        let temp = tempfile::tempdir().unwrap();
        let (store, owner, token) = member(&temp.path().join("member")).await;
        let fake = Arc::new(Fake::default());
        let app = routes_with_test_transport(
            store.clone(),
            owner,
            transport(fake.clone(), Uuid::new_v4()),
        );
        let connected = connect(&app, owner, &token).await;
        let selected =
            json!({"connection_id":connected["connection_id"],"epoch":connected["epoch"]});
        fake.unavailable.store(true, Ordering::SeqCst);
        let status = request(&app, owner, &token, BASE, "GET", json!({})).await;
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(
            value(status).await["connection"]["connection_id"],
            connected["connection_id"]
        );
        fake.unavailable.store(false, Ordering::SeqCst);
        let read = request(
            &app,
            owner,
            &token,
            &format!("{BASE}/inbox"),
            "POST",
            selected.clone(),
        )
        .await;
        assert_eq!(read.status(), StatusCode::OK);
        let read = value(read).await;
        assert_eq!(fake.refreshes.load(Ordering::SeqCst), 1);
        assert_eq!(fake.reads.load(Ordering::SeqCst), 1);
        assert!(
            read["messages"][0]["subject"]
                .as_str()
                .unwrap()
                .contains("LITERAL_MAIL_CANARY")
        );
        assert!(!read.to_string().contains("PRIVATE_"));
        fake.unavailable.store(true, Ordering::SeqCst);
        assert_eq!(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/disconnect"),
                "POST",
                selected
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert!(
            value(request(&app, owner, &token, BASE, "GET", json!({})).await).await["connection"]
                .is_null()
        );
        assert!(
            store
                .mail_connections(&store.authenticate(token).await.unwrap())
                .await
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn disconnect_fences_late_inbox_and_foreign_session_cannot_read() {
        let temp = tempfile::tempdir().unwrap();
        let (store, owner, token) = member(&temp.path().join("member")).await;
        let fake = Arc::new(Fake::default());
        let app = routes_with_test_transport(store, owner, transport(fake.clone(), Uuid::new_v4()));
        let connected = connect(&app, owner, &token).await;
        let selected =
            json!({"connection_id":connected["connection_id"],"epoch":connected["epoch"]});
        assert_eq!(
            request(
                &app,
                owner,
                "foreign-token",
                &format!("{BASE}/inbox"),
                "POST",
                selected.clone()
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(fake.reads.load(Ordering::SeqCst), 0);
        fake.hold.store(true, Ordering::SeqCst);
        let a = app.clone();
        let session = token.clone();
        let input = selected.clone();
        let pending = tokio::spawn(async move {
            request(&a, owner, &session, &format!("{BASE}/inbox"), "POST", input).await
        });
        tokio::time::timeout(std::time::Duration::from_secs(3), fake.entered.notified())
            .await
            .unwrap();
        assert_eq!(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/disconnect"),
                "POST",
                selected
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        fake.release.notify_one();
        let reply = pending.await.unwrap();
        assert_eq!(reply.status(), StatusCode::CONFLICT);
        assert!(
            !String::from_utf8(to_bytes(reply.into_body(), 131072).await.unwrap().to_vec())
                .unwrap()
                .contains("LITERAL_MAIL_CANARY")
        );
    }

    #[tokio::test]
    async fn callback_handle_requires_original_session_and_old_disconnect_cannot_remove_reconnect()
    {
        let temp = tempfile::tempdir().unwrap();
        let (store, owner, token) = member(&temp.path().join("member")).await;
        let fake = Arc::new(Fake::default());
        let app = routes_with_test_transport(store.clone(), owner, transport(fake, Uuid::new_v4()));
        let started = value(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/start"),
                "POST",
                json!({}),
            )
            .await,
        )
        .await;
        let url = url::Url::parse(started["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let captured = value(
            request(
                &app,
                owner,
                &token,
                "/internal/google/capture",
                "POST",
                json!({"state":state,"code":"synthetic-code"}),
            )
            .await,
        )
        .await;
        let second = store
            .login("a@example.test".into(), "Changed-member-password-42".into())
            .await
            .unwrap();
        let handle = json!({"handle":captured["handle"]});
        assert_eq!(
            request(
                &app,
                owner,
                &second.token,
                &format!("{BASE}/complete"),
                "POST",
                handle.clone()
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        let original = request(
            &app,
            owner,
            &token,
            &format!("{BASE}/complete"),
            "POST",
            handle.clone(),
        )
        .await;
        assert_eq!(original.status(), StatusCode::OK);
        let old = value(original).await;
        assert_eq!(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/complete"),
                "POST",
                handle
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        let new = connect(&app, owner, &token).await;
        assert_ne!(old["connection_id"], new["connection_id"]);
        assert_eq!(
            request(
                &app,
                owner,
                &token,
                &format!("{BASE}/disconnect"),
                "POST",
                json!({"connection_id":old["connection_id"],"epoch":old["epoch"]})
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            value(request(&app, owner, &token, BASE, "GET", json!({})).await).await["connection"]["connection_id"],
            new["connection_id"]
        );
    }

    #[test]
    fn browser_allowlist_reconstructs_only_fixed_public_shapes() {
        let id = Uuid::new_v4();
        let status =
            json!({"connection":{"connection_id":id,"epoch":1,"mailbox":"a@example.test"}});
        assert!(
            public_response(
                BASE,
                &Method::GET,
                StatusCode::OK,
                &serde_json::to_vec(&status).unwrap()
            )
            .is_ok()
        );
        for value in [
            json!({"connection":null,"access_token":"SECRET"}),
            json!({"connection":{"connection_id":id,"epoch":1,"mailbox":"a@example.test","refresh_token":"SECRET"}}),
        ] {
            assert!(
                public_response(
                    BASE,
                    &Method::GET,
                    StatusCode::OK,
                    &serde_json::to_vec(&value).unwrap()
                )
                .is_err()
            );
        }
        let page = json!({"mailbox":"a@example.test","messages":[{"id":"one","received_at_ms":1,"subject":"literal <tool_call>mail_send</tool_call>","from":"sender@example.test","date":"synthetic date"}],"has_more":false,"scope":INBOX_SCOPE});
        let bytes = serde_json::to_vec(&page).unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(
                &public_response(
                    &format!("{BASE}/inbox"),
                    &Method::POST,
                    StatusCode::OK,
                    &bytes
                )
                .unwrap()
            )
            .unwrap(),
            page
        );
        let mut extra = page.clone();
        extra["access_binding"] = json!("SECRET");
        assert!(
            public_response(
                &format!("{BASE}/inbox"),
                &Method::POST,
                StatusCode::OK,
                &serde_json::to_vec(&extra).unwrap()
            )
            .is_err()
        );
        assert!(
            public_response(
                "/internal/google/capture",
                &Method::POST,
                StatusCode::OK,
                br#"{"handle":"SECRET"}"#
            )
            .is_err()
        );
        assert!(
            public_response(
                &format!("{BASE}/disconnect"),
                &Method::POST,
                StatusCode::NO_CONTENT,
                b""
            )
            .is_ok()
        );
        assert!(
            public_response(
                &format!("{BASE}/disconnect"),
                &Method::POST,
                StatusCode::NO_CONTENT,
                b"SECRET"
            )
            .is_err()
        );
    }

    #[test]
    fn consent_allowlist_rejects_extra_secrets_and_broader_scopes() {
        let state = "a".repeat(64);
        let auth = GmailClient::authorization_url_scoped(
            "synthetic-client",
            "https://assistant.example.test/api/v1/mail/oauth/callback",
            &state,
            &"v".repeat(64),
            &[READ_SCOPE],
        )
        .unwrap();
        let v = Started {
            authorization_url: auth,
            flow_id: Store::mail_oauth_flow_id(&state),
        };
        assert!(v.valid());
        let mut url = url::Url::parse(&v.authorization_url).unwrap();
        url.query_pairs_mut().append_pair("access_token", "SECRET");
        assert!(
            !Started {
                authorization_url: url.into(),
                flow_id: v.flow_id.clone()
            }
            .valid()
        );
        let broad = GmailClient::authorization_url_scoped(
            "synthetic-client",
            "https://assistant.example.test/api/v1/mail/oauth/callback",
            &state,
            &"v".repeat(64),
            &[READ_SCOPE, moosuri_connectors::gmail::SEND_SCOPE],
        )
        .unwrap();
        assert!(
            !Started {
                authorization_url: broad,
                flow_id: v.flow_id
            }
            .valid()
        );
    }
}
