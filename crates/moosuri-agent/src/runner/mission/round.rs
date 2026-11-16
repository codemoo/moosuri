//! The same guarded completion path consumes fresh and recovered mission rounds.
use super::*;
use moosuri_store::{AgentToolSlot, MissionPendingRound, StepLease};

pub(super) enum RoundOutcome {
    Continue,
    Stop,
}

pub(super) struct RoundHandler<'a> {
    pub store: &'a Store,
    pub lease: &'a RunLease,
    pub signals: &'a Signals,
    pub mail: &'a crate::mail::MailService,
    pub web: &'a crate::web::Service,
}
impl RoundHandler<'_> {
    // Acceptance revisions, cancellation and owner/epoch withdrawal deliberately
    // fence old work. Never replace those outcomes with a new terminal mutation.
    async fn local<T>(&self, step: &StepLease, result: Result<T, StoreError>) -> Option<T> {
        match result {
            Ok(value) => Some(value),
            Err(error) if fenced(&error) => None,
            Err(error) => {
                let (state, error) = store_failure(&error);
                finish(
                    self.store,
                    self.lease,
                    Some(step),
                    state,
                    Some(error),
                    self.signals,
                )
                .await;
                None
            }
        }
    }
    async fn protocol(&self, step: &StepLease, state: RunState) -> RoundOutcome {
        finish(
            self.store,
            self.lease,
            Some(step),
            state,
            Some(RunFailure::ProtocolFailed),
            self.signals,
        )
        .await;
        RoundOutcome::Stop
    }
    async fn pending_guidance(&self, step: &StepLease) -> Option<bool> {
        self.local(step, self.store.pending_run_instructions(self.lease).await)
            .await
            .map(|receipts| !receipts.is_empty())
    }
    async fn clear(&self, step: &StepLease) -> RoundOutcome {
        if self
            .local(step, self.store.mission_clear_round(step).await)
            .await
            .is_none()
        {
            return RoundOutcome::Stop;
        }
        self.signals.changed();
        RoundOutcome::Continue
    }
    pub async fn handle(
        &self,
        step: &StepLease,
        pending: &MissionPendingRound,
        view: &MissionView,
        slots: &[AgentToolSlot],
    ) -> RoundOutcome {
        let calls = slots
            .iter()
            .filter(|slot| slot.step == pending.step)
            .collect::<Vec<_>>();
        let candidate = serde_json::from_str::<MissionCandidate>(&pending.text);
        let input = serde_json::from_str::<InputResponse>(&pending.text);
        let Some(guided) = self
            .local(step, self.store.step_has_run_instructions(step).await)
            .await
        else {
            return RoundOutcome::Stop;
        };
        if guided && !pending.planning && calls.is_empty() && candidate.is_err() && input.is_err() {
            return self.protocol(step, RunState::Failed).await;
        }
        if self
            .local(step, self.store.apply_step_instructions(step).await)
            .await
            .is_none()
        {
            return RoundOutcome::Stop;
        }
        self.signals.changed();
        if pending.planning {
            let plan = serde_json::from_str::<serde_json::Value>(&pending.text)
                .ok()
                .and_then(|value| {
                    serde_json::from_value::<Vec<String>>(value.get("steps")?.clone()).ok()
                });
            let Some(plan) = plan else {
                return self.protocol(step, RunState::Failed).await;
            };
            if self
                .local(
                    step,
                    self.store
                        .mission_save_plan_at(step, pending.acceptance_revision, plan)
                        .await,
                )
                .await
                .is_none()
            {
                return RoundOutcome::Stop;
            }
            self.signals.changed();
            if calls.is_empty() {
                return RoundOutcome::Continue;
            }
        }
        if !pending.planning
            && view.plan.is_empty()
            && self
                .local(
                    step,
                    self.store
                        .mission_save_plan_at(step, pending.acceptance_revision, initial_plan(view))
                        .await,
                )
                .await
                .is_none()
        {
            return RoundOutcome::Stop;
        }
        if !calls.is_empty() {
            let mut failed = false;
            for slot in calls {
                let result = if let Some(result) = &slot.result {
                    result.clone()
                } else {
                    let call = PendingKnowledgeCall {
                        call_id: slot.call_id.clone(),
                        input: slot.input.clone(),
                    };
                    if matches!(&call.input, KnowledgeToolInput::NativeMail { .. }) {
                        let Some(waiting) = self
                            .local(
                                step,
                                self.store
                                    .wait_for_native_mail_tool(step, call.call_id.clone())
                                    .await,
                            )
                            .await
                        else {
                            return RoundOutcome::Stop;
                        };
                        if waiting {
                            self.signals.changed();
                            return RoundOutcome::Stop;
                        }
                    }
                    let Some(result) = self
                        .local(
                            step,
                            execute_tool(
                                self.store,
                                step,
                                &call,
                                self.mail,
                                self.web,
                                self.signals.stopping(),
                            )
                            .await,
                        )
                        .await
                    else {
                        return RoundOutcome::Stop;
                    };
                    result
                };
                failed |= matches!(result, KnowledgeToolResult::Error(_));
            }
            if failed {
                let Some(stop) = self
                    .local(step, self.store.mission_tool_failure(step).await)
                    .await
                else {
                    return RoundOutcome::Stop;
                };
                self.signals.changed();
                return if stop {
                    RoundOutcome::Stop
                } else {
                    RoundOutcome::Continue
                };
            }
            return self.clear(step).await;
        }
        // Only a newer unassigned receipt supersedes a completed response. Clearing
        // the old round never changes that receipt; the next dispatch assigns it.
        match self.pending_guidance(step).await {
            Some(true) => return self.clear(step).await,
            Some(false) => (),
            None => return RoundOutcome::Stop,
        }
        if let Ok(input) = input {
            let result = self
                .store
                .mission_wait_for_input(step, pending.acceptance_revision, input.input_request)
                .await;
            if let Err(error) = result {
                // A receipt accepted between the check and Store's wait admission
                // needs a fresh round, in both live execution and restart recovery.
                match self.pending_guidance(step).await {
                    Some(true) => return self.clear(step).await,
                    Some(false) => (),
                    None => return RoundOutcome::Stop,
                }
                if matches!(error, StoreError::InvalidInput | StoreError::Forbidden) {
                    return self.protocol(step, RunState::Incomplete).await;
                }
                if error == StoreError::Conflict {
                    // Conflict also represents a duplicate input field. Preserve
                    // its existing protocol outcome only while the exact guarded
                    // step and accepted pending round are still current.
                    if self.store.step_has_run_instructions(step).await.is_ok()
                        && matches!(self.store.mission_execution(self.lease).await,
                            Ok(Some(current)) if acceptance_revision(&current.view) == pending.acceptance_revision
                                && current.pending_round.as_ref().is_some_and(|round| round.step == pending.step))
                    {
                        return self.protocol(step, RunState::Incomplete).await;
                    }
                    return RoundOutcome::Stop;
                }
                let _ = self.local::<()>(step, Err(error)).await;
            }
            self.signals.changed();
            return RoundOutcome::Stop;
        }
        let candidate = candidate.unwrap_or(MissionCandidate {
            text: "The model did not return a verifiable result.".into(),
            evidence: vec![],
        });
        let Some(done) = self
            .local(
                step,
                self.store
                    .mission_verify_at(step, pending.acceptance_revision, candidate)
                    .await,
            )
            .await
        else {
            return RoundOutcome::Stop;
        };
        self.signals.changed();
        if done {
            RoundOutcome::Stop
        } else {
            RoundOutcome::Continue
        }
    }
}
