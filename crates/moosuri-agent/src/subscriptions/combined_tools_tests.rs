//! Production definition composition plus real plan/shared HTTP continuation.
//! This tests preflight/transport, not Store admission or external adapter actions.
use super::*;
use moosuri_connectors::model::protocol::{
    FinishOutcome, ModelEvent, TextMessage, TextRequest, TextRole,
    tools::{FunctionDefinition, ToolCapabilities, ToolRequest, ToolResult, ToolRound},
};
use moosuri_core::{
    ArtifactRunProfile, CalendarAgentPin, CalendarRunProfile, MailConnectionPin, RunToolProfile,
    WebToolProfile,
};
use std::collections::BTreeSet;

const NAMES: [&str; 21] = [
    "knowledge_search",
    "knowledge_read_source",
    "knowledge_read_memory",
    "skill_read",
    "decimal_calculate",
    "mail_search",
    "mail_read",
    "mail_thread",
    "mail_prepare_draft",
    "web_read",
    "web_search",
    "browser_read",
    "browser_open",
    "browser_observe",
    "browser_handoff",
    "artifact_search",
    "artifact_read",
    "followup_briefing",
    "calendar_list_events",
    "calendar_read_event",
    "calendar_action_status",
];
const FINAL: &str = "Exact synthetic result: 0.3. No external action was performed.";

fn combined_definitions() -> Vec<FunctionDefinition> {
    let profile = RunToolProfile {
        version: 5,
        skills: vec![],
        community_metadata: vec![],
        mail_connections: vec![MailConnectionPin {
            id: "synthetic-mail".into(),
            epoch: 1,
            email: "owner@example.test".into(),
        }],
        web: Some(WebToolProfile {
            read: true,
            search: true,
            browser_pin: Some("synthetic-browser".into()),
        }),
    };
    let mut definitions = crate::knowledge_tools::definitions(Some(&profile));
    assert_eq!(definitions.len(), 15);
    definitions.extend(crate::artifact_tools::definitions(Some(
        &ArtifactRunProfile {
            schema_version: 1,
            selected: None,
            planned: true,
            write: false,
            kind: None,
        },
    )));
    assert_eq!(definitions.len(), 18);
    definitions.extend(crate::calendar_tools::definitions(Some(
        &CalendarRunProfile {
            version: 1,
            reference: 1700000000,
            timezone: Some("UTC".into()),
            connections: vec![CalendarAgentPin {
                id: "synthetic-calendar".into(),
                title: "Synthetic Calendar".into(),
                epoch: 1,
                client_id: "synthetic-client".into(),
                app_revision: None,
            }],
        },
    )));
    assert_eq!(definitions.len(), 21);
    definitions
}

