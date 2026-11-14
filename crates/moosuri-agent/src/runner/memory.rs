//! Isolated current-user intent. No retrieved data or remote planning preview can
//! become local write authority. Only a sealed Store command can mutate memories.
use super::*;
use crate::memory_intent::{self, Decision};
use crate::memory_target;
use moosuri_store::{
    AgentContext, MemoryCommandPreparation, MemoryTargetCandidates, PreparedMemoryCommand,
};

pub(super) struct PlanningSession<'a> {
    pub store: &'a Store,
    pub lease: &'a RunLease,
    pub client: &'a ModelClient,
    pub request: &'a TextRequest,
    pub signals: &'a Signals,
    pub protocol: Protocol,
    pub capabilities: &'a TextCapabilities,
    pub subscription: Option<(
        &'a crate::subscriptions::SubscriptionService,
        &'a moosuri_store::SubscriptionRef,
    )>,
}
impl PlanningSession<'_> {
    /// Refresh only before durable planner admission; never replay inference.
    pub(super) async fn ready_client(&self) -> Result<Option<ModelClient>, StoreError> {
        let client = match self.subscription {
            Some((service, reference)) => Some(
                service
                    .client(
                        moosuri_store::SubscriptionAuthority::Run(self.lease.clone()),
                        reference.clone(),
                    )
                    .await?,
            ),
            None => None,
        };
        // Do not drop a dispatched rotating refresh on shutdown. Complete its
        // durable outcome above, then stop before admitting another inference.
        if self.signals.stopping().is_cancelled() {
            return Err(StoreError::Unavailable);
        }
        Ok(client)
    }
}

async fn failed(session: &PlanningSession<'_>, step: Option<&StepLease>, error: StoreError) {
    let (state, reason) = store_failure(&error);
    finish(
        session.store,
        session.lease,
        step,
        state,
        Some(reason),
        session.signals,
    )
    .await;
}

async fn apply(session: &PlanningSession<'_>, prepared: PreparedMemoryCommand) {
    apply_local(session.store, session.lease, session.signals, prepared).await;
}

async fn apply_local(
    store: &Store,
    lease: &RunLease,
    signals: &Signals,
    prepared: PreparedMemoryCommand,
) {
    // Reserve the same gate as HTTP mutations. No provider or socket work occurs
    // while holding it; Store rechecks authority/CAS at the local commit boundary.
    let gate = tokio::select! {
        biased;
        _ = signals.stopping().cancelled() => {
            finish(store,lease,None,RunState::OutcomeUnknown,
                Some(RunFailure::Interrupted),signals).await;
            return;
        }
        gate = signals.delivery.write() => gate,
    };
    if signals.stopping().is_cancelled() {
        drop(gate);
        finish(
            store,
            lease,
            None,
            RunState::OutcomeUnknown,
            Some(RunFailure::Interrupted),
            signals,
        )
        .await;
        return;
    }
    let result = retry_local(|| store.execute_memory_command(&prepared)).await;
    drop(gate);
    signals.changed();
    if let Err(error) = result {
        let (state, reason) = store_failure(&error);
        finish(store, lease, None, state, Some(reason), signals).await;
    }
}

/// Resume only a typed, sealed local intent. A provider capsule alone supplies
/// no mutation authority and never authorizes another provider request.
pub(super) async fn resume_prepared(store: &Store, lease: &RunLease, signals: &Signals) -> bool {
    match retry_local(|| store.resume_memory_command(lease)).await {
        Ok(Some(prepared)) => apply_local(store, lease, signals, prepared).await,
        Ok(None) => return false,
        Err(error) => {
            let (state, reason) = store_failure(&error);
            finish(store, lease, None, state, Some(reason), signals).await;
        }
    }
    true
}

