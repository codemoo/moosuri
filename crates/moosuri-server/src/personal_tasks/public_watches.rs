//! `/api/v1/personal/tasks/public-watches`: an explicit, change-only watch of one public HTTPS page.
//!
//! Every handler authenticates through `personal_request_user` (session + auth epoch); the store
//! re-checks owner, tenant, volume and generation and pins the grant to the exact URL, thread and epochs.
//! Reads here never touch the page. Only the gateway-driven member timer (`/internal/member-work/tick`)
//! runs due slots, through [`run_due`]:
//!
//! 0. `public_watch_orphans` – stops disposable workspaces whose read ended without a confirmed stop;
//! 1. `claim_due_public_watches` – one transaction: dedup row + fresh workspace id + next due time, no IO;
//! 2. [`PublicReads::read`] – the app's typed reader: open the fresh workspace on the pinned URL through
//!    the verified member browser broker, read bounded text, stop it. Nothing else;
//! 3. `settle_public_watch` – the store alone decides baseline / unchanged / changed / failed.
//!
//! A process death between 1 and 3 leaves the slot `unknown` at restart; it is never read again.
use super::*;
use moosuri_store::personal_browser::{
    PublicRead, PublicReadOutcome, PublicWatchLease, PublicWatchRequest, PublicWatchStop,
};
use std::{future::Future, pin::Pin, time::Duration};
use uuid::Uuid;

/// Upper bound for one slot (open, read, stop). Longer than the broker's own deadlines.
const READ_LIMIT: Duration = Duration::from_secs(100);
const STOP_LIMIT: Duration = Duration::from_secs(50);

pub type PublicReadFuture<'a> = Pin<Box<dyn Future<Output = PublicRead> + Send + 'a>>;
pub type PublicStopFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;

/// Disposable, read-only page access for one claimed slot. Implemented by the app next to the existing
/// verified member browser broker; this crate never sees a socket, a cookie or a session.
///
/// Contract: `read` may only open `lease.url` in the *fresh* workspace `lease.workspace_id`, read its
/// bounded visible text, and stop that workspace (`closed` reports whether the stop was confirmed).
/// `stop` stops one disposable workspace id handed out by the store; it returns true when the broker
/// confirmed the workspace is gone (or never existed).
pub trait PublicReads: Send + Sync {
    fn read<'a>(&'a self, lease: &'a PublicWatchLease) -> PublicReadFuture<'a>;
    fn stop<'a>(&'a self, workspace_id: &'a str) -> PublicStopFuture<'a>;
}

pub(super) fn routes() -> Router<MemberTasks> {
    Router::new()
        .route(
            "/api/v1/personal/tasks/public-watches",
            get(list).post(create),
        )
        .route(
            "/api/v1/personal/tasks/public-watches/{id}/stop",
            axum::routing::post(stop),
        )
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ThreadFilter {
    thread_id: Uuid,
}

async fn list(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    axum::extract::Query(filter): axum::extract::Query<ThreadFilter>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .public_watches(
                s.owner,
                user,
                crate::personal_text::gateway_epoch(&headers)?,
                filter.thread_id,
            )
            .await?,
    )
    .into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Json(request): Json<PublicWatchRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .create_public_watch(s.owner, user, epoch, request)
            .await?,
    )
    .into_response())
}

