use super::*;
fn text() -> TextRequest {
    TextRequest {
        model: "fixture-model".into(),
        messages: vec![TextMessage {
            role: TextRole::User,
            text: "합성 검색".into(),
            phase: None,
        }],
        max_output_tokens: 128,
        effort: None,
    }
}
fn bounded_definitions(count: usize) -> Vec<FunctionDefinition> {
    (0..count).map(|index| FunctionDefinition {
        name: if index == 0 { "knowledge_search".into() } else { format!("synthetic_read_{index}") },
        description: "Synthetic bounded read definition".into(),
        parameters: json!({"type":"object","properties":{"q":{"type":"string"}},"required":["q"],"additionalProperties":false}),
    }).collect()
}

#[test]
fn definition_ceiling_accepts_32_without_truncation_and_rejects_33_in_every_dialect() {
    let text = text();
    let caps = ToolCapabilities {
        function_tools: true,
    };
    let text_caps = TextCapabilities::default();
    for count in [32, 33] {
        let definitions = bounded_definitions(count);
        let request = ToolRequest {
            text: &text,
            tools: &definitions,
            continuation: None,
            results: &[],
        };
        for protocol in [
            Protocol::Responses,
            Protocol::AnthropicMessages,
            Protocol::OpenRouterChat,
            Protocol::ChatCompletions,
        ] {
            let result = encode_tool_request(protocol, &request, &text_caps, &caps);
            if count == 32 {
                let body = result.unwrap();
                assert_eq!(body["tools"].as_array().unwrap().len(), 32);
            } else {
                assert_eq!(result.err(), Some(ProtocolError::LimitExceeded));
            }
        }
        let result = encode_chatgpt_plan_tool_request(&request, &text_caps, &caps);
        if count == 32 {
            assert_eq!(
                result.unwrap()["tools"][0]["tools"]
                    .as_array()
                    .unwrap()
                    .len(),
                32
            );
        } else {
            assert_eq!(result.err(), Some(ProtocolError::LimitExceeded));
        }
    }
}

