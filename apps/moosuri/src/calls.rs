use clap::Subcommand;
use std::path::PathBuf;
#[derive(Subcommand)]
pub enum Command {
    /// Import one explicit normalized call-export directory through custom ingest.
    Import {
        #[arg(long)]
        directory: PathBuf,
        #[arg(long)]
        origin: String,
        #[arg(long)]
        connection_id: String,
        #[arg(long)]
        token_file: PathBuf,
        #[arg(long)]
        state_file: PathBuf,
        /// Explicit opt-in for bounded fixed OpenAI WAV transcription. No default key.
        #[arg(long)]
        transcription_key_file: Option<PathBuf>,
    },
}
pub async fn run(command: &Command) -> Result<(), String> {
    #[cfg(unix)]
    {
        let Command::Import {
            directory,
            origin,
            connection_id,
            token_file,
            state_file,
            transcription_key_file,
        } = command;
        let receipt = moosuri_connectors::calls::run_once(&moosuri_connectors::calls::Options {
            source: moosuri_connectors::work_sessions::Options {
                directory: directory.clone(),
                origin: origin.clone(),
                connection_id: connection_id.clone(),
                token_file: token_file.clone(),
                state_file: state_file.clone(),
            },
            transcription_key_file: transcription_key_file.clone(),
        })
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
        Err("local_call_feed_requires_unix_private_file_permissions".into())
    }
}
#[cfg(all(test, unix, feature = "test-fixtures"))]
mod tests;
