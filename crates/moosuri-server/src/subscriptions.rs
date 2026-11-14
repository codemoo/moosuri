//! Personal settings use the authenticated member, never shared admin authority.
use super::*;
use moosuri_connectors::model::{
    CancellationToken,
    protocol::{FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole},
};
use moosuri_store::{SubscriptionAuthority, SubscriptionRef};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct LoginInput {
    label: String,
    binding_id: Option<String>,
    expected_revision: Option<i64>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Input {
    revision: i64,
    model: Option<String>,
    effort: Option<String>,
}
pub(super) async fn list(State(s): State<AppState>, h: HeaderMap) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(Json(s.store.subscriptions(&u).await?).into_response())
}
pub(super) async fn login(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<LoginInput>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let origin = url::Url::parse(&s.config.origin).map_err(|_| StoreError::InvalidInput)?;
    if origin
        .host_str()
        .and_then(|v| v.parse::<IpAddr>().ok())
        .is_none_or(|v| !v.is_loopback())
    {
        return Ok(failure(StatusCode::CONFLICT, "same_host_login_required"));
    }
    let existing = match (i.binding_id, i.expected_revision) {
        (Some(id), Some(r)) => Some((id, r)),
        (None, None) => None,
        _ => return Err(StoreError::InvalidInput.into()),
    };
    Ok(Json(
        s.runtime
            .subscription_service()
            .local_login(&u, existing, i.label)
            .await?,
    )
    .into_response())
}
pub(super) async fn attempt(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    Ok(
        Json(serde_json::json!({"phase":s.store.subscription_attempt(&u,id).await?}))
            .into_response(),
    )
}
pub(super) async fn disconnect(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let confirmed = s
        .runtime
        .subscription_service()
        .disconnect(&u, id, i.revision)
        .await?;
    Ok(Json(serde_json::json!({"remote_revocation_confirmed":confirmed})).into_response())
}
async fn binding(
    s: &AppState,
    u: &UserContext,
    id: String,
    i: &Input,
) -> Result<SubscriptionRef, StoreError> {
    s.store
        .subscription_ref(
            u,
            id,
            i.revision,
            i.model.clone().unwrap_or_else(|| "catalog".into()),
            i.effort.clone(),
        )
        .await
}
async fn valid(s: &AppState, u: &UserContext, b: &SubscriptionRef) -> Result<(), StoreError> {
    s.store
        .subscription_ref(
            u,
            b.id.clone(),
            b.revision,
            b.model.clone(),
            b.effort.clone(),
        )
        .await
        .map(|_| ())
}
pub(super) async fn catalog(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    let u = current(&s, &h).await?;
    let b = binding(&s, &u, id, &i).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let client = s
        .runtime
        .subscription_service()
        .client(SubscriptionAuthority::User(u.clone()), b.clone())
        .await?;
    let cancel = CancellationToken::new();
    let operation = client.list_models(&cancel);
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let result = loop {
        tokio::select! {biased;_=tick.tick()=>{valid(&s,&u,&b).await?;},r=&mut operation=>break r}
    };
    valid(&s, &u, &b).await?;
    match result {
        Ok(c) => Ok(Json(moosuri_core::ModelCatalog {
            models: c
                .models
                .into_iter()
                .map(|v| moosuri_core::ModelCatalogEntry {
                    id: v.id,
                    name: v.name,
                })
                .collect(),
            truncated: c.truncated,
        })
        .into_response()),
        Err(e) => Ok(providers::transport_failure(e)),
    }
}
pub(super) async fn probe(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<Input>,
) -> HttpResult {
    if i.model.is_none() {
        return Err(StoreError::InvalidInput.into());
    }
    let u = current(&s, &h).await?;
    let b = binding(&s, &u, id, &i).await?;
    let _permit = s
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    let client = s
        .runtime
        .subscription_service()
        .client(SubscriptionAuthority::User(u.clone()), b.clone())
        .await?
        .with_limits(moosuri_connectors::model::TransportLimits {
            connect: Duration::from_secs(10),
            idle: Duration::from_secs(20),
            total: Duration::from_secs(60),
            response_bytes: 1024 * 1024,
        })
        .map_err(|_| StoreError::Unavailable)?;
    let request = TextRequest {
        model: b.model.clone(),
        effort: b.effort.clone(),
        max_output_tokens: 2048,
        messages: vec![TextMessage {
            role: TextRole::User,
            text: "Reply with the single word OK.".into(),
            phase: None,
        }],
    };
    let cancel = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let operation = client.stream_text(&request, &tx, &cancel);
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    let (mut model, mut text, mut finish, mut refusal) = (None, false, None, false);
    let observe = |e: ModelEvent,
                   model: &mut Option<String>,
                   text: &mut bool,
                   finish: &mut Option<FinishOutcome>,
                   refusal: &mut bool| match e {
        ModelEvent::Started { model: m, .. } => *model = m,
        ModelEvent::TextDelta { text: t, .. } => *text |= !t.trim().is_empty(),
        ModelEvent::Finished { outcome, .. } => *finish = Some(outcome),
        ModelEvent::RefusalDelta { .. } => *refusal = true,
        _ => (),
    };
    let result = loop {
        tokio::select! {biased;_=tick.tick()=>valid(&s,&u,&b).await?,Some(e)=rx.recv()=>observe(e,&mut model,&mut text,&mut finish,&mut refusal),r=&mut operation=>break r}
    };
    while let Ok(e) = rx.try_recv() {
        observe(e, &mut model, &mut text, &mut finish, &mut refusal)
    }
    valid(&s, &u, &b).await?;
    if let Err(e) = result {
        return Ok(providers::transport_failure(e));
    }
    if model.as_deref() != Some(&b.model)
        || !text
        || refusal
        || finish != Some(FinishOutcome::Completed)
    {
        return Ok(failure(StatusCode::BAD_GATEWAY, "model_probe_incomplete"));
    }
    Ok(Json(s.store.record_subscription_probe(&u, b).await?).into_response())
}

