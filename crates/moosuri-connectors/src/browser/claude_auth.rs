//! Private worker for one Claude sign-in inside the disposable Google-image capsule.
//!
//! The host supplies exactly one official authorization URL. The worker exposes no
//! URL, script, CDP or page-content API: only sanitized screenshots (origin-only URL),
//! proof-bound human input, and, once, the `code#state` of the exact top-level callback
//! that it captured through Fetch *before* any network request was made for it.
//! Nothing here logs or serializes a URL, code, credential or page content.
use super::{
    BrowserConfig, BrowserError,
    capsule::{WorkspaceFrame, WorkspaceInput},
    private_proxy,
    workspace::claude::ClaudeWorkspace,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader},
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

pub const PROTOCOL: &str = "moosuri-claude-auth-v1";
const AUTHORIZE_PREFIX: &str = "https://claude.com/cai/oauth/authorize?";
const CALLBACK_PREFIX: &str = "https://platform.claude.com/oauth/code/callback?";
/// The CLI 2.1.322 `MANUAL_REDIRECT_URL`; the only redirect this worker accepts.
pub(crate) const CALLBACK: &str = "https://platform.claude.com/oauth/code/callback";
/// Browser lifetime. The host actor allows a short margin beyond it.
pub(crate) const ATTEMPT_SECONDS: u64 = 15 * 60;
/// Every host->worker line, and every secret-bearing worker->host line, is below this.
pub(crate) const LINE_LIMIT: usize = 16 * 1024;

