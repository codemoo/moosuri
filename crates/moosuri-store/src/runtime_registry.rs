//! Offline installation-owned account allocation. No engine access or user secrets.
use crate::{Result, StoreError, email_valid, files};
use fs2::FileExt;
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::{
    fs,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::Path,
    time::Duration,
};
use uuid::Uuid;

pub mod admin;
pub mod engine;

const REGISTRY: &str = "account-runtimes.sqlite";
pub const GOOGLE_WEB_CLIENT_FILE: &str = "google-web-client.json";
pub const GOOGLE_WEB_CLIENT_PENDING_FILE: &str = ".google-web-client.pending";
const APP_ID: i64 = 0x4d535252;

#[derive(Debug, Clone, Eq, PartialEq, Serialize)]
pub struct Allocation {
    pub account_id: String,
    pub tenant_id: String,
    pub volume_id: String,
    pub generation: i64,
}

/// Use a directory created for this registry by the installer. A legacy assistant
/// root, a symlink, or a directory containing unrelated files is never adopted.
fn open(root: &Path, create: bool) -> Result<(fs::File, Connection)> {
    let meta = fs::symlink_metadata(root).map_err(|_| StoreError::Unavailable)?;
    if !meta.is_dir()
        || meta.uid() != unsafe { libc::geteuid() }
        || meta.mode() & 0o077 != 0
        || fs::canonicalize(root).map_err(|_| StoreError::InvalidInput)? != root
    {
        return Err(StoreError::InvalidInput);
    }
    // Serialize the directory inspection with journal creation and unlink.
    let directory = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
        .open(root)
        .map_err(|_| StoreError::Unavailable)?;
    FileExt::lock_exclusive(&directory).map_err(|_| StoreError::Unavailable)?;
    let opened = directory.metadata().map_err(|_| StoreError::Unavailable)?;
    if opened.dev() != meta.dev()
        || opened.ino() != meta.ino()
        || opened.uid() != meta.uid()
        || opened.mode() & 0o077 != 0
    {
        return Err(StoreError::InvalidInput);
    }
    for item in fs::read_dir(root).map_err(|_| StoreError::Unavailable)? {
        let item = item.map_err(|_| StoreError::Unavailable)?;
        let name = item.file_name();
        // Google setup checks these independently so that a bad client file or
        // an interrupted replacement cannot stop unrelated member routing.
        if matches!(
            name.to_str(),
            Some(GOOGLE_WEB_CLIENT_FILE | GOOGLE_WEB_CLIENT_PENDING_FILE)
        ) {
            continue;
        }
        let m = fs::symlink_metadata(item.path()).map_err(|_| StoreError::InvalidInput)?;
        if !matches!(
            name.to_str(),
            Some("account-runtimes.sqlite" | "account-runtimes.sqlite-journal")
        ) || !m.is_file()
            || m.nlink() != 1
            || m.uid() != meta.uid()
            || m.mode() & 0o077 != 0
        {
            return Err(StoreError::InvalidInput);
        }
    }
    let file = root.join(REGISTRY);
    if create {
        files::private_file(&file)?;
    }
    // Recovery of a hot SQLite rollback journal needs a writable handle even
    // for lookup. No CREATE flag: a lookup cannot create a missing registry.
    let mut db = Connection::open_with_flags(
        &file,
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    db.busy_timeout(Duration::from_secs(5))?;
    db.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA synchronous=FULL;")?;
    if create {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: i64 = tx.query_row("PRAGMA application_id", [], |r| r.get(0))?;
        if id == 0 {
            let existing: bool =
                tx.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_schema)", [], |r| {
                    r.get(0)
                })?;
            if existing {
                return Err(StoreError::InvalidInput);
            }
            tx.execute_batch("CREATE TABLE allocations (
                request_id TEXT PRIMARY KEY, email TEXT NOT NULL UNIQUE, account_id TEXT NOT NULL UNIQUE,
                tenant_id TEXT NOT NULL UNIQUE, volume_id TEXT NOT NULL UNIQUE,
                generation INTEGER NOT NULL CHECK(generation>0),
                route_state TEXT NOT NULL DEFAULT 'reserved' CHECK(route_state IN ('reserved','ready','fenced'))
            ) STRICT; PRAGMA user_version=1;")?;
            tx.pragma_update(None, "application_id", APP_ID)?;
        } else if id != APP_ID {
            return Err(StoreError::InvalidInput);
        }
        tx.commit()?;
    }
    let id: i64 = db.query_row("PRAGMA application_id", [], |r| r.get(0))?;
    let version: i64 = db.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if id != APP_ID || !(1..=9).contains(&version) {
        return Err(StoreError::InvalidInput);
    }
    if version == 1 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0002_administrators.sql"
        ))?;
        tx.commit()?;
    }
    if version <= 2 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0003_web_session_epoch.sql"
        ))?;
        tx.commit()?;
    }
    if version <= 3 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0004_engine_policy.sql"
        ))?;
        tx.commit()?;
    }
    if version <= 4 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0005_member_name_engine_mode.sql"
        ))?;
        tx.commit()?;
    }
    if version <= 5 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0006_engine_quota.sql"
        ))?;
        tx.commit()?;
    }
    if version <= 6 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0007_quota_receipts.sql"
        ))?;
        tx.commit()?;
    }
    // `version` was read before these transactions. Concurrent migrators are excluded only by
    // the exclusive directory lock held for the lifetime of the returned handle (not by the
    // SQLite transaction), so never move the version read or that lock acquisition apart.
    if version <= 7 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0008_google_bootstrap.sql"
        ))?;
        tx.commit()?;
    }
    if version <= 8 {
        let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
        tx.execute_batch(include_str!(
            "../../../migrations/installation_registry/0009_google_console_steps.sql"
        ))?;
        tx.commit()?;
    }
    Ok((directory, db))
}
fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Allocation> {
    Ok(Allocation {
        account_id: row.get(0)?,
        tenant_id: row.get(1)?,
        volume_id: row.get(2)?,
        generation: row.get(3)?,
    })
}
/// The caller retries with the same request id. A lost reply must not mint a
/// second account or authorize a conflicting invite to the same address.
pub fn reserve(root: &Path, request: Uuid, email: String) -> Result<Allocation> {
    let email = email_valid(email)?;
    let (_directory, mut db) = open(root, true)?;
    let tx = db.transaction_with_behavior(TransactionBehavior::Immediate)?;
    admin::guard_reserve(&tx, request, &email)?;
    let result = reserve_in(&tx, request, email)?;
    tx.commit()?;
    fs::File::open(root)
        .and_then(|f| f.sync_all())
        .map_err(|_| StoreError::Unavailable)?;
    Ok(result)
}
// Requires the caller's registry directory lock and immediate transaction.
fn reserve_in(tx: &Connection, request: Uuid, email: String) -> Result<Allocation> {
    let request = request.to_string();
    let existing=tx.query_row(
        "SELECT account_id,tenant_id,volume_id,generation,email FROM allocations WHERE request_id=?1",
        [&request],|r|Ok((decode(r)?,r.get::<_,String>(4)?)),
    ).optional()?;
    let result = match existing {
        Some((allocation, previous)) if previous == email => allocation,
        Some(_) => return Err(StoreError::Conflict),
        None => {
            let allocation = Allocation {
                account_id: Uuid::new_v4().to_string(),
                tenant_id: Uuid::new_v4().to_string(),
                volume_id: Uuid::new_v4().to_string(),
                generation: 1,
            };
            tx.execute("INSERT INTO allocations(request_id,email,account_id,tenant_id,volume_id,generation) VALUES(?1,?2,?3,?4,?5,?6)",params![request,email,allocation.account_id,allocation.tenant_id,allocation.volume_id,allocation.generation])?;
            allocation
        }
    };
    Ok(result)
}
/// Compare an invite retry with its original allocation and current route state.
/// Only control metadata is read; no personal store or credential is opened.
#[derive(Debug, Clone, Copy, Eq, PartialEq)]
pub enum InviteState {
    Reserved,
    Ready,
    Fenced,
}
pub fn invitation_state(
    root: &Path,
    request: Uuid,
    allocation: &Allocation,
) -> Result<InviteState> {
    let (_directory, db) = open(root, false)?;
    let state:String=db.query_row("SELECT route_state FROM allocations WHERE request_id=?1 AND account_id=?2 AND tenant_id=?3 AND volume_id=?4 AND generation=?5 AND NOT EXISTS(SELECT 1 FROM installation_members m WHERE m.account_id=allocations.account_id AND m.tenant_id=allocations.tenant_id AND m.volume_id=allocations.volume_id AND m.generation=allocations.generation AND m.suspended=1)",params![request.to_string(),allocation.account_id,allocation.tenant_id,allocation.volume_id,allocation.generation],|r|r.get(0)).optional()?.ok_or(StoreError::Conflict)?;
    match state.as_str() {
        "reserved" => Ok(InviteState::Reserved),
        "ready" => Ok(InviteState::Ready),
        "fenced" => Ok(InviteState::Fenced),
        _ => Err(StoreError::Corrupt),
    }
}
/// Invite completion cannot revive a fenced route or adopt another request.
pub fn activate_invitation(root: &Path, request: Uuid, allocation: &Allocation) -> Result<()> {
    let (_directory, db) = open(root, false)?;
    let changed=db.execute("UPDATE allocations SET route_state='ready' WHERE request_id=?1 AND account_id=?2 AND tenant_id=?3 AND volume_id=?4 AND generation=?5 AND route_state='reserved' AND NOT EXISTS(SELECT 1 FROM installation_members m WHERE m.account_id=allocations.account_id AND m.tenant_id=allocations.tenant_id AND m.volume_id=allocations.volume_id AND m.generation=allocations.generation AND m.suspended=1)",params![request.to_string(),allocation.account_id,allocation.tenant_id,allocation.volume_id,allocation.generation])?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

pub fn lookup(root: &Path, email: String) -> Result<Allocation> {
    lookup_email(root, email, false)
}

pub fn lookup_ready(root: &Path, email: String) -> Result<Allocation> {
    lookup_email(root, email, true)
}
fn lookup_email(root: &Path, email: String, ready: bool) -> Result<Allocation> {
    let email = email_valid(email)?;
    let (_directory, db) = open(root, false)?;
    db.query_row(
        "SELECT account_id,tenant_id,volume_id,generation FROM allocations WHERE email=?1 AND (?2=0 OR (route_state='ready' AND NOT EXISTS(SELECT 1 FROM installation_members m WHERE m.account_id=allocations.account_id AND m.tenant_id=allocations.tenant_id AND m.volume_id=allocations.volume_id AND m.generation=allocations.generation AND m.suspended=1)))",
        params![email, ready],
        decode,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

/// Public sessions need the same allocation and its uninterrupted routing epoch.
pub enum SessionDestination {
    Account(Uuid),
    Email(String),
}
pub fn session_route(root: &Path, destination: SessionDestination) -> Result<(Allocation, i64)> {
    with_session_route(root, destination, |a, epoch| Ok((a, epoch)))
}
fn with_session_route<T>(
    root: &Path,
    destination: SessionDestination,
    action: impl FnOnce(Allocation, i64) -> Result<T>,
) -> Result<T> {
    let (_lock, db) = open(root, false)?;
    let (a, epoch) = ready_session(&db, destination)?;
    action(a, epoch)
}

fn ready_session(db: &Connection, destination: SessionDestination) -> Result<(Allocation, i64)> {
    let (column, value) = match destination {
        SessionDestination::Account(id) => ("account_id", id.to_string()),
        SessionDestination::Email(email) => ("email", email_valid(email)?),
    };
    let sql = format!(
        "SELECT account_id,tenant_id,volume_id,generation,session_epoch FROM allocations WHERE {column}=?1 AND route_state='ready'"
    );
    let (a, epoch) = db
        .query_row(&sql, [value], |r| Ok((decode(r)?, r.get(4)?)))
        .optional()?
        .ok_or(StoreError::NotFound)?;
    admin::unsuspended(db, &a)?;
    Ok((a, epoch))
}

/// Installation-only identity lookup for preparing an inert socket or mount.
/// This never grants routing, dispatch, or permission to reuse a session.
pub fn lookup_allocated_id(root: &Path, account_id: Uuid) -> Result<Allocation> {
    let (_directory, db) = open(root, false)?;
    db.query_row(
        "SELECT account_id,tenant_id,volume_id,generation FROM allocations WHERE account_id=?1",
        [account_id.to_string()],
        decode,
    )
    .optional()?
    .ok_or(StoreError::NotFound)
}

/// Recheck a signed route's immutable account and current generation.
pub fn lookup_id(root: &Path, account_id: Uuid) -> Result<Allocation> {
    with_ready_allocation(root, account_id, Ok)
}

/// Serialize a short local admission/ledger transition against gateway fencing.
/// The callback must not call the registry again or perform any network IO.
pub fn with_ready_allocation<T>(
    root: &Path,
    account_id: Uuid,
    action: impl FnOnce(Allocation) -> Result<T>,
) -> Result<T> {
    with_session_route(root, SessionDestination::Account(account_id), |a, _| {
        action(a)
    })
}

/// Keep a call pinned to the uninterrupted session route through the local ledger write.
pub fn with_ready_epoch<T>(
    root: &Path,
    account_id: Uuid,
    expected: i64,
    action: impl FnOnce(Allocation) -> Result<T>,
) -> Result<T> {
    with_session_route(root, SessionDestination::Account(account_id), |a, epoch| {
        if epoch != expected {
            return Err(StoreError::Forbidden);
        }
        action(a)
    })
}

/// Operator compare-and-set after the verified personal process has answered.
pub fn set_routing(root: &Path, allocation: &Allocation, enable: bool) -> Result<()> {
    let (_directory, db) = open(root, false)?;
    if enable {
        admin::unsuspended(&db, allocation)?;
    } else {
        admin::guard_fence(&db, allocation)?;
    }
    let changed = db.execute(
        "UPDATE allocations SET route_state=?1 WHERE account_id=?2 AND tenant_id=?3 AND volume_id=?4 AND generation=?5 AND (?1='fenced' OR route_state IN ('reserved','ready'))",
        params![if enable {"ready"} else {"fenced"},allocation.account_id,allocation.tenant_id,allocation.volume_id,allocation.generation])?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    // A dying writer leaves SQLite's rollback journal on disk. The first
    // lookup after restart must restore the committed registry before routing.
    #[test]
    fn lookup_recovers_a_crashed_registry_writer() {
        const CHILD: &str = "MOOSURI_SYNTHETIC_REGISTRY_CRASH_DIR";
        if let Some(root) = std::env::var_os(CHILD) {
            let root = std::path::PathBuf::from(root);
            let (_lock, db) = open(&root, false).unwrap();
            db.execute_batch("PRAGMA cache_size=2; BEGIN IMMEDIATE;
                UPDATE allocations SET email='discarded@example.test';
                CREATE TABLE disposable(payload BLOB); INSERT INTO disposable VALUES(zeroblob(1048576));").unwrap();
            std::process::exit(73);
        }
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let original = reserve(&root, Uuid::new_v4(), "before@example.test".into()).unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "runtime_registry::tests::lookup_recovers_a_crashed_registry_writer",
            ])
            .env(CHILD, &root)
            .status()
            .unwrap();
        assert_eq!(status.code(), Some(73));
        assert!(root.join("account-runtimes.sqlite-journal").is_file());
        assert_eq!(
            lookup(&root, "before@example.test".into()).unwrap(),
            original
        );
        assert!(!root.join("account-runtimes.sqlite-journal").exists());
    }
    #[test]
    fn retries_and_concurrent_invites_cannot_assign_two_volumes() {
        use std::{sync::Barrier, thread};
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        fs::set_permissions(&root, std::os::unix::fs::PermissionsExt::from_mode(0o700)).unwrap();
        let id = Uuid::new_v4();
        let start = Barrier::new(4);
        let allocations = thread::scope(|s| {
            (0..4)
                .map(|_| {
                    s.spawn(|| {
                        start.wait();
                        reserve(&root, id, "A@example.test".into()).unwrap()
                    })
                })
                .collect::<Vec<_>>()
                .into_iter()
                .map(|job| job.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert!(allocations.windows(2).all(|pair| pair[0] == pair[1]));
        assert_eq!(
            lookup(&root, "a@example.test".into()).unwrap(),
            allocations[0]
        );
        assert!(matches!(
            reserve(&root, id, "b@example.test".into()),
            Err(StoreError::Conflict)
        ));
        assert!(matches!(
            reserve(&root, Uuid::new_v4(), "a@example.test".into()),
            Err(StoreError::Conflict)
        ));
        let b = reserve(&root, Uuid::new_v4(), "b@example.test".into()).unwrap();
        assert_ne!(allocations[0].volume_id, b.volume_id);
        let aid = Uuid::parse_str(&allocations[0].account_id).unwrap();
        assert!(lookup_id(&root, aid).is_err());
        set_routing(&root, &allocations[0], true).unwrap();
        assert_eq!(
            lookup_ready(&root, "A@example.test".into()).unwrap(),
            allocations[0]
        );
        set_routing(&root, &allocations[0], false).unwrap();
        assert!(lookup_id(&root, aid).is_err());
        assert!(set_routing(&root, &allocations[0], true).is_err());
        fs::write(root.join("assistant.sqlite"), b"legacy root").unwrap();
        assert!(matches!(
            reserve(&root, Uuid::new_v4(), "c@example.test".into()),
            Err(StoreError::InvalidInput)
        ));
    }
}

#[cfg(test)]
mod invite_tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;
    #[test]
    fn invite_activation_is_request_bound_and_never_unfences() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path().canonicalize().unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        let request = Uuid::new_v4();
        let a = reserve(&root, request, "new@example.invalid".into()).unwrap();
        assert_eq!(
            reserve(&root, request, "NEW@example.invalid".into()).unwrap(),
            a
        );
        assert!(reserve(&root, Uuid::new_v4(), "new@example.invalid".into()).is_err());
        assert_eq!(
            invitation_state(&root, request, &a).unwrap(),
            InviteState::Reserved
        );
        assert!(activate_invitation(&root, Uuid::new_v4(), &a).is_err());
        activate_invitation(&root, request, &a).unwrap();
        assert_eq!(
            invitation_state(&root, request, &a).unwrap(),
            InviteState::Ready
        );
        assert!(activate_invitation(&root, request, &a).is_err());
        set_routing(&root, &a, false).unwrap();
        assert_eq!(
            invitation_state(&root, request, &a).unwrap(),
            InviteState::Fenced
        );
        assert!(activate_invitation(&root, request, &a).is_err());
        assert!(lookup_ready(&root, "new@example.invalid".into()).is_err());
    }
}

/// Installation timer discovery. A returned ID still requires a fresh
/// session_route check for suspension and generation before member admission.
pub fn scheduled_member_page(root: &Path, after: String) -> Result<Vec<Uuid>> {
    let (_lock, db) = open(root, false)?;
    let mut stmt=db.prepare("SELECT account_id FROM allocations WHERE route_state='ready' AND account_id>?1 ORDER BY account_id LIMIT 16")?;
    let ids = stmt
        .query_map([after], |r| r.get::<_, String>(0))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    ids.into_iter()
        .map(|v| Uuid::parse_str(&v).map_err(|_| StoreError::Corrupt))
        .collect()
}
