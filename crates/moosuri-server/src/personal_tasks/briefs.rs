//! `/api/v1/personal/tasks/briefs`: the explicit, read-only, recurring Gmail + Calendar morning brief.
//!
//! Every handler authenticates through `personal_request_user` (session + auth epoch); the store
//! re-checks owner, tenant, volume and generation and pins the grant to the exact Google
//! connection id/epoch the member saw. Reads here never touch Google. Only the gateway-driven member
//! timer (`/internal/member-work/tick`) runs due slots, through [`run_due`]:
//!
//! 1. `claim_due_member_briefs` – one transaction: dedup row + next due time, no provider IO;
//! 2. [`BriefReads::read`] – the app's typed, read-only Gmail/Calendar adapter using the pinned
//!    background authority (no member session is impersonated or synthesised);
//! 3. `settle_member_brief` – keeps minimal encrypted evidence only if the grant is still live.
//!
//! A process death between 1 and 3 leaves the slot `unknown` at restart; it is never re-read.
use super::*;
use moosuri_store::{MemberBriefLease, MemberBriefOutcome, MemberBriefRequest, MemberBriefStop};
use std::{future::Future, pin::Pin, time::Duration};
use uuid::Uuid;

/// Upper bound for one slot's provider reads (refresh + inbox page + calendar list).
const READ_LIMIT: Duration = Duration::from_secs(90);

pub type ReadFuture<'a> = Pin<Box<dyn Future<Output = MemberBriefOutcome> + Send + 'a>>;

/// Live, read-only provider access for one claimed slot. Implemented by the app next to the existing
/// typed member Google adapters; this crate never sees tokens.
pub trait BriefReads: Send + Sync {
    fn read<'a>(&'a self, lease: &'a MemberBriefLease) -> ReadFuture<'a>;
}

pub(super) fn routes() -> Router<MemberTasks> {
    Router::new()
        .route("/api/v1/personal/tasks/briefs", get(list).post(create))
        .route("/api/v1/personal/tasks/briefs/{id}", get(detail))
        .route(
            "/api/v1/personal/tasks/briefs/{id}/stop",
            axum::routing::post(stop),
        )
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadFilter {
    thread_id: Option<Uuid>,
}
async fn list(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    axum::extract::Query(filter): axum::extract::Query<ThreadFilter>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .member_briefs(
                s.owner,
                user,
                crate::personal_text::gateway_epoch(&headers)?,
                filter.thread_id,
            )
            .await?,
    )
    .into_response())
}

async fn detail(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .member_brief(
                s.owner,
                user,
                crate::personal_text::gateway_epoch(&headers)?,
                id,
            )
            .await?,
    )
    .into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Json(request): Json<MemberBriefRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .create_member_brief(s.owner, user, epoch, request)
            .await?,
    )
    .into_response())
}

async fn stop(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<MemberBriefStop>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .stop_member_brief(
                s.owner,
                user,
                crate::personal_text::gateway_epoch(&headers)?,
                id,
                request,
            )
            .await?,
    )
    .into_response())
}

fn clock() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

/// Timer entry point. Without a reader the slot is still claimed once and recorded `unavailable`
/// (a visible, honest outcome) instead of being retried or reported as a completed brief.
pub(super) async fn run_due(
    store: &Store,
    owner: moosuri_store::personal_volume::VolumeOwner,
    host_epoch: i64,
    reads: Option<&dyn BriefReads>,
) -> usize {
    let Ok(leases) = store
        .claim_due_member_briefs(owner, host_epoch, clock())
        .await
    else {
        return 0;
    };
    let count = leases.len();
    for lease in leases {
        let outcome = match reads {
            Some(reader) => match tokio::time::timeout(READ_LIMIT, reader.read(&lease)).await {
                Ok(outcome) => outcome,
                Err(_) => MemberBriefOutcome::Unavailable("timeout"),
            },
            None => MemberBriefOutcome::Unavailable("reader_unavailable"),
        };
        let _ = store.settle_member_brief(lease, outcome).await;
    }
    count
}
