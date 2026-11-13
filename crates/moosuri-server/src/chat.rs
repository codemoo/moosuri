//! Authenticated chat IO. Runtime tasks are independent of browser stream lifetimes.
use super::*;
use axum::{
    extract::Query,
    response::sse::{Event, KeepAlive, Sse},
};
use std::convert::Infallible;

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct PageQuery {
    before: Option<i64>,
}
#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct EventQuery {
    #[serde(default)]
    after: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SubmitInput {
    request_id: String,
    text: String,
    #[serde(default)]
    artifact_target: Option<moosuri_core::ArtifactTarget>,
    #[serde(default)]
    attachments: Vec<moosuri_core::MediaRef>,
    #[serde(default)]
    schedule_context: Option<moosuri_core::ScheduleChatContext>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct AttemptInput {
    expected_attempt: i32,
}

pub(super) async fn messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<PageQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.conversation(&user, id, query.before).await?).into_response())
}
pub(super) async fn submit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<SubmitInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    if state.runtime.is_stopping() {
        return Err(StoreError::Unavailable.into());
    }
    let turn = state
        .store
        .submit_turn_with_context(
            &user,
            id,
            input.request_id,
            input.text,
            input.artifact_target,
            input.attachments,
            input.schedule_context,
        )
        .await?;
    state.runtime.wake();
    Ok((StatusCode::CREATED, Json(turn)).into_response())
}
pub(super) async fn cancel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AttemptInput>,
) -> HttpResult {
    let _delivery = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    let turn = state
        .store
        .cancel_run(&user, id, input.expected_attempt)
        .await?;
    state.runtime.wake();
    Ok(Json(turn).into_response())
}
pub(super) async fn retry(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<AttemptInput>,
) -> HttpResult {
    let _delivery = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    if state.runtime.is_stopping() {
        return Err(StoreError::Unavailable.into());
    }
    let turn = state
        .store
        .retry_run(&user, id, input.expected_attempt)
        .await?;
    state.runtime.wake();
    Ok(Json(turn).into_response())
}
pub(super) async fn usage(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.usage_summary(&user).await?).into_response())
}
pub(super) async fn account_usage(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.account_usage(&user).await?).into_response())
}

struct StreamPermit {
    registry: Arc<Mutex<HashMap<String, usize>>>,
    account: String,
}
impl StreamPermit {
    fn acquire(state: &AppState, user: &UserContext) -> Result<Self, StoreError> {
        let mut registry = state.streams.lock().map_err(|_| StoreError::Unavailable)?;
        let account = user.account().id.clone();
        if registry.values().sum::<usize>() >= 64
            || registry.get(&account).copied().unwrap_or(0) >= 4
        {
            return Err(StoreError::Busy);
        }
        *registry.entry(account.clone()).or_default() += 1;
        Ok(Self {
            registry: state.streams.clone(),
            account,
        })
    }
}
impl Drop for StreamPermit {
    fn drop(&mut self) {
        if let Ok(mut registry) = self.registry.lock() {
            if let Some(count) = registry.get_mut(&self.account) {
                *count = count.saturating_sub(1);
            }
            if registry.get(&self.account) == Some(&0) {
                registry.remove(&self.account);
            }
        }
    }
}
fn closed(error: StoreError) -> Event {
    let code = match error {
        StoreError::Unauthorized => "unauthorized",
        StoreError::Forbidden => "forbidden",
        StoreError::NotFound => "not_found",
        _ => "unavailable",
    };
    Event::default()
        .event("closed")
        .data(format!("{{\"code\":\"{code}\"}}"))
}
pub(super) async fn events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Query(query): Query<EventQuery>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    if query.after < 0 {
        return Err(StoreError::InvalidInput.into());
    }
    // Fail ordinary authentication/not-found before starting an SSE response.
    state
        .store
        .chat_events(&user, id.clone(), query.after)
        .await?;
    let permit = StreamPermit::acquire(&state, &user)?;
    let (sender, receiver) = tokio::sync::mpsc::channel::<Event>(2);
    tokio::spawn(deliver(state, user, id, query.after, sender, permit));
    let stream = futures_util::stream::unfold(receiver, |mut receiver| async move {
        receiver
            .recv()
            .await
            .map(|event| (Ok::<Event, Infallible>(event), receiver))
    });
    Ok(Sse::new(stream)
        .keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
        .into_response())
}

async fn deliver(
    state: AppState,
    user: UserContext,
    id: String,
    mut cursor: i64,
    sender: tokio::sync::mpsc::Sender<Event>,
    _permit: StreamPermit,
) {
    let mut changed = state.runtime.subscribe();
    loop {
        // Wait for transport capacity before taking the delivery gate. A slow
        // browser cannot hold the deletion/password/logout gate indefinitely.
        let Ok(Ok(capacity)) = tokio::time::timeout(Duration::from_secs(3), sender.reserve()).await
        else {
            break;
        };
        let gate = state.delivery.read().await;
        if state.runtime.is_stopping() {
            capacity.send(closed(StoreError::Unavailable));
            break;
        }
        let batch = match state.store.chat_events(&user, id.clone(), cursor).await {
            Ok(batch) => batch,
            Err(error) => {
                capacity.send(closed(error));
                break;
            }
        };
        if batch.reset {
            capacity.send(Event::default().event("reset").data("{}"));
            break;
        }
        let had_events = !batch.events.is_empty();
        let mut capacity = Some(capacity);
        for event in batch.events {
            let Ok(encoded) = Event::default()
                .event("change")
                .id(event.sequence.to_string())
                .json_data(&event)
            else {
                return;
            };
            if let Some(capacity) = capacity.take() {
                capacity.send(encoded);
            } else if sender.try_send(encoded).is_err() {
                // Re-read unqueued events from the committed cursor, after a new
                // authorization check. Never retain a stale private batch.
                break;
            }
            cursor = event.sequence;
        }
        drop(capacity);
        drop(gate);
        if had_events {
            continue;
        }
        tokio::select! {
            _ = sender.closed() => break,
            _ = changed.changed() => (),
            _ = tokio::time::sleep(Duration::from_millis(250)) => (),
        }
    }
}
