//! Opt-in synthetic classification evaluation through the official account-backed CLI.
//! Scope: label classification plus production parser shape/origin checks only.
//! Captured calls receive synthetic decoder framing: this does not evaluate
//! provider EOF/stream completeness, fact/subject/target semantic quality, or writes.
use crate::memory_intent::{self, Clarification, Decision};
use moosuri_connectors::model::protocol::{
    Protocol, TextCapabilities, TextRequest, TextRole,
    tools::{ToolCapabilities, ToolRequest, ToolRoundDecoder},
};
use moosuri_core::MemoryCommandAction;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    fs::{File, OpenOptions},
    io::{BufReader, Read, Seek, SeekFrom, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const CASE_LIMIT: usize = 41;
const CASE_SECONDS: u64 = 60;
const OVERALL_SECONDS: u64 = 41 * CASE_SECONDS + 15;
const LINE_LIMIT: usize = 512 * 1024;
const OUTPUT_LIMIT: usize = 4 * 1024 * 1024;
const FUNCTION: &str = "memory_command_decision";
const DISABLED_FEATURES: &[&str] = &[
    "apps",
    "plugins",
    "shell_tool",
    "hooks",
    "memories",
    "multi_agent",
];
const CORPUS_BYTES: &[u8] = include_bytes!("../../tests/fixtures/memory_intent_semantic.json");
const CLASSIFIER_BYTES: &[u8] = include_bytes!("../memory_intent.rs");
const ADAPTER_BYTES: &[u8] = include_bytes!("live_memory.rs");
type EvalResult<T> = Result<T, &'static str>;

#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DecisionLabel {
    NoCommand,
    Clarify,
    Command,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ActionLabel {
    Remember,
    Correct,
    Forget,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum ClarifyLabel {
    Ambiguous,
    Temporal,
    Unsupported,
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Eq)]
struct Labels {
    decision: DecisionLabel,
    action: Option<ActionLabel>,
    clarification: Option<ClarifyLabel>,
}
#[derive(Deserialize)]
struct Corpus {
    cases: Vec<Case>,
}
#[derive(Deserialize)]
struct Case {
    id: String,
    current_text: String,
    expected_decision: DecisionLabel,
    expected_action: Option<ActionLabel>,
    expected_clarification: Option<ClarifyLabel>,
}
impl Case {
    fn expected(&self) -> Labels {
        Labels {
            decision: self.expected_decision,
            action: self.expected_action,
            clarification: self.expected_clarification,
        }
    }
}
#[derive(Default, Serialize)]
struct Metrics {
    model: Option<String>,
    effort: Option<String>,
    auth_type: Option<&'static str>,
    instruction_source_count: Option<usize>,
    usage: Option<Value>,
    closure: Option<&'static str>,
    inherited_mcp_count: Option<usize>,
    enabled_mcp_count: Option<usize>,
    mcp_tool_count: Option<usize>,
    turn_dispatch_attempted: bool,
    decision_captured: bool,
    evaluated: bool,
}
#[derive(Serialize)]
struct Row {
    case_id: String,
    outcome: &'static str,
    error: Option<&'static str>,
    latency_ms: u64,
    expected: Labels,
    raw: Option<Labels>,
    parsed: Option<Labels>,
    metrics: Metrics,
}

// All process diagnostics and RPC error payloads remain private and are discarded.
// A bounded reader prevents either an unterminated line or notification flood
// from growing the test process without limit. Dropping the receiver ends it.
struct Server {
    child: Child,
    stdin: ChildStdin,
    rx: Receiver<EvalResult<Vec<u8>>>,
    reader: Option<thread::JoinHandle<()>>,
    next_id: u64,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(reader) = self.reader.take() {
            let _ = reader.join();
        }
    }
}
impl Server {
    fn spawn(model: &str, effort: &str, cwd: &Path, disabled_mcp: &[String]) -> EvalResult<Self> {
        let mut command = Command::new("codex");
        command
            .args([
                "app-server",
                "--stdio",
                "-c",
                "model_provider=\"openai\"",
                "-c",
            ])
            .arg(format!("model={}", json!(model)))
            .arg("-c")
            .arg(format!("model_reasoning_effort={}", json!(effort)));
        for feature in DISABLED_FEATURES {
            command.args(["--disable", feature]);
        }
        for value in [
            "web_search=\"disabled\"",
            "skills.include_instructions=false",
            "project_doc_max_bytes=0",
        ] {
            command.args(["-c", value]);
        }
        for name in disabled_mcp {
            if !valid_server_name(name) {
                return Err("mcp_name_invalid");
            }
            command
                .arg("-c")
                .arg(format!("mcp_servers.{name}.enabled=false"));
        }
        let mut child = command
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| "spawn_failed")?;
        let stdin = child.stdin.take().ok_or("stdin_missing")?;
        let stdout = child.stdout.take().ok_or("stdout_missing")?;
        let (tx, rx) = mpsc::sync_channel(128);
        let reader = thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            let mut line = Vec::new();
            let mut total = 0usize;
            let mut chunk = [0u8; 4096];
            loop {
                let n = match reader.read(&mut chunk) {
                    Ok(0) => {
                        let _ = tx.try_send(Err("server_closed"));
                        return;
                    }
                    Ok(n) => n,
                    Err(_) => {
                        let _ = tx.try_send(Err("stdout_read_failed"));
                        return;
                    }
                };
                total += n;
                if total > OUTPUT_LIMIT {
                    let _ = tx.try_send(Err("output_limit"));
                    return;
                }
                for byte in &chunk[..n] {
                    if *byte == b'\n' {
                        if tx.try_send(Ok(std::mem::take(&mut line))).is_err() {
                            return;
                        }
                    } else {
                        line.push(*byte);
                        if line.len() > LINE_LIMIT {
                            let _ = tx.try_send(Err("line_limit"));
                            return;
                        }
                    }
                }
            }
        });
        Ok(Self {
            child,
            stdin,
            rx,
            reader: Some(reader),
            next_id: 1,
        })
    }
    fn send(&mut self, method: &str, params: Value) -> EvalResult<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.write(&json!({"id":id,"method":method,"params":params}))?;
        Ok(id)
    }
    fn write(&mut self, message: &Value) -> EvalResult<()> {
        let mut bytes = serde_json::to_vec(message).map_err(|_| "encode_failed")?;
        if bytes.len() > 64 * 1024 {
            return Err("request_limit");
        }
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .map_err(|_| "stdin_write_failed")?;
        self.stdin.flush().map_err(|_| "stdin_write_failed")
    }
    fn read_bytes(&self, deadline: Instant) -> EvalResult<Vec<u8>> {
        let left = deadline
            .checked_duration_since(Instant::now())
            .ok_or("deadline")?;
        self.rx
            .recv_timeout(left)
            .map_err(|_| "deadline_or_closed")?
    }
    fn read(&self, deadline: Instant) -> EvalResult<Value> {
        serde_json::from_slice(&self.read_bytes(deadline)?).map_err(|_| "invalid_json")
    }
    fn initialize(&mut self, deadline: Instant, metrics: &mut Metrics) -> EvalResult<()> {
        self.rpc(
            "initialize",
            json!({"clientInfo":{"name":"moosuri_evaluation","version":"0.1.0"},
            "capabilities":{"experimentalApi":true}}),
            deadline,
            metrics,
        )?;
        self.write(&json!({"method":"initialized","params":{}}))
    }
    // Selectively deserialize only names and enablement booleans. Credential,
    // endpoint, environment, layer, instruction and identity fields are skipped
    // by serde rather than copied into Value or exposed in diagnostics.
    fn config_metadata(
        &mut self,
        cwd: &Path,
        deadline: Instant,
        metrics: &mut Metrics,
    ) -> EvalResult<ConfigMetadata> {
        let id = self.send("config/read", json!({"includeLayers":false,"cwd":cwd}))?;
        loop {
            let bytes = self.read_bytes(deadline)?;
            let reply: MetadataReply =
                serde_json::from_slice(&bytes).map_err(|_| "config_metadata_invalid")?;
            if reply.method.is_some() {
                let event: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid_json")?;
                check_event(&event, metrics)?;
                continue;
            }
            if reply.id != Some(id) {
                return Err("unexpected_response");
            }
            return reply
                .result
                .map(|r| r.config)
                .ok_or("config_metadata_unavailable");
        }
    }
    fn rpc(
        &mut self,
        method: &str,
        params: Value,
        deadline: Instant,
        metrics: &mut Metrics,
    ) -> EvalResult<Value> {
        let id = self.send(method, params)?;
        loop {
            let v = self.read(deadline)?;
            if v.get("method").is_some() {
                check_event(&v, metrics)?;
                continue;
            }
            if v.get("id").and_then(Value::as_u64) != Some(id) {
                return Err("unexpected_response");
            }
            if v.get("error").is_some() {
                return Err("rpc_rejected");
            }
            return v.get("result").cloned().ok_or("missing_result");
        }
    }
}
#[derive(Deserialize)]
struct MetadataReply {
    id: Option<u64>,
    method: Option<String>,
    result: Option<MetadataResult>,
}
#[derive(Deserialize)]
struct MetadataResult {
    config: ConfigMetadata,
}
#[derive(Deserialize)]
struct ConfigMetadata {
    #[serde(default)]
    mcp_servers: BTreeMap<String, ServerEnablement>,
    features: BTreeMap<String, Option<bool>>,
    web_search: String,
}
#[derive(Deserialize)]
struct ServerEnablement {
    enabled: Option<bool>,
}
fn valid_server_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 128
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
}
fn verify_isolation(config: &ConfigMetadata) -> EvalResult<()> {
    if config.mcp_servers.len() > 64 || config.mcp_servers.keys().any(|n| !valid_server_name(n)) {
        return Err("mcp_inventory_invalid");
    }
    if config
        .mcp_servers
        .values()
        .any(|s| s.enabled != Some(false))
    {
        return Err("inherited_mcp_enabled");
    }
    if DISABLED_FEATURES
        .iter()
        .any(|feature| config.features.get(*feature) != Some(&Some(false)))
        || config.web_search != "disabled"
    {
        return Err("process_tool_flags_unverified");
    }
    Ok(())
}
fn verify_mcp_surface(status: &Value, config: &ConfigMetadata) -> EvalResult<()> {
    if status.get("nextCursor").is_some_and(|c| !c.is_null()) {
        return Err("mcp_inventory_paginated");
    }
    let servers = status
        .get("data")
        .and_then(Value::as_array)
        .ok_or("mcp_surface_invalid")?;
    for server in servers {
        let name = server
            .get("name")
            .and_then(Value::as_str)
            .ok_or("mcp_surface_invalid")?;
        if config
            .mcp_servers
            .get(name)
            .is_none_or(|s| s.enabled != Some(false))
            || server
                .get("tools")
                .and_then(Value::as_object)
                .is_none_or(|t| !t.is_empty())
            || server
                .get("resources")
                .and_then(Value::as_array)
                .is_none_or(|r| !r.is_empty())
            || server
                .get("resourceTemplates")
                .and_then(Value::as_array)
                .is_none_or(|r| !r.is_empty())
            || server.get("runtimeStatus").is_some_and(|s| {
                !s.is_null() && !matches!(s.as_str(), Some("disabled" | "notStarted"))
            })
        {
            return Err("mcp_surface_not_empty");
        }
    }
    Ok(())
}
fn isolated_server(
    model: &str,
    effort: &str,
    cwd: &Path,
    deadline: Instant,
    metrics: &mut Metrics,
) -> EvalResult<(Server, ConfigMetadata)> {
    // Discovery performs only initialize + config/read, never thread/start,
    // tool discovery, account access, or model turns. No global config is edited.
    let mut discovery = Server::spawn(model, effort, cwd, &[])?;
    discovery.initialize(deadline, metrics)?;
    let metadata = discovery.config_metadata(cwd, deadline, metrics)?;
    let names: Vec<String> = metadata.mcp_servers.keys().cloned().collect();
    if names.len() > 64 || names.iter().any(|n| !valid_server_name(n)) {
        return Err("mcp_inventory_invalid");
    }
    metrics.inherited_mcp_count = Some(names.len());
    drop(metadata);
    drop(discovery);
    let mut runtime = Server::spawn(model, effort, cwd, &names)?;
    runtime.initialize(deadline, metrics)?;
    let effective = runtime.config_metadata(cwd, deadline, metrics)?;
    verify_isolation(&effective)?;
    let status = runtime.rpc(
        "mcpServerStatus/list",
        json!({"limit":100,"detail":"toolsAndAuthOnly"}),
        deadline,
        metrics,
    )?;
    verify_mcp_surface(&status, &effective)?;
    metrics.enabled_mcp_count = Some(0);
    metrics.mcp_tool_count = Some(0);
    Ok((runtime, effective))
}

