//! Bounded text-only request encoding and incremental SSE decoding.
//!
//! This module performs no IO, retry, model selection or provider fallback.
//! A terminal event is evidence of the provider's declared outcome, not evidence
//! that an incomplete/refused/continuation result satisfied the user's request.
pub mod tools;

use std::collections::BTreeMap;

use serde_json::{Value, json};
use thiserror::Error;

const MAX_MESSAGES: usize = 128;
const MAX_MESSAGE_BYTES: usize = 64 * 1024;
const MAX_REQUEST_BYTES: usize = 512 * 1024;
const MAX_OUTPUT_TOKENS: u32 = 131_072;
const MAX_LINE_BYTES: usize = 64 * 1024;
const MAX_EVENT_BYTES: usize = 256 * 1024;
const MAX_STREAM_BYTES: usize = 8 * 1024 * 1024;
const MAX_TEXT_BYTES: usize = 1024 * 1024;
const MAX_EVENTS: usize = 32_768;
const MAX_ITEMS: usize = 256;
const MAX_PARTS: usize = 64;
const MAX_TOKEN_COUNT: u64 = 1_000_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Protocol {
    Responses,
    AnthropicMessages,
    OpenRouterChat,
    ChatCompletions,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TextRole {
    System,
    User,
    Assistant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssistantPhase {
    Commentary,
    FinalAnswer,
}

// Deliberately no Debug on request/event DTOs containing private text or IDs.
pub struct TextMessage {
    pub role: TextRole,
    pub text: String,
    pub phase: Option<AssistantPhase>,
}

pub struct TextRequest {
    pub model: String,
    pub messages: Vec<TextMessage>,
    pub max_output_tokens: u32,
    pub effort: Option<String>,
}

#[derive(Default)]
pub struct TextCapabilities {
    /// Explicitly verified levels for this exact model/provider combination.
    pub efforts: Vec<String>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FinishOutcome {
    Completed,
    Incomplete,
    Refused,
    ContinuationRequired,
}

pub enum ModelEvent {
    Started {
        response_id: Option<String>,
        model: Option<String>,
    },
    /// Persist this on the message, including when phase arrives after deltas.
    MessageMetadata {
        item_id: String,
        phase: Option<AssistantPhase>,
    },
    TextDelta {
        text: String,
        item_id: Option<String>,
        /// Missing phase stays unknown; consumers must not infer final_answer.
        phase: Option<AssistantPhase>,
    },
    RefusalDelta {
        text: String,
        item_id: Option<String>,
        phase: Option<AssistantPhase>,
    },
    UsageSnapshot(Usage),
    Finished {
        outcome: FinishOutcome,
        /// A validated protocol reason, never an arbitrary provider error body.
        native_reason: String,
    },
}

/// A chunk can contain valid events followed by a malformed/error frame.
/// Callers must forward the events and then handle error without declaring success.
pub struct DecodeBatch {
    pub events: Vec<ModelEvent>,
    pub error: Option<ProtocolError>,
}

#[derive(Clone, Copy, Debug, Error, PartialEq, Eq)]
pub enum ProtocolError {
    #[error("invalid model protocol data")]
    Protocol,
    #[error("model stream ended before a valid terminal marker")]
    Truncated,
    #[error("model protocol limit exceeded")]
    LimitExceeded,
    #[error("requested or streamed capability is unsupported")]
    UnsupportedCapability,
    #[error("model provider reported a failure")]
    Provider,
}

pub fn encode_request(
    protocol: Protocol,
    request: &TextRequest,
    capabilities: &TextCapabilities,
) -> Result<Value, ProtocolError> {
    use ProtocolError::{LimitExceeded, Protocol as Invalid, UnsupportedCapability};
    if request.model.is_empty()
        || request.model.len() > 200
        || !request.model.bytes().all(|b| b.is_ascii_graphic())
        || request.messages.is_empty()
        || request.max_output_tokens == 0
    {
        return Err(Invalid);
    }
    if request.messages.len() > MAX_MESSAGES || request.max_output_tokens > MAX_OUTPUT_TOKENS {
        return Err(LimitExceeded);
    }
    let mut total = 0usize;
    let mut has_conversation = false;
    for message in &request.messages {
        if message.text.is_empty() {
            return Err(Invalid);
        }
        if message.text.len() > MAX_MESSAGE_BYTES {
            return Err(LimitExceeded);
        }
        total = total.checked_add(message.text.len()).ok_or(LimitExceeded)?;
        if total > MAX_REQUEST_BYTES {
            return Err(LimitExceeded);
        }
        if message.phase.is_some() {
            if message.role != TextRole::Assistant {
                return Err(Invalid);
            }
            if protocol != Protocol::Responses {
                return Err(UnsupportedCapability);
            }
        }
        if message.role != TextRole::System {
            has_conversation = true;
        } else if protocol == Protocol::AnthropicMessages && has_conversation {
            // Moving a later system turn to the beginning would change meaning.
            return Err(UnsupportedCapability);
        }
    }
    if !has_conversation {
        return Err(Invalid);
    }
    if let Some(effort) = &request.effort
        && (effort.is_empty()
            || effort.len() > 32
            || !effort
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            || !capabilities.efforts.contains(effort)
            || protocol == Protocol::ChatCompletions)
    {
        return Err(UnsupportedCapability);
    }
    let role = |role| match role {
        TextRole::System => "system",
        TextRole::User => "user",
        TextRole::Assistant => "assistant",
    };
    let messages: Vec<Value> = request
        .messages
        .iter()
        .map(|message| {
            let mut value = json!({"role": role(message.role), "content": message.text});
            if let Some(phase) = message.phase {
                value["phase"] = json!(match phase {
                    AssistantPhase::Commentary => "commentary",
                    AssistantPhase::FinalAnswer => "final_answer",
                });
            }
            value
        })
        .collect();
    let mut value = match protocol {
        Protocol::Responses => json!({
            "model": request.model, "input": messages,
            "stream": true, "store": false, "max_output_tokens": request.max_output_tokens,
        }),
        Protocol::AnthropicMessages => {
            let conversation: Vec<Value> = request.messages.iter()
                .filter(|message| message.role != TextRole::System)
                .map(|message| json!({"role":role(message.role), "content":[{"type":"text", "text":message.text}]}))
                .collect();
            let mut value = json!({"model":request.model, "messages":conversation,
                "stream":true, "max_tokens":request.max_output_tokens});
            let system = request
                .messages
                .iter()
                .filter(|message| message.role == TextRole::System)
                .map(|message| message.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            if !system.is_empty() {
                value["system"] = json!(system);
            }
            value
        }
        Protocol::OpenRouterChat | Protocol::ChatCompletions => {
            let mut value = json!({"model":request.model, "messages":messages,
                "stream":true, "n":1, "max_tokens":request.max_output_tokens});
            if protocol == Protocol::OpenRouterChat {
                value["provider"] = json!({"allow_fallbacks":false,"require_parameters":true});
                value["stream_options"] = json!({"include_usage":true});
            }
            value
        }
    };
    if let Some(effort) = &request.effort {
        match protocol {
            Protocol::Responses | Protocol::OpenRouterChat => {
                value["reasoning"] = json!({"effort":effort})
            }
            Protocol::AnthropicMessages => value["output_config"] = json!({"effort":effort}),
            Protocol::ChatCompletions => return Err(UnsupportedCapability),
        }
    }
    // Escaping can make the wire representation larger than the source strings.
    if serde_json::to_vec(&value).map_err(|_| Invalid)?.len() > MAX_REQUEST_BYTES {
        return Err(LimitExceeded);
    }
    Ok(value)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum BlockKind {
    Text,
    Refusal,
    Reasoning,
}
struct Part {
    kind: BlockKind,
    closed: bool,
    text_done: bool,
    text: String,
}
struct ResponseItem {
    index: u64,
    kind: BlockKind,
    phase: Option<AssistantPhase>,
    closed: bool,
    parts: BTreeMap<u64, Part>,
}

/// Feed arbitrary transport chunks, then call finish exactly at HTTP EOF.
/// Any error poisons this decoder: it cannot later manufacture a success event.
pub struct StreamDecoder {
    protocol: Protocol,
    tools: Option<tools::RoundState>,
    line: Vec<u8>,
    after_cr: bool,
    first_line: bool,
    event_name: String,
    data: String,
    has_data: bool,
    wire_bytes: usize,
    event_count: usize,
    text_bytes: usize,
    error: Option<ProtocolError>,
    started: bool,
    terminal: bool,
    response_id: Option<String>,
    model: Option<String>,
    refusal: bool,
    usage: Usage,
    stop: Option<(FinishOutcome, &'static str)>,
    response_items: BTreeMap<String, ResponseItem>,
    anthropic_blocks: BTreeMap<u64, Part>,
}

impl StreamDecoder {
    pub fn new(protocol: Protocol) -> Self {
        Self {
            protocol,
            tools: None,
            line: Vec::new(),
            after_cr: false,
            first_line: true,
            event_name: String::new(),
            data: String::new(),
            has_data: false,
            wire_bytes: 0,
            event_count: 0,
            text_bytes: 0,
            error: None,
            started: false,
            terminal: false,
            response_id: None,
            model: None,
            refusal: false,
            usage: Usage::default(),
            stop: None,
            response_items: BTreeMap::new(),
            anthropic_blocks: BTreeMap::new(),
        }
    }

    pub fn is_terminal(&self) -> bool {
        self.terminal && self.error.is_none()
    }

    pub fn push(&mut self, bytes: &[u8]) -> DecodeBatch {
        if let Some(error) = self.error {
            return DecodeBatch {
                events: Vec::new(),
                error: Some(error),
            };
        }
        let mut events = Vec::new();
        let result = self.push_inner(bytes, &mut events);
        if let Err(error) = result {
            self.error = Some(error);
            events.retain(|event| !matches!(event, ModelEvent::Finished { .. }));
        }
        DecodeBatch {
            events,
            error: result.err(),
        }
    }

    pub fn finish(&mut self) -> Result<(), ProtocolError> {
        if let Some(error) = self.error {
            return Err(error);
        }
        if !self.terminal || !self.line.is_empty() || self.has_data || !self.event_name.is_empty() {
            self.error = Some(ProtocolError::Truncated);
            return Err(ProtocolError::Truncated);
        }
        Ok(())
    }

    fn push_inner(
        &mut self,
        bytes: &[u8],
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        for &byte in bytes {
            self.wire_bytes = self
                .wire_bytes
                .checked_add(1)
                .ok_or(ProtocolError::LimitExceeded)?;
            if self.wire_bytes > MAX_STREAM_BYTES {
                return Err(ProtocolError::LimitExceeded);
            }
            if self.after_cr {
                self.after_cr = false;
                if byte == b'\n' {
                    continue;
                }
            }
            match byte {
                b'\r' | b'\n' => {
                    let line = std::mem::take(&mut self.line);
                    let line = std::str::from_utf8(&line).map_err(|_| ProtocolError::Protocol)?;
                    self.consume_line(line, events)?;
                    self.after_cr = byte == b'\r';
                }
                _ => {
                    if self.line.len() >= MAX_LINE_BYTES {
                        return Err(ProtocolError::LimitExceeded);
                    }
                    self.line.push(byte);
                }
            }
        }
        Ok(())
    }

    fn consume_line(
        &mut self,
        line: &str,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        let line = if std::mem::take(&mut self.first_line) {
            line.strip_prefix('\u{feff}').unwrap_or(line)
        } else {
            line
        };
        if line.is_empty() {
            let name = std::mem::take(&mut self.event_name);
            let data = std::mem::take(&mut self.data);
            if !std::mem::take(&mut self.has_data) {
                return Ok(());
            }
            self.event_count += 1;
            if self.event_count > MAX_EVENTS {
                return Err(ProtocolError::LimitExceeded);
            }
            let prefix = events.len();
            let result = self.dispatch(&name, &data, events);
            if result.is_err() {
                events.truncate(prefix);
            }
            return result;
        }
        if line.starts_with(':') {
            return Ok(());
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => {
                if value.len() > 128 {
                    return Err(ProtocolError::LimitExceeded);
                }
                self.event_name = value.to_owned();
            }
            "data" => {
                let additional = value.len() + usize::from(self.has_data);
                if self.data.len().saturating_add(additional) > MAX_EVENT_BYTES {
                    return Err(ProtocolError::LimitExceeded);
                }
                if self.has_data {
                    self.data.push('\n');
                }
                self.data.push_str(value);
                self.has_data = true;
            }
            _ => {} // SSE id/retry/unknown fields do not change model semantics.
        }
        Ok(())
    }

    fn dispatch(
        &mut self,
        name: &str,
        data: &str,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        if self.terminal {
            return Err(ProtocolError::Protocol);
        }
        if name == "error" {
            return Err(ProtocolError::Provider);
        }
        if data == "[DONE]" {
            if !matches!(
                self.protocol,
                Protocol::OpenRouterChat | Protocol::ChatCompletions
            ) || !self.started
            {
                return Err(ProtocolError::Protocol);
            }
            let (outcome, reason) = self.stop.ok_or(ProtocolError::Protocol)?;
            self.complete(outcome, reason, events);
            return Ok(());
        }
        let value: Value = serde_json::from_str(data).map_err(|_| ProtocolError::Protocol)?;
        if !value.is_object() {
            return Err(ProtocolError::Protocol);
        }
        if value.get("error").is_some_and(|value| !value.is_null()) {
            return Err(ProtocolError::Provider);
        }
        if let Some(mut state) = self.tools.take() {
            let result = state.dispatch(self, name, &value, events);
            self.tools = Some(state);
            return result;
        }
        match self.protocol {
            Protocol::Responses => self.responses(name, &value, events),
            Protocol::AnthropicMessages => self.anthropic(name, &value, events),
            Protocol::OpenRouterChat | Protocol::ChatCompletions => self.chat(&value, events),
        }
    }

    fn start(
        &mut self,
        id: Option<String>,
        model: Option<String>,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        if self.started {
            return Err(ProtocolError::Protocol);
        }
        self.started = true;
        self.response_id = id.clone();
        self.model = model.clone();
        events.push(ModelEvent::Started {
            response_id: id,
            model,
        });
        Ok(())
    }

    fn text(
        &mut self,
        text: &str,
        refusal: bool,
        item_id: Option<String>,
        phase: Option<AssistantPhase>,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        self.text_bytes = self
            .text_bytes
            .checked_add(text.len())
            .ok_or(ProtocolError::LimitExceeded)?;
        if self.text_bytes > MAX_TEXT_BYTES {
            return Err(ProtocolError::LimitExceeded);
        }
        if !text.is_empty() {
            if refusal {
                self.refusal = true;
            }
            events.push(if refusal {
                ModelEvent::RefusalDelta {
                    text: text.to_owned(),
                    item_id,
                    phase,
                }
            } else {
                ModelEvent::TextDelta {
                    text: text.to_owned(),
                    item_id,
                    phase,
                }
            });
        }
        Ok(())
    }

    fn record_usage(
        &mut self,
        next: Usage,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        fn merge(old: &mut Option<u64>, next: Option<u64>) -> Result<(), ProtocolError> {
            if let Some(next) = next {
                if old.is_some_and(|old| next < old) {
                    return Err(ProtocolError::Protocol);
                }
                *old = Some(next);
            }
            Ok(())
        }
        merge(&mut self.usage.input_tokens, next.input_tokens)?;
        merge(&mut self.usage.output_tokens, next.output_tokens)?;
        merge(&mut self.usage.cache_read_tokens, next.cache_read_tokens)?;
        merge(&mut self.usage.cache_write_tokens, next.cache_write_tokens)?;
        events.push(ModelEvent::UsageSnapshot(self.usage));
        Ok(())
    }

    fn complete(
        &mut self,
        mut outcome: FinishOutcome,
        reason: &'static str,
        events: &mut Vec<ModelEvent>,
    ) {
        if outcome == FinishOutcome::Completed && self.refusal {
            outcome = FinishOutcome::Refused;
        }
        self.terminal = true;
        events.push(ModelEvent::Finished {
            outcome,
            native_reason: reason.to_owned(),
        });
    }

    fn responses(
        &mut self,
        name: &str,
        value: &Value,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        let kind = event_type(name, value)?;
        if kind == "error" || kind == "response.failed" {
            return Err(ProtocolError::Provider);
        }
        if unsupported_event(kind) {
            return Err(ProtocolError::UnsupportedCapability);
        }
        if kind == "response.created" {
            let response = object(value, "response")?;
            return self.start(
                Some(required_id(response, "id")?),
                optional_id(response, "model")?,
                events,
            );
        }
        match kind {
            "response.output_item.added" => {
                self.require_started()?;
                if self.response_items.len() >= MAX_ITEMS {
                    return Err(ProtocolError::LimitExceeded);
                }
                let item = object(value, "item")?;
                let id = required_id(item, "id")?;
                let index = index(value, "output_index")?;
                if self.response_items.contains_key(&id)
                    || self.response_items.values().any(|item| item.index == index)
                {
                    return Err(ProtocolError::Protocol);
                }
                let item_kind = response_item_kind(item)?;
                let phase = phase(item)?;
                if item_kind == BlockKind::Text {
                    let content = item
                        .get("content")
                        .and_then(Value::as_array)
                        .ok_or(ProtocolError::Protocol)?;
                    for part in content {
                        response_part_kind(part)?;
                    }
                    if !content.is_empty() {
                        return Err(ProtocolError::Protocol);
                    }
                }
                if item_kind == BlockKind::Text {
                    events.push(ModelEvent::MessageMetadata {
                        item_id: id.clone(),
                        phase,
                    });
                }
                self.response_items.insert(
                    id,
                    ResponseItem {
                        index,
                        kind: item_kind,
                        phase,
                        closed: false,
                        parts: BTreeMap::new(),
                    },
                );
            }
            "response.content_part.added" => {
                self.require_started()?;
                let part = object(value, "part")?;
                let kind = response_part_kind(part)?;
                let text = required_text(part, part_text_key(kind))?;
                let content_index = index(value, "content_index")?;
                let item = self.response_item(value)?;
                if item.kind == BlockKind::Reasoning
                    || item.closed
                    || item.parts.contains_key(&content_index)
                    || content_index != item.parts.len() as u64
                {
                    return Err(ProtocolError::Protocol);
                }
                if item.parts.len() >= MAX_PARTS {
                    return Err(ProtocolError::LimitExceeded);
                }
                item.parts.insert(
                    content_index,
                    Part {
                        kind,
                        closed: false,
                        text_done: false,
                        text: text.to_owned(),
                    },
                );
                let phase = item.phase;
                if kind == BlockKind::Refusal {
                    self.refusal = true;
                }
                self.text(
                    text,
                    kind == BlockKind::Refusal,
                    Some(required_id(value, "item_id")?),
                    phase,
                    events,
                )?;
            }
            "response.output_text.delta" | "response.refusal.delta" => {
                self.require_started()?;
                let content_index = index(value, "content_index")?;
                let item = self.response_item(value)?;
                if item.closed {
                    return Err(ProtocolError::Protocol);
                }
                let part = item
                    .parts
                    .get_mut(&content_index)
                    .ok_or(ProtocolError::Protocol)?;
                let refusal = kind == "response.refusal.delta";
                if part.closed
                    || part.text_done
                    || part.kind
                        != if refusal {
                            BlockKind::Refusal
                        } else {
                            BlockKind::Text
                        }
                {
                    return Err(ProtocolError::Protocol);
                }
                let delta = required_text(value, "delta")?;
                if part.text.len().saturating_add(delta.len()) > MAX_TEXT_BYTES {
                    return Err(ProtocolError::LimitExceeded);
                }
                part.text.push_str(delta);
                let phase = item.phase;
                self.text(
                    delta,
                    refusal,
                    Some(required_id(value, "item_id")?),
                    phase,
                    events,
                )?;
            }
            "response.output_text.done" | "response.refusal.done" => {
                self.require_started()?;
                let content_index = index(value, "content_index")?;
                let item = self.response_item(value)?;
                if item.closed {
                    return Err(ProtocolError::Protocol);
                }
                let part = item
                    .parts
                    .get_mut(&content_index)
                    .ok_or(ProtocolError::Protocol)?;
                let expected = if kind == "response.refusal.done" {
                    BlockKind::Refusal
                } else {
                    BlockKind::Text
                };
                if part.kind != expected
                    || part.closed
                    || part.text_done
                    || required_text(value, part_text_key(expected))? != part.text
                {
                    return Err(ProtocolError::Protocol);
                }
                part.text_done = true;
            }
            "response.content_part.done" => {
                self.require_started()?;
                let snapshot = object(value, "part")?;
                let part_kind = response_part_kind(snapshot)?;
                let content_index = index(value, "content_index")?;
                let item = self.response_item(value)?;
                if item.closed {
                    return Err(ProtocolError::Protocol);
                }
                let part = item
                    .parts
                    .get_mut(&content_index)
                    .ok_or(ProtocolError::Protocol)?;
                if part.closed
                    || !part.text_done
                    || part.kind != part_kind
                    || required_text(snapshot, part_text_key(part_kind))? != part.text
                {
                    return Err(ProtocolError::Protocol);
                }
                part.closed = true;
            }
            "response.output_item.done" => {
                self.require_started()?;
                let final_item = object(value, "item")?;
                let id = required_id(final_item, "id")?;
                let final_kind = response_item_kind(final_item)?;
                let final_phase = phase(final_item)?;
                let item = self
                    .response_items
                    .get_mut(&id)
                    .ok_or(ProtocolError::Protocol)?;
                if item.closed
                    || item.index != index(value, "output_index")?
                    || item.kind != final_kind
                    || item.parts.values().any(|part| !part.closed)
                {
                    return Err(ProtocolError::Protocol);
                }
                validate_item_snapshot(item, final_item)?;
                let previous_phase = item.phase;
                merge_phase(&mut item.phase, final_phase)?;
                if item.kind == BlockKind::Text && item.phase != previous_phase {
                    events.push(ModelEvent::MessageMetadata {
                        item_id: id.clone(),
                        phase: item.phase,
                    });
                }
                item.closed = true;
            }
            "response.completed" | "response.incomplete" => {
                self.require_started()?;
                let response = object(value, "response")?;
                if nonnull(response, "error").is_some() {
                    return Err(ProtocolError::Provider);
                }
                if self.response_id.as_deref() != Some(required_text(response, "id")?)
                    || self.response_items.values().any(|item| !item.closed)
                {
                    return Err(ProtocolError::Protocol);
                }
                {
                    let output = response
                        .get("output")
                        .and_then(Value::as_array)
                        .ok_or(ProtocolError::Protocol)?;
                    if output.len() != self.response_items.len() {
                        return Err(ProtocolError::Protocol);
                    }
                    for (output_index, item) in output.iter().enumerate() {
                        let id = required_id(item, "id")?;
                        let known = self
                            .response_items
                            .get_mut(&id)
                            .ok_or(ProtocolError::Protocol)?;
                        if known.index != output_index as u64 {
                            return Err(ProtocolError::Protocol);
                        }
                        validate_item_snapshot(known, item)?;
                        let previous_phase = known.phase;
                        merge_phase(&mut known.phase, phase(item)?)?;
                        if known.kind == BlockKind::Text && known.phase != previous_phase {
                            events.push(ModelEvent::MessageMetadata {
                                item_id: id,
                                phase: known.phase,
                            });
                        }
                    }
                }
                if let Some(usage) = nonnull(response, "usage") {
                    self.record_usage(response_usage(usage)?, events)?;
                }
                let (outcome, reason) = if kind == "response.completed" {
                    if response
                        .get("status")
                        .is_some_and(|status| status != "completed")
                    {
                        return Err(ProtocolError::Protocol);
                    }
                    (FinishOutcome::Completed, "completed")
                } else {
                    if response
                        .get("status")
                        .is_some_and(|status| status != "incomplete")
                    {
                        return Err(ProtocolError::Protocol);
                    }
                    let reason = object(response, "incomplete_details")?
                        .get("reason")
                        .and_then(Value::as_str)
                        .ok_or(ProtocolError::Protocol)?;
                    match reason {
                        "max_output_tokens" => (FinishOutcome::Incomplete, "max_output_tokens"),
                        "content_filter" => (FinishOutcome::Refused, "content_filter"),
                        _ => return Err(ProtocolError::Protocol),
                    }
                };
                self.complete(outcome, reason, events);
            }
            _ => {} // Includes reasoning summaries: never answer text.
        }
        Ok(())
    }

    fn require_started(&self) -> Result<(), ProtocolError> {
        if self.started {
            Ok(())
        } else {
            Err(ProtocolError::Protocol)
        }
    }

    fn response_item(&mut self, value: &Value) -> Result<&mut ResponseItem, ProtocolError> {
        let id = required_id(value, "item_id")?;
        let output_index = index(value, "output_index")?;
        let item = self
            .response_items
            .get_mut(&id)
            .ok_or(ProtocolError::Protocol)?;
        if item.index != output_index {
            return Err(ProtocolError::Protocol);
        }
        Ok(item)
    }

    fn anthropic(
        &mut self,
        name: &str,
        value: &Value,
        events: &mut Vec<ModelEvent>,
    ) -> Result<(), ProtocolError> {
        let kind = event_type(name, value)?;
        if unsupported_event(kind) {
            return Err(ProtocolError::UnsupportedCapability);
        }
        match kind {
            "error" => return Err(ProtocolError::Provider),
            "message_start" => {
                let message = object(value, "message")?;
                if message.get("role").is_some_and(|role| role != "assistant") {
                    return Err(ProtocolError::Protocol);
                }
                if message.get("content").is_some_and(|content| {
                    content.as_array().is_none_or(|content| !content.is_empty())
                }) {
                    return Err(ProtocolError::Protocol);
                }
                self.start(
                    Some(required_id(message, "id")?),
                    optional_id(message, "model")?,
                    events,
                )?;
                if let Some(usage) = nonnull(message, "usage") {
                    self.record_usage(anthropic_usage(usage)?, events)?;
                }
            }
            "content_block_start" => {
                self.require_started()?;
                if self.stop.is_some() {
                    return Err(ProtocolError::Protocol);
                }
                if self.anthropic_blocks.len() >= MAX_ITEMS {
                    return Err(ProtocolError::LimitExceeded);
                }
                let block_index = index(value, "index")?;
                if block_index != self.anthropic_blocks.len() as u64 {
                    return Err(ProtocolError::Protocol);
                }
                let block = object(value, "content_block")?;
                let kind = match required_text(block, "type")? {
                    "text" => BlockKind::Text,
                    "thinking" | "redacted_thinking" => BlockKind::Reasoning,
                    _ => return Err(ProtocolError::UnsupportedCapability),
                };
                self.anthropic_blocks.insert(
                    block_index,
                    Part {
                        kind,
                        closed: false,
                        text_done: false,
                        text: String::new(),
                    },
                );
                if kind == BlockKind::Text {
                    self.text(
                        required_text(block, "text")?,
                        false,
                        self.response_id.clone(),
                        None,
                        events,
                    )?;
                }
            }
            "content_block_delta" => {
                self.require_started()?;
                if self.stop.is_some() {
                    return Err(ProtocolError::Protocol);
                }
                let block = self
                    .anthropic_blocks
                    .get(&index(value, "index")?)
                    .ok_or(ProtocolError::Protocol)?;
                if block.closed {
                    return Err(ProtocolError::Protocol);
                }
                let delta = object(value, "delta")?;
                match required_text(delta, "type")? {
                    "text_delta" if block.kind == BlockKind::Text => self.text(
                        required_text(delta, "text")?,
                        false,
                        self.response_id.clone(),
                        None,
                        events,
                    )?,
                    "thinking_delta" | "signature_delta" if block.kind == BlockKind::Reasoning => {}
                    "text_delta" | "thinking_delta" | "signature_delta" => {
                        return Err(ProtocolError::Protocol);
                    }
                    _ => return Err(ProtocolError::UnsupportedCapability),
                }
            }
            "content_block_stop" => {
                self.require_started()?;
                let block = self
                    .anthropic_blocks
                    .get_mut(&index(value, "index")?)
                    .ok_or(ProtocolError::Protocol)?;
                if block.closed || self.stop.is_some() {
                    return Err(ProtocolError::Protocol);
                }
                block.closed = true;
            }
            "message_delta" => {
                self.require_started()?;
                if self.anthropic_blocks.values().any(|block| !block.closed) {
                    return Err(ProtocolError::Protocol);
                }
                let delta = object(value, "delta")?;
                if let Some(reason) = nonnull(delta, "stop_reason") {
                    let stop = anthropic_stop(reason.as_str().ok_or(ProtocolError::Protocol)?)?;
                    if self.stop.is_some_and(|previous| previous != stop) {
                        return Err(ProtocolError::Protocol);
                    }
                    self.stop = Some(stop);
                }
                if let Some(usage) = nonnull(value, "usage") {
                    self.record_usage(anthropic_usage(usage)?, events)?;
                }
            }
            "message_stop" => {
                self.require_started()?;
                if self.anthropic_blocks.values().any(|block| !block.closed) {
                    return Err(ProtocolError::Protocol);
                }
                let (outcome, reason) = self.stop.ok_or(ProtocolError::Protocol)?;
                self.complete(outcome, reason, events);
            }
            _ => {} // ping and future informational events carry no answer text.
        }
        Ok(())
    }

    fn chat(&mut self, value: &Value, events: &mut Vec<ModelEvent>) -> Result<(), ProtocolError> {
        let id = optional_id(value, "id")?;
        let model = optional_id(value, "model")?;
        if !self.started {
            self.start(id, model, events)?;
        } else if id
            .as_ref()
            .zip(self.response_id.as_ref())
            .is_some_and(|(a, b)| a != b)
            || model
                .as_ref()
                .zip(self.model.as_ref())
                .is_some_and(|(a, b)| a != b)
        {
            return Err(ProtocolError::Protocol);
        }
        let choices = value
            .get("choices")
            .and_then(Value::as_array)
            .ok_or(ProtocolError::Protocol)?;
        if choices.len() > 1 {
            return Err(ProtocolError::UnsupportedCapability);
        }
        let usage = nonnull(value, "usage");
        if let Some(choice) = choices.first() {
            if index(choice, "index")? != 0 {
                return Err(ProtocolError::UnsupportedCapability);
            }
            let delta = object(choice, "delta")?;
            for field in ["tool_calls", "function_call", "audio", "images"] {
                if nonnull(delta, field)
                    .is_some_and(|value| !value.as_array().is_some_and(Vec::is_empty))
                {
                    return Err(ProtocolError::UnsupportedCapability);
                }
            }
            if nonnull(delta, "role").is_some_and(|role| role != "assistant") {
                return Err(ProtocolError::Protocol);
            }
            let text = optional_text(delta, "content")?.unwrap_or("");
            let refusal = optional_text(delta, "refusal")?.unwrap_or("");
            let stop = nonnull(choice, "finish_reason")
                .map(|reason| chat_stop(reason.as_str().ok_or(ProtocolError::Protocol)?))
                .transpose()?;
            if let Some(previous) = self.stop {
                // OpenRouter legitimately repeats finish_reason in its final,
                // content-free usage frame. It is still just one terminal turn.
                let new_reasoning = ["reasoning", "reasoning_content", "reasoning_details"]
                    .iter()
                    .any(|field| nonnull(delta, field).is_some_and(nonempty));
                if !text.is_empty()
                    || !refusal.is_empty()
                    || new_reasoning
                    || usage.is_none()
                    || stop.is_some_and(|stop| stop != previous)
                {
                    return Err(ProtocolError::Protocol);
                }
            } else {
                self.text(text, false, self.response_id.clone(), None, events)?;
                self.text(refusal, true, self.response_id.clone(), None, events)?;
                if let Some(stop) = stop {
                    self.stop = Some(stop);
                }
            }
        } else if usage.is_none() && self.stop.is_some() {
            return Err(ProtocolError::Protocol);
        }
        if let Some(usage) = usage {
            self.record_usage(chat_usage(usage)?, events)?;
        }
        Ok(())
    }
}

fn object<'a>(value: &'a Value, key: &str) -> Result<&'a Value, ProtocolError> {
    value
        .get(key)
        .filter(|value| value.is_object())
        .ok_or(ProtocolError::Protocol)
}
fn required_text<'a>(value: &'a Value, key: &str) -> Result<&'a str, ProtocolError> {
    value
        .get(key)
        .and_then(Value::as_str)
        .ok_or(ProtocolError::Protocol)
}
fn optional_text<'a>(value: &'a Value, key: &str) -> Result<Option<&'a str>, ProtocolError> {
    nonnull(value, key)
        .map(|value| value.as_str().ok_or(ProtocolError::UnsupportedCapability))
        .transpose()
}
fn nonnull<'a>(value: &'a Value, key: &str) -> Option<&'a Value> {
    value.get(key).filter(|value| !value.is_null())
}
fn nonempty(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
        _ => true,
    }
}
fn required_id(value: &Value, key: &str) -> Result<String, ProtocolError> {
    let id = required_text(value, key)?;
    if id.is_empty() || id.len() > 256 || !id.bytes().all(|b| b.is_ascii_graphic()) {
        return Err(ProtocolError::Protocol);
    }
    Ok(id.to_owned())
}
fn optional_id(value: &Value, key: &str) -> Result<Option<String>, ProtocolError> {
    nonnull(value, key)
        .map(|_| required_id(value, key))
        .transpose()
}
fn index(value: &Value, key: &str) -> Result<u64, ProtocolError> {
    value
        .get(key)
        .and_then(Value::as_u64)
        .filter(|index| *index < MAX_ITEMS as u64)
        .ok_or(ProtocolError::Protocol)
}
fn phase(value: &Value) -> Result<Option<AssistantPhase>, ProtocolError> {
    match nonnull(value, "phase").map(Value::as_str) {
        None => Ok(None),
        Some(Some("commentary")) => Ok(Some(AssistantPhase::Commentary)),
        Some(Some("final_answer")) => Ok(Some(AssistantPhase::FinalAnswer)),
        _ => Err(ProtocolError::UnsupportedCapability),
    }
}
fn merge_phase(
    old: &mut Option<AssistantPhase>,
    next: Option<AssistantPhase>,
) -> Result<(), ProtocolError> {
    if old.is_some() && next.is_some() && *old != next {
        return Err(ProtocolError::Protocol);
    }
    if next.is_some() {
        *old = next;
    }
    Ok(())
}
fn event_type<'a>(name: &str, value: &'a Value) -> Result<&'a str, ProtocolError> {
    let kind = required_text(value, "type")?;
    if !name.is_empty() && name != "message" && name != kind {
        return Err(ProtocolError::Protocol);
    }
    Ok(kind)
}
fn unsupported_event(kind: &str) -> bool {
    [
        "audio",
        "image",
        "function_call",
        "tool",
        "computer",
        "web_search",
        "file_search",
        "code_interpreter",
    ]
    .iter()
    .any(|part| kind.contains(part))
}
fn response_item_kind(item: &Value) -> Result<BlockKind, ProtocolError> {
    match required_text(item, "type")? {
        "message" if required_text(item, "role")? == "assistant" => Ok(BlockKind::Text),
        "reasoning" => Ok(BlockKind::Reasoning),
        _ => Err(ProtocolError::UnsupportedCapability),
    }
}
fn response_part_kind(part: &Value) -> Result<BlockKind, ProtocolError> {
    match required_text(part, "type")? {
        "output_text" => Ok(BlockKind::Text),
        "refusal" => Ok(BlockKind::Refusal),
        _ => Err(ProtocolError::UnsupportedCapability),
    }
}
fn part_text_key(kind: BlockKind) -> &'static str {
    if kind == BlockKind::Refusal {
        "refusal"
    } else {
        "text"
    }
}
fn validate_item_snapshot(known: &ResponseItem, item: &Value) -> Result<(), ProtocolError> {
    if response_item_kind(item)? != known.kind {
        return Err(ProtocolError::Protocol);
    }
    if known.kind == BlockKind::Reasoning {
        if nonnull(item, "content").is_some_and(nonempty) {
            return Err(ProtocolError::UnsupportedCapability);
        }
        return Ok(());
    }
    let content = item
        .get("content")
        .and_then(Value::as_array)
        .ok_or(ProtocolError::Protocol)?;
    if content.len() != known.parts.len() {
        return Err(ProtocolError::Protocol);
    }
    for (index, snapshot) in content.iter().enumerate() {
        let kind = response_part_kind(snapshot)?;
        let part = known
            .parts
            .get(&(index as u64))
            .ok_or(ProtocolError::Protocol)?;
        if !part.closed
            || !part.text_done
            || part.kind != kind
            || required_text(snapshot, part_text_key(kind))? != part.text
        {
            return Err(ProtocolError::Protocol);
        }
    }
    Ok(())
}
fn count(value: &Value, key: &str) -> Result<Option<u64>, ProtocolError> {
    nonnull(value, key)
        .map(|value| {
            value
                .as_u64()
                .filter(|count| *count <= MAX_TOKEN_COUNT)
                .ok_or(ProtocolError::Protocol)
        })
        .transpose()
}
fn details_count(value: &Value, details: &str, key: &str) -> Result<Option<u64>, ProtocolError> {
    match nonnull(value, details) {
        Some(value) if value.is_object() => count(value, key),
        Some(_) => Err(ProtocolError::Protocol),
        None => Ok(None),
    }
}
fn response_usage(value: &Value) -> Result<Usage, ProtocolError> {
    if !value.is_object() {
        return Err(ProtocolError::Protocol);
    }
    Ok(Usage {
        input_tokens: count(value, "input_tokens")?,
        output_tokens: count(value, "output_tokens")?,
        cache_read_tokens: details_count(value, "input_tokens_details", "cached_tokens")?,
        cache_write_tokens: None,
    })
}
fn anthropic_usage(value: &Value) -> Result<Usage, ProtocolError> {
    if !value.is_object() {
        return Err(ProtocolError::Protocol);
    }
    Ok(Usage {
        input_tokens: count(value, "input_tokens")?,
        output_tokens: count(value, "output_tokens")?,
        cache_read_tokens: count(value, "cache_read_input_tokens")?,
        cache_write_tokens: count(value, "cache_creation_input_tokens")?,
    })
}
fn chat_usage(value: &Value) -> Result<Usage, ProtocolError> {
    if !value.is_object() {
        return Err(ProtocolError::Protocol);
    }
    Ok(Usage {
        input_tokens: count(value, "prompt_tokens")?,
        output_tokens: count(value, "completion_tokens")?,
        cache_read_tokens: details_count(value, "prompt_tokens_details", "cached_tokens")?,
        cache_write_tokens: details_count(value, "prompt_tokens_details", "cache_write_tokens")?,
    })
}
fn anthropic_stop(reason: &str) -> Result<(FinishOutcome, &'static str), ProtocolError> {
    use FinishOutcome::*;
    Ok(match reason {
        "end_turn" => (Completed, "end_turn"),
        "stop_sequence" => (Completed, "stop_sequence"),
        "max_tokens" => (Incomplete, "max_tokens"),
        "model_context_window_exceeded" => (Incomplete, "model_context_window_exceeded"),
        "refusal" => (Refused, "refusal"),
        "tool_use" => (ContinuationRequired, "tool_use"),
        "pause_turn" => (ContinuationRequired, "pause_turn"),
        _ => return Err(ProtocolError::Protocol),
    })
}
fn chat_stop(reason: &str) -> Result<(FinishOutcome, &'static str), ProtocolError> {
    use FinishOutcome::*;
    Ok(match reason {
        "stop" => (Completed, "stop"),
        "length" => (Incomplete, "length"),
        "content_filter" => (Refused, "content_filter"),
        "tool_calls" => (ContinuationRequired, "tool_calls"),
        "function_call" => (ContinuationRequired, "function_call"),
        "error" => return Err(ProtocolError::Provider),
        _ => return Err(ProtocolError::Protocol),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> TextRequest {
        TextRequest {
            model: "synthetic-model".into(),
            messages: vec![
                TextMessage {
                    role: TextRole::System,
                    text: "Synthetic instructions".into(),
                    phase: None,
                },
                TextMessage {
                    role: TextRole::User,
                    text: "Synthetic question".into(),
                    phase: None,
                },
            ],
            max_output_tokens: 128,
            effort: None,
        }
    }
    fn frame(value: Value) -> String {
        format!("data: {}\n\n", value)
    }
    fn wire(frames: Vec<Value>) -> String {
        frames.into_iter().map(frame).collect()
    }
    fn push_ok(decoder: &mut StreamDecoder, bytes: &[u8]) -> Vec<ModelEvent> {
        let batch = decoder.push(bytes);
        assert!(
            batch.error.is_none(),
            "synthetic stream unexpectedly failed"
        );
        batch.events
    }
    fn decode(protocol: Protocol, wire: &str, width: usize) -> Vec<ModelEvent> {
        let mut decoder = StreamDecoder::new(protocol);
        let mut events = Vec::new();
        for chunk in wire.as_bytes().chunks(width) {
            events.extend(push_ok(&mut decoder, chunk));
        }
        assert!(decoder.is_terminal());
        assert_eq!(decoder.finish(), Ok(()));
        events
    }
    fn text(events: &[ModelEvent]) -> String {
        events
            .iter()
            .filter_map(|event| match event {
                ModelEvent::TextDelta { text, .. } => Some(text.as_str()),
                _ => None,
            })
            .collect()
    }
    fn terminal(events: &[ModelEvent]) -> FinishOutcome {
        let terminal: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ModelEvent::Finished { outcome, .. } => Some(*outcome),
                _ => None,
            })
            .collect();
        assert_eq!(terminal.len(), 1);
        terminal[0]
    }
    fn last_usage(events: &[ModelEvent]) -> Usage {
        events
            .iter()
            .rev()
            .find_map(|event| match event {
                ModelEvent::UsageSnapshot(usage) => Some(*usage),
                _ => None,
            })
            .unwrap_or_default()
    }
    fn response_frames(phase: Option<&str>, refusal: bool) -> Vec<Value> {
        response_frames_with_text(phase, refusal, "안녕 🌱")
    }
    fn response_frames_with_text(phase: Option<&str>, refusal: bool, text: &str) -> Vec<Value> {
        let mut item = json!({"id":"item-1","type":"message","role":"assistant","status":"in_progress","content":[]});
        if let Some(phase) = phase {
            item["phase"] = json!(phase);
        }
        let key = if refusal { "refusal" } else { "text" };
        let part = if refusal {
            json!({"type":"refusal","refusal":""})
        } else {
            json!({"type":"output_text","text":"","annotations":[]})
        };
        let mut final_part = part.clone();
        final_part[key] = json!(text);
        let mut final_item = item.clone();
        final_item["content"] = json!([final_part.clone()]);
        final_item["status"] = json!("completed");
        let delta = if refusal {
            "response.refusal.delta"
        } else {
            "response.output_text.delta"
        };
        let done_type = if refusal {
            "response.refusal.done"
        } else {
            "response.output_text.done"
        };
        let mut done =
            json!({"type":done_type,"output_index":0,"item_id":"item-1","content_index":0});
        done[key] = json!(text);
        vec![
            json!({"type":"response.created","response":{"id":"response-1","model":"synthetic-model"}}),
            json!({"type":"response.output_item.added","output_index":0,"item":item}),
            json!({"type":"response.content_part.added","output_index":0,"item_id":"item-1","content_index":0,"part":part}),
            json!({"type":delta,"output_index":0,"item_id":"item-1","content_index":0,"delta":text}),
            done,
            json!({"type":"response.content_part.done","output_index":0,"item_id":"item-1","content_index":0,"part":final_part}),
            json!({"type":"response.output_item.done","output_index":0,"item":final_item.clone()}),
            json!({"type":"response.completed","response":{"id":"response-1","status":"completed","error":null,"output":[final_item],"usage":{"input_tokens":10,"output_tokens":3,"input_tokens_details":{"cached_tokens":0}}}}),
        ]
    }
    fn anthropic_frames(stop: &str) -> Vec<Value> {
        vec![
            json!({"type":"message_start","message":{"id":"message-1","model":"synthetic-model","role":"assistant","content":[],"usage":{"input_tokens":10,"output_tokens":1,"cache_read_input_tokens":2,"cache_creation_input_tokens":3}}}),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"synthetic reasoning, never answer text"}}),
            json!({"type":"content_block_delta","index":0,"delta":{"type":"signature_delta","signature":"synthetic-signature"}}),
            json!({"type":"content_block_stop","index":0}),
            json!({"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}),
            json!({"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":"Synthetic answer"}}),
            json!({"type":"content_block_stop","index":1}),
            json!({"type":"message_delta","delta":{"stop_reason":stop,"stop_sequence":null},"usage":{"output_tokens":8}}),
            json!({"type":"message_stop"}),
        ]
    }
    fn chat_chunk(delta: Value, stop: Value) -> Value {
        json!({"id":"chat-1","model":"synthetic-model","choices":[{"index":0,"delta":delta,"finish_reason":stop}]})
    }
    fn chat_wire(reason: &str) -> String {
        wire(vec![
            chat_chunk(json!({"role":"assistant","content":""}), Value::Null),
            chat_chunk(json!({"reasoning":"synthetic reasoning"}), Value::Null),
            chat_chunk(json!({"content":"Synthetic answer"}), Value::Null),
            chat_chunk(json!({}), json!(reason)),
        ]) + "data: [DONE]\n\n"
    }

    #[test]
    fn requests_are_explicit_text_streams_with_protocol_specific_effort() {
        let capabilities = TextCapabilities {
            efforts: vec!["high".into()],
        };
        for protocol in [
            Protocol::Responses,
            Protocol::AnthropicMessages,
            Protocol::OpenRouterChat,
        ] {
            let mut request = request();
            request.effort = Some("high".into());
            let encoded = encode_request(protocol, &request, &capabilities)
                .unwrap_or_else(|error| panic!("synthetic request failed: {error}"));
            assert!(encoded["stream"] == true);
            assert!(encoded["model"] == "synthetic-model");
            match protocol {
                Protocol::Responses => {
                    assert!(encoded["store"] == false && encoded["max_output_tokens"] == 128);
                    assert!(encoded["reasoning"]["effort"] == "high");
                    assert!(
                        encoded["input"]
                            .as_array()
                            .is_some_and(|messages| messages.len() == 2)
                    );
                }
                Protocol::AnthropicMessages => {
                    assert!(encoded["system"] == "Synthetic instructions");
                    assert!(
                        encoded["messages"]
                            .as_array()
                            .is_some_and(|messages| messages.len() == 1)
                    );
                    assert!(
                        encoded["output_config"]["effort"] == "high"
                            && encoded["max_tokens"] == 128
                    );
                }
                Protocol::OpenRouterChat => {
                    assert!(encoded["reasoning"]["effort"] == "high");
                    assert!(
                        encoded["provider"]["allow_fallbacks"] == false
                            && encoded["provider"]["require_parameters"] == true
                    );
                    assert!(encoded["provider"].get("only").is_none());
                }
                _ => unreachable!(),
            }
        }
        let encoded = encode_request(
            Protocol::ChatCompletions,
            &request(),
            &TextCapabilities::default(),
        )
        .unwrap_or_else(|error| panic!("synthetic request failed: {error}"));
        assert!(encoded["n"] == 1 && encoded.get("reasoning").is_none());
        assert!(encoded.get("stream_options").is_none());
    }

    #[test]
    fn phase_is_preserved_in_responses_history_and_never_dropped_for_other_protocols() {
        let mut request = request();
        request.messages.push(TextMessage {
            role: TextRole::Assistant,
            text: "Synthetic commentary".into(),
            phase: Some(AssistantPhase::Commentary),
        });
        request.messages.push(TextMessage {
            role: TextRole::Assistant,
            text: "Synthetic final".into(),
            phase: Some(AssistantPhase::FinalAnswer),
        });
        let encoded = encode_request(Protocol::Responses, &request, &TextCapabilities::default())
            .unwrap_or_else(|error| panic!("synthetic request failed: {error}"));
        assert!(encoded["input"][2]["phase"] == "commentary");
        assert!(encoded["input"][3]["phase"] == "final_answer");
        for protocol in [
            Protocol::AnthropicMessages,
            Protocol::OpenRouterChat,
            Protocol::ChatCompletions,
        ] {
            assert!(matches!(
                encode_request(protocol, &request, &TextCapabilities::default()),
                Err(ProtocolError::UnsupportedCapability)
            ));
        }
    }

    #[test]
    fn unknown_effort_missing_model_and_request_bounds_are_rejected_locally() {
        let mut request = request();
        request.effort = Some("high".into());
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &TextCapabilities::default()),
            Err(ProtocolError::UnsupportedCapability)
        ));
        assert!(matches!(
            encode_request(
                Protocol::ChatCompletions,
                &request,
                &TextCapabilities {
                    efforts: vec!["high".into()]
                }
            ),
            Err(ProtocolError::UnsupportedCapability)
        ));
        request.effort = None;
        request.model.clear();
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &TextCapabilities::default()),
            Err(ProtocolError::Protocol)
        ));
        request.model = "synthetic-model".into();
        request.max_output_tokens = 0;
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &TextCapabilities::default()),
            Err(ProtocolError::Protocol)
        ));
        request.max_output_tokens = 128;
        request.messages[1].text = "x".repeat(MAX_MESSAGE_BYTES + 1);
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &TextCapabilities::default()),
            Err(ProtocolError::LimitExceeded)
        ));
        request.messages[1].text.clear();
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &TextCapabilities::default()),
            Err(ProtocolError::Protocol)
        ));
    }

    #[test]
    fn interleaved_anthropic_system_turn_is_not_silently_reordered() {
        let mut request = request();
        request.messages.push(TextMessage {
            role: TextRole::System,
            text: "Later synthetic instruction".into(),
            phase: None,
        });
        assert!(matches!(
            encode_request(
                Protocol::AnthropicMessages,
                &request,
                &TextCapabilities::default()
            ),
            Err(ProtocolError::UnsupportedCapability)
        ));
    }

    #[test]
    fn request_count_total_wire_size_and_output_budget_have_independent_bounds() {
        let capabilities = TextCapabilities::default();
        let mut request = request();
        request.max_output_tokens = MAX_OUTPUT_TOKENS + 1;
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &capabilities),
            Err(ProtocolError::LimitExceeded)
        ));
        request.max_output_tokens = 128;
        request.messages = (0..MAX_MESSAGES + 1)
            .map(|_| TextMessage {
                role: TextRole::User,
                text: "x".into(),
                phase: None,
            })
            .collect();
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &capabilities),
            Err(ProtocolError::LimitExceeded)
        ));
        request.messages = (0..9)
            .map(|_| TextMessage {
                role: TextRole::User,
                text: "x".repeat(MAX_MESSAGE_BYTES),
                phase: None,
            })
            .collect();
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &capabilities),
            Err(ProtocolError::LimitExceeded)
        ));
        request.messages = (0..2)
            .map(|_| TextMessage {
                role: TextRole::User,
                text: "\0".repeat(MAX_MESSAGE_BYTES),
                phase: None,
            })
            .collect();
        assert!(matches!(
            encode_request(Protocol::Responses, &request, &capabilities),
            Err(ProtocolError::LimitExceeded)
        ));
    }

    #[test]
    fn optional_initial_utf8_bom_is_split_safely_without_losing_first_event() {
        let stream = format!("\u{feff}{}", chat_wire("stop"));
        let events = decode(Protocol::ChatCompletions, &stream, 1);
        assert!(text(&events) == "Synthetic answer");
        assert_eq!(terminal(&events), FinishOutcome::Completed);
    }

    #[test]
    fn responses_survive_every_two_chunk_boundary_including_unicode_and_crlf() {
        let stream = format!(
            ": synthetic keepalive\r\n\r\n{}",
            wire(response_frames(Some("final_answer"), false)).replace('\n', "\r\n")
        );
        for split in 0..=stream.len() {
            let mut decoder = StreamDecoder::new(Protocol::Responses);
            let mut events = push_ok(&mut decoder, &stream.as_bytes()[..split]);
            events.extend(push_ok(&mut decoder, &stream.as_bytes()[split..]));
            assert_eq!(decoder.finish(), Ok(()));
            assert!(text(&events) == "안녕 🌱");
            assert_eq!(terminal(&events), FinishOutcome::Completed);
            assert!(events.iter().any(|event| matches!(event, ModelEvent::TextDelta { item_id:Some(id), phase:Some(AssistantPhase::FinalAnswer), .. } if id == "item-1")));
            assert_eq!(last_usage(&events).cache_read_tokens, Some(0));
        }
    }

    #[test]
    fn bare_cr_sse_and_multiline_json_are_not_transport_boundaries() {
        let frames = anthropic_frames("end_turn");
        let mut stream = wire(frames);
        stream = stream.replacen("data: {", "event: message_start\ndata: {", 1);
        stream = stream.replacen(
            ",\"type\":\"message_start\"",
            ",\ndata: \"type\":\"message_start\"",
            1,
        );
        // Build a guaranteed multiline JSON event independently of object ordering.
        let first_end = stream
            .find("\n\n")
            .unwrap_or_else(|| panic!("missing synthetic frame"));
        let first = format!(
            "event: message_start\ndata: {{\"type\":\"message_start\",\ndata: \"message\":{}}}\n\n",
            anthropic_frames("end_turn")[0]["message"]
        );
        stream.replace_range(..first_end + 2, &first);
        let events = decode(Protocol::AnthropicMessages, &stream.replace('\n', "\r"), 1);
        assert!(text(&events) == "Synthetic answer");
        assert_eq!(terminal(&events), FinishOutcome::Completed);
    }

    #[test]
    fn responses_refusal_and_incomplete_are_not_success() {
        let events = decode(
            Protocol::Responses,
            &wire(response_frames(Some("final_answer"), true)),
            7,
        );
        assert!(text(&events).is_empty());
        assert_eq!(terminal(&events), FinishOutcome::Refused);
        let mut frames = response_frames(None, false);
        let last = frames
            .last_mut()
            .unwrap_or_else(|| panic!("missing synthetic frame"));
        let output = last["response"]["output"].clone();
        *last = json!({"type":"response.incomplete","response":{"id":"response-1","status":"incomplete","output":output,"incomplete_details":{"reason":"max_output_tokens"}}});
        let events = decode(Protocol::Responses, &wire(frames), 3);
        assert_eq!(terminal(&events), FinishOutcome::Incomplete);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelEvent::TextDelta { phase: None, .. }))
        );
    }

    #[test]
    fn responses_unknown_phase_or_unknown_item_cannot_become_final_answer() {
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        assert!(matches!(
            decoder
                .push(wire(response_frames(Some("future_phase"), false)).as_bytes())
                .error,
            Some(ProtocolError::UnsupportedCapability)
        ));
        let mut frames = response_frames(Some("commentary"), false);
        frames[3]["item_id"] = json!("different-item");
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        assert!(matches!(
            decoder.push(wire(frames).as_bytes()).error,
            Some(ProtocolError::Protocol)
        ));
        let mut frames = response_frames(Some("commentary"), false);
        frames[6]["item"]["phase"] = json!("final_answer");
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        assert!(matches!(
            decoder.push(wire(frames).as_bytes()).error,
            Some(ProtocolError::Protocol)
        ));
    }

    #[test]
    fn phase_learned_after_text_is_exposed_as_message_metadata_for_future_history() {
        let mut frames = response_frames(None, false);
        frames[6]["item"]["phase"] = json!("final_answer");
        frames[7]["response"]["output"][0]["phase"] = json!("final_answer");
        let events = decode(Protocol::Responses, &wire(frames), 1);
        assert!(
            events
                .iter()
                .any(|event| matches!(event, ModelEvent::TextDelta { phase: None, .. }))
        );
        let metadata: Vec<_> = events
            .iter()
            .filter_map(|event| match event {
                ModelEvent::MessageMetadata { item_id, phase } if item_id == "item-1" => {
                    Some(*phase)
                }
                _ => None,
            })
            .collect();
        assert_eq!(metadata, vec![None, Some(AssistantPhase::FinalAnswer)]);
        let mut request = request();
        request.messages.push(TextMessage {
            role: TextRole::Assistant,
            text: text(&events),
            phase: metadata.last().copied().flatten(),
        });
        let encoded = encode_request(Protocol::Responses, &request, &TextCapabilities::default())
            .unwrap_or_else(|error| panic!("synthetic history failed: {error}"));
        assert!(encoded["input"][2]["phase"] == "final_answer");
    }

    #[test]
    fn responses_delta_requires_created_item_and_open_content_lifecycles() {
        let frames = response_frames(None, false);
        for omitted in [0, 1, 2, 4, 5, 6] {
            let changed: Vec<_> = frames
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != omitted)
                .map(|(_, frame)| frame.clone())
                .collect();
            let mut decoder = StreamDecoder::new(Protocol::Responses);
            assert!(matches!(
                decoder.push(wire(changed).as_bytes()).error,
                Some(ProtocolError::Protocol)
            ));
        }
    }

    #[test]
    fn anthropic_usage_is_cumulative_not_summed_and_reasoning_is_not_text() {
        let events = decode(
            Protocol::AnthropicMessages,
            &wire(anthropic_frames("end_turn")),
            1,
        );
        assert!(text(&events) == "Synthetic answer");
        assert_eq!(
            last_usage(&events),
            Usage {
                input_tokens: Some(10),
                output_tokens: Some(8),
                cache_read_tokens: Some(2),
                cache_write_tokens: Some(3)
            }
        );
        assert_eq!(terminal(&events), FinishOutcome::Completed);
    }

    #[test]
    fn anthropic_stop_reasons_preserve_refusal_limit_and_continuation() {
        for (reason, expected) in [
            ("refusal", FinishOutcome::Refused),
            ("max_tokens", FinishOutcome::Incomplete),
            ("pause_turn", FinishOutcome::ContinuationRequired),
            ("tool_use", FinishOutcome::ContinuationRequired),
            ("model_context_window_exceeded", FinishOutcome::Incomplete),
        ] {
            let events = decode(
                Protocol::AnthropicMessages,
                &wire(anthropic_frames(reason)),
                11,
            );
            assert_eq!(terminal(&events), expected);
        }
    }

    #[test]
    fn anthropic_requires_block_closure_stop_reason_and_message_stop() {
        let frames = anthropic_frames("end_turn");
        for omitted in [0, 1, 4, 7, 8] {
            let changed = frames
                .iter()
                .enumerate()
                .filter(|(index, _)| *index != omitted)
                .map(|(_, frame)| frame.clone())
                .collect();
            let mut decoder = StreamDecoder::new(Protocol::AnthropicMessages);
            assert!(matches!(
                decoder.push(wire(changed).as_bytes()).error,
                Some(ProtocolError::Protocol)
            ));
        }
        let mut decoder = StreamDecoder::new(Protocol::AnthropicMessages);
        assert!(
            decoder
                .push(wire(frames[..frames.len() - 1].to_vec()).as_bytes())
                .error
                .is_none()
        );
        assert_eq!(decoder.finish(), Err(ProtocolError::Truncated));
    }

    #[test]
    fn openrouter_repeated_finish_reason_usage_frame_is_valid_but_only_one_terminal() {
        let mut final_usage = chat_chunk(json!({"content":""}), json!("stop"));
        final_usage["usage"] = json!({"prompt_tokens":10,"completion_tokens":3,"prompt_tokens_details":{"cached_tokens":0,"cache_write_tokens":2}});
        let stream = wire(vec![
            chat_chunk(json!({"content":"Synthetic answer"}), Value::Null),
            chat_chunk(json!({}), json!("stop")),
            final_usage,
        ]) + "data: [DONE]\n\n";
        let events = decode(Protocol::OpenRouterChat, &stream, 1);
        assert!(text(&events) == "Synthetic answer");
        assert_eq!(terminal(&events), FinishOutcome::Completed);
        assert_eq!(
            last_usage(&events),
            Usage {
                input_tokens: Some(10),
                output_tokens: Some(3),
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(2)
            }
        );
    }

    #[test]
    fn openrouter_empty_choices_usage_frame_and_unknown_usage_are_distinct() {
        let stream = wire(vec![
            chat_chunk(json!({"content":"Synthetic answer"}), json!("stop")),
            json!({"id":"chat-1","model":"synthetic-model","choices":[],"usage":{"prompt_tokens":0,"completion_tokens":0}}),
        ]) + "data: [DONE]\n\n";
        let events = decode(Protocol::OpenRouterChat, &stream, 5);
        assert_eq!(last_usage(&events).input_tokens, Some(0));
        let events = decode(Protocol::ChatCompletions, &chat_wire("stop"), 3);
        assert!(text(&events) == "Synthetic answer");
        assert_eq!(last_usage(&events), Usage::default());
        assert!(
            !events
                .iter()
                .any(|event| matches!(event, ModelEvent::UsageSnapshot(_)))
        );
    }

    #[test]
    fn chat_needs_finish_reason_then_done_and_rejects_new_content_after_finish() {
        let mut decoder = StreamDecoder::new(Protocol::OpenRouterChat);
        assert!(
            decoder
                .push(
                    wire(vec![chat_chunk(
                        json!({"content":"Synthetic answer"}),
                        json!("stop")
                    )])
                    .as_bytes()
                )
                .error
                .is_none()
        );
        assert!(!decoder.is_terminal());
        assert_eq!(decoder.finish(), Err(ProtocolError::Truncated));
        let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
        let stream = frame(chat_chunk(
            json!({"content":"Synthetic answer"}),
            Value::Null,
        )) + "data: [DONE]\n\n";
        assert!(matches!(
            decoder.push(stream.as_bytes()).error,
            Some(ProtocolError::Protocol)
        ));
        for delta in [
            json!({"content":"late text"}),
            json!({"refusal":"late refusal"}),
            json!({"reasoning":"late reasoning"}),
        ] {
            let mut decoder = StreamDecoder::new(Protocol::OpenRouterChat);
            let mut late = chat_chunk(delta, json!("stop"));
            late["usage"] = json!({"prompt_tokens":1});
            assert!(matches!(
                decoder
                    .push(wire(vec![chat_chunk(json!({}), json!("stop")), late]).as_bytes())
                    .error,
                Some(ProtocolError::Protocol)
            ));
        }
    }

    #[test]
    fn chat_preserves_non_success_reasons_and_does_not_emit_reasoning() {
        for protocol in [Protocol::ChatCompletions, Protocol::OpenRouterChat] {
            for (reason, expected) in [
                ("length", FinishOutcome::Incomplete),
                ("content_filter", FinishOutcome::Refused),
                ("tool_calls", FinishOutcome::ContinuationRequired),
            ] {
                let events = decode(protocol, &chat_wire(reason), 1);
                assert!(text(&events) == "Synthetic answer");
                assert_eq!(terminal(&events), expected);
            }
        }
    }

    #[test]
    fn tools_and_media_fail_explicitly_instead_of_becoming_answer_text() {
        for item_type in ["function_call", "image_generation_call", "computer_call"] {
            let frames = vec![
                response_frames(None, false)[0].clone(),
                json!({"type":"response.output_item.added","output_index":0,"item":{"id":"tool-1","type":item_type}}),
            ];
            let mut decoder = StreamDecoder::new(Protocol::Responses);
            assert!(matches!(
                decoder.push(wire(frames).as_bytes()).error,
                Some(ProtocolError::UnsupportedCapability)
            ));
        }
        let mut decoder = StreamDecoder::new(Protocol::AnthropicMessages);
        let frames = vec![
            anthropic_frames("end_turn")[0].clone(),
            json!({"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"tool-1","name":"synthetic-tool"}}),
        ];
        assert!(matches!(
            decoder.push(wire(frames).as_bytes()).error,
            Some(ProtocolError::UnsupportedCapability)
        ));
        for delta in [
            json!({"tool_calls":[{"id":"tool-1"}]}),
            json!({"content":[{"type":"image_url"}]}),
            json!({"audio":{"data":"synthetic"}}),
        ] {
            let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
            assert!(matches!(
                decoder
                    .push(frame(chat_chunk(delta, Value::Null)).as_bytes())
                    .error,
                Some(ProtocolError::UnsupportedCapability)
            ));
        }
    }

    #[test]
    fn provider_failure_after_partial_output_is_poisoned_and_never_finished() {
        for protocol in [
            Protocol::Responses,
            Protocol::AnthropicMessages,
            Protocol::OpenRouterChat,
            Protocol::ChatCompletions,
        ] {
            let mut decoder = StreamDecoder::new(protocol);
            let prefix = match protocol {
                Protocol::Responses => wire(response_frames(None, false)[..4].to_vec()),
                Protocol::AnthropicMessages => wire(anthropic_frames("end_turn")[..7].to_vec()),
                _ => frame(chat_chunk(
                    json!({"content":"Synthetic partial answer"}),
                    Value::Null,
                )),
            };
            let events = push_ok(&mut decoder, prefix.as_bytes());
            assert!(!text(&events).is_empty());
            assert!(
                !events
                    .iter()
                    .any(|event| matches!(event, ModelEvent::Finished { .. }))
            );
            let failure = frame(json!({"error":{"message":"synthetic private-shaped marker"}}));
            assert!(matches!(
                decoder.push(failure.as_bytes()).error,
                Some(ProtocolError::Provider)
            ));
            assert_eq!(decoder.finish(), Err(ProtocolError::Provider));
            assert!(!decoder.is_terminal());
            assert!(matches!(
                decoder.push(b"data: [DONE]\n\n").error,
                Some(ProtocolError::Provider)
            ));
        }
        assert!(ProtocolError::Provider.to_string() == "model provider reported a failure");
    }

    fn event_kinds(events: &[ModelEvent]) -> Vec<&'static str> {
        events
            .iter()
            .map(|event| match event {
                ModelEvent::Started { .. } => "started",
                ModelEvent::MessageMetadata { .. } => "metadata",
                ModelEvent::TextDelta { .. } => "text",
                ModelEvent::RefusalDelta { .. } => "refusal",
                ModelEvent::UsageSnapshot(_) => "usage",
                ModelEvent::Finished { .. } => "finished",
            })
            .collect()
    }

    #[test]
    fn coalesced_and_split_errors_keep_the_same_valid_text_usage_and_metadata_prefix() {
        for protocol in [
            Protocol::Responses,
            Protocol::AnthropicMessages,
            Protocol::OpenRouterChat,
            Protocol::ChatCompletions,
        ] {
            let prefix = match protocol {
                Protocol::Responses => {
                    wire(response_frames(Some("commentary"), false)[..4].to_vec())
                }
                Protocol::AnthropicMessages => wire(anthropic_frames("end_turn")[..7].to_vec()),
                _ => wire(vec![
                    chat_chunk(json!({"content":"Synthetic partial answer"}), Value::Null),
                    json!({"choices":[],"usage":{"prompt_tokens":10,"completion_tokens":2}}),
                ]),
            };
            let stream = prefix + &frame(json!({"error":{"message":"synthetic provider failure"}}));
            let mut coalesced = StreamDecoder::new(protocol);
            let batch = coalesced.push(stream.as_bytes());
            assert_eq!(batch.error, Some(ProtocolError::Provider));
            assert!(!text(&batch.events).is_empty());
            assert!(!event_kinds(&batch.events).contains(&"finished"));
            if protocol != Protocol::Responses {
                assert_eq!(last_usage(&batch.events).input_tokens, Some(10));
            }
            for width in [1, 7, 101] {
                let mut split = StreamDecoder::new(protocol);
                let mut events = Vec::new();
                let mut error = None;
                for chunk in stream.as_bytes().chunks(width) {
                    let batch = split.push(chunk);
                    events.extend(batch.events);
                    if batch.error.is_some() {
                        error = batch.error;
                        break;
                    }
                }
                assert_eq!(error, Some(ProtocolError::Provider));
                assert!(text(&events) == text(&batch.events));
                assert_eq!(last_usage(&events), last_usage(&batch.events));
                assert_eq!(event_kinds(&events), event_kinds(&batch.events));
                assert_eq!(split.finish(), Err(ProtocolError::Provider));
            }
        }
    }

    #[test]
    fn an_error_batch_never_announces_finished_even_after_a_terminal_prefix() {
        let stream = chat_wire("stop") + "data: [DONE]\n\n";
        let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
        let batch = decoder.push(stream.as_bytes());
        assert_eq!(batch.error, Some(ProtocolError::Protocol));
        assert!(text(&batch.events) == "Synthetic answer");
        assert!(!event_kinds(&batch.events).contains(&"finished"));
        assert!(!decoder.is_terminal());
        let poisoned = decoder.push(b"data: [DONE]\n\n");
        assert_eq!(poisoned.error, batch.error);
        assert!(poisoned.events.is_empty());
    }

    #[test]
    fn malformed_or_over_limit_suffixes_do_not_drop_valid_prefix_events() {
        let prefix = frame(chat_chunk(
            json!({"content":"Synthetic prefix"}),
            Value::Null,
        ));
        for (suffix, expected) in [
            ("data: {broken}\n\n".to_owned(), ProtocolError::Protocol),
            ("x".repeat(MAX_LINE_BYTES + 1), ProtocolError::LimitExceeded),
        ] {
            let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
            let batch = decoder.push((prefix.clone() + &suffix).as_bytes());
            assert_eq!(batch.error, Some(expected));
            assert!(text(&batch.events) == "Synthetic prefix");
            assert!(!event_kinds(&batch.events).contains(&"finished"));
        }
    }

    #[test]
    fn every_responses_final_snapshot_must_agree_with_streamed_text_or_refusal() {
        for refusal in [false, true] {
            let key = if refusal { "refusal" } else { "text" };
            for boundary in [4, 5, 6, 7] {
                let mut frames = response_frames(Some("final_answer"), refusal);
                match boundary {
                    4 => frames[4][key] = json!("Contradictory synthetic snapshot"),
                    5 => frames[5]["part"][key] = json!("Contradictory synthetic snapshot"),
                    6 => {
                        frames[6]["item"]["content"][0][key] =
                            json!("Contradictory synthetic snapshot")
                    }
                    7 => {
                        frames[7]["response"]["output"][0]["content"][0][key] =
                            json!("Contradictory synthetic snapshot")
                    }
                    _ => unreachable!(),
                }
                let mut decoder = StreamDecoder::new(Protocol::Responses);
                let batch = decoder.push(wire(frames).as_bytes());
                assert_eq!(batch.error, Some(ProtocolError::Protocol));
                assert!(!event_kinds(&batch.events).contains(&"finished"));
                assert!(batch.events.iter().any(|event| matches!(
                    event,
                    ModelEvent::TextDelta { .. } | ModelEvent::RefusalDelta { .. }
                )));
            }
        }
    }

    #[test]
    fn responses_missing_deltas_or_missing_snapshot_parts_cannot_succeed() {
        for refusal in [false, true] {
            let mut frames = response_frames(None, refusal);
            frames.remove(3);
            let mut decoder = StreamDecoder::new(Protocol::Responses);
            assert_eq!(
                decoder.push(wire(frames).as_bytes()).error,
                Some(ProtocolError::Protocol)
            );
            for boundary in [6, 7] {
                let mut frames = response_frames(None, refusal);
                if boundary == 6 {
                    frames[6]["item"]["content"] = json!([]);
                } else {
                    frames[7]["response"]["output"] = json!([]);
                }
                let mut decoder = StreamDecoder::new(Protocol::Responses);
                let batch = decoder.push(wire(frames).as_bytes());
                assert_eq!(batch.error, Some(ProtocolError::Protocol));
                assert!(!event_kinds(&batch.events).contains(&"finished"));
            }
        }
    }

    #[test]
    fn an_empty_responses_refusal_part_is_still_refused() {
        let events = decode(
            Protocol::Responses,
            &wire(response_frames_with_text(Some("final_answer"), true, "")),
            1,
        );
        assert!(text(&events).is_empty());
        assert!(!event_kinds(&events).contains(&"refusal"));
        assert_eq!(terminal(&events), FinishOutcome::Refused);
    }

    #[test]
    fn completed_and_incomplete_responses_with_nested_errors_are_failures() {
        for incomplete in [false, true] {
            let mut frames = response_frames(None, false);
            let final_frame = frames
                .last_mut()
                .unwrap_or_else(|| panic!("missing synthetic frame"));
            if incomplete {
                final_frame["type"] = json!("response.incomplete");
                final_frame["response"]["status"] = json!("incomplete");
                final_frame["response"]["incomplete_details"] =
                    json!({"reason":"max_output_tokens"});
            }
            final_frame["response"]["error"] =
                json!({"message":"synthetic private-shaped provider error"});
            let mut decoder = StreamDecoder::new(Protocol::Responses);
            let batch = decoder.push(wire(frames).as_bytes());
            assert_eq!(batch.error, Some(ProtocolError::Provider));
            assert!(!event_kinds(&batch.events).contains(&"finished"));
            assert!(!event_kinds(&batch.events).contains(&"usage"));
        }
    }

    #[test]
    fn explicit_responses_failed_event_is_provider_failure_not_completion() {
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        let stream = frame(response_frames(None, false)[0].clone())
            + &frame(
                json!({"type":"response.failed","response":{"id":"response-1","error":{"message":"synthetic marker"}}}),
            );
        assert!(matches!(
            decoder.push(stream.as_bytes()).error,
            Some(ProtocolError::Provider)
        ));
        assert!(!decoder.is_terminal());
    }

    #[test]
    fn invalid_utf8_malformed_usage_and_duplicate_terminal_are_not_accepted() {
        let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
        assert!(matches!(
            decoder.push(b"data: \xff\n\n").error,
            Some(ProtocolError::Protocol)
        ));
        for usage in [
            json!({"prompt_tokens":-1}),
            json!({"completion_tokens":1.5}),
            json!({"prompt_tokens_details":"invalid"}),
        ] {
            let mut decoder = StreamDecoder::new(Protocol::OpenRouterChat);
            let mut chunk = chat_chunk(json!({}), json!("stop"));
            chunk["usage"] = usage;
            assert!(matches!(
                decoder.push(frame(chunk).as_bytes()).error,
                Some(ProtocolError::Protocol)
            ));
        }
        let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
        assert!(decoder.push(chat_wire("stop").as_bytes()).error.is_none());
        assert!(matches!(
            decoder.push(b"data: [DONE]\n\n").error,
            Some(ProtocolError::Protocol)
        ));
        assert!(!decoder.is_terminal());
    }

    #[test]
    fn eof_in_partial_event_or_before_terminal_is_truncated() {
        for bytes in [
            b"data: {\"choices\":[]}".as_slice(),
            b": keepalive\n\n".as_slice(),
            b"data: [DO".as_slice(),
        ] {
            let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
            assert!(decoder.push(bytes).error.is_none());
            assert_eq!(decoder.finish(), Err(ProtocolError::Truncated));
            assert!(!decoder.is_terminal());
        }
    }

    #[test]
    fn line_event_wire_output_and_event_count_limits_are_enforced() {
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        assert!(matches!(
            decoder.push(&vec![b'x'; MAX_LINE_BYTES + 1]).error,
            Some(ProtocolError::LimitExceeded)
        ));
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        let line = format!("data: {}\n", "x".repeat(60 * 1024));
        for _ in 0..4 {
            assert!(decoder.push(line.as_bytes()).error.is_none());
        }
        assert!(matches!(
            decoder.push(line.as_bytes()).error,
            Some(ProtocolError::LimitExceeded)
        ));
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        let comment = format!(":{}\n", "x".repeat(60 * 1024));
        while decoder.wire_bytes + comment.len() <= MAX_STREAM_BYTES {
            assert!(decoder.push(comment.as_bytes()).error.is_none());
        }
        assert!(matches!(
            decoder.push(comment.as_bytes()).error,
            Some(ProtocolError::LimitExceeded)
        ));
        let mut decoder = StreamDecoder::new(Protocol::ChatCompletions);
        let chunk = frame(chat_chunk(
            json!({"content":"x".repeat(16*1024)}),
            Value::Null,
        ));
        for _ in 0..MAX_TEXT_BYTES / (16 * 1024) {
            assert!(decoder.push(chunk.as_bytes()).error.is_none());
        }
        assert!(matches!(
            decoder.push(chunk.as_bytes()).error,
            Some(ProtocolError::LimitExceeded)
        ));
        let mut decoder = StreamDecoder::new(Protocol::Responses);
        let info = frame(json!({"type":"response.synthetic_info"}));
        for _ in 0..MAX_EVENTS {
            assert!(decoder.push(info.as_bytes()).error.is_none());
        }
        assert!(matches!(
            decoder.push(info.as_bytes()).error,
            Some(ProtocolError::LimitExceeded)
        ));
    }
}