/// A secret string read from the wire. It is zeroized on drop and has no Debug.
/// serde/JSON parsing makes short-lived copies that cannot all be zeroized; the
/// disposable worker process and the host actor's buffers are the lifetime boundary.
#[derive(Deserialize)]
#[serde(transparent)]
pub(crate) struct Secret(String);
impl Secret {
    pub(crate) fn as_str(&self) -> &str {
        &self.0
    }
    pub(crate) fn into_zeroizing(mut self) -> Zeroizing<String> {
        Zeroizing::new(std::mem::take(&mut self.0))
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

fn same(a: &str, b: &str) -> bool {
    a.len() == b.len() && a.bytes().zip(b.bytes()).fold(0u8, |d, (x, y)| d | (x ^ y)) == 0
}
fn valid_code(code: &str) -> bool {
    (8..=2048).contains(&code.len()) && code.bytes().all(|b| b.is_ascii_graphic() && b != b'#')
}

/// A validated official authorization URL and the single state it carries.
pub(crate) struct AuthorizeUrl {
    url: Zeroizing<String>,
    state: Zeroizing<String>,
}
impl AuthorizeUrl {
    /// Exact official origin/path, no userinfo/port/fragment, exactly one `state`
    /// and exactly one `redirect_uri` equal to the official manual callback.
    pub(crate) fn parse(raw: &str) -> Result<Self, BrowserError> {
        if raw.len() > 4096
            || !raw.starts_with(AUTHORIZE_PREFIX)
            || raw.contains('#')
            || !raw.bytes().all(|b| b.is_ascii_graphic())
        {
            return Err(BrowserError::InvalidUrl);
        }
        let url = url::Url::parse(raw).map_err(|_| BrowserError::InvalidUrl)?;
        if url.scheme() != "https"
            || url.host_str() != Some("claude.com")
            || url.path() != "/cai/oauth/authorize"
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return Err(BrowserError::InvalidUrl);
        }
        let (mut states, mut redirects) = (Vec::new(), Vec::new());
        for (key, value) in url.query_pairs() {
            match &*key {
                "state" => states.push(value),
                "redirect_uri" => redirects.push(value),
                _ => {}
            }
        }
        if states.len() != 1 || redirects.len() != 1 || redirects[0] != CALLBACK {
            return Err(BrowserError::InvalidUrl);
        }
        let state = &states[0];
        if !(8..=256).contains(&state.len())
            || !state
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._~".contains(&b))
        {
            return Err(BrowserError::InvalidUrl);
        }
        Ok(Self {
            url: Zeroizing::new(raw.to_owned()),
            state: Zeroizing::new(state.to_string()),
        })
    }
    pub(crate) fn as_str(&self) -> &str {
        &self.url
    }
    /// Accept only the exact official callback: no userinfo/port/fragment, exactly one
    /// `code` and one `state` (equal to ours) and nothing else. Returns `code#state`.
    pub(crate) fn accept_callback(&self, raw: &str) -> Option<Zeroizing<String>> {
        if raw.len() > 4096 || !raw.starts_with(CALLBACK_PREFIX) || raw.contains('#') {
            return None;
        }
        let url = url::Url::parse(raw).ok()?;
        if url.scheme() != "https"
            || url.host_str() != Some("platform.claude.com")
            || url.path() != "/oauth/code/callback"
            || url.port().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.fragment().is_some()
        {
            return None;
        }
        let (mut code, mut state, mut pairs) = (None, None, 0);
        for (key, value) in url.query_pairs() {
            pairs += 1;
            match &*key {
                "code" if code.is_none() => code = Some(value),
                "state" if state.is_none() => state = Some(value),
                _ => return None,
            }
        }
        let (code, state) = (code?, state?);
        if pairs != 2 || !valid_code(&code) || !same(&state, &self.state) {
            return None;
        }
        Some(Zeroizing::new(format!("{code}#{state}")))
    }
    /// Host-side proof that a worker-returned value is one `code#state` for this attempt.
    pub(crate) fn owns_code(&self, value: &str) -> bool {
        value
            .split_once('#')
            .is_some_and(|(code, state)| valid_code(code) && same(state, &self.state))
    }
}

/// `about:blank` or `https://host[:port]`: never a path, query, fragment or userinfo.
pub(crate) fn sanitized_origin(raw: &str) -> Option<String> {
    if matches!(raw, "" | "chrome-error://chromewebdata/") {
        return Some(String::new());
    }
    if raw == "about:blank" {
        return Some(raw.to_owned());
    }
    let url = url::Url::parse(raw).ok()?;
    if url.scheme() != "https" {
        return None;
    }
    let host = url.host_str()?;
    Some(match url.port() {
        Some(port) => format!("https://{host}:{port}"),
        None => format!("https://{host}"),
    })
}
pub(crate) fn is_sanitized_origin(raw: &str) -> bool {
    raw.len() <= 300 && sanitized_origin(raw).as_deref() == Some(raw)
}

#[derive(Serialize)]
struct InitOut<'a> {
    protocol: &'static str,
    tenant: Uuid,
    authorization: &'a str,
}
#[derive(Serialize)]
struct InputOut<'a> {
    op: &'static str,
    frame_token: &'a str,
    input: &'a WorkspaceInput,
}
#[derive(Serialize)]
struct CodeOut<'a> {
    ok: bool,
    code: &'a str,
}
fn line<T: Serialize>(value: &T, capacity: usize) -> Result<Zeroizing<Vec<u8>>, BrowserError> {
    // Pre-sized so the secret-bearing buffer never reallocates (and leaves a copy).
    let mut bytes = Zeroizing::new(Vec::with_capacity(capacity));
    serde_json::to_writer(&mut *bytes, value).map_err(|_| BrowserError::Protocol)?;
    bytes.push(b'\n');
    if bytes.len() > capacity {
        return Err(BrowserError::OutputTooLarge);
    }
    Ok(bytes)
}
pub(crate) fn init_line(
    tenant: Uuid,
    authorize: &AuthorizeUrl,
) -> Result<Zeroizing<Vec<u8>>, BrowserError> {
    line(
        &InitOut {
            protocol: PROTOCOL,
            tenant,
            authorization: authorize.as_str(),
        },
        LINE_LIMIT,
    )
}
pub(crate) fn input_line(
    frame_token: &str,
    input: &WorkspaceInput,
) -> Result<Zeroizing<Vec<u8>>, BrowserError> {
    line(
        &InputOut {
            op: "input",
            frame_token,
            input,
        },
        LINE_LIMIT,
    )
}
pub(crate) const POLL_LINE: &[u8] = b"{\"op\":\"poll\"}\n";

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Init {
    protocol: String,
    tenant: Uuid,
    authorization: Secret,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    // Struct variants: serde ignores extra fields on an internally tagged *unit* variant.
    Poll {},
    Input {
        frame_token: String,
        input: WorkspaceInput,
    },
    Shutdown {},
}

