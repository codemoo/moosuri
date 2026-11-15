//! Bind one fresh, uniquely selected source number to an explicit exact DAG.
//! Reads use the existing runtime; this module performs no IO or semantic extraction.
use super::*;
use crate::ToolReadError;
use moosuri_core::{
    CurrentSpan, EvidenceBinding, EvidenceCalculation, EvidenceDecimalReceipt, EvidenceOrigin,
    EvidenceParameters, EvidencePlan, ToolReceiptReference,
};
const PREFIX: &str = "evidence-decimal-v1:";
fn decode<T: serde::de::DeserializeOwned>(raw: &str) -> Result<T> {
    serde_json::from_str(raw).map_err(|_| StoreError::Corrupt)
}
pub(super) fn plan(route: &str) -> Option<EvidencePlan> {
    let raw = route.strip_prefix(PREFIX)?;
    if raw.len() > 3072 {
        return None;
    }
    let plan: EvidencePlan = serde_json::from_str(raw).ok()?;
    let plan = plan.canonical()?;
    (encode(&plan).ok()?.as_str() == raw).then_some(plan)
}
fn route(plan: &EvidencePlan) -> Option<String> {
    Some(format!("{PREFIX}{}", encode(&plan.canonical()?).ok()?))
}
struct ResolvedParameters {
    query: CurrentSpan,
    prefix: CurrentSpan,
    scalars: Vec<CurrentSpan>,
}
fn quote(text: &str, literal: &str) -> Option<CurrentSpan> {
    if literal.is_empty() {
        return None;
    }
    let positions = text
        .char_indices()
        .filter_map(|(at, _)| text[at..].starts_with(literal).then_some(at))
        .take(2)
        .collect::<Vec<_>>();
    if positions.len() != 1 {
        return None;
    }
    Some(CurrentSpan {
        start_byte: positions[0] as u32,
        end_byte: (positions[0] + literal.len()) as u32,
    })
}
fn resolve_parameters(
    text: &str,
    query: &str,
    prefix: &str,
    scalars: &[String],
) -> Option<ResolvedParameters> {
    if query.is_empty()
        || query.len() > 512
        || query.trim() != query
        || prefix.is_empty()
        || prefix.len() > 160
        || prefix.trim() != prefix
        || prefix.chars().any(char::is_control)
    {
        return None;
    }
    let resolved = ResolvedParameters {
        query: quote(text, query)?,
        prefix: quote(text, prefix)?,
        scalars: scalars
            .iter()
            .map(|v| quote(text, v))
            .collect::<Option<Vec<_>>>()?,
    };
    let mut ranges = [&resolved.query, &resolved.prefix]
        .into_iter()
        .chain(resolved.scalars.iter())
        .collect::<Vec<_>>();
    ranges.sort_by_key(|v| v.start_byte);
    if ranges.windows(2).any(|v| v[0].end_byte > v[1].start_byte) {
        return None;
    }
    if resolved
        .scalars
        .iter()
        .any(|range| scalar(text, range).is_none())
    {
        return None;
    }
    Some(resolved)
}
fn span<'a>(text: &'a str, range: &CurrentSpan) -> Option<&'a str> {
    text.get(range.start_byte as usize..range.end_byte as usize)
}
fn number(text: &str) -> std::result::Result<String, ToolReadError> {
    crate::agent_execution::arithmetic::calculate(&DecimalCalculation {
        operation: DecimalOperation::Sum,
        operands: vec![text.into(), "0".into()],
    })
}
fn scalar(text: &str, range: &CurrentSpan) -> Option<String> {
    let literal = span(text, range)?;
    let mut preceding = text[..range.start_byte as usize].chars().rev();
    let left_separated = match preceding.next() {
        None => true,
        Some(c) if c.is_whitespace() => true,
        Some('(' | '[' | '{' | '\'' | '"' | '‘' | '“' | ';') => true,
        Some(',' | ':') => preceding.next().is_none_or(|c| !c.is_ascii_digit()),
        _ => false,
    };
    let mut following = text[range.end_byte as usize..].chars();
    let right_separated = match following.next() {
        None => true,
        Some(c) if c.is_whitespace() => true,
        Some(')' | ']' | '}' | '\'' | '"' | '’' | '”' | ';' | '!' | '?') => true,
        Some(',' | ':') => following.next().is_none_or(|c| !c.is_ascii_digit()),
        // A single sentence-final period is a separator. Decimal/version
        // continuations, grouping and adjacent operators are not delimiters.
        Some('.') => following.next().is_none_or(|c| {
            c.is_whitespace() || matches!(c, ')' | ']' | '}' | '\'' | '"' | '’' | '”')
        }),
        _ => false,
    };
    if !left_separated || !right_separated {
        return None;
    }
    number(literal).ok()
}
fn slot(
    c: &Connection,
    r: &chat::Run,
    address: &ToolReceiptReference,
    before: i64,
) -> Result<Option<(Input, Output)>> {
    if address.step == 0
        || i64::from(address.step) >= before
        || address.call_id.is_empty()
        || address.call_id.len() > 256
    {
        return Ok(None);
    }
    let row: Option<(String, String)> = if let Some(ordinal) = address.ordinal {
        c.query_row("SELECT t.input,t.result FROM learned_skill_receipts t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND t.ordinal=?5 AND s.state='completed'",params![r.tenant,r.id,address.step,address.call_id,ordinal],|v|Ok((v.get(0)?,v.get(1)?))).optional()?
    } else {
        c.query_row(
        "SELECT t.input,t.result FROM agent_tool_slots t JOIN agent_steps s USING(tenant_id,run_id,step) WHERE t.tenant_id=?1 AND t.run_id=?2 AND t.step=?3 AND t.call_id=?4 AND s.state='completed' AND t.result IS NOT NULL",
        params![r.tenant,r.id,address.step,address.call_id],|v| Ok((v.get(0)?,v.get(1)?))).optional()?
    };
    row.map(|(input, result)| Ok((decode(&input)?, decode(&result)?)))
        .transpose()
}
struct Bound {
    inputs: Vec<String>,
    query: String,
    prefix: String,
    evidence: EvidenceDecimalReceipt,
}
fn resolve(
    c: &Connection,
    r: &chat::Run,
    calculation: &EvidenceCalculation,
    step: i64,
) -> Result<Option<Bound>> {
    let plan = &calculation.plan;
    let b = &calculation.binding;
    if !plan.valid()
        || b.scalars.len() + 1 != plan.origins.len()
        || !(b.search.step < b.source.step
            && b.search.ordinal.is_none()
            && b.source.ordinal.is_none()
            || b.search.step == b.source.step
                && b.search.call_id == b.source.call_id
                && b.search.ordinal == Some(0)
                && b.source.ordinal == Some(1))
    {
        return Ok(None);
    }
    let Some(resolved) = resolve_parameters(&r.text, &b.query, &b.prefix, &b.scalars) else {
        return Ok(None);
    };
    let query = b.query.as_str();
    let prefix = b.prefix.as_str();
    let Some((Input::Search { query: q }, Output::Search(page))) = slot(
        c,
        r,
        &b.search,
        if b.search.ordinal.is_some() {
            i64::from(b.source.step) + 1
        } else {
            i64::from(b.source.step)
        },
    )?
    else {
        return Ok(None);
    };
    if q.kind != KnowledgeKind::Source
        || q.text != query
        || q.cursor.is_some()
        || q.collection_id.is_some()
        || q.subject.is_some()
        || q.speaker.is_some()
        || q.valid_at.is_some()
        || q.match_mode == Some(moosuri_core::KnowledgeMatchMode::KoreanAliasesV1)
        || q.observed_from.is_some()
        || q.observed_until.is_some()
        || page.hits.len() != 1
        || page.next_cursor.is_some()
        || page.truncated
    {
        return Ok(None);
    }
    // Recheck uniqueness now, not merely among a stale or partial earlier page.
    let current = knowledge::search::search_owner(c, &r.tenant, q.clone(), 5)?;
    if current.hits.len() != 1 || current.next_cursor.is_some() || current.truncated {
        return Ok(None);
    }
    let Some((
        Input::ReadSource {
            id,
            revision,
            start_byte: 0,
            max_bytes: 4096,
        },
        Output::Source(source),
    )) = slot(c, r, &b.source, step)?
    else {
        return Ok(None);
    };
    let hit = &page.hits[0];
    let fresh = &current.hits[0];
    if hit.id != id
        || hit.revision != revision
        || fresh.id != id
        || fresh.revision != revision
        || source.source.id != id
        || source.source.revision != revision
        || source.start_byte != 0
        || source.end_byte as usize != source.text.len()
        || source.has_more
        || source.next_start_byte.is_some()
        || source.text.is_empty()
    {
        return Ok(None);
    }
    let document = knowledge::sources::document(c, &r.tenant, &id, Some(revision))?;
    if document.text != source.text || document.source.revision != revision {
        return Ok(None);
    }
    let found = source
        .text
        .char_indices()
        .filter_map(|(at, _)| source.text[at..].starts_with(prefix).then_some(at))
        .take(2)
        .collect::<Vec<_>>();
    if found.len() != 1 {
        return Ok(None);
    }
    let after = found[0] + prefix.len();
    let tail = &source.text[after..];
    let start = after + tail.len() - tail.trim_start_matches([' ', '\t']).len();
    let end = start
        + source.text[start..]
            .find(char::is_whitespace)
            .unwrap_or(source.text.len() - start);
    let literal = &source.text[start..end];
    let Ok(source_number) = number(literal) else {
        return Ok(None);
    };
    let Some(scalars) = resolved
        .scalars
        .iter()
        .map(|s| scalar(&r.text, s))
        .collect::<Option<Vec<_>>>()
    else {
        return Ok(None);
    };
    let mut scalars = scalars.into_iter();
    let inputs = plan
        .origins
        .iter()
        .map(|kind| match kind {
            EvidenceOrigin::Source => source_number.clone(),
            EvidenceOrigin::Current => scalars.next().unwrap(),
        })
        .collect();
    Ok(Some(Bound {
        inputs,
        query: query.into(),
        prefix: prefix.into(),
        evidence: EvidenceDecimalReceipt {
            source_id: id,
            source_revision: revision,
            source_title: source.source.title,
            start_byte: start as u32,
            end_byte: end as u32,
            literal: literal.into(),
            current_query: resolved.query,
            current_prefix: resolved.prefix,
            current_scalars: resolved.scalars,
            search: b.search.clone(),
            source: b.source.clone(),
        },
    }))
}
fn expected(
    calculation: &EvidenceCalculation,
    bound: &Bound,
) -> std::result::Result<
    (
        Output,
        Vec<crate::agent_execution::arithmetic::WorkflowReceipt>,
    ),
    ToolReadError,
