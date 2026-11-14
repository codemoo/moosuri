//! Dictation transport never enters chat or invokes tools.
use super::*;
use axum::body::{Body, to_bytes};
use moosuri_connectors::{
    audio::{AudioError, MAX_WAV, validate_wav},
    model::CancellationToken,
};
use moosuri_core::AudioSelection;
use moosuri_store::audio::AudioAdmission;
use tokio::sync::Semaphore;
static UPLOADS: Semaphore = Semaphore::const_new(2);
pub(super) async fn settings(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.audio_settings(&user).await?).into_response())
}
pub(super) async fn status(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _gate = state.delivery.read().await;
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.audio_job(&user, id).await?).into_response())
}
pub(super) async fn remove(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let _gate = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    state.store.audio_delete(&user, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
pub(super) async fn disable(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let _gate = state.delivery.write().await;
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.audio_select_verified(&user, None, None).await?).into_response())
}
async fn wav(body: Body) -> Result<(Vec<u8>, u32), Response> {
    let bytes = tokio::time::timeout(Duration::from_secs(30), to_bytes(body, MAX_WAV))
        .await
        .map_err(|_| failure(StatusCode::BAD_REQUEST, "audio_invalid"))?
        .map_err(|_| failure(StatusCode::BAD_REQUEST, "audio_invalid"))?;
    let ms = validate_wav(&bytes).map_err(|_| failure(StatusCode::BAD_REQUEST, "audio_invalid"))?;
    Ok((bytes.to_vec(), ms))
}
pub(super) async fn upload(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    body: Body,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let permit = UPLOADS.try_acquire().map_err(|_| StoreError::Busy)?;
    let (bytes, ms) = match wav(body).await {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    let digest = Sha256::digest(&bytes).into();
    match state
        .store
        .audio_begin(&user, id.clone(), digest, ms)
        .await?
    {
        AudioAdmission::Existing(job) => Ok(Json(job).into_response()),
        AudioAdmission::Dispatch(lease, binding) => {
            let lease = *lease;
            let store = state.store.clone();
            let client = state.config.audio.clone();
            // Own admission until settlement. Lost upload ACK does not permit another bill.
            tokio::spawn(async move {
                let _permit = permit;
                let cancel = CancellationToken::new();
                let Some(secret) = binding.credential else {
                    let _ = store
                        .audio_finish(lease, None, Some("audio_unavailable".into()))
                        .await;
                    return;
                };
                if store.audio_alive(&lease).await.is_err() {
                    let _ = store
                        .audio_finish(lease, None, Some("audio_outcome_unknown".into()))
                        .await;
                    return;
                }
                let operation = client.transcribe(secret, &binding.model, bytes, &cancel);
                tokio::pin!(operation);
                let mut tick = tokio::time::interval(Duration::from_millis(250));
                let result = loop {
                    tokio::select! {r=&mut operation=>break r,_=tick.tick()=>if store.audio_alive(&lease).await.is_err(){cancel.cancel();break Err(AudioError::Unknown)}}
                };
                let (text, error) = match result {
                    Ok(t) => (Some(t), None),
                    Err(e) => (None, Some(e.to_string())),
                };
                let _ = store.audio_finish(lease, text, error).await;
            });
            Ok((
                StatusCode::ACCEPTED,
                Json(state.store.audio_job(&user, id).await?),
            )
                .into_response())
        }
    }
}
// A public phrase, never sent as a prompt to the provider. Actual response must match.
const PHRASE: &str = "the blue bird sings beside the quiet river";
fn words(text: &str) -> String {
    text.split(|c: char| !c.is_alphanumeric())
        .filter(|v| !v.is_empty())
        .map(str::to_lowercase)
        .collect::<Vec<_>>()
        .join(" ")
}
pub(super) async fn probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Body,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    // Check admin+fixed provider before collecting a private binary body.
    let selection = AudioSelection {
        provider_id: headers
            .get("X-Audio-Provider")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .into(),
        provider_revision: headers
            .get("X-Audio-Revision")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        model: "gpt-4o-mini-transcribe".into(),
    };
    let (binding, generation) = state
        .store
        .audio_probe_admit(&user, selection.clone())
        .await?;
    let _permit = state
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let (bytes, ms) = match wav(body).await {
        Ok(v) => v,
        Err(r) => return Ok(r),
    };
    if ms > 15000 {
        return Ok(failure(StatusCode::BAD_REQUEST, "audio_probe_too_long"));
    }
    state
        .store
        .audio_probe_current(&user, selection.clone(), generation)
        .await?;
    let cancel = CancellationToken::new();
    let secret = binding.credential.ok_or(StoreError::Unavailable)?;
    let operation = state
        .config
        .audio
        .transcribe(secret, &binding.model, bytes, &cancel);
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let result = loop {
        tokio::select! {
            r=&mut operation=>break r,
            _=tick.tick()=>if let Err(error)=state.store.audio_probe_current(&user,selection.clone(),generation).await {
                cancel.cancel();return Err(error.into());
            }
        }
    };
    match result {
        Ok(t) if words(&t) == PHRASE => {
            let _gate = state.delivery.write().await;
            Ok(Json(
                state
                    .store
                    .audio_select_verified(&user, Some(selection), Some(generation))
                    .await?,
            )
            .into_response())
        }
        Ok(_) => Ok(failure(StatusCode::BAD_GATEWAY, "audio_probe_mismatch")),
        Err(e) => Ok(failure(StatusCode::BAD_GATEWAY, &e.to_string())),
    }
}
