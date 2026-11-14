//! Manual independently pinned full journal checkpoints and exact-prefix forward recovery.
use super::*;
const FORMAT: &str = "moosuri-vault-journal-checkpoint";
const WIRE: &[u8; 8] = b"MVJC0001";
const RESTORE_FORMAT: &str = "moosuri-vault-historical-restore";
const LIMIT: u64 = JOURNAL_LIMIT + 1024 * 1024;
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JournalTrustedHead {
    journal: CheckpointBinding,
    ciphertext_size: u64,
    ciphertext_sha256: String,
}
impl JournalTrustedHead {
    /// Independently retain this exact receipt hash; adjacent files are not authenticity/latestness proof.
    pub fn pin(&self) -> Result<String> {
        Ok(hash(&encode(self)?))
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct JournalManifest {
    journal: CheckpointBinding,
    journal_size: u64,
    journal_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoricalProvenance {
    format: String,
    version: u32,
    snapshot: SnapshotTrustedHead,
    journal: JournalTrustedHead,
    destination: uuid::Uuid,
}
pub struct JournalInputs<'a> {
    pub checkpoint: &'a Path,
    pub trusted_head: &'a Path,
    pub trusted_head_sha256: &'a str,
    pub identity_file: &'a Path,
}
impl JournalInputs<'_> {
    pub(super) fn validate(&self, destination: &Path) -> Result<()> {
        unhex(self.trusted_head_sha256)?;
        for p in [self.checkpoint, self.trusted_head, self.identity_file] {
            if p.starts_with(destination) {
                return Err(Error::Invalid);
            }
            check_components(p)?;
        }
        Ok(())
    }
}
/// Offline manual path: the normal stopped writer lock is required, but no seal is made.
/// A missing source cannot be initialized by this exporter.
pub fn journal_checkpoint(
    root: &Path,
    recipient: &str,
    output: &Path,
    trusted_head: &Path,
) -> Result<JournalTrustedHead> {
    private_dir(root)?;
    open_private(&root.join("vault.sqlite"), DB_LIMIT)?;
    open_private(&root.join("vault.journal"), JOURNAL_LIMIT)?;
    let captured = {
        let db = Database::open(root)?;
        db.capture_snapshot(output.parent().ok_or(Error::Invalid)?)?
    };
    captured.encrypt_journal(recipient, output, trusted_head)
}
impl online::CapturedSnapshot {
    /// Usable with a live shared-mutex capture. Verification/encryption consume only the frozen copy.
    pub fn encrypt_journal(
        self,
        recipient: &str,
        output: &Path,
        trusted_head: &Path,
    ) -> Result<JournalTrustedHead> {
        let directory = self.stage.0.parent().ok_or(Error::Invalid)?;
        private_dir(directory)?;
        for p in [output, trusted_head] {
            check_components(p)?;
            if p.parent() != Some(directory) || p.symlink_metadata().is_ok() {
                return Err(Error::Conflict);
            }
        }
        if output == trusted_head {
            return Err(Error::Conflict);
        }
        let recipient: age::x25519::Recipient = recipient.parse().map_err(|_| Error::Invalid)?;
        open_private(&self.stage.0.join("vault.sqlite"), DB_LIMIT)?;
        let journal = self.stage.0.join("vault.journal");
        open_private(&journal, JOURNAL_LIMIT)?;
        let db = Database::open_recovery(&self.stage.0)?;
        if installation(&db)? != self.binding.source
            || db.sequence != self.binding.sequence
            || db.head != unhex(&self.binding.head)?
        {
            return Err(Error::Unavailable);
        }
        verify_semantics(&db, &journal, directory)?;
        drop(db);
        let mut binding = self.binding;
        binding.format = FORMAT.into();
        let (journal_size, journal_sha256) = digest_file(&journal, JOURNAL_LIMIT)?;
        let manifest = encode(&JournalManifest {
            journal: binding.clone(),
            journal_size,
            journal_sha256,
        })?;
        if manifest.len() as u64 > JSON_LIMIT {
            return Err(Error::Limit);
        }
        let partial = self.stage.0.join("journal.age");
        let encryptor =
            age::Encryptor::with_recipients(std::iter::once(&recipient as &dyn age::Recipient))
                .map_err(|_| Error::Invalid)?;
        let mut writer = encryptor
            .wrap_output(new_private(&partial)?)
            .map_err(|_| Error::Unavailable)?;
        writer.write_all(WIRE)?;
        writer.write_all(&(manifest.len() as u32).to_be_bytes())?;
        writer.write_all(&manifest)?;
        writer.write_all(&journal_size.to_be_bytes())?;
        if std::io::copy(&mut open_private(&journal, JOURNAL_LIMIT)?, &mut writer)? != journal_size
        {
            return Err(Error::Unavailable);
        }
        let file = writer.finish().map_err(|_| Error::Unavailable)?;
        file.sync_all()?;
        drop(file);
        let (ciphertext_size, ciphertext_sha256) = digest_file(&partial, LIMIT)?;
        let head = JournalTrustedHead {
            journal: binding,
            ciphertext_size,
            ciphertext_sha256,
        };
        fs::hard_link(&partial, output)?;
        fs::remove_file(&partial)?;
        sync_dir(directory)?;
        write_new(trusted_head, &encode(&head)?)?;
        Ok(head)
    }
}
pub fn restore_historical(
    destination: &Path,
    snapshot: &Path,
    snapshot_head: &Path,
    snapshot_identity: &Path,
    journal: &JournalInputs<'_>,
) -> Result<()> {
    restore_inner(
        destination,
        snapshot,
        snapshot_head,
        snapshot_identity,
        SNAPSHOT_FORMAT,
        SNAPSHOT_WIRE,
        Some(journal),
    )
}
fn decoded(
    inputs: &JournalInputs<'_>,
    stage: &Stage,
) -> Result<(JournalTrustedHead, JournalManifest, PathBuf)> {
    let mut raw = vec![];
    open_private(inputs.trusted_head, JSON_LIMIT)?.read_to_end(&mut raw)?;
    if hash(&raw) != inputs.trusted_head_sha256.to_ascii_lowercase() {
        return Err(Error::Unauthorized);
    }
    let head: JournalTrustedHead = serde_json::from_slice(&raw).map_err(|_| Error::Invalid)?;
    if head.journal.format != FORMAT
        || head.journal.version != 1
        || head.journal.source.is_nil()
        || head.journal.checkpoint.is_nil()
        || head.journal.sequence < 0
        || unhex(&head.journal.head).is_err()
    {
        return Err(Error::Invalid);
    }
    let mut input = open_private(inputs.checkpoint, LIMIT)?;
    if digest_input(&mut input, LIMIT)? != (head.ciphertext_size, head.ciphertext_sha256.clone()) {
        return Err(Error::Unavailable);
    }
    let key = identity(inputs.identity_file)?;
    let decryptor = age::Decryptor::new(input).map_err(|_| Error::Unavailable)?;
    let mut reader = decryptor
        .decrypt(std::iter::once(&key as &dyn age::Identity))
        .map_err(|_| Error::Unauthorized)?;
    let mut wire = [0; 8];
    reader.read_exact(&mut wire)?;
    if &wire != WIRE {
        return Err(Error::Invalid);
    }
    let mut len = [0; 4];
    reader.read_exact(&mut len)?;
    let len = u32::from_be_bytes(len) as u64;
    if len > JSON_LIMIT {
        return Err(Error::Limit);
    }
    let mut bytes = vec![0; len as usize];
    reader.read_exact(&mut bytes)?;
    let manifest: JournalManifest = serde_json::from_slice(&bytes).map_err(|_| Error::Invalid)?;
    if manifest.journal != head.journal {
        return Err(Error::Unavailable);
    }
    let path = stage.0.join("forward.journal");
    extract(
        &mut reader,
        &path,
        manifest.journal_size,
        &manifest.journal_sha256,
        JOURNAL_LIMIT,
    )?;
    let mut tail = [0; 1];
    if reader.read(&mut tail)? != 0 {
        return Err(Error::Invalid);
    }
    Ok((head, manifest, path))
}
pub(super) fn forward(
    db: &mut Database,
    snapshot_journal: &Path,
    parent: &Path,
    snapshot: &TrustedHead,
    inputs: &JournalInputs<'_>,
) -> Result<JournalTrustedHead> {
    let stage = Stage::new(parent)?;
    let (head, manifest, path) = decoded(inputs, &stage)?;
    if head.journal.source != snapshot.seal.source || head.journal.sequence < snapshot.seal.sequence
    {
        return Err(Error::Conflict);
    }
    let (prefix_size, prefix_digest) = digest_file(snapshot_journal, JOURNAL_LIMIT)?;
    if manifest.journal_size < prefix_size {
        return Err(Error::Conflict);
    }
    let mut prefix = open_private(&path, JOURNAL_LIMIT)?.take(prefix_size);
    let mut h = Sha256::new();
    let mut consumed = 0u64;
    let mut buf = [0; 65536];
    loop {
        let n = prefix.read(&mut buf)?;
        if n == 0 {
            break;
        }
        consumed += n as u64;
        h.update(&buf[..n]);
    }
    if consumed != prefix_size || format!("{:x}", h.finalize()) != prefix_digest {
        return Err(Error::Conflict);
    }
    let mut f = open_private(&path, JOURNAL_LIMIT)?;
    let mut header = [0; 24];
    f.read_exact(&mut header)?;
    if &header[..8] != MAGIC || &header[8..] != snapshot.seal.source.as_bytes() {
        return Err(Error::Conflict);
    }
    let mut sequence = 0i64;
    let mut chain = [0; 32];
    if snapshot.seal.sequence == 0 && (prefix_size != 24 || unhex(&snapshot.seal.head)? != chain) {
        return Err(Error::Conflict);
    }
    loop {
        let mut len = [0; 4];
        let n = f.read(&mut len)?;
        if n == 0 {
            break;
        }
        if n != 4 {
            return Err(Error::Unavailable);
        }
        let len = u32::from_be_bytes(len) as usize;
        if len > MAX_BODY + 4096 {
            return Err(Error::Limit);
        }
        let mut body = vec![0; len];
        f.read_exact(&mut body)?;
        let mut expected = [0; 32];
        f.read_exact(&mut expected)?;
        let mut hash = Sha256::new();
        hash.update(chain);
        hash.update(&body);
        chain = hash.finalize().into();
        if chain != expected {
            return Err(Error::Unavailable);
        }
        sequence = sequence.checked_add(1).ok_or(Error::Limit)?;
        let intent: Intent = serde_json::from_slice(&body).map_err(|_| Error::Invalid)?;
        if sequence == snapshot.seal.sequence
            && (f.stream_position()? != prefix_size || chain != unhex(&snapshot.seal.head)?)
        {
            return Err(Error::Conflict);
        }
        if sequence > snapshot.seal.sequence {
            db.replay_forward(intent, sequence, chain)?;
        }
    }
    if sequence != head.journal.sequence
        || chain != unhex(&head.journal.head)?
        || db.sequence != sequence
        || db.head != chain
    {
        return Err(Error::Unavailable);
    }
    if digest_file(snapshot_journal, JOURNAL_LIMIT)?
        != (manifest.journal_size, manifest.journal_sha256)
    {
        return Err(Error::Unavailable);
    }
    verify_semantics(db, snapshot_journal, parent)?;
    Ok(head)
}
pub(super) fn provenance(
    snapshot: &TrustedHead,
    journal: JournalTrustedHead,
    destination: uuid::Uuid,
) -> Result<Vec<u8>> {
    encode(&HistoricalProvenance {
        format: RESTORE_FORMAT.into(),
        version: 1,
        snapshot: snapshot.clone().into(),
        journal,
        destination,
    })
}
pub(super) fn existing_destination(
    destination: &Path,
    snapshot: &TrustedHead,
    journal: &JournalTrustedHead,
) -> Result<()> {
    let p: HistoricalProvenance = read_json(&destination.join("vault.restored"))?;
    if p.format != RESTORE_FORMAT
        || p.version != 1
        || p.snapshot != SnapshotTrustedHead::from(snapshot.clone())
        || &p.journal != journal
        || p.destination.is_nil()
    {
        return Err(Error::Conflict);
    }
    let db = Database::open(destination)?;
    if installation(&db)? != p.destination {
        return Err(Error::Unavailable);
    }
    Ok(())
}

#[cfg(test)]
mod tests;
