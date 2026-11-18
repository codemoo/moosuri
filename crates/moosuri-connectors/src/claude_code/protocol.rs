//! Conservative official CLI text profile. Per-block snapshots validate streamed text.
use crate::model::{
    TransportError,
    protocol::{FinishOutcome, ModelEvent, TextRequest, TextRole, Usage},
};
use serde_json::{Value, json};
use uuid::Uuid;
const MAX_TEXT: usize = 256 * 1024;
fn bad() -> TransportError {
    crate::model::protocol::ProtocolError::Protocol.into()
}
/// Shared bookkeeping validation for inference turns and fixed local commands.
/// A final ACK may follow result but cannot establish completion itself.
pub(super) fn command_lifecycle(
    v: &Value,
    session: &str,
    user: &str,
    init: bool,
    terminal: bool,
    done: &mut bool,
) -> Result<bool, TransportError> {
    if v["type"] != "command_lifecycle" {
        return Ok(false);
    }
    if v["session_id"] != session || !v["parent_tool_use_id"].is_null() || v["command_uuid"] != user
    {
        return Err(bad());
    }
    let expected = if terminal {
        v["state"] == "completed" && !*done
    } else {
        !init && matches!(v["state"].as_str(), Some("queued" | "started"))
    };
    if !expected {
        return Err(bad());
    }
    *done = terminal;
    Ok(true)
}
pub fn input(
    r: &TextRequest,
    session: &str,
    user: &str,
) -> Result<(String, Vec<u8>), TransportError> {
    if r.effort
        .as_deref()
        .is_some_and(|e| !matches!(e, "low" | "medium" | "high" | "xhigh"))
        || r.messages.is_empty()
        || r.messages.len() > 128
        || r.model.len() > 200
        || !r.model.starts_with("claude-")
        || !r
            .model
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'.')
    {
        return Err(TransportError::InvalidConfiguration);
    }
    let mut system = String::new();
    let mut history = Vec::new();
    let mut total = 0;
    for (i, m) in r.messages.iter().enumerate() {
        total += m.text.len();
        if total > 512 * 1024 || m.text.len() > 64 * 1024 {
            return Err(TransportError::LimitExceeded);
        }
        if m.role == TextRole::System {
            if i != 0 {
                return Err(TransportError::InvalidConfiguration);
            }
            system = m.text.clone();
        } else {
            history.push(
                json!({"role":if m.role==TextRole::User{"user"}else{"assistant"},"text":m.text}),
            );
        }
    }
    if history.is_empty() {
        return Err(TransportError::InvalidConfiguration);
    }
    let current = history.pop().ok_or_else(bad)?;
    if current["role"] != "user" {
        return Err(TransportError::InvalidConfiguration);
    }
    let text=serde_json::to_string(&json!({"format":"moosuri-local-history-v1","history_is_quoted_context":true,"history":history,"current_user_request":current["text"]})).map_err(|_|bad())?;
    let mut bytes=serde_json::to_vec(&json!({"type":"user","uuid":user,"session_id":session,"parent_tool_use_id":null,"client_composed":true,"message":{"role":"user","content":[{"type":"text","text":text}]}})).map_err(|_|bad())?;
    bytes.push(b'\n');
    Ok((system, bytes))
}
// The pinned CLI advertises its shipped definitions even with all executable
// tools disabled. Accept only its known inventory and keep tool outputs forbidden.
fn trusted_init_inventory(frame: &Value) -> bool {
    const SKILLS: &[&str] = &[
        "deep-research",
        "dataviz",
        "update-config",
        "verify",
        "debug",
        "code-review",
        "simplify",
        "batch",
        "fewer-permission-prompts",
        "doctor",
        "loop",
        "schedule",
        "claude-api",
        "workflow-authoring",
        "run",
        "run-skill-generator",
        "plugin-authoring",
    ];
    const AGENTS: &[&str] = &["Explore", "general-purpose", "Plan"];
    const PLUGINS: &[&str] = &[
        "cc-plugin-agents-md",
        "cc-plugin-privacy",
        "cc-plugin-plugin-authoring",
    ];
    let names_allowed = |key: &str, allowed: &[&str]| {
        frame.get(key).is_none_or(|value| {
            value.as_array().is_some_and(|items| {
                items.len() <= allowed.len()
                    && items
                        .iter()
                        .all(|item| item.as_str().is_some_and(|name| allowed.contains(&name)))
            })
        })
    };
    names_allowed("agents", AGENTS)
        && names_allowed("skills", SKILLS)
        && ["tools", "mcp_servers"]
            .iter()
            .all(|key| frame[*key].as_array().is_some_and(Vec::is_empty))
        && frame.get("plugins").is_none_or(|value| {
            value.as_array().is_some_and(|items| {
                items.len() <= PLUGINS.len()
                    && items.iter().all(|item| {
                        item["name"].as_str().is_some_and(|name| {
                            PLUGINS.contains(&name)
                                && item["path"] == "builtin"
                                && item["source"] == format!("{name}@builtin")
                        })
                    })
            })
        })
}
pub struct Decoder {
    session: String,
    user: String,
    model: String,
    version: String,
    init: bool,
    terminal: bool,
    lifecycle_done: bool,
    completed: bool,
    message: Option<String>,
    block: Option<u64>,
    block_kind: Option<String>,
    next_block: u64,
    block_snapshot: bool,
    block_nonempty: bool,
    block_text: String,
    stopped: bool,
    text: String,
    assistant: bool,
    usage: Option<Usage>,
    events: usize,
}
impl Decoder {
    pub fn new(session: &str, user: &str, model: &str, version: &str) -> Self {
        Self {
            session: session.into(),
            user: user.into(),
            model: model.into(),
            version: version.into(),
            init: false,
            terminal: false,
            lifecycle_done: false,
            completed: false,
            message: None,
            block: None,
            block_kind: None,
            next_block: 0,
            block_snapshot: false,
            block_nonempty: false,
            block_text: String::new(),
            stopped: false,
            text: String::new(),
            assistant: false,
            usage: None,
            events: 0,
        }
    }
    pub fn line(&mut self, line: &[u8]) -> Result<Vec<ModelEvent>, TransportError> {
        self.events += 1;
        if self.events > 32768 || line.len() > 256 * 1024 {
            return Err(TransportError::LimitExceeded);
        }
        let v: Value = serde_json::from_slice(line).map_err(|_| bad())?;
        if v["session_id"] != self.session || !v["parent_tool_use_id"].is_null() {
            return Err(bad());
        }
        // Official command bookkeeping surrounds one user turn. Its final ACK
        // can follow result; it never supplies content or establishes completion.
        if command_lifecycle(
            &v,
            &self.session,
            &self.user,
            self.init,
            self.terminal,
            &mut self.lifecycle_done,
        )? {
            return Ok(vec![]);
        }
        if self.terminal {
            return Err(bad());
        }
        let mut out = vec![];
        match v["type"].as_str() {
            Some("system") if v["subtype"] == "init" => {
                if self.init
                    || v["claude_code_version"] != self.version
                    || v["model"] != self.model
                    || v["cwd"] != "/work"
                {
                    return Err(bad());
                }
                if !trusted_init_inventory(&v) {
                    return Err(bad());
                }
                self.init = true;
                out.push(ModelEvent::Started {
                    response_id: Some(self.session.clone()),
                    model: Some(self.model.clone()),
                });
            }
            Some("system") if v["subtype"] == "ui_invalidate" && v["event"] == "ui.render" => {}
            Some("system")
                if self.init && v["subtype"] == "status" && v["status"] == "requesting" => {}
            Some("stream_event") if self.init => {
                let e = &v["event"];
                match e["type"].as_str() {
                    Some("message_start") => {
                        if self.message.is_some()
                            || self.assistant
                            || e["message"]["model"] != self.model
                            || e["message"]["role"] != "assistant"
                        {
                            return Err(bad());
                        }
                        let id = e["message"]["id"]
                            .as_str()
                            .filter(|s| !s.is_empty() && s.len() < 256)
                            .ok_or_else(bad)?;
                        self.message = Some(id.into());
                    }
                    Some("content_block_start") => {
                        if self.message.is_none()
                            || self.block.is_some()
                            || self.stopped
                            || e["index"].as_u64() != Some(self.next_block)
                        {
                            return Err(bad());
                        }
                        let kind = e["content_block"]["type"].as_str().ok_or_else(bad)?;
                        if !["text", "thinking", "redacted_thinking"].contains(&kind)
                            || kind == "text" && e["content_block"]["text"] != ""
                            || kind == "thinking" && e["content_block"]["thinking"] != ""
                            || kind == "redacted_thinking"
                                && !e["content_block"]["data"].is_string()
                        {
                            return Err(bad());
                        }
                        self.block = Some(e["index"].as_u64().ok_or_else(bad)?);
                        self.block_kind = Some(kind.into());
                        self.block_nonempty = kind == "redacted_thinking"
                            && e["content_block"]["data"]
                                .as_str()
                                .is_some_and(|s| !s.is_empty());
                    }
                    Some("content_block_delta") => {
                        if self.block != e["index"].as_u64()
                            || self.block.is_none()
                            || self.stopped
                            || self.block_snapshot
                        {
                            return Err(bad());
                        }
                        if self.block_kind.as_deref() != Some("text") {
                            if self.block_kind.as_deref() != Some("thinking")
                                || !matches!(
                                    e["delta"]["type"].as_str(),
                                    Some("thinking_delta") | Some("signature_delta")
                                )
                            {
                                return Err(bad());
                            }
                            let field = if e["delta"]["type"] == "thinking_delta" {
                                "thinking"
                            } else {
                                "signature"
                            };
                            self.block_nonempty |=
                                !e["delta"][field].as_str().ok_or_else(bad)?.is_empty();
                            return Ok(out);
                        }
                        if e["delta"]["type"] != "text_delta" {
                            return Err(bad());
                        }
                        let t = e["delta"]["text"].as_str().ok_or_else(bad)?;
                        if self.text.len() + t.len() > MAX_TEXT {
                            return Err(TransportError::LimitExceeded);
                        }
                        self.text.push_str(t);
                        self.block_text.push_str(t);
                        self.block_nonempty |= !t.is_empty();
                        out.push(ModelEvent::TextDelta {
                            text: t.into(),
                            item_id: self.message.clone(),
                            phase: None,
                        });
                    }
                    Some("content_block_stop") => {
                        if self.block.take() != e["index"].as_u64()
                            || self.block_kind.take().is_none()
                            || self.block_nonempty && !self.block_snapshot
                        {
                            return Err(bad());
                        }
                        self.next_block += 1;
                        self.block_snapshot = false;
                        self.block_nonempty = false;
                        self.block_text.clear();
                    }
                    Some("message_delta") => {
                        if self.message.is_none()
                            || !matches!(
                                e["delta"]["stop_reason"].as_str(),
                                Some("end_turn")
                                    | Some("max_tokens")
                                    | Some("stop_sequence")
                                    | None
                            )
                        {
                            return Err(bad());
                        }
                    }
                    Some("message_stop") => {
                        if self.message.is_none() || self.block.is_some() || self.stopped {
                            return Err(bad());
                        }
                        self.stopped = true;
                    }
                    Some("ping") => {}
                    _ => return Err(bad()),
                }
            }
            Some("assistant") if self.init => {
                let m = &v["message"];
                if self.block.is_none()
                    || self.block_snapshot
                    || self.stopped
                    || m["role"] != "assistant"
                    || m["model"] != self.model
                    || Some(m["id"].as_str().ok_or_else(bad)?) != self.message.as_deref()
                    || v.get("error").is_some()
                    || v.get("aborted").is_some()
                {
                    return Err(bad());
                }
                // The official CLI emits one snapshot for the completed content block,
                // before that block's stop event, rather than one final message snapshot.
                let blocks = m["content"].as_array().ok_or_else(bad)?;
                if blocks.len() != 1
                    || blocks[0]["type"].as_str() != self.block_kind.as_deref()
                    || self.block_kind.as_deref() == Some("text")
                        && blocks[0]["text"].as_str() != Some(self.block_text.as_str())
                    || self.block_kind.as_deref() == Some("thinking")
                        && (!blocks[0]["thinking"].is_string()
                            || blocks[0].get("signature").is_some_and(|x| !x.is_string()))
                    || self.block_kind.as_deref() == Some("redacted_thinking")
                        && !blocks[0]["data"].is_string()
                {
                    return Err(bad());
                }
                self.block_snapshot = true;
                self.assistant = true;
            }
            Some("rate_limit_event") if self.init => {
                let info = &v["rate_limit_info"];
                if !matches!(info["status"].as_str(), Some("allowed" | "allowed_warning"))
                    || info.get("errorCode").is_some()
                    || ["resetsAt", "utilization"].iter().any(|key| {
                        info.get(*key)
                            .is_some_and(|x| x.as_f64().is_none_or(|n| !n.is_finite() || n < 0.0))
                    })
                    || [
                        "canUserPurchaseCredits",
                        "hasChargeableSavedPaymentMethod",
                        "overageEnabled",
                    ]
                    .iter()
                    .any(|key| info.get(*key).is_some_and(|x| !x.is_boolean()))
                {
                    return Err(bad());
                }
                // Informational quota frames never authorize text or completion.
            }
            Some("result") if self.init => {
                if v["user_message_uuid"] != self.user {
                    return Err(bad());
                }
                let success = v["subtype"] == "success" && v["is_error"] == false;
                if success {
                    if !self.assistant
                        || !self.stopped
                        || self.message.is_none()
                        || self.block.is_some()
                        || v["result"] != self.text
                        || v.get("local_command").is_some()
                        || !matches!(
                            v["stop_reason"].as_str(),
                            Some("end_turn") | Some("stop_sequence")
                        )
                        || v.get("terminal_reason").is_some_and(|x| x != "completed")
                        || v.get("permission_denials")
                            .is_some_and(|x| x.as_array().is_none_or(|a| !a.is_empty()))
                    {
                        return Err(bad());
                    }
                } else if v["is_error"] != true
                    || !matches!(
                        v["subtype"].as_str(),
                        Some("error_during_execution")
                            | Some("error_max_turns")
                            | Some("error_max_budget_usd")
                            | Some("error_max_structured_output_retries")
                    )
                {
                    return Err(bad());
                }
                let mut evidence = vec![];
                fn n(v: &Value, k: &str) -> Result<Option<u64>, TransportError> {
                    if v.get(k).is_none_or(Value::is_null) {
                        return Ok(None);
                    }
                    Ok(Some(
                        v[k].as_u64()
                            .filter(|x| *x <= 1_000_000_000_000)
                            .ok_or_else(bad)?,
                    ))
                }
                let mut aggregate = Usage {
                    input_tokens: Some(0),
                    output_tokens: Some(0),
                    cache_read_tokens: Some(0),
                    cache_write_tokens: Some(0),
                };
                if let Some(values) = v.get("modelUsage").filter(|x| !x.is_null()) {
                    let values = values.as_object().ok_or_else(bad)?;
                    if values.is_empty() || values.len() > 32 {
                        return Err(bad());
                    }
                    for (name, u) in values {
                        if name.is_empty()
                            || name.len() > 200
                            || !name.bytes().all(|b| b.is_ascii_graphic())
                        {
                            return Err(bad());
                        }
                        let usage = Usage {
                            input_tokens: n(u, "inputTokens")?,
                            output_tokens: n(u, "outputTokens")?,
                            cache_read_tokens: n(u, "cacheReadInputTokens")?,
                            cache_write_tokens: n(u, "cacheCreationInputTokens")?,
                        };
                        fn add(
                            a: Option<u64>,
                            b: Option<u64>,
                        ) -> Result<Option<u64>, TransportError> {
                            match (a, b) {
                                (Some(a), Some(b)) => Ok(Some(
                                    a.checked_add(b)
                                        .filter(|x| *x <= 1_000_000_000_000)
                                        .ok_or_else(bad)?,
                                )),
                                _ => Ok(None),
                            }
                        }
                        aggregate.input_tokens = add(aggregate.input_tokens, usage.input_tokens)?;
                        aggregate.output_tokens =
                            add(aggregate.output_tokens, usage.output_tokens)?;
                        aggregate.cache_read_tokens =
                            add(aggregate.cache_read_tokens, usage.cache_read_tokens)?;
                        aggregate.cache_write_tokens =
                            add(aggregate.cache_write_tokens, usage.cache_write_tokens)?;
                        evidence.push((name.clone(), usage));
                    }
                } else {
                    aggregate = Usage::default();
                }
                self.usage = Some(aggregate);
                if !evidence.is_empty() {
                    out.push(ModelEvent::UsageBreakdown(evidence));
                }
                out.push(ModelEvent::UsageSnapshot(aggregate));
                self.completed = success;
                self.terminal = true;
            }
            _ => return Err(bad()),
        }
        Ok(out)
    }
    /// Completion is emitted only after a clean OS exit and EOF, never at the result line.
    pub fn finish(self, success: bool) -> Result<Vec<ModelEvent>, TransportError> {
        if !success || !self.terminal || !self.completed || self.text.trim().is_empty() {
            return Err(bad());
        }
        Ok(vec![ModelEvent::Finished {
            outcome: FinishOutcome::Completed,
            native_reason: "claude_code_text_complete".into(),
        }])
    }
}
pub fn ids() -> (String, String) {
    (Uuid::new_v4().to_string(), Uuid::new_v4().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    const MODEL: &str = "claude-fixture-v1";
    fn frames() -> Vec<Value> {
        vec![
            json!({"type":"system","subtype":"init","session_id":"s","claude_code_version":"2.1.322","model":MODEL,"cwd":"/work","tools":[],"mcp_servers":[],"plugins":[],"skills":[]}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"message_start","message":{"id":"m","role":"assistant","model":MODEL}}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_start","index":0,"content_block":{"type":"text","text":""}}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"안녕 OK"}}}),
            json!({"type":"assistant","session_id":"s","message":{"id":"m","role":"assistant","model":MODEL,"content":[{"type":"text","text":"안녕 OK"}]}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_stop","index":0}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"message_delta","delta":{"stop_reason":"end_turn"}}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"message_stop"}}),
            json!({"type":"result","session_id":"s","subtype":"success","is_error":false,"result":"안녕 OK","stop_reason":"end_turn","user_message_uuid":"u","modelUsage":{MODEL:{"inputTokens":8,"outputTokens":2,"cacheReadInputTokens":0,"cacheCreationInputTokens":0}}}),
        ]
    }
    fn decode(v: &[Value]) -> Result<Decoder, TransportError> {
        let mut d = Decoder::new("s", "u", MODEL, "2.1.322");
        for v in v {
            d.line(&serde_json::to_vec(v).unwrap())?;
        }
        Ok(d)
    }
    #[test]
    fn completion_requires_result_clean_exit_and_no_suffix() {
        let f = frames();
        assert!(decode(&f).unwrap().finish(true).is_ok());
        assert!(decode(&f).unwrap().finish(false).is_err());
        for end in 0..f.len() {
            assert!(decode(&f[..end]).unwrap().finish(true).is_err())
        }
        let mut d = decode(&f).unwrap();
        assert!(d.line(&serde_json::to_vec(&f[8]).unwrap()).is_err());
    }
    #[test]
    fn identities_tools_and_snapshots_cannot_authorize_text() {
        for (index, key, value) in [
            (0, "model", json!("other")),
            (0, "tools", json!(["Bash"])),
            (0, "plugins", json!([{"name":"bad"}])),
            (3, "session_id", json!("other")),
            (8, "user_message_uuid", json!("other")),
            (8, "result", json!("forged")),
            (8, "is_error", json!(true)),
            (8, "local_command", json!("model")),
        ] {
            let mut f = frames();
            f[index][key] = value;
            assert!(decode(&f).is_err(), "{index} {key}")
        }
        let mut f = frames();
        f[2]["event"]["content_block"]["type"] = json!("tool_use");
        assert!(decode(&f).is_err());
    }
    #[test]
    fn official_bundled_inventory_and_turn_notifications_do_not_grant_tools() {
        let mut f = frames();
        f[0]["agents"] = json!(["Explore", "Plan"]);
        f[0]["skills"] = json!(["doctor", "verify"]);
        f[0]["plugins"] = json!([{"name":"cc-plugin-privacy","path":"builtin","source":"cc-plugin-privacy@builtin"}]);
        f.insert(
            0,
            json!({"type":"system","session_id":"s","subtype":"ui_invalidate","event":"ui.render"}),
        );
        let notification = |state| json!({"type":"command_lifecycle","session_id":"s","command_uuid":"u","state":state});
        f.insert(1, notification("queued"));
        f.insert(2, notification("started"));
        f.insert(
            4,
            json!({"type":"system","session_id":"s","subtype":"status","status":"requesting"}),
        );
        f.push(notification("completed"));
        assert!(decode(&f).unwrap().finish(true).is_ok());
        let mut wrong = f.clone();
        wrong[3]["plugins"][0]["path"] = json!("/work/plugin");
        assert!(decode(&wrong).is_err());
        let mut wrong = f.clone();
        wrong[3]["skills"] = json!(["external-skill"]);
        assert!(decode(&wrong).is_err());
        let mut wrong = f.clone();
        wrong.last_mut().unwrap()["command_uuid"] = json!("other");
        assert!(decode(&wrong).is_err());
        f.push(notification("completed"));
        assert!(decode(&f).is_err());
    }

    #[test]
    fn input_keeps_system_private_and_commands_inert() {
        let r = TextRequest {
            model: MODEL.into(),
            effort: None,
            max_output_tokens: 2048,
            messages: vec![
                crate::model::protocol::TextMessage {
                    role: TextRole::System,
                    text: "trusted".into(),
                    phase: None,
                },
                crate::model::protocol::TextMessage {
                    role: TextRole::User,
                    text: "/model other @/host/private".into(),
                    phase: None,
                },
            ],
        };
        let (s, b) = input(&r, "s", "u").unwrap();
        assert_eq!(s, "trusted");
        let v: Value = serde_json::from_slice(&b).unwrap();
        assert_eq!(v["client_composed"], true);
        assert!(!String::from_utf8(b).unwrap().contains("trusted"));
    }
    #[test]
    fn usage_is_observed_before_later_nonzero_exit() {
        let f = frames();
        let mut d = Decoder::new("s", "u", MODEL, "2.1.322");
        let mut observed = false;
        for v in f {
            for e in d.line(&serde_json::to_vec(&v).unwrap()).unwrap() {
                observed |= matches!(e, ModelEvent::UsageSnapshot(_));
            }
        }
        assert!(observed);
        assert!(d.finish(false).is_err());
    }
    #[test]
    fn thinking_never_becomes_answer() {
        let mut f = frames();
        for frame in &mut f[2..6] {
            if frame["type"] == "stream_event" {
                frame["event"]["index"] = json!(1);
            }
        }
        let reasoning = vec![
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_start","index":0,"content_block":{"type":"thinking","thinking":""}}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_delta","index":0,"delta":{"type":"thinking_delta","thinking":"PRIVATE REASONING"}}}),
            json!({"type":"assistant","session_id":"s","message":{"id":"m","role":"assistant","model":MODEL,"content":[{"type":"thinking","thinking":"PRIVATE REASONING","signature":"sig"}]}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_stop","index":0}}),
        ];
        f.splice(2..2, reasoning);
        let mut d = Decoder::new("s", "u", MODEL, "2.1.322");
        let mut text = String::new();
        for v in f {
            for e in d.line(&serde_json::to_vec(&v).unwrap()).unwrap() {
                if let ModelEvent::TextDelta { text: t, .. } = e {
                    text.push_str(&t);
                }
            }
        }
        assert_eq!(text, "안녕 OK");
        assert!(d.finish(true).is_ok());
    }

    fn multi_block_frames() -> Vec<Value> {
        let mut f = frames();
        let second = vec![
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_start","index":1,"content_block":{"type":"text","text":""}}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_delta","index":1,"delta":{"type":"text_delta","text":" again"}}}),
            json!({"type":"assistant","session_id":"s","message":{"id":"m","role":"assistant","model":MODEL,"content":[{"type":"text","text":" again"}]}}),
            json!({"type":"stream_event","session_id":"s","event":{"type":"content_block_stop","index":1}}),
        ];
        f.splice(6..6, second);
        f.last_mut().unwrap()["result"] = json!("안녕 OK again");
        f
    }

    #[test]
    fn per_block_snapshots_validate_without_duplicating_text() {
        let f = multi_block_frames();
        let mut d = Decoder::new("s", "u", MODEL, "2.1.322");
        let mut text = String::new();
        for frame in &f {
            for event in d.line(&serde_json::to_vec(frame).unwrap()).unwrap() {
                if let ModelEvent::TextDelta { text: delta, .. } = event {
                    text.push_str(&delta);
                }
            }
        }
        assert_eq!(text, "안녕 OK again");
        assert!(d.finish(true).is_ok());
        for end in 0..f.len() {
            assert!(decode(&f[..end]).unwrap().finish(true).is_err());
        }
        assert!(decode(&f).unwrap().finish(false).is_err());
    }

    #[test]
    fn snapshots_must_match_one_open_block_once() {
        for (field, value) in [
            ("id", json!("other")),
            ("model", json!("other")),
            ("role", json!("user")),
            ("content", json!([{"type":"text","text":"forged"}])),
            ("content", json!([{"type":"thinking","thinking":"hidden"}])),
            (
                "content",
                json!([{"type":"text","text":"안녕 OK"},{"type":"text","text":"extra"}]),
            ),
        ] {
            let mut f = frames();
            f[4]["message"][field] = value;
            assert!(decode(&f).is_err(), "{field}");
        }
        let mut f = frames();
        f.insert(5, f[4].clone());
        assert!(decode(&f).is_err(), "duplicate snapshot");
        let mut f = frames();
        f.swap(4, 5);
        assert!(decode(&f).is_err(), "snapshot after stop");
        let mut f = frames();
        f.remove(4);
        assert!(decode(&f).is_err(), "missing snapshot");
        let mut f = multi_block_frames();
        f[8]["message"]["content"][0]["text"] = json!("안녕 OK again");
        assert!(
            decode(&f).is_err(),
            "cumulative snapshot instead of block snapshot"
        );
        let mut f = multi_block_frames();
        f[6]["event"]["index"] = json!(0);
        assert!(decode(&f).is_err(), "replayed block index");
    }

    #[test]
    fn only_valid_same_session_informational_rate_limits_are_accepted() {
        for status in ["allowed", "allowed_warning"] {
            let mut f = frames();
            f.insert(1, json!({"type":"rate_limit_event","session_id":"s","rate_limit_info":{"status":status,"resetsAt":123,"utilization":0.8,"overageEnabled":false}}));
            assert!(decode(&f).unwrap().finish(true).is_ok());
            assert!(decode(&f[..2]).unwrap().finish(true).is_err());
        }
        for event in [
            json!({"type":"rate_limit_event","session_id":"other","rate_limit_info":{"status":"allowed"}}),
            json!({"type":"rate_limit_event","session_id":"s","rate_limit_info":{"status":"rejected"}}),
            json!({"type":"rate_limit_event","session_id":"s","rate_limit_info":{"status":"allowed","errorCode":"credits_required"}}),
            json!({"type":"rate_limit_event","session_id":"s","rate_limit_info":{"status":"allowed_warning","overageEnabled":"yes"}}),
            json!({"type":"rate_limit_event","session_id":"s","rate_limit_info":{"status":"unknown"}}),
            json!({"type":"unknown_event","session_id":"s"}),
        ] {
            let mut f = frames();
            f.insert(1, event);
            assert!(decode(&f).is_err());
        }
    }
}
#[cfg(test)]
mod accounting_tests {
    use super::*;
    #[test]
    fn auxiliary_models_are_aggregated_without_changing_answer_identity() {
        let mut d = Decoder::new("s", "u", "claude-fixture-v1", "2.1.322");
        d.init = true;
        d.assistant = true;
        d.stopped = true;
        d.message = Some("m".into());
        d.text = "OK".into();
        let v = json!({"type":"result","session_id":"s","user_message_uuid":"u","subtype":"success","is_error":false,"result":"OK","stop_reason":"end_turn","modelUsage":{"claude-fixture-v1":{"inputTokens":8,"outputTokens":2,"cacheReadInputTokens":0,"cacheCreationInputTokens":0},"claude-auxiliary":{"inputTokens":3,"outputTokens":1,"cacheReadInputTokens":0,"cacheCreationInputTokens":0}}});
        let events = d.line(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(events.iter().any(|e| matches!(
            e,
            ModelEvent::UsageSnapshot(Usage {
                input_tokens: Some(11),
                output_tokens: Some(3),
                ..
            })
        )));
        assert!(
            events
                .iter()
                .any(|e| matches!(e,ModelEvent::UsageBreakdown(v)if v.len()==2))
        );
        assert!(d.finish(true).is_ok());
    }
    #[test]
    fn unknown_accounting_does_not_become_zero() {
        let mut d = Decoder::new("s", "u", "claude-fixture-v1", "2.1.322");
        d.init = true;
        d.assistant = true;
        d.stopped = true;
        d.message = Some("m".into());
        d.text = "OK".into();
        let v = json!({"type":"result","session_id":"s","user_message_uuid":"u","subtype":"success","is_error":false,"result":"OK","stop_reason":"end_turn"});
        let e = d.line(&serde_json::to_vec(&v).unwrap()).unwrap();
        assert!(e.iter().any(|e| matches!(
            e,
            ModelEvent::UsageSnapshot(Usage {
                input_tokens: None,
                output_tokens: None,
                ..
            })
        )));
        assert!(d.finish(true).is_ok());
    }
}