/// Wire-only image overlay. Only server-loaded canonical JPEG bytes are accepted;
/// no model/caller URL, file path or media identifier enters the wire request.
pub fn encode_image_request(
    protocol: Protocol,
    request: &TextRequest,
    capabilities: &TextCapabilities,
    images: &[Vec<u8>],
) -> Result<Value, ProtocolError> {
    use base64::{Engine as _, engine::general_purpose::STANDARD};
    let mut value = encode_request(protocol, request, capabilities)?;
    if images.is_empty() {
        return Ok(value);
    }
    if images.len() > 4
        || images
            .iter()
            .any(|v| v.len() > 2 * 1024 * 1024 || !v.starts_with(&[255, 216, 255]))
    {
        return Err(ProtocolError::LimitExceeded);
    }
    let index = request
        .messages
        .iter()
        .rposition(|m| m.role == TextRole::User)
        .ok_or(ProtocolError::Protocol)?;
    let mut content = vec![
        json!({"type":if protocol==Protocol::Responses{"input_text"}else{"text"},"text":request.messages[index].text}),
    ];
    for image in images {
        let data = STANDARD.encode(image);
        content.push(match protocol{
            Protocol::Responses=>json!({"type":"input_image","image_url":format!("data:image/jpeg;base64,{data}"),"detail":"auto"}),
            Protocol::AnthropicMessages=>json!({"type":"image","source":{"type":"base64","media_type":"image/jpeg","data":data}}),
            Protocol::OpenRouterChat|Protocol::ChatCompletions=>json!({"type":"image_url","image_url":{"url":format!("data:image/jpeg;base64,{data}")}})
        });
    }
    let key = if protocol == Protocol::Responses {
        "input"
    } else {
        "messages"
    };
    let projected_index = if protocol == Protocol::AnthropicMessages {
        request.messages[..index]
            .iter()
            .filter(|m| m.role != TextRole::System)
            .count()
    } else {
        index
    };
    value[key][projected_index]["content"] = json!(content);
    if serde_json::to_vec(&value)
        .map_err(|_| ProtocolError::Protocol)?
        .len()
        > 12 * 1024 * 1024
    {
        return Err(ProtocolError::LimitExceeded);
    }
    Ok(value)
}

