//! Structural DAG routes: references are explicit; training numbers never persist.
use super::*;
use moosuri_core::{DecimalGraph, DecimalReference, DecimalWorkflow};
const PREFIX: &str = "decimal-graph-v1:";
pub(super) fn graph(route: &str) -> Option<DecimalGraph> {
    let body = route.strip_prefix(PREFIX)?;
    if body.len() > 2048 {
        return None;
    }
    let graph: DecimalGraph = serde_json::from_str(body).ok()?;
    let canonical = graph.canonical()?;
    (encode(&canonical).ok()?.as_str() == body).then_some(canonical)
}
fn route(graph: &DecimalGraph) -> Option<String> {
    Some(format!("{PREFIX}{}", encode(&graph.canonical()?).ok()?))
}
fn expected(
    input: &DecimalWorkflow,
) -> std::result::Result<
    (
        Output,
        Vec<crate::agent_execution::arithmetic::WorkflowReceipt>,
    ),
    crate::ToolReadError,
> {
    let calls = crate::agent_execution::arithmetic::workflow(input)?;
    let results = calls
        .iter()
        .map(|(_, r)| r.clone())
        .collect::<std::result::Result<Vec<_>, _>>();
    let output = match results {
        Ok(results) => Output::DecimalWorkflow {
            exact: results.last().unwrap().clone(),
            results,
        },
        Err(e) => Output::Error(e),
    };
    Ok((output, calls))
}
pub(crate) fn execute(
    conn: &Connection,
    run: &chat::Run,
    input: &DecimalWorkflow,
    step: i64,
    call_id: &str,
) -> Result<(Output, i64)> {
    if run
        .tool_profile
        .as_ref()
        .is_none_or(|p| !matches!(p.version, 2..=5))
    {
        return Ok((Output::Error(crate::ToolReadError::Unavailable), 1));
    }
    let (output, calls) = match expected(input) {
        Ok(v) => v,
        Err(e) => return Ok((Output::Error(e), 1)),
    };
    for (ordinal, (calculation, result)) in calls.iter().enumerate() {
        let result = match result {
            Ok(exact) => Output::Decimal {
                exact: exact.clone(),
            },
            Err(e) => Output::Error(*e),
        };
        conn.execute(
            "INSERT INTO learned_skill_receipts VALUES(?1,?2,?3,?4,?5,?6,?7)",
            params![
                run.tenant,
                run.id,
                step,
                call_id,
                ordinal as i64,
                encode(&Input::DecimalCalculate {
                    calculation: calculation.clone()
                })?,
                encode(&result)?
            ],
        )?;
    }
    Ok((output, (calls.len() as i64).max(1)))
}
pub(super) fn proof(
    input: &DecimalWorkflow,
    result: &Output,
    output: &str,
    receipt: &str,
) -> Option<Proof> {
    let (expected, _) = expected(input).ok()?;
    let Output::DecimalWorkflow { exact, .. } = &expected else {
        return None;
    };
    if encode(&expected).ok()? != encode(result).ok()? || !adopted_number(output, exact) {
        return None;
    }
    // Canonical instantiated expressions collapse numeric notation and commutative
    // reordering. Equal numeric intermediate values never create graph edges.
    let mut identities = Vec::<String>::new();
    for step in &input.graph.steps {
        let mut children = step
            .operands
            .iter()
            .map(|r| match r {
                DecimalReference::Input { index } => {
                    crate::agent_execution::arithmetic::calculate(&DecimalCalculation {
                        operation: DecimalOperation::Sum,
                        operands: vec![input.inputs[usize::from(*index)].clone(), "0".into()],
                    })
                    .ok()
                    .map(|s| hash(&s))
                }
                DecimalReference::Result { index } => Some(identities[usize::from(*index)].clone()),
            })
            .collect::<Option<Vec<_>>>()?;
        if matches!(
            step.operation,
            DecimalOperation::Sum | DecimalOperation::Product
        ) {
            children.sort();
        }
        identities.push(hash(&encode(&(step.operation, children)).ok()?));
    }
    Some(Proof {
        route: route(&input.graph)?,
        input: identities.pop()?,
        receipt: receipt.into(),
        subject: None,
    })
}
