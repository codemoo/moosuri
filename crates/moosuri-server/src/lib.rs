//! HTTP boundary. Authentication, tenant scoping and persistence live in Store.
mod accounts;
mod artifacts;
mod automatic_memory;
mod browser;
mod chat;
mod community_skills;
mod google_setup;
mod knowledge;
mod mail;
mod media;
mod providers;
mod schedules;
mod source_connections;
use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, Path, Request, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use moosuri_core::{ApiError, SessionView, SystemStatus, ThreadsResponse};
use moosuri_store::{Store, StoreError, UserContext};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::{HashMap, VecDeque},
    net::{IpAddr, SocketAddr},
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

const COOKIE: &str = "moosuri_session";
// Database authorization expires after 30 idle days. The longer-lived locator cookie
// allows sliding activity without stale GET responses overwriting a new account cookie.
const COOKIE_MAX_AGE: u64 = 400 * 24 * 60 * 60;

#[cfg(test)]
mod web_configuration_tests {
    use super::*;
    #[test]
    fn browser_is_disabled_by_default_and_configuration_errors_are_static() {
        let config = ServerConfig::new("https://example.test", false).unwrap();
        assert!(config.browser.is_none());
        assert!(matches!(
            config.with_browser_capsule_json(r#"{"credential":"do-not-echo"}"#),
            Err("invalid_browser_configuration")
        ));
    }
    #[test]
    fn operator_web_configuration_advertises_only_actual_search_availability_and_redacts_invalid_keys()
     {
        let config = ServerConfig::new("https://example.test", false).unwrap();
        assert!(config.web.is_none());
        let read = config.clone().with_public_web(None).unwrap();
        assert!(!read.web.as_ref().unwrap().has_search());
        let search = config
            .clone()
            .with_public_web(Some("synthetic-key".into()))
            .unwrap();
        assert!(search.web.as_ref().unwrap().has_search());
        assert!(matches!(
            config.with_public_web(Some("sensitive-invalid key".into())),
            Err("invalid_web_configuration")
        ));
    }
}

#[derive(Clone)]
pub struct ServerConfig {
    origin: String,
    authority: String,
    secure: bool,
    trusted_proxy: Option<(IpAddr, header::HeaderName)>,
    google: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
    web: Option<Arc<moosuri_connectors::web::WebClient>>,
    browser: Option<Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
}
impl ServerConfig {
    pub fn new(origin: &str, allow_http_loopback: bool) -> Result<Self, &'static str> {
        let parsed = url::Url::parse(origin).map_err(|_| "invalid_origin")?;
        if !parsed.username().is_empty()
            || parsed.password().is_some()
            || parsed.query().is_some()
            || parsed.fragment().is_some()
            || parsed.path() != "/"
            || parsed.host_str().is_none()
        {
            return Err("invalid_origin");
        }
        let local = match parsed.host() {
            Some(url::Host::Domain("localhost")) => true,
            Some(url::Host::Ipv4(ip)) => ip.is_loopback(),
            Some(url::Host::Ipv6(ip)) => ip.is_loopback(),
            _ => false,
        };
        let secure = parsed.scheme() == "https";
        if !(secure || parsed.scheme() == "http" && allow_http_loopback && local) {
            return Err("https_origin_required");
        }
        let canonical = parsed.origin().ascii_serialization();
        let authority = canonical
            .split_once("://")
            .ok_or("invalid_origin")?
            .1
            .to_owned();
        Ok(Self {
            origin: canonical,
            authority,
            secure,
            trusted_proxy: None,
            google: None,
            web: None,
            browser: None,
        })
    }
    pub fn with_google_client_json_owned(self, value: String) -> Result<Self, &'static str> {
        let value = zeroize::Zeroizing::new(value);
        self.with_google_client_json(&value)
    }
    /// Compatibility for operator-owned files; web imports use the same parser.
    pub fn with_google_client_json(mut self, bytes: &str) -> Result<Self, &'static str> {
        let redirect = self.google_redirect_uri();
        self.google = Some(Arc::new(
            moosuri_connectors::gmail::OAuthClient::from_web_client_json(bytes, &redirect)
                .map_err(google_setup::client_file_code)?,
        ));
        Ok(self)
    }
    fn google_redirect_uri(&self) -> String {
        format!("{}/api/v1/mail/oauth/callback", self.origin)
    }
    /// Configure public HTTP reading and optional fixed-provider search before Store admission.
    /// This is trusted operator configuration, not an HTTP or model capability switch.
    pub fn with_public_web(mut self, key: Option<String>) -> Result<Self, &'static str> {
        let client = moosuri_connectors::web::WebClient::new(moosuri_connectors::web::WebConfig {
            brave_api_key: key,
        })
        .map_err(|_| "invalid_web_configuration")?;
        self.web = Some(Arc::new(client));
        Ok(self)
    }
    /// Trusted deployment file only. This does not provision Docker resources.
    pub fn with_browser_capsule_json(mut self, bytes: &str) -> Result<Self, &'static str> {
        use moosuri_connectors::browser::capsule::{CapsuleConfig, CapsuleReader};
        self.browser = Some(Arc::new(
            CapsuleReader::new(
                CapsuleConfig::from_json(bytes).map_err(|_| "invalid_browser_configuration")?,
            )
            .map_err(|_| "invalid_browser_configuration")?,
        ));
        Ok(self)
    }
    pub fn open_store(&self, path: &std::path::Path) -> Result<Store, StoreError> {
        let mut profile = self
            .web
            .as_ref()
            .map(|client| moosuri_core::WebToolProfile {
                read: true,
                search: client.has_search(),
                browser_pin: None,
            })
            .unwrap_or_default();
        profile.browser_pin = self
            .browser
            .as_ref()
            .map(|reader| reader.deployment_pin().to_owned());
        Store::open_with_web(path, profile)
    }
    pub fn start_runtime(&self, store: Store) -> moosuri_agent::Runtime {
        moosuri_agent::Runtime::start_with_google_setup(
            store,
            self.google.clone(),
            self.web.clone(),
            self.browser.clone(),
            self.google_redirect_uri(),
        )
    }
    pub fn with_trusted_proxy(
        mut self,
        peer: IpAddr,
        client_header: &str,
    ) -> Result<Self, &'static str> {
        if !peer.is_loopback() || !["x-real-ip", "x-forwarded-for"].contains(&client_header) {
            return Err("invalid_trusted_proxy");
        }
        self.trusted_proxy = Some((
            peer,
            header::HeaderName::from_bytes(client_header.as_bytes())
                .map_err(|_| "invalid_client_ip_header")?,
        ));
        Ok(self)
    }
}

