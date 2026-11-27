//! Owner-bound Claude subscription connection: the root parent that links the
//! existing credential-free host preparation, native isolation admission, the
//! private child login bridge and the existing engine worker.
//!
//! * `prepare` — never logs in and never starts a model worker. Under a
//!   whole-owner nonblocking flock it runs `prepare_with` and the bounded auth
//!   storage, launches the native isolation check as the worker BEFORE any
//!   credential exists, verifies its output against root's own inputs, writes
//!   the protected (root:worker 0440) worker inputs and persists a root-only
//!   record in `/var/lib/moosuri-claude-control/connections`.
//! * `login_command` — the existing hidden `claude-engine-login-bridge`, same
//!   executable, dedicated uid/gid, no supplementary groups, empty environment.
//! * `activate` — rechecks every pin, installs/starts the exact system unit and
//!   proves the worker with `Connection::connect` + `authorize` (no inference).
//! * `retire` — bounded cleanup of only this attempt.
//!
//! Existing credentials are never swept: a root that is not credential-free is
//! never probed again, and changed pins then require an explicit re-prepare.
mod evidence;
mod files;
mod host_exec;
mod record;
#[cfg(test)]
mod tests;
mod unit;

use super::{ARTIFACTS, PreparedRuntime, Request as RuntimeRequest, prepare_with, prereq, service};
use crate::engine_login::{Connection, Endpoint, Owner, Route};
use crate::installation::kiri::host::{self, Exec, Layout, System};
use moosuri_connectors::model::CancellationToken;
use record::{Attempt, Evidence, Hashes, Phase, Record, Storage, Store};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File},
    path::PathBuf,
    process::Stdio,
    sync::Arc,
    time::{Duration, Instant},
};
use uuid::Uuid;

type R<T> = Result<T, String>;
/// Shared by config publication and its lifecycle classification.
pub(crate) fn managed_socket(uid: u32) -> PathBuf {
    PathBuf::from(format!("{}/engine.sock", unit::socket_dir(uid)))
}
/// Cap for hashing pinned executables (cli, engine, helper).
const CAP: u64 = 512 << 20;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub request_id: Uuid,
    pub owner: Owner,
    pub email: String,
    pub model: String,
    pub effort: Option<String>,
    pub hosting_terms_accepted: bool,
}

fn valid_email(email: &str) -> bool {
    let Some((local, domain)) = email.split_once('@') else {
        return false;
    };
    email.len() <= 254
        && (1..=64).contains(&local.len())
        && domain.len() >= 3
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with(['.', '-'])
        && !domain.ends_with(['.', '-'])
        && !domain.contains("..")
        && email
            .bytes()
            .all(|b| b.is_ascii_graphic() && !b"\"\\'<>(),;:".contains(&b))
        && domain
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'.' || b == b'-')
}

impl Request {
    fn validate(&self) -> R<()> {
        RuntimeRequest::new(self.request_id, self.owner).validate()?;
        if !self.hosting_terms_accepted {
            return Err("claude_connection_terms_required".into());
        }
        let model = self.model.strip_prefix("claude-").unwrap_or("");
        if !valid_email(&self.email)
            || model.is_empty()
            || self.model.len() > 200
            || !self
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._:[]".contains(&b))
            || self.effort.as_ref().is_some_and(|e| {
                e.is_empty()
                    || e.len() > 32
                    || !e
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
            })
        {
            return Err("claude_connection_request_invalid".into());
        }
        Ok(())
    }
}

struct Inner {
    layout: Layout,
    request: Request,
    environment: Uuid,
    evidence: Evidence,
    /// Whole-owner lease: held for as long as any clone of `Prepared` lives.
    _lock: File,
}

/// Root-verified, not-yet-authenticated connection. Holds the owner lease.
#[derive(Clone)]
pub(crate) struct Prepared {
    pub request: Request,
    pub endpoint: Endpoint,
    inner: Arc<Inner>,
}

fn checkpoint(cancel: &CancellationToken) -> R<()> {
    if cancel.is_cancelled() {
        Err("claude_connection_cancelled".into())
    } else {
        Ok(())
    }
}

fn system() -> host_exec::HostSystem {
    host_exec::HostSystem(
        System {
            docker_config: PathBuf::from(format!("{ARTIFACTS}/docker-config-empty")),
        },
        None,
    )
}