fn assert_definitions(body: &Value, plan: bool) {
    let definitions = if plan {
        assert_eq!(body["tools"].as_array().unwrap().len(), 1);
        assert_eq!(body["tools"][0]["type"], "namespace");
        assert_eq!(body["tools"][0]["name"], "moosuri");
        body["tools"][0]["tools"].as_array().unwrap()
    } else {
        body["tools"].as_array().unwrap()
    };
    let functions = definitions
        .iter()
        .map(|d| {
            assert_eq!(d["type"], "function");
            if plan { d } else { &d["function"] }
        })
        .collect::<Vec<_>>();
    let names = functions
        .iter()
        .map(|d| d["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(functions.len(), 21); // Provider sees every definition; none is silently dropped.
    assert_eq!(names, NAMES.into_iter().collect());
    for definition in functions {
        assert_eq!(definition["strict"], true);
        assert_eq!(definition["parameters"]["additionalProperties"], false);
        if definition["name"] == "mail_read" {
            assert_eq!(
                definition["parameters"]["properties"]["connection_id"]["enum"],
                json!(["synthetic-mail"])
            );
        }
        if definition["name"] == "calendar_list_events" {
            assert_eq!(
                definition["parameters"]["properties"]["connection_id"]["enum"],
                json!(["synthetic-calendar"])
            );
        }
    }
    assert_eq!(body["parallel_tool_calls"], false);
}

fn frame(value: Value) -> String {
    format!("data: {value}\n\n")
}

fn plan_call() -> String {
    let arguments = r#"{"operation":"sum","operands":["0.1","0.2"]}"#;
    let call = json!({"type":"function_call","id":"fc-combined","call_id":"combined-decimal","namespace":"moosuri","name":"decimal_calculate","arguments":arguments,"status":"completed"});
    [
        json!({"type":"response.created","response":{"id":"r-call","model":"fixture-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"function_call","id":"fc-combined","call_id":"combined-decimal","namespace":"moosuri","name":"decimal_calculate","arguments":"","status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,"item_id":"fc-combined","delta":arguments}),
        json!({"type":"response.function_call_arguments.done","output_index":0,"item_id":"fc-combined","arguments":arguments}),
        json!({"type":"response.output_item.done","output_index":0,"item":call}),
        json!({"type":"response.completed","response":{"id":"r-call","model":"fixture-model","status":"completed","output":[call],"usage":{"input_tokens":11,"output_tokens":3}}}),
    ].into_iter().map(frame).collect()
}

fn plan_final() -> String {
    let item = json!({"type":"message","id":"m-combined","role":"assistant","status":"completed","content":[{"type":"output_text","text":FINAL,"annotations":[]}]});
    [
        json!({"type":"response.created","response":{"id":"r-final","model":"fixture-model"}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"type":"message","id":"m-combined","role":"assistant","content":[]}}),
        json!({"type":"response.content_part.added","output_index":0,"item_id":"m-combined","content_index":0,"part":{"type":"output_text","text":""}}),
        json!({"type":"response.output_text.delta","output_index":0,"item_id":"m-combined","content_index":0,"delta":FINAL}),
        json!({"type":"response.output_text.done","output_index":0,"item_id":"m-combined","content_index":0,"text":FINAL}),
        json!({"type":"response.content_part.done","output_index":0,"item_id":"m-combined","content_index":0,"part":{"type":"output_text","text":FINAL,"annotations":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"r-final","model":"fixture-model","status":"completed","output":[item],"usage":{"input_tokens":15,"output_tokens":8}}}),
    ].into_iter().map(frame).collect()
}

async fn fixture_response(body: Value, plan: bool, rounds: Arc<AtomicUsize>) -> impl IntoResponse {
    let round = rounds.fetch_add(1, Ordering::SeqCst);
    assert!(round < 2);
    assert_definitions(&body, plan);
    let messages = body[if plan { "input" } else { "messages" }]
        .as_array()
        .unwrap();
    if round == 1 {
        if plan {
            let call = messages
                .iter()
                .find(|m| m["type"] == "function_call")
                .unwrap();
            assert_eq!(call["namespace"], "moosuri");
            assert_eq!(call["call_id"], "combined-decimal");
        }
        let result = messages
            .iter()
            .find(|m| {
                if plan {
                    m["type"] == "function_call_output"
                } else {
                    m["role"] == "tool"
                }
            })
            .unwrap();
        assert_eq!(
            result[if plan { "call_id" } else { "tool_call_id" }],
            "combined-decimal"
        );
        let value: Value = serde_json::from_str(
            result[if plan { "output" } else { "content" }]
                .as_str()
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value, json!({"kind":"decimal","value":{"exact":"0.3"}}));
    }
    let wire = if plan {
        assert!(body.get("max_output_tokens").is_none());
        assert!(messages.iter().all(|m| m["role"] != "system"));
        if round == 0 {
            plan_call()
        } else {
            plan_final()
        }
    } else {
        let delta = if round == 0 {
            json!({"role":"assistant","tool_calls":[{"index":0,"id":"combined-decimal","type":"function","function":{"name":"decimal_calculate","arguments":r#"{"operation":"sum","operands":["0.1","0.2"]}"#}}]})
        } else {
            json!({"role":"assistant","content":FINAL})
        };
        frame(
            json!({"id":"combined-shared","model":"fixture-model","choices":[{"index":0,"delta":delta,"finish_reason":if round==0 {"tool_calls"}else{"stop"}}],"usage":{"prompt_tokens":11,"completion_tokens":3}}),
        ) + "data: [DONE]\n\n"
    };
    (
        [(axum::http::header::CONTENT_TYPE, "text/event-stream")],
        wire,
    )
}

async fn stream(client: &ModelClient, request: &ToolRequest<'_>) -> (ToolRound, Vec<ModelEvent>) {
    let (tx, mut rx) = tokio::sync::mpsc::channel(32);
    let caps = ToolCapabilities {
        function_tools: true,
    };
    let cancel = CancellationToken::new();
    let (round, events) = tokio::join!(
        async {
            let round = client.stream_tool_round(request, &caps, &tx, &cancel).await;
            drop(tx);
            round.unwrap()
        },
        async {
            let mut events = vec![];
            while let Some(event) = rx.recv().await {
                events.push(event);
            }
            events
        }
    );
    (round, events)
}

async fn combined_roundtrip(plan: bool) {
    let rounds = Arc::new(AtomicUsize::new(0));
    let captured = rounds.clone();
    let path = if plan {
        "/v1/responses"
    } else {
        "/v1/chat/completions"
    };
    let app = Router::new().route(
        path,
        post(move |Json(body): Json<Value>| fixture_response(body, plan, captured.clone())),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let client = if plan {
        ModelClient::synthetic_chatgpt_plan(
            &origin,
            Secret::new("synthetic-token".into()).unwrap(),
            TextCapabilities::default(),
        )
        .unwrap()
    } else {
        ModelClient::compatible(
            &format!("{origin}/v1"),
            NetworkPolicy::LocalExact(vec![IpAddr::from([127, 0, 0, 1])]),
            None,
            TextCapabilities::default(),
        )
        .unwrap()
    };
    let definitions = combined_definitions();
    let text = TextRequest {
        model: "fixture-model".into(),
        effort: None,
        max_output_tokens: 2048,
        messages: vec![TextMessage {
            role: TextRole::User,
            text: "Compute 0.1 plus 0.2; do not perform external actions.".into(),
            phase: None,
        }],
    };
    let (first, _) = stream(
        &client,
        &ToolRequest {
            text: &text,
            tools: &definitions,
            continuation: None,
            results: &[],
        },
    )
    .await;
    assert_eq!(first.outcome, FinishOutcome::ContinuationRequired);
    assert_eq!(first.calls.len(), 1);
    assert_eq!(first.calls[0].name(), "decimal_calculate");
    assert_eq!(
        first.calls[0].namespace(),
        if plan { Some("moosuri") } else { None }
    );
    assert_eq!(
        serde_json::from_str::<Value>(first.calls[0].arguments()).unwrap(),
        json!({"operation":"sum","operands":["0.1","0.2"]})
    );
    // Explicit synthetic tool result: this regression does not claim Store execution.
    let results = [ToolResult {
        call_id: first.calls[0].call_id().into(),
        output: json!({"kind":"decimal","value":{"exact":"0.3"}}).to_string(),
        is_error: false,
    }];
    let (second, events) = stream(
        &client,
        &ToolRequest {
            text: &text,
            tools: &definitions,
            continuation: Some(&first.continuation),
            results: &results,
        },
    )
    .await;
    assert_eq!(second.outcome, FinishOutcome::Completed);
    assert!(second.calls.is_empty());
    let final_text = events
        .iter()
        .filter_map(|e| match e {
            ModelEvent::TextDelta { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect::<String>();
    assert_eq!(final_text, FINAL);
    assert_eq!(rounds.load(Ordering::SeqCst), 2);
    task.abort();
}

#[tokio::test]
async fn combined_read_profiles_reach_plan_model_and_complete_continuation() {
    combined_roundtrip(true).await;
}

#[tokio::test]
async fn combined_read_profiles_reach_shared_model_and_complete_continuation() {
    combined_roundtrip(false).await;
}
