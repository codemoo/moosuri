//! Synthetic acceptance adapter for the real Rust Kiri launcher. Never production IPC.
use moosuri_connectors::browser::{
    capsule::{CapsuleConfig, CapsuleReader, GoogleAttempt, GooglePhase},
    google_auth::{Binding, Envelope},
};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Init {
    config: CapsuleConfig,
    binding: Binding,
}

async fn line(input: &mut BufReader<tokio::io::Stdin>) -> Result<Value, ()> {
    let mut bytes = Vec::new();
    (&mut *input)
        .take(16385)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| ())?;
    if bytes.len() > 16384 || bytes.pop() != Some(b'\n') {
        return Err(());
    }
    serde_json::from_slice(&bytes).map_err(|_| ())
}
async fn send(value: Value) -> Result<(), ()> {
    let mut bytes = value.to_string().into_bytes();
    bytes.push(b'\n');
    let mut out = tokio::io::stdout();
    out.write_all(&bytes).await.map_err(|_| ())?;
    out.flush().await.map_err(|_| ())
}
async fn dispatch(a: &mut GoogleAttempt, value: Value) -> Result<GooglePhase, ()> {
    if let Some(email) = value["google_email"].as_str() {
        let envelope: Envelope =
            serde_json::from_value(value["envelope"].clone()).map_err(|_| ())?;
        a.deliver(email, envelope).await.map_err(|_| ())
    } else if value.get("proceed").is_some() {
        if value["proceed"] != a.recipient().binding.challenge.to_string() {
            return Err(());
        }
        a.proceed().await.map_err(|_| ())
    } else {
        a.consent(
            value["url"].as_str().ok_or(())?,
            value["client_id"].as_str().ok_or(())?,
            value["service"].as_str().ok_or(())?,
        )
        .await
        .map_err(|_| ())
    }
}
async fn run() -> Result<(), ()> {
    let mut input = BufReader::new(tokio::io::stdin());
    let init: Init = serde_json::from_value(line(&mut input).await?).map_err(|_| ())?;
    let reader =
        CapsuleReader::new(init.config).map_err(|error| eprintln!("configuration: {error}"))?;
    let cancel = CancellationToken::new();
    let mut attempt = reader
        .start_google(init.binding, &cancel)
        .await
        .map_err(|error| eprintln!("start: {error}"))?;
    let result = async {
        send(json!({"status":"vault","recipient":attempt.recipient()})).await?;
        loop {
            let command = line(&mut input).await?;
            let mut phase = tokio::select! {biased;
                _=input.read_u8()=>return Err(()),
                p=dispatch(&mut attempt,command)=>p?,
            };
            send(json!({"status":phase})).await?;
            if phase == GooglePhase::Verifying {
                phase = tokio::select! {biased;
                    _=input.read_u8()=>return Err(()),
                    p=attempt.verification()=>p.map_err(|_|())?,
                };
                send(json!({"status":phase})).await?;
            }
            match phase {
                GooglePhase::Returned => return Ok(()),
                GooglePhase::Blocked | GooglePhase::NeedsHuman | GooglePhase::Unavailable => {
                    return Err(());
                }
                _ => {}
            }
        }
    }
    .await;
    attempt.shutdown().await.map_err(|_| ())?;
    result
}
#[tokio::main]
async fn main() {
    std::process::exit(if run().await.is_ok() { 0 } else { 1 });
}
