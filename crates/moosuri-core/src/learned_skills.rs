//! Private parameterized routes, never executable code or imported instructions.
use serde::{Deserialize, Serialize};
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum LearnedSkillArguments {
    Source { text: String },
    Decimal { operands: Vec<String> },
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LearnedSkillInvocation {
    pub pin: crate::SkillPin,
    pub arguments: LearnedSkillArguments,
}
