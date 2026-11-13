use axum::{
    body::Body,
    http::{StatusCode, Uri, header},
    response::{IntoResponse, Response},
};
use clap::{Parser, Subcommand, ValueEnum};
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
mod skills;

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
#[derive(Clone, ValueEnum)]
enum RecoveryRole {
    Admin,
    Member,
}

#[derive(Subcommand)]
enum Command {
    /// Author or check local instruction packages without an installation.
    Skills {
        #[command(subcommand)]
        command: skills::Command,
    },
    /// Recover one journal-locked account in a stopped installation.
    RecoverAccount {
        #[arg(long)]
        account_id: String,
        /// Explicit current role; the old role is never inferred.
        #[arg(long, value_enum)]
        role: RecoveryRole,
        /// Owner-only file containing a new temporary password; never printed.
        #[arg(long)]
        password_file: PathBuf,
    },
    /// Configure or create encrypted exports of a stopped installation.
    Backup {
        #[command(subcommand)]
        command: BackupCommand,
    },
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
        /// Owner-only Google OAuth web client JSON file; never pass a secret on the CLI.
        #[arg(long)]
        google_client_file: Option<PathBuf>,
        /// Optional owner-only Brave Search key file; public HTTP reading needs no key.
        #[arg(long)]
        web_search_key_file: Option<PathBuf>,
        /// Owner-only JSON for an already verified isolated Linux browser deployment.
        #[arg(long)]
        browser_capsule_file: Option<PathBuf>,
    },
    /// Check a stopped installation without printing account or secret data.
    Doctor,
    /// Print public TypeScript DTOs, generated from Rust.
    ExportTypes,
}

#[derive(Subcommand)]
enum BackupCommand {
    /// Generate a private recovery identity; print only its public age recipient.
    Keygen {
        #[arg(long)]
        output: PathBuf,
    },
    /// Verify an independently held age identity, then retain its public recipient epoch.
    Setup {
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        identity_file: PathBuf,
    },
    /// Export SQLite and snapshot journals to a new encrypted owner-private file.
    Create {
        #[arg(long)]
        output: PathBuf,
    },
}

