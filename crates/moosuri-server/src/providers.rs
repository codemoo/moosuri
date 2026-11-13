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

const TOOL_PROBE_NAME: &str = "moosuri_verification_ping";
const TOOL_PROBE_PROMPT: &str = "Call moosuri_verification_ping exactly once with an empty argument object. After receiving the tool result, reply with exactly its text, with no extra words or whitespace.";

// Fixed synthetic protocol proof. No knowledge access, arbitrary tool registry or
// client-supplied result can enter this verifier.
pub(super) async fn tool_probe(
    State(state): State<AppState>,
    headers: HeaderMap,
    Path(id): Path<String>,
    Json(input): Json<ProbeInput>,
) -> HttpResult {
    let user = current(&state, &headers).await?;
    let (binding, challenge) = state
        .store
        .tool_probe_binding(
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
    let client = match bound_client(&binding) {
        Ok(client) => client,
        Err(error) => return Ok(transport_failure(error)),
    };
    let cancellation = CancellationToken::new();
    let proof = verify_tool_probe(&state, &user, &binding, &client, &challenge, &cancellation);
    let result = tokio::time::timeout(Duration::from_secs(120), proof).await;
    cancellation.cancel();
    match result {
        Ok(Ok(())) => {}
        Ok(Err(ToolProbeError::Store(error))) => return Err(error.into()),
        Ok(Err(ToolProbeError::Transport(error))) => return Ok(transport_failure(error)),
        Ok(Err(ToolProbeError::Incomplete)) => {
            return Ok(failure(
                StatusCode::BAD_GATEWAY,
                "model_tool_probe_incomplete",
            ));
        }
        Err(_) => return Ok(failure(StatusCode::GATEWAY_TIMEOUT, "model_timeout")),
    }
    Ok(Json(
        state
            .store
            .record_tool_probe(&user, id, input.revision, input.model, input.effort)
            .await?,
    )
    .into_response())
}

enum ToolProbeError {
    Store(StoreError),
    Transport(TransportError),
    Incomplete,
}

#[derive(Default)]
struct ProbeObservation {
    reported_model: Option<String>,
    items: Vec<(
        String,
        Option<moosuri_connectors::model::protocol::AssistantPhase>,
        String,
    )>,
    refusal: bool,
    bytes: usize,
}
impl ProbeObservation {
    fn observe(&mut self, event: ModelEvent) -> Result<(), ToolProbeError> {
        let (key, phase, text) = match event {
            ModelEvent::Started { model, .. } => {
                self.reported_model = model;
                return Ok(());
            }
            ModelEvent::RefusalDelta { .. } => {
                self.refusal = true;
                return Ok(());
            }
            ModelEvent::MessageMetadata { item_id, phase } => (item_id, phase, String::new()),
            ModelEvent::TextDelta {
                text,
                item_id,
                phase,
            } => (
                item_id.unwrap_or_else(|| "unidentified-message".into()),
                phase,
                text,
            ),
            _ => return Ok(()),
        };
        self.bytes = self.bytes.saturating_add(text.len());
        if self.bytes > 16 * 1024 || self.items.len() >= 128 {
            return Err(ToolProbeError::Incomplete);
        }
        if let Some(item) = self.items.iter_mut().find(|item| item.0 == key) {
            if phase.is_some() {
                item.1 = phase;
            }
            item.2.push_str(&text);
        } else {
            self.items.push((key, phase, text));
        }
        // Explicit commentary can never establish a final answer. Unknown phase
        // remains eligible for dialects/models without native phase metadata.
        Ok(())
    }
    fn final_text(&self) -> String {
        self.items
            .iter()
            .filter(|item| {
                item.1 != Some(moosuri_connectors::model::protocol::AssistantPhase::Commentary)
            })
            .map(|item| item.2.as_str())
            .collect()
    }
}

async fn tool_probe_round(
    state: &AppState,
    user: &UserContext,
    binding: &ProviderBinding,
    client: &ModelClient,
    request: &moosuri_connectors::model::protocol::tools::ToolRequest<'_>,
    cancellation: &CancellationToken,
) -> Result<
    (
        moosuri_connectors::model::protocol::tools::ToolRound,
        ProbeObservation,
    ),
    ToolProbeError,
> {
    use moosuri_connectors::model::protocol::tools::ToolCapabilities;
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let capabilities = ToolCapabilities {
        function_tools: true,
    };
    let operation = client.stream_tool_round(request, &capabilities, &tx, cancellation);
    tokio::pin!(operation);
    let mut tick = tokio::time::interval(Duration::from_millis(500));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut observed = ProbeObservation::default();
    loop {
        tokio::select! {
            biased;
            _=tick.tick()=>{state.store.validate_provider_binding(user,binding.id.clone(),binding.revision).await.map_err(ToolProbeError::Store)?;},
            event=rx.recv()=>if let Some(event)=event{observed.observe(event)?;},
            result=&mut operation=>{
                let round=result.map_err(ToolProbeError::Transport)?;
                while let Ok(event)=rx.try_recv(){observed.observe(event)?;}
                state.store.validate_provider_binding(user,binding.id.clone(),binding.revision).await.map_err(ToolProbeError::Store)?;
                if observed.reported_model.as_deref()!=Some(binding.model.as_str()) || observed.refusal{return Err(ToolProbeError::Incomplete);}
                return Ok((round,observed));
            }
        }
    }
}

async fn verify_tool_probe(
    state: &AppState,
    user: &UserContext,
    binding: &ProviderBinding,
    client: &ModelClient,
    challenge: &str,
    cancellation: &CancellationToken,
) -> Result<(), ToolProbeError> {
    use moosuri_connectors::model::protocol::tools::{FunctionDefinition, ToolRequest, ToolResult};
    let definitions = [FunctionDefinition {
        name: TOOL_PROBE_NAME.into(),
        description: "Return a synthetic verification challenge. No external operation.".into(),
        parameters: serde_json::json!({"type":"object","properties":{},"required":[],"additionalProperties":false}),
    }];
    let text = TextRequest {
        model: binding.model.clone(),
        effort: binding.effort.clone(),
        // Verification is an application protocol, not a user's chat request.
        // Keep its exact-output contract in the instruction role across both
        // rounds; the unknown challenge is still supplied only as a tool result.
        messages: vec![
            TextMessage {
                role: TextRole::System,
                text: TOOL_PROBE_PROMPT.into(),
                phase: None,
            },
            TextMessage {
                role: TextRole::User,
                text: "Verify the tool connection.".into(),
                phase: None,
            },
        ],
        max_output_tokens: 2048,
    };
    let (first, _) = tool_probe_round(
        state,
        user,
        binding,
        client,
        &ToolRequest {
            text: &text,
            tools: &definitions,
            continuation: None,
            results: &[],
        },
        cancellation,
    )
    .await?;
    if first.outcome != FinishOutcome::ContinuationRequired || first.calls.len() != 1 {
        return Err(ToolProbeError::Incomplete);
    }
    let call = &first.calls[0];
    let args: serde_json::Value =
        serde_json::from_str(call.arguments()).map_err(|_| ToolProbeError::Incomplete)?;
    if call.name() != TOOL_PROBE_NAME || args != serde_json::json!({}) {
        return Err(ToolProbeError::Incomplete);
    }
    let results = [ToolResult {
        call_id: call.call_id().into(),
        output: challenge.into(),
        is_error: false,
    }];
    let (second, observed) = tool_probe_round(
        state,
        user,
        binding,
        client,
        &ToolRequest {
            text: &text,
            tools: &definitions,
            continuation: Some(&first.continuation),
            results: &results,
        },
        cancellation,
    )
    .await?;
    if second.outcome != FinishOutcome::Completed
        || !second.calls.is_empty()
        || observed.final_text() != challenge
    {
        return Err(ToolProbeError::Incomplete);
    }
    Ok(())
}

/// The unknown 4x4 pattern is present only in pixels, never in the prompt.
pub(super) async fn vision_probe(
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
        .map_err(|_| StoreError::Busy)?;
    let (image, expected) = moosuri_store::media::vision_challenge()?;
    let client = match bound_client(&binding).and_then(|c| c.with_images(vec![image])) {
        Ok(c) => c,
        Err(e) => return Ok(transport_failure(e)),
    };
    let request=TextRequest{model:binding.model.clone(),effort:binding.effort.clone(),max_output_tokens:256,messages:vec![TextMessage{role:TextRole::User,text:"Read the 4 by 4 colored tile image row by row, left to right. Return exactly 16 uppercase letters, one per tile: R=red, G=green, B=blue, Y=yellow. No spaces, punctuation or explanation.".into(),phase:None}]};
    let cancellation = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let stream = client.stream_text(&request, &tx, &cancellation);
    tokio::pin!(stream);
    let mut text = String::new();
    let mut model = None;
    let mut finish = None;
    let mut refusal = false;
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    loop {
        tokio::select! {biased;
            _=tick.tick()=>{state.store.validate_provider_binding(&user,id.clone(),input.revision).await?;},
            event=rx.recv()=>{if let Some(event)=event{match event{ModelEvent::Started{model:m,..}=>model=m,ModelEvent::TextDelta{text:t,..}=>{if text.len()+t.len()>128{return Ok(failure(StatusCode::BAD_GATEWAY,"vision_probe_failed"))}text.push_str(&t)},ModelEvent::Finished{outcome,..}=>finish=Some(outcome),ModelEvent::RefusalDelta{..}=>refusal=true,_=>{}}}},
            result=&mut stream=>{if let Err(e)=result{return Ok(transport_failure(e))}break;}
        }
    }
    // A bounded stream may complete with buffered events; inspect every one.
    while let Ok(event) = rx.try_recv() {
        match event {
            ModelEvent::Started { model: m, .. } => model = m,
            ModelEvent::TextDelta { text: t, .. } => {
                if text.len() + t.len() > 128 {
                    return Ok(failure(StatusCode::BAD_GATEWAY, "vision_probe_failed"));
                }
                text.push_str(&t)
            }
            ModelEvent::Finished { outcome, .. } => finish = Some(outcome),
            ModelEvent::RefusalDelta { .. } => refusal = true,
            _ => {}
        }
    }
    if text.trim() != expected
        || model.as_deref() != Some(binding.model.as_str())
        || finish != Some(FinishOutcome::Completed)
        || refusal
    {
        return Ok(failure(StatusCode::BAD_GATEWAY, "vision_probe_failed"));
    }
    state
        .store
        .record_vision_probe(&user, id, input.revision, input.model, input.effort)
        .await?;
    Ok(Json(serde_json::json!({"vision":true})).into_response())
}
