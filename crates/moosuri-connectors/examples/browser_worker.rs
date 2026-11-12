use base64::Engine;
use moosuri_connectors::browser::{BrowserConfig, BrowserSession, PageHandle};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Open { url: String },
    Read { handle: PageHandle },
    Screenshot { handle: PageHandle },
    Close { handle: PageHandle },
    Shutdown,
}

#[tokio::main]
async fn main() {
    let proxy = match std::env::var("MOOSURI_BROWSER_PROXY_IP")
        .ok()
        .and_then(|raw| moosuri_connectors::browser::private_proxy(&raw).ok())
    {
        Some(proxy) => proxy,
        None => {
            eprintln!("private IPv4 proxy configuration required");
            std::process::exit(1)
        }
    };
    if std::env::args().nth(1).as_deref() == Some("--validate-proxy") {
        println!("{proxy}");
        return;
    }
    if std::env::args().nth(1).is_some() {
        eprintln!("unsupported worker argument");
        std::process::exit(1)
    }
    let Some(tenant) = std::env::var("MOOSURI_BROWSER_TENANT")
        .ok()
        .and_then(|t| Uuid::parse_str(&t).ok())
    else {
        eprintln!("tenant UUID required");
        std::process::exit(1);
    };
    let cancellation = CancellationToken::new();
    let control = cancellation.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        control.cancel();
    });
    let javascript_enabled =
        std::env::var("MOOSURI_BROWSER_JAVASCRIPT").ok().as_deref() == Some("enabled");
    let config = BrowserConfig {
        tenant,
        chromium: PathBuf::from("/usr/bin/chromium"),
        profile_root: PathBuf::from("/profiles"),
        proxy,
        javascript_enabled,
    };
    let mut session = match BrowserSession::launch(config, cancellation.clone()).await {
        Ok(session) => session,
        Err(error) => {
            println!("{}", json!({"ready":false,"error":error.to_string()}));
            std::process::exit(1);
        }
    };
    let mut output = tokio::io::stdout();
    let ready = format!(
        "{}\n",
        json!({"ready":true,"javascript_enabled":javascript_enabled,"tenant":tenant})
    );
    let _ = output.write_all(ready.as_bytes()).await;
    let _ = output.flush().await;
    let mut reader = BufReader::new(tokio::io::stdin());
    let deadline = tokio::time::sleep(session.expires_in());
    tokio::pin!(deadline);
    loop {
        let mut bytes = Vec::new();
        let next = async {
            (&mut reader)
                .take(16 * 1024 + 1)
                .read_until(b'\n', &mut bytes)
                .await
        };
        let read = tokio::select! { biased;
            _ = cancellation.cancelled() => break,
            _ = &mut deadline => break,
            result = next => result,
        };
        if !matches!(read, Ok(1..)) || bytes.len() > 16 * 1024 {
            break;
        }
        let request = serde_json::from_slice::<Input>(&bytes);
        let result = match request {
            Ok(Input::Open { url }) => session.open(&url).await.map(|v|json!(v)),
            Ok(Input::Read { handle }) => session.read(&handle).await.map(|v|json!(v)),
            Ok(Input::Screenshot { handle }) => session.screenshot(&handle).await.map(|v| json!({"handle":v.handle,"mime":"image/png","base64":base64::engine::general_purpose::STANDARD.encode(v.png)})),
            Ok(Input::Close { handle }) => session.close(&handle).await.map(|_|json!({"closed":true})),
            Ok(Input::Shutdown) => break,
            Err(_) => { let _ = output.write_all(b"{\"ok\":false,\"error\":\"invalid command\"}\n").await; let _ = output.flush().await; continue; }
        };
        let value = match result {
            Ok(value) => json!({"ok":true,"result":value}),
            Err(error) => json!({"ok":false,"error":error.to_string()}),
        };
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        if output.write_all(&line).await.is_err() || output.flush().await.is_err() {
            break;
        }
    }
    session.shutdown().await;
}