#[test]
fn larger_definition_count_retains_aggregate_schema_budget_and_closed_names() {
    let text = text();
    let caps = ToolCapabilities {
        function_tools: true,
    };
    let text_caps = TextCapabilities::default();
    let mut definitions = bounded_definitions(32);
    // Each schema/description is individually valid; only their 64KiB sum fails.
    for definition in &mut definitions {
        definition.description = "d".repeat(1024);
        definition.parameters = json!({"type":"object","properties":{
            "q":{"type":"string","description":"q".repeat(1024)},
            "other":{"type":"string","description":"o".repeat(1024)}
        },"required":["q","other"],"additionalProperties":false});
        assert!(serde_json::to_vec(&definition.parameters).unwrap().len() < 16 * 1024);
    }
    let request = ToolRequest {
        text: &text,
        tools: &definitions,
        continuation: None,
        results: &[],
    };
    assert_eq!(
        encode_tool_request(Protocol::Responses, &request, &text_caps, &caps).err(),
        Some(ProtocolError::LimitExceeded)
    );
    assert_eq!(
        encode_chatgpt_plan_tool_request(&request, &text_caps, &caps).err(),
        Some(ProtocolError::LimitExceeded)
    );

    let definitions = bounded_definitions(32);
    let request = ToolRequest {
        text: &text,
        tools: &definitions,
        continuation: None,
        results: &[],
    };
    for plan in [false, true] {
        let mut decoder = if plan {
            ToolRoundDecoder::chatgpt_plan(&request, &text_caps, &caps)
        } else {
            ToolRoundDecoder::new(Protocol::Responses, &request, &text_caps, &caps)
        }
        .unwrap();
        let mut frames = if plan {
            plan_frames()
        } else {
            response_frames()
        };
        frames[9]["item"]["name"] = json!("not_in_admitted_definitions");
        let rejected = decoder
            .push(wire(Protocol::Responses, &frames).as_bytes())
            .error;
        assert!(
            rejected.is_some(),
            "raising the ceiling must not admit unknown calls"
        );
    }
}
fn tools() -> Vec<FunctionDefinition> {
    vec![FunctionDefinition {
        name: "knowledge_search".into(),
        description: "Search synthetic private data".into(),
        parameters: json!({"type":"object","properties":{"q":{"type":"string"}},"required":["q"],"additionalProperties":false}),
    }]
}
fn frame(v: Value) -> String {
    format!("data: {v}\n\n")
}
fn named(v: Value) -> String {
    format!("event: {}\ndata: {v}\n\n", v["type"].as_str().unwrap())
}
fn response_frames() -> Vec<Value> {
    let reasoning = json!({"type":"reasoning","id":"reason-1","summary":[{"type":"summary_text","text":"private synthetic summary"}],"encrypted_content":"opaque-reasoning"});
    let part = json!({"type":"output_text","text":"찾아봅니다","annotations":[]});
    let msg = json!({"type":"message","id":"msg-1","role":"assistant","phase":"commentary","status":"completed","content":[part.clone()]});
    let call = json!({"type":"function_call","id":"fc-1","call_id":"call-1","name":"knowledge_search","arguments":"{\"q\":\"홍차\"}","status":"completed"});
    vec![
        json!({"type":"response.created","response":{"id":"resp-1","model":"fixture-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"reason-1","type":"reasoning","summary":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":reasoning.clone()}),
        json!({"type":"response.output_item.added","output_index":1,"item":{"type":"message","id":"msg-1","role":"assistant","phase":"commentary","content":[]}}),
        json!({"type":"response.content_part.added","output_index":1,"item_id":"msg-1","content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","output_index":1,"item_id":"msg-1","content_index":0,"delta":"찾아봅니다"}),
        json!({"type":"response.output_text.done","output_index":1,"item_id":"msg-1","content_index":0,"text":"찾아봅니다"}),
        json!({"type":"response.content_part.done","output_index":1,"item_id":"msg-1","content_index":0,"part":part}),
        json!({"type":"response.output_item.done","output_index":1,"item":msg.clone()}),
        json!({"type":"response.output_item.added","output_index":2,"item":{"type":"function_call","id":"fc-1","call_id":"call-1","name":"knowledge_search","arguments":"","status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc-1","delta":"{\"q\":"}),
        json!({"type":"response.function_call_arguments.delta","output_index":2,"item_id":"fc-1","delta":"\"홍차\"}"}),
        json!({"type":"response.function_call_arguments.done","output_index":2,"item_id":"fc-1","arguments":"{\"q\":\"홍차\"}"}),
        json!({"type":"response.output_item.done","output_index":2,"item":call.clone()}),
        json!({"type":"response.completed","response":{"id":"resp-1","model":"fixture-model","status":"completed","output":[reasoning,msg,call],"usage":{"input_tokens":10,"output_tokens":3}}}),
    ]
}
fn anthropic_frames() -> Vec<Value> {
    vec![
        json!({"type":"message_start","message":{"id":"resp-1","model":"fixture-model","role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":0}}}),
        json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":"","signature":""}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"private synthetic thinking"}}),
        json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"opaque-signature"}}),
        json!({"type":"content_block_stop","index":0}),
        json!({"type":"content_block_start","index":1,"content_block":{"type":"redacted_thinking","data":"opaque-redaction"}}),
        json!({"type":"content_block_stop","index":1}),
        json!({"type":"content_block_start","index":2,"content_block":{"type":"text","text":"찾아봅니다"}}),
        json!({"type":"content_block_stop","index":2}),
        json!({"type":"content_block_start","index":3,"content_block":{"type":"tool_use","id":"call-1","name":"knowledge_search","input":{}}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"{\"q\":"}}),
        json!({"type":"content_block_delta","index":3,"delta":{"type":"input_json_delta","partial_json":"\"홍차\"}"}}),
        json!({"type":"content_block_stop","index":3}),
        json!({"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":3}}),
        json!({"type":"message_stop"}),
    ]
}
fn chat_frames() -> Vec<Value> {
    let delta = |d: Value, reason: Value| json!({"id":"resp-1","model":"fixture-model","choices":[{"index":0,"delta":d,"finish_reason":reason}]});
    vec![
        delta(
            json!({"role":"assistant","content":"찾아봅니다","reasoning_details":[{"type":"reasoning.text","id":"reason-1","index":0,"format":"synthetic","text":"private "}]}),
            Value::Null,
        ),
        delta(
            json!({"reasoning_details":[{"type":"reasoning.text","id":"reason-1","index":0,"format":"synthetic","text":"thinking","signature":"opaque-signature"}],"tool_calls":[{"index":0,"id":"call-1","type":"function","function":{"name":"knowledge_search","arguments":"{\"q\":"}}]}),
            Value::Null,
        ),
        delta(
            json!({"tool_calls":[{"index":0,"function":{"arguments":"\"홍차\"}"}}]}),
            Value::Null,
        ),
        delta(json!({}), json!("tool_calls")),
        json!({"id":"resp-1","model":"fixture-model","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3}}),
    ]
}
fn wire(p: Protocol, frames: &[Value]) -> String {
    let mut s = frames
        .iter()
        .map(|v| {
            if p == Protocol::AnthropicMessages {
                named(v.clone())
            } else {
                frame(v.clone())
            }
        })
        .collect::<String>();
    if matches!(p, Protocol::ChatCompletions | Protocol::OpenRouterChat) {
        s.push_str("data: [DONE]\n\n");
    }
    s
}
fn decode(
    p: Protocol,
    wire: &str,
    chunk: usize,
) -> Result<(ToolRound, Vec<ModelEvent>), ProtocolError> {
    let t = text();
    let ts = tools();
    let request = ToolRequest {
        text: &t,
        tools: &ts,
        continuation: None,
        results: &[],
    };
    let mut decoder = ToolRoundDecoder::new(
        p,
        &request,
        &TextCapabilities::default(),
        &ToolCapabilities {
            function_tools: true,
        },
    )?;
    let mut events = vec![];
    for bytes in wire.as_bytes().chunks(chunk) {
        let batch = decoder.push(bytes);
        events.extend(batch.events);
        if let Some(e) = batch.error {
            return Err(e);
        }
    }
    Ok((decoder.finish()?, events))
}
#[test]
fn all_dialects_fragmented_rounds_preserve_private_continuation_and_result_ids() {
    for (p, frames) in [
        (Protocol::Responses, response_frames()),
        (Protocol::AnthropicMessages, anthropic_frames()),
        (Protocol::OpenRouterChat, chat_frames()),
        (Protocol::ChatCompletions, chat_frames()),
    ] {
        for size in [1, 7, 65536] {
            let (round, events) = decode(p, &wire(p, &frames), size).unwrap();
            assert_eq!(round.calls.len(), 1);
            assert_eq!(round.calls[0].call_id(), "call-1");
            assert_eq!(round.calls[0].name(), "knowledge_search");
            assert_eq!(round.calls[0].arguments(), "{\"q\":\"홍차\"}");
            assert_eq!(round.outcome, FinishOutcome::ContinuationRequired);
            assert_eq!(round.usage.output_tokens, Some(3));
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(e, ModelEvent::Finished { .. }))
            );
            let checkpoint = round.continuation.to_private_checkpoint().unwrap();
            let restored =
                Continuation::from_private_checkpoint(&checkpoint, p, "fixture-model", None)
                    .unwrap();
            assert!(
                Continuation::from_private_checkpoint(&checkpoint, p, "wrong-model", None).is_err()
            );
            let t = text();
            let ts = tools();
            let results = [ToolResult {
                call_id: "call-1".into(),
                output: "{\"hits\":[]}".into(),
                is_error: false,
            }];
            let encoded = encode_tool_request(
                p,
                &ToolRequest {
                    text: &t,
                    tools: &ts,
                    continuation: Some(&restored),
                    results: &results,
                },
                &TextCapabilities::default(),
                &ToolCapabilities {
                    function_tools: true,
                },
            )
            .unwrap();
            match p {
                Protocol::Responses => {
                    let input = encoded["input"].as_array().unwrap();
                    assert_eq!(input[1]["encrypted_content"], "opaque-reasoning");
                    assert_eq!(input[2]["phase"], "commentary");
                    assert_eq!(input[3]["call_id"], input[4]["call_id"]);
                }
                Protocol::AnthropicMessages => {
                    assert_eq!(
                        encoded["messages"][1]["content"][0]["signature"],
                        "opaque-signature"
                    );
                    assert_eq!(
                        encoded["messages"][1]["content"][1]["data"],
                        "opaque-redaction"
                    );
                    assert_eq!(
                        encoded["messages"][2]["content"][0]["tool_use_id"],
                        "call-1"
                    );
                }
                _ => {
                    assert_eq!(
                        encoded["messages"][1]["reasoning_details"][0]["text"],
                        "private thinking"
                    );
                    assert_eq!(
                        encoded["messages"][1]["reasoning_details"][0]["signature"],
                        "opaque-signature"
                    );
                    assert_eq!(encoded["messages"][2]["tool_call_id"], "call-1");
                }
            }
        }
    }
}
#[test]
fn incomplete_or_changed_calls_never_return_executable_round() {
    let mut mutations = vec![];
    let frames = response_frames();
    for index in [12, 13, 14] {
        let mut f = frames.clone();
        if index == 12 {
            f[index]["arguments"] = json!("{}");
        } else if index == 13 {
            f[index]["item"]["call_id"] = json!("changed");
        } else {
            f[index]["response"]["output"][2]["name"] = json!("changed");
        }
        mutations.push(f);
    }
    let mut f = frames.clone();
    f[10]["item_id"] = json!("wrong-id");
    mutations.push(f);
    let mut f = frames.clone();
    f[9]["item"]["name"] = json!("unknown_tool");
    mutations.push(f);
    let mut f = frames.clone();
    f[0]["response"]["model"] = json!("wrong-model");
    mutations.push(f);
    let mut f = frames.clone();
    f.pop();
    mutations.push(f);
    let mut f = frames.clone();
    f[10]["delta"] = json!("x".repeat(MAX_ARGUMENTS + 1));
    mutations.push(f);
    for f in mutations {
        assert!(decode(Protocol::Responses, &wire(Protocol::Responses, &f), 7).is_err());
    }
    let mut f = anthropic_frames();
    f[13]["delta"]["stop_reason"] = json!("max_tokens");
    assert!(
        decode(
            Protocol::AnthropicMessages,
            &wire(Protocol::AnthropicMessages, &f),
            1
        )
        .is_err()
    );
    for bad in [
        json!({"index":0,"id":"changed","function":{"arguments":""}}),
        json!({"index":0,"function":{"name":"changed","arguments":""}}),
    ] {
        let mut f = chat_frames();
        f[2]["choices"][0]["delta"]["tool_calls"] = json!([bad]);
        assert!(
            decode(
                Protocol::ChatCompletions,
                &wire(Protocol::ChatCompletions, &f),
                1
            )
            .is_err()
        );
    }
}
#[test]
fn request_validation_caps_and_tool_results_are_closed_and_bound() {
    let t = text();
    let mut ts = tools();
    let request = ToolRequest {
        text: &t,
        tools: &ts,
        continuation: None,
        results: &[],
    };
    assert!(matches!(
        encode_tool_request(
            Protocol::Responses,
            &request,
            &TextCapabilities::default(),
            &ToolCapabilities::default()
        ),
        Err(ProtocolError::UnsupportedCapability)
    ));
    ts[0].parameters["additionalProperties"] = json!(true);
    assert!(
        encode_tool_request(
            Protocol::Responses,
            &ToolRequest {
                text: &t,
                tools: &ts,
                continuation: None,
                results: &[]
            },
            &TextCapabilities::default(),
            &ToolCapabilities {
                function_tools: true
            }
        )
        .is_err()
    );
    let (round, _) = decode(
        Protocol::Responses,
        &wire(Protocol::Responses, &response_frames()),
        7,
    )
    .unwrap();
    let ts = tools();
    for results in [
        vec![],
        vec![ToolResult {
            call_id: "wrong".into(),
            output: "ok".into(),
            is_error: false,
        }],
        vec![ToolResult {
            call_id: "call-1".into(),
            output: "x".repeat(MAX_ARGUMENTS + 1),
            is_error: false,
        }],
    ] {
        assert!(
            encode_tool_request(
                Protocol::Responses,
                &ToolRequest {
                    text: &t,
                    tools: &ts,
                    continuation: Some(&round.continuation),
                    results: &results
                },
                &TextCapabilities::default(),
                &ToolCapabilities {
                    function_tools: true
                }
            )
            .is_err()
        );
    }
}

