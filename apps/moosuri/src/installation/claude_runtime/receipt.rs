//! Root-only receipts, kept in the private control directory (never in a
//! worker-writable home and never in the strict account registry). A receipt
//! is written BEFORE `useradd`, so a crash afterwards can only adopt the exact
//! account carrying this request's random GECOS marker.
use super::request::Request;
use crate::engine_login::Owner;
use crate::installation::kiri::host::{self, Layout, Result};
use serde::{Deserialize, Serialize};
use std::{fs, io::Read, os::unix::fs::MetadataExt, path::PathBuf};
use uuid::Uuid;

const CAP: u64 = 16 * 1024;

/// `msc-` + 27 hex characters of the random request id (31 characters total).
pub(super) fn username(id: &Uuid) -> String {
    format!("msc-{}", &id.simple().to_string()[..27])
}
pub(super) fn marker(id: &Uuid) -> String {
    // No ':' or ',' (invalid in a passwd GECOS field).
    format!("moosuri-claude-{}", id.hyphenated())
}
pub(super) fn home(username: &str) -> String {
    format!("{}/{}", super::HOMES, username)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IdRange {
    pub start: u64,
    pub count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Ident {
    pub uid: u32,
    pub gid: u32,
    pub subuid: IdRange,
    pub subgid: IdRange,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Phase {
    /// Written before `useradd`; no identity has been observed yet.
    Reserved,
    /// Exact uid/gid/subordinate ranges were observed and recorded.
    Identified,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Receipt {
    pub version: u32,
    pub request_id: Uuid,
    pub owner: Owner,
    pub username: String,
    pub marker: String,
    pub home: String,
    pub phase: Phase,
    pub identity: Option<Ident>,
}

impl Receipt {
    pub(super) fn reserved(request: &Request) -> Self {
        let username = username(&request.request_id);
        Self {
            version: 1,
            request_id: request.request_id,
            owner: request.owner,
            marker: marker(&request.request_id),
            home: home(&username),
            username,
            phase: Phase::Reserved,
            identity: None,
        }
    }
    /// Every derived field must still be exactly what this request derives.
    fn bound_to(&self, request: &Request) -> Result<()> {
        let expected = Self::reserved(request);
        if self.version != 1
            || self.request_id != expected.request_id
            || self.username != expected.username
            || self.marker != expected.marker
            || self.home != expected.home
            || (self.phase == Phase::Identified) != self.identity.is_some()
        {
            return Err("claude_runtime_receipt_conflict".into());
        }
        if self.owner != request.owner {
            return Err("claude_runtime_request_owner_mismatch".into());
        }
        Ok(())
    }
}

pub(super) struct Store {
    layout: Layout,
    dir: PathBuf,
}

impl Store {
    pub(super) fn new(layout: &Layout) -> Self {
        Self {
            layout: layout.clone(),
            dir: layout.at(&format!("{}/receipts", super::CONTROL)),
        }
    }
    fn path(&self, id: &Uuid) -> PathBuf {
        self.dir.join(format!("{}.json", id.hyphenated()))
    }
    fn read(path: &std::path::Path) -> Result<(Receipt, Vec<u8>)> {
        let (mut file, _) = host::open_regular(path, CAP)?;
        let metadata = file
            .metadata()
            .map_err(|_| "claude_runtime_receipt_unavailable")?;
        if metadata.mode() & 0o077 != 0 {
            return Err("claude_runtime_receipt_conflict".into());
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(CAP + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "claude_runtime_receipt_unavailable")?;
        let receipt =
            serde_json::from_slice(&bytes).map_err(|_| "claude_runtime_receipt_conflict")?;
        Ok((receipt, bytes))
    }

    /// `None` when this request was never started. A present receipt must match
    /// the exact request and owner or the call fails closed.
    pub(super) fn load(&self, request: &Request) -> Result<Option<(Receipt, Vec<u8>)>> {
        let path = self.path(&request.request_id);
        match fs::symlink_metadata(&path) {
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err("claude_runtime_receipt_unavailable".into()),
        }
        let (receipt, bytes) = Self::read(&path)?;
        receipt.bound_to(request)?;
        Ok(Some((receipt, bytes)))
    }

    /// `previous` is the exact byte string this process last read or wrote;
    /// the shared helper refuses to overwrite anything else.
    pub(super) fn save(&self, receipt: &Receipt, previous: Option<&[u8]>) -> Result<Vec<u8>> {
        let bytes = serde_json::to_vec_pretty(receipt)
            .map_err(|_| "claude_runtime_receipt_unserializable")?;
        let history: Vec<String> = previous
            .map(|p| vec![host::sha256_bytes(p)])
            .unwrap_or_default();
        self.layout
            .write_file(&self.path(&receipt.request_id), &bytes, 0o600, &history)?;
        Ok(bytes)
    }

    /// One exact owner tuple may not be prepared by two different requests.
    pub(super) fn owner_in_use(&self, request: &Request) -> Result<()> {
        let entries = fs::read_dir(&self.dir).map_err(|_| "claude_runtime_receipt_unavailable")?;
        for entry in entries {
            let entry = entry.map_err(|_| "claude_runtime_receipt_unavailable")?;
            let name = entry.file_name();
            let name = name
                .to_str()
                .ok_or("claude_runtime_receipt_directory_foreign")?;
            // Interrupted atomic-write temporaries made by the shared helper.
            if name.starts_with(".moosuri-kiri-") {
                continue;
            }
            let id = name
                .strip_suffix(".json")
                .and_then(|s| Uuid::parse_str(s).ok())
                .ok_or("claude_runtime_receipt_directory_foreign")?;
            if id == request.request_id {
                continue;
            }
            let (other, _) = Self::read(&self.path(&id))?;
            if other.request_id != id || other.owner == request.owner {
                return Err(if other.request_id != id {
                    "claude_runtime_receipt_conflict".into()
                } else {
                    "claude_runtime_owner_already_prepared".to_owned()
                });
            }
        }
        Ok(())
    }
}
