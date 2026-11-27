//! Hosted Claude connection for a managed installation. Only the host
//! runtime-model reads these private files; members receive model sockets.
//! A saved configuration is not a successful inference or a live-worker proof.
//!
//! The key, the pending file and the config live in a dedicated private state
//! directory (`--model-setup-dir`) that is deliberately NOT the strict registry
//! directory: the registry refuses any foreign entry, so nothing here may ever
//! be written next to it. Every write still runs inside the registry directory
//! lock and a fresh administrator pin (`with_installation_setup`).
use super::*;
use moosuri_connectors::model::{
    CancellationToken, Secret, TransportLimits, protocol::TextCapabilities,
};
use moosuri_core::{ProviderConfig, ProviderKind};
use moosuri_store::runtime_registry::admin as control;
use std::{
    ffi::CString,
    io,
    os::fd::{AsRawFd, FromRawFd},
};

pub(super) mod claude;

pub(super) const CONFIG_FILE: &str = "runtime-model.json";
const PENDING_FILE: &str = "runtime-model.pending";
const KEY_PREFIX: &str = "model-api-key-";
const MAX_FILE: usize = 16 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    request_id: Uuid,
    api_key: Zeroizing<String>,
    model: String,
    effort: Option<String>,
    /// Replacement of an existing connection: the administrator names the
    /// connection revision being replaced (compare-and-set). Never implicit.
    #[serde(default)]
    replace_revision: Option<Uuid>,
}
impl Input {
    fn valid(&self) -> bool {
        !self.request_id.is_nil()
            && self
                .replace_revision
                .is_none_or(|r| !r.is_nil() && r != self.request_id)
            && (8..=8192).contains(&self.api_key.len())
            && self.api_key.bytes().all(|b| b.is_ascii_graphic())
            && self.model.starts_with("claude-")
            && self.model.len() <= 200
            && self
                .model
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            && self
                .effort
                .as_deref()
                .is_none_or(|v| matches!(v, "low" | "medium" | "high"))
    }
    fn key_name(&self) -> String {
        format!("{KEY_PREFIX}{}", self.request_id)
    }
}

fn provider() -> ProviderConfig {
    ProviderConfig {
        kind: ProviderKind::Anthropic,
        base_url: None,
        allowed_ips: vec![],
        router_providers: vec![],
        zero_data_retention: false,
    }
}
fn error(code: &'static str, status: StatusCode) -> Response {
    response(
        status,
        serde_json::json!({"code":code}).to_string().into_bytes(),
    )
}
fn view(saved: Option<Option<Uuid>>) -> Response {
    listed(saved, false)
}
/// `subscription_operation` only tells the page that a subscription setup
/// operation is unfinished, so a reload can resume it. It carries no detail.
fn listed(saved: Option<Option<Uuid>>, operation: bool) -> Response {
    let mut value = serde_json::json!({"supported":true,"saved":saved.is_some()});
    if let Some(Some(revision)) = saved {
        value["connection_revision"] = serde_json::json!(revision);
    }
    if operation {
        value["subscription_operation"] = serde_json::json!(true);
    }
    response(StatusCode::OK, value.to_string().into_bytes())
}
pub(super) fn routes() -> Router<Gateway> {
    Router::new()
        .route(
            "/api/v1/personal/admin/engine/setup",
            get(endpoint).post(endpoint),
        )
        .route(
            "/api/v1/personal/admin/engine/subscription",
            get(claude::handle).post(claude::handle),
        )
        .layer(DefaultBodyLimit::max(MAX_FILE))
}

async fn pin(g: &Gateway, headers: &HeaderMap) -> Result<control::AdminSession, StatusCode> {
    let (a, view) = admin::authenticated_session(g, headers).await?;
    let root = g.registry.clone();
    tokio::task::spawn_blocking(move || {
        control::checked_session(&root, &a, &view)
            .and_then(|p| p.at_epoch(a.epoch))
            .map_err(admin::error)
    })
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
}