fn check_event(v: &Value, metrics: &mut Metrics) -> EvalResult<()> {
    // Any server request is rejected unless explicitly consumed as our decision.
    if v.get("id").is_some() {
        return Err("unexpected_server_request");
    }
    let method = v
        .get("method")
        .and_then(Value::as_str)
        .ok_or("missing_method")?;
    if method == "error" {
        return Err("server_error");
    }
    if method == "mcpServer/startupStatus/updated"
        && !matches!(
            v.pointer("/params/status").and_then(Value::as_str),
            Some("disabled" | "notStarted")
        )
    {
        return Err("mcp_runtime_observed");
    }
    if method == "model/rerouted" {
        return Err("model_rerouted");
    }
    if method.starts_with("hook/")
        || method.starts_with("command/")
        || method.starts_with("process/")
        || method.starts_with("item/commandExecution/")
        || method.starts_with("item/fileChange/")
        || method.starts_with("item/mcpToolCall/")
        || method == "thread/environment/connected"
    {
        return Err("unexpected_tool_notification");
    }
    if matches!(method, "item/started" | "item/completed") {
        let kind = v
            .pointer("/params/item/type")
            .and_then(Value::as_str)
            .ok_or("missing_item_type")?;
        if !matches!(
            kind,
            "userMessage" | "reasoning" | "agentMessage" | "dynamicToolCall"
        ) {
            return Err("unexpected_tool_item");
        }
    }
    if method == "thread/tokenUsage/updated" {
        // Copy only numeric token counts; never serialize arbitrary server data.
        if let Some(last) = v
            .pointer("/params/tokenUsage/last")
            .and_then(Value::as_object)
        {
            let mut usage = serde_json::Map::new();
            for key in [
                "inputTokens",
                "outputTokens",
                "cachedInputTokens",
                "cacheWriteInputTokens",
                "reasoningOutputTokens",
                "totalTokens",
            ] {
                if let Some(n) = last.get(key).and_then(Value::as_u64) {
                    usage.insert(key.into(), n.into());
                }
            }
            if !usage.is_empty() {
                metrics.usage = Some(Value::Object(usage));
            }
        }
    }
    Ok(())
}

