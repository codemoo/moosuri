//! Protocol apparatus using only accepted source pins and real returned receipts.
use super::*;
pub(super) fn next(input: &Value) -> EvalResult<(Vec<IssuedCall>, Option<String>)> {
    let context: Value = serde_json::from_str(
        input["messages"][1]["content"]
            .as_str()
            .ok_or("followup_model_context")?,
    )
    .map_err(|_| "followup_model_context")?;
    let receipts = context["receipts"]
        .as_array()
        .ok_or("followup_model_receipts")?;
    if let Some(receipt) = receipts.iter().find(|r| r["result"]["kind"] == "source") {
        let text = receipt["result"]["value"]["text"]
            .as_str()
            .ok_or("followup_model_source")?;
        return Ok((vec![],Some(json!({"text":text,"evidence":[{"step":receipt["step"],"call_id":receipt["call_id"]}]}).to_string())));
    }
    let source = context["accepted"]["checks"]
        .as_array()
        .and_then(|checks| checks.iter().find(|c| c["kind"] == "source_quote"))
        .ok_or("followup_model_source_pin")?;
    Ok((
        vec![call(
            "knowledge_read_source",
            json!({"id":source["id"],"revision":source["revision"],"start_byte":0,"max_bytes":4096}),
        )],
        None,
    ))
}
