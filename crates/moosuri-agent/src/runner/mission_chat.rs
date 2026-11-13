use super::memory::PlanningSession;
use super::*;
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
pub(super) async fn run(
    session: &PlanningSession<'_>,
    context: AgentContext,
    text: &str,
) -> Option<AgentContext> {
    let planning = match session.store.mission_chat_planning(session.lease).await {
        Ok(Some(v)) => v,
        Ok(None) => return Some(context),
        Err(e) => {
            failed(session, None, e).await;
            return None;
        }
    };
    if !crate::mission_intent::candidate(text, planning.selected_kind.is_some()) {
        if let Err(error) = session.store.skip_mission_chat(session.lease).await {
            failed(session, None, error).await;
            return None;
        }
        return Some(context);
    }
    let metadata = serde_json::to_string(&planning).ok()?;
    let request = crate::mission_intent::request(
        session.request.model.clone(),
        session.request.effort.clone(),
        text,
        &metadata,
    );
    let definitions = crate::mission_intent::definitions();
    let (_, next, _) = planning_round(session, &context, &request, &definitions).await?;
    let _gate = session.signals.delivery.write().await;
    match session.store.apply_mission_chat(session.lease).await {
        Ok(Some(false)) => Some(next),
        Ok(Some(true)) => {
            session.signals.changed();
            session.signals.wake.notify_one();
            None
        }
        Ok(None) => {
            failed(session, None, StoreError::Conflict).await;
            None
        }
        Err(e) => {
            failed(session, None, e).await;
            None
        }
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
    if encode_tool_request(
        session.protocol,
        &tools,
        session.capabilities,
        &ToolCapabilities {
            function_tools: true,
        },
    )
    .is_err()
    {
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
        session.client,
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
    let Some(decision) = crate::mission_intent::parse(&round) else {
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
    let bytes = serde_json::json!({"kind":"mission_chat_decision","decision":decision})
        .to_string()
        .into_bytes();
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
