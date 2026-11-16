//! Synthetic-only public regression harness, never a live/quality benchmark.
pub mod baseline;
pub mod everyday;
pub mod followup;
mod grader;
mod model;
mod observe;
pub mod paired;
pub mod repair_choice;
mod runner;
mod world;

use serde::Serialize;
use sha2::{Digest, Sha256};
use std::time::{Duration, Instant};
use world::{Case, Kind};
pub type EvalResult<T> = Result<T, &'static str>;
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
#[derive(Serialize)]
pub struct Row {
    pub case_id: String,
    pub family_id: String,
    pub status: &'static str,
    pub error: Option<&'static str>,
    pub run_failures: Vec<String>,
    pub completion_eligible: bool,
    pub task_success: Option<bool>,
    pub mission_eligible: bool,
    pub mission_success: Option<bool>,
    pub handling_eligible: bool,
    pub handling_success: Option<bool>,
    pub checks: Vec<grader::Check>,
    pub setup_ms: u64,
    pub workflow_ms: Option<u64>,
    pub timings: Vec<runner::TurnTiming>,
    pub server_timings: Vec<Option<moosuri_core::RunTimings>>,
    pub provider_calls: usize,
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub request_digests: Vec<String>,
    pub stages: Vec<runner::Stage>,
    pub synthetic_clock: Option<serde_json::Value>,
}
#[derive(Serialize, Default)]
pub struct Counts {
    pub planned: usize,
    pub executed: usize,
    pub pass: usize,
    pub fail: usize,
    pub not_run: usize,
    pub completion_eligible: usize,
    pub task_success: usize,
    pub mission_eligible: usize,
    pub mission_success: usize,
    pub handling_eligible: usize,
    pub handling_success: usize,
}
#[derive(Serialize)]
pub struct Report {
    pub format_version: u32,
    pub evidence_class: &'static str,
    pub source_sha256: &'static str,
    pub fixture_sha256: String,
    pub profile_sha256: String,
    pub grader_sha256: String,
    pub profile: serde_json::Value,
    pub rows: Vec<Row>,
    pub counts: Counts,
    pub observed_latency_ms: serde_json::Value,
    pub validation_checks: Vec<grader::Check>,
    pub validation_pass: bool,
    pub unavailable: serde_json::Value,
    pub success: bool,
}
fn elapsed(t: Instant) -> u64 {
    t.elapsed().as_millis().try_into().unwrap_or(u64::MAX)
}
fn script(kind: Kind) -> model::Script {
    match kind {
        Kind::RepairChoice => model::Script::RepairChoice,
        Kind::FollowupMonitor => model::Script::FollowupMonitor,
        Kind::EverydayGuidance => model::Script::EverydayGuidance,
        Kind::EverydayClarification => model::Script::EverydayClarification,
        Kind::EverydayReminder => model::Script::Chat,
        Kind::Memory => model::Script::Memory,
        Kind::ChatMemory | Kind::TransitiveForget => model::Script::ChatMemory,
        Kind::CancelMission => model::Script::CancelMission,
        Kind::LearnedWorkflow => model::Script::LearnedWorkflow,
        Kind::Disconnect => model::Script::Chat,
        Kind::Truncated => model::Script::Truncated,
        Kind::Calculation => model::Script::Calculation,
        Kind::SkilledCalculation => model::Script::SkilledCalculation,
        Kind::Mission => model::Script::Mission,
    }
}
fn pending(case: Case) -> Row {
    let completion_eligible = matches!(
        case.kind,
        Kind::FollowupMonitor
            | Kind::EverydayGuidance
            | Kind::EverydayClarification
            | Kind::EverydayReminder
            | Kind::Memory
            | Kind::Calculation
            | Kind::SkilledCalculation
            | Kind::Mission
            | Kind::ChatMemory
            | Kind::TransitiveForget
            | Kind::LearnedWorkflow
    );
    Row {
        case_id: case.id.into(),
        family_id: case.family_id.into(),
        status: "not_run",
        error: None,
        run_failures: vec![],
        completion_eligible,
        task_success: None,
        mission_eligible: matches!(case.kind, Kind::Mission),
        mission_success: None,
        handling_eligible: matches!(
            case.kind,
            Kind::Truncated | Kind::CancelMission | Kind::RepairChoice
        ),
        handling_success: None,
        checks: vec![],
        setup_ms: 0,
        workflow_ms: None,
        timings: vec![],
        server_timings: vec![],
        provider_calls: 0,
        input_tokens: None,
        output_tokens: None,
        request_digests: vec![],
        stages: runner::stage_plan(case.kind),
        synthetic_clock: None,
    }
}
fn aggregate(rows: &[Row]) -> Counts {
    Counts {
        planned: rows.len(),
        executed: rows.iter().filter(|r| r.status != "not_run").count(),
        pass: rows.iter().filter(|r| r.status == "pass").count(),
        fail: rows.iter().filter(|r| r.status == "fail").count(),
        not_run: rows.iter().filter(|r| r.status == "not_run").count(),
        completion_eligible: rows.iter().filter(|r| r.completion_eligible).count(),
        task_success: rows.iter().filter(|r| r.task_success == Some(true)).count(),
        mission_eligible: rows.iter().filter(|r| r.mission_eligible).count(),
        mission_success: rows
            .iter()
            .filter(|r| r.mission_success == Some(true))
            .count(),
        handling_eligible: rows.iter().filter(|r| r.handling_eligible).count(),
        handling_success: rows
            .iter()
            .filter(|r| r.handling_success == Some(true))
            .count(),
    }
}
// Explicit control roles avoid coupling apparatus validity to case ordering.
// Both completion and handling setup failures remain applicable and planned.
fn setup_failure_controls() -> [Row; 2] {
    [Kind::Calculation, Kind::RepairChoice].map(|kind| {
        let mut row = pending(Case {
            id: "injected-setup-failure",
            family_id: "denominator-control",
            kind,
        });
        row.error = Some("injected_setup_failure");
        row
    })
}
fn grade(case: Case, out: &runner::Execution) -> Vec<grader::Check> {
    let Some(o) = out.observations.last() else {
        return vec![grader::check("observation_present", false)];
    };
    match case.kind {
        Kind::RepairChoice => grader::repair_choice(out),
        Kind::FollowupMonitor => grader::followup(out),
        Kind::EverydayGuidance | Kind::EverydayClarification | Kind::EverydayReminder => {
            grader::everyday(case.kind, out)
        }
        Kind::Memory => grader::memory(out),
        Kind::ChatMemory | Kind::TransitiveForget | Kind::CancelMission | Kind::LearnedWorkflow => {
            grader::outcome(case.kind, out)
        }
        Kind::Mission => grader::mission(out),
        Kind::Calculation | Kind::SkilledCalculation => grader::calculation(
            o,
            &out.account,
            out.source.as_deref().unwrap_or("missing"),
            matches!(case.kind, Kind::SkilledCalculation),
        ),
        Kind::Disconnect => {
            let mut checks = grader::completed(o, &out.account);
            checks.push(grader::check(
                "same_request_same_run_after_disconnect",
                out.replay_same_run && o.ledger.len() == 1 && o.slots.is_empty(),
            ));
            checks.push(grader::check(
                "persisted_final_body",
                o.output == "Synthetic response body.",
            ));
            checks
        }
        Kind::Truncated => vec![
            grader::check(
                "unknown_not_success",
                o.state == "outcome_unknown" && !o.finalized_no_call_step,
            ),
            grader::check(
                "one_unfinished_provider_dispatch",
                o.ledger.len() == 1 && !o.ledger[0].terminal && o.model_receipts == 1,
            ),
            grader::check(
                "no_completion_receipt_or_tool_effect",
                o.slots.is_empty() && o.evidence.is_empty(),
            ),
        ],
    }
}
/// All endpoint/world/budget choices are compiled in. No account, endpoint,
/// arbitrary database, fixture path or model credential can be supplied.
pub async fn run() -> Report {
    let mut rows = vec![];
    let mut validation = vec![];
    for &case in world::CASES {
        let mut row = pending(case);
        let setup = Instant::now();
        let setup_result = tokio::time::timeout(
            Duration::from_secs(40),
            runner::World::setup(script(case.kind)),
        )
        .await;
        row.setup_ms = elapsed(setup);
        let mut world = match setup_result {
            Ok(Ok(world)) => world,
            Ok(Err(e)) => {
                row.error = Some(e);
                rows.push(row);
                continue;
            }
            Err(_) => {
                row.error = Some("setup_timeout");
                rows.push(row);
                continue;
            }
        };
        row.status = "fail";
        let started = Instant::now();
        let executed = tokio::time::timeout(Duration::from_secs(40), world.execute(case)).await;
        row.workflow_ms = Some(elapsed(started));
        match executed {
            Ok(Ok(out)) => {
                row.synthetic_clock = out.outcome.get("clock").cloned();
                row.checks = grade(case, &out);
                let passed = grader::all(&row.checks);
                row.run_failures = out
                    .observations
                    .iter()
                    .filter_map(|o| o.failure.clone())
                    .collect();
                if !passed {
                    row.error = Some("outcome_postcondition_failed");
                }
                row.status = if passed { "pass" } else { "fail" };
                row.task_success = row.completion_eligible.then_some(passed);
                row.mission_success = row.mission_eligible.then_some(passed);
                row.handling_success = row.handling_eligible.then_some(passed);
                row.input_tokens = out
                    .observations
                    .iter()
                    .try_fold(0_u64, |n, o| o.input_tokens.map(|v| n + v));
                row.output_tokens = out
                    .observations
                    .iter()
                    .try_fold(0_u64, |n, o| o.output_tokens.map(|v| n + v));
                if matches!(case.kind, Kind::SkilledCalculation)
                    && let Some(o) = out.observations.last()
                {
                    validation.extend(grader::mutations(
                        o,
                        &out.account,
                        out.source.as_deref().unwrap_or("missing"),
                    ));
                }
                if matches!(case.kind, Kind::Mission) {
                    validation.extend(grader::mission_mutations(&out));
                }
                if matches!(
                    case.kind,
                    Kind::ChatMemory
                        | Kind::TransitiveForget
                        | Kind::CancelMission
                        | Kind::LearnedWorkflow
                ) {
                    validation.extend(grader::outcome_mutations(case.kind, &out));
                }
                if matches!(
                    case.kind,
                    Kind::EverydayGuidance | Kind::EverydayClarification | Kind::EverydayReminder
                ) {
                    validation.extend(grader::everyday_mutations(case.kind, &out));
                }
                if matches!(case.kind, Kind::FollowupMonitor) {
                    validation.extend(grader::followup_mutations(&out));
                }
                if matches!(case.kind, Kind::RepairChoice) {
                    validation.extend(grader::repair_choice_mutations(&out));
                }
                row.timings = out.timings;
            }
            Ok(Err(e)) => row.error = Some(e),
            Err(_) => row.error = Some("workflow_timeout"),
        }
        // Dispatch observations survive workflow errors. Tokens remain unknown
        // unless a complete SQLite observation was obtained, never inferred from
        // the scripted provider's nominal usage.
        if !world.stages().is_empty() {
            row.stages = world.stages();
        }
        if row.stages.iter().any(|stage| stage.status != "pass") && row.status == "pass" {
            row.status = "fail";
            row.error = Some("stage_contract_failed");
            row.task_success = row.completion_eligible.then_some(false);
        }
        row.server_timings = world.server_timings();
        let ledger = world.provider.ledger();
        row.provider_calls = ledger.len();
        row.request_digests = ledger.into_iter().map(|d| d.request_sha256).collect();
        if let Some((input, output)) = world.usage_totals() {
            row.input_tokens = input;
            row.output_tokens = output;
        }
        world.shutdown().await;
        rows.push(row);
    }
    // Actual skill-read-only execution is an apparatus negative control, not a
    // successful task or an extra primary world in the product denominator.
    let negative = tokio::time::timeout(Duration::from_secs(40), async {
        let mut world = runner::World::setup(model::Script::SkillOnly).await?;
        let case = Case {
            id: "skill-read-only-control",
            family_id: "grader-negative",
            kind: Kind::SkilledCalculation,
        };
        let execution = tokio::time::timeout(Duration::from_secs(15), world.execute(case)).await;
        world.shutdown().await;
        let out = execution.map_err(|_| "negative_timeout")??;
        let o = out.observations.last().ok_or("negative_observation")?;
        let actual_skill_read = o.slots.iter().any(|s| s.input["tool"] == "skill_read");
        Ok::<_, &'static str>(
            actual_skill_read
                && o.state == "completed"
                && !grader::all(&grader::calculation(
                    o,
                    &out.account,
                    out.source.as_deref().unwrap_or("missing"),
                    true,
                )),
        )
    })
    .await;
    validation.push(grader::check(
        "actual_skill_read_only_claim_rejected",
        matches!(negative, Ok(Ok(true))),
    ));
    // A setup error must remain planned/not-run and completion-eligible; this
    // structural check prevents apparatus failures silently improving the score.
    let denominator = aggregate(&setup_failure_controls());
    validation.push(grader::check(
        "setup_failure_retained_in_denominator",
        denominator.planned == 2
            && denominator.not_run == 2
            && denominator.executed == 0
            && denominator.pass == 0
            && denominator.completion_eligible == 1
            && denominator.handling_eligible == 1,
    ));
    let counts = aggregate(&rows);
    let required_validations = [
        "repair_choice_positive_control",
        "repair_choice_missing_live_settlement_rejected",
        "repair_choice_accepted_ambiguous_answer_rejected",
        "repair_choice_replaced_wait_rejected",
        "repair_choice_forged_candidate_rejected",
        "repair_choice_repeated_read_rejected",
        "repair_choice_duplicate_reply_rejected",
        "repair_choice_late_result_rejected",
        "repair_choice_visible_notice_rejected",
        "followup_positive_control",
        "followup_duplicate_child_rejected",
        "followup_wrong_source_scope_rejected",
        "followup_missing_changed_read_rejected",
        "followup_visible_stopped_notice_rejected",
        "followup_revived_policy_rejected",
        "followup_wrong_visible_mission_link_rejected",
        "followup_wrong_visible_schedule_link_rejected",
        "followup_wrong_visible_occurrence_link_rejected",
        "followup_contradictory_paid_status_rejected",
        "followup_negated_approval_rejected",
        "everyday_guidance_missing_receipt_rejected",
        "everyday_guidance_repeated_tool_rejected",
        "everyday_clarification_lost_work_rejected",
        "everyday_clarification_false_achievement_rejected",
        "everyday_reminder_wrong_target_rejected",
        "everyday_reminder_duplicate_replay_rejected",
        "everyday_reminder_wrong_time_rejected",
        "everyday_reminder_missing_tombstone_rejected",
        "positive_control",
        "negative_total_rejected",
        "unicode_negative_total_rejected",
        "mission_positive_control",
        "candidate_missing_source_receipt_rejected",
        "candidate_missing_calculation_receipt_rejected",
        "candidate_missing_required_quote_rejected",
        "false_success_claim_rejected",
        "missing_tool_receipt_rejected",
        "wrong_tool_receipt_rejected",
        "duplicate_reused_receipt_rejected",
        "other_owner_evidence_rejected",
        "unread_other_owner_candidate_rejected",
        "finalization_interruption_rejected",
        "actual_skill_read_only_claim_rejected",
        "setup_failure_retained_in_denominator",
        "chat_correction_wrong_identity_rejected",
        "forget_transitive_checkpoint_residue_rejected",
        "cancel_late_success_rejected",
        "learned_duplicate_proof_rejected",
        "learned_missing_nested_receipts_rejected",
        "learned_withdrawn_output_residue_rejected",
        "chat_success_without_command_receipt_rejected",
        "forget_unrelated_answer_loss_rejected",
        "cancel_false_certain_outcome_rejected",
        "learned_foreign_proof_rejected",
        "learned_stale_nested_arguments_rejected",
    ];
    let validation_pass = grader::all(&validation)
        && required_validations
            .iter()
            .all(|name| validation.iter().any(|c| c.name == *name));
    let success = counts.pass == counts.planned && validation_pass;
    let fixture = serde_json::json!({"cases":world::CASES,"source":world::SOURCE,"foreign_source":world::FOREIGN_SOURCE,"preference_a":world::PREFERENCE_A,"preference_b":world::PREFERENCE_B,"observed_at":world::FIXTURE_CLOCK,"script_sha256":digest(include_bytes!("model.rs")),"outcome_script_sha256":digest(include_bytes!("model/outcomes.rs")),"outcome_workflow_sha256":digest(include_bytes!("runner/outcomes.rs")),"everyday":everyday::FIXTURES,"repair_choice":repair_choice::FIXTURE,"repair_choice_workflow_sha256":digest(include_bytes!("runner/repair_choice.rs")),"repair_choice_script_sha256":digest(include_bytes!("model/repair_choice.rs")),"repair_choice_observer_sha256":digest(include_bytes!("observe/repair_choice.rs")),"followup":followup::FIXTURE,"followup_workflow_sha256":digest(include_bytes!("runner/followup.rs")),"followup_script_sha256":digest(include_bytes!("model/followup.rs")),"everyday_workflow_sha256":digest(include_bytes!("runner/everyday.rs")),"everyday_script_sha256":digest(include_bytes!("model/everyday.rs"))});
    let observed_latency_ms = latencies(&rows);
    Report {
        format_version: 3,
        evidence_class: "public_synthetic_contract_regression",
        source_sha256: env!("MOOSURI_EVAL_SOURCE_SHA256"),
        fixture_sha256: digest(&serde_json::to_vec(&fixture).unwrap()),
        profile_sha256: digest(world::PROFILE.as_bytes()),
        grader_sha256: digest(
            &[
                include_bytes!("grader.rs").as_slice(),
                include_bytes!("grader/outcomes.rs").as_slice(),
                include_bytes!("grader/everyday.rs").as_slice(),
                include_bytes!("grader/followup.rs").as_slice(),
                include_bytes!("grader/repair_choice.rs").as_slice(),
            ]
            .concat(),
        ),
        profile: serde_json::from_str(world::PROFILE).unwrap(),
        rows,
        counts,
        observed_latency_ms,
        validation_checks: validation,
        validation_pass,
        unavailable: serde_json::json!({"semantic_model_quality":"not_run","blind_holdout":"not_run","legacy_baseline":"not_run","non_inferiority":"not_run","pilot32_worlds":"not_run","live_provider":"not_run","device":"not_run","cost_usd":null,"dom_render_ms":null,"rss_bytes":null,"cpu_ms":null,"timing_method":"monotonic HTTP admission/10ms-polled terminal and separate per-run server RunView.timings; missing fields counted, never filled"}),
        success,
    }
}

