use clap::{Parser, Subcommand};
use moosuri_vault_service::{
    database::Database,
    http::{Adapter, Config, router_shared},
};
use std::{
    io::Read,
    net::SocketAddr,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::{Arc, Mutex},
};
#[derive(Parser)]
#[command(about = "Separate-origin client-encrypted vault; never accepts unlock secrets")]
struct Args {
    #[arg(long)]
    data_dir: Option<PathBuf>,
    #[arg(long, default_value = "127.0.0.1:3220")]
    bind: SocketAddr,
    #[arg(long)]
    origin: Option<String>,
    #[arg(long)]
    assistant_origin: Option<String>,
    #[arg(long, default_value = "http://127.0.0.1:3211/internal/v1/vault")]
    adapter_url: String,
    #[arg(long)]
    adapter_key_file: Option<PathBuf>,
    #[arg(long)]
    dev: bool,
    /// Existing private output directory for opt-in non-sealing encrypted snapshots.
    #[arg(long, requires = "backup_recipient")]
    backup_directory: Option<PathBuf>,
    #[arg(long, requires = "backup_directory")]
    backup_recipient: Option<String>,
    /// Delay after each completed snapshot (300..604800 seconds; default3600).
    #[arg(long, requires = "backup_directory")]
    backup_interval_seconds: Option<u64>,
    /// Opt-in independent full journal publication (all companion flags required).
    #[arg(long, requires_all = ["journal_recipient", "journal_snapshot_head", "journal_ssh_target", "journal_target_id"])]
    journal_directory: Option<PathBuf>,
    #[arg(long, requires = "journal_directory")]
    journal_recipient: Option<String>,
    #[arg(long, requires = "journal_directory")]
    journal_snapshot_head: Option<PathBuf>,
    #[arg(long, requires = "journal_directory")]
    journal_ssh_target: Option<String>,
    #[arg(long, requires = "journal_directory")]
    journal_target_id: Option<String>,
    #[arg(long, requires = "journal_directory")]
    journal_interval_seconds: Option<u64>,
    #[command(subcommand)]
    command: Option<Command>,
}
#[derive(Subcommand)]
enum Command {
    /// Offline owner-only sealed checkpoint and verified restore.
    Recovery {
        #[command(subcommand)]
        command: Recovery,
    },
}
#[derive(Subcommand)]
enum Recovery {
    /// Initialize a new independent receiver from an explicitly selected copied snapshot.
    TargetInit {
        #[arg(long)]
        target_id: String,
        #[arg(long)]
        artifacts: PathBuf,
        #[arg(long)]
        ledger: PathBuf,
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        snapshot_head: PathBuf,
        #[arg(long)]
        recipient: String,
    },
    /// Fixed SSH forced-command admission. Not a web route or arbitrary command.
    ReceiveJournal {
        #[arg(long)]
        target_id: String,
    },
    /// Query the independently retained exact historical recovery pin.
    TargetStatus {
        #[arg(long)]
        target_id: String,
    },
    /// Export a non-sealing full journal checkpoint under stopped local admission.
    JournalCheckpoint {
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        trusted_head: PathBuf,
    },
    /// Recover an older live snapshot through an explicitly pinned newer full journal.
    RestoreHistorical {
        #[arg(long)]
        snapshot: PathBuf,
        #[arg(long)]
        snapshot_head: PathBuf,
        #[arg(long)]
        snapshot_identity_file: PathBuf,
        #[arg(long)]
        journal: PathBuf,
        #[arg(long)]
        journal_head: PathBuf,
        #[arg(long)]
        journal_head_sha256: String,
        #[arg(long)]
        journal_identity_file: PathBuf,
    },
    Checkpoint {
        #[arg(long)]
        recipient: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        trusted_head: PathBuf,
        #[arg(long)]
        seal_source: bool,
    },
    Restore {
        #[arg(long)]
        checkpoint: PathBuf,
        #[arg(long)]
        trusted_head: PathBuf,
        #[arg(long)]
        identity_file: PathBuf,
    },
    /// Restore an explicitly selected point-in-time live snapshot, not independently latest history.
    RestoreSnapshot {
        #[arg(long)]
        checkpoint: PathBuf,
        #[arg(long)]
        trusted_head: PathBuf,
        #[arg(long)]
        identity_file: PathBuf,
    },
}
fn private_key(path: &std::path::Path) -> Result<String, &'static str> {
    let m = std::fs::symlink_metadata(path).map_err(|_| "vault_key_file_unavailable")?;
    // SAFETY: geteuid has no preconditions and returns only the current process identity.
    let uid = unsafe { libc::geteuid() };
    if m.uid() != uid
        || !m.is_file()
        || m.file_type().is_symlink()
        || m.permissions().mode() & 0o077 != 0
        || m.len() > 128
    {
        return Err("vault_private_key_file_required");
    }
    let mut f = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
        .map_err(|_| "vault_key_file_unavailable")?;
    if f.metadata()
        .map_err(|_| "vault_key_file_unavailable")?
        .ino()
        != m.ino()
    {
        return Err("vault_key_file_changed");
    }
    let mut value = zeroize::Zeroizing::new(String::new());
    f.read_to_string(&mut value)
        .map_err(|_| "vault_key_file_unavailable")?;
    Ok(value.trim_end_matches(['\r', '\n']).into())
}
#[tokio::main]
async fn main() {
    if run().await.is_err() {
        eprintln!("vault_service_failed");
        std::process::exit(1)
    }
}
async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    if let Some(Command::Recovery { command }) = args.command {
        match command {
            Recovery::TargetInit {
                target_id,
                artifacts,
                ledger,
                snapshot,
                snapshot_head,
                recipient,
            } => {
                let root =
                    moosuri_vault_service::recovery::publication::target_root(&target_id, true)?;
                moosuri_vault_service::recovery::publication::init_target(
                    &root,
                    &artifacts,
                    &ledger,
                    &snapshot,
                    &snapshot_head,
                    &recipient,
                )?;
            }
            Recovery::ReceiveJournal { target_id } => {
                let root =
                    moosuri_vault_service::recovery::publication::target_root(&target_id, false)?;
                let ack = moosuri_vault_service::recovery::publication::receive_command(&root)?;
                println!("{}", serde_json::to_string(&ack)?);
            }
            Recovery::TargetStatus { target_id } => {
                let root =
                    moosuri_vault_service::recovery::publication::target_root(&target_id, false)?;
                let status = moosuri_vault_service::recovery::publication::target_status(&root)?;
                println!("{}", serde_json::to_string(&status)?);
            }
            Recovery::JournalCheckpoint {
                recipient,
                output,
                trusted_head,
            } => {
                let head = moosuri_vault_service::recovery::historical::journal_checkpoint(
                    args.data_dir.as_deref().ok_or("vault_data_dir_required")?,
                    &recipient,
                    &output,
                    &trusted_head,
                )?;
                println!(
                    "vault_journal_head_sha256={}; captured head only, not independent latestness",
                    head.pin()?
                );
            }
            Recovery::RestoreHistorical {
                snapshot,
                snapshot_head,
                snapshot_identity_file,
                journal,
                journal_head,
                journal_head_sha256,
                journal_identity_file,
            } => {
                let inputs = moosuri_vault_service::recovery::historical::JournalInputs {
                    checkpoint: &journal,
                    trusted_head: &journal_head,
                    trusted_head_sha256: &journal_head_sha256,
                    identity_file: &journal_identity_file,
                };
                moosuri_vault_service::recovery::historical::restore_historical(
                    args.data_dir.as_deref().ok_or("vault_data_dir_required")?,
                    &snapshot,
                    &snapshot_head,
                    &snapshot_identity_file,
                    &inputs,
                )?;
            }
            Recovery::Checkpoint {
                recipient,
                output,
                trusted_head,
                seal_source,
            } => {
                moosuri_vault_service::recovery::checkpoint(
                    args.data_dir.as_deref().ok_or("vault_data_dir_required")?,
                    &recipient,
                    &output,
                    &trusted_head,
                    seal_source,
                )?;
            }
            Recovery::Restore {
                checkpoint,
                trusted_head,
                identity_file,
            } => {
                moosuri_vault_service::recovery::restore(
                    args.data_dir.as_deref().ok_or("vault_data_dir_required")?,
                    &checkpoint,
                    &trusted_head,
                    &identity_file,
                )?;
            }
            Recovery::RestoreSnapshot {
                checkpoint,
                trusted_head,
                identity_file,
            } => {
                moosuri_vault_service::recovery::restore_snapshot(
                    args.data_dir.as_deref().ok_or("vault_data_dir_required")?,
                    &checkpoint,
                    &trusted_head,
                    &identity_file,
                )?;
            }
        }
        return Ok(());
    }
    let data_dir = args.data_dir.as_deref().ok_or("vault_data_dir_required")?;
    let origin = args.origin.ok_or("vault_origin_required")?;
    let assistant_origin = args.assistant_origin.ok_or("assistant_origin_required")?;
    let adapter_key_file = args.adapter_key_file.ok_or("adapter_key_file_required")?;
    if !args.bind.ip().is_loopback() {
        return Err("loopback_bind_required_use_tls_proxy".into());
    }
    if !moosuri_vault_service::http::has_assets() {
        return Err("build_vault_web_before_rust_binary".into());
    }
    let config = Config::new(&origin, &assistant_origin, args.dev)?;
    let adapter = Adapter::new(&args.adapter_url, private_key(&adapter_key_file)?)?;
    let db = Arc::new(Mutex::new(Database::open(data_dir)?));
    let app = router_shared(db.clone(), adapter, config);
    let listener = tokio::net::TcpListener::bind(args.bind).await?;
    let publisher = if let Some(directory) = args.journal_directory {
        Some(moosuri_vault_service::recovery::publication::Worker::start(
            db.clone(),
            moosuri_vault_service::recovery::publication::WorkerConfig {
                directory,
                recipient: args
                    .journal_recipient
                    .ok_or("vault_publication_configuration_incomplete")?,
                snapshot_head: args
                    .journal_snapshot_head
                    .ok_or("vault_publication_configuration_incomplete")?,
                ssh_target: args
                    .journal_ssh_target
                    .ok_or("vault_publication_configuration_incomplete")?,
                target_id: args
                    .journal_target_id
                    .ok_or("vault_publication_configuration_incomplete")?,
                interval_seconds: args.journal_interval_seconds.unwrap_or(30),
            },
        )?)
    } else {
        None
    };
    let worker = match (args.backup_directory, args.backup_recipient) {
        (Some(directory), Some(recipient)) => {
            Some(moosuri_vault_service::recovery::online::Worker::start(
                db,
                &directory,
                &recipient,
                args.backup_interval_seconds.unwrap_or(3600),
            )?)
        }
        (None, None) => None,
        _ => return Err("vault_backup_configuration_incomplete".into()),
    };
    let result = axum::serve(listener, app)
        .with_graceful_shutdown(async {
            shutdown_signal().await;
        })
        .await;
    if let Some(publisher) = publisher {
        publisher.shutdown().await;
    }
    if let Some(worker) = worker {
        worker.shutdown().await;
    }
    result?;
    Ok(())
}
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        if let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        {
            tokio::select! {_=tokio::signal::ctrl_c()=>(),_=terminate.recv()=>()}
            return;
        }
    }
    let _ = tokio::signal::ctrl_c().await;
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn publisher_flags_are_opt_in_complete_and_receiver_has_no_source_data_dir() {
        assert!(
            Args::try_parse_from([
                "vault-service",
                "--data-dir",
                "/private/source",
                "--journal-directory",
                "/private/scratch"
            ])
            .is_err()
        );
        let args = Args::try_parse_from([
            "vault-service",
            "--data-dir",
            "/private/source",
            "--journal-directory",
            "/private/scratch",
            "--journal-recipient",
            "age1SYNTHETIC",
            "--journal-snapshot-head",
            "/private/snapshot.json",
            "--journal-ssh-target",
            "backup-host",
            "--journal-target-id",
            "vault-1",
        ])
        .unwrap();
        assert_eq!(args.journal_ssh_target.as_deref(), Some("backup-host"));
        for command in ["receive-journal", "target-status"] {
            let args = Args::try_parse_from([
                "vault-service",
                "recovery",
                command,
                "--target-id",
                "vault-1",
            ])
            .unwrap();
            assert!(args.data_dir.is_none());
        }
    }
    #[test]
    fn historical_journal_restore_requires_explicit_receipt_pin() {
        let flags = [
            "vault-service",
            "--data-dir",
            "/private/restored",
            "recovery",
            "restore-historical",
            "--snapshot",
            "/private/s.age",
            "--snapshot-head",
            "/private/s.json",
            "--snapshot-identity-file",
            "/private/s.key",
            "--journal",
            "/private/j.age",
            "--journal-head",
            "/private/j.json",
            "--journal-identity-file",
            "/private/j.key",
        ];
        assert!(Args::try_parse_from(flags).is_err());
        let mut args = flags.to_vec();
        args.extend([
            "--journal-head-sha256",
            "0000000000000000000000000000000000000000000000000000000000000000",
        ]);
        assert!(matches!(
            Args::try_parse_from(args).unwrap().command,
            Some(Command::Recovery {
                command: Recovery::RestoreHistorical { .. }
            })
        ));
    }
    #[test]
    fn ongoing_snapshot_service_flags_require_recipient_and_restore_is_explicit() {
        assert!(
            Args::try_parse_from([
                "vault-service",
                "--data-dir",
                "/private/example",
                "--backup-directory",
                "/private/exports"
            ])
            .is_err()
        );
        let args = Args::try_parse_from([
            "vault-service",
            "--data-dir",
            "/private/example",
            "--backup-directory",
            "/private/exports",
            "--backup-recipient",
            "age1SYNTHETIC",
            "--backup-interval-seconds",
            "300",
        ])
        .unwrap();
        assert_eq!(args.backup_interval_seconds, Some(300));
        let args = Args::try_parse_from([
            "vault-service",
            "--data-dir",
            "/private/new",
            "recovery",
            "restore-snapshot",
            "--checkpoint",
            "/private/snapshot.age",
            "--trusted-head",
            "/private/snapshot.head.json",
            "--identity-file",
            "/private/identity",
        ])
        .unwrap();
        assert!(matches!(
            args.command,
            Some(Command::Recovery {
                command: Recovery::RestoreSnapshot { .. }
            })
        ));
    }
    #[test]
    fn existing_service_flags_and_offline_recovery_parse() {
        let service = Args::try_parse_from([
            "vault-service",
            "--data-dir",
            "/private/example",
            "--origin",
            "https://vault.example.test",
            "--assistant-origin",
            "https://assistant.example.test",
            "--adapter-key-file",
            "/private/key",
        ])
        .unwrap();
        assert!(service.command.is_none());
        let recovery = Args::try_parse_from([
            "vault-service",
            "--data-dir",
            "/private/example",
            "recovery",
            "restore",
            "--checkpoint",
            "/private/current.age",
            "--trusted-head",
            "/private/head.json",
            "--identity-file",
            "/private/identity",
        ])
        .unwrap();
        assert!(matches!(
            recovery.command,
            Some(Command::Recovery {
                command: Recovery::Restore { .. }
            })
        ));
    }
}
