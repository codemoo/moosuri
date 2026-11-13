//! Append + fsync precedes password mutation. Records contain opaque IDs and epochs only.
use crate::{Result, StoreError, files::private_file};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use uuid::Uuid;
const SIZE: usize = 104;
const MAGIC: &[u8; 8] = b"MSAUTH01";
#[cfg(test)]
thread_local! { static FAIL_OPEN_SYNC: std::cell::Cell<bool> = const { std::cell::Cell::new(false) }; }
#[derive(Clone)]
pub(crate) struct Record {
    pub seq: i64,
    pub account: String,
    pub epoch: i64,
    pub hash: [u8; 32],
}
pub(crate) struct Journal {
    file: File,
    pub records: Vec<Record>,
}
impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = private_file(path)?;
        let length = file.metadata().map_err(|_| StoreError::Unavailable)?.len();
        if length % SIZE as u64 != 0 {
            return Err(StoreError::Corrupt);
        }
        let mut records = Vec::new();
        let mut previous = [0; 32];
        // Stream records rather than allocating an attacker-selected file length.
        for index in 0..length / SIZE as u64 {
            let mut bytes = [0; SIZE];
            file.read_exact(&mut bytes)
                .map_err(|_| StoreError::Corrupt)?;
            let seq = i64::from_be_bytes(bytes[8..16].try_into().unwrap());
            let epoch = i64::from_be_bytes(bytes[32..40].try_into().unwrap());
            let hash: [u8; 32] = Sha256::digest(&bytes[..72]).into();
            if &bytes[..8] != MAGIC
                || seq <= 0
                || seq as u64 != index + 1
                || epoch <= 0
                || bytes[40..72] != previous
                || bytes[72..] != hash
            {
                return Err(StoreError::Corrupt);
            }
            let account = Uuid::from_bytes(bytes[16..32].try_into().unwrap()).to_string();
            records.push(Record {
                seq,
                account,
                epoch,
                hash,
            });
            previous = hash;
        }
        // A failed append fsync can leave complete readable bytes. Reopening is
        // authority to replay them, so validation alone cannot adopt that tail.
        #[cfg(test)]
        if FAIL_OPEN_SYNC.get() {
            return Err(StoreError::Unavailable);
        }
        file.sync_all().map_err(|_| StoreError::Unavailable)?;
        Ok(Self { file, records })
    }
    pub fn append(&mut self, account: &str, epoch: i64) -> Result<Record> {
        let mut bytes = [0; SIZE];
        let seq = i64::try_from(self.records.len()).map_err(|_| StoreError::Corrupt)? + 1;
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&seq.to_be_bytes());
        bytes[16..32].copy_from_slice(
            Uuid::parse_str(account)
                .map_err(|_| StoreError::Corrupt)?
                .as_bytes(),
        );
        bytes[32..40].copy_from_slice(&epoch.to_be_bytes());
        if let Some(last) = self.records.last() {
            bytes[40..72].copy_from_slice(&last.hash);
        }
        let hash: [u8; 32] = Sha256::digest(&bytes[..72]).into();
        bytes[72..].copy_from_slice(&hash);
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&bytes))
            .and_then(|_| self.file.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        let record = Record {
            seq,
            account: account.to_owned(),
            epoch,
            hash,
        };
        self.records.push(record.clone());
        Ok(record)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reopening_a_complete_tail_requires_successful_sync_before_adoption() {
        let dir = crate::tests::directory();
        let path = dir.path().join("auth.journal");
        let mut journal = Journal::open(&path).unwrap();
        journal.append(&Uuid::new_v4().to_string(), 1).unwrap();
        drop(journal);
        let bytes = std::fs::read(&path).unwrap();
        FAIL_OPEN_SYNC.set(true);
        let result = Journal::open(&path);
        FAIL_OPEN_SYNC.set(false);
        assert!(matches!(result, Err(StoreError::Unavailable)));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(Journal::open(&path).unwrap().records.len(), 1);
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
    }
}
