//! Authenticated admin-only fixed invitation effect. No model/tool route.
use super::*;
use moosuri_connectors::invitation::InvitationSendResult;
use moosuri_core::{InvitationApproval, InvitationPreviewInput, InvitationSenderStatus};
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route("/sender", get(sender))
        .route("/", get(list).post(preview))
        .route("/{id}", get(one))
        .route("/{id}/approve", post(approve))
        .layer(DefaultBodyLimit::max(4096))
}
async fn sender(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    s.store.managed_accounts(&u).await?;
    Ok(Json(InvitationSenderStatus {
        configured: s.config.invitation_sender.is_some(),
        from: s
            .config
            .invitation_sender
            .as_ref()
            .map(|v| v.from().to_owned()),
    })
    .into_response())
}
async fn list(State(s): State<AppState>, headers: HeaderMap) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.invitation_deliveries(&u).await?).into_response())
}
async fn preview(
    State(s): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<InvitationPreviewInput>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let sender = s
        .config
        .invitation_sender
        .as_ref()
        .ok_or(StoreError::Unavailable)?;
    let v = s
        .store
        .preview_invitation(
            &u,
            input,
            sender.from().into(),
            s.config.origin.clone(),
            sender.pin().into(),
        )
        .await?;
    Ok(Json(v).into_response())
}
async fn approve(
    State(s): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<InvitationApproval>,
) -> HttpResult {
    let u = current(&s, &headers).await?;
    let sender = s
        .config
        .invitation_sender
        .as_ref()
        .ok_or(StoreError::Unavailable)?;
    let _permit = s
        .invitation_sends
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    if !sender.ready().await {
        return Err(StoreError::Unavailable.into());
    }
    let dispatch = s
        .store
        .commit_invitation(&u, id.clone(), input.digest, sender.pin().into())
        .await?;
    if let Some(d) = dispatch {
        if s.store.validate_invitation_dispatch(&d).await.is_err() {
            s.store.finish_invitation(&d, None, true).await?;
        } else {
            let (receipt, definite) = match sender.send_ready(&d.frozen).await {
                InvitationSendResult::Sent(id) => (Some(id), false),
                InvitationSendResult::DefiniteFailure => (None, true),
                InvitationSendResult::Unknown => (None, false),
            };
            // Persist only the local receipt again if busy. Never replay native delivery.
            for _ in 0..5 {
                match s
                    .store
                    .finish_invitation(&d, receipt.clone(), definite)
                    .await
                {
                    Err(StoreError::Busy) => {
                        tokio::time::sleep(std::time::Duration::from_millis(20)).await
                    }
                    _ => break,
                }
            }
        }
    }
    // Request ACK loss is reconciled by this immutable ID/list; never another send.
    let v = s.store.invitation_delivery(&u, id).await?;
    Ok(Json(v).into_response())
}

async fn one(State(s): State<AppState>, headers: HeaderMap, Path(id): Path<String>) -> HttpResult {
    let u = current(&s, &headers).await?;
    Ok(Json(s.store.invitation_delivery(&u, id).await?).into_response())
}
