//! Agent-visible scripted protocol only. Never imports the oracle or observed DB.
use crate::{EvalResult, digest};
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    response::{IntoResponse, Response},
    routing::post,
};
use serde_json::{Value, json};
use std::sync::{Arc, Mutex};
pub(super) struct Provider {
    pub port: u16,
    task: tokio::task::JoinHandle<()>,
    ledger: Arc<Mutex<Vec<String>>>,
}
impl Drop for Provider {
    fn drop(&mut self) {
        self.task.abort();
    }
}
impl Provider {
    pub async fn start() -> EvalResult<Self> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|_| "paired_fixture_bind")?;
        let port = listener
            .local_addr()
            .map_err(|_| "paired_fixture_port")?
            .port();
        let ledger = Arc::new(Mutex::new(vec![]));
        let app = Router::new()
            .route("/v1/responses", post(responses))
            .layer(DefaultBodyLimit::max(256 * 1024))
            .with_state(ledger.clone());
        let task = tokio::spawn(async move {
            let _ = axum::serve(listener, app).await;
        });
        Ok(Self { port, task, ledger })
    }
    pub fn calls(&self) -> usize {
        self.ledger.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}
pub(crate) fn calculate(text: &str) -> EvalResult<String> {
    let amounts = text
        .split_whitespace()
        .filter_map(|s| {
            let (whole, fraction) = s.split_once('.')?;
            if fraction.len() != 2
                || !whole.bytes().all(|b| b.is_ascii_digit())
                || !fraction.bytes().all(|b| b.is_ascii_digit())
            {
                return None;
            }
            let value = whole
                .parse::<u64>()
                .ok()?
                .checked_mul(100)?
                .checked_add(fraction.parse::<u64>().ok()?)?;
            Some(value)
        })
        .collect::<Vec<_>>();
    if amounts.len() != 2 {
        return Err("paired_fixture_amounts");
    }
    let cents = amounts[0]
        .checked_add(amounts[1])
        .ok_or("paired_fixture_overflow")?;
    Ok(format!("{}.{:02} USD", cents / 100, cents % 100))
}
fn strings(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::String(s) => out.push(s.clone()),
        Value::Array(v) => {
            for x in v {
                strings(x, out)
            }
        }
        Value::Object(m) => {
            for (k, v) in m {
                if matches!(k.as_str(), "text" | "content" | "input" | "instructions") {
                    strings(v, out)
                }
            }
        }
        _ => (),
    }
}
fn answer(input: &Value) -> EvalResult<String> {
    if input["model"] != "gpt-6.1-terra" || input["stream"] != true || input["store"] != false {
        return Err("paired_fixture_profile");
    }
    let items = input["input"].as_array().ok_or("paired_fixture_input")?;
    let user = items
        .iter()
        .rev()
        .find(|m| m["role"] == "user")
        .ok_or("paired_fixture_user")?;
    let mut current = vec![];
    strings(&user["content"], &mut current);
    let current = current.join(" ");
    if current.starts_with("Add ") {
        return calculate(&current);
    }
    let mut context = vec![];
    strings(input, &mut context);
    // Extract the live row supplied in the actual legacy system context, rather
    // than supplying the evaluator's expected fact to the subject.
    context
        .iter()
        .flat_map(|s| s.lines())
        .find_map(|line| {
            if !line.starts_with("- id=") || !line.contains(" revision=") {
                return None;
            }
            let data = line.split_once(": ")?.1;
            data.split_once(" (출처:").map(|(text, _)| text.to_owned())
        })
        .ok_or("paired_fixture_current_memory")
}
async fn responses(
    State(ledger): State<Arc<Mutex<Vec<String>>>>,
    Json(input): Json<Value>,
) -> Response {
    {
        let mut calls = ledger.lock().unwrap_or_else(|e| e.into_inner());
        if calls.len() >= 12 {
            return (axum::http::StatusCode::TOO_MANY_REQUESTS, "fixture_budget").into_response();
        }
        calls.push(digest(input.to_string().as_bytes()));
    }
    let answer = match answer(&input) {
        Ok(answer) => answer,
        Err(e) => return (axum::http::StatusCode::UNPROCESSABLE_ENTITY, e).into_response(),
    };
    let item = json!({"id":"msg_synthetic","type":"message","role":"assistant","status":"completed","content":[{"type":"output_text","text":answer,"annotations":[]}]});
    let response = json!({"id":"resp_synthetic","object":"response","created_at":1800000000,"status":"completed","model":"gpt-6.1-terra","output":[item],"parallel_tool_calls":false,"tool_choice":"auto","tools":[],"usage":{"input_tokens":1,"output_tokens":1,"total_tokens":2,"input_tokens_details":{"cached_tokens":0},"output_tokens_details":{"reasoning_tokens":0}}});
    let events = vec![
        json!({"type":"response.created","response":{"id":"resp_synthetic","object":"response","created_at":1800000000,"status":"in_progress","model":"gpt-6.1-terra","output":[]}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{"id":"msg_synthetic","type":"message","role":"assistant","status":"in_progress","content":[]}}),
        json!({"type":"response.content_part.added","item_id":"msg_synthetic","output_index":0,"content_index":0,"part":{"type":"output_text","text":"","annotations":[]}}),
        json!({"type":"response.output_text.delta","item_id":"msg_synthetic","output_index":0,"content_index":0,"delta":answer}),
        json!({"type":"response.output_text.done","item_id":"msg_synthetic","output_index":0,"content_index":0,"text":answer}),
        json!({"type":"response.content_part.done","item_id":"msg_synthetic","output_index":0,"content_index":0,"part":{"type":"output_text","text":answer,"annotations":[]}}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":response}),
    ];
    let mut body = String::new();
    for (n, mut event) in events.into_iter().enumerate() {
        event["sequence_number"] = json!(n);
        body.push_str(&format!(
            "event: {}\ndata: {}\n\n",
            event["type"].as_str().unwrap(),
            event
        ));
    }
    ([("content-type", "text/event-stream")], body).into_response()
}
#[cfg(test)]
mod tests {
    #[test]
    fn paired_fixture_uses_actual_input() {
        assert_eq!(
            super::calculate("Add 31.20 USD and 1.02 USD").unwrap(),
            "32.22 USD"
        );
        assert!(super::calculate("Answer 24.75 USD").is_err());
    }
}
