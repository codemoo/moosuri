//! Opt-in bridge between one private member Store and the v2 vault sidecar.
//! The handoff route requires the member's own session. The authority route is
//! service-only: it accepts the gateway UID on the private Unix socket, never a
//! cookie, and cannot be selected by a client-supplied account or origin.
use super::MemberPeer;
use crate::{HttpError, HttpResult, personal_request_user};
use axum::{
    Json, Router,
    extract::{ConnectInfo, State},
    http::{HeaderMap, header},
    response::IntoResponse,
    routing::{get, post},
};
use moosuri_store::{Store, StoreError, personal_volume::VolumeOwner};
use moosuri_vault_protocol::{AuthorityReply, AuthorityRequest};

pub const STATUS_PATH: &str = "/api/v1/personal/vault";
pub const HANDOFF_PATH: &str = "/api/v1/personal/vault/handoff";
pub const AUTHORITY_PATH: &str = "/internal/member-vault/authority";

#[derive(Clone)]
struct MemberVault {
    store: Store,
    owner: VolumeOwner,
    origin: String,
    gateway_uid: u32,
}

/// Exact canonical HTTPS origin; it is deployment configuration, never request input.
pub fn valid_origin(origin: &str) -> bool {
    url::Url::parse(origin).is_ok_and(|u| {
        u.scheme() == "https"
            && u.host_str().is_some()
            && u.username().is_empty()
            && u.password().is_none()
            && u.path() == "/"
            && u.query().is_none()
            && u.fragment().is_none()
            && u.origin().ascii_serialization() == origin
    })
}

pub fn routes(
    store: Store,
    owner: VolumeOwner,
    origin: &str,
    gateway_uid: u32,
) -> Result<Router, &'static str> {
    if !valid_origin(origin) {
        return Err("invalid_vault_origin");
    }
    Ok(Router::new()
        .route(STATUS_PATH, get(status))
        .route(HANDOFF_PATH, post(handoff))
        .route(AUTHORITY_PATH, post(authority))
        .with_state(MemberVault {
            store,
            owner,
            origin: origin.to_owned(),
            gateway_uid,
        }))
}

async fn status(State(s): State<MemberVault>, h: HeaderMap) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    if user.account().id != s.owner.account.to_string() {
        return Err(HttpError(StoreError::Forbidden));
    }
    Ok(
        Json(serde_json::json!({"vault_origin": s.origin, "account": s.owner.account}))
            .into_response(),
    )
}

async fn handoff(
    State(s): State<MemberVault>,
    h: HeaderMap,
    body: axum::body::Bytes,
) -> HttpResult {
    if !body.is_empty() && body.as_ref() != b"{}" {
        return Err(HttpError(StoreError::InvalidInput));
    }
    let user = personal_request_user(&s.store, &h).await?;
    if user.account().id != s.owner.account.to_string() {
        return Err(HttpError(StoreError::Forbidden));
    }
    let handoff = s.store.vault_handoff(&user, s.origin.clone()).await?;
    Ok(Json(handoff).into_response())
}

fn hex64(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}

/// Bounded scope: only the origin configured for this member and fixed-size tokens.
fn scoped(request: &AuthorityRequest, origin: &str) -> bool {
    match request {
        AuthorityRequest::LifecycleNext => true,
        AuthorityRequest::LifecycleAck { event } => !event.is_nil(),
        AuthorityRequest::Exchange {
            handoff,
            vault_origin,
        } => hex64(handoff) && vault_origin == origin,
        AuthorityRequest::Validate { grant } | AuthorityRequest::Close { grant } => hex64(grant),
        AuthorityRequest::Commit {
            grant,
            operation,
            digest,
        } => hex64(grant) && hex64(digest) && !operation.is_nil(),
    }
}

async fn authority(
    State(s): State<MemberVault>,
    ConnectInfo(peer): ConnectInfo<MemberPeer>,
    h: HeaderMap,
    input: Result<Json<AuthorityRequest>, axum::extract::rejection::JsonRejection>,
) -> HttpResult {
    if peer.0 != Some(s.gateway_uid)
        || h.contains_key(header::COOKIE)
        || h.get("x-moosuri-account").and_then(|v| v.to_str().ok())
            != Some(s.owner.account.to_string().as_str())
    {
        return Err(HttpError(StoreError::Forbidden));
    }
    let Json(request) = input.map_err(|_| HttpError(StoreError::InvalidInput))?;
    if !scoped(&request, &s.origin) {
        return Err(HttpError(StoreError::InvalidInput));
    }
    let reply: AuthorityReply = s.store.vault_authority(request).await?;
    // A member volume can only ever describe its own account and tenant.
    if reply.owner.account != s.owner.account || reply.owner.tenant != s.owner.tenant {
        return Err(HttpError(StoreError::Corrupt));
    }
    let mut response = Json(reply).into_response();
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        axum::http::HeaderValue::from_static("no-store"),
    );
    Ok(response)
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn scope_rejects_foreign_origin_and_malformed_tokens() {
        let origin = "https://vault.example.test";
        let ok = AuthorityRequest::Exchange {
            handoff: "a".repeat(64),
            vault_origin: origin.into(),
        };
        assert!(scoped(&ok, origin));
        assert!(!scoped(&ok, "https://other.example.test"));
        assert!(!scoped(
            &AuthorityRequest::Validate {
                grant: "g".repeat(64)
            },
            origin
        ));
        assert!(!scoped(
            &AuthorityRequest::Commit {
                grant: "a".repeat(64),
                operation: Uuid::nil(),
                digest: "b".repeat(64)
            },
            origin
        ));
        assert!(valid_origin(origin));
        assert!(!valid_origin("https://vault.example.test/x"));
        assert!(!valid_origin("http://vault.example.test"));
    }

    #[tokio::test]
    async fn authority_route_requires_the_gateway_peer_and_no_cookie() {
        let directory = tempfile::tempdir().unwrap();
        let owner = VolumeOwner {
            account: Uuid::new_v4(),
            tenant: Uuid::new_v4(),
            volume: Uuid::new_v4(),
            generation: 1,
        };
        let state = MemberVault {
            store: Store::open(directory.path()).unwrap(),
            owner,
            origin: "https://vault.example.test".into(),
            gateway_uid: 2000,
        };
        let request = || {
            Ok(Json(AuthorityRequest::Validate {
                grant: "a".repeat(64),
            }))
        };
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-moosuri-account",
            owner.account.to_string().parse().unwrap(),
        );
        for peer in [None, Some(2001)] {
            let denied = authority(
                State(state.clone()),
                ConnectInfo(MemberPeer(peer)),
                headers.clone(),
                request(),
            )
            .await;
            assert!(matches!(denied, Err(HttpError(StoreError::Forbidden))));
        }
        let mut cookie = headers.clone();
        cookie.insert(header::COOKIE, "moosuri_session=x".parse().unwrap());
        assert!(matches!(
            authority(
                State(state),
                ConnectInfo(MemberPeer(Some(2000))),
                cookie,
                request()
            )
            .await,
            Err(HttpError(StoreError::Forbidden))
        ));
    }
}