fn percentiles(mut values: Vec<u64>) -> serde_json::Value {
    values.sort_unstable();
    let value = |percent: usize| {
        values
            .get((values.len() * percent).div_ceil(100).saturating_sub(1))
            .copied()
    };
    serde_json::json!({"observed_samples":values.len(),"p50":value(50),"p95":value(95),"method":"nearest_rank"})
}
fn server_latencies(rows: &[Row]) -> serde_json::Value {
    let turns = rows
        .iter()
        .flat_map(|r| r.server_timings.iter())
        .collect::<Vec<_>>();
    let mut fields = serde_json::Map::new();
    type TimingSelector = fn(&moosuri_core::RunTimings) -> Option<u64>;
    let selectors: [(&str, TimingSelector); 7] = [
        ("queue_ms", |t| t.queue_ms),
        ("context_ms", |t| t.context_ms),
        ("first_dispatch_ms", |t| t.first_dispatch_ms),
        ("model_ms", |t| t.model_ms),
        ("tool_ms", |t| t.tool_ms),
        ("first_final_body_ms", |t| t.first_final_body_ms),
        ("terminal_ms", |t| t.terminal_ms),
    ];
    for (name, select) in selectors {
        let values = turns
            .iter()
            .filter_map(|t| t.as_ref().and_then(select))
            .collect::<Vec<_>>();
        let missing = turns.len() - values.len();
        let mut result = percentiles(values);
        result["missing"] = serde_json::json!(missing);
        fields.insert(name.into(), result);
    }
    let observed = turns.iter().filter_map(|t| t.as_ref()).collect::<Vec<_>>();
    serde_json::json!({"run_samples":turns.len(),"missing_run_timings":turns.len()-observed.len(),"clock_discontinuity":observed.iter().filter(|t|t.clock_discontinuity).count(),"model_calls":observed.iter().map(|t|u64::from(t.model_calls)).sum::<u64>(),"measured_model_calls":observed.iter().map(|t|u64::from(t.measured_model_calls)).sum::<u64>(),"tool_calls":observed.iter().map(|t|u64::from(t.tool_calls)).sum::<u64>(),"measured_tool_calls":observed.iter().map(|t|u64::from(t.measured_tool_calls)).sum::<u64>(),"stages":fields,"scope":"admitted runs including failed workflows; missing includes internal missions without a RunView API sample; not load or browser measurement"})
}
fn latencies(rows: &[Row]) -> serde_json::Value {
    serde_json::json!({"server":server_latencies(rows),"admission":percentiles(rows.iter().flat_map(|r|r.timings.iter().map(|t|t.admission_ms)).collect()),"terminal_observed":percentiles(rows.iter().flat_map(|r|r.timings.iter().map(|t|t.terminal_observed_ms)).collect()),"workflow_including_failed":percentiles(rows.iter().filter_map(|r|r.workflow_ms).collect())})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn setup_failure_does_not_shrink_applicable_denominator() {
        let c = aggregate(&setup_failure_controls());
        assert_eq!(
            (
                c.planned,
                c.executed,
                c.pass,
                c.not_run,
                c.completion_eligible,
                c.handling_eligible,
                c.task_success,
                c.handling_success
            ),
            (2, 0, 0, 2, 1, 1, 0, 0)
        );
    }
    #[tokio::test]
    async fn fresh_sqlite_http_worlds_and_independent_mutants() {
        let report = run().await;
        for row in &report.rows {
            assert_eq!(
                row.status,
                "pass",
                "{}: {:?}; failed checks {:?}",
                row.case_id,
                row.error,
                row.checks
                    .iter()
                    .filter(|c| !c.passed)
                    .map(|c| &c.name)
                    .collect::<Vec<_>>()
            );
        }
        assert!(
            report.validation_pass,
            "failed validations {:?}",
            report
                .validation_checks
                .iter()
                .filter(|c| !c.passed)
                .map(|c| &c.name)
                .collect::<Vec<_>>()
        );
        for row in &report.rows {
            for stage in &row.stages {
                assert_eq!(stage.status, "pass", "{} / {}", row.case_id, stage.name);
                assert!(
                    stage
                        .model_calls
                        .is_some_and(|calls| calls <= stage.model_limit)
                );
                assert!(
                    stage
                        .tool_calls
                        .is_some_and(|calls| calls <= stage.tool_limit)
                );
            }
        }
        assert_eq!(report.counts.task_success, 11);
        assert_eq!(report.counts.mission_success, 1);
        assert_eq!(report.counts.handling_success, 3);
        let server = &report.observed_latency_ms["server"];
        assert_eq!(server["run_samples"], 33);
        for stage in server["stages"].as_object().unwrap().values() {
            assert_eq!(
                stage["observed_samples"].as_u64().unwrap() + stage["missing"].as_u64().unwrap(),
                33
            );
        }
        assert!(report.success);
    }
}
