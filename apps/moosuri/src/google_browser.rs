//! Installation-owned, socket-activated transport for a sealed Google attempt.
//! Only the authenticated gateway can connect. No member-chosen commands or CDP.
use crate::runtime_google::{self, private_bytes, private_dir};
use clap::Args;
use fs2::FileExt;
use moosuri_connectors::{
    browser::{
        BrowserError,
        capsule::{BootstrapOutcome, CapsuleConfig, CapsuleReader, GoogleAttempt, GooglePhase},
        google_auth::{Binding, Envelope, PROTOCOL, bootstrap},
    },
    gmail::OAuthClient,
    model::CancellationToken,
};
use moosuri_store::runtime_registry::admin::google_bootstrap as registry;
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::json;
use std::{
    future::Future,
    os::{
        fd::AsFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::PathBuf,
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    time::{MissedTickBehavior, interval, timeout},
};
use uuid::Uuid;

#[derive(Args)]
pub struct Options {
    #[arg(long)]
    registry: PathBuf,
    #[arg(long)]
    capsule_config: PathBuf,
    /// Unix UID of the trusted installation gateway, never a member UID.
    #[arg(long, default_value_t = 0)]
    gateway_uid: u32,
    #[arg(long)]
    public_origin: String,
    /// Installation-owned OAuth client; may be absent before first setup.
    #[arg(long)]
    client_file: PathBuf,
    /// Shared root-owned admission lock for socket-activated broker processes.
    #[arg(long, default_value = "/run/moosuri-google-browser/start.lock")]
    startup_lock: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Init {
    protocol: String,
    binding: Binding,
    service: Service,
}
#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "lowercase")]
enum Service {
    Gmail,
    Drive,
    Docs,
    Sheets,
    Slides,
}
impl Service {
    fn name(self) -> &'static str {
        match self {
            Self::Gmail => "gmail",
            Self::Drive => "drive",
            Self::Docs => "docs",
            Self::Sheets => "sheets",
            Self::Slides => "slides",
        }
    }
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
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Consent {
    url: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BootstrapCommand {
    bootstrap: bootstrap::Request,
}
#[derive(Deserialize)]
#[serde(untagged)]
enum Command {
    Bootstrap(BootstrapCommand),
    Consent(Consent),
}

type Peer = BufReader<UnixStream>;
type Result<T> = std::result::Result<T, &'static str>;

async fn receive<T: DeserializeOwned>(peer: &mut Peer, seconds: u64) -> Result<T> {
    timeout(Duration::from_secs(seconds), async {
        let mut bytes = Vec::new();
        (&mut *peer)
            .take(12289)
            .read_until(b'\n', &mut bytes)
            .await
            .map_err(|_| "closed")?;
        if bytes.len() > 12288 || bytes.pop() != Some(b'\n') {
            return Err("invalid");
        }
        serde_json::from_slice(&bytes).map_err(|_| "invalid")
    })
    .await
    .map_err(|_| "expired")?
}
async fn send(peer: &mut Peer, value: serde_json::Value) -> Result<()> {
    let mut bytes = value.to_string().into_bytes();
    bytes.push(b'\n');
    timeout(Duration::from_secs(5), peer.get_mut().write_all(&bytes))
        .await
        .map_err(|_| "expired")?
        .map_err(|_| "closed")
}
/// Forwards one typed bootstrap response to the authorized gateway only. It may
/// carry the one-time OAuth client secret: it is serialized straight into a
/// zeroizing buffer and never logged, printed or placed in a web response.
async fn send_response(peer: &mut Peer, response: &bootstrap::Response) -> Result<()> {
    let line = response.to_line().map_err(|_| "invalid")?;
    timeout(Duration::from_secs(5), peer.get_mut().write_all(&line))
        .await
        .map_err(|_| "expired")?
        .map_err(|_| "closed")
}
async fn connected<T>(peer: &mut Peer, operation: impl Future<Output = Result<T>>) -> Result<T> {
    tokio::select! {biased;
        _=peer.read_u8()=>Err("closed"),
        result=operation=>result,
    }
}
async fn current(options: &Options, binding: &Binding) -> Result<()> {
    let path = options.registry.clone();
    let expected = binding.epoch;
    let allocation = runtime_google::Binding {
        account_id: binding.account,
        tenant_id: binding.tenant,
        volume_id: binding.volume,
        generation: binding.generation,
        peer_uid: options.gateway_uid,
    };
    let value = timeout(
        Duration::from_secs(3),
        tokio::task::spawn_blocking(move || runtime_google::epoch(&path, allocation)),
    )
    .await
    .map_err(|_| "fenced")?
    .map_err(|_| "fenced")??;
    if value != expected {
        return Err("fenced");
    }
    Ok(())
}

async fn record_teardown(options: &Options, binding: &Binding) -> Result<()> {
    for attempt in 0..3 {
        let root = options.registry.clone();
        let run = binding.challenge;
        if matches!(
            tokio::task::spawn_blocking(move || registry::close_sdk_run(&root, run)).await,
            Ok(Ok(()))
        ) {
            return Ok(());
        }
        if attempt < 2 {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Err("google_browser_cleanup_unconfirmed")
}
async fn startup_lock(options: &Options, cancel: &CancellationToken) -> Result<std::fs::File> {
    let parent = options.startup_lock.parent().ok_or("unavailable")?;
    private_dir(parent).map_err(|_| "unavailable")?;
    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(&options.startup_lock)
        .map_err(|_| "unavailable")?;
    let meta = file.metadata().map_err(|_| "unavailable")?;
    if !meta.is_file()
        || meta.uid() != 0
        || meta.mode() & 0o077 != 0
        || meta.nlink() != 1
        || meta.len() != 0
    {
        return Err("unavailable");
    }
    loop {
        match file.try_lock_exclusive() {
            Ok(()) => return Ok(file),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
            Err(_) => return Err("unavailable"),
        }
        tokio::select! {
            _=cancel.cancelled()=>return Err("cancelled"),
            _=tokio::time::sleep(Duration::from_millis(100))=>{},
        }
    }
}
async fn phase(peer: &mut Peer, value: GooglePhase) -> Result<bool> {
    send(peer, json!({"status":value})).await?;
    Ok(matches!(
        value,
        GooglePhase::Blocked
            | GooglePhase::NeedsHuman
            | GooglePhase::Unavailable
            | GooglePhase::Returned
    ))
}
fn client_id(options: &Options) -> Result<String> {
    let bytes = private_bytes(&options.client_file, 16384).map_err(|_| "setup_required")?;
    let text = std::str::from_utf8(&bytes).map_err(|_| "setup_required")?;
    let client = OAuthClient::from_web_client_json(
        text,
        &format!("{}/api/v1/mail/oauth/callback", options.public_origin),
    )
    .map_err(|_| "setup_required")?;
    Ok(client.client_id)
}
async fn flow(
    options: &Options,
    init: &Init,
    peer: &mut Peer,
    attempt: &mut GoogleAttempt,
) -> Result<()> {
    send(
        peer,
        json!({"status":"vault","recipient":attempt.recipient()}),
    )
    .await?;
    let delivery: Delivery = tokio::select! { biased;
        _=attempt.closed()=>return Err("unavailable"),
        delivery=receive(peer,730)=>delivery?,
    };
    let status = connected(peer, async {
        attempt
            .deliver(&delivery.google_email, delivery.envelope)
            .await
            .map_err(|_| "unavailable")
    })
    .await?;
    if phase(peer, status).await? {
        return Ok(());
    }
    let release: Release = receive(peer, 30).await?;
    if release.proceed != init.binding.challenge {
        return Err("invalid");
    }
    current(options, &init.binding).await?;
    let status = connected(peer, async {
        attempt.proceed().await.map_err(|_| "unavailable")
    })
    .await?;
    if phase(peer, status).await? {
        return Ok(());
    }
    if status == GooglePhase::Verifying {
        let status = connected(peer, async {
            attempt.verification().await.map_err(|_| "unavailable")
        })
        .await?;
        if phase(peer, status).await? {
            return Ok(());
        }
    }
    loop {
        // The trusted gateway owns the fresh administrator proof and durable
        // checkpoint. Only typed commands pass through; no member CLI/CDP input.
        let command: Command = receive(peer, 45).await?;
        current(options, &init.binding).await?;
        match command {
            Command::Bootstrap(command) => {
                command
                    .bootstrap
                    .validate(&init.binding)
                    .map_err(|_| "invalid")?;
                let root = options.registry.clone();
                let run = init.binding.challenge;
                let expected = command.bootstrap.setup_id;
                let pending =
                    tokio::task::spawn_blocking(move || registry::sdk_cleanup_for_run(&root, run))
                        .await
                        .map_err(|_| "unavailable")?
                        .map_err(|_| "unavailable")?;
                if pending != Some(expected) {
                    return Err("fenced");
                }
                let reply = connected(peer, async {
                    attempt
                        .bootstrap(command.bootstrap)
                        .await
                        .map_err(|_| "unavailable")
                })
                .await?;
                match reply {
                    BootstrapOutcome::Reply(reply) => send_response(peer, &reply).await?,
                    BootstrapOutcome::Terminal(status) => {
                        phase(peer, status).await?;
                        return Ok(());
                    }
                }
            }
            Command::Consent(consent) => {
                // Re-read after bootstrap committed the installation's client.
                // The web request supplies no client identity or secret.
                let client = client_id(options)?;
                let status = connected(peer, async {
                    attempt
                        .consent(&consent.url, &client, init.service.name())
                        .await
                        .map_err(|_| "unavailable")
                })
                .await?;
                phase(peer, status).await?;
                return Ok(());
            }
        }
    }
}

pub async fn serve(options: &Options) -> std::result::Result<(), String> {
    if unsafe { libc::geteuid() } != 0 {
        return Err("google_browser_owner_invalid".into());
    }
    private_dir(&options.registry)?;
    let config: CapsuleConfig =
        serde_json::from_slice(&private_bytes(&options.capsule_config, 16384)?)
            .map_err(|_| "google_browser_config_invalid")?;
    let owner = config.owner;
    let reader = CapsuleReader::new(config).map_err(|_| "google_browser_config_invalid")?;
    let fd = std::io::stdin()
        .as_fd()
        .try_clone_to_owned()
        .map_err(|_| "google_browser_socket_required")?;
    let stream = std::os::unix::net::UnixStream::from(fd);
    stream
        .set_nonblocking(true)
        .map_err(|_| "google_browser_socket_required")?;
    let stream = UnixStream::from_std(stream).map_err(|_| "google_browser_socket_required")?;
    if stream
        .peer_cred()
        .map_err(|_| "google_browser_peer_invalid")?
        .uid()
        != options.gateway_uid
    {
        return Err("google_browser_peer_invalid".into());
    }
    let mut peer = BufReader::new(stream);
    let init: Init = receive(&mut peer, 5).await?;
    if init.protocol != PROTOCOL
        || !init.binding.valid()
        || init.binding.installation != owner
        || init.binding.origin != options.public_origin
    {
        return Err("google_browser_binding_invalid".into());
    }
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(|_| "unavailable")?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(|_| "unavailable")?;
    current(options, &init.binding).await?;
    let cancel = CancellationToken::new();
    let startup = async {
        // Socket activation uses distinct processes, so a per-reader mutex
        // cannot serialize engine inventory selection and container creation.
        let _admission = startup_lock(options, &cancel).await?;
        let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
        loop {
            if cancel.is_cancelled() {
                return Err("cancelled");
            }
            match reader.start_google(init.binding.clone(), &cancel).await {
                Ok(attempt) => return Ok(attempt),
                Err(BrowserError::Capacity) if tokio::time::Instant::now() < deadline => {
                    // A just-cancelled key-only attempt may still be removing its
                    // container. No external action or credential is retried here.
                    tokio::select! {
                        _=cancel.cancelled()=>return Err("cancelled"),
                        _=tokio::time::sleep(Duration::from_millis(500))=>{},
                    }
                }
                Err(BrowserError::CleanupUnconfirmed) => {
                    return Err("google_browser_cleanup_unconfirmed");
                }
                Err(_) => return Err("google_browser_unavailable"),
            }
        }
    };
    tokio::pin!(startup);
    let mut checks = interval(Duration::from_secs(2));
    checks.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut attempt = loop {
        let stopped = tokio::select! {biased;
            _=term.recv()=>Some("cancelled"),
            _=interrupt.recv()=>Some("cancelled"),
            _=peer.read_u8()=>Some("closed"),
            _=checks.tick()=>current(options,&init.binding).await.err(),
            result=&mut startup=>break result?,
        };
        if let Some(reason) = stopped {
            cancel.cancel();
            // Keep the actor alive until startup cancellation has completed
            // cleanup, including cancellation before its recipient was ready.
            match timeout(Duration::from_secs(60), startup).await {
                Ok(Ok(attempt)) => {
                    timeout(Duration::from_secs(25), attempt.shutdown())
                        .await
                        .map_err(|_| "google_browser_cleanup_unconfirmed")?
                        .map_err(|_| "google_browser_cleanup_unconfirmed")?;
                }
                Ok(Err("google_browser_cleanup_unconfirmed")) | Err(_) => {
                    return Err("google_browser_cleanup_unconfirmed".into());
                }
                Ok(Err(_)) => {}
            }
            record_teardown(options, &init.binding).await?;
            return Err(reason.into());
        }
    };
    let result = async {
        current(options, &init.binding).await?;
        let mut checks = interval(Duration::from_secs(2));
        checks.set_missed_tick_behavior(MissedTickBehavior::Delay);
        let work = timeout(
            bootstrap::ATTEMPT_LIFETIME,
            flow(options, &init, &mut peer, &mut attempt),
        );
        tokio::pin!(work);
        loop {
            tokio::select! {biased;
                _=term.recv()=>return Err("cancelled"),
                _=interrupt.recv()=>return Err("cancelled"),
                _=checks.tick()=>current(options,&init.binding).await?,
                result=&mut work=>return result.map_err(|_|"expired")?,
            }
        }
    }
    .await;
    // Even EOF, revocation and a provider error wait for confirmed removal.
    if attempt.shutdown().await.is_err() {
        let _ = send(&mut peer, json!({"status":"cleanup_unconfirmed"})).await;
        return Err("google_browser_cleanup_unconfirmed".into());
    }
    // Root owns the receipt even after gateway cancellation/logout. A late
    // acknowledgement can clear only this authenticated challenge, never a
    // successor. Failed or killed SDK cleanup leaves the durable fence intact.
    if record_teardown(options, &init.binding).await.is_err() {
        let _ = send(&mut peer, json!({"status":"cleanup_unconfirmed"})).await;
        return Err("google_browser_cleanup_unconfirmed".into());
    }
    if result.is_err() {
        let _ = send(&mut peer, json!({"status":"unavailable"})).await;
    }
    result.map_err(str::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn both_service_templates_allow_sdk_teardown_to_finish() {
        for unit in [
            include_str!("../../../deployment/kiri/lifecycle/moosuri-google-browser@.service"),
            include_str!("../../../deployment/kiri/setup/moosuri-google-browser@.service.in"),
        ] {
            let seconds: u64 = unit
                .lines()
                .find_map(|line| line.strip_prefix("TimeoutStopSec="))
                .unwrap()
                .parse()
                .unwrap();
            assert!(seconds >= bootstrap::BOOTSTRAP_SHUTDOWN_GRACE.as_secs() + 20);
        }
    }
    #[test]
    fn signed_in_commands_have_one_exact_shape() {
        let bootstrap = json!({"bootstrap":{"setup_id":Uuid::new_v4(),
            "redirect_uri":"https://assistant.example.com/api/v1/mail/oauth/callback", "action":"authorize"}});
        assert!(matches!(
            serde_json::from_value::<Command>(bootstrap.clone()),
            Ok(Command::Bootstrap(_))
        ));
        assert!(matches!(
            serde_json::from_value::<Command>(json!({"url":"https://accounts.google.com/"})),
            Ok(Command::Consent(_))
        ));
        let mut mixed = bootstrap.clone();
        mixed["url"] = "https://accounts.google.com/".into();
        assert!(serde_json::from_value::<Command>(mixed).is_err());
        let mut flags = bootstrap;
        flags["bootstrap"]["args"] = json!(["--account=other"]);
        assert!(serde_json::from_value::<Command>(flags).is_err());
        assert!(
            serde_json::from_value::<Command>(
                json!({"url":"https://accounts.google.com/", "client_id":"override"})
            )
            .is_err()
        );
    }
}
