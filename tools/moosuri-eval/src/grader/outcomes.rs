//! Independent state, identity and arithmetic oracles for full user workflows.
use super::*;
use crate::{runner::Execution, world::Kind};
use serde_json::json;

fn command(o: &Observation, account: &str, action: &str, revision: i64) -> bool {
    let Some(c) = &o.command else { return false };
    o.state == "completed"
        && !o.output.is_empty()
        && o.account == account
        && c["account"] == account
        && c["state"] == "committed"
        && c["action"] == action
        && c["outcome"]["kind"] == "applied"
        && c["outcome"]["action"] == action
        && c["outcome"]["memory_id"].as_str().is_some()
        && c["outcome"]["revision"] == revision
        && o.ledger.len() == 1
        && o.model_receipts == 1
        && o.model_receipt_steps == [1]
        && c["proof_step"] == 1
        && o.ledger[0].terminal
        && o.ledger[0].calls.len() == 1
        && o.ledger[0].calls[0].name == "memory_command_decision"
        && o.ledger[0].calls[0].arguments["decision"] == "command"
        && o.ledger[0].calls[0].arguments["action"] == action
        && o.slots.is_empty()
}
fn memory_fact(o: &Observation, id: &Value, revision: i64, fact: &str) -> bool {
    o.output == fact
        && o.evidence
            .iter()
            .any(|e| e.kind == "memory" && json!(e.id) == *id && e.revision == revision)
        && o.slots.iter().any(|s| {
            s.input["tool"] == "knowledge_read_memory"
                && s.input["id"] == *id
                && s.input["revision"] == revision
                && s.result
                    .as_ref()
                    .is_some_and(|r| r["kind"] == "memory" && r["value"]["draft"]["text"] == fact)
        })
}
fn append_completed(checks: &mut Vec<Check>, o: &Observation, account: &str, index: usize) {
    checks.extend(completed(o, account).into_iter().map(|mut c| {
        c.name = format!("outcome_stage_{index}:{}", c.name);
        c
    }));
}
pub fn outcome(kind: Kind, out: &Execution) -> Vec<Check> {
    let observations = &out.observations;
    match kind {
        Kind::ChatMemory => {
            if observations.len() != 5 {
                return vec![check("chat_memory_all_stages", false)];
            }
            let id = &observations[0]
                .command
                .as_ref()
                .map(|c| c["outcome"]["memory_id"].clone())
                .unwrap_or(Value::Null);
            let mut checks = vec![
                check(
                    "chat_remember_committed_receipt",
                    command(&observations[0], &out.account, "remember", 1),
                ),
                check(
                    "chat_correct_committed_receipt",
                    command(&observations[2], &out.account, "correct", 2),
                ),
                check(
                    "correction_preserves_memory_identity",
                    !id.is_null()
                        && observations[2].command.as_ref().is_some_and(|c| {
                            c["target"] == *id
                                && c["target_revision"] == 1
                                && c["outcome"]["memory_id"] == *id
                        }),
                ),
            ];
            for (i, revision, fact) in [
                (1, 1, "My preference is synthetic amber."),
                (3, 2, "My preference is synthetic indigo."),
                (4, 2, "My preference is synthetic indigo."),
            ] {
                append_completed(&mut checks, &observations[i], &out.account, i);
                checks.push(check(
                    &format!("current_fact_stage_{i}"),
                    memory_fact(&observations[i], id, revision, fact),
                ));
            }
            checks
        }
        Kind::TransitiveForget => {
            if observations.len() != 7 {
                return vec![check("forget_all_stages", false)];
            }
            let mut checks = vec![
                check(
                    "forget_remember_command",
                    command(&observations[1], &out.account, "remember", 1),
                ),
                check(
                    "forget_committed_receipt",
                    command(&observations[4], &out.account, "forget", 2),
                ),
            ];
            for i in [0, 2, 3, 5, 6] {
                append_completed(&mut checks, &observations[i], &out.account, i);
            }
            let id = observations[1]
                .command
                .as_ref()
                .map(|c| c["outcome"]["memory_id"].clone())
                .unwrap_or(Value::Null);
            checks.push(check(
                "forget_exact_target",
                !id.is_null()
                    && observations[4].command.as_ref().is_some_and(|c| {
                        c["target"] == id
                            && c["target_revision"] == 1
                            && c["outcome"]["memory_id"] == id
                    }),
            ));
            checks.push(check(
                "memory_read_before_forget",
                memory_fact(
                    &observations[2],
                    &id,
                    1,
                    "My preference is synthetic amber.",
                ),
            ));
            let history = &observations[3];
            checks.push(check(
                "actual_history_read_and_transitive_memory_dependency",
                history.output.contains("synthetic amber")
                    && history
                        .history_dependencies
                        .iter()
                        .any(|d| d.id == observations[2].run_id)
                    && history.evidence.iter().any(|e| json!(e.id) == id)
                    && history.slots.iter().any(|s| {
                        s.input["tool"] == "conversation_read"
                            && s.input["turn_id"] == observations[2].run_id
                    }),
            ));
            checks.push(check(
                "forgotten_payload_index_and_transitive_material_purged",
                out.outcome["status"] == "forgotten"
                    && out.outcome["versions"] == 0
                    && out.outcome["index"] == 0
                    && out.outcome["purged"].as_array().is_some_and(|rows| {
                        rows.len() == 3
                            && rows.iter().all(|r| {
                                ["output", "slots", "checkpoints", "events_with_canary"]
                                    .iter()
                                    .all(|key| r[*key] == 0)
                            })
                    }),
            ));
            checks.push(check(
                "fresh_memory_and_history_recall_after_reopen_empty",
                [5, 6].iter().all(|i| {
                    !observations[*i].output.is_empty()
                        && !observations[*i].output.contains("synthetic amber")
                        && observations[*i].evidence.is_empty()
                        && observations[*i].returned_candidates.is_empty()
                        && observations[*i].history_dependencies.is_empty()
                }),
            ));
            checks.push(check(
                "independent_source_and_answer_survive_same_canary",
                out.outcome["independent"]["text"]
                    == "UNRELATED_SOURCE independent record: My preference is synthetic amber."
                    && out.outcome["independent"]["index"]
                        .as_i64()
                        .is_some_and(|n| n > 0)
                    && out.outcome["independent"]["output"] == out.outcome["independent"]["text"],
            ));
            checks
        }
        Kind::CancelMission => {
            if observations.len() != 1 {
                return vec![check("cancel_observed", false)];
            }
            let o = &observations[0];
            vec![
                check(
                    "cancel_terminal_no_late_achievement",
                    o.account == out.account
                        && o.state == "cancelled"
                        && o.mission
                            .as_ref()
                            .is_some_and(|m| m.state == "cancelled" && m.candidate.is_null())
                        && o.output.is_empty()
                        && o.slots.is_empty(),
                ),
                check(
                    "cancel_revokes_grant_preserves_uncertainty",
                    out.outcome["grant"] == false && out.outcome["uncertain"] == true,
                ),
                check(
                    "cancel_request_idempotent",
                    out.outcome["controls"] == 1 && out.outcome["replay_same_revision"] == true,
                ),
                check(
                    "cancel_inflight_receipt_preserved_no_reopen_dispatch",
                    o.ledger.len() == 1
                        && o.ledger[0].calls.is_empty()
                        && o.model_receipts == 1
                        && o.model_receipt_steps == [1]
                        && out.outcome["usage_preserved"] == true,
                ),
            ]
        }
        Kind::LearnedWorkflow => learned(out),
        _ => vec![check("known_outcome_world", false)],
    }
}
fn receipt_digest(o: &Observation) -> Option<String> {
    let calls = o
        .slots
        .iter()
        .map(|s| {
            Some((
                serde_json::from_value::<moosuri_store::KnowledgeToolInput>(s.input.clone())
                    .ok()?,
                serde_json::from_value::<moosuri_store::KnowledgeToolResult>(s.result.clone()?)
                    .ok()?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    serde_json::to_vec(&calls).ok().map(|v| crate::digest(&v))
}
fn canonical_decimal(value: &str) -> Option<String> {
    let value = value.strip_prefix('+').unwrap_or(value);
    let (negative, value) = value
        .strip_prefix('-')
        .map(|v| (true, v))
        .unwrap_or((false, value));
    let mut parts = value.split('.');
    let whole = parts.next()?;
    let fraction = parts.next().unwrap_or("");
    if parts.next().is_some()
        || whole.is_empty()
        || !whole.bytes().all(|b| b.is_ascii_digit())
        || !fraction.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    let whole = whole.trim_start_matches('0');
    let whole = if whole.is_empty() { "0" } else { whole };
    let fraction = fraction.trim_end_matches('0');
    let sign = if negative && (whole != "0" || !fraction.is_empty()) {
        "-"
    } else {
        ""
    };
    Some(if fraction.is_empty() {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{fraction}")
    })
}
fn commutative_operands(actual: &Value, expected: &Value) -> bool {
    let strings = |v: &Value| {
        v.as_array().and_then(|a| {
            a.iter()
                .map(|x| canonical_decimal(x.as_str()?))
                .collect::<Option<Vec<_>>>()
        })
    };
    let (Some(mut actual), Some(mut expected)) = (strings(actual), strings(expected)) else {
        return false;
    };
    actual.sort();
    expected.sort();
    actual == expected
}
fn learned(out: &Execution) -> Vec<Check> {
    if out.observations.len() != 4 {
        return vec![check("learned_all_stages", false)];
    }
    let mut checks = vec![];
    // Independent scaled-integer arithmetic for ((a*b)+c)*d; no product calculator is called.
    let gold = [
        ((1250_i64 * 2 + 400) * 8, 1000_i64, "23.2"),
        ((725 * 4 + 600) * 6, 1000, "21"),
        ((950 * 3 + 500) * 9, 1000, "30.15"),
        ((300 * 8 + 700) * 5, 1000, "15.5"),
    ];
    let inputs = [
        json!(["12.50", "2", "4", "0.8"]),
        json!(["7.25", "4", "6", "0.6"]),
        json!(["9.50", "3", "5", "0.9"]),
        json!(["3", "8", "7", "0.5"]),
    ];
    let nested = [
        json!(["25", "29", "23.2"]),
        json!(["29", "35", "21"]),
        json!(["28.5", "33.5", "30.15"]),
        json!(["24", "31", "15.5"]),
    ];
    let operands = [
        [
            json!(["12.50", "2"]),
            json!(["25", "4"]),
            json!(["29", "0.8"]),
        ],
        [
            json!(["7.25", "4"]),
            json!(["29", "6"]),
            json!(["35", "0.6"]),
        ],
        [
            json!(["9.50", "3"]),
            json!(["28.5", "5"]),
            json!(["33.5", "0.9"]),
        ],
        [json!(["3", "8"]), json!(["24", "7"]), json!(["31", "0.5"])],
    ];
    for (i, o) in out.observations.iter().enumerate() {
        append_completed(&mut checks, o, &out.account, i);
        let (numerator, denominator, exact) = gold[i];
        let parsed = exact
            .split_once('.')
            .map(|(a, b)| {
                (
                    a.parse::<i64>().unwrap() * 10_i64.pow(b.len() as u32)
                        + b.parse::<i64>().unwrap(),
                    10_i64.pow(b.len() as u32),
                )
            })
            .unwrap_or_else(|| (exact.parse().unwrap(), 1));
        checks.push(check(
            &format!("workflow_exact_independent_amount_{i}"),
            parsed.0 * denominator == numerator * parsed.1
                && o.output == format!("Exact current amount: {exact}."),
        ));
        let slot = o.slots.first();
        checks.push(check(
            &format!("workflow_fresh_input_arguments_{i}"),
            o.slots.len() == 1
                && slot.is_some_and(|s| {
                    if i == 2 {
                        s.input["tool"] == "learned_skill"
                            && s.input["invocation"]["arguments"]["inputs"] == inputs[i]
                    } else {
                        s.input["tool"] == "decimal_workflow"
                            && s.input["workflow"]["inputs"] == inputs[i]
                    }
                }),
        ));
        checks.push(check(
            &format!("workflow_nested_receipts_linked_{i}"),
            o.learned_receipts.len() == 3
                && o.learned_receipts.iter().enumerate().all(|(ordinal, r)| {
                    slot.is_some_and(|s| r["step"] == s.step && r["call_id"] == s.call_id)
                        && r["ordinal"] == ordinal
                        && r["input"]["tool"] == "decimal_calculate"
                        && r["input"]["calculation"]["operation"]
                            == if ordinal == 1 { "sum" } else { "product" }
                        && commutative_operands(
                            &r["input"]["calculation"]["operands"],
                            &operands[i][ordinal],
                        )
                        && r["result"]["kind"] == "decimal"
                        && r["result"]["value"]["exact"] == nested[i][ordinal]
                }),
        ));
    }
    let stages = &out.outcome["stages"];
    checks.push(check(
        "distinct_finalized_A_B_proofs_make_route_usable",
        stages[0]["cards"][0]["state"] == "candidate"
            && stages[0]["proofs"].as_array().is_some_and(|p| p.len() == 1)
            && stages[1]["cards"][0]["state"] == "usable"
            && stages[1]["proofs"].as_array().is_some_and(|proofs| {
                proofs.len() == 2
                    && proofs[0]["input_digest"] != proofs[1]["input_digest"]
                    && proofs.iter().all(|p| {
                        out.observations[..2].iter().any(|o| {
                            p["run_id"] == o.run_id
                                && p["account"] == out.account
                                && p["state"] == "completed"
                                && p["invalidated"] == false
                                && receipt_digest(o).is_some_and(|d| p["receipt_digest"] == d)
                        })
                    })
            }),
    ));
    let c = &out.observations[2];
    checks.push(check(
        "unseen_C_uses_admitted_pin_and_actual_route",
        c.ledger.iter().any(|d| d.private_skill_admitted)
            && c.slots
                .first()
                .is_some_and(|s| s.input["invocation"]["pin"]["id"] == stages[1]["cards"][0]["id"])
            && stages[2]["proofs"].as_array().is_some_and(|p| p.len() == 2)
            && stages[2]["uses"].as_array().is_some_and(|uses| {
                uses.iter().any(|u| {
                    u["run_id"] == c.run_id && u["skill_id"] == stages[1]["cards"][0]["id"]
                })
            }),
    ));
    checks.push(check(
        "proof_withdrawal_degrades_route_and_purges_C",
        out.outcome["withdrawn"]["cards"][0]["state"] == "candidate"
            && out.outcome["withdrawn"]["proofs"]
                .as_array()
                .is_some_and(|p| p.len() == 1 && p[0]["run_id"] == out.observations[0].run_id)
            && out.outcome["c_output_after_withdrawal"] == ""
            && out.outcome["c_state_after_withdrawal"] == "incomplete",
    ));
    checks.push(check(
        "fresh_D_has_no_usable_private_route_after_withdrawal",
        out.observations[3]
            .ledger
            .iter()
            .all(|d| !d.private_skill_admitted),
    ));
    checks
}

/// Apparatus controls mutate only evaluator observations, outside the task numerator.
pub fn outcome_mutations(kind: Kind, out: &Execution) -> Vec<Check> {
    let passes = |o: &Execution| all(&outcome(kind, o));
    let mut checks = vec![check(&format!("{:?}_outcome_positive", kind), passes(out))];
    // Mutants require a successful positive control. A failed/incomplete world
    // remains a failed validation; do not index absent proof/receipt arrays or
    // claim that an already-failing baseline demonstrates mutation rejection.
    if !passes(out) {
        return checks;
    }
    let mut mutant = out.clone();
    let name = match kind {
        Kind::ChatMemory => {
            if let Some(c) = mutant
                .observations
                .get_mut(2)
                .and_then(|o| o.command.as_mut())
            {
                c["outcome"]["memory_id"] = json!("wrong-memory");
            }
            "chat_correction_wrong_identity_rejected"
        }
        Kind::TransitiveForget => {
            mutant.outcome["purged"][2]["checkpoints"] = json!(1);
            "forget_transitive_checkpoint_residue_rejected"
        }
        Kind::CancelMission => {
            if let Some(m) = mutant.observations[0].mission.as_mut() {
                m.state = "achieved".into();
            }
            "cancel_late_success_rejected"
        }
        Kind::LearnedWorkflow => {
            mutant.outcome["stages"][1]["proofs"][1]["input_digest"] =
                mutant.outcome["stages"][1]["proofs"][0]["input_digest"].clone();
            "learned_duplicate_proof_rejected"
        }
        _ => return vec![],
    };
    checks.push(check(name, !passes(&mutant)));
    if matches!(kind, Kind::ChatMemory) {
        let mut mutant = out.clone();
        mutant.observations[0].command = None;
        checks.push(check(
            "chat_success_without_command_receipt_rejected",
            !passes(&mutant),
        ));
    }
    if matches!(kind, Kind::TransitiveForget) {
        let mut mutant = out.clone();
        mutant.outcome["independent"]["output"] = json!("");
        checks.push(check(
            "forget_unrelated_answer_loss_rejected",
            !passes(&mutant),
        ));
    }
    if matches!(kind, Kind::CancelMission) {
        let mut mutant = out.clone();
        mutant.outcome["uncertain"] = json!(false);
        checks.push(check(
            "cancel_false_certain_outcome_rejected",
            !passes(&mutant),
        ));
    }
    if matches!(kind, Kind::LearnedWorkflow) {
        let mut mutant = out.clone();
        mutant.outcome["stages"][1]["proofs"][0]["account"] = json!("foreign-owner");
        checks.push(check("learned_foreign_proof_rejected", !passes(&mutant)));
        let mut mutant = out.clone();
        mutant.observations[2].learned_receipts[0]["input"]["calculation"]["operands"] =
            json!(["12.50", "2"]);
        checks.push(check(
            "learned_stale_nested_arguments_rejected",
            !passes(&mutant),
        ));
        let mut mutant = out.clone();
        mutant.observations[2].learned_receipts.clear();
        checks.push(check(
            "learned_missing_nested_receipts_rejected",
            !passes(&mutant),
        ));
        let mut mutant = out.clone();
        mutant.outcome["c_output_after_withdrawal"] = json!("Exact current amount: 30.15.");
        checks.push(check(
            "learned_withdrawn_output_residue_rejected",
            !passes(&mutant),
        ));
    }
    checks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalized_nested_decimal_receipts_keep_fresh_numeric_values() {
        assert!(commutative_operands(
            &json!(["9.5", "3"]),
            &json!(["3", "9.50"])
        ));
        for stale in [
            json!(["12.5", "2"]),
            json!(["-9.5", "3"]),
            json!(["9.5.0", "3"]),
        ] {
            assert!(!commutative_operands(&stale, &json!(["9.50", "3"])));
        }
    }

    #[test]
    fn failed_positive_control_with_empty_proofs_is_reportable() {
        let out = Execution {
            observations: vec![],
            timings: vec![],
            account: "synthetic-owner".into(),
            source: None,
            lifecycle: None,
            replay_same_run: false,
            mission_reopen_preserved: false,
            outcome: json!({"stages":[{"cards":[],"proofs":[]},{"cards":[],"proofs":[]},{"cards":[],"proofs":[]}]}),
        };
        for kind in [
            Kind::ChatMemory,
            Kind::TransitiveForget,
            Kind::CancelMission,
            Kind::LearnedWorkflow,
        ] {
            let checks = outcome_mutations(kind, &out);
            assert_eq!(checks.len(), 1, "a failed baseline cannot validate mutants");
            assert!(!checks[0].passed);
        }
    }
}
