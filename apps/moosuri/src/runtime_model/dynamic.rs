//! Lifecycle-derived dynamic member sockets for the runtime-model broker.
//!
//! Every enrolled member gets an inert owner-bound socket derived from the
//! private lifecycle enrollment and the registry allocation, whether or not a
//! provider exists. The provider config (the existing `--config`) is optional:
//! absent means an explicit "unconfigured" socket, a present but malformed one
//! fails closed. The existing ledger, admission fences, connection revision and
//! the administrator's stored model selection stay authoritative; this module
//! only swaps in a verified `Broker` generation and republishes the member list.
use super::*;
use std::collections::HashMap;
use tokio::{
    task::JoinHandle,
    time::{MissedTickBehavior, interval, timeout},
};

/// How often membership and the provider config are reconciled.
pub(super) const POLL: Duration = Duration::from_secs(3);
/// Current enrolled owners. Production derives it from the private lifecycle
/// enrollment plus the registry allocation; tests inject synthetic owners.
pub(super) type Source = Arc<dyn Fn() -> Result<Vec<Member>, String> + Send + Sync>;

pub(super) fn lifecycle_source(options: &Options, config: &Path) -> Source {
    let registry = options.registry.clone();
    let config = config.to_owned();
    let root = options.socket_dir.clone();
    Arc::new(move || crate::kiri_lifecycle::onboard::model_bindings(&registry, &config, &root))
}

enum Provider {
    /// No provider config exists: every call gets `model_unconfigured`.
    Unconfigured,
    /// A config exists but failed verification: fail closed, never fall back.
    Invalid,
    /// Membership authority could not be checked: no call is admitted from the
    /// cached member list until the source answers again.
    Fenced,
    Ready {
        broker: Arc<Broker>,
        bindings: BTreeMap<Uuid, Result<Binding, &'static str>>,
    },
}
struct State {
    provider: Mutex<Arc<Provider>>,
    slots: Arc<Semaphore>,
}
impl State {
    fn current(&self) -> Arc<Provider> {
        self.provider
            .lock()
            .map(|p| p.clone())
            .unwrap_or_else(|_| Arc::new(Provider::Invalid))
    }
    fn set(&self, provider: Provider) {
        if let Ok(mut current) = self.provider.lock() {
            *current = Arc::new(provider);
        }
    }
}

fn unavailable(command: &Command, code: &str) -> Reply {
    match command {
        Command::Call(_) => Reply::error(code),
        // Selection clients only know the generic unavailable state.
        Command::Selection(query) => Reply {
            v: query.v,
            ..Reply::error("model_selection_unavailable")
        },
    }
}
async fn respond(provider: Arc<Provider>, member: &Member, command: Command) -> Reply {
    let (broker, bindings) = match &*provider {
        Provider::Unconfigured => return unavailable(&command, "model_unconfigured"),
        Provider::Invalid => return unavailable(&command, "model_config_invalid"),
        Provider::Fenced => return unavailable(&command, "model_membership_unavailable"),
        Provider::Ready { broker, bindings } => (broker, bindings),
    };
    let binding = match bindings.get(&member.account_id) {
        // A newer membership generation owns this account: this socket is stale.
        Some(Ok(b))
            if b.tenant_id == member.tenant_id
                && b.volume_id == member.volume_id
                && b.generation == member.generation
                && b.peer_uid == member.peer_uid =>
        {
            b.clone()
        }
        Some(Err(code)) => return unavailable(&command, code),
        _ => return unavailable(&command, "account_fenced"),
    };
    match command {
        Command::Call(request) => broker.call(&binding, request).await,
        Command::Selection(query) => broker.selection(&binding, query),
    }
}
async fn handle(mut stream: UnixStream, state: Arc<State>, slots: Arc<Semaphore>, member: Member) {
    let reply = if stream.peer_cred().map(|c| c.uid()).ok() != Some(member.peer_uid) {
        Reply::error("peer_denied")
    } else {
        match slots.try_acquire_owned() {
            Err(_) => Reply::error("broker_busy"),
            // The request is always consumed, even when inert, so callers see
            // an explicit state rather than a reset connection.
            Ok(_slot) => match frame(&mut stream, MAX_FRAME, Duration::from_secs(10)).await {
                Err(e) => Reply::error(&e),
                Ok(bytes) => match serde_json::from_slice::<Command>(&bytes) {
                    Ok(command) => respond(state.current(), &member, command).await,
                    Err(_) => Reply::error("request_invalid"),
                },
            },
        }
    };
    if let Ok(mut bytes) = serde_json::to_vec(&reply) {
        bytes.push(b'\n');
        let _ = timeout(Duration::from_secs(5), stream.write_all(&bytes)).await;
    }
}

