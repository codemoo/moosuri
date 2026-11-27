//! Explicit administrator-owned disconnect of the managed subscription.
//!
//! Crash ordering (every step is resumable from the one operation receipt and
//! the actual config; nothing is inferred from a poll):
//!  1. reserve: global job slot + durable non-secret receipt
//!     (`Disconnecting`, no intent yet). No config is touched.
//!  2. under a FRESH admin pin and the registry lock: re-read the exact config;
//!     it must name the target revision and bind exactly this full owner.
//!     Persist the intent (SHA-256 of that exact config) in the receipt, then
//!     unlink the config and fsync the directory.
//!  3. still under the same lock, bounded local root IO only:
//!     `host::begin_retirement(owner, target)` (no service command, no wait).
//!  4. lock released: `host::retire_request(owner, target)` does the bounded
//!     cleanup; only its acknowledgment AND a durable terminal receipt end it.
//!
//! The target (the OLD connection revision) is the only request id the host ever
//! sees; the new operation id is just the receipt/job identity. Before every
//! transition the target config is read again: still published and exact ->
//! finish the unpublish; absent or superseded -> confirm the directory is
//! durable and begin retirement (the host verifies owner and request itself);
//! unreadable, foreign or changed since the intent -> hold, nothing destructive.
use super::*;

#[derive(Debug, PartialEq, Eq)]
pub(super) enum Unpub {
    /// The target is durably unpublished and the root record is in cleanup.
    Begun,
    /// The config is not exactly the owned target: no destructive step ran.
    Hold,
}
/// `intent` is set whenever THIS call persisted the intent digest, even when a
/// later step failed, so the live job can mirror what the receipt now says.
pub(super) struct Report {
    pub intent: Option<String>,
    pub result: io::Result<Unpub>,
}

fn same_owner(r: &Receipt, o: &Owner) -> bool {
    r.account_id == o.account_id
        && r.tenant_id == o.tenant_id
        && r.volume_id == o.volume_id
        && r.generation == o.generation
}
/// Binds the operation id to the full owner and the exact target revision.
pub(super) fn digest_of(a: &Allocation, target: Uuid) -> String {
    hash(&format!(
        "claude-subscription-disconnect-v1\n{}\n{}\n{}\n{}\n{target}",
        a.account_id, a.tenant_id, a.volume_id, a.generation
    ))
}
/// The config is the single subscription binding of exactly this owner at the
/// target revision. Returns its model and effort for the non-secret receipt.
pub(super) fn binding(
    config: &[u8],
    owner: &Owner,
    target: Uuid,
) -> Option<(String, Option<String>)> {
    if revision_of(config)? != target {
        return None;
    }
    let v: Value = serde_json::from_slice(config).ok()?;
    if v["subscription"] != "claude_code" {
        return None;
    }
    let bindings = v["bindings"].as_array().filter(|b| b.len() == 1)?;
    let id = |key: &str, want: Uuid| {
        bindings[0][key]
            .as_str()
            .and_then(|s| s.parse::<Uuid>().ok())
            == Some(want)
    };
    (id("account_id", owner.account_id)
        && id("tenant_id", owner.tenant_id)
        && id("volume_id", owner.volume_id)
        && bindings[0]["generation"].as_i64() == Some(owner.generation))
    .then(|| {
        (
            v["model"].as_str().unwrap_or_default().to_owned(),
            v["effort"].as_str().map(str::to_owned),
        )
    })
    .filter(|(model, _)| !model.is_empty())
}

/// Persist the exact-config digest before anything is unlinked. Receipt lock
/// order is registry lock -> receipt lock; receipt IO never takes the registry.
fn record_intent(d: &File, r: &Receipt, sha: &str) -> io::Result<()> {
    let _guard = receipt::LOCK
        .lock()
        .map_err(|_| io::Error::from(io::ErrorKind::Other))?;
    match receipt::read_receipt(d)? {
        Some(old)
            if old.request_id == r.request_id
                && old.mode == Mode::Disconnect
                && old.target_revision == r.target_revision
                && !old.state.terminal()
                && old.config_sha256.as_deref().is_none_or(|old| old == sha) =>
        {
            ()
        }
        _ => return Err(io::ErrorKind::AlreadyExists.into()),
    }
    let mut next = r.clone();
    next.config_sha256 = Some(sha.to_owned());
    next.updated = now();
    receipt::write_receipt(d, &next)
}