fn many_response_calls(n: usize) -> Vec<Value> {
    let mut frames =
        vec![json!({"type":"response.created","response":{"id":"resp-1","model":"fixture-model"}})];
    let mut output = vec![];
    for i in 0..n {
        frames.push(json!({"type":"response.output_item.added","output_index":i,"item":{"type":"function_call","id":format!("fc-{i}"),"call_id":format!("call-{i}"),"name":"knowledge_search","arguments":"","status":"in_progress"}}));
    }
    for suffix in ["{\"q\":", "\"홍차\"}"] {
        for i in 0..n {
            frames.push(json!({"type":"response.function_call_arguments.delta","output_index":i,"item_id":format!("fc-{i}"),"delta":suffix}));
        }
    }
    for i in 0..n {
        frames.push(json!({"type":"response.function_call_arguments.done","output_index":i,"item_id":format!("fc-{i}"),"arguments":"{\"q\":\"홍차\"}"}));
        let item = json!({"type":"function_call","id":format!("fc-{i}"),"call_id":format!("call-{i}"),"name":"knowledge_search","arguments":"{\"q\":\"홍차\"}","status":"completed"});
        frames
            .push(json!({"type":"response.output_item.done","output_index":i,"item":item.clone()}));
        output.push(item);
    }
    frames.push(json!({"type":"response.completed","response":{"id":"resp-1","model":"fixture-model","status":"completed","output":output}}));
    frames
}
#[test]
fn interleaved_calls_have_unique_identity_and_hard_call_bound() {
    let (round, _) = decode(
        Protocol::Responses,
        &wire(Protocol::Responses, &many_response_calls(12)),
        3,
    )
    .unwrap();
    assert_eq!(round.calls.len(), 12);
    assert!(matches!(
        decode(
            Protocol::Responses,
            &wire(Protocol::Responses, &many_response_calls(13)),
            1
        ),
        Err(ProtocolError::LimitExceeded)
    ));
    let mut duplicate = many_response_calls(2);
    duplicate[2]["item"]["call_id"] = json!("call-0");
    assert!(
        decode(
            Protocol::Responses,
            &wire(Protocol::Responses, &duplicate),
            7
        )
        .is_err()
    );
    let mut f = chat_frames();
    f[1]["choices"][0]["delta"]["tool_calls"].as_array_mut().unwrap().push(json!({"index":1,"id":"call-2","type":"function","function":{"name":"knowledge_search","arguments":"{\"q\":"}}));
    f[2]["choices"][0]["delta"]["tool_calls"]
        .as_array_mut()
        .unwrap()
        .push(json!({"index":1,"function":{"arguments":"\"커피\"}"}}));
    let (round, _) = decode(
        Protocol::OpenRouterChat,
        &wire(Protocol::OpenRouterChat, &f),
        1,
    )
    .unwrap();
    assert_eq!(round.calls.len(), 2);
    assert_eq!(round.calls[1].arguments(), "{\"q\":\"커피\"}");
}
#[test]
fn checkpoints_refuse_forged_pending_ids_unknown_blocks_and_mismatched_pins() {
    let (round, _) = decode(
        Protocol::Responses,
        &wire(Protocol::Responses, &response_frames()),
        7,
    )
    .unwrap();
    let bytes = round.continuation.to_private_checkpoint().unwrap();
    let mut v: Value = serde_json::from_slice(&bytes).unwrap();
    v["pending"][0][0] = json!("unrelated-call");
    assert!(
        Continuation::from_private_checkpoint(
            &serde_json::to_vec(&v).unwrap(),
            Protocol::Responses,
            "fixture-model",
            None
        )
        .is_err()
    );
    assert!(
        Continuation::from_private_checkpoint(
            &bytes,
            Protocol::AnthropicMessages,
            "fixture-model",
            None
        )
        .is_err()
    );
    assert!(
        Continuation::from_private_checkpoint(
            &bytes,
            Protocol::Responses,
            "fixture-model",
            Some("high")
        )
        .is_err()
    );
    let mut f = response_frames();
    f[1]["item"]["type"] = json!("computer_call");
    assert!(matches!(
        decode(Protocol::Responses, &wire(Protocol::Responses, &f), 1),
        Err(ProtocolError::UnsupportedCapability)
    ));
    let mut f = anthropic_frames();
    f[1]["content_block"]["type"] = json!("server_tool_use");
    assert!(matches!(
        decode(
            Protocol::AnthropicMessages,
            &wire(Protocol::AnthropicMessages, &f),
            7
        ),
        Err(ProtocolError::UnsupportedCapability)
    ));
    let mut f = chat_frames();
    f[1]["choices"][0]["delta"]["reasoning_details"][0]["id"] = json!("changed-reasoning-id");
    assert!(
        decode(
            Protocol::OpenRouterChat,
            &wire(Protocol::OpenRouterChat, &f),
            7
        )
        .is_err()
    );
}
#[test]
fn complete_round_can_continue_to_phased_final_text_without_call_replay() {
    let (round, _) = decode(
        Protocol::Responses,
        &wire(Protocol::Responses, &response_frames()),
        7,
    )
    .unwrap();
    let t = text();
    let ts = tools();
    let result = [ToolResult {
        call_id: "call-1".into(),
        output: "no matches".into(),
        is_error: false,
    }];
    let req = ToolRequest {
        text: &t,
        tools: &ts,
        continuation: Some(&round.continuation),
        results: &result,
    };
    let mut decoder = ToolRoundDecoder::new(
        Protocol::Responses,
        &req,
        &TextCapabilities::default(),
        &ToolCapabilities {
            function_tools: true,
        },
    )
    .unwrap();
    let mut f = response_frames();
    f.retain(|v| {
        matches!(
            v["type"].as_str(),
            Some(
                "response.created"
                    | "response.content_part.added"
                    | "response.output_text.delta"
                    | "response.output_text.done"
                    | "response.content_part.done"
            )
        ) || (v["type"] == "response.output_item.added" && v["item"]["type"] == "message")
            || (v["type"] == "response.output_item.done" && v["item"]["type"] == "message")
    });
    for v in &mut f {
        if v.get("output_index").is_some() {
            v["output_index"] = json!(0);
        }
        if v.get("item").is_some() {
            v["item"]["phase"] = json!("final_answer");
        }
    }
    let final_item = f.last().unwrap()["item"].clone();
    f.push(json!({"type":"response.completed","response":{"id":"resp-1","model":"fixture-model","status":"completed","output":[final_item]}}));
    for bytes in wire(Protocol::Responses, &f).as_bytes().chunks(3) {
        assert!(decoder.push(bytes).error.is_none());
    }
    let final_round = decoder.finish().unwrap();
    assert!(final_round.calls.is_empty());
    assert_eq!(final_round.outcome, FinishOutcome::Completed);
    let checkpoint = final_round.continuation.to_private_checkpoint().unwrap();
    let value: Value = serde_json::from_slice(&checkpoint).unwrap();
    assert_eq!(value["history"][4]["phase"], "final_answer");
}