type LoginBuckets = HashMap<(IpAddr, [u8; 32]), VecDeque<Instant>>;
#[derive(Clone)]
struct AppState {
    store: Store,
    config: ServerConfig,
    logins: Arc<Mutex<LoginBuckets>>,
    probes: Arc<tokio::sync::Semaphore>,
    runtime: moosuri_agent::Runtime,
    delivery: Arc<tokio::sync::RwLock<()>>,
    streams: Arc<Mutex<HashMap<String, usize>>>,
    mail: moosuri_agent::mail::MailService,
    browser: moosuri_agent::browser::BrowserService,
}

pub fn router(store: Store, config: ServerConfig) -> Router {
    let runtime = config.start_runtime(store.clone());
    router_with_runtime(store, config, runtime)
}

pub fn router_with_runtime(
    store: Store,
    config: ServerConfig,
    runtime: moosuri_agent::Runtime,
) -> Router {
    let mail = runtime.mail_service();
    let browser = runtime.browser_service();
    let state = AppState {
        store,
        mail,
        browser,
        config,
        logins: Arc::new(Mutex::new(HashMap::new())),
        probes: Arc::new(tokio::sync::Semaphore::new(2)),
        delivery: runtime.delivery_gate(),
        runtime,
        streams: Arc::new(Mutex::new(HashMap::new())),
    };
    Router::new()
        .route("/health/live", get(|| async { StatusCode::NO_CONTENT }))
        .route("/health/ready", get(ready))
        .route("/api/v1/system", get(system))
        .route("/api/v1/auth/login", post(login))
        .route("/api/v1/auth/session", get(session))
        .route("/api/v1/auth/password", post(change_password))
        .route("/api/v1/auth/logout", post(logout))
        .route("/api/v1/admin/users", get(users).post(create_user))
        .route("/api/v1/admin/users/{id}/manage", post(accounts::manage))
        .route(
            "/api/v1/admin/providers",
            get(providers::list).post(providers::create),
        )
        .route(
            "/api/v1/admin/providers/default",
            post(providers::set_default),
        )
        .route(
            "/api/v1/admin/providers/{id}",
            axum::routing::put(providers::update),
        )
        .route("/api/v1/admin/providers/{id}/probe", post(providers::probe))
        .route(
            "/api/v1/admin/providers/{id}/tool-probe",
            post(providers::tool_probe),
        )
        .route(
            "/api/v1/admin/providers/{id}/models",
            post(providers::catalog),
        )
        .route(
            "/api/v1/settings/model",
            get(providers::settings).put(providers::set_preference),
        )
        .route(
            "/api/v1/admin/providers/{id}/vision-probe",
            post(providers::vision_probe),
        )
        .route("/api/v1/media", get(media::library))
        .route(
            "/api/v1/media/{id}",
            post(media::upload).delete(media::remove),
        )
        .route("/api/v1/media/{id}/content", get(media::content))
        .route("/api/v1/media/{id}/thumbnail", get(media::thumbnail))
        .route("/api/v1/threads", get(threads).post(create_thread))
        .route("/api/v1/threads/{id}", delete(delete_thread))
        .route(
            "/api/v1/threads/{id}/messages",
            get(chat::messages)
                .post(chat::submit)
                // One UTF-8 byte may occupy six JSON bytes as a Unicode escape.
                // Store still enforces the decoded 8,192-byte prompt limit.
                .layer(DefaultBodyLimit::max(64 * 1024)),
        )
        .route("/api/v1/threads/{id}/events", get(chat::events))
        .route("/api/v1/runs/{id}/cancel", post(chat::cancel))
        .route("/api/v1/runs/{id}/retry", post(chat::retry))
        .nest("/api/v1/community-skills", community_skills::routes())
        .nest("/api/v1/artifacts", artifacts::routes())
        .nest("/api/v1/knowledge", knowledge::routes())
        .nest("/api/v1/mail", mail::routes())
        .nest("/api/v1/admin/google", google_setup::routes())
        .nest("/api/v1/source-connections", source_connections::routes())
        .nest("/api/v1/source-ingest", source_connections::ingest_routes())
        .merge(browser::routes())
        .merge(schedules::routes())
        .route("/api/v1/usage", get(chat::usage))
        .route("/api/v1/admin/usage", get(chat::account_usage))
        .fallback(|| async { failure(StatusCode::NOT_FOUND, "not_found") })
        .layer(DefaultBodyLimit::max(16 * 1024))
        .layer(middleware::from_fn_with_state(state.clone(), boundary))
        .with_state(state)
}

