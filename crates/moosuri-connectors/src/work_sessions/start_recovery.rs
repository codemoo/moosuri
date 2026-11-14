//! A repeated invocation may recover its exact keyed start. It never adopts an
//! unrelated active run or rebinds a previously staged batch envelope.
use super::*;
use std::path::Path;
fn install(state: &mut State, ack: SourceRunStartReceipt) -> Result<()> {
    let p = state.pending.as_mut().ok_or("missing_pending")?;
    if let Some(run) = &p.run
        && (run.run_id != ack.run_id || run.generation != ack.generation)
    {
        return Err("saved_start_identity_changed_keep_state");
    }
    p.run = Some(Run {
        run_id: ack.run_id,
        generation: ack.generation,
    });
    Ok(())
}
async fn dispatch(
    state: &mut State,
    http: &transport::Http,
    path: &Path,
) -> Result<SourceRunStartReceipt> {
    let p = state.pending.as_mut().ok_or("missing_pending")?;
    p.run_start_attempted = true;
    let intent = p.start_intent.clone().ok_or("missing_saved_start_intent")?;
    transport::save(path, state)?;
    let ack = http.start(&intent).await?;
    install(state, ack.clone())?;
    transport::save(path, state)?;
    Ok(ack)
}
pub(super) async fn ensure(
    state: &mut State,
    options: &Options,
    http: &transport::Http,
    path: &Path,
    root: &str,
    resuming: bool,
) -> Result<()> {
    let current = http.status().await?;
    binding(state, options, http.origin(), &current, root)?;
    position(state, &current)?;
    let p = state.pending.as_ref().ok_or("missing_pending")?;
    if p.start_intent.is_none() && p.run.is_none() && p.run_start_attempted {
        return Err("legacy_unkeyed_start_unknown_keep_state");
    }
    if p.start_intent.is_none() {
        if let Some(run) = &p.run
            && http.same_run(run, &current)
        {
            return Ok(());
        }
        if p.run.is_some() {
            return Err("legacy_inactive_start_requires_reconciliation_keep_state");
        }
        if current.active_run.is_some() {
            return Err("unrelated_active_run_keep_state");
        }
        state
            .pending
            .as_mut()
            .ok_or("missing_pending")?
            .start_intent = Some(SourceRunStartIntent {
            start_binding: current
                .start_binding
                .clone()
                .ok_or("start_recovery_server_upgrade_required")?,
            request_id: Uuid::new_v4().to_string(),
            config_revision: state.config_revision,
            expected_run_generation: current
                .run_generation
                .ok_or("start_recovery_server_upgrade_required")?,
            expected_sequence: state.sequence,
            checkpoint_before: state.checkpoint.clone(),
        });
        transport::save(path, state)?;
    }
    // Even after a lost ACK, dispatch is the same persisted composite intent.
    let ack = dispatch(state, http, path).await?;
    let current = http.status().await?;
    binding(state, options, http.origin(), &current, root)?;
    position(state, &current)?;
    let p = state.pending.as_ref().ok_or("missing_pending")?;
    if ack.state == "running" && http.same_run(p.run.as_ref().ok_or("missing_saved_run")?, &current)
    {
        return Ok(());
    }
    if ack.state == "running" || !resuming || current.active_run.is_some() {
        return Err("inactive_start_retry_requires_same_saved_intent");
    }
    // An explicit later invocation can request one fresh attempt only when its
    // old historical identity is known and no batch envelope was ever staged.
    if p.envelope.is_some() || p.committed || http.receipt(&p.request_id).await?.is_some() {
        return Err("inactive_staged_attempt_keep_state");
    }
    let generation = current
        .run_generation
        .ok_or("start_recovery_server_upgrade_required")?;
    if generation < ack.generation {
        return Err("start_generation_regressed_keep_state");
    }
    let scan = files::Scan {
        entries: p.entries.clone(),
        metadata: p.metadata.clone(),
        files: p.files_after.clone(),
        objects: p.objects_after.clone(),
        scan_after: p.scan_after.clone(),
        examined: p.examined_files,
        deferred: p.deferred_files,
        unsupported: p.unsupported_files,
    };
    stage(state, scan, &current)?;
    transport::save(path, state)?;
    let fresh = dispatch(state, http, path).await?;
    let current = http.status().await?;
    binding(state, options, http.origin(), &current, root)?;
    position(state, &current)?;
    if fresh.state != "running"
        || !http.same_run(
            state
                .pending
                .as_ref()
                .ok_or("missing_pending")?
                .run
                .as_ref()
                .ok_or("missing_saved_run")?,
            &current,
        )
    {
        return Err("fresh_start_inactive_keep_state");
    }
    Ok(())
}
