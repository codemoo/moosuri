//! Opt-in private authentication gateway. TLS and trusted client-IP assignment
//! belong to the local front proxy; this module binds no TCP listener.
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use clap::Args;
use fs2::FileExt;
use moosuri_store::runtime_registry::{self, Allocation};
use ring::{
    digest, hmac,
    rand::{SecureRandom, SystemRandom},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    net::IpAddr,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use zeroize::Zeroizing;

mod admin;
mod google;
mod memory;

const ROUTER: &str = "__Host-moosuri_route";
const SESSION: &str = "moosuri_session";
const MAX_BODY: usize = 64 * 1024;
// The cookie locates a server session; Store separately enforces sliding expiry.
const MAX_AGE: u64 = moosuri_server::COOKIE_MAX_AGE;
const MAX_THREAD_BODY: usize = 32 * 1024 * 1024;
const WINDOW: Duration = Duration::from_secs(300);

#[derive(Args)]
pub struct Options {
    /// Exact HTTPS public origin. The local front proxy must overwrite X-Real-IP.
    #[arg(long)]
    public_origin: String,
    /// Exact HTTPS origin of a separate public vault site, for a link only.
    #[arg(long)]
    vault_site_origin: Option<String>,
    /// Existing private absolute installation registry directory.
    #[arg(long)]
    registry: PathBuf,
    /// Private absolute parent of account UUID directories containing personal.sock.
    #[arg(long)]
    ipc_root: PathBuf,
    /// Existing service-owner-only directory for gateway.sock and its persistent key.
    #[arg(long)]
    socket_dir: PathBuf,
    /// Optional separate service-owned mode 0710 directory for a dedicated HTTPS proxy group.
    #[arg(long, requires = "proxy_gid")]
    proxy_socket_dir: Option<PathBuf>,
    /// Only this trusted Unix group can connect to the public HTTP gateway socket.
    #[arg(long, requires = "proxy_socket_dir")]
    proxy_gid: Option<u32>,
}
#[derive(Clone)]
struct Gateway {
    origin: String,
    host: String,
    vault_site_origin: Option<String>,
    registry: PathBuf,
    ipc_root: PathBuf,
    key: Arc<Zeroizing<Vec<u8>>>,
    limits: Arc<Mutex<HashMap<IpAddr, VecDeque<Instant>>>>,
    inflight: Arc<tokio::sync::Semaphore>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Login {
    email: String,
    password: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Password {
    password: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Route {
    #[serde(default)]
    epoch: i64,
    account: Uuid,
    tenant: Uuid,
    volume: Uuid,
    generation: i64,
    expires: u64,
    session_hash: String,
}
fn private_dir(path: &Path) -> Result<(), String> {
    let m = fs::symlink_metadata(path).map_err(|_| "private_directory_unavailable")?;
    if !path.is_absolute()
        || fs::canonicalize(path).ok().as_deref() != Some(path)
        || !m.is_dir()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
    {
        return Err("private_directory_invalid".into());
    }
    Ok(())
}
fn private_file(path: &Path, create: bool) -> Result<File, String> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "private_file_unavailable")?;
    let m = file.metadata().map_err(|_| "private_file_unavailable")?;
    if !m.is_file()
        || m.uid() != unsafe { libc::geteuid() }
        || m.mode() & 0o077 != 0
        || m.nlink() != 1
    {
        return Err("private_file_invalid".into());
    }
    Ok(file)
}
fn key(dir: &Path) -> Result<Zeroizing<Vec<u8>>, String> {
    let path = dir.join("routing.key");
    let mut file = private_file(&path, true)?;
    let size = file
        .metadata()
        .map_err(|_| "routing_key_unavailable")?
        .len();
    let mut value = Zeroizing::new(vec![0u8; 32]);
    if size == 0 {
        SystemRandom::new()
            .fill(&mut value)
            .map_err(|_| "routing_key_unavailable")?;
        file.write_all(&value)
            .and_then(|_| file.sync_all())
            .map_err(|_| "routing_key_unavailable")?;
        File::open(dir)
            .and_then(|f| f.sync_all())
            .map_err(|_| "routing_key_unavailable")?;
    } else if size == 32 {
        file.read_exact(&mut value)
            .map_err(|_| "routing_key_unavailable")?;
    } else {
        return Err("routing_key_invalid".into());
    }
    Ok(value)
}
impl Gateway {
    fn new(options: &Options) -> Result<Self, String> {
        private_dir(&options.registry)?;
        private_dir(&options.ipc_root)?;
        let url = url::Url::parse(&options.public_origin).map_err(|_| "public_origin_invalid")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err("public_origin_invalid".into());
        }
        let origin = url.origin().ascii_serialization();
        if options.public_origin != origin {
            return Err("public_origin_invalid".into());
        }
        let vault_site_origin = options
            .vault_site_origin
            .as_deref()
            .map(|site| {
                let vault = url::Url::parse(site).map_err(|_| "vault_site_origin_invalid")?;
                let member_host = url.host_str().map(|host| host.trim_end_matches('.'));
                let distinct_host = vault
                    .host_str()
                    .is_some_and(|host| !host.ends_with('.') && Some(host) != member_host);
                if vault.scheme() != "https"
                    || !distinct_host
                    || site != vault.origin().ascii_serialization()
                {
                    return Err("vault_site_origin_invalid");
                }
                Ok(site.to_owned())
            })
            .transpose()?;
        Ok(Self {
            host: origin[8..].into(),
            origin,
            vault_site_origin,
            registry: options.registry.clone(),
            ipc_root: options.ipc_root.clone(),
            key: Arc::new(key(&options.socket_dir)?),
            limits: Arc::new(Mutex::new(HashMap::new())),
            inflight: Arc::new(tokio::sync::Semaphore::new(32)),
        })
    }
    fn headers(&self, h: &HeaderMap, method: &Method) -> Result<IpAddr, StatusCode> {
        if one(h, "host") != Some(self.host.as_str()) {
            return Err(StatusCode::BAD_REQUEST);
        }
        // Browser same-origin GETs commonly omit Origin. Fetch Metadata supplies
        // the same-origin fence there; all mutations require the exact Origin.
        if one(h, "origin") != Some(self.origin.as_str())
            && !(method == Method::GET
                && !h.contains_key("origin")
                && one(h, "sec-fetch-site") == Some("same-origin"))
        {
            return Err(StatusCode::FORBIDDEN);
        }
        one(h, "x-real-ip")
            .and_then(|s| s.parse().ok())
            .ok_or(StatusCode::BAD_REQUEST)
    }
    fn admit(&self, ip: IpAddr) -> bool {
        let Ok(mut limits) = self.limits.lock() else {
            return false;
        };
        let now = Instant::now();
        limits.retain(|_, history| {
            while history
                .front()
                .is_some_and(|t| now.duration_since(*t) >= WINDOW)
            {
                history.pop_front();
            }
            !history.is_empty()
        });
        if limits.len() >= 4096 && !limits.contains_key(&ip) {
            return false;
        }
        let history = limits.entry(ip).or_default();
        if history.len() >= 20 {
            return false;
        }
        history.push_back(now);
        true
    }
    fn sign(&self, route: &Route) -> Result<String, StatusCode> {
        let payload = URL_SAFE_NO_PAD
            .encode(serde_json::to_vec(route).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?);
        let signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, &self.key),
            payload.as_bytes(),
        );
        Ok(format!(
            "{payload}.{}",
            URL_SAFE_NO_PAD.encode(signature.as_ref())
        ))
    }
    fn route(&self, h: &HeaderMap, session: &str) -> Result<Route, StatusCode> {
        let token = cookie(h, ROUTER, 2048)?;
        let (payload, signature) = token.split_once('.').ok_or(StatusCode::UNAUTHORIZED)?;
        let signature = URL_SAFE_NO_PAD
            .decode(signature)
            .map_err(|_| StatusCode::UNAUTHORIZED)?;
        hmac::verify(
            &hmac::Key::new(hmac::HMAC_SHA256, &self.key),
            payload.as_bytes(),
            &signature,
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
        let route: Route = serde_json::from_slice(
            &URL_SAFE_NO_PAD
                .decode(payload)
                .map_err(|_| StatusCode::UNAUTHORIZED)?,
        )
        .map_err(|_| StatusCode::UNAUTHORIZED)?;
        let now = now()?;
        if route.expires <= now
            || route.expires > now + MAX_AGE
            || route.session_hash != hash(session)
        {
            return Err(StatusCode::UNAUTHORIZED);
        }
        Ok(route)
    }
    fn socket(&self, a: &Allocation) -> Result<(PathBuf, u32), StatusCode> {
        let account =
            Uuid::parse_str(&a.account_id).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if account.is_nil() || a.generation < 1 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let root = self.ipc_root.join(account.to_string());
        // ipc_root is gateway-owned and private. Its direct UUID child is
        // provisioned for the member UID, which creates the socket inside.
        let dir = fs::symlink_metadata(&root).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if !dir.is_dir() || dir.mode() & 0o077 != 0 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }

        let path = root.join("personal.sock");
        let meta = fs::symlink_metadata(&path).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if !meta.file_type().is_socket() || meta.uid() != dir.uid() || meta.mode() & 0o077 != 0 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        Ok((path, dir.uid()))
    }
    async fn proxy(
        &self,
        a: &SessionAllocation,
        method: Method,
        path: &str,
        body: Option<&[u8]>,
        session: Option<&str>,
    ) -> Result<(StatusCode, Vec<u8>, Option<(String, u64)>), StatusCode> {
        let unavailable = StatusCode::SERVICE_UNAVAILABLE;
        let (socket, uid) = self.socket(a)?;
        let stream = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::net::UnixStream::connect(socket),
        )
        .await
        .map_err(|_| unavailable)?
        .map_err(|_| unavailable)?;
        if stream.peer_cred().map_err(|_| unavailable)?.uid() != uid {
            return Err(unavailable);
        }
        let timeout = if path.ends_with("/text-preview") {
            85
        } else if path.starts_with("/api/v1/personal/google") {
            75
        } else {
            10
        };
        let limit = if path.starts_with("/api/v1/threads")
            || path.starts_with("/api/v1/knowledge/memories")
        {
            MAX_THREAD_BODY
        } else if path == "/api/v1/personal/google/history" {
            1024 * 1024
        } else if path.starts_with("/api/v1/personal/google") {
            128 * 1024
        } else {
            MAX_BODY
        };
        tokio::time::timeout(Duration::from_secs(timeout), async {
            // Keep this exact peer-verified stream through the last response byte.
            let (mut sender, connection) =
                hyper::client::conn::http1::handshake(hyper_util::rt::TokioIo::new(stream))
                    .await
                    .map_err(|_| unavailable)?;
            tokio::spawn(connection);
            let mut req = axum::http::Request::builder()
                .method(method)
                .uri(path)
                .header(header::HOST, "personal.internal")
                .header(header::ORIGIN, "https://personal.internal")
                .header("X-Moosuri-Account", &a.account_id);
            if let Some(token) = session {
                req = req.header(header::COOKIE, format!("{SESSION}={token}"));
                if path.ends_with("/text-preview") {
                    req = req.header(
                        moosuri_server::personal_text::SESSION_EPOCH_HEADER,
                        a.epoch.to_string(),
                    );
                }
            }
            if body.is_some() {
                req = req.header(header::CONTENT_TYPE, "application/json");
            }
            let req = req
                .body(Body::from(body.unwrap_or_default().to_vec()))
                .map_err(|_| unavailable)?;
            let response = sender.send_request(req).await.map_err(|_| unavailable)?;
            let status = response.status();
            if ![
                200, 201, 202, 204, 400, 401, 403, 404, 409, 413, 422, 429, 502, 503,
            ]
            .contains(&status.as_u16())
            {
                return Err(unavailable);
            }
            let set = if status.is_success()
                && matches!(path, "/api/v1/auth/login" | "/api/v1/auth/password")
            {
                Some(runtime_cookie(response.headers())?)
            } else {
                None
            };
            let bytes = axum::body::to_bytes(Body::new(response.into_body()), limit)
                .await
                .map_err(|_| unavailable)?;
            Ok((status, bytes.to_vec(), set))
        })
        .await
        .map_err(|_| unavailable)?
    }
}
#[derive(Clone, Copy)]
enum Endpoint {
    Login,
    Session,
    Password,
    Logout,
    Threads,
    PreviewInfo,
    TextPreview(Uuid),
    Thread(Uuid),
    History(Uuid, Option<i64>),
}
impl Endpoint {
    fn parse(method: &Method, uri: &Uri) -> Result<Self, StatusCode> {
        let path = uri.path();
        let endpoint = match path {
            "/api/v1/auth/login" => Self::Login,
            "/api/v1/auth/session" => Self::Session,
            "/api/v1/auth/password" => Self::Password,
            "/api/v1/auth/logout" => Self::Logout,
            "/api/v1/threads" => Self::Threads,
            "/api/v1/text-preview" => Self::PreviewInfo,
            _ => {
                let tail = path
                    .strip_prefix("/api/v1/threads/")
                    .ok_or(StatusCode::NOT_FOUND)?;
                if let Some(raw) = tail.strip_suffix("/text-preview") {
                    let id = Uuid::parse_str(raw)
                        .ok()
                        .filter(|id| !id.is_nil() && id.to_string() == raw)
                        .ok_or(StatusCode::BAD_REQUEST)?;
                    if method != Method::POST || uri.query().is_some() {
                        return Err(StatusCode::BAD_REQUEST);
                    }
                    return Ok(Self::TextPreview(id));
                }
                let (id, history) = tail
                    .strip_suffix("/messages")
                    .map(|id| (id, true))
                    .unwrap_or((tail, false));
                let id = Uuid::parse_str(id)
                    .ok()
                    .filter(|id| {
                        !id.is_nil()
                            && id.to_string() == tail.strip_suffix("/messages").unwrap_or(tail)
                    })
                    .ok_or(StatusCode::BAD_REQUEST)?;
                if history {
                    Self::History(id, None)
                } else {
                    Self::Thread(id)
                }
            }
        };
        let valid = match endpoint {
            Self::Session | Self::PreviewInfo | Self::History(..) => method == Method::GET,
            Self::Threads => method == Method::GET || method == Method::POST,
            Self::Thread(_) => method == Method::DELETE,
            _ => method == Method::POST,
        };
        if !valid {
            return Err(StatusCode::METHOD_NOT_ALLOWED);
        }
        match (endpoint, uri.query()) {
            (Self::History(id, _), Some(query)) if !query.is_empty() => {
                let pairs = url::form_urlencoded::parse(query.as_bytes()).collect::<Vec<_>>();
                if pairs.len() != 1 || pairs[0].0 != "before" {
                    return Err(StatusCode::BAD_REQUEST);
                }
                let before = pairs[0]
                    .1
                    .parse::<i64>()
                    .ok()
                    .filter(|n| *n > 0)
                    .ok_or(StatusCode::BAD_REQUEST)?;
                Ok(Self::History(id, Some(before)))
            }
            (Self::History(..), _) => Ok(endpoint),
            (_, Some(_)) => Err(StatusCode::BAD_REQUEST),
            _ => Ok(endpoint),
        }
    }
    fn path(self) -> String {
        match self {
            Self::Login => "/api/v1/auth/login".into(),
            Self::Session => "/api/v1/auth/session".into(),
            Self::Password => "/api/v1/auth/password".into(),
            Self::Logout => "/api/v1/auth/logout".into(),
            Self::Threads => "/api/v1/threads".into(),
            Self::PreviewInfo => "/api/v1/text-preview".into(),
            Self::TextPreview(id) => format!("/api/v1/threads/{id}/text-preview"),
            Self::Thread(id) => format!("/api/v1/threads/{id}"),
            Self::History(id, before) => format!(
                "/api/v1/threads/{id}/messages{}",
                before.map(|n| format!("?before={n}")).unwrap_or_default()
            ),
        }
    }
    fn threads(self) -> bool {
        matches!(
            self,
            Self::Threads
                | Self::Thread(_)
                | Self::History(..)
                | Self::TextPreview(_)
                | Self::PreviewInfo
        )
    }
}
#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct CreateThread {
    title: String,
    request_id: Option<String>,
}
fn api_error(status: StatusCode, body: &[u8]) -> Result<Response, StatusCode> {
    let error: moosuri_core::ApiError =
        serde_json::from_slice(body).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if error.code.is_empty()
        || error.code.len() > 80
        || !error
            .code
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b == b'_')
    {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(response(
        status,
        serde_json::to_vec(&error).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
    ))
}
fn thread_response(
    endpoint: Endpoint,
    method: &Method,
    status: StatusCode,
    body: Vec<u8>,
) -> Result<Response, StatusCode> {
    if !status.is_success() {
        return api_error(status, &body);
    }
    match endpoint {
        Endpoint::PreviewInfo => {
            let value: serde_json::Value =
                serde_json::from_slice(&body).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if status != StatusCode::OK
                || value["capability"] != "text_only_preview"
                || value["tools"] != false
            {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        Endpoint::TextPreview(id) => {
            let turn: moosuri_core::ChatTurn =
                serde_json::from_slice(&body).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if ![StatusCode::OK, StatusCode::CREATED, StatusCode::ACCEPTED].contains(&status)
                || turn.run.thread_id != id.to_string()
                || turn.run.model.function_tools
                || turn.run.mission_receipt.is_some()
                || turn.run.calendar_receipt.is_some()
            {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        Endpoint::Threads if method == Method::GET => {
            if status != StatusCode::OK {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::from_slice::<moosuri_core::ThreadsResponse>(&body)
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        }
        Endpoint::Threads => {
            if status != StatusCode::CREATED {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            serde_json::from_slice::<moosuri_core::ThreadSummary>(&body)
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        }
        Endpoint::History(id, _) => {
            let page: moosuri_core::ChatPage =
                serde_json::from_slice(&body).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if status != StatusCode::OK || page.thread_id != id.to_string() {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        Endpoint::Thread(_) => {
            if status != StatusCode::NO_CONTENT || !body.is_empty() {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
        }
        _ => return Err(StatusCode::SERVICE_UNAVAILABLE),
    }
    Ok(response(status, body))
}
fn one<'a>(headers: &'a HeaderMap, name: &str) -> Option<&'a str> {
    let mut values = headers.get_all(name).iter();
    let value = values.next()?.to_str().ok()?;
    if values.next().is_some() {
        None
    } else {
        Some(value)
    }
}
fn opaque(value: &str) -> bool {
    (32..=128).contains(&value.len())
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
}
fn cookie(headers: &HeaderMap, name: &str, max: usize) -> Result<String, StatusCode> {
    let mut found = None;
    for h in headers.get_all(header::COOKIE) {
        let value = h.to_str().map_err(|_| StatusCode::UNAUTHORIZED)?;
        if value.len() > 4096 {
            return Err(StatusCode::UNAUTHORIZED);
        }
        for item in value.split(';') {
            let Some((key, value)) = item.trim().split_once('=') else {
                continue;
            };
            if key == name {
                if found.is_some()
                    || value.is_empty()
                    || value.len() > max
                    || !value
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_-.".contains(&b))
                {
                    return Err(StatusCode::UNAUTHORIZED);
                }
                found = Some(value.to_string());
            }
        }
    }
    found.ok_or(StatusCode::UNAUTHORIZED)
}
fn runtime_cookie(headers: &HeaderMap) -> Result<(String, u64), StatusCode> {
    let mut found = None;
    for h in headers.get_all(header::SET_COOKIE) {
        let s = h.to_str().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let mut parts = s.split(';');
        let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        };
        if name != SESSION || !opaque(value) || found.is_some() {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let ages: Vec<_> = parts
            .filter_map(|p| p.trim().strip_prefix("Max-Age="))
            .collect();
        if ages.len() != 1 {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let age = ages[0]
            .parse::<u64>()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if age == 0 || age > MAX_AGE {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        found = Some((value.to_string(), age));
    }
    found.ok_or(StatusCode::SERVICE_UNAVAILABLE)
}
fn now() -> Result<u64, StatusCode> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|v| v.as_secs())
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
fn hash(session: &str) -> String {
    URL_SAFE_NO_PAD.encode(digest::digest(&digest::SHA256, session.as_bytes()))
}
fn failure(status: StatusCode) -> Response {
    let code = match status {
        StatusCode::UNAUTHORIZED => "unauthorized",
        StatusCode::FORBIDDEN => "forbidden",
        StatusCode::TOO_MANY_REQUESTS => "rate_limited",
        StatusCode::BAD_REQUEST => "bad_request",
        _ => "unavailable",
    };
    let mut response = (status, Json(serde_json::json!({"code":code}))).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    if status == StatusCode::TOO_MANY_REQUESTS {
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("300"));
    }
    response
}
fn response(status: StatusCode, body: Vec<u8>) -> Response {
    let mut response = (status, Body::from(body)).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/json"),
    );
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    response
}
struct SessionAllocation {
    allocation: Allocation,
    epoch: i64,
}
impl std::ops::Deref for SessionAllocation {
    type Target = Allocation;
    fn deref(&self) -> &Allocation {
        &self.allocation
    }
}
async fn resolve(
    g: &Gateway,
    destination: runtime_registry::SessionDestination,
) -> Result<SessionAllocation, StatusCode> {
    let root = g.registry.clone();
    let (allocation, epoch) =
        tokio::task::spawn_blocking(move || runtime_registry::session_route(&root, destination))
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .map_err(route_error)?;
    Ok(SessionAllocation { allocation, epoch })
}
fn route_error(e: moosuri_store::StoreError) -> StatusCode {
    match e {
        moosuri_store::StoreError::NotFound | moosuri_store::StoreError::Forbidden => {
            StatusCode::UNAUTHORIZED
        }
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}
async fn allocation(g: &Gateway, email: String) -> Result<SessionAllocation, StatusCode> {
    resolve(g, runtime_registry::SessionDestination::Email(email)).await
}
async fn active(g: &Gateway, route: &Route) -> Result<SessionAllocation, StatusCode> {
    let a = resolve(
        g,
        runtime_registry::SessionDestination::Account(route.account),
    )
    .await?;
    if a.epoch != route.epoch {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if a.tenant_id != route.tenant.to_string()
        || a.volume_id != route.volume.to_string()
        || a.generation != route.generation
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(a)
}
async fn recheck(g: &Gateway, allocation: &SessionAllocation) -> Result<(), StatusCode> {
    let route = Route {
        epoch: allocation.epoch,
        account: Uuid::parse_str(&allocation.account_id)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        tenant: Uuid::parse_str(&allocation.tenant_id)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        volume: Uuid::parse_str(&allocation.volume_id)
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        generation: allocation.generation,
        expires: 0,
        session_hash: String::new(),
    };
    active(g, &route).await.map(|_| ())
}
async fn authenticated_allocation(
    g: &Gateway,
    headers: &HeaderMap,
    require_account_header: bool,
) -> Result<(SessionAllocation, String), StatusCode> {
    let session = cookie(headers, SESSION, 128)?;
    if !opaque(&session) {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let route = g.route(headers, &session)?;
    if headers.contains_key("x-moosuri-account") && one(headers, "x-moosuri-account").is_none() {
        return Err(StatusCode::UNAUTHORIZED);
    }
    if let Some(account) = one(headers, "x-moosuri-account") {
        if account != route.account.to_string() {
            return Err(StatusCode::UNAUTHORIZED);
        }
    } else if require_account_header {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let a = active(g, &route).await?;
    Ok((a, session))
}
fn member_session_view(
    bytes: &[u8],
    allocation: &Allocation,
) -> Result<moosuri_core::SessionView, StatusCode> {
    let view: moosuri_core::SessionView =
        serde_json::from_slice(bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if view.user.id != allocation.account_id {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(view)
}

async fn vault_status(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    #[derive(Serialize)]
    #[serde(tag = "state", rename_all = "snake_case")]
    enum VaultStatus<'a> {
        SetupRequired {
            #[serde(skip_serializing_if = "Option::is_none")]
            site_origin: Option<&'a str>,
        },
    }

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
        let body = serde_json::to_vec(&VaultStatus::SetupRequired {
            site_origin: g.vault_site_origin.as_deref(),
        })
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        Ok(response(StatusCode::OK, body))
    }
    .await;
    result.unwrap_or_else(failure)
}

async fn handler(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    match handle(&g, method, uri, &headers, &body).await {
        Ok(response) => response,
        Err(status) => failure(status),
    }
}
async fn handle(
    g: &Gateway,
    method: Method,
    uri: Uri,
    headers: &HeaderMap,
    body: &[u8],
) -> Result<Response, StatusCode> {
    let endpoint = Endpoint::parse(&method, &uri)?;
    let path = endpoint.path();
    let ip = g.headers(headers, &method)?;
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if method == Method::POST
        && one(headers, "content-type").and_then(|s| s.split(';').next())
            != Some("application/json")
    {
        return Err(StatusCode::BAD_REQUEST);
    }
    if path == "/api/v1/auth/login" {
        if !g.admit(ip) {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        let input: Login = serde_json::from_slice(body).map_err(|_| StatusCode::BAD_REQUEST)?;
        let password = Zeroizing::new(input.password);
        let email = input.email.trim().to_ascii_lowercase();
        if email.is_empty()
            || email.len() > 320
            || !email.is_ascii()
            || !email.contains('@')
            || password.is_empty()
            || password.len() > 1024
        {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let a = allocation(g, email.clone()).await?;
        let bytes = Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({"email":email,"password":password.as_str()}))
                .map_err(|_| StatusCode::BAD_REQUEST)?,
        );
        let result = g.proxy(&a, method, &path, Some(&bytes), None).await;
        recheck(g, &a).await?;
        let (status, body, set) = result?;
        if !status.is_success() {
            return Err(
                if status == StatusCode::BAD_REQUEST || status == StatusCode::UNAUTHORIZED {
                    StatusCode::UNAUTHORIZED
                } else {
                    status
                },
            );
        }
        recheck(g, &a).await?;
        let body = admin::public_session(g, &a, &body).await?;
        return issued(
            g,
            &a,
            status,
            body,
            set.ok_or(StatusCode::SERVICE_UNAVAILABLE)?,
        );
    }
    let (a, session) =
        authenticated_allocation(g, headers, method != Method::GET || endpoint.threads()).await?;
    if endpoint.threads() {
        let payload = if matches!(endpoint, Endpoint::TextPreview(_)) {
            #[derive(Deserialize, Serialize)]
            #[serde(deny_unknown_fields)]
            struct Input {
                request_id: String,
                text: String,
                capability: String,
            }
            let input: Input =
                serde_json::from_slice(body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            if input.capability != "text_only_preview" {
                return Err(StatusCode::BAD_REQUEST);
            }
            Some(serde_json::to_vec(&input).map_err(|_| StatusCode::BAD_REQUEST)?)
        } else if matches!(endpoint, Endpoint::Threads) && method == Method::POST {
            let input: CreateThread =
                serde_json::from_slice(body).map_err(|_| StatusCode::UNPROCESSABLE_ENTITY)?;
            Some(serde_json::to_vec(&input).map_err(|_| StatusCode::BAD_REQUEST)?)
        } else {
            if !body.is_empty() && body != b"{}" {
                return Err(StatusCode::BAD_REQUEST);
            }
            None
        };
        let result = g
            .proxy(
                &a,
                method.clone(),
                &path,
                payload.as_deref(),
                Some(&session),
            )
            .await;
        // A fenced generation cannot publish either private results or stale
        // error information, including timeouts and runtime 404 responses.
        recheck(g, &a).await?;
        let (status, body, _) = result?;
        return thread_response(endpoint, &method, status, body);
    }
    let payload = if path == "/api/v1/auth/password" {
        let input: Password = serde_json::from_slice(body).map_err(|_| StatusCode::BAD_REQUEST)?;
        let password = Zeroizing::new(input.password);
        if password.is_empty() || password.len() > 1024 {
            return Err(StatusCode::BAD_REQUEST);
        }
        Some(Zeroizing::new(
            serde_json::to_vec(&serde_json::json!({"password":password.as_str()}))
                .map_err(|_| StatusCode::BAD_REQUEST)?,
        ))
    } else {
        if !body.is_empty() && body != b"{}" {
            return Err(StatusCode::BAD_REQUEST);
        }
        None
    };
    let result = g
        .proxy(
            &a,
            method,
            &path,
            payload.as_ref().map(|p| p.as_slice()),
            Some(&session),
        )
        .await;
    recheck(g, &a).await?;
    let (status, body, set) = result?;
    if !status.is_success() {
        return Err(status);
    }
    if path == "/api/v1/auth/logout" {
        return Ok(response(status, body));
    }
    let body = admin::public_session(g, &a, &body).await?;
    if let Some(set) = set {
        issued(g, &a, status, body, set)
    } else {
        Ok(response(status, body))
    }
}
fn check_user(body: &[u8], a: &Allocation) -> Result<(), StatusCode> {
    let value: serde_json::Value =
        serde_json::from_slice(body).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if value["user"]["id"].as_str() != Some(a.account_id.as_str()) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(())
}
fn issued(
    g: &Gateway,
    a: &SessionAllocation,
    status: StatusCode,
    body: Vec<u8>,
    set: (String, u64),
) -> Result<Response, StatusCode> {
    check_user(&body, a)?;
    let route = Route {
        epoch: a.epoch,
        account: Uuid::parse_str(&a.account_id).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        tenant: Uuid::parse_str(&a.tenant_id).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        volume: Uuid::parse_str(&a.volume_id).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        generation: a.generation,
        expires: now()? + set.1,
        session_hash: hash(&set.0),
    };
    let mut response = response(status, body);
    for (name, value) in [(SESSION, set.0), (ROUTER, g.sign(&route)?)] {
        response.headers_mut().append(
            header::SET_COOKIE,
            HeaderValue::from_str(&format!(
                "{name}={value}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age={}",
                set.1
            ))
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?,
        );
    }
    Ok(response)
}
// This shell is public, immutable build output. It never selects a personal
// runtime, authenticates a cookie, caches private JSON or forwards a URL.
async fn public_shell(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
) -> Response {
    if one(&headers, "host") != Some(g.host.as_str()) {
        return failure(StatusCode::BAD_REQUEST);
    }
    if headers.contains_key("origin") && one(&headers, "origin") != Some(g.origin.as_str()) {
        return failure(StatusCode::FORBIDDEN);
    }
    if uri.path().starts_with("/api/") {
        return failure(StatusCode::NOT_FOUND);
    }
    if !matches!(method, Method::GET | Method::HEAD) {
        return failure(StatusCode::METHOD_NOT_ALLOWED);
    }
    if uri.query().is_some() {
        return failure(StatusCode::NOT_FOUND);
    }
    let path = uri.path();
    let asset = if matches!(
        path,
        "/" | "/preview" | "/app" | "/login" | "/personal.html"
    ) {
        "/personal.html"
    } else if matches!(
        path,
        "/moosuri-icon-192.png" | "/moosuri-icon-512.png" | "/personal.webmanifest"
    ) {
        path
    } else if path.strip_prefix("/assets/").is_some_and(|name| {
        !name.is_empty()
            && name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"_.-".contains(&c))
    }) {
        path
    } else {
        return failure(StatusCode::NOT_FOUND);
    };
    let Ok(uri) = asset.parse() else {
        return failure(StatusCode::NOT_FOUND);
    };
    super::static_asset(uri).await
}
fn router(g: Gateway) -> Router {
    Router::new()
        .route("/api/v1/auth/login", post(handler))
        .route("/api/v1/auth/session", get(handler))
        .route("/api/v1/auth/password", post(handler))
        .route("/api/v1/auth/logout", post(handler))
        .route("/api/v1/personal/vault", get(vault_status))
        .route("/api/v1/threads", get(handler).post(handler))
        .route("/api/v1/threads/{id}", delete(handler))
        .route("/api/v1/threads/{id}/messages", get(handler))
        .route("/api/v1/text-preview", get(handler))
        .route(
            "/api/v1/threads/{id}/text-preview",
            post(handler).layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .fallback(public_shell)
        .layer(DefaultBodyLimit::max(4096))
        .merge(memory::routes())
        .merge(google::routes())
        .merge(admin::routes())
        .with_state(g)
}
async fn bind_public_socket(options: &Options) -> Result<(File, tokio::net::UnixListener), String> {
    let (dir, mode) = match (&options.proxy_socket_dir, options.proxy_gid) {
        (None, None) => (&options.socket_dir, 0o600),
        (Some(dir), Some(gid)) => {
            let meta = fs::symlink_metadata(dir).map_err(|_| "proxy_socket_directory_invalid")?;
            if gid == 0
                || !dir.is_absolute()
                || fs::canonicalize(dir).ok().as_ref() != Some(dir)
                || !meta.is_dir()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.gid() != gid
                || meta.mode() & 0o7777 != 0o710
            {
                return Err("proxy_socket_directory_invalid".into());
            }
            (dir, 0o660)
        }
        _ => return Err("proxy_socket_directory_invalid".into()),
    };
    let lock = File::open(dir).map_err(|_| "gateway_socket_unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "gateway_socket_directory_busy")?;
    let socket = dir.join("gateway.sock");
    match fs::symlink_metadata(&socket) {
        Ok(meta) => {
            if !meta.file_type().is_socket()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.nlink() != 1
            {
                return Err("gateway_socket_invalid".into());
            }
            match tokio::time::timeout(
                Duration::from_secs(1),
                tokio::net::UnixStream::connect(&socket),
            )
            .await
            {
                Ok(Err(e)) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    fs::remove_file(&socket).map_err(|_| "gateway_socket_unavailable")?;
                }
                _ => return Err("gateway_socket_active".into()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err("gateway_socket_unavailable".into()),
    }
    let listener =
        tokio::net::UnixListener::bind(&socket).map_err(|_| "gateway_socket_unavailable")?;
    if let Some(gid) = options.proxy_gid {
        std::os::unix::fs::chown(&socket, None, Some(gid))
            .map_err(|_| "gateway_socket_group_unavailable")?;
    }
    fs::set_permissions(&socket, fs::Permissions::from_mode(mode))
        .map_err(|_| "gateway_socket_unavailable")?;
    Ok((lock, listener))
}
pub async fn run(options: &Options) -> Result<(), String> {
    private_dir(&options.socket_dir)?;
    let lock = private_file(&options.socket_dir.join("gateway.lock"), true)?;
    lock.try_lock_exclusive()
        .map_err(|_| "gateway_already_running")?;
    let g = Gateway::new(options)?;
    let (_socket_directory, listener) = bind_public_socket(options).await?;
    axum::serve(listener, router(g))
        .await
        .map_err(|_| "gateway_stopped".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use tower::ServiceExt;
    fn directory(parent: &Path, name: &str) -> PathBuf {
        let p = parent.join(name);
        fs::create_dir(&p).unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        p
    }
    fn options(parent: &Path) -> Options {
        Options {
            public_origin: "https://assistant.example.test".into(),
            vault_site_origin: None,
            registry: directory(parent, "registry"),
            ipc_root: directory(parent, "ipc"),
            socket_dir: directory(parent, "gateway"),
            proxy_socket_dir: None,
            proxy_gid: None,
        }
    }
    #[test]
    fn vault_site_origin_must_be_an_exact_https_origin_on_a_distinct_host() {
        let temp = tempfile::tempdir().unwrap();
        let mut options = options(&temp.path().canonicalize().unwrap());
        for invalid in [
            "http://vault.example.test",
            "https://assistant.example.test",
            "https://assistant.example.test:8443",
            "https://assistant.example.test.",
            "https://vault.example.test/",
            "https://vault.example.test/login",
            "https://vault.example.test?token=synthetic",
            "https://vault.example.test#response",
            "https://vault-user@vault.example.test",
        ] {
            options.vault_site_origin = Some(invalid.into());
            assert_eq!(
                Gateway::new(&options).err().unwrap(),
                "vault_site_origin_invalid"
            );
        }
        options.vault_site_origin = Some("https://vault.example.test".into());
        assert_eq!(
            Gateway::new(&options).unwrap().vault_site_origin,
            options.vault_site_origin
        );
    }
    #[tokio::test]
    async fn proxy_group_can_only_reach_the_socket_and_an_active_listener_is_preserved() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mut options = options(&root);
        let proxy = directory(&root, "proxy");
        // Use the current effective group where available (CI may execute as root).
        let gid = match unsafe { libc::getegid() } {
            0 => 65534,
            other => other,
        };
        std::os::unix::fs::chown(&proxy, None, Some(gid)).unwrap();
        options.proxy_socket_dir = Some(proxy.clone());
        options.proxy_gid = Some(gid);
        assert!(bind_public_socket(&options).await.is_err());
        fs::set_permissions(&proxy, fs::Permissions::from_mode(0o710)).unwrap();
        let (lock, listener) = bind_public_socket(&options).await.unwrap();
        let socket = proxy.join("gateway.sock");
        let meta = fs::symlink_metadata(&socket).unwrap();
        assert_eq!(meta.mode() & 0o777, 0o660);
        assert_eq!(meta.gid(), gid);
        assert!(!proxy.join("routing.key").exists());
        drop(lock);
        assert!(bind_public_socket(&options).await.is_err());
        assert_eq!(fs::symlink_metadata(&socket).unwrap().ino(), meta.ino());
        drop(listener);
        let _new_listener = bind_public_socket(&options).await.unwrap();
    }
    fn headers() -> HeaderMap {
        let mut h = HeaderMap::new();
        for (n, v) in [
            ("host", "assistant.example.test"),
            ("origin", "https://assistant.example.test"),
            ("content-type", "application/json"),
            ("x-real-ip", "192.0.2.9"),
        ] {
            h.insert(
                axum::http::HeaderName::from_bytes(n.as_bytes()).unwrap(),
                HeaderValue::from_static(v),
            );
        }
        h
    }
    fn cookies(response: &Response) -> String {
        response
            .headers()
            .get_all(header::SET_COOKIE)
            .iter()
            .map(|h| h.to_str().unwrap().split(';').next().unwrap())
            .collect::<Vec<_>>()
            .join("; ")
    }
    async fn request(
        g: &Gateway,
        path: &str,
        method: Method,
        h: HeaderMap,
        body: &str,
    ) -> Response {
        router(g.clone())
            .oneshot(
                axum::http::Request::builder()
                    .method(method)
                    .uri(path)
                    .body(Body::from(body.to_string()))
                    .map(|mut r| {
                        *r.headers_mut() = h;
                        r
                    })
                    .unwrap(),
            )
            .await
            .unwrap()
    }
    async fn fake(
        options: &Options,
        email: &str,
        token: &str,
    ) -> (Allocation, Arc<AtomicUsize>, tokio::task::JoinHandle<()>) {
        let a = runtime_registry::reserve(&options.registry, Uuid::new_v4(), email.into()).unwrap();
        runtime_registry::set_routing(&options.registry, &a, true).unwrap();
        let root = directory(&options.ipc_root, &a.account_id);
        let path = root.join("personal.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let count = Arc::new(AtomicUsize::new(0));
        let counted = count.clone();
        let account = a.account_id.clone();
        let token = token.to_string();
        let email = email.to_string();
        let app=Router::new().fallback(axum::routing::any(move|method:Method,uri:Uri,h:HeaderMap,body:axum::body::Bytes|{let account=account.clone();let email=email.clone();let token=token.clone();let count=counted.clone();async move {
            assert_eq!(one(&h,"host"),Some("personal.internal"));assert_eq!(one(&h,"origin"),Some("https://personal.internal"));assert_eq!(one(&h,"x-moosuri-account"),Some(account.as_str()));
            assert!(!h.contains_key("x-real-ip"));count.fetch_add(1,Ordering::SeqCst);
            if uri.path()=="/api/v1/auth/logout" {return StatusCode::NO_CONTENT.into_response();}
            if uri.path()!="/api/v1/auth/login" {assert_eq!(cookie(&h,SESSION,128).unwrap(),token);}
            else {assert_eq!(method,Method::POST);let value:serde_json::Value=serde_json::from_slice(&body).unwrap();assert_eq!(value["password"],"Synthetic-password-42");assert!(!h.contains_key(header::COOKIE));}
            let mut response=Json(serde_json::json!({"user":{"id":account,"email":email,"name":"Member","role":"member","must_change_password":false},"password_change_required":false,"chat_draft_scope":"a".repeat(64)})).into_response();
            if uri.path()=="/api/v1/auth/login"||uri.path()=="/api/v1/auth/password" {response.headers_mut().insert(header::SET_COOKIE,HeaderValue::from_str(&format!("moosuri_session={token}; Path=/; HttpOnly; Secure; SameSite=Strict; Max-Age=3600")).unwrap());}
            response
        }}));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        (a, count, task)
    }
    #[tokio::test]
    async fn two_private_runtimes_login_session_restart_password_and_logout_preserve_owner() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = options(&root);
        let (a, ac, at) = fake(&options, "a@example.test", &"a".repeat(43)).await;
        let (b, bc, bt) = fake(&options, "b@example.test", &"b".repeat(43)).await;
        let g = Gateway::new(&options).unwrap();
        let mut all = Vec::new();
        for (email, account) in [(" A@EXAMPLE.TEST ", &a), ("b@example.test", &b)] {
            let login = request(
                &g,
                "/api/v1/auth/login",
                Method::POST,
                headers(),
                &serde_json::json!({"email":email,"password":"Synthetic-password-42"}).to_string(),
            )
            .await;
            assert_eq!(login.status(), StatusCode::OK);
            assert_eq!(
                login.headers().get_all(header::SET_COOKIE).iter().count(),
                2
            );
            for value in login.headers().get_all(header::SET_COOKIE) {
                let value = value.to_str().unwrap();
                assert!(value.contains("; HttpOnly; Secure; SameSite=Strict;"));
            }
            let cookies = cookies(&login);
            let mut h = headers();
            h.insert(header::COOKIE, HeaderValue::from_str(&cookies).unwrap());
            h.remove("origin");
            h.insert("sec-fetch-site", HeaderValue::from_static("same-origin"));
            let session = request(&g, "/api/v1/auth/session", Method::GET, h.clone(), "").await;
            assert_eq!(session.status(), StatusCode::OK);
            assert!(!session.headers().contains_key(header::SET_COOKIE));
            all.push((account.account_id.clone(), cookies));
        }
        // An authentic route cookie from A must never select B with B's real
        // session cookie, even though both logins are individually valid.
        let route_a = all[0]
            .1
            .split("; ")
            .find(|part| part.starts_with(ROUTER))
            .unwrap();
        let session_b = all[1]
            .1
            .split("; ")
            .find(|part| part.starts_with(SESSION))
            .unwrap();
        let mut mixed = headers();
        mixed.insert(
            header::COOKIE,
            HeaderValue::from_str(&format!("{route_a}; {session_b}")).unwrap(),
        );
        assert_eq!(
            request(&g, "/api/v1/auth/session", Method::GET, mixed, "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(ac.load(Ordering::SeqCst), 2);
        assert_eq!(bc.load(Ordering::SeqCst), 2);
        let restarted = Gateway::new(&options).unwrap();
        for (account, cookies) in &all {
            let mut h = headers();
            h.insert(header::COOKIE, HeaderValue::from_str(cookies).unwrap());
            h.insert("x-moosuri-account", HeaderValue::from_str(account).unwrap());
            assert_eq!(
                request(
                    &restarted,
                    "/api/v1/auth/session",
                    Method::GET,
                    h.clone(),
                    ""
                )
                .await
                .status(),
                StatusCode::OK
            );
            assert_eq!(
                request(
                    &restarted,
                    "/api/v1/auth/password",
                    Method::POST,
                    h.clone(),
                    r#"{"password":"Another-synthetic-password"}"#
                )
                .await
                .status(),
                StatusCode::OK
            );
            let logout = request(&restarted, "/api/v1/auth/logout", Method::POST, h, "{}").await;
            assert_eq!(logout.status(), StatusCode::NO_CONTENT);
            assert!(!logout.headers().contains_key(header::SET_COOKIE));
        }
        assert_eq!(ac.load(Ordering::SeqCst), 5);
        assert_eq!(bc.load(Ordering::SeqCst), 5);
        at.abort();
        bt.abort();
    }
    #[tokio::test]
    async fn forged_mixed_origin_fenced_and_unknown_routes_cannot_touch_private_runtime() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = options(&root);
        let (a, count, task) = fake(&options, "a@example.test", &"a".repeat(43)).await;
        let g = Gateway::new(&options).unwrap();
        let login = request(
            &g,
            "/api/v1/auth/login",
            Method::POST,
            headers(),
            r#"{"email":"a@example.test","password":"Synthetic-password-42"}"#,
        )
        .await;
        let valid = cookies(&login);
        assert_eq!(count.load(Ordering::SeqCst), 1);
        for changed in [
            valid.replace(&"a".repeat(43), &"b".repeat(43)),
            format!("{valid}x"),
            format!("{valid}; moosuri_session={}", "a".repeat(43)),
        ] {
            let mut h = headers();
            h.insert(header::COOKIE, HeaderValue::from_str(&changed).unwrap());
            assert_eq!(
                request(&g, "/api/v1/auth/session", Method::GET, h, "")
                    .await
                    .status(),
                StatusCode::UNAUTHORIZED
            );
        }
        let mut h = headers();
        h.insert(header::COOKIE, HeaderValue::from_str(&valid).unwrap());
        h.insert(
            "origin",
            HeaderValue::from_static("https://foreign.example.test"),
        );
        assert_eq!(
            request(&g, "/api/v1/auth/session", Method::GET, h, "")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let mut h = headers();
        h.insert(header::COOKIE, HeaderValue::from_str(&valid).unwrap());
        h.insert(
            "x-moosuri-account",
            HeaderValue::from_str(&Uuid::new_v4().to_string()).unwrap(),
        );
        assert_eq!(
            request(&g, "/api/v1/auth/password", Method::POST, h, "{}")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&g, "/api/v1/admin/users", Method::POST, headers(), "{}")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(&g, "/api/v1/auth/session", Method::POST, headers(), "{}")
                .await
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            request(&g, "/api/v1/auth/session", Method::HEAD, headers(), "")
                .await
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        runtime_registry::set_routing(&options.registry, &a, false).unwrap();
        let mut h = headers();
        h.insert(header::COOKIE, HeaderValue::from_str(&valid).unwrap());
        assert_eq!(
            request(&g, "/api/v1/auth/session", Method::GET, h, "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(
                &g,
                "/api/v1/auth/login",
                Method::POST,
                headers(),
                r#"{"email":"a@example.test","password":"Synthetic-password-42"}"#
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(count.load(Ordering::SeqCst), 1);
        task.abort();
    }
    #[tokio::test]
    async fn unknown_account_rotation_shares_ip_limit_and_json_is_strict() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = options(&root);
        runtime_registry::reserve(
            &options.registry,
            Uuid::new_v4(),
            "reserved@example.test".into(),
        )
        .unwrap();
        let g = Gateway::new(&options).unwrap();
        for i in 0..20 {
            assert_eq!(request(&g,"/api/v1/auth/login",Method::POST,headers(),&serde_json::json!({"email":format!("missing-{i}@example.test"),"password":"synthetic"}).to_string()).await.status(),StatusCode::UNAUTHORIZED);
        }
        assert_eq!(
            request(
                &g,
                "/api/v1/auth/login",
                Method::POST,
                headers(),
                r#"{"email":"another@example.test","password":"synthetic"}"#
            )
            .await
            .status(),
            StatusCode::TOO_MANY_REQUESTS
        );
        let mut h = headers();
        h.insert("x-real-ip", HeaderValue::from_static("192.0.2.10"));
        assert_eq!(request(&g,"/api/v1/auth/login",Method::POST,h.clone(),r#"{"email":"reserved@example.test","password":"synthetic","url":"http://attacker.invalid"}"#).await.status(),StatusCode::BAD_REQUEST);
        h.append("x-real-ip", HeaderValue::from_static("192.0.2.11"));
        assert_eq!(
            request(&g, "/api/v1/auth/login", Method::POST, h, "{}")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
    }
    #[tokio::test]
    async fn private_listener_lock_and_stale_socket_restart_reuse_persistent_key() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = Arc::new(options(&root));
        runtime_registry::reserve(
            &options.registry,
            Uuid::new_v4(),
            "reserved@example.test".into(),
        )
        .unwrap();
        async fn start(options: Arc<Options>) -> tokio::task::JoinHandle<Result<(), String>> {
            let running = options.clone();
            let task = tokio::spawn(async move { run(&running).await });
            let client = reqwest::Client::builder()
                .no_proxy()
                .unix_socket(options.socket_dir.join("gateway.sock"))
                .timeout(Duration::from_millis(200))
                .build()
                .unwrap();
            tokio::time::timeout(Duration::from_secs(3),async {
                loop {
                    if let Ok(response)=client.post("http://assistant.example.test/api/v1/auth/login").header("Host","assistant.example.test").header("Origin","https://assistant.example.test").header("X-Real-IP","192.0.2.100").json(&serde_json::json!({"email":"unknown@example.test","password":"synthetic"})).send().await {
                        assert_eq!(response.status(),StatusCode::UNAUTHORIZED);break;
                    }
                    tokio::time::sleep(Duration::from_millis(5)).await;
                }
            }).await.unwrap();
            task
        }
        let first = start(options.clone()).await;
        let socket = options.socket_dir.join("gateway.sock");
        let meta = fs::symlink_metadata(&socket).unwrap();
        assert!(meta.file_type().is_socket());
        assert_eq!(meta.mode() & 0o777, 0o600);
        assert_eq!(run(&options).await.unwrap_err(), "gateway_already_running");
        let key = Zeroizing::new(fs::read(options.socket_dir.join("routing.key")).unwrap());
        first.abort();
        let _ = first.await;
        assert!(socket.exists());
        let second = start(options.clone()).await;
        assert!(
            key.as_slice()
                == fs::read(options.socket_dir.join("routing.key"))
                    .unwrap()
                    .as_slice()
        );
        second.abort();
        let _ = second.await;
    }
    async fn real_member(
        options: &Options,
        email: &str,
    ) -> (
        Allocation,
        moosuri_store::Store,
        tokio::task::JoinHandle<()>,
    ) {
        let a = runtime_registry::reserve(&options.registry, Uuid::new_v4(), email.into()).unwrap();
        serve_test_member(options, email, a, None).await
    }
    async fn serve_test_member(
        options: &Options,
        email: &str,
        a: Allocation,
        google: Option<crate::personal_google::TestTransport>,
    ) -> (
        Allocation,
        moosuri_store::Store,
        tokio::task::JoinHandle<()>,
    ) {
        let root = directory(options.ipc_root.parent().unwrap(), &a.volume_id);
        let owner = moosuri_store::personal_volume::VolumeOwner {
            account: Uuid::parse_str(&a.account_id).unwrap(),
            tenant: Uuid::parse_str(&a.tenant_id).unwrap(),
            volume: Uuid::parse_str(&a.volume_id).unwrap(),
            generation: a.generation as u64,
        };
        owner
            .initialize(
                &root,
                "Synthetic member".into(),
                email.into(),
                "Temporary-synthetic-42".into(),
            )
            .await
            .unwrap();
        let store = owner.open_verified(&root).await.unwrap();
        let initial = store
            .login(email.into(), "Temporary-synthetic-42".into())
            .await
            .unwrap();
        store
            .change_password(&initial.user, "Synthetic-password-42".into())
            .await
            .unwrap();
        let ipc = directory(&options.ipc_root, &a.account_id);
        let socket = ipc.join("personal.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
        let extensions = google
            .map(|t| crate::personal_google::routes_with_test_transport(store.clone(), owner, t))
            .unwrap_or_default();
        let router = moosuri_server::personal_service_router(store.clone(), None, extensions);
        let task = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        runtime_registry::set_routing(&options.registry, &a, true).unwrap();
        (a, store, task)
    }
    async fn json(response: Response) -> serde_json::Value {
        serde_json::from_slice(
            &axum::body::to_bytes(response.into_body(), MAX_THREAD_BODY)
                .await
                .unwrap(),
        )
        .unwrap()
    }
    fn authenticated(cookies: &str, a: &Allocation) -> HeaderMap {
        let mut h = headers();
        h.insert(header::COOKIE, HeaderValue::from_str(cookies).unwrap());
        h.insert(
            "x-moosuri-account",
            HeaderValue::from_str(&a.account_id).unwrap(),
        );
        h
    }
    const VAULT_STATUS: &str = "/api/v1/personal/vault";
    async fn check_vault_status(g: &Gateway, a: &HeaderMap, b: &HeaderMap) {
        assert_eq!(
            request(g, VAULT_STATUS, Method::GET, headers(), "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        for member in [a, b] {
            let status = request(g, VAULT_STATUS, Method::GET, member.clone(), "").await;
            assert_eq!(status.status(), StatusCode::OK);
            assert_eq!(status.headers()[header::CACHE_CONTROL], "no-store");
            assert!(!status.headers().contains_key(header::SET_COOKIE));
            assert_eq!(
                json(status).await,
                serde_json::json!({"state":"setup_required"})
            );
        }
        let mut mixed = a.clone();
        mixed.insert("x-moosuri-account", b["x-moosuri-account"].clone());
        assert_eq!(
            request(g, VAULT_STATUS, Method::GET, mixed, "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        let mut foreign = a.clone();
        foreign.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://other.example.test"),
        );
        assert_eq!(
            request(g, VAULT_STATUS, Method::GET, foreign, "")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(g, VAULT_STATUS, Method::POST, a.clone(), "{}")
                .await
                .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            request(g, "/api/v1/vault/handoff", Method::POST, a.clone(), "{}")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }

    async fn check_memory_lifecycle(g: &Gateway, a: &HeaderMap, b: &HeaderMap) {
        const BASE: &str = "/api/v1/knowledge/memories";
        let draft = |text: &str| {
            serde_json::json!({"text":text, "subject":"Synthetic schedule",
            "speaker":"Synthetic owner", "valid_from":null,"valid_until":null,"evidence":[]})
        };
        let create =
            serde_json::json!({"request_id":Uuid::new_v4(),"draft":draft("Synthetic Friday")})
                .to_string();
        let response = request(g, BASE, Method::POST, a.clone(), &create).await;
        assert_eq!(response.status(), StatusCode::CREATED);
        let memory = json(response).await;
        let path = format!("{BASE}/{}", memory["id"].as_str().unwrap());
        assert_eq!(
            json(request(g, BASE, Method::POST, a.clone(), &create).await).await["id"],
            memory["id"]
        );
        assert_eq!(
            json(request(g, BASE, Method::GET, b.clone(), "").await).await["items"],
            serde_json::json!([])
        );
        assert_eq!(
            request(g, &path, Method::GET, b.clone(), "").await.status(),
            StatusCode::NOT_FOUND
        );
        let correction = serde_json::json!({"request_id":Uuid::new_v4(),"expected_revision":1,
            "draft":draft("Synthetic Monday")})
        .to_string();
        let corrected = request(g, &path, Method::PUT, a.clone(), &correction).await;
        assert_eq!(corrected.status(), StatusCode::OK);
        assert_eq!(json(corrected).await["revision"], 2);
        let history =
            json(request(g, &format!("{path}/history"), Method::GET, a.clone(), "").await).await;
        assert_eq!(history["items"].as_array().unwrap().len(), 2);
        assert_eq!(
            request(
                g,
                &path,
                Method::DELETE,
                a.clone(),
                r#"{"expected_revision":1}"#
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert_eq!(
            request(
                g,
                &path,
                Method::DELETE,
                a.clone(),
                r#"{"expected_revision":2}"#
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            request(g, &path, Method::GET, a.clone(), "").await.status(),
            StatusCode::NOT_FOUND
        );
    }
    #[tokio::test]
    async fn actual_member_sockets_keep_cookie_locator_and_thread_history_private() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let mut options = options(&root);
        let (a, store_a, task_a) = real_member(&options, "a@example.test").await;
        let (b, _store_b, task_b) = real_member(&options, "b@example.test").await;
        let g = Gateway::new(&options).unwrap();
        let mut sessions = Vec::new();
        for email in ["a@example.test", "b@example.test"] {
            let login = request(
                &g,
                "/api/v1/auth/login",
                Method::POST,
                headers(),
                &serde_json::json!({"email":email,"password":"Synthetic-password-42"}).to_string(),
            )
            .await;
            assert_eq!(login.status(), StatusCode::OK);
            // Real unrestricted Store login uses a 400-day locator. Database
            // authorization keeps its separate sliding inactivity deadline.
            assert!(login.headers().get_all(header::SET_COOKIE).iter().all(|h| {
                h.to_str()
                    .unwrap()
                    .contains(&format!("Max-Age={}", moosuri_server::COOKIE_MAX_AGE))
            }));
            sessions.push(cookies(&login));
        }
        let a_headers = authenticated(&sessions[0], &a);
        let b_headers = authenticated(&sessions[1], &b);
        check_vault_status(&g, &a_headers, &b_headers).await;
        options.vault_site_origin = Some("https://vault.example.test".into());
        let with_site = Gateway::new(&options).unwrap();
        let status = request(&with_site, VAULT_STATUS, Method::GET, a_headers.clone(), "").await;
        assert_eq!(status.status(), StatusCode::OK);
        assert_eq!(
            json(status).await,
            serde_json::json!({"state":"setup_required","site_origin":"https://vault.example.test"})
        );
        let anonymous = request(&with_site, VAULT_STATUS, Method::GET, headers(), "").await;
        assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            json(anonymous).await,
            serde_json::json!({"code":"unauthorized"})
        );
        let absent = request(
            &g,
            "/api/v1/text-preview",
            Method::GET,
            a_headers.clone(),
            "",
        )
        .await;
        assert_eq!(absent.status(), StatusCode::NOT_FOUND);
        assert_eq!(json(absent).await["code"], "not_found");
        check_memory_lifecycle(&g, &a_headers, &b_headers).await;
        let key = Uuid::new_v4().to_string();
        let body = serde_json::json!({"title":"Only A history","request_id":key}).to_string();
        let created = request(
            &g,
            "/api/v1/threads",
            Method::POST,
            a_headers.clone(),
            &body,
        )
        .await;
        assert_eq!(created.status(), StatusCode::CREATED);
        let created = json(created).await;
        let replay = json(
            request(
                &g,
                "/api/v1/threads",
                Method::POST,
                a_headers.clone(),
                &body,
            )
            .await,
        )
        .await;
        assert_eq!(created, replay);
        let a_id = created["id"].as_str().unwrap();
        let b_thread = json(
            request(
                &g,
                "/api/v1/threads",
                Method::POST,
                b_headers.clone(),
                &serde_json::json!({"title":"Only B history","request_id":Uuid::new_v4()})
                    .to_string(),
            )
            .await,
        )
        .await;
        let listed =
            json(request(&g, "/api/v1/threads", Method::GET, a_headers.clone(), "").await).await;
        assert_eq!(listed["threads"].as_array().unwrap().len(), 1);
        assert_eq!(listed["threads"][0]["title"], "Only A history");
        let history = format!("/api/v1/threads/{a_id}/messages");
        let page = json(request(&g, &history, Method::GET, a_headers.clone(), "").await).await;
        assert_eq!(page["thread_id"], a_id);
        assert_eq!(page["turns"], serde_json::json!([]));
        let missing = request(&g, &history, Method::GET, b_headers.clone(), "").await;
        assert_eq!(missing.status(), StatusCode::NOT_FOUND);
        assert_eq!(json(missing).await["code"], "not_found");
        assert_eq!(
            request(
                &g,
                &format!("/api/v1/threads/{a_id}"),
                Method::DELETE,
                b_headers.clone(),
                ""
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &g,
                &format!("{history}?before=2&before=3"),
                Method::GET,
                a_headers.clone(),
                ""
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &g,
                &format!("{history}?before=1"),
                Method::GET,
                a_headers.clone(),
                ""
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(
                &g,
                &history,
                Method::POST,
                a_headers.clone(),
                r#"{"text":"must not start an agent"}"#
            )
            .await
            .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        assert_eq!(
            request(
                &g,
                &format!("/api/v1/threads/{a_id}"),
                Method::DELETE,
                a_headers.clone(),
                ""
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert_eq!(
            request(&g, &history, Method::GET, a_headers.clone(), "")
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        let listed =
            json(request(&g, "/api/v1/threads", Method::GET, b_headers.clone(), "").await).await;
        assert_eq!(listed["threads"][0]["id"], b_thread["id"]);
        assert!(store_a.claim_agent_run().await.unwrap().is_none());
        let changed = request(
            &g,
            "/api/v1/auth/password",
            Method::POST,
            a_headers.clone(),
            r#"{"password":"Changed-synthetic-password-42"}"#,
        )
        .await;
        assert_eq!(changed.status(), StatusCode::OK);
        assert!(
            changed
                .headers()
                .get_all(header::SET_COOKIE)
                .iter()
                .all(|h| h
                    .to_str()
                    .unwrap()
                    .contains(&format!("Max-Age={}", moosuri_server::COOKIE_MAX_AGE)))
        );
        let session = request(
            &g,
            "/api/v1/auth/session",
            Method::GET,
            authenticated(&cookies(&changed), &a),
            "",
        )
        .await;
        assert_eq!(session.status(), StatusCode::OK);
        assert!(!session.headers().contains_key(header::SET_COOKIE));
        // A password change revokes the private session without invalidating the
        // signed route to that member, so the status must check both.
        assert!(authenticated_allocation(&g, &a_headers, true).await.is_ok());
        assert_eq!(
            request(&g, VAULT_STATUS, Method::GET, a_headers, "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        runtime_registry::set_routing(&options.registry, &b, false).unwrap();
        assert_eq!(
            request(&g, VAULT_STATUS, Method::GET, b_headers, "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        task_a.abort();
        task_b.abort();
    }
    #[tokio::test]
    async fn fenced_generation_withholds_runtime_errors_after_the_request_started() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = options(&root);
        let a =
            runtime_registry::reserve(&options.registry, Uuid::new_v4(), "a@example.test".into())
                .unwrap();
        runtime_registry::set_routing(&options.registry, &a, true).unwrap();
        let ipc = directory(&options.ipc_root, &a.account_id);
        let path = ipc.join("personal.sock");
        let listener = tokio::net::UnixListener::bind(&path).unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        let entered = Arc::new(tokio::sync::Notify::new());
        let released = Arc::new(tokio::sync::Notify::new());
        let e = entered.clone();
        let r = released.clone();
        let app = Router::new().fallback(axum::routing::any(move || {
            let e = e.clone();
            let r = r.clone();
            async move {
                e.notify_one();
                r.notified().await;
                (
                    StatusCode::NOT_FOUND,
                    Json(serde_json::json!({"code":"not_found"})),
                )
            }
        }));
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        let g = Gateway::new(&options).unwrap();
        let session = "s".repeat(43);
        let route = g
            .sign(&Route {
                epoch: 0,
                account: Uuid::parse_str(&a.account_id).unwrap(),
                tenant: Uuid::parse_str(&a.tenant_id).unwrap(),
                volume: Uuid::parse_str(&a.volume_id).unwrap(),
                generation: a.generation,
                expires: now().unwrap() + 3600,
                session_hash: hash(&session),
            })
            .unwrap();
        let h = authenticated(&format!("{SESSION}={session}; {ROUTER}={route}"), &a);
        let gateway = g.clone();
        let history = format!("/api/v1/threads/{}/messages", Uuid::new_v4());
        let work =
            tokio::spawn(async move { request(&gateway, &history, Method::GET, h, "").await });
        entered.notified().await;
        runtime_registry::set_routing(&options.registry, &a, false).unwrap();
        released.notify_one();
        assert_eq!(work.await.unwrap().status(), StatusCode::UNAUTHORIZED);
        task.abort();
    }
    #[tokio::test]
    async fn opt_in_preview_crosses_gateway_and_member_socket_once_and_uses_saved_history() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = options(&root);
        let (a, store, old) = real_member(&options, "preview@example.test").await;
        old.abort();
        let _ = old.await;
        let personal = options.ipc_root.join(&a.account_id).join("personal.sock");
        fs::remove_file(&personal).unwrap();
        let model = root.join("model.sock");
        let listener = tokio::net::UnixListener::bind(&model).unwrap();
        fs::set_permissions(&model, fs::Permissions::from_mode(0o600)).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counted = calls.clone();
        let broker = tokio::spawn(async move {
            loop {
                let (mut stream, _) = listener.accept().await.unwrap();
                let mut bytes = Vec::new();
                loop {
                    let byte = stream.read_u8().await.unwrap();
                    if byte == b'\n' {
                        break;
                    }
                    bytes.push(byte);
                    assert!(bytes.len() < 64 * 1024);
                }
                let r: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
                assert_eq!(r["selection_epoch"], 0);
                if r["operation"] == "selection" {
                    let selection = serde_json::json!({
                        "v": 1, "state": "selection", "connection_revision": Uuid::from_u128(1),
                        "selection_epoch": 0, "model": "synthetic-text", "kind": "compatible",
                    });
                    let mut bytes = serde_json::to_vec(&selection).unwrap();
                    bytes.push(b'\n');
                    stream.write_all(&bytes).await.unwrap();
                    continue;
                }
                counted.fetch_add(1, Ordering::SeqCst);
                let reply = serde_json::json!({"v":1,"state":"settled","call_id":r["call_id"],"connection_revision":r["connection_revision"],"model":r["model"],"output":"Synthetic bounded text answer","outcome":"completed","receipt":{"state":"settled","input_tokens":7,"output_tokens":3,"charged_tokens":10}});
                let mut bytes = serde_json::to_vec(&reply).unwrap();
                bytes.push(b'\n');
                stream.write_all(&bytes).await.unwrap();
            }
        });
        let owner = moosuri_store::personal_volume::VolumeOwner {
            account: Uuid::parse_str(&a.account_id).unwrap(),
            tenant: Uuid::parse_str(&a.tenant_id).unwrap(),
            volume: Uuid::parse_str(&a.volume_id).unwrap(),
            generation: a.generation as u64,
        };
        let router = moosuri_server::personal_text_preview_router(
            store.clone(),
            owner,
            moosuri_server::personal_text::Options {
                socket: model,
                broker_uid: unsafe { libc::geteuid() },
                max_output_tokens: 128,
            },
        );
        let listener = tokio::net::UnixListener::bind(&personal).unwrap();
        fs::set_permissions(&personal, fs::Permissions::from_mode(0o600)).unwrap();
        let member = tokio::spawn(async move {
            axum::serve(listener, router).await.unwrap();
        });
        let g = Gateway::new(&options).unwrap();
        let login = request(
            &g,
            "/api/v1/auth/login",
            Method::POST,
            headers(),
            r#"{"email":"preview@example.test","password":"Synthetic-password-42"}"#,
        )
        .await;
        assert_eq!(login.status(), StatusCode::OK);
        let mut h = authenticated(&cookies(&login), &a);
        h.insert(
            moosuri_server::personal_text::SESSION_EPOCH_HEADER,
            HeaderValue::from_static("999"),
        );
        assert_eq!(
            json(request(&g, "/api/v1/text-preview", Method::GET, h.clone(), "").await).await["tools"],
            false
        );
        let thread = json(
            request(
                &g,
                "/api/v1/threads",
                Method::POST,
                h.clone(),
                &serde_json::json!({"title":"Preview","request_id":Uuid::new_v4()}).to_string(),
            )
            .await,
        )
        .await;
        let id = thread["id"].as_str().unwrap();
        let endpoint = format!("/api/v1/threads/{id}/text-preview");
        let body=serde_json::json!({"request_id":Uuid::new_v4(),"text":"Explain a queue","capability":"text_only_preview"}).to_string();
        let inbox=serde_json::json!({"request_id":Uuid::new_v4(),"text":"show me my latest email","capability":"text_only_preview"}).to_string();
        let handoff = request(&g, &endpoint, Method::POST, h.clone(), &inbox).await;
        assert_eq!(handoff.status(), StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(json(handoff).await["code"], "personal_inbox_requested");
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        let first = request(&g, &endpoint, Method::POST, h.clone(), &body).await;
        assert_eq!(first.status(), StatusCode::CREATED);
        let first = json(first).await;
        assert_eq!(first["run"]["state"], "completed");
        assert_eq!(first["run"]["usage"]["input_tokens"], 7);
        let duplicate = request(&g, &endpoint, Method::POST, h.clone(), &body).await;
        assert_eq!(duplicate.status(), StatusCode::OK);
        assert_eq!(json(duplicate).await["run"]["id"], first["run"]["id"]);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        let history = json(
            request(
                &g,
                &format!("/api/v1/threads/{id}/messages"),
                Method::GET,
                h.clone(),
                "",
            )
            .await,
        )
        .await;
        assert_eq!(history["turns"].as_array().unwrap().len(), 1);
        assert_eq!(
            history["turns"][0]["assistant"][0]["text"],
            "Synthetic bounded text answer"
        );
        assert_eq!(
            request(
                &g,
                &format!("/api/v1/threads/{id}/messages"),
                Method::POST,
                h.clone(),
                "{}"
            )
            .await
            .status(),
            StatusCode::METHOD_NOT_ALLOWED
        );
        runtime_registry::set_routing(&options.registry, &a, false).unwrap();
        assert_eq!(
            request(&g, &endpoint, Method::POST, h, &body)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        broker.abort();
        member.abort();
    }
    #[cfg(feature = "test-fixtures")]
    #[tokio::test]
    async fn google_callback_claims_only_the_originating_member_and_resumes_a_pinned_read() {
        use crate::runtime_google::tests::Fixture;
        let provider = Fixture::new().await;
        let (stop, broker) = provider.start().await;
        let temp = tempfile::tempdir().unwrap();
        let mut options = options(&temp.path().canonicalize().unwrap());
        options.registry = provider.registry();
        let g = Gateway::new(&options).unwrap();
        let mut clients = Vec::new();
        for (index, email) in ["a@example.test", "b@example.test"].into_iter().enumerate() {
            let a = runtime_registry::lookup(&options.registry, email.to_string()).unwrap();
            let (a, _store, member) =
                serve_test_member(&options, email, a, Some(provider.transport(index))).await;
            let login = request(
                &g,
                "/api/v1/auth/login",
                Method::POST,
                headers(),
                &serde_json::json!({"email":email,"password":"Synthetic-password-42"}).to_string(),
            )
            .await;
            assert_eq!(login.status(), StatusCode::OK);
            clients.push((authenticated(&cookies(&login), &a), member, a));
        }
        let base = "/api/v1/personal/google";
        let (a, b) = (&clients[0].0, &clients[1].0);
        let start = request(&g, &format!("{base}/start"), Method::POST, a.clone(), "{}").await;
        assert_eq!(start.status(), StatusCode::OK);
        let start = json(start).await;
        let url = url::Url::parse(start["authorization_url"].as_str().unwrap()).unwrap();
        let state = url
            .query_pairs()
            .find(|(k, _)| k == "state")
            .unwrap()
            .1
            .into_owned();
        let callback = format!("/api/v1/mail/oauth/callback?state={state}&code=synthetic-a");
        let mut no_cookie = HeaderMap::new();
        no_cookie.insert(
            header::HOST,
            HeaderValue::from_static("assistant.example.test"),
        );
        let mut wrong_origin = g.clone();
        wrong_origin.origin = "https://another.example.test".into();
        assert_eq!(
            request(&wrong_origin, &callback, Method::GET, no_cookie.clone(), "")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let mut wrong_state = URL_SAFE_NO_PAD.decode(&state).unwrap();
        wrong_state[16] ^= 1;
        let tampered = callback.replace(&state, &URL_SAFE_NO_PAD.encode(wrong_state));
        assert_eq!(
            request(&g, &tampered, Method::GET, no_cookie.clone(), "")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let returned = request(&g, &callback, Method::GET, no_cookie.clone(), "").await;
        assert!(returned.status().is_redirection());
        let fragment = returned.headers()[header::LOCATION]
            .to_str()
            .unwrap()
            .strip_prefix("/#")
            .unwrap();
        let fields: std::collections::BTreeMap<_, _> =
            url::form_urlencoded::parse(fragment.as_bytes())
                .into_owned()
                .collect();
        assert_eq!(fields["flow"], start["flow_id"]);
        assert_eq!(
            request(&g, &callback, Method::GET, no_cookie, "")
                .await
                .status(),
            StatusCode::BAD_REQUEST
        );
        let complete = format!("{base}/complete");
        let handle = serde_json::json!({"handle":fields["google_handle"]}).to_string();
        assert!(
            !request(&g, &complete, Method::POST, b.clone(), &handle)
                .await
                .status()
                .is_success()
        );
        let reply = request(&g, &complete, Method::POST, a.clone(), &handle).await;
        assert_eq!(reply.status(), StatusCode::OK);
        let grant = json(reply).await;
        assert_eq!(grant["flow_id"], start["flow_id"]);
        assert_eq!(grant["mailbox"], "a@example.test");
        let pin =
            serde_json::json!({"connection_id":grant["connection_id"],"epoch":grant["epoch"]})
                .to_string();
        let inbox = format!("{base}/inbox");
        assert!(
            !request(&g, &complete, Method::POST, a.clone(), &handle)
                .await
                .status()
                .is_success()
        );
        assert!(
            !request(&g, &inbox, Method::POST, b.clone(), &pin)
                .await
                .status()
                .is_success()
        );
        let page = request(&g, &inbox, Method::POST, a.clone(), &pin).await;
        assert_eq!(page.status(), StatusCode::OK);
        let page = json(page).await;
        assert_eq!(page["messages"][0]["id"], "message-a");
        assert_eq!(page["has_more"], true);
        assert_eq!(
            request(
                &g,
                &format!("{base}/disconnect"),
                Method::POST,
                a.clone(),
                &pin
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        assert!(
            !request(&g, &inbox, Method::POST, a.clone(), &pin)
                .await
                .status()
                .is_success()
        );
        stop.cancel();
        broker.await.unwrap().unwrap();
        for (_, member, _) in clients {
            member.abort();
        }
    }
    async fn check_engine_administration(
        g: &Gateway,
        registry: &std::path::Path,
        a: &Allocation,
        b: &Allocation,
        admin: &HeaderMap,
        member: &HeaderMap,
    ) {
        use moosuri_store::runtime_registry::engine::{self, ModelChoice};
        let engine_url = "/api/v1/personal/admin/engine";
        let member_url = "/api/v1/personal/admin/engine/member";
        let engine_id = Uuid::new_v4();
        let default = ModelChoice {
            model: "model-a".into(),
            effort: None,
        };
        let selected = ModelChoice {
            model: "model-b".into(),
            effort: Some("high".into()),
        };
        let aid = a.account_id.parse().unwrap();
        let bid = b.account_id.parse().unwrap();
        let publish = |connection| {
            engine::publish_models(
                registry,
                connection,
                moosuri_core::ProviderKind::OpenAi,
                &default,
                &[default.clone(), selected.clone()],
                &[aid, bid],
            )
            .unwrap()
        };
        publish(engine_id);
        let state = json(request(g, engine_url, Method::GET, admin.clone(), "").await).await;
        assert_eq!(state["configured"], true);
        let assignment = serde_json::json!({"revision":state["revision"],"connection_revision":engine_id,"account_id":bid,"generation":b.generation,"choice":selected});
        let submit = |headers, value: serde_json::Value| {
            let g = g.clone();
            async move { request(&g, member_url, Method::POST, headers, &value.to_string()).await }
        };
        assert_eq!(
            submit(member.clone(), assignment.clone()).await.status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(g, engine_url, Method::GET, member.clone(), "")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let mut unsupported = assignment.clone();
        unsupported["choice"]["effort"] = "unsupported".into();
        assert_eq!(
            submit(admin.clone(), unsupported).await.status(),
            StatusCode::BAD_REQUEST
        );
        let saved = submit(admin.clone(), assignment.clone()).await;
        assert_eq!(saved.status(), StatusCode::OK);
        let revision = json(saved).await["revision"].clone();
        assert_eq!(
            submit(admin.clone(), assignment).await.status(),
            StatusCode::CONFLICT
        );
        publish(engine_id);
        assert_eq!(
            engine::member_model_policy(registry, bid)
                .unwrap()
                .unwrap()
                .effort
                .as_deref(),
            Some("high")
        );
        assert_eq!(
            engine::member_model_policy(registry, aid)
                .unwrap()
                .unwrap()
                .model,
            "model-a"
        );
        let pause = serde_json::json!({"revision":revision,"connection_revision":engine_id,"enabled":false,"default_choice":null});
        let paused = request(
            &g,
            engine_url,
            Method::POST,
            admin.clone(),
            &pause.to_string(),
        )
        .await;
        assert_eq!(paused.status(), StatusCode::OK);
        assert!(engine::member_model_policy(registry, bid).is_err());
        let new_connection = Uuid::new_v4();
        publish(new_connection);
        assert!(
            engine::publish_models(
                registry,
                engine_id,
                moosuri_core::ProviderKind::OpenAi,
                &default,
                &[default.clone(), selected.clone()],
                &[aid, bid]
            )
            .is_err()
        );
        let state = json(request(g, engine_url, Method::GET, admin.clone(), "").await).await;
        assert_eq!(state["enabled"], false);
        let resume = serde_json::json!({"revision":state["revision"],"connection_revision":new_connection,"enabled":true,"default_choice":selected});
        let state = json(
            request(
                g,
                engine_url,
                Method::POST,
                admin.clone(),
                &resume.to_string(),
            )
            .await,
        )
        .await;
        assert!(
            engine::member_model_policy(registry, bid).is_err(),
            "old connection assignment must be reviewed"
        );
        let reset = serde_json::json!({"revision":state["revision"],"connection_revision":new_connection,"account_id":bid,"generation":b.generation,"choice":null});
        assert_eq!(submit(admin.clone(), reset).await.status(), StatusCode::OK);
        assert_eq!(
            engine::member_model_policy(registry, bid)
                .unwrap()
                .unwrap()
                .model,
            "model-b"
        );
    }
    #[tokio::test]
    async fn private_member_session_and_host_role_control_web_admin_even_after_demotion() {
        use moosuri_store::runtime_registry::admin as control;
        let temp = tempfile::tempdir().unwrap();
        let options = options(&temp.path().canonicalize().unwrap());
        let (a, _, a_task) = real_member(&options, "a@example.test").await;
        let (b, _, b_task) = real_member(&options, "b@example.test").await;
        let g = Gateway::new(&options).unwrap();
        let mut auth = Vec::new();
        for (allocated, email) in [(&a, "a@example.test"), (&b, "b@example.test")] {
            let login = request(
                &g,
                "/api/v1/auth/login",
                Method::POST,
                headers(),
                &serde_json::json!({"email":email,"password":"Synthetic-password-42"}).to_string(),
            )
            .await;
            assert_eq!(login.status(), StatusCode::OK);
            let h = authenticated(&cookies(&login), allocated);
            let view = json(login).await;
            assert_eq!(view["installation_admin"], false);
            auth.push((
                h,
                serde_json::from_value::<moosuri_core::SessionView>(view).unwrap(),
            ));
        }
        control::bootstrap(&options.registry, Uuid::new_v4(), &a, &auth[0].1).unwrap();
        let collection = "/api/v1/personal/admin/members";
        let list = request(&g, collection, Method::GET, auth[0].0.clone(), "").await;
        assert_eq!(list.status(), StatusCode::OK);
        assert_eq!(
            request(&g, collection, Method::GET, auth[1].0.clone(), "")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        check_engine_administration(&g, &options.registry, &a, &b, &auth[0].0, &auth[1].0).await;
        let rows = json(list).await;
        let row = |id: &str| {
            rows["members"]
                .as_array()
                .unwrap()
                .iter()
                .find(|m| m["account_id"] == id)
                .unwrap()
        };
        let mutation = |m: &serde_json::Value, action: &str| {
            serde_json::json!({"request_id":Uuid::new_v4(),"account_id":m["account_id"],"generation":m["generation"],"revision":m["revision"],"action":action}).to_string()
        };
        let change = format!("{collection}/change");
        let last = mutation(row(&a.account_id), "suspend");
        let protected = request(&g, &change, Method::POST, auth[0].0.clone(), &last).await;
        assert!(!protected.status().is_success());
        let invitation=serde_json::json!({"request_id":Uuid::new_v4(),"name":"Guest","email":"guest@example.test"}).to_string();
        for _ in 0..2 {
            assert_eq!(
                request(
                    &g,
                    "/api/v1/personal/admin/invitations",
                    Method::POST,
                    auth[0].0.clone(),
                    &invitation
                )
                .await
                .status(),
                StatusCode::OK
            );
        }
        let guest =
            runtime_registry::lookup(&options.registry, "guest@example.test".into()).unwrap();
        assert!(
            runtime_registry::lookup_id(&options.registry, guest.account_id.parse().unwrap())
                .is_err()
        );
        let promote = mutation(row(&b.account_id), "promote");
        assert_eq!(
            request(&g, &change, Method::POST, auth[0].0.clone(), &promote)
                .await
                .status(),
            StatusCode::OK
        );
        let demote = mutation(row(&a.account_id), "demote");
        assert_eq!(
            request(&g, &change, Method::POST, auth[1].0.clone(), &demote)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(&g, collection, Method::GET, auth[0].0.clone(), "")
                .await
                .status(),
            StatusCode::FORBIDDEN
        );
        let list = json(request(&g, collection, Method::GET, auth[1].0.clone(), "").await).await;
        let current = list["members"]
            .as_array()
            .unwrap()
            .iter()
            .find(|m| m["account_id"] == a.account_id)
            .unwrap();
        let suspended = json(
            request(
                &g,
                &change,
                Method::POST,
                auth[1].0.clone(),
                &mutation(current, "suspend"),
            )
            .await,
        )
        .await;
        assert_eq!(suspended["suspended"], true);
        let resumed = request(
            &g,
            &change,
            Method::POST,
            auth[1].0.clone(),
            &mutation(&suspended, "resume"),
        )
        .await;
        assert_eq!(resumed.status(), StatusCode::OK);
        assert_eq!(
            request(
                &g,
                "/api/v1/auth/session",
                Method::GET,
                auth[0].0.clone(),
                ""
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
        let fresh = request(
            &g,
            "/api/v1/auth/login",
            Method::POST,
            headers(),
            r#"{"email":"a@example.test","password":"Synthetic-password-42"}"#,
        )
        .await;
        assert_eq!(fresh.status(), StatusCode::OK);
        let logout = request(
            &g,
            "/api/v1/auth/logout",
            Method::POST,
            auth[1].0.clone(),
            "{}",
        )
        .await;
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        assert_eq!(
            request(&g, collection, Method::GET, auth[1].0.clone(), "")
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        a_task.abort();
        b_task.abort();
    }
    #[tokio::test]
    async fn public_preview_entry_serves_built_dependencies_without_a_session_or_api_fallback() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        let options = options(&root);
        let gateway = Gateway::new(&options).unwrap();
        let mut h = headers();
        h.remove("origin");
        h.remove("x-real-ip");
        let response = request(&gateway, "/", Method::GET, h.clone(), "").await;
        assert_eq!(response.status(), StatusCode::OK);
        assert!(!response.headers().contains_key(header::SET_COOKIE));
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        assert!(
            response.headers()[header::CONTENT_SECURITY_POLICY]
                .to_str()
                .unwrap()
                .contains("connect-src 'self'")
        );
        let bytes = axum::body::to_bytes(response.into_body(), 512 * 1024)
            .await
            .unwrap();
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(html.contains("<title>Moosuri</title>"));
        assert!(html.contains("href=\"/personal.webmanifest\""));
        let manifest = request(
            &gateway,
            "/personal.webmanifest",
            Method::GET,
            h.clone(),
            "",
        )
        .await;
        assert_eq!(manifest.status(), StatusCode::OK);
        let manifest: serde_json::Value = serde_json::from_slice(
            &axum::body::to_bytes(manifest.into_body(), 8192)
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(manifest["start_url"], "/app");
        assert!(manifest.get("share_target").is_none());
        let script = html
            .split("src=\"")
            .nth(1)
            .unwrap()
            .split('"')
            .next()
            .unwrap();
        assert!(script.starts_with("/assets/"));
        let dependency = request(&gateway, script, Method::GET, h.clone(), "").await;
        assert_eq!(dependency.status(), StatusCode::OK);
        assert!(
            dependency.headers()[header::CONTENT_TYPE]
                .to_str()
                .unwrap()
                .contains("javascript")
        );
        for path in [
            "/api/v1/admin/users",
            "/api/v1/unavailable",
            "/assets/../personal.html",
            "/settings/mail",
        ] {
            assert_eq!(
                request(&gateway, path, Method::GET, h.clone(), "")
                    .await
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        h.insert("host", HeaderValue::from_static("foreign.example.test"));
        assert_eq!(
            request(&gateway, "/", Method::GET, h, "").await.status(),
            StatusCode::BAD_REQUEST
        );
    }
}
