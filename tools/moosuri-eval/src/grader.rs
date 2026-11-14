//! Independent deterministic gold. This module never constructs model requests.
use crate::{
    model::IssuedCall,
    observe::{Observation, Slot},
};
use serde::Serialize;
use serde_json::Value;

#[derive(Clone, Serialize)]
pub struct Check {
    pub name: String,
    pub passed: bool,
}
pub fn check(name: &str, passed: bool) -> Check {
    Check {
        name: name.into(),
        passed,
    }
}
fn arguments(slot: &Slot) -> Option<&Value> {
    match slot.input["tool"].as_str()? {
        "knowledge_search" => Some(&slot.input["query"]),
        "knowledge_read_source" | "knowledge_read_memory" => None,
        "skill_read" => Some(&slot.input["pin"]),
        "decimal_calculate" => Some(&slot.input["calculation"]),
        _ => None,
    }
}
fn linked(slot: &Slot, call: &IssuedCall, step: i64) -> bool {
    if slot.step != step || slot.call_id != call.id || slot.input["tool"] != call.name {
        return false;
    }
    let args = if let Some(args) = arguments(slot) {
        args.clone()
    } else {
        let mut v = slot.input.clone();
        if let Some(o) = v.as_object_mut() {
            o.remove("tool");
        }
        v
    };
    args == call.arguments && slot.result.as_ref().is_some_and(|r| r["kind"] != "error")
}
pub fn completed(o: &Observation, account: &str) -> Vec<Check> {
    let calls = o
        .ledger
        .iter()
        .enumerate()
        .flat_map(|(index, d)| d.calls.iter().map(move |c| (index as i64 + 1, c)))
        .collect::<Vec<_>>();
    vec![
        check(
            "actual_terminal_completion",
            o.state == "completed" && o.finalized_no_call_step,
        ),
        check(
            "current_owner",
            o.account == account
                && o.evidence
                    .iter()
                    .all(|e| e.owner.as_deref() == Some(o.tenant.as_str())),
        ),
        check(
            "all_returned_candidates_owned",
            o.returned_candidates
                .iter()
                .all(|e| e.owner.as_deref() == Some(o.tenant.as_str())),
        ),
        check(
            "model_dispatch_receipts",
            o.model_receipts == o.ledger.len()
                && o.model_receipt_steps == (1..=o.ledger.len() as i64).collect::<Vec<_>>()
                && (1..=6).contains(&o.model_receipts)
                && o.ledger.iter().all(|d| d.terminal),
        ),
        check(
            "linked_tool_receipts",
            o.slots.len() == calls.len()
                && o.slots.iter().all(|slot| {
                    calls
                        .iter()
                        .filter(|(step, call)| linked(slot, call, *step))
                        .count()
                        == 1
                })
                && calls.iter().all(|(step, call)| {
                    o.slots
                        .iter()
                        .filter(|slot| linked(slot, call, *step))
                        .count()
                        == 1
                }),
        ),
        check(
            "no_forbidden_external_effect",
            calls.iter().all(|(_, c)| {
                matches!(
                    c.name.as_str(),
                    "knowledge_search"
                        | "knowledge_read_source"
                        | "knowledge_read_memory"
                        | "skill_read"
                        | "decimal_calculate"
                )
            }),
        ),
    ]
}
pub fn calculation(
    o: &Observation,
    account: &str,
    source: &str,
    require_skill: bool,
) -> Vec<Check> {
    let mut checks = completed(o, account);
    // Independent integer-cent oracle: the fixture's 1825 + 650 = 2475.
    // Neither output prose nor the tool's own arithmetic result defines gold.
    checks.push(check(
        "exact_result_fact",
        o.output
            .split_whitespace()
            .any(|s| s.trim_matches(|c: char| !c.is_ascii_alphanumeric() && c != '.') == "USD")
            && o.output.split_whitespace().any(|s| {
                s.trim_matches(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | '−'))
                    .trim_end_matches('.')
                    == "24.75"
            }),
    ));
    checks.push(check(
        "actual_source_evidence",
        o.evidence
            .iter()
            .any(|e| e.kind == "source" && e.id == source && e.revision == 1),
    ));
    checks.push(check(
        "exact_source_read_receipt",
        o.slots.iter().any(|s| {
            s.input["tool"] == "knowledge_read_source"
                && s.input["id"] == source
                && s.input["revision"] == 1
                && s.result.as_ref().is_some_and(|r| {
                    r["kind"] == "source"
                        && r["value"]["source"]["id"] == source
                        && r["value"]["source"]["revision"] == 1
                        && r["value"]["text"] == "Synthetic quote: amount 18.25 shipping 6.50 USD."
                })
        }),
    ));
    checks.push(check(
        "exact_calculation_receipt",
        o.slots.iter().any(|s| {
            s.input["tool"] == "decimal_calculate"
                && s.input["calculation"]
                    == serde_json::json!({"operation":"sum","operands":["18.25","6.50"]})
                && s.result
                    .as_ref()
                    .is_some_and(|r| r["kind"] == "decimal" && r["value"]["exact"] == "24.75")
        }),
    ));
    checks.push(check(
        "no_foreign_canary",
        !o.output.contains("FOREIGN_EVAL_CANARY")
            && o.slots.iter().all(|s| {
                !s.result
                    .as_ref()
                    .is_some_and(|r| r.to_string().contains("FOREIGN_EVAL_CANARY"))
            }),
    ));
    if require_skill {
        checks.push(check(
            "skill_read_and_actual_use",
            o.slots.iter().any(|s| {
                s.input["tool"] == "skill_read"
                    && s.result.as_ref().is_some_and(|r| r["kind"] == "skill")
            }) && o
                .slots
                .iter()
                .any(|s| s.input["tool"] == "decimal_calculate"),
        ));
    }
    checks
}
pub fn all(checks: &[Check]) -> bool {
    !checks.is_empty() && checks.iter().all(|c| c.passed)
}
pub fn mission_mutations(out: &crate::runner::Execution) -> Vec<Check> {
    let passes = |out: &crate::runner::Execution| all(&mission(out));
    let mut checks = vec![check("mission_positive_control", passes(out))];
    for (name, omitted) in [
        (
            "candidate_missing_source_receipt_rejected",
            "knowledge_read_source",
        ),
        (
            "candidate_missing_calculation_receipt_rejected",
            "decimal_calculate",
        ),
    ] {
        let mut mutant = out.clone();
        if let Some(observed) = mutant.observations.first_mut() {
            let slots = &observed.slots;
            if let Some(refs) = observed
                .mission
                .as_mut()
                .and_then(|m| m.candidate["evidence"].as_array_mut())
            {
                refs.retain(|r| {
                    !slots.iter().any(|s| {
                        s.input["tool"] == omitted
                            && r["step"] == s.step
                            && r["call_id"] == s.call_id
                    })
                });
            }
        }
        checks.push(check(name, !passes(&mutant)));
    }
    let mut mutant = out.clone();
    if let Some(observed) = mutant.observations.first_mut() {
        observed.output = "Total USD = 24.75.".into();
        if let Some(mission) = &mut observed.mission {
            mission.candidate["text"] = serde_json::json!(observed.output);
        }
    }
    checks.push(check(
        "candidate_missing_required_quote_rejected",
        !passes(&mutant),
    ));
    checks
}
pub fn mission(out: &crate::runner::Execution) -> Vec<Check> {
    let Some(o) = out.observations.first() else {
        return vec![check("mission_observed", false)];
    };
    let mut checks = calculation(
        o,
        &out.account,
        out.source.as_deref().unwrap_or("missing"),
        false,
    );
    checks.push(check(
        "durable_achieved_plan_and_result",
        o.mission.as_ref().is_some_and(|m| {
            m.state == "achieved"
                && m.mode == "durable"
                && m.plan.as_array().is_some_and(|p| !p.is_empty())
                && m.report.as_array().is_some_and(|p| p.is_empty())
        }),
    ));
    checks.push(check(
        "candidate_evidence_links_actual_receipts",
        o.mission
            .as_ref()
            .and_then(|m| m.candidate["evidence"].as_array())
            .is_some_and(|refs| {
                !refs.is_empty()
                    && refs.iter().all(|r| {
                        o.slots.iter().any(|s| {
                            r["step"] == s.step && r["call_id"] == s.call_id && s.result.is_some()
                        })
                    })
            }),
    ));
    // Check the authored acceptance independently of the product's achieved
    // state/report. A completed unrelated search cannot stand in for the source
    // quote and exact calculation that this mission actually requested.
    let source = out.source.as_deref().unwrap_or("missing");
    let candidate = o.mission.as_ref().map(|m| &m.candidate);
    let references = candidate.and_then(|c| c["evidence"].as_array());
    let cited = |slot: &Slot| {
        references.is_some_and(|refs| {
            refs.iter()
                .any(|r| r["step"] == slot.step && r["call_id"] == slot.call_id)
        })
    };
    checks.push(check(
        "candidate_contains_required_source_quote",
        candidate
            .and_then(|c| c["text"].as_str())
            .is_some_and(|text| {
                text == o.output
                    && text.contains("Synthetic quote: amount 18.25 shipping 6.50 USD.")
            }),
    ));
    checks.push(check(
        "candidate_cites_required_source_read",
        o.slots.iter().any(|s| {
            cited(s)
                && s.input["tool"] == "knowledge_read_source"
                && s.input["id"] == source
                && s.input["revision"] == 1
                && s.result.as_ref().is_some_and(|r| {
                    r["kind"] == "source"
                        && r["value"]["source"]["id"] == source
                        && r["value"]["source"]["revision"] == 1
                        && r["value"]["text"] == "Synthetic quote: amount 18.25 shipping 6.50 USD."
                })
        }),
    ));
    checks.push(check(
        "candidate_cites_required_calculation",
        o.slots.iter().any(|s| {
            cited(s)
                && s.input["tool"] == "decimal_calculate"
                && s.input["calculation"]
                    == serde_json::json!({"operation":"sum","operands":["18.25","6.50"]})
                && s.result
                    .as_ref()
                    .is_some_and(|r| r["kind"] == "decimal" && r["value"]["exact"] == "24.75")
        }),
    ));
    checks.push(check(
        "achieved_state_and_result_survive_reopen_without_dispatch",
        out.mission_reopen_preserved,
    ));
    checks
}