fn kernel_release() -> R<String> {
    let text = fs::read_to_string("/proc/sys/kernel/osrelease")
        .map_err(|_| "claude_connection_kernel_unavailable")?;
    let text = text.trim();
    if text.is_empty() || text.len() > 128 || !text.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("claude_connection_kernel_unavailable".into());
    }
    Ok(text.to_owned())
}
fn boot_id() -> R<String> {
    let text = fs::read_to_string("/proc/sys/kernel/random/boot_id")
        .map_err(|_| "claude_connection_kernel_unavailable")?;
    Uuid::parse_str(text.trim())
        .map(|id| id.to_string())
        .map_err(|_| "claude_connection_kernel_unavailable".into())
}

#[derive(Debug, PartialEq, Eq)]
enum Plan {
    Fresh,
    /// Exact same request (restart / lost result).
    Replay,
    /// Never-logged-in attempt replaced by a new request.
    Supersede,
    /// A fully cleaned attempt; the prepared runtime is reused, not recreated.
    Reuse,
    FinishCleanup,
}

fn decide(existing: Option<&Record>, request: &Request, email_sha: &str) -> R<Plan> {
    let Some(rec) = existing else {
        return Ok(Plan::Fresh);
    };
    let a = &rec.attempt;
    if a.request_id == request.request_id {
        if a.email_sha256 != email_sha
            || a.model != request.model
            || a.effort != request.effort
            || a.hosting_terms_accepted != request.hosting_terms_accepted
        {
            return Err("claude_connection_request_conflict".into());
        }
        return if a.phase == Phase::Retired {
            Err("claude_connection_request_retired".into())
        } else {
            Ok(Plan::Replay)
        };
    }
    if request.request_id == rec.runtime_request_id {
        // The runtime's own id may only ever be replayed, never restarted.
        return Err("claude_connection_request_retired".into());
    }
    match a.phase {
        Phase::Retired => Ok(Plan::Reuse),
        Phase::Cleanup | Phase::RemovingFiles => Ok(Plan::FinishCleanup),
        Phase::Reserved | Phase::Prepared => Ok(Plan::Supersede),
        // Retry with the original request id to resume, or retire it.
        _ => Err("claude_connection_setup_in_progress".into()),
    }
}

fn new_record(
    request: &Request,
    email_sha: &str,
    runtime_request_id: Uuid,
    evidence: Option<Evidence>,
) -> Record {
    Record {
        version: 1,
        owner: request.owner,
        runtime_request_id,
        attempt: Attempt {
            request_id: request.request_id,
            environment_id: Uuid::new_v4(),
            email_sha256: email_sha.to_owned(),
            model: request.model.clone(),
            effort: request.effort.clone(),
            hosting_terms_accepted: true,
            phase: Phase::Reserved,
            files: None,
            unit_sha256: None,
        },
        evidence,
    }
}

