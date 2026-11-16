//! Current-owner intent admission and exact named/recent receipt target resolution.
use super::*;

fn previous_reference(quote: &str, kind: ArtifactKind) -> bool {
    let quote = quote.to_lowercase().split_whitespace().collect::<String>();
    let names: &[&str] = match kind {
        ArtifactKind::Document => &[
            "그문서",
            "이문서",
            "방금만든문서",
            "방금저장한문서",
            "방금수정한문서",
            "그보고서",
            "이보고서",
            "방금만든보고서",
            "방금저장한보고서",
            "thatdocument",
            "thisdocument",
            "thepreviousdocument",
            "thatreport",
            "thisreport",
            "thepreviousreport",
        ],
        ArtifactKind::Goal => &[
            "그목표",
            "이목표",
            "방금만든목표",
            "방금저장한목표",
            "thatgoal",
            "thisgoal",
            "thepreviousgoal",
        ],
        ArtifactKind::Followup => &[
            "그요청기록",
            "이요청기록",
            "그약속",
            "방금저장한요청기록",
            "thatfollowup",
            "thatfollow-up",
            "thepreviousfollowup",
        ],
    };
    names.contains(&quote.as_str())
}

fn explicitly_quoted_name(text: &str, quote: &str) -> bool {
    // A title literally named “그 문서” is distinct from an unquoted reference.
    // Repeated uses may mix payload and target; do not guess which span the
    // classifier meant without a valid current receipt or composer selection.
    text.match_indices(quote).count() == 1 && contains_quoted_name(text, quote)
}

fn contains_quoted_name(text: &str, quote: &str) -> bool {
    [('\'', '\''), ('"', '"'), ('‘', '’'), ('“', '”')]
        .iter()
        .any(|(open, close)| text.contains(&format!("{open}{quote}{close}")))
}

fn previous_target(
    c: &Connection,
    run: &Run,
    expected_kind: ArtifactKind,
) -> Result<Option<ArtifactTarget>> {
    // A receipt supplies identity only. The isolated current intent still grants
    // exactly one edit. Never scan past an intervening turn or trust model IDs.
    let previous: Option<String> = c.query_row(
        "SELECT r.id FROM chat_runs r JOIN agent_execution e ON e.tenant_id=r.tenant_id AND e.run_id=r.id WHERE r.tenant_id=?1 AND r.thread_id=?2 AND r.sequence=?3 AND r.account_id=?4 AND r.session_digest=?5 AND r.auth_epoch=?6 AND r.state='completed' AND r.created_at>=?7 AND e.invalidated=0",
        params![run.tenant, run.thread, run.sequence-1, run.account, run.session.as_slice(), run.epoch, now()?-1800], |row|row.get(0)
    ).optional()?;
    let Some(previous) = previous else {
        return Ok(None);
    };
    let mut saved = receipts(c, &run.tenant, &previous)?;
    if saved.len() != 1 {
        return Ok(None);
    }
    let receipt = saved.remove(0);
    let current = version(c, &run.tenant, &receipt.id, None)?;
    if receipt.status != ArtifactStatus::Active
        || current.artifact.kind != expected_kind
        || current.artifact.revision != receipt.revision
        || current.draft.is_none()
    {
        return Err(StoreError::Conflict);
    }
    Ok(Some(ArtifactTarget {
        id: receipt.id,
        revision: receipt.revision,
    }))
}