#[test]
fn refusal_and_token_limits_without_calls_keep_their_actual_outcome() {
    for (native, expected) in [
        ("content_filter", FinishOutcome::Refused),
        ("max_output_tokens", FinishOutcome::Incomplete),
    ] {
        let mut frames = response_frames();
        frames.retain(|v| v["type"] == "response.created");
        frames.push(json!({"type":"response.incomplete","response":{"id":"resp-1","model":"fixture-model","status":"incomplete","output":[],"incomplete_details":{"reason":native}}}));
        let (round, _) =
            decode(Protocol::Responses, &wire(Protocol::Responses, &frames), 1).unwrap();
        assert_eq!(round.outcome, expected);
        assert!(round.calls.is_empty());
    }
    let mut f = anthropic_frames();
    f.retain(|v| v["type"] != "content_block_delta" || v["index"] != 3);
    let t = text();
    let mut definitions = tools();
    definitions[0].parameters =
        json!({"type":"object","properties":{},"required":[],"additionalProperties":false});
    let request = ToolRequest {
        text: &t,
        tools: &definitions,
        continuation: None,
        results: &[],
    };
    let mut decoder = ToolRoundDecoder::new(
        Protocol::AnthropicMessages,
        &request,
        &TextCapabilities::default(),
        &ToolCapabilities {
            function_tools: true,
        },
    )
    .unwrap();
    for bytes in wire(Protocol::AnthropicMessages, &f).as_bytes().chunks(1) {
        assert!(decoder.push(bytes).error.is_none());
    }
    let round = decoder.finish().unwrap();
    assert_eq!(round.calls[0].arguments(), "{}");
}

