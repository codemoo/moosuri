//! Owner-only, offline sealed-current cutover. Never opens a vault envelope.
use crate::{
    database::{Database, Intent, JOURNAL_LIMIT, MAGIC},
    *,
};
use rusqlite::{Connection, params, types::Value};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

const FORMAT: &str = "moosuri-vault-sealed-current";
const WIRE: &[u8; 8] = b"MVSC0001";
const DB_LIMIT: u64 = 512 * 1024 * 1024;
const ARCHIVE_LIMIT: u64 = DB_LIMIT + JOURNAL_LIMIT + 1024 * 1024;
const JSON_LIMIT: u64 = 16 * 1024;
const STAGE_PREFIX: &str = ".vault-recovery-stage-";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Seal {
    format: String,
    version: u32,
    checkpoint: uuid::Uuid,
    source: uuid::Uuid,
    sequence: i64,
    head: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    seal: Seal,
    database_size: u64,
    database_sha256: String,
    journal_size: u64,
    journal_sha256: String,
}
/// Retain this exact head through a separately trusted operator handoff.
/// It selects one ciphertext; it does not prove independently current history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedHead {
    seal: Seal,
    ciphertext_size: u64,
    ciphertext_sha256: String,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Provenance {
    format: String,
    version: u32,
    trusted: TrustedHead,
    destination: uuid::Uuid,
}

