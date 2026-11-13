//! Private, bounded single-writer SQLite account and thread storage.
//! Authentication capabilities can only be created here and are revalidated on every use.
mod automatic_memory;
pub use automatic_memory::{ExtractionClaim, ExtractionLease, ExtractionUnit};
mod artifacts;
mod community_skills;
pub use community_skills::author as skill_author;
mod google_setup;
pub use google_setup::*;
mod source_connections;
pub use source_connections::*;
mod agent_execution;
mod chat;
mod destructive_intents;
mod knowledge;
mod knowledge_journal;
mod memory_commands;
pub use agent_execution::{
    AgentClaim, AgentContext, AgentStepStatus, AgentToolSlot, KnowledgeToolInput,
    KnowledgeToolResult, PendingKnowledgeCall, PrivateCheckpoint, SourceExcerpt, StepLease,
    TOOL_INPUT_LIMIT, ToolReadError,
};
pub use chat::{ClaimedRun, ContextMessage, ContextRole, OutputPatch, RunFailure, RunLease};
pub use memory_commands::{
    MemoryCommandClarification, MemoryCommandOutcome, MemoryCommandPreparation,
    MemoryTargetCandidate, MemoryTargetCandidates, MemoryTargetChoice, PreparedMemoryCommand,
};
pub mod backup;
mod browser;
mod files;
mod journal;
mod mail;
mod web;
pub use browser::BrowserAccess;
pub use mail::*;
mod providers;
mod recovery;
use argon2::{Argon2, PasswordHash, PasswordHasher, PasswordVerifier, password_hash::SaltString};
use fs2::FileExt;
use moosuri_core::{AccountSummary, Role, ThreadSummary};
pub use providers::{CredentialChange, ProviderBinding, ProviderChange};
use rand::{RngCore, rngs::OsRng};
pub use recovery::recover_account;
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    path::Path,
    sync::{Arc, LazyLock, Mutex},
    thread::JoinHandle,
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::{Semaphore, mpsc, oneshot};
use uuid::Uuid;
use zeroize::Zeroizing;

pub type Result<T> = std::result::Result<T, StoreError>;
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum StoreError {
    #[error("Google connections use a different application")]
    GoogleConnectionsInUse,
    #[error("Google authorization is in progress")]
    GoogleAuthorizationInProgress,
    #[error("invalid input")]
    InvalidInput,
    #[error("authentication required")]
    Unauthorized,
    #[error("operation forbidden")]
    Forbidden,
    #[error("already exists")]
    Conflict,
    #[error("execution budget exhausted")]
    BudgetExhausted,
    #[error("not found")]
    NotFound,
    #[error("store busy")]
    Busy,
    #[error("store unavailable")]
    Unavailable,
    #[error("store integrity check failed")]
    Corrupt,
    #[error("installation already initialized")]
    AlreadyInitialized,
}
impl From<rusqlite::Error> for StoreError {
    fn from(error: rusqlite::Error) -> Self {
        use rusqlite::{Error::SqliteFailure, ErrorCode};
        match error {
            SqliteFailure(e, _) => match e.code {
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked => Self::Busy,
                ErrorCode::ConstraintViolation => Self::Conflict,
                ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase => Self::Corrupt,
                _ => Self::Unavailable,
            },
            _ => Self::Unavailable,
        }
    }
}
#[derive(Clone)]
struct AccountId(String);
#[derive(Clone)]
struct TenantId(String);
/// Opaque session capability. No serialization or Debug implementation.
#[derive(Clone)]
pub struct UserContext {
    account: AccountSummary,
    tenant: TenantId,
    digest: [u8; 32],
    epoch: i64,
    restricted: bool,
}
impl UserContext {
    pub fn account(&self) -> &AccountSummary {
        &self.account
    }
    pub fn password_change_required(&self) -> bool {
        self.restricted
    }
}
/// Contains a bearer token; deliberately does not implement Debug.
pub struct LoginResult {
    pub token: String,
    pub user: UserContext,
}
/// The generated credential is returned once; deliberately does not implement Debug.
pub struct Invitation {
    pub user: AccountSummary,
    pub temporary_password: String,
}

