//! Local installation operator interface for stable provisioning identities.
mod browser;
mod browser_model;
mod browser_wire;
mod gws_setup;
mod public_watch;
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
            use std::os::unix::fs::MetadataExt;
            let owner_dir = ipc_root.join(&allocation.account_id);
            let root = std::fs::symlink_metadata(ipc_root).map_err(|_| "invalid_ipc_root")?;
            let member =
                std::fs::symlink_metadata(&owner_dir).map_err(|_| "invalid_member_directory")?;
            if !root.is_dir()
                || root.mode() & 0o077 != 0
                || root.uid() != unsafe { libc::geteuid() }
                || !member.is_dir()
                || member.mode() & 0o077 != 0
            {
                return Err("invalid_member_directory".into());
            }
            crate::kiri_lifecycle::health(
                &owner_dir.join("personal.sock"),
                &allocation,
                member.uid(),
            )
            .await?;
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
    /// Installer-only owner check; run as the dedicated member UID, never root.
    InitializationStatus {
        #[arg(long)]
        email: String,
    },
    /// Create the allocated member with a temporary password read from the terminal.
    Initialize {
        #[arg(long)]
        name: String,
        #[arg(long)]
        email: String,
        /// Accept one newline-terminated temporary password on private standard input.
        #[arg(long)]
        password_stdin: bool,
        /// The invitation capability was checked by the host; password was chosen by its member.
        #[arg(long, requires = "password_stdin")]
        member_chosen_password: bool,
    },
    /// Check the owner, generation and persisted account before accepting traffic.
    Verify,
    /// Register a Google web OAuth client inside this member. Read its downloaded JSON from stdin.
    ConfigureGws {
        #[arg(long)]
        redirect_uri: String,
    },
    /// Accept private member login, password change, session and logout over Unix HTTP.
    Serve {
        #[arg(long)]
        socket: PathBuf,
        /// Optional owner-only JSON for explicitly limited broker text previews.
        #[arg(long)]
        text_preview_config: Option<PathBuf>,
        /// Enable the dedicated member browser broker at /browser/browser.sock.
        #[arg(long)]
        browser: bool,
        /// Mount the member-scoped Google egress socket at /google/google.sock.
        #[arg(long)]
        google_readonly: bool,
        /// Run Google OAuth and the installed gws command inside this member.
        #[arg(long, conflicts_with = "google_readonly")]
        local_gws: bool,
        /// Opt-in v2 vault handoff for this one member: exact distinct HTTPS vault origin.
        #[arg(long)]
        vault_origin: Option<String>,
        /// Test binaries only: use this same-UID private socket as the browser broker (acceptance fixture).
        /// The option does not exist in a production build.
        #[cfg(feature = "test-fixtures")]
        #[arg(long, conflicts_with = "browser")]
        browser_test_socket: Option<PathBuf>,
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
        PersonalOperation::InitializationStatus { email } => {
            if unsafe { libc::geteuid() } == 0 {
                return Err("member_initialization_requires_nonroot_uid".into());
            }
            let initialized = owner
                .initialization_status(&options.root, email.clone())
                .await
                .map_err(|_| "member_initialization_status_failed")?;
            println!("{}", serde_json::json!({"initialized":initialized}));
            return Ok(());
        }
        PersonalOperation::ConfigureGws { redirect_uri } => {
            let directory = owner
                .verified_member_directory(&options.root)
                .map_err(|_| "invalid_member_volume")?;
            gws_setup::configure(
                &directory,
                redirect_uri,
                std::io::stdin().lock(),
                gws_setup::ExistingClient::Rotate,
            )?;
            println!("ok");
            return Ok(());
        }
        PersonalOperation::Serve {
            socket,
            text_preview_config,
            browser,
            google_readonly,
            local_gws,
            vault_origin,
            #[cfg(feature = "test-fixtures")]
            browser_test_socket,
        } => {
            // Deployed path: the fixed broker socket, owned by root. Test builds may name a private
            // fixture socket owned by this same UID instead; it is unreachable otherwise.
            let (browser_socket, browser_uid) =
                (browser.then(|| PathBuf::from("/browser/browser.sock")), 0);
            #[cfg(feature = "test-fixtures")]
            let (browser_socket, browser_uid) = match browser_test_socket {
                Some(path) => (Some(path.clone()), unsafe { libc::geteuid() }),
                None => (browser_socket, browser_uid),
            };
            let store = owner
                .open_verified(&options.root)
                .await
                .map_err(|e| e.to_string())?;
            let preview = text_preview_config
                .as_ref()
                .map(|p| read_preview(p))
                .transpose()?;
            let (extensions, brief_reads) = if *local_gws {
                let directory = owner
                    .verified_member_directory(&options.root)
                    .map_err(|_| "invalid_member_volume")?;
                let proxy = crate::gws_relay::start_member("/google/google.sock".into(), 0).await?;
                gws_setup::sync_from_installation(directory.clone());
                let (routes, reader) = crate::personal_google::local_routes_with_briefs(
                    store.clone(),
                    owner,
                    directory
                        .join(moosuri_store::personal_volume::GWS_DIRECTORY)
                        .join("gws-client.json"),
                    proxy,
                );
                (routes, Some(reader))
            } else if *google_readonly {
                (
                    crate::personal_google::routes(
                        store.clone(),
                        owner,
                        "/google/google.sock".into(),
                        0,
                    ),
                    None,
                )
            } else {
                (axum::Router::new(), None)
            };
            let extensions = if *local_gws {
                extensions
            } else {
                extensions.merge(crate::personal_google::mail_work::routes(
                    store.clone(),
                    owner,
                    None,
                    None,
                ))
            };
            let extensions =
                extensions.merge(moosuri_server::personal_tasks::routes(store.clone(), owner));
            let public_reads =
                public_watch::reader(browser_socket.clone(), browser_uid, owner.generation);
            let extensions = extensions.merge(browser::routes(
                store.clone(),
                owner,
                browser_socket,
                browser_uid,
                preview.clone(),
            ));
            return serve_member(
                socket,
                store,
                owner,
                preview,
                extensions,
                brief_reads,
                public_reads,
                vault_origin.as_deref(),
            )
            .await;
        }
        PersonalOperation::Initialize {
            name,
            email,
            password_stdin,
            member_chosen_password,
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
            if *member_chosen_password {
                owner
                    .initialize_member_password(
                        &options.root,
                        name.clone(),
                        email.clone(),
                        password,
                    )
                    .await
            } else {
                owner
                    .initialize(&options.root, name.clone(), email.clone(), password)
                    .await
            }
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
    preview: Option<moosuri_server::personal_text::Options>,
    extensions: axum::Router,
    brief_reads: Option<std::sync::Arc<dyn moosuri_server::personal_tasks::BriefReads>>,
    public_reads: std::sync::Arc<dyn moosuri_server::personal_tasks::PublicReads>,
    vault_origin: Option<&str>,
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
    // Inside Kiri /ipc is the member's bind mount under the image's root.
    // On a shared host it is the direct member subdirectory of the gateway's IPC root.
    let gateway_uid = std::fs::metadata(parent.parent().ok_or("invalid_ipc_root")?)
        .map_err(|_| "invalid_ipc_root")?
        .uid();
    let extensions = match vault_origin {
        Some(origin) => extensions.merge(moosuri_server::personal_tasks::member_vault::routes(
            store.clone(),
            owner,
            origin,
            gateway_uid,
        )?),
        None => extensions,
    };
    let extensions = extensions.merge(moosuri_server::personal_tasks::timer_routes_with_readers(
        store.clone(),
        owner,
        gateway_uid,
        brief_reads,
        Some(public_reads),
    ));
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
        moosuri_server::personal_service_router(
            store,
            preview.map(|options| (owner, options)),
            extensions,
        )
        .route("/health/ready", ready)
        .into_make_service_with_connect_info::<moosuri_server::personal_tasks::MemberPeer>(),
    )
    .await
    .map_err(|_| "personal_service_stopped".into())
}

