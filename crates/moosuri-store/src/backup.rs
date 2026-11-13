//! Stopped-installation encrypted exports. This is not a historical restore API.
use crate::{CURRENT_SCHEMA_VERSION, StoreError, destructive_intents, journal, knowledge_journal};
use rusqlite::{Connection, OpenFlags, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;
use zeroize::Zeroizing;
pub(crate) mod files;
#[cfg(test)]
mod tests;

pub type Result<T> = std::result::Result<T, Error>;
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Error {
    #[error("installation_sealed")]
    Sealed,
    #[error("checkpoint_journal_pending")]
    JournalPending,
    #[error("backup_invalid_input")]
    InvalidInput,
    #[error("backup_unavailable")]
    Unavailable,
    #[error("backup_busy")]
    Busy,
    #[error("backup_limit_exceeded")]
    LimitExceeded,
    #[error("backup_unsupported_installation")]
    UnsupportedInstallation,
    #[error("backup_integrity_failed")]
    Integrity,
    #[error("backup_unconfigured")]
    Unconfigured,
    #[error("backup_key_missing")]
    KeyMissing,
    #[error("backup_decryption_failed")]
    DecryptionFailed,
    #[error("backup_output_exists")]
    OutputExists,
}
impl From<rusqlite::Error> for Error {
    fn from(_: rusqlite::Error) -> Self {
        Self::Integrity
    }
}
impl From<StoreError> for Error {
    fn from(_: StoreError) -> Self {
        Self::Integrity
    }
}
pub(crate) const DB_LIMIT: u64 = 1024 * 1024 * 1024;
pub(crate) const JOURNAL_LIMIT: u64 = 64 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(60);
const CHALLENGE: &[u8] = b"moosuri/assistant-export/setup/v1";

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KeyEpoch {
    pub epoch: i64,
    pub recipient: String,
    pub verified_at: i64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Configuration {
    pub installation_id: String,
    pub active_epoch: Option<i64>,
    pub retained_epochs: Vec<KeyEpoch>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub name: String,
    pub sha256: String,
    pub size: u64,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalHead {
    pub sequence: i64,
    pub head: String,
    pub snapshot_sequence: i64,
    pub snapshot_head: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub purpose: String,
    pub installation_id: String,
    pub export_id: String,
    pub schema: i64,
    pub created_at: i64,
    pub key_epoch: i64,
    pub recipient: String,
    pub entries: Vec<Entry>,
    pub auth_journal: JournalHead,
    pub knowledge_journal: JournalHead,
    pub authoritative_files: Vec<String>,
    pub limitations: Vec<String>,
}
/// Explicit format2 metadata. Format1 remains the unchanged four-entry codec.
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub version: u32,
    pub schema_baseline: i64,
    pub legacy_sequence: i64,
    pub legacy_head: String,
    pub successor_format: String,
    pub successor_journal: JournalHead,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ManifestV2 {
    pub format: u32,
    pub purpose: String,
    pub installation_id: String,
    pub export_id: String,
    pub schema: i64,
    pub created_at: i64,
    pub key_epoch: i64,
    pub recipient: String,
    pub entries: Vec<Entry>,
    pub auth_journal: JournalHead,
    pub knowledge_journal: JournalHead,
    pub authoritative_files: Vec<String>,
    pub limitations: Vec<String>,
    pub coverage: Coverage,
}
pub enum DecodedManifest {
    V1(Manifest),
    V2(ManifestV2),
}
/// Bounded version dispatch only; this does not decrypt, extract or restore data.
pub fn decode_manifest(bytes: &[u8]) -> Result<DecodedManifest> {
    if bytes.len() > 32768 {
        return Err(Error::LimitExceeded);
    }
    let value: serde_json::Value = serde_json::from_slice(bytes).map_err(|_| Error::Integrity)?;
    fn inventory(entries: &[Entry], successor: bool) -> Result<()> {
        let mut expected = std::collections::BTreeSet::from([
            "assistant.sqlite",
            "auth-epochs.journal",
            "knowledge-invalidations.journal",
        ]);
        if successor {
            expected.insert(destructive_intents::FILE_NAME);
        }
        if entries.len() != expected.len() {
            return Err(Error::Integrity);
        }
        for item in entries {
            if !expected.remove(item.name.as_str())
                || item.sha256.len() != 64
                || !item
                    .sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            {
                return Err(Error::Integrity);
            }
            let cap = if item.name == "assistant.sqlite" {
                DB_LIMIT
            } else {
                JOURNAL_LIMIT
            };
            if item.size > cap {
                return Err(Error::LimitExceeded);
            }
        }
        Ok(())
    }
    match value.get("format").and_then(|v| v.as_u64()) {
        Some(1) => {
            let manifest: Manifest = serde_json::from_value(value).map_err(|_| Error::Integrity)?;
            inventory(&manifest.entries, false)?;
            Ok(DecodedManifest::V1(manifest))
        }
        Some(2) => {
            let manifest: ManifestV2 =
                serde_json::from_value(value).map_err(|_| Error::Integrity)?;
            inventory(&manifest.entries, true)?;
            if manifest.coverage.version != 1
                || manifest.coverage.schema_baseline != destructive_intents::COVERAGE_SCHEMA
                || manifest.coverage.successor_format != "MSDSTR01"
                || manifest.coverage.legacy_sequence != manifest.knowledge_journal.sequence
                || manifest.coverage.legacy_head != manifest.knowledge_journal.head
            {
                return Err(Error::Integrity);
            }
            for head in [
                &manifest.auth_journal,
                &manifest.knowledge_journal,
                &manifest.coverage.successor_journal,
            ] {
                if head.snapshot_sequence < 0
                    || head.sequence < head.snapshot_sequence
                    || [head.head.as_str(), head.snapshot_head.as_str()]
                        .iter()
                        .any(|value| {
                            value.len() != 64
                                || !value
                                    .bytes()
                                    .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
                        })
                    || (head.sequence == head.snapshot_sequence && head.head != head.snapshot_head)
                {
                    return Err(Error::Integrity);
                }
            }
            Ok(DecodedManifest::V2(manifest))
        }
        _ => Err(Error::UnsupportedInstallation),
    }
}

pub struct ExportSummary {
    pub export_id: String,
    pub key_epoch: i64,
}
pub(crate) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}
fn now() -> Result<i64> {
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::Unavailable)?
            .as_secs(),
    )
    .map_err(|_| Error::Unavailable)
}
fn check_deadline(start: Instant) -> Result<()> {
    if start.elapsed() > DEADLINE {
        Err(Error::Busy)
    } else {
        Ok(())
    }
}

