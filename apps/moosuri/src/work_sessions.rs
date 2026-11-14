use clap::Subcommand;
use std::path::PathBuf;
#[derive(Subcommand)]
pub enum Command {
    /// Once-only import of supported interactive Codex JSONL user requests.
    /// No default directory, token, destination or background service is used.
    Codex {
        #[arg(long)]
        directory: PathBuf,
        /// Exact HTTPS origin; this command never follows redirects or proxies.
        #[arg(long)]
        origin: String,
        /// The dedicated custom HTTP connection ID from your source settings.
        #[arg(long)]
        connection_id: String,
        /// Existing owner-only token file outside the source directory.
        #[arg(long)]
        token_file: PathBuf,
        /// Durable owner-only cursor outside the source directory and any Git checkout.
        #[arg(long)]
        state_file: PathBuf,
    },
}
pub async fn run(command: &Command) -> Result<(), String> {
    #[cfg(unix)]
    {
        let Command::Codex {
            directory,
            origin,
            connection_id,
            token_file,
            state_file,
        } = command;
        let receipt = moosuri_connectors::work_sessions::run_once(
            &moosuri_connectors::work_sessions::Options {
                directory: directory.clone(),
                origin: origin.clone(),
                connection_id: connection_id.clone(),
                token_file: token_file.clone(),
                state_file: state_file.clone(),
            },
        )
        .await
        .map_err(str::to_owned)?;
        println!(
            "{}",
            serde_json::to_string(&receipt).map_err(|_| "receipt_encode_failed")?
        );
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = command;
        Err("local_work_sessions_requires_unix_private_file_permissions".into())
    }
}

#[cfg(all(test, unix, feature = "test-fixtures"))]
mod recovery_tests;
