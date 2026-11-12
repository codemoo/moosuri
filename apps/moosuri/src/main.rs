use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use clap::{Parser, Subcommand};
use moosuri_store::Store;
use rand::{Rng, distributions::Alphanumeric};
use std::{
    fs::OpenOptions,
    future::IntoFuture,
    io::{Read, Write},
    net::{IpAddr, SocketAddr},
    path::{Path, PathBuf},
    time::Duration,
};

include!(concat!(env!("OUT_DIR"), "/assets.rs"));

#[derive(Parser)]
#[command(
    version,
    about = "A private, self-hosted assistant. Development preview."
)]
struct Cli {
    #[arg(long, global = true, value_name = "DIRECTORY")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}
#[derive(Subcommand)]
enum Command {
    /// Create one administrator. Password input is hidden; blank generates a temporary one.
    Init {
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: String,
        /// Read a chosen password from an owner-only file; it is never printed.
        #[arg(long)]
        password_file: Option<PathBuf>,
    },
    /// Serve the built React app, private chat API and runtime behind an HTTPS proxy.
    Serve {
        #[arg(long, default_value = "127.0.0.1:3210")]
        bind: SocketAddr,
        /// Exact browser origin (for example https://assistant.example.com).
        #[arg(long)]
        origin: String,
        /// Permit HTTP only for an explicit localhost/loopback origin during development.
        #[arg(long)]
        dev: bool,
        /// Trust a loopback proxy only when it overwrites the selected client-IP header.
        #[arg(long)]
        trusted_proxy: Option<IpAddr>,
        #[arg(long, default_value = "x-real-ip")]
        client_ip_header: String,
    },
    /// Check a stopped installation without printing account or secret data.
    Doctor,
    /// Print public TypeScript DTOs, generated from Rust.
    ExportTypes,
}

#[tokio::main]
async fn main() {
    if let Err(code) = run(Cli::parse()).await {
        eprintln!("moosuri: {code}");
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> Result<(), String> {
    if matches!(cli.command, Command::ExportTypes) {
        print!("{}", moosuri_core::typescript_contract());
        return Ok(());
    }
    let data = cli
        .data_dir
        .or_else(default_data_dir)
        .ok_or("data_directory_required")?;
    if !data.is_absolute() {
        return Err("data_directory_must_be_absolute".into());
    }
    match cli.command {
        Command::Init {
            name,
            email,
            password_file,
        } => {
            let store = Store::open(&data).map_err(|e| e.to_string())?;
            if store.initialized().await.map_err(|e| e.to_string())? {
                return Err("already_initialized".into());
            }
            let chosen = match password_file {
                Some(path) => read_password_file(&path)?,
                None => rpassword::prompt_password(
                    "Administrator password (blank generates a temporary password): ",
                )
                .map_err(|_| "controlling_terminal_required_use_password_file")?,
            };
            let temporary = chosen.is_empty();
            // Open the private output channel before creating an unrecoverable secret.
            let mut tty = if temporary {
                Some(
                    OpenOptions::new()
                        .write(true)
                        .open("/dev/tty")
                        .map_err(|_| "controlling_terminal_required")?,
                )
            } else {
                None
            };
            let password = if temporary {
                rand::rngs::OsRng
                    .sample_iter(Alphanumeric)
                    .take(32)
                    .map(char::from)
                    .collect()
            } else {
                chosen
            };
            store
                .bootstrap(name, email, password.clone(), temporary)
                .await
                .map_err(|e| e.to_string())?;
            if let Some(output) = tty.as_mut() {
                writeln!(
                    output,
                    "Temporary administrator password (shown once): {password}"
                )
                .map_err(|_| "terminal_output_failed")?;
            }
            println!(
                "Administrator created. Start moosuri serve with the same data directory and an exact origin."
            );
        }
        Command::Serve {
            bind,
            origin,
            dev,
            trusted_proxy,
            client_ip_header,
        } => {
            if !bind.ip().is_loopback() {
                return Err("loopback_bind_required_use_a_tls_proxy".into());
            }
            let mut config =
                moosuri_server::ServerConfig::new(&origin, dev).map_err(str::to_owned)?;
            if let Some(proxy) = trusted_proxy {
                config = config
                    .with_trusted_proxy(proxy, &client_ip_header)
                    .map_err(str::to_owned)?;
            }
            if !ASSETS.iter().any(|(name, _)| *name == "/index.html") {
                return Err(
                    "web_assets_missing_run_npm_ci_and_build_in_web_then_rebuild_rust".into(),
                );
            }
            let store = Store::open(&data).map_err(|e| e.to_string())?;
            let runtime = moosuri_agent::Runtime::start(store.clone());
            let app = moosuri_server::router_with_runtime(store, config, runtime.clone())
                .fallback(static_asset);
            let listener = tokio::net::TcpListener::bind(bind)
                .await
                .map_err(|_| "listen_failed")?;
            println!(
                "moosuri {} listening on {}",
                env!("CARGO_PKG_VERSION"),
                listener.local_addr().map_err(|_| "listen_failed")?
            );
            let (drain, stopping) = tokio::sync::oneshot::channel::<()>();
            let server = axum::serve(
                listener,
                app.into_make_service_with_connect_info::<SocketAddr>(),
            )
            .with_graceful_shutdown(async {
                let _ = stopping.await;
            })
            .into_future();
            tokio::pin!(server);
            let result = tokio::select! {
                result = &mut server => result.map_err(|_| "server_failed"),
                _ = shutdown_signal() => {
                    // Stop accepting first, interrupt model work, and bound HTTP
                    // draining even if a peer never finishes its body or reads SSE.
                    runtime.stop();
                    let _ = drain.send(());
                    let (http, ()) = tokio::join!(
                        tokio::time::timeout(Duration::from_secs(3), &mut server),
                        runtime.shutdown(),
                    );
                    match http { Ok(Err(_)) => Err("server_failed"), _ => Ok(()) }
                }
            };
            if !runtime.is_stopping() {
                runtime.shutdown().await;
            }
            result?;
        }
        Command::Doctor => {
            if !data.join("assistant.sqlite").is_file() {
                return Err("installation_missing".into());
            }
            let store = Store::open(&data).map_err(|e| e.to_string())?;
            let ready = store.initialized().await.map_err(|e| e.to_string())?;
            println!(
                "SQLite: ready\nPrivate writer lock: acquired\nAdministrator initialized: {ready}\nText chat: available with a verified connection\nBackup/restore: not implemented"
            );
        }
        Command::ExportTypes => unreachable!(),
    }
    Ok(())
}
fn default_data_dir() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("MOOSURI_DATA_DIR") {
        return Some(PathBuf::from(path));
    }
    if let Some(path) = std::env::var_os("XDG_DATA_HOME") {
        return Some(PathBuf::from(path).join("moosuri"));
    }
    let user_root = PathBuf::from(std::env::var_os("HOME")?);
    if cfg!(target_os = "macos") {
        Some(user_root.join("Library/Application Support/moosuri"))
    } else {
        Some(user_root.join(".local/share/moosuri"))
    }
}
fn read_password_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "password_file_unreadable")?;
    let info = file.metadata().map_err(|_| "password_file_unreadable")?;
    if !info.is_file() || info.permissions().mode() & 0o077 != 0 || info.len() > 1024 {
        return Err("password_file_must_be_private_regular_and_small".into());
    }
    let mut password = String::new();
    file.read_to_string(&mut password)
        .map_err(|_| "password_file_unreadable")?;
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    Ok(password)
}
async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };
    let terminate = async {
        if let Ok(mut signal) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            signal.recv().await;
        }
    };
    tokio::select! { _ = ctrl_c => (), _ = terminate => () }
}
async fn static_asset(uri: Uri) -> Response {
    let path = match uri.path() {
        "/" | "/app" | "/admin" | "/admin/providers" | "/admin/usage" | "/settings/model"
        | "/settings/usage" | "/app/knowledge" | "/app/memory" | "/login" => "/index.html",
        other => other,
    };
    let Some((_, bytes)) = ASSETS.iter().find(|(name, _)| *name == path) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let content_type = match path.rsplit('.').next() {
        Some("html") => "text/html; charset=utf-8",
        Some("js") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("webmanifest" | "json") => "application/manifest+json",
        Some("woff2") => "font/woff2",
        _ => "application/octet-stream",
    };
    let mut response = Response::new(Body::from(*bytes));
    let h = response.headers_mut();
    h.insert(header::CONTENT_TYPE, content_type.parse().unwrap());
    h.insert(header::CACHE_CONTROL, "no-cache".parse().unwrap());
    h.insert(header::X_CONTENT_TYPE_OPTIONS, "nosniff".parse().unwrap());
    h.insert(header::REFERRER_POLICY, "no-referrer".parse().unwrap());
    h.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'; font-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}