async fn local<T: Send + 'static>(
    g: &Gateway,
    headers: &HeaderMap,
    action: impl FnOnce(&File) -> io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    let pin = pin(g, headers).await?;
    local_with(g, pin, action).await
}
/// Same bounded local section for an already-checked administrator pin (the
/// subscription job reauthenticates its ORIGINAL session before each use).
async fn local_with<T: Send + 'static>(
    g: &Gateway,
    pin: control::AdminSession,
    action: impl FnOnce(&File) -> io::Result<T> + Send + 'static,
) -> Result<T, StatusCode> {
    let root = g.registry.clone();
    let dir = g
        .model_setup_dir
        .clone()
        .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
    tokio::task::spawn_blocking(move || {
        // The registry directory lock and the fresh admin check are held for the
        // whole callback; the private model directory is opened under them.
        control::with_installation_setup(&root, &pin, move |_registry| {
            model_dir(&dir).and_then(|d| action(&d))
        })
        .map_err(admin::error)?
        .map_err(|e| {
            if e.kind() == io::ErrorKind::AlreadyExists {
                StatusCode::CONFLICT
            } else {
                StatusCode::SERVICE_UNAVAILABLE
            }
        })
    })
    .await
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
}

async fn endpoint(
    State(g): State<Gateway>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let result = async {
        g.headers(&headers, &method)?;
        if uri.query().is_some() {
            return Err(StatusCode::BAD_REQUEST);
        }
        // Even unsupported installations require a current administrator.
        pin(&g, &headers).await?;
        if !g.model_setup {
            return Err(StatusCode::NOT_FOUND);
        }
        let dir = g
            .model_setup_dir
            .clone()
            .ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let _slot = g
            .inflight
            .try_acquire()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if method == Method::GET && body.is_empty() {
            return local(&g, &headers, |d| {
                let operation = claude::unfinished(d);
                let saved = match read(d, CONFIG_FILE) {
                    Ok(config) => config.map(|c| revision_of(&c)),
                    // Preserve unknown-config status while allowing the owner
                    // to open the separately authorized recovery operation.
                    Err(_) if operation => Some(None),
                    Err(e) => return Err(e),
                };
                Ok((saved, operation))
            })
            .await
            .map(|(saved, operation)| listed(saved, operation));
        }
        if method != Method::POST
            || one(&headers, "content-type").and_then(|s| s.split(';').next())
                != Some("application/json")
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let input: Input = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
        if !input.valid() {
            return Err(StatusCode::BAD_REQUEST);
        }
        // An exact retry checks the saved bytes before any provider request. A
        // different connection never replaces a working connection implicitly:
        // it needs `replace_revision` equal to the current revision.
        let expected = config(&dir, &input)?;
        let probe_expected = expected.clone();
        let probe_key = input.api_key.clone();
        let key_name = input.key_name();
        let probe_name = key_name.clone();
        let (request, replace) = (input.request_id, input.replace_revision);
        if local(&g, &headers, move |d| {
            existing(
                d,
                &probe_expected,
                &probe_name,
                probe_key.as_bytes(),
                request,
                replace,
            )
        })
        .await?
        {
            return Ok(view(Some(Some(request))));
        }
        // Catalog lookup uses the fixed Anthropic origin and sends no member
        // prompt. Catalog presence does not claim verified inference/effort.
        let client = moosuri_agent::model_client(
            &provider(),
            Some(Secret::new(input.api_key.to_string()).map_err(|_| StatusCode::BAD_REQUEST)?),
            TextCapabilities::default(),
        )
        .and_then(|c| {
            c.with_limits(TransportLimits {
                connect: Duration::from_secs(5),
                idle: Duration::from_secs(10),
                total: Duration::from_secs(20),
                response_bytes: 1024 * 1024,
            })
        })
        .map_err(|_| StatusCode::BAD_REQUEST)?;
        let catalog = match client.list_models(&CancellationToken::new()).await {
            Ok(v) => v,
            Err(_) => {
                return Ok(error(
                    "model_provider_verification_failed",
                    StatusCode::BAD_GATEWAY,
                ));
            }
        };
        if !catalog.models.iter().any(|m| m.id == input.model) {
            return Ok(error(
                "model_not_in_provider_catalog",
                StatusCode::UNPROCESSABLE_ENTITY,
            ));
        }
        // Reauthenticate after network I/O; the role/epoch check and every file
        // commit run under the same registry directory lock. The revision is
        // checked again there, so a concurrent replacement loses with 409.
        local(&g, &headers, move |d| {
            save(
                d,
                &expected,
                &key_name,
                input.api_key.as_bytes(),
                request,
                replace,
            )
        })
        .await?;
        Ok(view(Some(Some(request))))
    }
    .await;
    result.unwrap_or_else(failure)
}