fn labels_from_parser(text: &str, base: &TextRequest, call: &Value) -> EvalResult<Labels> {
    let tools = memory_intent::definitions();
    let req = ToolRequest {
        text: base,
        tools: &tools,
        continuation: None,
        results: &[],
    };
    let mut decoder = ToolRoundDecoder::new(
        Protocol::Responses,
        &req,
        // Live evaluation has verified this exact requested model/effort in
        // model/list before reaching the adapter. Preserve that capability here.
        &TextCapabilities {
            efforts: base.effort.iter().cloned().collect(),
        },
        &ToolCapabilities {
            function_tools: true,
        },
    )
    .map_err(|_| "decoder_init_failed")?;
    let arguments = serde_json::to_string(call.get("arguments").ok_or("missing_arguments")?)
        .map_err(|_| "arguments_encode_failed")?;
    // Normalize the already-captured dynamic call into a complete Responses
    // function-call round. This adapter does not claim these synthetic framing
    // events were observed on the app-server wire, and performs no inference.
    let item = json!({"type":"function_call","id":"live-adapter-item",
        "call_id":call["callId"],"name":call["tool"],"arguments":arguments,"status":"completed"});
    let frames = [
        json!({"type":"response.created","response":{"id":"live-adapter","model":base.model}}),
        json!({"type":"response.output_item.added","output_index":0,"item":{
            "type":"function_call","id":"live-adapter-item","call_id":call["callId"],
            "name":call["tool"],"arguments":"","status":"in_progress"}}),
        json!({"type":"response.function_call_arguments.delta","output_index":0,
            "item_id":"live-adapter-item","delta":arguments}),
        json!({"type":"response.function_call_arguments.done","output_index":0,
            "item_id":"live-adapter-item","arguments":arguments}),
        json!({"type":"response.output_item.done","output_index":0,"item":item}),
        json!({"type":"response.completed","response":{"id":"live-adapter","model":base.model,
            "status":"completed","output":[item]}}),
    ];
    let wire: String = frames
        .iter()
        .map(|frame| format!("data: {frame}\n\n"))
        .collect();
    if decoder.push(wire.as_bytes()).error.is_some() {
        return Err("decoder_rejected");
    }
    let round = decoder.finish().map_err(|_| "decoder_incomplete")?;
    let parsed = memory_intent::parse_completed(text, &round).map_err(|_| "parser_rejected")?;
    Ok(match parsed {
        Decision::NoCommand => Labels {
            decision: DecisionLabel::NoCommand,
            action: None,
            clarification: None,
        },
        Decision::Clarify(c) => Labels {
            decision: DecisionLabel::Clarify,
            action: None,
            clarification: Some(match c {
                Clarification::Ambiguous => ClarifyLabel::Ambiguous,
                Clarification::Temporal => ClarifyLabel::Temporal,
                Clarification::Unsupported => ClarifyLabel::Unsupported,
            }),
        },
        Decision::Command(intent) => Labels {
            decision: DecisionLabel::Command,
            action: Some(match intent.action {
                MemoryCommandAction::Remember => ActionLabel::Remember,
                MemoryCommandAction::Correct => ActionLabel::Correct,
                MemoryCommandAction::Forget => ActionLabel::Forget,
            }),
            clarification: None,
        },
    })
}

