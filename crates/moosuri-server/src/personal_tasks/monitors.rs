//! `/api/v1/personal/tasks/monitors`: member-owned change-only watches and daily local briefs.
//! Every handler authenticates through `personal_request_user` (session + auth epoch), and the
//! store re-checks owner, tenant, volume and generation. Reads never run a check; the member timer does.
use super::*;
use moosuri_store::personal_browser::{MonitorRequest, MonitorStop};
use uuid::Uuid;

pub(super) fn routes() -> Router<MemberTasks> {
    Router::new()
        .route("/api/v1/personal/tasks/monitors", get(list).post(create))
        .route("/api/v1/personal/tasks/monitors/{id}", get(detail))
        .route(
            "/api/v1/personal/tasks/monitors/{id}/stop",
            axum::routing::post(stop),
        )
}

async fn list(State(s): State<MemberTasks>, headers: HeaderMap) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(s.store.member_monitors(s.owner, user).await?).into_response())
}

async fn detail(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(s.store.member_monitor(s.owner, user, id).await?).into_response())
}

async fn create(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Json(request): Json<MonitorRequest>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    let epoch = crate::personal_text::gateway_epoch(&headers)?;
    Ok(Json(
        s.store
            .create_member_monitor(s.owner, user, epoch, request)
            .await?,
    )
    .into_response())
}

async fn stop(
    State(s): State<MemberTasks>,
    headers: HeaderMap,
    Path(id): Path<Uuid>,
    Json(request): Json<MonitorStop>,
) -> HttpResult {
    let user = personal_request_user(&s.store, &headers).await?;
    Ok(Json(
        s.store
            .stop_member_monitor(s.owner, user, id, request)
            .await?,
    )
    .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_contract_is_strict_snake_case() {
        let body = |extra: &str| {
            format!(
                r#"{{"request_id":"{}","kind":"brief","title":"브리핑","scope":"memory","query":"일정",
                "at":"09:00","timezone":"Asia/Seoul","days":7{extra}}}"#,
                Uuid::new_v4()
            )
        };
        let ok: MonitorRequest = serde_json::from_str(&body("")).unwrap();
        assert_eq!(ok.at.as_deref(), Some("09:00"));
        assert!(ok.every_seconds.is_none());
        // Unknown fields (for example a smuggled URL to probe) are refused, not ignored.
        assert!(
            serde_json::from_str::<MonitorRequest>(&body(r#","url":"https://example.org""#))
                .is_err()
        );
        assert!(
            serde_json::from_str::<MonitorStop>(r#"{"request_id":"x","revision":1,"force":true}"#)
                .is_err()
        );
    }
}
