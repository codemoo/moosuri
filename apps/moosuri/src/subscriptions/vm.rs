//! Transfer-only native helper. No Store, model client, refresh or revoke path.
use super::private_state::PrivateState;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use moosuri_connectors::chatgpt_oauth::{
    AuthorizationAttempt, ChatGptAuthClient, IssuedRegistration,
    handoff::{HandoffContext, HandoffKey, MAX_ENVELOPE, envelope_digest, seal_handoff},
};
use moosuri_store::HandoffStatus;
use reqwest::{Client, header};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{self, Write},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use zeroize::{Zeroize, Zeroizing};
const MAX_FRAME: usize = 256 * 1024;
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Journal {
    version: u8,
    phase: String,
    origin: String,
    ssh_alias: String,
    context: HandoffContext,
    cookie: String,
    account: String,
    transfer_key: String,
    expires_at: i64,
    client_id: Option<String>,
    subject: Option<String>,
    envelope_digest: Option<Vec<u8>>,
}
impl Drop for Journal {
    fn drop(&mut self) {
        self.cookie.zeroize();
        self.transfer_key.zeroize();
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Frame {
    version: u8,
    origin: String,
    cookie: String,
    account: String,
    ticket: String,
    envelope: String,
}
impl Drop for Frame {
    fn drop(&mut self) {
        self.cookie.zeroize();
    }
}
fn failure() -> String {
    "vm_handoff_incomplete_retry_with_same_helper".into()
}
fn origin(value: &str) -> Result<String, String> {
    let u = url::Url::parse(value).map_err(|_| "invalid_https_server")?;
    let allowed = u.scheme() == "https";
    #[cfg(feature = "test-fixtures")]
    let allowed = allowed
        || (u.scheme() == "http" && u.host_str() == Some("127.0.0.1") && u.port().is_some());
    if !allowed
        || u.host_str().is_none()
        || !u.username().is_empty()
        || u.password().is_some()
        || u.path() != "/"
        || u.query().is_some()
        || u.fragment().is_some()
    {
        return Err("https_server_required".into());
    }
    Ok(u.origin().ascii_serialization())
}
fn ssh_alias(s: &str) -> Result<(), String> {
    if s.is_empty()
        || s.len() > 255
        || s.starts_with('-')
        || !s
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return Err("existing_ssh_host_alias_required".into());
    }
    Ok(())
}
fn prompt(label: &str) -> Result<String, String> {
    print!("{label}: ");
    io::stdout().flush().map_err(|_| failure())?;
    let mut s = String::new();
    io::stdin().read_line(&mut s).map_err(|_| failure())?;
    if s.len() > 1024 {
        return Err("input_too_long".into());
    }
    Ok(s.trim().to_owned())
}
fn client() -> Result<Client, String> {
    Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(40))
        .build()
        .map_err(|_| failure())
}
async fn body(mut r: reqwest::Response, limit: usize) -> Result<Zeroizing<Vec<u8>>, String> {
    if !r.status().is_success() {
        return Err(failure());
    }
    let mut out = Zeroizing::new(Vec::new());
    while let Some(chunk) = r.chunk().await.map_err(|_| failure())? {
        if out.len() + chunk.len() > limit {
            return Err(failure());
        }
        out.extend_from_slice(&chunk);
    }
    Ok(out)
}
fn api(c: &Client, j: &Journal, method: reqwest::Method, suffix: &str) -> reqwest::RequestBuilder {
    c.request(
        method,
        format!(
            "{}/api/v1/settings/chatgpt/handoffs/{}{}",
            j.origin, j.context.ticket, suffix
        ),
    )
    .header(header::ORIGIN, &j.origin)
    .header(header::COOKIE, &j.cookie)
    .header("x-moosuri-account", &j.account)
}
fn save(state: &PrivateState, j: &Journal) -> Result<(), String> {
    let plain = Zeroizing::new(serde_json::to_vec(j).map_err(|_| failure())?);
    let cipher = state
        .key
        .seal_private_journal(&plain)
        .map_err(|_| failure())?;
    state.write("journal", &cipher)
}
fn load(state: &PrivateState) -> Result<Option<Journal>, String> {
    let Some(cipher) = state.read("journal", MAX_ENVELOPE)? else {
        return Ok(None);
    };
    let plain = state
        .key
        .open_private_journal(&cipher)
        .map_err(|_| failure())?;
    let j: Journal = serde_json::from_slice(&plain).map_err(|_| failure())?;
    if j.version != 1 || origin(&j.origin)? != j.origin || j.context.aad().is_err() {
        return Err(failure());
    }
    ssh_alias(&j.ssh_alias)?;
    Ok(Some(j))
}
async fn status(c: &Client, j: &Journal) -> Result<HandoffStatus, String> {
    let bytes = body(
        api(c, j, reqwest::Method::GET, "")
            .send()
            .await
            .map_err(|_| failure())?,
        16384,
    )
    .await?;
    serde_json::from_slice(&bytes).map_err(|_| failure())
}
fn confirm(j: &Journal, s: &HandoffStatus, key: &HandoffKey) -> Result<bool, String> {
    if s.phase != "imported" {
        return Ok(false);
    }
    let r = s.receipt.as_ref().ok_or_else(failure)?;
    let mac = s.receipt_mac.as_ref().ok_or_else(failure)?;
    if r.context != j.context || Some(&r.envelope_digest) != j.envelope_digest.as_ref() {
        return Err(failure());
    }
    r.verify(key, mac).map_err(|_| failure())?;
    Ok(true)
}
async fn acknowledge(c: &Client, state: &PrivateState, j: &mut Journal) -> Result<(), String> {
    // Persist safe tombstone before removing the encrypted renewable payload.
    j.phase = "handed_off".into();
    j.transfer_key.zeroize();
    j.transfer_key.clear();
    let cookie = Zeroizing::new(std::mem::take(&mut j.cookie));
    save(state, j)?;
    state.remove("envelope")?;
    // This is the helper's own newly-created Moosuri session, never browser state.
    let _ = c
        .post(format!("{}/api/v1/auth/logout", j.origin))
        .header(header::ORIGIN, &j.origin)
        .header(header::COOKIE, cookie.as_str())
        .header("x-moosuri-account", &j.account)
        .send()
        .await;
    println!(
        "ChatGPT connected on the server. Return to personal model settings to verify and select a text model."
    );
    Ok(())
}
async fn frame_read<R: AsyncRead + Unpin>(r: &mut R) -> Result<Zeroizing<Vec<u8>>, String> {
    let length = r.read_u32().await.map_err(|_| failure())? as usize;
    if length == 0 || length > MAX_FRAME {
        return Err(failure());
    }
    let mut bytes = Zeroizing::new(vec![0; length]);
    r.read_exact(&mut bytes).await.map_err(|_| failure())?;
    let mut extra = [0; 1];
    if r.read(&mut extra).await.map_err(|_| failure())? != 0 {
        return Err(failure());
    }
    Ok(bytes)
}
async fn pipe(j: &Journal, envelope: &[u8]) -> Result<(), String> {
    ssh_alias(&j.ssh_alias)?;
    let f = Frame {
        version: 1,
        origin: j.origin.clone(),
        cookie: j.cookie.clone(),
        account: j.account.clone(),
        ticket: j.context.ticket.clone(),
        envelope: URL_SAFE_NO_PAD.encode(envelope),
    };
    let bytes = Zeroizing::new(serde_json::to_vec(&f).map_err(|_| failure())?);
    if bytes.len() > MAX_FRAME {
        return Err(failure());
    }
    let mut child = tokio::process::Command::new("ssh")
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "--",
            &j.ssh_alias,
            "moosuri subscriptions chatgpt-receive --stdin",
        ])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| "ssh_transfer_failed".to_string())?;
    let mut stdin = child.stdin.take().ok_or_else(failure)?;
    stdin
        .write_u32(bytes.len() as u32)
        .await
        .map_err(|_| failure())?;
    stdin.write_all(&bytes).await.map_err(|_| failure())?;
    stdin.shutdown().await.map_err(|_| failure())?;
    drop(stdin);
    let mut stdout = child.stdout.take().ok_or_else(failure)?;
    let output = frame_read(&mut stdout).await?;
    // SSH output is only a bounded safe receipt; independent HTTPS status is authority.
    let _: HandoffStatus = serde_json::from_slice(&output).map_err(|_| failure())?;
    if !child.wait().await.map_err(|_| failure())?.success() {
        return Err("ssh_transfer_failed".into());
    }
    Ok(())
}
async fn transfer(c: &Client, state: &PrivateState, j: &mut Journal) -> Result<(), String> {
    let key = HandoffKey::from_private_transport(&j.transfer_key).map_err(|_| failure())?;
    if let Ok(s) = status(c, j).await
        && confirm(j, &s, &key)?
    {
        return acknowledge(c, state, j).await;
    }
    let envelope = state.read("envelope", MAX_ENVELOPE)?.ok_or_else(failure)?;
    if Some(envelope_digest(&envelope).as_slice()) != j.envelope_digest.as_deref() {
        return Err(failure());
    }
    // Missing SSH acknowledgement never re-enables source refresh or destroys journal.
    let _ = tokio::time::timeout(Duration::from_secs(90), pipe(j, &envelope)).await;
    let s = status(c, j).await?;
    if confirm(j, &s, &key)? {
        acknowledge(c, state, j).await
    } else {
        Err(failure())
    }
}
async fn callback(
    listener: TcpListener,
    attempt: AuthorizationAttempt,
) -> Result<
    (
        moosuri_connectors::chatgpt_oauth::PendingCode,
        tokio::net::TcpStream,
    ),
    String,
