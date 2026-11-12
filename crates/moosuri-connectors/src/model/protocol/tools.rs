//! Bounded client-function rounds. Private continuation data is never UI/log data.
use super::*;
use std::collections::BTreeSet;

const MAX_CALLS: usize = 12;
const MAX_ARGUMENTS: usize = 16 * 1024;
const MAX_CAPSULE: usize = 256 * 1024;
const MAX_DEFINITIONS: usize = 16;

/// Explicit opt-in for this connection/model. Text verification does not grant it.
#[derive(Default)]
pub struct ToolCapabilities {
    pub function_tools: bool,
}
/// Closed object parameters with all properties required; optional values use
/// nullable type unions. Supports objects, arrays, scalar types/enums and bounded
/// descriptions to depth8. Other schema keywords fail explicitly.
pub struct FunctionDefinition {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}
pub struct ToolResult {
    pub call_id: String,
    pub output: String,
    pub is_error: bool,
}
pub struct ToolRequest<'a> {
    pub text: &'a TextRequest,
    pub tools: &'a [FunctionDefinition],
    pub continuation: Option<&'a Continuation>,
    pub results: &'a [ToolResult],
}
/// Only constructed after a complete validated terminal round. No streamed call
/// is executable; arguments remain untrusted and require registry validation.
pub struct CompletedToolCall {
    call_id: String,
    name: String,
    arguments: String,
}
impl CompletedToolCall {
    pub fn call_id(&self) -> &str {
        &self.call_id
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn arguments(&self) -> &str {
        &self.arguments
    }
}
/// Opaque replayable provider items. Not Debug/Serialize/TS: the explicitly named
/// checkpoint methods are for a future private durable store, never logs/API.
pub struct Continuation {
    protocol: Protocol,
    model: String,
    effort: Option<String>,
    history: Vec<Value>,
    pending: Vec<(String, String)>,
}
/// Provider-declared round outcome, not proof of task completion. Calls still
/// require the runtime's typed registry, tenant authority and commit checks.
pub struct ToolRound {
    pub outcome: FinishOutcome,
    pub usage: Usage,
    pub calls: Vec<CompletedToolCall>,
    pub continuation: Continuation,
}
fn dialect(p: Protocol) -> &'static str {
    match p {
        Protocol::Responses => "responses",
        Protocol::AnthropicMessages => "anthropic",
        Protocol::OpenRouterChat => "openrouter_chat",
        Protocol::ChatCompletions => "chat",
    }
}
impl Continuation {
    pub fn to_private_checkpoint(&self) -> Result<Vec<u8>, ProtocolError> {
        let bytes=serde_json::to_vec(&json!({"v":1,"protocol":dialect(self.protocol),"model":self.model,"effort":self.effort,"history":self.history,"pending":self.pending})).map_err(|_|ProtocolError::Protocol)?;
        if bytes.len() > MAX_CAPSULE {
            return Err(ProtocolError::LimitExceeded);
        }
        Ok(bytes)
    }
    /// Checkpoint authenticity/tenant binding belongs to the private Store. This
    /// parser validates bounds and dialect/model pins, not an authorization token.
    pub fn from_private_checkpoint(
        bytes: &[u8],
        protocol: Protocol,
        model: &str,
        effort: Option<&str>,
    ) -> Result<Self, ProtocolError> {
        if bytes.len() > MAX_CAPSULE {
            return Err(ProtocolError::LimitExceeded);
        }
        let v: Value = serde_json::from_slice(bytes).map_err(|_| ProtocolError::Protocol)?;
        if v["v"] != 1
            || v["protocol"] != dialect(protocol)
            || v["model"] != model
            || v.get("effort")
                .is_none_or(|e| !(e.is_null() || e.is_string()))
            || v.get("effort").and_then(Value::as_str) != effort
            || v.as_object().is_none_or(|m| m.len() != 6)
        {
            return Err(ProtocolError::Protocol);
        }
        let history = v["history"]
            .as_array()
            .ok_or(ProtocolError::Protocol)?
            .clone();
        if history.len() > MAX_ITEMS {
            return Err(ProtocolError::LimitExceeded);
        }
        let pending: Vec<(String, String)> =
            serde_json::from_value(v["pending"].clone()).map_err(|_| ProtocolError::Protocol)?;
        if pending.len() > MAX_CALLS {
            return Err(ProtocolError::LimitExceeded);
        }
        let mut seen = BTreeSet::new();
        for (id, name) in &pending {
            valid_id(id)?;
            valid_name(name)?;
            if !seen.insert(id) {
                return Err(ProtocolError::Protocol);
            }
        }
        for item in &history {
            validate_history_item(protocol, item)?;
        }
        validate_history(protocol, &history, &pending)?;
        Ok(Self {
            protocol,
            model: model.into(),
            effort: effort.map(str::to_owned),
            history,
            pending,
        })
    }
}
fn valid_id(id: &str) -> Result<(), ProtocolError> {
    if id.is_empty() || id.len() > 256 || !id.bytes().all(|b| b.is_ascii_graphic()) {
        Err(ProtocolError::Protocol)
    } else {
        Ok(())
    }
}
fn valid_name(name: &str) -> Result<(), ProtocolError> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        Err(ProtocolError::Protocol)
    } else {
        Ok(())
    }
}
fn bounded_json(v: &Value, max: usize) -> Result<(), ProtocolError> {
    if serde_json::to_vec(v)
        .map_err(|_| ProtocolError::Protocol)?
        .len()
        > max
    {
        Err(ProtocolError::LimitExceeded)
    } else {
        Ok(())
    }
}
fn validate_schema(v: &Value, depth: usize) -> Result<(), ProtocolError> {
    if depth > 8 {
        return Err(ProtocolError::LimitExceeded);
    }
    let o = v.as_object().ok_or(ProtocolError::Protocol)?;
    if o.keys().any(|k| {
        !matches!(
            k.as_str(),
            "type"
                | "properties"
                | "required"
                | "additionalProperties"
                | "items"
                | "enum"
                | "description"
        )
    }) {
        return Err(ProtocolError::UnsupportedCapability);
    }
    let types = match &v["type"] {
        Value::String(s) => vec![s.as_str()],
        Value::Array(a) => a
            .iter()
            .map(|v| v.as_str().ok_or(ProtocolError::Protocol))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(ProtocolError::Protocol),
    };
    if types.is_empty()
        || types.len() > 2
        || types.iter().collect::<BTreeSet<_>>().len() != types.len()
        || types.iter().any(|t| {
            !matches!(
                *t,
                "object" | "array" | "string" | "number" | "integer" | "boolean" | "null"
            )
        })
    {
        return Err(ProtocolError::Protocol);
    }
    if let Some(description) = v.get("description")
        && description.as_str().is_none_or(|s| s.len() > 1024)
    {
        return Err(ProtocolError::LimitExceeded);
    }
    if types.contains(&"object") {
        if v["additionalProperties"] != false {
            return Err(ProtocolError::Protocol);
        }
        let props = v["properties"].as_object().ok_or(ProtocolError::Protocol)?;
        let required = v["required"].as_array().ok_or(ProtocolError::Protocol)?;
        if props.len() > 64 || required.len() != props.len() {
            return Err(ProtocolError::Protocol);
        }
        let names = required
            .iter()
            .map(|v| v.as_str().ok_or(ProtocolError::Protocol))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if names.len() != props.len() || props.keys().any(|k| !names.contains(k.as_str())) {
            return Err(ProtocolError::Protocol);
        }
        for (name, child) in props {
            valid_name(name)?;
            validate_schema(child, depth + 1)?;
        }
    }
    if types.contains(&"array") {
        validate_schema(&v["items"], depth + 1)?;
    }
    if let Some(values) = v.get("enum") {
        let a = values.as_array().ok_or(ProtocolError::Protocol)?;
        if a.is_empty() || a.len() > 64 || a.iter().any(|v| v.is_array() || v.is_object()) {
            return Err(ProtocolError::Protocol);
        }
    }
    Ok(())
}
fn definitions(tools: &[FunctionDefinition]) -> Result<(), ProtocolError> {
    if tools.is_empty() || tools.len() > MAX_DEFINITIONS {
        return Err(ProtocolError::LimitExceeded);
    }
    let mut names = BTreeSet::new();
    let mut bytes = 0;
    for t in tools {
        valid_name(&t.name)?;
        if !names.insert(&t.name)
            || t.description.trim().is_empty()
            || t.description.len() > 1024
            || t.description.chars().any(char::is_control)
            || t.parameters["type"] != "object"
        {
            return Err(ProtocolError::Protocol);
        }
        validate_schema(&t.parameters, 0)?;
        bounded_json(&t.parameters, 16 * 1024)?;
        bytes += serde_json::to_vec(&t.parameters)
            .map_err(|_| ProtocolError::Protocol)?
            .len()
            + t.description.len();
    }
    if bytes > 64 * 1024 {
        return Err(ProtocolError::LimitExceeded);
    }
    Ok(())
}
pub fn encode_tool_request(
    protocol: Protocol,
    r: &ToolRequest<'_>,
    text_caps: &TextCapabilities,
    caps: &ToolCapabilities,
) -> Result<Value, ProtocolError> {
    if !caps.function_tools {
        return Err(ProtocolError::UnsupportedCapability);
    }
    definitions(r.tools)?;
    let mut body = encode_request(protocol, r.text, text_caps)?;
    let mut history = Vec::new();
    if let Some(c) = r.continuation {
        if c.protocol != protocol || c.model != r.text.model || c.effort != r.text.effort {
            return Err(ProtocolError::Protocol);
        }
        c.to_private_checkpoint()?;
        if r.results.len() != c.pending.len() {
            return Err(ProtocolError::Protocol);
        }
        let mut seen = BTreeSet::new();
        for result in r.results {
            valid_id(&result.call_id)?;
            if result_wire_output(result).len() > MAX_ARGUMENTS {
                return Err(ProtocolError::LimitExceeded);
            }
            if !c.pending.iter().any(|(id, _)| id == &result.call_id)
                || !seen.insert(&result.call_id)
            {
                return Err(ProtocolError::Protocol);
            }
        }
        history = c.history.clone();
        match protocol {
            Protocol::Responses => {
                for result in r.results {
                    history.push(json!({"type":"function_call_output","call_id":result.call_id,"output":result_wire_output(result)}));
                }
            }
            Protocol::AnthropicMessages => {
                if !r.results.is_empty() {
                    history.push(json!({"role":"user","content":r.results.iter().map(|x|json!({"type":"tool_result","tool_use_id":x.call_id,"content":x.output,"is_error":x.is_error})).collect::<Vec<_>>()}));
                }
            }
            _ => {
                for result in r.results {
                    history.push(json!({"role":"tool","tool_call_id":result.call_id,"content":result_wire_output(result)}));
                }
            }
        }
    } else if !r.results.is_empty() {
        return Err(ProtocolError::Protocol);
    }
    bounded_json(&json!(history), MAX_CAPSULE)?;
    let key = if protocol == Protocol::Responses {
        "input"
    } else {
        "messages"
    };
    body[key]
        .as_array_mut()
        .ok_or(ProtocolError::Protocol)?
        .extend(history);
    body["tools"]=Value::Array(r.tools.iter().map(|t|match protocol{
 Protocol::Responses=>json!({"type":"function","name":t.name,"description":t.description,"parameters":t.parameters,"strict":true}),
 Protocol::AnthropicMessages=>json!({"name":t.name,"description":t.description,"input_schema":t.parameters,"strict":true}),
 _=>json!({"type":"function","function":{"name":t.name,"description":t.description,"parameters":t.parameters,"strict":true}}),}).collect());
    if protocol != Protocol::AnthropicMessages {
        body["parallel_tool_calls"] = json!(false);
    }
    if protocol == Protocol::Responses {
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
    bounded_json(&body, MAX_REQUEST_BYTES)?;
    Ok(body)
}

struct Call {
    id: String,
    name: String,
    args: String,
    args_done: bool,
    closed: bool,
}
pub(super) struct RoundState {
    protocol: Protocol,
    model: String,
    effort: Option<String>,
    history: Vec<Value>,
    allowed: BTreeSet<String>,
    calls: BTreeMap<u64, Call>,
    items: BTreeMap<u64, Value>,
    closed: BTreeSet<u64>,
    chat: Value,
    reasoning: Vec<Value>,
    outcome: Option<FinishOutcome>,
}
pub struct ToolRoundDecoder {
    inner: StreamDecoder,
}
impl ToolRoundDecoder {
    pub fn new(
        protocol: Protocol,
        r: &ToolRequest<'_>,
        text_caps: &TextCapabilities,
        caps: &ToolCapabilities,
    ) -> Result<Self, ProtocolError> {
        let body = encode_tool_request(protocol, r, text_caps, caps)?;
        let key = if protocol == Protocol::Responses {
            "input"
        } else {
            "messages"
        };
        let initial = encode_request(protocol, r.text, text_caps)?[key]
            .as_array()
            .ok_or(ProtocolError::Protocol)?
            .len();
        let history = body[key].as_array().ok_or(ProtocolError::Protocol)?[initial..].to_vec();
        let mut inner = StreamDecoder::new(protocol);
        inner.tools = Some(RoundState {
            protocol,
            model: r.text.model.clone(),
            effort: r.text.effort.clone(),
            history,
            allowed: r.tools.iter().map(|t| t.name.clone()).collect(),
            calls: BTreeMap::new(),
            items: BTreeMap::new(),
            closed: BTreeSet::new(),
            chat: json!({"role":"assistant","content":null}),
            reasoning: Vec::new(),
            outcome: None,
        });
        Ok(Self { inner })
    }
    pub fn push(&mut self, bytes: &[u8]) -> DecodeBatch {
        let mut batch = self.inner.push(bytes);
        batch
            .events
            .retain(|e| !matches!(e, ModelEvent::Finished { .. }));
        batch
    }
    pub fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }
    /// Declares actual stream EOF, not just receipt of a terminal marker.
    /// ModelClient waits for bounded clean HTTP EOF before invoking this method.
    pub fn finish(mut self) -> Result<ToolRound, ProtocolError> {
        self.inner.finish()?;
        let state = self.inner.tools.take().ok_or(ProtocolError::Protocol)?;
        state.finish(self.inner.usage, self.inner.stop.map(|s| s.0))
    }
}
impl RoundState {
    fn identity(&self, d: &StreamDecoder, v: &Value) -> Result<(), ProtocolError> {
        if let Some(model) = nonnull(v, "model")
            && model.as_str() != Some(self.model.as_str())
        {
            return Err(ProtocolError::Protocol);
        }
        if d.started && d.model.as_deref() != Some(self.model.as_str()) {
            return Err(ProtocolError::Protocol);
        }
        Ok(())
    }
    fn new_call(
        &mut self,
        index: u64,
        id: String,
        name: String,
        args: String,
    ) -> Result<(), ProtocolError> {
        valid_id(&id)?;
        valid_name(&name)?;
        if !self.allowed.contains(&name) {
            return Err(ProtocolError::UnsupportedCapability);
        }
        if self
            .history
            .iter()
            .any(|v| history_call_ids(self.protocol, v).contains(&id))
        {
            return Err(ProtocolError::Protocol);
        }
        if self.calls.len() >= MAX_CALLS || args.len() > MAX_ARGUMENTS {
            return Err(ProtocolError::LimitExceeded);
        }
        if self.calls.contains_key(&index) || self.calls.values().any(|c| c.id == id) {
            return Err(ProtocolError::Protocol);
        }
        self.calls.insert(
            index,
            Call {
                id,
                name,
                args,
                args_done: false,
                closed: false,
            },
        );
        Ok(())
    }
    fn call(&mut self, index: u64) -> Result<&mut Call, ProtocolError> {
        self.calls.get_mut(&index).ok_or(ProtocolError::Protocol)
    }
    fn append_args(&mut self, index: u64, delta: &str) -> Result<(), ProtocolError> {
        let c = self.call(index)?;
        if c.closed || c.args_done {
            return Err(ProtocolError::Protocol);
        }
        if c.args.len().saturating_add(delta.len()) > MAX_ARGUMENTS {
            return Err(ProtocolError::LimitExceeded);
        }
        c.args.push_str(delta);
        Ok(())
    }
    fn validate_args(c: &Call) -> Result<(), ProtocolError> {
        let v: Value = serde_json::from_str(&c.args).map_err(|_| ProtocolError::Protocol)?;
        if !v.is_object() {
            return Err(ProtocolError::Protocol);
        }
        Ok(())
    }
    fn finish(
        mut self,
        usage: Usage,
        stop: Option<FinishOutcome>,
    ) -> Result<ToolRound, ProtocolError> {
        let mut outcome = self.outcome.or(stop).ok_or(ProtocolError::Protocol)?;
        if self.calls.values().any(|c| !c.closed || !c.args_done) {
            return Err(ProtocolError::Truncated);
        }
        // Incomplete/refused rounds cannot yield executable calls.
        // No-call terminal rounds retain observed history; requesting another
        // round is a caller policy decision.
        if !self.calls.is_empty() {
            if !matches!(
                outcome,
                FinishOutcome::Completed | FinishOutcome::ContinuationRequired
            ) {
                return Err(ProtocolError::Protocol);
            }
            outcome = FinishOutcome::ContinuationRequired;
        } else if outcome == FinishOutcome::ContinuationRequired {
            return Err(ProtocolError::UnsupportedCapability);
        }
        let mut calls = Vec::new();
        for c in self.calls.into_values() {
            Self::validate_args(&c)?;
            calls.push(CompletedToolCall {
                call_id: c.id,
                name: c.name,
                arguments: c.args,
            });
        }
        match self.protocol {
            Protocol::Responses => self.history.extend(self.items.into_values()),
            Protocol::AnthropicMessages => self.history.push(
                json!({"role":"assistant","content":self.items.into_values().collect::<Vec<_>>()}),
            ),
            _ => {
                if !self.reasoning.is_empty() {
                    self.chat["reasoning_details"] = json!(self.reasoning);
                }
                self.history.push(self.chat);
            }
        }
        let pending = calls
            .iter()
            .map(|c| (c.call_id.clone(), c.name.clone()))
            .collect::<Vec<_>>();
        validate_history(self.protocol, &self.history, &pending)?;
        let continuation = Continuation {
            protocol: self.protocol,
            model: self.model,
            effort: self.effort,
            history: self.history,
            pending,
        };
        continuation.to_private_checkpoint()?;
        Ok(ToolRound {
            outcome,
            usage,
            calls,
            continuation,
        })
    }
}

