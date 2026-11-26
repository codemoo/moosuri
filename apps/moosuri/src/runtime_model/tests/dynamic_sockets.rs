//! Lifecycle-derived dynamic member sockets, with synthetic owners only. The
//! production source (private lifecycle enrollment joined with the registry) is
//! covered in `kiri_lifecycle::onboard`; here a mutable member list stands in
//! for it so enrollment, fencing and provider reloads can be driven in place.
use super::*;
use std::sync::atomic::AtomicBool;

type Members = Arc<Mutex<Vec<Member>>>;
const TICK: Duration = Duration::from_millis(40);

fn member(b: &Binding) -> Member {
    Member {
        account_id: b.account_id,
        tenant_id: b.tenant_id,
        volume_id: b.volume_id,
        generation: b.generation,
        peer_uid: b.peer_uid,
    }
}
fn start(
    options: &Options,
    members: &Members,
) -> (
    CancellationToken,
    tokio::task::JoinHandle<Result<(), String>>,
) {
    let list = members.clone();
    start_with(options, Arc::new(move || Ok(list.lock().unwrap().clone())))
}
fn start_with(
    options: &Options,
    source: dynamic::Source,
) -> (
    CancellationToken,
    tokio::task::JoinHandle<Result<(), String>>,
) {
    let stop = CancellationToken::new();
    let (o, s) = (options.clone_for_test(), stop.clone());
    let task = tokio::spawn(async move { dynamic::serve_with(&o, source, s, TICK).await });
    (stop, task)
}
macro_rules! eventually {
    ($condition:expr) => {{
        let mut reached = false;
        for _ in 0..240 {
            if $condition {
                reached = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
        assert!(reached, "not reached: {}", stringify!($condition));
    }};
}
/// Atomic owner-only replacement, as an administrator write would be.
fn replace(path: &Path, bytes: &[u8]) {
    let temp = path.with_extension("next");
    file(&temp, bytes);
    fs::rename(&temp, path).unwrap();
}
async fn state(path: &Path, r: &Request) -> String {
    match personal_call(path, &serde_json::to_vec(r).unwrap()).await {
        Ok(bytes) => serde_json::from_slice::<Reply>(&bytes)
            .map(|r| r.state)
            .unwrap_or_else(|_| "reply_invalid".into()),
        Err(error) => error,
    }
}
async fn selection(path: &Path, options: &Options, b: &Binding) -> Reply {
    let query = serde_json::json!({
        "v": 1, "operation": "selection", "generation": b.generation,
        "selection_epoch": epoch(&options.registry, b).unwrap(),
    });
    let mut stream = UnixStream::connect(path).await.unwrap();
    stream
        .write_all(&serde_json::to_vec(&query).unwrap())
        .await
        .unwrap();
    stream.write_all(b"\n").await.unwrap();
    serde_json::from_slice(
        &frame(&mut stream, MAX_REPLY, Duration::from_secs(2))
            .await
            .unwrap(),
    )
    .unwrap()
}
fn dummy_url() -> String {
    "http://127.0.0.1:65531/v1".into()
}

#[tokio::test]
async fn empty_unconfigured_installation_provisions_inert_sockets_as_members_arrive() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (options, b) = fixture(&root, dummy_url());
    // A clean installation has no provider file at all.
    fs::remove_file(&options.config).unwrap();
    let members = Members::default();
    let (stop, task) = start(&options, &members);
    tokio::time::sleep(TICK * 5).await;
    assert!(!task.is_finished());
    assert_eq!(fs::read_dir(&options.socket_dir).unwrap().count(), 0);

    // First new member: its inert socket appears without a restart.
    members.lock().unwrap().push(member(&b[0]));
    let first = path(&options, &b[0]);
    listening(&first).await;
    let meta = fs::symlink_metadata(&first).unwrap();
    assert!(meta.file_type().is_socket());
    assert_eq!(meta.mode() & 0o777, 0o600);
    assert_eq!(meta.uid(), b[0].peer_uid);
    assert_eq!(
        fs::metadata(first.parent().unwrap()).unwrap().mode() & 0o777,
        0o711
    );
    assert_eq!(state(&first, &request(&b[0])).await, "model_unconfigured");
    let reply = selection(&first, &options, &b[0]).await;
    assert_eq!(
        (reply.v, reply.state.as_str()),
        (1, "model_selection_unavailable")
    );
    assert!(reply.model.is_none() && reply.connection_revision.is_none());

    // A later member appears the same way; the first keeps serving.
    members.lock().unwrap().push(member(&b[1]));
    let later = path(&options, &b[1]);
    listening(&later).await;
    assert_eq!(state(&later, &request(&b[1])).await, "model_unconfigured");
    assert_eq!(state(&first, &request(&b[0])).await, "model_unconfigured");

    stop.cancel();
    task.await.unwrap().unwrap();
    assert!(!first.exists() && !later.exists());
    assert!(first.parent().unwrap().exists());
}

#[tokio::test]
async fn absent_config_is_inert_while_malformed_config_fails_closed() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    let valid = fs::read(&options.config).unwrap();
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let socket = path(&options, &b[0]);

    // At startup an existing config that is malformed, has unknown fields or is
    // not owner-only is an error before any socket exists. Never a fallback.
    for (bytes, mode, expected) in [
        (&b"{not json"[..], 0o600, "model_config_invalid"),
        (
            &br#"{"version":1,"surprise":true}"#[..],
            0o600,
            "model_config_invalid",
        ),
        (&valid[..], 0o644, "private_file_invalid"),
    ] {
        file(&options.config, bytes);
        fs::set_permissions(&options.config, fs::Permissions::from_mode(mode)).unwrap();
        let (_stop, task) = start(&options, &members);
        assert_eq!(task.await.unwrap().unwrap_err(), expected);
        assert!(!socket.exists());
    }

    // Genuinely absent: inert, explicit, and nothing reaches the provider.
    fs::remove_file(&options.config).unwrap();
    let (stop, task) = start(&options, &members);
    listening(&socket).await;
    assert_eq!(state(&socket, &request(&b[0])).await, "model_unconfigured");

    // Verified config written atomically: attached without restart.
    replace(&options.config, &valid);
    eventually!(state(&socket, &request(&b[0])).await == "settled");
    assert_eq!(provider.count.load(Ordering::SeqCst), 1);

    // A malformed replacement after that is not ignored and not a fallback to
    // the previous provider.
    replace(&options.config, b"{not json");
    // Poll with the non-admitting selection query so the old provider cannot
    // consume a call while the reload is pending.
    eventually!(selection(&socket, &options, &b[0]).await.state == "model_selection_unavailable");
    assert_eq!(
        state(&socket, &request(&b[0])).await,
        "model_config_invalid"
    );
    // Removing the file is the explicit unconfigured state again; a valid
    // rewrite recovers.
    fs::remove_file(&options.config).unwrap();
    eventually!(state(&socket, &request(&b[0])).await == "model_unconfigured");
    replace(&options.config, &valid);
    eventually!(state(&socket, &request(&b[0])).await == "settled");
    assert_eq!(provider.count.load(Ordering::SeqCst), 2);
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn first_and_later_members_are_published_without_restart() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let (stop, task) = start(&options, &members);
    let first = path(&options, &b[0]);
    listening(&first).await;
    eventually!(selection(&first, &options, &b[0]).await.state == "selection");
    assert_eq!(state(&first, &request(&b[0])).await, "settled");

    // The second member is in neither the provider file's bindings nor the
    // registry catalog until the supervisor sees it.
    let later = path(&options, &b[1]);
    assert!(!later.exists());
    members.lock().unwrap().push(member(&b[1]));
    listening(&later).await;
    eventually!(selection(&later, &options, &b[1]).await.state == "selection");
    let reply = selection(&later, &options, &b[1]).await;
    assert_eq!(reply.model.as_deref(), Some("synthetic-model"));
    // No explicit entry: the bounded default budget, whose reservation fits.
    assert_eq!(state(&later, &request(&b[1])).await, "settled");
    assert_eq!(provider.count.load(Ordering::SeqCst), 2);
    // Calls are owner-bound: the other member's account is never accepted.
    let mut forged = serde_json::to_value(request(&b[1])).unwrap();
    forged["account_id"] = serde_json::json!(b[0].account_id);
    assert!(
        personal_call(&later, &serde_json::to_vec(&forged).unwrap())
            .await
            .is_err()
    );
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn provider_reload_keeps_the_administrator_choice_for_current_and_new_members() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    let choice = ModelChoice {
        model: "synthetic-alternative".into(),
        effort: None,
    };
    let mut config: Config = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    config.models = vec![choice.clone()];
    file(&options.config, &serde_json::to_vec(&config).unwrap());
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let (stop, task) = start(&options, &members);
    let first = path(&options, &b[0]);
    listening(&first).await;
    eventually!(selection(&first, &options, &b[0]).await.state == "selection");
    assert_eq!(
        selection(&first, &options, &b[0]).await.model.as_deref(),
        Some("synthetic-model")
    );

    let operator = operator(&options, &b[0], "a@example.invalid");
    engine::change(
        &options.registry,
        &operator,
        engine::InstallationChange {
            revision: settings_revision(&options, &operator),
            connection_revision: config.connection_revision,
            enabled: true,
            default_choice: Some(choice.clone()),
        },
    )
    .unwrap();
    let revision = settings_revision(&options, &operator);
    // Many polls with unchanged inputs never republish or clobber the choice.
    tokio::time::sleep(TICK * 10).await;
    assert_eq!(settings_revision(&options, &operator), revision);
    assert_eq!(
        selection(&first, &options, &b[0]).await.model.as_deref(),
        Some(choice.model.as_str())
    );

    // Verified config reload (same connection, new default budget): live
    // immediately, and the administrator's choice survives it.
    config.default_budget = Some(Budget {
        max_calls: 1,
        max_tokens: 8192,
        reservation_tokens: 2048,
    });
    replace(&options.config, &serde_json::to_vec(&config).unwrap());
    members.lock().unwrap().push(member(&b[1]));
    let later = path(&options, &b[1]);
    listening(&later).await;
    eventually!(selection(&later, &options, &b[1]).await.state == "selection");
    for binding in [&b[0], &b[1]] {
        let socket = path(&options, binding);
        assert_eq!(
            selection(&socket, &options, binding).await.model.as_deref(),
            Some(choice.model.as_str())
        );
    }
    let mut call = request(&b[1]);
    call.model = choice.model.clone();
    assert_eq!(state(&later, &call).await, "settled");
    let mut call = request(&b[1]);
    call.model = choice.model.clone();
    assert_eq!(state(&later, &call).await, "quota_exhausted");
    assert_eq!(provider.alternative_calls.load(Ordering::SeqCst), 1);
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn generation_state_and_membership_changes_fence_the_old_socket_and_calls() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, _provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    // No per-member supplement: a stale supplement is itself fail-closed
    // (`model_binding_invalid`, covered by the binding test below).
    let mut config: Config = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    config.bindings.clear();
    file(&options.config, &serde_json::to_vec(&config).unwrap());
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let (stop, task) = start(&options, &members);
    let socket = path(&options, &b[0]);
    listening(&socket).await;
    eventually!(state(&socket, &request(&b[0])).await == "settled");

    // A new generation replaces the socket: the old generation's calls are no
    // longer accepted, and the new owner cannot use the old registry generation.
    let mut next = member(&b[0]);
    next.generation += 1;
    *members.lock().unwrap() = vec![next];
    eventually!(state(&socket, &request(&b[0])).await == "request_invalid");
    let mut future = request(&b[0]);
    future.generation = next.generation;
    assert_eq!(state(&socket, &future).await, "account_fenced");

    // Back to the registry's generation, then fence its route: same socket,
    // but no fresh admission.
    *members.lock().unwrap() = vec![member(&b[0])];
    eventually!(state(&socket, &request(&b[0])).await == "settled");
    let allocation =
        runtime_registry::lookup_allocated_id(&options.registry, b[0].account_id).unwrap();
    runtime_registry::set_routing(&options.registry, &allocation, false).unwrap();
    assert_eq!(state(&socket, &request(&b[0])).await, "account_fenced");

    // Leaving the enrollment removes the socket itself.
    members.lock().unwrap().clear();
    eventually!(!socket.exists());
    assert!(UnixStream::connect(&socket).await.is_err());
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn a_member_without_a_worker_never_borrows_another_owners_subscription() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (options, b) = fixture(&root, dummy_url());
    let worker_uid = unsafe { libc::geteuid() } + 1000;
    let mut config: Config = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    config.key_file = None;
    config.subscription = Some(Route::ChatgptPlan);
    config.provider = ProviderConfig {
        kind: ProviderKind::OpenAi,
        base_url: None,
        allowed_ips: vec![],
        router_providers: vec![],
        zero_data_retention: false,
    };
    let mut enrolled = b[0].clone();
    enrolled.worker = Some(Endpoint {
        socket: root.join("w").join("engine.sock"),
        worker_uid,
    });
    config.bindings = vec![enrolled];
    let shared = Shared {
        key: load_key(&options.fingerprint_key).unwrap(),
        ledger: Arc::new(Mutex::new(Ledger::open(&options.ledger).unwrap())),
        slots: Arc::new(Semaphore::new(8)),
    };
    let broker = Broker::build(&options, config, Some(shared), CancellationToken::new()).unwrap();
    let (a, c) = (member(&b[0]), member(&b[1]));
    let bound = broker.bindings(&[a, c]);
    let with = bound[&a.account_id].as_ref().unwrap();
    assert_eq!(with.worker.as_ref().unwrap().worker_uid, worker_uid);
    assert_eq!(with.budget.max_calls, b[0].budget.max_calls);
    let without = bound[&c.account_id].as_ref().unwrap();
    assert!(without.worker.is_none());
    assert_eq!(without.budget.max_calls, DEFAULT_BUDGET.max_calls);
    let reply = broker.call(without, request(without)).await;
    assert_eq!(reply.state, "model_worker_unavailable");

    // A supplement for a different owner identity, or a worker UID that is
    // another member's UID, fails that member closed.
    let invalid = |bound: &BTreeMap<Uuid, Result<Binding, &'static str>>, id| {
        bound[&id].as_ref().err().copied()
    };
    let mut stale = a;
    stale.generation += 1;
    assert_eq!(
        invalid(&broker.bindings(&[stale]), a.account_id),
        Some("model_binding_invalid")
    );
    let mut collision = c;
    collision.peer_uid = worker_uid;
    assert_eq!(
        invalid(&broker.bindings(&[a, collision]), a.account_id),
        Some("model_binding_invalid")
    );
}

