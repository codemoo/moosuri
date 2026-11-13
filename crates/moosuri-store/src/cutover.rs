//! Sealed local capture and exact matching offline restore; no off-host publication.
use crate::{backup, destructive_intents};
use backup::files;
use backup::{Coverage, Entry, Error, JournalHead, Result};
use rusqlite::{Connection, OpenFlags};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use uuid::Uuid;
mod admission;
mod publication;
pub(crate) mod restore;
pub(crate) use admission::{
    RuntimeAdmission, require_active, runtime_admission, sync_parent_before_admission,
};
pub use restore::{EnrollmentAccount, RestoreSummary, restore};
#[cfg(test)]
mod tests;
pub(crate) const MARKER: &str = "cutover-seal.json";
pub(crate) const STAGING: &str = "cutover-checkpoint";
pub(crate) const SCHEMA: i64 = 19;
const CIPHER_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const DEADLINE: Duration = Duration::from_secs(60);
const JOURNALS: [&str; 3] = [
    "auth-epochs.journal",
    "knowledge-invalidations.journal",
    "destructive-intents.journal",
];

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Seal {
    pub version: u32,
    pub installation_id: String,
    pub seal_id: String,
    pub checkpoint_id: String,
    pub schema: i64,
    pub key_epoch: i64,
    pub recipient: String,
    pub auth_journal: JournalHead,
    pub knowledge_journal: JournalHead,
    pub coverage: Coverage,
    pub entries: Vec<Entry>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: u32,
    pub purpose: String,
    pub seal: Seal,
    pub seal_sha256: String,
    pub entries: Vec<Entry>,
    pub authoritative_files: Vec<String>,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TrustedHead {
    pub format: u32,
    pub purpose: String,
    pub closure_kind: String,
    pub installation_id: String,
    pub seal_id: String,
    pub checkpoint_id: String,
    pub schema: i64,
    pub key_epoch: i64,
    pub seal_sha256: String,
    pub manifest_sha256: String,
    pub ciphertext_sha256: String,
    pub ciphertext_size: u64,
    pub auth_journal: JournalHead,
    pub knowledge_journal: JournalHead,
    pub coverage: Coverage,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    manifest: Manifest,
    head: TrustedHead,
}
enum State {
    Active,
    Sealed(Seal),
    Ready(Seal, Box<Receipt>),
}
pub struct CheckpointSummary {
    pub seal_id: String,
    pub checkpoint_id: String,
    pub ciphertext_sha256: String,
}
fn encode<T: Serialize + ?Sized>(value: &T, cap: usize) -> Result<Vec<u8>> {
    let b = serde_json::to_vec(value).map_err(|_| Error::Integrity)?;
    if b.len() > cap {
        return Err(Error::LimitExceeded);
    }
    Ok(b)
}
fn decode<T: serde::de::DeserializeOwned>(value: &str, cap: usize) -> Result<T> {
    if value.len() > cap {
        return Err(Error::LimitExceeded);
    }
    serde_json::from_str(value).map_err(|_| Error::Integrity)
}
fn hash(bytes: &[u8]) -> String {
    backup::hex(&Sha256::digest(bytes))
}
fn same<A: Serialize + ?Sized, B: Serialize + ?Sized>(a: &A, b: &B) -> Result<bool> {
    Ok(encode(a, 65536)? == encode(b, 65536)?)
}
fn deadline(start: Instant) -> Result<()> {
    if start.elapsed() > DEADLINE {
        Err(Error::Busy)
    } else {
        Ok(())
    }
}
fn validate_seal(seal: &Seal) -> Result<()> {
    if seal.version != 1
        || seal.schema < SCHEMA
        || seal.schema > crate::CURRENT_SCHEMA_VERSION
        || seal.key_epoch <= 0
        || [&seal.installation_id, &seal.seal_id, &seal.checkpoint_id]
            .iter()
            .any(|id| Uuid::parse_str(id).is_err())
        || seal.recipient.parse::<age::x25519::Recipient>().is_err()
        || seal.coverage.version != 1
        || seal.coverage.schema_baseline != 15
        || seal.coverage.successor_format != "MSDSTR01"
        || seal.entries.len() != 3
    {
        return Err(Error::Integrity);
    }
    for (entry, name) in seal.entries.iter().zip(JOURNALS) {
        if entry.name != name || entry.size > backup::JOURNAL_LIMIT || !digest(&entry.sha256) {
            return Err(Error::Integrity);
        }
    }
    for head in [
        &seal.auth_journal,
        &seal.knowledge_journal,
        &seal.coverage.successor_journal,
    ] {
        if head.sequence < 0
            || head.sequence != head.snapshot_sequence
            || head.head != head.snapshot_head
            || !digest(&head.head)
        {
            return Err(Error::Integrity);
        }
    }
    if seal.coverage.legacy_sequence != seal.knowledge_journal.sequence
        || seal.coverage.legacy_head != seal.knowledge_journal.head
    {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn digest(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}
fn state(conn: &Connection) -> Result<State> {
    let (phase_len,seal_len,receipt_len):(i64,Option<i64>,Option<i64>)=conn.query_row("SELECT length(CAST(phase AS BLOB)),length(CAST(seal AS BLOB)),length(CAST(checkpoint AS BLOB)) FROM installation_cutover WHERE singleton=1",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    if phase_len > 32
        || seal_len.is_some_and(|n| n > 16384)
        || receipt_len.is_some_and(|n| n > 65536)
    {
        return Err(Error::LimitExceeded);
    }
    let count: i64 = conn.query_row("SELECT count(*) FROM installation_cutover", [], |r| {
        r.get(0)
    })?;
    if count != 1 {
        return Err(Error::Integrity);
    }
    let (phase, seal, receipt): (String, Option<String>, Option<String>) = conn.query_row(
        "SELECT phase,seal,checkpoint FROM installation_cutover WHERE singleton=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
    )?;
    match (phase.as_str(), seal, receipt) {
        ("active", None, None) => Ok(State::Active),
        ("sealed", Some(s), None) => {
            let seal: Seal = decode(&s, 16384)?;
            validate_seal(&seal)?;
            Ok(State::Sealed(seal))
        }
        ("checkpoint_ready", Some(s), Some(r)) => {
            let seal: Seal = decode(&s, 16384)?;
            validate_seal(&seal)?;
            let receipt: Receipt = decode(&r, 65536)?;
            validate_receipt(&seal, &receipt)?;
            Ok(State::Ready(seal, Box::new(receipt)))
        }
        _ => Err(Error::Integrity),
    }
}
fn head(manifest: &Manifest, cipher: &Entry) -> Result<TrustedHead> {
    let s = &manifest.seal;
    Ok(TrustedHead {
        format: 1,
        purpose: "assistant-sealed-cutover-head".into(),
        closure_kind: "sealed_installation_cutover_v1".into(),
        installation_id: s.installation_id.clone(),
        seal_id: s.seal_id.clone(),
        checkpoint_id: s.checkpoint_id.clone(),
        schema: s.schema,
        key_epoch: s.key_epoch,
        seal_sha256: manifest.seal_sha256.clone(),
        manifest_sha256: hash(&encode(manifest, 32768)?),
        ciphertext_sha256: cipher.sha256.clone(),
        ciphertext_size: cipher.size,
        auth_journal: s.auth_journal.clone(),
        knowledge_journal: s.knowledge_journal.clone(),
        coverage: s.coverage.clone(),
    })
}
fn validate_manifest(m: &Manifest) -> Result<()> {
    validate_seal(&m.seal)?;
    if m.format != 1
        || m.purpose != "assistant-sealed-cutover"
        || m.seal_sha256 != hash(&encode(&m.seal, 16384)?)
        || !m.authoritative_files.is_empty()
        || m.entries.len() != 5
    {
        return Err(Error::Integrity);
    }
    let names = [
        "assistant.sqlite",
        JOURNALS[0],
        JOURNALS[1],
        JOURNALS[2],
        MARKER,
    ];
    for (e, name) in m.entries.iter().zip(names) {
        let cap = if name == "assistant.sqlite" {
            backup::DB_LIMIT
        } else if name == MARKER {
            16384
        } else {
            backup::JOURNAL_LIMIT
        };
        if e.name != name || e.size > cap || !digest(&e.sha256) {
            return Err(Error::Integrity);
        }
    }
    if m.entries[0].size == 0
        || !same(&m.entries[1..4], &m.seal.entries)?
        || m.entries[4].sha256 != m.seal_sha256
        || m.entries[4].size != encode(&m.seal, 16384)?.len() as u64
    {
        return Err(Error::Integrity);
    }
    Ok(())
}
/// Bounded metadata codec only; does not decrypt, extract, import or activate.
pub fn decode_manifest(bytes: &[u8]) -> Result<Manifest> {
    if bytes.len() > 32768 {
        return Err(Error::LimitExceeded);
    }
    let m: Manifest = serde_json::from_slice(bytes).map_err(|_| Error::Integrity)?;
    validate_manifest(&m)?;
    Ok(m)
}
/// A parsed local head is not automatically trusted or independently fresh.
pub fn decode_head(bytes: &[u8]) -> Result<TrustedHead> {
    if bytes.len() > 8192 {
        return Err(Error::LimitExceeded);
    }
    let h: TrustedHead = serde_json::from_slice(bytes).map_err(|_| Error::Integrity)?;
    if h.format != 1
        || h.purpose != "assistant-sealed-cutover-head"
        || h.closure_kind != "sealed_installation_cutover_v1"
        || h.schema < SCHEMA
        || h.schema > crate::CURRENT_SCHEMA_VERSION
        || h.key_epoch <= 0
        || h.ciphertext_size == 0
        || h.ciphertext_size > CIPHER_LIMIT
        || [&h.installation_id, &h.seal_id, &h.checkpoint_id]
            .iter()
            .any(|s| Uuid::parse_str(s).is_err())
        || [&h.seal_sha256, &h.manifest_sha256, &h.ciphertext_sha256]
            .iter()
            .any(|s| !digest(s))
        || h.coverage.version != 1
        || h.coverage.schema_baseline != 15
        || h.coverage.successor_format != "MSDSTR01"
        || h.coverage.legacy_sequence != h.knowledge_journal.sequence
        || h.coverage.legacy_head != h.knowledge_journal.head
    {
        return Err(Error::Integrity);
    }
    for j in [
        &h.auth_journal,
        &h.knowledge_journal,
        &h.coverage.successor_journal,
    ] {
        if j.sequence < 0
            || j.sequence != j.snapshot_sequence
            || j.head != j.snapshot_head
            || !digest(&j.head)
        {
            return Err(Error::Integrity);
        }
    }
    Ok(h)
}
fn validate_receipt(seal: &Seal, r: &Receipt) -> Result<()> {
    validate_manifest(&r.manifest)?;
    if !same(&r.manifest.seal, seal)? {
        return Err(Error::Integrity);
    }
    decode_head(&encode(&r.head, 8192)?)?;
    if !same(
        &head(
            &r.manifest,
            &Entry {
                name: String::new(),
                sha256: r.head.ciphertext_sha256.clone(),
                size: r.head.ciphertext_size,
            },
        )?,
        &r.head,
    )? {
        return Err(Error::Integrity);
    }
    Ok(())
}

fn descriptor(o: &backup::Offline, seal_id: String, checkpoint_id: String) -> Result<Seal> {
    let config = backup::configuration(&o.conn)?;
    let epoch = config.retained_epochs.last().ok_or(Error::Unconfigured)?;
    let (auth, knowledge) = backup::journal_heads(&o.conn, &o.auth, &o.knowledge)?;
    let (applied, applied_head) =
        destructive_intents::verify(&o.conn, &o.knowledge, &o.destructive)?;
    let (complete, complete_head) = o
        .destructive
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, o.destructive.binding.header_head));
    if auth.sequence != auth.snapshot_sequence
        || knowledge.sequence != knowledge.snapshot_sequence
        || applied != complete
    {
        return Err(Error::JournalPending);
    }
    let mut entries = vec![];
    for name in JOURNALS {
        entries.push(backup::entry(
            &o.temp.0.join(name),
            name,
            backup::JOURNAL_LIMIT,
        )?)
    }
    let seal = Seal {
        version: 1,
        installation_id: config.installation_id,
        seal_id,
        checkpoint_id,
        schema: crate::CURRENT_SCHEMA_VERSION,
        key_epoch: epoch.epoch,
        recipient: epoch.recipient.clone(),
        auth_journal: auth,
        knowledge_journal: knowledge,
        coverage: Coverage {
            version: 1,
            schema_baseline: 15,
            legacy_sequence: o.destructive.binding.legacy_sequence,
            legacy_head: backup::hex(&o.destructive.binding.legacy_head),
            successor_format: "MSDSTR01".into(),
            successor_journal: JournalHead {
                sequence: complete,
                head: backup::hex(&complete_head),
                snapshot_sequence: applied,
                snapshot_head: backup::hex(&applied_head),
            },
        },
        entries,
    };
    validate_seal(&seal)?;
    Ok(seal)
}
fn writable(root: &Path) -> Result<Connection> {
    let conn = Connection::open_with_flags(
        root.join("assistant.sqlite"),
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NOFOLLOW,
    )?;
    conn.execute_batch(
        "PRAGMA foreign_keys=ON;PRAGMA synchronous=FULL;PRAGMA trusted_schema=OFF;",
    )?;
    Ok(conn)
}
fn sync_journals(root: &Path) -> Result<()> {
    for name in JOURNALS {
        let f = files::read_file(&root.join(name), backup::JOURNAL_LIMIT)?;
        fault(root, "journal_sync")?;
        f.sync_all().map_err(|_| Error::Unavailable)?;
    }
    fault(root, "journal_dir_sync")?;
    files::sync_dir(root)
}
fn marker(root: &Path, seal: &Seal) -> Result<()> {
    let encoded = encode(seal, 16384)?;
    let path = root.join(MARKER);
    let mut file = match fs::symlink_metadata(&path) {
        Ok(_) => {
            let f = files::read_file(&path, 16384)?;
            let mut b = vec![];
            f.take(16385)
                .read_to_end(&mut b)
                .map_err(|_| Error::Unavailable)?;
            if !encoded.starts_with(&b) {
                return Err(Error::Integrity);
            }
            let mut f = OpenOptions::new()
                .append(true)
                .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                .open(&path)
                .map_err(|_| Error::Unavailable)?;
            f.write_all(&encoded[b.len()..])
                .map_err(|_| Error::Unavailable)?;
            f
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            let mut f = files::create_file(&path)?;
            f.write_all(&encoded).map_err(|_| Error::Unavailable)?;
            f
        }
        Err(_) => return Err(Error::Unavailable),
    };
    file.flush().map_err(|_| Error::Unavailable)?;
    fault(root, "marker_sync")?;
    file.sync_all().map_err(|_| Error::Unavailable)?;
    fault(root, "marker_dir_sync")?;
    files::sync_dir(root)
}
use std::os::unix::fs::OpenOptionsExt;
/// Reports only the stored local phase, not output verification or latestness.
pub struct CheckpointStatus {
    pub phase: &'static str,
    pub seal_id: Option<String>,
    pub checkpoint_id: Option<String>,
}
pub fn status(root: &Path) -> Result<CheckpointStatus> {
    if rusqlite::version_number() < 3_054_000 {
        return Err(Error::UnsupportedInstallation);
    }
    let (root, _lock) = files::stopped_lock(root)?;
    let (_temp, conn) = admission::inspect(&root)?;
    let schema: i64 = conn.query_row("PRAGMA user_version", [], |r| r.get(0))?;
    if schema != crate::CURRENT_SCHEMA_VERSION {
        return Err(Error::UnsupportedInstallation);
    }
    let (phase, seal) = match state(&conn)? {
        State::Active => {
            if admission::controls_present(&root)? {
                return Err(Error::Integrity);
            }
            ("active", None)
        }
        State::Sealed(s) => ("sealed", Some(s)),
        State::Ready(s, _) => ("checkpoint_ready", Some(s)),
    };
    Ok(CheckpointStatus {
        phase,
        seal_id: seal.as_ref().map(|s| s.seal_id.clone()),
        checkpoint_id: seal.map(|s| s.checkpoint_id),
    })
}
/// Seal a stopped installation and publish one exact locally durable checkpoint.
/// This does not restore data, transfer files off-host or authenticate a publisher.
pub fn checkpoint(root: &Path, output: &Path, head_output: &Path) -> Result<CheckpointSummary> {
    let started = Instant::now();
    let mut o = backup::Offline::for_cutover(root)?;
    let output = publication::target(&o.root, output)?;
    let head_output = publication::target(&o.root, head_output)?;
    let output_parent = output.parent().ok_or(Error::InvalidInput)?;
    let head_parent = head_output.parent().ok_or(Error::InvalidInput)?;
    if output_parent.starts_with(head_parent) || head_parent.starts_with(output_parent) {
        return Err(Error::InvalidInput);
    }
    let current = state(&o.conn)?;
    let seal = match current {
        State::Active => {
            if admission::controls_present(&o.root)? {
                return Err(Error::Integrity);
            }
            // Before irreversible adoption, conflicts must reject without sealing.
            for p in [&output, &head_output] {
                match fs::symlink_metadata(p) {
                    Ok(_) => return Err(Error::OutputExists),
                    Err(e) if e.kind() == io::ErrorKind::NotFound => (),
                    Err(_) => return Err(Error::InvalidInput),
                }
            }
            let s = descriptor(&o, Uuid::new_v4().to_string(), Uuid::new_v4().to_string())?;
            sync_journals(&o.root)?;
            deadline(started)?;
            let mut conn = writable(&o.root)?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if !matches!(state(&tx)?, State::Active) {
                return Err(Error::Integrity);
            }
            tx.execute(
                "UPDATE installation_cutover SET phase='sealed',seal=?1 WHERE singleton=1",
                [String::from_utf8(encode(&s, 16384)?).map_err(|_| Error::Integrity)?],
            )?;
            fault(&o.root, "before_seal_commit")?;
            tx.commit()?;
            fault(&o.root, "after_seal_commit")?;
            s
        }
        State::Sealed(s) | State::Ready(s, _) => s,
    };
    if !same(
        &descriptor(&o, seal.seal_id.clone(), seal.checkpoint_id.clone())?,
        &seal,
    )? {
        return Err(Error::Integrity);
    }
    marker(&o.root, &seal)?;
    deadline(started)?;
    // Current copy may predate seal adoption. Re-capture while retaining the lock.
    o = o.refresh()?;
    if !same(
        &descriptor(&o, seal.seal_id.clone(), seal.checkpoint_id.clone())?,
        &seal,
    )? {
        return Err(Error::Integrity);
    }
    let receipt = match state(&o.conn)? {
        State::Ready(_, r) => *r,
        State::Sealed(_) => {
            fault(&o.root, "snapshot")?;
            let snapshot = o.temp.0.join("snapshot.sqlite");
            let f = files::create_file(&snapshot)?;
            let mut dest = Connection::open(&snapshot)?;
            {
                let b = rusqlite::backup::Backup::new(&o.conn, &mut dest)?;
                loop {
                    deadline(started)?;
                    match b.step(256)? {
                        rusqlite::backup::StepResult::Done => break,
                        rusqlite::backup::StepResult::More => (),
                        _ => return Err(Error::Busy),
                    }
                }
            }
            crate::push::strip_for_export(&dest)?;
            if !matches!(state(&dest)?, State::Sealed(_))
                || dest.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))? != "ok"
                || dest.prepare("PRAGMA foreign_key_check")?.exists([])?
            {
                return Err(Error::Integrity);
            }
            drop(dest);
            f.sync_all().map_err(|_| Error::Unavailable)?;
            files::copy_file(&o.root.join(MARKER), &o.temp.0.join(MARKER), 16384)?;
            let mut entries = vec![backup::entry(
                &snapshot,
                "assistant.sqlite",
                backup::DB_LIMIT,
            )?];
            entries.extend(seal.entries.clone());
            entries.push(backup::entry(&o.temp.0.join(MARKER), MARKER, 16384)?);
            let manifest = Manifest {
                format: 1,
                purpose: "assistant-sealed-cutover".into(),
                seal: seal.clone(),
                seal_sha256: hash(&encode(&seal, 16384)?),
                entries,
                authoritative_files: vec![],
            };
            let stage = publication::stage(&o.root, &seal, true)?;
            let file = files::create_file(&stage)?;
            fault(&o.root, "age_finish")?;
            let file = backup::write_archive(
                publication::BoundedWriter::new(file, CIPHER_LIMIT),
                &seal.recipient.parse().map_err(|_| Error::Integrity)?,
                &manifest.entries,
                &encode(&manifest, 32768)?,
                &snapshot,
                &o.temp.0,
                started,
            )?
            .into_inner();
            fault(&o.root, "stage_sync")?;
            file.sync_all().map_err(|_| Error::Unavailable)?;
            fault(&o.root, "stage_dir_sync")?;
            files::sync_dir(stage.parent().ok_or(Error::Integrity)?)?;
            let cipher = backup::entry(&stage, "checkpoint.age", CIPHER_LIMIT)?;
            let r = Receipt {
                head: head(&manifest, &cipher)?,
                manifest,
            };
            validate_receipt(&seal, &r)?;
            let mut conn = writable(&o.root)?;
            let tx = conn.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
            if !matches!(state(&tx)?, State::Sealed(_)) {
                return Err(Error::Integrity);
            }
            tx.execute("UPDATE installation_cutover SET phase='checkpoint_ready',checkpoint=?1 WHERE singleton=1",[String::from_utf8(encode(&r,65536)?).map_err(|_|Error::Integrity)?])?;
            fault(&o.root, "before_ready_commit")?;
            tx.commit()?;
            fault(&o.root, "after_ready_commit")?;
            r
        }
        State::Active => return Err(Error::Integrity),
    };
    validate_receipt(&seal, &receipt)?;
    deadline(started)?;
    fault(&o.root, "ready_sync")?;
    let stage = publication::ready_stage(&o.root, &seal, &output, &receipt.head)?;
    publication::publish_file(
        &o.root,
        &stage,
        &output,
        &seal.checkpoint_id,
        (
            &receipt.head.ciphertext_sha256,
            receipt.head.ciphertext_size,
        ),
        "cipher",
        started,
    )?;
    let encoded = encode(&receipt.head, 8192)?;
    publication::publish_bytes(
        &o.root,
        &encoded,
        &head_output,
        &seal.checkpoint_id,
        "head",
        started,
    )?;
    Ok(CheckpointSummary {
        seal_id: seal.seal_id,
        checkpoint_id: seal.checkpoint_id,
        ciphertext_sha256: receipt.head.ciphertext_sha256,
    })
}
#[cfg(test)]
static FAULTS: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(Vec::new());
fn fault(_root: &Path, _point: &str) -> Result<()> {
    #[cfg(test)]
    if std::env::var_os("MOOSURI_CUTOVER_EXIT_ROOT").is_some_and(|r| Path::new(&r) == _root)
        && std::env::var("MOOSURI_CUTOVER_EXIT_POINT").is_ok_and(|p| p == _point)
    {
        // Abrupt synthetic child exit: no Rust Drop/cleanup runs at this boundary.
        std::process::exit(73);
    }
    #[cfg(test)]
    if FAULTS
        .lock()
        .unwrap()
        .iter()
        .any(|(r, p)| r == _root && p == _point)
    {
        return Err(Error::Unavailable);
    }
    Ok(())
}
