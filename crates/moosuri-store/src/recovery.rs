//! Trusted local operator recovery, deliberately unavailable through account APIs.
use super::*;
use std::{
    fs::{self, OpenOptions},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
};

struct PreflightDirectory(PathBuf);
impl Drop for PreflightDirectory {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn existing_private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| StoreError::NotFound)?;
    let info = file.metadata().map_err(|_| StoreError::Unavailable)?;
    if !info.is_file()
        || info.uid() != unsafe { libc::geteuid() }
        || info.nlink() != 1
        || info.permissions().mode() & 0o077 != 0
    {
        return Err(StoreError::InvalidInput);
    }
    Ok(file)
}
fn preflight(path: &Path) -> Result<()> {
    // Do not create/chmod even the lock or required installation markers.
    // Caller retains the central runtime-admission lock through Store construction.
    for name in [
        "auth-epochs.journal",
        "knowledge-invalidations.journal",
        "provider-secrets.key",
    ] {
        existing_private_file(&path.join(name))?;
    }
    // SQLite READ_ONLY can still create WAL/SHM beside its input. Inspect a
    // private disposable DB+WAL copy under the stopped-installation lock instead;
    // immutable=1 is unsuitable because it ignores committed WAL contents.
    let temp_parent = std::env::temp_dir()
        .canonicalize()
        .map_err(|_| StoreError::Unavailable)?;
    let temp_path = temp_parent.join(format!("moosuri-recovery-preflight-{}", Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&temp_path)
        .map_err(|_| StoreError::Unavailable)?;
    let temp = PreflightDirectory(temp_path);
    for name in ["assistant.sqlite", "assistant.sqlite-wal"] {
        let source_path = path.join(name);
        if name.ends_with("-wal") {
            match fs::symlink_metadata(&source_path) {
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(_) => return Err(StoreError::Unavailable),
                Ok(_) => {}
            }
        }
        let mut source = existing_private_file(&source_path)?;
        let mut destination = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(temp.0.join(name))
            .map_err(|_| StoreError::Unavailable)?;
        std::io::copy(&mut source, &mut destination).map_err(|_| StoreError::Unavailable)?;
    }
    let conn = Connection::open_with_flags(
        temp.0.join("assistant.sqlite"),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | rusqlite::OpenFlags::SQLITE_OPEN_NOFOLLOW
            | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )?;
    conn.execute_batch("PRAGMA trusted_schema=OFF;")?;
    let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if version == 0 {
        return Err(StoreError::NotFound);
    }
    if !(1..=CURRENT_SCHEMA_VERSION).contains(&version) {
        return Err(StoreError::Corrupt);
    }
    let initialized: bool =
        conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts)", [], |r| r.get(0))?;
    if !initialized {
        return Err(StoreError::NotFound);
    }
    Ok(())
}

/// Recover a journal-locked account in an existing, stopped installation.
///
/// This is a trusted local operator boundary, not an authenticated HTTP action or
/// a general password reset. The caller explicitly selects the account and role.
/// The supplied credential expires after 24 hours and must be changed at login.
/// No session is issued and no old execution/connection authority is rewritten.
pub async fn recover_account(
    path: &Path,
    account_id: String,
    role: Role,
    password: String,
) -> Result<()> {
    let password = Zeroizing::new(password);
    if !path.is_absolute() || Uuid::parse_str(&account_id).is_err() {
        return Err(StoreError::InvalidInput);
    }
    password_valid(&password)?;
    // Store::open also serves initialization. This entry point must never create
    // a missing installation, including when an attacker supplies a linked DB.
    let file = existing_private_file(&path.join("assistant.sqlite"))?;
    if file.metadata().map_err(|_| StoreError::Unavailable)?.len() == 0 {
        return Err(StoreError::NotFound);
    }
    if rusqlite::version_number() < 3_054_000 {
        return Err(StoreError::Unavailable);
    }
    // Recovery is never initialization: unlike normal legacy Store open it
    // requires the existing lock before any admission helper could create one.
    existing_private_file(&path.join("writer.lock"))?;
    let admission = cutover::runtime_admission(path)?.ok_or(StoreError::NotFound)?;
    preflight(path)?;
    let store = Store::open_admitted(
        path,
        moosuri_core::WebToolProfile::default(),
        Some(admission),
    )?;
    store
        .recover_locked_account(account_id, role, password)
        .await
}

fn locked_epoch(conn: &Connection, account: &str) -> Result<i64> {
    let row: Option<(i64, bool)> = conn
        .query_row(
            "SELECT auth_epoch,recovery_required FROM accounts WHERE id=?1",
            [account],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    match row {
        Some((epoch, true)) => Ok(epoch),
        Some((_, false)) => Err(StoreError::Conflict),
        None => Err(StoreError::NotFound),
    }
}

impl Store {
    async fn recover_locked_account(
        &self,
        account: String,
        role: Role,
        password: Zeroizing<String>,
    ) -> Result<()> {
        let account = Uuid::parse_str(&account)
            .map_err(|_| StoreError::InvalidInput)?
            .to_string();
        password_valid(&password)?;
        let check = account.clone();
        let old_hash: String = self
            .call(move |db| {
                locked_epoch(&db.conn, &check)?;
                Ok(db.conn.query_row(
                    "SELECT password_hash FROM accounts WHERE id=?1",
                    [&check],
                    |r| r.get(0),
                )?)
            })
            .await?;
        // Explicit enrollment must not make the revoked password valid again.
        if verify_password(password.clone(), old_hash).await? {
            return Err(StoreError::InvalidInput);
        }
        let hash = hash_password(password).await?;
        self.call(move |db| apply(db, &account, role, hash)).await
    }
}

fn apply(db: &mut Database, account: &str, role: Role, hash: String) -> Result<()> {
    // Recheck after the KDF: a concurrent recovery cannot reset a healthy account.
    let previous = locked_epoch(&db.conn, account)?;
    let epoch = previous.checked_add(1).ok_or(StoreError::Corrupt)?;
    let expiry = now()?
        .checked_add(TEMP_SECONDS)
        .ok_or(StoreError::Corrupt)?;
    db.poisoned = true;
    let record = db.journal.append(account, epoch)?;
    let tx = db
        .conn
        .transaction_with_behavior(TransactionBehavior::Immediate)?;
    let changed = tx.execute(
        "UPDATE accounts SET role=?1,password_hash=?2,must_change_password=1,temporary_expires_at=?3,auth_epoch=?4,recovery_required=0,management_revision=management_revision+1 WHERE id=?5 AND auth_epoch=?6 AND recovery_required=1",
        params![if role == Role::Admin { "admin" } else { "member" }, hash, expiry, epoch, account, previous],
    )?;
    if changed != 1 {
        return Err(StoreError::Conflict);
    }
    tx.execute("DELETE FROM sessions WHERE account_id=?1", [account])?;
    tx.execute(
        "UPDATE store_meta SET journal_seq=?1,journal_head=?2 WHERE singleton=1",
        params![record.seq, record.hash.as_slice()],
    )?;
    tx.commit()?;
    db.poisoned = false;
    Ok(())
}

#[cfg(test)]
mod tests;