async fn stop(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<PublicWatchStop>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .stop_public_watch(
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

/// Timer entry point. Without a reader a due slot is still claimed once and recorded `failed`
/// (a visible, honest outcome); it is never retried and never reported as a change.
pub(super) async fn run_due(
    store: &Store,
    owner: moosuri_store::personal_volume::VolumeOwner,
    host_epoch: i64,
    reads: Option<&dyn PublicReads>,
) -> usize {
    // Reaps stale claims even without a reader; only a reader can stop what it left behind.
    if let Ok(orphans) = store.public_watch_orphans(owner, clock()).await
        && let Some(reader) = reads
    {
        for workspace in orphans {
            if tokio::time::timeout(STOP_LIMIT, reader.stop(&workspace))
                .await
                .unwrap_or(false)
            {
                let _ = store.ack_public_watch_workspace(owner, workspace).await;
            }
        }
    }
    let Ok(leases) = store
        .claim_due_public_watches(owner, host_epoch, clock())
        .await
    else {
        return 0;
    };
    let count = leases.len();
    for lease in leases {
        if !store
            .public_watch_can_read(lease.clone())
            .await
            .unwrap_or(false)
        {
            let _ = store
                .settle_public_watch(
                    lease,
                    PublicRead {
                        outcome: PublicReadOutcome::Failed("grant_unavailable"),
                        closed: true,
                    },
                )
                .await;
            continue;
        }
        let read = match reads {
            Some(reader) => match tokio::time::timeout(READ_LIMIT, reader.read(&lease)).await {
                Ok(read) => read,
                // The reader was cut off: its workspace is unconfirmed and is stopped by step 0 later.
                Err(_) => PublicRead {
                    outcome: PublicReadOutcome::Failed("timeout"),
                    closed: false,
                },
            },
            None => PublicRead {
                outcome: PublicReadOutcome::Failed("reader_unavailable"),
                closed: true,
            },
        };
        let _ = store.settle_public_watch(lease, read).await;
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;
    use moosuri_store::personal_browser::{PublicWatchCheck, PublicWatchPage, PublicWatchView};

    #[test]
    fn contract_is_strict_snake_case_and_omits_absent_excerpts() {
        let body = |extra: &str| {
            format!(
                r#"{{"thread_id":"{}","request_id":"{}","message":"매시간 확인","url":"https://status.example.test/service",
                "every_seconds":3600,"days":7{extra}}}"#,
                Uuid::new_v4(),
                Uuid::new_v4()
            )
        };
        let ok: PublicWatchRequest = serde_json::from_str(&body("")).unwrap();
        assert_eq!((ok.every_seconds, ok.days), (3600, 7));
        // A smuggled header, cookie, method or selector is refused, not ignored.
        for extra in [
            r#","cookie":"a=b""#,
            r#","headers":{"x":"y"}"#,
            r#","method":"POST""#,
            r##","selector":"#main""##,
        ] {
            assert!(
                serde_json::from_str::<PublicWatchRequest>(&body(extra)).is_err(),
                "{extra}"
            );
        }
        assert!(
            serde_json::from_str::<PublicWatchStop>(
                r#"{"request_id":"x","revision":1,"force":true}"#
            )
            .is_err()
        );
        // The list query takes exactly one canonical thread id.
        assert!(
            serde_json::from_str::<ThreadFilter>(&format!(
                r#"{{"thread_id":"{}"}}"#,
                Uuid::new_v4()
            ))
            .is_ok()
        );
        assert!(serde_json::from_str::<ThreadFilter>(r#"{"thread_id":"x"}"#).is_err());
        let item = |excerpt: Option<&str>| PublicWatchView {
            id: Uuid::new_v4().to_string(),
            thread_id: Uuid::new_v4().to_string(),
            url: "https://status.example.test/service".into(),
            every_seconds: 3600,
            days: 7,
            state: "active".into(),
            revision: 1,
            checks: vec![PublicWatchCheck {
                due_at: 1,
                state: "baseline".into(),
                excerpt: excerpt.map(str::to_owned),
            }],
        };
        let json = serde_json::to_value(PublicWatchPage {
            items: vec![item(None), item(Some("ok"))],
        })
        .unwrap();
        let keys = |v: &serde_json::Value| {
            let mut k: Vec<_> = v.as_object().unwrap().keys().cloned().collect();
            k.sort();
            k
        };
        assert_eq!(
            keys(&json["items"][0]),
            [
                "checks",
                "days",
                "every_seconds",
                "id",
                "revision",
                "state",
                "thread_id",
                "url"
            ]
        );
        assert_eq!(keys(&json["items"][0]["checks"][0]), ["due_at", "state"]);
        assert_eq!(
            keys(&json["items"][1]["checks"][0]),
            ["due_at", "excerpt", "state"]
        );
    }
}
