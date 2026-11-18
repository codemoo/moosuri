//! Ordered assistant SQL only. Admission floors, journal durability, transaction
//! boundaries, FK policy, quarantine and final integrity checks belong to callers.
#[derive(Clone, Copy)]
pub(crate) struct Migration {
    pub(crate) version: i64,
    pub(crate) sql: &'static str,
}

const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        sql: include_str!("../../../migrations/assistant/0001_accounts_threads.sql"),
    },
    Migration {
        version: 2,
        sql: include_str!("../../../migrations/assistant/0002_providers.sql"),
    },
    Migration {
        version: 3,
        sql: include_str!("../../../migrations/assistant/0003_chat.sql"),
    },
    Migration {
        version: 4,
        sql: include_str!("../../../migrations/assistant/0004_knowledge.sql"),
    },
    Migration {
        version: 5,
        sql: include_str!("../../../migrations/assistant/0005_agent_execution.sql"),
    },
    Migration {
        version: 6,
        sql: include_str!("../../../migrations/assistant/0006_memory_commands.sql"),
    },
    Migration {
        version: 7,
        sql: include_str!("../../../migrations/assistant/0007_gmail.sql"),
    },
    Migration {
        version: 8,
        sql: include_str!("../../../migrations/assistant/0008_browser_workspaces.sql"),
    },
    Migration {
        version: 9,
        sql: include_str!("../../../migrations/assistant/0009_source_connections.sql"),
    },
    Migration {
        version: 10,
        sql: include_str!("../../../migrations/assistant/0010_google_setup.sql"),
    },
    Migration {
        version: 11,
        sql: include_str!("../../../migrations/assistant/0011_community_skills.sql"),
    },
    Migration {
        version: 12,
        sql: include_str!("../../../migrations/assistant/0012_memory_target_resolution.sql"),
    },
    Migration {
        version: 13,
        sql: include_str!("../../../migrations/assistant/0013_stopped_backup_exports.sql"),
    },
    Migration {
        version: 14,
        sql: include_str!("../../../migrations/assistant/0014_automatic_memory.sql"),
    },
    Migration {
        version: 15,
        sql: include_str!("../../../migrations/assistant/0015_destructive_intents.sql"),
    },
    Migration {
        version: 16,
        sql: include_str!("../../../migrations/assistant/0016_personal_artifacts.sql"),
    },
    Migration {
        version: 17,
        sql: include_str!("../../../migrations/assistant/0017_schedules.sql"),
    },
    Migration {
        version: 18,
        sql: include_str!("../../../migrations/assistant/0018_account_management.sql"),
    },
    Migration {
        version: 19,
        sql: include_str!("../../../migrations/assistant/0019_sealed_cutover.sql"),
    },
    Migration {
        version: 20,
        sql: include_str!("../../../migrations/assistant/0020_restore.sql"),
    },
    Migration {
        version: 21,
        sql: include_str!("../../../migrations/assistant/0021_media.sql"),
    },
    Migration {
        version: 22,
        sql: include_str!("../../../migrations/assistant/0022_schedule_chat.sql"),
    },
    Migration {
        version: 23,
        sql: include_str!("../../../migrations/assistant/0023_web_push.sql"),
    },
    Migration {
        version: 24,
        sql: include_str!("../../../migrations/assistant/0024_vault_authority.sql"),
    },
    Migration {
        version: 25,
        sql: include_str!("../../../migrations/assistant/0025_missions.sql"),
    },
    Migration {
        version: 26,
        sql: include_str!("../../../migrations/assistant/0026_calendar.sql"),
    },
    Migration {
        version: 27,
        sql: include_str!("../../../migrations/assistant/0027_private_files.sql"),
    },
    Migration {
        version: 28,
        sql: include_str!("../../../migrations/assistant/0028_recurring_missions.sql"),
    },
    Migration {
        version: 29,
        sql: include_str!("../../../migrations/assistant/0029_mission_chat.sql"),
    },
    Migration {
        version: 30,
        sql: include_str!("../../../migrations/assistant/0030_calendar_conversation.sql"),
    },
    Migration {
        version: 31,
        sql: include_str!("../../../migrations/assistant/0031_mail_reconciliation.sql"),
    },
    Migration {
        version: 32,
        sql: include_str!("../../../migrations/assistant/0032_personal_chatgpt.sql"),
    },
    Migration {
        version: 33,
        sql: include_str!("../../../migrations/assistant/0033_bounded_audio.sql"),
    },
    Migration {
        version: 34,
        sql: include_str!("../../../migrations/assistant/0034_chatgpt_vm_handoff.sql"),
    },
    Migration {
        version: 35,
        sql: include_str!("../../../migrations/assistant/0035_community_catalog_cache.sql"),
    },
    Migration {
        version: 36,
        sql: include_str!("../../../migrations/assistant/0036_personal_chatgpt_tools.sql"),
    },
    Migration {
        version: 37,
        sql: include_str!("../../../migrations/assistant/0037_credential_broker.sql"),
    },
    Migration {
        version: 38,
        sql: include_str!("../../../migrations/assistant/0038_learned_skills.sql"),
    },
    Migration {
        version: 39,
        sql: include_str!("../../../migrations/assistant/0039_notification_topics.sql"),
    },
    Migration {
        version: 40,
        sql: include_str!("../../../migrations/assistant/0040_mail_workflows.sql"),
    },
    Migration {
        version: 41,
        sql: include_str!("../../../migrations/assistant/0041_account_deletion.sql"),
    },
    Migration {
        version: 42,
        sql: include_str!("../../../migrations/assistant/0042_request_continuations.sql"),
    },
    Migration {
        version: 43,
        sql: include_str!("../../../migrations/assistant/0043_invitation_delivery.sql"),
    },
    Migration {
        version: 44,
        sql: include_str!("../../../migrations/assistant/0044_native_credential_adapter.sql"),
    },
    Migration {
        version: 45,
        sql: include_str!("../../../migrations/assistant/0045_credential_chat.sql"),
    },
    Migration {
        version: 46,
        sql: include_str!("../../../migrations/assistant/0046_followup_notifications.sql"),
    },
    Migration {
        version: 47,
        sql: include_str!("../../../migrations/assistant/0047_conversation_recall.sql"),
    },
    Migration {
        version: 48,
        sql: include_str!("../../../migrations/assistant/0048_source_start_recovery.sql"),
    },
    Migration {
        version: 49,
        sql: include_str!("../../../migrations/assistant/0049_recovery_forward_budget.sql"),
    },
    Migration {
        version: 50,
        sql: include_str!("../../../migrations/assistant/0050_personal_claude_code.sql"),
    },
    Migration {
        version: 51,
        sql: include_str!("../../../migrations/assistant/0051_reminder_chat_drafts.sql"),
    },
    Migration {
        version: 52,
        sql: include_str!("../../../migrations/assistant/0052_mission_acceptances.sql"),
    },
    Migration {
        version: 53,
        sql: include_str!("../../../migrations/assistant/0053_browser_recovery.sql"),
    },
    Migration {
        version: 54,
        sql: include_str!("../../../migrations/assistant/0054_external_report_documents.sql"),
    },
    Migration {
        version: 55,
        sql: include_str!("../../../migrations/assistant/0055_learned_page_collections.sql"),
    },
    Migration {
        version: 56,
        sql: include_str!("../../../migrations/assistant/0056_native_mail_send.sql"),
    },
    Migration {
        version: 57,
        sql: include_str!("../../../migrations/assistant/0057_proactive_suggestions.sql"),
    },
    Migration {
        version: 58,
        sql: include_str!("../../../migrations/assistant/0058_planner_step_identity.sql"),
    },
    Migration {
        version: 59,
        sql: include_str!("../../../migrations/assistant/0059_compound_reports.sql"),
    },
    Migration {
        version: 60,
        sql: include_str!("../../../migrations/assistant/0060_connection_waits.sql"),
    },
    Migration {
        version: 61,
        sql: include_str!("../../../migrations/assistant/0061_native_model_mail.sql"),
    },
    Migration {
        version: 62,
        sql: include_str!("../../../migrations/assistant/0062_provider_draft_receipts.sql"),
    },
    Migration {
        version: 63,
        sql: include_str!("../../../migrations/assistant/0063_native_task_mail.sql"),
    },
    Migration {
        version: 64,
        sql: include_str!("../../../migrations/assistant/0064_compound_learning.sql"),
    },
    Migration {
        version: 65,
        sql: include_str!("../../../migrations/assistant/0065_mission_actions.sql"),
    },
    Migration {
        version: 66,
        sql: include_str!("../../../migrations/assistant/0066_selected_browser_missions.sql"),
    },
    Migration {
        version: 67,
        sql: include_str!("../../../migrations/assistant/0067_compound_draft_learning.sql"),
    },
    Migration {
        version: 68,
        sql: include_str!("../../../migrations/assistant/0068_native_mail_settlement_cursor.sql"),
    },
    Migration {
        version: 69,
        sql: include_str!("../../../migrations/assistant/0069_mission_input_waits.sql"),
    },
    Migration {
        version: 70,
        sql: include_str!("../../../migrations/assistant/0070_flexible_mission_inputs.sql"),
    },
    Migration {
        version: 71,
        sql: include_str!("../../../migrations/assistant/0071_thread_creation_receipts.sql"),
    },
    Migration {
        version: 72,
        sql: include_str!("../../../migrations/assistant/0072_run_instructions.sql"),
    },
    Migration {
        version: 73,
        sql: include_str!("../../../migrations/assistant/0073_task_monitors.sql"),
    },
    Migration {
        version: 74,
        sql: include_str!("../../../migrations/assistant/0074_mail_reply_watches.sql"),
    },
    Migration {
        version: 75,
        sql: include_str!("../../../migrations/assistant/0075_telegram.sql"),
    },
    Migration {
        version: 76,
        sql: include_str!("../../../migrations/assistant/0076_claude_model_efforts.sql"),
    },
    Migration {
        version: 77,
        sql: include_str!("../../../migrations/assistant/0077_personal_compatible.sql"),
    },
    Migration {
        version: 78,
        sql: include_str!("../../../migrations/assistant/0078_claude_host_tools.sql"),
    },
    Migration {
        version: 79,
        sql: include_str!("../../../migrations/assistant/0079_subscription_vision.sql"),
    },
];

pub(crate) const CURRENT_VERSION: i64 = MIGRATIONS[MIGRATIONS.len() - 1].version;

pub(crate) fn migration(version: i64) -> Option<&'static Migration> {
    let index = usize::try_from(version.checked_sub(1)?).ok()?;
    MIGRATIONS.get(index).filter(|m| m.version == version)
}

/// Validate the entire registry before returning a caller-supported suffix.
/// A higher supported baseline never silently grants older-schema admission.
pub(crate) fn after(captured: i64, supported_baseline: i64) -> Result<&'static [Migration], ()> {
    suffix(MIGRATIONS, captured, supported_baseline, CURRENT_VERSION)
}

fn suffix(
    migrations: &[Migration],
    captured: i64,
    supported_baseline: i64,
    current: i64,
) -> Result<&[Migration], ()> {
    if supported_baseline < 0
        || captured < supported_baseline
        || captured > current
        || migrations.len() as i64 != current
        || migrations
            .iter()
            .enumerate()
            .any(|(i, m)| m.version != i as i64 + 1)
    {
        return Err(());
    }
    Ok(&migrations[captured as usize..])
}

#[cfg(test)]
pub(crate) mod tests;