fn read_preview(path: &std::path::Path) -> Result<moosuri_server::personal_text::Options, String> {
    use std::{
        io::Read,
        os::unix::fs::{MetadataExt, OpenOptionsExt},
    };
    let fixed = path == std::path::Path::new("/etc/moosuri/text-preview.json");
    if !path.is_absolute() || std::fs::canonicalize(path).ok().as_deref() != Some(path) {
        return Err("preview_config_invalid".into());
    }
    // The immutable image config and every ancestor must stay root-owned,
    // without a symlink or writable group/other component.
    if fixed {
        let mut parent = path.parent();
        while let Some(p) = parent {
            let m = std::fs::symlink_metadata(p).map_err(|_| "preview_config_invalid")?;
            if !m.is_dir() || m.uid() != 0 || m.mode() & 0o022 != 0 {
                return Err("preview_config_invalid".into());
            }
            parent = p.parent();
        }
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| "preview_config_unavailable")?;
    let m = file.metadata().map_err(|_| "preview_config_unavailable")?;
    let permissions = if fixed {
        m.uid() == 0 && m.mode() & 0o777 == 0o444
    } else {
        m.uid() == unsafe { libc::geteuid() } && m.mode() & 0o077 == 0
    };
    if !m.is_file() || !permissions || m.nlink() != 1 || m.len() > 8192 {
        return Err("preview_config_invalid".into());
    }
    let mut bytes = Vec::new();
    file.take(8193)
        .read_to_end(&mut bytes)
        .map_err(|_| "preview_config_unavailable")?;
    if bytes.len() > 8192 {
        return Err("preview_config_invalid".into());
    }
    #[derive(serde::Deserialize)]
    #[serde(deny_unknown_fields)]
    struct PreviewConfig {
        socket: std::path::PathBuf,
        broker_uid: u32,
        max_output_tokens: Option<u32>,
        pin: Option<moosuri_store::personal_text_preview::ModelPin>,
    }
    let config: PreviewConfig =
        serde_json::from_slice(&bytes).map_err(|_| "preview_config_invalid")?;
    // Existing images may include a legacy model pin. Retain its local output
    // bound, while obtaining the live model and effort from the host broker.
    let max_output_tokens = match (config.max_output_tokens, config.pin) {
        (Some(max), None) => max,
        (None, Some(pin)) => pin.max_output_tokens,
        _ => return Err("preview_config_invalid".into()),
    };
    if (fixed
        && (config.socket != std::path::Path::new("/model/model.sock") || config.broker_uid != 0))
        || !config.socket.is_absolute()
        || !(1..=2048).contains(&max_output_tokens)
    {
        return Err("preview_config_invalid".into());
    }
    Ok(moosuri_server::personal_text::Options {
        socket: config.socket,
        broker_uid: config.broker_uid,
        max_output_tokens,
    })
}
