//! Claude sign-in over the existing human frame/input proofs.
//!
//! This is not a generic browser API: there is no observation, URL, script or CDP
//! command here. A fixed Fetch rule pauses only the official callback document in the
//! top frame; the exact callback (one `code`, the matching `state`) is answered with a
//! fixed blank page *before any network request*, its `code#state` is held privately,
//! and the page is then replaced by a fresh blank target. Frames carry only an origin.
use super::*;
use crate::browser::{
    claude_auth::{self, AuthorizeUrl},
    google_auth::Display,
};
use zeroize::{Zeroize, Zeroizing};

const STEP: Duration = Duration::from_secs(5);
/// Fixed, nonsecret document served for the intercepted callback navigation.
const BLANK_PAGE: &str =
    "<!doctype html><meta charset=utf-8><title>Done</title><p>You can close this window.</p>";

/// Identity comparisons retain the full private URL, including long SSO redirects.
/// Public origins still pass the existing destination policy. Chrome's own network
/// error page stays visible so the owner can see the failure and cancel or retry.
pub(super) fn identity_url(raw: &str) -> Result<String, BrowserError> {
    if matches!(raw, "about:blank" | "chrome-error://chromewebdata/") {
        return Ok(raw.to_owned());
    }
    if raw.len() > 16 * 1024 {
        return Err(BrowserError::OutputTooLarge);
    }
    let url = url::Url::parse(raw).map_err(|_| BrowserError::InvalidUrl)?;
    if url.scheme() != "https" || !url.username().is_empty() || url.password().is_some() {
        return Err(BrowserError::InvalidUrl);
    }
    net::public_url(&url.origin().ascii_serialization())?;
    Ok(url.into())
}

enum Phase {
    Waiting,
    /// The exact callback was captured; its blank replacement has not been taken yet.
    Captured(Zeroizing<String>),
    /// The code was handed out once. Later callbacks are only aborted.
    Delivered,
    /// A wrong/duplicate/unaccounted callback or Fetch failure. Fail closed.
    Failed,
}

pub(crate) struct ClaudeWorkspace {
    inner: BrowserWorkspace,
    display: Option<Display>,
    authorize: AuthorizeUrl,
    /// The target session that has Fetch enabled and its top frame.
    fetch_session: String,
    top_frame: String,
    phase: Phase,
}