#[tokio::test]
async fn a_failed_provider_build_is_retried_when_the_missing_key_arrives() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    let mut config: Config = serde_json::from_slice(&fs::read(&options.config).unwrap()).unwrap();
    let late = root.join("late.key");
    config.key_file = Some(late.clone());
    fs::remove_file(&options.config).unwrap();
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let (stop, task) = start(&options, &members);
    let socket = path(&options, &b[0]);
    listening(&socket).await;
    assert_eq!(state(&socket, &request(&b[0])).await, "model_unconfigured");

    // The config is valid but its key is not there yet: fail closed, keep
    // retrying with the very same config bytes.
    replace(&options.config, &serde_json::to_vec(&config).unwrap());
    eventually!(state(&socket, &request(&b[0])).await == "model_config_invalid");
    tokio::time::sleep(TICK * 4).await;
    assert_eq!(
        state(&socket, &request(&b[0])).await,
        "model_config_invalid"
    );
    assert!(!task.is_finished());

    // A key that arrives with the wrong mode is still refused...
    file(&late, b"SYNTHETIC-MODEL-KEY");
    fs::set_permissions(&late, fs::Permissions::from_mode(0o644)).unwrap();
    tokio::time::sleep(TICK * 4).await;
    assert_eq!(
        state(&socket, &request(&b[0])).await,
        "model_config_invalid"
    );
    // ...and the corrected key recovers without a config rewrite or restart.
    fs::set_permissions(&late, fs::Permissions::from_mode(0o600)).unwrap();
    eventually!(state(&socket, &request(&b[0])).await == "settled");
    assert_eq!(provider.count.load(Ordering::SeqCst), 1);
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn unavailable_membership_fences_the_cached_members_then_recovers_without_a_restart() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let failing = Arc::new(AtomicBool::new(false));
    let source: dynamic::Source = {
        let (list, failing) = (members.clone(), failing.clone());
        Arc::new(move || {
            if failing.load(Ordering::SeqCst) {
                Err("synthetic_membership_unavailable".into())
            } else {
                Ok(list.lock().unwrap().clone())
            }
        })
    };
    let (stop, task) = start_with(&options, source);
    let socket = path(&options, &b[0]);
    listening(&socket).await;
    eventually!(state(&socket, &request(&b[0])).await == "settled");

    // Poll with the non-admitting selection query so no call is consumed
    // before the fence is in place.
    failing.store(true, Ordering::SeqCst);
    eventually!(selection(&socket, &options, &b[0]).await.state == "model_selection_unavailable");
    assert_eq!(
        state(&socket, &request(&b[0])).await,
        "model_membership_unavailable"
    );
    assert_eq!(
        selection(&socket, &options, &b[0]).await.state,
        "model_selection_unavailable"
    );
    // Nothing is terminated: the inert socket stays and the source is retried.
    tokio::time::sleep(TICK * 4).await;
    assert!(!task.is_finished());
    assert!(UnixStream::connect(&socket).await.is_ok());
    assert_eq!(provider.count.load(Ordering::SeqCst), 1);

    failing.store(false, Ordering::SeqCst);
    eventually!(state(&socket, &request(&b[0])).await == "settled");
    assert_eq!(provider.count.load(Ordering::SeqCst), 2);
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn one_members_bad_socket_never_stops_the_healthy_members() {
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (url, provider, upstream) = upstream().await;
    let (options, b) = fixture(&root, url);
    // Member two has a foreign, non-socket file where its socket must go.
    let leaf = options.socket_dir.join(b[1].account_id.to_string());
    fs::create_dir(&leaf).unwrap();
    fs::set_permissions(&leaf, fs::Permissions::from_mode(0o711)).unwrap();
    let bad = leaf.join("model.sock");
    file(&bad, b"not a socket");
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0]), member(&b[1])]));
    let (stop, task) = start(&options, &members);
    let healthy = path(&options, &b[0]);
    listening(&healthy).await;
    eventually!(state(&healthy, &request(&b[0])).await == "settled");
    tokio::time::sleep(TICK * 5).await;
    assert!(!task.is_finished());
    assert_eq!(fs::read(&bad).unwrap(), b"not a socket");
    assert_eq!(state(&healthy, &request(&b[0])).await, "settled");

    // Once the operator removes the foreign file the same service binds it.
    fs::remove_file(&bad).unwrap();
    listening(&bad).await;
    eventually!(state(&bad, &request(&b[1])).await == "settled");
    assert_eq!(provider.count.load(Ordering::SeqCst), 3);
    stop.cancel();
    task.await.unwrap().unwrap();
    upstream.abort();
}

