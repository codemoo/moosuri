use super::BrowserError;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::{HashSet, VecDeque};
use std::time::Duration;
use tokio::net::TcpStream;
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream,
    tungstenite::{Message, protocol::WebSocketConfig},
};

pub(super) struct Cdp {
    socket: WebSocketStream<MaybeTlsStream<TcpStream>>,
    next: u64,
    loaded: HashSet<(String, String)>,
    /// `Fetch.requestPaused` events are queued (at most `MAX_PAUSED`). The separate
    /// opt-in attachment queue retains only identity metadata. This queue holds URLs that may
    /// carry secrets, so it is never logged and `Cdp` has no `Debug`.
    paused: VecDeque<Value>,
    paused_overflow: bool,
    /// Opt-in (Claude sign-in only): browser-level `Target.attachedToTarget` /
    /// `detachedFromTarget` metadata, bounded, never logged. Ids only, no URLs.
    track_attach: bool,
    attached: VecDeque<Attached>,
    detached: VecDeque<String>,
    attach_overflow: bool,
    attach_seq: u64,
    /// At most one sent human-input command whose reply has not been consumed.
    pending_input: Option<PendingInput>,
}

/// The only commands that may be left pending while other CDP traffic is handled.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum InputOp {
    Mouse,
    Key,
    Text,
}
impl InputOp {
    pub(super) fn method(self) -> &'static str {
        match self {
            Self::Mouse => "Input.dispatchMouseEvent",
            Self::Key => "Input.dispatchKeyEvent",
            Self::Text => "Input.insertText",
        }
    }
}

/// Exact id and session of the sent command, and its single retained reply
/// (`true` = clean result). Never holds params, so no text or URL is kept.
struct PendingInput {
    id: u64,
    session: String,
    reply: Option<bool>,
}

/// Overwrites a private `text` parameter (typed input) in an owned request.
fn wipe_text(request: &mut Value) {
    use zeroize::Zeroize;
    if let Some(Value::String(text)) = request.pointer_mut("/params/text") {
        text.zeroize();
    }
}

const MAX_PAUSED: usize = 8;
const MAX_ATTACHED: usize = 8;
const MAX_DETACHED: usize = 16;

/// One auto-attached target, reduced to identity facts. `seq` orders arrival.
pub(super) struct Attached {
    pub session: String,
    pub target: String,
    pub page: bool,
    pub opener: Option<String>,
    pub can_access_opener: bool,
    pub waiting: bool,
    pub seq: u64,
}