async fn boundary(State(state): State<AppState>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok());
    if host != Some(state.config.authority.as_str()) {
        return failure(StatusCode::MISDIRECTED_REQUEST, "invalid_host");
    }
    let collector = request.uri().path().starts_with("/api/v1/source-ingest/");
    if collector
        && (request.headers().contains_key(header::COOKIE)
            || request.headers().contains_key(header::ORIGIN))
    {
        return failure(StatusCode::FORBIDDEN, "collector_bearer_required");
    }
    if !collector
        && !matches!(
            *request.method(),
            Method::GET | Method::HEAD | Method::OPTIONS
        )
    {
        let origin = request
            .headers()
            .get(header::ORIGIN)
            .and_then(|h| h.to_str().ok());
        if origin != Some(state.config.origin.as_str()) {
            return failure(StatusCode::FORBIDDEN, "invalid_origin");
        }
    }
    let mut response = next.run(request).await;
    let headers = response.headers_mut();
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    // Private download handlers can supply a stricter sandbox policy.
    headers
        .entry(header::CONTENT_SECURITY_POLICY)
        .or_insert(HeaderValue::from_static(
            "default-src 'none'; frame-ancestors 'none'",
        ));
    response
}

fn failure(status: StatusCode, code: &str) -> Response {
    (
        status,
        Json(ApiError {
            code: code.to_owned(),
        }),
    )
        .into_response()
}
struct HttpError(StoreError);
impl From<StoreError> for HttpError {
    fn from(error: StoreError) -> Self {
        Self(error)
    }
}
impl IntoResponse for HttpError {
    fn into_response(self) -> Response {
        let (status, code) = match self.0 {
            StoreError::InvalidInput => (StatusCode::BAD_REQUEST, "invalid_input"),
            StoreError::Unauthorized => (StatusCode::UNAUTHORIZED, "unauthorized"),
            StoreError::Forbidden => (StatusCode::FORBIDDEN, "forbidden"),
            StoreError::Conflict | StoreError::AlreadyInitialized => {
                (StatusCode::CONFLICT, "conflict")
            }
            StoreError::GoogleConnectionsInUse => {
                (StatusCode::CONFLICT, "google_connections_in_use")
            }
            StoreError::GoogleAuthorizationInProgress => {
                (StatusCode::CONFLICT, "google_authorization_in_progress")
            }
            StoreError::BudgetExhausted => (StatusCode::CONFLICT, "budget_exhausted"),
            StoreError::NotFound => (StatusCode::NOT_FOUND, "not_found"),
            StoreError::Busy => (StatusCode::SERVICE_UNAVAILABLE, "busy"),
            StoreError::Unavailable | StoreError::Corrupt => {
                (StatusCode::SERVICE_UNAVAILABLE, "unavailable")
            }
        };
        failure(status, code)
    }
}

