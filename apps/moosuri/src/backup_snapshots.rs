//! Opt-in periodic encrypted full snapshots, using the existing live Store writer.
use moosuri_store::{Store, backup};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::watch;
use uuid::Uuid;

pub struct Worker {
    stop: watch::Sender<bool>,
    task: Option<tokio::task::JoinHandle<()>>,
}
impl Worker {
    pub async fn start(
        store: Store,
        root: &Path,
        directory: &Path,
        interval_seconds: u64,
    ) -> Result<Self, String> {
        if !(300..=604_800).contains(&interval_seconds) {
            return Err("backup_interval_out_of_range".into());
        }
        let directory =
            backup::online::validate_directory(root, directory).map_err(|e| e.to_string())?;
        if store
            .export_configuration()
            .await
            .map_err(|e| e.to_string())?
            .active_epoch
            .is_none()
        {
            return Err("backup_unconfigured".into());
        }
        let (stop, mut stopping) = watch::channel(false);
        let task = tokio::spawn(async move {
            loop {
                if *stopping.borrow() {
                    break;
                }
                let result = create(&store, &directory).await;
                match result {
                    Ok(summary) => eprintln!(
                        "backup_snapshot complete export_id={} key_epoch={}; local checkpoint only",
                        summary.export_id, summary.key_epoch
                    ),
                    Err(error) => eprintln!("backup_snapshot failed code={error}"),
                }
                if *stopping.borrow() {
                    break;
                }
                tokio::select! {
                    _ = stopping.changed() => break,
                    _ = tokio::time::sleep(Duration::from_secs(interval_seconds)) => (),
                }
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
        // Finish the admitted bounded capture/encryption, then stop. Aborting a
        // spawn_blocking future cannot cancel its file IO or safely release it.
        let _ = self.stop.send(true);
    }
}
async fn create(store: &Store, directory: &Path) -> Result<backup::ExportSummary, backup::Error> {
    let capture = store.capture_export().await?;
    let output: PathBuf = directory.join(format!("assistant-{}.age", Uuid::new_v4()));
    tokio::task::spawn_blocking(move || capture.encrypt(&output))
        .await
        .map_err(|_| backup::Error::Unavailable)?
}

#[cfg(test)]
mod tests;