#[tokio::test]
async fn the_broker_serves_the_gateway_written_config_beside_crash_leftovers() {
    use crate::personal_gateway::model_setup::seam::{Crash, crash_at, save_for_test};
    let root = tempfile::tempdir().unwrap();
    let root = root.path().canonicalize().unwrap();
    let (mut options, b) = fixture(&root, dummy_url());
    // The private model state directory is outside the strict registry.
    let setup = dir(&root, "m");
    crash_at(&setup, Uuid::new_v4(), b"orphan-key-one", Crash::KeyOnly);
    crash_at(
        &setup,
        Uuid::new_v4(),
        b"orphan-key-two",
        Crash::PendingOnly,
    );
    let revision = Uuid::new_v4();
    save_for_test(&setup, revision, "synthetic-key-material", None);
    options.config = setup.join("runtime-model.json");
    let members: Members = Arc::new(Mutex::new(vec![member(&b[0])]));
    let (stop, task) = start(&options, &members);
    let socket = path(&options, &b[0]);
    listening(&socket).await;
    eventually!(selection(&socket, &options, &b[0]).await.state == "selection");
    let reply = selection(&socket, &options, &b[0]).await;
    assert_eq!(reply.model.as_deref(), Some("claude-synthetic"));
    assert_eq!(reply.effort.as_deref(), Some("medium"));
    assert_eq!(reply.connection_revision, Some(revision));
    assert!(matches!(reply.kind, Some(ProviderKind::Anthropic)));
    // A CAS replacement (new key and revision) never stops the service.
    save_for_test(
        &setup,
        Uuid::new_v4(),
        "synthetic-key-replacement",
        Some(revision),
    );
    tokio::time::sleep(TICK * 8).await;
    assert!(!task.is_finished());
    assert!(UnixStream::connect(&socket).await.is_ok());
    stop.cancel();
    task.await.unwrap().unwrap();
}
