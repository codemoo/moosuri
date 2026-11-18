//! Bounded I/O bridge to the unchanged official login process. No token parser.
use super::*;
use zeroize::Zeroizing;

/// One-time pasted code; deliberately neither Debug nor Serialize.
pub struct LoginCode(Zeroizing<String>);
impl LoginCode {
    pub fn new(value: Zeroizing<String>) -> Result<Self, TransportError> {
        if !(8..=4096).contains(&value.len()) || !value.bytes().all(|b| b.is_ascii_graphic()) {
            return Err(err());
        }
        Ok(Self(value))
    }
}
/// Only the validated official URL is exposed, never raw terminal output.
pub enum LoginEvent {
    AuthorizationUrl(String),
}

#[derive(Default)]
struct Output {
    bytes: Zeroizing<Vec<u8>>,
    sent: bool,
}
impl Output {
    fn feed(&mut self, bytes: &[u8]) -> Result<Option<String>, TransportError> {
        if self.bytes.len() + bytes.len() > 32768 {
            return Err(TransportError::LimitExceeded);
        }
        self.bytes.extend_from_slice(bytes);
        if self.sent {
            return Ok(None);
        }
        let Some(start) = self.bytes.windows(8).position(|b| b == b"https://") else {
            return Ok(None);
        };
        let tail = &self.bytes[start..];
        let Some(end) = tail.iter().position(|b| b.is_ascii_whitespace()) else {
            return Ok(None);
        };
        let text = std::str::from_utf8(&tail[..end]).map_err(|_| err())?;
        let url = url::Url::parse(text).map_err(|_| err())?;
        if text.len() > 8192
            || url.scheme() != "https"
            || url.host_str() != Some("claude.com")
            || url.path() != "/cai/oauth/authorize"
            || !url.username().is_empty()
            || url.password().is_some()
            || url.port().is_some()
            || url.fragment().is_some()
            || url.query().is_none()
            || !text.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(TransportError::DestinationDenied);
        }
        self.sent = true;
        Ok(Some(text.to_owned()))
    }
    fn successful(&self) -> bool {
        self.bytes
            .windows(b"Login successful".len())
            .any(|b| b == b"Login successful")
    }
}

impl Profile {
    pub async fn login_web(
        &self,
        environment: &str,
        events: &mpsc::Sender<LoginEvent>,
        codes: &mut mpsc::Receiver<LoginCode>,
        external: &CancellationToken,
    ) -> Result<(), TransportError> {
        let _os = tokio::select! {biased;_=external.cancelled()=>return Err(TransportError::Cancelled),r=self.os_lock(environment)=>r?};
        self.login_allowed(environment)?;
        let (env, temp, listener) = tokio::select! {biased;_=external.cancelled()=>return Err(TransportError::Cancelled),r=self.prepare(environment)=>r?};
        let name = format!("moosuri-claude-{}", uuid::Uuid::new_v4());
        let cancel = CancellationToken::new();
        let proxy = ProxyTask(tokio::spawn(proxy_server(listener, cancel.clone())));
        let operation = async {
            self.allowed_environment(environment)?;
            let mut child = self
                .launch(&env, temp.path(), &name, true, false, None)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .map_err(|_| err())?;
            let mut stdin = child.stdin.take().ok_or_else(err)?;
            let mut stdout = child.stdout.take().ok_or_else(err)?;
            let mut stderr = child.stderr.take().ok_or_else(err)?;
            let (mut out, mut errors) = (Output::default(), Output::default());
            let (mut out_done, mut err_done, mut url_sent, mut code_sent) =
                (false, false, false, false);
            let (mut a, mut b) = (Zeroizing::new([0u8; 1024]), Zeroizing::new([0u8; 1024]));
            while !out_done || !err_done {
                let next = tokio::select! {
                    n=stdout.read(a.as_mut()), if !out_done=>{
                        let n=n.map_err(|_| TransportError::Transport)?;
                        out_done=n==0; out.feed(&a[..n])?
                    },
                    n=stderr.read(b.as_mut()), if !err_done=>{
                        let n=n.map_err(|_| TransportError::Transport)?;
                        err_done=n==0; errors.feed(&b[..n])?
                    },
                    value=codes.recv(), if url_sent && !code_sent=>{
                        let value=value.ok_or(TransportError::Cancelled)?;
                        stdin.write_all(value.0.as_bytes()).await.map_err(|_| TransportError::Transport)?;
                        stdin.write_all(b"\n").await.map_err(|_| TransportError::Transport)?;
                        stdin.flush().await.map_err(|_| TransportError::Transport)?;
                        code_sent=true; None
                    },
                };
                if let Some(url) = next {
                    if url_sent {
                        return Err(TransportError::Transport);
                    }
                    events
                        .send(LoginEvent::AuthorizationUrl(url))
                        .await
                        .map_err(|_| TransportError::Cancelled)?;
                    url_sent = true;
                }
            }
            let status = child.wait().await.map_err(|_| TransportError::Transport)?;
            if !status.success() || !url_sent || !(out.successful() || errors.successful()) {
                return Err(TransportError::Transport);
            }
            Ok(())
        };
        let result = tokio::select! {biased;
            _=external.cancelled()=>Err(TransportError::Cancelled),
            _=events.closed()=>Err(TransportError::Cancelled),
            r=tokio::time::timeout(Duration::from_secs(300),operation)=>r.unwrap_or(Err(TransportError::Timeout)),
        };
        cancel.cancel();
        proxy.0.abort();
        if self.remove(&name).await.is_err() {
            let _ = self.block_environment(environment);
            return Err(TransportError::Transport);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn code_returns_to_original_process_and_success_requires_clean_exit() {
        for behavior in ["web-login", "web-login-nonzero", "web-login-untrusted"] {
            let f = crate::claude_code::fixture::Fixture::new();
            f.behavior(behavior);
            let (events, mut rx) = mpsc::channel(1);
            let (codes, mut input) = mpsc::channel(1);
            let cancel = CancellationToken::new();
            let run = f.profile.login_web(
                "00000000-0000-4000-8000-000000000001",
                &events,
                &mut input,
                &cancel,
            );
            let interact = async {
                if behavior != "web-login-untrusted" {
                    let Some(LoginEvent::AuthorizationUrl(url)) = rx.recv().await else {
                        panic!("missing URL");
                    };
                    assert!(url.starts_with("https://claude.com/cai/oauth/authorize?"));
                    codes
                        .send(
                            LoginCode::new(Zeroizing::new("synthetic-code#state".into())).unwrap(),
                        )
                        .await
                        .ok()
                        .unwrap();
                }
            };
            let (result, ()) = tokio::join!(run, interact);
            assert_eq!(result.is_ok(), behavior == "web-login");
            assert_eq!(f.launches(), 1);
            assert!(f.directory.path().join("removed").exists());
        }
    }
    #[test]
    fn code_rejects_terminal_control_and_multiline_input() {
        for bad in [
            "",
            "short",
            "abcdefgh\ncommand",
            "abcdefgh\r",
            "abcdefgh\x1b",
        ] {
            assert!(LoginCode::new(Zeroizing::new(bad.into())).is_err());
        }
    }
}