/// Stop only this owner's worker and retire only this attempt's environment.
/// The cleanup marker is persisted first and only replaced by `Retired` after
/// every step succeeded.
fn cleanup(layout: &Layout, exec: &dyn Exec, store: &Store, rec: &mut Record) -> R<()> {
    if rec.attempt.phase == Phase::Active {
        return Err("claude_connection_active_retire_refused".into());
    }
    if rec.attempt.phase == Phase::RemovingFiles {
        files::remove_all(layout, &rec.attempt.environment_id)?;
        rec.attempt.files = None;
        rec.attempt.unit_sha256 = None;
        rec.attempt.phase = Phase::Retired;
        return store.save(rec);
    }
    // A cancelled preparation may never have created a login environment.
    // Do not require complete admission evidence to remove its root-only inputs.
    let Some(ev) = rec.evidence.clone() else {
        if rec.attempt.phase.credentials_possible() && rec.attempt.phase != Phase::Retired {
            return Err("claude_connection_record_conflict".into());
        }
        files::remove_all(layout, &rec.attempt.environment_id)?;
        rec.attempt.phase = Phase::Retired;
        return store.save(rec);
    };
    if rec.attempt.phase != Phase::Cleanup {
        rec.attempt.phase = Phase::Cleanup;
        store.save(rec)?;
    }
    let (uid, gid) = (ev.runtime.service.uid, ev.runtime.service.gid);
    let env = rec.attempt.environment_id;
    let worker_file = files::path(&env, files::WORKER);
    let text = unit::render(
        uid,
        gid,
        &ev.storage.filesystem_unit,
        &ev.runtime.helper.path,
        &worker_file,
    )?;
    let expected = rec
        .attempt
        .unit_sha256
        .clone()
        .unwrap_or_else(|| host::sha256_bytes(text.as_bytes()));
    unit::remove(layout, exec, uid, &expected)?;
    let present = layout.exists(
        &layout.at(files::dir(&env)
            .to_str()
            .ok_or("claude_connection_path_invalid")?),
    )?;
    if let (true, Some(hashes)) = (present, rec.attempt.files.as_ref()) {
        // Never run anything from files that are not byte-exact root inputs.
        files::verify_all(layout, &env, gid, hashes)?;
        let helper = &ev.runtime.helper;
        if prereq::trusted_file(layout, &helper.path, CAP)? != *helper {
            return Err("claude_connection_helper_changed".into());
        }
        let tools = prereq::check(layout, exec)?;
        service::ensure_unit(
            layout,
            &ev.runtime.service,
            &ev.runtime.rootless_wrapper.path,
        )?;
        service::start_manager(exec, &tools, &ev.runtime.service)?;
        service::start_service(exec, &tools, &ev.runtime.service)?;
        let tail = vec![
            helper
                .path
                .to_str()
                .ok_or("claude_connection_path_invalid")?
                .to_owned(),
            "claude-engine-retire".to_owned(),
            "--config".to_owned(),
            worker_file
                .to_str()
                .ok_or("claude_connection_path_invalid")?
                .to_owned(),
        ];
        exec.run(
            "runuser",
            &unit::as_worker(&ev.runtime.service.username, &tail),
            host::Input::None,
            Duration::from_secs(120),
        )?
        .ok("claude_connection_cleanup_failed")?;
    }
    rec.attempt.phase = Phase::RemovingFiles;
    store.save(rec)?;
    files::remove_all(layout, &env)?;
    rec.attempt.files = None;
    rec.attempt.unit_sha256 = None;
    rec.attempt.phase = Phase::Retired;
    store.save(rec)
}

fn assemble(layout: Layout, request: Request, rec: &Record, lock: File) -> R<Prepared> {
    let evidence = rec
        .evidence
        .clone()
        .ok_or("claude_connection_record_conflict")?;
    let uid = evidence.runtime.service.uid;
    let endpoint = Endpoint {
        socket: managed_socket(uid),
        worker_uid: uid,
    };
    endpoint.validate()?;
    Ok(Prepared {
        request: request.clone(),
        endpoint,
        inner: Arc::new(Inner {
            layout,
            request,
            environment: rec.attempt.environment_id,
            evidence,
            _lock: lock,
        }),
    })
}

/// Root-launched native admission on a credential-free auth root.
fn admit(
    layout: &Layout,
    exec: &dyn Exec,
    rec: &Record,
    runtime: PreparedRuntime,
    storage: Storage,
    kernel: String,
    cancel: &CancellationToken,
) -> R<Evidence> {
    let engine_stack = engine_stack(layout)?;
    let expected = evidence::candidate(&runtime, &storage)?;
    let (env, gid) = (rec.attempt.environment_id, runtime.service.gid);
    files::ensure_dir(layout, &env, gid)?;
    files::write(
        layout,
        &env,
        gid,
        files::CANDIDATE,
        &evidence::candidate_bytes(&expected)?,
    )?;
    checkpoint(cancel)?;
    let admitted = evidence::run_admission(
        exec,
        &runtime,
        &files::path(&env, files::CANDIDATE),
        &expected,
    )?;
    if engine_stack != self::engine_stack(layout)? {
        return Err("claude_connection_artifact_changed".into());
    }
    checkpoint(cancel)?;
    Ok(Evidence {
        runtime,
        engine_stack,
        storage,
        candidate_sha256: admitted.candidate_sha256,
        profile_json: admitted.profile_json,
        profile_digest: admitted.profile_digest,
        receipt_json: admitted.receipt_json,
        receipt_sha256: admitted.receipt_sha256,
        kernel_release: kernel,
        boot_id: boot_id()?,
    })
}

fn engine_stack(layout: &Layout) -> R<Vec<prereq::PinnedFile>> {
    [
        "dockerd",
        "containerd",
        "runc",
        "rootlesskit",
        "slirp4netns",
        "newuidmap",
        "newgidmap",
        "fuse-overlayfs",
    ]
    .iter()
    .map(|name| prereq::trusted_file(layout, &host::find_tool(name)?, CAP))
    .collect()
}

