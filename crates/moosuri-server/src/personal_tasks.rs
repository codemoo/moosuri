//! Historical reads remain available if the browser worker is closed or offline.
use super::*;
mod briefs;
mod public_watches;
pub use public_watches::{PublicReadFuture, PublicReads, PublicStopFuture};
mod conversation;
mod monitors;
pub use briefs::{BriefReads, ReadFuture};
mod research_outcomes;
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
        .merge(conversation::routes())
        .merge(monitors::routes())
        .merge(briefs::routes())
        .merge(public_watches::routes())
        .merge(research_outcomes::routes())
        .route("/api/v1/personal/tasks", get(page))
        .route("/api/v1/personal/tasks/{id}", get(detail))
        .route(
            "/api/v1/personal/tasks/workflows",
            axum::routing::post(create),
        )
        .route(
            "/api/v1/personal/tasks/workflows/{id}",
            get(schedule_history).post(change),
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
    timer_routes_with_briefs(store, owner, gateway_uid, None)
}

/// Same timer route, plus the live read-only reader for connected morning briefs. Without a reader a
/// due brief slot is recorded `unavailable` once; it is never reported as completed.
pub fn timer_routes_with_briefs(
    store: Store,
    owner: VolumeOwner,
    gateway_uid: u32,
    reads: Option<std::sync::Arc<dyn BriefReads>>,
) -> Router {
    timer_routes_with_readers(store, owner, gateway_uid, reads, None)
}

pub fn timer_routes_with_readers(
    store: Store,
    owner: VolumeOwner,
    gateway_uid: u32,
    reads: Option<std::sync::Arc<dyn BriefReads>>,
    public: Option<std::sync::Arc<dyn PublicReads>>,
) -> Router {
    Router::new()
        .route("/internal/member-work/tick", axum::routing::post(tick))
        .with_state(Tick::new(
            MemberTasks { store, owner },
            gateway_uid,
            reads,
            public,
        ))
}

#[derive(Clone)]
struct Tick {
    tasks: MemberTasks,
    gateway_uid: u32,
    reads: Option<std::sync::Arc<dyn BriefReads>>,
    public: Option<std::sync::Arc<dyn PublicReads>>,
    watch_gate: std::sync::Arc<tokio::sync::Semaphore>,
    /// One brief batch at a time; the 5 s gateway timer never stacks provider reads.
    gate: std::sync::Arc<tokio::sync::Semaphore>,
}
impl Tick {
    fn new(
        tasks: MemberTasks,
        gateway_uid: u32,
        reads: Option<std::sync::Arc<dyn BriefReads>>,
        public: Option<std::sync::Arc<dyn PublicReads>>,
    ) -> Self {
        Self {
            tasks,
            gateway_uid,
            reads,
            public,
            watch_gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
            gate: std::sync::Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
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

async fn schedule_history(
    State(s): State<MemberTasks>,
    h: HeaderMap,
    Path(id): Path<uuid::Uuid>,
    Query(q): Query<PageQuery>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &h).await?;
    Ok(Json(
        s.store
            .member_task_history_page(s.owner, user, Some(id), q.before)
            .await?,
    )
    .into_response())
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
    State(Tick {
        tasks: s,
        gateway_uid,
        reads,
        gate,
        public,
        watch_gate,
    }): State<Tick>,
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
    // Connected briefs do provider IO, so they run detached: a gateway timeout cannot cancel a claimed
    // slot half-way. Their failures never fail the existing member tick.
    s.store.fence_member_brief_epoch(s.owner, pin.epoch).await?;
    s.store.fence_public_watch_epoch(s.owner, pin.epoch).await?;
    if let Ok(permit) = watch_gate.try_acquire_owned() {
        let (store, owner, epoch) = (s.store.clone(), s.owner, pin.epoch);
        tokio::spawn(async move {
            public_watches::run_due(&store, owner, epoch, public.as_deref()).await;
            drop(permit);
        });
    }
    if let Ok(permit) = gate.try_acquire_owned() {
        let (store, owner, epoch) = (s.store.clone(), s.owner, pin.epoch);
        tokio::spawn(async move {
            briefs::run_due(&store, owner, epoch, reads.as_deref()).await;
            drop(permit);
        });
    }
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
                State(Tick::new(state.clone(), 2000, None, None)),
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