type HttpResult = Result<Response, HttpError>;
fn cookie_token(headers: &HeaderMap) -> Result<String, StoreError> {
    let mut found = None;
    for value in headers.get_all(header::COOKIE) {
        let value = value.to_str().map_err(|_| StoreError::Unauthorized)?;
        for item in value.split(';') {
            let Some((name, token)) = item.trim().split_once('=') else {
                continue;
            };
            if name != COOKIE {
                continue;
            }
            if found.is_some()
                || token.len() < 32
                || token.len() > 128
                || !token
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            {
                return Err(StoreError::Unauthorized);
            }
            found = Some(token.to_owned());
        }
    }
    found.ok_or(StoreError::Unauthorized)
}
async fn current(state: &AppState, headers: &HeaderMap) -> Result<UserContext, HttpError> {
    let user = state.store.authenticate(cookie_token(headers)?).await?;
    expected_account(headers, &user)?;
    Ok(user)
}
fn expected_account(headers: &HeaderMap, user: &UserContext) -> Result<(), StoreError> {
    if headers
        .get("x-moosuri-account")
        .and_then(|h| h.to_str().ok())
        != Some(user.account().id.as_str())
    {
        return Err(StoreError::Unauthorized);
    }
    Ok(())
}
fn session_view(user: &UserContext) -> SessionView {
    SessionView {
        user: user.account().clone(),
        password_change_required: user.password_change_required(),
    }
}
fn session_response(user: &UserContext, token: &str, secure: bool) -> Response {
    let mut result = Json(session_view(user)).into_response();
    let max_age = if user.password_change_required() {
        24 * 60 * 60
    } else {
        COOKIE_MAX_AGE
    };
    set_cookie(&mut result, token, secure, max_age);
    result
}
fn set_cookie(response: &mut Response, token: &str, secure: bool, max_age: u64) {
    let suffix = if secure { "; Secure" } else { "" };
    // Tokens are generated by Store; HeaderValue rejects control characters.
    let value =
        format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={max_age}{suffix}");
    if let Ok(value) = HeaderValue::from_str(&value) {
        response.headers_mut().insert(header::SET_COOKIE, value);
    }
}

