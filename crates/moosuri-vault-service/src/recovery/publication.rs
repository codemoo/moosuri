//! Periodic off-host full-journal publication. Receiver stores only encrypted
//! journals and exact immutable receipts; recovery identities remain offline.
use super::*;
use fs2::FileExt;
use historical::{JournalManifest, JournalTrustedHead};
use std::time::{Duration, Instant};
mod transport;
pub use transport::{Worker, WorkerConfig, receive_command, target_root};
const WIRE: &[u8; 8] = b"MVJP0001";
const FORMAT: &str = "moosuri-vault-journal-publication";
const GENERATIONS: u64 = 100_000;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    format: u32,
    recipient: String,
    baseline_sha256: String,
    binding: CheckpointBinding,
    journal_size: u64,
    journal_sha256: String,
}
impl Request {
    fn fingerprint(&self) -> Result<String> {
        let mut value = self.clone();
        value.binding.checkpoint = uuid::Uuid::nil();
        Ok(hash(&encode(&value)?))
    }
    fn validate(&self) -> Result<()> {
        let b = &self.binding;
        let _: age::x25519::Recipient = self.recipient.parse().map_err(|_| Error::Invalid)?;
        if self.format != 1
            || b.format != historical::FORMAT
            || b.version != 1
            || b.source.is_nil()
            || b.checkpoint.is_nil()
            || b.sequence < 0
            || self.journal_size < 24
            || self.journal_size > JOURNAL_LIMIT
            || b.sequence as u64 > self.journal_size.saturating_sub(24) / 37
        {
            return Err(Error::Invalid);
        }
        for digest in [&self.baseline_sha256, &self.journal_sha256, &b.head] {
            unhex(digest)?;
        }
        Ok(())
    }
}
pub(super) struct Prepared {
    capture: online::CapturedSnapshot,
    request: Request,
}
impl online::CapturedSnapshot {
    fn prepare_publication(self, recipient: &str, baseline: &Path) -> Result<Prepared> {
        let bytes = bytes(baseline, JSON_LIMIT)?;
        let head: SnapshotTrustedHead =
            serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
        snapshot_binding(&head)?;
        if self.binding.source != head.snapshot.source
            || self.binding.sequence < head.snapshot.sequence
        {
            return Err(Error::Conflict);
        }
        let parent = self.stage.0.parent().ok_or(Error::Invalid)?;
        let journal = self.stage.0.join("vault.journal");
        let db = Database::open_recovery(&self.stage.0)?;
        if installation(&db)? != self.binding.source
            || db.sequence != self.binding.sequence
            || db.head != unhex(&self.binding.head)?
        {
            return Err(Error::Unavailable);
        }
        verify_semantics(&db, &journal, parent)?;
        drop(db);
        let (journal_size, journal_sha256) = digest_file(&journal, JOURNAL_LIMIT)?;
        let mut binding = self.binding.clone();
        binding.format = historical::FORMAT.into();
        let request = Request {
            format: 1,
            recipient: recipient.into(),
            baseline_sha256: hash(&bytes),
            binding,
            journal_size,
            journal_sha256,
        };
        request.validate()?;
        Ok(Prepared {
            capture: self,
            request,
        })
    }
}
impl Prepared {
    fn write_to(&self, mut out: impl Write) -> Result<()> {
        let meta = encode(&self.request)?;
        if meta.len() as u64 > JSON_LIMIT {
            return Err(Error::Limit);
        }
        out.write_all(WIRE)?;
        out.write_all(&(meta.len() as u32).to_be_bytes())?;
        out.write_all(&meta)?;
        let mut input = open_private(&self.capture.stage.0.join("vault.journal"), JOURNAL_LIMIT)?;
        if std::io::copy(&mut input, &mut out)? != self.request.journal_size {
            return Err(Error::Unavailable);
        }
        out.flush()?;
        Ok(())
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct TargetConfig {
    format: String,
    version: u32,
    source: uuid::Uuid,
    recipient: String,
    baseline: SnapshotTrustedHead,
    baseline_sha256: String,
    artifacts: PathBuf,
    ledger: PathBuf,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Acceptance {
    pub format: String,
    pub version: u32,
    pub generation: u64,
    pub source: uuid::Uuid,
    pub capture_sha256: String,
    pub previous_receipt_sha256: String,
    pub trusted_head_sha256: String,
    pub journal: JournalTrustedHead,
    pub journal_size: u64,
    pub journal_sha256: String,
    pub baseline_sha256: String,
    pub recipient: String,
}
#[derive(Serialize)]
pub struct TargetStatus {
    pub source: uuid::Uuid,
    pub current: Option<Acceptance>,
    pub snapshot: PathBuf,
    pub snapshot_head: PathBuf,
    pub journal: Option<PathBuf>,
    pub journal_head: Option<PathBuf>,
}
fn bytes(path: &Path, limit: u64) -> Result<Vec<u8>> {
    let mut value = vec![];
    open_private(path, limit)?
        .take(limit + 1)
        .read_to_end(&mut value)?;
    if value.len() as u64 > limit {
        return Err(Error::Limit);
    }
    Ok(value)
}
fn snapshot_binding(head: &SnapshotTrustedHead) -> Result<()> {
    let b = &head.snapshot;
    if b.format != SNAPSHOT_FORMAT
        || b.version != 1
        || b.source.is_nil()
        || b.checkpoint.is_nil()
        || b.sequence < 0
        || head.ciphertext_size > ARCHIVE_LIMIT
    {
        return Err(Error::Invalid);
    }
    unhex(&b.head)?;
    unhex(&head.ciphertext_sha256)?;
    Ok(())
}
fn verify_file(path: &Path, size: u64, digest: &str, limit: u64) -> Result<()> {
    if digest_file(path, limit)? != (size, digest.into()) {
        return Err(Error::Unavailable);
    }
    Ok(())
}
fn disjoint(a: &Path, b: &Path) -> Result<()> {
    if a.starts_with(b) || b.starts_with(a) {
        return Err(Error::Invalid);
    }
    Ok(())
}
/// Explicit target setup from an independently copied, selected baseline. Empty
/// private roots only; partial initialization is never silently adopted.
pub fn init_target(
    root: &Path,
    artifacts: &Path,
    ledger: &Path,
    snapshot: &Path,
    snapshot_head: &Path,
    recipient: &str,
) -> Result<()> {
    let _: age::x25519::Recipient = recipient.parse().map_err(|_| Error::Invalid)?;
    for d in [root, artifacts, ledger] {
        private_dir(d)?;
        if fs::read_dir(d)?.next().is_some() {
            return Err(Error::Conflict);
        }
    }
    disjoint(root, artifacts)?;
    disjoint(root, ledger)?;
    disjoint(artifacts, ledger)?;
    for input in [snapshot, snapshot_head] {
        for d in [root, artifacts, ledger] {
            disjoint(d, input)?;
        }
    }
    let raw = bytes(snapshot_head, JSON_LIMIT)?;
    let head: SnapshotTrustedHead = serde_json::from_slice(&raw).map_err(|_| Error::Invalid)?;
    snapshot_binding(&head)?;
    verify_file(
        snapshot,
        head.ciphertext_size,
        &head.ciphertext_sha256,
        ARCHIVE_LIMIT,
    )?;
    let mut input = open_private(snapshot, ARCHIVE_LIMIT)?;
    let mut output = new_private(&artifacts.join("snapshot.age"))?;
    if std::io::copy(&mut input, &mut output)? != head.ciphertext_size {
        return Err(Error::Unavailable);
    }
    output.sync_all()?;
    verify_file(
        &artifacts.join("snapshot.age"),
        head.ciphertext_size,
        &head.ciphertext_sha256,
        ARCHIVE_LIMIT,
    )?;
    write_new(&artifacts.join("snapshot.json"), &raw)?;
    let config = TargetConfig {
        format: FORMAT.into(),
        version: 1,
        source: head.snapshot.source,
        recipient: recipient.into(),
        baseline: head,
        baseline_sha256: hash(&raw),
        artifacts: artifacts.into(),
        ledger: ledger.into(),
    };
    new_private(&root.join("receiver.lock"))?.sync_all()?;
    sync_dir(artifacts)?;
    sync_dir(ledger)?;
    write_new(&root.join("target.json"), &encode(&config)?)?;
    Ok(())
}
fn config(root: &Path) -> Result<TargetConfig> {
    private_dir(root)?;
    let c: TargetConfig = read_json(&root.join("target.json"))?;
    if c.format != FORMAT || c.version != 1 || c.source != c.baseline.snapshot.source {
        return Err(Error::Invalid);
    }
    snapshot_binding(&c.baseline)?;
    let _: age::x25519::Recipient = c.recipient.parse().map_err(|_| Error::Invalid)?;
    for d in [&c.artifacts, &c.ledger] {
        private_dir(d)?;
        disjoint(root, d)?;
    }
    disjoint(&c.artifacts, &c.ledger)?;
    Ok(c)
}
fn lock(root: &Path) -> Result<File> {
    let f = open_private(&root.join("receiver.lock"), 0)?;
    f.try_lock_exclusive().map_err(|_| Error::Conflict)?;
    Ok(f)
}
fn generation(n: u64) -> String {
    format!("g{n:020}")
}
fn baseline(c: &TargetConfig) -> Result<()> {
    if hash(&bytes(&c.artifacts.join("snapshot.json"), JSON_LIMIT)?) != c.baseline_sha256 {
        return Err(Error::Unavailable);
    }
    verify_file(
        &c.artifacts.join("snapshot.age"),
        c.baseline.ciphertext_size,
        &c.baseline.ciphertext_sha256,
        ARCHIVE_LIMIT,
    )
}
fn current(c: &TargetConfig) -> Result<Option<(Acceptance, String)>> {
    let mut names = vec![];
    for item in fs::read_dir(&c.ledger)? {
        let name = item?
            .file_name()
            .into_string()
            .map_err(|_| Error::Invalid)?;
        if name.starts_with(".partial-") {
            continue;
        }
        if name.len() != 26 || !name.starts_with('g') || !name.ends_with(".json") {
            return Err(Error::Unavailable);
        }
        names.push(name);
        if names.len() as u64 > GENERATIONS {
            return Err(Error::Limit);
        }
    }
    names.sort();
    let mut previous = "00".repeat(32);
    let mut last: Option<Acceptance> = None;
    for (i, name) in names.iter().enumerate() {
        let n = i as u64 + 1;
        if *name != format!("{}.json", generation(n)) {
            return Err(Error::Unavailable);
        }
        let raw = bytes(&c.ledger.join(name), JSON_LIMIT)?;
        let a: Acceptance = serde_json::from_slice(&raw).map_err(|_| Error::Invalid)?;
        if a.format != FORMAT
            || a.version != 1
            || a.generation != n
            || a.source != c.source
            || a.previous_receipt_sha256 != previous
            || a.baseline_sha256 != c.baseline_sha256
            || a.recipient != c.recipient
            || a.journal.journal.source != a.source
            || a.journal.pin()? != a.trusted_head_sha256
        {
            return Err(Error::Unavailable);
        }
        let request = request_from(&a);
        request.validate()?;
        if request.fingerprint()? != a.capture_sha256
            || a.journal.journal.sequence < c.baseline.snapshot.sequence
            || last.as_ref().is_some_and(|old| {
                a.journal.journal.sequence < old.journal.journal.sequence
                    || a.journal_size < old.journal_size
            })
        {
            return Err(Error::Unavailable);
        }
        unhex(&a.trusted_head_sha256)?;
        previous = hash(&raw);
        last = Some(a);
    }
    if let Some(a) = &last {
        verify_generation(c, a)?;
    }
    Ok(last.map(|a| (a, previous)))
}
fn request_from(a: &Acceptance) -> Request {
    Request {
        format: 1,
        recipient: a.recipient.clone(),
        baseline_sha256: a.baseline_sha256.clone(),
        binding: a.journal.journal.clone(),
        journal_size: a.journal_size,
        journal_sha256: a.journal_sha256.clone(),
    }
}
fn verify_generation(c: &TargetConfig, a: &Acceptance) -> Result<()> {
    let dir = c.artifacts.join(generation(a.generation));
    private_dir(&dir)?;
    let raw = bytes(&dir.join("head.json"), JSON_LIMIT)?;
    let head: JournalTrustedHead = serde_json::from_slice(&raw).map_err(|_| Error::Invalid)?;
    if hash(&raw) != a.trusted_head_sha256 || head != a.journal {
        return Err(Error::Unavailable);
    }
    verify_file(
        &dir.join("journal.age"),
        head.ciphertext_size,
        &head.ciphertext_sha256,
        historical::LIMIT,
    )
}
fn sync_current(c: &TargetConfig, a: &Acceptance) -> Result<()> {
    let dir = c.artifacts.join(generation(a.generation));
    for p in [
        dir.join("head.json"),
        dir.join("journal.age"),
        c.ledger.join(format!("{}.json", generation(a.generation))),
    ] {
        open_private(&p, historical::LIMIT)?.sync_all()?;
    }
    sync_dir(&dir)?;
    sync_dir(&c.artifacts)?;
    sync_dir(&c.ledger)
}
pub fn target_status(root: &Path) -> Result<TargetStatus> {
    let c = config(root)?;
    let _lock = lock(root)?;
    baseline(&c)?;
    let current = current(&c)?.map(|v| v.0);
    if let Some(a) = &current {
        sync_current(&c, a)?;
    }
    Ok(TargetStatus {
        source: c.source,
        current: current.clone(),
        snapshot: c.artifacts.join("snapshot.age"),
        snapshot_head: c.artifacts.join("snapshot.json"),
        journal: current.as_ref().map(|a| {
            c.artifacts
                .join(generation(a.generation))
                .join("journal.age")
        }),
        journal_head: current
            .as_ref()
            .map(|a| c.artifacts.join(generation(a.generation)).join("head.json")),
    })
}
fn deadline(start: Instant) -> Result<()> {
    if start.elapsed() > Duration::from_secs(60) {
        Err(Error::Unavailable)
    } else {
        Ok(())
    }
}
/// Bounded streaming verifier. Only encrypted age output touches receiver disk.
/// The exact accepted prior byte prefix is SHA256 compared, not inferred from sequence.
fn stream_journal(
    input: &mut impl Read,
    out: &mut impl Write,
    m: &Request,
    c: &TargetConfig,
    old: Option<&Acceptance>,
    start: Instant,
) -> Result<()> {
    let mut remaining = m.journal_size;
    let old_size = old.map(|a| a.journal_size).unwrap_or(0);
    let mut consumed = 0u64;
    let mut full = Sha256::new();
    let mut prefix = Sha256::new();
    let mut copy = |buf: &mut [u8]| -> Result<()> {
        deadline(start)?;
        if buf.len() as u64 > remaining {
            return Err(Error::Invalid);
        }
        input.read_exact(buf)?;
        full.update(&*buf);
        let n = old_size.saturating_sub(consumed).min(buf.len() as u64) as usize;
        prefix.update(&buf[..n]);
        out.write_all(buf)?;
        consumed += buf.len() as u64;
        remaining -= buf.len() as u64;
        Ok(())
    };
    let mut header = [0; 24];
    copy(&mut header)?;
    if &header[..8] != MAGIC || &header[8..] != c.source.as_bytes() {
        return Err(Error::Conflict);
    }
    let mut sequence = 0i64;
    let mut chain = [0; 32];
    let base = &c.baseline.snapshot;
    if base.sequence == 0 && unhex(&base.head)? != chain {
        return Err(Error::Conflict);
    }
    // The bounded metadata limits iteration without unbounded allocations.
    for _ in 0..m.binding.sequence {
        let mut len = [0; 4];
        copy(&mut len)?;
        let len = u32::from_be_bytes(len) as usize;
        if len == 0 || len > MAX_BODY + 4096 {
            return Err(Error::Limit);
        }
        let mut body = zeroize::Zeroizing::new(vec![0; len]);
        copy(&mut body)?;
        let _: Intent = serde_json::from_slice(&body).map_err(|_| Error::Invalid)?;
        let mut expected = [0; 32];
        copy(&mut expected)?;
        let mut h = Sha256::new();
        h.update(chain);
        h.update(&body);
        chain = h.finalize().into();
        if expected != chain {
            return Err(Error::Unavailable);
        }
        sequence = sequence.checked_add(1).ok_or(Error::Limit)?;
        if sequence == base.sequence && chain != unhex(&base.head)? {
            return Err(Error::Conflict);
        }
        if old.is_some_and(|a| {
            sequence == a.journal.journal.sequence
                && chain != unhex(&a.journal.journal.head).unwrap_or([0; 32])
        }) {
            return Err(Error::Conflict);
        }
    }
    if remaining != 0
        || sequence != m.binding.sequence
        || chain != unhex(&m.binding.head)?
        || format!("{:x}", full.finalize()) != m.journal_sha256
    {
        return Err(Error::Unavailable);
    }
    if let Some(a) = old
        && (consumed < old_size || format!("{:x}", prefix.finalize()) != a.journal_sha256)
    {
        return Err(Error::Conflict);
    }
    Ok(())
}
/// Authenticated SSH publication admission. No recovery identity is read here.
/// Caller must impose a process/stream deadline for a stalled Read implementation.
pub fn receive(root: &Path, mut input: impl Read) -> Result<Acceptance> {
    let start = Instant::now();
    let c = config(root)?;
    let _lock = lock(root)?;
    baseline(&c)?;
    let mut wire = [0; 8];
    input.read_exact(&mut wire)?;
    if &wire != WIRE {
        return Err(Error::Invalid);
    }
    let mut len = [0; 4];
    input.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as u64;
    if len > JSON_LIMIT {
        return Err(Error::Limit);
    }
    let mut raw = vec![0; len as usize];
    input.read_exact(&mut raw)?;
    let m: Request = serde_json::from_slice(&raw).map_err(|_| Error::Invalid)?;
    m.validate()?;
    if m.binding.source != c.source
        || m.recipient != c.recipient
        || m.baseline_sha256 != c.baseline_sha256
        || m.binding.sequence < c.baseline.snapshot.sequence
    {
        return Err(Error::Conflict);
    }
    let old = current(&c)?;
    if old.as_ref().is_some_and(|(a, _)| {
        m.binding.sequence < a.journal.journal.sequence || m.journal_size < a.journal_size
    }) {
        return Err(Error::Conflict);
    }
    let stage = Stage::new(&c.artifacts)?;
    let recipient: age::x25519::Recipient = c.recipient.parse().map_err(|_| Error::Invalid)?;
    let manifest = encode(&JournalManifest {
        journal: m.binding.clone(),
        journal_size: m.journal_size,
        journal_sha256: m.journal_sha256.clone(),
    })?;
    let archive = stage.0.join("journal.age");
    let encryptor =
        age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
            .map_err(|_| Error::Invalid)?;
    let mut writer = encryptor
        .wrap_output(new_private(&archive)?)
        .map_err(|_| Error::Unavailable)?;
    writer.write_all(historical::WIRE)?;
    writer.write_all(&(manifest.len() as u32).to_be_bytes())?;
    writer.write_all(&manifest)?;
    writer.write_all(&m.journal_size.to_be_bytes())?;
    stream_journal(
        &mut input,
        &mut writer,
        &m,
        &c,
        old.as_ref().map(|v| &v.0),
        start,
    )?;
    let mut tail = [0];
    if input.read(&mut tail)? != 0 {
        return Err(Error::Invalid);
    }
    let file = writer.finish().map_err(|_| Error::Unavailable)?;
    file.sync_all()?;
    drop(file);
    deadline(start)?;
    let fingerprint = m.fingerprint()?;
    if let Some((a, _)) = &old
        && a.capture_sha256 == fingerprint
    {
        sync_current(&c, a)?;
        return Ok(a.clone());
    }
    let n = old.as_ref().map(|v| v.0.generation + 1).unwrap_or(1);
    if n > GENERATIONS {
        return Err(Error::Limit);
    }
    let (ciphertext_size, ciphertext_sha256) = digest_file(&archive, historical::LIMIT)?;
    let head = JournalTrustedHead {
        journal: m.binding,
        ciphertext_size,
        ciphertext_sha256,
    };
    let raw = encode(&head)?;
    write_new(&stage.0.join("head.json"), &raw)?;
    sync_dir(&stage.0)?;
    let destination = c.artifacts.join(generation(n));
    if destination.symlink_metadata().is_ok() {
        private_dir(&destination)?;
        publish_directory(
            &destination,
            &c.artifacts
                .join(format!(".orphan-{}", uuid::Uuid::new_v4())),
        )?;
        sync_dir(&c.artifacts)?;
    }
    fault("before_generation")?;
    publish_directory(&stage.0, &destination)?;
    sync_dir(&c.artifacts)?;
    fault("after_generation")?;
    let a = Acceptance {
        format: FORMAT.into(),
        version: 1,
        generation: n,
        source: c.source,
        capture_sha256: fingerprint,
        previous_receipt_sha256: old.map(|v| v.1).unwrap_or_else(|| "00".repeat(32)),
        trusted_head_sha256: hash(&raw),
        journal: head,
        journal_size: m.journal_size,
        journal_sha256: m.journal_sha256,
        baseline_sha256: c.baseline_sha256.clone(),
        recipient: c.recipient.clone(),
    };
    let partial = c.ledger.join(format!(".partial-{}", uuid::Uuid::new_v4()));
    write_new(&partial, &encode(&a)?)?;
    fault("before_receipt")?;
    publish_directory(&partial, &c.ledger.join(format!("{}.json", generation(n))))?;
    sync_dir(&c.ledger)?;
    fault("after_receipt")?;
    sync_current(&c, &a)?;
    Ok(a)
}
#[cfg(test)]
thread_local! {static FAILURE:std::cell::Cell<Option<&'static str>>=const {std::cell::Cell::new(None)};}
fn fault(_point: &str) -> Result<()> {
    #[cfg(test)]
    if FAILURE.with(|v| v.get() == Some(_point)) {
        return Err(Error::Unavailable);
    }
    Ok(())
}
#[cfg(test)]
mod tests;
