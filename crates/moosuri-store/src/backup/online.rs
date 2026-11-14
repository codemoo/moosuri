//! Immutable full-data capture on the live writer; encryption never occupies it.
use super::*;
use crate::Store;

pub struct CapturedExport {
    root: PathBuf,
    temp: files::Temporary,
}
impl CapturedExport {
    /// Consume only the frozen private stage, never the live database or keys.
    pub fn encrypt(self, output: &Path) -> Result<ExportSummary> {
        let started = Instant::now();
        // Journal::open also supports first-install initialization. A captured
        // export must never reconstruct a missing input, even an empty journal.
        for (name, cap) in [
            ("assistant.sqlite", DB_LIMIT),
            ("auth-epochs.journal", JOURNAL_LIMIT),
            ("knowledge-invalidations.journal", JOURNAL_LIMIT),
            (destructive_intents::FILE_NAME, JOURNAL_LIMIT),
        ] {
            files::read_file(&self.temp.0.join(name), cap)?;
        }
        let conn = Connection::open_with_flags(
            self.temp.0.join("assistant.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
        let auth = journal::Journal::open(&self.temp.0.join("auth-epochs.journal"))?;
        let knowledge =
            knowledge_journal::Journal::open(&self.temp.0.join("knowledge-invalidations.journal"))?;
        let destructive =
            destructive_intents::Journal::open(&self.temp.0.join(destructive_intents::FILE_NAME))?;
        ExportInput {
            root: &self.root,
            staging: &self.temp.0,
            conn: &conn,
            auth: &auth,
            knowledge: &knowledge,
            destructive: &destructive,
        }
        .encrypt(output, started)
    }
}

/// Validate an existing private destination, including source/data separation.
/// No files are created and no private identity is required.
pub fn validate_directory(root: &Path, directory: &Path) -> Result<PathBuf> {
    let directory = files::directory(directory)?;
    files::output_path(root, &directory.join(format!("{}.age", Uuid::new_v4())))?;
    Ok(directory)
}

impl Store {
    pub async fn export_configuration(&self) -> Result<Configuration> {
        self.call(|db| Ok(configuration(&db.conn)))
            .await
            .map_err(Error::from)?
    }

    /// One writer job freezes SQLite's committed WAL and all deletion journals.
    /// Capture is bounded by existing size limits and checked 60s deadline; slow
    /// OS IO is not a hard real-time guarantee. Remote IO/age happen afterwards.
    pub async fn capture_export(&self) -> Result<CapturedExport> {
        self.call(|db| Ok(capture(db))).await.map_err(Error::from)?
    }
}

fn capture(db: &mut crate::Database) -> Result<CapturedExport> {
    let started = Instant::now();
    let root = PathBuf::from(db.conn.path().ok_or(Error::Integrity)?)
        .parent()
        .ok_or(Error::Integrity)?
        .to_owned();
    crate::cutover::require_active(&db.conn, &root)?;
    let configuration = configuration(&db.conn)?;
    if configuration.active_epoch.is_none() {
        return Err(Error::Unconfigured);
    }
    let (auth, knowledge) = journal_heads(&db.conn, &db.journal, &db.knowledge_journal)?;
    let (applied, _) =
        destructive_intents::verify(&db.conn, &db.knowledge_journal, &db.destructive_journal)?;
    let complete = db.destructive_journal.records.last().map_or(0, |r| r.seq);
    if auth.sequence != auth.snapshot_sequence
        || knowledge.sequence != knowledge.snapshot_sequence
        || applied != complete
    {
        return Err(Error::JournalPending);
    }
    let pages: i64 = db.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
    let size: i64 = db.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
    if pages
        .checked_mul(size)
        .is_none_or(|n| n < 0 || n > DB_LIMIT as i64)
    {
        return Err(Error::LimitExceeded);
    }
    let temp = files::Temporary::new(&root)?;
    let path = temp.0.join("assistant.sqlite");
    let file = files::create_file(&path)?;
    let mut snapshot = Connection::open(&path)?;
    {
        let backup = rusqlite::backup::Backup::new(&db.conn, &mut snapshot)?;
        loop {
            check_deadline(started)?;
            match backup.step(256)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => (),
                _ => return Err(Error::Busy),
            }
        }
    }
    drop(snapshot);
    file.sync_all().map_err(|_| Error::Unavailable)?;
    if file.metadata().map_err(|_| Error::Unavailable)?.len() > DB_LIMIT {
        return Err(Error::LimitExceeded);
    }
    for name in [
        "auth-epochs.journal",
        "knowledge-invalidations.journal",
        destructive_intents::FILE_NAME,
    ] {
        check_deadline(started)?;
        files::copy_file(&root.join(name), &temp.0.join(name), JOURNAL_LIMIT)?;
    }
    check_deadline(started)?;
    Ok(CapturedExport { root, temp })
}

#[cfg(test)]
mod tests;
