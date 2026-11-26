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
mod account_runtime;
mod backup_live;
mod backup_snapshots;
mod browser_relay;
mod calls;
mod engine_login;
mod google_browser;
mod gws_inbox;
mod gws_relay;
mod import_private;
mod installation;
mod installation_admin;
mod kiri_invite;
mod kiri_chrome_signin;
mod kiri_lifecycle;
mod kiri_provision;
mod kiri_storage;
mod legacy_relations;
mod personal_gateway;
mod personal_google;
mod runtime_google;
mod runtime_model;
mod skills;
mod subscriptions;
mod work_sessions;
mod workstation_models;

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
// One startup value, never a collection or hot-path allocation. Keep clap's flat CLI.
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Allocate or inspect a personal runtime identity in a separate installation registry.
    AccountRuntime(account_runtime::Options),
    /// Local installation administrator bootstrap and status.
    InstallationAdmin(installation_admin::Options),
    /// Initialize or verify a single new member volume inside its own runtime.
    PersonalVolume(account_runtime::PersonalOptions),
    /// Serve the private Unix gateway for isolated member authentication.
    PersonalGateway(personal_gateway::Options),
    /// Reconcile a prepared member's Linux Kiri container.
    KiriProvision(kiri_provision::Options),
    /// Invite a new member through a private, recoverable Linux installer flow.
    KiriInvite(kiri_invite::Options),
    /// Prepare persistent member storage from a reserved Linux allocation.
    KiriStorage(kiri_storage::Options),
    /// Fulfill a web invitation with managed storage and restart recovery.
    KiriOnboard(kiri_lifecycle::onboard::Options),
    /// Prepare authorized web invitations within the installation's UID and disk budget.
    KiriOnboardWorker(kiri_lifecycle::worker::Options),
    /// Restore configured active member runtimes after host or engine restart.
    KiriLifecycle(kiri_lifecycle::Options),
    /// Serve an installation-owned, pinned model to allocated members over Unix sockets.
    RuntimeModel(runtime_model::Options),
    /// Serve a pinned login-backed model behind the installation engine (Linux service UID).
    EngineLoginWorker(engine_login::Options),
    /// Enroll the official Claude Code account for a host model worker.
    ClaudeEngineLogin(engine_login::claude::LoginOptions),
    /// Inspect an enrolled host-side ChatGPT engine connection.
    EngineChatgptInspect(engine_login::ChatGptInspectOptions),
    /// Make one text call using the member broker socket and private standard input.
    RuntimeModelCall(runtime_model::ClientOptions),
    /// Forward opaque TLS from an allocated member to allowed Google endpoints.
    RuntimeGwsRelay(gws_relay::Options),
    /// Serve isolated Chromium workspaces to enrolled member runtimes.
    RuntimeBrowserRelay(browser_relay::Options),
    /// Handle one owner-bound fresh Google sign-in from a private systemd socket.
    KiriChromeSignin(kiri_chrome_signin::Options),
    /// Serve one gateway-authorized Google browser attempt in a fresh Kiri capsule.
    RuntimeGoogleBrowser(google_browser::Options),
    /// Serve fixed read-only Google OAuth/Gmail egress to isolated members.
    RuntimeGoogle(runtime_google::Options),
    /// Read public Google broker configuration or build a consent URL.
    RuntimeGoogleCall(runtime_google::ClientOptions),
    /// Serve a bounded local Gmail INBOX metadata bridge using existing gws authentication.
    GwsInbox(gws_inbox::Options),
    /// Guided installation (English / 한국어), or resume an existing setup.
    Setup(installation::setup::Options),
    /// Clean Ubuntu 24.04 amd64 installation of member Kiri, gateway, vault and Google transport.
    SetupKiri(installation::kiri::Options),
    /// Show the desktop menu. Closing it leaves the server running.
    Desktop(installation::setup::Location),
    /// Enable or disable server startup at login without stopping the server.
    Autostart(installation::setup::desktop::Autostart),
    /// Connect and inspect private Tailscale HTTPS access.
    Tailscale(installation::setup::tailscale::Options),
    /// Connect a public domain with optional automatic DNS and renewable HTTPS.
    Public(installation::setup::public_access::Options),
    /// Start your installed assistant; --foreground runs in this terminal.
    Start(installation::setup::Start),
    /// Stop only the installed assistant's user service.
    Stop(installation::setup::Location),
    /// Restart the installed user service, preserving login-autostart.
    Restart(installation::setup::Location),
    /// Show installation and service readiness.
    Status(installation::setup::Location),
    /// Open your installed assistant in the browser.
    Open(installation::setup::Open),
    /// Remove managed runtime and service; always preserve private data.
    Uninstall(installation::setup::Location),
    /// Import an explicitly selected private normalized export into a NEW staging data root.
    ImportPrivate(import_private::Command),
    /// Import a complete private PostgreSQL schema23 relation snapshot into a NEW root.
    ImportLegacyRelations(legacy_relations::Command),
    /// Allocate new forward requests after historical recovery; old usage stays unknown.
    RecoveryBudget {
        #[command(subcommand)]
        command: RecoveryBudgetCommand,
    },
    /// Opt-in local user-request evidence to a dedicated custom source connection.
    WorkSessions {
        #[command(subcommand)]
        command: work_sessions::Command,
    },
    /// Explicit normalized call transcript/audio feed; no discovery or background worker.
    Calls {
        #[command(subcommand)]
        command: calls::Command,
    },
    /// Verify/install/manage a local signed native bundle. No system services are changed.
    Installation {
        #[command(subcommand)]
        command: installation::Command,
    },
    /// Discover and temporarily connect models running on this workstation.
    Models {
        #[command(subcommand)]
        command: workstation_models::Command,
    },
    /// Personal model sign-in on this computer.
    Subscriptions {
        #[command(subcommand)]
        command: subscriptions::Command,
    },
    #[command(hide = true, name = "internal-pdf-text")]
    InternalPdfText,
    #[command(hide = true, name = "internal-claude-run")]
    InternalClaudeRun {
        mode: String,
        model: Option<String>,
        session: Option<String>,
    },
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
        /// Private managed public-HTTPS receipt; ordinary access still requires app login.
        #[arg(long, conflicts_with_all = ["dev", "trusted_proxy"])]
        public_https_config: Option<PathBuf>,
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
        /// Owner-only per-account metadata-only GWS inbox helper registry JSON.
        #[arg(long)]
        gws_inbox_file: Option<PathBuf>,
        /// Explicit native gws binary for system invitations; no ambient authentication.
        #[arg(long,requires_all=["invitation_gws_credentials","invitation_gws_config","invitation_from"])]
        invitation_gws: Option<PathBuf>,
        /// Dedicated operator-owned0600 gws credentials; never a credential value.
        #[arg(long, requires = "invitation_gws")]
        invitation_gws_credentials: Option<PathBuf>,
        /// Dedicated existing0700 gws directory; current user's default is never reused.
        #[arg(long, requires = "invitation_gws")]
        invitation_gws_config: Option<PathBuf>,
        #[arg(long, requires = "invitation_gws")]
        invitation_from: Option<String>,
        /// Existing private directory for optional live encrypted full-data snapshots.
        #[arg(long)]
        backup_directory: Option<PathBuf>,
        /// Seconds after each snapshot finishes (300..604800; default3600).
        #[arg(long, requires = "backup_directory")]
        backup_interval_seconds: Option<u64>,
        /// Retained baseline export for optional periodic recovery publication (30s cadence).
        #[arg(long,requires_all=["recovery_ssh_target","recovery_target_id"])]
        recovery_export: Option<PathBuf>,
        /// Existing OpenSSH alias with pinned host key and forced receiver command.
        #[arg(long, requires = "recovery_export")]
        recovery_ssh_target: Option<String>,
        #[arg(long, requires = "recovery_export")]
        recovery_target_id: Option<String>,
        /// Optional owner-only Brave Search key file; public HTTP reading needs no key.
        #[arg(long)]
        web_search_key_file: Option<PathBuf>,
        /// Owner-only JSON for an already verified isolated Linux browser deployment.
        #[arg(long)]
        browser_capsule_file: Option<PathBuf>,
        /// Optional verified rootless Linux official Claude Code execution profile.
        #[arg(long)]
        claude_profile_file: Option<PathBuf>,
        /// Public GitHub repository hosting contributor-generated static catalog files.
        #[arg(long,requires_all=["community_catalog_ref","community_catalog_directory"])]
        community_catalog_repository: Option<String>,
        #[arg(long, requires = "community_catalog_repository")]
        community_catalog_ref: Option<String>,
        #[arg(long, requires = "community_catalog_repository")]
        community_catalog_directory: Option<String>,
        /// Separate installed vault browser origin; no public default is assumed.
        #[arg(long)]
        vault_origin: Option<String>,
        /// Loopback listener reserved exclusively for the authenticated ciphertext service.
        #[arg(long, default_value = "127.0.0.1:3211")]
        vault_adapter_bind: SocketAddr,
        /// Separate owner-only copy of a random256-bit hex deployment adapter key.
        #[arg(long)]
        vault_adapter_key_file: Option<PathBuf>,
    },
    /// Check a stopped installation without printing account or secret data.
    Doctor,
    /// Print public TypeScript DTOs, generated from Rust.
    ExportTypes,
}

