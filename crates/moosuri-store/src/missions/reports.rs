//! One requested private output through the existing artifact writer. No queue or
//! model tool grants writes; current accepted owner checks are the sole authority.
use super::*;
use moosuri_core::{
    ArtifactDraft, ArtifactPayload, ArtifactWrite, MissionCompound, MissionCompoundReceipt,
    MissionCompoundStep,
};

pub(super) fn requested(input: &MissionCreate) -> Option<&str> {
    input.checks.iter().find_map(|c| match c {
        MissionCheck::PrivateReport { title } => Some(title.as_str()),
        _ => None,
    })
}
pub(super) fn originally_requested(c: &Connection, run: &Run) -> Result<bool> {
    let raw: String = c.query_row(
        "SELECT payload FROM missions WHERE tenant_id=?1 AND run_id=?2 AND account_id=?3",
        params![run.tenant, run.id, run.account],
        |r| r.get(0),
    )?;
    Ok(requested(&parse::<MissionCreate>(&raw)?).is_some())
}
fn fingerprint(c: &Connection, run: &Run) -> Result<String> {
    // Hash the originally admitted profile. A guarded unresolved-slot fill is
    // materialized separately and never rebinds a mission to current settings.
    let profile: String = c.query_row("SELECT coalesce(json_extract(model,'$.agent_profile'),'null') FROM chat_runs WHERE tenant_id=?1 AND id=?2 AND account_id=?3",params![run.tenant,run.id,run.account],|r|r.get(0))?;
    Ok(format!("{:x}", Sha256::digest(profile.as_bytes())))
}
fn calculation_required(input: &MissionCreate) -> bool {
    let text = format!("{} {}", input.task, input.constraints).to_lowercase();
    let words = text
        .split(|c: char| !c.is_alphabetic())
        .filter(|v| !v.is_empty())
        .collect::<Vec<_>>();
    let tokens = text.split_whitespace().collect::<Vec<_>>();
    let numeric_operator = tokens.windows(2).any(|v| {
        matches!(v[0], "add" | "divide" | "sum")
            && v[1]
                .chars()
                .next()
                .is_some_and(|c| c.is_ascii_digit() || c == '-')
    });
    // These are bounded arithmetic cues, not semantic certification. Generic
    // prose such as product documentation, differences in claims, adding
    // citations or dividing a report into sections must remain read-only.
    input
        .checks
        .iter()
        .any(|v| matches!(v, MissionCheck::DecimalEquals { .. }))
        || numeric_operator
        || words.iter().any(|v| {
            [
                "calculate",
                "calculation",
                "calculations",
                "compute",
                "subtract",
                "multiply",
                "average",
                "percentage",
            ]
            .contains(v)
        })
        || words.windows(2).any(|v| {
            matches!(
                (v[0], v[1]),
                (
                    "sum",
                    "the" | "of" | "these" | "prices" | "amounts" | "numbers"
                ) | ("total", "cost" | "amount" | "price")
                    | ("add", "up")
            )
        })
        || [
            "계산",
            "합계",
            "합산",
            "총액",
            "더해",
            "곱해",
            "빼기",
            "차액",
            "나누",
            "평균",
            "비율",
            "백분율",
        ]
        .iter()
        .any(|v| text.contains(v))
}

