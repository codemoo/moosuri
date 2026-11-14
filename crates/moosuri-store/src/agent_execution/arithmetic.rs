//! Bounded exact decimal operations. No expression language or floating point.
use moosuri_core::{DecimalCalculation, DecimalOperation};

use super::ToolReadError;

fn decimal(value: &str) -> Result<(i128, u32), ToolReadError> {
    if value.is_empty() || value.len() > 48 {
        return Err(ToolReadError::InvalidInput);
    }
    let (negative, value) = match value.strip_prefix('-') {
        Some(value) => (true, value),
        None => (false, value),
    };
    let mut coefficient = 0i128;
    let mut scale = 0;
    let mut fraction = false;
    let mut digits = 0;
    for byte in value.bytes() {
        if byte == b'.' && !fraction && digits > 0 {
            fraction = true;
            continue;
        }
        if !byte.is_ascii_digit() {
            return Err(ToolReadError::InvalidInput);
        }
        digits += 1;
        if fraction {
            scale += 1;
            if scale > 6 {
                return Err(ToolReadError::InvalidInput);
            }
        }
        coefficient = coefficient
            .checked_mul(10)
            .and_then(|v| {
                if negative {
                    v.checked_sub((byte - b'0') as i128)
                } else {
                    v.checked_add((byte - b'0') as i128)
                }
            })
            .ok_or(ToolReadError::ArithmeticOverflow)?;
    }
    if digits == 0 || (fraction && scale == 0) {
        return Err(ToolReadError::InvalidInput);
    }
    Ok((coefficient, scale))
}

pub(crate) fn calculate(input: &DecimalCalculation) -> Result<String, ToolReadError> {
    if !(2..=32).contains(&input.operands.len())
        || (matches!(input.operation, DecimalOperation::Product) && input.operands.len() != 2)
    {
        return Err(ToolReadError::InvalidInput);
    }
    let values = input
        .operands
        .iter()
        .map(|value| decimal(value))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(format_value(evaluate(input.operation, values)?))
}
fn evaluate(
    operation: DecimalOperation,
    values: Vec<(i128, u32)>,
) -> Result<(i128, u32), ToolReadError> {
    let (mut coefficient, mut scale) = if matches!(operation, DecimalOperation::Product) {
        (
            values[0]
                .0
                .checked_mul(values[1].0)
                .ok_or(ToolReadError::ArithmeticOverflow)?,
            values[0].1 + values[1].1,
        )
    } else {
        let scale = values.iter().map(|v| v.1).max().unwrap_or(0);
        let mut result = 0i128;
        for (index, (value, places)) in values.into_iter().enumerate() {
            let value = if value == 0 {
                0
            } else {
                value
                    .checked_mul(
                        10i128
                            .checked_pow(scale - places)
                            .ok_or(ToolReadError::ArithmeticOverflow)?,
                    )
                    .ok_or(ToolReadError::ArithmeticOverflow)?
            };
            result = if index > 0 && matches!(operation, DecimalOperation::Difference) {
                result.checked_sub(value)
            } else {
                result.checked_add(value)
            }
            .ok_or(ToolReadError::ArithmeticOverflow)?;
        }
        (result, scale)
    };
    while scale > 0 && coefficient % 10 == 0 {
        coefficient /= 10;
        scale -= 1;
    }
    Ok((coefficient, scale))
}
fn format_value((mut coefficient, mut scale): (i128, u32)) -> String {
    while scale > 0 && coefficient % 10 == 0 {
        coefficient /= 10;
        scale -= 1;
    }
    let sign = if coefficient < 0 { "-" } else { "" };
    let digits = coefficient.unsigned_abs().to_string();
    if scale == 0 {
        format!("{sign}{digits}")
    } else {
        let digits = format!("{:0>width$}", digits, width = scale as usize + 1);
        let (whole, fraction) = digits.split_at(digits.len() - scale as usize);
        format!("{sign}{whole}.{fraction}")
    }
}

