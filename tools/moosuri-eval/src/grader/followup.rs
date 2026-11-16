//! Independent saved-source follow-up oracle; never used to construct requests.
use super::*;
use crate::runner::Execution;
use serde_json::json;
// Independent literal-quote gold for this explicitly requested quote task.
// Do not import agent-visible fixtures or derive gold from the subject's output.
const INITIAL_QUOTE: &str = "Synthetic return RMA-104: warehouse received the unopened kettle. Refund status: pending inspection. No refund has been issued.";
const CHANGED_QUOTE: &str = "Synthetic return RMA-104: inspection completed. Refund status: approved for USD 42.00. Payment settlement is still pending; approval does not prove payment.";
fn normalized_quote(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
fn quoted_state(o: &Observation, expected: &str) -> bool {
    let expected = normalized_quote(expected);
    normalized_quote(&o.output) == expected
        && o.mission
            .as_ref()
            .and_then(|m| m.candidate["text"].as_str())
            .is_some_and(|text| normalized_quote(text) == expected)
        && o.slots
            .first()
            .and_then(|slot| slot.result.as_ref())
            .and_then(|result| result["value"]["text"].as_str())
            .is_some_and(|text| normalized_quote(text) == expected)
}
fn notification_link(notice: &Value, child: &Value, schedule: &str) -> bool {
    child["id"]
        .as_str()
        .is_some_and(|id| !id.is_empty() && notice["mission_id"] == id)
        && notice["id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && child["notification_id"] == id)
        && child["schedule_id"] == schedule
        && notice["schedule_id"] == schedule
        && child["occurrence_id"]
            .as_str()
            .is_some_and(|id| !id.is_empty() && notice["occurrence_id"] == id)
}
fn exact_read(o: &Observation, source: &str, revision: i64) -> bool {
    o.slots.len() == 1
        && o.slots[0].input["tool"] == "knowledge_read_source"
        && o.slots[0].input["id"] == source
        && o.slots[0].input["revision"] == revision
        && o.slots[0].result.as_ref().is_some_and(|r| {
            r["kind"] == "source"
                && r["value"]["source"]["id"] == source
                && r["value"]["source"]["revision"] == revision
        })
        && o.evidence.iter().any(|e| {
            e.kind == "source"
                && e.id == source
                && e.revision == revision
                && e.owner.as_deref() == Some(o.tenant.as_str())
        })
        && o.mission
            .as_ref()
            .and_then(|m| m.candidate["evidence"].as_array())
            .is_some_and(|es| {
                es.len() == 1
                    && es[0]["step"] == o.slots[0].step
                    && es[0]["call_id"] == o.slots[0].call_id
            })
}
fn scope(snapshot: &Value, source: &str) -> bool {
    snapshot["monitor"]["scope"]["kind"] == "exact_sources"
        && snapshot["monitor"]["scope"]["source_ids"] == json!([source])
        && snapshot["monitor"]["scope"]["connection_pins"] == json!([])
        && snapshot["policy"]["scope"] == snapshot["monitor"]["scope"]
}
pub fn grade(out: &Execution) -> Vec<Check> {
    if out.observations.len() != 2 {
        return vec![check("followup_two_actual_runs", false)];
    }
    let parent = &out.observations[0];
    let child = &out.observations[1];
    let v = &out.outcome;
    let source = out.source.as_deref().unwrap_or("missing");
    let mut checks = completed(parent, &out.account);
    checks.extend(completed(child, &out.account));
    let accepted = &v["accepted"];
    let published = &v["published"];
    let reopened = &v["reopened"];
    let published_child = &published["children"][0];
    let schedule = published["monitor"]["schedule_id"]
        .as_str()
        .unwrap_or("missing-monitor-schedule");
    checks.extend([
        check(
            "followup_verified_parent_and_changed_read",
            exact_read(parent, source, 1)
                && exact_read(child, source, 2)
                && parent
                    .mission
                    .as_ref()
                    .is_some_and(|m| m.state == "achieved")
                && child
                    .mission
                    .as_ref()
                    .is_some_and(|m| m.state == "achieved"),
        ),
        check(
            "followup_status_facts_from_changed_revision",
            quoted_state(parent, INITIAL_QUOTE) && quoted_state(child, CHANGED_QUOTE),
        ),
        check(
            "followup_exact_source_scope_owned",
            source != v["decoy"].as_str().unwrap_or(source)
                && [accepted, published, reopened].iter().all(|s| {
                    scope(s, source)
                        && s["monitor"]["account"] == out.account
                        && s["policy"]["account"] == out.account
                        && s["monitor"]["tenant"] == child.tenant
                        && s["policy"]["tenant"] == child.tenant
                }),
        ),
        check(
            "followup_accept_replay_one_policy",
            v["active"] == v["accept_replay"]
                && v["active"]["state"] == "active"
                && accepted["policy_count"] == 1
                && accepted["monitor"]["parent"] == v["parent"]["id"]
                && accepted["policy"]["state"] == "active"
                && accepted["policy"]["cadence_seconds"] == 60,
        ),
        check(
            "followup_unchanged_skips_before_inference",
            v["unchanged"]["children"] == json!([])
                && v["unchanged"]["notifications"] == json!([])
                && v["unchanged"]["unchanged_occurrences"]
                    .as_i64()
                    .is_some_and(|n| n >= 1),
        ),
        check(
            "followup_one_changed_child_no_duplicate",
            published["children"].as_array().is_some_and(|cs| {
                cs.len() == 1
                    && cs[0]["ordinal"] == 1
                    && cs[0]["run"] == child.run_id
                    && cs[0]["state"] == "achieved"
                    && cs[0]["publication_state"] == "published"
            }) && v["duplicate"]["children"] == published["children"]
                && reopened["children"] == published["children"],
        ),
        check(
            "followup_one_real_visible_notification",
            published["notifications"].as_array().is_some_and(|ns| {
                ns.len() == 1
                    && ns[0]["account"] == out.account
                    && ns[0]["dismissed"] == false
                    && ns[0]["push_suppressed"] == false
                    && notification_link(&ns[0], published_child, schedule)
            }) && v["inbox"]["items"].as_array().is_some_and(|ns| {
                ns.len() == 1
                    && ns[0]["id"] == published["notifications"][0]["id"]
                    && notification_link(&ns[0], published_child, schedule)
            }) && v["duplicate"]["notifications"] == published["notifications"],
        ),
        check(
            "followup_stop_replay_revokes_policy_schedule",
            v["stopped"] == v["stop_replay"]
                && v["stopped"]["state"] == "stopped"
                && v["stopped"]["reason"] == "policy_revoked"
                && reopened["monitor"]["state"] == "stopped"
                && reopened["policy"]["state"] == "revoked"
                && reopened["policy"]["schedule_state"] == "revoked"
                && reopened["policy_count"] == 1
                && v["reopened_view"]["state"] == "stopped",
        ),
        check(
            "followup_notice_suppressed_after_stop_reopen",
            v["stopped_inbox"]["items"] == json!([])
                && v["reopened_inbox"]["items"] == json!([])
                && reopened["notifications"].as_array().is_some_and(|ns| {
                    ns.len() == 1
                        && ns[0]["id"] == published["notifications"][0]["id"]
                        && notification_link(&ns[0], published_child, schedule)
                        && ns[0]["dismissed"] == true
                        && ns[0]["push_suppressed"] == true
                        && ns[0]["source_suppressed"] == false
                }),
        ),
        check(
            "followup_synthetic_clock_bounded_and_disclosed",
            v["clock"]["base"]
                .as_i64()
                .zip(v["clock"]["expires_at"].as_i64())
                .is_some_and(|(base, end)| {
                    let Some(ticks) = v["clock"]["ticks"].as_array() else {
                        return false;
                    };
                    let mut last = base;
                    end.checked_sub(base) == Some(3600)
                        && accepted["policy"]["expires_at"] == end
                        && ticks.len() == 5
                        && ticks.iter().enumerate().all(|(i, t)| {
                            let valid = t["at"].as_i64().is_some_and(|at| {
                                let ok = at >= last && at < end;
                                last = at;
                                ok
                            });
                            valid && t["admitted"] == if i == 1 { 1 } else { 0 }
                        })
                }),
        ),
    ]);
    checks
}
pub fn mutations(out: &Execution) -> Vec<Check> {
    let mut controls = vec![check("followup_positive_control", all(&grade(out)))];
    let mut altered = out.clone();
    let child = altered.outcome["published"]["children"][0].clone();
    if let Some(children) = altered.outcome["published"]["children"].as_array_mut() {
        children.push(child);
    }
    controls.push(check(
        "followup_duplicate_child_rejected",
        !all(&grade(&altered)),
    ));
    let mut altered = out.clone();
    altered.outcome["accepted"]["monitor"]["scope"]["source_ids"] = json!([out.outcome["decoy"]]);
    controls.push(check(
        "followup_wrong_source_scope_rejected",
        !all(&grade(&altered)),
    ));
    let mut altered = out.clone();
    if let Some(child) = altered.observations.last_mut() {
        child.slots.clear();
    }
    controls.push(check(
        "followup_missing_changed_read_rejected",
        !all(&grade(&altered)),
    ));
    let mut altered = out.clone();
    altered.outcome["reopened_inbox"] = out.outcome["inbox"].clone();
    controls.push(check(
        "followup_visible_stopped_notice_rejected",
        !all(&grade(&altered)),
    ));
    let mut altered = out.clone();
    altered.outcome["reopened"]["policy"]["state"] = json!("active");
    controls.push(check(
        "followup_revived_policy_rejected",
        !all(&grade(&altered)),
    ));
    // Keep the visible notification ID and all other positive observations,
    // changing only the user-facing task link. Each exact link is contractual.
    for (field, name) in [
        ("mission_id", "followup_wrong_visible_mission_link_rejected"),
        (
            "schedule_id",
            "followup_wrong_visible_schedule_link_rejected",
        ),
        (
            "occurrence_id",
            "followup_wrong_visible_occurrence_link_rejected",
        ),
    ] {
        let mut altered = out.clone();
        altered.outcome["inbox"]["items"][0][field] = json!("wrong-synthetic-link");
        controls.push(check(
            name,
            grade(&altered)
                .iter()
                .any(|c| c.name == "followup_one_real_visible_notification" && !c.passed),
        ));
    }
    // Both mutants retain every original positive status keyword. Mutate the
    // persisted candidate and observed output together to model an actual bad
    // response, rather than relying on disagreement between observer copies.
    for (suffix, name) in [
        (
            " The refund was already paid.",
            "followup_contradictory_paid_status_rejected",
        ),
        (
            " The refund is not approved.",
            "followup_negated_approval_rejected",
        ),
    ] {
        let mut altered = out.clone();
        if let Some(child) = altered.observations.last_mut() {
            child.output.push_str(suffix);
            if let Some(mission) = child.mission.as_mut() {
                mission.candidate["text"] = json!(child.output);
            }
        }
        controls.push(check(
            name,
            grade(&altered)
                .iter()
                .any(|c| c.name == "followup_status_facts_from_changed_revision" && !c.passed),
        ));
    }
    controls
}