pub fn memory(out: &crate::runner::Execution) -> Vec<Check> {
    let mut checks = vec![];
    if out.observations.len() != 3 {
        return vec![check("complete_memory_workflow", false)];
    }
    for (index, o) in out.observations.iter().enumerate() {
        checks.extend(completed(o, &out.account).into_iter().map(|mut c| {
            c.name = format!("stage_{}:{}", index + 1, c.name);
            c
        }));
    }
    checks.push(check(
        "initial_fact_with_actual_memory_read",
        out.observations[0].output.contains("synthetic amber")
            && !out.observations[0].output.contains("synthetic indigo")
            && out.observations[0]
                .evidence
                .iter()
                .any(|e| e.kind == "memory" && e.revision == 1),
    ));
    checks.push(check(
        "corrected_fact_with_current_revision",
        out.observations[1].output.contains("synthetic indigo")
            && !out.observations[1].output.contains("synthetic amber")
            && out.observations[1]
                .evidence
                .iter()
                .any(|e| e.kind == "memory" && e.revision == 2),
    ));
    checks.push(check(
        "no_fact_or_evidence_after_forget_reopen",
        !out.observations[2].output.is_empty()
            && !out.observations[2].output.contains("synthetic amber")
            && !out.observations[2].output.contains("synthetic indigo")
            && out.observations[2].evidence.is_empty()
            && !out.observations[2]
                .slots
                .iter()
                .any(|s| s.input["tool"] == "knowledge_read_memory"),
    ));
    checks.push(check(
        "persistent_tombstone_and_plaintext_purge",
        out.lifecycle.as_ref().is_some_and(|l| {
            l.same_memory_id
                && l.corrected_revision == 2
                && l.forgotten_status == "forgotten"
                && l.remaining_versions == 0
                && l.remaining_index_rows == 0
                && l.remaining_derived_output == 0
        }),
    ));
    checks
}