struct Slot {
    member: Member,
    stop: CancellationToken,
    task: JoinHandle<JoinSet<()>>,
}
async fn serve_member(
    listener: UnixListener,
    guard: SocketGuard,
    member: Member,
    state: Arc<State>,
    stop: CancellationToken,
) -> JoinSet<()> {
    let mut requests = JoinSet::new();
    loop {
        tokio::select! {
            _ = stop.cancelled() => break,
            _ = requests.join_next(), if !requests.is_empty() => {},
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => {
                    let slots = state.slots.clone();
                    requests.spawn(handle(stream, state.clone(), slots, member));
                }
                Err(_) => break,
            },
        }
    }
    // Fence first: the old socket is gone before a replacement can be bound.
    // Calls already admitted finish as in-flight work and are never re-accepted;
    // their admission and result are re-checked against the registry.
    drop(listener);
    drop(guard);
    requests
}

/// Sockets whose release is still pending. The handle (and the in-flight calls
/// it returns) is retained until the task ends, so admitted calls are never
/// dropped by a slow release, and no replacement is bound for that account.
type Releasing = HashMap<Uuid, JoinHandle<JoinSet<()>>>;

/// Drop the settled in-flight call sets; keep those with calls still running.
fn reap(drains: &mut Vec<JoinSet<()>>) {
    for drain in drains.iter_mut() {
        while drain.try_join_next().is_some() {}
    }
    drains.retain(|drain| !drain.is_empty());
}

/// Split an enrollment listing into usable members and per-member rejections.
/// One malformed or ambiguous entry never rejects its neighbours; only an
/// over-large listing (not a plausible enrollment) is rejected as a whole.
fn usable(listed: Vec<Member>) -> Result<(Vec<Member>, Vec<(Uuid, String)>), String> {
    if listed.len() > 128 {
        return Err("model_enrollment_invalid".into());
    }
    let mut count: BTreeMap<Uuid, usize> = BTreeMap::new();
    for m in &listed {
        *count.entry(m.account_id).or_default() += 1;
    }
    let (mut ok, mut bad) = (Vec::new(), Vec::new());
    for m in listed {
        if m.account_id.is_nil() || m.generation <= 0 || count[&m.account_id] != 1 {
            bad.push((m.account_id, "model_enrollment_invalid".to_owned()));
        } else {
            ok.push(m);
        }
    }
    Ok((ok, bad))
}

/// Make the live sockets equal the desired members. Any change of account,
/// tenant, volume, generation or UID replaces the whole socket. A failure for
/// one member (bind, invalid socket path, slow release) is returned as that
/// member's fault and retried on the next tick; healthy members are untouched.
async fn reconcile(
    root: &Path,
    state: &Arc<State>,
    slots: &mut HashMap<Uuid, Slot>,
    releasing: &mut Releasing,
    drains: &mut Vec<JoinSet<()>>,
    desired: &[Member],
) -> Vec<(Uuid, String)> {
    let mut faults = Vec::new();
    let done: Vec<Uuid> = releasing
        .iter()
        .filter(|(_, task)| task.is_finished())
        .map(|(id, _)| *id)
        .collect();
    for id in done {
        if let Some(task) = releasing.remove(&id)
            && let Ok(requests) = task.await
        {
            drains.push(requests);
        }
    }
    let old: Vec<Uuid> = slots
        .iter()
        .filter(|(_, s)| s.task.is_finished() || !desired.contains(&s.member))
        .map(|(id, _)| *id)
        .collect();
    for id in &old {
        slots[id].stop.cancel();
    }
    for id in old {
        if let Some(mut slot) = slots.remove(&id) {
            match timeout(Duration::from_secs(2), &mut slot.task).await {
                Ok(Ok(requests)) => drains.push(requests),
                Ok(Err(_)) => (),
                Err(_) => {
                    releasing.insert(id, slot.task);
                }
            }
        }
    }
    for member in desired {
        if slots.contains_key(&member.account_id) {
            continue;
        }
        if releasing.contains_key(&member.account_id) {
            faults.push((member.account_id, "model_socket_release_pending".to_owned()));
            continue;
        }
        let bound = timeout(
            Duration::from_secs(5),
            listener_for(root, member.account_id, member.peer_uid),
        )
        .await;
        let (listener, guard) = match bound {
            Ok(Ok(bound)) => bound,
            Ok(Err(code)) => {
                faults.push((member.account_id, code));
                continue;
            }
            Err(_) => {
                faults.push((member.account_id, "model_socket_bind_timeout".to_owned()));
                continue;
            }
        };
        let stop = CancellationToken::new();
        let task = tokio::spawn(serve_member(
            listener,
            guard,
            *member,
            state.clone(),
            stop.clone(),
        ));
        slots.insert(
            member.account_id,
            Slot {
                member: *member,
                stop,
                task,
            },
        );
    }
    faults
}