#[allow(unsafe_code)]
pub(crate) fn current_uid() -> u32 {
    // SAFETY: geteuid has no preconditions and returns only process identity.
    unsafe { libc::geteuid() }
}
/// Reject linked or other-UID-mutable ancestors, including absent outputs.
/// Root/current-UID ownership makes mode stability a trusted-operator boundary.
/// Sticky shared directories are safe only while every following existing entry
/// is also root/current-UID owned, so an unrelated UID cannot rename that entry.
pub(crate) fn check_components(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::Invalid);
    }
    let uid = current_uid();
    let mut current = PathBuf::new();
    for part in path.components() {
        if matches!(part, Component::ParentDir | Component::CurDir) {
            return Err(Error::Invalid);
        }
        current.push(part.as_os_str());
        match fs::symlink_metadata(&current) {
            Ok(m) => {
                if m.file_type().is_symlink() || (m.uid() != 0 && m.uid() != uid) {
                    return Err(Error::Unavailable);
                }
                let mode = m.permissions().mode();
                if m.is_dir() && mode & 0o022 != 0 && mode & 0o1000 == 0 {
                    return Err(Error::Unavailable);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
            Err(_) => return Err(Error::Unavailable),
        }
    }
    Ok(())
}
pub(crate) fn reserved_stage_name(path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| name.as_bytes().starts_with(STAGE_PREFIX.as_bytes()))
}
fn private_dir(path: &Path) -> Result<()> {
    check_components(path)?;
    let m = fs::symlink_metadata(path)?;
    if !m.is_dir() || m.uid() != current_uid() || m.permissions().mode() & 0o077 != 0 {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn open_private(path: &Path, limit: u64) -> Result<File> {
    check_components(path)?;
    let f = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?;
    let m = f.metadata()?;
    if !m.is_file()
        || m.uid() != current_uid()
        || m.nlink() != 1
        || m.permissions().mode() & 0o077 != 0
    {
        return Err(Error::Unavailable);
    }
    if m.len() > limit {
        return Err(Error::Limit);
    }
    Ok(f)
}
fn new_private(path: &Path) -> Result<File> {
    check_components(path)?;
    Ok(OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)?)
}
fn sync_dir(path: &Path) -> Result<()> {
    File::open(path)?.sync_all()?;
    Ok(())
}
fn write_new(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = new_private(path)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    sync_dir(path.parent().ok_or(Error::Invalid)?)
}
fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T> {
    let f = open_private(path, JSON_LIMIT)?;
    serde_json::from_reader(f).map_err(|_| Error::Invalid)
}
fn encode<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    serde_json::to_vec(value).map_err(|_| Error::Invalid)
}
fn digest_file(path: &Path, limit: u64) -> Result<(u64, String)> {
    let mut f = open_private(path, limit)?;
    digest_input(&mut f, limit)
}
fn digest_input(f: &mut File, limit: u64) -> Result<(u64, String)> {
    f.seek(SeekFrom::Start(0))?;
    let size = f.metadata()?.len();
    if size > limit {
        return Err(Error::Limit);
    }
    let mut consumed = 0u64;
    let mut h = Sha256::new();
    let mut buf = [0u8; 65536];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        consumed = consumed.checked_add(n as u64).ok_or(Error::Limit)?;
        if consumed > limit {
            return Err(Error::Limit);
        }
        h.update(&buf[..n]);
    }
    if consumed != size {
        return Err(Error::Unavailable);
    }
    f.seek(SeekFrom::Start(0))?;
    Ok((size, format!("{:x}", h.finalize())))
}
struct Stage(PathBuf);
impl Stage {
    fn new(parent: &Path) -> Result<Self> {
        private_dir(parent)?;
        let p = parent.join(format!("{STAGE_PREFIX}{}", uuid::Uuid::new_v4()));
        fs::DirBuilder::new().mode(0o700).create(&p)?;
        Ok(Self(p))
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn installation(db: &Database) -> Result<uuid::Uuid> {
    let id: String = db
        .conn
        .query_row("SELECT installation FROM meta", [], |r| r.get(0))?;
    id.parse().map_err(|_| Error::Unavailable)
}

fn rows(conn: &Connection, query: &str) -> Result<Vec<Vec<Value>>> {
    let mut s = conn.prepare(query)?;
    let columns = s.column_count();
    s.query_map([], |r| (0..columns).map(|i| r.get(i)).collect())?
        .collect::<std::result::Result<_, _>>()
        .map_err(Into::into)
}
/// Recompute every durable ciphertext effect, CAS outcome, receipt, nonce reservation
/// and tombstone from the exact admitted journal. Session creation is intentionally
/// not journaled; owners are copied only as non-authoritative namespace anchors.
fn verify_semantics(db: &Database, journal_path: &Path, parent: &Path) -> Result<()> {
    let expected = Connection::open_in_memory()?;
    expected.execute_batch(include_str!("schema.sql"))?;
    let schema = "SELECT type,name,tbl_name,sql FROM sqlite_schema ORDER BY type,name";
    if rows(&db.conn, schema)? != rows(&expected, schema)? {
        return Err(Error::Unavailable);
    }
    let integrity: String = db
        .conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))?;
    if integrity != "ok" || !rows(&db.conn, "PRAGMA foreign_key_check")?.is_empty() {
        return Err(Error::Unavailable);
    }
    let replay = Stage::new(parent)?;
    let mut rebuilt = Database::open_recovery(&replay.0)?;
    for row in rows(
        &db.conn,
        "SELECT tenant,owner FROM owners ORDER BY tenant,owner",
    )? {
        rebuilt
            .conn
            .execute("INSERT INTO owners VALUES(?1,?2)", params![row[0], row[1]])?;
    }
    let mut f = open_private(journal_path, JOURNAL_LIMIT)?;
    let mut prefix = [0; 24];
    f.read_exact(&mut prefix)?;
    loop {
        let mut len = [0; 4];
        let n = f.read(&mut len)?;
        if n == 0 {
            break;
        }
        if n != 4 {
            return Err(Error::Unavailable);
        }
        let length = u32::from_be_bytes(len) as usize;
        if length > MAX_BODY + 4096 {
            return Err(Error::Limit);
        }
        let mut body = vec![0; length];
        f.read_exact(&mut body)?;
        let mut head = [0; 32];
        f.read_exact(&mut head)?;
        let intent: Intent = serde_json::from_slice(&body).map_err(|_| Error::Unavailable)?;
        match intent {
            Intent::Prepare { owner, operation } => {
                rebuilt.prepare(&owner, operation)?;
            }
            Intent::Adopt {
                owner,
                operation,
                digest,
            } => {
                rebuilt.commit(&owner, operation, &digest)?;
            }
            Intent::Cancel {
                owner,
                operation,
                digest,
            } => {
                if hash(&encode(&operation)?) != digest {
                    return Err(Error::Unavailable);
                }
                rebuilt.cancel(&owner, operation.id(), operation)?;
            }
            Intent::Close { .. } => (),
        }
    }
    for (table, columns) in [
        ("owners", 2),
        ("vaults", 4),
        ("headers", 5),
        ("items", 6),
        ("item_versions", 5),
        ("operations", 7),
        ("nonce_ledger", 6),
    ] {
        let order = (1..=columns)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("SELECT * FROM {table} ORDER BY {order}");
        if rows(&db.conn, &query)? != rows(&rebuilt.conn, &query)? {
            return Err(Error::Unavailable);
        }
    }
    Ok(())
}