fn config(dir: &Path, input: &Input) -> Result<Vec<u8>, StatusCode> {
    serde_json::to_vec(
        &serde_json::json!({"version":1,"connection_revision":input.request_id,
        "provider":provider(),"model":input.model,"effort":input.effort,"models":[],
        "key_file":dir.join(input.key_name()),"subscription":null,"bindings":[]}),
    )
    .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
fn revision_of(config: &[u8]) -> Option<Uuid> {
    serde_json::from_slice::<serde_json::Value>(config)
        .ok()?
        .get("connection_revision")?
        .as_str()?
        .parse()
        .ok()
}
/// Open the installer-owned private directory by its exact canonical path:
/// no symlink, same effective uid, no group/other access, and the opened
/// handle is the inspected inode.
fn model_dir(path: &Path) -> io::Result<File> {
    let invalid = || io::Error::from(io::ErrorKind::InvalidData);
    let meta = fs::symlink_metadata(path)?;
    if !path.is_absolute()
        || !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || fs::canonicalize(path)? != path
    {
        return Err(invalid());
    }
    let dir = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(path)?;
    let opened = dir.metadata()?;
    if opened.dev() != meta.dev()
        || opened.ino() != meta.ino()
        || opened.uid() != meta.uid()
        || opened.mode() & 0o077 != 0
    {
        return Err(invalid());
    }
    Ok(dir)
}
fn open(d: &File, name: &str, flags: i32) -> io::Result<File> {
    let name = CString::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
    let fd = unsafe {
        libc::openat(
            d.as_raw_fd(),
            name.as_ptr(),
            flags | libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK,
            0o600,
        )
    };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    let file = unsafe { File::from_raw_fd(fd) };
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o7777 != 0o600
        || meta.nlink() != 1
        || meta.len() > MAX_FILE as u64
    {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(file)
}
fn read(d: &File, name: &str) -> io::Result<Option<Zeroizing<Vec<u8>>>> {
    let file = match open(d, name, libc::O_RDONLY) {
        Ok(f) => f,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };
    let mut value = Zeroizing::new(Vec::new());
    file.take((MAX_FILE + 1) as u64).read_to_end(&mut value)?;
    if value.len() > MAX_FILE {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(Some(value))
}
fn unlink(d: &File, name: &str) -> io::Result<()> {
    let name = CString::new(name).map_err(|_| io::ErrorKind::InvalidInput)?;
    if unsafe { libc::unlinkat(d.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
/// `Ok(true)`: this exact connection is already saved. `Ok(false)`: nothing
/// is saved, or the named current revision may be replaced. Anything else
/// conflicts and is never changed implicitly.
fn existing(
    d: &File,
    expected: &[u8],
    key_name: &str,
    key: &[u8],
    request: Uuid,
    replace: Option<Uuid>,
) -> io::Result<bool> {
    claude::ensure_settled(d)?;
    if let Some(current) = read(d, CONFIG_FILE)? {
        // Do not abandon an Active subscription worker by changing only the
        // broker config. This gate also precedes any API catalog request.
        let value: serde_json::Value =
            serde_json::from_slice(&current).map_err(|_| io::ErrorKind::InvalidData)?;
        if value["subscription"] == "claude_code" {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if current.as_slice() == expected
            && read(d, key_name)?.as_deref().map(|v| v.as_slice()) == Some(key)
        {
            return Ok(true);
        }
        let revision = revision_of(&current);
        if revision.is_some() && replace == revision && revision != Some(request) {
            return Ok(false);
        }
        return Err(io::ErrorKind::AlreadyExists.into());
    }
    Ok(false)
}
/// Remove key files no longer referenced by the published config (a replaced
/// key, or one orphaned by a crash before publication). Best effort: the
/// config is already durable, so a failure here is never reported as one.
fn prune_keys(d: &File, keep: &str) {
    let Ok(entries) = fs::read_dir(format!("/proc/self/fd/{}", d.as_raw_fd())) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let Some(name) = name.to_str() else { continue };
        let strict = name
            .strip_prefix(KEY_PREFIX)
            .and_then(|id| id.parse::<Uuid>().ok())
            .is_some_and(|id| format!("{KEY_PREFIX}{id}") == name);
        if strict && name != keep {
            let _ = unlink(d, name);
        }
    }
}
fn save(
    d: &File,
    expected: &[u8],
    key_name: &str,
    key: &[u8],
    request: Uuid,
    replace: Option<Uuid>,
) -> io::Result<()> {
    if existing(d, expected, key_name, key, request, replace)? {
        return d.sync_all();
    }
    match read(d, key_name)? {
        Some(staged) if staged.as_slice() == key => (),
        Some(_) => return Err(io::ErrorKind::AlreadyExists.into()),
        None => {
            let mut file = open(d, key_name, libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL)?;
            file.write_all(key)?;
            file.sync_all()?;
        }
    }
    // The key is immutable and durable before publishing the config. A crash
    // leaves an inert private key; only the exact request can adopt it. The
    // previous key stays until the new config is durable.
    d.sync_all()?;
    if read(d, PENDING_FILE)?.is_some() {
        unlink(d, PENDING_FILE)?;
    }
    let mut file = open(
        d,
        PENDING_FILE,
        libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
    )?;
    file.write_all(expected)?;
    file.sync_all()?;
    let from = CString::new(PENDING_FILE).unwrap();
    let to = CString::new(CONFIG_FILE).unwrap();
    if unsafe { libc::renameat(d.as_raw_fd(), from.as_ptr(), d.as_raw_fd(), to.as_ptr()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    d.sync_all()?;
    prune_keys(d, key_name);
    d.sync_all()
}

/// Crash points of `save` and a provider-free commit, for tests that must prove
/// the registry and the broker stay usable across them.
#[cfg(test)]
pub(crate) mod seam {
    use super::*;
    #[derive(Clone, Copy)]
    pub(crate) enum Crash {
        KeyOnly,
        PendingOnly,
    }
    pub(crate) fn crash_at(dir: &Path, request: Uuid, key: &[u8], at: Crash) {
        let d = model_dir(dir).unwrap();
        let mut file = open(
            &d,
            &format!("{KEY_PREFIX}{request}"),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        file.write_all(key).unwrap();
        file.sync_all().unwrap();
        if matches!(at, Crash::PendingOnly) {
            let mut p = open(
                &d,
                PENDING_FILE,
                libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
            )
            .unwrap();
            p.write_all(b"{\"partial\":").unwrap();
            p.sync_all().unwrap();
        }
    }
    pub(crate) fn save_for_test(dir: &Path, request: Uuid, key: &str, replace: Option<Uuid>) {
        let i = Input {
            request_id: request,
            api_key: Zeroizing::new(key.into()),
            model: "claude-synthetic".into(),
            effort: Some("medium".into()),
            replace_revision: replace,
        };
        let d = model_dir(dir).unwrap();
        let c = config(dir, &i).unwrap();
        save(
            &d,
            &c,
            &i.key_name(),
            i.api_key.as_bytes(),
            request,
            replace,
        )
        .unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        seam::{Crash, crash_at},
        *,
    };
    fn input() -> Input {
        Input {
            request_id: Uuid::new_v4(),
            api_key: Zeroizing::new("synthetic-api-key".into()),
            model: "claude-synthetic".into(),
            effort: Some("medium".into()),
            replace_revision: None,
        }
    }
    fn ex(d: &File, c: &[u8], i: &Input) -> io::Result<bool> {
        existing(
            d,
            c,
            &i.key_name(),
            i.api_key.as_bytes(),
            i.request_id,
            i.replace_revision,
        )
    }
    fn sv(d: &File, c: &[u8], i: &Input) -> io::Result<()> {
        save(
            d,
            c,
            &i.key_name(),
            i.api_key.as_bytes(),
            i.request_id,
            i.replace_revision,
        )
    }
    fn tmp() -> (tempfile::TempDir, PathBuf, File) {
        let t = tempfile::tempdir().unwrap();
        let p = t.path().canonicalize().unwrap();
        fs::set_permissions(&p, fs::Permissions::from_mode(0o700)).unwrap();
        let d = model_dir(&p).unwrap();
        (t, p, d)
    }
    #[test]
    fn private_save_replay_and_conflicting_request() {
        let (_t, p, d) = tmp();
        let i = input();
        let c = config(&p, &i).unwrap();
        assert!(!ex(&d, &c, &i).unwrap());
        sv(&d, &c, &i).unwrap();
        assert!(ex(&d, &c, &i).unwrap());
        sv(&d, &c, &i).unwrap();
        let other = input();
        let next = config(&p, &other).unwrap();
        assert_eq!(
            sv(&d, &next, &other).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert_eq!(read(&d, CONFIG_FILE).unwrap().unwrap().as_slice(), c);
        assert_eq!(
            fs::metadata(p.join(i.key_name())).unwrap().mode() & 0o777,
            0o600
        );
        assert_eq!(revision_of(&c), Some(i.request_id));
    }
    #[test]
    fn interrupted_key_is_adopted_only_by_exact_request_and_key() {
        let (_t, p, d) = tmp();
        let i = input();
        let c = config(&p, &i).unwrap();
        let mut key = open(
            &d,
            &i.key_name(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        key.write_all(i.api_key.as_bytes()).unwrap();
        key.sync_all().unwrap();
        let mut wrong = input();
        wrong.request_id = i.request_id;
        wrong.api_key = Zeroizing::new("different-key".into());
        assert_eq!(
            sv(&d, &c, &wrong).unwrap_err().kind(),
            io::ErrorKind::AlreadyExists
        );
        assert!(read(&d, CONFIG_FILE).unwrap().is_none());
        sv(&d, &c, &i).unwrap();
    }
    #[test]
    fn replacement_needs_the_current_revision_keeps_the_old_key_until_durable_then_prunes() {
        let (_t, p, d) = tmp();
        let first = input();
        let c1 = config(&p, &first).unwrap();
        sv(&d, &c1, &first).unwrap();
        // No / stale / self-referential revisions never replace.
        let mut next = input();
        next.api_key = Zeroizing::new("replacement-key".into());
        let c2 = config(&p, &next).unwrap();
        for replace in [None, Some(Uuid::new_v4()), Some(next.request_id)] {
            next.replace_revision = replace;
            assert_eq!(
                ex(&d, &c2, &next).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
            assert_eq!(
                sv(&d, &c2, &next).unwrap_err().kind(),
                io::ErrorKind::AlreadyExists
            );
        }
        assert_eq!(read(&d, CONFIG_FILE).unwrap().unwrap().as_slice(), c1);
        // Crash after the new key is staged: the old config and key still work.
        next.replace_revision = Some(first.request_id);
        let mut staged = open(
            &d,
            &next.key_name(),
            libc::O_WRONLY | libc::O_CREAT | libc::O_EXCL,
        )
        .unwrap();
        staged.write_all(next.api_key.as_bytes()).unwrap();
        assert_eq!(read(&d, CONFIG_FILE).unwrap().unwrap().as_slice(), c1);
        assert_eq!(
            read(&d, &first.key_name()).unwrap().unwrap().as_slice(),
            first.api_key.as_bytes()
        );
        // Retry adopts the staged key, publishes, then prunes the old key.
        assert!(!ex(&d, &c2, &next).unwrap());
        sv(&d, &c2, &next).unwrap();
        assert_eq!(read(&d, CONFIG_FILE).unwrap().unwrap().as_slice(), c2);
        assert!(!p.join(first.key_name()).exists());
        assert!(p.join(next.key_name()).exists());
        // The replaced request cannot be revived without the new revision.
        let mut back = first_again(&first);
        back.replace_revision = Some(first.request_id);
        assert_eq!(
            sv(&d, &config(&p, &back).unwrap(), &back)
                .unwrap_err()
                .kind(),
            io::ErrorKind::AlreadyExists
        );
        // An exact retry of the replacement is idempotent.
        assert!(ex(&d, &c2, &next).unwrap());
    }
    fn first_again(i: &Input) -> Input {
        Input {
            request_id: i.request_id,
            api_key: i.api_key.clone(),
            model: i.model.clone(),
            effort: i.effort.clone(),
            replace_revision: None,
        }
    }
    #[test]
    fn crash_orphans_are_pruned_by_the_next_successful_save() {
        let (_t, p, d) = tmp();
        let orphan = Uuid::new_v4();
        crash_at(&p, orphan, b"orphan-key-material", Crash::PendingOnly);
        let i = input();
        let c = config(&p, &i).unwrap();
        sv(&d, &c, &i).unwrap();
        assert!(!p.join(format!("{KEY_PREFIX}{orphan}")).exists());
        fs::write(p.join("model-api-key-not-a-uuid"), b"x").unwrap();
        fs::write(p.join("unrelated"), b"x").unwrap();
        prune_keys(&d, &i.key_name());
        assert!(p.join("model-api-key-not-a-uuid").exists() && p.join("unrelated").exists());
        assert!(p.join(i.key_name()).exists());
    }
    #[test]
    fn symlink_and_public_file_are_refused() {
        let (_t, p, d) = tmp();
        let other = p.join("other");
        fs::write(&other, b"unchanged").unwrap();
        std::os::unix::fs::symlink(&other, p.join(CONFIG_FILE)).unwrap();
        assert!(read(&d, CONFIG_FILE).is_err());
        assert_eq!(fs::read(&other).unwrap(), b"unchanged");
        fs::remove_file(p.join(CONFIG_FILE)).unwrap();
        fs::write(p.join(CONFIG_FILE), b"{}").unwrap();
        fs::set_permissions(p.join(CONFIG_FILE), fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read(&d, CONFIG_FILE).is_err());
    }
    #[test]
    fn model_directory_must_be_private_canonical_and_not_a_symlink() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().canonicalize().unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let real = root.join("real");
        fs::create_dir(&real).unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(model_dir(&real).is_ok());
        let link = root.join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        assert!(model_dir(&link).is_err());
        assert!(model_dir(&real.join("..").join("real")).is_err());
        assert!(model_dir(Path::new("relative")).is_err());
        fs::set_permissions(&real, fs::Permissions::from_mode(0o750)).unwrap();
        assert!(model_dir(&real).is_err());
        assert!(model_dir(&root.join("missing")).is_err());
    }
    #[test]
    fn bounded_input_cannot_choose_host_or_worker() {
        let mut i = input();
        assert!(i.valid());
        i.model = "other-model".into();
        assert!(!i.valid());
        i = input();
        i.api_key = Zeroizing::new("secret\nheader".into());
        assert!(!i.valid());
        i = input();
        i.replace_revision = Some(i.request_id);
        assert!(!i.valid());
        i.replace_revision = Some(Uuid::new_v4());
        assert!(i.valid());
        assert!(serde_json::from_value::<Input>(serde_json::json!({"request_id":Uuid::new_v4(),"api_key":"synthetic","model":"claude-synthetic","effort":null,"base_url":"https://wrong.example"})).is_err());
    }
}