#[test]
fn sparse_terminal_snapshots_do_not_drop_earlier_assistant_phase() {
    let mut f = response_frames();
    f[8]["item"].as_object_mut().unwrap().remove("phase");
    f[14]["response"]["output"][1]
        .as_object_mut()
        .unwrap()
        .remove("phase");
    let (round, _) = decode(Protocol::Responses, &wire(Protocol::Responses, &f), 1).unwrap();
    let v: Value =
        serde_json::from_slice(&round.continuation.to_private_checkpoint().unwrap()).unwrap();
    assert_eq!(v["history"][1]["phase"], "commentary");
}

fn plan_frames() -> Vec<Value> {
    let mut frames = response_frames();
    for f in &mut frames {
        if f["item"]["type"] == "function_call" {
            f["item"]["namespace"] = json!("moosuri");
        }
        if let Some(output) = f
            .pointer_mut("/response/output")
            .and_then(Value::as_array_mut)
        {
            for item in output {
                if item["type"] == "function_call" {
                    item["namespace"] = json!("moosuri");
                }
            }
        }
    }
    frames
}
fn decode_plan(frames: &[Value], chunk: usize) -> Result<ToolRound, ProtocolError> {
    let t = text();
    let ts = tools();
    let r = ToolRequest {
        text: &t,
        tools: &ts,
        continuation: None,
        results: &[],
    };
    let mut d = ToolRoundDecoder::chatgpt_plan(
        &r,
        &TextCapabilities::default(),
        &ToolCapabilities {
            function_tools: true,
        },
    )?;
    for bytes in wire(Protocol::Responses, frames).as_bytes().chunks(chunk) {
        let b = d.push(bytes);
        if let Some(e) = b.error {
            return Err(e);
        }
    }
    d.finish()
}
#[test]
fn plan_fragmented_namespace_calls_checkpoint_and_result_roundtrip_keep_exact_identity() {
    for size in [1, 7, 65536] {
        let round = decode_plan(&plan_frames(), size).unwrap();
        assert_eq!(round.calls[0].namespace(), Some("moosuri"));
        assert_eq!(round.calls[0].name(), "knowledge_search");
        assert_eq!(round.calls[0].call_id(), "call-1");
        assert_eq!(round.calls[0].arguments(), "{\"q\":\"홍차\"}");
        let bytes = round.continuation.to_private_checkpoint().unwrap();
        assert!(
            Continuation::from_private_checkpoint(
                &bytes,
                Protocol::Responses,
                "fixture-model",
                None
            )
            .is_err()
        );
        let restored =
            Continuation::from_chatgpt_plan_checkpoint(&bytes, "fixture-model", None).unwrap();
        assert_eq!(restored.to_private_checkpoint().unwrap(), bytes);
        let t = text();
        let ts = tools();
        let results = [ToolResult {
            call_id: "call-1".into(),
            output: "fresh synthetic challenge".into(),
            is_error: false,
        }];
        let r = ToolRequest {
            text: &t,
            tools: &ts,
            continuation: Some(&restored),
            results: &results,
        };
        let body = encode_chatgpt_plan_tool_request(
            &r,
            &TextCapabilities::default(),
            &ToolCapabilities {
                function_tools: true,
            },
        )
        .unwrap();
        assert_eq!(body["tools"][0]["type"], "namespace");
        assert_eq!(body["tools"][0]["name"], "moosuri");
        assert_eq!(body["tools"][0]["tools"][0]["name"], "knowledge_search");
        assert_eq!(body["input"][3]["namespace"], "moosuri");
        assert_eq!(body["input"][3]["call_id"], "call-1");
        assert_eq!(
            body["input"][4],
            json!({"type":"function_call_output","call_id":"call-1","output":"fresh synthetic challenge"})
        );
        assert!(body.get("max_output_tokens").is_none());
        assert!(body.get("previous_response_id").is_none());
        assert!(
            ToolRoundDecoder::chatgpt_plan(
                &r,
                &TextCapabilities::default(),
                &ToolCapabilities {
                    function_tools: true
                }
            )
            .is_ok()
        );
        assert!(
            encode_tool_request(
                Protocol::Responses,
                &r,
                &TextCapabilities::default(),
                &ToolCapabilities {
                    function_tools: true
                }
            )
            .is_err()
        );
        let mut corrupt: Value = serde_json::from_slice(&bytes).unwrap();
        corrupt["history"][2]["namespace"] = json!("foreign");
        assert!(
            Continuation::from_chatgpt_plan_checkpoint(
                &serde_json::to_vec(&corrupt).unwrap(),
                "fixture-model",
                None
            )
            .is_err()
        );
        let wrong = [ToolResult {
            call_id: "wrong-id".into(),
            output: "challenge".into(),
            is_error: false,
        }];
        assert!(
            encode_chatgpt_plan_tool_request(
                &ToolRequest {
                    results: &wrong,
                    ..r
                },
                &TextCapabilities::default(),
                &ToolCapabilities {
                    function_tools: true
                }
            )
            .is_err()
        );
    }
}
#[test]
fn plan_missing_foreign_or_changed_namespace_name_and_call_id_are_not_executable() {
    for (index, key, value) in [
        (9, "namespace", Value::Null),
        (9, "namespace", json!("foreign")),
        (9, "name", json!("unoffered")),
        (13, "namespace", json!("foreign")),
        (13, "name", json!("other")),
        (13, "call_id", json!("changed")),
        (13, "id", json!("changed-item")),
    ] {
        let mut frames = plan_frames();
        frames[index]["item"][key] = value;
        assert!(decode_plan(&frames, 1).is_err(), "{index} {key}");
    }
    let mut frames = plan_frames();
    frames[14]["response"]["output"][2]["namespace"] = json!("foreign");
    assert!(decode_plan(&frames, 1).is_err());
    // Ordinary API flat tools still succeed and never claim a namespace.
    let (api, _) = decode(
        Protocol::Responses,
        &wire(Protocol::Responses, &response_frames()),
        1,
    )
    .unwrap();
    assert_eq!(api.calls[0].namespace(), None);
    let bytes = api.continuation.to_private_checkpoint().unwrap();
    assert!(Continuation::from_chatgpt_plan_checkpoint(&bytes, "fixture-model", None).is_err());
}