pub(crate) struct Offline {
    pub(crate) root: PathBuf,
    _lock: File,
    pub(crate) temp: files::Temporary,
    pub(crate) conn: Connection,
    pub(crate) auth: journal::Journal,
    pub(crate) knowledge: knowledge_journal::Journal,
    pub(crate) destructive: destructive_intents::Journal,
}
impl Offline {
    fn open(root: &Path) -> Result<Self> {
        let offline = Self::for_cutover(root)?;
        crate::cutover::require_active(&offline.conn, &offline.root)?;
        Ok(offline)
    }
    pub(crate) fn for_cutover(root: &Path) -> Result<Self> {
        if rusqlite::version_number() < 3_054_000 {
            return Err(Error::UnsupportedInstallation);
        }
        let (root, lock) = files::stopped_lock(root)?;
        Self::capture(root, lock)
    }
    fn capture(root: PathBuf, lock: File) -> Result<Self> {
        let temp = files::Temporary::new(&root)?;
        files::copy_file(
            &root.join("assistant.sqlite"),
            &temp.0.join("assistant.sqlite"),
            DB_LIMIT,
        )?;
        match fs::symlink_metadata(root.join("assistant.sqlite-wal")) {
            Ok(_) => files::copy_file(
                &root.join("assistant.sqlite-wal"),
                &temp.0.join("assistant.sqlite-wal"),
                DB_LIMIT,
            )?,
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(_) => return Err(Error::Unavailable),
        }
        for name in [
            "auth-epochs.journal",
            "knowledge-invalidations.journal",
            destructive_intents::FILE_NAME,
        ] {
            files::copy_file(&root.join(name), &temp.0.join(name), JOURNAL_LIMIT)?;
        }
        let conn = Connection::open_with_flags(
            temp.0.join("assistant.sqlite"),
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NOFOLLOW,
        )?;
        conn.execute_batch("PRAGMA trusted_schema=OFF; PRAGMA foreign_keys=ON;")?;
        let schema: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
        if schema != CURRENT_SCHEMA_VERSION {
            return Err(Error::UnsupportedInstallation);
        }
        let initialized: bool =
            conn.query_row("SELECT EXISTS(SELECT 1 FROM accounts)", [], |r| r.get(0))?;
        if !initialized {
            return Err(Error::UnsupportedInstallation);
        }
        if conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok"
            || conn.prepare("PRAGMA foreign_key_check")?.exists([])?
        {
            return Err(Error::Integrity);
        }
        let pages: i64 = conn.query_row("PRAGMA page_count", [], |r| r.get(0))?;
        let page_size: i64 = conn.query_row("PRAGMA page_size", [], |r| r.get(0))?;
        if pages
            .checked_mul(page_size)
            .is_none_or(|size| size > DB_LIMIT as i64)
        {
            return Err(Error::LimitExceeded);
        }
        configuration(&conn)?;
        let auth = journal::Journal::open(&temp.0.join("auth-epochs.journal"))?;
        let knowledge =
            knowledge_journal::Journal::open(&temp.0.join("knowledge-invalidations.journal"))?;
        journal_heads(&conn, &auth, &knowledge)?;
        let destructive =
            destructive_intents::Journal::open(&temp.0.join(destructive_intents::FILE_NAME))?;
        destructive_intents::verify(&conn, &knowledge, &destructive)?;
        Ok(Self {
            root,
            _lock: lock,
            temp,
            conn,
            auth,
            knowledge,
            destructive,
        })
    }
    pub(crate) fn refresh(self) -> Result<Self> {
        let Self {
            root,
            _lock,
            temp,
            conn,
            auth,
            knowledge,
            destructive,
        } = self;
        drop((conn, auth, knowledge, destructive, temp));
        Self::capture(root, _lock)
    }
}
pub(crate) fn configuration(conn: &Connection) -> Result<Configuration> {
    let (installation_id, active_epoch): (String, Option<i64>) = conn.query_row(
        "SELECT installation_id,active_epoch FROM backup_installation WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if Uuid::parse_str(&installation_id).is_err() {
        return Err(Error::Integrity);
    }
    let rows = conn
        .prepare(
            "SELECT epoch,recipient,verified_at FROM backup_key_epochs ORDER BY epoch LIMIT 65",
        )?
        .query_map([], |r| {
            Ok(KeyEpoch {
                epoch: r.get(0)?,
                recipient: r.get(1)?,
                verified_at: r.get(2)?,
            })
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    if rows.len() > 64 {
        return Err(Error::LimitExceeded);
    }
    for (index, row) in rows.iter().enumerate() {
        if row.epoch != index as i64 + 1 || row.recipient.parse::<age::x25519::Recipient>().is_err()
        {
            return Err(Error::Integrity);
        }
    }
    if active_epoch != rows.last().map(|v| v.epoch) {
        return Err(Error::Integrity);
    }
    Ok(Configuration {
        installation_id,
        active_epoch,
        retained_epochs: rows,
    })
}
pub(crate) fn journal_heads(
    conn: &Connection,
    auth: &journal::Journal,
    knowledge: &knowledge_journal::Journal,
) -> Result<(JournalHead, JournalHead)> {
    let (seq, head): (i64, Vec<u8>) = conn.query_row(
        "SELECT journal_seq,journal_head FROM store_meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    let (kseq, khead): (i64, Vec<u8>) = conn.query_row(
        "SELECT sequence,head FROM knowledge_journal_meta WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    fn verify(seq: i64, head: Vec<u8>, records: Vec<(i64, [u8; 32])>) -> Result<JournalHead> {
        if seq < 0 {
            return Err(Error::Integrity);
        }
        let expected = if seq == 0 {
            [0; 32]
        } else {
            records
                .get(usize::try_from(seq - 1).map_err(|_| Error::Integrity)?)
                .ok_or(Error::Integrity)?
                .1
        };
        if head != expected {
            return Err(Error::Integrity);
        }
        let (sequence, last) = records.last().copied().unwrap_or((0, [0; 32]));
        Ok(JournalHead {
            sequence,
            head: hex(&last),
            snapshot_sequence: seq,
            snapshot_head: hex(&head),
        })
    }
    Ok((
        verify(
            seq,
            head,
            auth.records.iter().map(|r| (r.seq, r.hash)).collect(),
        )?,
        verify(
            kseq,
            khead,
            knowledge.records.iter().map(|r| (r.seq, r.hash)).collect(),
        )?,
    ))
}

/// Generate an independent recovery identity without opening an installation.
/// Only the public recipient is returned; secret text is written to a private file.
pub fn keygen(output: &Path) -> Result<String> {
    use age::secrecy::ExposeSecret;
    let output = files::private_output_path(output)?;
    let parent = output.parent().ok_or(Error::InvalidInput)?;
    let partial = parent.join(format!(".moosuri-identity-{}.partial", Uuid::new_v4()));
    let result = (|| {
        let mut file = files::create_file(&partial)?;
        let identity = age::x25519::Identity::generate();
        let recipient = identity.to_public().to_string();
        writeln!(
            file,
            "# moosuri backup recovery identity\n# public key: {recipient}"
        )
        .map_err(|_| Error::Unavailable)?;
        let secret = identity.to_string();
        file.write_all(secret.expose_secret().as_bytes())
            .and_then(|_| file.write_all(b"\n"))
            .and_then(|_| file.sync_all())
            .map_err(|_| Error::Unavailable)?;
        files::publish(&partial, &output)?;
        Ok(recipient)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}

/// Prove an operator-held X25519 identity can decrypt before activating its public recipient.
/// Neither the identity nor its pathname is stored in installation state.
pub fn setup(root: &Path, recipient: &str, identity_file: &Path) -> Result<Configuration> {
    if recipient.len() > 128 {
        return Err(Error::InvalidInput);
    }
    let recipient: age::x25519::Recipient = recipient.parse().map_err(|_| Error::InvalidInput)?;
    let identity_parent = files::directory(identity_file.parent().ok_or(Error::InvalidInput)?)?;
    let root = files::directory(root)?;
    if identity_parent.starts_with(&root) {
        return Err(Error::InvalidInput);
    }
    let offline = Offline::open(&root)?;
    let file = files::read_file(identity_file, 4096).map_err(|e| {
        if e == Error::InvalidInput {
            Error::KeyMissing
        } else {
            e
        }
    })?;
    let mut identity_text = Zeroizing::new(String::new());
    file.take(4097)
        .read_to_string(&mut identity_text)
        .map_err(|_| Error::KeyMissing)?;
    if identity_text.len() > 4096 {
        return Err(Error::LimitExceeded);
    }
    let lines = identity_text
        .lines()
        .map(str::trim)
        .filter(|v| !v.is_empty() && !v.starts_with('#'))
        .collect::<Vec<_>>();
    if lines.len() != 1 {
        return Err(Error::KeyMissing);
    }
    let identity: age::x25519::Identity = lines[0].parse().map_err(|_| Error::KeyMissing)?;
    let encrypted = age::encrypt(&recipient, CHALLENGE).map_err(|_| Error::Unavailable)?;
    let plain =
        Zeroizing::new(age::decrypt(&identity, &encrypted).map_err(|_| Error::DecryptionFailed)?);
    if plain.as_slice() != CHALLENGE {
        return Err(Error::DecryptionFailed);
    }
    let mut conn = Connection::open_with_flags(
        offline.root.join("assistant.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL; PRAGMA trusted_schema=OFF;",
    )?;
    let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
    let config = configuration(&tx)?;
    let recipient = recipient.to_string();
    if config
        .retained_epochs
        .last()
        .is_some_and(|v| v.recipient == recipient)
    {
        tx.commit()?;
        return Ok(config);
    }
    if config.retained_epochs.len() >= 64 {
        return Err(Error::LimitExceeded);
    }
    let epoch = config
        .active_epoch
        .unwrap_or(0)
        .checked_add(1)
        .ok_or(Error::Integrity)?;
    tx.execute(
        "INSERT INTO backup_key_epochs VALUES(?1,?2,?3)",
        params![epoch, recipient, now()?],
    )?;
    tx.execute(
        "UPDATE backup_installation SET active_epoch=?1 WHERE singleton=1",
        [epoch],
    )?;
    let config = configuration(&tx)?;
    tx.commit()?;
    files::sync_dir(&offline.root)?;
    Ok(config)
}

pub(crate) fn entry(path: &Path, name: &str, cap: u64) -> Result<Entry> {
    let mut file = files::read_file(path, cap)?;
    let size = file.metadata().map_err(|_| Error::Unavailable)?.len();
    let mut hash = Sha256::new();
    let mut copied = 0;
    let mut bytes = [0; 65536];
    loop {
        let count = file.read(&mut bytes).map_err(|_| Error::Unavailable)?;
        if count == 0 {
            break;
        }
        copied += count as u64;
        if copied > cap {
            return Err(Error::LimitExceeded);
        }
        hash.update(&bytes[..count]);
    }
    if copied != size {
        return Err(Error::Integrity);
    }
    Ok(Entry {
        name: name.into(),
        sha256: hex(&hash.finalize()),
        size,
    })
}
fn append<W: Write>(tar: &mut tar::Builder<W>, item: &Entry, mut input: impl Read) -> Result<()> {
    let mut header = tar::Header::new_ustar();
    header
        .set_path(&item.name)
        .map_err(|_| Error::InvalidInput)?;
    header.set_size(item.size);
    header.set_mode(0o600);
    header.set_uid(0);
    header.set_gid(0);
    header.set_mtime(0);
    header.set_entry_type(tar::EntryType::Regular);
    header.set_cksum();
    tar.append(&header, &mut input)
        .map_err(|_| Error::Unavailable)
}

pub(crate) fn write_archive<W: Write>(
    output: W,
    recipient: &age::x25519::Recipient,
    entries: &[Entry],
    encoded: &[u8],
    snapshot: &Path,
    staging: &Path,
    started: Instant,
) -> Result<W> {
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(recipient as &dyn age::Recipient))
            .map_err(|_| Error::Unavailable)?;
    let writer = encryptor
        .wrap_output(output)
        .map_err(|_| Error::Unavailable)?;
    let mut tar = tar::Builder::new(writer);
    append(
        &mut tar,
        &Entry {
            name: "manifest.json".into(),
            sha256: hex(&Sha256::digest(encoded)),
            size: encoded.len() as u64,
        },
        encoded,
    )?;
    for item in entries {
        check_deadline(started)?;
        let path = if item.name == "assistant.sqlite" {
            snapshot.to_owned()
        } else {
            staging.join(&item.name)
        };
        let current = entry(&path, &item.name, DB_LIMIT)?;
        if current.sha256 != item.sha256 || current.size != item.size {
            return Err(Error::Integrity);
        }
        append(&mut tar, item, files::read_file(&path, DB_LIMIT)?)?;
    }
    tar.finish().map_err(|_| Error::Unavailable)?;
    let writer = tar.into_inner().map_err(|_| Error::Unavailable)?;
    writer.finish().map_err(|_| Error::Unavailable)
}

/// Create a complete encrypted export of the current stopped installation only.
/// It does not supply an independently maintained latest journal or restore authority.
pub fn create(root: &Path, output: &Path) -> Result<ExportSummary> {
    let started = Instant::now();
    let offline = Offline::open(root)?;
    let output = files::output_path(&offline.root, output)?;
    let config = configuration(&offline.conn)?;
    let epoch = config.retained_epochs.last().ok_or(Error::Unconfigured)?;
    let recipient: age::x25519::Recipient =
        epoch.recipient.parse().map_err(|_| Error::Integrity)?;
    let snapshot = offline.temp.0.join("snapshot.sqlite");
    let snapshot_file = files::create_file(&snapshot)?;
    let mut destination = Connection::open(&snapshot)?;
    {
        let backup = rusqlite::backup::Backup::new(&offline.conn, &mut destination)?;
        loop {
            check_deadline(started)?;
            match backup.step(256)? {
                rusqlite::backup::StepResult::Done => break,
                rusqlite::backup::StepResult::More => {}
                _ => return Err(Error::Busy),
            }
        }
    }
    if destination.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok"
        || destination
            .prepare("PRAGMA foreign_key_check")?
            .exists([])?
    {
        return Err(Error::Integrity);
    }
    let (auth, knowledge) = journal_heads(&destination, &offline.auth, &offline.knowledge)?;
    let (successor_applied, successor_head) =
        destructive_intents::verify(&destination, &offline.knowledge, &offline.destructive)?;
    let (successor_complete, complete_head) = offline
        .destructive
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, offline.destructive.binding.header_head));
    let coverage = Coverage {
        version: 1,
        schema_baseline: destructive_intents::COVERAGE_SCHEMA,
        legacy_sequence: offline.destructive.binding.legacy_sequence,
        legacy_head: hex(&offline.destructive.binding.legacy_head),
        successor_format: "MSDSTR01".into(),
        successor_journal: JournalHead {
            sequence: successor_complete,
            head: hex(&complete_head),
            snapshot_sequence: successor_applied,
            snapshot_head: hex(&successor_head),
        },
    };
    drop(destination);
    snapshot_file.sync_all().map_err(|_| Error::Unavailable)?;
    let mut entries = Vec::new();
    for (name, path, cap) in [
        ("assistant.sqlite", snapshot.clone(), DB_LIMIT),
        (
            "auth-epochs.journal",
            offline.temp.0.join("auth-epochs.journal"),
            JOURNAL_LIMIT,
        ),
        (
            "knowledge-invalidations.journal",
            offline.temp.0.join("knowledge-invalidations.journal"),
            JOURNAL_LIMIT,
        ),
        (
            destructive_intents::FILE_NAME,
            offline.temp.0.join(destructive_intents::FILE_NAME),
            JOURNAL_LIMIT,
        ),
    ] {
        entries.push(entry(&path, name, cap)?);
    }
    let export_id = Uuid::new_v4().to_string();
    let manifest = ManifestV2 {
        format: 2,
        purpose: "assistant-export".into(),
        installation_id: config.installation_id,
        export_id: export_id.clone(),
        schema: CURRENT_SCHEMA_VERSION,
        created_at: now()?,
        key_epoch: epoch.epoch,
        recipient: epoch.recipient.clone(),
        entries,
        auth_journal: auth,
        knowledge_journal: knowledge,
        coverage,
        authoritative_files: vec![],
        limitations: vec![
            "current_db_only_no_authoritative_media".into(),
            "snapshot_journals_not_independent_latest_checkpoint".into(),
            "historical_restore_unavailable".into(),
            "server_credentials_require_future_re_registration".into(),
        ],
    };
    let encoded = serde_json::to_vec(&manifest).map_err(|_| Error::Integrity)?;
    if encoded.len() > 32768 {
        return Err(Error::LimitExceeded);
    }
    let parent = output.parent().ok_or(Error::InvalidInput)?;
    let partial = parent.join(format!(".moosuri-export-{}.partial", Uuid::new_v4()));
    let result = (|| {
        let file = files::create_file(&partial)?;
        let file = write_archive(
            file,
            &recipient,
            &manifest.entries,
            &encoded,
            &snapshot,
            &offline.temp.0,
            started,
        )?;
        file.sync_all().map_err(|_| Error::Unavailable)?;
        check_deadline(started)?;
        files::publish(&partial, &output)?;
        Ok(ExportSummary {
            export_id,
            key_epoch: epoch.epoch,
        })
    })();
    if result.is_err() {
        let _ = fs::remove_file(&partial);
    }
    result
}