/// `None` is only a genuinely absent file. Any other problem is an error.
fn read_config(path: &Path) -> Result<Option<Zeroizing<Vec<u8>>>, String> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err("model_config_unavailable".into()),
        Ok(_) => (),
    }
    private_bytes(path, MAX_FRAME).map(Some)
}

struct Supervisor<'a> {
    options: &'a Options,
    shared: Shared,
    state: Arc<State>,
    stop: CancellationToken,
    /// Tag of the config bytes behind the current verified broker. A failed
    /// build leaves it empty so it is retried every tick: a key file that is
    /// fixed or arrives late is noticed without a config rewrite or restart.
    /// (A built broker re-reads and re-verifies its key file on every call.)
    seen: Option<[u8; 32]>,
    /// What an idle (broker-less) provider answers.
    configured: bool,
    broker: Option<Arc<Broker>>,
    quota: Option<JoinHandle<()>>,
    /// (config tag, members) of the last successful catalog publication. The
    /// registry replays an identical publication without touching the admin's
    /// stored choice; it is repeated only when this changes.
    published: Option<([u8; 32], Vec<Member>)>,
}
impl Supervisor<'_> {
    /// Fence and drop the current provider generation.
    fn retire(&mut self) {
        self.state.set(Provider::Invalid);
        if let Some(broker) = self.broker.take() {
            broker.stop.cancel();
        }
        // No verified generation: whatever comes next must be built afresh.
        self.seen = None;
        self.published = None;
    }
    fn fail(&mut self, startup: bool, code: String) -> Result<(), String> {
        self.retire();
        self.configured = true;
        if startup { Err(code) } else { Ok(()) }
    }
    /// The provider to answer with when there is no verified broker.
    fn idle(&self) -> Provider {
        if self.configured {
            Provider::Invalid
        } else {
            Provider::Unconfigured
        }
    }
    /// Reload the provider config when its bytes change. A startup failure of
    /// a present config is fatal; later failures leave every socket inert.
    fn load(&mut self, startup: bool) -> Result<(), String> {
        let bytes = match read_config(&self.options.config) {
            Ok(bytes) => bytes,
            Err(code) => {
                self.seen = None;
                return self.fail(startup, code);
            }
        };
        let Some(bytes) = bytes else {
            if self.seen.is_some() || self.broker.is_some() {
                self.retire();
            }
            self.seen = None;
            self.configured = false;
            self.state.set(Provider::Unconfigured);
            return Ok(());
        };
        let tag: [u8; 32] = hmac::sign(&self.shared.key, &bytes)
            .as_ref()
            .try_into()
            .map_err(|_| "model_config_invalid")?;
        if self.seen == Some(tag) {
            return Ok(());
        }
        self.retire();
        let built = parse_config(&bytes).and_then(|config| {
            Broker::build(
                self.options,
                config,
                Some(self.shared.clone()),
                self.stop.child_token(),
            )
        });
        match built {
            Ok(broker) => {
                if broker.config.provider.kind == ProviderKind::OpenRouter
                    && broker.config.subscription.is_none()
                {
                    self.quota = Some(tokio::spawn(broker.clone().quota_worker()));
                }
                self.broker = Some(broker);
                self.seen = Some(tag);
                self.configured = true;
                Ok(())
            }
            // `seen` stays empty: the build is retried on the next tick.
            Err(code) => self.fail(startup, code),
        }
    }
    fn refresh(&mut self, members: &[Member]) {
        let _ = self.load(false);
        let Some(broker) = self.broker.clone() else {
            // Also lifts a membership fence: the idle answer is re-established.
            self.state.set(self.idle());
            return;
        };
        if let (Some(tag), false) = (self.seen, members.is_empty()) {
            let key = (tag, members.to_vec());
            if self.published.as_ref() != Some(&key) {
                let ids: Vec<Uuid> = members.iter().map(|m| m.account_id).collect();
                // Failure keeps the old catalog unusable-by-default (the call
                // path requires a matching published policy) and retries.
                if broker.publish_models_for(&ids).is_ok() {
                    self.published = Some(key);
                }
            }
        }
        self.state.set(Provider::Ready {
            bindings: broker.bindings(members),
            broker,
        });
    }
}

