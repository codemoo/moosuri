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

pub(crate) fn current_skill(profile: Option<&RunToolProfile>, id: &str) -> bool {
    BUNDLED_SKILLS
        .iter()
        .find(|s| s.id == id)
        .is_some_and(|s| profile.is_some_and(|p| p.skills.contains(&pin(s))))
}
pub(crate) fn profile() -> RunToolProfile {
    RunToolProfile {
        credential_github: false,
        credential_google: false,
        credential_google_preview: false,
        conversation_recall: false,
        mission_reads: None,
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
    if available && requested.id.starts_with("private:") {
        return crate::learned_skills::read(conn, run, requested)
            .ok()
            .flatten()
            .map(KnowledgeToolResult::Skill)
            .unwrap_or(KnowledgeToolResult::Error(ToolReadError::Unavailable));
    }
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
