//! Closed built-in tool classification, shared by model exposure and Store admission.
//! This is a static upper bound, never an account, source or effect grant. Each
//! caller must retain its current profile, receipt, scope and lifecycle guards.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToolCapability {
    SavedRead,
    SourceRead,
    SkillRead,
    Calculation,
    SourceCalculation,
    /// Pinned recipes still require current route/argument and nested-effect checks.
    LearnedRecipe,
    PublicRead,
    PageCollection,
    MailRead,
    CalendarRead,
    ConversationRead,
    ArtifactRead,
    ActionStatus,
    ParcelRead,
    Browser,
    NativeCredential,
    NativeMailRead,
    MailDraft,
    MissionEffect,
    ArtifactWrite,
    /// A bounded failed parse may be persisted for model repair; never executes.
    InvalidArguments,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadToolPolicy {
    LiveInstructions,
    Mission,
    TaskMonitor,
}

impl ToolCapability {
    pub const fn allowed(self, policy: ReadToolPolicy) -> bool {
        use ToolCapability::*;
        match policy {
            ReadToolPolicy::LiveInstructions => matches!(
                self,
                SavedRead
                    | SourceRead
                    | SkillRead
                    | Calculation
                    | SourceCalculation
                    | LearnedRecipe
                    | PublicRead
                    | PageCollection
                    | MailRead
                    | CalendarRead
                    | ConversationRead
                    | ArtifactRead
                    | ActionStatus
                    | ParcelRead
                    | InvalidArguments
            ),
            ReadToolPolicy::Mission => matches!(
                self,
                SavedRead
                    | SourceRead
                    | SkillRead
                    | Calculation
                    | SourceCalculation
                    | LearnedRecipe
                    | PublicRead
                    | PageCollection
                    | MailRead
                    | CalendarRead
                    | NativeMailRead
                    | MissionEffect
            ),
            // Source identity, revision, byte range and aggregate budget remain
            // authoritative in task_monitors; ordinary saved search is excluded.
            ReadToolPolicy::TaskMonitor => matches!(self, SourceRead | Calculation),
        }
    }
}

impl crate::ExternalToolRequest {
    pub fn capability(&self) -> ToolCapability {
        use crate::{
            CalendarReadRequest as Calendar, MailToolRequest as Mail, WebToolRequest as Web,
        };
        match self {
            Self::Mail(
                Mail::Search { .. }
                | Mail::Read { .. }
                | Mail::Thread { .. }
                | Mail::Attachment { .. }
                | Mail::Attachments { .. },
            ) => ToolCapability::MailRead,
            Self::Mail(Mail::Draft { .. }) => ToolCapability::MailDraft,
            Self::Calendar(Calendar::List { .. } | Calendar::Read { .. }) => {
                ToolCapability::CalendarRead
            }
            Self::Calendar(Calendar::Status { .. }) => ToolCapability::ActionStatus,
            Self::Web(Web::Read { .. } | Web::Search { .. }) => ToolCapability::PublicRead,
            Self::Web(Web::Parcel { .. } | Web::ParcelHistory { .. }) => ToolCapability::ParcelRead,
            Self::Web(
                Web::BrowserRead { .. }
                | Web::BrowserOpen { .. }
                | Web::BrowserObserve { .. }
                | Web::BrowserHandoff { .. },
            ) => ToolCapability::Browser,
        }
    }
}

impl crate::ArtifactToolRequest {
    pub fn capability(&self) -> ToolCapability {
        match self {
            Self::Search { .. } | Self::Read { .. } | Self::Briefing { .. } => {
                ToolCapability::ArtifactRead
            }
            Self::Write { .. } => ToolCapability::ArtifactWrite,
        }
    }
}

