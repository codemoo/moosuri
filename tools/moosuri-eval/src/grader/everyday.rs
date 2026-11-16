//! Independent evaluator-only outcome and tampered-observation controls.
use super::*;
use crate::{runner::Execution, world::Kind};
use serde_json::json;

fn fact(text: &str, expected: &str) -> bool {
    text.split_whitespace().any(|word| {
        word.trim_matches(|c: char| !c.is_ascii_digit() && !matches!(c, '.' | '-' | '+' | '−'))
            .trim_end_matches('.')
            == expected
    })
}
fn same_slots(before: &Value, slots: &[Slot]) -> bool {
    before.as_array().is_some_and(|saved| {
        saved.iter().all(|s| {
            slots
                .iter()
                .filter(|slot| serde_json::to_value(*slot).ok().as_ref() == Some(s))
                .count()
                == 1
        })
    })
}
fn read_and_sum(o: &Observation, source: &str) -> Vec<Check> {
    vec![
        check(
            "owned_source_revision_read_once",
            o.evidence.iter().any(|e| {
                e.kind == "source"
                    && e.id == source
                    && e.revision == 1
                    && e.owner.as_deref() == Some(o.tenant.as_str())
            }) && o
                .slots
                .iter()
                .filter(|s| {
                    s.input["tool"] == "knowledge_read_source"
                        && s.input["id"] == source
                        && s.input["revision"] == 1
                        && s.result.as_ref().is_some_and(|r| {
                            r["kind"] == "source"
                                && r["value"]["source"]["id"] == source
                                && r["value"]["source"]["revision"] == 1
                        })
                })
                .count()
                == 1,
        ),
        check(
            "sum_receipt_once",
            o.slots
                .iter()
                .filter(|s| {
                    s.input["tool"] == "decimal_calculate"
                        && s.input["calculation"]
                            == json!({"operation":"sum","operands":["18.25","6.50"]})
                        && s.result.as_ref().is_some_and(|r| {
                            r["kind"] == "decimal" && r["value"]["exact"] == "24.75"
                        })
                })
                .count()
                == 1,
        ),
        check(
            "independent_exact_total_fact",
            fact(&o.output, "24.75") && o.output.contains("USD"),
        ),
    ]
}
// Independent civil-time arithmetic for this fixed modern Asia/Seoul fixture.
// No product slot-label formatting or product scheduling helper defines gold.
fn changed_time_retains_local_day(v: &Value) -> bool {
    let Some((created, changed, saved_at)) = v["created_saved"]["rule"]["at"]
        .as_i64()
        .zip(v["changed_saved"]["rule"]["at"].as_i64())
        .zip(v["created_saved"]["created_at"].as_i64())
        .map(|((created, changed), saved_at)| (created, changed, saved_at))
    else {
        return false;
    };
    let Some((local_created, local_changed, local_saved)) = created
        .checked_add(9 * 3600)
        .zip(changed.checked_add(9 * 3600))
        .zip(saved_at.checked_add(9 * 3600))
        .map(|((a, b), c)| (a, b, c))
    else {
        return false;
    };
    v["created_saved"]["rule"]["kind"] == "once"
        && v["changed_saved"]["rule"]["kind"] == "once"
        && changed.checked_sub(created) == Some(30 * 60)
        && local_created.rem_euclid(86400) == 10 * 3600
        && local_changed.rem_euclid(86400) == 10 * 3600 + 30 * 60
        && local_created.div_euclid(86400) == local_changed.div_euclid(86400)
        && local_created.div_euclid(86400) == local_saved.div_euclid(86400) + 1
}
pub fn grade(kind: Kind, out: &Execution) -> Vec<Check> {
    let Some(o) = out.observations.last() else {
        return vec![check("everyday_observation_present", false)];
    };
    let v = &out.outcome;
    match kind {
        Kind::EverydayGuidance => {
            let mut checks = completed(o, &out.account);
            checks.extend(read_and_sum(o, out.source.as_deref().unwrap_or("missing")));
            checks.extend([
                check(
                    "instruction_received_applied_same_attempt",
                    v["ack"]["state"] == "received"
                        && v["ack"] == v["replay"]
                        && v["receipt"]["state"] == "applied"
                        && v["receipt"]["run_id"] == o.run_id
                        && v["receipt"]["attempt"] == 1
                        && v["receipt"]["sequence"] == v["ack"]["sequence"],
                ),
                check("guidance_adds_shipping_fact", fact(&o.output, "6.50")),
                check(
                    "completed_work_preserved",
                    v["before_slots"].as_array().is_some_and(|s| s.len() == 3)
                        && same_slots(&v["before_slots"], &o.slots)
                        && v["reopened_slots"]
                            == serde_json::to_value(&o.slots).unwrap_or(Value::Null),
                ),
                check(
                    "result_persisted_after_reopen",
                    v["reopened_output"] == o.output,
                ),
                check(
                    "exactly_one_guidance_continuation",
                    o.ledger.len() == 5 && o.slots.len() == 3,
                ),
            ]);
            checks
        }
        Kind::EverydayClarification => {
            // A context answer requires semantic owner review. Product must
            // retain partial rather than converting arithmetic checks to success.
            let mut checks = completed(o, &out.account)
                .into_iter()
                .filter(|c| c.name != "actual_terminal_completion")
                .collect::<Vec<_>>();
            checks.extend(read_and_sum(o, out.source.as_deref().unwrap_or("missing")));
            checks.extend([
                check(
                    "owner_and_receipts",
                    o.account == out.account
                        && o.model_receipts == o.ledger.len()
                        && (1..=6).contains(&o.model_receipts)
                        && o.slots.iter().all(|s| s.result.is_some()),
                ),
                check(
                    "waiting_survives_reopen",
                    v["waiting"]["state"] == "waiting"
                        && v["reopened"]["state"] == "waiting"
                        && v["waiting"]["input_wait"] == v["reopened"]["input_wait"]
                        && v["waiting"]["input_wait"]["field"] == "context",
                ),
                check(
                    "original_acceptance_and_run_preserved",
                    v["waiting"]["request"] == v["final"]["request"]
                        && v["waiting"]["instruction_target"]["run_id"] == o.run_id
                        && v["final"]["instruction_target"]["run_id"] == o.run_id,
                ),
                check(
                    "completed_read_preserved_not_repeated",
                    v["before_slots"].as_array().is_some_and(|s| s.len() == 2)
                        && same_slots(&v["before_slots"], &o.slots)
                        && o.slots.len() == 3,
                ),
                check(
                    "context_answer_requires_owner_review",
                    v["final"]["state"] == "partial"
                        && o.mission.as_ref().is_some_and(|m| m.state == "partial")
                        && v["final"]["report"].as_array().is_some_and(|rs| {
                            rs.iter()
                                .any(|r| r.as_str().is_some_and(|s| s.contains("owner review")))
                        }),
                ),
                check(
                    "one_answer_resumed",
                    v["reply_count"] == 1
                        && v["final"]["inputs"].as_array().is_some_and(|inputs| {
                            inputs.len() == 1
                                && inputs[0]["state"] == "resumed"
                                && inputs[0]["value"]
                                    .as_str()
                                    .is_some_and(|s| s.to_lowercase().contains("standard"))
                        }),
                ),
                check(
                    "final_evidence_links_completed_work",
                    o.mission
                        .as_ref()
                        .and_then(|m| m.candidate["evidence"].as_array())
                        .is_some_and(|evidence| {
                            evidence.len() == o.slots.len()
                                && o.slots.iter().all(|slot| {
                                    evidence
                                        .iter()
                                        .filter(|e| {
                                            e["step"] == slot.step && e["call_id"] == slot.call_id
                                        })
                                        .count()
                                        == 1
                                })
                        }),
                ),
                check(
                    "selected_delivery_reflected",
                    o.output.to_lowercase().contains("standard"),
                ),
                check(
                    "only_admitted_read_arithmetic",
                    o.ledger.iter().flat_map(|d| &d.calls).all(|c| {
                        matches!(
                            c.name.as_str(),
                            "knowledge_search" | "knowledge_read_source" | "decimal_calculate"
                        )
                    }),
                ),
            ]);
            checks
        }
        Kind::EverydayReminder => {
            let id = &v["created"]["id"];
            let rows = v["schedules"].as_array();
            vec![
                check(
                    "receipt_bound_identity_chain",
                    id.is_string()
                        && v["changed"]["id"] == *id
                        && v["cancelled"]["id"] == *id
                        && *id != v["decoy"]["id"]
                        && ["created_saved", "changed_saved", "reopened_saved"]
                            .iter()
                            .all(|key| {
                                v[*key]["id"] == *id
                                    && v[*key]["account"] == out.account
                                    && v[*key]["tenant"] == o.tenant
                            }),
                ),
                check(
                    "revision_chain_create_edit_cancel",
                    v["created"]["revision"] == 1
                        && v["changed"]["revision"] == 2
                        && v["cancelled"]["revision"] == 3
                        && v["created"]["action"] == "create"
                        && v["changed"]["action"] == "edit"
                        && v["cancelled"]["state"] == "revoked"
                        && v["created_saved"]["revision"] == 1
                        && v["changed_saved"]["revision"] == 2
                        && v["created_saved"]["state"] == "active"
                        && v["changed_saved"]["state"] == "active"
                        && v["created_saved"]["title"] == v["changed_saved"]["title"]
                        && v["created_saved"]["message"] == v["changed_saved"]["message"],
                ),
                check(
                    "changed_time_retains_local_day",
                    changed_time_retains_local_day(v),
                ),
                check(
                    "exact_replay_same_run_and_receipt",
                    v["replay"]["id"] == v["changed_run"]
                        && v["replay"]["schedule_receipts"][0] == v["changed"]
                        && v["replayed_saved"] == v["changed_saved"],
                ),
                check(
                    "cancelled_persisted_decoy_untouched",
                    v["reopened_saved"]["id"] == *id
                        && v["reopened_saved"]["account"] == out.account
                        && v["reopened_saved"]["tenant"] == o.tenant
                        && v["reopened_saved"]["state"] == "revoked"
                        && v["reopened_saved"]["revision"] == 3
                        && v["reopened_saved"]["rule"] == v["changed_saved"]["rule"]
                        && v["decoy_before"].is_object()
                        && v["decoy_before"] == v["decoy_after"]
                        && rows.is_some_and(|rows| {
                            rows.len() == 1
                                && rows[0]["id"] == v["decoy"]["id"]
                                && rows[0]["revision"] == v["decoy"]["revision"]
                                && rows[0]["state"] == v["decoy"]["state"]
                        }),
                ),
                check(
                    "no_model_tool_or_cancelled_notification",
                    o.ledger.is_empty()
                        && o.model_receipts == 0
                        && o.slots.is_empty()
                        && v["notifications"] == 0,
                ),
            ]
        }
        _ => vec![check("everyday_kind", false)],
    }
}
pub fn mutations(kind: Kind, out: &Execution) -> Vec<Check> {
    let mut controls = vec![check(
        &format!("{kind:?}_positive_control"),
        all(&grade(kind, out)),
    )];
    let mut tampered = out.clone();
    let names = match kind {
        Kind::EverydayGuidance => {
            tampered.outcome["receipt"]["state"] = json!("received");
            (
                "everyday_guidance_missing_receipt_rejected",
                "everyday_guidance_repeated_tool_rejected",
            )
        }
        Kind::EverydayClarification => {
            tampered
                .observations
                .last_mut()
                .unwrap()
                .slots
                .retain(|s| s.input["tool"] != "knowledge_read_source");
            (
                "everyday_clarification_lost_work_rejected",
                "everyday_clarification_false_achievement_rejected",
            )
        }
        Kind::EverydayReminder => {
            tampered.outcome["cancelled"]["id"] = tampered.outcome["decoy"]["id"].clone();
            (
                "everyday_reminder_wrong_target_rejected",
                "everyday_reminder_duplicate_replay_rejected",
            )
        }
        _ => return vec![],
    };
    controls.push(check(names.0, !all(&grade(kind, &tampered))));
    let mut tampered = out.clone();
    match kind {
        Kind::EverydayGuidance => {
            let o = tampered.observations.last_mut().unwrap();
            if let Some(slot) = o.slots.first().cloned() {
                o.slots.push(slot);
            }
        }
        Kind::EverydayClarification => {
            tampered.outcome["final"]["state"] = json!("achieved");
        }
        Kind::EverydayReminder => {
            tampered.outcome["replay"]["id"] = json!("different-synthetic-run");
        }
        _ => {}
    }
    controls.push(check(names.1, !all(&grade(kind, &tampered))));
    if matches!(kind, Kind::EverydayReminder) {
        let mut time = out.clone();
        if let Some(at) = time.outcome["changed_saved"]["rule"]["at"]
            .as_i64()
            .and_then(|at| at.checked_add(60))
        {
            for key in ["changed_saved", "replayed_saved", "reopened_saved"] {
                time.outcome[key]["rule"]["at"] = json!(at);
            }
        }
        controls.push(check(
            "everyday_reminder_wrong_time_rejected",
            grade(kind, &time)
                .iter()
                .any(|c| c.name == "changed_time_retains_local_day" && !c.passed),
        ));
        let mut tombstone = out.clone();
        tombstone.outcome["reopened_saved"]["state"] = json!("active");
        controls.push(check(
            "everyday_reminder_missing_tombstone_rejected",
            grade(kind, &tombstone)
                .iter()
                .any(|c| c.name == "cancelled_persisted_decoy_untouched" && !c.passed),
        ));
    }
    controls
}
