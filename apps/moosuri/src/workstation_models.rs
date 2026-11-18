//! On-demand workstation model access. No daemon management, discovery scans or account login.
use axum::{
    body::{Body, to_bytes},
    extract::{Request, State},
    http::{Method, StatusCode, header},
    response::{IntoResponse, Response},
};
use clap::Subcommand;
use rand::{Rng, distributions::Alphanumeric};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    future::IntoFuture,
    io::{IsTerminal, Write},
    net::SocketAddr,
    process::{Child, Command as Process, Stdio},
    sync::Arc,
    time::Duration,
};

const REQUEST_LIMIT: usize = 1024 * 1024;
const RESPONSE_LIMIT: usize = 8 * 1024 * 1024;

#[derive(Subcommand)]
pub enum Command {
    /// List exact loopback Ollama/LM Studio models; optionally connect a selected model until Ctrl+C.
    Workstation {
        /// Only list installed model IDs. Never invokes inference.
        #[arg(long)]
        list: bool,
        /// Existing OpenSSH alias of YOUR trusted Moosuri Linux server (admin account only).
        #[arg(long, conflicts_with = "list")]
        ssh_host: Option<String>,
    },
}

#[derive(Clone, Debug)]
struct InstalledModel {
    application: &'static str,
    base_url: String,
    model: String,
}

fn client(timeout: Duration) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(2))
        .timeout(timeout)
        .build()
        .map_err(|_| "workstation_http_unavailable".into())
}

