use super::{
    Binding, Envelope, Error, PROTOCOL, Receiver,
    chrome::{Chrome, Page},
    consent::Consent,
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

async fn receive<T: DeserializeOwned>(
    input: &mut (impl AsyncBufRead + Unpin),
    seconds: u64,
) -> Result<T, Error> {
    tokio::time::timeout(Duration::from_secs(seconds), async {
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
    })
    .await
    .map_err(|_| Error::Unavailable)?
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
// During a running operation only a live, otherwise silent controller is valid.
// EOF, cancellation or pipelined commands abort before another action is taken.
async fn connected<T>(
    input: &mut (impl AsyncBufRead + Unpin),
    seconds: u64,
    operation: impl Future<Output = Result<T, Error>>,
) -> Result<T, Error> {
    tokio::select! { biased;
        _ = input.read_u8() => Err(Error::Unavailable),
        result = tokio::time::timeout(Duration::from_secs(seconds), operation) => result.map_err(|_| Error::Unavailable)?,
    }
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

async fn run(
    input: &mut (impl AsyncBufRead + Unpin),
    output: &mut (impl AsyncWrite + Unpin),
    tenant: Uuid,
) -> Result<(), Error> {
    let init: Init = receive(input, 10).await?;
    if init.protocol != PROTOCOL || !init.binding.valid() || init.binding.tenant != tenant {
        return Err(Error::Invalid);
    }
    let receiver = connected(input, 25, Receiver::create(init.binding.clone())).await?;
    send(
        output,
        json!({"status":"vault","recipient":receiver.recipient()}),
    )
    .await?;
    let delivery: Delivery = receive(input, 730).await?;
    let secret = connected(
        input,
        25,
        receiver.open(&delivery.google_email, delivery.envelope),
    )
    .await?;
    send(output, json!({"status":"verified"})).await?;
    let release: Release = receive(input, 30).await?;
    if release.proceed != init.binding.challenge {
        return Err(Error::Invalid);
    }
    let cancel = CancellationToken::new();
    let mut browser = connected(input, 35, Chrome::start(&init.binding, cancel.clone())).await?;
    let outcome = async {
        connected(
            input,
            240,
            login(&mut browser, &delivery.google_email, secret, output),
        )
        .await?;
        send(output, json!({"status":"signed_in"})).await?;
        let consent: Consent = receive(input, 45).await?;
        connected(
            input,
            120,
            browser.consent(consent, &init.binding, &delivery.google_email),
        )
        .await
    }
    .await;
    // No terminal receipt until Chrome and X have been stopped. The launcher
    // still must confirm removal of its owned container before releasing a slot.
    cancel.cancel();
    browser.shutdown().await;
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
    let outcome = if let Some(tenant) = tenant.filter(|id| !id.is_nil()) {
        if Chrome::config(tenant)
            .is_ok_and(|c| crate::browser::capsule_health_mode(&c, true).is_ok())
        {
            tokio::select! { biased;
                _ = stopped() => Err(Error::Unavailable),
                result = tokio::time::timeout(Duration::from_secs(22 * 60), run(&mut input, &mut output, tenant)) =>
                    result.unwrap_or(Err(Error::Unavailable)),
            }
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
        return false;
    }
    true
}