/// Permanently retire a stopped source, then encrypt its exact final checkpoint.
/// An output failure after sealing intentionally leaves the source retired.
pub fn checkpoint(
    root: &Path,
    recipient: &str,
    output: &Path,
    trusted_head: &Path,
    seal_source: bool,
) -> Result<TrustedHead> {
    if !seal_source {
        return Err(Error::Invalid);
    }
    private_dir(root)?;
    let parent = output.parent().ok_or(Error::Invalid)?;
    private_dir(parent)?;
    private_dir(trusted_head.parent().ok_or(Error::Invalid)?)?;
    check_components(output)?;
    check_components(trusted_head)?;
    if output.starts_with(root)
        || trusted_head.starts_with(root)
        || output == trusted_head
        || output.exists()
        || trusted_head.exists()
    {
        return Err(Error::Conflict);
    }
    let recipient: age::x25519::Recipient = recipient.parse().map_err(|_| Error::Invalid)?;
    // A stopped recovery operation must never initialize absent source evidence.
    open_private(&root.join("vault.sqlite"), DB_LIMIT)?;
    open_private(&root.join("vault.journal"), JOURNAL_LIMIT)?;
    for (name, limit) in [
        ("vault.sqlite-wal", DB_LIMIT),
        ("vault.sqlite-shm", 32 * 1024 * 1024),
    ] {
        if root.join(name).symlink_metadata().is_ok() {
            open_private(&root.join(name), limit)?;
        }
    }
    let db = Database::open_recovery(root)?;
    let seal = Seal {
        format: FORMAT.into(),
        version: 1,
        checkpoint: uuid::Uuid::new_v4(),
        source: installation(&db)?,
        sequence: db.sequence,
        head: db.head.iter().map(|b| format!("{b:02x}")).collect(),
    };
    let seal_path = root.join("vault.sealed");
    let seal = if seal_path.symlink_metadata().is_ok() {
        let old: Seal = read_json(&seal_path)?;
        if old.format != FORMAT
            || old.version != 1
            || old.source != seal.source
            || old.sequence != seal.sequence
            || old.head != seal.head
            || old.checkpoint.is_nil()
        {
            return Err(Error::Unavailable);
        }
        old
    } else {
        write_new(&seal_path, &encode(&seal)?)?;
        seal
    };
    // This fsync is repeated even for retries, before any source bytes are copied.
    sync_dir(root)?;
    #[cfg(test)]
    fail_at("source_sealed")?;
    verify_semantics(&db, &root.join("vault.journal"), parent)?;
    let stage = Stage::new(parent)?;
    let snapshot = stage.0.join("vault.sqlite");
    new_private(&snapshot)?;
    let mut target = Connection::open(&snapshot)?;
    let backup = rusqlite::backup::Backup::new(&db.conn, &mut target)?;
    backup.run_to_completion(128, std::time::Duration::from_millis(1), None)?;
    drop(backup);
    drop(target);
    let (database_size, database_sha256) = digest_file(&snapshot, DB_LIMIT)?;
    let journal_path = root.join("vault.journal");
    let (journal_size, journal_sha256) = digest_file(&journal_path, JOURNAL_LIMIT)?;
    let manifest = encode(&Manifest {
        seal: seal.clone(),
        database_size,
        database_sha256,
        journal_size,
        journal_sha256,
    })?;
    if manifest.len() as u64 > JSON_LIMIT {
        return Err(Error::Limit);
    }
    let partial = stage.0.join("checkpoint.age");
    let f = new_private(&partial)?;
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| Error::Invalid)?;
    let mut writer = encryptor.wrap_output(f).map_err(|_| Error::Unavailable)?;
    writer.write_all(WIRE)?;
    writer.write_all(&(manifest.len() as u32).to_be_bytes())?;
    writer.write_all(&manifest)?;
    for (path, len) in [(&snapshot, database_size), (&journal_path, journal_size)] {
        writer.write_all(&len.to_be_bytes())?;
        let mut input = open_private(path, len)?;
        if std::io::copy(&mut input, &mut writer)? != len {
            return Err(Error::Unavailable);
        }
    }
    let f = writer.finish().map_err(|_| Error::Unavailable)?;
    f.sync_all()?;
    drop(f);
    let (ciphertext_size, ciphertext_sha256) = digest_file(&partial, ARCHIVE_LIMIT)?;
    let head = TrustedHead {
        seal,
        ciphertext_size,
        ciphertext_sha256,
    };
    // Hard-link publication is atomic and cannot replace an existing file.
    fs::hard_link(&partial, output)?;
    fs::remove_file(&partial)?;
    sync_dir(parent)?;
    write_new(trusted_head, &encode(&head)?)?;
    Ok(head)
}
fn identity(path: &Path) -> Result<age::x25519::Identity> {
    let mut text = zeroize::Zeroizing::new(String::new());
    open_private(path, 4096)?.read_to_string(&mut text)?;
    let lines = text
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err(Error::Invalid);
    }
    lines[0].parse().map_err(|_| Error::Invalid)
}
fn extract(
    reader: &mut dyn Read,
    path: &Path,
    expected_size: u64,
    expected_digest: &str,
    limit: u64,
) -> Result<()> {
    let mut length = [0; 8];
    reader.read_exact(&mut length)?;
    if u64::from_be_bytes(length) != expected_size || expected_size > limit {
        return Err(Error::Limit);
    }
    let mut f = new_private(path)?;
    if std::io::copy(&mut reader.take(expected_size), &mut f)? != expected_size {
        return Err(Error::Unavailable);
    }
    f.sync_all()?;
    drop(f);
    if digest_file(path, limit)?.1 != expected_digest {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn quarantine(db: &mut Database) -> Result<()> {
    let mut statement = db.conn.prepare("SELECT tenant,owner,epoch,body FROM operations WHERE status='pending' ORDER BY tenant,owner,id")?;
    let pending = statement
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
                r.get::<_, Vec<u8>>(3)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    drop(statement);
    for (tenant, account, epoch, body) in pending {
        let owner = protocol::Owner {
            tenant: tenant.parse().map_err(|_| Error::Unavailable)?,
            account: account.parse().map_err(|_| Error::Unavailable)?,
            installation: uuid::Uuid::nil(),
            epoch,
        };
        let operation: protocol::Operation =
            serde_json::from_slice(&body).map_err(|_| Error::Unavailable)?;
        db.cancel(&owner, operation.id(), operation)?;
    }
    db.conn.execute("DELETE FROM sessions", [])?;
    Ok(())
}
#[allow(unsafe_code)]
fn publish_directory(stage: &Path, destination: &Path) -> Result<()> {
    let from = std::ffi::CString::new(stage.as_os_str().as_bytes()).map_err(|_| Error::Invalid)?;
    let to =
        std::ffi::CString::new(destination.as_os_str().as_bytes()).map_err(|_| Error::Invalid)?;
    // SAFETY: both C strings are valid for this call; no pointers are retained.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    return Err(Error::Unavailable);
    #[cfg(any(target_os = "linux", target_os = "macos"))]
    if result != 0 {
        return Err(Error::Unavailable);
    }
    Ok(())
}
/// Restore one trusted exact current checkpoint. A repeated exact restore only
/// resyncs its complete destination and never reapplies quarantine to new work.
pub fn restore(
    destination: &Path,
    checkpoint: &Path,
    trusted_head: &Path,
    identity_file: &Path,
) -> Result<()> {
    if reserved_stage_name(destination) {
        return Err(Error::Invalid);
    }
    check_components(destination)?;
    let parent = destination.parent().ok_or(Error::Invalid)?;
    private_dir(parent)?;
    for input in [checkpoint, trusted_head, identity_file] {
        if input.starts_with(destination) {
            return Err(Error::Invalid);
        }
    }
    let head: TrustedHead = read_json(trusted_head)?;
    if head.seal.format != FORMAT
        || head.seal.version != 1
        || head.seal.checkpoint.is_nil()
        || head.seal.source.is_nil()
        || head.seal.sequence < 0
        || unhex(&head.seal.head).is_err()
    {
        return Err(Error::Invalid);
    }
    let identity = identity(identity_file)?;
    let mut input = open_private(checkpoint, ARCHIVE_LIMIT)?;
    if digest_input(&mut input, ARCHIVE_LIMIT)?
        != (head.ciphertext_size, head.ciphertext_sha256.clone())
    {
        return Err(Error::Unavailable);
    }
    let stage = Stage::new(parent)?;
    let decryptor = age::Decryptor::new(input).map_err(|_| Error::Unavailable)?;
    let mut reader = decryptor
        .decrypt(std::iter::once(&identity as &dyn age::Identity))
        .map_err(|_| Error::Unauthorized)?;
    let mut magic = [0; 8];
    reader.read_exact(&mut magic)?;
    if &magic != WIRE {
        return Err(Error::Invalid);
    }
    let mut size = [0; 4];
    reader.read_exact(&mut size)?;
    let size = u32::from_be_bytes(size) as usize;
    if size as u64 > JSON_LIMIT {
        return Err(Error::Limit);
    }
    let mut manifest = vec![0; size];
    reader.read_exact(&mut manifest)?;
    let manifest: Manifest = serde_json::from_slice(&manifest).map_err(|_| Error::Invalid)?;
    if manifest.seal != head.seal {
        return Err(Error::Unavailable);
    }
    let database = stage.0.join("vault.sqlite");
    let journal = stage.0.join("vault.journal");
    extract(
        &mut reader,
        &database,
        manifest.database_size,
        &manifest.database_sha256,
        DB_LIMIT,
    )?;
    extract(
        &mut reader,
        &journal,
        manifest.journal_size,
        &manifest.journal_sha256,
        JOURNAL_LIMIT,
    )?;
    let mut tail = [0; 1];
    if reader.read(&mut tail)? != 0 {
        return Err(Error::Invalid);
    } // age authentication through EOF
    drop(reader);
    let mut db = Database::open_recovery(&stage.0)?;
    if installation(&db)? != head.seal.source
        || db.sequence != head.seal.sequence
        || db.head != unhex(&head.seal.head)?
    {
        return Err(Error::Unavailable);
    }
    verify_semantics(&db, &journal, parent)?;
    if destination.symlink_metadata().is_ok() {
        private_dir(destination)?;
        let provenance: Provenance = read_json(&destination.join("vault.restored"))?;
        if provenance.format != FORMAT
            || provenance.version != 1
            || provenance.trusted != head
            || provenance.destination.is_nil()
        {
            return Err(Error::Conflict);
        }
        let db = Database::open(destination)?;
        if installation(&db)? != provenance.destination {
            return Err(Error::Unavailable);
        }
        sync_dir(parent)?;
        return Ok(());
    }
    quarantine(&mut db)?;
    // Service-local identity is fresh. The immutable provenance binds its retained
    // journal transcript to the original source; vault owner/AAD stays unchanged.
    let new_id = uuid::Uuid::new_v4();
    db.conn
        .execute("UPDATE meta SET installation=?1", [new_id.to_string()])?;
    db.conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE)")?;
    drop(db);
    let mut journal_file = OpenOptions::new()
        .write(true)
        .custom_flags(libc::O_NOFOLLOW)
        .open(&journal)?;
    journal_file.write_all(MAGIC)?;
    journal_file.write_all(new_id.as_bytes())?;
    journal_file.sync_all()?;
    drop(journal_file);
    let db = Database::open_recovery(&stage.0)?;
    verify_semantics(&db, &journal, parent)?;
    drop(db);
    write_new(
        &stage.0.join("vault.restored"),
        &encode(&Provenance {
            format: FORMAT.into(),
            version: 1,
            trusted: head,
            destination: new_id,
        })?,
    )?;
    for entry in fs::read_dir(&stage.0)? {
        open_private(&entry?.path(), ARCHIVE_LIMIT)?.sync_all()?;
    }
    sync_dir(&stage.0)?;
    #[cfg(test)]
    fail_at("before_publication")?;
    publish_directory(&stage.0, destination)?;
    #[cfg(test)]
    fail_at("after_publication")?;
    sync_dir(parent)
}

#[cfg(test)]
thread_local! { static FAULT: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) }; }
#[cfg(test)]
fn fail_at(point: &'static str) -> Result<()> {
    if FAULT.with(|value| value.get() == Some(point)) {
        Err(Error::Unavailable)
    } else {
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use age::secrecy::ExposeSecret;
    fn private_temp() -> tempfile::TempDir {
        let temp = tempfile::tempdir().unwrap();
        fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
        temp
    }
    fn kit(parent: &Path) -> (age::x25519::Identity, PathBuf) {
        let identity = age::x25519::Identity::generate();
        let path = parent.join("identity");
        write_new(&path, identity.to_string().expose_secret().as_bytes()).unwrap();
        (identity, path)
    }
    #[test]
    fn source_seal_and_publication_faults_are_fail_closed_and_retryable() {
        let tmp = private_temp();
        let (identity, key) = kit(tmp.path());
        let source = tmp.path().join("source");
        drop(Database::open(&source).unwrap());
        let archive = tmp.path().join("checkpoint.age");
        let head = tmp.path().join("head.json");
        FAULT.with(|v| v.set(Some("source_sealed")));
        assert!(
            checkpoint(
                &source,
                &identity.to_public().to_string(),
                &archive,
                &head,
                true
            )
            .is_err()
        );
        assert!(source.join("vault.sealed").exists());
        assert!(Database::open(&source).is_err());
        assert!(!archive.exists());
        FAULT.with(|v| v.set(None));
        checkpoint(
            &source,
            &identity.to_public().to_string(),
            &archive,
            &head,
            true,
        )
        .unwrap();
        let destination = tmp.path().join("destination");
        FAULT.with(|v| v.set(Some("before_publication")));
        assert!(restore(&destination, &archive, &head, &key).is_err());
        assert!(!destination.exists());
        FAULT.with(|v| v.set(Some("after_publication")));
        assert!(restore(&destination, &archive, &head, &key).is_err());
        assert!(destination.exists());
        FAULT.with(|v| v.set(None));
        let owner = protocol::Owner {
            account: uuid::Uuid::new_v4(),
            tenant: uuid::Uuid::new_v4(),
            installation: uuid::Uuid::new_v4(),
            epoch: 0,
        };
        let mut db = Database::open(&destination).unwrap();
        db.create_session("post-publication", &owner).unwrap();
        drop(db);
        restore(&destination, &archive, &head, &key).unwrap();
        assert!(
            Database::open(&destination)
                .unwrap()
                .session("post-publication")
                .is_ok()
        );
        assert!(Database::open(&source).is_err());
    }
    #[test]
    fn authenticated_malformed_frames_and_rehashed_ciphertext_corruption_never_publish() {
        for mode in [
            "traversal",
            "trailing",
            "file_digest",
            "file_limit",
            "truncated_plaintext",
            "age_tag",
        ] {
            let tmp = private_temp();
            let (identity, key) = kit(tmp.path());
            let source = tmp.path().join("source");
            drop(Database::open(&source).unwrap());
            let archive = tmp.path().join("checkpoint.age");
            let head_path = tmp.path().join("head.json");
            checkpoint(
                &source,
                &identity.to_public().to_string(),
                &archive,
                &head_path,
                true,
            )
            .unwrap();
            let bytes = fs::read(&archive).unwrap();
            let mut plain = age::decrypt(&identity, &bytes).unwrap();
            match mode {
                "traversal" => {
                    let size = u32::from_be_bytes(plain[8..12].try_into().unwrap()) as usize;
                    let mut manifest: serde_json::Value =
                        serde_json::from_slice(&plain[12..12 + size]).unwrap();
                    manifest["path"] = "../../escape".into();
                    let new = serde_json::to_vec(&manifest).unwrap();
                    let rest = plain[12 + size..].to_vec();
                    plain.truncate(8);
                    plain.extend_from_slice(&(new.len() as u32).to_be_bytes());
                    plain.extend_from_slice(&new);
                    plain.extend_from_slice(&rest);
                }
                "trailing" => plain.push(1),
                "file_digest" | "file_limit" => {
                    let size = u32::from_be_bytes(plain[8..12].try_into().unwrap()) as usize;
                    let mut manifest: Manifest =
                        serde_json::from_slice(&plain[12..12 + size]).unwrap();
                    if mode == "file_digest" {
                        manifest.database_sha256 = "0".repeat(64);
                    } else {
                        manifest.database_size = DB_LIMIT + 1;
                    }
                    let new = encode(&manifest).unwrap();
                    let rest = plain[12 + size..].to_vec();
                    plain.truncate(8);
                    plain.extend_from_slice(&(new.len() as u32).to_be_bytes());
                    plain.extend_from_slice(&new);
                    plain.extend_from_slice(&rest);
                }
                "truncated_plaintext" => {
                    plain.pop();
                }
                _ => (),
            }
            let mut encrypted = age::encrypt(&identity.to_public(), &plain).unwrap();
            if mode == "age_tag" {
                let last = encrypted.len() - 1;
                encrypted[last] ^= 1;
            }
            fs::write(&archive, &encrypted).unwrap();
            let mut head: TrustedHead = read_json(&head_path).unwrap();
            head.ciphertext_size = encrypted.len() as u64;
            head.ciphertext_sha256 = hash(&encrypted);
            fs::write(&head_path, encode(&head).unwrap()).unwrap();
            let destination = tmp.path().join("destination");
            assert!(
                restore(&destination, &archive, &head_path, &key).is_err(),
                "{mode}"
            );
            assert!(!destination.exists());
            assert!(!tmp.path().parent().unwrap().join("escape").exists());
        }
    }
}
