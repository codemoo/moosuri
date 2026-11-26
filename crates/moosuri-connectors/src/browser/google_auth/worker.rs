use super::{
    Binding, Envelope, Error, PROTOCOL, Receiver,
    bootstrap::{self, Action, Reply, Response, Sequence},
    chrome::{Chrome, Page},
    client,
    consent::Consent,
    console,
    sdk::{Email, LoginHandle, ProjectId, ProjectPresence, Sdk, SetupLabel},
};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use std::{future::Future, time::Duration};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Init {
    protocol: String,
    binding: Binding,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivery {
    google_email: String,
    envelope: Envelope,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Release {
    proceed: Uuid,
}

/// After `signed_in`: the existing consent command, or one bootstrap command.
#[derive(Deserialize)]
#[serde(untagged)]
enum Command {
    Bootstrap(bootstrap::Envelope),
    Consent(Consent),
}

async fn receive<T: DeserializeOwned>(
    input: &mut (impl AsyncBufRead + Unpin),
    stop: &CancellationToken,
    seconds: u64,
) -> Result<T, Error> {
    let read = tokio::time::timeout(Duration::from_secs(seconds), async {
        let mut bytes = Vec::new();
        (&mut *input)
            .take(12 * 1024 + 1)
            .read_until(b'\n', &mut bytes)
            .await
            .map_err(|_| Error::Unavailable)?;
        if bytes.len() > 12 * 1024 || bytes.pop() != Some(b'\n') {
            return Err(Error::Invalid);
        }
        serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)
    });
    tokio::select! { biased;
        _ = stop.cancelled() => Err(Error::Unavailable),
        r = read => r.map_err(|_| Error::Unavailable)?,
    }
}
async fn send(
    output: &mut (impl AsyncWrite + Unpin),
    value: serde_json::Value,
) -> Result<(), Error> {
    let mut bytes = value.to_string().into_bytes();
    bytes.push(b'\n');
    tokio::time::timeout(Duration::from_secs(5), async {
        output.write_all(&bytes).await?;
        output.flush().await
    })
    .await
    .map_err(|_| Error::Unavailable)?
    .map_err(|_| Error::Unavailable)
}
/// Private response line (may carry the one-time client secret): serialized into a
/// zeroizing pre-sized buffer, never logged, never placed in a `Value`.
async fn send_response(
    output: &mut (impl AsyncWrite + Unpin),
    response: &Response,
) -> Result<(), Error> {
    let line = response.to_line()?;
    tokio::time::timeout(Duration::from_secs(5), async {
        output.write_all(&line).await?;
        output.flush().await
    })
    .await
    .map_err(|_| Error::Unavailable)?
    .map_err(|_| Error::Unavailable)
}
// During a running operation only a live, otherwise silent controller is valid.
// EOF, cancellation or pipelined commands abort before another action is taken.
// Only for operations that own no SDK credentials: they may simply be dropped.
async fn connected<T>(
    input: &mut (impl AsyncBufRead + Unpin),
    stop: &CancellationToken,
    seconds: u64,
    operation: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::select! { biased;
        _ = stop.cancelled() => Err(Error::Unavailable),
        _ = input.read_u8() => Err(Error::Unavailable),
        result = tokio::time::timeout(Duration::from_secs(seconds), operation) => result.map_err(|_| Error::Unavailable)?,
    }
}

/// Cooperative variant for bootstrap actions that may own SDK state. EOF, any
/// pipelined byte, global cancellation or the action deadline cancel `token`, and
/// the operation is then awaited until it has unwound (never dropped), so its own
/// cleanup runs. After any such cancellation the result is an error, even if the
/// operation happened to finish.
async fn supervised<T>(
    input: &mut (impl AsyncBufRead + Unpin),
    stop: &CancellationToken,
    token: &CancellationToken,
    seconds: u64,
    operation: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::pin!(operation);
    let deadline = tokio::time::sleep(Duration::from_secs(seconds));
    tokio::pin!(deadline);
    let mut cancelled = false;
    let result = loop {
        tokio::select! { biased;
            _ = input.read_u8(), if !cancelled => { cancelled = true; token.cancel(); }
            _ = stop.cancelled(), if !cancelled => { cancelled = true; token.cancel(); }
            _ = &mut deadline, if !cancelled => { cancelled = true; token.cancel(); }
            result = &mut operation => break result,
        }
    };
    if cancelled {
        Err(Error::Unavailable)
    } else {
        result
    }
}

/// SDK state lives here, outside every action future, so that it can always be
/// cleaned by an explicit await however an action ended.
struct Bootstrap {
    sdk: Option<Sdk>,
    /// The one delivered Google email; identity is never taken from a command.
    google_email: String,
    seq: Sequence,
    /// A login process could not be proven reaped; credentials cannot be cleaned.
    unreaped: bool,
}

impl Bootstrap {
    /// Awaited credential cleanup (up to 60 s, independent of any token).
    async fn cleanup(&mut self) -> Result<(), Error> {
        let Some(sdk) = self.sdk.take() else {
            return Ok(());
        };
        if self.unreaped {
            return Err(Error::Unavailable);
        }
        sdk.cleanup_login_credentials().await
    }
}

fn sdk_proxy() -> Result<std::net::SocketAddr, Error> {
    let raw = std::env::var("MOOSURI_BROWSER_PROXY_IP").map_err(|_| Error::Unavailable)?;
    crate::browser::private_proxy(&raw).map_err(|_| Error::Unavailable)
}

async fn authorize(
    sdk: &Sdk,
    browser: &mut Chrome,
    email: &Email,
    token: &CancellationToken,
    unreaped: &mut bool,
) -> Result<(), Error> {
    let handle = sdk.begin_login(email, token).await?;
    let callback = match browser
        .authorize(handle.request(), email, token, Duration::from_secs(240))
        .await
    {
        Ok(callback) => callback,
        Err(error) => {
            *unreaped = stop_login(handle).await.is_err();
            return Err(error);
        }
    };
    if let Err(error) = sdk.deliver_callback(&handle, callback, token).await {
        *unreaped = stop_login(handle).await.is_err();
        return Err(error);
    }
    // On failure `finish_login` itself reaps the child and cleans up.
    sdk.finish_login(handle, token).await
}

async fn stop_login(handle: LoginHandle) -> Result<(), Error> {
    handle.stop().await
}

async fn inspect(
    sdk: &Sdk,
    id: &ProjectId,
    label: &SetupLabel,
    token: &CancellationToken,
) -> Result<Reply, Error> {
    match sdk.probe_project(id, label, token).await? {
        ProjectPresence::Absent => Ok(Reply::Absent),
        ProjectPresence::Owned => {
            sdk.verify_unbilled(id, token).await?;
            Ok(Reply::Ready)
        }
    }
}

async fn create(
    sdk: &Sdk,
    id: &ProjectId,
    label: &SetupLabel,
    token: &CancellationToken,
) -> Result<Reply, Error> {
    // Re-prove absence immediately before the single create.
    if sdk.probe_project(id, label, token).await? != ProjectPresence::Absent {
        return Err(Error::Blocked);
    }
    sdk.create_project(id, label, token).await?;
    sdk.describe_project(id, label, token).await?;
    sdk.verify_unbilled(id, token).await?;
    Ok(Reply::Ready)
}

async fn enable(
    sdk: &Sdk,
    id: &ProjectId,
    label: &SetupLabel,
    token: &CancellationToken,
) -> Result<Reply, Error> {
    if sdk.probe_project(id, label, token).await? != ProjectPresence::Owned {
        return Err(Error::Blocked);
    }
    sdk.verify_unbilled(id, token).await?;
    sdk.enable_apis(id, token).await?;
    sdk.verify_apis(id, token).await?;
    Ok(Reply::Ready)
}

async fn bootstrap_action(
    input: &mut (impl AsyncBufRead + Unpin),
    output: &mut (impl AsyncWrite + Unpin),
    stop: &CancellationToken,
    boot: &mut Bootstrap,
    browser: &mut Chrome,
    binding: &Binding,
    request: bootstrap::Request,
) -> Result<(), Error> {
    boot.seq.admit(&request, binding)?;
    let token = CancellationToken::new();
    let response = if request.action == Action::Authorize {
        let email = Email::parse(&boot.google_email)?;
        boot.sdk = Some(Sdk::create(sdk_proxy()?)?);
        let sdk = boot.sdk.as_ref().ok_or(Error::Unavailable)?;
        let work = authorize(sdk, browser, &email, &token, &mut boot.unreaped);
        supervised(input, stop, &token, 420, work)
            .await
            .map(|()| Response::Status(Reply::Ready))?
    } else {
        let sdk = boot.sdk.as_ref().ok_or(Error::Invalid)?;
        let id = ProjectId::parse(&request.project_id())?;
        let label = SetupLabel::parse(&request.setup_label())?;
        match request.action {
            Action::InspectProject => Response::Status(
                supervised(input, stop, &token, 150, inspect(sdk, &id, &label, &token)).await?,
            ),
            Action::CreateProject => Response::Status(
                supervised(input, stop, &token, 420, create(sdk, &id, &label, &token)).await?,
            ),
            Action::EnableApis => Response::Status(
                supervised(input, stop, &token, 600, enable(sdk, &id, &label, &token)).await?,
            ),
            Action::VerifyApis => {
                let work = async { sdk.verify_apis(&id, &token).await.map(|()| Reply::Ready) };
                Response::Status(supervised(input, stop, &token, 90, work).await?)
            }
            Action::InspectClient | Action::CreateClient | Action::RecoverClientSecret => {
                // Values come only from the request's setup id / exact callback and
                // the verified bound email. The SDK is untouched. Chrome is tainted
                // on any error, so an uncertain create or secret recovery is never
                // repeated (the Sequence also spends the shared write budget).
                let op = client::Op::from_action(request.action).ok_or(Error::Invalid)?;
                let email = Email::parse(&boot.google_email)?;
                let spec = client::Spec::new(&request, &id, &email);
                let seconds = if op == client::Op::Inspect { 150 } else { 420 };
                let work = browser.client(op, &spec, &token);
                supervised(input, stop, &token, seconds, work)
                    .await?
                    .into_response()
            }
            Action::InspectBranding
            | Action::ConfigureBranding
            | Action::InspectAudience
            | Action::ConfigureAudience
            | Action::InspectScopes
            | Action::ConfigureScopes
            | Action::InspectDomains
            | Action::ConfigureDomains
            | Action::InspectPublication
            | Action::ConfigurePublication => {
                // Values come only from the verified bound email and the derived
                // project id (and, for public information, the binding origin that
                // `admit` proved equal to the request's exact callback origin). The SDK is
                // untouched; the token cancels the Chrome walk, which taints Chrome on any
                // error (teardown still runs).
                let op = console::Op::from_action(request.action).ok_or(Error::Invalid)?;
                let email = Email::parse(&boot.google_email)?;
                let spec = match op {
                    console::Op::InspectDomains | console::Op::ConfigureDomains => {
                        console::Spec::with_site(&id, &email, &binding.origin)?
                    }
                    _ => console::Spec::new(&id, &email),
                };
                let seconds = match op {
                    console::Op::InspectBranding
                    | console::Op::InspectScopes
                    | console::Op::InspectAudience
                    | console::Op::InspectDomains
                    | console::Op::InspectPublication => 150,
                    _ => 420,
                };
                let work = browser.console(op, &spec, &token);
                Response::Status(supervised(input, stop, &token, seconds, work).await?)
            }
            Action::Finish => {
                // Proof that no SDK credential remains; only then is the SDK discarded.
                let work = async { sdk.cleanup_login_credentials().await.map(|()| Reply::Ready) };
                let reply = supervised(input, stop, &token, 90, work).await?;
                boot.sdk = None;
                Response::Status(reply)
            }
            Action::Authorize => return Err(Error::Invalid),
        }
    };
    // A private `Client` answer is only ever valid for Create/Recover and this exact
    // request; anything else is dropped (zeroized) without being sent.
    if !response.answers(&request) {
        return Err(Error::Invalid);
    }
    boot.seq.record_response(request.action, &response);
    send_response(output, &response).await
}

async fn login(
    browser: &mut Chrome,
    email: &str,
    secret: zeroize::Zeroizing<Vec<u8>>,
    output: &mut (impl AsyncWrite + Unpin),
) -> Result<(), Error> {
    browser.open_login().await?;
    let mut identified = false;
    let mut submitted = false;
    let mut verifying = false;
    let mut secret = Some(secret);
    for _ in 0..300 {
        match browser.page(email).await? {
            Page::Identifier if !identified => {
                browser.identifier(email).await?;
                identified = true;
            }
            Page::Password if identified && !submitted => {
                let value = secret.take().ok_or(Error::Invalid)?;
                browser
                    .password(
                        email,
                        std::str::from_utf8(&value).map_err(|_| Error::Invalid)?,
                    )
                    .await?;
                submitted = true;
            }
            Page::Ready if identified => return Ok(()),
            Page::Mismatch | Page::Rejected => return Err(Error::Blocked),
            Page::Verification if !verifying => {
                verifying = true;
                send(output, json!({"status":"verifying"})).await?;
            }
            _ => {}
        }
        tokio::time::sleep(Duration::from_millis(600)).await;
    }
    Err(Error::NeedsHuman)
}

/// For bootstrap workers, zero exit is a cleanup receipt, independent of the
/// operation's terminal status. It is never granted before SDK and Chrome teardown.
#[derive(Default, PartialEq, Eq)]
enum CleanupProof {
    #[default]
    Unused,
    Pending,
    Proven,
}

async fn run(
    input: &mut (impl AsyncBufRead + Unpin),
    output: &mut (impl AsyncWrite + Unpin),
    tenant: Uuid,
    stop: &CancellationToken,
    cleanup: &mut CleanupProof,
) -> Result<(), Error> {
    let init: Init = receive(input, stop, 10).await?;
    if init.protocol != PROTOCOL || !init.binding.valid() || init.binding.tenant != tenant {
        return Err(Error::Invalid);
    }
    let receiver = connected(input, stop, 25, Receiver::create(init.binding.clone())).await?;
    send(
        output,
        json!({"status":"vault","recipient":receiver.recipient()}),
    )
    .await?;
    let delivery: Delivery = receive(input, stop, 730).await?;
    let secret = connected(
        input,
        stop,
        25,
        receiver.open(&delivery.google_email, delivery.envelope),
    )
    .await?;
    send(output, json!({"status":"verified"})).await?;
    let release: Release = receive(input, stop, 30).await?;
    if release.proceed != init.binding.challenge {
        return Err(Error::Invalid);
    }
    let cancel = CancellationToken::new();
    let mut browser = connected(
        input,
        stop,
        35,
        Chrome::start(&init.binding, cancel.clone()),
    )
    .await?;
    let mut boot = Bootstrap {
        sdk: None,
        google_email: delivery.google_email.clone(),
        seq: Sequence::default(),
        unreaped: false,
    };
    let outcome = async {
        connected(
            input,
            stop,
            240,
            login(&mut browser, &delivery.google_email, secret, output),
        )
        .await?;
        send(output, json!({"status":"signed_in"})).await?;
        loop {
            let wait = if boot.seq.active() { 600 } else { 45 };
            match receive::<Command>(input, stop, wait).await? {
                Command::Consent(consent) => {
                    if !boot.seq.consent_allowed() {
                        return Err(Error::Invalid);
                    }
                    return connected(
                        input,
                        stop,
                        120,
                        browser.consent(consent, &init.binding, &delivery.google_email),
                    )
                    .await;
                }
                Command::Bootstrap(command) => {
                    *cleanup = CleanupProof::Pending;
                    bootstrap_action(
                        input,
                        output,
                        stop,
                        &mut boot,
                        &mut browser,
                        &init.binding,
                        command.bootstrap,
                    )
                    .await?;
                }
            }
        }
    }
    .await;
    // Awaited SDK credential cleanup comes first and is never skipped or raced
    // against cancellation, however the operation above ended.
    let sdk_cleanup = boot.cleanup().await;
    // No terminal receipt until Chrome and X have been stopped. The launcher
    // still must confirm removal of its owned container before releasing a slot.
    cancel.cancel();
    browser.shutdown().await;
    if *cleanup == CleanupProof::Pending && sdk_cleanup.is_ok() && !boot.unreaped {
        *cleanup = CleanupProof::Proven;
    }
    // A cleanup error must not be hidden by an earlier operation error.
    sdk_cleanup?;
    outcome?;
    send(output, json!({"status":"returned"})).await
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

/// Empty-profile local startup only. Never opens Google or accepts credentials.
pub async fn installation_probe() -> bool {
    let Some(tenant) = std::env::var("MOOSURI_BROWSER_TENANT")
        .ok()
        .and_then(|s| s.parse::<Uuid>().ok())
        .filter(|id| !id.is_nil())
    else {
        return false;
    };
    let binding = Binding {
        installation: Uuid::new_v4(),
        account: Uuid::new_v4(),
        tenant,
        volume: Uuid::new_v4(),
        generation: 1,
        epoch: 0,
        challenge: Uuid::new_v4(),
        email: "probe@example.invalid".into(),
        origin: "https://probe.example.invalid".into(),
    };
    let cancel = CancellationToken::new();
    let Ok(browser) = Chrome::start(&binding, cancel.clone()).await else {
        return false;
    };
    let mut output = tokio::io::stdout();
    let mut input = BufReader::new(tokio::io::stdin());
    let ready = send(
        &mut output,
        json!({"ready":true,"tenant":tenant,
        "javascript_enabled":true,"chrome":"158.0.8127.88"}),
    )
    .await;
    if ready.is_ok() {
        let _: Result<serde_json::Value, _> =
            receive(&mut input, &CancellationToken::new(), 10).await;
    }
    cancel.cancel();
    browser.shutdown().await;
    ready.is_ok()
}

pub async fn serve() -> bool {
    #[cfg(target_os = "linux")]
    {
        // Protect the broker's decrypted buffer from same-uid proc access and
        // host core handlers; the browser remains separately sandboxed.
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
    let tenant = std::env::var("MOOSURI_BROWSER_TENANT")
        .ok()
        .and_then(|s| s.parse::<Uuid>().ok());
    let mut input = BufReader::new(tokio::io::stdin());
    let mut output = tokio::io::stdout();
    let mut cleanup = CleanupProof::Unused;
    let outcome = if let Some(tenant) = tenant.filter(|id| !id.is_nil()) {
        if Chrome::config(tenant)
            .is_ok_and(|c| crate::browser::capsule_health_mode(&c, true).is_ok())
        {
            // Signals and the global deadline only cancel; `run` is never dropped,
            // so its awaited SDK and Chrome cleanup always executes.
            let stop = CancellationToken::new();
            let watcher = tokio::spawn({
                let stop = stop.clone();
                async move {
                    tokio::select! {
                        _ = stopped() => {},
                        _ = tokio::time::sleep(super::bootstrap::ATTEMPT_LIFETIME) => {},
                    }
                    stop.cancel();
                }
            });
            let result = run(&mut input, &mut output, tenant, &stop, &mut cleanup).await;
            watcher.abort();
            result
        } else {
            Err(Error::Unavailable)
        }
    } else {
        Err(Error::Invalid)
    };
    if let Err(error) = outcome {
        let status = match error {
            Error::Blocked => "blocked",
            Error::NeedsHuman => "needs_human",
            _ => "unavailable",
        };
        let _ = send(&mut output, json!({"status":status})).await;
        // The phase remains blocked/needs_human/unavailable. Only exit status
        // certifies cleanup for the capsule actor; it is not operation success.
        // Cancellation can reach the worker before the first bootstrap frame.
        // Unused means no SDK credential was ever created. The phase is still
        // failure; zero exit is solely proof that no remote SDK grant remains.
        return cleanup != CleanupProof::Pending;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    // The controller side stays open (silent) unless a test closes it.
    fn pipe() -> (tokio::io::DuplexStream, BufReader<tokio::io::DuplexStream>) {
        let (controller, worker) = tokio::io::duplex(1024);
        (controller, BufReader::new(worker))
    }

    #[tokio::test]
    async fn eof_cancels_then_awaits_operation_cleanup() {
        let (controller, mut input) = pipe();
        let (stop, token) = (CancellationToken::new(), CancellationToken::new());
        let cleaned = AtomicBool::new(false);
        let operation = async {
            token.cancelled().await;
            // Stands in for the SDK's own bounded cleanup after cancellation.
            tokio::time::sleep(Duration::from_millis(50)).await;
            cleaned.store(true, Ordering::SeqCst);
            Ok::<_, Error>(Reply::Ready)
        };
        let run = supervised(&mut input, &stop, &token, 30, operation);
        tokio::pin!(run);
        tokio::select! { _ = &mut run => panic!("silent controller must not end it"),
        _ = tokio::time::sleep(Duration::from_millis(50)) => {} }
        assert!(!cleaned.load(Ordering::SeqCst));
        drop(controller);
        // Even though the operation reports success, a cancelled run is an error.
        assert_eq!(run.await, Err(Error::Unavailable));
        assert!(
            cleaned.load(Ordering::SeqCst),
            "operation was dropped, not awaited"
        );
    }

    #[tokio::test]
    async fn global_cancellation_and_deadline_unwind_cooperatively() {
        for deadline in [false, true] {
            let (_controller, mut input) = pipe();
            let (stop, token) = (CancellationToken::new(), CancellationToken::new());
            let cleaned = AtomicBool::new(false);
            let operation = async {
                token.cancelled().await;
                cleaned.store(true, Ordering::SeqCst);
                Err::<(), _>(Error::Blocked)
            };
            if !deadline {
                stop.cancel();
            }
            let seconds = if deadline { 0 } else { 30 };
            let result = supervised(&mut input, &stop, &token, seconds, operation).await;
            assert_eq!(result, Err(Error::Unavailable));
            assert!(cleaned.load(Ordering::SeqCst));
        }
    }

    #[tokio::test]
    async fn pipelined_command_during_action_cancels_it() {
        use tokio::io::AsyncWriteExt;
        let (mut controller, mut input) = pipe();
        controller.write_all(b"{}\n").await.unwrap();
        let (stop, token) = (CancellationToken::new(), CancellationToken::new());
        let operation = async {
            token.cancelled().await;
            Ok::<_, Error>(())
        };
        assert_eq!(
            supervised(&mut input, &stop, &token, 30, operation).await,
            Err(Error::Unavailable)
        );
    }

    #[tokio::test]
    async fn quiet_controller_lets_operation_finish() {
        let (_controller, mut input) = pipe();
        let (stop, token) = (CancellationToken::new(), CancellationToken::new());
        let result = supervised(&mut input, &stop, &token, 30, async { Ok(7) }).await;
        assert_eq!(result, Ok(7));
        assert!(!token.is_cancelled());
    }

    #[test]
    fn command_is_exactly_consent_or_bootstrap() {
        let setup = Uuid::new_v4();
        let bootstrap = json!({"bootstrap":{"setup_id":setup,"redirect_uri":"https://x.test/api/v1/mail/oauth/callback","action":"finish"}});
        assert!(matches!(
            serde_json::from_value::<Command>(bootstrap.clone()),
            Ok(Command::Bootstrap(_))
        ));
        let consent =
            json!({"url":"https://accounts.google.com/x","client_id":"c","service":"gmail"});
        assert!(matches!(
            serde_json::from_value::<Command>(consent.clone()),
            Ok(Command::Consent(_))
        ));
        let mut mixed = bootstrap;
        mixed["url"] = "https://accounts.google.com/x".into();
        assert!(serde_json::from_value::<Command>(mixed).is_err());
        let mut unknown = consent;
        unknown["bootstrap"] = json!(null);
        assert!(serde_json::from_value::<Command>(unknown).is_err());
        let flags = json!({"bootstrap":{"setup_id":setup,"redirect_uri":"x","action":"finish","args":["--x"]}});
        assert!(serde_json::from_value::<Command>(flags).is_err());
    }
}