#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn private_image_dialects_and_text_history_compatibility() {
        let request = TextRequest {
            model: "synthetic".into(),
            messages: vec![
                TextMessage {
                    role: TextRole::System,
                    text: "untrusted images".into(),
                    phase: None,
                },
                TextMessage {
                    role: TextRole::User,
                    text: "inspect".into(),
                    phase: None,
                },
            ],
            max_output_tokens: 100,
            effort: None,
        };
        let caps = TextCapabilities { efforts: vec![] };
        let images = vec![vec![255, 216, 255, 0]];
        for p in [
            Protocol::Responses,
            Protocol::AnthropicMessages,
            Protocol::ChatCompletions,
            Protocol::OpenRouterChat,
        ] {
            assert_eq!(
                encode_request(p, &request, &caps).unwrap(),
                encode_image_request(p, &request, &caps, &[]).unwrap()
            );
            let value = encode_image_request(p, &request, &caps, &images).unwrap();
            let image = match p {
                Protocol::Responses => &value["input"][1]["content"][1],
                Protocol::AnthropicMessages => &value["messages"][0]["content"][1],
                _ => &value["messages"][1]["content"][1],
            };
            match p {
                Protocol::Responses => assert_eq!(image["type"], "input_image"),
                Protocol::AnthropicMessages => {
                    assert_eq!(image["source"]["type"], "base64");
                    assert_eq!(image["source"]["media_type"], "image/jpeg")
                }
                _ => assert_eq!(image["type"], "image_url"),
            };
        }
        assert!(
            encode_image_request(
                Protocol::Responses,
                &request,
                &caps,
                &vec![images[0].clone(); 5]
            )
            .is_err()
        );
        assert!(
            encode_image_request(
                Protocol::Responses,
                &request,
                &caps,
                &[b"https://foreign/image".to_vec()]
            )
            .is_err()
        );
    }
}
