//! Scripted apparatus: derives values solely from actual requests/tool results.
use super::*;
pub(super) fn next(model: &Model, input: &Value) -> EvalResult<(Vec<IssuedCall>, Option<String>)> {
    if matches!(model.script, Script::EverydayGuidance) {
        let calculation = Model {
            script: Script::Calculation,
            ledger: model.ledger.clone(),
            barrier: model.barrier.clone(),
        };
        let (calls, answer) = super::next(&calculation, input)?;
        let revised = input["messages"].as_array().is_some_and(|messages| {
            messages
                .iter()
                .any(|m| m["role"] == "user" && m["content"] == crate::everyday::GUIDANCE)
        });
        let answer = answer.map(|mut text| {
            if revised
                && let Ok(results) = tool_results(input)
                && let Some(source) = results
                    .iter()
                    .find(|r| r["kind"] == "source")
                    .and_then(|r| r["value"]["text"].as_str())
            {
                let words = source.split_whitespace().collect::<Vec<_>>();
                if let Some(shipping) = words.windows(2).find(|w| w[0] == "shipping") {
                    text.push_str(&format!(" Shipping USD = {}.", shipping[1]));
                }
            }
            text
        });
        return Ok((calls, answer));
    }
    if input["tools"]
        .as_array()
        .is_none_or(|tools| tools.is_empty())
    {
        return Ok((vec![], Some(json!({"steps":["Read the saved quote","Ask about delivery","Calculate and describe the selected option"]}).to_string())));
    }
    let context: Value = serde_json::from_str(
        input["messages"][1]["content"]
            .as_str()
            .ok_or("everyday_context")?,
    )
    .map_err(|_| "everyday_context")?;
    let receipts = context["receipts"].as_array().ok_or("everyday_receipts")?;
    let has_source = receipts.iter().any(|r| r["result"]["kind"] == "source");
    let owner_answer = context["owner_inputs"]
        .as_array()
        .and_then(|inputs| inputs.iter().find(|v| v["state"] == "resumed"))
        .and_then(|v| v["value"].as_str());
    if has_source && owner_answer.is_none() {
        return Ok((vec![], Some(json!({"input_request":{"field":"context","question":"Which delivery option should I describe?","candidates":[]}}).to_string())));
    }
    // The existing mission script already adapts saved receipt results into the
    // ordinary calculation protocol and returns references to those receipts.
    let mission = Model {
        script: Script::Mission,
        ledger: model.ledger.clone(),
        barrier: model.barrier.clone(),
    };
    let (calls, answer) = super::next(&mission, input)?;
    let answer = answer.map(|answer| {
        let mut candidate: Value = serde_json::from_str(&answer).unwrap_or(Value::Null);
        if let (Some(text), Some(owner)) = (candidate["text"].as_str(), owner_answer) {
            candidate["text"] = json!(format!("{text} Owner criterion: {owner}"));
        }
        candidate.to_string()
    });
    Ok((calls, answer))
}
