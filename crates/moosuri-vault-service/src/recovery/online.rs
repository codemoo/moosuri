//! Non-sealing capture under the service's exact Database mutex; encryption runs after release.
use super::*;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::watch;

pub struct CapturedSnapshot {
    stage: Stage,
    binding: CheckpointBinding,
}
fn root(db: &Database) -> Result<PathBuf> {
    Ok(Path::new(db.conn.path().ok_or(Error::Unavailable)?)
        .parent()
        .ok_or(Error::Unavailable)?
        .to_owned())
}
fn directory(db: &Database, path: &Path) -> Result<()> {
    private_dir(path)?;
    let root = root(db)?;
    if path.starts_with(&root) || root.starts_with(path) {
        return Err(Error::Invalid);
    }
    Ok(())
}
impl Database {
    /// Caller retains its ordinary exclusive Database access through DB+journal copy.
    /// No source seal, session change or mutation is performed. Staging is private.
    pub fn capture_snapshot(&self, output_directory: &Path) -> Result<CapturedSnapshot> {
        self.ready()?;
        directory(self, output_directory)?;
        let root = root(self)?;
        private_dir(&root)?;
        if root.join("vault.sealed").symlink_metadata().is_ok() {
            return Err(Error::Unavailable);
        }
        let (sequence, head): (i64, Vec<u8>) = self.conn.query_row(
            "SELECT sequence,head FROM meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if sequence != self.sequence || head != self.head {
            return Err(Error::Unavailable);
        }
        let pages: i64 = self.conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let size: i64 = self.conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        if pages
            .checked_mul(size)
            .is_none_or(|n| n < 0 || n > DB_LIMIT as i64)
        {
            return Err(Error::Limit);
        }
        let started = Instant::now();
        let stage = Stage::new(output_directory)?;
        let snapshot = stage.0.join("vault.sqlite");
        let file = new_private(&snapshot)?;
        let mut target = Connection::open(&snapshot)?;
        {
            let backup = rusqlite::backup::Backup::new(&self.conn, &mut target)?;
            loop {
                if started.elapsed() > Duration::from_secs(60) {
                    return Err(Error::Unavailable);
                }
                match backup.step(128)? {
                    rusqlite::backup::StepResult::Done => break,
                    rusqlite::backup::StepResult::More => (),
                    _ => return Err(Error::Unavailable),
                }
            }
        }
        drop(target);
        file.sync_all()?;
        if file.metadata()?.len() > DB_LIMIT {
            return Err(Error::Limit);
        }
        // Independent descriptor: cloning/seeking the live append descriptor
        // would move its shared offset and corrupt subsequent writer appends.
        let mut input = open_private(&root.join("vault.journal"), JOURNAL_LIMIT)?;
        let expected = input.metadata()?.len();
        let mut journal = new_private(&stage.0.join("vault.journal"))?;
        if std::io::copy(
            &mut Read::by_ref(&mut input).take(JOURNAL_LIMIT + 1),
            &mut journal,
        )? != expected
        {
            return Err(Error::Unavailable);
        }
        if journal.metadata()?.len() > JOURNAL_LIMIT {
            return Err(Error::Limit);
        }
        journal.sync_all()?;
        if started.elapsed() > Duration::from_secs(60) {
            return Err(Error::Unavailable);
        }
        let binding = CheckpointBinding {
            format: SNAPSHOT_FORMAT.into(),
            version: 1,
            checkpoint: uuid::Uuid::new_v4(),
            source: installation(self)?,
            sequence: self.sequence,
            head: hash_head(&self.head),
        };
        Ok(CapturedSnapshot { stage, binding })
    }
}
fn hash_head(head: &[u8; 32]) -> String {
    head.iter().map(|v| format!("{v:02x}")).collect()
}
impl CapturedSnapshot {
    /// Consume only frozen stage files. Semantics and journal/head checks precede
    /// publication. The public recipient cannot authenticate source or latestness.
    pub fn encrypt(
        self,
        recipient: &str,
        output: &Path,
        trusted_head: &Path,
    ) -> Result<SnapshotTrustedHead> {
        let directory = self.stage.0.parent().ok_or(Error::Invalid)?;
        for p in [output, trusted_head] {
            check_components(p)?;
            if p.parent() != Some(directory) || p.symlink_metadata().is_ok() {
                return Err(Error::Conflict);
            }
        }
        if output == trusted_head {
            return Err(Error::Conflict);
        }
        private_dir(directory)?;
        let recipient: age::x25519::Recipient = recipient.parse().map_err(|_| Error::Invalid)?;
        open_private(&self.stage.0.join("vault.sqlite"), DB_LIMIT)?;
        let journal = self.stage.0.join("vault.journal");
        open_private(&journal, JOURNAL_LIMIT)?;
        let db = Database::open_recovery(&self.stage.0)?;
        if installation(&db)? != self.binding.source
            || db.sequence != self.binding.sequence
            || hash_head(&db.head) != self.binding.head
        {
            return Err(Error::Unavailable);
        }
        verify_semantics(&db, &journal, directory)?;
        drop(db);
        publish_checkpoint(
            &self.stage,
            &journal,
            self.binding,
            &recipient,
            output,
            trusted_head,
            SNAPSHOT_WIRE,
        )
        .map(SnapshotTrustedHead::from)
    }
}
pub struct Worker {
    stop: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Worker {
    pub fn start(
        db: Arc<Mutex<Database>>,
        output_directory: &Path,
        recipient: &str,
        interval_seconds: u64,
    ) -> Result<Self> {
        if !(300..=604800).contains(&interval_seconds) {
            return Err(Error::Invalid);
        }
        let _: age::x25519::Recipient = recipient.parse().map_err(|_| Error::Invalid)?;
        {
            let guard = db.lock().map_err(|_| Error::Unavailable)?;
            directory(&guard, output_directory)?;
        }
        let output_directory = output_directory.to_owned();
        let recipient = recipient.to_owned();
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                if *stopping.borrow() {
                    break;
                }
                let db = db.clone();
                let output_directory = output_directory.clone();
                let recipient = recipient.clone();
                let result = tokio::task::spawn_blocking(move || {
                    let captured = db
                        .lock()
                        .map_err(|_| Error::Unavailable)?
                        .capture_snapshot(&output_directory)?;
                    let id = uuid::Uuid::new_v4();
                    captured.encrypt(
                        &recipient,
                        &output_directory.join(format!("vault-snapshot-{id}.age")),
                        &output_directory.join(format!("vault-snapshot-{id}.head.json")),
                    )
                })
                .await;
                match result {
                    Ok(Ok(_)) => eprintln!("vault_snapshot complete local_checkpoint_only"),
                    _ => eprintln!("vault_snapshot failed"),
                }
                if *stopping.borrow() {
                    break;
                }
                tokio::select! {_=stopping.changed()=>break,_=tokio::time::sleep(Duration::from_secs(interval_seconds))=>()}
            }
        });
        Ok(Self {
            stop,
            task: Some(task),
        })
    }
    pub async fn shutdown(mut self) {
        let _ = self.stop.send(true);
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}
impl Drop for Worker {
    fn drop(&mut self) {
        let _ = self.stop.send(true);
    }
}

#[cfg(test)]
mod tests;
