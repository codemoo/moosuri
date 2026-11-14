//! Private parameterized routes, never executable code or imported instructions.
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LearnedSkillArguments {
    Source {
        text: String,
    },
    Decimal {
        operands: Vec<String>,
    },
    Workflow {
        inputs: Vec<String>,
    },
    /// Japan Post only; must match one explicit number in the current request.
    Parcel {
        tracking_number: String,
    },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedSkillInvocation {
    pub pin: crate::SkillPin,
    pub arguments: LearnedSkillArguments,
}

/// Typed local arithmetic graph. Numeric values live only in fresh workflow inputs.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum DecimalReference {
    Input { index: u8 },
    Result { index: u8 },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecimalStep {
    pub operation: crate::DecimalOperation,
    pub operands: Vec<DecimalReference>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecimalGraph {
    pub input_count: u8,
    pub steps: Vec<DecimalStep>,
}
impl DecimalGraph {
    /// Every input/operation must contribute to the final result. Numeric equality
    /// never establishes an edge: only an explicit prior-result reference does.
    pub fn valid(&self) -> bool {
        if !(2..=32).contains(&self.input_count)
            || !(2..=4).contains(&self.steps.len())
            || self.steps.iter().map(|s| s.operands.len()).sum::<usize>() > 32
        {
            return false;
        }
        for (at, step) in self.steps.iter().enumerate() {
            if !(2..=32).contains(&step.operands.len())
                || matches!(step.operation, crate::DecimalOperation::Product)
                    && step.operands.len() != 2
            {
                return false;
            }
            if step.operands.iter().any(|r| match r {
                DecimalReference::Input { index } => *index >= self.input_count,
                DecimalReference::Result { index } => usize::from(*index) >= at,
            }) {
                return false;
            }
        }
        let mut needed = vec![false; self.steps.len()];
        *needed.last_mut().unwrap() = true;
        let mut inputs = vec![false; usize::from(self.input_count)];
        for at in (0..self.steps.len()).rev() {
            if needed[at] {
                for r in &self.steps[at].operands {
                    match r {
                        DecimalReference::Input { index } => inputs[usize::from(*index)] = true,
                        DecimalReference::Result { index } => needed[usize::from(*index)] = true,
                    }
                }
            }
        }
        needed.into_iter().all(|v| v) && inputs.into_iter().all(|v| v)
    }
    pub fn canonical(&self) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let mut graph = self.clone();
        for step in &mut graph.steps {
            if matches!(
                step.operation,
                crate::DecimalOperation::Sum | crate::DecimalOperation::Product
            ) {
                step.operands.sort();
            }
        }
        Some(graph)
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecimalWorkflow {
    pub graph: DecimalGraph,
    pub inputs: Vec<String>,
}