#[derive(Subcommand)]
enum RecoveryBudgetCommand {
    Status,
    Allocate {
        #[arg(long)]
        restore_id: String,
        #[arg(long)]
        request_id: String,
        #[arg(long)]
        calls: u32,
        #[arg(long, default_value_t = 0)]
        audio_seconds: u32,
    },
}
#[derive(Subcommand)]
enum BackupCommand {
    /// Initialize an independent receiver from an already copied encrypted baseline.
    TargetInit {
        #[arg(long)]
        target_id: String,
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long)]
        export: PathBuf,
        #[arg(long)]
        installation_id: String,
    },
    /// Read the receiver's independently retained current receipt and recovery paths.
    TargetStatus {
        #[arg(long)]
        target_id: String,
    },
    /// Fixed SSH receiver command. Reads bounded opaque journal framing on stdin.
    ReceiveJournal {
        #[arg(long)]
        target_id: String,
    },
    /// Publish encrypted complete journals for one retained export, without sealing the stopped source.
    Journal {
        #[arg(long)]
        export: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        head_output: PathBuf,
    },
    /// Restore an exact historical export with a separately pinned latest journal head.
    RestoreHistorical {
        #[arg(long)]
        export: PathBuf,
        #[arg(long)]
        journal: PathBuf,
        #[arg(long)]
        trusted_head: PathBuf,
        #[arg(long)]
        latest_head_sha256: String,
        #[arg(long)]
        export_identity_file: PathBuf,
        #[arg(long)]
        journal_identity_file: PathBuf,
    },
    /// Restore one matching sealed checkpoint into a new --data-dir. Requires independent trust in the head.
    Restore {
        #[arg(long)]
        checkpoint: PathBuf,
        #[arg(long)]
        trusted_head: PathBuf,
        #[arg(long)]
        identity_file: PathBuf,
    },
    /// Inspect the local cutover phase without opening runtime or verifying transfer.
    CheckpointStatus,
    /// Permanently retire this stopped source and create a local cutover checkpoint.
    Checkpoint {
        #[arg(long, required=true, action=clap::ArgAction::SetTrue)]
        seal_installation: bool,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        head_output: PathBuf,
    },
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

