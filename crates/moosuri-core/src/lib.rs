//! Public API contracts. No database connections, authentication secrets or IO.
mod push;
pub use push::*;

mod missions;
pub use missions::*;
mod schedules;
pub use schedules::*;
mod calendar;
pub use calendar::*;
mod accounts;
pub use accounts::*;
mod automatic_memory;
pub use automatic_memory::*;
mod artifacts;
pub use artifacts::*;
mod community_skills;
pub use community_skills::*;
mod google_setup;
pub use google_setup::*;
mod source_connections;
pub use source_connections::*;
mod web;
pub use web::*;
mod browser;
pub use browser::*;
mod mail;
pub use mail::*;
mod media;
pub use media::*;
mod chat;
mod knowledge;
mod memory_command;
mod skills;
pub use knowledge::*;
pub use memory_command::*;
pub use skills::*;
mod providers;
pub use chat::*;
pub use providers::*;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Admin,
    Member,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountSummary {
    pub id: String,
    pub name: String,
    pub email: String,
    pub role: Role,
    pub must_change_password: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SessionView {
    pub user: AccountSummary,
    pub password_change_required: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct SystemStatus {
    pub version: String,
    pub initialized: bool,
    pub stage: String,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ThreadSummary {
    pub id: String,
    pub title: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ThreadsResponse {
    pub threads: Vec<ThreadSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct UsersResponse {
    pub users: Vec<AccountSummary>,
}
#[derive(Clone, Debug, Serialize, Deserialize, TS)]
pub struct ApiError {
    pub code: String,
}

/// Generated for the React client; request secrets deliberately have no Debug DTO.
pub fn typescript_contract() -> String {
    let declarations = [
        PushDeviceView::decl(),
        PushConfiguration::decl(),
        ScheduleRule::decl(),
        ScheduleTarget::decl(),
        ScheduleChatContext::decl(),
        ScheduleChatReceipt::decl(),
        ScheduleDraft::decl(),
        ScheduleCreate::decl(),
        ScheduleEdit::decl(),
        ScheduleControl::decl(),
        ScheduleView::decl(),
        SchedulePreview::decl(),
        ScheduleOccurrence::decl(),
        ScheduleSkippedRange::decl(),
        ScheduleHistory::decl(),
        QuietHours::decl(),
        QuietHoursChange::decl(),
        LocalNotification::decl(),
        NotificationPage::decl(),
        CalendarConnection::decl(),
        CalendarConnections::decl(),
        CalendarInfo::decl(),
        CalendarList::decl(),
        CalendarFields::decl(),
        CalendarEvent::decl(),
        CalendarEvents::decl(),
        CalendarRange::decl(),
        CalendarMutation::decl(),
        CalendarChange::decl(),
        CalendarAction::decl(),
        AccountStatus::decl(),
        ManagedAccount::decl(),
        AccountManagementAction::decl(),
        AccountManagementInput::decl(),
        AccountManagementReceipt::decl(),
        MediaRef::decl(),
        MediaSummary::decl(),
        MediaLibrary::decl(),
        GitHubSkillLocation::decl(),
        GitHubSkillPreviewInput::decl(),
        MemoryExtractionChange::decl(),
        MemoryExtractionStatus::decl(),
        MemoryExtractionOrigin::decl(),
        MissionMode::decl(),
        MissionCheck::decl(),
        MissionCreate::decl(),
        MissionControl::decl(),
        MissionAction::decl(),
        MissionEvidence::decl(),
        MissionCandidate::decl(),
        MissionView::decl(),
        MissionReceipt::decl(),
        MissionChatCandidate::decl(),
        MissionChatTarget::decl(),
        MissionChatContext::decl(),
        MissionChatReceipt::decl(),
        MissionChatDecision::decl(),
        CalendarChatReceipt::decl(),
        MissionChatTime::decl(),
        RecurringMissionCreate::decl(),
        RecurringMissionView::decl(),
        RecurringMissionOccurrence::decl(),
        ArtifactKind::decl(),
        ArtifactStatus::decl(),
        GoalLifecycle::decl(),
        FollowupKind::decl(),
        FollowupLifecycle::decl(),
        FollowupProgress::decl(),
        ArtifactDate::decl(),
        ArtifactPayload::decl(),
        ArtifactDependencyKind::decl(),
        ArtifactDependency::decl(),
        ArtifactDraft::decl(),
        ArtifactWrite::decl(),
        ArtifactDelete::decl(),
        ArtifactSummary::decl(),
        ArtifactVersion::decl(),
        ArtifactReceipt::decl(),
        ArtifactPage::decl(),
        ArtifactHistory::decl(),
        CommunitySkillCard::decl(),
        CommunitySkillMutation::decl(),
        GoogleSetupStatus::decl(),
        GoogleSetupValidation::decl(),
        GoogleConnectionCheck::decl(),
        BrowserWorkspaceState::decl(),
        BrowserWorkspaceView::decl(),
        BrowserWorkspaceStatus::decl(),
        BrowserControl::decl(),
        BrowserFrame::decl(),
        MailConnection::decl(),
        MailConnections::decl(),
        MailReply::decl(),
        MailDraftContent::decl(),
        MailDraftInput::decl(),
        MailAction::decl(),
        MailReconciliation::decl(),
        MailApproval::decl(),
        Role::decl(),
        AccountSummary::decl(),
        SessionView::decl(),
        SystemStatus::decl(),
        ThreadSummary::decl(),
        ThreadsResponse::decl(),
        UsersResponse::decl(),
        ApiError::decl(),
        ProviderKind::decl(),
        ProviderConfig::decl(),
        ModelSelection::decl(),
        ModelChoice::decl(),
        ProviderSummary::decl(),
        ProvidersResponse::decl(),
        ModelSettings::decl(),
        ModelCatalogEntry::decl(),
        ModelCatalog::decl(),
        RunState::decl(),
        MessagePhase::decl(),
        RunUsage::decl(),
        AssistantMessage::decl(),
        ChatEvidence::decl(),
        WebEvidenceKind::decl(),
        WebEvidence::decl(),
        RunView::decl(),
        ChatTurn::decl(),
        ChatPage::decl(),
        ChatChange::decl(),
        ChatEvent::decl(),
        ChatEvents::decl(),
        UsageSummary::decl(),
        AccountUsage::decl(),
        AccountUsageResponse::decl(),
        SourceConnectionKind::decl(),
        CreateSourceConnection::decl(),
        SourceConnection::decl(),
        SourceSyncRun::decl(),
        SourceRunOutcome::decl(),
        SourceBatch::decl(),
        SourceBatchReceipt::decl(),
        SourceIngestStatus::decl(),
        SourceProvenance::decl(),
        SourceObjectMetadata::decl(),
        DisconnectMode::decl(),
        KnowledgeKind::decl(),
        MemoryStatus::decl(),
        KnowledgeCollection::decl(),
        SourceInput::decl(),
        IngestBatch::decl(),
        IngestReceipt::decl(),
        SourceSummary::decl(),
        SourceDocument::decl(),
        SourcePage::decl(),
        EvidenceRef::decl(),
        MemoryDraft::decl(),
        MemoryDocument::decl(),
        MemoryPage::decl(),
        MemoryHistory::decl(),
        KnowledgeQuery::decl(),
        KnowledgeHit::decl(),
        KnowledgeSearch::decl(),
    ];
    let mut result = String::from("// Generated by moosuri export-types. Do not edit.\n");
    for declaration in declarations {
        result.push_str("export ");
        result.push_str(&declaration);
        result.push('\n');
    }
    result
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
