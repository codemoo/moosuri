//! Admin-owned connections and user-owned model preferences. No provider body or
//! API credential crosses this boundary in a response, trace or error.
use super::*;
use moosuri_connectors::model::{
    CancellationToken, ModelClient, Secret, TransportError, TransportLimits,
    protocol::{FinishOutcome, ModelEvent, TextCapabilities, TextMessage, TextRequest, TextRole},
};
use moosuri_core::{ModelCatalog, ModelCatalogEntry, ModelSelection, ProviderConfig};
use moosuri_store::{CredentialChange, ProviderBinding, ProviderChange};
use zeroize::Zeroizing;

#[derive(Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
enum CredentialInput {
    Keep,
    Replace { value: Zeroizing<String> },
    Clear,
}
impl From<CredentialInput> for CredentialChange {
    fn from(value: CredentialInput) -> Self {
        match value {
            CredentialInput::Keep => Self::Keep,
            CredentialInput::Replace { value } => Self::Replace(value),
            CredentialInput::Clear => Self::Clear,
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProviderInput {
    name: String,
    config: ProviderConfig,
    enabled: bool,
    credential: CredentialInput,
}
impl ProviderInput {
    fn into_change(self) -> Result<ProviderChange, HttpError> {
        // Constructor validation performs no DNS/HTTP. The real transport repeats
        // destination validation immediately before any credential can be sent.
        validate_config(&self.config)?;
        Ok(ProviderChange {
            name: self.name,
            config: self.config,
            enabled: self.enabled,
            credential: self.credential.into(),
        })
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateInput {
    name: String,
    config: ProviderConfig,
    enabled: bool,
    credential: CredentialInput,
    expected_revision: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct SelectionInput {
    selection: Option<ModelSelection>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProbeInput {
    revision: i64,
    model: String,
    effort: Option<String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogInput {
    revision: i64,
}

fn validate_config(config: &ProviderConfig) -> Result<(), HttpError> {
    // A fixed synthetic value is used ONLY for constructing/validating configuration.
    // This client is immediately dropped and never calls a network operation.
    let dummy = Secret::new("configuration-validation-only".into())
        .map_err(|_| HttpError(StoreError::InvalidInput))?;
    moosuri_agent::model_client(config, Some(dummy), TextCapabilities::default())
        .map(|_| ())
        .map_err(|_| StoreError::InvalidInput.into())
}

fn bound_client(binding: &ProviderBinding) -> Result<ModelClient, TransportError> {
    let secret = binding
        .credential
        .as_ref()
        .map(|v| Secret::new(v.to_string()))
        .transpose()?;
    moosuri_agent::model_client(
        &binding.config,
        secret,
        TextCapabilities {
            // This explicit probe asks the provider whether this exact effort works.
            // Normal execution will only use capabilities recorded after full completion.
            efforts: binding.effort.iter().cloned().collect(),
        },
    )?
    .with_limits(TransportLimits {
        connect: Duration::from_secs(10),
        idle: Duration::from_secs(20),
        total: Duration::from_secs(60),
        response_bytes: 1024 * 1024,
    })
}

pub(super) async fn list(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.list_providers(&user).await?).into_response())
}
pub(super) async fn create(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<ProviderInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let provider = state
        .store
        .create_provider(&user, input.into_change()?)
        .await?;
    Ok((StatusCode::CREATED, Json(provider)).into_response())
}
pub(super) async fn update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<UpdateInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let revision = input.expected_revision;
    let change = ProviderInput {
        name: input.name,
        config: input.config,
        enabled: input.enabled,
        credential: input.credential,
    }
    .into_change()?;
    Ok(Json(
        state
            .store
            .update_provider(&user, id, revision, change)
            .await?,
    )
    .into_response())
}
pub(super) async fn set_default(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SelectionInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(
        state
            .store
            .set_default_model(&user, input.selection)
            .await?,
    )
    .into_response())
}
pub(super) async fn settings(State(state): State<AppState>, headers: HeaderMap) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(state.store.model_settings(&user).await?).into_response())
}
pub(super) async fn set_preference(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(input): Json<SelectionInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    Ok(Json(
        state
            .store
            .set_model_preference(&user, input.selection)
            .await?,
    )
    .into_response())
}

fn transport_failure(error: TransportError) -> Response {
    let (status, code) = match error {
        TransportError::InvalidConfiguration | TransportError::DestinationDenied => {
            (StatusCode::BAD_REQUEST, "model_configuration_invalid")
        }
        TransportError::Timeout => (StatusCode::GATEWAY_TIMEOUT, "model_timeout"),
        TransportError::Http {
            status: 401 | 403, ..
        } => (StatusCode::BAD_GATEWAY, "model_credentials_rejected"),
        TransportError::Http { status: 429, .. } => {
            (StatusCode::TOO_MANY_REQUESTS, "model_rate_limited")
        }
        TransportError::Protocol(_) => (StatusCode::BAD_GATEWAY, "model_protocol_failed"),
        _ => (StatusCode::BAD_GATEWAY, "model_connection_failed"),
    };
    failure(status, code)
}

pub(super) async fn probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ProbeInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let binding = state
        .store
        .probe_binding(
            &user,
            id.clone(),
            input.revision,
            input.model.clone(),
            input.effort.clone(),
        )
        .await?;
    let _permit = state
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| HttpError(StoreError::Busy))?;
    let model = match bound_client(&binding) {
        Ok(client) => client,
        Err(error) => return Ok(transport_failure(error)),
    };
    let request = TextRequest {
        model: binding.model.clone(),
        effort: binding.effort.clone(),
        messages: vec![TextMessage {
            role: TextRole::User,
            text: "Reply with the single word OK.".into(),
            phase: None,
        }],
        max_output_tokens: 2048,
    };
    let cancellation = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let stream = model.stream_text(&request, &tx, &cancellation);
    tokio::pin!(stream);
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    let mut finish = None;
    let mut has_text = false;
    let mut reported_model = None;
    loop {
        tokio::select! {
            biased;
            _ = tick.tick() => {
                // Revoke/logout fences a probe while it is pending. Dropping its
                // future closes local IO; remote processing/charges may continue.
                state.store.validate_provider_binding(&user, id.clone(), input.revision).await?;
            }
            event = rx.recv() => {
                if let Some(event) = event { observe_probe(event, &mut finish, &mut has_text, &mut reported_model); }
            }
            result = &mut stream => {
                if let Err(error) = result { return Ok(transport_failure(error)); }
                while let Ok(event) = rx.try_recv() { observe_probe(event, &mut finish, &mut has_text, &mut reported_model); }
                break;
            }
        }
    }
    if finish != Some(FinishOutcome::Completed) || !has_text {
        return Ok(failure(StatusCode::BAD_GATEWAY, "model_probe_incomplete"));
    }
    if reported_model.as_deref() != Some(input.model.as_str()) {
        // An alias/snapshot mapping must be explicitly supported, never guessed
        // from a prefix. Unknown or substituted model identities stay unverified.
        return Ok(failure(
            StatusCode::BAD_GATEWAY,
            "model_identity_unverified",
        ));
    }
    let choice = state
        .store
        .record_model_probe(&user, id, input.revision, input.model, input.effort)
        .await?;
    Ok(Json(choice).into_response())
}
fn observe_probe(
    event: ModelEvent,
    finish: &mut Option<FinishOutcome>,
    has_text: &mut bool,
    reported_model: &mut Option<String>,
) {
    match event {
        ModelEvent::Started { model, .. } => *reported_model = model,
        ModelEvent::TextDelta { text, .. } => *has_text |= !text.trim().is_empty(),
        ModelEvent::Finished { outcome, .. } => *finish = Some(outcome),
        _ => (),
    }
}

pub(super) async fn catalog(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<CatalogInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let binding = state
        .store
        .probe_binding(&user, id.clone(), input.revision, "catalog".into(), None)
        .await?;
    let _permit = state
        .probes
        .clone()
        .try_acquire_owned()
        .map_err(|_| HttpError(StoreError::Busy))?;
    let model = match bound_client(&binding) {
        Ok(client) => client,
        Err(error) => return Ok(transport_failure(error)),
    };
    let cancellation = CancellationToken::new();
    let operation = model.list_models(&cancellation);
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    let catalog = loop {
        tokio::select! {
            biased;
            _ = tick.tick() => state.store.validate_provider_binding(&user,id.clone(),input.revision).await?,
            result = &mut operation => match result {
                Ok(catalog) => break catalog,
                Err(error) => return Ok(transport_failure(error)),
            },
        }
    };
    state
        .store
        .validate_provider_binding(&user, id, input.revision)
        .await?;
    Ok(Json(ModelCatalog {
        models: catalog
            .models
            .into_iter()
            .map(|model| ModelCatalogEntry {
                id: model.id,
                name: model.name,
            })
            .collect(),
        truncated: catalog.truncated,
    })
    .into_response())
}
