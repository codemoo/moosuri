//! Live single-writer capture and an independently operated append-only receiver.
//! Periodic publication has an RPO: local writes never wait for this receiver.
use super::*;
use crate::{
    Store,
    cutover::restore::historical::{JournalManifest, LatestHead, PURPOSE},
};
use fs2::FileExt;
use std::os::unix::fs::DirBuilderExt;
const MAGIC: &[u8; 8] = b"MSLIVE01";
const META_LIMIT: u64 = 32768;
const CIPHER_LIMIT: u64 = 2 * 1024 * 1024 * 1024;
const NAMES: [&str; 3] = [
    "auth-epochs.journal",
    "knowledge-invalidations.journal",
    "destructive-intents.journal",
];

fn encode<T: Serialize>(v: &T) -> Result<Vec<u8>> {
    let b = serde_json::to_vec(v).map_err(|_| Error::Integrity)?;
    if b.len() > META_LIMIT as usize {
        return Err(Error::LimitExceeded);
    }
    Ok(b)
}
fn read_bytes(path: &Path, cap: u64) -> Result<Vec<u8>> {
    let mut b = vec![];
    files::read_file(path, cap)?
        .take(cap + 1)
        .read_to_end(&mut b)
        .map_err(|_| Error::Unavailable)?;
    if b.len() as u64 > cap {
        return Err(Error::LimitExceeded);
    }
    Ok(b)
}
fn write_private(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut f = files::create_file(path)?;
    f.write_all(bytes).map_err(|_| Error::Unavailable)?;
    f.sync_all().map_err(|_| Error::Unavailable)
}
fn same<T: Serialize>(a: &T, b: &T) -> Result<bool> {
    Ok(encode(a)? == encode(b)?)
}
fn digest(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}
fn verify_entry(path: &Path, e: &Entry, cap: u64) -> Result<()> {
    if !same(&entry(path, &e.name, cap)?, e)? {
        return Err(Error::Integrity);
    }
    Ok(())
}
fn deadline(start: Instant) -> Result<()> {
    if start.elapsed() > Duration::from_secs(60) {
        Err(Error::Busy)
    } else {
        Ok(())
    }
}
fn prefix(old: &Path, new: &Path) -> Result<()> {
    let mut a = files::read_file(old, JOURNAL_LIMIT)?;
    let mut b = files::read_file(new, JOURNAL_LIMIT)?;
    let mut x = [0; 65536];
    let mut y = [0; 65536];
    loop {
        let n = a.read(&mut x).map_err(|_| Error::Integrity)?;
        if n == 0 {
            return Ok(());
        }
        b.read_exact(&mut y[..n]).map_err(|_| Error::Integrity)?;
        if x[..n] != y[..n] {
            return Err(Error::Integrity);
        }
    }
}