#[tokio::main]
async fn main() {
    if let Err(code) = run(Cli::parse()).await {
        eprintln!("moosuri: {code}");
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> Result<(), String> {
    if let Command::Skills { command } = &cli.command {
        return skills::run(command);
    }
    if let Command::Backup {
        command: BackupCommand::Keygen { output },
    } = &cli.command
    {
        let recipient = moosuri_store::backup::keygen(output).map_err(|e| e.to_string())?;
        println!("{recipient}");
        return Ok(());
    }
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
        Command::Skills { .. } => unreachable!("skills authoring runs without installation state"),
        Command::Backup { command } => match command {
            BackupCommand::Keygen { .. } => unreachable!("keygen runs without installation state"),
            BackupCommand::Setup {
                recipient,
                identity_file,
            } => {
                let configured = moosuri_store::backup::setup(&data, &recipient, &identity_file)
                    .map_err(|e| e.to_string())?;
                println!(
                    "Encrypted export configured at key epoch {}. Retain all required private identities outside the installation and export directory. This is not an off-host restore test.",
                    configured.active_epoch.unwrap_or(0)
                );
            }
            BackupCommand::Create { output } => {
                let export =
                    moosuri_store::backup::create(&data, &output).map_err(|e| e.to_string())?;
                println!(
                    "Encrypted export {} complete at key epoch {}. Historical restore and an independently maintained latest journal remain unavailable.",
                    export.export_id, export.key_epoch
                );
            }
        },
        Command::RecoverAccount {
            account_id,
            role,
            password_file,
        } => {
            let password = read_password_file(&password_file)?;
            moosuri_store::recover_account(
                &data,
                account_id,
                match role {
                    RecoveryRole::Admin => moosuri_core::Role::Admin,
                    RecoveryRole::Member => moosuri_core::Role::Member,
                },
                password,
            )
            .await
            .map_err(|e| match e {
                moosuri_store::StoreError::Conflict => "account_not_recovery_required".into(),
                _ => e.to_string(),
            })?;
            println!(
                "Account recovered. Log in with the supplied temporary password and change it within 24 hours. Old sessions remain revoked."
            );
        }
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
            google_client_file,
            web_search_key_file,
            browser_capsule_file,
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
            if let Some(path) = google_client_file {
                config = config
                    .with_google_client_json_owned(read_google_client_file(&path)?)
                    .map_err(str::to_owned)?;
            }
            config = config
                .with_public_web(
                    web_search_key_file
                        .map(|path| read_web_search_key_file(&path))
                        .transpose()?,
                )
                .map_err(str::to_owned)?;
            if let Some(path) = browser_capsule_file {
                config = config
                    .with_browser_capsule_json(&read_browser_capsule_file(&path)?)
                    .map_err(str::to_owned)?;
            }
            if !ASSETS.iter().any(|(name, _)| *name == "/index.html") {
                return Err(
                    "web_assets_missing_run_npm_ci_and_build_in_web_then_rebuild_rust".into(),
                );
            }
            let store = config.open_store(&data).map_err(|e| e.to_string())?;
            let runtime = config.start_runtime(store.clone());
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
                "SQLite: ready\nPrivate writer lock: acquired\nAdministrator initialized: {ready}\nText chat: available with a verified connection\nStopped encrypted export: available after setup\nHistorical restore: not implemented"
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
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "password_file_unreadable")?;
    let info = file.metadata().map_err(|_| "password_file_unreadable")?;
    if !info.is_file()
        || info.uid() != unsafe { libc::geteuid() }
        || info.nlink() != 1
        || info.permissions().mode() & 0o077 != 0
        || info.len() > 1024
    {
        return Err("password_file_must_be_private_regular_and_small".into());
    }
    let mut password = String::new();
    file.take(1025)
        .read_to_string(&mut password)
        .map_err(|_| "password_file_unreadable")?;
    if password.len() > 1024 {
        return Err("password_file_must_be_private_regular_and_small".into());
    }
    if password.ends_with('\n') {
        password.pop();
        if password.ends_with('\r') {
            password.pop();
        }
    }
    Ok(password)
}
fn read_google_client_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "google_client_file_unreadable")?;
    let info = file
        .metadata()
        .map_err(|_| "google_client_file_unreadable")?;
    if !info.is_file()
        || info.uid() != unsafe { libc::geteuid() }
        || info.permissions().mode() & 0o077 != 0
        || info.nlink() != 1
        || info.len() > 16384
    {
        return Err("google_client_file_must_be_owner_only_regular_and_small".into());
    }
    let mut bytes = String::new();
    file.take(16385)
        .read_to_string(&mut bytes)
        .map_err(|_| "google_client_file_unreadable")?;
    if bytes.len() > 16384 {
        return Err("google_client_file_too_large".into());
    }
    Ok(bytes)
}
fn read_browser_capsule_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "browser_capsule_file_unreadable")?;
    let info = file
        .metadata()
        .map_err(|_| "browser_capsule_file_unreadable")?;
    if !info.is_file()
        || info.uid() != unsafe { libc::geteuid() }
        || info.permissions().mode() & 0o077 != 0
        || info.nlink() != 1
        || info.len() > 16384
    {
        return Err("browser_capsule_file_must_be_owner_only_regular_and_small".into());
    }
    let mut bytes = String::new();
    file.take(16385)
        .read_to_string(&mut bytes)
        .map_err(|_| "browser_capsule_file_unreadable")?;
    if bytes.len() > 16384 {
        return Err("browser_capsule_file_too_large".into());
    }
    Ok(bytes)
}
fn read_web_search_key_file(path: &Path) -> Result<String, String> {
    use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "web_search_key_file_unreadable")?;
    let info = file
        .metadata()
        .map_err(|_| "web_search_key_file_unreadable")?;
    if !info.is_file()
        || info.uid() != unsafe { libc::geteuid() }
        || info.permissions().mode() & 0o077 != 0
        || info.nlink() != 1
        || info.len() > 8192
    {
        return Err("web_search_key_file_must_be_owner_only_regular_and_small".into());
    }
    let mut key = String::new();
    file.take(8193)
        .read_to_string(&mut key)
        .map_err(|_| "web_search_key_file_unreadable")?;
    if key.len() > 8192 {
        return Err("web_search_key_file_too_large".into());
    }
    // One optional terminal newline is a file convention, not part of the key.
    if key.ends_with('\n') {
        key.pop();
        if key.ends_with('\r') {
            key.pop();
        }
    }
    Ok(key)
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
        "/" | "/app" | "/admin" | "/admin/providers" | "/admin/usage" | "/admin/google"
        | "/settings/model" | "/settings/mail" | "/settings/sources" | "/settings/usage"
        | "/app/knowledge" | "/app/memory" | "/app/skills" | "/login" => "/index.html",
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