/// Unknown/new tool names fail closed until explicitly classified here.
pub fn tool_capability(name: &str) -> Option<ToolCapability> {
    use ToolCapability::*;
    Some(match name {
        "knowledge_search" | "knowledge_read_memory" => SavedRead,
        "knowledge_read_source" => SourceRead,
        "skill_read" => SkillRead,
        "decimal_calculate" | "decimal_workflow" => Calculation,
        "source_calculate" | "evidence_calculate" => SourceCalculation,
        "learned_skill_execute" => LearnedRecipe,
        "web_read" | "web_search" => PublicRead,
        "public_pages_collect" => PageCollection,
        "mail_search" | "mail_read" | "mail_thread" | "mail_attachment" | "mail_attachments" => {
            MailRead
        }
        "calendar_list_events" | "calendar_read_event" => CalendarRead,
        "conversation_search" | "conversation_read" => ConversationRead,
        "artifact_search" | "artifact_read" | "followup_briefing" => ArtifactRead,
        "calendar_action_status" => ActionStatus,
        "parcel_track" | "parcel_history" => ParcelRead,
        "browser_read" | "browser_open" | "browser_observe" | "browser_handoff" => Browser,
        "github_repository_metadata"
        | "gitea_repository_issues"
        | "google_inbox_counts"
        | "google_inbox_preview" => NativeCredential,
        "mail_prepare_draft" => MailDraft,
        "mission_prepare_mail" | "mission_save_provider_draft" | "mission_calendar_action" => {
            MissionEffect
        }
        "artifact_write" => ArtifactWrite,
        _ => return None,
    })
}

pub fn tool_policy_allows(name: &str, policy: ReadToolPolicy) -> bool {
    tool_capability(name).is_some_and(|capability| capability.allowed(policy))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_surfaces_keep_effects_native_routes_and_unknown_tools_out() {
        for name in [
            "mail_prepare_draft",
            "mission_prepare_mail",
            "mission_save_provider_draft",
            "mission_calendar_action",
            "artifact_write",
            "github_repository_metadata",
            "gitea_repository_issues",
            "google_inbox_counts",
            "google_inbox_preview",
            "browser_read",
            "browser_open",
            "browser_observe",
            "browser_handoff",
            "mail_send",
            "knowledge_future_tool",
        ] {
            assert!(
                !tool_policy_allows(name, ReadToolPolicy::LiveInstructions),
                "{name}"
            );
        }
        for name in [
            "knowledge_search",
            "knowledge_read_source",
            "decimal_workflow",
            "skill_read",
            "learned_skill_execute",
            "mail_search",
            "calendar_read_event",
            "web_read",
            "conversation_read",
            "artifact_read",
            "parcel_history",
        ] {
            assert!(
                tool_policy_allows(name, ReadToolPolicy::LiveInstructions),
                "{name}"
            );
        }
    }

    #[test]
    fn monitor_and_mission_are_distinct_upper_bounds() {
        for name in [
            "knowledge_read_source",
            "decimal_calculate",
            "decimal_workflow",
        ] {
            assert!(
                tool_policy_allows(name, ReadToolPolicy::TaskMonitor),
                "{name}"
            );
        }
        for name in [
            "knowledge_search",
            "source_calculate",
            "evidence_calculate",
            "skill_read",
            "learned_skill_execute",
            "web_read",
            "mail_read",
            "calendar_read_event",
            "artifact_write",
            "mission_prepare_mail",
        ] {
            assert!(
                !tool_policy_allows(name, ReadToolPolicy::TaskMonitor),
                "{name}"
            );
        }
        for name in [
            "parcel_track",
            "parcel_history",
            "conversation_read",
            "artifact_read",
            "calendar_action_status",
            "browser_read",
            "google_inbox_preview",
        ] {
            assert!(!tool_policy_allows(name, ReadToolPolicy::Mission), "{name}");
        }
        assert!(tool_policy_allows(
            "mission_prepare_mail",
            ReadToolPolicy::Mission
        ));
        assert!(tool_policy_allows("mail_read", ReadToolPolicy::Mission));
        assert!(!tool_policy_allows(
            "mail_prepare_draft",
            ReadToolPolicy::Mission
        ));
    }
}