type Line = Result<Zeroizing<Vec<u8>>, ()>;
/// Reads bounded host lines on a task so a partial line is never lost to a `select!`.
/// EOF closes the channel; an oversized or unterminated line is sent once as `Err`.
fn spawn_lines() -> mpsc::Receiver<Line> {
    let (tx, rx) = mpsc::channel(1);
    tokio::spawn(async move {
        let mut reader = BufReader::new(tokio::io::stdin());
        loop {
            let mut bytes = Zeroizing::new(Vec::with_capacity(LINE_LIMIT + 1));
            let read = (&mut reader)
                .take(LINE_LIMIT as u64 + 1)
                .read_until(b'\n', &mut bytes)
                .await;
            let line = match read {
                Ok(0) => return,
                Ok(_) if bytes.len() <= LINE_LIMIT && bytes.ends_with(b"\n") => Ok(bytes),
                _ => Err(()),
            };
            let bad = line.is_err();
            if tx.send(line).await.is_err() || bad {
                return;
            }
        }
    });
    rx
}

async fn send_bytes(
    output: &mut (impl AsyncWrite + Unpin),
    bytes: &[u8],
) -> Result<(), BrowserError> {
    tokio::time::timeout(Duration::from_secs(10), async {
        output.write_all(bytes).await?;
        output.flush().await
    })
    .await
    .map_err(|_| BrowserError::Timeout)?
    .map_err(|_| BrowserError::Protocol)
}
async fn send_json(
    output: &mut (impl AsyncWrite + Unpin),
    value: &serde_json::Value,
) -> Result<(), BrowserError> {
    let mut bytes = serde_json::to_vec(value).map_err(|_| BrowserError::Protocol)?;
    bytes.push(b'\n');
    send_bytes(output, &bytes).await
}
async fn send_code(
    output: &mut (impl AsyncWrite + Unpin),
    code: &Zeroizing<String>,
) -> Result<(), BrowserError> {
    let bytes = line(
        &CodeOut {
            ok: true,
            code: code.as_str(),
        },
        code.len() * 6 + 64,
    )?;
    send_bytes(output, &bytes).await
}
/// Only a stale or invalid-handle result (and capacity) leaves the attempt usable.
fn recoverable(error: &BrowserError) -> bool {
    matches!(
        error,
        BrowserError::StalePage | BrowserError::InvalidHandle | BrowserError::Capacity
    )
}
async fn send_error(
    output: &mut (impl AsyncWrite + Unpin),
    error: &BrowserError,
) -> (Result<(), BrowserError>, bool) {
    let code = match error {
        BrowserError::StalePage => "stale_page",
        BrowserError::InvalidHandle => "invalid_handle",
        BrowserError::Capacity => "capacity",
        _ => "failed",
    };
    (
        send_json(output, &json!({"ok":false,"error":code})).await,
        recoverable(error),
    )
}

