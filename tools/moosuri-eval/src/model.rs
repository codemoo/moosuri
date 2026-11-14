//! Bounded protocol fixture. Receives actual agent requests; never receives gold.
use crate::{EvalResult, digest, world};
use axum::{
    Router,
    body::Body,
    extract::{DefaultBodyLimit, State},
    response::{IntoResponse, Response},
    routing::post,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::{net::TcpListener, task::JoinHandle};

#[derive(Clone, Copy)]
pub enum Script {
    Memory,
    Chat,
    Truncated,
    Calculation,
    SkilledCalculation,
    SkillOnly,
    Mission,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Dispatch {
    pub request_sha256: String,
    pub calls: Vec<IssuedCall>,
    pub terminal: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct IssuedCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}
#[derive(Clone)]
struct Model {
    script: Script,
    ledger: Arc<Mutex<Vec<Dispatch>>>,
}
pub struct Provider {
    pub base: String,
    ledger: Arc<Mutex<Vec<Dispatch>>>,
    task: JoinHandle<()>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Provider {
    pub async fn start(script: Script) -> EvalResult<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "fixture_bind")?;
        let base = format!(
            "http://{}/v1",
            listener.local_addr().map_err(|_| "fixture_address")?
        );
        let ledger = Arc::new(Mutex::new(Vec::new()));
        let state = Model {
            script,
            ledger: ledger.clone(),
        };
        let app = Router::new()
            .route("/v1/chat/completions", post(round))
            .layer(DefaultBodyLimit::max(256 * 1024))
            .with_state(state);
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { base, ledger, task })
    }
    pub fn ledger(&self) -> Vec<Dispatch> {
        self.ledger
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
fn call(name: &str, arguments: Value) -> IssuedCall {
    IssuedCall {
        id: uuid::Uuid::new_v4().to_string(),
        name: name.into(),
        arguments,
    }
}
fn tool_results(input: &Value) -> EvalResult<Vec<Value>> {
    input["messages"]
        .as_array()
        .ok_or("fixture_messages")?
        .iter()
        .filter(|m| m["role"] == "tool")
        .map(|m| {
            serde_json::from_str(m["content"].as_str().ok_or("fixture_tool_content")?)
                .map_err(|_| "fixture_tool_json")
        })
        .collect()
}
fn skill(input: &Value) -> EvalResult<IssuedCall> {
    let system = input["messages"][0]["content"]
        .as_str()
        .ok_or("fixture_system")?;
    let line = system
        .lines()
        .find(|s| s.starts_with("evidence-comparison revision="))
        .ok_or("fixture_skill_metadata")?;
    let fields = line.split_whitespace().collect::<Vec<_>>();
    let revision = fields
        .get(1)
        .and_then(|s| s.strip_prefix("revision="))
        .ok_or("fixture_revision")?;
    let hash = fields
        .get(2)
        .and_then(|s| s.strip_prefix("digest="))
        .ok_or("fixture_digest")?
        .trim_end_matches(':');
    Ok(call(
        "skill_read",
        json!({"id":"evidence-comparison","revision":revision,"digest":hash}),
    ))
}
fn next(model: &Model, input: &Value) -> EvalResult<(Vec<IssuedCall>, Option<String>)> {
    let results = tool_results(input)?;
    let names = input["tools"]
        .as_array()
        .map(|ts| {
            ts.iter()
                .filter_map(|t| t["function"]["name"].as_str())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // These classifiers route no changes in these read-only worlds. They remain
    // actual production calls, counted in the independent ledger and budgets.
    if names.contains(&"memory_command_decision") {
        return Ok((
            vec![call(
                "memory_command_decision",
                json!({"decision":"no_command","clarification":null,"action":null,"intent_quote":null,"fact_quote":null,"subject":null,"target_quote":null}),
            )],
            None,
        ));
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
    match model.script {
        Script::Mission => {
            if input
                .get("tools")
                .and_then(Value::as_array)
                .is_none_or(|t| t.is_empty())
            {
                return Ok((vec![],Some(json!({"steps":["Read the saved quote","Calculate its total","Verify the saved evidence"]}).to_string())));
            }
            let context: Value = serde_json::from_str(
                input["messages"][1]["content"]
                    .as_str()
                    .ok_or("fixture_mission_context")?,
            )
            .map_err(|_| "fixture_mission_context")?;
            let receipts = context["receipts"]
                .as_array()
                .ok_or("fixture_mission_receipts")?;
            let mut proxy = input.clone();
            let mut messages = input["messages"]
                .as_array()
                .cloned()
                .ok_or("fixture_mission_messages")?;
            for receipt in receipts {
                let result = receipt["result"].as_str().ok_or("fixture_mission_result")?;
                messages.push(json!({"role":"tool","content":result}));
            }
            proxy["messages"] = json!(messages);
            let calculation = Model {
                script: Script::Calculation,
                ledger: model.ledger.clone(),
            };
            let (calls, answer) = next(&calculation, &proxy)?;
            let final_answer = answer.map(|mut text| {
                for receipt in receipts {
                    if let Some(raw) = receipt["result"].as_str()
                        && let Ok(result) = serde_json::from_str::<Value>(raw)
                        && result["kind"] == "source"
                        && let Some(source) = result["value"]["text"].as_str()
                    {
                        text.push(' ');
                        text.push_str(source);
                    }
                }
                let evidence = receipts
                    .iter()
                    .map(|r| json!({"step":r["step"],"call_id":r["call_id"]}))
                    .collect::<Vec<_>>();
                json!({"text":text,"evidence":evidence}).to_string()
            });
            Ok((calls, final_answer))
        }
        Script::Chat | Script::Truncated => Ok((vec![], Some("Synthetic response body.".into()))),
        Script::Memory => {
            if let Some(memory) = results.iter().find(|r| r["kind"] == "memory") {
                return Ok((
                    vec![],
                    Some(
                        memory["value"]["draft"]["text"]
                            .as_str()
                            .ok_or("fixture_memory")?
                            .into(),
                    ),
                ));
            }
            if let Some(search) = results.iter().find(|r| r["kind"] == "search") {
                let hits = search["value"]["hits"].as_array().ok_or("fixture_hits")?;
                return if let Some(hit) = hits.first() {
                    Ok((
                        vec![call(
                            "knowledge_read_memory",
                            json!({"id":hit["id"],"revision":hit["revision"]}),
                        )],
                        None,
                    ))
                } else {
                    Ok((vec![], Some("No saved preference is available.".into())))
                };
            }
            Ok((
                vec![call(
                    "knowledge_search",
                    json!({"kind":"memory","text":"preference","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null}),
                )],
                None,
            ))
        }
        Script::Calculation | Script::SkilledCalculation | Script::SkillOnly => {
            if matches!(model.script, Script::SkilledCalculation | Script::SkillOnly)
                && !results.iter().any(|r| r["kind"] == "skill")
            {
                return Ok((vec![skill(input)?], None));
            }
            if matches!(model.script, Script::SkillOnly) {
                // Deliberately false claim used only to validate the grader.
                return Ok((vec![], Some("Total USD = 24.75. Completed.".into())));
            }
            if let Some(result) = results.iter().find(|r| r["kind"] == "decimal") {
                let value = result["value"]["exact"].as_str().ok_or("fixture_decimal")?;
                return Ok((
                    vec![],
                    Some(format!("Total USD = {value}. No purchase was made.")),
                ));
            }
            if let Some(source) = results.iter().find(|r| r["kind"] == "source") {
                let text = source["value"]["text"].as_str().ok_or("fixture_source")?;
                let fields = text.split_whitespace().collect::<Vec<_>>();
                let amount = fields
                    .windows(2)
                    .find(|w| w[0] == "amount")
                    .ok_or("fixture_amount")?[1];
                let shipping = fields
                    .windows(2)
                    .find(|w| w[0] == "shipping")
                    .ok_or("fixture_shipping")?[1];
                return Ok((
                    vec![call(
                        "decimal_calculate",
                        json!({"operation":"sum","operands":[amount,shipping]}),
                    )],
                    None,
                ));
            }
            if let Some(search) = results.iter().find(|r| r["kind"] == "search") {
                let hit = search["value"]["hits"]
                    .as_array()
                    .and_then(|h| h.first())
                    .ok_or("fixture_source_missing")?;
                return Ok((
                    vec![call(
                        "knowledge_read_source",
                        json!({"id":hit["id"],"revision":hit["revision"],"start_byte":0,"max_bytes":4096}),
                    )],
                    None,
                ));
            }
            Ok((
                vec![call(
                    "knowledge_search",
                    json!({"kind":"source","text":"Synthetic quote","collection_id":null,"subject":null,"speaker":null,"valid_at":null,"cursor":null}),
                )],
                None,
            ))
        }
    }
}
async fn round(State(model): State<Model>, axum::Json(input): axum::Json<Value>) -> Response {
    if input["model"] != world::MODEL
        || model.ledger.lock().unwrap_or_else(|e| e.into_inner()).len() >= 32
    {
        return axum::http::StatusCode::TOO_MANY_REQUESTS.into_response();
    }
    let Ok((calls, answer)) = next(&model, &input) else {
        return axum::http::StatusCode::BAD_REQUEST.into_response();
    };
    let terminal = !matches!(model.script, Script::Truncated);
    model
        .ledger
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .push(Dispatch {
            request_sha256: digest(&serde_json::to_vec(&input).unwrap()),
            calls: calls.clone(),
            terminal,
        });
    let id = uuid::Uuid::new_v4().to_string();
    let tool_calls = calls.iter().enumerate().map(|(index, c)| json!({"index":index,"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.arguments.to_string()}})).collect::<Vec<_>>();
    let delta = if answer.is_some() {
        json!({"role":"assistant","content":answer})
    } else {
        json!({"role":"assistant","tool_calls":tool_calls})
    };
    let head = json!({"id":id,"model":world::MODEL,"choices":[{"index":0,"delta":delta,"finish_reason":null}]});
    let mut wire = format!("data: {head}\n\n");
    if terminal {
        let end = json!({"id":id,"model":world::MODEL,"choices":[{"index":0,"delta":{},"finish_reason":if calls.is_empty() {"stop"} else {"tool_calls"}}],"usage":{"prompt_tokens":13,"completion_tokens":5}});
        wire.push_str(&format!("data: {end}\n\ndata: [DONE]\n\n"));
    }
    Response::builder()
        .header("content-type", "text/event-stream")
        .body(Body::from(wire))
        .unwrap()
}