> {
    let calls =
        crate::agent_execution::arithmetic::evidence_workflow(&moosuri_core::DecimalWorkflow {
            graph: calculation.plan.graph.clone(),
            inputs: bound.inputs.clone(),
        })?;
    let results = calls
        .iter()
        .map(|(_, r)| r.clone())
        .collect::<std::result::Result<Vec<_>, _>>();
    let result = match results {
        Ok(results) => Output::EvidenceCalculation {
            exact: results.last().unwrap().clone(),
            results,
            evidence: bound.evidence.clone(),
        },
        Err(error) => Output::Error(error),
    };
    Ok((result, calls))
}
pub(crate) fn execute(
    c: &Connection,
    r: &chat::Run,
    calculation: &EvidenceCalculation,
    step: i64,
    call: &str,
) -> Result<(Output, i64)> {
    if !r
        .tool_profile
        .as_ref()
        .is_some_and(|p| p.evidence_calculation)
    {
        return Ok((Output::Error(ToolReadError::Unavailable), 1));
    }
    if calculation.binding.search.ordinal.is_some() || calculation.binding.source.ordinal.is_some()
    {
        return Ok((Output::Error(ToolReadError::InvalidInput), 1));
    }
    let mut accepted;
    let r = if crate::missions::is_mission(c, &r.tenant, &r.id)? {
        let floor = crate::missions::acceptance_floor(c, &r.tenant, &r.id)?;
        if i64::from(calculation.binding.search.step) < floor
            || i64::from(calculation.binding.source.step) < floor
        {
            return Ok((Output::Error(ToolReadError::InvalidInput), 1));
        }
        accepted = r.clone();
        accepted.text = crate::missions::learned_task_text(c, r)?;
        &accepted
    } else {
        r
    };
    execute_at(c, r, calculation, step, call, step)
}
fn execute_at(
    c: &Connection,
    r: &chat::Run,
    calculation: &EvidenceCalculation,
    step: i64,
    call: &str,
    before: i64,
) -> Result<(Output, i64)> {
    let Some(bound) = resolve(c, r, calculation, before)? else {
        return Ok((Output::Error(ToolReadError::InvalidInput), 1));
    };
    let (result, calls) = match expected(calculation, &bound) {
        Ok(v) => v,
        Err(error) => return Ok((Output::Error(error), 1)),
    };
    let offset = if calculation.binding.source.ordinal.is_some() {
        2
    } else {
        0
    };
    for (ordinal, (calculation, result)) in calls.iter().enumerate() {
        let result = match result {
            Ok(exact) => Output::Decimal {
                exact: exact.clone(),
            },
            Err(error) => Output::Error(*error),
        };
        c.execute(
            "INSERT INTO learned_skill_receipts VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                r.tenant,
                r.id,
                step,
                call,
                (ordinal + offset) as i64,
                encode(&Input::DecimalCalculate {
                    calculation: calculation.clone()
                })?,
                encode(&result)?
            ],
        )?;
    }
    Ok((result, (calls.len() as i64).max(1)))
}
pub(super) fn execute_fresh(
    c: &Connection,
    r: &chat::Run,
    plan: EvidencePlan,
    parameters: &EvidenceParameters,
    step: i64,
    call: &str,
) -> Result<(Output, i64)> {
    if !r
        .tool_profile
        .as_ref()
        .is_some_and(|p| p.evidence_calculation)
        || !plan.valid()
    {
        return Ok((Output::Error(ToolReadError::Unavailable), 1));
    }
    if parameters.scalars.len() + 1 != plan.origins.len()
        || resolve_parameters(
            &r.text,
            &parameters.query,
            &parameters.prefix,
            &parameters.scalars,
        )
        .is_none()
    {
        return Ok((Output::Error(ToolReadError::InvalidInput), 1));
    }
    let query = parameters.query.as_str();
    let mut ordinal = 0i64;
    let result = super::source_route(query, true, |input| {
        let result = crate::agent_execution::read_tool(c, r, &input)?;
        c.execute(
            "INSERT INTO learned_skill_receipts VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                r.tenant,
                r.id,
                step,
                call,
                ordinal,
                encode(&input)?,
                encode(&result)?
            ],
        )?;
        ordinal += 1;
        Ok(result)
    })?;
    if !matches!(result, Output::Source(_)) {
        return Ok((result, ordinal.max(1)));
    }
    let address = |ordinal| ToolReceiptReference {
        step: step as u32,
        call_id: call.into(),
        ordinal: Some(ordinal),
    };
    let calculation = EvidenceCalculation {
        plan,
        binding: EvidenceBinding {
            search: address(0),
            source: address(1),
            query: parameters.query.clone(),
            prefix: parameters.prefix.clone(),
            scalars: parameters.scalars.clone(),
        },
    };
    let (result, cost) = execute_at(c, r, &calculation, step, call, step + 1)?;
    Ok((result, ordinal + cost))
}
/// A separate mixed trace never becomes an implicit procedure. The final tool
/// explicitly names its source receipts and the structure it actually evaluated.
pub(super) fn proof(
    c: &Connection,
    tenant: &str,
    account: &str,
    run: &str,
    calls: &[(Input, Output)],
    receipt: &str,
) -> Result<Option<Proof>> {
    let [
        (Input::Search { .. }, _),
        (Input::ReadSource { .. }, _),
        (Input::EvidenceCalculate { calculation }, result),
    ] = calls
    else {
        return Ok(None);
    };
    let r = chat::load_run(c, tenant, run)?;
    if r.account != account
        || !r
            .tool_profile
            .as_ref()
            .is_some_and(|p| p.evidence_calculation)
    {
        return Ok(None);
    }
    let parent:Option<(i64,String)>=c.query_row("SELECT step,call_id FROM agent_tool_slots WHERE tenant_id=?1 AND run_id=?2 AND input=?3",params![tenant,run,encode(&Input::EvidenceCalculate{calculation:calculation.clone()})?],|v|Ok((v.get(0)?,v.get(1)?))).optional()?;
    let Some((step, call)) = parent else {
        return Ok(None);
    };
    let Some(bound) = resolve(c, &r, calculation, step)? else {
        return Ok(None);
    };
    let Ok((expected, operations)) = expected(calculation, &bound) else {
        return Ok(None);
    };
    let Output::EvidenceCalculation { exact, .. } = &expected else {
        return Ok(None);
    };
    if encode(&expected)? != encode(result)? {
        return Ok(None);
    }
    let children=c.prepare("SELECT input,result FROM learned_skill_receipts WHERE tenant_id=?1 AND run_id=?2 AND step=?3 AND call_id=?4 ORDER BY ordinal")?.query_map(params![tenant,run,step,call],|v|Ok((v.get::<_,String>(0)?,v.get::<_,String>(1)?)))?.collect::<std::result::Result<Vec<_>,_>>()?;
    if children.len() != operations.len() {
        return Ok(None);
    }
    for ((input, result), (calculation, value)) in children.iter().zip(operations) {
        let Ok(exact) = value else {
            return Ok(None);
        };
        if *input != encode(&Input::DecimalCalculate { calculation })?
            || *result != encode(&Output::Decimal { exact })?
        {
            return Ok(None);
        }
    }
    let output = terminal_output(c, tenant, run)?;
    if !adopted_number(&output, exact)
        || !adopted_number(&output, &bound.evidence.literal)
        || !output.contains(&bound.evidence.source_title)
        || !output.contains(&bound.prefix)
    {
        return Ok(None);
    }
    // Fresh parameters and instantiated expressions, not coincidentally equal
    // results, define the input. Subject uniqueness also requires distinct sources.
    let input = hash(&encode(&(
        canonical_text(&bound.query),
        &bound.prefix,
        &bound.inputs,
        &calculation.plan.graph,
    ))?);
    Ok(Some(Proof {
        route: route(&calculation.plan).ok_or(StoreError::Corrupt)?,
        input,
        receipt: receipt.into(),
        subject: Some(hash(&bound.evidence.source_id)),
    }))
}
pub(super) fn terminal_output(c: &Connection, tenant: &str, run: &str) -> Result<String> {
    let step: Option<i64> = c.query_row(
        "SELECT max(step) FROM agent_steps WHERE tenant_id=?1 AND run_id=?2 AND state='completed'",
        params![tenant, run],
        |v| v.get(0),
    )?;
    let Some(step) = step else {
        return Ok(String::new());
    };
    Ok(c.prepare("SELECT text FROM chat_output WHERE tenant_id=?1 AND run_id=?2 AND item LIKE ?3 AND refusal=0 AND (phase IS NULL OR phase='final_answer') ORDER BY ordinal")?.query_map(params![tenant,run,format!("step-{step}-%")],|v|v.get::<_,String>(0))?.collect::<std::result::Result<Vec<_>,_>>()?.join("\n"))
}
