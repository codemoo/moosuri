//! Local installation operator interface for stable provisioning identities.
use clap::{Args, Subcommand};
use moosuri_store::runtime_registry;
use std::path::PathBuf;
use uuid::Uuid;

#[derive(Args)]
pub struct Options {
    /// Existing private absolute directory dedicated to the installation registry.
    #[arg(long)]
    registry: PathBuf,
    #[command(subcommand)]
    command: Operation,
}

#[derive(Subcommand)]
enum Operation {
    /// Reserve an account, tenant and volume identity. Retry with the original request ID.
    Reserve {
        #[arg(long)]
        request_id: Uuid,
        #[arg(long)]
        email: String,
    },
    /// Open routing after a matching member process answers over its private socket.
    Activate {
        #[arg(long)]
        email: String,
        #[arg(long)]
        ipc_root: PathBuf,
    },
    /// Permanently fence this generation at the account gateway.
    Fence {
        #[arg(long)]
        email: String,
    },
    /// Look up an existing allocation without opening a user's data volume.
    Lookup {
        #[arg(long)]
        email: String,
    },
}

pub async fn run(options: &Options) -> Result<(), String> {
    let allocation = match &options.command {
        Operation::Reserve { request_id, email } => {
            runtime_registry::reserve(&options.registry, *request_id, email.clone())
        }
        Operation::Activate { email, ipc_root } => {
            let allocation = runtime_registry::lookup(&options.registry, email.clone())
                .map_err(|e| e.to_string())?;
            let socket = ipc_root.join(&allocation.account_id).join("personal.sock");
            if !ipc_root.is_absolute()
                || std::fs::canonicalize(&socket).ok().as_ref() != Some(&socket)
            {
                return Err("invalid_member_socket".into());
            }
            let client = reqwest::Client::builder()
                .unix_socket(socket)
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(std::time::Duration::from_secs(4))
                .build()
                .map_err(|_| "invalid_member_transport")?;
            let response = client
                .get("http://personal.internal/health/ready")
                .send()
                .await
                .map_err(|_| "member_unavailable")?;
            let headers = response.headers();
            if response.status() != 204
                || headers
                    .get("x-moosuri-volume")
                    .and_then(|h| h.to_str().ok())
                    != Some(allocation.volume_id.as_str())
                || headers
                    .get("x-moosuri-account")
                    .and_then(|h| h.to_str().ok())
                    != Some(allocation.account_id.as_str())
                || headers
                    .get("x-moosuri-tenant")
                    .and_then(|h| h.to_str().ok())
                    != Some(allocation.tenant_id.as_str())
                || headers
                    .get("x-moosuri-generation")
                    .and_then(|h| h.to_str().ok())
                    .and_then(|n| n.parse::<i64>().ok())
                    != Some(allocation.generation)
            {
                return Err("member_identity_mismatch".into());
            }
            runtime_registry::set_routing(&options.registry, &allocation, true).map(|()| allocation)
        }
        Operation::Fence { email } => {
            let allocation = runtime_registry::lookup(&options.registry, email.clone())
                .map_err(|e| e.to_string())?;
            runtime_registry::set_routing(&options.registry, &allocation, false)
                .map(|()| allocation)
        }
        Operation::Lookup { email } => runtime_registry::lookup(&options.registry, email.clone()),
    }
    .map_err(|e| e.to_string())?;
    println!(
        "{}",
        serde_json::to_string(&allocation).map_err(|e| e.to_string())?
    );
    Ok(())
}