/// What the loop needs from the browser. `ClaudeWorkspace` is the real one.
trait Session {
    async fn watch(&mut self) -> Result<(), BrowserError>;
    async fn captured_code(&mut self) -> Option<Zeroizing<String>>;
    async fn human_frame(&mut self) -> Result<WorkspaceFrame, BrowserError>;
    async fn human_input(
        &mut self,
        frame_token: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError>;
}
impl Session for ClaudeWorkspace {
    async fn watch(&mut self) -> Result<(), BrowserError> {
        ClaudeWorkspace::watch(self).await
    }
    async fn captured_code(&mut self) -> Option<Zeroizing<String>> {
        ClaudeWorkspace::captured_code(self).await
    }
    async fn human_frame(&mut self) -> Result<WorkspaceFrame, BrowserError> {
        ClaudeWorkspace::human_frame(self).await
    }
    async fn human_input(
        &mut self,
        frame_token: &str,
        input: WorkspaceInput,
    ) -> Result<(), BrowserError> {
        ClaudeWorkspace::human_input(self, frame_token, input).await
    }
}

enum View {
    Frame(WorkspaceFrame),
    Code(Zeroizing<String>),
}
async fn poll_once(session: &mut impl Session) -> Result<View, BrowserError> {
    session.watch().await?;
    if let Some(code) = session.captured_code().await {
        return Ok(View::Code(code));
    }
    Ok(View::Frame(session.human_frame().await?))
}

/// Serves host requests one at a time, and independently watches for the callback
/// every 250 ms so it is captured even when the host is not polling. A callback
/// failure noticed between requests cannot be written unsolicited (it would be read
/// as the next reply); it is held and answered to the next request, which ends the
/// attempt. After the code has been returned once, no further request is served.
async fn session_loop(
    session: &mut impl Session,
    lines: &mut mpsc::Receiver<Line>,
    output: &mut (impl AsyncWrite + Unpin),
    stop: &CancellationToken,
    lifetime: Duration,
) {
    let mut tick = tokio::time::interval(Duration::from_millis(250));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    let deadline = tokio::time::sleep(lifetime);
    tokio::pin!(deadline);
    let (mut failed, mut delivered) = (false, false);
    loop {
        let bytes = tokio::select! { biased;
            _ = stop.cancelled() => break,
            _ = &mut deadline => break,
            next = lines.recv() => match next { Some(Ok(bytes)) => bytes, _ => break },
            _ = tick.tick() => {
                if !failed && session.watch().await.is_err() {
                    failed = true;
                }
                continue;
            }
        };
        let command = serde_json::from_slice::<Command>(&bytes);
        drop(bytes);
        let command = match command {
            Ok(Command::Shutdown {}) | Err(_) => break,
            Ok(command) => command,
        };
        if failed || delivered {
            let _ = send_json(output, &json!({"ok":false,"error":"failed"})).await;
            break;
        }
        let (written, keep) = match command {
            Command::Poll {} => match poll_once(session).await {
                Ok(View::Frame(frame)) => (
                    send_json(output, &json!({"ok":true,"frame":frame})).await,
                    true,
                ),
                Ok(View::Code(code)) => {
                    delivered = true;
                    (send_code(output, &code).await, true)
                }
                Err(error) => send_error(output, &error).await,
            },
            Command::Input { frame_token, input } => {
                match session.human_input(&frame_token, input).await {
                    Ok(()) => (
                        send_json(output, &json!({"ok":true,"accepted":true})).await,
                        true,
                    ),
                    Err(error) => send_error(output, &error).await,
                }
            }
            Command::Shutdown {} => break,
        };
        if written.is_err() || !keep {
            break;
        }
    }
}

fn config(tenant: Uuid) -> Result<BrowserConfig, BrowserError> {
    Ok(BrowserConfig {
        tenant,
        chromium: PathBuf::from("/usr/bin/google-chrome-stable"),
        profile_root: PathBuf::from("/profiles"),
        proxy: private_proxy(
            &std::env::var("MOOSURI_BROWSER_PROXY_IP")
                .map_err(|_| BrowserError::IsolationRequired)?,
        )?,
        javascript_enabled: true,
    })
}

async fn init(
    lines: &mut mpsc::Receiver<Line>,
    tenant: Uuid,
    stop: &CancellationToken,
) -> Option<AuthorizeUrl> {
    let received = tokio::select! { biased;
        _ = stop.cancelled() => return None,
        received = tokio::time::timeout(Duration::from_secs(10), lines.recv()) => received,
    };
    let Ok(Some(Ok(bytes))) = received else {
        return None;
    };
    let init = serde_json::from_slice::<Init>(&bytes).ok();
    drop(bytes);
    let init = init?;
    if init.protocol != PROTOCOL || init.tenant != tenant {
        return None;
    }
    AuthorizeUrl::parse(init.authorization.as_str()).ok()
}

/// Returns whether everything this worker owns (Chrome, X, profile) was torn down.
async fn run(
    lines: &mut mpsc::Receiver<Line>,
    output: &mut (impl AsyncWrite + Unpin),
    tenant: Uuid,
    stop: &CancellationToken,
) -> bool {
    let unavailable = json!({"status":"unavailable"});
    let Some(authorize) = init(lines, tenant, stop).await else {
        let _ = send_json(output, &unavailable).await;
        return true;
    };
    let Ok(config) = config(tenant) else {
        let _ = send_json(output, &unavailable).await;
        return true;
    };
    let cancel = CancellationToken::new();
    let launching = ClaudeWorkspace::launch(config, authorize, cancel.clone());
    tokio::pin!(launching);
    // Anything from the host before readiness (EOF, a stray line, a signal) cancels.
    let mut interrupted = false;
    let launched = loop {
        tokio::select! { biased;
            _ = stop.cancelled(), if !interrupted => { interrupted = true; cancel.cancel(); }
            _ = lines.recv(), if !interrupted => { interrupted = true; cancel.cancel(); }
            result = &mut launching => break result,
        }
    };
    let Ok(mut workspace) = launched else {
        let _ = send_json(output, &unavailable).await;
        return true;
    };
    if interrupted
        || send_json(output, &json!({"status":"ready","tenant":tenant}))
            .await
            .is_err()
    {
        workspace.shutdown().await;
        return true;
    }
    session_loop(
        &mut workspace,
        lines,
        output,
        stop,
        Duration::from_secs(ATTEMPT_SECONDS),
    )
    .await;
    workspace.shutdown().await;
    true
}

async fn stopped() {
    #[cfg(unix)]
    {
        let Ok(mut term) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            return;
        };
        tokio::select! { _ = term.recv() => {}, _ = tokio::signal::ctrl_c() => {} }
    }
    #[cfg(not(unix))]
    let _ = tokio::signal::ctrl_c().await;
}