fn top(e: &moosuri_core::MissionEvidence) -> MissionCompoundReceipt {
    MissionCompoundReceipt {
        step: e.step,
        call_id: e.call_id.clone(),
        ordinal: None,
    }
}
fn bound_source(
    c: &Connection,
    run: &Run,
    source: &moosuri_core::EvidenceDecimalReceipt,
) -> Result<Option<MissionCompoundReceipt>> {
    let e = &source.source;
    if i64::from(e.step) < acceptance::floor(c, &run.tenant, &run.id)? {
        return Ok(None);
    }
    let slot: Option<(String, String)> = if let Some(ordinal) = e.ordinal {
        c.query_row("SELECT t.input,t.result FROM learned_skill_receipts t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND t.ordinal=?5 AND s.state='completed'",params![run.tenant,run.id,e.step,e.call_id,ordinal],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
    } else {
        c.query_row("SELECT t.input,t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed' AND t.result IS NOT NULL",params![run.tenant,run.id,e.step,e.call_id],|r|Ok((r.get(0)?,r.get(1)?))).optional()?
    };
    let Some((input, result)) = slot else {
        return Ok(None);
    };
    match (
        parse::<KnowledgeToolInput>(&input)?,
        parse::<KnowledgeToolResult>(&result)?,
    ) {
        (KnowledgeToolInput::ReadSource { id, revision, .. }, KnowledgeToolResult::Source(s))
            if id == source.source_id
                && revision == source.source_revision
                && s.source.id == id
                && s.source.revision == revision =>
        {
            Ok(Some(MissionCompoundReceipt {
                step: i64::from(e.step),
                call_id: e.call_id.clone(),
                ordinal: e.ordinal,
            }))
        }
        _ => Ok(None),
    }
}
fn node(
    id: &str,
    purpose: &str,
    capability: &str,
    slots: &[&str],
    dependencies: &[&str],
    output: &str,
    checks: &[&str],
) -> MissionCompoundStep {
    MissionCompoundStep {
        id: id.into(),
        purpose: purpose.into(),
        capability: capability.into(),
        input_slots: slots.iter().map(|s| s.to_string()).collect(),
        depends_on: dependencies.iter().map(|s| s.to_string()).collect(),
        receipts: vec![],
        output: output.into(),
        checks: checks.iter().map(|s| s.to_string()).collect(),
        state: "pending".into(),
    }
}
pub(super) fn admit(c: &Connection, run: &Run, revision: i64) -> Result<()> {
    let request = acceptance::current(c, &run.tenant, &run.id)?;
    if requested(&request).is_none() {
        return Ok(());
    }
    if !originally_requested(c, run)? {
        return Err(StoreError::Forbidden);
    }
    let hash = fingerprint(c, run)?;
    let contract = MissionCompound {
        schema_version: 1,
        calculation_required: calculation_required(&request),
        acceptance_revision: revision,
        capability_profile_sha256: hash.clone(),
        output: None,
        review_required: true,
        steps: vec![
            node(
                "read",
                "Read requested evidence",
                "admitted_read",
                &["current_request"],
                &[],
                "source_receipts",
                &["current owner", "current revision", "bounded actual read"],
            ),
            node(
                "analysis",
                "Compare source claims, gaps and conflicts",
                "synthesis",
                &["source_receipts"],
                &["read"],
                "candidate_report",
                &["owner semantic review"],
            ),
            // Decimal inputs are not assumed to derive from a read. Only the
            // source-bound arithmetic receipt below adds that causal edge.
            node(
                "calculate",
                "Check supported exact calculation",
                "exact_decimal",
                &["current_request"],
                &[],
                "calculation_receipts",
                &["actual successful calculation"],
            ),
            node(
                "report",
                "Save the requested private document",
                "private_document_create",
                &[
                    "candidate_report",
                    "source_receipts",
                    "calculation_receipts",
                ],
                &["read", "analysis", "calculate"],
                "artifact_revision",
                &[
                    "current request",
                    "artifact mutation receipt",
                    "provenance current",
                ],
            ),
        ],
    };
    c.execute("INSERT OR IGNORE INTO mission_report_contracts(tenant_id,run_id,acceptance_revision,account_id,schema_version,profile_sha256,mutation_id,contract) VALUES(?1,?2,?3,?4,1,?5,?6,?7)", params![run.tenant,run.id,revision,run.account,hash,Uuid::new_v4().to_string(),json(&contract)?])?;
    Ok(())
}
pub(super) fn view(c: &Connection, tenant: &str, run: &str) -> Result<Option<MissionCompound>> {
    let revision = acceptance::revision(c, tenant, run)?;
    let raw: Option<String> = c.query_row("SELECT contract FROM mission_report_contracts WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3", params![tenant,run,revision], |r|r.get(0)).optional()?;
    let Some(raw) = raw else { return Ok(None) };
    let mut value: MissionCompound = parse(&raw)?;
    if let Some(output) = &value.output {
        let active: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM personal_artifacts WHERE tenant_id=?1 AND id=?2 AND revision=?3 AND status='active') AND NOT EXISTS(SELECT 1 FROM agent_execution WHERE tenant_id=?1 AND run_id=?4 AND invalidated=1)", params![tenant,output.id,output.revision,run], |r|r.get(0))?;
        if !active {
            value.output = None;
            value.steps[3].state = "withdrawn".into();
        }
    }
    Ok(Some(value))
}
/// Called inside the verifier transaction after mechanical checks. The stable
/// mutation UUID, artifact version and completion record commit together.
pub(super) fn save(
    c: &Connection,
    run: &Run,
    revision: i64,
    candidate: &MissionCandidate,
) -> Result<bool> {
    let input = acceptance::current(c, &run.tenant, &run.id)?;
    let Some(title) = requested(&input) else {
        return Ok(false);
    };
    if !originally_requested(c, run)? {
        return Err(StoreError::Forbidden);
    }
    let row: Option<(String,String,String,String)> = c.query_row("SELECT account_id,profile_sha256,mutation_id,contract FROM mission_report_contracts WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3 AND schema_version=1",params![run.tenant,run.id,revision],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?))).optional()?;
    let Some((account, hash, mutation, raw)) = row else {
        return Ok(false);
    };
    if account != run.account || hash != fingerprint(c, run)? {
        return Err(StoreError::Conflict);
    }
    let mut contract: MissionCompound = parse(&raw)?;
    if contract.output.is_some() {
        return Ok(view(c, &run.tenant, &run.id)?.is_some_and(|v| v.output.is_some()));
    }
    let mut reads = vec![];
    let mut calculations = vec![];
    let mut causal = vec![];
    let mut seen = std::collections::BTreeSet::new();
    for evidence in &candidate.evidence {
        if !seen.insert((evidence.step, evidence.call_id.as_str())) {
            continue;
        }
        if evidence.step < acceptance::floor(c, &run.tenant, &run.id)? {
            return Ok(false);
        }
        let slot: Option<(String,String)> = c.query_row("SELECT t.input,t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed' AND t.result IS NOT NULL", params![run.tenant,run.id,evidence.step,evidence.call_id], |r|Ok((r.get(0)?,r.get(1)?))).optional()?;
        let Some((raw_input, raw_result)) = slot else {
            return Ok(false);
        };
        let tool: KnowledgeToolInput = parse(&raw_input)?;
        match parse::<KnowledgeToolResult>(&raw_result)? {
            KnowledgeToolResult::Source(_) | KnowledgeToolResult::Memory(_) => {
                reads.push(top(evidence))
            }
            KnowledgeToolResult::External(_)
                if matches!(
                    tool,
                    KnowledgeToolInput::External {
                        request: moosuri_core::ExternalToolRequest::Web(
                            moosuri_core::WebToolRequest::Read { .. }
                        )
                    } | KnowledgeToolInput::External {
                        request: moosuri_core::ExternalToolRequest::Mail(
                            moosuri_core::MailToolRequest::Read { .. }
                                | moosuri_core::MailToolRequest::Thread { .. }
                                | moosuri_core::MailToolRequest::Attachment { .. }
                        )
                    }
                ) =>
            {
                reads.push(top(evidence))
            }
            KnowledgeToolResult::External(_)
                if matches!(tool, KnowledgeToolInput::NativeMail { .. })
                    && crate::native_task_mail::report_receipt(
                        c,
                        run,
                        evidence.step,
                        &evidence.call_id,
                    )? =>
            {
                reads.push(top(evidence))
            }
            KnowledgeToolResult::Decimal { .. } | KnowledgeToolResult::DecimalWorkflow { .. } => {
                calculations.push(top(evidence))
            }
            KnowledgeToolResult::EvidenceCalculation {
                evidence: source, ..
            } => {
                calculations.push(top(evidence));
                let Some(read) = bound_source(c, run, &source)? else {
                    return Ok(false);
                };
                causal.push(read.clone());
                reads.push(read);
            }
            KnowledgeToolResult::Error(_) => return Ok(false),
            _ => {}
        }
    }
    let mut unique = std::collections::BTreeSet::new();
    reads.retain(|e| unique.insert((e.step, e.call_id.clone(), e.ordinal)));
    if reads.is_empty() || calculations.is_empty() && contract.calculation_required {
        return Ok(false);
    }
    if !causal.is_empty() {
        if causal.iter().any(|e| {
            !reads
                .iter()
                .any(|r| r.step == e.step && r.call_id == e.call_id && r.ordinal == e.ordinal)
        }) {
            return Ok(false);
        }
        contract.steps[2].depends_on.push("read".into());
        contract.steps[2].input_slots.push("source_receipts".into());
    }
    contract.steps[0].receipts = reads;
    contract.steps[0].state = "completed".into();
    contract.steps[1].state = "review_required".into();
    contract.steps[2].state = if calculations.is_empty() {
        "skipped"
    } else {
        "completed"
    }
    .into();
    contract.steps[2].receipts = calculations;
    let refs = crate::artifacts::agent::run_refs(c, run, None)?;
    let receipt = crate::artifacts::write(
        c,
        &run.tenant,
        ArtifactWrite {
            request_id: mutation,
            id: None,
            expected_revision: None,
            draft: ArtifactDraft {
                title: title.into(),
                body: candidate.text.clone(),
                payload: ArtifactPayload::Document,
                references: vec![],
            },
        },
        refs,
        Some(&run.id),
    )?;
    contract.steps[3].state = "completed".into();
    contract.output = Some(receipt.clone());
    c.execute("UPDATE mission_report_contracts SET contract=?4,artifact_id=?5,artifact_revision=?6 WHERE tenant_id=?1 AND run_id=?2 AND acceptance_revision=?3 AND artifact_id IS NULL",params![run.tenant,run.id,revision,json(&contract)?,receipt.id,receipt.revision])?;
    Ok(true)
}