/// Controlled evaluator mutations, kept outside primary product pass numerators.
pub fn mutations(o: &Observation, account: &str, source: &str) -> Vec<Check> {
    let passes = |v: &Observation| all(&calculation(v, account, source, true));
    let mut output = vec![check("positive_control", passes(o))];
    for (name, text) in [
        ("negative_total_rejected", "Total USD = -24.75."),
        ("unicode_negative_total_rejected", "Total USD = −24.75."),
    ] {
        let mut mutant = o.clone();
        mutant.output = text.into();
        output.push(check(name, !passes(&mutant)));
    }
    let mut mutant = o.clone();
    mutant.slots.clear();
    mutant.evidence.clear();
    output.push(check("false_success_claim_rejected", !passes(&mutant)));
    let mut mutant = o.clone();
    mutant
        .slots
        .retain(|s| s.input["tool"] != "decimal_calculate");
    output.push(check("missing_tool_receipt_rejected", !passes(&mutant)));
    let mut mutant = o.clone();
    if let Some(slot) = mutant
        .slots
        .iter_mut()
        .find(|s| s.input["tool"] == "decimal_calculate")
    {
        slot.call_id = "another-run-receipt".into();
    }
    output.push(check("wrong_tool_receipt_rejected", !passes(&mutant)));
    let mut mutant = o.clone();
    if mutant.slots.len() > 1 {
        mutant.slots[1] = mutant.slots[0].clone();
    }
    output.push(check("duplicate_reused_receipt_rejected", !passes(&mutant)));
    let mut mutant = o.clone();
    if let Some(e) = mutant.evidence.first_mut() {
        e.owner = Some("other-owner".into());
    }
    output.push(check("other_owner_evidence_rejected", !passes(&mutant)));
    let mut mutant = o.clone();
    if let Some(candidate) = mutant.returned_candidates.first_mut() {
        candidate.owner = Some("other-owner".into());
    }
    output.push(check(
        "unread_other_owner_candidate_rejected",
        !passes(&mutant),
    ));
    let mut mutant = o.clone();
    mutant.finalized_no_call_step = false;
    output.push(check(
        "finalization_interruption_rejected",
        !passes(&mutant),
    ));
    output
}
