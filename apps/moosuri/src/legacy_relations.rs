//! Local schema23 relation import CLI. Never opens an old DB or runs psql.
use clap::Args;
use moosuri_store::{legacy_import::Enrollment, legacy_relations};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;
#[derive(Args)]
pub struct Command {
    #[arg(long)]
    pub snapshot: PathBuf,
    #[arg(long)]
    pub legacy_owner: String,
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub email: String,
    #[arg(long)]
    pub password_file: PathBuf,
    #[arg(long)]
    pub report_dir: PathBuf,
}
pub async fn run(dest: &Path, c: Command) -> Result<(), String> {
    let password = Zeroizing::new(crate::read_password_file(&c.password_file)?);
    let result = legacy_relations::import_snapshot(
        &c.snapshot,
        &c.legacy_owner,
        Enrollment {
            name: c.name,
            email: c.email,
            password,
        },
        dest,
        &c.report_dir,
    )
    .await
    .map_err(|e| e.to_string())?;
    println!(
        "Schema23 current-projection import published for new account {}: {} original rows accounted, {} converted objects, {} quarantined rows. Old revisions are metadata, not invented history. No old authority or source service was activated.",
        result.report.account_id,
        result.adapter.total_rows,
        result.adapter.converted_objects,
        result.adapter.quarantined_rows
    );
    Ok(())
}
