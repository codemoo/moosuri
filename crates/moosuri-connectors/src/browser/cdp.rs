use super::BrowserError;
use futures_util::{SinkExt, StreamExt};
use serde_json::{Value, json};
use std::collections::HashSet;
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
        })
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
        self.socket
            .send(Message::Text(request.to_string().into()))
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