/// One paused request, reduced to what the decision needs. The URL (which may carry
/// the code) is moved out of the CDP value into a zeroizing buffer and wiped there.
struct Paused {
    id: String,
    session: String,
    own: bool,
    url: Option<Zeroizing<String>>,
}
fn paused(event: &mut Value, fetch_session: &str, top_frame: &str) -> Option<Paused> {
    let text = |pointer: &str| {
        event
            .pointer(pointer)
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let id = text("/params/requestId");
    let session = text("/sessionId");
    // Exactly a top-level GET document request at the request stage in our own session.
    let own = session.as_deref() == Some(fetch_session)
        && text("/params/resourceType").as_deref() == Some("Document")
        && text("/params/frameId").as_deref() == Some(top_frame)
        && text("/params/request/method").as_deref() == Some("GET")
        && event.pointer("/params/responseStatusCode").is_none()
        && event.pointer("/params/responseErrorReason").is_none()
        && event.pointer("/params/request/urlFragment").is_none();
    let url = text("/params/request/url").map(Zeroizing::new);
    if let Some(Value::String(url)) = event.pointer_mut("/params/request/url") {
        url.zeroize();
    }
    Some(Paused {
        id: id?,
        session: session?,
        own,
        url,
    })
}

impl ClaudeWorkspace {
    /// Starts private X, Google Chrome (fresh ephemeral profile, JavaScript on) and
    /// opens the already validated authorization URL with the callback rule in place.
    /// On any failure everything started here is stopped before returning.
    pub(crate) async fn launch(
        config: BrowserConfig,
        authorize: AuthorizeUrl,
        cancel: CancellationToken,
    ) -> Result<Self, BrowserError> {
        // Check kernel isolation before starting either X or Chrome.
        capsule_health_mode(&config, true)?;
        let mut display = Display::start().await.map_err(|_| BrowserError::Setup)?;
        let lifetime = Duration::from_secs(claude_auth::ATTEMPT_SECONDS);
        let session = match BrowserSession::launch_mode(config, cancel, lifetime, true).await {
            Ok(session) => session,
            Err(error) => {
                display.stop().await;
                return Err(error);
            }
        };
        let mut this = Self {
            inner: BrowserWorkspace {
                session,
                current: None,
                proof: None,
                prior_proof: None,
                ignored_targets: HashSet::new(),
                last_links: None,
                human_interacted: false,
                private_auth: true,
            },
            display: Some(display),
            authorize,
            fetch_session: String::new(),
            top_frame: String::new(),
            phase: Phase::Waiting,
        };
        if let Err(error) = this.open_authorize().await {
            this.shutdown().await;
            return Err(error);
        }
        Ok(this)
    }

    async fn open_authorize(&mut self) -> Result<(), BrowserError> {
        self.inner.blank().await?;
        let identity = {
            let inner = &mut self.inner;
            bounded!(inner, inner.refresh())?
        };
        self.top_frame = identity.frame;
        let sid = self.inner.sid()?;
        // Fetch is enabled before the first navigation, so the callback can never
        // reach the proxy. It is never disabled: closing the target ends its requests.
        self.inner
            .session
            .cdp
            .command_timeout(
                "Fetch.enable",
                json!({"patterns":[{
                    "urlPattern":format!("{}*", claude_auth::CALLBACK),
                    "resourceType":"Document",
                    "requestStage":"Request",
                }]}),
                Some(&sid),
                STEP,
            )
            .await?;
        drop(self.inner.session.cdp.take_paused());
        self.fetch_session = sid.clone();
        self.inner.session.cdp.before_navigation(&sid);
        let navigation = self
            .inner
            .session
            .cdp
            .command_timeout(
                "Page.navigate",
                json!({"url":self.authorize.as_str()}),
                Some(&sid),
                Duration::from_secs(15),
            )
            .await?;
        if navigation.get("errorText").is_some() {
            return Err(BrowserError::Network);
        }
        Ok(())
    }

    /// Reads CDP events for a moment and accounts for every paused request.
    /// Independent of any frame or input, so the callback finishes on its own.
    pub(crate) async fn watch(&mut self) -> Result<(), BrowserError> {
        if matches!(self.phase, Phase::Failed) {
            return Err(BrowserError::Expired);
        }
        if let Err(error) = self.inner.session.cdp.pump(Duration::from_millis(40)).await {
            self.phase = Phase::Failed;
            return Err(error);
        }
        self.drain().await
    }

    async fn drain(&mut self) -> Result<(), BrowserError> {
        let (events, overflow) = self.inner.session.cdp.take_paused();
        let (mut failed, mut rejected, mut duplicate) = (overflow, false, false);
        for mut event in events {
            let Some(request) = paused(&mut event, &self.fetch_session, &self.top_frame) else {
                failed = true;
                continue;
            };
            let accepted = match (&self.phase, request.own) {
                (Phase::Waiting, true) => request
                    .url
                    .as_deref()
                    .and_then(|url| self.authorize.accept_callback(url)),
                _ => None,
            };
            if let Some(code) = accepted {
                if self.fulfill(&request).await.is_ok() {
                    self.phase = Phase::Captured(code);
                } else {
                    failed = true;
                    let _ = self.abort(&request).await;
                }
            } else {
                if request.own {
                    // A callback-shaped request that is not the one exact callback, or a
                    // second one while one is held. Delivered attempts just abort extras.
                    match self.phase {
                        Phase::Waiting => rejected = true,
                        Phase::Captured(_) => duplicate = true,
                        _ => {}
                    }
                }
                failed |= self.abort(&request).await.is_err();
            }
        }
        if failed || rejected || duplicate {
            // Dropping a held code zeroizes it: an ambiguous callback is never used.
            self.phase = Phase::Failed;
            return Err(BrowserError::Protocol);
        }
        Ok(())
    }

    async fn fulfill(&mut self, request: &Paused) -> Result<(), BrowserError> {
        self.inner
            .session
            .cdp
            .command_timeout(
                "Fetch.fulfillRequest",
                json!({
                    "requestId": request.id,
                    "responseCode": 200,
                    "responseHeaders": [
                        {"name":"Content-Type","value":"text/html; charset=utf-8"},
                        {"name":"Cache-Control","value":"no-store"},
                        {"name":"Referrer-Policy","value":"no-referrer"},
                    ],
                    "body": base64::engine::general_purpose::STANDARD.encode(BLANK_PAGE),
                }),
                Some(&request.session),
                STEP,
            )
            .await
            .map(drop)
    }
    async fn abort(&mut self, request: &Paused) -> Result<(), BrowserError> {
        self.inner
            .session
            .cdp
            .command_timeout(
                "Fetch.failRequest",
                json!({"requestId":request.id,"errorReason":"BlockedByClient"}),
                Some(&request.session),
                STEP,
            )
            .await
            .map(drop)
    }

    /// Hands out the captured `code#state` exactly once, after replacing the page with
    /// a fresh blank target. If the replacement fails the browser is killed (the callback
    /// page itself was already the fixed blank document, so no code was ever rendered).
    pub(crate) async fn captured_code(&mut self) -> Option<Zeroizing<String>> {
        if !matches!(self.phase, Phase::Captured(_)) {
            return None;
        }
        let Phase::Captured(code) = std::mem::replace(&mut self.phase, Phase::Delivered) else {
            return None;
        };
        if self.replace_with_blank().await.is_err() {
            self.inner.session.poisoned = true;
            self.inner.session.kill();
        }
        Some(code)
    }

    async fn replace_with_blank(&mut self) -> Result<(), BrowserError> {
        let handle = self
            .inner
            .current
            .take()
            .ok_or(BrowserError::InvalidHandle)?;
        let target = self
            .inner
            .session
            .targets
            .remove(&handle.page)
            .ok_or(BrowserError::InvalidHandle)?;
        self.inner.clear_proofs();
        self.inner
            .session
            .cdp
            .command_timeout(
                "Target.closeTarget",
                json!({"targetId":target.target}),
                None,
                STEP,
            )
            .await?;
        drop(self.inner.session.cdp.take_paused());
        self.inner.blank().await
    }

    /// Sanitized screenshot: the page URL is reduced to its origin before leaving.
    /// Frames are shared while geometry is unchanged, so a token stays valid for input.
    pub(crate) async fn human_frame(&mut self) -> Result<WorkspaceFrame, BrowserError> {
        if !matches!(self.phase, Phase::Waiting) {
            return Err(BrowserError::Expired);
        }
        let inner = &mut self.inner;
        let mut frame = bounded!(inner, inner.frame_inner(true))?;
        frame.url = claude_auth::sanitized_origin(&frame.url).ok_or(BrowserError::Protocol)?;
        Ok(frame)
    }

    /// Proof-bound human input; no observation is produced or retained.
    pub(crate) async fn human_input(
        &mut self,
        frame_token: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError> {
        if !matches!(self.phase, Phase::Waiting) {
            return Err(BrowserError::Expired);
        }
        self.inner.input(frame_token, input).await
    }

    /// Awaited teardown of Chrome, its profile and the private X server.
    pub(crate) async fn shutdown(self) {
        let Self { inner, display, .. } = self;
        inner.shutdown().await;
        if let Some(mut display) = display {
            display.stop().await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures_util::{SinkExt, StreamExt};
    use tokio::sync::mpsc;
    use tokio_tungstenite::tungstenite::Message;

    const STATE: &str = "state_VALUE-123~abcdefgh";
    const CODE: &str = "CANARYcode123456";
    const AUTHORIZE: &str = "https://claude.com/cai/oauth/authorize?response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&state=state_VALUE-123~abcdefgh";

    type Log = Arc<Mutex<Vec<(String, Value)>>>;
    struct Fixture {
        workspace: ClaudeWorkspace,
        log: Log,
        events: mpsc::UnboundedSender<Value>,
        server: tokio::task::JoinHandle<()>,
    }
    fn callback(code: &str, state: &str) -> String {
        format!("https://platform.claude.com/oauth/code/callback?code={code}&state={state}")
    }
    fn event(id: &str, url: &str) -> Value {
        json!({"method":"Fetch.requestPaused","sessionId":"session","params":{
            "requestId":id,"resourceType":"Document","frameId":"frame",
            "request":{"method":"GET","url":url}}})
    }
    fn png() -> String {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend(1280u32.to_be_bytes());
        png.extend(900u32.to_be_bytes());
        base64::engine::general_purpose::STANDARD.encode(png)
    }
    async fn fixture() -> Fixture {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let log: Log = Arc::default();
        let url = Arc::new(Mutex::new(AUTHORIZE.to_owned()));
        let (events, mut inbox) = mpsc::unbounded_channel::<Value>();
        let (record, current) = (log.clone(), url.clone());
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut ws = tokio_tungstenite::accept_async(tcp).await.unwrap();
            loop {
                tokio::select! {
                    message = ws.next() => {
                        let Some(Ok(message)) = message else { break };
                        let request: Value =
                            serde_json::from_str(message.to_text().unwrap()).unwrap();
                        let method = request["method"].as_str().unwrap().to_owned();
                        record.lock().unwrap().push((method.clone(), request["params"].clone()));
                        let page = current.lock().unwrap().clone();
                        let result = match method.as_str() {
                            "Page.getFrameTree" => json!({"frameTree":{"frame":{"id":"frame","loaderId":"loader","url":page}}}),
                            "Target.createTarget" => {
                                *current.lock().unwrap() = "about:blank".into();
                                json!({"targetId":"target2"})
                            }
                            "Target.attachToTarget" => json!({"sessionId":"session2"}),
                            "Target.closeTarget" => json!({"success":true}),
                            "Page.navigate" => json!({"frameId":"frame","loaderId":"nav"}),
                            "Page.getLayoutMetrics" => json!({"cssLayoutViewport":{"pageX":0,"pageY":0,"clientWidth":1280,"clientHeight":900},"cssVisualViewport":{"offsetX":0,"offsetY":0,"pageX":0,"pageY":0,"clientWidth":1280,"clientHeight":900,"scale":1},"cssContentSize":{"x":0,"y":0,"width":1280,"height":900}}),
                            "Accessibility.getFullAXTree" => json!({"nodes":[]}),
                            "DOMSnapshot.captureSnapshot" => json!({"strings":[page,"frame","block"],"documents":[{"documentURL":0,"frameId":1,"scrollOffsetX":0,"scrollOffsetY":0,"nodes":{"nodeType":[9,1],"backendNodeId":[1,2]},"layout":{"nodeIndex":[1],"bounds":[[0,0,10,10]],"styles":[[2,2,2,2,2,2,2,2]],"paintOrders":[1]}}]}),
                            "Page.captureScreenshot" => json!({"data":png()}),
                            _ => json!({}),
                        };
                        let reply = json!({"id":request["id"],"sessionId":request["sessionId"],"result":result});
                        if ws.send(Message::Text(reply.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                    pushed = inbox.recv() => {
                        let Some(pushed) = pushed else { break };
                        if ws.send(Message::Text(pushed.to_string().into())).await.is_err() {
                            break;
                        }
                    }
                }
            }
        });
        let (generation, page) = (Uuid::new_v4(), Uuid::new_v4());
        let mut targets = HashMap::new();
        targets.insert(
            page,
            Target {
                target: "target".into(),
                session: "session".into(),
                frame: "frame".into(),
                loader: "loader".into(),
            },
        );
        let session = BrowserSession {
            tenant: Uuid::new_v4(),
            generation,
            child: Arc::new(Mutex::new(None)),
            watchdog: tokio::spawn(async {}),
            persistent: false,
            profile: std::env::temp_dir().join(format!("no-browser-profile-{}", Uuid::new_v4())),
            cdp: Cdp::connect(&endpoint).await.unwrap(),
            targets,
            deadline: Instant::now() + Duration::from_secs(100),
            poisoned: false,
            cancellation: CancellationToken::new(),
            javascript_enabled: true,
        };
        Fixture {
            workspace: ClaudeWorkspace {
                inner: BrowserWorkspace {
                    session,
                    current: Some(PageHandle {
                        page,
                        worker_generation: generation,
                        navigation_generation: 1,
                    }),
                    proof: None,
                    prior_proof: None,
                    ignored_targets: HashSet::new(),
                    last_links: None,
                    human_interacted: false,
                    private_auth: true,
                },
                display: None,
                authorize: AuthorizeUrl::parse(AUTHORIZE).unwrap(),
                fetch_session: "session".into(),
                top_frame: "frame".into(),
                phase: Phase::Waiting,
            },
            log,
            events,
            server,
        }
    }
    fn methods(log: &Log, name: &str) -> Vec<Value> {
        log.lock()
            .unwrap()
            .iter()
            .filter(|(method, _)| method == name)
            .map(|(_, params)| params.clone())
            .collect()
    }
    /// Pump until the predicate holds (or the attempt failed closed).
    async fn settle(workspace: &mut ClaudeWorkspace, done: impl Fn(&Phase) -> bool) {
        for _ in 0..100 {
            let _ = workspace.watch().await;
            if done(&workspace.phase) {
                return;
            }
        }
        panic!("the synthetic callback was not accounted for");
    }
    async fn pump_for_a_while(workspace: &mut ClaudeWorkspace) {
        for _ in 0..12 {
            let _ = workspace.watch().await;
        }
    }

    #[test]
    fn private_identity_handles_long_redirects_and_visible_network_failures() {
        let long = format!("https://claude.com/login?continue={}", "a".repeat(4000));
        assert_eq!(identity_url(&long).unwrap(), long);
        assert!(net::public_url(&long).is_err());
        assert_eq!(
            identity_url("chrome-error://chromewebdata/").unwrap(),
            "chrome-error://chromewebdata/"
        );
        assert_eq!(
            claude_auth::sanitized_origin("chrome-error://chromewebdata/").as_deref(),
            Some("")
        );
        assert!(claude_auth::is_sanitized_origin(""));
        for bad in [
            "https://localhost/",
            "https://127.0.0.1/",
            "https://user:pass@claude.com/",
            "data:text/html,secret",
            "file:///etc/passwd",
        ] {
            assert!(identity_url(bad).is_err());
        }
        assert!(identity_url(&format!("https://claude.com/?{}", "x".repeat(17000))).is_err());
    }

    #[tokio::test]
    async fn exact_callback_is_answered_blank_before_the_network_and_returned_once() {
        let mut f = fixture().await;
        f.events.send(event("r1", &callback(CODE, STATE))).unwrap();
        settle(&mut f.workspace, |p| matches!(p, Phase::Captured(_))).await;
        // Answered locally with the fixed blank page; never continued to the network.
        let fulfilled = methods(&f.log, "Fetch.fulfillRequest");
        assert_eq!(fulfilled.len(), 1);
        let body = base64::engine::general_purpose::STANDARD
            .decode(fulfilled[0]["body"].as_str().unwrap())
            .unwrap();
        assert_eq!(body, BLANK_PAGE.as_bytes());
        assert!(methods(&f.log, "Fetch.continueRequest").is_empty());
        // No command the worker ever sent contains the code.
        assert!(
            !serde_json::to_string(&*f.log.lock().unwrap())
                .unwrap()
                .contains(CODE)
        );
        // A frame cannot be taken while a code is held; the code is taken first, once.
        assert!(matches!(
            f.workspace.human_frame().await,
            Err(BrowserError::Expired)
        ));
        let code = f.workspace.captured_code().await.unwrap();
        assert_eq!(code.as_str(), format!("{CODE}#{STATE}"));
        let created = methods(&f.log, "Target.createTarget");
        assert_eq!(created.last().unwrap()["url"], "about:blank");
        assert_eq!(methods(&f.log, "Target.closeTarget").len(), 1);
        assert!(f.workspace.captured_code().await.is_none());
        assert!(matches!(
            f.workspace.human_frame().await,
            Err(BrowserError::Expired)
        ));
        assert!(matches!(
            f.workspace
                .human_input(
                    &"a".repeat(64),
                    WorkspaceInput::Key {
                        key: WorkspaceKey::Enter
                    }
                )
                .await,
            Err(BrowserError::Expired)
        ));
        // A later callback after delivery is only aborted; it never yields a second code.
        f.events
            .send(event("r2", &callback("SECONDcode123456", STATE)))
            .unwrap();
        pump_for_a_while(&mut f.workspace).await;
        assert!(f.workspace.captured_code().await.is_none());
        assert!(
            methods(&f.log, "Fetch.failRequest")
                .iter()
                .any(|p| p["requestId"] == "r2")
        );
        assert_eq!(methods(&f.log, "Fetch.fulfillRequest").len(), 1);
        f.workspace.shutdown().await;
        f.server.abort();
    }

    #[tokio::test]
    async fn wrong_state_and_malformed_callbacks_fail_closed_without_a_code() {
        for url in [
            callback(CODE, "another_state_value"),
            format!("{}&extra=1", callback(CODE, STATE)),
            format!("{}&error=access_denied", callback(CODE, STATE)),
            format!("https://platform.claude.com/oauth/code/callback?state={STATE}"),
            callback("short", STATE),
            format!("https://platform.claude.com/oauth/code/callback/x?code={CODE}&state={STATE}"),
        ] {
            let mut f = fixture().await;
            f.events.send(event("r1", &url)).unwrap();
            settle(&mut f.workspace, |p| matches!(p, Phase::Failed)).await;
            assert!(f.workspace.captured_code().await.is_none());
            assert!(methods(&f.log, "Fetch.fulfillRequest").is_empty(), "{url}");
            assert_eq!(methods(&f.log, "Fetch.failRequest").len(), 1);
            assert!(matches!(
                f.workspace.watch().await,
                Err(BrowserError::Expired)
            ));
            assert!(matches!(
                f.workspace.human_frame().await,
                Err(BrowserError::Expired)
            ));
            f.workspace.shutdown().await;
            f.server.abort();
        }
    }

    #[tokio::test]
    async fn duplicate_callbacks_drop_the_first_code_and_fail_closed() {
        let mut f = fixture().await;
        f.events.send(event("r1", &callback(CODE, STATE))).unwrap();
        f.events.send(event("r2", &callback(CODE, STATE))).unwrap();
        pump_for_a_while(&mut f.workspace).await;
        assert!(matches!(f.workspace.phase, Phase::Failed));
        assert!(f.workspace.captured_code().await.is_none());
        assert!(!methods(&f.log, "Fetch.failRequest").is_empty());
        f.workspace.shutdown().await;
        f.server.abort();
    }

    #[tokio::test]
    async fn requests_that_are_not_our_top_level_get_are_aborted_not_adopted() {
        let mut f = fixture().await;
        let good = callback(CODE, STATE);
        let mut foreign_session = event("a", &good);
        foreign_session["sessionId"] = json!("other");
        let mut subframe = event("b", &good);
        subframe["params"]["frameId"] = json!("child");
        let mut post = event("c", &good);
        post["params"]["request"]["method"] = json!("POST");
        let mut response = event("d", &good);
        response["params"]["responseStatusCode"] = json!(302);
        let mut fragment = event("e", &good);
        fragment["params"]["request"]["urlFragment"] = json!("#x");
        let mut script = event("f", &good);
        script["params"]["resourceType"] = json!("Script");
        for pushed in [foreign_session, subframe, post, response, fragment, script] {
            f.events.send(pushed).unwrap();
        }
        pump_for_a_while(&mut f.workspace).await;
        assert!(matches!(f.workspace.phase, Phase::Waiting));
        assert!(methods(&f.log, "Fetch.fulfillRequest").is_empty());
        let aborted = methods(&f.log, "Fetch.failRequest");
        for id in ["a", "b", "c", "d", "e", "f"] {
            assert!(aborted.iter().any(|p| p["requestId"] == id), "{id}");
        }
        assert!(f.workspace.captured_code().await.is_none());
        f.workspace.shutdown().await;
        f.server.abort();
    }

    #[tokio::test]
    async fn frames_carry_only_an_origin_and_input_is_bound_to_the_frame_token() {
        let mut f = fixture().await;
        let frame = f.workspace.human_frame().await.unwrap();
        assert_eq!(frame.url, "https://claude.com");
        assert!(claude_auth::is_sanitized_origin(&frame.url));
        assert!(!serde_json::to_string(&frame).unwrap().contains(STATE));
        // Unchanged geometry keeps the token valid across host polls.
        let again = f.workspace.human_frame().await.unwrap();
        assert_eq!(again.frame_token, frame.frame_token);
        assert!(matches!(
            f.workspace
                .human_input(&"0".repeat(64), WorkspaceInput::Click { x: 1, y: 1 })
                .await,
            Err(BrowserError::StalePage)
        ));
        assert!(methods(&f.log, "Input.dispatchMouseEvent").is_empty());
        f.workspace
            .human_input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
            .await
            .unwrap();
        assert_eq!(methods(&f.log, "Input.dispatchMouseEvent").len(), 2);
        // The token was consumed: replay is stale and dispatches nothing.
        assert!(matches!(
            f.workspace
                .human_input(&frame.frame_token, WorkspaceInput::Click { x: 10, y: 20 })
                .await,
            Err(BrowserError::StalePage)
        ));
        assert_eq!(methods(&f.log, "Input.dispatchMouseEvent").len(), 2);
        // No observation-style CDP was ever used.
        for (method, _) in f.log.lock().unwrap().iter() {
            assert!(
                !matches!(method.as_str(), "Runtime.evaluate" | "Page.captureSnapshot"),
                "{method}"
            );
        }
        f.workspace.shutdown().await;
        f.server.abort();
    }
}

#[cfg(test)]
mod browser_tests;
