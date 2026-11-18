//! Shared transport for isolated current-request planners. Domain parsing and
//! mutation admission stay with each planner and the Store.
use super::*;
use moosuri_connectors::model::protocol::tools::FunctionDefinition;

pub(super) struct PlanningSession<'a> {
    pub store: &'a Store,
    pub lease: &'a RunLease,
    pub client: &'a ExecutionClient,
    pub request: &'a TextRequest,
    pub mail_connection_context: &'a str,
    pub signals: &'a Signals,
    pub protocol: Protocol,
    pub capabilities: &'a TextCapabilities,
    pub subscription: Option<(
        &'a crate::subscriptions::SubscriptionService,
        &'a moosuri_store::SubscriptionRef,
    )>,
}

pub(super) enum Admission {
    LocalIntent,
    MissionChat,
}

impl PlanningSession<'_> {
    pub async fn failed(&self, step: Option<&StepLease>, error: StoreError) {
        let (state, reason) = store_failure(&error);
        self.finish(step, state, reason).await;
    }

    pub async fn finish(&self, step: Option<&StepLease>, state: RunState, reason: RunFailure) {
        finish(
            self.store,
            self.lease,
            step,
            state,
            Some(reason),
            self.signals,
        )
        .await;
    }

    pub async fn dispatch(
        &self,
        context: &AgentContext,
        request: &TextRequest,
        definitions: &[FunctionDefinition],
        admission: Admission,
    ) -> Option<(ToolRound, StepLease)> {
        // Keep the planner's original current user input and domain contract.
        // Add only the runtime observation, never prior conversational claims.
        let mut messages = request
            .messages
            .iter()
            .map(|m| TextMessage {
                role: m.role,
                text: m.text.clone(),
                phase: m.phase,
            })
            .collect::<Vec<_>>();
        if let Some(system) = messages.first_mut().filter(|m| m.role == TextRole::System) {
            system.text.push('\n');
            system.text.push_str(self.mail_connection_context);
            system.text.push_str(" Capability availability does not change this helper's domain classification, exact quote requirements or output schema. Do not execute mail or turn this observation into a mutation.");
        }
        let request = TextRequest {
            model: request.model.clone(),
            effort: request.effort.clone(),
            max_output_tokens: request.max_output_tokens,
            messages,
        };
        let request = &request;
        let tools = ToolRequest {
            text: request,
            tools: definitions,
            continuation: None,
            results: &[],
        };
        let ready = match self
            .client
            .ready_client(self.subscription, self.lease)
            .await
        {
            Ok(client) => client,
            Err(error) => {
                self.failed(None, error).await;
                return None;
            }
        };
        // Settle a dispatched rotating refresh before observing shutdown.
        if self.signals.stopping().is_cancelled() {
            self.failed(None, StoreError::Unavailable).await;
            return None;
        }
        let client = ready
            .as_ref()
            .map(TextExecutor::Http)
            .unwrap_or_else(|| self.client.executor());
        if preflight_tools(client, self.protocol, &tools, self.capabilities).is_err() {
            self.finish(None, RunState::Incomplete, RunFailure::ContextLimit)
                .await;
            return None;
        }
        // This acknowledgment is the dispatch boundary. It is never retried.
        let admitted = match admission {
            Admission::LocalIntent => {
                self.store
                    .begin_agent_step(self.lease, context.checkpoint_seq)
                    .await
            }
            Admission::MissionChat => {
                self.store
                    .begin_mission_chat_step(self.lease, context.checkpoint_seq)
                    .await
            }
        };
        let step = match admitted {
            Ok(step) => step,
            Err(error) => {
                self.failed(None, error).await;
                return None;
            }
        };
        let checkpoint = Checkpoint {
            store: self.store,
            lease: &step,
            signals: self.signals,
            expected_model: &request.model,
        };
        let observed = match stream_round(
            client,
            request,
            Some(&tools),
            &checkpoint,
            self.lease,
            0,
            false,
        )
        .await
        {
            Ok(observed) => observed,
            Err((state, error)) => {
                self.finish(Some(&step), state, error).await;
                return None;
            }
        };
        match observed.round {
            Some(round) => Some((round, step)),
            None => {
                self.finish(Some(&step), RunState::Failed, RunFailure::ProtocolFailed)
                    .await;
                None
            }
        }
    }

    /// Retry only the same local checkpoint after a transient Store failure.
    pub async fn commit(&self, step: &StepLease, bytes: Vec<u8>) -> Option<AgentContext> {
        match commit_round(self.store, step, &bytes, &[]).await {
            Ok(context) => Some(context),
            Err(error) => {
                self.failed(Some(step), error).await;
                None
            }
        }
    }
}
