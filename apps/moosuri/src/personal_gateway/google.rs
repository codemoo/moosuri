//! The public Google callback has no session cookie. Its encrypted state selects
//! one original member and a short-lived flow. Only that member's authenticated
//! completion endpoint may exchange the captured code.
use super::*;
use ring::aead::{self, Aad, LessSafeKey, Nonce, UnboundKey};
use serde_json::{Value, json};

const CALLBACK: &str = "/api/v1/mail/oauth/callback";
const BASE: &str = "/api/v1/personal/google";
const TTL: u64 = 600;
const AAD: &[u8] = b"moosuri/personal/google/gmail-readonly/callback/v1";
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(CALLBACK, get(callback))
        .route(BASE, get(member))
        .route("/api/v1/personal/google/start", post(member))
        .route("/api/v1/personal/google/complete", post(member))
        .route("/api/v1/personal/google/inbox", post(member))
        .route("/api/v1/personal/google/disconnect", post(member))
        .layer(DefaultBodyLimit::max(4096))
}
fn cipher(g: &Gateway) -> Result<LessSafeKey, StatusCode> {
    let master = hmac::Key::new(hmac::HMAC_SHA256, &g.key);
    // Distinct from route-cookie signatures. An origin change invalidates a flow.
    let mut ctx = hmac::Context::with_key(&master);
    ctx.update(AAD);
    ctx.update(g.origin.as_bytes());
    let key = UnboundKey::new(&aead::AES_256_GCM, ctx.sign().as_ref())
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    Ok(LessSafeKey::new(key))
}
fn seal(g: &Gateway, a: &SessionAllocation, state: &str) -> Result<String, StatusCode> {
    let mut nonce = [0u8; 12];
    SystemRandom::new()
        .fill(&mut nonce)
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut data = Vec::new();
    for field in [&a.account_id, &a.tenant_id, &a.volume_id] {
        data.extend_from_slice(
            Uuid::parse_str(field)
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
                .as_bytes(),
        );
    }
    data.extend_from_slice(&a.generation.to_be_bytes());
    data.extend_from_slice(&(now()? + TTL).to_be_bytes());
    if state.len() != 64 || !state.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    data.extend_from_slice(state.as_bytes());
    data.extend_from_slice(&a.epoch.to_be_bytes());
    cipher(g)?
        .seal_in_place_append_tag(
            Nonce::assume_unique_for_key(nonce),
            Aad::from(AAD),
            &mut data,
        )
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut frame = nonce.to_vec();
    frame.extend_from_slice(&data);
    Ok(URL_SAFE_NO_PAD.encode(frame))
}
fn open(g: &Gateway, state: &str) -> Result<(Route, String), StatusCode> {
    if state.len() > 256 {
        return Err(StatusCode::BAD_REQUEST);
    }
    let mut frame = URL_SAFE_NO_PAD
        .decode(state)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    // 3 UUIDs, generation, expiry, 64 lowercase hex state bytes, AEAD overhead.
    if ![12 + 128 + 16, 12 + 136 + 16].contains(&frame.len()) {
        return Err(StatusCode::BAD_REQUEST);
    }
    let (nonce, data) = frame.split_at_mut(12);
    let plain = cipher(g)?
        .open_in_place(
            Nonce::try_assume_unique_for_key(nonce).map_err(|_| StatusCode::BAD_REQUEST)?,
            Aad::from(AAD),
            data,
        )
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    let mut data = plain.chunks_exact(16);
    let mut id = || {
        Uuid::from_slice(data.next().ok_or(StatusCode::BAD_REQUEST)?)
            .map_err(|_| StatusCode::BAD_REQUEST)
    };
    let (account, tenant, volume) = (id()?, id()?, id()?);
    let expires = u64::from_be_bytes(plain[56..64].try_into().unwrap());
    let ts = now()?;
    if expires <= ts || expires > ts + TTL {
        return Err(StatusCode::BAD_REQUEST);
    }
    let inner = std::str::from_utf8(&plain[64..128])
        .map_err(|_| StatusCode::BAD_REQUEST)?
        .to_owned();
    Ok((
        Route {
            epoch: if plain.len() == 136 {
                i64::from_be_bytes(plain[128..136].try_into().unwrap())
            } else {
                0
            },
            account,
            tenant,
            volume,
            generation: i64::from_be_bytes(plain[48..56].try_into().unwrap()),
            expires,
            session_hash: String::new(),
        },
        inner,
    ))
}
fn consent_url(g: &Gateway, a: &SessionAllocation, bytes: &[u8]) -> Result<Response, StatusCode> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Start {
        authorization_url: String,
        flow_id: String,
    }
    let result: Start =
        serde_json::from_slice(bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let mut url =
        url::Url::parse(&result.authorization_url).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    // The member's typed projection verifies Google, PKCE and the exact scope.
    // Only the public gateway knows the installation origin and route key.
    let mut fields: std::collections::BTreeMap<String, String> =
        url.query_pairs().into_owned().collect();
    if fields.get("redirect_uri") != Some(&format!("{}{CALLBACK}", g.origin)) {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    let inner = fields.get("state").ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    let envelope = seal(g, a, inner)?;
    fields.insert("state".into(), envelope);
    url.query_pairs_mut().clear().extend_pairs(fields);
    Ok(response(
        StatusCode::OK,
        json!({"authorization_url":url.as_str(),"flow_id":result.flow_id})
            .to_string()
            .into_bytes(),
    ))
}
async fn member(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        g.headers(&headers, &method)?;
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let (a, session) = authenticated_allocation(&g, &headers, true).await?;
        let payload = if method == Method::POST {
            if one(&headers, "content-type").and_then(|s| s.split(';').next())
                != Some("application/json")
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            Some(body.as_ref())
        } else {
            if !body.is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            None
        };
        let reply = g
            .proxy(&a, method.clone(), uri.path(), payload, Some(&session))
            .await;
        recheck(&g, &a).await?;
        let (status, bytes, _) = reply?;
        if !status.is_success() {
            if status == StatusCode::NOT_FOUND {
                return Ok(response(status, br#"{"code":"not_found"}"#.to_vec()));
            }
            return api_error(status, &bytes);
        }
        let bytes = crate::personal_google::public_response(uri.path(), &method, status, &bytes)?;
        if uri.path().ends_with("/start") {
            return consent_url(&g, &a, &bytes);
        }
        Ok(response(status, bytes))
    }
    .await;
    result.unwrap_or_else(failure)
}
#[derive(Deserialize)]
struct Callback {
    state: String,
    code: Option<String>,
    error: Option<String>,
}
async fn callback(
    State(g): State<Gateway>,
    headers: HeaderMap,
    query: Result<axum::extract::Query<Callback>, axum::extract::rejection::QueryRejection>,
) -> Response {
    let result = async {
        let axum::extract::Query(input) = query.map_err(|_| StatusCode::BAD_REQUEST)?;
        if one(&headers, "host") != Some(g.host.as_str())
            || input.code.is_some() == input.error.is_some()
            || input
                .code
                .as_ref()
                .is_some_and(|c| c.is_empty() || c.len() > 4096 || c.chars().any(char::is_control))
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let (route, inner) = open(&g, &input.state)?;
        let a = active(&g, &route).await?;
        let flow = moosuri_store::Store::mail_oauth_flow_id(&inner);
        let payload = Zeroizing::new(json!({"state":inner,"code":input.code}).to_string());
        let result = g
            .proxy(
                &a,
                Method::POST,
                "/internal/google/capture",
                Some(payload.as_bytes()),
                None,
            )
            .await;
        recheck(&g, &a).await?;
        let (status, bytes, _) = result?;
        if !status.is_success() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let v: Value =
            serde_json::from_slice(&bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let path = if input.error.is_some() {
            format!("/#google_denied={flow}")
        } else {
            let handle = v["handle"]
                .as_str()
                .filter(|v| v.len() == 64 && v.bytes().all(|c| c.is_ascii_hexdigit()))
                .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
            format!("/#google_handle={handle}&flow={flow}")
        };
        Ok(axum::response::Redirect::to(&path).into_response())
    }
    .await;
    let mut res = result.unwrap_or_else(failure);
    res.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    res.headers_mut().insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("no-referrer"),
    );
    res
}
