//! Actual launcher acceptance, invoked only by deployment/browser/smoke.py.
//! The configuration identifies that harness's synthetic image and owned proxy.
use moosuri_connectors::browser::{
    BrowserError,
    capsule::{CapsuleConfig, CapsuleReader},
};
use tokio::io::AsyncReadExt;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut bytes = String::new();
    tokio::io::stdin()
        .take(16385)
        .read_to_string(&mut bytes)
        .await?;
    let reader = CapsuleReader::new(CapsuleConfig::from_json(&bytes)?)?;
    let cancel = CancellationToken::new();
    let first_account = Uuid::new_v4();
    let second_account = Uuid::new_v4();
    let page = reader
        .read(first_account, "http://93.184.216.34/", &cancel)
        .await?;
    assert!(page.text.contains("SCRIPT_RENDERED_OK"));
    println!("PASS KL-01: Rust launcher renders the owned fixture");

    let mut first = reader
        .start_member_workspace(first_account, &cancel)
        .await?;
    let mut second = reader
        .start_member_workspace(second_account, &cancel)
        .await?;
    let result = async {
        assert!(matches!(
            reader.start_member_workspace(Uuid::new_v4(), &cancel).await,
            Err(BrowserError::Capacity)
        ));
        first.open("http://93.184.216.34/").await?;
        second.open("http://93.184.216.34/refresh").await?;
        assert_ne!(
            first.frame().await?.frame_token,
            second.frame().await?.frame_token
        );
        println!("PASS KL-02: two member workers have separate pages and capacity is enforced");
        Ok::<(), BrowserError>(())
    }
    .await;
    let first_stopped = first.shutdown().await;
    if let Some(error) = result.err().or(first_stopped.err()) {
        let _ = second.shutdown().await;
        return Err(error.into());
    }
    let survives = second.observe().await;
    let second_stopped = second.shutdown().await;
    survives?;
    second_stopped?;
    println!("PASS KL-03: stopping one member preserves the other and both shut down");
    let replacement = reader
        .start_member_workspace(first_account, &cancel)
        .await?;
    replacement.shutdown().await?;
    println!("PASS KL-04: cleaned member slots can be reused");
    Ok(())
}