fn ensure_files(layout: &Layout, store: &Store, rec: &mut Record, email: &str) -> R<()> {
    let ev = rec
        .evidence
        .clone()
        .ok_or("claude_connection_record_conflict")?;
    let (uid, gid) = (ev.runtime.service.uid, ev.runtime.service.gid);
    let env = rec.attempt.environment_id;
    let worker = files::render_worker(&rec.attempt, &rec.owner, uid, &ev.profile_digest)?;
    files::ensure_dir(layout, &env, gid)?;
    let hashes = Hashes {
        profile: files::write(
            layout,
            &env,
            gid,
            files::PROFILE,
            ev.profile_json.as_bytes(),
        )?,
        email: files::write(layout, &env, gid, files::EMAIL, &files::email_bytes(email))?,
        worker: files::write(layout, &env, gid, files::WORKER, &worker)?,
    };
    if rec.attempt.files.as_ref().is_some_and(|h| *h != hashes) {
        return Err("claude_connection_config_changed".into());
    }
    // The probe input is not needed by anything afterwards.
    if let Some(candidate) = files::path(&env, files::CANDIDATE).to_str() {
        let _ = fs::remove_file(layout.at(candidate));
    }
    rec.attempt.files = Some(hashes);
    if rec.attempt.phase == Phase::Reserved {
        rec.attempt.phase = Phase::Prepared;
    }
    store.save(rec)
}

fn pinned_helper(layout: &Layout) -> R<PathBuf> {
    let current = std::env::current_exe()
        .and_then(fs::canonicalize)
        .map_err(|_| "claude_connection_helper_unknown")?;
    let pin = prereq::trusted_file(layout, &current, CAP)?;
    // Preserve the exact binary for cleanup/reboot even when the gateway is
    // upgraded. Workers never point at a replaceable deployment executable.
    for dir in [
        ARTIFACTS.to_owned(),
        format!("{ARTIFACTS}/helpers"),
        format!("{ARTIFACTS}/helpers/{}", pin.sha256),
    ] {
        layout.ensure_dir(&layout.at(&dir), 0o755)?;
    }
    let path = PathBuf::from(format!("{ARTIFACTS}/helpers/{}/moosuri", pin.sha256));
    let mut source = File::open(&current).map_err(|_| "claude_connection_helper_unknown")?;
    layout.write_stream(&path, &mut source, CAP, &pin.sha256, 0o755, &[])?;
    Ok(path)
}

