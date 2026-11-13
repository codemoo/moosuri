use base64::Engine;
use moosuri_connectors::browser::{
    BrowserConfig, BrowserError, BrowserSession, PageHandle, capsule::WorkspaceInput,
    workspace::BrowserWorkspace,
};
use serde::Deserialize;
use serde_json::json;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum WorkerRequest {
    Open {
        url: String,
    },
    Read {
        handle: PageHandle,
    },
    Screenshot {
        handle: PageHandle,
    },
    Close {
        handle: PageHandle,
    },
    WorkspaceOpen {
        url: String,
    },
    WorkspaceObserve,
    WorkspaceFrame,
    WorkspaceInput {
        expected_frame_token: String,
        input: WorkspaceInput,
    },
    Shutdown,
}

enum Worker {
    Read(BrowserSession),
    Workspace(BrowserWorkspace),
}
impl Worker {
    fn expires_in(&self) -> std::time::Duration {
        match self {
            Self::Read(s) => s.expires_in(),
            Self::Workspace(s) => s.expires_in(),
        }
    }
    async fn shutdown(self) {
        match self {
            Self::Read(s) => s.shutdown().await,
            Self::Workspace(s) => s.shutdown().await,
        }
    }
}
fn code(error: &BrowserError) -> &'static str {
    match error {
        BrowserError::StalePage => "stale_page",
        BrowserError::InvalidHandle => "invalid_handle",
        BrowserError::InvalidUrl => "invalid_url",
        BrowserError::ForbiddenDestination => "forbidden_destination",
        BrowserError::OutputTooLarge => "output_too_large",
        _ => "failed",
    }
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
    let workspace = std::env::args().nth(1).as_deref() == Some("--workspace");
    if std::env::args().len() > 2 || (std::env::args().nth(1).is_some() && !workspace) {
        eprintln!("unsupported worker argument");
        std::process::exit(1)
    }
    let Some(tenant) = std::env::var("MOOSURI_BROWSER_TENANT")
        .ok()
        .and_then(|t| Uuid::parse_str(&t).ok())
        .filter(|t| !t.is_nil())
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
    let launched = if workspace {
        BrowserWorkspace::launch(config, cancellation.clone())
            .await
            .map(Worker::Workspace)
    } else {
        BrowserSession::launch(config, cancellation.clone())
            .await
            .map(Worker::Read)
    };
    let mut session = match launched {
        Ok(session) => session,
        Err(error) => {
            println!("{}", json!({"ready":false,"error":error.to_string()}));
            std::process::exit(1);
        }
    };
    let mut output = tokio::io::stdout();
    let ready = format!(
        "{}\n",
        json!({"ready":true,"javascript_enabled":javascript_enabled,"tenant":tenant,"workspace":workspace})
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
        let idle = tokio::time::sleep(std::time::Duration::from_secs(if workspace {
            30 * 60
        } else {
            120
        }));
        let read = tokio::select! { biased;
            _ = idle => break,
            _ = cancellation.cancelled() => break,
            _ = &mut deadline => break,
            result = next => result,
        };
        if !matches!(read, Ok(1..)) || bytes.len() > 16 * 1024 || !bytes.ends_with(b"\n") {
            break;
        }
        let request = serde_json::from_slice::<WorkerRequest>(&bytes);
        if matches!(request, Ok(WorkerRequest::Shutdown)) {
            break;
        }
        let result = match (&mut session, request) {
            (Worker::Read(s), Ok(WorkerRequest::Open { url })) => s.open(&url).await.map(|v|json!(v)),
            (Worker::Read(s), Ok(WorkerRequest::Read { handle })) => s.read(&handle).await.map(|v|json!(v)),
            (Worker::Read(s), Ok(WorkerRequest::Screenshot { handle })) => s.screenshot(&handle).await.map(|v| json!({"handle":v.handle,"mime":"image/png","base64":base64::engine::general_purpose::STANDARD.encode(v.png)})),
            (Worker::Read(s), Ok(WorkerRequest::Close { handle })) => s.close(&handle).await.map(|_|json!({"closed":true})),
            (Worker::Workspace(s), Ok(WorkerRequest::WorkspaceOpen { url })) => s.open(&url).await.map(|v|json!(v)),
            (Worker::Workspace(s), Ok(WorkerRequest::WorkspaceObserve)) => s.observe().await.map(|v|json!(v)),
            (Worker::Workspace(s), Ok(WorkerRequest::WorkspaceFrame)) => s.frame().await.map(|v|json!(v)),
            (Worker::Workspace(s), Ok(WorkerRequest::WorkspaceInput { expected_frame_token, input })) => s.input(&expected_frame_token, input).await.map(|_|json!({"accepted":true})),
            _ => Err(BrowserError::Protocol),
        };
        let fatal = workspace
            && matches!(&result, Err(e) if !matches!(e, BrowserError::StalePage | BrowserError::InvalidHandle | BrowserError::InvalidUrl | BrowserError::ForbiddenDestination));
        let value = match result {
            Ok(value) => json!({"ok":true,"result":value}),
            Err(error) => json!({"ok":false,"error":error.to_string(),"code":code(&error)}),
        };
        let mut line = serde_json::to_vec(&value).unwrap();
        line.push(b'\n');
        if output.write_all(&line).await.is_err() || output.flush().await.is_err() || fatal {
            break;
        }
    }
    session.shutdown().await;
}