fn terminal_without_decision_error(event: &Value) -> &'static str {
    if event
        .pointer("/params/turn/error")
        .is_some_and(|e| !e.is_null())
    {
        return "turn_terminal_error";
    }
    match event.pointer("/params/turn/status").and_then(Value::as_str) {
        Some("completed") => "completed_without_decision",
        Some("failed") => "turn_failed",
        Some("interrupted") => "turn_interrupted",
        _ => "invalid_turn_terminal",
    }
}
fn model_format_error(error: &str) -> bool {
    matches!(error, "completed_without_decision" | "arguments_limit")
}
fn case_evaluated(row: &Row) -> bool {
    row.metrics.evaluated || matches!(row.outcome, "cue_bypassed_match" | "cue_bypassed_mismatch")
}
fn report_complete(rows: &[Row]) -> bool {
    rows.len() == CASE_LIMIT && rows.iter().all(case_evaluated)
}
#[test]
fn terminal_failures_and_partial_reports_are_not_model_quality_results() {
    let terminal =
        |status: &str, error: Value| json!({"params":{"turn":{"status":status,"error":error}}});
    assert!(model_format_error(terminal_without_decision_error(
        &terminal("completed", Value::Null)
    )));
    for status in ["failed", "interrupted", "inProgress"] {
        assert!(!model_format_error(terminal_without_decision_error(
            &terminal(status, Value::Null)
        )));
    }
    assert!(!model_format_error(terminal_without_decision_error(
        &terminal("completed", json!({"message":"synthetic"}))
    )));
    assert!(model_format_error("arguments_limit"));
    let expected = Labels {
        decision: DecisionLabel::NoCommand,
        action: None,
        clarification: None,
    };
    let mut rows: Vec<Row> = (0..CASE_LIMIT)
        .map(|_| Row {
            case_id: "synthetic".into(),
            outcome: "model_format_error",
            error: Some("arguments_limit"),
            latency_ms: 0,
            expected,
            raw: None,
            parsed: None,
            metrics: Metrics {
                evaluated: true,
                ..Metrics::default()
            },
        })
        .collect();
    assert!(report_complete(&rows));
    rows[0].metrics.evaluated = false;
    rows[0].outcome = "transport_error";
    assert!(!report_complete(&rows));
    rows[0].outcome = "cue_bypassed_match";
    assert!(report_complete(&rows));
    rows.pop();
    assert!(!report_complete(&rows));
}

