//! Installation roles are authoritative in the host registry. A cookie only
//! locates the private service that can validate the current member login.
use super::*;
use moosuri_core::SessionView;
use moosuri_store::runtime_registry::engine;
use moosuri_store::{StoreError, runtime_registry::admin as control};
use serde_json::json;

fn error(e: StoreError) -> StatusCode {
    match e {
        StoreError::Unauthorized => StatusCode::UNAUTHORIZED,
        StoreError::Forbidden => StatusCode::FORBIDDEN,
        StoreError::InvalidInput => StatusCode::BAD_REQUEST,
        StoreError::Conflict | StoreError::AlreadyInitialized => StatusCode::CONFLICT,
        StoreError::NotFound => StatusCode::NOT_FOUND,
        _ => StatusCode::SERVICE_UNAVAILABLE,
    }
}
fn private_view(bytes: &[u8], a: &Allocation) -> Result<SessionView, StatusCode> {
    let view: SessionView =
        serde_json::from_slice(bytes).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    if view.user.id != a.account_id {
        return Err(StatusCode::SERVICE_UNAVAILABLE);
    }
    Ok(view)
}
pub(super) async fn public_session(
    g: &Gateway,
    a: &Allocation,
    body: &[u8],
) -> Result<Vec<u8>, StatusCode> {
    let view = private_view(body, a)?;
    let root = g.registry.clone();
    let allocation = a.clone();
    tokio::task::spawn_blocking(move || {
        let member = control::observe_session(&root, &allocation, &view).map_err(error)?;
        let mut json = serde_json::to_value(view).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        json["installation_admin"] =
            json!(member.role == "admin" && !member.suspended && member.eligible);
        serde_json::to_vec(&json).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
    })
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ChangeRequest {
    request_id: Uuid,
    account_id: Uuid,
    generation: i64,
    revision: i64,
    action: Action,
}
#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Promote,
    Demote,
    Suspend,
    Resume,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Invite {
    request_id: Uuid,
    name: String,
    email: String,
}
enum Operation {
    List,
    Engine,
    Configure(engine::Change),
    Change(ChangeRequest),
    Invite(Invite),
}
fn member(m: control::Member) -> serde_json::Value {
    json!({"account_id":m.account_id,"email":m.email,"generation":m.generation,"revision":m.role_revision,
        "admin":m.role=="admin","eligible":m.eligible,"suspended":m.suspended,"ready":m.route_state=="ready","invitation_request_id":m.invitation_request_id,"invitation_name":m.invitation_name})
}
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(
            "/api/v1/personal/admin/engine",
            get(endpoint).post(endpoint),
        )
        .route("/api/v1/personal/admin/engine/member", post(endpoint))
        .route("/api/v1/personal/admin/members", get(endpoint))
        .route("/api/v1/personal/admin/members/change", post(endpoint))
        .route("/api/v1/personal/admin/invitations", post(endpoint))
        .layer(DefaultBodyLimit::max(4096))
}
async fn endpoint(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        g.headers(&headers, &method)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        let _permit = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let input = if method == Method::POST {
            if one(&headers, "content-type").and_then(|s| s.split(';').next())
                != Some("application/json")
            {
                return Err(StatusCode::BAD_REQUEST);
            }
            fn decode<T: serde::de::DeserializeOwned>(bytes: &[u8]) -> Result<T, StatusCode> {
                serde_json::from_slice(bytes).map_err(|_| StatusCode::BAD_REQUEST)
            }
            match uri.path() {
                "/api/v1/personal/admin/invitations" => Operation::Invite(decode(&body)?),
                "/api/v1/personal/admin/members/change" => Operation::Change(decode(&body)?),
                "/api/v1/personal/admin/engine" => {
                    Operation::Configure(engine::Change::Installation(decode(&body)?))
                }
                "/api/v1/personal/admin/engine/member" => {
                    Operation::Configure(engine::Change::Member(decode(&body)?))
                }
                _ => return Err(StatusCode::NOT_FOUND),
            }
        } else {
            if !body.is_empty() {
                return Err(StatusCode::BAD_REQUEST);
            }
            match uri.path() {
                "/api/v1/personal/admin/engine" => Operation::Engine,
                "/api/v1/personal/admin/members" => Operation::List,
                _ => return Err(StatusCode::NOT_FOUND),
            }
        };
        let (a, session) = authenticated_allocation(&g, &headers, true).await?;
        let (status, bytes, _) = g
            .proxy(
                &a,
                Method::GET,
                "/api/v1/auth/session",
                None,
                Some(&session),
            )
            .await?;
        if status != StatusCode::OK {
            return Err(status);
        }
        let view = private_view(&bytes, &a)?;
        let root = g.registry.clone();
        let out = tokio::task::spawn_blocking(move || {
            let pin = control::checked_session(&root, &a, &view)
                .and_then(|proof| proof.at_epoch(a.epoch))
                .map_err(error)?;
            match input {
                Operation::Engine => engine::read(&root, &pin).map(|s| json!(s)).map_err(error),
                Operation::Configure(c) => engine::change(&root, &pin, c)
                    .map(|s| json!(s))
                    .map_err(error),
                Operation::Change(r) => {
                    let change = match r.action {
                        Action::Promote => control::Change::Promote,
                        Action::Demote => control::Change::Demote,
                        Action::Suspend => control::Change::Suspend,
                        Action::Resume => control::Change::Resume,
                    };
                    control::change_member(
                        &root,
                        &pin,
                        r.request_id,
                        r.account_id,
                        r.generation,
                        r.revision,
                        change,
                    )
                    .map(member)
                    .map_err(error)
                }
                Operation::Invite(r) => {
                    control::reserve_invitation(&root, &pin, r.request_id, r.name, r.email)
                        .map(|i| json!({"request_id":i.request_id,"email":i.email}))
                        .map_err(error)
                }
                Operation::List => control::list_members(&root, &pin)
                    .map(|rows| json!({"members":rows.into_iter().map(member).collect::<Vec<_>>()}))
                    .map_err(error),
            }
        })
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)??;
        Ok(response(StatusCode::OK, out.to_string().into_bytes()))
    }
    .await;
    result.unwrap_or_else(failure)
}