fn target(
    c: &Connection,
    run: &Run,
    intent: &ArtifactIntent,
    selected: Option<&ArtifactTarget>,
) -> Result<Option<ArtifactTarget>> {
    if intent.action == "create" {
        if intent.target_quote.is_some() {
            return Err(StoreError::InvalidInput);
        }
        return Ok(None);
    }
    if let Some(selected) = selected {
        if let Some(quote) = intent.target_quote.as_deref() {
            if quote.trim().is_empty() || quote.len() > 128 || !run.text.contains(quote) {
                return Err(StoreError::InvalidInput);
            }
            // An explicit selection resolves an unquoted deictic reference,
            // but never overrides a different literal current-user title, even
            // when the same quoted phrase also occurs in the requested body.
            let deictic =
                previous_reference(quote, intent.kind) && !contains_quoted_name(&run.text, quote);
            if !deictic && version(c, &run.tenant, &selected.id, None)?.artifact.title != quote {
                return Err(StoreError::Conflict);
            }
        }
        return Ok(Some(selected.clone()));
    }
    let quote = intent
        .target_quote
        .as_deref()
        .filter(|q| !q.trim().is_empty() && q.len() <= 128 && run.text.contains(q))
        .ok_or(StoreError::Conflict)?;
    // Fetch only exact title matches; unrelated documents cannot exhaust a
    // whole-library candidate cap. Two matches are sufficient to detect ambiguity.
    let matches = c.prepare("SELECT a.id,a.revision FROM personal_artifacts a JOIN personal_artifact_versions v ON v.tenant_id=a.tenant_id AND v.artifact_id=a.id AND v.revision=a.revision WHERE a.tenant_id=?1 AND a.kind=?2 AND a.status='active' AND json_extract(v.draft,'$.title')=?3 ORDER BY a.id LIMIT 2")?
        .query_map(params![run.tenant,kind(intent.kind),quote], |row|Ok(ArtifactTarget{id:row.get(0)?,revision:row.get(1)?}))?
        .collect::<std::result::Result<Vec<_>,_>>()?;
    if matches.len() > 1 {
        return Err(StoreError::Conflict);
    }
    let recent =
        if previous_reference(quote, intent.kind) && !explicitly_quoted_name(&run.text, quote) {
            Some(previous_target(c, run, intent.kind)?.ok_or(StoreError::Conflict)?)
        } else {
            None
        };
    match (matches.into_iter().next(), recent) {
        (Some(named), Some(recent)) if named.id != recent.id => Err(StoreError::Conflict),
        (Some(named), _) => Ok(Some(named)),
        (None, Some(recent)) => Ok(Some(recent)),
        (None, None) => Err(StoreError::Conflict),
    }
}

impl Store {
    /// Only a completed isolated current-message classification can admit a plan.
    /// Retrieval, old receipts and model-provided IDs never grant a new action.
    pub async fn prepare_artifact_intent(
        &self,
        lease: &StepLease,
        intent: Option<ArtifactIntent>,
    ) -> Result<ArtifactRunProfile> {
        let lease = lease.clone();
        self.call(move |db| {
            let tx=db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let run=crate::agent_execution::guard_step(&tx,&db.provider_key,&lease)?;
            let profile=profile(&tx,&run.tenant,&run.id)?.ok_or(StoreError::Conflict)?;
            if profile.planned { return Ok(profile); }
            let completed:bool=tx.query_row("SELECT EXISTS(SELECT 1 FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND state='completed') AND NOT EXISTS(SELECT 1 FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2)",params![run.tenant,run.id,lease.step()],|row|row.get(0))?;
            if !completed { return Err(StoreError::Conflict); }
            if intent.is_none() { crate::agent_execution::record_planner_step(&tx,&run,&lease.run,lease.step(),&lease.nonce,"artifact_no_command")?; }
            let tools:i64=tx.query_row("SELECT tool_calls FROM agent_execution WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id],|row|row.get(0))?;
            if tools!=0 { return Err(StoreError::Conflict); }
            if let Some(intent)=intent {
                if !moosuri_core::ARTIFACT_INTENT_ACTIONS.contains(&intent.action.as_str()) || intent.intent_quote.trim().is_empty() || intent.intent_quote.len()>512 || !run.text.contains(&intent.intent_quote) { return Err(StoreError::InvalidInput); }
                let target=target(&tx,&run,&intent,profile.selected.as_ref())?;
                if let Some(t)=&target {
                    let v=version(&tx,&run.tenant,&t.id,None)?;
                    if v.artifact.revision!=t.revision || v.artifact.kind!=intent.kind || v.draft.is_none() { return Err(StoreError::Conflict); }
                }
                if profile.selected.is_none() && let Some(t)=&target {
                    tx.execute("UPDATE personal_artifact_run_profiles SET selected_id=?3,selected_revision=?4 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id,t.id,t.revision])?;
                }
                tx.execute("INSERT INTO personal_artifact_command_plans(tenant_id,run_id,schema_version,action,kind,target_id,target_revision,generation,used,request_id) VALUES(?1,?2,1,?3,?4,?5,?6,?7,0,?8)",params![run.tenant,run.id,intent.action,kind(intent.kind),target.as_ref().map(|t|&t.id),target.as_ref().map(|t|t.revision),knowledge::generation(&tx,&run.tenant)?,Uuid::new_v4().to_string()])?;
            }
            tx.execute("UPDATE personal_artifact_run_profiles SET planned=1 WHERE tenant_id=?1 AND run_id=?2",params![run.tenant,run.id])?;
            let result=super::profile(&tx,&run.tenant,&run.id)?.ok_or(StoreError::Corrupt)?;
            tx.commit()?;
            Ok(result)
        }).await
    }
}
