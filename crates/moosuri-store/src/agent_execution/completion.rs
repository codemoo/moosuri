//! Guarded execution settlement. This module never dispatches or repeats work.
use super::*;

fn answer_step(c: &Connection, run: &Run, step: &StepLease) -> Result<()> {
    if memory_commands::has_slot(c, &run.tenant, &run.id)?
        || memory_commands::has_target_plan(c, &run.tenant, &run.id)?
    {
        return Err(StoreError::Conflict);
    }
    let complete: bool = c.query_row(
        "SELECT state='completed' AND NOT EXISTS(SELECT 1 FROM agent_tool_slots t WHERE t.tenant_id=agent_steps.tenant_id AND t.run_id=agent_steps.run_id AND t.step=agent_steps.step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3",
        params![run.tenant, run.id, step.step], |r| r.get(0),
    )?;
    if !complete {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

fn complete_answer(c: &Connection, run: &Run, error: Option<RunFailure>) -> Result<()> {
    chat::transition(c, run, RunState::Completed, error)?;
    crate::learned_skills::finalize(c, run)
}

fn valid_terminal(state: RunState, error: Option<RunFailure>) -> Result<()> {
    if state.active()
        || (state == RunState::OutcomeUnknown
            && !matches!(
                error,
                Some(
                    RunFailure::Interrupted
                        | RunFailure::TimedOut
                        | RunFailure::ProtocolFailed
                        | RunFailure::ConnectionFailed
                )
            ))
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(())
}

impl Store {
    /// Settle a supervisor failure before the first step or between steps.
    /// Successful completion requires a completed no-call step.
    pub async fn finish_agent_execution(
        &self,
        lease: &RunLease,
        state: RunState,
        error: Option<RunFailure>,
    ) -> Result<()> {
        let lease = lease.clone();
        self.call(move |db| {
            valid_terminal(state, error)?;
            if state == RunState::Completed {
                return Err(StoreError::InvalidInput);
            }
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match chat::guard(&tx, &db.provider_key, &lease) {
                Ok(run) => run,
                Err(error) => { tx.commit()?; return Err(error); }
            };
            if !is_agent(&tx, &run.tenant, &run.id)? {
                return Err(StoreError::Conflict);
            }
            tx.execute("UPDATE agent_steps SET state='unknown' WHERE tenant_id=?1 AND run_id=?2 AND state='dispatched'", params![run.tenant, run.id])?;
            chat::transition(&tx, &run, state, error)?;
            tx.commit()?;
            Ok(())
        }).await
    }

    pub async fn finish_agent_run(
        &self,
        lease: &StepLease,
        state: RunState,
        error: Option<RunFailure>,
    ) -> Result<()> {
        if self.try_finish_agent_run(lease, state, error).await? {
            Ok(())
        } else {
            Err(StoreError::Conflict)
        }
    }

    /// Acknowledge only this completed foreground answer's assigned guidance.
    /// True completes it; false retains the run for newer guidance and demotes
    /// this step's output. All three projections commit or roll back together.
    pub async fn settle_agent_answer(&self, lease: &StepLease) -> Result<bool> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match guard_step(&tx, &db.provider_key, &lease) {
                Ok(run) => run,
                Err(error) => {
                    tx.commit()?;
                    return Err(error);
                }
            };
            // Missions require their own evidence/acceptance verification.
            if crate::missions::is_mission(&tx, &run.tenant, &run.id)? {
                return Err(StoreError::Conflict);
            }
            answer_step(&tx, &run, &lease)?;
            crate::run_instructions::apply_to_step(&tx, &run, lease.step)?;
            let complete = !crate::run_instructions::pending(&tx, &run)?;
            if complete {
                complete_answer(&tx, &run, None)?;
            } else {
                crate::run_instructions::supersede_output(&tx, &run, lease.step)?;
            }
            tx.commit()?;
            Ok(complete)
        })
        .await
    }

    /// False means a late accepted instruction requires another safe round.
    /// Callers that only settle a terminal state never acknowledge guidance.
    pub async fn try_finish_agent_run(
        &self,
        lease: &StepLease,
        state: RunState,
        error: Option<RunFailure>,
    ) -> Result<bool> {
        let lease = lease.clone();
        self.call(move |db| {
            valid_terminal(state, error)?;
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run = match guard_step(&tx, &db.provider_key, &lease) {
                Ok(run) => run,
                Err(error) => { tx.commit()?; return Err(error); }
            };
            if state == RunState::Completed {
                if crate::run_instructions::pending(&tx, &run)? {
                    tx.commit()?;
                    return Ok(false);
                }
                answer_step(&tx, &run, &lease)?;
                complete_answer(&tx, &run, error)?;
            } else {
                tx.execute("UPDATE agent_steps SET state='unknown' WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='dispatched'", params![run.tenant, run.id, lease.step])?;
                chat::transition(&tx, &run, state, error)?;
            }
            tx.commit()?;
            Ok(true)
        }).await
    }
}
