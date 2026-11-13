//! Periodic source IO has no model calls and never holds the database writer or
//! chat delivery gate during a provider request. Each page is fenced at commit.
use crate::{Signals, mail::MailService};
use moosuri_connectors::{
    model::CancellationToken,
    source::{
        Coverage, ReadError, SourceChange, SyncPage,
        gmail::{GmailCheckpoint, GmailScope, GmailSyncClient},
        slack::{SlackCheckpoint, SlackClient, SlackScope},
    },
};
use moosuri_core::{
    SourceBatch, SourceConnectionKind, SourceInput, SourceObjectMetadata, SourceRunOutcome,
};
use moosuri_store::{SourceIngestCapability, SourceRunLease, Store, StoreError};
use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const PAGE_BUDGET: usize = 8;
const RUN_SECONDS: u64 = 180;
fn now() -> Result<i64, StoreError> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs(),
    )
    .map_err(|_| StoreError::Unavailable)
}
#[derive(Clone)]
pub(crate) struct Service {
    store: Store,
    mail: MailService,
    signals: Signals,
}
impl Service {
    pub(crate) fn new(store: Store, mail: MailService, signals: Signals) -> Self {
        Self {
            store,
            mail,
            signals,
        }
    }
    pub(crate) async fn worker(self) {
        let mut tick = tokio::time::interval(Duration::from_secs(5));
        tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! { _=self.signals.stop.cancelled()=>break, _=tick.tick()=>{} }
            // At most one connection executes here. Active-run fencing also
            // protects against another dispatcher and concurrent manual sync.
            let Ok(Some(cap)) = self.store.next_source_connection().await else {
                continue;
            };
            let Ok(lease) = self.store.begin_source_run(&cap).await else {
                continue;
            };
            self.signals.changed();
            let cancel = self.signals.stop.child_token();
            let mut work = Box::pin(self.sync(&cap, &lease, &cancel));
            let mut heartbeat = tokio::time::interval(Duration::from_secs(20));
            let deadline = tokio::time::sleep(Duration::from_secs(RUN_SECONDS));
            tokio::pin!(deadline);
            let result = loop {
                tokio::select! {
                    result=&mut work=>break result,
                    _=self.signals.stop.cancelled()=>{ cancel.cancel(); break Err(Failure::interrupted()) },
                    _=&mut deadline=>{ cancel.cancel(); break Err(Failure::continuation()) },
                    _=heartbeat.tick()=>{
                        if self.store.source_heartbeat(&lease).await.is_err() {
                            cancel.cancel(); break Err(Failure::interrupted())
                        }
                    }
                }
            };
            drop(work);
            let (outcome, error, retry) = match result {
                Ok(v) => (v, None, None),
                Err(e) => (
                    e.outcome,
                    (e.code != "continuation").then(|| e.code.into()),
                    e.retry,
                ),
            };
            // Idempotent local settlement only; never retry a network page here.
            for _ in 0..3 {
                match self
                    .store
                    .finish_source_run(&lease, outcome, error.clone(), retry)
                    .await
                {
                    Err(StoreError::Busy) => tokio::time::sleep(Duration::from_millis(25)).await,
                    _ => break,
                }
            }
            self.signals.changed();
        }
    }
    async fn sync(
        &self,
        cap: &SourceIngestCapability,
        lease: &SourceRunLease,
        cancel: &CancellationToken,
    ) -> Result<SourceRunOutcome, Failure> {
        let mut changed = false;
        for _ in 0..PAGE_BUDGET {
            if cancel.is_cancelled() {
                return Err(Failure::interrupted());
            }
            let binding = self
                .store
                .source_runtime_binding(cap)
                .await
                .map_err(Failure::store)?;
            let status = self
                .store
                .source_ingest_status(cap)
                .await
                .map_err(Failure::store)?;
            let (changes, checkpoint, coverage) = match binding.connection.kind {
                SourceConnectionKind::Gmail => {
                    let scope: GmailScope = serde_json::from_str(&binding.connection.scope)
                        .map_err(|_| Failure::invalid())?;
                    let old: Option<GmailCheckpoint> = binding
                        .checkpoint
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()
                        .map_err(|_| Failure::invalid())?;
                    let credentials = self
                        .mail
                        .source_credentials(cap, cancel)
                        .await
                        .map_err(Failure::store)?;
                    let page = GmailSyncClient::default()
                        .next_page(
                            &credentials.access_token,
                            &scope,
                            old.as_ref(),
                            now().map_err(Failure::store)?,
                            cancel,
                        )
                        .await
                        .map_err(Failure::provider)?;
                    parts(page)?
                }
                SourceConnectionKind::Slack => {
                    let scope: SlackScope = serde_json::from_str(&binding.connection.scope)
                        .map_err(|_| Failure::invalid())?;
                    let old: Option<SlackCheckpoint> = binding
                        .checkpoint
                        .as_deref()
                        .map(serde_json::from_str)
                        .transpose()
                        .map_err(|_| Failure::invalid())?;
                    let token = binding.slack_token.as_ref().ok_or_else(Failure::auth)?;
                    let page = SlackClient::default()
                        .next_page(
                            token,
                            &scope,
                            old.as_ref(),
                            now().map_err(Failure::store)?,
                            cancel,
                        )
                        .await
                        .map_err(Failure::provider)?;
                    parts(page)?
                }
                SourceConnectionKind::CustomHttp => return Err(Failure::invalid()),
            };
            changed |= !changes.is_empty();
            let (entries, tombstones, metadata) = normalize(changes);
            let batch = SourceBatch {
                protocol_version: 1,
                request_id: uuid::Uuid::new_v4().to_string(),
                config_revision: status.config_revision,
                run_id: lease.run_id().into(),
                generation: lease.generation(),
                expected_sequence: status.local_sequence,
                checkpoint_before: status.checkpoint,
                checkpoint_after: Some(checkpoint),
                entries,
                tombstones,
                metadata,
                complete_snapshot: false,
            };
            {
                let _delivery = self.signals.delivery.write().await;
                // Retrying the exact local envelope is safe if admission was busy.
                let mut attempts = 0;
                loop {
                    match self.store.commit_source_batch(lease, batch.clone()).await {
                        Err(StoreError::Busy) if attempts < 2 => {
                            attempts += 1;
                            tokio::time::sleep(Duration::from_millis(20)).await;
                        }
                        result => {
                            result.map_err(Failure::store)?;
                            break;
                        }
                    }
                }
            }
            self.signals.changed();
            match coverage {
                Coverage::MorePages => {}
                Coverage::GmailHistoryComplete => {
                    return Ok(if changed {
                        SourceRunOutcome::Completed
                    } else {
                        SourceRunOutcome::NoChange
                    });
                }
                Coverage::SlackPollingWindow => return Ok(SourceRunOutcome::Polled),
            }
        }
        Ok(SourceRunOutcome::Partial)
    }
}
fn parts<C: Serialize>(
    page: SyncPage<C>,
) -> Result<(Vec<SourceChange>, String, Coverage), Failure> {
    Ok((
        page.changes,
        serde_json::to_string(&page.checkpoint).map_err(|_| Failure::invalid())?,
        page.coverage,
    ))
}
fn normalize(
    changes: Vec<SourceChange>,
) -> (Vec<SourceInput>, Vec<String>, Vec<SourceObjectMetadata>) {
    let mut entries = Vec::new();
    let mut tombstones = Vec::new();
    let mut metadata = Vec::new();
    for change in changes {
        match change {
            SourceChange::Upsert(item) => {
                metadata.push(SourceObjectMetadata {
                    external_id: item.external_id.clone(),
                    thread_id: Some(item.thread_id),
                    parent_id: item.parent_id,
                });
                entries.push(SourceInput {
                    external_id: item.external_id,
                    version: item.remote_version,
                    expected_revision: None,
                    title: item.subject,
                    text: item.text,
                    speaker: (!item.speaker.is_empty()).then_some(item.speaker),
                    observed_at: item.observed_at,
                    valid_from: None,
                    valid_until: None,
                });
            }
            SourceChange::Delete { external_id, .. } => tombstones.push(external_id),
        }
    }
    (entries, tombstones, metadata)
}
struct Failure {
    outcome: SourceRunOutcome,
    code: &'static str,
    retry: Option<u32>,
}
impl Failure {
    fn auth() -> Self {
        Self {
            outcome: SourceRunOutcome::NeedsReconnect,
            code: "authentication_required",
            retry: None,
        }
    }
    fn invalid() -> Self {
        Self {
            outcome: SourceRunOutcome::Failed,
            code: "invalid_batch",
            retry: None,
        }
    }
    fn interrupted() -> Self {
        Self {
            outcome: SourceRunOutcome::Unknown,
            code: "outcome_unknown",
            retry: None,
        }
    }
    fn continuation() -> Self {
        Self {
            outcome: SourceRunOutcome::Partial,
            code: "continuation",
            retry: Some(30),
        }
    }
    fn store(error: StoreError) -> Self {
        match error {
            StoreError::Unauthorized | StoreError::Forbidden => Self::auth(),
            StoreError::InvalidInput => Self::invalid(),
            _ => Self::interrupted(),
        }
    }
    fn provider(error: ReadError) -> Self {
        match error {
            ReadError::AuthenticationRequired
            | ReadError::ScopeDenied
            | ReadError::IdentityMismatch => Self::auth(),
            ReadError::ResyncRequired => Self {
                outcome: SourceRunOutcome::NeedsReconnect,
                code: "checkpoint_invalid",
                retry: None,
            },
            ReadError::RateLimited {
                retry_after_seconds,
            } => Self {
                outcome: SourceRunOutcome::Failed,
                code: "rate_limited",
                retry: Some(retry_after_seconds.min(86400) as u32),
            },
            ReadError::Cancelled => Self::interrupted(),
            ReadError::InvalidInput | ReadError::InvalidResponse | ReadError::LimitExceeded => {
                Self::invalid()
            }
            _ => Self {
                outcome: SourceRunOutcome::Failed,
                code: "network_error",
                retry: None,
            },
        }
    }
}
