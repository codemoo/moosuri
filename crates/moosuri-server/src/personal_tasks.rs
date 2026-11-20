//! Historical reads remain available if the browser worker is closed or offline.
use super::*;
use axum::extract::Query;
#[cfg(unix)]
use axum::{extract::connect_info::Connected, serve::IncomingStream};
use moosuri_core::personal_browser::{WorkAction, WorkRequest};
use moosuri_store::personal_volume::VolumeOwner;
#[cfg(unix)]
use tokio::net::UnixListener;

#[derive(Clone)]
struct MemberTasks {
    store: Store,
    owner: VolumeOwner,
}

pub fn routes(store: Store, owner: VolumeOwner) -> Router {
    Router::new()
        .route("/api/v1/personal/tasks", get(page))
        .route("/api/v1/personal/tasks/{id}", get(detail))
        .route(
            "/api/v1/personal/tasks/workflows",
            axum::routing::post(create),
        )
        .route(
            "/api/v1/personal/tasks/workflows/{id}",
            axum::routing::post(change),
        )
        .with_state(MemberTasks { store, owner })
}

#[derive(Clone)]
pub struct MemberPeer(Option<u32>);

#[cfg(unix)]
impl Connected<IncomingStream<'_, UnixListener>> for MemberPeer {
    fn connect_info(stream: IncomingStream<'_, UnixListener>) -> Self {
        Self(stream.io().peer_cred().ok().map(|cred| cred.uid()))
    }
}

pub fn timer_routes(store: Store, owner: VolumeOwner, gateway_uid: u32) -> Router {
    Router::new()
        .route("/internal/member-work/tick", axum::routing::post(tick))
        .with_state((MemberTasks { store, owner }, gateway_uid))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PageQuery {
    before: Option<String>,
}

async fn page(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Query(q): Query<PageQuery>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(s.store.personal_task_page(s.owner, user, q.before).await?).into_response())
}
async fn detail(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Path(id): Path<uuid::Uuid>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(s.store.personal_task_detail(s.owner, user, id).await?).into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Json(q): Json<WorkRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(
        s.store
            .create_member_workflow(s.owner, user, super::personal_text::gateway_epoch(&h)?, q)
            .await?,
    )
    .into_response())
}
async fn change(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Json(action): Json<WorkAction>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(
        s.store
            .change_member_workflow(
                s.owner,
                user,
                super::personal_text::gateway_epoch(&h)?,
                id,
                action,
            )
            .await?,
    )
    .into_response())
}

async fn tick(
    State((s, gateway_uid)): State<(MemberTasks, u32)>,
    ConnectInfo(peer): ConnectInfo<MemberPeer>,
    h: HeaderMap,
    Json(pin): Json<moosuri_core::personal_browser::WorkTick>,
) -> HttpResult {
    if peer.0 != Some(gateway_uid)
        || h.get("x-moosuri-account").and_then(|v| v.to_str().ok())
            != Some(s.owner.account.to_string().as_str())
        || pin.tenant != s.owner.tenant.to_string()
        || pin.volume != s.owner.volume.to_string()
        || pin.generation != s.owner.generation
        || pin.epoch < 0
    {
        return Err(HttpError(StoreError::Forbidden));
    }
    s.store.tick_member_workflows(s.owner, pin.epoch).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use moosuri_core::personal_browser::WorkTick;
    use uuid::Uuid;

    #[tokio::test]
    async fn member_timer_requires_gateway_peer_before_using_allocation() {
        let directory = tempfile::tempdir().unwrap();
        let state = MemberTasks {
            store: Store::open(directory.path()).unwrap(),
            owner: VolumeOwner {
                account: Uuid::new_v4(),
                tenant: Uuid::new_v4(),
                volume: Uuid::new_v4(),
                generation: 1,
            },
        };
        let owner = state.owner;
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-moosuri-account",
            owner.account.to_string().parse().unwrap(),
        );
        for uid in [None, Some(2001)] {
            let response = tick(
                State((state.clone(), 2000)),
                ConnectInfo(MemberPeer(uid)),
                headers.clone(),
                Json(WorkTick {
                    tenant: owner.tenant.to_string(),
                    volume: owner.volume.to_string(),
                    generation: owner.generation,
                    epoch: 1,
                }),
            )
            .await;
            assert!(matches!(response, Err(HttpError(StoreError::Forbidden))));
        }
    }
}
