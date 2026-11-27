//! Explicit administrator-owned disconnect of the managed subscription.
//!
//! Crash ordering (every step is resumable from the one operation receipt and
//! the actual config; nothing is inferred from a poll):
//!  1. reserve: global job slot + durable non-secret receipt
//!     (`Disconnecting`, no intent yet). No config is touched.
//!  2. under a FRESH admin pin and the registry lock: re-read the exact owned
//!     managed config and acquire its checked root retirement lease. Missing,
//!     foreign or ineligible host records cannot unpublish the connection.
//!     Persist the intent, unlink the config and fsync the directory.
//!  3. still holding the registry and root leases: persist the root cleanup
//!     phase (bounded local IO only; no service command or wait).
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
use crate::engine_login::Endpoint;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Management {
    Managed,
    External,
}

/// Classify only a valid single-owner subscription endpoint. A malformed
/// spelling in the managed namespace is never treated as an external login.
pub(super) fn management(config: &[u8], owner: &Owner, target: Uuid) -> Option<Management> {
    binding(config, owner, target)?;
    let value: Value = serde_json::from_slice(config).ok()?;
    let endpoint: Endpoint = serde_json::from_value(value["bindings"][0]["worker"].clone()).ok()?;
    endpoint.validate().ok()?;
    let socket = endpoint.socket.to_str()?;
    if endpoint.socket == host::managed_socket(endpoint.worker_uid) {
        Some(Management::Managed)
    } else if socket.starts_with("/run/moosuri-claude-worker-") {
        None
    } else {
        Some(Management::External)
    }
}

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
/// caller holds a fresh admin pin and the registry lock. `prepare` acquires the
/// checked root lease before mutation and returns the post-unpublish transition.
pub(super) fn unpublish<F: FnOnce() -> Result<(), String>>(
    d: &File,
    owner: &Owner,
    r: &Receipt,
    prepare: impl FnOnce(Option<u32>) -> Result<F, String>,
) -> Report {
    let mut intent = None;
    let result = (|| -> io::Result<Unpub> {
        let target = match (r.mode, r.target_revision) {
            (Mode::Disconnect, Some(t)) if same_owner(r, owner) => t,
            _ => return Err(io::ErrorKind::InvalidInput.into()),
        };
        // Validate the exact config before acquiring its root retirement lease.
        // A legacy external connection must remain published until replacement.
        let current = read(d, CONFIG_FILE)?;
        if let Some(bytes) = current.as_deref() {
            if revision_of(bytes) == Some(target)
                && management(bytes, owner, target) != Some(Management::Managed)
            {
                return Ok(Unpub::Hold);
            }
        }
        if r.config_sha256.is_none()
            && current.as_deref().and_then(|bytes| revision_of(bytes)) != Some(target)
        {
            return Ok(Unpub::Hold);
        }
        let worker = current
            .as_deref()
            .filter(|bytes| revision_of(bytes) == Some(target))
            .and_then(|bytes| serde_json::from_slice::<Value>(bytes).ok())
            .and_then(|v| v["bindings"][0]["worker"]["worker_uid"].as_u64())
            .and_then(|uid| u32::try_from(uid).ok());
        let begin = match prepare(worker) {
            Ok(begin) => begin,
            Err(e) if definitive_preflight(&e) => return Ok(Unpub::Hold),
            Err(_) => return Err(io::Error::other("retirement_preflight")),
        };
        // An unreadable config is an error here, never "absent".
        if let Some(bytes) = current {
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

/// A prior intent write can succeed before its acknowledgement is lost. Read
/// that durable receipt before deciding that this disconnect never started.
pub(super) fn unpublish_recorded<F: FnOnce() -> Result<(), String>>(
    d: &File,
    owner: &Owner,
    r: &Receipt,
    prepare: impl FnOnce(Option<u32>) -> Result<F, String>,
) -> io::Result<Report> {
    let disk = receipt::read_receipt(d)?.ok_or(io::ErrorKind::NotFound)?;
    if disk.request_id != r.request_id
        || disk.mode != Mode::Disconnect
        || disk.target_revision != r.target_revision
        || !same_owner(&disk, owner)
        || disk.digest != r.digest
        || disk.state.terminal()
        || r.config_sha256
            .as_ref()
            .is_some_and(|sha| disk.config_sha256.as_ref() != Some(sha))
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut report = unpublish(d, owner, &disk, prepare);
    if report.intent.is_none() {
        report.intent = disk.config_sha256;
    }
    Ok(report)
}

fn definitive_preflight(error: &str) -> bool {
    matches!(
        error,
        "claude_connection_record_missing"
            | "claude_connection_request_conflict"
            | "claude_connection_phase_invalid"
            | "claude_connection_record_invalid"
            | "claude_connection_record_conflict"
            | "claude_connection_worker_conflict"
    )
}

#[derive(Debug, PartialEq, Eq)]
enum Pass {
    Done,
    Refused,
    Retry,
    Hold,
}
fn report_outcome(report: &Report, had_intent: bool) -> Pass {
    match &report.result {
        Ok(Unpub::Begun) => Pass::Done,
        Ok(Unpub::Hold) if !had_intent && report.intent.is_none() => Pass::Refused,
        Ok(Unpub::Hold) => Pass::Hold,
        Err(_) => Pass::Retry,
    }
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
    let had_intent = r.config_sha256.is_some();
    let report = local_with(g, admin, move |d| {
        unpublish_recorded(d, &owner, &r, |worker| {
            let lease = host::prepare_retirement(owner, target)?;
            if let Some(uid) = worker {
                lease.verify_worker(uid)?;
            }
            Ok(move || lease.begin())
        })
    })
    .await;
    let Ok(report) = report else {
        return Pass::Retry;
    };
    let outcome = report_outcome(&report, had_intent);
    if let Some(sha) = report.intent {
        with_job(g, id, |j| j.config_sha256 = Some(sha));
    }
    if outcome != Pass::Done {
        return outcome;
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
            Pass::Refused => {
                // No intent and no mutation: a durable failed receipt releases
                // the slot without pretending that a login was removed.
                finish(&g, id, Some(Stage::Failed)).await;
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
    reserve_checked(g, a, session, input, |owner, target| {
        host::prepare_retirement(owner, target).map(drop)
    })
    .await
}
async fn reserve_checked(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    input: Input,
    check: impl FnOnce(Owner, Uuid) -> Result<(), String> + Send + 'static,
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
        .filter(|c| management(c, &owner, target) == Some(Management::Managed))
        .and_then(|c| binding(c, &owner, target))
        .ok_or(StatusCode::CONFLICT)?;
    tokio::task::spawn_blocking(move || check(owner, target))
        .await
        .map_err(|_| UNAVAILABLE)?
        .map_err(|e| {
            if definitive_preflight(&e) {
                StatusCode::PRECONDITION_FAILED
            } else {
                UNAVAILABLE
            }
        })?;
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

#[cfg(test)]
pub(super) async fn reserve_for_test(
    g: &Gateway,
    a: &SessionAllocation,
    session: &str,
    input: Input,
    check: impl FnOnce(Owner, Uuid) -> Result<(), String> + Send + 'static,
) -> Result<(Value, bool), StatusCode> {
    reserve_checked(g, a, session, input, check).await
}

#[cfg(test)]
mod outcome_tests {
    use super::*;
    #[test]
    fn only_an_unstarted_definitive_refusal_releases_the_operation() {
        let held = Report {
            intent: None,
            result: Ok(Unpub::Hold),
        };
        assert_eq!(report_outcome(&held, false), Pass::Refused);
        assert_eq!(report_outcome(&held, true), Pass::Hold);
        let uncertain = Report {
            intent: None,
            result: Err(io::Error::other("busy")),
        };
        assert_eq!(report_outcome(&uncertain, false), Pass::Retry);
        let persisted = Report {
            intent: Some("digest".into()),
            result: Ok(Unpub::Hold),
        };
        assert_eq!(report_outcome(&persisted, false), Pass::Hold);
    }
}