// Native HTTPS capability responses are never part of public/browser DTOs.
#[derive(serde::Serialize)]
struct PrivateHandoffPreparation {
    context: moosuri_connectors::chatgpt_oauth::handoff::HandoffContext,
    transfer_key: zeroize::Zeroizing<String>,
    expires_at: i64,
    client_id: Option<String>,
    subject: Option<String>,
}
pub(super) async fn handoff_prepare(
    State(s): State<AppState>,
    h: HeaderMap,
    Json(i): Json<LoginInput>,
) -> HttpResult {
    let u = current_handoff(&s, &h).await?;
    let origin = url::Url::parse(&s.config.origin).map_err(|_| StoreError::InvalidInput)?;
    let allowed = origin.scheme() == "https";
    #[cfg(feature = "test-fixtures")]
    let allowed = allowed
        || (origin.scheme() == "http"
            && origin.host_str() == Some("127.0.0.1")
            && origin.port().is_some());
    if !allowed {
        return Ok(failure(StatusCode::CONFLICT, "https_handoff_required"));
    }
    let existing = match (i.binding_id, i.expected_revision) {
        (Some(id), Some(r)) => Some((id, r)),
        (None, None) => None,
        _ => return Err(StoreError::InvalidInput.into()),
    };
    let p = s
        .store
        .prepare_chatgpt_handoff(&u, existing, i.label)
        .await?;
    Ok(Json(PrivateHandoffPreparation {
        context: p.context,
        transfer_key: p.key.for_private_transport(),
        expires_at: p.expires_at,
        client_id: p.client_id,
        subject: p.subject,
    })
    .into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct NonceInput {
    commitment: Vec<u8>,
    source_host: String,
}
pub(super) async fn handoff_nonce(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<NonceInput>,
) -> HttpResult {
    let u = current_handoff(&s, &h).await?;
    s.store
        .commit_chatgpt_handoff_nonce(&u, id, i.commitment, i.source_host)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RegistrationInput {
    client_id: String,
}
pub(super) async fn handoff_registration(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    Json(i): Json<RegistrationInput>,
) -> HttpResult {
    let u = current_handoff(&s, &h).await?;
    s.store
        .save_chatgpt_handoff_registration(&u, id, i.client_id)
        .await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
pub(super) async fn handoff_import(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
    body: axum::body::Bytes,
) -> HttpResult {
    let u = current_handoff(&s, &h).await?;
    Ok(Json(
        s.runtime
            .subscription_service()
            .import_handoff(&u, id, body.to_vec())
            .await?,
    )
    .into_response())
}
pub(super) async fn handoff_status(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current_handoff(&s, &h).await?;
    Ok(Json(s.store.chatgpt_handoff_status(&u, id).await?).into_response())
}
pub(super) async fn handoff_cancel(
    State(s): State<AppState>,
    h: HeaderMap,
    Path(id): Path<String>,
) -> HttpResult {
    let u = current_handoff(&s, &h).await?;
    s.store.cancel_chatgpt_handoff(&u, id).await?;
    Ok(StatusCode::NO_CONTENT.into_response())
}