fn only_keys(v: &Value, allowed: &[&str]) -> Result<(), ProtocolError> {
    if v.as_object()
        .is_none_or(|m| m.keys().any(|k| !allowed.contains(&k.as_str())))
    {
        Err(ProtocolError::UnsupportedCapability)
    } else {
        Ok(())
    }
}
fn validate_history_item(protocol: Protocol, v: &Value) -> Result<(), ProtocolError> {
    bounded_json(v, MAX_CAPSULE)?;
    let keys = |allowed: &[&str]| -> Result<(), ProtocolError> {
        if v.as_object()
            .is_none_or(|m| m.keys().any(|k| !allowed.contains(&k.as_str())))
        {
            Err(ProtocolError::UnsupportedCapability)
        } else {
            Ok(())
        }
    };
    match protocol {
        Protocol::Responses => match required_text(v, "type")? {
            "message" => {
                keys(&["id", "type", "role", "status", "content", "phase"])?;
                if required_text(v, "role")? != "assistant" {
                    return Err(ProtocolError::Protocol);
                }
                phase(v)?;
                let parts = v["content"].as_array().ok_or(ProtocolError::Protocol)?;
                for p in parts {
                    response_part_kind(p)?;
                    only_keys(p, &["type", "text", "refusal", "annotations", "logprobs"])?;
                    required_text(
                        p,
                        if p["type"] == "refusal" {
                            "refusal"
                        } else {
                            "text"
                        },
                    )?;
                }
            }
            "reasoning" => {
                if nonnull(v, "encrypted_content").is_some_and(|v| !v.is_string())
                    || v.get("summary").is_some_and(|v| !v.is_array())
                {
                    return Err(ProtocolError::Protocol);
                }
                if v.as_object().is_none_or(|m| {
                    m.keys().any(|k| {
                        !matches!(
                            k.as_str(),
                            "id" | "type" | "summary" | "encrypted_content" | "status"
                        )
                    })
                }) {
                    return Err(ProtocolError::UnsupportedCapability);
                }
            }
            "function_call" => {
                keys(&["id", "type", "call_id", "name", "arguments", "status"])?;
                required_id(v, "id")?;
                valid_id(required_text(v, "call_id")?)?;
                valid_name(required_text(v, "name")?)?;
                if required_text(v, "arguments")?.len() > MAX_ARGUMENTS {
                    return Err(ProtocolError::LimitExceeded);
                }
            }
            "function_call_output" => {
                keys(&["type", "call_id", "output"])?;
                valid_id(required_text(v, "call_id")?)?;
                if required_text(v, "output")?.len() > MAX_ARGUMENTS {
                    return Err(ProtocolError::LimitExceeded);
                }
            }
            _ => return Err(ProtocolError::UnsupportedCapability),
        },
        Protocol::AnthropicMessages => {
            keys(&["role", "content"])?;
            if !matches!(required_text(v, "role")?, "assistant" | "user") {
                return Err(ProtocolError::Protocol);
            }
            let content = v["content"].as_array().ok_or(ProtocolError::Protocol)?;
            for b in content {
                match required_text(b, "type")? {
                    "text" => {
                        only_keys(b, &["type", "text"])?;
                        required_text(b, "text")?;
                    }
                    "thinking" => {
                        only_keys(b, &["type", "thinking", "signature"])?;
                        required_text(b, "thinking")?;
                        required_text(b, "signature")?;
                    }
                    "redacted_thinking" => {
                        only_keys(b, &["type", "data"])?;
                        required_text(b, "data")?;
                    }
                    "tool_use" => {
                        only_keys(b, &["type", "id", "name", "input"])?;
                        required_id(b, "id")?;
                        valid_name(required_text(b, "name")?)?;
                        if !b["input"].is_object() {
                            return Err(ProtocolError::Protocol);
                        }
                    }
                    "tool_result" => {
                        only_keys(b, &["type", "tool_use_id", "content", "is_error"])?;
                        required_id(b, "tool_use_id")?;
                        required_text(b, "content")?;
                        if b.get("is_error").is_some_and(|v| !v.is_boolean()) {
                            return Err(ProtocolError::Protocol);
                        }
                    }
                    _ => return Err(ProtocolError::UnsupportedCapability),
                }
            }
        }
        _ => {
            keys(&[
                "role",
                "content",
                "refusal",
                "tool_calls",
                "tool_call_id",
                "reasoning",
                "reasoning_content",
                "reasoning_details",
            ])?;
            if !matches!(required_text(v, "role")?, "assistant" | "tool") {
                return Err(ProtocolError::Protocol);
            }
            if v["role"] == "tool" {
                required_id(v, "tool_call_id")?;
                required_text(v, "content")?;
            } else {
                if nonnull(v, "content").is_some_and(|x| !x.is_string())
                    || nonnull(v, "refusal").is_some_and(|x| !x.is_string())
                {
                    return Err(ProtocolError::UnsupportedCapability);
                }
                if let Some(a) = nonnull(v, "tool_calls") {
                    for c in a.as_array().ok_or(ProtocolError::Protocol)? {
                        required_id(c, "id")?;
                        if c["type"] != "function" {
                            return Err(ProtocolError::UnsupportedCapability);
                        }
                        valid_name(required_text(&c["function"], "name")?)?;
                        required_text(&c["function"], "arguments")?;
                    }
                }
                if let Some(a) = nonnull(v, "reasoning_details") {
                    validate_reasoning(a)?;
                }
            }
        }
    }
    Ok(())
}
fn validate_reasoning(v: &Value) -> Result<(), ProtocolError> {
    let a = v.as_array().ok_or(ProtocolError::Protocol)?;
    if a.len() > MAX_ITEMS {
        return Err(ProtocolError::LimitExceeded);
    }
    for r in a {
        only_keys(
            r,
            &[
                "type",
                "id",
                "format",
                "index",
                "text",
                "summary",
                "data",
                "signature",
            ],
        )?;
        if r["data"] == "[REDACTED]" {
            return Err(ProtocolError::UnsupportedCapability);
        }
        for key in ["text", "summary", "data", "signature"] {
            if nonnull(r, key).is_some_and(|v| !v.is_string()) {
                return Err(ProtocolError::Protocol);
            }
        }
        if !matches!(
            required_text(r, "type")?,
            "reasoning.text" | "reasoning.summary" | "reasoning.encrypted"
        ) {
            return Err(ProtocolError::UnsupportedCapability);
        }
        if !r.is_object() {
            return Err(ProtocolError::Protocol);
        }
    }
    Ok(())
}
fn canonical_response_item(d: &StreamDecoder, item: &Value) -> Result<Value, ProtocolError> {
    let mut result = item.clone();
    if item["type"] == "message" {
        let known = d
            .response_items
            .get(required_text(item, "id")?)
            .ok_or(ProtocolError::Protocol)?;
        if let Some(phase) = known.phase {
            result["phase"] = json!(match phase {
                AssistantPhase::Commentary => "commentary",
                AssistantPhase::FinalAnswer => "final_answer",
            });
        }
    }
    Ok(result)
}
fn tool_placeholder(item: &Value) -> Value {
    json!({"type":"reasoning","id":item["id"]})
}
impl RoundState {
    pub(super) fn dispatch(
        &mut self,
        d: &mut StreamDecoder,
        name: &str,
        v: &Value,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        self.identity(d, v)?;
        match self.protocol {
            Protocol::Responses => self.responses(d, name, v, events),
            Protocol::AnthropicMessages => self.anthropic(d, name, v, events),
            _ => self.chat(d, v, events),
        }?;
        if d.started && d.model.as_deref() != Some(self.model.as_str()) {
            return Err(ProtocolError::Protocol);
        }
        let size = serde_json::to_vec(&(&self.items, &self.chat, &self.reasoning))
            .map_err(|_| ProtocolError::Protocol)?
            .len();
        if size > MAX_CAPSULE {
            return Err(ProtocolError::LimitExceeded);
        }
        Ok(())
    }
    fn responses(
        &mut self,
        d: &mut StreamDecoder,
        name: &str,
        v: &Value,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        let kind = event_type(name, v)?;
        if let Some(response) = nonnull(v, "response") {
            self.identity(d, response)?;
        }
        if let Some(response_id) = nonnull(v, "response_id")
            && d.response_id.as_deref() != response_id.as_str()
        {
            return Err(ProtocolError::Protocol);
        }
        match kind {
            "response.output_item.added" => {
                d.require_started()?;
                let item = object(v, "item")?;
                let idx = index(v, "output_index")?;
                validate_history_item(self.protocol, item)?;
                if self.items.contains_key(&idx)
                    || self.items.values().any(|x| x["id"] == item["id"])
                {
                    return Err(ProtocolError::Protocol);
                }
                if item["type"] == "function_call" {
                    self.new_call(
                        idx,
                        required_id(item, "call_id")?,
                        required_text(item, "name")?.into(),
                        required_text(item, "arguments")?.into(),
                    )?;
                    let mut shadow = v.clone();
                    shadow["item"] = tool_placeholder(item);
                    d.responses(name, &shadow, events)?;
                } else {
                    d.responses(name, v, events)?;
                }
                self.items.insert(idx, item.clone());
            }
            "response.function_call_arguments.delta" | "response.function_call_arguments.done" => {
                d.require_started()?;
                let idx = index(v, "output_index")?;
                let item = self.items.get(&idx).ok_or(ProtocolError::Protocol)?;
                if item["id"] != required_text(v, "item_id")? {
                    return Err(ProtocolError::Protocol);
                }
                if kind.ends_with(".delta") {
                    self.append_args(idx, required_text(v, "delta")?)?;
                } else {
                    let c = self.call(idx)?;
                    if c.closed || c.args_done || required_text(v, "arguments")? != c.args {
                        return Err(ProtocolError::Protocol);
                    }
                    Self::validate_args(c)?;
                    c.args_done = true;
                }
            }
            "response.output_item.done" => {
                let item = object(v, "item")?;
                let idx = index(v, "output_index")?;
                validate_history_item(self.protocol, item)?;
                if self.closed.contains(&idx) {
                    return Err(ProtocolError::Protocol);
                }
                let original = self.items.get(&idx).ok_or(ProtocolError::Protocol)?;
                if item["type"] != original["type"] || item["id"] != original["id"] {
                    return Err(ProtocolError::Protocol);
                }
                if item["type"] == "function_call" {
                    let c = self.call(idx)?;
                    if !c.args_done
                        || c.closed
                        || required_text(item, "call_id")? != c.id
                        || required_text(item, "name")? != c.name
                        || required_text(item, "arguments")? != c.args
                        || item["status"] != "completed"
                    {
                        return Err(ProtocolError::Protocol);
                    }
                    c.closed = true;
                    let mut shadow = v.clone();
                    shadow["item"] = tool_placeholder(item);
                    d.responses(name, &shadow, events)?;
                } else {
                    d.responses(name, v, events)?;
                }
                self.items.insert(idx, canonical_response_item(d, item)?);
                self.closed.insert(idx);
            }
            "response.completed" | "response.incomplete" => {
                let output = v["response"]["output"]
                    .as_array()
                    .ok_or(ProtocolError::Protocol)?;
                if output.len() != self.items.len() || self.closed.len() != self.items.len() {
                    return Err(ProtocolError::Protocol);
                }
                let mut shadow = v.clone();
                for (idx, item) in output.iter().enumerate() {
                    let known = self
                        .items
                        .get(&(idx as u64))
                        .ok_or(ProtocolError::Protocol)?;
                    let mut old = known.clone();
                    let mut next = item.clone();
                    old.as_object_mut()
                        .ok_or(ProtocolError::Protocol)?
                        .remove("phase");
                    next.as_object_mut()
                        .ok_or(ProtocolError::Protocol)?
                        .remove("phase");
                    if old != next {
                        return Err(ProtocolError::Protocol);
                    }
                    validate_history_item(self.protocol, item)?;
                    if item["type"] == "function_call" {
                        shadow["response"]["output"][idx] = tool_placeholder(item);
                    }
                }
                d.responses(name, &shadow, events)?;
                for (idx, item) in output.iter().enumerate() {
                    self.items
                        .insert(idx as u64, canonical_response_item(d, item)?);
                }
                self.outcome = events.iter().rev().find_map(|event| match event {
                    ModelEvent::Finished { outcome, .. } => Some(*outcome),
                    _ => None,
                });
            }
            "response.created"
            | "response.in_progress"
            | "response.queued"
            | "response.content_part.added"
            | "response.content_part.done"
            | "response.output_text.delta"
            | "response.output_text.done"
            | "response.refusal.delta"
            | "response.refusal.done"
            | "response.reasoning_summary_part.added"
            | "response.reasoning_summary_part.done"
            | "response.reasoning_summary_text.delta"
            | "response.reasoning_summary_text.done"
            | "response.reasoning_text.delta"
            | "response.reasoning_text.done"
            | "response.failed"
            | "error" => {
                d.responses(name, v, events)?;
            }
            _ => return Err(ProtocolError::UnsupportedCapability),
        }
        Ok(())
    }
    fn anthropic(
        &mut self,
        d: &mut StreamDecoder,
        name: &str,
        v: &Value,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        let kind = event_type(name, v)?;
        match kind {
            "message_start" => {
                self.identity(d, object(v, "message")?)?;
                d.anthropic(name, v, events)?;
            }
            "content_block_start" => {
                d.require_started()?;
                if d.stop.is_some() {
                    return Err(ProtocolError::Protocol);
                }
                let idx = index(v, "index")?;
                if idx != self.items.len() as u64 {
                    return Err(ProtocolError::Protocol);
                }
                let b = object(v, "content_block")?;
                let mut shadow = v.clone();
                match required_text(b, "type")? {
                    "tool_use" => {
                        if b.get("toolset_name").is_some() {
                            return Err(ProtocolError::UnsupportedCapability);
                        }
                        if b["input"] != json!({}) {
                            return Err(ProtocolError::Protocol);
                        }
                        self.new_call(
                            idx,
                            required_id(b, "id")?,
                            required_text(b, "name")?.into(),
                            String::new(),
                        )?;
                        shadow["content_block"] =
                            json!({"type":"thinking","thinking":"","signature":""});
                    }
                    "text" | "thinking" | "redacted_thinking" => {}
                    _ => return Err(ProtocolError::UnsupportedCapability),
                }
                d.anthropic(name, &shadow, events)?;
                self.items.insert(idx, b.clone());
            }
            "content_block_delta" => {
                let idx = index(v, "index")?;
                if self.closed.contains(&idx) || d.stop.is_some() {
                    return Err(ProtocolError::Protocol);
                }
                let delta = object(v, "delta")?;
                let b = self.items.get_mut(&idx).ok_or(ProtocolError::Protocol)?;
                match (required_text(b, "type")?, required_text(delta, "type")?) {
                    ("tool_use", "input_json_delta") => {
                        self.append_args(idx, required_text(delta, "partial_json")?)?;
                    }
                    ("text", "text_delta") => {
                        let text = b["text"]
                            .as_str()
                            .ok_or(ProtocolError::Protocol)?
                            .to_owned()
                            + required_text(delta, "text")?;
                        b["text"] = json!(text);
                        d.anthropic(name, v, events)?;
                    }
                    ("thinking", "thinking_delta") => {
                        let text = b["thinking"]
                            .as_str()
                            .ok_or(ProtocolError::Protocol)?
                            .to_owned()
                            + required_text(delta, "thinking")?;
                        b["thinking"] = json!(text);
                        d.anthropic(name, v, events)?;
                    }
                    ("thinking", "signature_delta") => {
                        let text = b
                            .get("signature")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_owned()
                            + required_text(delta, "signature")?;
                        b["signature"] = json!(text);
                        d.anthropic(name, v, events)?;
                    }
                    _ => return Err(ProtocolError::UnsupportedCapability),
                }
            }
            "content_block_stop" => {
                let idx = index(v, "index")?;
                if self.closed.contains(&idx) {
                    return Err(ProtocolError::Protocol);
                }
                if self.calls.contains_key(&idx) {
                    let c = self.call(idx)?;
                    // An empty-parameter tool can close with the initial input:{}
                    // and no JSON deltas. A nonempty partial string still fails.
                    if c.args.is_empty() {
                        c.args = "{}".into();
                    }
                    Self::validate_args(c)?;
                    c.args_done = true;
                    c.closed = true;
                    let input: Value =
                        serde_json::from_str(&c.args).map_err(|_| ProtocolError::Protocol)?;
                    self.items.get_mut(&idx).ok_or(ProtocolError::Protocol)?["input"] = input;
                }
                d.anthropic(name, v, events)?;
                self.closed.insert(idx);
                validate_history_item(
                    self.protocol,
                    &json!({"role":"assistant","content":[self.items[&idx].clone()]}),
                )?;
            }
            "message_delta" => {
                if !self.calls.is_empty()
                    && nonnull(&v["delta"], "stop_reason").is_some_and(|v| v != "tool_use")
                {
                    return Err(ProtocolError::Protocol);
                }
                d.anthropic(name, v, events)?;
            }
            "message_stop" => {
                d.anthropic(name, v, events)?;
                self.outcome = d.stop.map(|x| x.0);
            }
            "ping" | "error" => {
                d.anthropic(name, v, events)?;
            }
            _ => return Err(ProtocolError::UnsupportedCapability),
        }
        Ok(())
    }
    fn merge_reasoning(&mut self, detail: &Value) -> Result<(), ProtocolError> {
        let idx = detail.get("index").and_then(Value::as_u64);
        if let Some(idx) = idx {
            if idx >= MAX_ITEMS as u64 {
                return Err(ProtocolError::LimitExceeded);
            }
            if let Some(existing) = self
                .reasoning
                .iter_mut()
                .find(|r| r.get("index").and_then(Value::as_u64) == Some(idx))
            {
                for (key, value) in detail.as_object().ok_or(ProtocolError::Protocol)? {
                    if matches!(key.as_str(), "text" | "summary" | "data" | "signature") {
                        let part = value.as_str().ok_or(ProtocolError::Protocol)?;
                        let old = existing.get(key).and_then(Value::as_str).unwrap_or("");
                        existing[key] = json!(old.to_owned() + part);
                    } else if !value.is_null() {
                        if existing
                            .get(key)
                            .is_some_and(|old| !old.is_null() && old != value)
                        {
                            return Err(ProtocolError::Protocol);
                        }
                        existing[key] = value.clone();
                    }
                }
                return Ok(());
            }
            if self
                .reasoning
                .iter()
                .filter_map(|r| r.get("index").and_then(Value::as_u64))
                .any(|old| old >= idx)
            {
                return Err(ProtocolError::Protocol);
            }
        }
        if self.reasoning.len() >= MAX_ITEMS {
            return Err(ProtocolError::LimitExceeded);
        }
        self.reasoning.push(detail.clone());
        Ok(())
    }
    fn chat(
        &mut self,
        d: &mut StreamDecoder,
        v: &Value,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        self.identity(d, v)?;
        if !d.started {
            required_id(v, "id")?;
        }
        let choices = v["choices"].as_array().ok_or(ProtocolError::Protocol)?;
        let mut shadow = v.clone();
        if let Some(choice) = choices.first() {
            let delta = object(choice, "delta")?;
            if d.stop.is_some() && delta.as_object().is_some_and(|m| m.values().any(nonempty)) {
                return Err(ProtocolError::Protocol);
            }
            if delta.as_object().is_none_or(|m| {
                m.keys().any(|k| {
                    !matches!(
                        k.as_str(),
                        "role"
                            | "content"
                            | "refusal"
                            | "tool_calls"
                            | "reasoning"
                            | "reasoning_content"
                            | "reasoning_details"
                    )
                })
            }) {
                return Err(ProtocolError::UnsupportedCapability);
            }
            for field in ["content", "refusal", "reasoning", "reasoning_content"] {
                if let Some(t) = optional_text(delta, field)? {
                    let old = self.chat.get(field).and_then(Value::as_str).unwrap_or("");
                    self.chat[field] = json!(old.to_owned() + t);
                }
            }
            if let Some(details) = nonnull(delta, "reasoning_details") {
                validate_reasoning(details)?;
                for detail in details.as_array().ok_or(ProtocolError::Protocol)? {
                    self.merge_reasoning(detail)?;
                }
            }
            if let Some(tool_calls) = nonnull(delta, "tool_calls") {
                for c in tool_calls.as_array().ok_or(ProtocolError::Protocol)? {
                    only_keys(c, &["index", "id", "type", "function"])?;
                    only_keys(&c["function"], &["name", "arguments"])?;
                    let idx = index(c, "index")?;
                    if !self.calls.contains_key(&idx) {
                        if idx != self.calls.len() as u64 {
                            return Err(ProtocolError::Protocol);
                        }
                        if c["type"] != "function" {
                            return Err(ProtocolError::UnsupportedCapability);
                        }
                        self.new_call(
                            idx,
                            required_id(c, "id")?,
                            required_text(&c["function"], "name")?.into(),
                            String::new(),
                        )?;
                    }
                    let known = self.call(idx)?;
                    if nonnull(c, "id").is_some_and(|id| id.as_str() != Some(known.id.as_str()))
                        || nonnull(c, "type").is_some_and(|t| t != "function")
                        || nonnull(&c["function"], "name")
                            .is_some_and(|name| name.as_str() != Some(known.name.as_str()))
                    {
                        return Err(ProtocolError::Protocol);
                    }
                    self.append_args(
                        idx,
                        optional_text(&c["function"], "arguments")?.unwrap_or(""),
                    )?;
                }
                shadow["choices"][0]["delta"]
                    .as_object_mut()
                    .ok_or(ProtocolError::Protocol)?
                    .remove("tool_calls");
            }
            if let Some(reason) = nonnull(choice, "finish_reason") {
                let (outcome, _) = chat_stop(reason.as_str().ok_or(ProtocolError::Protocol)?)?;
                if !self.calls.is_empty() {
                    if outcome != FinishOutcome::ContinuationRequired {
                        return Err(ProtocolError::Protocol);
                    }
                    for c in self.calls.values_mut() {
                        Self::validate_args(c)?;
                        c.args_done = true;
                        c.closed = true;
                    }
                }
            }
        }
        d.chat(&shadow, events)?;
        if !self.calls.is_empty() {
            self.chat["tool_calls"]=json!(self.calls.values().map(|c|json!({"id":c.id,"type":"function","function":{"name":c.name,"arguments":c.args}})).collect::<Vec<_>>());
        }
        Ok(())
    }
}