fn prepare_blocking(request: Request, cancel: &CancellationToken) -> R<Prepared> {
    super::require_linux_root()?;
    host::find_tool("systemd-run")?;
    let layout = Layout::system();
    let store = Store::new(&layout);
    store.ensure()?;
    layout.ensure_dir(&layout.at(files::BASE), 0o755)?;
    let lock = store.lock(&request.owner)?;
    let email_sha = host::sha256_bytes(request.email.as_bytes());
    let mut exec = system();
    exec.1 = Some(cancel.clone());
    let existing = store.load(&request.owner)?;
    let plan = decide(existing.as_ref(), &request, &email_sha)?;
    let mut rec = match (plan, existing) {
        (Plan::Fresh, _) => new_record(&request, &email_sha, request.request_id, None),
        (Plan::Replay, Some(rec)) => rec,
        (Plan::Supersede | Plan::Reuse, Some(old)) => {
            files::remove_all(&layout, &old.attempt.environment_id)?;
            new_record(&request, &email_sha, old.runtime_request_id, old.evidence)
        }
        (Plan::FinishCleanup, Some(mut old)) => {
            cleanup(&layout, &exec, &store, &mut old)?;
            new_record(&request, &email_sha, old.runtime_request_id, old.evidence)
        }
        _ => return Err("claude_connection_record_conflict".into()),
    };
    store.save(&rec)?;
    if matches!(rec.attempt.phase, Phase::Cleanup | Phase::RemovingFiles) {
        // Replay only to finish a requested cleanup: nothing is re-prepared.
        if let Some(ev) = &rec.evidence {
            evidence::validate_stored(ev)?;
        }
        return assemble(layout, request, &rec, lock);
    }
    checkpoint(cancel)?;

    let helper = match &rec.evidence {
        Some(ev) => ev.runtime.helper.path.clone(),
        None => pinned_helper(&layout)?,
    };
    let runtime = prepare_with(
        &layout,
        &exec,
        RuntimeRequest::new(rec.runtime_request_id, request.owner),
        &helper,
    )?;
    checkpoint(cancel)?;
    let storage = Storage::from_prepared(crate::kiri_storage::claude::prepare(
        rec.runtime_request_id,
        request.owner,
        runtime.service.uid,
        runtime.service.gid,
    )?)?;
    checkpoint(cancel)?;
    let kernel = kernel_release()?;

    let reusable = rec.evidence.as_ref().is_some_and(|e| {
        e.matches(&runtime, &storage, &kernel)
            && engine_stack(&layout).is_ok_and(|current| current == e.engine_stack)
            && evidence::validate_stored(e).is_ok()
    });
    if !reusable {
        // Credentials may exist (or the root is not provably empty): never
        // probe or delete anything; an explicit re-prepare is required.
        if rec.attempt.phase.credentials_possible()
            || !evidence::auth_root_fresh(&layout, &storage.auth_root)?
        {
            return Err("claude_connection_reprepare_required".into());
        }
        files::remove_all(&layout, &rec.attempt.environment_id)?;
        rec.attempt.files = None;
        rec.attempt.phase = Phase::Reserved;
        rec.evidence = None;
        let fresh = admit(&layout, &exec, &rec, runtime, storage, kernel, cancel)?;
        rec.evidence = Some(fresh);
        store.save(&rec)?;
    }
    ensure_files(&layout, &store, &mut rec, &request.email)?;
    if let (Some(hashes), Some(ev)) = (&rec.attempt.files, &rec.evidence) {
        files::verify_all(
            &layout,
            &rec.attempt.environment_id,
            ev.runtime.service.gid,
            hashes,
        )?;
    }
    assemble(layout, request, &rec, lock)
}

/// Root receipt, native admission and protected inputs. No login, no worker.
pub(crate) async fn prepare(request: Request, cancel: &CancellationToken) -> R<Prepared> {
    request.validate()?;
    checkpoint(cancel)?;
    let token = cancel.clone();
    tokio::task::spawn_blocking(move || prepare_blocking(request, &token))
        .await
        .map_err(|_| "claude_connection_prepare_aborted".to_owned())?
}

