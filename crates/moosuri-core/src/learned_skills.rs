//! Private parameterized routes, never executable code or imported instructions.
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LearnedSkillArguments {
    Pages {
        urls: Vec<String>,
    },
    Source {
        text: String,
    },
    Decimal {
        operands: Vec<String>,
    },
    Workflow {
        inputs: Vec<String>,
    },
    Evidence {
        parameters: EvidenceParameters,
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
        self.valid_from(2)
    }
    fn valid_from(&self, minimum: usize) -> bool {
        if !(2..=32).contains(&self.input_count)
            || !(minimum..=4).contains(&self.steps.len())
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

/// An exact nonempty UTF-8 byte range in this run's current user message.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CurrentSpan {
    pub start_byte: u32,
    pub end_byte: u32,
}
/// Addresses only a completed earlier tool slot in the same run.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolReceiptReference {
    pub step: u32,
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ordinal: Option<u8>,
}
#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvidenceOrigin {
    Source,
    Current,
}
/// Structure only: one source number, fresh current-message scalars and exact DAG.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidencePlan {
    pub origins: Vec<EvidenceOrigin>,
    pub graph: DecimalGraph,
}
impl EvidencePlan {
    pub fn valid(&self) -> bool {
        self.graph.valid_from(1)
            && self.origins.len() == usize::from(self.graph.input_count)
            && self
                .origins
                .iter()
                .filter(|v| matches!(v, EvidenceOrigin::Source))
                .count()
                == 1
    }
    pub fn canonical(&self) -> Option<Self> {
        if !self.valid() {
            return None;
        }
        let mut plan = self.clone();
        for step in &mut plan.graph.steps {
            if matches!(
                step.operation,
                crate::DecimalOperation::Sum | crate::DecimalOperation::Product
            ) {
                step.operands.sort();
            }
        }
        Some(plan)
    }
}
/// Exact quotes in the current user message. The server resolves unique ranges.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceParameters {
    pub query: String,
    pub prefix: String,
    pub scalars: Vec<String>,
}
/// Fresh quote parameters plus actual earlier read receipt addresses.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceBinding {
    pub search: ToolReceiptReference,
    pub source: ToolReceiptReference,
    pub query: String,
    pub prefix: String,
    pub scalars: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceCalculation {
    pub plan: EvidencePlan,
    pub binding: EvidenceBinding,
}
/// Server-derived provenance. The actual arithmetic operations have child receipts.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EvidenceDecimalReceipt {
    pub source_id: String,
    pub source_revision: i64,
    pub source_title: String,
    pub start_byte: u32,
    pub end_byte: u32,
    pub literal: String,
    pub current_query: CurrentSpan,
    pub current_prefix: CurrentSpan,
    pub current_scalars: Vec<CurrentSpan>,
    pub search: ToolReceiptReference,
    pub source: ToolReceiptReference,
}

/// Structural read→exact calculation→requested report recipe. No remembered values,
/// account/resource bindings, credentials, approval or send operations can occur.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundReportRecipe {
    pub schema_version: u32,
    pub revision: u32,
    pub calculation: EvidencePlan,
}
impl CompoundReportRecipe {
    pub fn valid(&self) -> bool {
        self.schema_version == 1
            && (1..=1_000_000).contains(&self.revision)
            && self.calculation.valid()
            && self.node_count() <= 8
    }
    /// Search + source read + each exact arithmetic leaf + requested document.
    pub fn node_count(&self) -> usize {
        3 + self.calculation.graph.steps.len()
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundReportAdoption {
    pub artifact_id: String,
    pub artifact_revision: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundRecipeEdit {
    pub expected_revision: i64,
    pub calculation: EvidencePlan,
}

/// Owner adoption of one original local draft result, never send approval.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompoundDraftAdoption {
    pub action_id: String,
    pub revision: i64,
    pub digest: String,
}
