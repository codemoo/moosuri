//! Bundled instruction recipes. These descriptions grant no tool authority.
//! The execution profile is server-generated private state, not a browser DTO.
use serde::{Deserialize, Serialize};

pub const BUILTIN_TOOLSET_VERSION: u32 = 1;

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillPin {
    pub id: String,
    pub revision: String,
    pub digest: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunToolProfile {
    pub version: u32,
    pub skills: Vec<SkillPin>,
}

pub struct BundledSkill {
    pub id: &'static str,
    pub revision: &'static str,
    pub description: &'static str,
    pub instructions: &'static str,
}

pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        id: "source-briefing",
        revision: "1.0.0",
        description: "Prepare a concise grounded briefing from saved sources and memories.",
        instructions: include_str!("../../../community/skills/source-briefing/SKILL.md"),
    },
    BundledSkill {
        id: "meeting-preparation",
        revision: "1.0.0",
        description: "Prepare an agenda, questions and decisions from saved meeting context.",
        instructions: include_str!("../../../community/skills/meeting-preparation/SKILL.md"),
    },
    BundledSkill {
        id: "evidence-comparison",
        revision: "1.0.0",
        description: "Compare alternatives using saved evidence and exact arithmetic when useful.",
        instructions: include_str!("../../../community/skills/evidence-comparison/SKILL.md"),
    },
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillDocument {
    pub pin: SkillPin,
    pub instructions: String,
}

#[derive(Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DecimalOperation {
    Sum,
    Difference,
    Product,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecimalCalculation {
    pub operation: DecimalOperation,
    pub operands: Vec<String>,
}