fn main() {
    let cli = Cli::parse();
    if matches!(cli.command, Command::InternalPdfText) {
        std::process::exit(moosuri_documents::child_main());
    }
    if let Command::Desktop(location) = &cli.command {
        if let Err(code) = installation::setup::desktop::run(location) {
            eprintln!("moosuri: {code}");
            std::process::exit(1);
        }
        return;
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("moosuri: runtime_unavailable");
            std::process::exit(1);
        }
    };
    if let Err(code) = runtime.block_on(run(cli)) {
        eprintln!("moosuri: {code}");
        std::process::exit(1);
    }
}
async fn run(cli: Cli) -> Result<(), String> {
    use installation::setup;
    match &cli.command {
        Command::Tailscale(options) => return setup::tailscale::run(options).await,
        Command::Public(options) => return setup::public_access::run(options).await,
        Command::Autostart(options) => return setup::desktop::autostart(options),
        Command::Desktop(_) => unreachable!(),
        Command::Setup(options) => return setup::run(options, cli.data_dir.as_deref()).await,
        Command::SetupKiri(options) => return installation::kiri::run(options).await,
        Command::Start(options) => return setup::start(options).await,
        Command::Stop(location) => return setup::stop(location),
        Command::Restart(location) => return setup::restart(location).await,
        Command::Status(location) => return setup::status(location).await,
        Command::Open(location) => return setup::open(location).await,
        Command::Uninstall(location) => return setup::uninstall(location).await,
        _ => (),
    }
    if let Command::InternalClaudeRun {
        mode,
        model,
        session,
    } = &cli.command
    {
        return moosuri_connectors::claude_code::helper_main(
            mode,
            model.as_deref(),
            session.as_deref(),
        )
        .await
        .map_err(|_| "claude_execution_failed".into());
    }

    if let Command::KiriChromeSignin(options) = &cli.command {
        return kiri_chrome_signin::serve(options).await;
    }
    if let Command::RuntimeGoogleBrowser(options) = &cli.command {
        return google_browser::serve(options).await;
    }
    if let Command::RuntimeBrowserRelay(options) = &cli.command {
        return browser_relay::run(options).await;
    }
    if let Command::RuntimeGwsRelay(options) = &cli.command {
        return gws_relay::run(options).await;
    }
    if let Command::RuntimeGoogle(options) = &cli.command {
        return runtime_google::run(options).await;
    }
    if let Command::RuntimeGoogleCall(options) = &cli.command {
        return runtime_google::run_client(options).await;
    }
    if let Command::EngineLoginWorker(options) = &cli.command {
        return engine_login::run(options).await;
    }
    if let Command::EngineChatgptInspect(options) = &cli.command {
        return engine_login::inspect_chatgpt(options).await;
    }
    if let Command::ClaudeEngineLogin(options) = &cli.command {
        return engine_login::claude::login(options).await;
    }
    if let Command::RuntimeModel(options) = &cli.command {
        return runtime_model::run(options).await;
    }
    if let Command::RuntimeModelCall(options) = &cli.command {
        return runtime_model::run_client(options).await;
    }
    if let Command::PersonalGateway(options) = &cli.command {
        return personal_gateway::run(options).await;
    }
    if let Command::KiriLifecycle(options) = &cli.command {
        return kiri_lifecycle::run(options).await;
    }
    if let Command::KiriStorage(o) = &cli.command {
        return kiri_storage::run(o).await;
    }
    if let Command::KiriOnboardWorker(o) = &cli.command {
        return kiri_lifecycle::worker::run(o).await;
    }
    if let Command::KiriOnboard(o) = &cli.command {
        return kiri_lifecycle::onboard::run(o).await;
    }
    if let Command::KiriInvite(options) = &cli.command {
        return kiri_invite::run(options).await;
    }
    if let Command::KiriProvision(options) = &cli.command {
        return kiri_provision::run(options).await;
    }
    if let Command::PersonalVolume(options) = &cli.command {
        return account_runtime::personal(options).await;
    }
    if let Command::InstallationAdmin(options) = &cli.command {
        return installation_admin::run(options).await;
    }
    if let Command::AccountRuntime(options) = &cli.command {
        return account_runtime::run(options).await;
    }
    if let Command::GwsInbox(options) = &cli.command {
        return gws_inbox::run(options).await;
    }
    if let Command::Models { command } = &cli.command {
        return workstation_models::run(command).await;
    }
    if let Command::Calls { command } = &cli.command {
        return calls::run(command).await;
    }
    if let Command::WorkSessions { command } = &cli.command {
        return work_sessions::run(command).await;
    }
    if let Command::Installation { command } = &cli.command {
        return installation::run(command);
    }
    if let Command::Subscriptions { command } = &cli.command {
        return subscriptions::run(command).await;
    }
    if let Command::Skills { command } = &cli.command {
        return skills::run(command).await;
    }
    if let Command::Backup {
        command: BackupCommand::Keygen { output },
    } = &cli.command
    {
        let recipient = moosuri_store::backup::keygen(output).map_err(|e| e.to_string())?;
        println!("{recipient}");
        return Ok(());
    }
    if let Command::Backup { command } = &cli.command {
        match command {
            BackupCommand::TargetInit {
                target_id,
                artifacts,
                ledger,
                export,
                installation_id,
            } => {
                let root = backup_live::target_root(target_id, true)?;
                moosuri_store::backup::live::init_target(
                    &root,
                    artifacts,
                    ledger,
                    export,
                    installation_id,
                )
                .map_err(|e| e.to_string())?;
                println!(
                    "Recovery receiver initialized. Retain independent access to its ledger and the matching offline age identities. Local directory setup alone does not prove off-host storage."
                );
                return Ok(());
            }
            BackupCommand::TargetStatus { target_id } => {
                let root = backup_live::target_root(target_id, false)?;
                let status =
                    moosuri_store::backup::live::target_status(&root).map_err(|e| e.to_string())?;
                println!(
                    "{}",
                    serde_json::to_string(&status).map_err(|_| "recovery_status_failed")?
                );
                return Ok(());
            }
            BackupCommand::ReceiveJournal { target_id } => {
                let root = backup_live::target_root(target_id, false)?;
                let ack = backup_live::receive_command(&root)?;
                println!(
                    "{}",
                    serde_json::to_string(&ack).map_err(|_| "recovery_ack_failed")?
                );
                return Ok(());
            }
            _ => (),
        }
    }
    if matches!(cli.command, Command::ExportTypes) {
        print!("{}", moosuri_core::typescript_contract());
        return Ok(());
    }
    let _managed_lock = installation::guard_managed_executable()?;
    let data = cli
        .data_dir
        .or_else(default_data_dir)
        .ok_or("data_directory_required")?;
    if !data.is_absolute() {
        return Err("data_directory_must_be_absolute".into());
    }
    match cli.command {
        Command::ImportPrivate(command) => import_private::run(&data, command).await?,
        Command::ImportLegacyRelations(command) => legacy_relations::run(&data, command).await?,
        Command::RecoveryBudget { command } => {
            let status = match command {
                RecoveryBudgetCommand::Status => {
                    moosuri_store::recovery_budget::status(&data).await
                }
                RecoveryBudgetCommand::Allocate {
                    restore_id,
                    request_id,
                    calls,
                    audio_seconds,
                } => {
                    moosuri_store::recovery_budget::allocate(
                        &data,
                        &restore_id,
                        &request_id,
                        i64::from(calls),
                        i64::from(audio_seconds) * 1000,
                    )
                    .await
                }
            }
            .map_err(|e| e.to_string())?;
            println!(
                "{}",
                serde_json::to_string_pretty(&status)
                    .map_err(|_| "recovery_budget_status_failed")?
            );
            println!(
                "Historical usage remains unknown. This allowance limits new inference request count and validated STT input duration; it is not a token or financial ceiling. Unknown outcomes consume allowance without refund."
            );
        }
        Command::Public(_)
        | Command::Tailscale(_)
        | Command::Desktop(_)
        | Command::Autostart(_)
        | Command::Setup(_)
        | Command::SetupKiri(_)
        | Command::Start(_)
        | Command::Stop(_)
        | Command::Restart(_)
        | Command::Status(_)
        | Command::Open(_)
        | Command::Uninstall(_)
        | Command::Installation { .. } => {
            unreachable!("installation runs without ordinary data admission")
        }
        Command::InternalPdfText => unreachable!("PDF extraction does not open installation state"),
        Command::InternalClaudeRun { .. } => {
            unreachable!("container helper does not open installation state")
        }
        Command::KiriChromeSignin(_)
        | Command::RuntimeGoogleBrowser(_)
        | Command::RuntimeBrowserRelay(_)
        | Command::RuntimeGwsRelay(_)
        | Command::RuntimeGoogle(_)
        | Command::RuntimeGoogleCall(_)
        | Command::EngineLoginWorker(_)
        | Command::ClaudeEngineLogin(_)
        | Command::EngineChatgptInspect(_)
        | Command::RuntimeModel(_)
        | Command::RuntimeModelCall(_)
        | Command::PersonalGateway(_)
        | Command::KiriLifecycle(_)
        | Command::KiriInvite(_)
        | Command::KiriStorage(_)
        | Command::KiriOnboardWorker(_)
        | Command::KiriOnboard(_)
        | Command::KiriProvision(_)
        | Command::PersonalVolume(_)
        | Command::InstallationAdmin(_)
        | Command::AccountRuntime(_)
        | Command::GwsInbox(_)
        | Command::Models { .. }
        | Command::Skills { .. }
        | Command::Subscriptions { .. }
        | Command::WorkSessions { .. }
        | Command::Calls { .. } => {
            unreachable!("native helpers run without installation state")
        }
        Command::Backup { command } => match command {
            BackupCommand::TargetInit { .. }
            | BackupCommand::TargetStatus { .. }
            | BackupCommand::ReceiveJournal { .. } => {
                unreachable!("receiver commands do not admit source installation")
            }
            BackupCommand::Keygen { .. } => unreachable!("keygen runs without installation state"),
            BackupCommand::Restore {
                checkpoint,
                trusted_head,
                identity_file,
            } => {
                let result = moosuri_store::cutover::restore(
                    &checkpoint,
                    &trusted_head,
                    &identity_file,
                    &data,
                )
                .map_err(|e| e.to_string())?;
                if result.already_published {
                    println!(
                        "Matching restore already published as installation {} from checkpoint {}. This retry preserved current destination enrollment and work state; source stays sealed.",
                        result.installation_id, result.checkpoint_id
                    );
                } else {
                    println!(
                        "Restored installation {} from checkpoint {}. All accounts require fresh local enrollment; source stays sealed. Provider/mail/source connections require re-enrollment; schedules and background grants stay quarantined. Configure backup setup again for this new installation.",
                        result.installation_id, result.checkpoint_id
                    );
                }
                for account in result.accounts {
                    println!(
                        "For account {} still requiring enrollment: recover-account --account-id {} --role {} --password-file <private-new-password-file> (status: {}). Suspended accounts stay suspended.",
                        account.account_id, account.account_id, account.role, account.status
                    );
                }
            }
            BackupCommand::Journal {
                export,
                output,
                head_output,
            } => {
                let result =
                    moosuri_store::backup::publish_journal(&data, &export, &output, &head_output)
                        .map_err(|e| e.to_string())?;
                println!(
                    "Manual journal checkpoint {} locally complete. Independently retain latest head SHA256 {}. Source remains active. No off-host transfer, unattended publication or independent freshness verification was performed.",
                    result.checkpoint_id, result.trusted_head_sha256
                );
            }
            BackupCommand::RestoreHistorical {
                export,
                journal,
                trusted_head,
                latest_head_sha256,
                export_identity_file,
                journal_identity_file,
            } => {
                let result = moosuri_store::backup::restore_historical(
                    &export,
                    &journal,
                    &trusted_head,
                    &latest_head_sha256,
                    &export_identity_file,
                    &journal_identity_file,
                    &data,
                )
                .map_err(|e| e.to_string())?;
                println!(
                    "Historical export recovery installation {} through journal checkpoint {} (already published: {}). Accounting is accounting_unknown: all model dispatch, probes and current usage totals remain unavailable until an explicit allocation/reconcile flow is implemented. This is data recovery only. Connections and old work remain quarantined. Source was not sealed; prevent simultaneous operation before enrolling this destination.",
                    result.installation_id, result.checkpoint_id, result.already_published
                );
                for account in result.accounts {
                    println!(
                        "For currently eligible account {} (snapshot status: {}), recover-account --account-id {} --role <explicit-current-admin-or-member> --password-file <private-new-password-file>. Latest role and suspension are not reconstructed; suspended snapshot accounts stay suspended.",
                        account.account_id, account.status, account.account_id
                    );
                }
            }
            BackupCommand::CheckpointStatus => {
                let s = moosuri_store::cutover::status(&data).map_err(|e| e.to_string())?;
                println!(
                    "Local cutover phase: {}. Seal: {}. Checkpoint: {}. This does not verify output transfer, restore readiness or independent freshness.",
                    s.phase,
                    s.seal_id.as_deref().unwrap_or("none"),
                    s.checkpoint_id.as_deref().unwrap_or("none")
                );
            }
            BackupCommand::Checkpoint {
                seal_installation,
                output,
                head_output,
            } => {
                if !seal_installation {
                    return Err("seal_installation_flag_required".into());
                }
                let result=moosuri_store::cutover::checkpoint(&data,&output,&head_output).map_err(|e|format!("{e}; any adopted seal remains permanent. Retry the same checkpoint; there is no unseal; matching backup restore requires the trusted head and recovery identity."))?;
                println!(
                    "Source permanently sealed: {}. Checkpoint {} locally complete (SHA256 {}). Carry its trusted head and recovery identity through an independent channel. No off-host transfer, restore or ongoing disaster protection was performed.",
                    result.seal_id, result.checkpoint_id, result.ciphertext_sha256
                );
            }
            BackupCommand::Setup {
                recipient,
                identity_file,
            } => {
                let configured = moosuri_store::backup::setup(&data, &recipient, &identity_file)
                    .map_err(|e| e.to_string())?;
                println!(
                    "Installation {}: encrypted export configured at key epoch {}. Retain all required private identities outside the installation and export directory. This is not an off-host restore test.",
                    configured.installation_id,
                    configured.active_epoch.unwrap_or(0)
                );
            }
            BackupCommand::Create { output } => {
                let export =
                    moosuri_store::backup::create(&data, &output).map_err(|e| e.to_string())?;
                println!(
                    "Encrypted export {} complete at key epoch {}. Historical restore requires a separately published journal and an independently retained latest head digest.",
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
            public_https_config,
            bind,
            origin,
            dev,
            trusted_proxy,
            client_ip_header,
            google_client_file,
            gws_inbox_file,
            invitation_gws,
            invitation_gws_credentials,
            invitation_gws_config,
            invitation_from,
            backup_directory,
            backup_interval_seconds,
            recovery_export,
            recovery_ssh_target,
            recovery_target_id,
            web_search_key_file,
            browser_capsule_file,
            claude_profile_file,
            community_catalog_repository,
            community_catalog_ref,
            community_catalog_directory,
            vault_origin,
            vault_adapter_bind,
            vault_adapter_key_file,
        } => {
            if !bind.ip().is_loopback() {
                return Err("loopback_bind_required_use_a_tls_proxy".into());
            }
            let public_tls = public_https_config
                .as_deref()
                .map(|p| installation::setup::public_access::tls::Prepared::load(p, &origin, &data))
                .transpose()?;
            let mut config = moosuri_server::ServerConfig::new(&origin, dev)
                .map_err(str::to_owned)?
                .with_pdf_extractor(
                    std::env::current_exe().map_err(|_| "pdf_extractor_unavailable")?,
                )
                .map_err(str::to_owned)?;
            if let Some(managed) = &_managed_lock {
                config = config
                    .with_runtime_identity(managed.identity_for(&data)?)
                    .map_err(str::to_owned)?;
            }
            if let (Some(repository), Some(reference), Some(directory)) = (
                community_catalog_repository,
                community_catalog_ref,
                community_catalog_directory,
            ) {
                config = config
                    .with_community_catalog(&repository, &reference, &directory)
                    .map_err(str::to_owned)?;
            }
            if let Some(proxy) = trusted_proxy {
                config = config
                    .with_trusted_proxy(proxy, &client_ip_header)
                    .map_err(str::to_owned)?;
            }
            if let (Some(executable), Some(credentials), Some(directory), Some(from)) = (
                invitation_gws,
                invitation_gws_credentials,
                invitation_gws_config,
                invitation_from,
            ) {
                config = config
                    .with_invitation_sender(executable, credentials, directory, from)
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
            if let Some(path) = gws_inbox_file {
                config = config.with_gws_inbox_file(path).map_err(str::to_owned)?;
            }
            if let Some(path) = claude_profile_file {
                let json = read_browser_capsule_file(&path)?;
                config = config
                    .with_claude_profile_json(&json)
                    .map_err(str::to_owned)?;
            }
            if let Some(path) = browser_capsule_file {
                config = config
                    .with_browser_capsule_json(&read_browser_capsule_file(&path)?)
                    .map_err(str::to_owned)?;
            }
            let vault_key = match (&vault_origin, vault_adapter_key_file) {
                (Some(origin), Some(path)) => {
                    if !vault_adapter_bind.ip().is_loopback() {
                        return Err("vault_adapter_loopback_required".into());
                    }
                    config = config
                        .with_vault_origin(origin, dev)
                        .map_err(str::to_owned)?;
                    Some(read_password_file(&path)?)
                }
                (None, None) => None,
                _ => return Err("vault_origin_and_private_adapter_key_required".into()),
            };
            if !ASSETS.iter().any(|(name, _)| *name == "/index.html") {
                return Err(
                    "web_assets_missing_run_npm_ci_and_build_in_web_then_rebuild_rust".into(),
                );
            }
            let store = config.open_store(&data).map_err(|e| e.to_string())?;
            let snapshot_worker = if let Some(directory) = backup_directory {
                Some(
                    backup_snapshots::Worker::start(
                        store.clone(),
                        &data,
                        &directory,
                        backup_interval_seconds.unwrap_or(3600),
                    )
                    .await?,
                )
            } else {
                None
            };
            let recovery_worker = if let (Some(export), Some(alias), Some(id)) =
                (recovery_export, recovery_ssh_target, recovery_target_id)
            {
                Some(backup_live::Worker::start(
                    store.clone(),
                    &export,
                    alias,
                    id,
                )?)
            } else {
                None
            };
            let runtime = config.start_runtime(store.clone());
            let adapter_router = vault_key
                .as_ref()
                .map(|key| moosuri_server::vault::adapter_router(store.clone(), key))
                .transpose()
                .map_err(str::to_owned)?;
            let vault_form_origin = vault_origin.as_ref().map(|v| {
                url::Url::parse(v)
                    .expect("validated vault origin")
                    .origin()
                    .ascii_serialization()
            });
            let app = moosuri_server::router_with_runtime(store, config, runtime.clone()).fallback(
                move |uri: Uri| {
                    let vault_origin = vault_form_origin.clone();
                    async move {
                        let mut response = static_asset(uri).await;
                        allow_vault_form(&mut response, vault_origin.as_deref());
                        response
                    }
                },
            );
            let listener = tokio::net::TcpListener::bind(bind)
                .await
                .map_err(|_| "listen_failed")?;
            let adapter_task = if let Some(router) = adapter_router {
                let listener = tokio::net::TcpListener::bind(vault_adapter_bind)
                    .await
                    .map_err(|_| "vault_adapter_listen_failed")?;
                Some(tokio::spawn(async move {
                    let _ = axum::serve(listener, router).await;
                }))
            } else {
                None
            };
            println!(
                "moosuri {} listening on {}",
                env!("CARGO_PKG_VERSION"),
                listener.local_addr().map_err(|_| "listen_failed")?
            );
            let public_worker = public_tls.map(|tls| tls.start(app.clone()));
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
                _ = async {
                    loop {
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        if public_worker.as_ref().is_some_and(|w| !w.healthy()) { break; }
                    }
                } => {
                    runtime.stop();
                    let _ = drain.send(());
                    runtime.shutdown().await;
                    Err("public_https_worker_stopped")
                },
                _ = shutdown_signal() => {
                    if let Some(worker) = &public_worker { worker.stop(); }
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
            if let Some(worker) = public_worker {
                worker.shutdown().await;
            }
            if let Some(worker) = snapshot_worker {
                worker.shutdown().await;
            }
            if let Some(worker) = recovery_worker {
                worker.shutdown().await;
            }
            if let Some(task) = adapter_task {
                task.abort();
            }
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
                "SQLite: ready\nPrivate writer lock: acquired\nAdministrator initialized: {ready}\nText chat: available with a verified connection\nStopped encrypted export: available after setup\nSealed cutover restore: available with separately trusted head; ordinary historical exports unsupported"
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
fn allow_vault_form(response: &mut Response, vault_origin: Option<&str>) {
    if let Some(origin) = vault_origin
        && let Some(policy) = response.headers().get(header::CONTENT_SECURITY_POLICY)
            .and_then(|v| v.to_str().ok()).map(str::to_owned)
    {
        // Preserve the assistant Origin on the vault's cross-origin handoff POST.
        response.headers_mut().insert(header::REFERRER_POLICY, header::HeaderValue::from_static("strict-origin"));
        let policy = policy.replace("form-action 'self'", &format!("form-action 'self' {origin}"));
        response.headers_mut().insert(header::CONTENT_SECURITY_POLICY, policy.parse().expect("validated vault origin"));
    }
}

async fn static_asset(uri: Uri) -> Response {
    let path = match uri.path() {
        "/" | "/app" | "/admin" | "/admin/providers" | "/admin/usage" | "/admin/google"
        | "/admin/telegram" | "/settings/model" | "/settings/calendar" | "/settings/mail"
        | "/settings/sources" | "/settings/usage" | "/settings/telegram" | "/app/knowledge"
        | "/app/memory" | "/app/skills" | "/app/artifacts" | "/app/schedules" | "/app/missions"
        | "/app/feed" | "/app/goals" | "/app/library" | "/login" => "/index.html",
        path if ["/app/artifacts/", "/app/goals/"].iter().any(|prefix| {
            path.strip_prefix(prefix)
                .is_some_and(|id| !id.is_empty() && !id.contains('/'))
        }) =>
        {
            "/index.html"
        }
        "/demo" => "/demo.html",
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
    h.insert(header::CONTENT_SECURITY_POLICY, "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; media-src 'self' blob:; worker-src 'self'; connect-src 'self'; font-src 'self'; object-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}

// Account fixtures share the process-wide two-slot password hashing limit.
#[cfg(test)]
static ACCOUNT_FIXTURE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

#[cfg(test)]
mod schedule_route_tests {
    use super::*;
    #[tokio::test]
    async fn private_page_deep_links_serve_only_public_shell_without_caching_user_data() {
        for (page, api) in [
            ("/app/schedules", "/api/v1/notifications"),
            ("/app/missions", "/api/v1/missions"),
        ] {
            let response = static_asset(page.parse().unwrap()).await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(
                response.headers()[header::CONTENT_TYPE],
                "text/html; charset=utf-8"
            );
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
            assert_eq!(
                static_asset(api.parse().unwrap()).await.status(),
                StatusCode::NOT_FOUND
            );
        }
    }
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
    #[tokio::test]
    async fn settings_shell_navigation_returns_the_public_app_entry() {
        for path in [
            "/settings/calendar",
            "/admin/telegram",
            "/settings/telegram",
        ] {
            let response = static_asset(Uri::from_static(path)).await;
            assert_eq!(response.status(), axum::http::StatusCode::OK, "{path}");
            assert!(
                response.headers()[axum::http::header::CONTENT_TYPE]
                    .to_str()
                    .unwrap()
                    .starts_with("text/html")
            );
        }
        assert_eq!(
            static_asset(Uri::from_static("/settings/not-a-page"))
                .await
                .status(),
            axum::http::StatusCode::NOT_FOUND
        );
    }
}
