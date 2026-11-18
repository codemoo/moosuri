//! Opt-in private authentication gateway. TLS and trusted client-IP assignment
//! belong to the local front proxy; this module binds no TCP listener.
use axum::{
    Json, Router,
    body::Body,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    response::{IntoResponse, Response},
    routing::{get, post},
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

const ROUTER: &str = "__Host-moosuri_route";
const SESSION: &str = "moosuri_session";
const MAX_BODY: usize = 64 * 1024;
const MAX_AGE: u64 = moosuri_server::COOKIE_MAX_AGE;
const WINDOW: Duration = Duration::from_secs(300);

#[derive(Args)]
pub struct Options {
    /// Exact HTTPS public origin. The local front proxy must overwrite X-Real-IP.
    #[arg(long)]
    public_origin: String,
    /// Existing private absolute installation registry directory.
    #[arg(long)]
    registry: PathBuf,
    /// Private absolute parent of account UUID directories containing personal.sock.
    #[arg(long)]
    ipc_root: PathBuf,
    /// Existing service-owner-only directory for gateway.sock and its persistent key.
    #[arg(long)]
    socket_dir: PathBuf,
}
#[derive(Clone)]
struct Gateway {
    origin: String,
    host: String,
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
        Ok(Self {
            host: origin[8..].into(),
            origin,
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
    fn socket(&self, a: &Allocation) -> Result<PathBuf, StatusCode> {
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
        Ok(path)
    }
    async fn proxy(
        &self,
        a: &Allocation,
        method: Method,
        path: &str,
        body: Option<&[u8]>,
        session: Option<&str>,
    ) -> Result<(StatusCode, Vec<u8>, Option<(String, u64)>), StatusCode> {
        let socket = self.socket(a)?;
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(Duration::from_secs(2))
            .timeout(Duration::from_secs(10))
            .unix_socket(socket)
            .build()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        // HTTP is confined to the protected local socket. The runtime's HTTPS
        // security origin stays fixed and never comes from browser input.
        let mut request = client
            .request(method, format!("http://personal.internal{path}"))
            .header("Host", "personal.internal")
            .header("Origin", "https://personal.internal")
            .header("X-Moosuri-Account", &a.account_id);
        if let Some(session) = session {
            request = request.header("Cookie", format!("{SESSION}={session}"));
        }
        if let Some(body) = body {
            request = request
                .header("Content-Type", "application/json")
                .body(body.to_vec());
        }
        let mut response = request
            .send()
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let status = response.status();
        if ![200, 204, 400, 401, 403, 409, 429, 503].contains(&status.as_u16()) {
            return Err(StatusCode::SERVICE_UNAVAILABLE);
        }
        let mut result = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        {
            if result.len() + chunk.len() > MAX_BODY {
                return Err(StatusCode::SERVICE_UNAVAILABLE);
            }
            result.extend_from_slice(&chunk);
        }
        let set = if status.is_success()
            && path != "/api/v1/auth/session"
            && path != "/api/v1/auth/logout"
        {
            Some(runtime_cookie(response.headers())?)
        } else {
            None
        };
        Ok((status, result, set))
    }
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
async fn allocation(g: &Gateway, email: String) -> Result<Allocation, StatusCode> {
    let registry = g.registry.clone();
    tokio::task::spawn_blocking(move || runtime_registry::lookup_ready(&registry, email))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(|e| {
            if matches!(e, moosuri_store::StoreError::NotFound) {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        })
}
async fn active(g: &Gateway, route: &Route) -> Result<Allocation, StatusCode> {
    let registry = g.registry.clone();
    let id = route.account;
    let a = tokio::task::spawn_blocking(move || runtime_registry::lookup_id(&registry, id))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
        .map_err(|e| {
            if matches!(e, moosuri_store::StoreError::NotFound) {
                StatusCode::UNAUTHORIZED
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        })?;
    if a.tenant_id != route.tenant.to_string()
        || a.volume_id != route.volume.to_string()
        || a.generation != route.generation
    {
        return Err(StatusCode::UNAUTHORIZED);
    }
    Ok(a)
}
async fn recheck(g: &Gateway, allocation: &Allocation) -> Result<(), StatusCode> {
    let route = Route {
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
    // Axum's GET router also recognizes HEAD; authentication exposes only the
    // four explicit method/path pairs and never forwards implicit variants.
    if (uri.path() == "/api/v1/auth/session" && method != Method::GET)
        || (uri.path() != "/api/v1/auth/session" && method != Method::POST)
    {
        return Err(StatusCode::METHOD_NOT_ALLOWED);
    }
    let ip = g.headers(headers, &method)?;
    let _permit = g
        .inflight
        .try_acquire()
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if uri.query().is_some() {
        return Err(StatusCode::BAD_REQUEST);
    }
    let path = uri.path();
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
        let (status, body, set) = g.proxy(&a, method, path, Some(&bytes), None).await?;
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
        return issued(
            g,
            &a,
            status,
            body,
            set.ok_or(StatusCode::SERVICE_UNAVAILABLE)?,
        );
    }
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
    } else if method != Method::GET {
        return Err(StatusCode::UNAUTHORIZED);
    }
    let a = active(g, &route).await?;
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
    let (status, body, set) = g
        .proxy(
            &a,
            method,
            path,
            payload.as_ref().map(|p| p.as_slice()),
            Some(&session),
        )
        .await?;
    if !status.is_success() {
        return Err(status);
    }
    recheck(g, &a).await?;
    if path == "/api/v1/auth/logout" {
        return Ok(response(status, body));
    }
    check_user(&body, &a)?;
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
    a: &Allocation,
    status: StatusCode,
    body: Vec<u8>,
    set: (String, u64),
) -> Result<Response, StatusCode> {
    check_user(&body, a)?;
    let route = Route {
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
fn router(g: Gateway) -> Router {
    Router::new()
        .route("/api/v1/auth/login", post(handler))
        .route("/api/v1/auth/session", get(handler))
        .route("/api/v1/auth/password", post(handler))
        .route("/api/v1/auth/logout", post(handler))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(g)
}
pub async fn run(options: &Options) -> Result<(), String> {
    private_dir(&options.socket_dir)?;
    let lock = private_file(&options.socket_dir.join("gateway.lock"), true)?;
    lock.try_lock_exclusive()
        .map_err(|_| "gateway_already_running")?;
    let g = Gateway::new(options)?;
    let socket = options.socket_dir.join("gateway.sock");
    match fs::symlink_metadata(&socket) {
        Ok(meta) => {
            if !meta.file_type().is_socket()
                || meta.uid() != unsafe { libc::geteuid() }
                || meta.mode() & 0o077 != 0
            {
                return Err("gateway_socket_invalid".into());
            }
            fs::remove_file(&socket).map_err(|_| "gateway_socket_unavailable")?;
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(_) => return Err("gateway_socket_unavailable".into()),
    }
    let listener =
        tokio::net::UnixListener::bind(&socket).map_err(|_| "gateway_socket_unavailable")?;
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600))
        .map_err(|_| "gateway_socket_unavailable")?;
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
            registry: directory(parent, "registry"),
            ipc_root: directory(parent, "ipc"),
            socket_dir: directory(parent, "gateway"),
        }
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
        let app=Router::new().fallback(axum::routing::any(move|method:Method,uri:Uri,h:HeaderMap,body:axum::body::Bytes|{let account=account.clone();let token=token.clone();let count=counted.clone();async move {
            assert_eq!(one(&h,"host"),Some("personal.internal"));assert_eq!(one(&h,"origin"),Some("https://personal.internal"));assert_eq!(one(&h,"x-moosuri-account"),Some(account.as_str()));
            assert!(!h.contains_key("x-real-ip"));count.fetch_add(1,Ordering::SeqCst);
            if uri.path()=="/api/v1/auth/logout" {return StatusCode::NO_CONTENT.into_response();}
            if uri.path()!="/api/v1/auth/login" {assert_eq!(cookie(&h,SESSION,128).unwrap(),token);}
            else {assert_eq!(method,Method::POST);let value:serde_json::Value=serde_json::from_slice(&body).unwrap();assert_eq!(value["password"],"Synthetic-password-42");assert!(!h.contains_key(header::COOKIE));}
            let mut response=Json(serde_json::json!({"user":{"id":account},"password_change_required":false,"chat_draft_scope":"synthetic"})).into_response();
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
}