/// A baseline ciphertext fingerprint; no identity/private key is used.
#[derive(Clone)]
pub struct ExportBinding(Entry);
impl ExportBinding {
    pub fn open(path: &Path) -> Result<Self> {
        Ok(Self(entry(path, "export.age", CIPHER_LIMIT)?))
    }
}
pub struct Captured {
    temp: files::Temporary,
    manifest: JournalManifest,
}
impl Captured {
    pub fn write_to(&self, mut out: impl Write) -> Result<()> {
        let start = Instant::now();
        let meta = encode(&self.manifest)?;
        out.write_all(MAGIC)
            .and_then(|_| out.write_all(&(meta.len() as u32).to_be_bytes()))
            .and_then(|_| out.write_all(&meta))
            .map_err(|_| Error::Unavailable)?;
        for e in &self.manifest.entries {
            deadline(start)?;
            verify_entry(&self.temp.0.join(&e.name), e, JOURNAL_LIMIT)?;
            let n = io::copy(
                &mut files::read_file(&self.temp.0.join(&e.name), JOURNAL_LIMIT)?,
                &mut out,
            )
            .map_err(|_| Error::Unavailable)?;
            if n != e.size {
                return Err(Error::Integrity);
            }
        }
        out.flush().map_err(|_| Error::Unavailable)
    }
    pub fn fingerprint(&self) -> Result<String> {
        capture_digest(&self.manifest)
    }
    pub fn installation_id(&self) -> &str {
        &self.manifest.installation_id
    }
}
impl Store {
    /// The writer blocks only for bounded journal copying, never remote IO or age.
    /// Deadlines are checked between IO; OS stalls do not have a hard wall-time cap.
    pub async fn capture_recovery_journals(&self, export: ExportBinding) -> Result<Captured> {
        self.call(move |db| capture(db, export))
            .await
            .map_err(Error::from)
    }
}
fn capture(db: &mut crate::Database, export: ExportBinding) -> crate::Result<Captured> {
    capture_inner(db, export).map_err(|_| crate::StoreError::Unavailable)
}
fn capture_inner(db: &mut crate::Database, export: ExportBinding) -> Result<Captured> {
    let start = Instant::now();
    let root = PathBuf::from(db.conn.path().ok_or(Error::Integrity)?)
        .parent()
        .ok_or(Error::Integrity)?
        .to_owned();
    crate::cutover::require_active(&db.conn, &root)?;
    let config = configuration(&db.conn)?;
    let epoch = config.retained_epochs.last().ok_or(Error::Unconfigured)?;
    let (a, k) = journal_heads(&db.conn, &db.journal, &db.knowledge_journal)?;
    let (d, dh) =
        destructive_intents::verify(&db.conn, &db.knowledge_journal, &db.destructive_journal)?;
    let complete = db
        .destructive_journal
        .records
        .last()
        .map(|r| (r.seq, r.hash))
        .unwrap_or((0, db.destructive_journal.binding.header_head));
    if a.sequence != a.snapshot_sequence || k.sequence != k.snapshot_sequence || d != complete.0 {
        return Err(Error::JournalPending);
    }
    let temp = files::Temporary::new(&root)?;
    let mut entries = vec![];
    for name in NAMES {
        deadline(start)?;
        files::copy_file(&root.join(name), &temp.0.join(name), JOURNAL_LIMIT)?;
        entries.push(entry(&temp.0.join(name), name, JOURNAL_LIMIT)?);
    }
    // Single writer owns all append methods; verify captured codec heads too.
    let m = JournalManifest {
        format: 1,
        purpose: PURPOSE.into(),
        installation_id: config.installation_id,
        checkpoint_id: Uuid::new_v4().to_string(),
        schema: crate::CURRENT_SCHEMA_VERSION,
        key_epoch: epoch.epoch,
        recipient: epoch.recipient.clone(),
        entries,
        auth_journal: a,
        knowledge_journal: k,
        coverage: Coverage {
            version: 1,
            schema_baseline: 15,
            legacy_sequence: db.destructive_journal.binding.legacy_sequence,
            legacy_head: hex(&db.destructive_journal.binding.legacy_head),
            successor_format: "MSDSTR01".into(),
            successor_journal: JournalHead {
                sequence: d,
                head: hex(&dh),
                snapshot_sequence: d,
                snapshot_head: hex(&dh),
            },
        },
        export: export.0,
    };
    verify_journals(&temp.0, &m)?;
    deadline(start)?;
    Ok(Captured { temp, manifest: m })
}
fn capture_digest(m: &JournalManifest) -> Result<String> {
    let mut m = m.clone();
    m.checkpoint_id = "00000000-0000-0000-0000-000000000000".into();
    Ok(digest(&encode(&m)?))
}
fn verify_journals(root: &Path, m: &JournalManifest) -> Result<()> {
    crate::cutover::restore::historical::validate(m)?;
    let a = journal::Journal::open(&root.join(NAMES[0]))?;
    let k = knowledge_journal::Journal::open(&root.join(NAMES[1]))?;
    let d = destructive_intents::Journal::open(&root.join(NAMES[2]))?;
    let ah = a
        .records
        .last()
        .map(|r| (r.seq, hex(&r.hash)))
        .unwrap_or((0, hex(&[0; 32])));
    let kh = k
        .records
        .last()
        .map(|r| (r.seq, hex(&r.hash)))
        .unwrap_or((0, hex(&[0; 32])));
    let dh = d
        .records
        .last()
        .map(|r| (r.seq, hex(&r.hash)))
        .unwrap_or((0, hex(&d.binding.header_head)));
    if ah != (m.auth_journal.sequence, m.auth_journal.head.clone())
        || kh
            != (
                m.knowledge_journal.sequence,
                m.knowledge_journal.head.clone(),
            )
        || dh
            != (
                m.coverage.successor_journal.sequence,
                m.coverage.successor_journal.head.clone(),
            )
    {
        return Err(Error::Integrity);
    }
    d.check_binding(
        &m.installation_id,
        m.coverage.legacy_sequence,
        &d.binding.legacy_head,
    )?;
    if hex(&d.binding.legacy_head) != m.coverage.legacy_head
        || kh != (m.coverage.legacy_sequence, m.coverage.legacy_head.clone())
    {
        return Err(Error::Integrity);
    }
    for e in &m.entries {
        verify_entry(&root.join(&e.name), e, JOURNAL_LIMIT)?;
    }
    Ok(())
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetConfig {
    format: u32,
    installation_id: String,
    export: Entry,
    artifacts: PathBuf,
    ledger: PathBuf,
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    pub format: u32,
    pub generation: u64,
    pub previous_receipt_sha256: String,
    pub trusted_head_sha256: String,
    pub installation_id: String,
    pub checkpoint_id: String,
    pub manifest_sha256: String,
    pub capture_sha256: String,
    pub ciphertext: Entry,
}
#[derive(Serialize)]
pub struct TargetStatus {
    pub installation_id: String,
    pub current: Option<Acceptance>,
    pub journal: Option<PathBuf>,
    pub trusted_head: Option<PathBuf>,
    pub export: PathBuf,
}
fn config(root: &Path) -> Result<TargetConfig> {
    let root = files::directory(root)?;
    let c: TargetConfig =
        serde_json::from_slice(&read_bytes(&root.join("target.json"), META_LIMIT)?)
            .map_err(|_| Error::Integrity)?;
    if c.format != 1 || Uuid::parse_str(&c.installation_id).is_err() {
        return Err(Error::Integrity);
    }
    files::directory(&c.artifacts)?;
    files::directory(&c.ledger)?;
    Ok(c)
}
fn lock(root: &Path) -> Result<File> {
    let f = files::read_file(&root.join("receiver.lock"), 0)?;
    f.try_lock_exclusive().map_err(|_| Error::Busy)?;
    Ok(f)
}
/// Configure a new receiver from an independently copied baseline. Existing target
/// state is never overwritten. The source needs append-only SSH command access.
pub fn init_target(
    root: &Path,
    artifacts: &Path,
    ledger: &Path,
    export: &Path,
    installation: &str,
) -> Result<()> {
    if Uuid::parse_str(installation).is_err() {
        return Err(Error::InvalidInput);
    }
    let root = files::directory(root)?;
    let artifacts = files::directory(artifacts)?;
    let ledger = files::directory(ledger)?;
    for (a, b) in [(&root, &artifacts), (&root, &ledger), (&artifacts, &ledger)] {
        if a.starts_with(b) || b.starts_with(a) {
            return Err(Error::InvalidInput);
        }
    }
    for (d, name) in [
        (&root, "target.json"),
        (&artifacts, "export.age"),
        (&ledger, "receiver.json"),
    ] {
        files::private_output_path(&d.join(name))?;
    }
    // All roots must start empty. Do not turn a general directory into managed state.
    for d in [&root, &artifacts, &ledger] {
        if fs::read_dir(d)
            .map_err(|_| Error::Unavailable)?
            .next()
            .is_some()
        {
            return Err(Error::OutputExists);
        }
    }
    let export_path = export;
    let export = entry(export_path, "export.age", CIPHER_LIMIT)?;
    let c = TargetConfig {
        format: 1,
        installation_id: installation.into(),
        export,
        artifacts: artifacts.clone(),
        ledger: ledger.clone(),
    };
    files::copy_file(export_path, &artifacts.join("export.age"), CIPHER_LIMIT)?;
    verify_entry(&artifacts.join("export.age"), &c.export, CIPHER_LIMIT)?;
    files::create_file(&root.join("receiver.lock"))?
        .sync_all()
        .map_err(|_| Error::Unavailable)?;
    write_private(&root.join("target.json"), &encode(&c)?)?;
    files::sync_dir(&artifacts)?;
    files::sync_dir(&ledger)?;
    files::sync_dir(&root)?;
    Ok(())
}
fn generation(n: u64) -> String {
    format!("g{n:020}")
}
fn current(c: &TargetConfig) -> Result<Option<(Acceptance, String)>> {
    let mut names = vec![];
    for e in fs::read_dir(&c.ledger).map_err(|_| Error::Unavailable)? {
        let e = e.map_err(|_| Error::Unavailable)?;
        let name = e.file_name().into_string().map_err(|_| Error::Integrity)?;
        if name.starts_with(".partial-") {
            continue;
        }
        if name.len() != 26 || !name.starts_with('g') || !name.ends_with(".json") {
            return Err(Error::Integrity);
        }
        names.push(name);
        if names.len() > 100_000 {
            return Err(Error::LimitExceeded);
        }
    }
    names.sort();
    let mut previous = hex(&[0; 32]);
    let mut last = None;
    for (i, name) in names.iter().enumerate() {
        let n = i as u64 + 1;
        if name != &format!("{}.json", generation(n)) {
            return Err(Error::Integrity);
        }
        let bytes = read_bytes(&c.ledger.join(name), META_LIMIT)?;
        let a: Acceptance = serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)?;
        if a.format != 1
            || a.generation != n
            || a.previous_receipt_sha256 != previous
            || a.installation_id != c.installation_id
        {
            return Err(Error::Integrity);
        }
        previous = digest(&bytes);
        last = Some(a);
    }
    if let Some(a) = last {
        verify_generation(c, &a)?;
        Ok(Some((a, previous)))
    } else {
        Ok(None)
    }
}
fn verify_generation(c: &TargetConfig, a: &Acceptance) -> Result<JournalManifest> {
    let dir = c.artifacts.join(generation(a.generation));
    files::directory(&dir)?;
    let bytes = read_bytes(&dir.join("head.json"), 8192)?;
    if digest(&bytes) != a.trusted_head_sha256 {
        return Err(Error::Integrity);
    }
    let h: LatestHead = serde_json::from_slice(&bytes).map_err(|_| Error::Integrity)?;
    if h.format != 1
        || h.purpose != format!("{PURPOSE}-head")
        || h.manifest.checkpoint_id != a.checkpoint_id
        || h.manifest.installation_id != c.installation_id
        || h.manifest_sha256 != digest(&encode(&h.manifest)?)
        || h.manifest_sha256 != a.manifest_sha256
        || capture_digest(&h.manifest)? != a.capture_sha256
        || !same(&h.ciphertext, &a.ciphertext)?
        || !same(&h.manifest.export, &c.export)?
    {
        return Err(Error::Integrity);
    }
    verify_entry(&dir.join("journal.age"), &h.ciphertext, CIPHER_LIMIT)?;
    verify_journals(&dir, &h.manifest)?;
    Ok(h.manifest)
}
fn sync_current(c: &TargetConfig, a: &Acceptance) -> Result<()> {
    let dir = c.artifacts.join(generation(a.generation));
    for name in [NAMES[0], NAMES[1], NAMES[2], "journal.age", "head.json"] {
        files::read_file(&dir.join(name), CIPHER_LIMIT)?
            .sync_all()
            .map_err(|_| Error::Unavailable)?;
    }
    files::read_file(
        &c.ledger.join(format!("{}.json", generation(a.generation))),
        META_LIMIT,
    )?
    .sync_all()
    .map_err(|_| Error::Unavailable)?;
    files::sync_dir(&dir)?;
    files::sync_dir(&c.artifacts)?;
    files::sync_dir(&c.ledger)
}
pub fn target_status(root: &Path) -> Result<TargetStatus> {
    let c = config(root)?;
    let _lock = lock(root)?;
    verify_entry(&c.artifacts.join("export.age"), &c.export, CIPHER_LIMIT)?;
    let a = current(&c)?.map(|v| v.0);
    if let Some(a) = &a {
        sync_current(&c, a)?;
    }
    Ok(TargetStatus {
        installation_id: c.installation_id.clone(),
        journal: a.as_ref().map(|a| {
            c.artifacts
                .join(generation(a.generation))
                .join("journal.age")
        }),
        trusted_head: a
            .as_ref()
            .map(|a| c.artifacts.join(generation(a.generation)).join("head.json")),
        current: a,
        export: c.artifacts.join("export.age"),
    })
}
struct TargetStage(PathBuf);
impl Drop for TargetStage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}
fn stage(parent: &Path) -> Result<TargetStage> {
    let path = parent.join(format!(".stage-{}", Uuid::new_v4()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&path)
        .map_err(|_| Error::Unavailable)?;
    Ok(TargetStage(path))
}
fn incoming(input: &mut impl Read, root: &Path) -> Result<JournalManifest> {
    let mut magic = [0; 8];
    input.read_exact(&mut magic).map_err(|_| Error::Integrity)?;
    if &magic != MAGIC {
        return Err(Error::Integrity);
    }
    let mut length = [0; 4];
    input
        .read_exact(&mut length)
        .map_err(|_| Error::Integrity)?;
    let length = u32::from_be_bytes(length) as u64;
    if length > META_LIMIT {
        return Err(Error::LimitExceeded);
    }
    let mut meta = vec![0; length as usize];
    input.read_exact(&mut meta).map_err(|_| Error::Integrity)?;
    let m: JournalManifest = serde_json::from_slice(&meta).map_err(|_| Error::Integrity)?;
    crate::cutover::restore::historical::validate(&m)?;
    for e in &m.entries {
        let mut out = files::create_file(&root.join(&e.name))?;
        let count = io::copy(&mut std::io::Read::by_ref(input).take(e.size), &mut out)
            .map_err(|_| Error::Integrity)?;
        if count != e.size {
            return Err(Error::Integrity);
        }
        out.sync_all().map_err(|_| Error::Unavailable)?;
    }
    let mut extra = [0];
    if input.read(&mut extra).map_err(|_| Error::Integrity)? != 0 {
        return Err(Error::Integrity);
    }
    verify_journals(root, &m)?;
    Ok(m)
}
/// Receive only bounded opaque journals, independently check extension, then
/// encrypt and atomically accept a new latest head. Caller bounds stdin lifetime.
pub fn receive(root: &Path, mut input: impl Read) -> Result<Acceptance> {
    let start = Instant::now();
    let c = config(root)?;
    let _lock = lock(root)?;
    verify_entry(&c.artifacts.join("export.age"), &c.export, CIPHER_LIMIT)?;
    let stage = stage(&c.artifacts)?;
    let m = incoming(&mut input, &stage.0)?;
    deadline(start)?;
    if m.installation_id != c.installation_id || !same(&m.export, &c.export)? {
        return Err(Error::Integrity);
    }
    let old = current(&c)?;
    if let Some((a, _)) = &old {
        let previous = verify_generation(&c, a)?;
        if m.schema < previous.schema
            || m.key_epoch < previous.key_epoch
            || (m.key_epoch == previous.key_epoch && m.recipient != previous.recipient)
            || m.coverage.legacy_sequence != previous.coverage.legacy_sequence
            || m.coverage.legacy_head != previous.coverage.legacy_head
        {
            return Err(Error::Integrity);
        }
        for name in NAMES {
            prefix(
                &c.artifacts.join(generation(a.generation)).join(name),
                &stage.0.join(name),
            )?;
        }
        if same(&m.entries, &previous.entries)?
            && m.key_epoch == previous.key_epoch
            && m.schema == previous.schema
        {
            sync_current(&c, a)?;
            return Ok(a.clone());
        }
    }
    let n = old.as_ref().map(|v| v.0.generation + 1).unwrap_or(1);
    if n > 100_000 {
        return Err(Error::LimitExceeded);
    }
    let generation_path = c.artifacts.join(generation(n));
    // An interrupted pre-receipt generation is never latest. It may be retained
    // under an orphan name, never overwritten or confused with accepted history.
    if fs::symlink_metadata(&generation_path).is_ok() {
        files::directory(&generation_path)?;
        crate::cutover::publication::no_replace(
            &generation_path,
            &c.artifacts.join(format!(".orphan-{}", Uuid::new_v4())),
        )?;
        files::sync_dir(&c.artifacts)?;
    }
    let f = files::create_file(&stage.0.join("journal.age"))?;
    let f = write_archive(
        f,
        &m.recipient.parse().map_err(|_| Error::Integrity)?,
        &m.entries,
        &encode(&m)?,
        &stage.0.join("unused.sqlite"),
        &stage.0,
        start,
    )?;
    f.sync_all().map_err(|_| Error::Unavailable)?;
    let cipher = entry(&stage.0.join("journal.age"), "journal.age", CIPHER_LIMIT)?;
    let mh = digest(&encode(&m)?);
    let h = LatestHead {
        format: 1,
        purpose: format!("{PURPOSE}-head"),
        manifest: m.clone(),
        manifest_sha256: mh.clone(),
        ciphertext: cipher.clone(),
    };
    let head = encode(&h)?;
    if head.len() > 8192 {
        return Err(Error::LimitExceeded);
    }
    write_private(&stage.0.join("head.json"), &head)?;
    files::sync_dir(&stage.0)?;
    fault(root, "before_generation")?;
    crate::cutover::publication::no_replace(&stage.0, &generation_path)?;
    files::sync_dir(&c.artifacts)?;
    fault(root, "after_generation")?;
    let capture_sha256 = capture_digest(&m)?;
    let a = Acceptance {
        format: 1,
        generation: n,
        previous_receipt_sha256: old.map(|v| v.1).unwrap_or_else(|| hex(&[0; 32])),
        trusted_head_sha256: digest(&head),
        installation_id: m.installation_id,
        checkpoint_id: m.checkpoint_id,
        manifest_sha256: mh,
        capture_sha256,
        ciphertext: cipher,
    };
    let partial = c.ledger.join(format!(".partial-{}", Uuid::new_v4()));
    write_private(&partial, &encode(&a)?)?;
    fault(root, "before_receipt")?;
    // A hard-link/unlink publication can leave nlink=2 after a crash, which
    // violates strict private-file admission on retry. Atomic no-replace rename
    // publishes exactly one name and never overwrites an accepted receipt.
    crate::cutover::publication::no_replace(
        &partial,
        &c.ledger.join(format!("{}.json", generation(n))),
    )?;
    files::sync_dir(&c.ledger)?;
    fault(root, "after_receipt")?;
    sync_current(&c, &a)?;
    Ok(a)
}
#[cfg(test)]
static FAILURES: std::sync::Mutex<Vec<(PathBuf, String)>> = std::sync::Mutex::new(vec![]);
fn fault(_root: &Path, _point: &str) -> Result<()> {
    #[cfg(test)]
    if FAILURES
        .lock()
        .unwrap()
        .iter()
        .any(|(r, p)| r == _root && p == _point)
    {
        return Err(Error::Unavailable);
    }
    Ok(())
}
#[cfg(test)]
mod tests;