impl Cdp {
    pub async fn connect(endpoint: &str) -> Result<Self, BrowserError> {
        let url = url::Url::parse(endpoint).map_err(|_| BrowserError::Protocol)?;
        if url.scheme() != "ws"
            || url.host_str() != Some("127.0.0.1")
            || url.port().is_none()
            || !url.path().starts_with("/devtools/browser/")
        {
            return Err(BrowserError::Protocol);
        }
        let config = WebSocketConfig::default()
            .max_message_size(Some(4 * 1024 * 1024))
            .max_frame_size(Some(4 * 1024 * 1024));
        let (socket, _) =
            tokio_tungstenite::connect_async_with_config(endpoint, Some(config), true)
                .await
                .map_err(|_| BrowserError::Protocol)?;
        Ok(Self {
            socket,
            next: 0,
            loaded: HashSet::new(),
            paused: VecDeque::new(),
            paused_overflow: false,
            track_attach: false,
            attached: VecDeque::new(),
            detached: VecDeque::new(),
            attach_overflow: false,
            attach_seq: 0,
            pending_input: None,
        })
    }
    fn note_attachment(&mut self, value: &Value) {
        // Only browser-level events (no sessionId); nested ones are never requested.
        if value.get("sessionId").is_some() {
            return;
        }
        let params = &value["params"];
        match value.get("method").and_then(Value::as_str) {
            Some("Target.attachedToTarget") => {
                self.attach_seq = self.attach_seq.saturating_add(1);
                let text = |pointer: &str| {
                    params
                        .pointer(pointer)
                        .and_then(Value::as_str)
                        .map(str::to_owned)
                };
                let parsed = (|| {
                    Some(Attached {
                        session: text("/sessionId")?,
                        target: text("/targetInfo/targetId")?,
                        page: text("/targetInfo/type")? == "page",
                        opener: text("/targetInfo/openerId"),
                        can_access_opener: params
                            .pointer("/targetInfo/canAccessOpener")
                            .and_then(Value::as_bool)
                            .unwrap_or(false),
                        waiting: params.get("waitingForDebugger")?.as_bool()?,
                        seq: self.attach_seq,
                    })
                })();
                match parsed {
                    Some(item) if self.attached.len() < MAX_ATTACHED => {
                        self.attached.push_back(item)
                    }
                    _ => self.attach_overflow = true,
                }
            }
            Some("Target.detachedFromTarget") => {
                match params.get("sessionId").and_then(Value::as_str) {
                    Some(session) if self.detached.len() < MAX_DETACHED => {
                        self.detached.push_back(session.to_owned())
                    }
                    _ => self.attach_overflow = true,
                }
            }
            _ => {}
        }
    }
    /// Starts queueing attachment metadata. Normal browsers never call this.
    pub fn track_attachments(&mut self) {
        self.track_attach = true;
    }
    /// Browser-level auto-attach for pages only. With `on`, new pages start paused
    /// (`waitingForDebugger`) until the owner configures them and resumes them.
    /// Turning it off also stops queueing and discards pending metadata.
    pub async fn set_page_auto_attach(
        &mut self,
        on: bool,
        limit: Duration,
    ) -> Result<(), BrowserError> {
        if on {
            self.track_attach = true;
        }
        let mut params = json!({"autoAttach":on,"waitForDebuggerOnStart":on,"flatten":true});
        if on {
            params["filter"] = json!([{"type":"page"},{"exclude":true}]);
        }
        let result = self
            .command_timeout("Target.setAutoAttach", params, None, limit)
            .await
            .map(drop);
        if !on {
            self.track_attach = false;
            self.attached.clear();
            self.detached.clear();
            self.attach_overflow = false;
        }
        result
    }
    /// Arrival counter: attachments with a larger `seq` arrived after this call.
    pub fn attach_mark(&self) -> u64 {
        self.attach_seq
    }
    pub fn detached_session(&self, session: &str) -> bool {
        !self.attach_overflow && self.detached.iter().any(|id| id == session)
    }
    /// Takes queued attach/detach metadata; the flag reports dropped or malformed events.
    pub fn take_attachments(&mut self) -> (Vec<Attached>, Vec<String>, bool) {
        (
            self.attached.drain(..).collect(),
            self.detached.drain(..).collect(),
            std::mem::take(&mut self.attach_overflow),
        )
    }
    async fn receive(&mut self) -> Result<Value, BrowserError> {
        loop {
            match self
                .socket
                .next()
                .await
                .ok_or(BrowserError::Protocol)?
                .map_err(|_| BrowserError::Protocol)?
            {
                Message::Text(text) => {
                    let value: Value =
                        serde_json::from_str(&text).map_err(|_| BrowserError::Protocol)?;
                    if value.get("method").and_then(Value::as_str) == Some("Page.lifecycleEvent")
                        && value.pointer("/params/name").and_then(Value::as_str) == Some("load")
                        && let (Some(session), Some(loader)) = (
                            value.get("sessionId").and_then(Value::as_str),
                            value.pointer("/params/loaderId").and_then(Value::as_str),
                        )
                    {
                        if self.loaded.len() >= 256 {
                            self.loaded.clear();
                        }
                        self.loaded.insert((session.to_owned(), loader.to_owned()));
                    }
                    if value.get("method").and_then(Value::as_str) == Some("Fetch.requestPaused") {
                        if self.paused.len() < MAX_PAUSED {
                            self.paused.push_back(value.clone());
                        } else {
                            self.paused_overflow = true;
                        }
                    }
                    if self.track_attach {
                        self.note_attachment(&value);
                    }
                    self.note_input_reply(&value);
                    return Ok(value);
                }
                Message::Ping(data) => self
                    .socket
                    .send(Message::Pong(data))
                    .await
                    .map_err(|_| BrowserError::Protocol)?,
                Message::Pong(_) => {}
                _ => return Err(BrowserError::Protocol),
            }
        }
    }
    pub async fn command(
        &mut self,
        method: &'static str,
        params: Value,
        session: Option<&str>,
    ) -> Result<Value, BrowserError> {
        self.next = self.next.checked_add(1).ok_or(BrowserError::Protocol)?;
        let id = self.next;
        let mut request = json!({"id": id, "method": method, "params": params});
        if let Some(session) = session {
            request["sessionId"] = json!(session);
        }
        let wire = request.to_string();
        wipe_text(&mut request);
        self.socket
            .send(Message::Text(wire.into()))
            .await
            .map_err(|_| BrowserError::Protocol)?;
        for _ in 0..4096 {
            let response = self.receive().await?;
            if response.get("id").and_then(Value::as_u64) == Some(id) {
                if response.get("error").is_some()
                    || response.get("sessionId").and_then(Value::as_str) != session
                {
                    return Err(BrowserError::Protocol);
                }
                return response
                    .get("result")
                    .cloned()
                    .ok_or(BrowserError::Protocol);
            }
        }
        Err(BrowserError::Protocol)
    }
    /// `command` with an explicit bound. A reply that arrives late is discarded by
    /// the monotonic-id match in later commands.
    pub async fn command_timeout(
        &mut self,
        method: &'static str,
        params: Value,
        session: Option<&str>,
        limit: Duration,
    ) -> Result<Value, BrowserError> {
        tokio::time::timeout(limit, self.command(method, params, session))
            .await
            .unwrap_or(Err(BrowserError::Protocol))
    }
    /// Retains the reply of the one pending input command (exact id) whichever
    /// command is being awaited when it arrives. Any other message is ignored here.
    fn note_input_reply(&mut self, value: &Value) {
        let Some(pending) = self.pending_input.as_mut() else {
            return;
        };
        if value.get("id").and_then(Value::as_u64) != Some(pending.id) {
            return;
        }
        let clean = pending.reply.is_none()
            && value.get("error").is_none()
            && value.get("result").is_some()
            && value.get("sessionId").and_then(Value::as_str) == Some(pending.session.as_str());
        // A duplicate or mismatched reply can only make the outcome worse.
        pending.reply = Some(clean);
    }
    #[cfg(test)]
    pub fn input_pending(&self) -> bool {
        self.pending_input.is_some()
    }
    /// Sends one typed human-input command exactly once and leaves it pending, so the
    /// owner can keep servicing the browser (attachments, Fetch, resume) while Chrome
    /// finishes the input. Never replays; at most one command may be pending.
    pub async fn begin_input(
        &mut self,
        op: InputOp,
        params: Value,
        session: &str,
    ) -> Result<(), BrowserError> {
        if self.pending_input.is_some() {
            return Err(BrowserError::Protocol);
        }
        self.next = self.next.checked_add(1).ok_or(BrowserError::Protocol)?;
        let id = self.next;
        let mut request =
            json!({"id": id, "method": op.method(), "params": params, "sessionId": session});
        let wire = request.to_string();
        wipe_text(&mut request);
        self.pending_input = Some(PendingInput {
            id,
            session: session.to_owned(),
            reply: None,
        });
        if self.socket.send(Message::Text(wire.into())).await.is_err() {
            self.pending_input = None;
            return Err(BrowserError::Protocol);
        }
        Ok(())
    }
    /// The retained reply once it has arrived, which frees the single slot.
    pub fn take_input_reply(&mut self) -> Option<Result<(), BrowserError>> {
        let clean = self.pending_input.as_ref()?.reply?;
        self.pending_input = None;
        Some(if clean {
            Ok(())
        } else {
            Err(BrowserError::Protocol)
        })
    }
    /// Forgets the pending input; a late reply matches nothing and is dropped.
    pub fn abandon_input(&mut self) {
        self.pending_input = None;
    }
    /// Reads events for at most `wait`, returning early once the pending input's
    /// reply has been retained.
    pub async fn pump_input(&mut self, wait: Duration) -> Result<(), BrowserError> {
        async fn until_reply(cdp: &mut Cdp) -> Result<(), BrowserError> {
            loop {
                if cdp.pending_input.as_ref().is_none_or(|p| p.reply.is_some()) {
                    return Ok(());
                }
                cdp.receive().await?;
            }
        }
        tokio::time::timeout(wait, until_reply(self))
            .await
            .unwrap_or(Ok(()))
    }
    async fn pump_forever(&mut self) -> BrowserError {
        loop {
            if let Err(error) = self.receive().await {
                return error;
            }
        }
    }
    /// Reads asynchronous events for at most `wait` (queueing paused requests)
    /// while no command is pending. Never waits longer than `wait`.
    pub async fn pump(&mut self, wait: Duration) -> Result<(), BrowserError> {
        match tokio::time::timeout(wait, self.pump_forever()).await {
            Ok(error) => Err(error),
            Err(_) => Ok(()),
        }
    }
    /// Takes every queued `Fetch.requestPaused` event (whole CDP messages, with
    /// their `sessionId`; callers match the session when consuming). If events were
    /// dropped because the queue was full, the batch is returned with `overflow`.
    pub fn take_paused(&mut self) -> (Vec<Value>, bool) {
        let overflow = std::mem::take(&mut self.paused_overflow);
        (self.paused.drain(..).collect(), overflow)
    }
    pub fn before_navigation(&mut self, session: &str) {
        self.loaded.retain(|(sid, _)| sid != session);
    }
    pub async fn workspace_screenshot(&mut self, session: &str) -> Result<Value, BrowserError> {
        // Chromium can leave a compositor capture unanswered across navigation.
        // Abandon only this read, not the task's authenticated browser. A later
        // reply cannot satisfy another command: IDs are monotonic and matched by
        // `command`. Input/navigation timeouts retain their fatal/unknown semantics.
        tokio::time::timeout(
            Duration::from_secs(2),
            self.command(
                "Page.captureScreenshot",
                json!({"format":"png","captureBeyondViewport":false,"fromSurface":true}),
                Some(session),
            ),
        )
        .await
        .unwrap_or(Err(BrowserError::StalePage))
    }
    pub async fn wait_loaded(&mut self, session: &str, loader: &str) -> Result<(), BrowserError> {
        for _ in 0..4096 {
            if self
                .loaded
                .contains(&(session.to_owned(), loader.to_owned()))
            {
                return Ok(());
            }
            self.receive().await?;
        }
        Err(BrowserError::Protocol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn lifecycle_wait_matches_exact_session_and_loader() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let request: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            for event in [
                json!({"method":"Page.lifecycleEvent","sessionId":"other","params":{"name":"load","loaderId":"wanted"}}),
                json!({"method":"Page.lifecycleEvent","sessionId":"session","params":{"name":"load","loaderId":"old"}}),
                json!({"id":request["id"],"sessionId":"session","result":{"loaderId":"wanted"}}),
                json!({"method":"Page.lifecycleEvent","sessionId":"session","params":{"name":"load","loaderId":"wanted"}}),
            ] {
                socket
                    .send(Message::Text(event.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        cdp.before_navigation("session");
        cdp.command(
            "Page.navigate",
            json!({"url":"https://example.com"}),
            Some("session"),
        )
        .await
        .unwrap();
        assert!(!cdp.loaded.contains(&("session".into(), "wanted".into())));
        tokio::time::timeout(
            std::time::Duration::from_secs(1),
            cdp.wait_loaded("session", "wanted"),
        )
        .await
        .unwrap()
        .unwrap();
        server.await.unwrap();
    }
    #[tokio::test]
    async fn only_paused_requests_are_queued_with_a_fixed_bound() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let mut events = vec![json!({"method":"Page.frameNavigated","params":{}})];
            for n in 0..10 {
                events.push(json!({"method":"Fetch.requestPaused","sessionId":"s","params":{"requestId":n.to_string()}}));
            }
            for event in events {
                socket
                    .send(Message::Text(event.to_string().into()))
                    .await
                    .unwrap();
            }
            std::future::pending::<()>().await;
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        cdp.pump(Duration::from_millis(300)).await.unwrap();
        let (events, overflow) = cdp.take_paused();
        assert_eq!(events.len(), MAX_PAUSED);
        assert!(overflow);
        assert_eq!(events[0]["params"]["requestId"], "0");
        let (events, overflow) = cdp.take_paused();
        assert!(events.is_empty() && !overflow);
        server.abort();
    }

    #[tokio::test]
    async fn attachment_metadata_is_opt_in_bounded_and_carries_no_url() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let attach = |n: u32, opener: Option<&str>| {
            json!({"method":"Target.attachedToTarget","params":{"sessionId":format!("s{n}"),
                "waitingForDebugger":true,
                "targetInfo":{"targetId":format!("t{n}"),"type":"page","openerId":opener,
                    "canAccessOpener":true,"url":"https://secret.invalid/?code=CANARY"}}})
        };
        let server = tokio::spawn({
            let attach = attach.clone();
            async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let mut events = vec![attach(0, None)];
                // Not opted in: nothing above may be queued. Then the opted-in batch.
                events.push(json!({"id":1,"result":{}}));
                for n in 1..=10 {
                    events.push(attach(n, Some("root")));
                }
                events.push(
                    json!({"method":"Target.detachedFromTarget","params":{"sessionId":"s1"}}),
                );
                events.push(
                    json!({"method":"Target.attachedToTarget","sessionId":"nested","params":{}}),
                );
                for event in events {
                    socket
                        .send(Message::Text(event.to_string().into()))
                        .await
                        .unwrap();
                }
                std::future::pending::<()>().await;
            }
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        // Reading the first command reply consumes the untracked event.
        let _ = cdp.command("Target.getTargets", json!({}), None).await;
        let (items, gone, overflow) = cdp.take_attachments();
        assert!(items.is_empty() && gone.is_empty() && !overflow);
        cdp.track_attachments();
        cdp.pump(Duration::from_millis(300)).await.unwrap();
        let mark = cdp.attach_mark();
        let (items, gone, overflow) = cdp.take_attachments();
        assert_eq!(items.len(), MAX_ATTACHED);
        assert!(overflow, "excess attachments must be reported");
        assert_eq!(gone, vec!["s1".to_owned()]);
        assert_eq!(items[0].opener.as_deref(), Some("root"));
        assert!(items[0].page && items[0].waiting && items[0].can_access_opener);
        assert!(items.windows(2).all(|w| w[0].seq < w[1].seq));
        assert_eq!(mark, 10, "the nested-session event is not counted");
        server.abort();
    }

    #[tokio::test]
    async fn protocol_rejects_a_response_for_another_target_session() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            socket.next().await.unwrap().unwrap();
            socket
                .send(Message::Text(
                    json!({"id":1,"sessionId":"wrong","result":{}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        assert_eq!(
            cdp.command("Page.getFrameTree", json!({}), Some("expected"))
                .await
                .unwrap_err(),
            BrowserError::Protocol
        );
        server.await.unwrap();
    }

    /// The server answers the pending input (and optional extras) before the reply of a
    /// command that was awaited meanwhile; returns the outcome the client retained.
    async fn retained_input_outcome(replies: Vec<Value>) -> Option<Result<(), BrowserError>> {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let input: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(input["method"], "Input.dispatchMouseEvent");
            let next: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(next["method"], "Page.getFrameTree");
            assert_ne!(input["id"], next["id"]);
            let mut all = vec![json!({"id":9999,"sessionId":"s","result":{}})];
            for reply in replies {
                let mut reply = reply;
                reply["id"] = input["id"].clone();
                all.push(reply);
            }
            all.push(json!({"id":next["id"],"sessionId":"s","result":{"ok":true}}));
            for reply in all {
                socket
                    .send(Message::Text(reply.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        cdp.begin_input(InputOp::Mouse, json!({"type":"mouseReleased"}), "s")
            .await
            .unwrap();
        // One slot: a second input is refused and nothing is sent for it.
        assert_eq!(
            cdp.begin_input(InputOp::Key, json!({}), "s")
                .await
                .unwrap_err(),
            BrowserError::Protocol
        );
        assert!(cdp.take_input_reply().is_none());
        let other = tokio::time::timeout(
            Duration::from_secs(2),
            cdp.command("Page.getFrameTree", json!({}), Some("s")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(other, json!({"ok":true}));
        server.await.unwrap();
        let outcome = cdp.take_input_reply();
        assert!(!cdp.input_pending());
        outcome
    }

    #[tokio::test]
    async fn pending_input_reply_is_retained_by_exact_id_while_another_command_runs() {
        let clean = retained_input_outcome(vec![json!({"sessionId":"s","result":{}})]).await;
        assert_eq!(clean, Some(Ok(())));
        // A duplicate reply, an error reply and another session's reply all fail it.
        for bad in [
            vec![
                json!({"sessionId":"s","result":{}}),
                json!({"sessionId":"s","result":{}}),
            ],
            vec![json!({"sessionId":"s","error":{"code":-32000}})],
            vec![json!({"sessionId":"other","result":{}})],
        ] {
            assert_eq!(
                retained_input_outcome(bad).await,
                Some(Err(BrowserError::Protocol))
            );
        }
    }

    #[tokio::test]
    async fn pending_input_can_be_abandoned_and_a_late_reply_is_ignored() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let input: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(input["method"], "Input.insertText");
            socket
                .send(Message::Text(
                    json!({"id":input["id"],"sessionId":"s","result":{}})
                        .to_string()
                        .into(),
                ))
                .await
                .unwrap();
            std::future::pending::<()>().await;
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        cdp.begin_input(InputOp::Text, json!({"text":"private"}), "s")
            .await
            .unwrap();
        cdp.abandon_input();
        cdp.pump(Duration::from_millis(200)).await.unwrap();
        assert!(cdp.take_input_reply().is_none() && !cdp.input_pending());
        server.abort();
    }

    #[tokio::test]
    async fn stalled_workspace_capture_preserves_protocol_without_replay_or_late_reply_adoption() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = format!(
            "ws://{}/devtools/browser/synthetic",
            listener.local_addr().unwrap()
        );
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let capture: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(capture["method"], "Page.captureScreenshot");
            // The next command arrives only after capture times out. There is no
            // replay of the capture or of the human action that caused navigation.
            let next: Value =
                serde_json::from_str(socket.next().await.unwrap().unwrap().to_text().unwrap())
                    .unwrap();
            assert_eq!(next["method"], "Page.getFrameTree");
            assert_ne!(next["id"], capture["id"]);
            for response in [
                json!({"id":capture["id"],"sessionId":"session","result":{"data":"stale-pixels"}}),
                json!({"id":next["id"],"sessionId":"session","result":{"current_frame":true}}),
            ] {
                socket
                    .send(Message::Text(response.to_string().into()))
                    .await
                    .unwrap();
            }
        });
        let mut cdp = Cdp::connect(&endpoint).await.unwrap();
        assert_eq!(
            cdp.workspace_screenshot("session").await.unwrap_err(),
            BrowserError::StalePage
        );
        let current = tokio::time::timeout(
            Duration::from_secs(1),
            cdp.command("Page.getFrameTree", json!({}), Some("session")),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(current, json!({"current_frame":true}));
        server.await.unwrap();
    }
}
