use super::*;
use moosuri_core::{
    BUILTIN_TOOLSET_VERSION, BUNDLED_SKILLS, RunToolProfile, SkillDocument, SkillPin,
};

fn pin(skill: &moosuri_core::BundledSkill) -> SkillPin {
    SkillPin {
        id: skill.id.into(),
        revision: skill.revision.into(),
        digest: format!("{:x}", Sha256::digest(skill.instructions.as_bytes())),
    }
}

pub(crate) fn profile() -> RunToolProfile {
    RunToolProfile {
        version: BUILTIN_TOOLSET_VERSION,
        skills: BUNDLED_SKILLS.iter().map(pin).collect(),
        community_metadata: vec![],
        mail_connections: vec![],
        web: None,
    }
}

pub(crate) fn read(conn: &Connection, run: &Run, requested: &SkillPin) -> KnowledgeToolResult {
    let available = run.tool_profile.as_ref().is_some_and(|profile| {
        matches!(profile.version, 1..=5) && profile.skills.contains(requested)
    });
    if available
        && let Some(document) = crate::community_skills::read(conn, requested)
            .ok()
            .flatten()
    {
        return KnowledgeToolResult::Skill(document);
    }
    if available && let Some(skill) = BUNDLED_SKILLS.iter().find(|skill| pin(skill) == *requested) {
        return KnowledgeToolResult::Skill(SkillDocument {
            pin: requested.clone(),
            instructions: skill.instructions.into(),
        });
    }
    KnowledgeToolResult::Error(ToolReadError::Unavailable)
}