fn apparatus_error(error: &str) -> bool {
    matches!(
        error,
        "decoder_init_failed"
            | "decoder_rejected"
            | "decoder_incomplete"
            | "arguments_encode_failed"
            | "production_request_failed"
            | "missing_system"
            | "missing_user"
    )
}
#[test]
fn inherited_mcp_and_error_boundaries_fail_closed() {
    let mut features: BTreeMap<String, Option<bool>> = DISABLED_FEATURES
        .iter()
        .map(|k| ((*k).into(), Some(false)))
        .collect();
    features.insert("synthetic_unset_feature".into(), None);
    // Effective metadata includes null for unset feature toggles. Unrelated
    // configuration fields remain ignored by this selective projection.
    let mut config: ConfigMetadata = serde_json::from_value(json!({
        "mcp_servers":{}, "features":features,"web_search":"disabled",
        "unrelated_private_field":"synthetic_ignored_value"
    }))
    .expect("nullable effective feature metadata");
    config
        .mcp_servers
        .insert("synthetic".into(), ServerEnablement { enabled: None });
    assert!(matches!(
        verify_isolation(&config),
        Err("inherited_mcp_enabled")
    ));
    config.mcp_servers.get_mut("synthetic").unwrap().enabled = Some(true);
    assert!(verify_isolation(&config).is_err());
    config.mcp_servers.get_mut("synthetic").unwrap().enabled = Some(false);
    assert!(verify_isolation(&config).is_ok());
    assert!(verify_mcp_surface(&json!({"data":[],"nextCursor":null}), &config).is_ok());
    assert!(
        verify_mcp_surface(
            &json!({"data":[{"name":"synthetic","tools":{"unsafe":{}},
        "resources":[],"resourceTemplates":[],"runtimeStatus":"connected"}]}),
            &config
        )
        .is_err()
    );
    assert!(apparatus_error("decoder_rejected") && apparatus_error("decoder_incomplete"));
    assert!(!apparatus_error("completed_without_decision") && !apparatus_error("parser_rejected"));
}

#[test]
fn live_high_effort_call_reaches_production_parser() {
    let text = "I like tea. Remember that I like tea.";
    let base = memory_intent::request("synthetic-model".into(), Some("high".into()), text)
        .expect("synthetic production request");
    let mut call = json!({"tool":FUNCTION,"callId":"synthetic-live-call","arguments":{
        "decision":"command","clarification":null,"action":"remember",
        "intent_quote":"Remember that I like tea.","fact_quote":"I like tea",
        "subject":"drink preference","target_quote":null
    }});
    let parsed = labels_from_parser(text, &base, &call);
    assert!(
        matches!(
            parsed,
            Ok(Labels {
                decision: DecisionLabel::Command,
                action: Some(ActionLabel::Remember),
                clarification: None
            })
        ),
        "high-effort adapter failed: {}",
        parsed.err().unwrap_or("unexpected_labels")
    );
    // A literal-origin violation must reach and be rejected by the production
    // parser, rather than being mistaken for an adapter initialization failure.
    call["arguments"]["fact_quote"] = json!("I like coffee");
    assert!(matches!(
        labels_from_parser(text, &base, &call),
        Err("parser_rejected")
    ));
}