#[test]
fn plan_preserves_safe_direct_call_metadata_and_rejects_async_or_programmatic_execution() {
    for (caller, asynchronous, allowed) in [
        (json!({"type":"direct"}), json!(false), true),
        (Value::Null, Value::Null, true),
        (
            json!({"type":"program","caller_id":"other"}),
            json!(false),
            false,
        ),
        (json!({"type":"direct"}), json!(true), false),
    ] {
        let mut frames = plan_frames();
        for f in &mut frames {
            if f["item"]["type"] == "function_call" {
                f["item"]["caller"] = caller.clone();
                f["item"]["async"] = asynchronous.clone();
            }
            if let Some(output) = f
                .pointer_mut("/response/output")
                .and_then(Value::as_array_mut)
            {
                for item in output {
                    if item["type"] == "function_call" {
                        item["caller"] = caller.clone();
                        item["async"] = asynchronous.clone();
                    }
                }
            }
        }
        let result = decode_plan(&frames, 1);
        if allowed {
            let bytes = result
                .unwrap()
                .continuation
                .to_private_checkpoint()
                .unwrap();
            let v: Value = serde_json::from_slice(&bytes).unwrap();
            assert_eq!(v["history"][2]["caller"], caller);
            assert_eq!(v["history"][2]["async"], asynchronous);
            assert!(
                Continuation::from_chatgpt_plan_checkpoint(&bytes, "fixture-model", None).is_ok()
            );
        } else {
            assert!(matches!(result, Err(ProtocolError::UnsupportedCapability)));
        }
    }
}