/// External inputs retain the primitive six-place parser. Intermediate values
/// remain typed coefficient/scale pairs, preserving exact generated precision.
pub(crate) type WorkflowReceipt = (DecimalCalculation, Result<String, ToolReadError>);
pub(crate) fn workflow(
    input: &moosuri_core::DecimalWorkflow,
) -> Result<Vec<WorkflowReceipt>, ToolReadError> {
    use moosuri_core::DecimalReference;
    if !input.graph.valid() || input.inputs.len() != usize::from(input.graph.input_count) {
        return Err(ToolReadError::InvalidInput);
    }
    let inputs = input
        .inputs
        .iter()
        .map(|s| decimal(s))
        .collect::<Result<Vec<_>, _>>()?;
    let mut values = Vec::new();
    let mut receipts = Vec::new();
    for step in &input.graph.steps {
        let operands = step
            .operands
            .iter()
            .map(|r| match r {
                DecimalReference::Input { index } => inputs[usize::from(*index)],
                DecimalReference::Result { index } => values[usize::from(*index)],
            })
            .collect::<Vec<_>>();
        let calculation = DecimalCalculation {
            operation: step.operation,
            operands: operands.iter().map(|v| format_value(*v)).collect(),
        };
        match evaluate(step.operation, operands) {
            Ok(value) => {
                let exact = format_value(value);
                values.push(value);
                receipts.push((calculation, Ok(exact)));
            }
            Err(error) => {
                receipts.push((calculation, Err(error)));
                break;
            }
        }
    }
    Ok(receipts)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_decimals_reject_lossy_or_unbounded_inputs() {
        let run = |operation, values: &[&str]| {
            calculate(&DecimalCalculation {
                operation,
                operands: values.iter().map(|v| (*v).into()).collect(),
            })
        };
        assert_eq!(
            run(DecimalOperation::Sum, &["0.1", "0.2"]),
            Ok("0.3".into())
        );
        assert_eq!(
            run(DecimalOperation::Difference, &["1", "2.001", "-0.001"]),
            Ok("-1".into())
        );
        assert_eq!(
            run(DecimalOperation::Product, &["-0.000001", "0.000001"]),
            Ok("-0.000000000001".into())
        );
        assert_eq!(run(DecimalOperation::Sum, &["-0", "0.00"]), Ok("0".into()));
        assert_eq!(
            run(
                DecimalOperation::Sum,
                &["-170141183460469231731687303715884105728", "0"]
            ),
            Ok(i128::MIN.to_string())
        );
        assert_eq!(
            run(
                DecimalOperation::Product,
                &["-170141183460469231731687303715884105728", "-1"]
            ),
            Err(ToolReadError::ArithmeticOverflow)
        );
        for invalid in [
            "1e2",
            "NaN",
            ".2",
            "1.",
            " 1",
            "+1",
            "1.0000001",
            "-",
            "1.2.3",
        ] {
            assert_eq!(
                run(DecimalOperation::Sum, &[invalid, "1"]),
                Err(ToolReadError::InvalidInput)
            );
        }
        assert_eq!(
            run(
                DecimalOperation::Sum,
                &["170141183460469231731687303715884105727", "1"]
            ),
            Err(ToolReadError::ArithmeticOverflow)
        );
        assert_eq!(
            run(
                DecimalOperation::Product,
                &["170141183460469231731687303715884105727", "2"]
            ),
            Err(ToolReadError::ArithmeticOverflow)
        );
        assert_eq!(
            run(
                DecimalOperation::Sum,
                &["999999999999999999999999999999999999999", "0"]
            ),
            Err(ToolReadError::ArithmeticOverflow)
        );
        assert_eq!(
            run(DecimalOperation::Sum, &["1"]),
            Err(ToolReadError::InvalidInput)
        );
        assert_eq!(
            run(DecimalOperation::Product, &["1", "2", "3"]),
            Err(ToolReadError::InvalidInput)
        );
    }
}

#[cfg(test)]
mod workflow_tests {
    use super::*;
    use moosuri_core::{DecimalGraph, DecimalReference::*, DecimalStep, DecimalWorkflow};
    #[test]
    fn intermediate_precision_is_exact_and_external_limits_and_reference_bounds_remain() {
        let graph = DecimalGraph {
            input_count: 3,
            steps: vec![
                DecimalStep {
                    operation: DecimalOperation::Product,
                    operands: vec![Input { index: 0 }, Input { index: 1 }],
                },
                DecimalStep {
                    operation: DecimalOperation::Sum,
                    operands: vec![Result { index: 0 }, Input { index: 2 }],
                },
            ],
        };
        let calls = workflow(&DecimalWorkflow {
            graph: graph.clone(),
            inputs: vec!["0.000001".into(), "0.000001".into(), "0.000001".into()],
        })
        .unwrap();
        assert_eq!(calls[1].1, Ok("0.000001000001".into()));
        assert!(matches!(
            workflow(&DecimalWorkflow {
                graph: graph.clone(),
                inputs: vec!["0.0000001".into(), "1".into(), "1".into()]
            }),
            Err(ToolReadError::InvalidInput)
        ));
        let tiny = DecimalGraph {
            input_count: 2,
            steps: vec![
                DecimalStep {
                    operation: DecimalOperation::Product,
                    operands: vec![Input { index: 0 }, Input { index: 0 }],
                },
                DecimalStep {
                    operation: DecimalOperation::Product,
                    operands: vec![Result { index: 0 }, Result { index: 0 }],
                },
                DecimalStep {
                    operation: DecimalOperation::Product,
                    operands: vec![Result { index: 1 }, Result { index: 1 }],
                },
                DecimalStep {
                    operation: DecimalOperation::Sum,
                    operands: vec![Result { index: 2 }, Input { index: 1 }],
                },
            ],
        };
        assert_eq!(
            workflow(&DecimalWorkflow {
                graph: tiny,
                inputs: vec!["0.000001".into(), "0".into()]
            })
            .unwrap()[3]
                .1,
            Ok(format!("0.{}1", "0".repeat(47)))
        );
        let mut bad = graph.clone();
        bad.steps[0].operands[0] = Result { index: 1 };
        assert!(!bad.valid());
        let mut unused = graph;
        unused.input_count = 4;
        assert!(!unused.valid());
    }
}
