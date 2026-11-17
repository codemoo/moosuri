use super::*;
use moosuri_connectors::telegram::Error;
use moosuri_store::telegram::PairInput;
use zeroize::Zeroizing;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Save {
    expected_revision: i64,
    token: Zeroizing<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Revision {
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Notifications {
    binding_id: String,
    notifications: bool,
}
pub(super) fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/v1/admin/telegram",
            get(admin).put(save).delete(disable),
        )
        .route(
            "/api/v1/settings/telegram",
            get(personal).put(notifications).delete(disconnect),
        )
        .route("/api/v1/settings/telegram/pair", post(pair))
        .layer(DefaultBodyLimit::max(4096))
}
async fn admin(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.telegram_settings(&u, true).await?).into_response())
}
async fn personal(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.telegram_settings(&u, false).await?).into_response())
}
async fn save(State(s): State<AppState>, h: HeaderMap, Json(input): Json<Save>) -> HttpResult {
    let u = current(&s, &h).await?;
    let current = s.store.telegram_settings(&u, true).await?;
    if current.bot.revision != input.expected_revision {
        return Err(StoreError::Conflict.into());
    }
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let client = s.store.telegram_client(input.token.clone()).await?;
    let identity = match client.verify().await {
        Ok(i) => i,
        Err(e) => {
            return Ok(failure(
                if e == Error::Conflict {
                    StatusCode::CONFLICT
                } else {
                    StatusCode::BAD_REQUEST
                },
                match e {
                    Error::Conflict => "telegram_webhook_or_consumer_conflict",
                    Error::Unauthorized => "telegram_token_rejected",
                    _ => "telegram_connection_failed",
                },
            ));
        }
    };
    let _gate = s.delivery.write().await;
    s.store
        .configure_telegram(&u, input.expected_revision, identity, input.token)
        .await?;
    s.runtime.wake();
    Ok(Json(s.store.telegram_settings(&u, true).await?).into_response())
}
async fn disable(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(input): Json<Revision>,
) -> HttpResult {
    let _gate = s.delivery.write().await;
    let u = current(&s, &h).await?;
    s.store
        .disable_telegram(&u, input.expected_revision)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn pair(State(s): State<AppState>, h: HeaderMap, Json(input): Json<PairInput>) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.pair_telegram(&u, input).await?).into_response())
}
async fn disconnect(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let _gate = s.delivery.write().await;
    let u = current(&s, &h).await?;
    s.store.disconnect_telegram(&u).await?;
    s.runtime.wake();
    Ok(StatusCode::NO_CONTENT.into_response())
}
async fn notifications(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(input): Json<Notifications>,
) -> HttpResult {
    let _gate = s.delivery.write().await;
    let u = current(&s, &h).await?;
    s.store
        .telegram_notifications(&u, input.binding_id, input.notifications)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