type Command = Box<dyn FnOnce(&mut Database) + Send + 'static>;
struct Inner {
    sender: Option<mpsc::Sender<Command>>,
    worker: Mutex<Option<JoinHandle<()>>>,
    dummy_hash: String,
}
impl Drop for Inner {
    fn drop(&mut self) {
        self.sender.take();
        if let Some(worker) = self.worker.get_mut().ok().and_then(Option::take) {
            let _ = worker.join();
        }
    }
}
#[derive(Clone)]
pub struct Store {
    inner: Arc<Inner>,
}
struct Database {
    conn: Connection,
    journal: journal::Journal,
    _lock: File,
    poisoned: bool,
    provider_key: Zeroizing<[u8; 32]>,
    knowledge_journal: knowledge_journal::Journal,
    destructive_journal: destructive_intents::Journal,
    web_profile: moosuri_core::WebToolProfile,
}
#[derive(Clone)]
struct LoginAccount {
    summary: AccountSummary,
    tenant: TenantId,
    hash: String,
    epoch: i64,
    temporary_expiry: Option<i64>,
    recovery: bool,
}
static KDF_LIMIT: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));
const CURRENT_SCHEMA_VERSION: i64 = 16;
const SESSION_SECONDS: i64 = 30 * 24 * 60 * 60;
const TEMP_SECONDS: i64 = 24 * 60 * 60;
// Public dummy PHC with the same Argon2id work parameters as account hashes.
// Its output is arbitrary: nonexistent accounts can never receive a session.
const DUMMY_HASH: &str =
    "$argon2id$v=19$m=19456,t=2,p=1$bW9vc3VyaS1kdW1teQ$AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA";