async fn bounded_response(mut response: reqwest::Response) -> Result<Vec<u8>, ()> {
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
        if bytes.len().saturating_add(chunk.len()) > RESPONSE_LIMIT {
            return Err(());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

async fn discover_at(application: &'static str, base_url: &str) -> Vec<InstalledModel> {
    let Ok(client) = client(Duration::from_secs(4)) else {
        return vec![];
    };
    let Ok(response) = client.get(format!("{base_url}/models")).send().await else {
        return vec![];
    };
    if !response.status().is_success() {
        return vec![];
    }
    let Ok(bytes) = bounded_response(response).await else {
        return vec![];
    };
    #[derive(Deserialize)]
    struct Catalog {
        data: Vec<Model>,
    }
    #[derive(Deserialize)]
    struct Model {
        id: String,
    }
    let Ok(catalog) = serde_json::from_slice::<Catalog>(&bytes) else {
        return vec![];
    };
    let mut models = Vec::new();
    for model in catalog.data.into_iter().take(512) {
        if model.id.is_empty() || model.id.len() > 256 || model.id.chars().any(char::is_control) {
            continue;
        }
        if !models.iter().any(|m: &InstalledModel| m.model == model.id) {
            models.push(InstalledModel {
                application,
                base_url: base_url.to_owned(),
                model: model.id,
            });
        }
    }
    models
}

pub async fn run(command: &Command) -> Result<(), String> {
    let Command::Workstation { list, ssh_host } = command;
    // These exact endpoints are the only discovery targets. Never follows redirects or proxies.
    let (mut models, lm_studio) = tokio::join!(
        discover_at("Ollama", "http://127.0.0.1:11434/v1"),
        discover_at("LM Studio", "http://127.0.0.1:1234/v1")
    );
    models.extend(lm_studio);
    for (index, model) in models.iter().enumerate() {
        println!("{}. {} — {}", index + 1, model.application, model.model);
    }
    if models.is_empty() {
        println!(
            "No installed models found. Start Ollama or LM Studio's local server on this computer, then try again."
        );
        return Ok(());
    }
    if *list {
        return Ok(());
    }
    let Some(host) = ssh_host else {
        println!(
            "For Moosuri on this computer, use Settings → local model discovery. For Moosuri on another Linux server, run this again with --ssh-host YOUR_EXISTING_SSH_ALIAS (your administrator account only)."
        );
        return Ok(());
    };
    if !std::io::stdin().is_terminal() {
        return Err("workstation_interactive_terminal_required".into());
    }
    let choice = prompt("Select one model number (blank cancels): ")?;
    if choice.is_empty() {
        return Ok(());
    }
    let index: usize = choice
        .parse()
        .map_err(|_| "workstation_invalid_selection")?;
    let selected = models
        .get(index.wrapping_sub(1))
        .ok_or("workstation_invalid_selection")?
        .clone();
    println!(
        "This temporary connection permits model listing and chat for only the selected model. Chat starts only when you request it in Moosuri. Keep this terminal open; Ctrl+C revokes access."
    );
    if ssh_host.is_some() {
        println!(
            "Remote mode is for your own Moosuri administrator account. The trusted server operator can access traffic and the private connection credential. Other Moosuri members receive no grant. An existing SSH alias with a verified host key is required."
        );
    }
    if prompt("Create this temporary connection? Type yes: ")? != "yes" {
        return Ok(());
    }
    let mut shutdown = Shutdown::new()?;
    let token: String = rand::thread_rng()
        .sample_iter(&Alphanumeric)
        .take(64)
        .map(char::from)
        .collect();
    let state = Arc::new(ProxyState {
        selected: selected.clone(),
        token: token.clone(),
        client: client(Duration::from_secs(180))?,
    });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .map_err(|_| "workstation_bind_failed")?;
    let local = listener
        .local_addr()
        .map_err(|_| "workstation_bind_failed")?;
    let port = if selected.application == "Ollama" {
        11434
    } else {
        1234
    };
    let mut tunnel = SshTunnel::open(host, port, local.port()).await?;
    let private = tempfile::Builder::new()
        .prefix("moosuri-workstation-")
        .tempdir()
        .map_err(|_| "workstation_private_file_failed")?;
    let path = private.path().join("connection.json");
    let config = json!({
        "format": "moosuri-workstation-model-v1", "name": format!("Workstation {}", selected.application),
        "base_url": format!("http://127.0.0.1:{port}/v1"), "model": selected.model,
        "credential": { "action": "replace", "value": token },
        "temporary": true, "remote_admin_only": true
    });
    write_private(
        &path,
        &serde_json::to_vec_pretty(&config).map_err(|_| "workstation_private_file_failed")?,
    )?;
    println!(
        "In your signed-in Moosuri Settings, import this private connection file and explicitly save:\n{}",
        path.display()
    );
    println!(
        "This file contains a temporary credential. Do not upload it elsewhere. Remove the saved connection after stopping. Ctrl+C closes access and deletes this file."
    );
    let app = axum::Router::new().fallback(proxy).with_state(state);
    let serve = axum::serve(listener, app).into_future();
    tokio::pin!(serve);
    loop {
        tokio::select! {
            result = &mut serve => {
                result.map_err(|_| "workstation_proxy_failed")?;
                break;
            },
            _ = shutdown.wait() => break,
            _ = tokio::time::sleep(Duration::from_secs(1)) => {
                if !tunnel.alive() { break; }
            }
        }
    }
    // Drop closes the exact owned SSH process, then removes the credential and control socket.
    println!("Workstation connection stopped.");
    Ok(())
}

#[cfg(unix)]
struct Shutdown {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
    hangup: tokio::signal::unix::Signal,
}
#[cfg(unix)]
impl Shutdown {
    fn new() -> Result<Self, String> {
        use tokio::signal::unix::{SignalKind, signal};
        Ok(Self {
            interrupt: signal(SignalKind::interrupt()).map_err(|_| "workstation_signal_failed")?,
            terminate: signal(SignalKind::terminate()).map_err(|_| "workstation_signal_failed")?,
            hangup: signal(SignalKind::hangup()).map_err(|_| "workstation_signal_failed")?,
        })
    }
    async fn wait(&mut self) {
        tokio::select! {
            _ = self.interrupt.recv() => (),
            _ = self.terminate.recv() => (),
            _ = self.hangup.recv() => (),
        }
    }
}
#[cfg(not(unix))]
struct Shutdown;
#[cfg(not(unix))]
impl Shutdown {
    fn new() -> Result<Self, String> {
        Ok(Self)
    }
    async fn wait(&mut self) {
        let _ = tokio::signal::ctrl_c().await;
    }
}

fn prompt(message: &str) -> Result<String, String> {
    print!("{message}");
    std::io::stdout()
        .flush()
        .map_err(|_| "workstation_input_failed")?;
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(|_| "workstation_input_failed")?;
    Ok(answer.trim().to_owned())
}

fn write_private(path: &std::path::Path, data: &[u8]) -> Result<(), String> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .and_then(|mut f| f.write_all(data))
        .map_err(|_| "workstation_private_file_failed".into())
}

struct ProxyState {
    selected: InstalledModel,
    token: String,
    client: reqwest::Client,
}

async fn proxy(State(state): State<Arc<ProxyState>>, request: Request) -> Response {
    // CORS is deliberately absent: a browser page cannot operate this daemon.
    let expected = format!("Bearer {}", state.token);
    if request
        .headers()
        .get(header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        != Some(expected.as_str())
    {
        return StatusCode::UNAUTHORIZED.into_response();
    }
    if request.uri().query().is_some() {
        return StatusCode::NOT_FOUND.into_response();
    }
    match (request.method(), request.uri().path()) {
        (&Method::GET, "/v1/models") => axum::Json(json!({"object":"list", "data":[{"id":state.selected.model,"object":"model","owned_by":"workstation"}]})).into_response(),
        (&Method::POST, "/v1/chat/completions") => {
            let bytes = match tokio::time::timeout(Duration::from_secs(10), to_bytes(request.into_body(), REQUEST_LIMIT)).await {
                Ok(Ok(bytes)) => bytes,
                Ok(Err(_)) => return StatusCode::PAYLOAD_TOO_LARGE.into_response(),
                Err(_) => return StatusCode::REQUEST_TIMEOUT.into_response(),
            };
            let Ok(value) = serde_json::from_slice::<Value>(&bytes) else { return StatusCode::BAD_REQUEST.into_response() };
            if value.get("model").and_then(Value::as_str) != Some(state.selected.model.as_str()) {
                return StatusCode::FORBIDDEN.into_response();
            }
            if !text_chat(&value) { return StatusCode::BAD_REQUEST.into_response(); }
            // No request headers are forwarded (especially the temporary bearer).
            let Ok(response) = state.client.post(format!("{}/chat/completions", state.selected.base_url)).json(&value).send().await else {
                return StatusCode::BAD_GATEWAY.into_response();
            };
            let status = response.status();
            let content_type = response.headers().get(header::CONTENT_TYPE).cloned();
            let chunks = futures_util::stream::try_unfold((response, 0usize), |(mut upstream, count)| async move {
                let chunk = upstream.chunk().await.map_err(|_| std::io::Error::other("workstation_upstream_failed"))?;
                match chunk {
                    Some(bytes) if count.saturating_add(bytes.len()) <= RESPONSE_LIMIT => {
                        let count = count + bytes.len();
                        Ok(Some((bytes, (upstream, count))))
                    },
                    Some(_) => Err(std::io::Error::other("workstation_response_too_large")),
                    None => Ok(None),
                }
            });
            let mut response = (status, Body::from_stream(chunks)).into_response();
            if let Some(content_type) = content_type { response.headers_mut().insert(header::CONTENT_TYPE, content_type); }
            response
        }
        _ => StatusCode::NOT_FOUND.into_response(),
    }
}

// Only ordinary text messages and declarative function tools. In particular no LM Studio
// integrations, external media URLs, daemon administration, alternate endpoint or MCP tools.
fn text_chat(value: &Value) -> bool {
    let Some(object) = value.as_object() else {
        return false;
    };
    if object.keys().any(|k| {
        ![
            "model",
            "messages",
            "stream",
            "n",
            "max_tokens",
            "max_completion_tokens",
            "temperature",
            "top_p",
            "reasoning_effort",
            "tools",
            "tool_choice",
            "parallel_tool_calls",
            "stream_options",
        ]
        .contains(&k.as_str())
    }) {
        return false;
    }
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return false;
    };
    if messages.is_empty() || messages.len() > 1024 {
        return false;
    }
    for message in messages {
        let Some(message) = message.as_object() else {
            return false;
        };
        if message.keys().any(|k| {
            ![
                "role",
                "content",
                "name",
                "tool_calls",
                "tool_call_id",
                "phase",
            ]
            .contains(&k.as_str())
        }) {
            return false;
        }
        if !matches!(
            message.get("role").and_then(Value::as_str),
            Some("system" | "developer" | "user" | "assistant" | "tool")
        ) {
            return false;
        }
        match message.get("content") {
            Some(Value::String(_) | Value::Null) | None => (),
            Some(Value::Array(blocks))
                if blocks.iter().all(|block| {
                    block.as_object().is_some_and(|b| {
                        b.len() == 2
                            && b.get("type").and_then(Value::as_str) == Some("text")
                            && b.get("text").is_some_and(Value::is_string)
                    })
                }) =>
            {
                ()
            }
            _ => return false,
        }
        if let Some(calls) = message.get("tool_calls") {
            let Some(calls) = calls.as_array() else {
                return false;
            };
            if !calls.iter().all(|call| {
                call.get("type").and_then(Value::as_str) == Some("function")
                    && call
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .is_some_and(Value::is_string)
            }) {
                return false;
            }
        }
    }
    if let Some(tools) = value.get("tools") {
        let Some(tools) = tools.as_array() else {
            return false;
        };
        if !tools.iter().all(|t| {
            t.as_object().is_some_and(|t| {
                t.len() == 2
                    && t.get("type").and_then(Value::as_str) == Some("function")
                    && t.get("function").is_some_and(Value::is_object)
            })
        }) {
            return false;
        }
    }
    if let Some(choice) = value.get("tool_choice") {
        if !matches!(choice.as_str(), Some("auto" | "none" | "required"))
            && !choice.as_object().is_some_and(|o| {
                o.len() == 2
                    && o.get("type").and_then(Value::as_str) == Some("function")
                    && o.get("function")
                        .and_then(Value::as_object)
                        .is_some_and(|f| {
                            f.len() == 1 && f.get("name").is_some_and(Value::is_string)
                        })
            })
        {
            return false;
        }
    }
    if let Some(options) = value.get("stream_options") {
        if !options
            .as_object()
            .is_some_and(|o| o.len() == 1 && o.get("include_usage").is_some_and(Value::is_boolean))
        {
            return false;
        }
    }
    // JSON schemas are text data; remote schema references are unnecessary here.
    fn local_refs(value: &Value) -> bool {
        match value {
            Value::Object(o) => o.iter().all(|(k, v)| {
                (k != "$ref" || v.as_str().is_some_and(|s| s.starts_with('#'))) && local_refs(v)
            }),
            Value::Array(a) => a.iter().all(local_refs),
            _ => true,
        }
    }
    local_refs(value)
}

struct SshTunnel {
    child: Child,
    _private: tempfile::TempDir,
}
impl SshTunnel {
    async fn open(host: &str, remote_port: u16, local_port: u16) -> Result<Self, String> {
        if !valid_ssh_alias(host) || !(1024..=65535).contains(&remote_port) {
            return Err("workstation_invalid_ssh_target".into());
        }
        let private = tempfile::Builder::new()
            .prefix("moosuri-ssh-")
            .tempdir()
            .map_err(|_| "workstation_ssh_failed")?;
        let control = private.path().join("control");
        let mut process = ssh(&control);
        process
            .args([
                "-M",
                "-N",
                "-T",
                "-o",
                "ControlPersist=no",
                "-o",
                "ClearAllForwardings=yes",
                "-o",
                "ServerAliveInterval=15",
                "-o",
                "ServerAliveCountMax=2",
            ])
            .arg(host);
        let child = process.spawn().map_err(|_| "workstation_ssh_unavailable")?;
        let mut tunnel = Self {
            child,
            _private: private,
        };
        let mut ready = false;
        for _ in 0..100 {
            if !tunnel.alive() {
                return Err("workstation_ssh_failed".into());
            }
            if control.exists() {
                ready = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if !ready {
            return Err("workstation_ssh_timeout".into());
        }
        let forward = format!("127.0.0.1:{remote_port}:127.0.0.1:{local_port}");
        if !ssh(&control)
            .args([
                "-O",
                "forward",
                "-o",
                "ExitOnForwardFailure=yes",
                "-R",
                &forward,
            ])
            .arg(host)
            .status()
            .map_err(|_| "workstation_ssh_failed")?
            .success()
        {
            return Err("workstation_ssh_forward_failed_port_must_be_unused".into());
        }
        // sshd GatewayPorts=yes may override the requested loopback binding. Fail closed.
        // Fixed command plus validated integer; no host input enters a remote shell command.
        let inspect = format!("ss -H -ltn 'sport = :{remote_port}'");
        let output = ssh(&control)
            .args(["-o", "ClearAllForwardings=yes"])
            .arg(host)
            .arg(inspect)
            .stdout(Stdio::piped())
            .output()
            .map_err(|_| "workstation_ssh_binding_unverified")?;
        if !tunnel.alive()
            || !output.status.success()
            || !loopback_bindings(&output.stdout, remote_port)
        {
            return Err(
                "workstation_ssh_binding_unverified_require_loopback_gatewayports_no".into(),
            );
        }
        Ok(tunnel)
    }
    fn alive(&mut self) -> bool {
        matches!(self.child.try_wait(), Ok(None))
    }
}
impl Drop for SshTunnel {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
fn valid_ssh_alias(host: &str) -> bool {
    !host.is_empty()
        && host.len() <= 253
        && !host.starts_with('-')
        && host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"._-".contains(&b))
}
fn ssh(control: &std::path::Path) -> Process {
    let mut command = Process::new("ssh");
    command
        .args([
            "-o",
            "BatchMode=yes",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "ConnectTimeout=8",
            "-o",
            "LogLevel=ERROR",
            "-o",
            "PermitLocalCommand=no",
            "-S",
        ])
        .arg(control)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    command
}
fn loopback_bindings(bytes: &[u8], port: u16) -> bool {
    let Ok(text) = std::str::from_utf8(bytes) else {
        return false;
    };
    let mut count = 0;
    for line in text.lines().filter(|l| !l.trim().is_empty()) {
        let Some(address) = line.split_whitespace().nth(3) else {
            return false;
        };
        let Ok(address) = address.parse::<SocketAddr>() else {
            return false;
        };
        if !address.ip().is_loopback() || address.port() != port {
            return false;
        }
        count += 1;
    }
    count > 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use tower::ServiceExt;

    fn state(base_url: String) -> Arc<ProxyState> {
        Arc::new(ProxyState {
            selected: InstalledModel {
                application: "Ollama",
                base_url,
                model: "fixture-installed".into(),
            },
            token: "synthetic-private-bearer".into(),
            client: client(Duration::from_secs(3)).unwrap(),
        })
    }
    fn request(method: &str, path: &str, auth: bool, body: Value) -> Request {
        let mut request = Request::builder().method(method).uri(path);
        if auth {
            request = request.header(header::AUTHORIZATION, "Bearer synthetic-private-bearer");
        }
        request.body(Body::from(body.to_string())).unwrap()
    }
    #[tokio::test]
    async fn authenticates_and_exposes_only_selected_model_and_routes() {
        let app = axum::Router::new()
            .fallback(proxy)
            .with_state(state("http://127.0.0.1:1/v1".into()));
        assert_eq!(
            app.clone()
                .oneshot(request("GET", "/v1/models", false, Value::Null))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        for path in [
            "/api/delete",
            "/v1/models/other",
            "/v1/models?other=true",
            "/v1/embeddings",
        ] {
            assert_eq!(
                app.clone()
                    .oneshot(request("GET", path, true, Value::Null))
                    .await
                    .unwrap()
                    .status(),
                StatusCode::NOT_FOUND
            );
        }
        let response = app
            .clone()
            .oneshot(request("GET", "/v1/models", true, Value::Null))
            .await
            .unwrap();
        let body: Value =
            serde_json::from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        assert_eq!(body["data"].as_array().unwrap().len(), 1);
        assert_eq!(body["data"][0]["id"], "fixture-installed");
        assert_eq!(
            app.clone()
                .oneshot(request(
                    "POST",
                    "/v1/chat/completions",
                    true,
                    json!({"model":"other"})
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(app.oneshot(request("POST", "/v1/chat/completions", true, json!({"model":"fixture-installed", "messages":[{"role":"user","content":[{"type":"image_url","image_url":{"url":"http://127.0.0.1/private"}}]}]}))).await.unwrap().status(), StatusCode::BAD_REQUEST);
    }
    #[tokio::test]
    async fn discovers_installed_catalog_and_forwards_only_text_without_bearer() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = axum::Router::new()
            .route("/v1/models", axum::routing::get(|| async { axum::Json(json!({"data":[{"id":"fixture-installed"},{"id":"fixture-installed"},{"id":"bad\nterminal"}]})) }))
            .route("/v1/chat/completions", axum::routing::post(|request: Request| async {
                assert!(!request.headers().contains_key(header::AUTHORIZATION));
                assert!(!request.headers().contains_key("cookie"));
                let body: Value = serde_json::from_slice(&to_bytes(request.into_body(), REQUEST_LIMIT).await.unwrap()).unwrap();
                assert_eq!(body["model"], "fixture-installed");
                ([(header::CONTENT_TYPE, "text/event-stream")], "data: synthetic\n\ndata: [DONE]\n\n")
            }));
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let models = discover_at("Ollama", &base).await;
        assert_eq!(models.len(), 1);
        let app = axum::Router::new().fallback(proxy).with_state(state(base));
        let mut req = request(
            "POST",
            "/v1/chat/completions",
            true,
            json!({"model":"fixture-installed","stream":true,"messages":[{"role":"user","content":"synthetic"}]}),
        );
        req.headers_mut()
            .insert(header::COOKIE, "private-cookie".parse().unwrap());
        let response = app.oneshot(req).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()[header::CONTENT_TYPE],
            "text/event-stream"
        );
        assert_eq!(
            to_bytes(response.into_body(), 1024).await.unwrap(),
            "data: synthetic\n\ndata: [DONE]\n\n"
        );
        server.abort();
    }
    #[tokio::test]
    async fn does_not_follow_catalog_redirects() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let base = format!("http://{}/v1", listener.local_addr().unwrap());
        let app = axum::Router::new().fallback(|| async {
            (
                StatusCode::FOUND,
                [(header::LOCATION, "http://192.0.2.1/private")],
            )
        });
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        assert!(discover_at("Ollama", &base).await.is_empty());
        server.abort();
    }
    #[test]
    fn accepts_only_text_and_declarative_function_tools() {
        let mut request = json!({"model":"fixture-installed","messages":[{"role":"user","content":"hello"}],"tools":[{"type":"function","function":{"name":"read","parameters":{"type":"object"}}}]});
        assert!(text_chat(&request));
        request["tools"][0] = json!({"type":"mcp", "server_url":"http://127.0.0.1/private"});
        assert!(!text_chat(&request));
        request.as_object_mut().unwrap().remove("tools");
        request["integrations"] = json!(["unapproved"]);
        assert!(!text_chat(&request));
    }
    #[test]
    fn rejects_wildcard_or_unknown_remote_bindings_and_shell_aliases() {
        assert!(loopback_bindings(
            b"LISTEN 0 128 127.0.0.1:11434 0.0.0.0:*\n",
            11434
        ));
        assert!(loopback_bindings(
            b"LISTEN 0 128 [::1]:11434 [::]:*\n",
            11434
        ));
        for output in [
            "",
            "LISTEN 0 128 0.0.0.0:11434 0.0.0.0:*\n",
            "LISTEN 0 128 [::]:11434 [::]:*\n",
            "LISTEN 0 128 127.0.0.1:1234 0.0.0.0:*\n",
            "unexpected",
        ] {
            assert!(!loopback_bindings(output.as_bytes(), 11434));
        }
        assert!(valid_ssh_alias("my-moosuri-server"));
        for host in [
            "-oProxyCommand=x",
            "host;touch /tmp/no",
            "user@host",
            "host\nother",
            "",
        ] {
            assert!(!valid_ssh_alias(host));
        }
    }
    #[cfg(unix)]
    #[test]
    fn private_connection_file_cannot_overwrite_and_is_owner_only() {
        use std::os::unix::fs::PermissionsExt;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("connection.json");
        write_private(&path, b"synthetic").unwrap();
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert!(write_private(&path, b"replacement").is_err());
        assert_eq!(std::fs::read(&path).unwrap(), b"synthetic");
    }
}