/// Entry point of `--claude-auth`. Exit success (`true`) is the cleanup receipt the
/// launcher requires: it is returned only after Chrome, X and the profile have been
/// stopped and awaited (or when nothing was ever launched), never earlier.
pub async fn serve() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Keep the authorization URL, state and code out of core dumps/proc access.
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: 0,
        };
        if unsafe { libc::prctl(libc::PR_SET_DUMPABLE, 0, 0, 0, 0) } != 0
            || unsafe { libc::setrlimit(libc::RLIMIT_CORE, &limit) } != 0
        {
            return false;
        }
    }
    let mut output = tokio::io::stdout();
    let Some(tenant) = std::env::var("MOOSURI_BROWSER_TENANT")
        .ok()
        .and_then(|s| s.parse::<Uuid>().ok())
        .filter(|id| !id.is_nil())
    else {
        let _ = send_json(&mut output, &json!({"status":"unavailable"})).await;
        return true;
    };
    let stop = CancellationToken::new();
    let watcher = tokio::spawn({
        let stop = stop.clone();
        async move {
            stopped().await;
            stop.cancel();
        }
    });
    let mut lines = spawn_lines();
    let clean = run(&mut lines, &mut output, tenant, &stop).await;
    watcher.abort();
    clean
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::VecDeque;

    const STATE: &str = "state_VALUE-123~abcdefgh";
    fn authorization(state: &str) -> String {
        format!(
            "https://claude.com/cai/oauth/authorize?code=true&client_id=00000000-0000-4000-8000-000000000000&response_type=code&redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback&scope=user%3Ainference&code_challenge=abcdefghijklmnopqrstuvwxyz0123456789ABCDEFG&code_challenge_method=S256&state={state}"
        )
    }
    fn callback(query: &str) -> String {
        format!("https://platform.claude.com/oauth/code/callback?{query}")
    }

    #[test]
    fn only_the_exact_official_authorization_url_with_one_state_and_redirect_is_accepted() {
        let good = authorization(STATE);
        assert!(AuthorizeUrl::parse(&good).is_ok());
        let redirect = "redirect_uri=https%3A%2F%2Fplatform.claude.com%2Foauth%2Fcode%2Fcallback";
        let bad = [
            good.replace("https://claude.com", "http://claude.com"),
            good.replace("https://claude.com", "https://claude.com:444"),
            good.replace("https://claude.com", "https://user@claude.com"),
            good.replace("https://claude.com", "https://claude.ai"),
            good.replace(
                "https://claude.com",
                "https://evil.example/https://claude.com",
            ),
            good.replace("/cai/oauth/authorize", "/cai/oauth/authorize/x"),
            good.replace("platform.claude.com", "platform.claude.example"),
            good.replace(
                redirect,
                "redirect_uri=http%3A%2F%2Flocalhost%3A1%2Fcallback",
            ),
            format!("{good}&state=another_state_value"),
            format!("{good}&{redirect}"),
            good.replace(redirect, "x=1"),
            good.replace(&format!("state={STATE}"), "x=1"),
            good.replace(STATE, "short"),
            good.replace(STATE, "bad%20state%20value"),
            format!("{good}#fragment"),
            format!("{good} "),
            format!("{good}&pad={}", "a".repeat(5000)),
            "https://claude.com/cai/oauth/authorize".to_owned(),
        ];
        for url in bad {
            assert!(AuthorizeUrl::parse(&url).is_err(), "{url}");
        }
    }

    #[test]
    fn callback_is_exactly_one_code_and_the_matching_state() {
        let auth = AuthorizeUrl::parse(&authorization(STATE)).unwrap();
        let code = "abcDEF123456_-";
        let accepted = auth
            .accept_callback(&callback(&format!("code={code}&state={STATE}")))
            .unwrap();
        assert_eq!(accepted.as_str(), format!("{code}#{STATE}"));
        assert!(auth.owns_code(&accepted));
        assert!(
            auth.accept_callback(&callback(&format!("state={STATE}&code={code}"),))
                .is_some()
        );
        let wrong_state = "state=another_state_value";
        for url in [
            callback(&format!("code={code}&{wrong_state}")),
            callback(&format!("code={code}")),
            callback(&format!("state={STATE}")),
            callback(&format!("code={code}&code={code}&state={STATE}")),
            callback(&format!("code={code}&state={STATE}&state={STATE}")),
            callback(&format!("code={code}&state={STATE}&extra=1")),
            callback(&format!("code={code}&state={STATE}&error=access_denied")),
            callback(&format!("error=access_denied&state={STATE}")),
            callback(&format!("code=short&state={STATE}")),
            callback(&format!("code=abc%23defghijk&state={STATE}")),
            callback(&format!("code=abc+defghijk&state={STATE}")),
            callback(&format!("code=&state={STATE}")),
            callback(&format!("code={code}&state={STATE}#fragment")),
            format!("https://platform.claude.com/oauth/code/callback/x?code={code}&state={STATE}"),
            format!(
                "https://platform.claude.com:8443/oauth/code/callback?code={code}&state={STATE}"
            ),
            format!("https://u@platform.claude.com/oauth/code/callback?code={code}&state={STATE}"),
            format!("http://platform.claude.com/oauth/code/callback?code={code}&state={STATE}"),
            format!("https://claude.com/oauth/code/callback?code={code}&state={STATE}"),
            format!(
                "https://platform.claude.com/oauth/code/callback.evil?code={code}&state={STATE}"
            ),
        ] {
            assert!(auth.accept_callback(&url).is_none(), "{url}");
        }
        for value in [
            format!("{code}#other_state_value"),
            format!("{code}#{STATE}#{STATE}"),
            format!("{code}{STATE}"),
            format!("#{STATE}"),
            format!("short#{STATE}"),
        ] {
            assert!(!auth.owns_code(&value), "{value}");
        }
    }

    #[test]
    fn frame_urls_are_origin_only() {
        for good in [
            "", // Chrome's own error page has no public origin.
            "about:blank",
            "https://claude.com",
            "https://auth.example:8443",
        ] {
            assert!(is_sanitized_origin(good), "{good}");
        }
        for bad in [
            "https://claude.com/",
            "https://claude.com/cai/oauth/authorize?state=x",
            "https://claude.com?x=1",
            "https://claude.com#x",
            "https://u@claude.com",
            "http://claude.com",
            "https://CLAUDE.com",
            "javascript:alert(1)",
            "chrome-error://chromewebdata/",
        ] {
            assert!(!is_sanitized_origin(bad), "{bad}");
        }
        assert_eq!(
            sanitized_origin(&callback("code=SECRETCODE&state=SECRETSTATE")).as_deref(),
            Some("https://platform.claude.com")
        );
        assert_eq!(sanitized_origin("file:///etc/passwd"), None);
    }

    #[tokio::test]
    async fn wire_shapes_are_exact_and_bounded() {
        let auth = AuthorizeUrl::parse(&authorization(STATE)).unwrap();
        let tenant = Uuid::new_v4();
        let init = init_line(tenant, &auth).unwrap();
        let parsed: Init = serde_json::from_slice(&init).unwrap();
        assert_eq!(parsed.protocol, PROTOCOL);
        assert_eq!(parsed.authorization.as_str(), auth.as_str());
        assert_eq!(parsed.tenant, tenant);
        assert!(init.ends_with(b"\n"));
        let input = WorkspaceInput::InsertText {
            text: "p\"a\\ss\nword".into(),
        };
        let line = input_line(&"a".repeat(64), &input).unwrap();
        assert!(matches!(
            serde_json::from_slice::<Command>(&line),
            Ok(Command::Input { .. })
        ));
        assert!(matches!(
            serde_json::from_slice::<Command>(POLL_LINE),
            Ok(Command::Poll {})
        ));
        for bad in [
            r#"{"op":"poll","x":1}"#,
            r#"{"op":"open","url":"https://example.com"}"#,
            r#"{"op":"evaluate","expression":"1"}"#,
            r#"{"op":"input","frame_token":"a","input":{"kind":"navigate","url":"x"}}"#,
            r#"{"op":"input","input":{"kind":"key","key":"enter"}}"#,
            r#"{"op":"code"}"#,
            r#"{}"#,
        ] {
            assert!(serde_json::from_str::<Command>(bad).is_err(), "{bad}");
        }
        let huge = WorkspaceInput::InsertText {
            text: "\"".repeat(4096),
        };
        // Escaped worst case still fits the fixed buffer, so it never reallocates.
        assert!(input_line(&"a".repeat(64), &huge).is_ok());
        let secret = Zeroizing::new("CODE#STATE".to_owned());
        let mut out = Vec::new();
        send_code(&mut out, &secret).await.unwrap();
        assert_eq!(out, b"{\"ok\":true,\"code\":\"CODE#STATE\"}\n");
    }

    #[derive(Default)]
    struct Fake {
        watch_fails: bool,
        code: Option<Zeroizing<String>>,
        frames: VecDeque<Result<WorkspaceFrame, BrowserError>>,
        input_results: VecDeque<Result<(), BrowserError>>,
        inputs: usize,
    }
    impl Session for Fake {
        async fn watch(&mut self) -> Result<(), BrowserError> {
            if self.watch_fails {
                Err(BrowserError::Protocol)
            } else {
                Ok(())
            }
        }
        async fn captured_code(&mut self) -> Option<Zeroizing<String>> {
            self.code.take()
        }
        async fn human_frame(&mut self) -> Result<WorkspaceFrame, BrowserError> {
            self.frames
                .pop_front()
                .unwrap_or(Err(BrowserError::Protocol))
        }
        async fn human_input(&mut self, _: &str, _: WorkspaceInput) -> Result<(), BrowserError> {
            self.inputs += 1;
            self.input_results.pop_front().unwrap_or(Ok(()))
        }
    }
    fn frame() -> WorkspaceFrame {
        WorkspaceFrame {
            png_base64: "AAAA".into(),
            width: 1280,
            height: 900,
            url: "https://claude.com".into(),
            frame_token: "a".repeat(64),
        }
    }
    fn host_line(text: &str) -> Line {
        let mut bytes = text.as_bytes().to_vec();
        bytes.push(b'\n');
        Ok(Zeroizing::new(bytes))
    }
    async fn reply(reader: &mut BufReader<tokio::io::DuplexStream>) -> serde_json::Value {
        let mut text = String::new();
        reader.read_line(&mut text).await.unwrap();
        serde_json::from_str(&text).unwrap()
    }
    const INPUT: &str =
        r#"{"op":"input","frame_token":"aaaa","input":{"kind":"key","key":"enter"}}"#;

    #[tokio::test]
    async fn poll_serves_frames_then_the_code_exactly_once_and_then_nothing() {
        let mut fake = Fake::default();
        fake.frames.push_back(Ok(frame()));
        fake.frames.push_back(Err(BrowserError::StalePage));
        let (tx, mut rx) = mpsc::channel(1);
        let (mut out, read) = tokio::io::duplex(1 << 16);
        let mut read = BufReader::new(read);
        let stop = CancellationToken::new();
        let run = session_loop(&mut fake, &mut rx, &mut out, &stop, Duration::from_secs(20));
        let drive = async {
            tx.send(host_line(r#"{"op":"poll"}"#)).await.unwrap();
            let first = reply(&mut read).await;
            assert_eq!(first["ok"], true);
            assert_eq!(first["frame"]["url"], "https://claude.com");
            // Stale is recoverable: the attempt stays usable.
            tx.send(host_line(r#"{"op":"poll"}"#)).await.unwrap();
            assert_eq!(
                reply(&mut read).await,
                json!({"ok":false,"error":"stale_page"})
            );
            tx.send(host_line(INPUT)).await.unwrap();
            assert_eq!(reply(&mut read).await, json!({"ok":true,"accepted":true}));
            tx.send(host_line(r#"{"op":"poll"}"#)).await.unwrap();
            // Nothing queued: a protocol failure, which ends the loop.
            assert_eq!(reply(&mut read).await["ok"], false);
        };
        tokio::join!(run, drive);
        assert_eq!(fake.inputs, 1);

        let mut fake = Fake {
            code: Some(Zeroizing::new("CODE#STATE".into())),
            ..Fake::default()
        };
        let (tx, mut rx) = mpsc::channel(1);
        let (mut out, read) = tokio::io::duplex(1 << 16);
        let mut read = BufReader::new(read);
        let run = session_loop(&mut fake, &mut rx, &mut out, &stop, Duration::from_secs(20));
        let drive = async {
            tx.send(host_line(r#"{"op":"poll"}"#)).await.unwrap();
            assert_eq!(
                reply(&mut read).await,
                json!({"ok":true,"code":"CODE#STATE"})
            );
            // Delivered: any further request is refused and ends the attempt.
            tx.send(host_line(r#"{"op":"poll"}"#)).await.unwrap();
            assert_eq!(reply(&mut read).await, json!({"ok":false,"error":"failed"}));
        };
        tokio::join!(run, drive);
        assert!(fake.code.is_none());
    }

    #[tokio::test]
    async fn uncertain_input_is_never_replayed_and_ends_the_attempt() {
        let mut fake = Fake::default();
        fake.input_results.push_back(Err(BrowserError::Protocol));
        fake.input_results.push_back(Ok(()));
        let (tx, mut rx) = mpsc::channel(1);
        let (mut out, read) = tokio::io::duplex(1 << 16);
        let mut read = BufReader::new(read);
        let stop = CancellationToken::new();
        let run = session_loop(&mut fake, &mut rx, &mut out, &stop, Duration::from_secs(20));
        let drive = async {
            tx.send(host_line(INPUT)).await.unwrap();
            assert_eq!(reply(&mut read).await, json!({"ok":false,"error":"failed"}));
        };
        tokio::join!(run, drive);
        assert_eq!(fake.inputs, 1);
    }

    #[tokio::test]
    async fn a_callback_failure_between_requests_is_answered_to_the_next_request() {
        let mut fake = Fake {
            watch_fails: true,
            ..Fake::default()
        };
        fake.frames.push_back(Ok(frame()));
        let (tx, mut rx) = mpsc::channel(1);
        let (mut out, read) = tokio::io::duplex(1 << 16);
        let mut read = BufReader::new(read);
        let stop = CancellationToken::new();
        let run = session_loop(&mut fake, &mut rx, &mut out, &stop, Duration::from_secs(20));
        let drive = async {
            // The first tick fails immediately and is held; no unsolicited line appears.
            tokio::time::sleep(Duration::from_millis(100)).await;
            tx.send(host_line(r#"{"op":"poll"}"#)).await.unwrap();
            assert_eq!(reply(&mut read).await, json!({"ok":false,"error":"failed"}));
        };
        tokio::join!(run, drive);
        // The queued frame was never served.
        assert_eq!(fake.frames.len(), 1);
    }

    #[tokio::test]
    async fn malformed_commands_eof_and_shutdown_end_the_loop_without_a_reply() {
        for input in [
            Some(host_line(r#"{"op":"evaluate"}"#)),
            Some(host_line(r#"{"op":"shutdown"}"#)),
            Some(Err(())),
            None,
        ] {
            let mut fake = Fake::default();
            let (tx, mut rx) = mpsc::channel(1);
            let (mut out, read) = tokio::io::duplex(1 << 16);
            let stop = CancellationToken::new();
            let run = session_loop(&mut fake, &mut rx, &mut out, &stop, Duration::from_secs(20));
            let drive = async {
                if let Some(input) = input {
                    tx.send(input).await.unwrap();
                }
                drop(tx);
            };
            tokio::join!(run, drive);
            drop(out);
            let mut rest = Vec::new();
            let mut read = read;
            read.read_to_end(&mut rest).await.unwrap();
            assert!(rest.is_empty());
            assert_eq!(fake.inputs, 0);
        }
    }

    #[tokio::test]
    async fn stop_and_lifetime_end_the_loop() {
        let (_tx, mut rx) = mpsc::channel::<Line>(1);
        let mut out = Vec::new();
        let stop = CancellationToken::new();
        stop.cancel();
        session_loop(
            &mut Fake::default(),
            &mut rx,
            &mut out,
            &stop,
            Duration::from_secs(20),
        )
        .await;
        let stop = CancellationToken::new();
        session_loop(
            &mut Fake::default(),
            &mut rx,
            &mut out,
            &stop,
            Duration::from_millis(50),
        )
        .await;
        assert!(out.is_empty());
    }
}
