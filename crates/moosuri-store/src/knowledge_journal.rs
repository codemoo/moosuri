//! Opaque destructive knowledge intents. No text, external identifiers or credentials.
use crate::{Result, StoreError, files::private_file};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::Path,
};
use uuid::Uuid;
const SIZE: usize = 120;
const MAGIC: &[u8; 8] = b"MSKNOW01";
#[derive(Clone)]
pub(super) struct Record {
    pub seq: i64,
    pub tenant: String,
    pub target: String,
    pub action: u8,
    pub hash: [u8; 32],
}
pub(super) struct Journal {
    file: File,
    pub records: Vec<Record>,
    frozen: bool,
}
impl Journal {
    pub fn open(path: &Path) -> Result<Self> {
        let mut file = private_file(path)?;
        let length = file.metadata().map_err(|_| StoreError::Unavailable)?.len();
        if length % SIZE as u64 != 0 {
            return Err(StoreError::Corrupt);
        }
        let mut previous = [0; 32];
        let mut records = Vec::new();
        for i in 0..length / SIZE as u64 {
            let mut bytes = [0; SIZE];
            file.read_exact(&mut bytes)
                .map_err(|_| StoreError::Corrupt)?;
            let seq = i64::from_be_bytes(bytes[8..16].try_into().unwrap());
            let hash: [u8; 32] = Sha256::digest(&bytes[..88]).into();
            if &bytes[..8] != MAGIC
                || seq <= 0
                || seq as u64 != i + 1
                || !(1..=6).contains(&bytes[48])
                || bytes[49..56] != [0; 7]
                || bytes[56..88] != previous
                || bytes[88..] != hash
            {
                return Err(StoreError::Corrupt);
            }
            records.push(Record {
                seq,
                tenant: Uuid::from_bytes(bytes[16..32].try_into().unwrap()).to_string(),
                target: Uuid::from_bytes(bytes[32..48].try_into().unwrap()).to_string(),
                action: bytes[48],
                hash,
            });
            previous = hash;
        }
        // Also persist initial creation before the caller commits its migration.
        file.sync_all().map_err(|_| StoreError::Unavailable)?;
        Ok(Self {
            file,
            records,
            frozen: false,
        })
    }
    pub fn freeze(&mut self) {
        self.frozen = true;
    }
    #[allow(dead_code)] // retained legacy-format fixture writer; live journals are frozen
    pub fn append(&mut self, tenant: &str, target: &str, action: u8) -> Result<Record> {
        if self.frozen {
            return Err(StoreError::Corrupt);
        }
        let seq = i64::try_from(self.records.len())
            .map_err(|_| StoreError::Corrupt)?
            .checked_add(1)
            .ok_or(StoreError::Corrupt)?;
        let mut bytes = [0; SIZE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8..16].copy_from_slice(&seq.to_be_bytes());
        bytes[16..32].copy_from_slice(
            Uuid::parse_str(tenant)
                .map_err(|_| StoreError::Corrupt)?
                .as_bytes(),
        );
        bytes[32..48].copy_from_slice(
            Uuid::parse_str(target)
                .map_err(|_| StoreError::Corrupt)?
                .as_bytes(),
        );
        if !(1..=6).contains(&action) {
            return Err(StoreError::Corrupt);
        }
        bytes[48] = action;
        if let Some(last) = self.records.last() {
            bytes[56..88].copy_from_slice(&last.hash);
        }
        let hash: [u8; 32] = Sha256::digest(&bytes[..88]).into();
        bytes[88..].copy_from_slice(&hash);
        self.file
            .seek(SeekFrom::End(0))
            .and_then(|_| self.file.write_all(&bytes))
            .and_then(|_| self.file.sync_all())
            .map_err(|_| StoreError::Unavailable)?;
        let record = Record {
            seq,
            tenant: tenant.into(),
            target: target.into(),
            action,
            hash,
        };
        self.records.push(record.clone());
        Ok(record)
    }
}
