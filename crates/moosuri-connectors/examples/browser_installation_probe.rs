//! Reuse the installer's actual browser startup and cleanup check on an existing
//! installation. Supply its unverified CapsuleConfig through private stdin.
//! No credentials, account session or remote page are used by this probe.
use moosuri_connectors::browser::capsule::{CapsuleConfig, CapsuleReader};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let google = match std::env::args().skip(1).collect::<Vec<_>>().as_slice() {
        [] => false,
        [flag] if flag == "--google" => true,
        _ => return Err("usage: browser_installation_probe [--google] < capsule.json".into()),
    };
    let mut bytes = Vec::new();
    tokio::io::stdin().take(16385).read_to_end(&mut bytes).await?;
    if bytes.len() > 16384 {
        return Err("capsule_config_too_large".into());
    }
    let config: CapsuleConfig = serde_json::from_slice(&bytes)?;
    let (config, evidence) =
        CapsuleReader::verify_installation(config, google, &CancellationToken::new()).await?;
    let evidence: serde_json::Value = serde_json::from_slice(&evidence)?;
    println!("{}", serde_json::json!({ "config": config, "evidence": evidence }));
    Ok(())
}
