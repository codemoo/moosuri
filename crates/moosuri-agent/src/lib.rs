//! Supervised, bounded assistant execution. No browser owns a model task.
//! Watch notifications contain only a monotonic counter, never private identifiers.
mod artifact_tools;
mod automatic_memory;
pub mod browser;
pub mod calendar;
mod calendar_intent;
mod calendar_tools;
pub mod claude_execution;
mod claude_login;
mod conversation_tools;
pub mod credential_broker;
mod credential_chat;
mod intent_contract;
mod knowledge_tools;
pub mod mail;
mod mail_tools;
mod memory_intent;
mod memory_target;
mod mission_intent;
mod native_mail_tools;
mod push;
mod runner;
mod schedules;
mod source_connections;
mod store_retry;
pub mod subscriptions;
mod telegram;
mod web;
mod web_tools;

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

const RUN_DRAIN: Duration = Duration::from_secs(6);
const MAIL_DRAIN: Duration = Duration::from_secs(2);
const BROWSER_DRAIN: Duration = Duration::from_secs(7);
// Claude cancellation settles a bounded engine remove (10s) and, when needed,
// an independent absence query (10s) before releasing its authentication lock.
const EXTRACTION_DRAIN: Duration = Duration::from_secs(22);
// Enclose every sequential drain, including extraction cleanup, for both the
// supervisor owner and concurrent public shutdown callers.
const SUPERVISOR_WAIT: Duration = Duration::from_secs(
    RUN_DRAIN.as_secs()
        + BROWSER_DRAIN.as_secs()
        + EXTRACTION_DRAIN.as_secs()
        + 6 * MAIL_DRAIN.as_secs()
        + 2,
);

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
    delivery: Arc<tokio::sync::RwLock<()>>,
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
    run_activity: watch::Receiver<usize>,
    mail: mail::MailService,
    browser: browser::BrowserService,
    subscriptions: subscriptions::SubscriptionService,
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
        Self::start_with_mail(store, None)
    }
    pub fn start_with_mail(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
    ) -> Self {
        Self::start_with_tools(store, oauth, None)
    }
    pub fn start_with_tools(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: Option<Arc<moosuri_connectors::web::WebClient>>,
    ) -> Self {
        Self::start_with_browser_tools(store, oauth, web, None)
    }
    pub fn start_with_browser_tools(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: Option<Arc<moosuri_connectors::web::WebClient>>,
        browser: Option<Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
    ) -> Self {
        Self::start_with_web_service(store, oauth, web::Service::new(web).with_browser(browser))
    }
    /// Hosted settings keep the configured origin separate from request headers.
    pub fn start_with_google_setup(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: Option<Arc<moosuri_connectors::web::WebClient>>,
        browser: Option<Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
        redirect_uri: String,
    ) -> Self {
        Self::start_with_services(
            store,
            oauth,
            web::Service::new(web).with_browser(browser),
            Some(redirect_uri),
            None,
        )
    }
    fn start_with_web_service(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: web::Service,
    ) -> Self {
        Self::start_with_services(store, oauth, web, None, None)
    }
    pub fn start_with_google_setup_claude(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: Option<Arc<moosuri_connectors::web::WebClient>>,
        browser: Option<Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
        redirect_uri: String,
        claude: Option<Arc<moosuri_connectors::claude_code::Profile>>,
    ) -> Self {
        Self::start_with_services(
            store,
            oauth,
            web::Service::new(web).with_browser(browser),
            Some(redirect_uri),
            claude,
        )
    }
    pub fn start_with_google_setup_claude_pdf(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: Option<Arc<moosuri_connectors::web::WebClient>>,
        browser: Option<Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
        redirect_uri: String,
        claude: Option<Arc<moosuri_connectors::claude_code::Profile>>,
        pdf_extractor: Option<std::path::PathBuf>,
    ) -> Self {
        Self::start_with_services_pdf(
            store,
            oauth,
            web::Service::new(web).with_browser(browser),
            Some(redirect_uri),
            claude,
            pdf_extractor,
            None,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub fn start_with_google_setup_claude_pdf_gws(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: Option<Arc<moosuri_connectors::web::WebClient>>,
        browser: Option<Arc<moosuri_connectors::browser::capsule::CapsuleReader>>,
        redirect_uri: String,
        claude: Option<Arc<moosuri_connectors::claude_code::Profile>>,
        pdf_extractor: Option<std::path::PathBuf>,
        gws_inbox: Option<Arc<moosuri_connectors::gws_inbox::GwsInboxRegistry>>,
    ) -> Self {
        Self::start_with_services_pdf(
            store,
            oauth,
            web::Service::new(web).with_browser(browser),
            Some(redirect_uri),
            claude,
            pdf_extractor,
            gws_inbox,
        )
    }
    fn start_with_services(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        web: web::Service,
        redirect_uri: Option<String>,
        claude: Option<Arc<moosuri_connectors::claude_code::Profile>>,
    ) -> Self {
        Self::start_with_services_pdf(store, oauth, web, redirect_uri, claude, None, None)
    }
    fn start_with_services_pdf(
        store: Store,
        oauth: Option<Arc<moosuri_connectors::gmail::OAuthClient>>,
        mut web: web::Service,
        redirect_uri: Option<String>,
        claude: Option<Arc<moosuri_connectors::claude_code::Profile>>,
        pdf_extractor: Option<std::path::PathBuf>,
        gws_inbox: Option<Arc<moosuri_connectors::gws_inbox::GwsInboxRegistry>>,
    ) -> Self {
        let (changes, _) = watch::channel(0);
        let (done_tx, done) = watch::channel(false);
        let (run_activity_tx, run_activity) = watch::channel(0);
        let signals = Signals {
            stop: CancellationToken::new(),
            wake: Arc::new(Notify::new()),
            changes,
            delivery: Arc::new(tokio::sync::RwLock::new(())),
        };
        let telegram_origin = redirect_uri
            .as_deref()
            .and_then(|s| url::Url::parse(s).ok())
            .map(|u| u.origin().ascii_serialization());
        let mail = mail::MailService::new(store.clone(), oauth, signals.clone())
            .with_managed_google(redirect_uri)
            .with_pdf_extractor(pdf_extractor)
            .with_gws_inbox(gws_inbox);
        let browser =
            browser::BrowserService::new(store.clone(), web.reader(), signals.stop.clone());
        web.set_workspaces(browser.clone());
        let mut subscriptions =
            subscriptions::SubscriptionService::new(store.clone(), signals.clone());
        subscriptions.claude = claude;
        let supervisor = tokio::spawn(supervise(
            store,
            signals.clone(),
            done_tx,
            run_activity_tx,
            mail.clone(),
            web,
            subscriptions.clone(),
            telegram_origin,
        ));
        Self {
            control: Arc::new(Control {
                signals,
                supervisor: Mutex::new(Some(supervisor)),
                done,
                run_activity,
                mail,
                browser,
                subscriptions,
            }),
        }
    }
    pub fn subscription_service(&self) -> subscriptions::SubscriptionService {
        self.control.subscriptions.clone()
    }
    pub fn browser_service(&self) -> browser::BrowserService {
        self.control.browser.clone()
    }
    pub fn mail_service(&self) -> mail::MailService {
        self.control.mail.clone()
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
    /// Counts only active supervised run workers and reaped workers whose local
    /// retirement still needs settlement. It does not count DB-queued work,
    /// background service workers or provider requests. Updates follow actual
    /// JoinSet spawn/reap and retirement; channel closure or is_stopping() means
    /// a zero value cannot establish live-runtime quiescence.
    pub fn subscribe_run_activity(&self) -> watch::Receiver<usize> {
        self.control.run_activity.clone()
    }
    /// The same short gate orders HTTP and runtime mutations against SSE delivery.
    /// Never hold it while waiting for a provider response or a client network write.
    pub fn delivery_gate(&self) -> Arc<tokio::sync::RwLock<()>> {
        self.control.signals.delivery.clone()
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
            if tokio::time::timeout(SUPERVISOR_WAIT, &mut handle)
                .await
                .is_err()
            {
                handle.abort();
                let _ = handle.await;
            }
        } else {
            // Concurrent callers also wait for supervision to finish.
            let mut done = self.control.done.clone();
            let _ = tokio::time::timeout(SUPERVISOR_WAIT, async {
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
            .finish_agent_execution(
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
#[allow(clippy::too_many_arguments)]
async fn supervise(
    store: Store,
    signals: Signals,
    done: watch::Sender<bool>,
    run_activity: watch::Sender<usize>,
    mail: mail::MailService,
    web: web::Service,
    subscriptions: subscriptions::SubscriptionService,
    telegram_origin: Option<String>,
) {
    let claude_service = subscriptions.clone();
    let claude_worker =
        tokio::spawn(async move { claude_service.claude_retirement_worker().await });
    let mut extraction_worker = tokio::spawn(automatic_memory::worker(
        store.clone(),
        signals.clone(),
        subscriptions.clone(),
    ));
    let mut schedule_worker = tokio::spawn(schedules::worker(store.clone(), signals.clone()));
    let mut push_worker = tokio::spawn(push::worker(store.clone(), signals.clone()));
    let mut telegram_worker = tokio::spawn(telegram::worker(
        store.clone(),
        signals.clone(),
        telegram_origin.clone(),
    ));
    let mut mail_worker = tokio::spawn(mail.clone().worker(signals.stop.clone()));
    let mut reply_watch_worker =
        tokio::spawn(mail.clone().reply_watch_worker(signals.stop.clone()));
    let mut source_worker = tokio::spawn(
        source_connections::Service::new(store.clone(), mail.clone(), signals.clone()).worker(),
    );
    let workspace = web.workspaces().expect("runtime workspace service");
    let mut browser_worker = tokio::spawn(async move { workspace.maintenance().await });
    let mut credential_check = tokio::time::Instant::now();
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
        run_activity.send_replace(tasks.len() + retired.len());
        if cfg!(feature = "credential-broker-chat")
            && credential_check.elapsed() >= Duration::from_secs(2)
        {
            credential_check = tokio::time::Instant::now();
            if store.settle_native_task_mail().await.is_ok_and(|n| n > 0) {
                signals.changed();
                signals.wake.notify_one();
            }
            if store.settle_credential_chats().await.is_ok_and(|n| n > 0) {
                signals.changed();
            }
        }
        if reply_watch_worker.is_finished() {
            let _ = (&mut reply_watch_worker).await;
            reply_watch_worker =
                tokio::spawn(mail.clone().reply_watch_worker(signals.stop.clone()));
        }
        if telegram_worker.is_finished() {
            let _ = (&mut telegram_worker).await;
            telegram_worker = tokio::spawn(telegram::worker(
                store.clone(),
                signals.clone(),
                telegram_origin.clone(),
            ));
        }
        if push_worker.is_finished() {
            let _ = (&mut push_worker).await;
            push_worker = tokio::spawn(push::worker(store.clone(), signals.clone()));
        }
        if schedule_worker.is_finished() {
            let _ = (&mut schedule_worker).await;
            schedule_worker = tokio::spawn(schedules::worker(store.clone(), signals.clone()));
        }
        if extraction_worker.is_finished() {
            let _ = (&mut extraction_worker).await;
            extraction_worker = tokio::spawn(automatic_memory::worker(
                store.clone(),
                signals.clone(),
                subscriptions.clone(),
            ));
        }
        if source_worker.is_finished() {
            let _ = (&mut source_worker).await;
            source_worker = tokio::spawn(
                source_connections::Service::new(store.clone(), mail.clone(), signals.clone())
                    .worker(),
            );
        }
        while tasks.len() + retired.len() < 2 && !signals.stop.is_cancelled() {
            match store.claim_agent_run().await {
                Ok(Some(claimed)) => {
                    signals.changed();
                    let lease = claimed.lease.clone();
                    let id = tasks
                        .spawn(runner::execute(
                            store.clone(),
                            claimed,
                            signals.clone(),
                            mail.clone(),
                            web.clone(),
                            subscriptions.clone(),
                        ))
                        .id();
                    leases.insert(id, lease);
                    run_activity.send_replace(tasks.len() + retired.len());
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
    claude_worker.abort();
    let _ = claude_worker.await;
    let drain = async {
        while let Some(result) = tasks.join_next_with_id().await {
            reap(result, &mut leases, &mut retired);
            run_activity.send_replace(tasks.len() + retired.len());
        }
        settle_retired(&mut retired, &store, &signals).await;
        run_activity.send_replace(tasks.len() + retired.len());
    };
    if tokio::time::timeout(RUN_DRAIN, drain).await.is_err() {
        tasks.abort_all();
        while tasks.join_next().await.is_some() {}
        // Uncommitted attempts remain dispatched and recover as unknown, never
        // queued. Do not wait unboundedly on a failed store during shutdown.
    }
    if tokio::time::timeout(MAIL_DRAIN, &mut reply_watch_worker)
        .await
        .is_err()
    {
        reply_watch_worker.abort();
        let _ = reply_watch_worker.await;
    }
    if tokio::time::timeout(MAIL_DRAIN, &mut mail_worker)
        .await
        .is_err()
    {
        mail_worker.abort();
        let _ = mail_worker.await;
    }
    if tokio::time::timeout(BROWSER_DRAIN, &mut browser_worker)
        .await
        .is_err()
    {
        browser_worker.abort();
        let _ = browser_worker.await;
    }
    if tokio::time::timeout(MAIL_DRAIN, &mut source_worker)
        .await
        .is_err()
    {
        source_worker.abort();
        let _ = source_worker.await;
    }
    if tokio::time::timeout(EXTRACTION_DRAIN, &mut extraction_worker)
        .await
        .is_err()
    {
        extraction_worker.abort();
        let _ = extraction_worker.await;
    }
    if tokio::time::timeout(MAIL_DRAIN, &mut telegram_worker)
        .await
        .is_err()
    {
        telegram_worker.abort();
        let _ = telegram_worker.await;
    }
    if tokio::time::timeout(MAIL_DRAIN, &mut push_worker)
        .await
        .is_err()
    {
        push_worker.abort();
        let _ = push_worker.await;
    }
    if tokio::time::timeout(MAIL_DRAIN, &mut schedule_worker)
        .await
        .is_err()
    {
        schedule_worker.abort();
        let _ = schedule_worker.await;
    }
    done.send_replace(true);
}

#[cfg(test)]
mod tests;
