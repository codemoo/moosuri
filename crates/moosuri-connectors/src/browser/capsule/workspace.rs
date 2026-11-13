//! A task-owned, bounded browser process. Frames and input belong to human UI only.
use super::*;
use base64::Engine;
use tokio::sync::{mpsc, oneshot};

pub const WORKSPACE_IDLE_SECONDS: u64 = 30 * 60;
pub const WORKSPACE_MAX_SECONDS: u64 = 8 * 60 * 60;
const MAX_FRAME_RESPONSE: usize = 3 * 1024 * 1024 + 4096;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceObservation {
    pub page: RenderedPage,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceFrame {
    pub png_base64: String,
    pub width: u32,
    pub height: u32,
    pub url: String,
    pub frame_token: String,
}
/// Human UI input only. Never expose this API as an agent/model tool.
/// Debug is intentionally absent because InsertText can contain a secret.
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum WorkspaceInput {
    Click { x: u32, y: u32 },
    InsertText { text: String },
    Key { key: WorkspaceKey },
    Scroll { delta_x: i32, delta_y: i32 },
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkspaceKey {
    Enter,
    Tab,
    Backspace,
    Escape,
    ArrowLeft,
    ArrowRight,
    ArrowUp,
    ArrowDown,
    Delete,
    Home,
    End,
    PageUp,
    PageDown,
    Space,
}
impl WorkspaceInput {
    pub fn validate(&self) -> Result<(), BrowserError> {
        let valid = match self {
            Self::Click { x, y } => *x < 1280 && *y < 900,
            Self::InsertText { text } => {
                !text.is_empty()
                    && text.len() <= 4096
                    && !text
                        .chars()
                        .any(|c| c.is_control() && c != '\n' && c != '\t')
            }
            Self::Key { .. } => true,
            Self::Scroll { delta_x, delta_y } => {
                (-2000..=2000).contains(delta_x)
                    && (-2000..=2000).contains(delta_y)
                    && (*delta_x != 0 || *delta_y != 0)
            }
        };
        if valid {
            Ok(())
        } else {
            Err(BrowserError::InvalidHandle)
        }
    }
}
struct Request {
    body: Value,
    limit: usize,
    reply: oneshot::Sender<Result<Value, BrowserError>>,
}
/// Single mutable controller; the broker owns it, never an HTTP request/model turn.
/// Dropping it or any pending operation cancels the independent cleanup owner.
pub struct CapsuleWorkspace {
    requests: mpsc::Sender<Request>,
    cancel: CancellationToken,
    done: Option<oneshot::Receiver<Result<(), BrowserError>>>,
}
struct Pending(Option<CancellationToken>);
impl Drop for Pending {
    fn drop(&mut self) {
        if let Some(cancel) = self.0.take() {
            cancel.cancel();
        }
    }
}
impl CapsuleReader {
    pub async fn start_workspace(
        &self,
        account: Uuid,
        cancel: &CancellationToken,
    ) -> Result<CapsuleWorkspace, BrowserError> {
        if account.is_nil() {
            return Err(BrowserError::IsolationRequired);
        }
        if cancel.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        let capacity = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| BrowserError::Capacity)?;
        let cancel = cancel.child_token();
        let mut pending = Pending(Some(cancel.clone()));
        let (requests, rx) = mpsc::channel(1);
        let (started, startup) = oneshot::channel();
        let (finished, done) = oneshot::channel();
        let this = self.clone();
        let actor_cancel = cancel.clone();
        tokio::spawn(async move {
            let _capacity = capacity;
            let result = this
                .workspace_actor(account, actor_cancel, rx, started)
                .await;
            let _ = finished.send(result);
        });
        startup.await.map_err(|_| BrowserError::Setup)??;
        pending.0 = None;
        Ok(CapsuleWorkspace {
            requests,
            cancel,
            done: Some(done),
        })
    }
    async fn workspace_actor(
        &self,
        account: Uuid,
        cancel: CancellationToken,
        mut requests: mpsc::Receiver<Request>,
        started: oneshot::Sender<Result<(), BrowserError>>,
    ) -> Result<(), BrowserError> {
        let setup = async {
            #[cfg(test)]
            let fixture = self.fixture.is_some();
            #[cfg(not(test))]
            let fixture = false;
            if !fixture {
                if !cfg!(target_os = "linux") {
                    return Err(BrowserError::IsolationRequired);
                }
                self.config.validate()?;
                self.validate_resources(&cancel).await?;
                empty_docker_config(Path::new(DOCKER_CONFIG))?;
            }
            Ok(())
        }
        .await;
        if let Err(error) = setup {
            let _ = started.send(Err(error));
            return Ok(());
        }
        let identity = Uuid::new_v4();
        let name = format!("moosuri-browser-workspace-{identity}");
        let mut command = self.worker_command(account, identity, &name);
        command
            .arg("--workspace")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = match command.spawn() {
            Ok(child) => child,
            Err(_) => {
                let _ = started.send(Err(BrowserError::Setup));
                return Ok(());
            }
        };
        let mut creation_observed = false;
        let startup = async {
            let stdout = child.stdout.take().ok_or(BrowserError::Setup)?;
            let mut reader = BufReader::new(stdout);
            let ready = read_line(&mut reader, 2048).await?;
            if ready["ready"] != true
                || ready["workspace"] != true
                || ready["tenant"] != account.to_string()
                || ready["javascript_enabled"] != self.config.javascript_enabled
            {
                return Err(BrowserError::Protocol);
            }
            Ok(reader)
        };
        let ready = tokio::select! { biased;
            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
            result = tokio::time::timeout(Duration::from_secs(25), startup) => result.unwrap_or(Err(BrowserError::Timeout)),
        };
        let mut start_failure = None;
        match ready {
            Err(error) => start_failure = Some((started, error)),
            Ok(mut reader) => {
                creation_observed = true;
                if started.send(Ok(())).is_ok() {
                    let maximum =
                        tokio::time::Instant::now() + Duration::from_secs(WORKSPACE_MAX_SECONDS);
                    loop {
                        let idle = tokio::time::sleep(Duration::from_secs(WORKSPACE_IDLE_SECONDS));
                        let request = tokio::select! { biased;
                            _ = cancel.cancelled() => break,
                            _ = tokio::time::sleep_until(maximum) => break,
                            _ = idle => break,
                            request = requests.recv() => match request { Some(r) => r, None => break },
                        };
                        // A disconnected caller has no authority to dispatch a delayed action.
                        if request.reply.is_closed() {
                            break;
                        }
                        let operation = async {
                            let mut bytes = serde_json::to_vec(&request.body)
                                .map_err(|_| BrowserError::Protocol)?;
                            if bytes.len() > 16 * 1024 {
                                return Err(BrowserError::OutputTooLarge);
                            }
                            bytes.push(b'\n');
                            child
                                .stdin
                                .as_mut()
                                .ok_or(BrowserError::Protocol)?
                                .write_all(&bytes)
                                .await
                                .map_err(|_| BrowserError::Protocol)?;
                            child
                                .stdin
                                .as_mut()
                                .ok_or(BrowserError::Protocol)?
                                .flush()
                                .await
                                .map_err(|_| BrowserError::Protocol)?;
                            let response = read_line(&mut reader, request.limit).await?;
                            if response["ok"] == true {
                                response
                                    .get("result")
                                    .cloned()
                                    .ok_or(BrowserError::Protocol)
                            } else {
                                Err(match response["code"].as_str() {
                                    Some("stale_page") => BrowserError::StalePage,
                                    Some("invalid_handle") => BrowserError::InvalidHandle,
                                    Some("invalid_url") => BrowserError::InvalidUrl,
                                    Some("forbidden_destination") => {
                                        BrowserError::ForbiddenDestination
                                    }
                                    Some("output_too_large") => BrowserError::OutputTooLarge,
                                    _ => BrowserError::Protocol,
                                })
                            }
                        };
                        let result = tokio::select! { biased;
                            _ = cancel.cancelled() => Err(BrowserError::Cancelled),
                            _ = tokio::time::sleep_until(maximum) => Err(BrowserError::Expired),
                            result = tokio::time::timeout(Duration::from_secs(40), operation) => result.unwrap_or(Err(BrowserError::Timeout)),
                        };
                        let poisoned = matches!(&result, Err(e) if !matches!(e, BrowserError::StalePage | BrowserError::InvalidHandle | BrowserError::InvalidUrl | BrowserError::ForbiddenDestination));
                        if request.reply.send(result).is_err() || poisoned {
                            break;
                        }
                    }
                }
            }
        }
        let cleanup = self
            .cleanup(&mut child, &name, identity, creation_observed)
            .await;
        #[cfg(test)]
        if let Some(f) = &self.fixture {
            f.cleaned.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        }
        if let Some((started, error)) = start_failure {
            let _ = started.send(Err(cleanup.err().unwrap_or(error)));
            return Ok(());
        }
        cleanup
    }
}
impl CapsuleWorkspace {
    /// True once cancellation or actor termination makes further dispatch impossible.
    pub fn is_closed(&self) -> bool {
        self.cancel.is_cancelled() || self.requests.is_closed()
    }
    async fn request(&mut self, body: Value, limit: usize) -> Result<Value, BrowserError> {
        if self.cancel.is_cancelled() {
            return Err(BrowserError::Cancelled);
        }
        let mut pending = Pending(Some(self.cancel.clone()));
        let (reply, response) = oneshot::channel();
        self.requests
            .send(Request { body, limit, reply })
            .await
            .map_err(|_| BrowserError::Expired)?;
        let result = tokio::select! { biased;
            _ = self.cancel.cancelled() => return Err(BrowserError::Cancelled),
            result = response => result.map_err(|_| BrowserError::Expired)?,
        };
        pending.0 = None;
        result
    }
    fn checked<T>(&self, result: Result<T, BrowserError>) -> Result<T, BrowserError> {
        if result.is_err() {
            self.cancel.cancel();
        }
        result
    }
    pub async fn open(&mut self, url: &str) -> Result<WorkspaceObservation, BrowserError> {
        let url = public_url(url)?.to_string();
        let value = self
            .request(json!({"op":"workspace_open","url":url}), MAX_RESPONSE)
            .await?;
        self.checked(observation(value))
    }
    pub async fn observe(&mut self) -> Result<WorkspaceObservation, BrowserError> {
        let value = self
            .request(json!({"op":"workspace_observe"}), MAX_RESPONSE)
            .await?;
        self.checked(observation(value))
    }
    /// Human-only screenshot. Never include it in model context or durable chat.
    pub async fn frame(&mut self) -> Result<WorkspaceFrame, BrowserError> {
        let value = self
            .request(json!({"op":"workspace_frame"}), MAX_FRAME_RESPONSE)
            .await?;
        self.checked(frame(value))
    }
    /// Acknowledgment only: typing must never implicitly expose a page observation.
    pub async fn input(
        &mut self,
        expected_frame_token: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError> {
        input.validate()?;
        if !hex64(expected_frame_token) {
            return Err(BrowserError::StalePage);
        }
        let result = self.request(json!({"op":"workspace_input","expected_frame_token":expected_frame_token,"input":input}), 2048).await?;
        self.checked(if result == json!({"accepted":true}) {
            Ok(())
        } else {
            Err(BrowserError::Protocol)
        })
    }
    pub async fn shutdown(mut self) -> Result<(), BrowserError> {
        self.cancel.cancel();
        self.done
            .take()
            .ok_or(BrowserError::Setup)?
            .await
            .map_err(|_| BrowserError::Setup)?
    }
}
impl Drop for CapsuleWorkspace {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}
fn observation(value: Value) -> Result<WorkspaceObservation, BrowserError> {
    let result: WorkspaceObservation =
        serde_json::from_value(value).map_err(|_| BrowserError::Protocol)?;
    let page = &result.page;
    public_url(&page.url)?;
    if page.text.len() > 8192
        || page.title.len() > 512
        || page.links.len() > 4
        || page.warning.len() > 512
    {
        return Err(BrowserError::OutputTooLarge);
    }
    for link in &page.links {
        public_url(&link.url)?;
        if link.text.len() > 512 {
            return Err(BrowserError::OutputTooLarge);
        }
    }
    Ok(result)
}
fn frame(value: Value) -> Result<WorkspaceFrame, BrowserError> {
    let result: WorkspaceFrame =
        serde_json::from_value(value).map_err(|_| BrowserError::Protocol)?;
    public_url(&result.url)?;
    if result.width != 1280
        || result.height != 900
        || !hex64(&result.frame_token)
        || result.png_base64.len() > 3 * 1024 * 1024
    {
        return Err(BrowserError::Protocol);
    }
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(&result.png_base64)
        .map_err(|_| BrowserError::Protocol)?;
    if bytes.len() > 2 * 1024 * 1024
        || bytes.len() < 24
        || !bytes.starts_with(b"\x89PNG\r\n\x1a\n")
        || bytes[12..16] != *b"IHDR"
        || bytes[16..20] != 1280u32.to_be_bytes()
        || bytes[20..24] != 900u32.to_be_bytes()
    {
        return Err(BrowserError::Protocol);
    }
    Ok(result)
}

#[cfg(test)]
mod tests;
