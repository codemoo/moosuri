//! Independent invalid-selection, persisted-work and cancellation oracle.
use super::*;
use crate::runner::Execution;
use serde_json::json;
fn candidates_are_real(v: &Value, tenant: &str) -> bool {
    let Some(waits) = v["waiting"]["waits"].as_array() else {
        return false;
    };
    let Some(slots) = v["before_slots"].as_array() else {
        return false;
    };
    if waits.len() != 1 || slots.len() != 3 || waits[0]["field"] != "source_choice" {
        return false;
    }
    let Some(candidates) = waits[0]["candidates"].as_array() else {
        return false;
    };
    if candidates.len() != 2 || candidates[0]["id"] == candidates[1]["id"] {
        return false;
    }
    candidates.iter().all(|candidate| {
        candidate["revision"] == "1"
            && slots
                .iter()
                .filter(|slot| {
                    slot["input"]["tool"] == "knowledge_read_source"
                        && slot["input"]["id"] == candidate["id"]
                        && slot["input"]["revision"] == 1
                        && slot["step"] == candidate["step"]
                        && slot["call_id"] == candidate["call_id"]
                        && slot["result"]["kind"] == "source"
                        && slot["result"]["value"]["source"]["id"] == candidate["id"]
                        && slot["result"]["value"]["source"]["revision"] == 1
                        && slot["result"]["value"]["source"]["title"] == candidate["label"]
                })
                .count()
                == 1
            && v["before_evidence"].as_array().is_some_and(|es| {
                es.iter().any(|e| {
                    e["kind"] == "source"
                        && e["id"] == candidate["id"]
                        && e["revision"] == 1
                        && e["owner"] == tenant
                })
            })
    })
}
fn known_estimates(v: &Value) -> bool {
    // Independently authored synthetic source facts, never fixture imports or
    // values derived from the model's chosen output.
    let expected = [
        (
            "Synthetic repair estimate — refrigerator",
            "Synthetic repair estimate RF-210: refrigerator door seal replacement, USD 64.00 including labor. No service has been booked.",
        ),
        (
            "Synthetic repair estimate — washing machine",
            "Synthetic repair estimate WM-305: washing machine inlet hose replacement, USD 48.00 including labor. No service has been booked.",
        ),
    ];
    v["before_slots"].as_array().is_some_and(|slots| {
        expected.iter().all(|(title, text)| {
            slots
                .iter()
                .filter(|s| {
                    s["result"]["kind"] == "source"
                        && s["result"]["value"]["source"]["title"] == *title
                        && s["result"]["value"]["text"] == *text
                })
                .count()
                == 1
        })
    })
}
fn original_context(v: &Value) -> bool {
    let first = &v["waiting"]["mission"];
    [
        "rejected",
        "corrected",
        "resumed",
        "cancelled",
        "live_settled",
        "final",
    ]
    .iter()
    .all(|key| {
        let current = &v[*key]["mission"];
        [
            "id",
            "run",
            "account",
            "tenant",
            "request",
            "expires_ms",
            "model_profile_sha256",
        ]
        .iter()
        .all(|field| current[*field] == first[*field])
            && v[*key]["execution"]["deadline_ms"] == v["waiting"]["execution"]["deadline_ms"]
    })
}
pub fn grade(out: &Execution) -> Vec<Check> {
    let Some(o) = out.observations.last() else {
        return vec![check("repair_choice_observation", false)];
    };
    let v = &out.outcome;
    let first = &v["waiting"];
    let final_state = &v["final"];
    let mut checks = vec![
        check(
            "repair_choice_actual_owned_candidates",
            candidates_are_real(v, &o.tenant)
                && known_estimates(v)
                && o.account == out.account
                && first["mission"]["account"] == out.account
                && first["mission"]["tenant"] == o.tenant
                && v["before_candidates"]
                    .as_array()
                    .is_some_and(|cs| cs.len() == 2 && cs.iter().all(|c| c["owner"] == o.tenant)),
        ),
        check(
            "repair_choice_invalid_preserves_wait_and_work",
            v["invalid_status"] == 400
                && v["rejected"] == *first
                && first["mission"]["state"] == "waiting"
                && first["waits"][0]["state"] == "waiting"
                && first["waits"][0]["value"].is_null()
                && first["replies"] == json!([])
                && first["notices"].as_array().is_some_and(|ns| {
                    ns.len() == 1
                        && ns[0]["id"] == first["waits"][0]["notification_id"]
                        && ns[0]["dismissed"] == false
                }),
        ),
        check(
            "repair_choice_original_authority_and_deadline",
            original_context(v)
                && o.run_id == first["mission"]["run"]
                && first["waits"][0]["acceptance_revision"] == 1,
        ),
        check(
            "repair_choice_corrects_one_real_selection",
            v["selected"]["label"] == "Synthetic repair estimate — refrigerator"
                && first["waits"][0]["candidates"]
                    .as_array()
                    .is_some_and(|cs| cs.contains(&v["selected"]))
                && v["corrected"]["replies"].as_array().is_some_and(|rs| {
                    rs.len() == 1
                        && rs[0]["request_id"] != v["ambiguous_request_id"]
                        && rs[0]["payload"]["wait_request_id"] == first["waits"][0]["request_id"]
                        && rs[0]["payload"]["value"] == v["selected"]["id"]
                })
                && v["corrected_view"] == v["reply_replay"]
                && v["resumed"]["waits"][0]["state"] == "resumed"
                && v["resumed"]["waits"][0]["value"] == v["selected"]["id"]
                && v["resumed"]["replies"] == v["corrected"]["replies"]
                && v["final"]["replies"] == v["corrected"]["replies"],
        ),
        check(
            "repair_choice_wait_identity_survives_correction",
            ["corrected", "resumed", "cancelled", "final"]
                .iter()
                .all(|key| {
                    v[*key]["waits"].as_array().is_some_and(|ws| {
                        ws.len() == 1
                            && [
                                "request_id",
                                "field",
                                "acceptance_revision",
                                "expires_ms",
                                "candidates",
                                "notification_id",
                            ]
                            .iter()
                            .all(|field| ws[0][*field] == first["waits"][0][*field])
                    })
                }),
        ),
        check(
            "repair_choice_no_repeated_completed_reads",
            v["before_slots"].as_array().is_some_and(|ss| ss.len() == 3)
                && v["resumed_slots"] == v["before_slots"]
                && serde_json::to_value(&o.slots).ok().as_ref() == Some(&v["before_slots"])
                && first["execution"]["remote_calls"] == 3
                && first["execution"]["tool_calls"] == 3
                && v["resumed"]["execution"]["remote_calls"] == 4
                && v["resumed"]["execution"]["tool_calls"] == 3
                && final_state["execution"]["remote_calls"] == 4
                && final_state["execution"]["tool_calls"] == 3,
        ),
        check(
            "repair_choice_worker_settled_before_shutdown",
            v["runtime_settlement"]["before_release"] == 1
                && v["runtime_settlement"]["after_release"] == 0
                && v["runtime_settlement"]["runtime_alive"] == true
                && v["runtime_settlement"]["watcher_open"] == true
                && v["live_settled"]["mission"]["state"] == "cancelled"
                && v["live_settled"]["mission"]["grant"] == false
                && v["live_settled"]["mission"]["candidate"].is_null()
                && v["live_settled"]["controls"] == 1
                && v["live_settled"]["effects"]
                    == json!({"artifacts":0,"mail":0,"calendar":0,"chat_output":0})
                && v["live_inbox"]["items"] == json!([]),
        ),
        check(
            "repair_choice_cancelled_not_late_success",
            o.state == "cancelled"
                && o.output.is_empty()
                && o.mission
                    .as_ref()
                    .is_some_and(|m| m.state == "cancelled" && m.candidate.is_null())
                && v["cancelled_view"] == v["cancel_replay"]
                && v["cancelled"]["controls"] == 1
                && final_state["controls"] == 1
                && final_state["mission"]["state"] == "cancelled"
                && final_state["mission"]["grant"] == false
                && final_state["mission"]["uncertain"] == true
                && final_state["mission"]["candidate"].is_null(),
        ),
        check(
            "repair_choice_no_effect_or_visible_notice",
            [
                first,
                &v["rejected"],
                &v["corrected"],
                &v["resumed"],
                &v["cancelled"],
                final_state,
            ]
            .iter()
            .all(|s| s["effects"] == json!({"artifacts":0,"mail":0,"calendar":0,"chat_output":0}))
                && v["inbox"]["items"] == json!([])
                && final_state["notices"].as_array().is_some_and(|ns| {
                    ns.len() == 1
                        && ns[0]["id"] == first["notices"][0]["id"]
                        && ns[0]["dismissed"] == true
                        && ns[0]["push_suppressed"] == true
                }),
        ),
        check(
            "repair_choice_real_bounded_dispatch_receipts",
            o.ledger.len() == 4
                && o.model_receipts == 4
                && o.model_receipt_steps == [1, 2, 3, 4]
                && o.ledger.iter().all(|d| d.terminal)
                && o.ledger.iter().flat_map(|d| &d.calls).count() == 3
                && o.ledger.iter().flat_map(|d| &d.calls).all(|c| {
                    matches!(
                        c.name.as_str(),
                        "knowledge_search" | "knowledge_read_source"
                    )
                }),
        ),
    ];
    // Receipt links are step-specific even though this task deliberately never
    // reaches ordinary final completion after the held response is cancelled.
    checks.push(check(
        "repair_choice_tool_receipts_link_dispatches",
        o.slots.iter().all(|s| {
            o.ledger
                .iter()
                .enumerate()
                .flat_map(|(i, d)| d.calls.iter().map(move |c| (i as i64 + 1, c)))
                .filter(|(step, c)| super::linked(s, c, *step))
                .count()
                == 1
        }),
    ));
    checks
}
pub fn mutations(out: &Execution) -> Vec<Check> {
    let mut controls = vec![check("repair_choice_positive_control", all(&grade(out)))];
    type Mutation = (&'static str, &'static str, fn(&mut Execution));
    let mutations: [Mutation; 8] = [
        (
            "repair_choice_missing_live_settlement_rejected",
            "repair_choice_worker_settled_before_shutdown",
            |o| {
                o.outcome["runtime_settlement"]["after_release"] = json!(1);
            },
        ),
        (
            "repair_choice_accepted_ambiguous_answer_rejected",
            "repair_choice_invalid_preserves_wait_and_work",
            |o| o.outcome["rejected"]["replies"] = json!([{"value":"Whichever is cheaper."}]),
        ),
        (
            "repair_choice_replaced_wait_rejected",
            "repair_choice_wait_identity_survives_correction",
            |o| o.outcome["resumed"]["waits"][0]["request_id"] = json!("replacement-wait"),
        ),
        (
            "repair_choice_forged_candidate_rejected",
            "repair_choice_actual_owned_candidates",
            |o| {
                o.outcome["waiting"]["waits"][0]["candidates"][0]["call_id"] =
                    json!("invented-call")
            },
        ),
        (
            "repair_choice_repeated_read_rejected",
            "repair_choice_no_repeated_completed_reads",
            |o| {
                if let Some(slot) = o.observations.last().and_then(|s| s.slots.first()).cloned() {
                    o.observations.last_mut().unwrap().slots.push(slot);
                }
            },
        ),
        (
            "repair_choice_duplicate_reply_rejected",
            "repair_choice_corrects_one_real_selection",
            |o| {
                let first = o.outcome["final"]["replies"][0].clone();
                if let Some(rs) = o.outcome["final"]["replies"].as_array_mut() {
                    rs.push(first);
                }
            },
        ),
        (
            "repair_choice_late_result_rejected",
            "repair_choice_cancelled_not_late_success",
            |o| {
                o.outcome["final"]["mission"]["candidate"] =
                    json!({"text":"Late selected estimate","evidence":[]});
            },
        ),
        (
            "repair_choice_visible_notice_rejected",
            "repair_choice_no_effect_or_visible_notice",
            |o| o.outcome["inbox"]["items"] = json!([{"id":"retired-wait-notice"}]),
        ),
    ];
    for (name, target, mutate) in mutations {
        let mut altered = out.clone();
        mutate(&mut altered);
        controls.push(check(
            name,
            grade(&altered)
                .iter()
                .any(|c| c.name == target && !c.passed),
        ));
    }
    controls
}
