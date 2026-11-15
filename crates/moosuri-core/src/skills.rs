//! Bundled instruction recipes. These descriptions grant no tool authority.
//! The execution profile is server-generated private state, not a browser DTO.
use serde::{Deserialize, Serialize};

pub const BUILTIN_TOOLSET_VERSION: u32 = 3;

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
    #[serde(default, skip_serializing_if = "is_false")]
    pub native_mail_model_read: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub native_mail_model_background: bool,
    pub version: u32,
    #[serde(default, skip_serializing_if = "is_false")]
    pub evidence_calculation: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub credential_github: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub credential_google: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub credential_google_preview: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub conversation_recall: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mission_reads: Option<ConnectedMissionReads>,
    pub skills: Vec<SkillPin>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub community_metadata: Vec<SkillMetadata>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mail_connections: Vec<crate::MailConnectionPin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub web: Option<crate::WebToolProfile>,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SkillMetadata {
    pub pin: SkillPin,
    pub description: String,
}

pub struct BundledSkill {
    pub id: &'static str,
    pub revision: &'static str,
    pub description: &'static str,
    pub instructions: &'static str,
}

pub const BUNDLED_SKILLS: &[BundledSkill] = &[
    BundledSkill {
        id: "parcel-tracking",
        revision: "1.1.0",
        description: "Read Japan Post latest status or official visible shipment history with actual source/time and same-thread tracking receipt reuse.",
        instructions: include_str!("../../../community/skills/parcel-tracking/SKILL.md"),
    },
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
    BundledSkill {
        id: "web-research",
        revision: "1.0.0",
        description: "Research public options with actual sources, date checks and exact supported cost totals.",
        instructions: include_str!("../../../community/skills/web-research/SKILL.md"),
    },
    BundledSkill {
        id: "inbox-briefing",
        revision: "1.0.0",
        description: "Brief bounded personal Gmail results into priorities, deadlines and next actions.",
        instructions: include_str!("../../../community/skills/inbox-briefing/SKILL.md"),
    },
    BundledSkill {
        id: "mail-reply",
        revision: "1.0.0",
        description: "Read the correct Gmail conversation and prepare a real local reply draft for mandatory final human approval.",
        instructions: include_str!("../../../community/skills/mail-reply/SKILL.md"),
    },
    BundledSkill {
        id: "public-page-comparison",
        revision: "1.0.0",
        description: "Collect two to four explicit current-task public pages and compare actual observations with citations and exact short quotes.",
        instructions: include_str!("../../../community/skills/public-page-comparison/SKILL.md"),
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

fn is_false(v: &bool) -> bool {
    !*v
}

/// Immutable owner delegation snapshot, private model state; no session/credential.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ConnectedMissionReads {
    pub app_revision: Option<i64>,
    pub mail: Vec<MissionMailPin>,
    pub calendar: Vec<crate::CalendarAgentPin>,
    pub primary_ids: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MissionMailPin {
    pub connection: crate::MailConnectionPin,
    pub client_id: String,
}
/// Domain names in the immutable owner task, not a model-provided grant or time parser.
pub fn mission_read_domains(text: &str) -> (bool, bool) {
    let lower = text.to_lowercase();
    let words = lower
        .split(|c: char| !c.is_ascii_alphabetic())
        .collect::<Vec<_>>();
    (
        words
            .iter()
            .any(|w| matches!(*w, "gmail" | "mail" | "email" | "mailbox"))
            || lower.contains("메일"),
        words.contains(&"calendar")
            || lower.contains("캘린더")
            || lower
                .replace("일정 간격", "")
                .replace("일정한", "")
                .contains("일정"),
    )
}
