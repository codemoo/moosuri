//! The only caller-supplied input: a private root-owned JSON request that names
//! the owner and a random request id. It carries no path, endpoint, image,
//! digest, uid, service name or command; every destination derives from it.
use crate::engine_login::Owner;
use crate::installation::kiri::host::{self, Layout, Result};
use serde::{Deserialize, Serialize};
use std::{
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};
use uuid::Uuid;

const REQUEST_CAP: u64 = 16 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Request {
    pub version: u32,
    pub request_id: Uuid,
    pub owner: Owner,
}

impl Request {
    pub(crate) fn new(request_id: Uuid, owner: Owner) -> Self {
        Self {
            version: 1,
            request_id,
            owner,
        }
    }

    pub(crate) fn validate(&self) -> Result<()> {
        let o = &self.owner;
        if self.version != 1
            || self.request_id.is_nil()
            || o.account_id.is_nil()
            || o.tenant_id.is_nil()
            || o.volume_id.is_nil()
            || o.generation <= 0
        {
            return Err("claude_runtime_request_invalid".into());
        }
        Ok(())
    }

    pub(crate) fn parse(bytes: &[u8]) -> Result<Self> {
        let request: Self =
            serde_json::from_slice(bytes).map_err(|_| "claude_runtime_request_invalid")?;
        request.validate()?;
        Ok(request)
    }

    /// Read a private (0600-class), root-owned, single-link, <=16 KiB request.
    pub(crate) fn load(layout: &Layout, path: &Path) -> Result<Self> {
        if !path.is_absolute()
            || path
                .components()
                .any(|c| matches!(c, Component::ParentDir | Component::CurDir))
        {
            return Err("claude_runtime_request_path_invalid".into());
        }
        let text = path.to_str().ok_or("claude_runtime_request_path_invalid")?;
        let actual = layout.at(text);
        layout.safe_ancestors(&actual)?;
        let (mut file, _) = host::open_regular(&actual, REQUEST_CAP)?;
        let metadata = file
            .metadata()
            .map_err(|_| "claude_runtime_request_unreadable")?;
        if metadata.mode() & 0o077 != 0 {
            return Err("claude_runtime_request_not_private".into());
        }
        let mut bytes = Vec::new();
        (&mut file)
            .take(REQUEST_CAP + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| "claude_runtime_request_unreadable")?;
        if bytes.len() as u64 > REQUEST_CAP {
            return Err("claude_runtime_request_invalid".into());
        }
        Self::parse(&bytes)
    }
}
