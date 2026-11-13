use crate::*;
use axum::body::Body;
include!(concat!(env!("OUT_DIR"), "/assets.rs"));
pub fn has_assets() -> bool {
    ASSETS.iter().any(|(p, _)| *p == "/index.html")
}
use axum::{
    Form, Json, Router,
    extract::{DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, Uri, header},
    middleware::{self, Next},
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use protocol::{AuthorityReply, AuthorityRequest, Operation, Owner};
use serde::Deserialize;
use std::sync::{Arc, Mutex};
use zeroize::Zeroizing;

#[derive(Clone)]
pub struct Adapter {
    client: reqwest::Client,
    url: String,
    key: Arc<Zeroizing<String>>,
}
impl Adapter {
    pub fn new(url: &str, key: String) -> Result<Self> {
        let key = Zeroizing::new(key);
        let u = url::Url::parse(url).map_err(|_| Error::Invalid)?;
        if u.scheme() != "http"
            || !matches!(u.host(),Some(url::Host::Ipv4(ip)) if ip.is_loopback())
            || !u.username().is_empty()
            || u.password().is_some()
            || u.query().is_some()
            || u.fragment().is_some()
            || u.path() != "/internal/v1/vault"
            || key.len() != 64
            || !key.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::Invalid);
        }
        let client = reqwest::Client::builder()
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .connect_timeout(std::time::Duration::from_secs(2))
            .timeout(std::time::Duration::from_secs(4))
            .build()
            .map_err(|_| Error::Unavailable)?;
        Ok(Self {
            client,
            url: u.to_string(),
            key: Arc::new(key),
        })
    }
    pub async fn call(&self, input: &AuthorityRequest) -> Result<AuthorityReply> {
        let bytes = serde_json::to_vec(input).map_err(|_| Error::Invalid)?;
        if bytes.len() > 4096 {
            return Err(Error::Limit);
        }
        let mut response = self
            .client
            .post(&self.url)
            .header("x-moosuri-vault-adapter", self.key.as_str())
            .header("content-type", "application/json")
            .body(bytes)
            .send()
            .await
            .map_err(|_| Error::Unavailable)?;
        if response.status().as_u16() == 401 || response.status().as_u16() == 403 {
            return Err(Error::Unauthorized);
        }
        if !response.status().is_success() {
            return Err(Error::Unavailable);
        }
        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| Error::Unavailable)? {
            if body.len() + chunk.len() > 4096 {
                return Err(Error::Limit);
            }
            body.extend(chunk)
        }
        serde_json::from_slice(&body).map_err(|_| Error::Unavailable)
    }
}
#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub assistant_origin: String,
    pub authority: String,
    pub secure: bool,
}
fn origin(raw: &str, dev: bool) -> Result<(String, String, bool)> {
    let u = url::Url::parse(raw).map_err(|_| Error::Invalid)?;
    if !u.username().is_empty()
        || u.password().is_some()
        || u.query().is_some()
        || u.fragment().is_some()
        || u.path() != "/"
    {
        return Err(Error::Invalid);
    }
    let secure = u.scheme() == "https";
    let local = matches!(u.host(),Some(url::Host::Ipv4(ip))if ip.is_loopback())
        || u.host_str() == Some("localhost");
    if !(secure || dev && local && u.scheme() == "http") {
        return Err(Error::Invalid);
    }
    let o = u.origin().ascii_serialization();
    let a = o.split_once("://").ok_or(Error::Invalid)?.1.to_string();
    Ok((o, a, secure))
}
impl Config {
    pub fn new(vault: &str, assistant: &str, dev: bool) -> Result<Self> {
        let (o, a, s) = origin(vault, dev)?;
        let (other, _, _) = origin(assistant, dev)?;
        if o == other
            || url::Url::parse(&o).map_err(|_| Error::Invalid)?.host_str()
                == url::Url::parse(&other)
                    .map_err(|_| Error::Invalid)?
                    .host_str()
        {
            return Err(Error::Invalid);
        }
        Ok(Self {
            origin: o,
            authority: a,
            secure: s,
            assistant_origin: other,
        })
    }
}
#[derive(Clone)]
struct Service {
    db: Arc<Mutex<database::Database>>,
    adapter: Adapter,
    config: Config,
    jobs: Arc<tokio::sync::Semaphore>,
}
impl IntoResponse for Error {
    fn into_response(self) -> Response {
        let code = match self {
            Self::Unauthorized => StatusCode::UNAUTHORIZED,
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::Conflict => StatusCode::CONFLICT,
            Self::Limit => StatusCode::PAYLOAD_TOO_LARGE,
            Self::Invalid => StatusCode::BAD_REQUEST,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        };
        (code, Json(serde_json::json!({"code":self.to_string()}))).into_response()
    }
}
pub fn router(db: database::Database, adapter: Adapter, config: Config) -> Router {
    let state = Service {
        db: Arc::new(Mutex::new(db)),
        adapter,
        config,
        jobs: Arc::new(tokio::sync::Semaphore::new(2)),
    };
    Router::new()
        .route("/health/live", get(|| async { StatusCode::NO_CONTENT }))
        .route("/public/config", get(public_config))
        .route(
            "/session",
            post(exchange).layer(DefaultBodyLimit::max(4096)),
        )
        .route("/session/current", get(current))
        .route("/session/logout", post(logout))
        .route("/api/state", get(state_read))
        .route("/api/operations", post(prepare))
        .route("/api/operations/{id}", get(receipt))
        .route("/api/operations/{id}/commit", post(commit))
        .route("/api/operations/{id}/cancel", post(cancel))
        .fallback(asset)
        .layer(DefaultBodyLimit::max(MAX_BODY))
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}
async fn boundary(State(s): State<Service>, request: Request, next: Next) -> Response {
    if request
        .headers()
        .get(header::HOST)
        .and_then(|v| v.to_str().ok())
        != Some(s.config.authority.as_str())
    {
        return Error::Invalid.into_response();
    }
    if request.uri().path().starts_with("/api/") {
        let valid = cookie(&s, request.headers())
            .and_then(|(c, _)| s.db.lock().map_err(|_| Error::Unavailable)?.session(&c));
        if let Err(e) = valid {
            return e.into_response();
        }
    }
    let _permit = match s.jobs.clone().try_acquire_owned() {
        Ok(p) => p,
        Err(_) => return Error::Unavailable.into_response(),
    };
    if !matches!(*request.method(), Method::GET | Method::HEAD) {
        let o = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|v| v.to_str().ok());
        let allowed = if request.uri().path() == "/session" {
            o == Some(s.config.assistant_origin.as_str())
        } else {
            o == Some(s.config.origin.as_str())
        };
        if !allowed {
            return Error::Unauthorized.into_response();
        }
    }
    let mut response =
        match tokio::time::timeout(std::time::Duration::from_secs(30), next.run(request)).await {
            Ok(r) => r,
            Err(_) => Error::Unavailable.into_response(),
        };
    let h = response.headers_mut();
    h.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    h.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    h.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    h.insert(header::CONTENT_SECURITY_POLICY,HeaderValue::from_static("default-src 'none'; script-src 'self' 'wasm-unsafe-eval'; worker-src 'self'; style-src 'self'; connect-src 'self'; img-src 'self'; font-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'"));
    response
}
async fn public_config(State(s): State<Service>) -> Json<serde_json::Value> {
    Json(serde_json::json!({"assistant_origin":s.config.assistant_origin}))
}
fn cookie_name(s: &Service) -> &'static str {
    if s.config.secure {
        "__Host-moosuri_vault"
    } else {
        "moosuri_vault_dev"
    }
}
fn cookie(s: &Service, h: &HeaderMap) -> Result<(String, String)> {
    let mut found = None;
    for line in h.get_all(header::COOKIE) {
        for pair in line.to_str().map_err(|_| Error::Unauthorized)?.split(';') {
            if let Some((name, value)) = pair.trim().split_once('=')
                && name == cookie_name(s)
            {
                if found.is_some() {
                    return Err(Error::Unauthorized);
                }
                found = Some(value.to_string());
            }
        }
    }
    let value = found.ok_or(Error::Unauthorized)?;
    let (nonce, grant) = value.split_once('.').ok_or(Error::Unauthorized)?;
    unhex(nonce).map_err(|_| Error::Unauthorized)?;
    unhex(grant).map_err(|_| Error::Unauthorized)?;
    Ok((value.clone(), grant.into()))
}
async fn authorized(s: &Service, h: &HeaderMap, expected: bool) -> Result<(Owner, String)> {
    let (c, g) = cookie(s, h)?;
    let local = s.db.lock().map_err(|_| Error::Unavailable)?.session(&c)?;
    let reply = s
        .adapter
        .call(&AuthorityRequest::Validate { grant: g.clone() })
        .await?;
    if reply.owner != local {
        return Err(Error::Unauthorized);
    }
    if expected
        && h.get("x-moosuri-vault-account")
            .and_then(|v| v.to_str().ok())
            != Some(local.account.to_string().as_str())
    {
        return Err(Error::Unauthorized);
    }
    Ok((local, g))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Exchange {
    handoff: String,
}
async fn exchange(
    State(s): State<Service>,
    input: std::result::Result<Form<Exchange>, axum::extract::rejection::FormRejection>,
) -> Result<Response> {
    let Form(input) = input.map_err(|_| Error::Invalid)?;
    if input.handoff.len() != 64 {
        return Err(Error::Invalid);
    }
    let result = s
        .adapter
        .call(&AuthorityRequest::Exchange {
            handoff: input.handoff,
            vault_origin: s.config.origin.clone(),
        })
        .await?;
    let grant = result.grant.ok_or(Error::Unavailable)?;
    unhex(&grant)?;
    let value = format!("{}.{}", token()?, grant);
    s.db.lock()
        .map_err(|_| Error::Unavailable)?
        .create_session(&value, &result.owner)?;
    let mut response = Redirect::to("/").into_response();
    let secure = if s.config.secure { "; Secure" } else { "" };
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}={value}; Path=/; HttpOnly; SameSite=Strict; Max-Age=86400{secure}",
            cookie_name(&s)
        ))
        .map_err(|_| Error::Unavailable)?,
    );
    Ok(response)
}
async fn current(State(s): State<Service>, h: HeaderMap) -> Result<Json<protocol::CurrentSession>> {
    let (owner, _) = authorized(&s, &h, false).await?;
    let head = s.db.lock().map_err(|_| Error::Unavailable)?.head(&owner)?;
    Ok(Json(protocol::CurrentSession { owner, head }))
}
async fn state_read(State(s): State<Service>, h: HeaderMap) -> Result<Json<protocol::VaultState>> {
    let (o, _) = authorized(&s, &h, true).await?;
    Ok(Json(
        s.db.lock().map_err(|_| Error::Unavailable)?.state(&o)?,
    ))
}
async fn prepare(
    State(s): State<Service>,
    h: HeaderMap,
    input: std::result::Result<Json<Operation>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<protocol::Receipt>> {
    let Json(op) = input.map_err(|_| Error::Invalid)?;
    let (o, _) = authorized(&s, &h, true).await?;
    Ok(Json(
        s.db.lock()
            .map_err(|_| Error::Unavailable)?
            .prepare(&o, op)?,
    ))
}
async fn receipt(
    State(s): State<Service>,
    h: HeaderMap,
    Path(raw): Path<String>,
) -> Result<Json<protocol::OperationRecord>> {
    let id = raw.parse().map_err(|_| Error::Invalid)?;
    let (o, _) = authorized(&s, &h, true).await?;
    let (operation, receipt) =
        s.db.lock()
            .map_err(|_| Error::Unavailable)?
            .operation(&o, id)?;
    Ok(Json(protocol::OperationRecord { operation, receipt }))
}
async fn commit(
    State(s): State<Service>,
    h: HeaderMap,
    Path(raw): Path<String>,
) -> Result<Json<protocol::Receipt>> {
    let id = raw.parse().map_err(|_| Error::Invalid)?;
    let (o, g) = authorized(&s, &h, true).await?;
    let (_, r) =
        s.db.lock()
            .map_err(|_| Error::Unavailable)?
            .operation(&o, id)?;
    let reply = s
        .adapter
        .call(&AuthorityRequest::Commit {
            grant: g,
            operation: id,
            digest: r.payload_sha256.clone(),
        })
        .await?;
    if reply.owner != o
        || reply.operation != Some(id)
        || reply.digest.as_deref() != Some(&r.payload_sha256)
    {
        return Err(Error::Unauthorized);
    }
    Ok(Json(s.db.lock().map_err(|_| Error::Unavailable)?.commit(
        &o,
        id,
        &r.payload_sha256,
    )?))
}
async fn logout(State(s): State<Service>, h: HeaderMap) -> Result<Response> {
    let (c, g) = cookie(&s, &h)?;
    // Local close is durable even if authority is unavailable; it grants no read.
    s.db.lock()
        .map_err(|_| Error::Unavailable)?
        .close_session(&c)?;
    let _ = s.adapter.call(&AuthorityRequest::Close { grant: g }).await;
    let mut r = StatusCode::NO_CONTENT.into_response();
    let secure = if s.config.secure { "; Secure" } else { "" };
    r.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{}=; Path=/; HttpOnly; SameSite=Strict; Max-Age=0{secure}",
            cookie_name(&s)
        ))
        .map_err(|_| Error::Unavailable)?,
    );
    Ok(r)
}

async fn cancel(
    State(s): State<Service>,
    h: HeaderMap,
    Path(raw): Path<String>,
    input: std::result::Result<Json<Operation>, axum::extract::rejection::JsonRejection>,
) -> Result<Json<protocol::Receipt>> {
    let Json(op) = input.map_err(|_| Error::Invalid)?;
    let id = raw.parse().map_err(|_| Error::Invalid)?;
    let (o, _) = authorized(&s, &h, true).await?;
    Ok(Json(
        s.db.lock()
            .map_err(|_| Error::Unavailable)?
            .cancel(&o, id, op)?,
    ))
}

async fn asset(uri: Uri) -> Response {
    let path = if uri.path() == "/" {
        "/index.html"
    } else {
        uri.path()
    };
    let Some((_, bytes)) = ASSETS.iter().find(|(name, _)| *name == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mime = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("wasm") => "application/wasm",
        _ => "application/octet-stream",
    };
    let mut r = Response::new(Body::from(*bytes));
    r.headers_mut()
        .insert(header::CONTENT_TYPE, mime.parse().unwrap());
    r
}