pub(super) async fn run(
    session: &PlanningSession<'_>,
    context: AgentContext,
    current_text: &str,
) -> Option<AgentContext> {
    // The runner has already resumed any accepted local command before it
    // constructed this planning session. Pending target selection still uses
    // its separate sealed snapshot and existing dispatch authority below.
    match retry_local(|| session.store.resume_memory_targets(session.lease)).await {
        Ok(Some(candidates)) => {
            select_target(session, context, current_text, candidates).await;
            return None;
        }
        Ok(None) => (),
        Err(error) => {
            failed(session, None, error).await;
            return None;
        }
    }
    if !context.memory_planning_allowed || !memory_intent::has_memory_cue(current_text) {
        return Some(context);
    }
    let request = match memory_intent::request(
        session.request.model.clone(),
        session.request.effort.clone(),
        current_text,
    ) {
        Ok(request) => request,
        Err(_) => {
            finish(
                session.store,
                session.lease,
                None,
                RunState::Failed,
                Some(RunFailure::ProtocolFailed),
                session.signals,
            )
            .await;
            return None;
        }
    };
    let definitions = memory_intent::definitions();
    let (round, next, step) = planning_round(session, &context, &request, &definitions).await?;
    let Ok(decision) = memory_intent::parse_completed(current_text, &round) else {
        finish(
            session.store,
            session.lease,
            Some(&step),
            RunState::Failed,
            Some(RunFailure::ProtocolFailed),
            session.signals,
        )
        .await;
        return None;
    };
    match decision {
        // The caller starts a fresh normal request. This classifier capsule and
        // its function call/result are never forwarded into conversation context.
        Decision::NoCommand => Some(next),
        Decision::Clarify(reason) => {
            let output = OutputPatch::Append {
                item: format!("step-{}-memory-clarification", step.step()),
                text: reason.message().into(),
                phase: Some(MessagePhase::FinalAnswer),
                refusal: false,
            };
            match retry_local(|| {
                session
                    .store
                    .append_agent_output(&step, copy_patches(std::slice::from_ref(&output)))
            })
            .await
            {
                Ok(()) => {
                    finish(
                        session.store,
                        session.lease,
                        Some(&step),
                        RunState::Completed,
                        None,
                        session.signals,
                    )
                    .await
                }
                Err(error) => failed(session, Some(&step), error).await,
            }
            None
        }
        Decision::Command(intent) => {
            let preparation = match retry_local(|| {
                session
                    .store
                    .prepare_memory_command_resolution(&step, intent.clone())
            })
            .await
            {
                Ok(prepared) => prepared,
                Err(error) => {
                    failed(session, Some(&step), error).await;
                    return None;
                }
            };
            match preparation {
                MemoryCommandPreparation::Prepared(prepared) => apply(session, prepared).await,
                MemoryCommandPreparation::Resolve(candidates) => {
                    select_target(session, next, current_text, candidates).await
                }
            }
            None
        }
    }
}

async fn select_target(
    session: &PlanningSession<'_>,
    context: AgentContext,
    current_text: &str,
    candidates: MemoryTargetCandidates,
) {
    let request = memory_target::request(
        session.request.model.clone(),
        session.request.effort.clone(),
        current_text,
        &candidates,
    );
    let definitions = memory_target::definitions();
    let Some((round, _next, step)) =
        planning_round(session, &context, &request, &definitions).await
    else {
        return;
    };
    let Some(choice) = memory_target::parse_completed(&round, candidates.candidates().len()) else {
        finish(
            session.store,
            session.lease,
            Some(&step),
            RunState::Failed,
            Some(RunFailure::ProtocolFailed),
            session.signals,
        )
        .await;
        return;
    };
    match retry_local(|| {
        session
            .store
            .select_memory_target(&step, &candidates, choice)
    })
    .await
    {
        Ok(prepared) => apply(session, prepared).await,
        Err(error) => failed(session, Some(&step), error).await,
    }
}

async fn planning_round(
    session: &PlanningSession<'_>,
    context: &AgentContext,
    request: &TextRequest,
    definitions: &[moosuri_connectors::model::protocol::tools::FunctionDefinition],
) -> Option<(ToolRound, AgentContext, StepLease)> {
    let tools = ToolRequest {
        text: request,
        tools: definitions,
        continuation: None,
        results: &[],
    };
    let ready = match session.ready_client().await {
        Ok(client) => client,
        Err(error) => {
            failed(session, None, error).await;
            return None;
        }
    };
    let client = ready.as_ref().unwrap_or(session.client);
    if preflight_tools(client, session.protocol, &tools, session.capabilities).is_err() {
        finish(
            session.store,
            session.lease,
            None,
            RunState::Incomplete,
            Some(RunFailure::ContextLimit),
            session.signals,
        )
        .await;
        return None;
    }
    // No retry after an ambiguous dispatch acknowledgement.
    let step = match session
        .store
        .begin_agent_step(session.lease, context.checkpoint_seq)
        .await
    {
        Ok(step) => step,
        Err(error) => {
            failed(session, None, error).await;
            return None;
        }
    };
    let checkpoint = Checkpoint {
        store: session.store,
        lease: &step,
        signals: session.signals,
        expected_model: &request.model,
    };
    let observed = match stream_round(
        client,
        request,
        Some(&tools),
        &checkpoint,
        session.lease,
        0,
        false,
    )
    .await
    {
        Ok(observed) => observed,
        Err((state, error)) => {
            finish(
                session.store,
                session.lease,
                Some(&step),
                state,
                Some(error),
                session.signals,
            )
            .await;
            return None;
        }
    };
    let Some(round) = observed.round else {
        finish(
            session.store,
            session.lease,
            Some(&step),
            RunState::Failed,
            Some(RunFailure::ProtocolFailed),
            session.signals,
        )
        .await;
        return None;
    };
    let Ok(bytes) = round.continuation.to_private_checkpoint() else {
        finish(
            session.store,
            session.lease,
            Some(&step),
            RunState::Failed,
            Some(RunFailure::ProtocolFailed),
            session.signals,
        )
        .await;
        return None;
    };
    let next = match retry_local(|| {
        session.store.commit_agent_step(
            &step,
            PrivateCheckpoint {
                bytes: bytes.clone(),
            },
            vec![],
        )
    })
    .await
    {
        Ok(next) => next,
        Err(error) => {
            failed(session, Some(&step), error).await;
            return None;
        }
    };
    Some((round, next, step))
}
