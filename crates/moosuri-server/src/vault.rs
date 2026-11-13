//! Dedicated internal router: bind only a loopback listener; never merge into public API.
use super::*;
use moosuri_vault_protocol::AuthorityRequest;
use subtle::ConstantTimeEq;

pub(super) async fn handoff(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    let origin = state.config.vault_origin.ok_or(StoreError::Unavailable)?;
    Ok(Json(state.store.vault_handoff(&user, origin).await?).into_response())
}
pub(super) async fn status(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    if user.password_change_required() {
        return Err(StoreError::Forbidden.into());
    }
    Ok(Json(serde_json::json!({"available":state.config.vault_origin.is_some()})).into_response())
}
#[derive(Clone)]
struct Internal {
    store: Store,
    key_digest: [u8; 32],
}
/// The deployment key is random256-bit hex read from an owner-only file. This
/// identifies one ciphertext service, not a user or a decryption capability.
pub fn adapter_router(store: Store, key: &str) -> Result<Router, &'static str> {
    if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err("invalid_vault_adapter_key");
    }
    Ok(Router::new()
        .route("/internal/v1/vault", post(authority))
        .layer(DefaultBodyLimit::max(4096))
        .with_state(Internal {
            store,
            key_digest: Sha256::digest(key.as_bytes()).into(),
        }))
}
async fn authority(
    State(state): State<Internal>,
    headers: HeaderMap,
    input: std::result::Result<Json<AuthorityRequest>, axum::extract::rejection::JsonRejection>,
) -> HttpResult {
    // Origin/cookies are never accepted on this service-only transport.
    if headers.contains_key(header::ORIGIN) || headers.contains_key(header::COOKIE) {
        return Err(StoreError::Forbidden.into());
    }
    let key = headers
        .get("x-moosuri-vault-adapter")
        .and_then(|v| v.to_str().ok())
        .ok_or(StoreError::Unauthorized)?;
    if key.len() != 64 {
        return Err(StoreError::Unauthorized.into());
    }
    let got: [u8; 32] = Sha256::digest(key.as_bytes()).into();
    // Fixed-length digest comparison, independent of supplied prefix.
    if !bool::from(got.ct_eq(&state.key_digest)) {
        return Err(StoreError::Unauthorized.into());
    }
    let Json(input) = input.map_err(|_| StoreError::InvalidInput)?;
    let reply = state.store.vault_authority(input).await?;
    let mut response = Json(reply).into_response();
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    Ok(response)
}