/// One registry-locked transition: intent, durable unpublish, root begin. The
/// caller holds a fresh admin pin and the registry lock. `begin` is
/// `host::begin_retirement(owner, target)` (injected so it is testable).
pub(super) fn unpublish(
    d: &File,
    owner: &Owner,
    r: &Receipt,
    begin: impl FnOnce() -> Result<(), String>,
) -> Report {
    let mut intent = None;
    let result = (|| -> io::Result<Unpub> {
        let target = match (r.mode, r.target_revision) {
            (Mode::Disconnect, Some(t)) if same_owner(r, owner) => t,
            _ => return Err(io::ErrorKind::InvalidInput.into()),
        };
        // An unreadable config is an error here, never "absent".
        if let Some(bytes) = read(d, CONFIG_FILE)? {
            match revision_of(&bytes) {
                None => return Ok(Unpub::Hold),
                Some(revision) if revision == target => {
                    if binding(&bytes, owner, target).is_none() {
                        return Ok(Unpub::Hold);
                    }
                    let sha = receipt::sha(&bytes);
                    match r.config_sha256.as_deref() {
                        Some(previous) if previous != sha => return Ok(Unpub::Hold),
                        Some(_) => (),
                        None => {
                            record_intent(d, r, &sha)?;
                            intent = Some(sha);
                        }
                    }
                    match unlink(d, CONFIG_FILE) {
                        Ok(()) => (),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                        Err(e) => return Err(e),
                    }
                }
                // Superseded by another revision: not ours to touch.
                Some(_) => (),
            }
        }
        // Reservation alone did not prove the exact config under the registry
        // lock. Absence/supersession cannot manufacture that durable intent.
        if r.config_sha256.is_none() && intent.is_none() {
            return Ok(Unpub::Hold);
        }
        // Removal (or absence) must be durable before the root record moves.
        d.sync_all()?;
        begin().map_err(|_| io::Error::other("begin_retirement"))?;
        Ok(Unpub::Begun)
    })();
    Report { intent, result }
}

#[derive(PartialEq, Eq)]
enum Pass {
    Done,
    Retry,
    Hold,
}
async fn step(g: &Gateway, id: Uuid) -> Pass {
    let Some((a, session, r)) = with_job(g, id, |j| (j.session(), j.session.clone(), j.snapshot()))
    else {
        return Pass::Hold;
    };
    let (Some(owner), Some(target)) = (owner_of(&a.allocation), r.target_revision) else {
        return Pass::Hold;
    };
    let Ok(admin) = admin_for(g, &a, &session).await else {
        return Pass::Retry;
    };
    let report = local_with(g, admin, move |d| {
        Ok(unpublish(d, &owner, &r, || {
            host::begin_retirement(owner, target)
        }))
    })
    .await;
    let Ok(report) = report else {
        return Pass::Retry;
    };
    if let Some(sha) = report.intent {
        with_job(g, id, |j| j.config_sha256 = Some(sha));
    }
    match report.result {
        Ok(Unpub::Begun) => (),
        Ok(Unpub::Hold) => return Pass::Hold,
        Err(_) => return Pass::Retry,
    }
    // Withdraw the exact cached public catalog too. Its historical revision
    // fences late publication by a broker that read the old config. Otherwise
    // reloading the UI would still claim this removed subscription is present.
    let Ok(admin) = admin_for(g, &a, &session).await else {
        return Pass::Retry;
    };
    let root = g.registry.clone();
    if !matches!(
        tokio::task::spawn_blocking(move || {
            moosuri_store::runtime_registry::engine::retire_published_models(&root, &admin, target)
        })
        .await,
        Ok(Ok(()))
    ) {
        return Pass::Retry;
    }
    // Registry lock released. The bounded host cleanup is awaited to completion.
    match host::retire_request(owner, target).await {
        Ok(()) => Pass::Done,
        Err(_) => Pass::Retry,
    }
}
/// Runs (or resumes) the disconnect of the job `id`. A failure keeps the job
/// `CleanupPending` (slot held, receipt non-terminal); an authenticated session
/// of the exact owner resumes it with the existing cancel/cleanup request.
pub(super) async fn drive(g: Gateway, id: Uuid) {
    for attempt in 1..=3u64 {
        match step(&g, id).await {
            Pass::Done => {
                finish(&g, id, Some(Stage::Disconnected)).await;
                return;
            }
            Pass::Hold => break,
            Pass::Retry => sleep(Duration::from_secs(2 * attempt)).await,
        }
    }
    with_job(&g, id, |j| {
        j.stage = Stage::CleanupPending;
        j.retiring = false;
    });
    persist(&g, id).await;
}

