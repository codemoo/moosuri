//! Fixed feature suites over existing user-result worlds. No runtime/provider inputs.
use crate::{
    Counts, Row, aggregate, grader,
    world::{self, Case, Kind},
};
use serde::Serialize;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Feature {
    Chat,
    Memory,
    Tasks,
    Skills,
    Schedules,
    Monitors,
}
pub const FEATURES: [Feature; 6] = [
    Feature::Chat,
    Feature::Memory,
    Feature::Tasks,
    Feature::Skills,
    Feature::Schedules,
    Feature::Monitors,
];
impl std::str::FromStr for Feature {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "chat" => Ok(Self::Chat),
            "memory" => Ok(Self::Memory),
            "tasks" => Ok(Self::Tasks),
            "skills" => Ok(Self::Skills),
            "schedules" => Ok(Self::Schedules),
            "monitors" => Ok(Self::Monitors),
            _ => Err("unknown_feature"),
        }
    }
}
pub(crate) fn feature(kind: Kind) -> Feature {
    match kind {
        Kind::Disconnect | Kind::Truncated | Kind::EverydayGuidance => Feature::Chat,
        Kind::Memory | Kind::ChatMemory | Kind::TransitiveForget => Feature::Memory,
        Kind::Mission | Kind::CancelMission | Kind::EverydayClarification | Kind::RepairChoice => {
            Feature::Tasks
        }
        Kind::Calculation | Kind::SkilledCalculation | Kind::LearnedWorkflow => Feature::Skills,
        Kind::EverydayReminder => Feature::Schedules,
        Kind::FollowupMonitor => Feature::Monitors,
    }
}
pub(crate) fn selected(case: &Case, selection: Option<Feature>) -> bool {
    selection.is_none_or(|value| feature(case.kind) == value)
}
pub(crate) const REQUIRED: &[(Option<Feature>, &str)] = &[
    (Some(Feature::Tasks), "repair_choice_positive_control"),
    (
        Some(Feature::Tasks),
        "repair_choice_missing_live_settlement_rejected",
    ),
    (
        Some(Feature::Tasks),
        "repair_choice_accepted_ambiguous_answer_rejected",
    ),
    (Some(Feature::Tasks), "repair_choice_replaced_wait_rejected"),
    (
        Some(Feature::Tasks),
        "repair_choice_forged_candidate_rejected",
    ),
    (Some(Feature::Tasks), "repair_choice_repeated_read_rejected"),
    (
        Some(Feature::Tasks),
        "repair_choice_duplicate_reply_rejected",
    ),
    (Some(Feature::Tasks), "repair_choice_late_result_rejected"),
    (
        Some(Feature::Tasks),
        "repair_choice_visible_notice_rejected",
    ),
    (Some(Feature::Monitors), "followup_positive_control"),
    (Some(Feature::Monitors), "followup_duplicate_child_rejected"),
    (
        Some(Feature::Monitors),
        "followup_wrong_source_scope_rejected",
    ),
    (
        Some(Feature::Monitors),
        "followup_missing_changed_read_rejected",
    ),
    (
        Some(Feature::Monitors),
        "followup_visible_stopped_notice_rejected",
    ),
    (Some(Feature::Monitors), "followup_revived_policy_rejected"),
    (
        Some(Feature::Monitors),
        "followup_wrong_visible_mission_link_rejected",
    ),
    (
        Some(Feature::Monitors),
        "followup_wrong_visible_schedule_link_rejected",
    ),
    (
        Some(Feature::Monitors),
        "followup_wrong_visible_occurrence_link_rejected",
    ),
    (
        Some(Feature::Monitors),
        "followup_contradictory_paid_status_rejected",
    ),
    (
        Some(Feature::Monitors),
        "followup_negated_approval_rejected",
    ),
    (
        Some(Feature::Chat),
        "everyday_guidance_missing_receipt_rejected",
    ),
    (
        Some(Feature::Chat),
        "everyday_guidance_repeated_tool_rejected",
    ),
    (
        Some(Feature::Tasks),
        "everyday_clarification_lost_work_rejected",
    ),
    (
        Some(Feature::Tasks),
        "everyday_clarification_false_achievement_rejected",
    ),
    (
        Some(Feature::Schedules),
        "everyday_reminder_wrong_target_rejected",
    ),
    (
        Some(Feature::Schedules),
        "everyday_reminder_duplicate_replay_rejected",
    ),
    (
        Some(Feature::Schedules),
        "everyday_reminder_wrong_time_rejected",
    ),
    (
        Some(Feature::Schedules),
        "everyday_reminder_missing_tombstone_rejected",
    ),
    (Some(Feature::Skills), "positive_control"),
    (Some(Feature::Skills), "negative_total_rejected"),
    (Some(Feature::Skills), "unicode_negative_total_rejected"),
    (Some(Feature::Tasks), "mission_positive_control"),
    (
        Some(Feature::Tasks),
        "candidate_missing_source_receipt_rejected",
    ),
    (
        Some(Feature::Tasks),
        "candidate_missing_calculation_receipt_rejected",
    ),
    (
        Some(Feature::Tasks),
        "candidate_missing_required_quote_rejected",
    ),
    (Some(Feature::Skills), "false_success_claim_rejected"),
    (Some(Feature::Skills), "missing_tool_receipt_rejected"),
    (Some(Feature::Skills), "wrong_tool_receipt_rejected"),
    (Some(Feature::Skills), "duplicate_reused_receipt_rejected"),
    (Some(Feature::Skills), "other_owner_evidence_rejected"),
    (
        Some(Feature::Skills),
        "unread_other_owner_candidate_rejected",
    ),
    (Some(Feature::Skills), "finalization_interruption_rejected"),
    (
        Some(Feature::Skills),
        "actual_skill_read_only_claim_rejected",
    ),
    (None, "setup_failure_retained_in_denominator"),
    (
        Some(Feature::Memory),
        "chat_correction_wrong_identity_rejected",
    ),
    (
        Some(Feature::Memory),
        "forget_transitive_checkpoint_residue_rejected",
    ),
    (Some(Feature::Tasks), "cancel_late_success_rejected"),
    (Some(Feature::Skills), "learned_duplicate_proof_rejected"),
    (
        Some(Feature::Skills),
        "learned_missing_nested_receipts_rejected",
    ),
    (
        Some(Feature::Skills),
        "learned_withdrawn_output_residue_rejected",
    ),
    (
        Some(Feature::Memory),
        "chat_success_without_command_receipt_rejected",
    ),
    (
        Some(Feature::Memory),
        "forget_unrelated_answer_loss_rejected",
    ),
    (
        Some(Feature::Tasks),
        "cancel_false_certain_outcome_rejected",
    ),
    (Some(Feature::Skills), "learned_foreign_proof_rejected"),
    (
        Some(Feature::Skills),
        "learned_stale_nested_arguments_rejected",
    ),
];
pub(crate) fn validations_pass(selection: Option<Feature>, checks: &[grader::Check]) -> bool {
    grader::all(checks)
        && REQUIRED
            .iter()
            .filter(|(owner, _)| selection.is_none() || owner.is_none() || *owner == selection)
            .all(|(_, name)| {
                checks
                    .iter()
                    .any(|check| check.name == *name && check.passed)
            })
}
#[derive(Serialize)]
pub struct Selection {
    pub feature: Option<Feature>,
    pub full_suite: bool,
    pub selected_case_ids: Vec<&'static str>,
    pub excluded_case_ids: Vec<&'static str>,
}
pub(crate) fn selection(value: Option<Feature>) -> Selection {
    Selection {
        feature: value,
        full_suite: value.is_none(),
        selected_case_ids: world::CASES
            .iter()
            .filter(|c| selected(c, value))
            .map(|c| c.id)
            .collect(),
        excluded_case_ids: world::CASES
            .iter()
            .filter(|c| !selected(c, value))
            .map(|c| c.id)
            .collect(),
    }
}
#[derive(Serialize)]
pub struct FeatureResult {
    pub feature: Feature,
    pub status: &'static str,
    pub available_cases: usize,
    pub counts: Counts,
}
pub(crate) fn summarize(
    selection: Option<Feature>,
    rows: &[Row],
    validation_pass: bool,
) -> Vec<FeatureResult> {
    FEATURES
        .into_iter()
        .map(|feature| {
            let mut expected = world::CASES
                .iter()
                .filter(|c| self::feature(c.kind) == feature)
                .map(|c| c.id)
                .collect::<Vec<_>>();
            expected.sort_unstable();
            let selected = selection.is_none_or(|value| value == feature);
            let group = rows
                .iter()
                .filter(|r| r.feature == feature)
                .collect::<Vec<_>>();
            let mut counts = aggregate(group.iter().copied());
            let mut observed = group.iter().map(|r| r.case_id.as_str()).collect::<Vec<_>>();
            observed.sort_unstable();
            let exact = observed == expected;
            if selected {
                counts.planned = expected.len();
                counts.not_run += expected.iter().filter(|id| !observed.contains(id)).count();
            }
            let status = if !selected {
                "not_selected"
            } else if !exact || counts.pass != counts.planned {
                "fail"
            } else if !validation_pass {
                "unverified"
            } else {
                "pass"
            };
            FeatureResult {
                feature,
                status,
                available_cases: expected.len(),
                counts,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pending;

    #[test]
    fn selected_failure_cannot_shrink_denominator_or_claim_other_features_passed() {
        let mut rows = world::CASES
            .iter()
            .filter(|c| selected(c, Some(Feature::Memory)))
            .map(|c| pending(*c))
            .collect::<Vec<_>>();
        rows[0].status = "pass";
        rows[1].status = "fail";
        let summary = summarize(Some(Feature::Memory), &rows, true);
        let memory = summary
            .iter()
            .find(|f| f.feature == Feature::Memory)
            .unwrap();
        assert_eq!(memory.status, "fail");
        assert_eq!(
            (
                memory.counts.planned,
                memory.counts.pass,
                memory.counts.fail,
                memory.counts.not_run
            ),
            (3, 1, 1, 1)
        );
        assert!(
            summary
                .iter()
                .filter(|f| f.feature != Feature::Memory)
                .all(|f| f.status == "not_selected" && f.counts.planned == 0)
        );
        // Accidentally dropping a failed or unstarted row must not improve acceptance.
        rows.remove(1);
        let summary = summarize(Some(Feature::Memory), &rows, true);
        let memory = summary
            .iter()
            .find(|f| f.feature == Feature::Memory)
            .unwrap();
        assert_eq!(
            (memory.status, memory.counts.planned, memory.counts.not_run),
            ("fail", 3, 2)
        );
        for r in &mut rows {
            r.status = "pass";
        }
        assert_eq!(
            summarize(Some(Feature::Memory), &rows, true)
                .iter()
                .find(|f| f.feature == Feature::Memory)
                .unwrap()
                .status,
            "fail"
        );
    }

    #[test]
    fn missing_or_failed_oracle_control_cannot_certify_a_feature() {
        let mut checks = REQUIRED
            .iter()
            .filter(|(owner, _)| owner.is_none() || *owner == Some(Feature::Memory))
            .map(|(_, name)| grader::check(name, true))
            .collect::<Vec<_>>();
        assert!(validations_pass(Some(Feature::Memory), &checks));
        assert!(
            !validations_pass(None, &checks),
            "a selected suite cannot satisfy full-suite controls"
        );
        let mut removed = checks.pop().unwrap();
        assert!(!validations_pass(Some(Feature::Memory), &checks));
        removed.passed = false;
        checks.push(removed);
        assert!(!validations_pass(Some(Feature::Memory), &checks));
        let mut rows = world::CASES
            .iter()
            .filter(|c| selected(c, Some(Feature::Memory)))
            .map(|c| pending(*c))
            .collect::<Vec<_>>();
        for r in &mut rows {
            r.status = "pass";
        }
        assert_eq!(
            summarize(Some(Feature::Memory), &rows, false)
                .iter()
                .find(|f| f.feature == Feature::Memory)
                .unwrap()
                .status,
            "unverified"
        );
    }

    #[tokio::test]
    async fn memory_only_runs_original_worlds_and_keeps_exclusions_and_controls_visible() {
        let _guard = crate::WORLD_TEST_LOCK.lock().await;
        let report = crate::run_feature(Some(Feature::Memory)).await;
        assert!(
            report.success,
            "{:?}",
            report
                .rows
                .iter()
                .map(|r| (&r.case_id, r.status, r.error))
                .collect::<Vec<_>>()
        );
        assert_eq!(report.selection.feature, Some(Feature::Memory));
        assert!(!report.selection.full_suite);
        assert_eq!(report.selection.selected_case_ids.len(), 3);
        assert_eq!(report.selection.excluded_case_ids.len(), 12);
        assert_eq!(report.counts.planned, 3);
        assert_eq!(report.counts.pass, 3);
        assert!(report.rows.iter().all(|r| r.feature == Feature::Memory));
        assert_eq!(
            report
                .features
                .iter()
                .filter(|f| f.status == "pass")
                .count(),
            1
        );
        assert_eq!(
            report
                .features
                .iter()
                .filter(|f| f.status == "not_selected")
                .count(),
            5
        );
        assert_eq!(report.profile["selection"]["feature"], "memory");
        assert!(report.validation_pass);
        assert!(
            !report
                .validation_checks
                .iter()
                .any(|c| c.name == "actual_skill_read_only_claim_rejected")
        );
    }
}