fn now() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| StoreError::Unavailable)?
            .as_secs(),
    )
    .map_err(|_| StoreError::Unavailable)
}
fn random_token() -> Result<String> {
    let mut bytes = [0; 32];
    OsRng
        .try_fill_bytes(&mut bytes)
        .map_err(|_| StoreError::Unavailable)?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
fn digest(token: &str) -> [u8; 32] {
    Sha256::digest(token.as_bytes()).into()
}
fn password_valid(password: &str) -> Result<()> {
    if !(12..=256).contains(&password.len()) {
        Err(StoreError::InvalidInput)
    } else {
        Ok(())
    }
}
fn name_valid(name: String, limit: usize) -> Result<String> {
    let name = name.trim().to_owned();
    if name.is_empty() || name.len() > limit || name.chars().any(char::is_control) {
        return Err(StoreError::InvalidInput);
    }
    Ok(name)
}
fn email_valid(email: String) -> Result<String> {
    let email = email.trim().to_lowercase();
    if email.len() > 254
        || !email.is_ascii()
        || email.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(StoreError::InvalidInput);
    }
    let Some((local, domain)) = email.split_once('@') else {
        return Err(StoreError::InvalidInput);
    };
    if local.is_empty() || domain.is_empty() || domain.contains('@') || local.len() > 64 {
        return Err(StoreError::InvalidInput);
    }
    Ok(email)
}
fn hash_sync(password: &str) -> Result<String> {
    let mut salt_bytes = [0; 16];
    OsRng
        .try_fill_bytes(&mut salt_bytes)
        .map_err(|_| StoreError::Unavailable)?;
    let salt = SaltString::encode_b64(&salt_bytes).map_err(|_| StoreError::Unavailable)?;
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|_| StoreError::Unavailable)
}
async fn hash_password(password: Zeroizing<String>) -> Result<String> {
    let permit = KDF_LIMIT
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_sync(&password)
    })
    .await
    .map_err(|_| StoreError::Unavailable)?
}
async fn verify_password(password: Zeroizing<String>, hash: String) -> Result<bool> {
    let permit = KDF_LIMIT
        .clone()
        .try_acquire_owned()
        .map_err(|_| StoreError::Busy)?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let parsed = PasswordHash::new(&hash).map_err(|_| StoreError::Corrupt)?;
        Ok(Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok())
    })
    .await
    .map_err(|_| StoreError::Unavailable)?
}
impl Store {
    /// Opens a private local data directory and holds its exclusive writer lock until all clones drop.
    pub fn open(path: &Path) -> Result<Self> {
        Self::open_with_web(path, moosuri_core::WebToolProfile::default())
    }
    /// Trusted installation configuration; never accepted from a model or HTTP body.
    pub fn open_with_web(path: &Path, web_profile: moosuri_core::WebToolProfile) -> Result<Self> {
        // RUSTSEC-2026-0459: reject affected SQLite before opening any DB/WAL pair.
        if rusqlite::version_number() < 3_054_000 {
            return Err(StoreError::Unavailable);
        }
        let dir = files::prepare_dir(path)?;
        let lock = files::private_file(&dir.join("writer.lock"))?;
        lock.try_lock_exclusive().map_err(|e| {
            if e.kind() == std::io::ErrorKind::WouldBlock {
                StoreError::Busy
            } else {
                StoreError::Unavailable
            }
        })?;
        for name in [
            "assistant.sqlite",
            "assistant.sqlite-wal",
            "assistant.sqlite-shm",
        ] {
            files::private_file(&dir.join(name))?;
        }
        let journal = journal::Journal::open(&dir.join("auth-epochs.journal"))?;
        File::open(&dir)
            .and_then(|f| f.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        let conn = Connection::open(dir.join("assistant.sqlite"))?;
        conn.busy_timeout(std::time::Duration::from_secs(2))?;
        conn.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;")?;
        let version: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if version == 0 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0001_accounts_threads.sql")
            ))?;
        } else if !(1..=CURRENT_SCHEMA_VERSION).contains(&version) {
            return Err(StoreError::Corrupt);
        }
        if version < 2 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0002_providers.sql")
            ))?;
        }
        if version < 3 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0003_chat.sql")
            ))?;
        }
        let knowledge_journal_path = dir.join("knowledge-invalidations.journal");
        // A restored database cannot establish that a missing destructive journal is empty.
        // Only the first migration into this feature may create its journal.
        if version >= 4
            && !knowledge_journal_path
                .try_exists()
                .map_err(|_| StoreError::Unavailable)?
        {
            return Err(StoreError::Corrupt);
        }
        let mut knowledge_journal = knowledge_journal::Journal::open(&knowledge_journal_path)?;
        File::open(&dir)
            .and_then(|f| f.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        // Make the empty journal and its directory entry durable before committing
        // schema4, whose future opens require this file. A crash before the schema
        // commit leaves schema3 plus an empty journal and can safely retry migration.
        if version < 4 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0004_knowledge.sql")
            ))?;
        }
        if version < 5 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0005_agent_execution.sql")
            ))?;
        }
        if version < 6 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0006_memory_commands.sql")
            ))?;
        }
        if version < 7 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0007_gmail.sql")
            ))?;
        }
        if version < 8 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0008_browser_workspaces.sql")
            ))?;
        }
        if version < 9 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0009_source_connections.sql")
            ))?;
        }
        if version < 10 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0010_google_setup.sql")
            ))?;
        }
        if version < 11 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0011_community_skills.sql")
            ))?;
        }
        if version < 12 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0012_memory_target_resolution.sql")
            ))?;
        }
        if version < 13 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0013_stopped_backup_exports.sql")
            ))?;
        }
        if version < 14 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0014_automatic_memory.sql")
            ))?;
        }
        let destructive_journal = destructive_intents::open_or_migrate(
            &conn,
            &dir,
            &mut knowledge_journal,
            version < 15,
        )?;
        if version < 16 {
            conn.execute_batch(&format!(
                "BEGIN IMMEDIATE; {} COMMIT;",
                include_str!("../../../migrations/assistant/0016_personal_artifacts.sql")
            ))?;
        }
        automatic_memory::restart(&conn)?;
        source_connections::restart(&conn)?;
        browser::restart(&conn)?;
        conn.execute("UPDATE mail_actions SET version=version+1,state='outcome_unknown',raw=NULL,content=NULL WHERE state='dispatch_committed'",[])?;
        let provider_key = providers::open_key(&conn, &dir)?;
        let integrity: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        if integrity != "ok" || conn.prepare("PRAGMA foreign_key_check")?.exists([])? {
            return Err(StoreError::Corrupt);
        }
        let mut database = Database {
            conn,
            journal,
            _lock: lock,
            poisoned: false,
            provider_key,
            knowledge_journal,
            destructive_journal,
            web_profile,
        };
        database.replay_journal()?;
        knowledge::replay(&mut database)?;
        destructive_intents::replay(&mut database)?;
        chat::restart(&mut database.conn)?;
        let dummy_hash = DUMMY_HASH.to_owned();
        let (sender, mut receiver) = mpsc::channel::<Command>(64);
        let worker = std::thread::Builder::new()
            .name("moosuri-sqlite".to_owned())
            .spawn(move || {
                while let Some(command) = receiver.blocking_recv() {
                    command(&mut database);
                }
                // SQLite closes before the installation lock is released (struct field order).
            })
            .map_err(|_| StoreError::Unavailable)?;
        Ok(Self {
            inner: Arc::new(Inner {
                sender: Some(sender),
                worker: Mutex::new(Some(worker)),
                dummy_hash,
            }),
        })
    }
    async fn call<T: Send + 'static>(
        &self,
        job: impl FnOnce(&mut Database) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (sender, receiver) = oneshot::channel();
        let command: Command = Box::new(move |db| {
            let result = if db.poisoned {
                Err(StoreError::Unavailable)
            } else {
                job(db)
            };
            let _ = sender.send(result);
        });
        self.inner
            .sender
            .as_ref()
            .ok_or(StoreError::Unavailable)?
            .try_send(command)
            .map_err(|e| match e {
                mpsc::error::TrySendError::Full(_) => StoreError::Busy,
                _ => StoreError::Unavailable,
            })?;
        receiver.await.map_err(|_| StoreError::Unavailable)?
    }
    pub async fn initialized(&self) -> Result<bool> {
        self.call(|db| {
            Ok(db
                .conn
                .query_row("SELECT EXISTS(SELECT 1 FROM accounts)", [], |r| r.get(0))?)
        })
        .await
    }
    pub async fn bootstrap(
        &self,
        name: String,
        email: String,
        password: String,
        temporary: bool,
    ) -> Result<AccountSummary> {
        let password = Zeroizing::new(password);
        let name = name_valid(name, 200)?;
        let email = email_valid(email)?;
        password_valid(&password)?;
        let hash = hash_password(password).await?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            if tx.query_row("SELECT EXISTS(SELECT 1 FROM accounts)", [], |r| {
                r.get::<_, bool>(0)
            })? {
                return Err(StoreError::AlreadyInitialized);
            }
            let account = insert_account(&tx, name, email, hash, Role::Admin, temporary)?;
            tx.commit()?;
            Ok(account)
        })
        .await
    }
    pub async fn login(&self, email: String, password: String) -> Result<LoginResult> {
        let password = Zeroizing::new(password);
        // Invalid account names still execute the same bounded dummy verification.
        let email = email_valid(email).ok();
        if password.len() > 256 {
            return Err(StoreError::Unauthorized);
        }
        let candidate = self
            .call(move |db| match email {
                Some(email) => load_login(&db.conn, &email),
                None => Ok(None),
            })
            .await?;
        let hash = candidate
            .as_ref()
            .map(|a| a.hash.clone())
            .unwrap_or_else(|| self.inner.dummy_hash.clone());
        let verified = verify_password(password, hash).await?;
        let Some(candidate) = candidate.filter(|_| verified) else {
            return Err(StoreError::Unauthorized);
        };
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            let current =
                load_login(&tx, &candidate.summary.email)?.ok_or(StoreError::Unauthorized)?;
            if current.epoch != candidate.epoch
                || current.hash != candidate.hash
                || current.recovery
                || current
                    .temporary_expiry
                    .is_some_and(|expires| expires <= now().unwrap_or(i64::MAX))
            {
                return Err(StoreError::Unauthorized);
            }
            let result = issue_session(&tx, current)?;
            tx.commit()?;
            Ok(result)
        })
        .await
    }
    pub async fn authenticate(&self, token: String) -> Result<UserContext> {
        let token = Zeroizing::new(token);
        if token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(StoreError::Unauthorized);
        }
        let digest = digest(&token);
        self.call(move |db| load_context(&db.conn, digest, true))
            .await
    }
    pub async fn change_password(
        &self,
        user: &UserContext,
        password: String,
    ) -> Result<LoginResult> {
        let password = Zeroizing::new(password);
        password_valid(&password)?;
        let user = user.clone();
        let hash = hash_password(password).await?;
        self.call(move |db| {
            // Revalidate after the KDF; stale and concurrent exchanges never append a revocation.
            let current = validate(&db.conn, &user, false, false)?;
            let next_epoch = current.epoch.checked_add(1).ok_or(StoreError::Corrupt)?;
            // Any failure after journal append leaves this process fail-closed until startup replay.
            db.poisoned = true;
            let record = db.journal.append(&current.account.id, next_epoch)?;
            let tx = db.conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let changed = tx.execute("UPDATE accounts SET password_hash=?1, must_change_password=0, temporary_expires_at=NULL, auth_epoch=?2 WHERE id=?3 AND auth_epoch=?4 AND recovery_required=0", params![hash, next_epoch, current.account.id, current.epoch])?;
            if changed != 1 { return Err(StoreError::Unauthorized); }
            tx.execute("DELETE FROM sessions WHERE tenant_id=?1 AND account_id=?2", params![current.tenant.0, current.account.id])?;
            tx.execute("UPDATE store_meta SET journal_seq=?1, journal_head=?2 WHERE singleton=1", params![record.seq, record.hash.as_slice()])?;
            let account = load_login(&tx, &current.account.email)?.ok_or(StoreError::Corrupt)?;
            let result = issue_session(&tx, account)?;
            tx.commit()?; db.poisoned = false; Ok(result)
        }).await
    }
    pub async fn logout(&self, user: &UserContext) -> Result<()> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, false, false)?;
            db.conn.execute(
                "DELETE FROM sessions WHERE digest=?1",
                [user.digest.as_slice()],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn create_user(
        &self,
        user: &UserContext,
        name: String,
        email: String,
    ) -> Result<Invitation> {
        let name = name_valid(name, 200)?;
        let email = email_valid(email)?;
        let user = user.clone();
        // Reject unauthorized requests before spending bounded KDF capacity, then check again at commit.
        let check = user.clone();
        self.call(move |db| validate(&db.conn, &check, true, true).map(|_| ()))
            .await?;
        let temporary_password = Zeroizing::new(random_token()?);
        let hash = hash_password(temporary_password.clone()).await?;
        self.call(move |db| {
            let tx = db
                .conn
                .transaction_with_behavior(TransactionBehavior::Immediate)?;
            validate(&tx, &user, true, true)?;
            let account = insert_account(&tx, name, email, hash, Role::Member, true)?;
            tx.commit()?;
            Ok(Invitation {
                user: account,
                temporary_password: temporary_password.to_string(),
            })
        })
        .await
    }
    pub async fn list_users(&self, user: &UserContext) -> Result<Vec<AccountSummary>> {
        let user = user.clone();
        self.call(move |db| {
            validate(&db.conn, &user, true, true)?;
            let mut stmt = db.conn.prepare(
                "SELECT id,name,email,role,must_change_password FROM accounts ORDER BY email",
            )?;
            let rows = stmt
                .query_map([], summary_row)?
                .collect::<std::result::Result<Vec<_>, _>>()?;
            Ok(rows)
        })
        .await
    }
    pub async fn create_thread(&self, user: &UserContext, title: String) -> Result<ThreadSummary> {
        let title = name_valid(title, 500)?;
        let user = user.clone();
        self.call(move |db| {
            let current=validate(&db.conn,&user,true,false)?; let time=now()?;
            let thread=ThreadSummary { id: Uuid::new_v4().to_string(), title, created_at:time,updated_at:time };
            db.conn.execute("INSERT INTO threads(tenant_id,id,account_id,title,created_at,updated_at) VALUES (?1,?2,?3,?4,?5,?5)", params![current.tenant.0,thread.id,current.account.id,thread.title,time])?; Ok(thread)
        }).await
    }
    pub async fn list_threads(&self, user: &UserContext) -> Result<Vec<ThreadSummary>> {
        let user = user.clone();
        self.call(move |db| { let current=validate(&db.conn,&user,true,false)?;
            let mut stmt=db.conn.prepare("SELECT id,title,created_at,updated_at FROM threads WHERE tenant_id=?1 ORDER BY updated_at DESC,id")?;
            let rows=stmt.query_map([current.tenant.0], |r| Ok(ThreadSummary { id:r.get(0)?,title:r.get(1)?,created_at:r.get(2)?,updated_at:r.get(3)? }))?.collect::<std::result::Result<Vec<_>,_>>()?; Ok(rows)
        }).await
    }
    pub async fn delete_thread(&self, user: &UserContext, id: String) -> Result<()> {
        let user = user.clone();
        if Uuid::parse_str(&id).is_err() {
            return Err(StoreError::InvalidInput);
        }
        self.call(move |db| {
            let current = validate(&db.conn, &user, true, false)?;
            let exists: bool = db.conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM threads WHERE tenant_id=?1 AND id=?2)",
                params![current.tenant.0, id],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(StoreError::NotFound);
            }
            destructive_intents::commit(
                db,
                &current.tenant.0,
                destructive_intents::Effect::DeleteThread { target: id },
            )
        })
        .await
    }
}
fn summary_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<AccountSummary> {
    let role: String = row.get(3)?;
    let role = match role.as_str() {
        "admin" => Role::Admin,
        "member" => Role::Member,
        _ => return Err(rusqlite::Error::InvalidQuery),
    };
    Ok(AccountSummary {
        id: row.get(0)?,
        name: row.get(1)?,
        email: row.get(2)?,
        role,
        must_change_password: row.get(4)?,
    })
}
fn load_login(conn: &Connection, email: &str) -> Result<Option<LoginAccount>> {
    Ok(conn.query_row("SELECT id,name,email,role,must_change_password,tenant_id,password_hash,auth_epoch,temporary_expires_at,recovery_required FROM accounts WHERE email=?1", [email], |r| Ok(LoginAccount { summary:summary_row(r)?,tenant:TenantId(r.get(5)?),hash:r.get(6)?,epoch:r.get(7)?,temporary_expiry:r.get(8)?,recovery:r.get(9)? })).optional()?)
}
fn insert_account(
    conn: &Connection,
    name: String,
    email: String,
    hash: String,
    role: Role,
    temporary: bool,
) -> Result<AccountSummary> {
    let account = AccountId(Uuid::new_v4().to_string());
    let tenant = TenantId(Uuid::new_v4().to_string());
    conn.execute("INSERT INTO tenants(id) VALUES (?1)", [&tenant.0])?;
    conn.execute("INSERT INTO accounts(id,tenant_id,name,email,role,password_hash,must_change_password,temporary_expires_at,auth_epoch) VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0)", params![account.0,tenant.0,name,email,if role==Role::Admin { "admin" } else { "member" },hash,temporary,if temporary { Some(now()?+TEMP_SECONDS) } else { None }])?;
    Ok(AccountSummary {
        id: account.0,
        name,
        email,
        role,
        must_change_password: temporary,
    })
}
fn issue_session(conn: &Connection, account: LoginAccount) -> Result<LoginResult> {
    let token = random_token()?;
    let digest = digest(&token);
    let restricted = account.summary.must_change_password;
    let expiry = if restricted {
        account.temporary_expiry.ok_or(StoreError::Corrupt)?
    } else {
        now()? + SESSION_SECONDS
    };
    conn.execute("INSERT INTO sessions(digest,tenant_id,account_id,auth_epoch,password_change_only,expires_at) VALUES (?1,?2,?3,?4,?5,?6)",params![digest.as_slice(),account.tenant.0,account.summary.id,account.epoch,restricted,expiry])?;
    Ok(LoginResult {
        token,
        user: UserContext {
            account: account.summary,
            tenant: account.tenant,
            digest,
            epoch: account.epoch,
            restricted,
        },
    })
}
fn load_context(conn: &Connection, digest: [u8; 32], renew: bool) -> Result<UserContext> {
    let time = now()?;
    let result=conn.query_row("SELECT a.id,a.name,a.email,a.role,a.must_change_password,a.tenant_id,a.auth_epoch,s.password_change_only FROM sessions s JOIN accounts a ON a.tenant_id=s.tenant_id AND a.id=s.account_id WHERE s.digest=?1 AND s.auth_epoch=a.auth_epoch AND a.recovery_required=0 AND s.expires_at>?2 AND s.password_change_only=a.must_change_password AND (a.temporary_expires_at IS NULL OR a.temporary_expires_at>?2)",params![digest.as_slice(),time],|r| Ok(UserContext { account:summary_row(r)?,tenant:TenantId(r.get(5)?),epoch:r.get(6)?,restricted:r.get(7)?,digest })).optional()?.ok_or(StoreError::Unauthorized)?;
    if renew && !result.restricted {
        conn.execute(
            "UPDATE sessions SET expires_at=?1 WHERE digest=?2",
            params![time + SESSION_SECONDS, digest.as_slice()],
        )?;
    }
    Ok(result)
}
fn validate(conn: &Connection, user: &UserContext, full: bool, admin: bool) -> Result<UserContext> {
    let current = load_context(conn, user.digest, false)?;
    if current.epoch != user.epoch
        || current.tenant.0 != user.tenant.0
        || current.account.id != user.account.id
    {
        return Err(StoreError::Unauthorized);
    }
    if (full && current.restricted) || (admin && current.account.role != Role::Admin) {
        return Err(StoreError::Forbidden);
    }
    Ok(current)
}
impl Database {
    fn replay_journal(&mut self) -> Result<()> {
        let (seq, head): (i64, Vec<u8>) = self.conn.query_row(
            "SELECT journal_seq,journal_head FROM store_meta WHERE singleton=1",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )?;
        if seq < 0 || seq as usize > self.journal.records.len() {
            return Err(StoreError::Corrupt);
        }
        let expected = if seq == 0 {
            [0; 32]
        } else {
            self.journal.records[seq as usize - 1].hash
        };
        if head != expected {
            return Err(StoreError::Corrupt);
        }
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        for record in self.journal.records.iter().skip(seq as usize) {
            if tx.execute("UPDATE accounts SET auth_epoch=MAX(auth_epoch,?1), recovery_required=1 WHERE id=?2",params![record.epoch,record.account])?!=1 { return Err(StoreError::Corrupt); }
            tx.execute(
                "DELETE FROM sessions WHERE account_id=?1",
                [&record.account],
            )?;
            tx.execute(
                "UPDATE store_meta SET journal_seq=?1,journal_head=?2 WHERE singleton=1",
                params![record.seq, record.hash.as_slice()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }
}
#[cfg(test)]
mod tests;