impl Inner {
    fn store(&self) -> Store {
        Store::new(&self.layout)
    }
    fn uid(&self) -> u32 {
        self.evidence.runtime.service.uid
    }
    fn gid(&self) -> u32 {
        self.evidence.runtime.service.gid
    }
    /// The on-disk root record must still be exactly this prepared attempt.
    fn current(&self) -> R<(Store, Record)> {
        let store = self.store();
        let rec = store
            .load(&self.request.owner)?
            .ok_or("claude_connection_record_missing")?;
        if rec.attempt.request_id != self.request.request_id
            || rec.attempt.environment_id != self.environment
            || rec.evidence.as_ref() != Some(&self.evidence)
        {
            return Err("claude_connection_record_changed".into());
        }
        Ok((store, rec))
    }
    /// Evidence consistency, every protected file hash and the exact worker
    /// authority (the config must be exactly what root derives from the record).
    fn verify_config(&self, rec: &Record) -> R<()> {
        evidence::validate_stored(&self.evidence)?;
        let hashes = rec
            .attempt
            .files
            .as_ref()
            .ok_or("claude_connection_config_changed")?;
        files::verify_all(&self.layout, &self.environment, self.gid(), hashes)?;
        let worker = files::render_worker(
            &rec.attempt,
            &rec.owner,
            self.uid(),
            &self.evidence.profile_digest,
        )?;
        if host::sha256_bytes(&worker) != hashes.worker
            || host::sha256_bytes(self.evidence.profile_json.as_bytes()) != hashes.profile
            || host::sha256_bytes(&files::email_bytes(&self.request.email)) != hashes.email
            || host::sha256_bytes(self.request.email.as_bytes()) != rec.attempt.email_sha256
        {
            return Err("claude_connection_config_changed".into());
        }
        Ok(())
    }
    /// Artifact, engine, image, mount and kernel pins as observed right now.
    fn verify_runtime(&self, exec: &dyn Exec) -> R<()> {
        let ev = &self.evidence;
        let tools = prereq::check(&self.layout, exec)?;
        let cli = prereq::PinnedFile {
            path: ev.runtime.official_cli.path.clone(),
            sha256: ev.runtime.official_cli.sha256.clone(),
        };
        if tools.engine != ev.runtime.engine || kernel_release()? != ev.kernel_release {
            return Err("claude_connection_artifact_changed".into());
        }
        if engine_stack(&self.layout)? != ev.engine_stack {
            return Err("claude_connection_artifact_changed".into());
        }
        for pin in [
            &ev.runtime.engine,
            &cli,
            &ev.runtime.rootless_wrapper,
            &ev.runtime.helper,
        ] {
            if prereq::trusted_file(&self.layout, &pin.path, CAP)? != *pin {
                return Err("claude_connection_artifact_changed".into());
            }
        }
        unit::mount_active(exec, &ev.storage.filesystem_unit)?;
        service::verify_engine(&self.layout, exec, &tools, &ev.runtime.service)?;
        service::ensure_image(exec, &tools, &ev.runtime.service, &ev.runtime.image)
    }
    fn unit_text(&self) -> R<String> {
        unit::render(
            self.uid(),
            self.gid(),
            &self.evidence.storage.filesystem_unit,
            &self.evidence.runtime.helper.path,
            &files::path(&self.environment, files::WORKER),
        )
    }
    fn install_worker(&self, cancel: &CancellationToken) -> R<()> {
        super::require_linux_root()?;
        let (store, mut rec) = self.current()?;
        if !matches!(
            rec.attempt.phase,
            Phase::LoginIssued | Phase::Activating | Phase::WorkerReady | Phase::Active
        ) {
            return Err("claude_connection_phase_invalid".into());
        }
        self.verify_config(&rec)?;
        let mut exec = system();
        exec.1 = Some(cancel.clone());
        self.verify_runtime(&exec)?;
        checkpoint(cancel)?;
        let text = self.unit_text()?;
        let digest = host::sha256_bytes(text.as_bytes());
        if rec.attempt.phase == Phase::LoginIssued {
            rec.attempt.phase = Phase::Activating;
            rec.attempt.unit_sha256 = Some(digest.clone());
            store.save(&rec)?;
        }
        if rec.attempt.unit_sha256.as_deref() != Some(digest.as_str()) {
            return Err("claude_connection_unit_conflict".into());
        }
        unit::install_and_start(&self.layout, &exec, self.uid(), &text)
    }
    fn mark_worker_ready(&self) -> R<()> {
        let (store, mut rec) = self.current()?;
        match rec.attempt.phase {
            Phase::WorkerReady | Phase::Active => Ok(()),
            Phase::Activating => {
                rec.attempt.phase = Phase::WorkerReady;
                store.save(&rec)
            }
            _ => Err("claude_connection_phase_invalid".into()),
        }
    }
}