> {
    let port = listener.local_addr().map_err(|_| failure())?.port();
    let (mut stream, _) = listener.accept().await.map_err(|_| failure())?;
    let mut bytes = Zeroizing::new(Vec::new());
    let mut chunk = [0; 1024];
    loop {
        let n = stream.read(&mut chunk).await.map_err(|_| failure())?;
        if n == 0 {
            return Err(failure());
        }
        bytes.extend_from_slice(&chunk[..n]);
        if bytes.len() > 16384 {
            return Err(failure());
        }
        if bytes.windows(4).any(|v| v == b"\r\n\r\n") {
            break;
        }
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| failure())?;
    let mut words = text.lines().next().ok_or_else(failure)?.split_whitespace();
    if words.next() != Some("GET") {
        return Err(failure());
    }
    let path = words.next().ok_or_else(failure)?;
    if !path.starts_with("/auth/callback?")
        || words.next() != Some("HTTP/1.1")
        || words.next().is_some()
    {
        return Err(failure());
    }
    let expected = format!("127.0.0.1:{port}");
    let hosts = text
        .lines()
        .skip(1)
        .filter_map(|l| l.split_once(':'))
        .filter(|(k, _)| k.eq_ignore_ascii_case("host"))
        .map(|(_, v)| v.trim())
        .collect::<Vec<_>>();
    if hosts != [expected.as_str()] {
        return Err(failure());
    }
    let callback = Zeroizing::new(format!("http://127.0.0.1:{port}{path}"));
    let pending = attempt.accept_callback(&callback).map_err(|_| failure())?;
    Ok((pending, stream))
}
async fn auth_client() -> Result<ChatGptAuthClient, String> {
    #[cfg(feature = "test-fixtures")]
    if let Ok(fixture) = std::env::var("MOOSURI_TEST_CHATGPT_ORIGIN") {
        return ChatGptAuthClient::synthetic_fixture(&fixture)
            .await
            .map_err(|_| failure());
    }
    ChatGptAuthClient::discover().await.map_err(|_| failure())
}
async fn login_owner(
    c: &Client,
    origin: &str,
    email: &Option<String>,
) -> Result<(Zeroizing<String>, String), String> {
    let email = match email {
        Some(v) => v.clone(),
        None => prompt("Moosuri email")?,
    };
    let password = Zeroizing::new(
        rpassword::prompt_password("Moosuri password: ").map_err(|_| "password_input_failed")?,
    );
    let response = c
        .post(format!("{origin}/api/v1/auth/login"))
        .header(header::ORIGIN, origin)
        .json(&json!({"email":email,"password":password.as_str(),"purpose":"chatgpt_helper"}))
        .send()
        .await
        .map_err(|_| failure())?;
    if !response.status().is_success() {
        return Err("moosuri_login_failed".into());
    }
    let cookie = Zeroizing::new(
        response
            .headers()
            .get(header::SET_COOKIE)
            .and_then(|h| h.to_str().ok())
            .and_then(|h| h.split(';').next())
            .ok_or_else(failure)?
            .to_owned(),
    );
    let bytes = body(response, 16384).await?;
    let login: Value = serde_json::from_slice(&bytes).map_err(|_| failure())?;
    let account = login
        .pointer("/user/id")
        .and_then(Value::as_str)
        .ok_or_else(failure)?
        .to_owned();
    Ok((cookie, account))
}
async fn logout_owner(c: &Client, origin: &str, cookie: &str, account: &str) {
    let _ = c
        .post(format!("{origin}/api/v1/auth/logout"))
        .header(header::ORIGIN, origin)
        .header(header::COOKIE, cookie)
        .header("x-moosuri-account", account)
        .send()
        .await;
}
async fn reauthenticated_receipt(
    c: &Client,
    state: &PrivateState,
    j: &mut Journal,
    email: &Option<String>,
) -> Result<bool, String> {
    println!("Sign in to Moosuri again to reconcile the unresolved server receipt.");
    let (cookie, account) = login_owner(c, &j.origin, email).await?;
    if account != j.account {
        logout_owner(c, &j.origin, &cookie, &account).await;
        return Err("same_moosuri_owner_required".into());
    }
    let previous = Zeroizing::new(std::mem::replace(&mut j.cookie, cookie.to_string()));
    let result = status(c, j).await;
    let confirmed = match result {
        Ok(s) => {
            let key = HandoffKey::from_private_transport(&j.transfer_key).map_err(|_| failure())?;
            confirm(j, &s, &key)
        }
        Err(e) => Err(e),
    };
    if matches!(confirmed, Ok(true)) {
        acknowledge(c, state, j).await?;
        logout_owner(c, &j.origin, &previous, &account).await;
        return Ok(true);
    }
    j.cookie = previous.to_string();
    logout_owner(c, &j.origin, &cookie, &account).await;
    confirmed
}
pub async fn run(
    server: &Option<String>,
    email: &Option<String>,
    ssh: &Option<String>,
    label: &str,
    binding: &Option<String>,
    revision: &Option<i64>,
    restart: bool,
) -> Result<(), String> {
    let home = std::env::var_os("HOME").ok_or("private_helper_state_unavailable")?;
    let path = std::path::PathBuf::from(home).join(".local/state/moosuri/chatgpt-vm");
    let state = PrivateState::open(&path)?;
    let host = state.local_host()?;
    let c = client()?;
    let old = load(&state)?;
    let mut existing = binding.clone().zip(*revision);
    if binding.is_some() != revision.is_some() {
        return Err("binding_and_revision_required_together".into());
    }
    if let Some(mut old) = old {
        if let Some(server) = server
            && origin(server)? != old.origin
            && old.phase != "handed_off"
        {
            return Err("pending_handoff_targets_another_server".into());
        }
        if old.phase == "pending_handoff" {
            if transfer(&c, &state, &mut old).await.is_ok() {
                return Ok(());
            }
            match reauthenticated_receipt(&c, &state, &mut old, email).await {
                Ok(true) => return Ok(()),
                Ok(false) => (),
                Err(error) if !restart => return Err(error),
                Err(_) => (),
            }
            if !restart {
                return Err(failure());
            }
        }
        if old.phase != "handed_off" {
            if !restart {
                return Err("vm_signin_restart_required_use_restart_for_fresh_oauth".into());
            }
            // Query receipt first; cancellation never revokes an imported target session.
            if let Ok(s) = status(&c, &old).await {
                if s.phase == "imported" {
                    return Err("receipt_reconciliation_required".into());
                }
                body(
                    api(&c, &old, reqwest::Method::DELETE, "")
                        .send()
                        .await
                        .map_err(|_| failure())?,
                    16384,
                )
                .await?;
            }
            if existing.is_none() {
                existing = Some((old.context.binding.clone(), old.context.expected_revision));
            }
        }
    }
    let server = match server {
        Some(v) => v.clone(),
        None => prompt("Moosuri HTTPS server")?,
    };
    let origin = origin(&server)?;
    let ssh_alias_value = match ssh {
        Some(v) => v.clone(),
        None => prompt("Existing SSH host alias")?,
    };
    ssh_alias(&ssh_alias_value)?;
    let (cookie, account) = login_owner(&c, &origin, email).await?;
    let preparation=async {
    let response=c.post(format!("{origin}/api/v1/settings/chatgpt/handoffs")).header(header::ORIGIN,&origin).header(header::COOKIE,cookie.as_str()).header("x-moosuri-account",&account).json(&json!({"label":label,"binding_id":existing.as_ref().map(|v|&v.0),"expected_revision":existing.as_ref().map(|v|v.1)})).send().await.map_err(|_|failure())?;
    let bytes = body(response, 16384).await?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Preparation {
        context: HandoffContext,
        transfer_key: String,
        expires_at: i64,
        client_id: Option<String>,
        subject: Option<String>,
    }
    impl Drop for Preparation {
        fn drop(&mut self) {
            self.transfer_key.zeroize();
        }
    }
    let mut p: Preparation = serde_json::from_slice(&bytes).map_err(|_| failure())?;
    if p.context.target_host == host || p.context.account != account || p.context.aad().is_err() {
        return Err(failure());
    }
    let j = Journal {
        version: 1,
        phase: "oauth_pending".into(),
        origin:origin.clone(),
        ssh_alias: ssh_alias_value,
        context: p.context.clone(),
        cookie: cookie.to_string(),
        account:account.clone(),
        transfer_key: std::mem::take(&mut p.transfer_key),
        expires_at: p.expires_at,
        client_id: p.client_id.take(),
        subject: p.subject.take(),
        envelope_digest: None,
    };
    save(&state, &j)?;
    Ok::<Journal,String>(j)
    }.await;
    let mut j = match preparation {
        Ok(j) => j,
        Err(error) => {
            logout_owner(&c, &origin, &cookie, &account).await;
            return Err(error);
        }
    };
    // Listener and durable target commitment exist before opening the browser.
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .map_err(|_| failure())?;
    let port = listener.local_addr().map_err(|_| failure())?.port();
    let auth = auth_client().await?;
    let (attempt, url) = if let Some(client) = &j.client_id {
        auth.begin_reauthorization(
            IssuedRegistration::from_saved_metadata(client.clone(), host.clone())
                .map_err(|_| failure())?,
            port,
        )
    } else {
        auth.begin_registration(&host, port)
    }
    .map_err(|_| failure())?;
    let commitment = attempt
        .handoff_nonce_commitment(&j.context.ticket)
        .map_err(|_| failure())?;
    body(
        api(&c, &j, reqwest::Method::PUT, "/nonce")
            .json(&json!({"commitment":commitment,"source_host":host}))
            .send()
            .await
            .map_err(|_| failure())?,
        16384,
    )
    .await?;
    #[cfg(target_os = "macos")]
    let opener = "open";
    #[cfg(not(target_os = "macos"))]
    let opener = "xdg-open";
    if !tokio::process::Command::new(opener)
        .arg(url.for_system_browser())
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .await
        .map_err(|_| "system_browser_unavailable")?
        .success()
    {
        return Err("system_browser_unavailable".into());
    }
    println!(
        "Continue with ChatGPT in your browser. This helper only transfers the connection to your server."
    );
    let (pending, mut stream) =
        tokio::time::timeout(Duration::from_secs(300), callback(listener, attempt))
            .await
            .map_err(|_| failure())??;
    // Issued client is retained on both sides before redeeming the one-use code.
    j.client_id = Some(pending.registration().client_id().to_owned());
    save(&state, &j)?;
    body(
        api(&c, &j, reqwest::Method::PUT, "/registration")
            .json(&json!({"client_id":j.client_id}))
            .send()
            .await
            .map_err(|_| failure())?,
        16384,
    )
    .await?;
    let bundle = auth.exchange(pending).await.map_err(|_| failure())?;
    if j.client_id.as_deref() != Some(bundle.identity().client_id())
        || j.subject
            .as_ref()
            .is_some_and(|s| s != bundle.identity().subject())
    {
        return Err(failure());
    }
    let key = HandoffKey::from_private_transport(&j.transfer_key).map_err(|_| failure())?;
    let envelope = seal_handoff(bundle, &j.context, &key).map_err(|_| failure())?;
    // Durable source refresh-disabled marker precedes envelope publication/SSH bytes.
    j.phase = "pending_handoff".into();
    j.envelope_digest = Some(envelope_digest(&envelope).to_vec());
    save(&state, &j)?;
    state.write("envelope", &envelope)?;
    let _=stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nCache-Control: no-store\r\nContent-Security-Policy: default-src 'none'\r\nConnection: close\r\n\r\nSign-in received. Return to the Moosuri helper to finish server transfer.").await;
    println!("Transferring the protected connection to your server…");
    transfer(&c, &state, &mut j).await
}
pub async fn receive(stdin: bool) -> Result<(), String> {
    if !stdin {
        return Err("stdin_frame_required".into());
    }
    let bytes = tokio::time::timeout(Duration::from_secs(30), frame_read(&mut tokio::io::stdin()))
        .await
        .map_err(|_| failure())??;
    let frame: Frame = serde_json::from_slice(&bytes).map_err(|_| failure())?;
    if frame.version != 1
        || origin(&frame.origin)? != frame.origin
        || uuid::Uuid::parse_str(&frame.ticket).is_err()
        || frame.envelope.len() > 192 * 1024
    {
        return Err(failure());
    }
    let envelope = URL_SAFE_NO_PAD
        .decode(&frame.envelope)
        .map_err(|_| failure())?;
    if envelope.len() > MAX_ENVELOPE {
        return Err(failure());
    }
    let c = client()?;
    // Ordinary owner session, same account header and same HTTPS origin as import.
    let r = c
        .get(format!("{}/api/v1/auth/session", frame.origin))
        .header(header::COOKIE, &frame.cookie)
        .header("x-moosuri-account", &frame.account)
        .send()
        .await
        .map_err(|_| failure())?;
    body(r, 16384).await?;
    let r = c
        .post(format!(
            "{}/api/v1/settings/chatgpt/handoffs/{}/import",
            frame.origin, frame.ticket
        ))
        .header(header::ORIGIN, &frame.origin)
        .header(header::COOKIE, &frame.cookie)
        .header("x-moosuri-account", &frame.account)
        .header(header::CONTENT_TYPE, "application/octet-stream")
        .body(envelope)
        .send()
        .await
        .map_err(|_| failure())?;
    let bytes = body(r, 16384).await?;
    let receipt: HandoffStatus = serde_json::from_slice(&bytes).map_err(|_| failure())?;
    let safe = serde_json::to_vec(&receipt).map_err(|_| failure())?;
    let mut out = tokio::io::stdout();
    out.write_u32(safe.len() as u32)
        .await
        .map_err(|_| failure())?;
    out.write_all(&safe).await.map_err(|_| failure())?;
    out.flush().await.map_err(|_| failure())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_destination_and_ssh_authority_are_bounded() {
        for s in [
            "-oProxyCommand=bad",
            "host;sh",
            "host\nother",
            "user@host",
            "../host",
            "",
        ] {
            assert!(ssh_alias(s).is_err());
        }
        ssh_alias("my-vm_1.example").unwrap();
        for s in [
            "http://example.com",
            "https://user@example.com",
            "https://example.com/path",
            "https://example.com/?secret=x",
        ] {
            assert!(origin(s).is_err());
        }
        assert_eq!(
            origin("https://example.com").unwrap(),
            "https://example.com"
        );
    }
    #[test]
    fn pending_source_survives_restart_and_forged_receipt_without_releasing_payload() {
        use moosuri_connectors::chatgpt_oauth::handoff::HandoffReceipt;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("private-helper");
        let state = PrivateState::open(&path).unwrap();
        let key = HandoffKey::generate().unwrap();
        let other = HandoffKey::generate().unwrap();
        let envelope = b"synthetic encrypted renewable envelope";
        let context = HandoffContext {
            version: 1,
            ticket: "synthetic-ticket".into(),
            runtime: "runtime".into(),
            tenant: "tenant".into(),
            account: "account".into(),
            target_host: "urn:uuid:target".into(),
            binding: "binding".into(),
            expected_revision: 0,
            staged_revision: 1,
        };
        let j = Journal {
            version: 1,
            phase: "pending_handoff".into(),
            origin: "https://example.test".into(),
            ssh_alias: "synthetic-vm".into(),
            context: context.clone(),
            cookie: "synthetic-owner-cookie".into(),
            account: "account".into(),
            transfer_key: key.for_private_transport().to_string(),
            expires_at: 9999999999,
            client_id: Some("issued-client".into()),
            subject: Some("subject".into()),
            envelope_digest: Some(envelope_digest(envelope).to_vec()),
        };
        save(&state, &j).unwrap();
        assert!(state.read("envelope", MAX_ENVELOPE).unwrap().is_none());
        drop(state);
        let state = PrivateState::open(&path).unwrap();
        let pending = load(&state).unwrap().unwrap();
        assert_eq!(pending.phase, "pending_handoff");
        state.write("envelope", envelope).unwrap();
        let receipt = HandoffReceipt {
            version: 1,
            receipt_id: "synthetic-receipt".into(),
            context,
            generation: 1,
            envelope_digest: envelope_digest(envelope).to_vec(),
        };
        let mut status = HandoffStatus {
            phase: "imported".into(),
            receipt: Some(receipt.clone()),
            receipt_mac: Some(receipt.authenticate(&other).unwrap()),
        };
        assert!(confirm(&pending, &status, &key).is_err());
        assert_eq!(load(&state).unwrap().unwrap().phase, "pending_handoff");
        assert_eq!(
            &**state
                .read("envelope", MAX_ENVELOPE)
                .unwrap()
                .as_ref()
                .unwrap(),
            envelope
        );
        status.receipt_mac = Some(receipt.authenticate(&key).unwrap());
        assert!(confirm(&pending, &status, &key).unwrap());
    }
    #[tokio::test]
    async fn receiver_frame_rejects_oversize_and_trailing_bytes() {
        let mut bytes = &[0, 4, 0, 1][..];
        assert!(frame_read(&mut bytes).await.is_err());
        let mut bytes = &[0, 0, 0, 1, 42, 99][..];
        assert!(frame_read(&mut bytes).await.is_err());
        let mut bytes = &[0, 0, 0, 1, 42][..];
        assert_eq!(&*frame_read(&mut bytes).await.unwrap(), &[42]);
    }
}
