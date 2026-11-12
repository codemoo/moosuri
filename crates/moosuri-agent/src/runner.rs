use crate::knowledge_tools::{self, Invocation};
use crate::{Signals, model_client};
use moosuri_connectors::model::protocol::tools::{
    Continuation, ToolCapabilities, ToolRequest, ToolResult, ToolRound, encode_tool_request,
};
use moosuri_connectors::model::{
    CancellationToken, ModelClient, Secret, TransportError,
    protocol::{
        AssistantPhase, FinishOutcome, ModelEvent, Protocol, ProtocolError, TextCapabilities,
        TextMessage, TextRequest, TextRole, Usage, encode_request,
    },
};
use moosuri_core::{MessagePhase, ProviderKind, RunState, RunUsage};
use moosuri_store::{
    AgentClaim, ContextMessage, ContextRole, KnowledgeToolInput, KnowledgeToolResult, OutputPatch,
    PendingKnowledgeCall, PrivateCheckpoint, RunFailure, RunLease, StepLease, Store, StoreError,
};
use std::{collections::BTreeMap, time::Duration};

mod memory;

const MAX_OUTPUT: usize = 256 * 1024;
const MAX_PATCH: usize = 8 * 1024;
const MAX_ITEMS: usize = 128;
const SYSTEM: &str = "You are moosuri, a helpful assistant. Answer clearly in the user's language. Tools, external actions, persistent memory and attachments are unavailable; never claim to have used them. 무수리는 사용자의 언어로 정확하게 답하며, 지원하지 않는 작업을 수행했다고 주장하지 않습니다.";

