//! Private offline import CLI; no legacy DB/network/provider access.
use clap::Args;
use moosuri_store::legacy_import::{self, Enrollment};
use std::path::{Path, PathBuf};
use zeroize::Zeroizing;
#[derive(Args)]
pub struct Command {
    /// Explicit owner-private normalized transfer bundle, never a raw database.
    #[arg(long)]
    pub bundle: PathBuf,
    /// Exact legacy owner identifier to map; other owners are quarantined.
    #[arg(long)]
    pub legacy_owner: String,
    #[arg(long)]
    pub name: String,
    #[arg(long)]
    pub email: String,
    /// Fresh local account password in a private file; never an old credential row.
    #[arg(long)]
    pub password_file: PathBuf,
    /// Separate absent output directory for private reports (published before data).
    #[arg(long)]
    pub report_dir: PathBuf,
}
pub async fn run(data: &Path, c: Command) -> Result<(), String> {
    let password = Zeroizing::new(crate::read_password_file(&c.password_file)?);
    let report = legacy_import::import_bundle(
        &c.bundle,
        &c.legacy_owner,
        Enrollment {
            name: c.name,
            email: c.email,
            password,
        },
        data,
        &c.report_dir,
    )
    .await
    .map_err(|e| e.to_string())?;
    println!(
        "Private staging import published for newly enrolled account {}: {} events applied, {} quarantined, {} total. No old authority or work was imported. Retain the unchanged private bundle and both reports; this is not production migration or cutover.",
        report.account_id, report.imported_events, report.quarantined_events, report.total_events
    );
    Ok(())
}
