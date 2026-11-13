use super::*;
use moosuri_connectors::model::protocol::{
    Protocol, TextCapabilities,
    tools::{ToolCapabilities, ToolRequest, ToolRoundDecoder},
};
fn round(args: &str, function: &str, duplicate_call: bool) -> ToolRound {
    let base = TextRequest {
        model: "synthetic-selector".into(),
        effort: None,
        max_output_tokens: 1024,
        messages: vec![
            TextMessage {
                role: TextRole::System,
                text: SYSTEM.into(),
                phase: None,
            },
            TextMessage {
                role: TextRole::User,
                text: "Synthetic target-selection data".into(),
                phase: None,
            },
        ],
    };
    let definitions = definitions();
    let tools = ToolRequest {
        text: &base,
        tools: &definitions,
        continuation: None,
        results: &[],
    };
    let mut decoder = ToolRoundDecoder::new(
        Protocol::ChatCompletions,
        &tools,
        &TextCapabilities::default(),
        &ToolCapabilities {
            function_tools: true,
        },
    )
    .unwrap();
    let call = json!({"index":0,"id":"target-call","type":"function","function":{"name":function,"arguments":args}});
    let mut calls = vec![call.clone()];
    if duplicate_call {
        let mut second = call;
        second["index"] = json!(1);
        second["id"] = json!("second");
        calls.push(second);
    }
    let start = json!({"id":"target-response","model":"synthetic-selector","choices":[{"index":0,"delta":{"role":"assistant","tool_calls":calls},"finish_reason":null}]});
    let end = json!({"id":"target-response","model":"synthetic-selector","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]});
    let wire = format!("data: {start}\n\ndata: {end}\n\ndata: [DONE]\n\n");
    for bytes in wire.as_bytes().chunks(5) {
        assert!(decoder.push(bytes).error.is_none());
    }
    decoder.finish().unwrap()
}
#[test]
fn index_only_completed_selection_or_fixed_clarification_is_accepted() {
    assert!(
        parse_completed(
            &round(r#"{"selection":"select","index":1}"#, FUNCTION, false),
            2
        ) == Some(MemoryTargetChoice::Select(1))
    );
    assert!(
        parse_completed(
            &round(r#"{"selection":"missing","index":null}"#, FUNCTION, false),
            2
        ) == Some(MemoryTargetChoice::Missing)
    );
    assert!(
        parse_completed(
            &round(r#"{"selection":"ambiguous","index":null}"#, FUNCTION, false),
            2
        ) == Some(MemoryTargetChoice::Ambiguous)
    );
}
#[test]
fn malformed_forged_authority_indexes_and_multiple_calls_are_rejected() {
    for args in [
        r#"{"selection":"select","index":2}"#,
        r#"{"selection":"select","index":-1}"#,
        r#"{"selection":"select","index":0.0}"#,
        r#"{"selection":"select","index":"0"}"#,
        r#"{"selection":"select","index":null}"#,
        r#"{"selection":"missing","index":0}"#,
        r#"{"selection":"ambiguous"}"#,
        r#"{"selection":"select","index":0,"index":1}"#,
        r#"{"selection":"select","index":0,"memory_id":"forged"}"#,
        r#"{"selection":"select","index":0,"tenant_id":"forged"}"#,
        r#"{"selection":"select","index":0,"action":"forget"}"#,
        r#"{"selection":"select","index":0,"fact_quote":"injected"}"#,
    ] {
        assert!(
            parse_completed(&round(args, FUNCTION, false), 2).is_none(),
            "unexpected accepted shape"
        );
    }
    assert!(
        parse_completed(
            &round(r#"{"selection":"select","index":0}"#, FUNCTION, true),
            2
        )
        .is_none()
    );
    let mut incomplete = round(r#"{"selection":"select","index":0}"#, FUNCTION, false);
    incomplete.outcome = FinishOutcome::Completed;
    assert!(parse_completed(&incomplete, 2).is_none());
}