fn evaluate(
    case: &Case,
    model: &str,
    effort: &str,
    cwd: &Path,
    deadline: Instant,
    metrics: &mut Metrics,
) -> EvalResult<Value> {
    let base = memory_intent::request(model.into(), Some(effort.into()), &case.current_text)
        .map_err(|_| "production_request_failed")?;
    let system = base
        .messages
        .iter()
        .find(|m| m.role == TextRole::System)
        .ok_or("missing_system")?;
    let user = base
        .messages
        .iter()
        .find(|m| m.role == TextRole::User)
        .ok_or("missing_user")?;
    let tools: Vec<Value> = memory_intent::definitions().into_iter().map(|d|
        json!({"type":"function","name":d.name,"description":d.description,"inputSchema":d.parameters})).collect();
    let (mut server, isolation) = isolated_server(model, effort, cwd, deadline, metrics)?;
    let account = server.rpc(
        "account/read",
        json!({"refreshToken":false}),
        deadline,
        metrics,
    )?;
    if account.pointer("/account/type").and_then(Value::as_str) != Some("chatgpt") {
        return Err("chatgpt_auth_required");
    }
    metrics.auth_type = Some("chatgpt");
    drop(account);
    let models = server.rpc(
        "model/list",
        json!({"limit":100,"includeHidden":true}),
        deadline,
        metrics,
    )?;
    let catalog = models
        .get("data")
        .and_then(Value::as_array)
        .ok_or("missing_catalog")?;
    let selected = catalog
        .iter()
        .find(|m| m.get("model").and_then(Value::as_str) == Some(model))
        .ok_or("requested_model_unavailable")?;
    if !selected
        .get("supportedReasoningEfforts")
        .and_then(Value::as_array)
        .is_some_and(|efforts| {
            efforts
                .iter()
                .any(|e| e.get("reasoningEffort").and_then(Value::as_str) == Some(effort))
        })
    {
        return Err("requested_effort_unavailable");
    }
    let started = server.rpc("thread/start", json!({"model":model,"modelProvider":"openai",
        "ephemeral":true,"cwd":cwd,"environments":[],"selectedCapabilityRoots":[],
        "baseInstructions":system.text,"developerInstructions":"","approvalPolicy":"never","sandbox":"read-only",
        "config":{"model_reasoning_effort":effort,"features.apps":false,"features.plugins":false,
          "features.memories":false,"features.multi_agent":false,"features.shell_tool":false,
          "features.hooks":false,"skills.include_instructions":false,"project_doc_max_bytes":0,"web_search":"disabled"},
        "dynamicTools":tools}), deadline, metrics)?;
    if started.get("model").and_then(Value::as_str) != Some(model)
        || started.get("reasoningEffort").and_then(Value::as_str) != Some(effort)
    {
        return Err("actual_model_effort_mismatch");
    }
    if started.pointer("/sandbox/type").and_then(Value::as_str) != Some("readOnly") {
        return Err("sandbox_mismatch");
    }
    metrics.model = Some(model.into());
    metrics.effort = Some(effort.into());
    metrics.instruction_source_count = started
        .get("instructionSources")
        .and_then(Value::as_array)
        .map(Vec::len);
    let thread_id = started
        .pointer("/thread/id")
        .and_then(Value::as_str)
        .ok_or("missing_thread_id")?;
    let surface = server.rpc(
        "mcpServerStatus/list",
        json!({"threadId":thread_id,
        "limit":100,"detail":"toolsAndAuthOnly"}),
        deadline,
        metrics,
    )?;
    verify_mcp_surface(&surface, &isolation)?;
    metrics.turn_dispatch_attempted = true;
    let turn = server.rpc(
        "turn/start",
        json!({"threadId":thread_id,"effort":effort,
        "environments":[],"input":[{"type":"text","text":user.text}]}),
        deadline,
        metrics,
    )?;
    let turn_id = turn
        .pointer("/turn/id")
        .and_then(Value::as_str)
        .ok_or("missing_turn_id")?;
    let call = loop {
        let v = server.read(deadline)?;
        if v.get("method").and_then(Value::as_str) == Some("item/tool/call") {
            if v.get("id").is_none() {
                return Err("missing_server_request_id");
            }
            let p = v.get("params").ok_or("missing_call_params")?;
            if p.get("tool").and_then(Value::as_str) != Some(FUNCTION)
                || p.get("threadId").and_then(Value::as_str) != Some(thread_id)
                || p.get("turnId").and_then(Value::as_str) != Some(turn_id)
                || p.get("namespace").is_some_and(|n| !n.is_null())
            {
                return Err("unexpected_dynamic_tool");
            }
            if p.get("callId")
                .and_then(Value::as_str)
                .is_none_or(|id| id.is_empty() || id.len() > 200)
            {
                return Err("invalid_call_id");
            }
            metrics.decision_captured = true;
            if serde_json::to_vec(&p["arguments"])
                .map_err(|_| "invalid_arguments")?
                .len()
                > 8192
            {
                return Err("arguments_limit");
            }
            break p.clone();
        }
        if v.get("method").and_then(Value::as_str) == Some("turn/completed") {
            return Err(terminal_without_decision_error(&v));
        }
        check_event(&v, metrics)?;
    };
    // Intentionally leave the dynamic server request unresolved. Returning a tool
    // result would authorize another inference. Interrupt and close instead.
    let interrupt_id = server.send(
        "turn/interrupt",
        json!({"threadId":thread_id,"turnId":turn_id}),
    )?;
    let cleanup_deadline = deadline.min(Instant::now() + Duration::from_secs(3));
    let mut terminal = false;
    while let Ok(v) = server.read(cleanup_deadline) {
        if v.get("id").and_then(Value::as_u64) == Some(interrupt_id) {
            if v.get("error").is_some() {
                break;
            }
            continue;
        }
        if v.get("method").and_then(Value::as_str) == Some("turn/completed") {
            match terminal_without_decision_error(&v) {
                // Interruption is the intentional cleanup outcome after capture.
                "completed_without_decision" | "turn_interrupted" => terminal = true,
                error => return Err(error),
            }
            break;
        }
        check_event(&v, metrics)?;
    }
    if terminal {
        // Closing the process remains the fallback if unsubscribe is blocked.
        match server.rpc(
            "thread/unsubscribe",
            json!({"threadId":thread_id}),
            cleanup_deadline,
            metrics,
        ) {
            Ok(_) => metrics.closure = Some("terminal_unsubscribed_process_closed"),
            Err(
                "unexpected_server_request"
                | "unexpected_tool_item"
                | "unexpected_tool_notification"
                | "model_rerouted",
            ) => {
                return Err("unexpected_cleanup_event");
            }
            Err(_) => metrics.closure = Some("terminal_process_closed"),
        }
    }
    if metrics.closure.is_none() {
        metrics.closure = Some("process_closed_after_unresolved_interrupt");
    }
    // Drop kills and reaps the child before returning. No case is ever replayed.
    Ok(call)
}