fn phase(value: Option<AssistantPhase>) -> Option<MessagePhase> {
    value.map(|v| match v {
        AssistantPhase::Commentary => MessagePhase::Commentary,
        AssistantPhase::FinalAnswer => MessagePhase::FinalAnswer,
    })
}
fn wire_phase(value: Option<MessagePhase>) -> Option<AssistantPhase> {
    value.map(|v| match v {
        MessagePhase::Commentary => AssistantPhase::Commentary,
        MessagePhase::FinalAnswer => AssistantPhase::FinalAnswer,
    })
}
fn protocol(kind: ProviderKind) -> Protocol {
    match kind {
        ProviderKind::OpenAi => Protocol::Responses,
        ProviderKind::Anthropic => Protocol::AnthropicMessages,
        ProviderKind::OpenRouter => Protocol::OpenRouterChat,
        ProviderKind::Compatible => Protocol::ChatCompletions,
    }
}
/// Wire-only projection. Canonical history remains untouched in the store.
fn project(messages: Vec<ContextMessage>, responses: bool) -> Vec<TextMessage> {
    let mut result = vec![TextMessage {
        role: TextRole::System,
        text: SYSTEM.into(),
        phase: None,
    }];
    for message in messages {
        if !responses
            && matches!(message.role, ContextRole::Assistant)
            && message.phase == Some(MessagePhase::Commentary)
        {
            continue;
        }
        result.push(TextMessage {
            role: match message.role {
                ContextRole::System => TextRole::System,
                ContextRole::User => TextRole::User,
                ContextRole::Assistant => TextRole::Assistant,
            },
            text: message.text,
            phase: if responses {
                wire_phase(message.phase)
            } else {
                None
            },
        });
    }
    result
}
fn usage(value: Usage) -> RunUsage {
    RunUsage {
        input_tokens: value.input_tokens,
        output_tokens: value.output_tokens,
        cache_read_tokens: value.cache_read_tokens,
        cache_write_tokens: value.cache_write_tokens,
    }
}
struct Item {
    key: String,
    phase: Option<MessagePhase>,
    emitted: bool,
}
#[derive(Default)]
struct Pending {
    prefix: String,
    patches: Vec<OutputPatch>,
    bytes: usize,
    total_bytes: usize,
    items: BTreeMap<(String, bool), Item>,
    phases: BTreeMap<String, Option<MessagePhase>>,
    usage: Option<RunUsage>,
    terminal: Option<FinishOutcome>,
    identity: bool,
    has_text: bool,
}
impl Pending {
    fn item(&mut self, key: String, refusal: bool) -> Result<&mut Item, RunFailure> {
        if !self.items.contains_key(&(key.clone(), refusal)) && self.items.len() >= MAX_ITEMS {
            return Err(RunFailure::OutputLimit);
        }
        let next = self.items.len();
        let phase = self.phases.get(&key).copied().flatten();
        Ok(self.items.entry((key, refusal)).or_insert_with(|| Item {
            key: format!("{}assistant-{next}", self.prefix),
            phase,
            emitted: false,
        }))
    }
    fn metadata(&mut self, key: String, next: Option<MessagePhase>) -> Result<(), RunFailure> {
        if !self.phases.contains_key(&key) && self.phases.len() >= MAX_ITEMS {
            return Err(RunFailure::OutputLimit);
        }
        self.phases.insert(key.clone(), next);
        for refusal in [false, true] {
            if let Some(item) = self.items.get_mut(&(key.clone(), refusal))
                && item.phase != next
            {
                item.phase = next;
                if item.emitted {
                    self.patches.push(OutputPatch::Phase {
                        item: item.key.clone(),
                        phase: next,
                    });
                }
            }
        }
        Ok(())
    }
}
fn fenced(error: &StoreError) -> bool {
    matches!(
        error,
        StoreError::Unauthorized
            | StoreError::Forbidden
            | StoreError::NotFound
            | StoreError::Conflict
    )
}
struct Checkpoint<'a> {
    store: &'a Store,
    lease: &'a StepLease,
    signals: &'a Signals,
    expected_model: &'a str,
}
fn copy_patches(patches: &[OutputPatch]) -> Vec<OutputPatch> {
    patches
        .iter()
        .map(|patch| match patch {
            OutputPatch::Append {
                item,
                text,
                phase,
                refusal,
            } => OutputPatch::Append {
                item: item.clone(),
                text: text.clone(),
                phase: *phase,
                refusal: *refusal,
            },
            OutputPatch::Phase { item, phase } => OutputPatch::Phase {
                item: item.clone(),
                phase: *phase,
            },
        })
        .collect()
}
async fn retry_authority<F, Fut>(mut check: F) -> Result<(), StoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<(), StoreError>>,
{
    for retry in 0..5 {
        match check().await {
            Err(StoreError::Busy | StoreError::Unavailable) if retry < 4 => {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            result => return result,
        }
    }
    unreachable!()
}
async fn write_usage(checkpoint: &Checkpoint<'_>, usage: &RunUsage) -> Result<(), StoreError> {
    for retry in 0..5 {
        match checkpoint
            .store
            .update_agent_step_usage(checkpoint.lease, usage.clone())
            .await
        {
            Err(StoreError::Busy) if retry < 4 => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            result => return result,
        }
    }
    unreachable!()
}
async fn write_output(
    checkpoint: &Checkpoint<'_>,
    patches: &[OutputPatch],
) -> Result<(), StoreError> {
    for retry in 0..5 {
        match checkpoint
            .store
            .append_agent_output(checkpoint.lease, copy_patches(patches))
            .await
        {
            Err(StoreError::Busy) if retry < 4 => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            result => return result,
        }
    }
    unreachable!()
}
async fn flush(checkpoint: &Checkpoint<'_>, pending: &mut Pending) -> Result<(), RunFailure> {
    let signals = checkpoint.signals;
    // Accounting remains useful after cancellation/deletion. Never make private
    // output eligibility a prerequisite for recording an observed usage receipt.
    if let Some(usage) = pending.usage.take() {
        if write_usage(checkpoint, &usage).await.is_err() {
            pending.usage = Some(usage);
            return Err(RunFailure::ConnectionFailed);
        }
        signals.changed();
    }
    if !pending.patches.is_empty() {
        let patches = std::mem::take(&mut pending.patches);
        pending.bytes = 0;
        let result = write_output(checkpoint, &patches).await;
        signals.changed();
        if let Err(error) = result {
            if !fenced(&error) && error != StoreError::BudgetExhausted {
                pending.patches = patches;
                pending.bytes = pending
                    .patches
                    .iter()
                    .map(|patch| match patch {
                        OutputPatch::Append { text, .. } => text.len(),
                        _ => 0,
                    })
                    .sum();
            }
            return Err(store_failure(&error).1);
        }
    }
    Ok(())
}
async fn event(
    value: ModelEvent,
    allow_output: bool,
    checkpoint: &Checkpoint<'_>,
    pending: &mut Pending,
) -> Result<(), RunFailure> {
    match value {
        ModelEvent::Started { model, .. } => {
            if model.as_deref() != Some(checkpoint.expected_model) {
                return Err(RunFailure::IdentityUnverified);
            }
            pending.identity = true;
        }
        ModelEvent::UsageSnapshot(value) => {
            pending.usage = Some(usage(value));
        }
        ModelEvent::Finished { outcome, .. } => {
            pending.terminal = Some(outcome);
        }
        ModelEvent::MessageMetadata {
            item_id,
            phase: next,
        } if allow_output && pending.identity => {
            pending.metadata(item_id, phase(next))?;
        }
        _ => (),
    }
    if pending.patches.len() >= 16 || pending.bytes >= MAX_PATCH {
        flush(checkpoint, pending).await?;
    }
    Ok(())
}
async fn append_event(
    text: String,
    item_id: Option<String>,
    next: Option<AssistantPhase>,
    refusal: bool,
    checkpoint: &Checkpoint<'_>,
    pending: &mut Pending,
) -> Result<(), RunFailure> {
    if text.is_empty() {
        return Ok(());
    }
    let key = item_id.unwrap_or_else(|| "unidentified-message".into());
    if pending.patches.len() >= 14 {
        flush(checkpoint, pending).await?;
    }
    if let Some(next) = phase(next) {
        pending.metadata(key.clone(), Some(next))?;
    }
    let mut remaining = text.as_str();
    while !remaining.is_empty() {
        let mut length = remaining.len().min(MAX_PATCH);
        while !remaining.is_char_boundary(length) {
            length -= 1;
        }
        let piece = &remaining[..length];
        if pending.total_bytes.saturating_add(length) > MAX_OUTPUT {
            return Err(RunFailure::OutputLimit);
        }
        if pending.patches.len() >= 16 || pending.bytes.saturating_add(length) > MAX_PATCH {
            flush(checkpoint, pending).await?;
        }
        let item = pending.item(key.clone(), refusal)?;
        item.emitted = true;
        let phase = item.phase;
        let item_key = item.key.clone();
        pending.patches.push(OutputPatch::Append {
            item: item_key,
            text: piece.into(),
            phase,
            refusal,
        });
        pending.bytes += length;
        pending.total_bytes += length;
        pending.has_text |= !refusal && !piece.trim().is_empty();
        remaining = &remaining[length..];
        if pending.bytes >= MAX_PATCH {
            flush(checkpoint, pending).await?;
        }
    }
    Ok(())
}
async fn observe(
    value: ModelEvent,
    allow_output: bool,
    checkpoint: &Checkpoint<'_>,
    pending: &mut Pending,
) -> Result<(), RunFailure> {
    if allow_output && pending.patches.len() >= 14 {
        flush(checkpoint, pending).await?;
    }
    match value {
        ModelEvent::TextDelta {
            text,
            item_id,
            phase,
        } if allow_output && pending.identity => {
            append_event(text, item_id, phase, false, checkpoint, pending).await
        }
        ModelEvent::RefusalDelta {
            text,
            item_id,
            phase,
        } if allow_output && pending.identity => {
            append_event(text, item_id, phase, true, checkpoint, pending).await
        }
        value => event(value, allow_output, checkpoint, pending).await,
    }
}
fn transport_failure(error: TransportError) -> (RunState, RunFailure) {
    use RunFailure::*;
    match error {
        TransportError::Http {
            status: 401 | 403, ..
        } => (RunState::Failed, CredentialsRejected),
        TransportError::Http { status: 429, .. } => (RunState::Failed, RateLimited),
        TransportError::Http { .. } => (RunState::Failed, ProviderUnavailable),
        TransportError::Timeout => (RunState::OutcomeUnknown, TimedOut),
        TransportError::Cancelled => (RunState::OutcomeUnknown, Interrupted),
        TransportError::LimitExceeded | TransportError::Protocol(ProtocolError::LimitExceeded) => {
            (RunState::Incomplete, OutputLimit)
        }
        TransportError::Protocol(ProtocolError::Truncated) => {
            (RunState::OutcomeUnknown, ProtocolFailed)
        }
        TransportError::Protocol(ProtocolError::UnsupportedCapability) => {
            (RunState::Incomplete, ContinuationUnsupported)
        }
        TransportError::Protocol(_) | TransportError::ContentType => {
            (RunState::Failed, ProtocolFailed)
        }
        TransportError::InvalidConfiguration | TransportError::DestinationDenied => {
            (RunState::Failed, ProviderUnavailable)
        }
        _ => (RunState::OutcomeUnknown, ConnectionFailed),
    }
}
fn local_failure(failure: RunFailure) -> RunState {
    match failure {
        RunFailure::OutputLimit
        | RunFailure::ContinuationUnsupported
        | RunFailure::BudgetExhausted => RunState::Incomplete,
        RunFailure::Interrupted | RunFailure::ConnectionFailed => RunState::OutcomeUnknown,
        _ => RunState::Failed,
    }
}
async fn finish(
    store: &Store,
    lease: &RunLease,
    step: Option<&StepLease>,
    state: RunState,
    error: Option<RunFailure>,
    signals: &Signals,
) {
    for retry in 0..5 {
        let result = match step {
            Some(step) => store.finish_agent_run(step, state, error).await,
            None => store.finish_agent_execution(lease, state, error).await,
        };
        signals.changed();
        if matches!(result, Err(StoreError::Busy)) && retry < 4 {
            tokio::time::sleep(Duration::from_millis(20)).await;
        } else {
            break;
        }
    }
}
async fn retry_local<T, F, Fut>(mut operation: F) -> Result<T, StoreError>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, StoreError>>,
{
    for retry in 0..5 {
        match operation().await {
            Err(StoreError::Busy) if retry < 4 => {
                tokio::time::sleep(Duration::from_millis(20)).await
            }
            result => return result,
        }
    }
    unreachable!()
}
fn typed_input(
    call: &moosuri_connectors::model::protocol::tools::CompletedToolCall,
) -> KnowledgeToolInput {
    let input = match knowledge_tools::parse(call.name(), call.arguments()) {
        Invocation::Search(query) => KnowledgeToolInput::Search { query },
        Invocation::ReadSource(v) => KnowledgeToolInput::ReadSource {
            id: v.id,
            revision: v.revision,
            start_byte: v.start_byte,
            max_bytes: v.max_bytes,
        },
        Invocation::ReadMemory(v) => KnowledgeToolInput::ReadMemory {
            id: v.id,
            revision: v.revision,
        },
        Invocation::SkillRead(pin) => KnowledgeToolInput::SkillRead { pin },
        Invocation::DecimalCalculate(calculation) => {
            KnowledgeToolInput::DecimalCalculate { calculation }
        }
        Invocation::External(request) => KnowledgeToolInput::External { request },
        Invocation::InvalidArguments => KnowledgeToolInput::InvalidArguments,
    };
    // Even well-formed JSON can exceed the private slot bound. Record a small
    // repairable error slot instead of failing the entire completed model round.
    if serde_json::to_vec(&input).is_ok_and(|v| v.len() <= moosuri_store::TOOL_INPUT_LIMIT) {
        input
    } else {
        KnowledgeToolInput::InvalidArguments
    }
}
fn store_failure(error: &StoreError) -> (RunState, RunFailure) {
    if *error == StoreError::BudgetExhausted {
        return (RunState::Incomplete, RunFailure::BudgetExhausted);
    }
    let reason = if fenced(error) {
        RunFailure::AuthorityRevoked
    } else if matches!(error, StoreError::InvalidInput) {
        RunFailure::ProtocolFailed
    } else {
        RunFailure::ConnectionFailed
    };
    (local_failure(reason), reason)
}
struct Observation {
    round: Option<ToolRound>,
    pending: Pending,
}
async fn stream_round(
    client: &ModelClient,
    request: &TextRequest,
    tools: Option<&ToolRequest<'_>>,
    checkpoint: &Checkpoint<'_>,
    run: &RunLease,
    prior_bytes: usize,
    visible_output: bool,
) -> Result<Observation, (RunState, RunFailure)> {
    let cancellation = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::channel(16);
    let capability = ToolCapabilities {
        function_tools: true,
    };
    let operation = async {
        match tools {
            Some(tools) => client
                .stream_tool_round(tools, &capability, &tx, &cancellation)
                .await
                .map(Some),
            None => client
                .stream_text(request, &tx, &cancellation)
                .await
                .map(|()| None),
        }
    };
    tokio::pin!(operation);
    let mut authority = tokio::time::interval(Duration::from_millis(250));
    let mut batch = tokio::time::interval(Duration::from_millis(32));
    authority.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    batch.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut pending = Pending {
        prefix: format!("step-{}-", checkpoint.lease.step()),
        total_bytes: prior_bytes,
        ..Pending::default()
    };
    let mut failure = None;
    let mut authority_fenced = false;
    let result = loop {
        tokio::select! {
            biased;
            _ = checkpoint.signals.stopping().cancelled(), if failure.is_none() => {
                failure=Some(RunFailure::Interrupted); cancellation.cancel();
            }
            _ = authority.tick(), if !authority_fenced => {
                if let Err(error) = retry_authority(|| checkpoint.store.check_run(run)).await {
                    checkpoint.signals.changed();
                    authority_fenced=fenced(&error) || error == StoreError::BudgetExhausted;
                    if authority_fenced { pending.patches.clear();pending.bytes=0; }
                    failure=Some(store_failure(&error).1);
                    cancellation.cancel();
                }
            }
            _ = batch.tick(), if !pending.patches.is_empty() || pending.usage.is_some() => {
                if let Err(error)=flush(checkpoint,&mut pending).await {
                    authority_fenced=matches!(error,RunFailure::AuthorityRevoked|RunFailure::BudgetExhausted);
                    failure=Some(error);cancellation.cancel();
                }
            }
            Some(value) = rx.recv() => {
                if let Err(error)=observe(value,failure.is_none() && visible_output,checkpoint,&mut pending).await {
                    authority_fenced=matches!(error,RunFailure::AuthorityRevoked|RunFailure::BudgetExhausted);
                    failure=Some(error);cancellation.cancel();
                }
            }
            result = &mut operation => break result,
        }
    };
    while let Ok(value) = rx.try_recv() {
        if let Err(error) = observe(
            value,
            failure.is_none() && visible_output,
            checkpoint,
            &mut pending,
        )
        .await
        {
            authority_fenced = matches!(
                error,
                RunFailure::AuthorityRevoked | RunFailure::BudgetExhausted
            );
            failure = Some(error);
        }
    }
    if let Ok(Some(round)) = &result {
        pending.usage = Some(usage(round.usage));
    }
    if authority_fenced {
        pending.patches.clear();
        pending.bytes = 0;
    }
    if let Err(error) = flush(checkpoint, &mut pending).await {
        failure = Some(error);
    }
    if let Some(error) = failure {
        return Err((local_failure(error), error));
    }
    let round = result.map_err(transport_failure)?;
    if !pending.identity {
        return Err((RunState::Failed, RunFailure::IdentityUnverified));
    }
    Ok(Observation { round, pending })
}

pub(crate) async fn execute(
    store: Store,
    claimed: AgentClaim,
    signals: Signals,
    mail: crate::mail::MailService,
    web: crate::web::Service,
) {
    let AgentClaim {
        lease,
        mut binding,
        model,
        messages,
        mut context,
        tool_profile,
    } = claimed;
    if signals.stopping().is_cancelled() {
        finish(
            &store,
            &lease,
            None,
            RunState::OutcomeUnknown,
            Some(RunFailure::Interrupted),
            &signals,
        )
        .await;
        return;
    }
    // Explicit retry starts a new attempt. Unknown remote work is never resumed
    // merely because a private checkpoint exists in a previous attempt.
    if context.checkpoint.is_some() || !context.calls.is_empty() {
        finish(
            &store,
            &lease,
            None,
            RunState::Failed,
            Some(RunFailure::ProtocolFailed),
            &signals,
        )
        .await;
        return;
    }
    let protocol = protocol(binding.config.kind);
    let capabilities = TextCapabilities {
        efforts: binding.effort.iter().cloned().collect(),
    };
    let mut messages = project(messages, protocol == Protocol::Responses);
    if model.function_tools {
        messages[0].text = knowledge_tools::system(tool_profile.as_ref());
    }
    let request = TextRequest {
        model: binding.model.clone(),
        effort: binding.effort.clone(),
        messages,
        max_output_tokens: 4096,
    };
    let credential = binding
        .credential
        .take()
        .map(|v| Secret::new(v.to_string()))
        .transpose();
    let client = match credential.and_then(|credential| {
        model_client(
            &binding.config,
            credential,
            TextCapabilities {
                efforts: capabilities.efforts.clone(),
            },
        )
    }) {
        Ok(client) => client,
        Err(error) => {
            let (state, error) = transport_failure(error);
            finish(&store, &lease, None, state, Some(error), &signals).await;
            return;
        }
    };
    if model.function_tools {
        // The final user message is the original current turn. No prior messages
        // or source results are included in the separate intent request.
        let Some(current_text) = request
            .messages
            .last()
            .filter(|m| m.role == TextRole::User)
            .map(|m| m.text.as_str())
        else {
            finish(
                &store,
                &lease,
                None,
                RunState::Failed,
                Some(RunFailure::ProtocolFailed),
                &signals,
            )
            .await;
            return;
        };
        let planning = memory::PlanningSession {
            store: &store,
            lease: &lease,
            client: &client,
            request: &request,
            signals: &signals,
            protocol,
            capabilities: &capabilities,
        };
        let Some(next) = memory::run(&planning, context, current_text).await else {
            return;
        };
        context = next;
    }
    let definitions = knowledge_tools::definitions(tool_profile.as_ref());
    let tool_capability = ToolCapabilities {
        function_tools: true,
    };
    let mut continuation: Option<Continuation> = None;
    let mut results = Vec::<ToolResult>::new();
    let mut total_bytes = 0;
    for _ in 0..6 {
        let tool_request = ToolRequest {
            text: &request,
            tools: &definitions,
            continuation: continuation.as_ref(),
            results: &results,
        };
        let valid = if model.function_tools {
            encode_tool_request(protocol, &tool_request, &capabilities, &tool_capability)
        } else {
            encode_request(protocol, &request, &capabilities)
        };
        if valid.is_err() {
            finish(
                &store,
                &lease,
                None,
                RunState::Incomplete,
                Some(RunFailure::ContextLimit),
                &signals,
            )
            .await;
            return;
        }
        // Dispatch admission is the actor transaction's linearization point. A
        // failed/unknown acknowledgement never authorizes a provider request.
        let step = match store.begin_agent_step(&lease, context.checkpoint_seq).await {
            Ok(step) => step,
            Err(error) => {
                let (state, error) = store_failure(&error);
                finish(&store, &lease, None, state, Some(error), &signals).await;
                return;
            }
        };
        let checkpoint = Checkpoint {
            store: &store,
            lease: &step,
            signals: &signals,
            expected_model: &request.model,
        };
        let observed = match stream_round(
            &client,
            &request,
            model.function_tools.then_some(&tool_request),
            &checkpoint,
            &lease,
            total_bytes,
            true,
        )
        .await
        {
            Ok(value) => value,
            Err((state, error)) => {
                finish(&store, &lease, Some(&step), state, Some(error), &signals).await;
                return;
            }
        };
        total_bytes = observed.pending.total_bytes;
        let (outcome, calls, bytes) = match &observed.round {
            Some(round) => {
                let bytes = match round.continuation.to_private_checkpoint() {
                    Ok(bytes) => bytes,
                    Err(_) => {
                        finish(
                            &store,
                            &lease,
                            Some(&step),
                            RunState::Failed,
                            Some(RunFailure::ProtocolFailed),
                            &signals,
                        )
                        .await;
                        return;
                    }
                };
                let calls = round
                    .calls
                    .iter()
                    .map(|call| PendingKnowledgeCall {
                        call_id: call.call_id().into(),
                        input: typed_input(call),
                    })
                    .collect::<Vec<_>>();
                (round.outcome, calls, bytes)
            }
            None => match observed.pending.terminal {
                Some(outcome) => (
                    outcome,
                    vec![],
                    br#"{"v":1,"kind":"text_terminal"}"#.to_vec(),
                ),
                None => {
                    finish(
                        &store,
                        &lease,
                        Some(&step),
                        RunState::Failed,
                        Some(RunFailure::ProtocolFailed),
                        &signals,
                    )
                    .await;
                    return;
                }
            },
        };
        context = match retry_local(|| {
            store.commit_agent_step(
                &step,
                PrivateCheckpoint {
                    bytes: bytes.clone(),
                },
                calls.clone(),
            )
        })
        .await
        {
            Ok(value) => value,
            Err(error) => {
                let (state, error) = store_failure(&error);
                finish(&store, &lease, Some(&step), state, Some(error), &signals).await;
                return;
            }
        };
        signals.changed();
        if outcome != FinishOutcome::ContinuationRequired || calls.is_empty() {
            let (state, error) = match outcome {
                FinishOutcome::Completed if observed.pending.has_text && calls.is_empty() => {
                    (RunState::Completed, None)
                }
                FinishOutcome::Incomplete => (RunState::Incomplete, Some(RunFailure::OutputLimit)),
                FinishOutcome::Refused => (RunState::Refused, None),
                _ => (RunState::Failed, Some(RunFailure::ProtocolFailed)),
            };
            finish(&store, &lease, Some(&step), state, error, &signals).await;
            return;
        }
        let Some(saved) = context.checkpoint.as_ref() else {
            finish(
                &store,
                &lease,
                Some(&step),
                RunState::Failed,
                Some(RunFailure::ProtocolFailed),
                &signals,
            )
            .await;
            return;
        };
        continuation = match Continuation::from_private_checkpoint(
            &saved.bytes,
            protocol,
            &request.model,
            request.effort.as_deref(),
        ) {
            Ok(value) => Some(value),
            Err(_) => {
                finish(
                    &store,
                    &lease,
                    Some(&step),
                    RunState::Failed,
                    Some(RunFailure::ProtocolFailed),
                    &signals,
                )
                .await;
                return;
            }
        };
        results.clear();
        for call in &calls {
            let value =
                match execute_tool(&store, &step, call, &mail, &web, signals.stopping()).await {
                    Ok(value) => value,
                    Err(error) => {
                        let (state, error) = store_failure(&error);
                        finish(&store, &lease, Some(&step), state, Some(error), &signals).await;
                        return;
                    }
                };
            let output = match serde_json::to_string(&value) {
                Ok(output) => output,
                Err(_) => {
                    finish(
                        &store,
                        &lease,
                        Some(&step),
                        RunState::Failed,
                        Some(RunFailure::ProtocolFailed),
                        &signals,
                    )
                    .await;
                    return;
                }
            };
            results.push(ToolResult {
                call_id: call.call_id.clone(),
                output,
                is_error: matches!(value, KnowledgeToolResult::Error(_)),
            });
        }
        context = match retry_local(|| store.agent_context(&lease)).await {
            Ok(value) => value,
            Err(error) => {
                let (state, error) = store_failure(&error);
                finish(&store, &lease, Some(&step), state, Some(error), &signals).await;
                return;
            }
        };
    }
    finish(
        &store,
        &lease,
        None,
        RunState::Incomplete,
        Some(RunFailure::BudgetExhausted),
        &signals,
    )
    .await;
}

async fn execute_tool(
    store: &Store,
    step: &StepLease,
    call: &PendingKnowledgeCall,
    mail: &crate::mail::MailService,
    web: &crate::web::Service,
    cancel: &CancellationToken,
) -> Result<KnowledgeToolResult, StoreError> {
    let external = matches!(&call.input,KnowledgeToolInput::External{request} if !matches!(request,moosuri_core::ExternalToolRequest::Mail(moosuri_core::MailToolRequest::Draft{..})));
    if !external {
        return retry_local(|| store.execute_knowledge_tool(step, call.call_id.clone())).await;
    }
    match store
        .prepare_external_tool(step, call.call_id.clone())
        .await?
    {
        moosuri_store::ExternalToolStart::Complete(v) => Ok(v),
        moosuri_store::ExternalToolStart::Dispatch(permit) => {
            let io_cancel = cancel.child_token();
            let operation = async {
                match permit.request() {
                    moosuri_core::ExternalToolRequest::Mail(_) => {
                        mail.external(&permit, &io_cancel).await
                    }
                    moosuri_core::ExternalToolRequest::Web(request) => {
                        web.execute(
                            request,
                            &permit.user().account().id,
                            permit.browser_pin(),
                            &io_cancel,
                        )
                        .await
                    }
                }
            };
            tokio::pin!(operation);
            let mut tick = tokio::time::interval(Duration::from_millis(250));
            tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            let outcome = loop {
                tokio::select! {biased;
                    _=cancel.cancelled()=>break Err(StoreError::Unavailable),
                    _=tick.tick()=>if let Err(error)=store.check_external_tool(&permit).await {break Err(error)},
                    result=&mut operation=>break Ok(result)
                }
            };
            let result = match outcome {
                Ok(result) => result,
                Err(error) => {
                    io_cancel.cancel();
                    if matches!(
                        permit.request(),
                        moosuri_core::ExternalToolRequest::Web(
                            moosuri_core::WebToolRequest::BrowserRead { .. }
                        )
                    ) {
                        // The capsule owns bounded cleanup independently; await it before
                        // returning on revoked authority, even with stalled stdout.
                        let _ = (&mut operation).await;
                    } else {
                        let _ = tokio::time::timeout(Duration::from_secs(4), &mut operation).await;
                    }
                    return Err(error);
                }
            };
            retry_local(|| store.complete_external_tool(&permit, result.clone())).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn transient_store_pressure_recovers_without_revoking_authority() {
        let mut attempts = 0;
        let result = retry_authority(|| {
            attempts += 1;
            std::future::ready(if attempts < 3 {
                Err(StoreError::Busy)
            } else {
                Ok(())
            })
        })
        .await;
        assert!(result.is_ok());
        assert_eq!(attempts, 3);
        attempts = 0;
        let result = retry_authority(|| {
            attempts += 1;
            std::future::ready(Err(StoreError::Unavailable))
        })
        .await;
        assert!(matches!(result, Err(StoreError::Unavailable)));
        assert!(!fenced(&result.unwrap_err()));
        assert_eq!(attempts, 5);
        attempts = 0;
        let result = retry_authority(|| {
            attempts += 1;
            std::future::ready(Err(StoreError::Unauthorized))
        })
        .await;
        assert!(fenced(&result.unwrap_err()));
        assert_eq!(attempts, 1);
    }
    fn history() -> Vec<ContextMessage> {
        vec![
            ContextMessage {
                role: ContextRole::System,
                text: "Synthetic omission note".into(),
                phase: None,
            },
            ContextMessage {
                role: ContextRole::User,
                text: "Prior question".into(),
                phase: None,
            },
            ContextMessage {
                role: ContextRole::Assistant,
                text: "Commentary".into(),
                phase: Some(MessagePhase::Commentary),
            },
            ContextMessage {
                role: ContextRole::Assistant,
                text: "Final answer".into(),
                phase: Some(MessagePhase::FinalAnswer),
            },
            ContextMessage {
                role: ContextRole::Assistant,
                text: "Unknown phase".into(),
                phase: None,
            },
            ContextMessage {
                role: ContextRole::User,
                text: "New question".into(),
                phase: None,
            },
        ]
    }
    #[test]
    fn wire_projection_preserves_responses_phase_and_omits_other_protocol_commentary() {
        let responses = project(history(), true);
        assert_eq!(responses.len(), 7);
        assert_eq!(responses[3].phase, Some(AssistantPhase::Commentary));
        assert_eq!(responses[4].phase, Some(AssistantPhase::FinalAnswer));
        let compatible = project(history(), false);
        assert_eq!(compatible.len(), 6);
        assert!(compatible.iter().all(|message| message.phase.is_none()));
        assert!(
            !compatible
                .iter()
                .any(|message| message.text == "Commentary")
        );
        assert!(
            compatible
                .iter()
                .any(|message| message.text == "Final answer")
        );
        assert!(
            compatible
                .iter()
                .any(|message| message.text == "Unknown phase")
        );
    }
    #[test]
    fn late_phase_updates_both_answer_and_refusal_without_mixing_content() {
        let mut pending = Pending::default();
        for refusal in [false, true] {
            pending
                .item("source-item".into(), refusal)
                .unwrap_or_else(|_| panic!("synthetic item rejected"))
                .emitted = true;
        }
        assert!(
            pending
                .metadata("source-item".into(), Some(MessagePhase::FinalAnswer))
                .is_ok()
        );
        assert_eq!(pending.patches.len(), 2);
        let keys: Vec<_> = pending
            .patches
            .iter()
            .map(|patch| match patch {
                OutputPatch::Phase { item, phase } => {
                    assert_eq!(*phase, Some(MessagePhase::FinalAnswer));
                    item.clone()
                }
                _ => panic!("unexpected synthetic patch"),
            })
            .collect();
        assert_ne!(keys[0], keys[1]);
    }
}