/// Entrypoint for a fresh member volume inside a personal runtime. It needs
/// no control-plane database, engine socket or installation credentials.
#[derive(Args)]
pub struct PersonalOptions {
    #[arg(long)]
    root: PathBuf,
    #[arg(long)]
    account: Uuid,
    #[arg(long)]
    tenant: Uuid,
    #[arg(long)]
    volume: Uuid,
    #[arg(long)]
    generation: u64,
    #[command(subcommand)]
    command: PersonalOperation,
}
#[derive(Subcommand)]
enum PersonalOperation {
    /// Create the allocated member with a temporary password read from the terminal.
    Initialize {
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: String,
        /// Accept one newline-terminated temporary password on private standard input.
        #[arg(long)]
        password_stdin: bool,
    },
    /// Check the owner, generation and persisted account before accepting traffic.
    Verify,
    /// Accept private member login, password change, session and logout over Unix HTTP.
    Serve {
        #[arg(long)]
        socket: PathBuf,
    },
}
pub async fn personal(options: &PersonalOptions) -> Result<(), String> {
    let owner = moosuri_store::personal_volume::VolumeOwner {
        account: options.account,
        tenant: options.tenant,
        volume: options.volume,
        generation: options.generation,
    };
    match &options.command {
        PersonalOperation::Serve { socket } => {
            let store = owner
                .open_verified(&options.root)
                .await
                .map_err(|e| e.to_string())?;
            return serve_member(socket, store, owner).await;
        }
        PersonalOperation::Initialize {
            name,
            email,
            password_stdin,
        } => {
            let password = if *password_stdin {
                use std::io::Read;
                let mut value = zeroize::Zeroizing::new(String::new());
                std::io::stdin()
                    .lock()
                    .take(259)
                    .read_to_string(&mut value)
                    .map_err(|_| "password_input_unavailable")?;
                let line = value
                    .strip_suffix("\n")
                    .and_then(|s| (!s.contains(['\r', '\n']) && s.len() <= 256).then_some(s))
                    .ok_or("expected_one_password_line")?;
                line.to_owned()
            } else {
                rpassword::prompt_password("Temporary password / 임시 비밀번호: ")
                    .map_err(|_| "password_input_unavailable")?
            };
            owner
                .initialize(&options.root, name.clone(), email.clone(), password)
                .await
        }
        PersonalOperation::Verify => owner.open_verified(&options.root).await.map(drop),
    }
    .map_err(|error| error.to_string())?;
    println!("ok");
    Ok(())
}

async fn serve_member(
    socket: &std::path::Path,
    store: moosuri_store::Store,
    owner: moosuri_store::personal_volume::VolumeOwner,
) -> Result<(), String> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
    use tokio::net::UnixListener;
    let parent = socket.parent().ok_or("invalid_socket_path")?;
    let meta = std::fs::symlink_metadata(parent).map_err(|_| "invalid_socket_directory")?;
    if !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || std::fs::canonicalize(parent).map_err(|_| "invalid_socket_directory")? != parent
        || socket.file_name() != Some("personal.sock".as_ref())
    {
        return Err("invalid_socket_directory".into());
    }
    // Parent is dedicated to this runtime. Hold its lock for the service lifetime,
    // so two starts cannot unlink or rebind a live listener during startup.
    let lock = std::fs::File::open(parent).map_err(|_| "socket_directory_unavailable")?;
    fs2::FileExt::try_lock_exclusive(&lock).map_err(|_| "socket_directory_busy")?;
    match std::fs::symlink_metadata(socket) {
        Ok(m) if m.file_type().is_socket() && m.uid() == meta.uid() => {
            match tokio::net::UnixStream::connect(socket).await {
                Err(e) if e.kind() == std::io::ErrorKind::ConnectionRefused => {
                    std::fs::remove_file(socket).map_err(|_| "stale_socket_unavailable")?
                }
                _ => return Err("socket_busy".into()),
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        _ => return Err("invalid_socket_path".into()),
    }
    let listener = UnixListener::bind(socket).map_err(|_| "socket_bind_failed")?;
    std::fs::set_permissions(socket, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| "socket_permissions_failed")?;
    let ready = axum::routing::get(move || async move {
        (
            axum::http::StatusCode::NO_CONTENT,
            [
                ("x-moosuri-account", owner.account.to_string()),
                ("x-moosuri-tenant", owner.tenant.to_string()),
                ("x-moosuri-volume", owner.volume.to_string()),
                ("x-moosuri-generation", owner.generation.to_string()),
                ("cache-control", "no-store".to_string()),
            ],
        )
    });
    axum::serve(
        listener,
        moosuri_server::personal_auth_router(store).route("/health/ready", ready),
    )
    .await
    .map_err(|_| "personal_service_stopped".into())
}
