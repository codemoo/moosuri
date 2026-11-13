//! Bounded local checksummed journal; not sender authentication or an off-host log.
use super::*;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
const MAGIC: &[u8; 8] = b"MSDSTR01";
const HEADER_SIZE: usize = 96;
const MAX_PAYLOAD: usize = 8192;
const MAX_FILE: u64 = 64 * 1024 * 1024;
#[derive(Clone)]
pub(crate) struct Binding {
    pub installation: String,
    pub legacy_sequence: i64,
    pub legacy_head: [u8; 32],
    pub header_head: [u8; 32],
}
#[derive(Clone)]
pub(crate) struct Record {
    pub seq: i64,
    pub tenant: String,
    pub effect: Effect,
    pub previous: [u8; 32],
    pub hash: [u8; 32],
}
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    tenant: String,
    effect: Effect,
}
pub(crate) struct Prepared {
    record: Record,
    bytes: Vec<u8>,
}
pub(crate) struct Journal {
    file: File,
    path: PathBuf,
    pub binding: Binding,
    pub records: Vec<Record>,
}
// Keep the test fault scoped to one synthetic journal path across actor threads.
// Readable bytes are not durable authority until this sync succeeds.
fn sync_file(file: &File, _path: &Path) -> std::io::Result<()> {
    #[cfg(test)]
    if SYNC_FAILURES
        .lock()
        .unwrap()
        .iter()
        .any(|path| path == _path)
    {
        return Err(std::io::Error::other("synthetic successor sync failure"));
    }
    file.sync_all()
}
#[cfg(test)]
static SYNC_FAILURES: std::sync::Mutex<Vec<PathBuf>> = std::sync::Mutex::new(Vec::new());
#[cfg(test)]
pub(super) struct SyncFailure(PathBuf);
#[cfg(test)]
pub(super) fn fail_sync_for_test(path: &Path) -> SyncFailure {
    SYNC_FAILURES.lock().unwrap().push(path.to_path_buf());
    SyncFailure(path.to_path_buf())
}
#[cfg(test)]
impl Drop for SyncFailure {
    fn drop(&mut self) {
        SYNC_FAILURES.lock().unwrap().retain(|path| path != &self.0);
    }
}
fn header(
    installation: &str,
    legacy_sequence: i64,
    legacy_head: [u8; 32],
) -> Result<[u8; HEADER_SIZE]> {
    if legacy_sequence < 0 {
        return Err(StoreError::Corrupt);
    }
    let mut bytes = [0; HEADER_SIZE];
    bytes[..8].copy_from_slice(MAGIC);
    bytes[8..24].copy_from_slice(
        Uuid::parse_str(installation)
            .map_err(|_| StoreError::Corrupt)?
            .as_bytes(),
    );
    bytes[24..32].copy_from_slice(&legacy_sequence.to_be_bytes());
    bytes[32..64].copy_from_slice(&legacy_head);
    let hash: [u8; 32] = Sha256::digest(&bytes[..64]).into();
    bytes[64..].copy_from_slice(&hash);
    Ok(bytes)
}
impl Journal {
    /// Only migration may create a new header; an existing interrupted file is
    /// validated verbatim, never truncated/reinitialized because migration failed.
    pub fn initialize(path: &Path, installation: &str, seq: i64, head: [u8; 32]) -> Result<Self> {
        match std::fs::symlink_metadata(path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                use std::os::unix::fs::OpenOptionsExt;
                let mut f = std::fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(0o600)
                    .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC)
                    .open(path)
                    .map_err(|_| StoreError::Unavailable)?;
                f.write_all(&header(installation, seq, head)?)
                    .and_then(|_| sync_file(&f, path))
                    .map_err(|_| StoreError::Unavailable)?;
            }
            Err(_) => return Err(StoreError::Unavailable),
        }
        let journal = Self::open(path)?;
        journal.check_binding(installation, seq, &head)?;
        if !journal.records.is_empty() {
            return Err(StoreError::Corrupt);
        }
        // Also publish a reused header's directory entry durably: an earlier
        // create/write/sync attempt may have failed before directory sync.
        File::open(path.parent().ok_or(StoreError::InvalidInput)?)
            .and_then(|f| f.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        Ok(journal)
    }
    pub fn open(path: &Path) -> Result<Self> {
        if !path.try_exists().map_err(|_| StoreError::Unavailable)? {
            return Err(StoreError::Corrupt);
        }
        let mut file = crate::files::private_file(path)?;
        let length = file.metadata().map_err(|_| StoreError::Unavailable)?.len();
        if length < HEADER_SIZE as u64 || length > MAX_FILE {
            return Err(StoreError::Corrupt);
        }
        let mut bytes = [0; HEADER_SIZE];
        file.read_exact(&mut bytes)
            .map_err(|_| StoreError::Corrupt)?;
        let legacy_sequence = i64::from_be_bytes(bytes[24..32].try_into().unwrap());
        let header_head: [u8; 32] = Sha256::digest(&bytes[..64]).into();
        if &bytes[..8] != MAGIC || legacy_sequence < 0 || bytes[64..] != header_head {
            return Err(StoreError::Corrupt);
        }
        let binding = Binding {
            installation: Uuid::from_bytes(bytes[8..24].try_into().unwrap()).to_string(),
            legacy_sequence,
            legacy_head: bytes[32..64].try_into().unwrap(),
            header_head,
        };
        let mut records = Vec::new();
        let mut previous = header_head;
        let mut position = HEADER_SIZE as u64;
        while position < length {
            let mut prefix = [0; 44];
            file.read_exact(&mut prefix)
                .map_err(|_| StoreError::Corrupt)?;
            let size = u32::from_be_bytes(prefix[..4].try_into().unwrap()) as usize;
            let seq = i64::from_be_bytes(prefix[4..12].try_into().unwrap());
            if size == 0
                || size > MAX_PAYLOAD
                || seq <= 0
                || seq as usize != records.len() + 1
                || prefix[12..] != previous
            {
                return Err(StoreError::Corrupt);
            }
            let mut payload = vec![0; size];
            let mut hash = [0; 32];
            file.read_exact(&mut payload)
                .and_then(|_| file.read_exact(&mut hash))
                .map_err(|_| StoreError::Corrupt)?;
            let mut expected = Sha256::new();
            expected.update(prefix);
            expected.update(&payload);
            if hash != <[u8; 32]>::from(expected.finalize()) {
                return Err(StoreError::Corrupt);
            }
            let value: Payload =
                serde_json::from_slice(&payload).map_err(|_| StoreError::Corrupt)?;
            validate(&value.tenant, &value.effect).map_err(|_| StoreError::Corrupt)?;
            records.push(Record {
                seq,
                tenant: value.tenant,
                effect: value.effect,
                previous,
                hash,
            });
            previous = hash;
            position += 44 + size as u64 + 32;
        }
        if position != length {
            return Err(StoreError::Corrupt);
        }
        // A prior failed sync can leave a complete, checksum-valid suffix in the
        // page cache. Never promote it to schema/replay authority until synced.
        sync_file(&file, path).map_err(|_| StoreError::Unavailable)?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            binding,
            records,
        })
    }
    pub fn check_binding(&self, id: &str, seq: i64, head: &[u8]) -> Result<()> {
        if self.binding.installation != id
            || self.binding.legacy_sequence != seq
            || self.binding.legacy_head != head
        {
            return Err(StoreError::Corrupt);
        }
        Ok(())
    }
    pub fn prepare(&self, tenant: &str, effect: Effect) -> Result<Prepared> {
        validate(tenant, &effect)?;
        let payload = serde_json::to_vec(&Payload {
            tenant: tenant.into(),
            effect: effect.clone(),
        })
        .map_err(|_| StoreError::Corrupt)?;
        if payload.is_empty() || payload.len() > MAX_PAYLOAD {
            return Err(StoreError::InvalidInput);
        }
        let seq = i64::try_from(self.records.len())
            .map_err(|_| StoreError::Corrupt)?
            .checked_add(1)
            .ok_or(StoreError::Corrupt)?;
        let previous = self
            .records
            .last()
            .map(|r| r.hash)
            .unwrap_or(self.binding.header_head);
        let mut bytes = Vec::with_capacity(76 + payload.len());
        bytes.extend((payload.len() as u32).to_be_bytes());
        bytes.extend(seq.to_be_bytes());
        bytes.extend(previous);
        bytes.extend(payload);
        let hash: [u8; 32] = Sha256::digest(&bytes).into();
        bytes.extend(hash);
        if self
            .file
            .metadata()
            .map_err(|_| StoreError::Unavailable)?
            .len()
            .checked_add(bytes.len() as u64)
            .is_none_or(|size| size > MAX_FILE)
        {
            return Err(StoreError::Busy);
        }
        Ok(Prepared {
            record: Record {
                seq,
                tenant: tenant.into(),
                effect,
                previous,
                hash,
            },
            bytes,
        })
    }
    pub fn append(&mut self, prepared: Prepared) -> Result<Record> {
        if prepared.record.seq as usize != self.records.len() + 1 {
            return Err(StoreError::Corrupt);
        }
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&prepared.bytes))
            .and_then(|_| sync_file(&self.file, &self.path))
            .map_err(|_| StoreError::Unavailable)?;
        self.records.push(prepared.record.clone());
        Ok(prepared.record)
    }
}