#[test]
fn plan_tool_failure_usage_is_never_unattributed_and_keeps_prior_visible_text() {
    let t = text();
    let ts = tools();
    let r = ToolRequest {
        text: &t,
        tools: &ts,
        continuation: None,
        results: &[],
    };
    let valid = json!({"id":"resp-1","model":"fixture-model","error":{"code":"subscription_sharing_usage_limit_exceeded"},"usage":{"input_tokens":99,"output_tokens":2}});
    let mut responses = vec![valid.clone()];
    for key in ["id", "model"] {
        let mut missing = valid.clone();
        missing.as_object_mut().unwrap().remove(key);
        responses.push(missing);
        let mut wrong = valid.clone();
        wrong[key] = json!("foreign");
        responses.push(wrong);
    }
    for started in [false, true] {
        for response in &responses {
            let mut d = ToolRoundDecoder::chatgpt_plan(
                &r,
                &TextCapabilities::default(),
                &ToolCapabilities {
                    function_tools: true,
                },
            )
            .unwrap();
            let mut events = vec![];
            if started {
                // The complete valid prefix includes visible text, but no tool call.
                for frame_value in plan_frames().into_iter().take(9) {
                    let batch = d.push(frame(frame_value).as_bytes());
                    assert!(batch.error.is_none());
                    events.extend(batch.events);
                }
            }
            let batch =
                d.push(frame(json!({"type":"response.failed","response":response})).as_bytes());
            let attributed = started && response == &valid;
            assert_eq!(
                batch.error,
                Some(if attributed {
                    ProtocolError::ChatGptPlan(PlanError::UsageLimitExceeded)
                } else {
                    ProtocolError::Protocol
                })
            );
            events.extend(batch.events);
            assert_eq!(
                events
                    .iter()
                    .filter(|event| matches!(event, ModelEvent::UsageSnapshot(_)))
                    .count(),
                usize::from(attributed)
            );
            assert_eq!(
                events
                    .iter()
                    .any(|event| matches!(event, ModelEvent::TextDelta { .. })),
                started
            );
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, ModelEvent::Finished { .. }))
            );
            assert!(d.finish().is_err());
        }
    }
}

#[test]
fn plan_tool_foreign_progress_cannot_be_followed_by_a_valid_executable_round() {
    for kind in ["response.in_progress", "response.queued"] {
        for response in [
            json!({"id":"foreign","model":"fixture-model"}),
            json!({"id":"resp-1","model":"foreign"}),
            json!({"model":"fixture-model"}),
            json!({"id":"resp-1"}),
        ] {
            let mut frames = plan_frames();
            frames.insert(1, json!({"type":kind,"response":response}));
            assert!(matches!(
                decode_plan(&frames, 1),
                Err(ProtocolError::Protocol)
            ));
        }
        let mut frames = plan_frames();
        frames.insert(
            1,
            json!({"type":kind,"response":{"id":"resp-1","model":"fixture-model"}}),
        );
        assert_eq!(decode_plan(&frames, 1).unwrap().calls.len(), 1);
    }
}