pub(super) async fn serve(
    options: &Options,
    source: Source,
    stop: CancellationToken,
) -> Result<(), String> {
    serve_with(options, source, stop, POLL).await
}
pub(super) async fn serve_with(
    options: &Options,
    source: Source,
    stop: CancellationToken,
    poll: Duration,
) -> Result<(), String> {
    private_dir(&options.socket_dir)?;
    private_dir(&options.registry)?;
    if options.socket_dir.starts_with(&options.registry)
        || options.registry.starts_with(&options.socket_dir)
    {
        return Err("model_private_socket_root_required".into());
    }
    let lock = OpenOptions::new()
        .read(true)
        .open(&options.socket_dir)
        .map_err(|_| "model_ipc_unavailable")?;
    lock.try_lock_exclusive()
        .map_err(|_| "model_broker_already_running")?;
    let shared = Shared {
        key: load_key(&options.fingerprint_key)?,
        ledger: Arc::new(Mutex::new(
            Ledger::open(&options.ledger).map_err(|_| "model_ledger_unavailable")?,
        )),
        slots: Arc::new(Semaphore::new(8)),
    };
    let state = Arc::new(State {
        provider: Mutex::new(Arc::new(Provider::Unconfigured)),
        slots: shared.slots.clone(),
    });
    let mut supervisor = Supervisor {
        options,
        shared,
        state: state.clone(),
        stop: stop.clone(),
        seen: None,
        configured: false,
        broker: None,
        quota: None,
        published: None,
    };
    // A present but invalid provider config must not leave sockets serving.
    supervisor.load(true)?;
    let mut slots: HashMap<Uuid, Slot> = HashMap::new();
    let mut releasing = Releasing::new();
    let mut drains: Vec<JoinSet<()>> = Vec::new();
    let mut known: BTreeMap<Uuid, String> = BTreeMap::new();
    let mut membership_ok = true;
    let mut ticker = interval(poll);
    ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
    // Only lock, ledger and fingerprint-key failures above are fatal. Everything
    // below is per-member or retried on the next tick.
    let result: Result<(), String> = loop {
        tokio::select! {
            _ = stop.cancelled() => break Ok(()),
            _ = ticker.tick() => {
                let source = source.clone();
                reap(&mut drains);
                let listed = tokio::task::spawn_blocking(move || source())
                    .await
                    .map_err(|_| "model_enrollment_unavailable".to_owned())
                    .and_then(|listed| listed)
                    .and_then(usable);
                let (members, mut faults) = match listed {
                    Ok(listed) => listed,
                    Err(code) => {
                        // Membership authority cannot be checked: never keep
                        // serving the cached member list. Existing sockets stay
                        // bound (inert) and the source is retried next tick.
                        state.set(Provider::Fenced);
                        if membership_ok {
                            eprintln!("model_membership_fenced:{code}");
                        }
                        membership_ok = false;
                        continue;
                    }
                };
                membership_ok = true;
                // Sockets first so containers can mount them before any provider
                // exists; then the provider and catalog for exactly these members.
                faults.extend(
                    reconcile(&options.socket_dir, &state, &mut slots, &mut releasing, &mut drains, &members).await,
                );
                supervisor.refresh(&members);
                let now: BTreeMap<Uuid, String> = faults.into_iter().collect();
                for (id, code) in &now {
                    if known.get(id) != Some(code) {
                        eprintln!("model_member_fault:{id}:{code}");
                    }
                }
                known = now;
            }
        }
    };
    stop.cancel();
    supervisor.retire();
    for slot in slots.values() {
        slot.stop.cancel();
    }
    for (_, slot) in slots {
        if let Ok(requests) = slot.task.await {
            drains.push(requests);
        }
    }
    for (_, task) in releasing {
        if let Ok(requests) = task.await {
            drains.push(requests);
        }
    }
    if let Some(quota) = supervisor.quota.take() {
        let _ = quota.await;
    }
    // Keep the ledger and the process lock until admitted calls have settled.
    for mut requests in drains {
        while requests.join_next().await.is_some() {}
    }
    drop(lock);
    result
}
