//! Scripted apparatus derives candidate pins and final text only from real receipts.
use super::*;
pub(super) fn next(input: &Value) -> EvalResult<(Vec<IssuedCall>, Option<String>)> {
    let context: Value = serde_json::from_str(
        input["messages"][1]["content"]
            .as_str()
            .ok_or("repair_choice_context")?,
    )
    .map_err(|_| "repair_choice_context")?;
    let receipts = context["receipts"]
        .as_array()
        .ok_or("repair_choice_receipts")?;
    let sources = receipts
        .iter()
        .filter(|r| r["result"]["kind"] == "source")
        .collect::<Vec<_>>();
    if sources.len() == 2 {
        if let Some(selected) = context["owner_inputs"]
            .as_array()
            .and_then(|inputs| {
                inputs
                    .iter()
                    .find(|i| i["field"] == "source_choice" && i["state"] == "resumed")
            })
            .and_then(|i| i["value"].as_str())
        {
            let chosen = sources
                .iter()
                .find(|r| r["result"]["value"]["source"]["id"] == selected)
                .ok_or("repair_choice_selected_receipt")?;
            let quote = chosen["result"]["value"]["text"]
                .as_str()
                .ok_or("repair_choice_quote")?;
            return Ok((vec![],Some(json!({"text":format!("Selected estimate: {quote}"),"evidence":[{"step":chosen["step"],"call_id":chosen["call_id"]}]}).to_string())));
        }
        let candidates=sources.iter().map(|r| {
            let source=&r["result"]["value"]["source"];
            json!({"id":source["id"],"revision":source["revision"].as_i64().unwrap_or(0).to_string(),"step":r["step"],"call_id":r["call_id"],"label":source["title"]})
        }).collect::<Vec<_>>();
        return Ok((vec![],Some(json!({"input_request":{"field":"source_choice","question":"Which appliance estimate should I quote?","candidates":candidates}}).to_string())));
    }
    if let Some(search) = receipts.iter().find(|r| r["result"]["kind"] == "search") {
        let hits = search["result"]["value"]["hits"]
            .as_array()
            .ok_or("repair_choice_search_hits")?;
        if hits.len() != 2 {
            return Err("repair_choice_two_estimates");
        }
        return Ok((hits.iter().map(|hit|call("knowledge_read_source",json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}))).collect(),None));
    }
    Ok((
        vec![call(
            "knowledge_search",
            json!({"kind":"source","text":"Synthetic repair estimate","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null}),
        )],
        None,
    ))
}