fn history_call_ids(protocol: Protocol, v: &Value) -> Vec<String> {
    match protocol {
        Protocol::Responses => {
            if v["type"] == "function_call" {
                v["call_id"]
                    .as_str()
                    .map(|s| vec![s.into()])
                    .unwrap_or_default()
            } else {
                vec![]
            }
        }
        Protocol::AnthropicMessages => v["content"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|v| v["type"] == "tool_use")
            .filter_map(|v| v["id"].as_str().map(str::to_owned))
            .collect(),
        _ => v["tool_calls"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v["id"].as_str().map(str::to_owned))
            .collect(),
    }
}
#[cfg(test)]
mod tests;

fn result_wire_output(result: &ToolResult) -> String {
    if result.is_error {
        json!({"is_error":true,"output":result.output}).to_string()
    } else {
        result.output.clone()
    }
}
fn validate_history(
    protocol: Protocol,
    history: &[Value],
    pending: &[(String, String)],
) -> Result<(), ProtocolError> {
    if history.len() > MAX_ITEMS {
        return Err(ProtocolError::LimitExceeded);
    }
    let mut seen = BTreeSet::new();
    let mut unresolved = BTreeMap::new();
    for item in history {
        let mut calls = Vec::new();
        let mut results = Vec::new();
        match protocol {
            Protocol::Responses => match item["type"].as_str() {
                Some("function_call") => calls.push((
                    required_text(item, "call_id")?.to_owned(),
                    required_text(item, "name")?.to_owned(),
                )),
                Some("function_call_output") => {
                    results.push(required_text(item, "call_id")?.to_owned())
                }
                _ => {}
            },
            Protocol::AnthropicMessages => {
                if item["role"] == "assistant" && !unresolved.is_empty() {
                    return Err(ProtocolError::Protocol);
                }
                for block in item["content"].as_array().ok_or(ProtocolError::Protocol)? {
                    match block["type"].as_str() {
                        Some("tool_use") if item["role"] == "assistant" => calls.push((
                            required_text(block, "id")?.into(),
                            required_text(block, "name")?.into(),
                        )),
                        Some("tool_result") if item["role"] == "user" => {
                            results.push(required_text(block, "tool_use_id")?.into())
                        }
                        _ if item["role"] == "user" || block["type"] == "tool_result" => {
                            return Err(ProtocolError::Protocol);
                        }
                        _ => {}
                    }
                }
            }
            _ => {
                if item["role"] == "assistant" {
                    if !unresolved.is_empty() {
                        return Err(ProtocolError::Protocol);
                    }
                    for call in item["tool_calls"].as_array().into_iter().flatten() {
                        calls.push((
                            required_text(call, "id")?.into(),
                            required_text(&call["function"], "name")?.into(),
                        ));
                    }
                } else {
                    results.push(required_text(item, "tool_call_id")?.into());
                }
            }
        }
        match protocol {
            Protocol::Responses if item["type"] == "function_call" => {
                if item["status"] != "completed" {
                    return Err(ProtocolError::Protocol);
                }
                let args: Value = serde_json::from_str(required_text(item, "arguments")?)
                    .map_err(|_| ProtocolError::Protocol)?;
                if !args.is_object() {
                    return Err(ProtocolError::Protocol);
                }
            }
            Protocol::ChatCompletions | Protocol::OpenRouterChat => {
                for call in item["tool_calls"].as_array().into_iter().flatten() {
                    let raw = required_text(&call["function"], "arguments")?;
                    if raw.len() > MAX_ARGUMENTS {
                        return Err(ProtocolError::LimitExceeded);
                    }
                    let args: Value =
                        serde_json::from_str(raw).map_err(|_| ProtocolError::Protocol)?;
                    if !args.is_object() {
                        return Err(ProtocolError::Protocol);
                    }
                }
            }
            _ => {}
        }
        for (id, name) in calls {
            if !seen.insert(id.clone()) || unresolved.insert(id, name).is_some() {
                return Err(ProtocolError::Protocol);
            }
        }
        for id in results {
            if unresolved.remove(&id).is_none() {
                return Err(ProtocolError::Protocol);
            }
        }
    }
    let wanted = pending.iter().cloned().collect::<BTreeMap<_, _>>();
    if wanted.len() != pending.len() || wanted != unresolved {
        return Err(ProtocolError::Protocol);
    }
    Ok(())
}