impl Job {
    pub(super) fn disconnect(
        id: Uuid,
        a: &SessionAllocation,
        session: &str,
        digest: String,
        target: Uuid,
        (model, effort): (String, Option<String>),
    ) -> Self {
        let mut job = Self::new(
            id,
            a,
            session,
            digest,
            None,
            None,
            model,
            effort,
            mpsc::channel(1).0,
            CancellationToken::new(),
        );
        job.code = None;
        job.mode = Mode::Disconnect;
        job.target = Some(target);
        job.stage = Stage::Disconnecting;
        job.after = Stage::Disconnected;
        // The drive task is started by the request that reserved it.
        job.retiring = true;
        job
    }
}

/// Everything before the first config mutation: validation, replay, slot and
/// durable receipt. `Ok((view, true))` means a fresh reservation (start `drive`).
pub(super) async fn reserve(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    input: Input,
) -> Result<(Value, bool), StatusCode> {
    let id = input.request_id;
    let target = input.replace_revision.ok_or(StatusCode::BAD_REQUEST)?;
    if target.is_nil() || target == id {
        return Err(StatusCode::BAD_REQUEST);
    }
    let owner = owner_of(&a.allocation).ok_or(UNAVAILABLE)?;
    let digest = digest_of(&a.allocation, target);
    {
        let mut jobs = lock(g)?;
        prune(&mut jobs);
        if let Some(j) = jobs.iter().find(|j| j.id == id) {
            if !j.owned_by(a, session) || j.digest != digest {
                return Err(StatusCode::CONFLICT);
            }
            return Ok((wrap(j.view(true)), false));
        }
        if jobs.iter().any(|j| !j.stage.terminal()) {
            return Err(StatusCode::CONFLICT);
        }
    }
    let seen = stored(g).await?;
    if let Some(r) = seen.receipt.as_ref() {
        if r.request_id == id {
            if r.owner_is(&a.allocation) && r.digest == digest {
                return Ok((wrap(r.view(seen.relation)), false));
            }
            return Err(StatusCode::CONFLICT);
        }
        // Never overwrite another unresolved operation.
        if r.unresolved(seen.relation) {
            return Err(StatusCode::CONFLICT);
        }
    }
    // Advisory pre-check; the authoritative one runs under the registry lock.
    let config = receipts(g, |d| read(d, CONFIG_FILE)).await?;
    let described = config
        .as_deref()
        .and_then(|c| binding(c, &owner, target))
        .ok_or(StatusCode::CONFLICT)?;
    let job = Job::disconnect(id, a, session, digest, target, described);
    let first = job.snapshot();
    let view = {
        let mut jobs = lock(g)?;
        prune(&mut jobs);
        if jobs.iter().any(|j| !j.stage.terminal()) {
            return Err(StatusCode::CONFLICT);
        }
        while jobs.len() >= MAX_JOBS {
            jobs.remove(0);
        }
        let view = job.view(true);
        jobs.push(job);
        view
    };
    if let Err(e) = receipts_io(g, move |d| receipt::begin(d, &first)).await {
        lock(g)?.retain(|j| j.id != id);
        return Err(if e.kind() == io::ErrorKind::AlreadyExists {
            StatusCode::CONFLICT
        } else {
            UNAVAILABLE
        });
    }
    Ok((wrap(view), true))
}
pub(super) async fn disconnect(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    input: Input,
) -> Result<Value, StatusCode> {
    let id = input.request_id;
    let (view, fresh) = reserve(g, a, session, input).await?;
    if fresh {
        tokio::spawn(drive(g.clone(), id));
    }
    Ok(view)
}