fn report_file() -> EvalResult<File> {
    let path =
        PathBuf::from(std::env::var_os("MOOSURI_LIVE_REPORT").ok_or("report_path_required")?);
    if !path.is_absolute() {
        return Err("absolute_report_path_required");
    }
    let parent = path
        .parent()
        .ok_or("report_parent_required")?
        .canonicalize()
        .map_err(|_| "report_parent_missing")?;
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .map_err(|_| "repo_path_missing")?;
    if parent.starts_with(repo) {
        return Err("report_must_be_outside_repository");
    }
    let file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(parent.join(path.file_name().ok_or("report_filename_required")?))
        .map_err(|_| "private_report_create_failed")?;
    if file
        .metadata()
        .map_err(|_| "report_metadata_failed")?
        .mode()
        & 0o777
        != 0o600
    {
        return Err("report_permissions_mismatch");
    }
    Ok(file)
}

#[test]
#[ignore = "requires MOOSURI_LIVE_CHATGPT=1 and an explicitly authorized logged-in ChatGPT account"]
fn live_chatgpt_memory_semantics() {
    assert_eq!(
        std::env::var("MOOSURI_LIVE_CHATGPT").ok().as_deref(),
        Some("1"),
        "live gate required"
    );
    let model = std::env::var("MOOSURI_LIVE_MODEL").expect("MOOSURI_LIVE_MODEL required");
    let effort = std::env::var("MOOSURI_LIVE_EFFORT").expect("MOOSURI_LIVE_EFFORT required");
    assert_eq!(
        effort, "high",
        "this bounded evaluation requires the requested high effort"
    );
    assert!(
        !model.is_empty() && model.len() <= 200 && model.bytes().all(|b| b.is_ascii_graphic()),
        "invalid model"
    );
    let mut report = report_file().unwrap_or_else(|e| panic!("{e}"));
    let corpus: Corpus = serde_json::from_str(include_str!(
        "../../tests/fixtures/memory_intent_semantic.json"
    ))
    .expect("synthetic corpus invalid");
    assert_eq!(corpus.cases.len(), CASE_LIMIT, "bounded corpus changed");
    assert!(
        corpus.cases.iter().all(|c| !c.id.is_empty()
            && c.id.len() <= 80
            && c.id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')),
        "invalid synthetic ID"
    );
    // An empty temporary cwd suppresses project-local instructions without changing
    // HOME, CODEX_HOME, user configuration, or the authenticated account.
    let cwd = tempfile::tempdir().expect("temporary evaluation cwd required");
    let began = Instant::now();
    let overall = began + Duration::from_secs(OVERALL_SECONDS);
    let mut rows = Vec::new();
    let mut blocked = None;
    let mut setup_attempted = 0;
    let mut bypassed = 0;
    let mut result = json!({"schema_version":4,"corpus_kind":"repeated_public_synthetic_not_blind",
        "scope":"label_classification_and_parser_shape_origin_only",
        "corpus_denominator":CASE_LIMIT,"setup_attempted":0,"turn_dispatch_attempted":0,
        "decisions_captured":0,"evaluated":0,"cue_bypassed":0,
        "requested_model":model,"requested_effort":effort,"cli_version":cli_version(),
        "sha256":{"corpus":source_sha256(CORPUS_BYTES),"classifier":source_sha256(CLASSIFIER_BYTES),
            "adapter":source_sha256(ADAPTER_BYTES)},"transport":"official_codex_app_server_stdio_adapter",
        "limitations":["global_instructions_may_remain_count_only",
            "app_server_prompt_scaffolding_differs_from_production_provider",
            "captured_dynamic_call_adapted_to_production_tool_round_decoder",
            "interrupted_before_tool_result_no_followup_inference",
            "usage_may_be_null_after_interrupt","repeated_corpus_not_blind_holdout",
            "synthetic_framing_does_not_measure_provider_eof_quality",
            "fact_subject_target_content_quality_not_evaluated"],
        "complete":false,"report_finished":false,"cases_evaluated":0,"cases_unevaluated":CASE_LIMIT,
        "cases_recorded":0,"cases_remaining":CASE_LIMIT,
        "overall_latency_ms":0,"passed":null,"cases":[]});
    write_checkpoint(&mut report, &result);
    for case in corpus.cases {
        let start = Instant::now();
        let mut row = Row {
            case_id: case.id.clone(),
            outcome: "not_run",
            error: None,
            latency_ms: 0,
            expected: case.expected(),
            raw: None,
            parsed: None,
            metrics: Metrics::default(),
        };
        if !memory_intent::has_memory_cue(&case.current_text) {
            bypassed += 1;
            let routed = Labels {
                decision: DecisionLabel::NoCommand,
                action: None,
                clarification: None,
            };
            row.parsed = Some(routed);
            row.outcome = if routed == row.expected {
                "cue_bypassed_match"
            } else {
                "cue_bypassed_mismatch"
            };
        } else if blocked.is_some() || Instant::now() >= overall {
            row.error = Some(blocked.unwrap_or("not_run_overall_deadline"));
        } else {
            setup_attempted += 1;
            let deadline = overall.min(start + Duration::from_secs(CASE_SECONDS));
            match evaluate(
                &case,
                &model,
                &effort,
                cwd.path(),
                deadline,
                &mut row.metrics,
            ) {
                Ok(call) => {
                    row.raw = serde_json::from_value(call["arguments"].clone()).ok();
                    let base = memory_intent::request(
                        model.clone(),
                        Some(effort.clone()),
                        &case.current_text,
                    )
                    .expect("production request checked");
                    match labels_from_parser(&case.current_text, &base, &call) {
                        Ok(parsed) => {
                            row.parsed = Some(parsed);
                            row.metrics.evaluated = true;
                        }
                        Err(e) => row.error = Some(e),
                    }
                    row.metrics.evaluated |= row.error == Some("parser_rejected");
                    row.outcome = if row.error.is_some_and(apparatus_error) {
                        blocked = Some("not_run_apparatus_blocked");
                        "apparatus_error"
                    } else if row.raw == Some(row.expected) && row.parsed == Some(row.expected) {
                        "match"
                    } else {
                        "semantic_mismatch"
                    };
                    if row.raw.is_none() && row.outcome != "apparatus_error" {
                        row.error = Some("raw_label_invalid");
                    }
                }
                Err(e) => {
                    row.error = Some(e);
                    if model_format_error(e) {
                        row.outcome = "model_format_error";
                        row.metrics.evaluated = true;
                    } else if apparatus_error(e) {
                        row.outcome = "apparatus_error";
                        blocked = Some("not_run_apparatus_blocked");
                    } else {
                        row.outcome = "transport_error";
                        blocked = Some("not_run_transport_blocked");
                    }
                }
            }
        }
        row.latency_ms = start.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        println!(
            "{} {} {}",
            row.case_id,
            row.outcome,
            row.error.unwrap_or("none")
        );
        rows.push(row);
        result["cases"] = serde_json::to_value(&rows).expect("sanitized rows encode failed");
        result["setup_attempted"] = json!(setup_attempted);
        result["turn_dispatch_attempted"] = json!(
            rows.iter()
                .filter(|r| r.metrics.turn_dispatch_attempted)
                .count()
        );
        result["decisions_captured"] =
            json!(rows.iter().filter(|r| r.metrics.decision_captured).count());
        result["evaluated"] = json!(rows.iter().filter(|r| r.metrics.evaluated).count());
        result["cue_bypassed"] = json!(bypassed);
        result["cases_evaluated"] = json!(rows.iter().filter(|r| case_evaluated(r)).count());
        result["cases_unevaluated"] =
            json!(CASE_LIMIT - rows.iter().filter(|r| case_evaluated(r)).count());
        result["complete"] = json!(report_complete(&rows));
        result["cases_recorded"] = json!(rows.len());
        result["cases_remaining"] = json!(CASE_LIMIT - rows.len());
        result["overall_latency_ms"] = json!(began.elapsed().as_millis() as u64);
        write_checkpoint(&mut report, &result);
    }
    let passed = rows
        .iter()
        .all(|r| matches!(r.outcome, "match" | "cue_bypassed_match"));
    result["complete"] = json!(report_complete(&rows));
    result["report_finished"] = json!(true);
    result["passed"] = json!(passed);
    result["overall_latency_ms"] = json!(began.elapsed().as_millis() as u64);
    write_checkpoint(&mut report, &result);
    assert!(
        passed,
        "live evaluation failed; inspect the private sanitized report"
    );
}

// The same create_new, mode-0600 file is durably refreshed before dispatch and
// after every case, so interrupted runs retain sanitized completed-case evidence.
// complete measures evaluated coverage (including cue routing); report_finished
// only means the bounded loop finalized. Recorded-but-blocked rows remain incomplete.
fn write_checkpoint(report: &mut File, result: &Value) {
    report
        .seek(SeekFrom::Start(0))
        .expect("private report seek failed");
    report.set_len(0).expect("private report truncate failed");
    serde_json::to_writer_pretty(&mut *report, result).expect("private report write failed");
    report
        .write_all(b"\n")
        .expect("private report write failed");
    report.sync_all().expect("private report sync failed");
}

fn source_sha256(bytes: &[u8]) -> String {
    let mut child = Command::new("/usr/bin/shasum")
        .args(["-a", "256"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("source hash command unavailable");
    child
        .stdin
        .take()
        .expect("source hash stdin missing")
        .write_all(bytes)
        .expect("source hash input failed");
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                let mut output = Vec::new();
                child
                    .stdout
                    .take()
                    .expect("source hash output missing")
                    .take(100)
                    .read_to_end(&mut output)
                    .expect("source hash read failed");
                let hash = std::str::from_utf8(&output)
                    .expect("source hash invalid")
                    .split_whitespace()
                    .next()
                    .expect("source hash missing");
                assert!(
                    hash.len() == 64 && hash.bytes().all(|b| b.is_ascii_hexdigit()),
                    "source hash invalid"
                );
                return hash.to_owned();
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                panic!("source hash failed");
            }
        }
    }
}

fn cli_version() -> Option<String> {
    // The app-server initialize response is not used here because it may contain
    // local paths. Only a strictly shaped numeric CLI version is retained.
    let mut child = Command::new("codex")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                if !status.success() {
                    return None;
                }
                let mut bytes = Vec::new();
                child
                    .stdout
                    .take()?
                    .take(100)
                    .read_to_end(&mut bytes)
                    .ok()?;
                let text = std::str::from_utf8(&bytes).ok()?.trim();
                let version = text.strip_prefix("codex-cli ")?;
                return (version.len() <= 64
                    && version
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b".-+".contains(&b)))
                .then(|| text.to_owned());
            }
            Ok(None) if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
}