#[cfg(test)]
mod web_key_tests {
    use super::*;
    use std::os::unix::{
        fs::{PermissionsExt, symlink},
        net::UnixListener,
    };
    #[test]
    fn password_file_rejects_links_permissions_special_files_and_unbounded_reads() {
        let dir = tempfile::tempdir_in(std::env::temp_dir().canonicalize().unwrap()).unwrap();
        let path = dir.path().join("password");
        std::fs::write(&path, "Synthetic-password-canary\r\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            read_password_file(&path).unwrap(),
            "Synthetic-password-canary"
        );
        let link = dir.path().join("link");
        symlink(&path, &link).unwrap();
        assert!(read_password_file(&link).is_err());
        let hard = dir.path().join("hard");
        std::fs::hard_link(&path, &hard).unwrap();
        assert!(read_password_file(&path).is_err());
        std::fs::remove_file(hard).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_password_file(&path).is_err());
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
        std::fs::write(&path, "x".repeat(1025)).unwrap();
        assert!(read_password_file(&path).is_err());
        let socket = dir.path().join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(read_password_file(&socket).is_err());
        assert!(read_password_file(dir.path()).is_err());
        let fifo = dir.path().join("fifo");
        let fifo_path = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
        assert!(read_password_file(&fifo).is_err());
    }
    #[test]
    fn browser_file_is_bounded_owner_only_regular_and_never_follows_links() {
        let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let file = dir.path().join("browser.json");
        std::fs::write(&file, "{}").unwrap();
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(read_browser_capsule_file(&file).unwrap(), "{}");
        let link = dir.path().join("link");
        symlink(&file, &link).unwrap();
        assert!(read_browser_capsule_file(&link).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o640)).unwrap();
        assert!(read_browser_capsule_file(&file).is_err());
        std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
        let hard = dir.path().join("hard");
        std::fs::hard_link(&file, &hard).unwrap();
        assert!(read_browser_capsule_file(&file).is_err());
        std::fs::remove_file(hard).unwrap();
        std::fs::write(&file, "x".repeat(16385)).unwrap();
        assert!(read_browser_capsule_file(&file).is_err());
        let socket = dir.path().join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(read_browser_capsule_file(&socket).is_err());
    }
    #[test]
    fn optional_search_key_file_rejects_symlinks_permissions_hardlinks_and_large_or_special_files()
    {
        let dir = tempfile::tempdir_in(std::env::temp_dir()).unwrap();
        let key = dir.path().join("key");
        std::fs::write(&key, "synthetic-brave-key\r\n").unwrap();
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        assert_eq!(
            read_web_search_key_file(&key).unwrap(),
            "synthetic-brave-key"
        );
        let link = dir.path().join("link");
        symlink(&key, &link).unwrap();
        assert!(read_web_search_key_file(&link).is_err());
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read_web_search_key_file(&key).is_err());
        std::fs::set_permissions(&key, std::fs::Permissions::from_mode(0o600)).unwrap();
        let hard = dir.path().join("hard");
        std::fs::hard_link(&key, &hard).unwrap();
        assert!(read_web_search_key_file(&key).is_err());
        std::fs::remove_file(hard).unwrap();
        std::fs::write(&key, "x".repeat(8193)).unwrap();
        assert!(read_web_search_key_file(&key).is_err());
        let socket = dir.path().join("socket");
        let _listener = UnixListener::bind(&socket).unwrap();
        assert!(read_web_search_key_file(&socket).is_err());
    }
}
