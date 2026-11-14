//! Public protocol responses driven only by current requests and actual results.
use super::*;

pub(super) fn next(model: &Model, input: &Value) -> EvalResult<(Vec<IssuedCall>, Option<String>)> {
    let user = input["messages"]
        .as_array()
        .and_then(|messages| messages.iter().rev().find(|m| m["role"] == "user"))
        .and_then(|m| m["content"].as_str())
        .ok_or("fixture_current_user")?;
    let names = input["tools"]
        .as_array()
        .map(|tools| {
            tools
                .iter()
                .filter_map(|t| t["function"]["name"].as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if names.contains(&"memory_command_decision") {
        let mut decision = json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null});
        let action = if user.starts_with("Remember: ") {
            Some("remember")
        } else if user.starts_with("Correct my preference: ") {
            Some("correct")
        } else if user == "Forget my preference." {
            Some("forget")
        } else {
            None
        };
        if let Some(action) = action {
            decision["decision"] = json!("command");
            decision["action"] = json!(action);
            decision["intent_quote"] = json!(user);
            if action != "forget" {
                decision["fact_quote"] = json!(user.split_once(": ").ok_or("fixture_fact")?.1);
                decision["subject"] = json!("preference");
            }
            if action != "remember" {
                decision["target_quote"] = json!("preference");
            }
        }
        return Ok((vec![call("memory_command_decision", decision)], None));
    }
    if names.contains(&"mission_chat_decision") {
        return Ok((
            vec![call(
                "mission_chat_decision",
                json!({"decision":"no_command","action":null,"target_kind":null,"intent_quote":null,"task_quote":null,"constraints_quote":null,"time_quote":null,"target_quote":null,"instruction_quote":null,"time":null,"clarification":null}),
            )],
            None,
        ));
    }
    let results = tool_results(input)?;
    if matches!(model.script, Script::LearnedWorkflow) {
        if let Some(result) = results.iter().find(|r| r["kind"] == "decimal_workflow") {
            return Ok((
                vec![],
                Some(format!(
                    "Exact current amount: {}.",
                    result["value"]["exact"]
                        .as_str()
                        .ok_or("fixture_workflow_result")?
                )),
            ));
        }
        let inputs: Value = serde_json::from_str(
            user.split_once("inputs=")
                .ok_or("fixture_workflow_inputs")?
                .1,
        )
        .map_err(|_| "fixture_workflow_inputs")?;
        if user.starts_with("Workflow C ") {
            let line = input["messages"][0]["content"]
                .as_str()
                .and_then(|s| s.lines().find(|line| line.starts_with("private:")))
                .ok_or("fixture_learned_metadata")?;
            let fields = line.split_whitespace().collect::<Vec<_>>();
            let pin = json!({"id":fields[0],"revision":fields.get(1).and_then(|s| s.strip_prefix("revision=")).ok_or("fixture_learned_revision")?,"digest":fields.get(2).and_then(|s| s.strip_prefix("digest=")).ok_or("fixture_learned_digest")?.trim_end_matches(':')});
            return Ok((
                vec![call(
                    "learned_skill_execute",
                    json!({"pin":pin,"arguments":{"kind":"workflow","inputs":inputs}}),
                )],
                None,
            ));
        }
        return Ok((
            vec![call(
                "decimal_workflow",
                json!({"inputs":inputs,"graph":{"input_count":4,"steps":[{"operation":"product","operands":[{"kind":"input","index":0},{"kind":"input","index":1}]},{"operation":"sum","operands":[{"kind":"result","index":0},{"kind":"input","index":2}]},{"operation":"product","operands":[{"kind":"result","index":1},{"kind":"input","index":3}]}]}}),
            )],
            None,
        ));
    }
    if user == "Read the unrelated source." {
        if let Some(result) = results.iter().find(|r| r["kind"] == "source") {
            return Ok((
                vec![],
                Some(
                    result["value"]["text"]
                        .as_str()
                        .ok_or("fixture_unrelated")?
                        .into(),
                ),
            ));
        }
        if let Some(result) = results.iter().find(|r| r["kind"] == "search") {
            let hit = result["value"]["hits"]
                .as_array()
                .and_then(|h| h.first())
                .ok_or("fixture_unrelated_hit")?;
            return Ok((
                vec![call(
                    "knowledge_read_source",
                    json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}),
                )],
                None,
            ));
        }
        return Ok((
            vec![call(
                "knowledge_search",
                json!({"kind":"source","text":"UNRELATED_SOURCE","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null}),
            )],
            None,
        ));
    }
    if user.starts_with("Find the earlier assistant preference") {
        if let Some(result) = results.iter().find(|r| r["kind"] == "conversation_turn") {
            let text = result["value"]["messages"]
                .as_array()
                .ok_or("fixture_history")?
                .iter()
                .filter(|m| m["role"] == "assistant")
                .filter_map(|m| m["text"].as_str())
                .collect::<Vec<_>>()
                .join(" ");
            return Ok((vec![], Some(text)));
        }
        if let Some(result) = results.iter().find(|r| r["kind"] == "conversation_search") {
            return if let Some(hit) = result["value"]["hits"]
                .as_array()
                .and_then(|hits| hits.first())
            {
                Ok((
                    vec![call(
                        "conversation_read",
                        json!({"turn_id":hit["turn_id"],"revision":hit["revision"]}),
                    )],
                    None,
                ))
            } else {
                Ok((
                    vec![],
                    Some("No earlier preference answer is available.".into()),
                ))
            };
        }
        return Ok((
            vec![call(
                "conversation_search",
                json!({"query":"history-chain-anchor","scope":"owned_conversations"}),
            )],
            None,
        ));
    }
    // The existing real memory-search/read response logic receives the same request.
    let memory = Model {
        script: Script::Memory,
        ledger: model.ledger.clone(),
        barrier: model.barrier.clone(),
    };
    super::next(&memory, input)
}