#[cfg(target_os = "linux")]
fn drop_privileges(uid: u32, gid: u32) -> std::io::Result<()> {
    // Async-signal-safe syscalls only (runs between fork and exec).
    let fail = || Err(std::io::Error::last_os_error());
    unsafe {
        if libc::setgroups(0, std::ptr::null()) != 0
            || libc::setresgid(gid, gid, gid) != 0
            || libc::setresuid(uid, uid, uid) != 0
        {
            return fail();
        }
        // Privileges must be irrecoverable.
        if libc::setuid(0) == 0 {
            return Err(std::io::Error::from(std::io::ErrorKind::PermissionDenied));
        }
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn drop_privileges(_: u32, _: u32) -> std::io::Result<()> {
    Err(std::io::Error::from(std::io::ErrorKind::Unsupported))
}

/// The existing hidden `claude-engine-login-bridge`, run as the dedicated
/// worker. The caller owns the JSON-lines protocol and must keep stdin open
/// until it has read the terminal result and the process exit. Persists
/// `LoginIssued` first: from here credentials may exist.
pub(crate) fn login_command(prepared: &Prepared) -> R<tokio::process::Command> {
    super::require_linux_root()?;
    let inner = &prepared.inner;
    let (store, mut rec) = inner.current()?;
    if !matches!(rec.attempt.phase, Phase::Prepared | Phase::LoginIssued) {
        return Err("claude_connection_phase_invalid".into());
    }
    inner.verify_config(&rec)?;
    let helper = &inner.evidence.runtime.helper;
    if prereq::trusted_file(&inner.layout, &helper.path, CAP)? != *helper {
        return Err("claude_connection_helper_changed".into());
    }
    if rec.attempt.phase == Phase::Prepared {
        rec.attempt.phase = Phase::LoginIssued;
        store.save(&rec)?;
    }
    let (uid, gid) = (inner.uid(), inner.gid());
    let mut command = tokio::process::Command::new(&helper.path);
    command
        .arg("claude-engine-login-bridge")
        .arg("--config")
        .arg(files::path(&inner.environment, files::WORKER))
        .env_clear()
        .current_dir("/")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    // SAFETY: the closure only performs async-signal-safe syscalls.
    unsafe {
        command.pre_exec(move || drop_privileges(uid, gid));
    }
    Ok(command)
}

async fn wait_ready(prepared: &Prepared, cancel: &CancellationToken) -> R<()> {
    let deadline = Instant::now() + Duration::from_secs(120);
    let request = &prepared.request;
    let uid = prepared.endpoint.worker_uid;
    let mut tick = 0u32;
    loop {
        checkpoint(cancel)?;
        if let Ok(connection) = Connection::connect(&prepared.endpoint).await {
            match connection
                .authorize(
                    Route::ClaudeCode,
                    request.owner,
                    request.request_id,
                    &request.model,
                    request.effort.as_deref(),
                )
                .await
            {
                Ok(_) => return Ok(()),
                Err(e) if e == "engine_worker_pin_rejected" => {
                    return Err("claude_connection_worker_pin_rejected".into());
                }
                Err(_) => {}
            }
        }
        if Instant::now() >= deadline {
            return Err("claude_connection_worker_not_ready".into());
        }
        tick += 1;
        if tick % 10 == 0 {
            let state = tokio::task::spawn_blocking(move || unit::active_state(&system(), uid))
                .await
                .map_err(|_| "claude_connection_worker_not_ready".to_owned())??;
            if !matches!(state.as_str(), "active" | "activating") {
                return Err("claude_connection_worker_exited".into());
            }
        }
        tokio::select! {
            _ = cancel.cancelled() => return Err("claude_connection_cancelled".into()),
            _ = tokio::time::sleep(Duration::from_millis(500)) => {}
        }
    }
}

/// Only after the bridge reported ready and the gateway's own fresh authority
/// checks. Success means the exact pinned worker answered the exact pin; no
/// inference was made. On failure the gateway should `retire`.
pub(crate) async fn activate(prepared: &Prepared, cancel: &CancellationToken) -> R<()> {
    let inner = prepared.inner.clone();
    let token = cancel.clone();
    tokio::task::spawn_blocking(move || inner.install_worker(&token))
        .await
        .map_err(|_| "claude_connection_activate_aborted".to_owned())??;
    wait_ready(prepared, cancel).await?;
    let inner = prepared.inner.clone();
    tokio::task::spawn_blocking(move || inner.mark_worker_ready())
        .await
        .map_err(|_| "claude_connection_activate_aborted".to_owned())?
}

/// Called by the gateway only after the exact config has been published (or
/// reconciled after an uncertain rename), under the same registry/admin lock.
pub(crate) fn published(prepared: &Prepared) -> R<()> {
    super::require_linux_root()?;
    let (store, mut rec) = prepared.inner.current()?;
    mark_record(&store, &mut rec, prepared.request.request_id)
}

/// Finish the root publication mark after a gateway restart. The caller must
/// hold the registry/admin lock and have re-read and fsynced the exact config.
/// A missing, different, or pre-activation record is never accepted.
pub(crate) fn published_request(owner: Owner, request_id: Uuid) -> R<()> {
    RuntimeRequest::new(request_id, owner).validate()?;
    super::require_linux_root()?;
    published_in(&Layout::system(), owner, request_id)
}
fn published_in(layout: &Layout, owner: Owner, request_id: Uuid) -> R<()> {
    let store = Store::new(layout);
    store.ensure()?;
    let _lock = store.lock(&owner)?;
    let mut rec = store
        .load(&owner)?
        .ok_or("claude_connection_record_missing")?;
    mark_record(&store, &mut rec, request_id)
}
fn mark_record(store: &Store, rec: &mut Record, request_id: Uuid) -> R<()> {
    if rec.attempt.request_id != request_id {
        return Err("claude_connection_request_conflict".into());
    }
    match rec.attempt.phase {
        Phase::Active => Ok(()),
        Phase::WorkerReady => {
            rec.attempt.phase = Phase::Active;
            store.save(rec)
        }
        _ => Err("claude_connection_phase_invalid".into()),
    }
}

/// A checked full-owner retirement lease. Keep the root record locked while
/// the gateway durably unpublishes its exact config; only then call `begin`.
/// Merely preparing this lease never changes a record, worker or credential.
pub(crate) struct Retirement {
    store: Store,
    record: Record,
    _lock: File,
}
impl Retirement {
    pub(crate) fn verify_worker(&self, uid: u32) -> R<()> {
        if self.record.evidence.as_ref().map(|e| e.runtime.service.uid) == Some(uid) {
            Ok(())
        } else {
            Err("claude_connection_worker_conflict".into())
        }
    }
    pub(crate) fn begin(mut self) -> R<()> {
        if matches!(
            self.record.attempt.phase,
            Phase::Active | Phase::WorkerReady
        ) {
            self.record.attempt.phase = Phase::Cleanup;
            self.store.save(&self.record)?;
        }
        Ok(())
    }
}
pub(crate) fn prepare_retirement(owner: Owner, request_id: Uuid) -> R<Retirement> {
    RuntimeRequest::new(request_id, owner).validate()?;
    super::require_linux_root()?;
    prepare_retirement_in(&Layout::system(), owner, request_id)
}
fn prepare_retirement_in(layout: &Layout, owner: Owner, request_id: Uuid) -> R<Retirement> {
    let store = Store::new(layout);
    store.ensure()?;
    let lock = store.lock(&owner)?;
    let record = store
        .load(&owner)?
        .ok_or("claude_connection_record_missing")?;
    if record.owner != owner || record.attempt.request_id != request_id {
        return Err("claude_connection_request_conflict".into());
    }
    if !matches!(
        record.attempt.phase,
        Phase::Active | Phase::WorkerReady | Phase::Cleanup | Phase::RemovingFiles | Phase::Retired
    ) {
        return Err("claude_connection_phase_invalid".into());
    }
    Ok(Retirement {
        store,
        record,
        _lock: lock,
    })
}
#[cfg(test)]
fn begin_retirement_in(layout: &Layout, owner: Owner, request_id: Uuid) -> R<()> {
    prepare_retirement_in(layout, owner, request_id)?.begin()
}

/// Recovery after gateway restart. The caller must authenticate the current
/// full owner and verify this request is absent from the published model config.
/// No credential or email is needed and no new probe/login is started.
pub(crate) async fn retire_request(owner: Owner, request_id: Uuid) -> R<()> {
    RuntimeRequest::new(request_id, owner).validate()?;
    tokio::task::spawn_blocking(move || {
        super::require_linux_root()?;
        retire_in(&Layout::system(), &system(), owner, request_id)
    })
    .await
    .map_err(|_| "claude_connection_retire_aborted".to_owned())?
}

fn retire_in(layout: &Layout, exec: &dyn Exec, owner: Owner, request_id: Uuid) -> R<()> {
    let store = Store::new(layout);
    store.ensure()?;
    let _lock = store.lock(&owner)?;
    let Some(mut rec) = store.load(&owner)? else {
        return Ok(());
    };
    // Another request never reached this slot; preserve its record untouched.
    if rec.attempt.request_id != request_id || rec.attempt.phase == Phase::Retired {
        return Ok(());
    }
    cleanup(layout, exec, &store, &mut rec)
}

/// Bounded cleanup of one failed/unwanted attempt (never an Active one). Marks
/// `Cleanup` in the root record first; the next setup for the owner finishes a
/// cleanup that was interrupted before it can proceed.
pub(crate) async fn retire(prepared: &Prepared) -> R<()> {
    let inner = prepared.inner.clone();
    tokio::task::spawn_blocking(move || {
        super::require_linux_root()?;
        let (store, mut rec) = inner.current()?;
        match rec.attempt.phase {
            Phase::Retired => return Ok(()),
            Phase::Active => return Err("claude_connection_active_retire_refused".to_owned()),
            _ => {}
        }
        cleanup(&inner.layout, &system(), &store, &mut rec)
    })
    .await
    .map_err(|_| "claude_connection_retire_aborted".to_owned())?
}