async fn ready(State(state): State<AppState>) -> HttpResult {
    if state.store.initialized().await? {
        Ok(StatusCode::NO_CONTENT.into_response())
    } else {
        Ok(failure(StatusCode::SERVICE_UNAVAILABLE, "setup_required"))
    }
}
async fn system(State(state): State<AppState>) -> HttpResult {
    Ok(Json(SystemStatus {
        version: env!("CARGO_PKG_VERSION").to_owned(),
        initialized: state.store.initialized().await?,
        stage: "durable-text-chat".to_owned(),
    })
    .into_response())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoginRequest {
    email: String,
    password: String,
}
async fn login(
    State(state): State<AppState>,
    peer: Result<ConnectInfo<SocketAddr>, axum::extract::rejection::ExtensionRejection>,
    headers: HeaderMap,
    Json(input): Json<LoginRequest>,
) -> HttpResult {
    let peer = peer.ok().map(|p| p.0.ip());
    let peer_ip = peer.unwrap_or(IpAddr::from([127, 0, 0, 1]));
    let ip = if let Some((trusted_peer, client_header)) = &state.config.trusted_proxy {
        if peer != Some(*trusted_peer) {
            return Ok(failure(StatusCode::BAD_REQUEST, "untrusted_proxy"));
        }
        let values: Vec<_> = headers.get_all(client_header).iter().collect();
        let parsed = if values.len() == 1 {
            values[0]
                .to_str()
                .ok()
                .and_then(|v| v.trim().parse::<IpAddr>().ok())
        } else {
            None
        };
        let Some(ip) = parsed else {
            return Ok(failure(StatusCode::BAD_REQUEST, "invalid_client_ip"));
        };
        ip
    } else {
        peer_ip
    };
    // A shared NAT/proxy peer cannot lock every account with one small attempt bucket.
    let email_hash: [u8; 32] =
        Sha256::digest(input.email.trim().to_ascii_lowercase().as_bytes()).into();
    let key = (ip, email_hash);
    let now = Instant::now();
    let accepted = {
        let mut limits = state.logins.lock().map_err(|_| StoreError::Unavailable)?;
        limits.retain(|_, history| {
            while history
                .front()
                .is_some_and(|t| now.duration_since(*t) >= Duration::from_secs(300))
            {
                history.pop_front();
            }
            !history.is_empty()
        });
        if limits.len() >= 4096 && !limits.contains_key(&key) {
            // Bounded memory without denying every unseen legitimate account.
            if let Some(oldest) = limits
                .iter()
                .min_by_key(|(_, history)| history.back())
                .map(|(key, _)| *key)
            {
                limits.remove(&oldest);
            }
        }
        {
            let history = limits.entry(key).or_default();
            if history.len() >= 20 {
                false
            } else {
                history.push_back(now);
                true
            }
        }
    };
    if !accepted {
        let mut response = failure(StatusCode::TOO_MANY_REQUESTS, "rate_limited");
        response
            .headers_mut()
            .insert(header::RETRY_AFTER, HeaderValue::from_static("300"));
        return Ok(response);
    }
    let result = state.store.login(input.email, input.password).await?;
    Ok(session_response(
        &result.user,
        &result.token,
        state.config.secure,
    ))
}
async fn session(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let token = cookie_token(&headers)?;
    let user = state.store.authenticate(token.clone()).await?;
    // Read responses never set cookies: a delayed response from A must not replace B's login.
    Ok(Json(session_view(&user)).into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PasswordRequest {
    password: String,
}
async fn change_password(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<PasswordRequest>,
) -> HttpResult {
    let _delivery = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    let result = state.store.change_password(&user, input.password).await?;
    Ok(session_response(
        &result.user,
        &result.token,
        state.config.secure,
    ))
}
async fn logout(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let _delivery = state.delivery.write().await;
    // Revoke the supplied session, but never send Set-Cookie: a late logout
    // response from another tab must not erase a more recent login cookie.
    match cookie_token(&headers) {
        Ok(token) => match state.store.authenticate(token).await {
            Ok(user) => {
                expected_account(&headers, &user)?;
                state.store.logout(&user).await?;
            }
            Err(StoreError::Unauthorized) => (),
            Err(error) => return Err(error.into()),
        },
        Err(StoreError::Unauthorized) => (),
        Err(error) => return Err(error.into()),
    }
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn users(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    let management = state.store.managed_accounts(&user).await?;
    Ok(Json(accounts::ManagedUsersResponse {
        users: management.iter().map(|m| m.user.clone()).collect(),
        management,
    })
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateUserRequest {
    name: String,
    email: String,
}
#[derive(Serialize)]
struct InvitationResponse {
    management: moosuri_core::ManagedAccount,
    user: moosuri_core::AccountSummary,
    temporary_password: String,
}
async fn create_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateUserRequest>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let result = state
        .store
        .create_user(&user, input.name, input.email)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(InvitationResponse {
            management: result.management,
            user: result.user,
            temporary_password: result.temporary_password,
        }),
    )
        .into_response())
}
async fn threads(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(ThreadsResponse {
        threads: state.store.list_threads(&user).await?,
    })
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CreateThreadRequest {
    title: String,
}
async fn create_thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<CreateThreadRequest>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok((
        StatusCode::CREATED,
        Json(state.store.create_thread(&user, input.title).await?),
    )
        .into_response())
}
async fn delete_thread(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _delivery = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    state.store.delete_thread(&user, id).await?;
    state.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
