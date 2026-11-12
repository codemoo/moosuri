//! Supervised, bounded plain-text execution. No browser owns a model task.
//! Watch notifications contain only a monotonic counter, never private identifiers.
mod runner;

use moosuri_connectors::model::{
    CancellationToken, ModelClient, NetworkPolicy, RouterPolicy, Secret, TransportError,
    protocol::{Protocol, TextCapabilities},
};
use moosuri_core::{ProviderConfig, ProviderKind, RunState};
use moosuri_store::{RunFailure, RunLease, Store, StoreError};
use std::{
    collections::HashMap,
    net::IpAddr,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::{
    sync::{Notify, watch},
    task::{Id, JoinHandle, JoinSet},
};

/// Constructs a connection without DNS or IO. Hosted secrets cannot be retargeted.
pub fn model_client(
    config: &ProviderConfig,
    credential: Option<Secret>,
    capabilities: TextCapabilities,
) -> Result<ModelClient, TransportError> {
    if config.kind == ProviderKind::Compatible {
        if !config.router_providers.is_empty() || config.zero_data_retention {
            return Err(TransportError::InvalidConfiguration);
        }
        let network = if config.allowed_ips.is_empty() {
            NetworkPolicy::PublicHttps
        } else {
            NetworkPolicy::LocalExact(
                config
                    .allowed_ips
                    .iter()
                    .map(|v| {
                        v.parse::<IpAddr>()
                            .map_err(|_| TransportError::InvalidConfiguration)
                    })
                    .collect::<Result<Vec<_>, _>>()?,
            )
        };
        return ModelClient::compatible(
            config
                .base_url
                .as_deref()
                .ok_or(TransportError::InvalidConfiguration)?,
            network,
            credential,
            capabilities,
        );
    }
    if config.base_url.is_some()
        || !config.allowed_ips.is_empty()
        || (config.kind != ProviderKind::OpenRouter
            && (!config.router_providers.is_empty() || config.zero_data_retention))
    {
        return Err(TransportError::InvalidConfiguration);
    }
    let protocol = match config.kind {
        ProviderKind::OpenAi => Protocol::Responses,
        ProviderKind::Anthropic => Protocol::AnthropicMessages,
        ProviderKind::OpenRouter => Protocol::OpenRouterChat,
        ProviderKind::Compatible => unreachable!(),
    };
    ModelClient::hosted(
        protocol,
        credential.ok_or(TransportError::InvalidConfiguration)?,
        capabilities,
        (config.kind == ProviderKind::OpenRouter).then(|| RouterPolicy {
            providers: config.router_providers.clone(),
            zero_data_retention: config.zero_data_retention,
        }),
    )
}

#[derive(Clone)]
pub(crate) struct Signals {
    stop: CancellationToken,
    wake: Arc<Notify>,
    changes: watch::Sender<u64>,
}
impl Signals {
    pub(crate) fn changed(&self) {
        self.changes.send_modify(|n| *n = n.saturating_add(1));
    }
    pub(crate) fn stopping(&self) -> &CancellationToken {
        &self.stop
    }
}
struct Control {
    signals: Signals,
    supervisor: Mutex<Option<JoinHandle<()>>>,
    done: watch::Receiver<bool>,
}
impl Drop for Control {
    fn drop(&mut self) {
        // The supervisor owns Signals, not Control: the last public handle really
        // reaches this destructor and the child JoinSet is then drained/aborted.
        self.signals.stop.cancel();
        self.signals.changed();
    }
}

#[derive(Clone)]
pub struct Runtime {
    control: Arc<Control>,
}
impl Runtime {
    pub fn start(store: Store) -> Self {
        let (changes, _) = watch::channel(0);
        let (done_tx, done) = watch::channel(false);
        let signals = Signals {
            stop: CancellationToken::new(),
            wake: Arc::new(Notify::new()),
            changes,
        };
        let supervisor = tokio::spawn(supervise(store, signals.clone(), done_tx));
        Self {
            control: Arc::new(Control {
                signals,
                supervisor: Mutex::new(Some(supervisor)),
                done,
            }),
        }
    }
    pub fn wake(&self) {
        self.control.signals.wake.notify_one();
        self.control.signals.changed();
    }
    pub fn is_stopping(&self) -> bool {
        self.control.signals.stop.is_cancelled()
    }
    pub fn stop(&self) {
        self.control.signals.stop.cancel();
        self.control.signals.changed();
    }
    pub fn subscribe(&self) -> watch::Receiver<u64> {
        self.control.signals.changes.subscribe()
    }
    pub async fn shutdown(&self) {
        self.stop();
        let handle = self
            .control
            .supervisor
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .take();
        if let Some(mut handle) = handle {
            if tokio::time::timeout(Duration::from_secs(5), &mut handle)
                .await
                .is_err()
            {
                handle.abort();
                let _ = handle.await;
            }
        } else {
            // Concurrent callers also wait for supervision to finish.
            let mut done = self.control.done.clone();
            let _ = tokio::time::timeout(Duration::from_secs(5), async {
                while !*done.borrow_and_update() && done.changed().await.is_ok() {}
            })
            .await;
        }
    }
}

fn reap(
    result: Result<(Id, ()), tokio::task::JoinError>,
    leases: &mut HashMap<Id, RunLease>,
    retired: &mut Vec<RunLease>,
) {
    let id = match result {
        Ok((id, ())) => id,
        Err(error) => error.id(),
    };
    if let Some(lease) = leases.remove(&id) {
        retired.push(lease);
    }
}
async fn settle_retired(retired: &mut Vec<RunLease>, store: &Store, signals: &Signals) {
    let mut unresolved = Vec::new();
    for lease in retired.drain(..) {
        // Covers both panic and a normal return whose final checkpoint could not
        // commit. Already-terminal/deleted/fenced attempts reject this transition.
        // Transient storage pressure is retried by the supervisor, never by HTTP.
        match store
            .finish_run(
                &lease,
                RunState::OutcomeUnknown,
                Some(RunFailure::Interrupted),
            )
            .await
        {
            Ok(()) => signals.changed(),
            Err(StoreError::Busy | StoreError::Unavailable | StoreError::Corrupt) => {
                unresolved.push(lease)
            }
            Err(_) => (),
        }
    }
    *retired = unresolved;
}
async fn supervise(store: Store, signals: Signals, done: watch::Sender<bool>) {
    let mut tasks = JoinSet::new();
    let mut leases = HashMap::new();
    let mut retired = Vec::new();
    let mut poll = tokio::time::interval(Duration::from_millis(250));
    poll.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            biased;
            _ = signals.stop.cancelled() => break,
            result = tasks.join_next_with_id(), if !tasks.is_empty() => {
                if let Some(result) = result { reap(result, &mut leases, &mut retired); }
            }
            _ = poll.tick() => (),
            _ = signals.wake.notified() => (),
        }
        settle_retired(&mut retired, &store, &signals).await;
        while tasks.len() + retired.len() < 2 && !signals.stop.is_cancelled() {
            match store.claim_run().await {
                Ok(Some(claimed)) => {
                    signals.changed();
                    let lease = claimed.lease.clone();
                    let id = tasks
                        .spawn(runner::execute(store.clone(), claimed, signals.clone()))
                        .id();
                    leases.insert(id, lease);
                }
                Ok(None) => {
                    signals.changed();
                    break;
                }
                Err(_) => break,
            }
        }
    }
    // Cancelling the shared signal interrupts network IO. The bounded deadline
    // also covers a blocked checkpoint; dropping JoinSet aborts every child.
    let drain = async {
        while let Some(result) = tasks.join_next_with_id().await {
            reap(result, &mut leases, &mut retired);
        }
        settle_retired(&mut retired, &store, &signals).await;
    };
    if tokio::time::timeout(Duration::from_secs(4), drain)
        .await
        .is_err()
    {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        // Uncommitted attempts remain dispatched and recover as unknown, never
        // queued. Do not wait unboundedly on a failed store during shutdown.
    }
    done.send_replace(true);
}

#[cfg(test)]
mod tests;
